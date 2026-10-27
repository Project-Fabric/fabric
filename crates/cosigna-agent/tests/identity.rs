//! Synthetic identities only. Private signing keys are generated in memory, never persisted.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cosigna_agent::{
    Actor, Error, Role, Service,
    api::Credentials,
    gateway,
    identity::{
        ApproverBindingInput, Enrollment, GatewayBindingInput, IdentityError as E, MembershipInput,
        OrganizationInput, Subject, Verifier,
    },
    now,
    planner::FixturePlanner,
};

fn approval_subject(sub: &str) -> Subject {
    let mut c = claims();
    c["sub"] = json!(sub);
    c["scope"] = json!("cosigna:read cosigna:approve");
    verifier(&keys())
        .unwrap()
        .verify(&signed(&c, &header(), key()), now())
        .unwrap()
}

struct ApprovalLab {
    dir: tempfile::TempDir,
    service: Service,
    user: Value,
    org: Value,
    membership: Value,
    gateway_binding: Value,
    approver_binding: Value,
    approver: cosigna_agent::approvers::IssuedApprover,
    budget: String,
}

impl ApprovalLab {
    fn new(max: i64, two: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::open(dir.path()).unwrap();
        let user = enroll(&s, "subject-a");
        let o = org(&s, "Company A");
        let membership = member(&s, &user, &o);
        let gateway_binding = s
            .bind_identity_gateway(
                &owner(),
                GatewayBindingInput {
                    organization_id: o["id"].as_str().unwrap().into(),
                },
                now(),
            )
            .unwrap();
        let approver = s
            .register_approver(
                &owner(),
                cosigna_agent::approvers::ApproverInput {
                    name: "Priya".into(),
                    currency: "USD".into(),
                    max_approval_minor: max,
                },
                now(),
            )
            .unwrap();
        let approver_binding = s
            .bind_identity_approver(
                &owner(),
                ApproverBindingInput {
                    membership_id: membership["id"].as_str().unwrap().into(),
                    approver_id: approver.approver.id.clone(),
                },
                now(),
            )
            .unwrap();
        let budget = s
            .create_budget(
                &owner(),
                cosigna_agent::BudgetInput {
                    currency: "USD".into(),
                    limit_minor: 10_000,
                    per_purchase_minor: 10_000,
                    merchants: vec!["fixture-cafe".into()],
                    expires_at: now() + 3600,
                    second_approval_above_minor: two.then_some(500),
                },
                now(),
            )
            .unwrap()
            .id;
        Self {
            dir,
            service: s,
            user,
            org: o,
            membership,
            gateway_binding,
            approver_binding,
            approver,
            budget,
        }
    }
    fn refund(&self, amount: i64) -> cosigna_agent::operations::Operation {
        self.service.request_operation(&owner(),serde_json::from_value(json!({"budget_id":self.budget,"kind":"refund","rail":"fixture","source":"pay_fixture_2041","amount_minor":amount,"currency":"USD","reason":"Synthetic refund","idempotency_key":"identity-refund"})).unwrap(),now()).unwrap()
    }
    fn call(
        &self,
        subject: &Subject,
        method: &str,
        suffix: &str,
        body: Value,
    ) -> Result<Value, Error> {
        self.service.identity_request(
            subject,
            method,
            &format!(
                "/identity/v1/organizations/{}{}",
                self.org["id"].as_str().unwrap(),
                suffix
            ),
            &serde_json::to_vec(&body).unwrap(),
            now(),
        )
    }
    fn approve(
        &self,
        subject: &Subject,
        op: &cosigna_agent::operations::Operation,
    ) -> Result<Value, Error> {
        self.call(
            subject,
            "POST",
            &format!("/operations/{}/approve", op.id),
            json!({"proposal_digest":op.proposal.as_ref().unwrap().digest().unwrap()}),
        )
    }
    fn refunded(&self) -> i64 {
        self.service
            .rails
            .fixture_rail()
            .unwrap()
            .refunded_minor("pay_fixture_2041")
            .unwrap()
    }
}

#[test]
fn verified_identity_approves_one_exact_refund_and_evidence_names_provider_and_subject() {
    let lab = ApprovalLab::new(2000, false);
    let op = lab.refund(1000);
    let subject = approval_subject("subject-a");
    assert_eq!(
        lab.call(&subject, "GET", "/operations?stage=held", json!({}))
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let result = lab.approve(&subject, &op).unwrap();
    assert_eq!(result["data"]["stage"], "succeeded");
    assert_eq!(result["data"]["approver"], lab.approver.approver.id);
    assert_eq!(lab.refunded(), 1000);
    assert_eq!(
        lab.approve(&subject, &op).unwrap()["data"]["attempt_key"],
        result["data"]["attempt_key"]
    );
    assert_eq!(lab.refunded(), 1000);
    let db = rusqlite::Connection::open(lab.dir.path().join("transactions.sqlite")).unwrap();
    let body: String=db.query_row("SELECT body FROM evidence WHERE json_extract(body,'$.event.kind')='gateway.operation.approver_approved'",[],|r|r.get(0)).unwrap();
    let event: Value = serde_json::from_str(&body).unwrap();
    let identity = &event["event"]["by"]["identity"];
    assert_eq!(identity["issuer"], ISSUER);
    assert_eq!(identity["subject"], "subject-a");
    assert_eq!(identity["user_id"], lab.user["user"]["id"]);
    assert_eq!(identity["membership_id"], lab.membership["id"]);
    assert_eq!(identity["approver_binding_id"], lab.approver_binding["id"]);
    assert!(!body.contains("Bearer"));
    assert!(!body.contains(&lab.approver.token));
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn identity_approval_scope_digest_and_per_person_limit_are_independent_checks() {
    let lab = ApprovalLab::new(500, false);
    let op = lab.refund(1000);
    assert!(matches!(
        lab.approve(&subject(), &op),
        Err(Error::Identity(E::InsufficientApprovalScope))
    ));
    let subject = approval_subject("subject-a");
    assert!(matches!(
        lab.call(
            &subject,
            "POST",
            &format!("/operations/{}/approve", op.id),
            json!({"proposal_digest":"tampered"})
        ),
        Err(Error::Conflict(_))
    ));
    assert!(
        matches!(lab.approve(&subject,&op),Err(Error::Conflict(s)) if s.starts_with("above_approver_limit"))
    );
    assert_eq!(lab.refunded(), 0);
    assert_eq!(
        lab.service.operation(&owner(), &op.id).unwrap().stage,
        cosigna_agent::operations::OperationStage::Held
    );
    assert_eq!(
        lab.call(
            &subject,
            "POST",
            &format!("/operations/{}/reject", op.id),
            json!({})
        )
        .unwrap()["data"]["stage"],
        "rejected"
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn membership_roles_do_not_replace_explicit_approver_binding_or_owner_mapping() {
    let lab = ApprovalLab::new(2000, false);
    let op = lab.refund(1000);
    let user = enroll(&lab.service, "subject-b");
    lab.service
        .create_identity_membership(
            &owner(),
            MembershipInput {
                user_id: user["user"]["id"].as_str().unwrap().into(),
                organization_id: lab.org["id"].as_str().unwrap().into(),
                role: "owner".into(),
            },
            now(),
        )
        .unwrap();
    assert!(matches!(
        lab.approve(&approval_subject("subject-b"), &op),
        Err(Error::Identity(E::NotFound))
    ));
    let other = org(&lab.service, "Other company");
    assert!(matches!(
        lab.service.bind_identity_gateway(
            &owner(),
            GatewayBindingInput {
                organization_id: other["id"].as_str().unwrap().into()
            },
            now()
        ),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        lab.service.identity_request(
            &approval_subject("subject-a"),
            "GET",
            &format!(
                "/identity/v1/organizations/{}/operations",
                other["id"].as_str().unwrap()
            ),
            b"",
            now()
        ),
        Err(Error::Identity(E::NotFound))
    ));
    for suffix in [
        "/operations/forged/execute",
        "/operations/forged/reconcile",
        "/tasks",
        "/budgets",
        "/identity/users",
    ] {
        assert!(
            lab.call(&approval_subject("subject-a"), "POST", suffix, json!({}))
                .is_err()
        );
    }
    assert_eq!(lab.refunded(), 0);
}

#[test]
fn all_identity_and_binding_revocations_stop_new_approvals() {
    for resource in [
        "users",
        "subjects",
        "memberships",
        "organizations",
        "gateway_bindings",
        "approver_bindings",
        "approver",
    ] {
        let lab = ApprovalLab::new(2000, false);
        let op = lab.refund(1000);
        let subject = approval_subject("subject-a");
        let id = match resource {
            "users" => &lab.user["user"]["id"],
            "subjects" => &lab.user["identity_id"],
            "memberships" => &lab.membership["id"],
            "organizations" => &lab.org["id"],
            "gateway_bindings" => &lab.gateway_binding["id"],
            _ => &lab.approver_binding["id"],
        };
        if resource == "approver" {
            lab.service
                .revoke_approver(&owner(), &lab.approver.approver.id, now())
                .unwrap();
        } else {
            lab.service
                .revoke_identity_record(&owner(), resource, id.as_str().unwrap(), now())
                .unwrap();
        }
        assert!(lab.approve(&subject, &op).is_err(), "{resource}");
        assert_eq!(lab.refunded(), 0);
        assert!(lab.service.verify_evidence().unwrap().ok());
    }
}

#[test]
fn binding_permanently_disables_old_key_and_prevents_duplicate_person_slots() {
    let lab = ApprovalLab::new(2000, true);
    let op = lab.refund(1000);
    let credentials = Credentials::initialize(&lab.dir.path().join("credentials.json")).unwrap();
    let legacy_call = || {
        gateway::dispatch(
            &lab.service,
            &FixturePlanner,
            &credentials,
            &format!("Bearer {}", lab.approver.token),
            "POST",
            &format!("/gateway/v1/operations/{}/approve", op.id),
            &serde_json::to_vec(
                &json!({"proposal_digest":op.proposal.as_ref().unwrap().digest().unwrap()}),
            )
            .unwrap(),
            now(),
        )
    };
    assert!(matches!(legacy_call(), Err(Error::Forbidden)));
    let other = lab
        .service
        .register_approver(
            &owner(),
            cosigna_agent::approvers::ApproverInput {
                name: "Alias".into(),
                currency: "USD".into(),
                max_approval_minor: 2000,
            },
            now(),
        )
        .unwrap();
    assert!(matches!(
        lab.service.bind_identity_approver(
            &owner(),
            ApproverBindingInput {
                membership_id: lab.membership["id"].as_str().unwrap().into(),
                approver_id: other.approver.id.clone()
            },
            now()
        ),
        Err(Error::Conflict(_))
    ));
    lab.service
        .revoke_identity_record(
            &owner(),
            "approver_bindings",
            lab.approver_binding["id"].as_str().unwrap(),
            now(),
        )
        .unwrap();
    assert!(matches!(legacy_call(), Err(Error::Forbidden)));
    assert!(matches!(
        lab.service.bind_identity_approver(
            &owner(),
            ApproverBindingInput {
                membership_id: lab.membership["id"].as_str().unwrap().into(),
                approver_id: other.approver.id
            },
            now()
        ),
        Err(Error::Conflict(_))
    ));
    assert_eq!(lab.refunded(), 0);
}

#[test]
fn two_identity_approvers_are_required_and_replaying_one_identity_cannot_finish() {
    let lab = ApprovalLab::new(2000, true);
    let op = lab.refund(1000);
    let first = approval_subject("subject-a");
    assert_eq!(lab.approve(&first, &op).unwrap()["data"]["stage"], "held");
    assert_eq!(lab.refunded(), 0);
    assert!(
        matches!(lab.approve(&first,&op),Err(Error::Conflict(s)) if s.starts_with("second_approver_required"))
    );
    let user = enroll(&lab.service, "subject-b");
    let membership = member(&lab.service, &user, &lab.org);
    let approver = lab
        .service
        .register_approver(
            &owner(),
            cosigna_agent::approvers::ApproverInput {
                name: "Alex".into(),
                currency: "USD".into(),
                max_approval_minor: 2000,
            },
            now(),
        )
        .unwrap();
    lab.service
        .bind_identity_approver(
            &owner(),
            ApproverBindingInput {
                membership_id: membership["id"].as_str().unwrap().into(),
                approver_id: approver.approver.id,
            },
            now(),
        )
        .unwrap();
    assert_eq!(
        lab.approve(&approval_subject("subject-b"), &op).unwrap()["data"]["stage"],
        "succeeded"
    );
    assert_eq!(lab.refunded(), 1000);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn a_revoked_first_identity_cannot_be_counted_when_the_second_identity_approves() {
    let lab = ApprovalLab::new(2000, true);
    let op = lab.refund(1000);
    lab.approve(&approval_subject("subject-a"), &op).unwrap();
    let user = enroll(&lab.service, "subject-b");
    let membership = member(&lab.service, &user, &lab.org);
    let approver = lab
        .service
        .register_approver(
            &owner(),
            cosigna_agent::approvers::ApproverInput {
                name: "Alex".into(),
                currency: "USD".into(),
                max_approval_minor: 2000,
            },
            now(),
        )
        .unwrap();
    lab.service
        .bind_identity_approver(
            &owner(),
            ApproverBindingInput {
                membership_id: membership["id"].as_str().unwrap().into(),
                approver_id: approver.approver.id,
            },
            now(),
        )
        .unwrap();
    lab.service
        .revoke_identity_record(
            &owner(),
            "subjects",
            lab.user["identity_id"].as_str().unwrap(),
            now(),
        )
        .unwrap();
    assert!(
        matches!(lab.approve(&approval_subject("subject-b"),&op),Err(Error::Conflict(s)) if s.starts_with("first_approval_no_longer_authorized"))
    );
    assert_eq!(lab.refunded(), 0);
    assert_eq!(
        lab.service.operation(&owner(), &op.id).unwrap().stage,
        cosigna_agent::operations::OperationStage::Held
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn version_ten_upgrade_creates_empty_bindings_and_preserves_existing_identity_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path()).unwrap();
    enroll(&service, "subject-a");
    let head = service.verify_evidence().unwrap().head;
    let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
    db.execute_batch("DROP TABLE identity_approver_bindings; DROP TABLE identity_gateway_bindings; PRAGMA user_version=10;").unwrap();
    drop(db);
    let upgraded = Service::open(dir.path()).unwrap();
    assert_eq!(upgraded.verify_evidence().unwrap().head, head);
    assert!(upgraded.verify_evidence().unwrap().ok());
    assert!(
        upgraded
            .identity_read(&subject(), "GET", "/identity/v1/me")
            .is_ok()
    );
    let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM identity_approver_bindings", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
}

#[test]
fn cross_owner_operations_and_binding_targets_are_not_visible_or_mutable() {
    let lab = ApprovalLab::new(2000, false);
    let other = Actor {
        owner: "other-owner".into(),
        role: Role::Owner,
    };
    let budget = lab
        .service
        .create_budget(
            &other,
            cosigna_agent::BudgetInput {
                currency: "USD".into(),
                limit_minor: 10_000,
                per_purchase_minor: 10_000,
                merchants: vec!["fixture-cafe".into()],
                expires_at: now() + 3600,
                second_approval_above_minor: None,
            },
            now(),
        )
        .unwrap();
    let op=lab.service.request_operation(&other,serde_json::from_value(json!({"budget_id":budget.id,"kind":"refund","rail":"fixture","source":"pay_fixture_2041","amount_minor":1000,"currency":"USD","reason":"Other owner","idempotency_key":"other"})).unwrap(),now()).unwrap();
    assert!(matches!(
        lab.approve(&approval_subject("subject-a"), &op),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        lab.call(
            &approval_subject("subject-a"),
            "GET",
            &format!("/operations/{}", op.id),
            json!({})
        ),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        lab.service.bind_identity_approver(
            &other,
            ApproverBindingInput {
                membership_id: lab.membership["id"].as_str().unwrap().into(),
                approver_id: lab.approver.approver.id.clone()
            },
            now()
        ),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        lab.service.bind_identity_gateway(
            &other,
            GatewayBindingInput {
                organization_id: lab.org["id"].as_str().unwrap().into()
            },
            now()
        ),
        Err(Error::NotFound)
    ));
    assert_eq!(lab.refunded(), 0);
}

#[test]
fn binding_state_changes_are_covered_by_the_existing_ledger_evidence_verifier() {
    for sql in [
        "UPDATE identity_gateway_bindings SET revoked=1",
        "UPDATE identity_approver_bindings SET revoked=1",
        "DELETE FROM identity_approver_bindings",
    ] {
        let lab = ApprovalLab::new(2000, false);
        assert!(lab.service.verify_evidence().unwrap().ok());
        let db = rusqlite::Connection::open(lab.dir.path().join("transactions.sqlite")).unwrap();
        db.execute(sql, []).unwrap();
        assert!(!lab.service.verify_evidence().unwrap().ok(), "{sql}");
    }
}
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

const ISSUER: &str = "https://identity.example/";
const AUDIENCE: &str = "cosigna-test";
fn key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap())
}
fn other_key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap())
}
fn jwk(key: &RsaPrivateKey, kid: &str) -> Value {
    json!({"kty":"RSA","kid":kid,"alg":"RS256","use":"sig",
        "n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())})
}
fn keys() -> Value {
    json!({"keys":[jwk(key(),"test-key")]})
}
fn verifier(raw: &Value) -> Result<Verifier, E> {
    Verifier::new(
        ISSUER.into(),
        AUDIENCE.into(),
        vec!["console".into()],
        &serde_json::to_vec(raw).unwrap(),
    )
}
fn claims() -> Value {
    let at = now();
    json!({"iss":ISSUER,"sub":"subject-a","aud":AUDIENCE,"iat":at,"exp":at+300,"jti":"test-request","client_id":"console","scope":"cosigna:read"})
}
fn header() -> Header {
    let mut h = Header::new(Algorithm::RS256);
    h.typ = Some("at+jwt".into());
    h.kid = Some("test-key".into());
    h
}
fn signed(c: &Value, h: &Header, key: &RsaPrivateKey) -> String {
    format!(
        "Bearer {}",
        encode(
            h,
            c,
            &EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes())
        )
        .unwrap()
    )
}
fn auth() -> String {
    signed(&claims(), &header(), key())
}
fn subject() -> Subject {
    verifier(&keys()).unwrap().verify(&auth(), now()).unwrap()
}
fn owner() -> Actor {
    Actor {
        owner: "operator-a".into(),
        role: Role::Owner,
    }
}
fn enrollment(sub: &str) -> Enrollment {
    Enrollment {
        display_name: "Development user".into(),
        issuer: ISSUER.into(),
        subject: sub.into(),
    }
}
fn enroll(s: &Service, sub: &str) -> Value {
    s.enroll_identity(&owner(), enrollment(sub), now()).unwrap()
}
fn org(s: &Service, name: &str) -> Value {
    s.create_identity_organization(&owner(), OrganizationInput { name: name.into() }, now())
        .unwrap()
}
fn member(s: &Service, user: &Value, org: &Value) -> Value {
    s.create_identity_membership(
        &owner(),
        MembershipInput {
            user_id: user["user"]["id"].as_str().unwrap().into(),
            organization_id: org["id"].as_str().unwrap().into(),
            role: "viewer".into(),
        },
        now(),
    )
    .unwrap()
}

#[test]
fn trusted_access_token_profile_and_array_audience_work() {
    let v = verifier(&keys()).unwrap();
    assert!(v.verify(&auth(), now()).is_ok());
    let mut c = claims();
    c["aud"] = json!(["another-api", AUDIENCE]);
    let mut h = header();
    h.typ = Some("APPLICATION/AT+JWT".into());
    assert!(v.verify(&signed(&c, &h, key()), now()).is_ok());
}

#[test]
fn forged_algorithm_id_token_and_remote_key_headers_are_refused() {
    let v = verifier(&keys()).unwrap();
    assert!(matches!(
        v.verify(&signed(&claims(), &header(), other_key()), now()),
        Err(E::InvalidToken)
    ));
    for typ in [None, Some("JWT"), Some("id+jwt")] {
        let mut h = header();
        h.typ = typ.map(String::from);
        assert!(matches!(
            v.verify(&signed(&claims(), &h, key()), now()),
            Err(E::InvalidToken)
        ));
    }
    for field in ["jku", "jwk", "x5u", "x5c", "crit", "unexpected"] {
        let mut h = header();
        h.extras
            .insert(field, json!("https://untrusted.example/keys"));
        assert!(
            matches!(
                v.verify(&signed(&claims(), &h, key()), now()),
                Err(E::InvalidToken)
            ),
            "{field}"
        );
    }
    let mut h = header();
    h.kid = Some("unknown".into());
    assert!(matches!(
        v.verify(&signed(&claims(), &h, key()), now()),
        Err(E::InvalidToken)
    ));
    let mut h = header();
    h.alg = Algorithm::HS256;
    let token = encode(
        &h,
        &claims(),
        &EncodingKey::from_secret(b"synthetic-hmac-secret"),
    )
    .unwrap();
    assert!(matches!(
        v.verify(&format!("Bearer {token}"), now()),
        Err(E::InvalidToken)
    ));
}

#[test]
fn required_claims_and_integer_time_boundaries_fail_closed() {
    let v = verifier(&keys()).unwrap();
    let at = now();
    for field in [
        "iss",
        "sub",
        "aud",
        "exp",
        "iat",
        "jti",
        "client_id",
        "scope",
    ] {
        let mut c = claims();
        c.as_object_mut().unwrap().remove(field);
        assert!(
            matches!(
                v.verify(&signed(&c, &header(), key()), at),
                Err(E::InvalidToken)
            ),
            "missing {field}"
        );
    }
    for (field, bad) in [
        ("iss", json!("https://wrong.example/")),
        ("aud", json!("wrong")),
        ("client_id", json!("other")),
        ("sub", json!("")),
        ("jti", json!("")),
        ("sub", json!("a b")),
        ("exp", json!(at)),
        ("exp", json!(at - 1)),
        ("iat", json!(at + 100)),
        ("exp", json!(at + 3601)),
        ("exp", json!((at + 300).to_string())),
        ("iat", json!(at as f64)),
        ("nbf", Value::Null),
        ("nbf", json!("0")),
        ("nbf", json!(at + 100)),
        ("nbf", json!(-1)),
        ("aud", json!([AUDIENCE, 7])),
        ("aud", json!([])),
        ("scope", json!("cosigna:read\ncosigna:pay")),
    ] {
        let mut c = claims();
        c[field] = bad;
        assert!(
            matches!(
                v.verify(&signed(&c, &header(), key()), at),
                Err(E::InvalidToken)
            ),
            "{field}"
        );
    }
}

#[test]
fn malformed_shapes_bearer_parsing_and_scope_have_safe_errors() {
    let v = verifier(&keys()).unwrap();
    for c in [Value::Null, json!([]), json!(123), json!("text")] {
        assert!(matches!(
            v.verify(&signed(&c, &header(), key()), now()),
            Err(E::InvalidToken)
        ));
    }
    for token in [
        String::new(),
        "Basic secret".into(),
        "Bearer a.b.c.d".into(),
        format!("{},other", auth()),
        format!("Bearer {}", "x".repeat(16_384)),
    ] {
        assert!(matches!(v.verify(&token, now()), Err(E::InvalidToken)));
    }
    let mut c = claims();
    c["scope"] = json!("openid profile");
    assert!(matches!(
        v.verify(&signed(&c, &header(), key()), now()),
        Err(E::InsufficientScope)
    ));
    let (status, error) = E::InvalidToken.response();
    assert_eq!(status, 401);
    assert_eq!(
        error,
        json!({"schema":"cosigna.identity/v1","error":"invalid_token"})
    );
}

#[test]
fn keys_and_provider_configuration_are_strictly_operator_controlled() {
    let k = jwk(key(), "test-key");
    let mut sets = vec![
        json!({"keys":[]}),
        json!({"keys":[k.clone(),k.clone()]}),
        json!({"keys":[null]}),
    ];
    for (name, bad) in [
        ("d", json!("private")),
        ("p", json!("private")),
        ("k", json!("secret")),
        ("use", json!("enc")),
        ("alg", json!("HS256")),
        ("key_ops", json!(["sign"])),
        ("n", json!("invalid!")),
        ("n", json!(URL_SAFE_NO_PAD.encode(vec![255u8; 128]))),
        ("e", json!("Aw")),
        ("kid", json!("")),
    ] {
        let mut bad_key = k.clone();
        bad_key[name] = bad;
        sets.push(json!({"keys":[bad_key]}));
    }
    for set in sets {
        assert!(matches!(verifier(&set), Err(E::ConfigurationInvalid)));
    }
    for issuer in [
        "http://identity.example/",
        "https://user:pass@identity.example/",
        "https://identity.example/?x=y",
        "https://identity.example/#fragment",
    ] {
        assert!(matches!(
            Verifier::new(
                issuer.into(),
                AUDIENCE.into(),
                vec!["console".into()],
                &serde_json::to_vec(&keys()).unwrap()
            ),
            Err(E::ConfigurationInvalid)
        ));
    }
    for clients in [vec![], vec!["console".into(), "console".into()]] {
        assert!(matches!(
            Verifier::new(
                ISSUER.into(),
                AUDIENCE.into(),
                clients,
                &serde_json::to_vec(&keys()).unwrap()
            ),
            Err(E::ConfigurationInvalid)
        ));
    }
}

fn settings(path: &std::path::Path) -> [Option<String>; 4] {
    [
        Some(ISSUER.into()),
        Some(AUDIENCE.into()),
        Some("[\"console\"]".into()),
        Some(path.to_str().unwrap().into()),
    ]
}

#[test]
fn configuration_absence_partial_updates_and_key_retirement_fail_closed() {
    assert!(matches!(
        Verifier::from_settings([None, None, None, None]),
        Err(E::Unconfigured)
    ));
    assert!(matches!(
        Verifier::from_settings([Some(ISSUER.into()), None, None, None]),
        Err(E::ConfigurationInvalid)
    ));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("public.json");
    std::fs::write(&path, serde_json::to_vec(&keys()).unwrap()).unwrap();
    assert!(
        Verifier::from_settings(settings(&path))
            .unwrap()
            .verify(&auth(), now())
            .is_ok()
    );
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({"keys":[jwk(other_key(),"next-key")]})).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        Verifier::from_settings(settings(&path))
            .unwrap()
            .verify(&auth(), now()),
        Err(E::InvalidToken)
    ));
    let mut h = header();
    h.kid = Some("next-key".into());
    assert!(
        Verifier::from_settings(settings(&path))
            .unwrap()
            .verify(&signed(&claims(), &h, other_key()), now())
            .is_ok()
    );
    for bad in [String::new(), "not json".into(), "x".repeat(65_537)] {
        std::fs::write(&path, bad).unwrap();
        assert!(matches!(
            Verifier::from_settings(settings(&path)),
            Err(E::ConfigurationInvalid)
        ));
    }
    assert!(matches!(
        Verifier::from_settings(settings(std::path::Path::new("relative.json"))),
        Err(E::ConfigurationInvalid)
    ));
}

#[test]
fn organization_reads_use_stored_membership_never_claimed_email_role_or_owner() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    let a = enroll(&s, "subject-a");
    let b = enroll(&s, "subject-b");
    let oa = org(&s, "A");
    let ob = org(&s, "B");
    member(&s, &a, &oa);
    member(&s, &b, &ob);
    let mut c = claims();
    c["email"] = json!("admin@example.com");
    c["role"] = json!("owner");
    c["organization_id"] = ob["id"].clone();
    c["owner"] = json!("operator-a");
    let identity = verifier(&keys())
        .unwrap()
        .verify(&signed(&c, &header(), key()), now())
        .unwrap();
    let result = s
        .identity_read(&identity, "GET", "/identity/v1/organizations")
        .unwrap();
    assert_eq!(
        result["data"],
        json!([{"id":oa["id"],"name":"A","role":"viewer"}])
    );
    let forbidden = s.identity_read(
        &identity,
        "GET",
        &format!("/identity/v1/organizations/{}", ob["id"].as_str().unwrap()),
    );
    let missing = s.identity_read(
        &identity,
        "GET",
        &format!("/identity/v1/organizations/org_{}", "0".repeat(64)),
    );
    assert_eq!(forbidden, Err(E::NotFound));
    assert_eq!(missing, forbidden);
    assert_eq!(
        s.identity_read(&identity, "POST", "/identity/v1/organizations"),
        Err(E::NotFound)
    );
    let mut c = claims();
    c["sub"] = json!("unenrolled");
    c["email"] = json!("admin@example.com");
    let stranger = verifier(&keys())
        .unwrap()
        .verify(&signed(&c, &header(), key()), now())
        .unwrap();
    assert_eq!(
        s.identity_read(&stranger, "GET", "/identity/v1/me"),
        Err(E::InvalidToken)
    );
    assert!(s.verify_evidence().unwrap().ok());
}

#[test]
fn each_revocation_is_effective_on_the_next_read_and_is_permanent() {
    for resource in ["users", "subjects", "organizations", "memberships"] {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::open(dir.path()).unwrap();
        let a = enroll(&s, "subject-a");
        let oa = org(&s, "A");
        let membership = member(&s, &a, &oa);
        let identity = subject();
        assert_eq!(
            s.identity_read(&identity, "GET", "/identity/v1/organizations")
                .unwrap()["data"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let id = match resource {
            "users" => &a["user"]["id"],
            "subjects" => &a["identity_id"],
            "organizations" => &oa["id"],
            _ => &membership["id"],
        }
        .as_str()
        .unwrap();
        s.revoke_identity_record(&owner(), resource, id, now())
            .unwrap();
        s.revoke_identity_record(&owner(), resource, id, now())
            .unwrap();
        let result = s.identity_read(&identity, "GET", "/identity/v1/organizations");
        if matches!(resource, "users" | "subjects") {
            assert_eq!(result, Err(E::InvalidToken));
        } else {
            assert_eq!(result.unwrap()["data"], json!([]));
        }
        assert!(s.verify_evidence().unwrap().ok());
    }
}

#[test]
fn pagination_is_bounded_and_cursors_grant_no_membership() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    let a = enroll(&s, "subject-a");
    for i in 0..52 {
        let organization = org(&s, &format!("Organization {i}"));
        member(&s, &a, &organization);
    }
    let subject = subject();
    let first = s
        .identity_read(&subject, "GET", "/identity/v1/organizations")
        .unwrap();
    assert_eq!(first["data"].as_array().unwrap().len(), 50);
    let next = first["next_cursor"].as_str().unwrap();
    let second = s
        .identity_read(
            &subject,
            "GET",
            &format!("/identity/v1/organizations?after={next}"),
        )
        .unwrap();
    assert_eq!(second["data"].as_array().unwrap().len(), 2);
    assert_eq!(second["next_cursor"], Value::Null);
    for query in [
        "after=bad",
        "after=org_1&token=secret",
        "role=owner",
        "after=",
    ] {
        assert_eq!(
            s.identity_read(
                &subject,
                "GET",
                &format!("/identity/v1/organizations?{query}")
            ),
            Err(E::InvalidCursor)
        );
    }
}

#[test]
fn operator_enrollment_is_unique_scoped_and_roles_cannot_authorize_money() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    let a = enroll(&s, "subject-a");
    let o = org(&s, "A");
    member(&s, &a, &o);
    assert!(matches!(
        s.enroll_identity(&owner(), enrollment("subject-a"), now()),
        Err(Error::Conflict(_))
    ));
    let planner = Actor {
        role: Role::Planner,
        ..owner()
    };
    assert!(matches!(
        s.enroll_identity(&planner, enrollment("other"), now()),
        Err(Error::Forbidden)
    ));
    let other = Actor {
        owner: "other-operator".into(),
        role: Role::Owner,
    };
    assert!(matches!(
        s.create_identity_membership(
            &other,
            MembershipInput {
                user_id: a["user"]["id"].as_str().unwrap().into(),
                organization_id: o["id"].as_str().unwrap().into(),
                role: "owner".into()
            },
            now()
        ),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.revoke_identity_record(&other, "users", a["user"]["id"].as_str().unwrap(), now()),
        Err(Error::NotFound)
    ));
    let credentials = Credentials::initialize(&dir.path().join("credentials.json")).unwrap();
    for (method, path, body) in [
        ("GET", "/gateway/v1/operations", json!({})),
        (
            "POST",
            "/gateway/v1/identity/users",
            json!({"display_name":"self","issuer":ISSUER,"subject":"self"}),
        ),
        (
            "POST",
            "/gateway/v1/operations/op_unknown/approve",
            json!({"proposal_digest":"forged"}),
        ),
    ] {
        assert!(
            gateway::dispatch(
                &s,
                &FixturePlanner,
                &credentials,
                &auth(),
                method,
                path,
                &serde_json::to_vec(&body).unwrap(),
                now()
            )
            .is_err()
        );
    }
    assert!(s.verify_evidence().unwrap().ok());
}

#[test]
fn ledger_constraints_and_evidence_detect_registry_tampering_and_deletion() {
    for mutation in [
        "UPDATE identity_subjects SET subject='attacker'",
        "UPDATE identity_users SET display_name='attacker'",
        "UPDATE identity_memberships SET role='owner'",
        "UPDATE identity_organizations SET revoked=1",
        "DELETE FROM identity_memberships",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let s = Service::open(dir.path()).unwrap();
        let a = enroll(&s, "subject-a");
        let o = org(&s, "A");
        member(&s, &a, &o);
        assert!(s.verify_evidence().unwrap().ok());
        let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        assert!(
            db.execute("UPDATE identity_memberships SET role='superuser'", [])
                .is_err()
        );
        assert!(
            db.execute("UPDATE identity_memberships SET user_id='missing'", [])
                .is_err()
        );
        db.execute(mutation, []).unwrap();
        let report = s.verify_evidence().unwrap();
        assert!(report.chain_intact);
        assert!(!report.state_matches, "{mutation}");
    }
}

#[test]
fn version_nine_migration_is_additive_and_reopens_without_new_records() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    let before = s.verify_evidence().unwrap();
    let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
    db.execute_batch("DROP TABLE identity_approver_bindings; DROP TABLE identity_gateway_bindings; DROP TABLE identity_memberships; DROP TABLE identity_subjects; DROP TABLE identity_organizations; DROP TABLE identity_users; PRAGMA user_version=9;").unwrap();
    drop(db);
    let upgraded = Service::open(dir.path()).unwrap();
    assert_eq!(upgraded.verify_evidence().unwrap().head, before.head);
    let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM identity_users", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    assert_eq!(
        Service::open(dir.path())
            .unwrap()
            .verify_evidence()
            .unwrap()
            .head,
        before.head
    );
}

#[test]
fn operator_routes_derive_owner_reject_unknown_fields_and_record_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    let credentials = Credentials::initialize(&dir.path().join("credentials.json")).unwrap();
    let raw: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("credentials.json")).unwrap())
            .unwrap();
    let authorization = format!("Bearer {}", raw["owner"].as_str().unwrap());
    let call = |auth: &str, path: &str, body: Value| {
        gateway::dispatch(
            &s,
            &FixturePlanner,
            &credentials,
            auth,
            "POST",
            path,
            &serde_json::to_vec(&body).unwrap(),
            now(),
        )
    };
    let user_body =
        json!({"display_name":"Development user","issuer":ISSUER,"subject":"subject-a"});
    let mut forged = user_body.clone();
    forged["owner"] = json!("operator-a");
    assert!(matches!(
        call(&authorization, "/gateway/v1/identity/users", forged),
        Err(Error::Invalid(_))
    ));
    for credential in [
        raw["planner"].as_str().unwrap(),
        raw["executor"].as_str().unwrap(),
    ] {
        assert!(matches!(
            call(
                &format!("Bearer {credential}"),
                "/gateway/v1/identity/users",
                user_body.clone()
            ),
            Err(Error::Forbidden)
        ));
    }
    let user = call(&authorization, "/gateway/v1/identity/users", user_body).unwrap();
    assert_eq!(user["schema"], "cosigna.identity/v1");
    let organization = call(
        &authorization,
        "/gateway/v1/identity/organizations",
        json!({"name":"Development company"}),
    )
    .unwrap();
    let membership=call(&authorization,"/gateway/v1/identity/memberships",json!({"user_id":user["data"]["user"]["id"],"organization_id":organization["data"]["id"],"role":"admin"})).unwrap();
    assert_eq!(
        s.identity_read(&subject(), "GET", "/identity/v1/organizations")
            .unwrap()["data"][0]["role"],
        "admin"
    );
    let path = format!(
        "/gateway/v1/identity/memberships/{}/revoke",
        membership["data"]["id"].as_str().unwrap()
    );
    assert!(matches!(
        call(&authorization, &path, json!({"role":"owner"})),
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        call(&authorization, &path, json!({})).unwrap()["data"]["revoked"],
        true
    );
    assert_eq!(
        s.identity_read(&subject(), "GET", "/identity/v1/organizations")
            .unwrap()["data"],
        json!([])
    );
    assert!(s.verify_evidence().unwrap().ok());
}

#[test]
fn identity_storage_failure_is_a_fixed_unavailable_error() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    enroll(&s, "subject-a");
    let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
    db.execute_batch("DROP TABLE identity_approver_bindings; DROP TABLE identity_gateway_bindings; DROP TABLE identity_memberships;")
        .unwrap();
    assert_eq!(
        s.identity_read(&subject(), "GET", "/identity/v1/organizations"),
        Err(E::Unavailable)
    );
    assert_eq!(
        E::Unavailable.response(),
        (
            503,
            json!({"schema":"cosigna.identity/v1","error":"identity_unavailable"})
        )
    );
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn identity_approvals_keep_the_named_approvers_currency_boundary() {
    let lab = ApprovalLab::new(2000, false);
    let budget = lab
        .service
        .create_budget(
            &owner(),
            cosigna_agent::BudgetInput {
                currency: "INR".into(),
                limit_minor: 10_000,
                per_purchase_minor: 10_000,
                merchants: vec!["fixture-cafe".into()],
                expires_at: now() + 3600,
                second_approval_above_minor: None,
            },
            now(),
        )
        .unwrap();
    let op=lab.service.request_operation(&owner(),serde_json::from_value(json!({"budget_id":budget.id,"kind":"refund","rail":"fixture","source":"pay_fixture_3001","amount_minor":1000,"currency":"INR","reason":"Synthetic refund","idempotency_key":"currency"})).unwrap(),now()).unwrap();
    assert!(
        matches!(lab.approve(&approval_subject("subject-a"),&op),Err(Error::Conflict(s)) if s.starts_with("above_approver_limit"))
    );
    assert_eq!(
        lab.service
            .rails
            .fixture_rail()
            .unwrap()
            .refunded_minor("pay_fixture_3001")
            .unwrap(),
        0
    );
}

#[test]
fn real_http_identity_approval_enforces_scope_content_type_and_exact_proposal() {
    let lab = ApprovalLab::new(2000, false);
    let op = lab.refund(1000);
    Credentials::initialize(&lab.dir.path().join("credentials.json")).unwrap();
    let public = lab.dir.path().join("public.json");
    std::fs::write(&public, serde_json::to_vec(&keys()).unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .args(["serve", "--data-dir"])
        .arg(lab.dir.path())
        .args(["--port", &port.to_string()])
        .env("COSIGNA_IDENTITY_ISSUER", ISSUER)
        .env("COSIGNA_IDENTITY_AUDIENCE", AUDIENCE)
        .env("COSIGNA_IDENTITY_CLIENT_IDS", "[\"console\"]")
        .env("COSIGNA_IDENTITY_JWKS_PATH", &public)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _process = Process(child);
    let started = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    let client = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(4)))
        .build()
        .new_agent();
    let url = format!(
        "http://127.0.0.1:{port}/identity/v1/organizations/{}/operations/{}/approve",
        lab.org["id"].as_str().unwrap(),
        op.id
    );
    let body = json!({"proposal_digest":op.proposal.as_ref().unwrap().digest().unwrap()});
    let read_only = client
        .post(&url)
        .header("Authorization", auth())
        .send_json(&body)
        .unwrap();
    assert_eq!(read_only.status(), 403);
    assert!(
        read_only.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("cosigna:approve")
    );
    let mut c = claims();
    c["scope"] = json!("cosigna:read cosigna:approve");
    let authorization = signed(&c, &header(), key());
    assert_eq!(
        client
            .post(&url)
            .header("Authorization", &authorization)
            .header("Content-Type", "text/plain")
            .send(serde_json::to_string(&body).unwrap())
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .post(&url)
            .header("Authorization", &authorization)
            .send_json(json!({"proposal_digest":"tampered"}))
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client
            .post(&url)
            .header("Authorization", &authorization)
            .send_json(&body)
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .post(&url)
            .header("Authorization", &authorization)
            .send_json(&body)
            .unwrap()
            .status(),
        200
    );
    assert_eq!(lab.refunded(), 1000);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn real_http_gateway_uses_identity_status_codes_no_store_and_no_payment_authority() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path()).unwrap();
    enroll(&s, "subject-a");
    Credentials::initialize(&dir.path().join("credentials.json")).unwrap();
    let public = dir.path().join("public.json");
    std::fs::write(&public, serde_json::to_vec(&keys()).unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .args(["serve", "--data-dir"])
        .arg(dir.path())
        .args(["--port", &port.to_string()])
        .env("COSIGNA_IDENTITY_ISSUER", ISSUER)
        .env("COSIGNA_IDENTITY_AUDIENCE", AUDIENCE)
        .env("COSIGNA_IDENTITY_CLIENT_IDS", "[\"console\"]")
        .env("COSIGNA_IDENTITY_JWKS_PATH", &public)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _process = Process(child);
    let started = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    let client = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(4)))
        .build()
        .new_agent();
    let address = format!("http://127.0.0.1:{port}");
    let authorization = auth();
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(stream,"GET /identity/v1/me HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {authorization}\r\nAuthorization: {authorization}\r\nConnection: close\r\n\r\n").unwrap();
    let mut duplicate_response = String::new();
    stream.read_to_string(&mut duplicate_response).unwrap();
    assert!(duplicate_response.starts_with("HTTP/1.1 401"));
    let mut result = client
        .get(format!("{address}/identity/v1/me"))
        .header("Authorization", &authorization)
        .call()
        .unwrap();
    assert_eq!(result.status(), 200);
    assert_eq!(result.headers()["cache-control"], "no-store");
    let body = result.body_mut().read_to_string().unwrap();
    assert!(!body.contains("subject-a"));
    assert!(!body.contains(ISSUER));
    assert!(!body.contains(&authorization));
    for path in ["/identity/v1/me", "/identity/v1/me?token=anything"] {
        let mut result = client.get(format!("{address}{path}")).call().unwrap();
        assert_eq!(result.status(), 401);
        assert!(result.headers().contains_key("www-authenticate"));
        assert_eq!(
            result.body_mut().read_json::<Value>().unwrap()["error"],
            "invalid_token"
        );
    }
    let mut c = claims();
    c["scope"] = json!("openid");
    let result = client
        .get(format!("{address}/identity/v1/me"))
        .header("Authorization", signed(&c, &header(), key()))
        .call()
        .unwrap();
    assert_eq!(result.status(), 403);
    assert!(
        result.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("cosigna:read")
    );
    for path in [
        "/gateway/v1/operations",
        "/gateway/v1/identity/users",
        "/v1/catalog",
    ] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .header("Authorization", &authorization)
                .call()
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        client
            .get(format!("{address}/identity/v1/me"))
            .header("Authorization", &authorization)
            .header("Origin", "https://untrusted.example")
            .call()
            .unwrap()
            .status(),
        403
    );
    std::fs::write(&public, b"").unwrap();
    assert_eq!(
        client
            .get(format!("{address}/identity/v1/me"))
            .header("Authorization", &authorization)
            .call()
            .unwrap()
            .status(),
        503
    );
    assert_eq!(
        client
            .get(format!("{address}/v1/health"))
            .call()
            .unwrap()
            .status(),
        200
    );
}
