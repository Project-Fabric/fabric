//! Synthetic identities only. Private signing keys are generated in memory, never persisted.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cosigna_agent::{
    Actor, Error, Role, Service,
    api::Credentials,
    gateway,
    identity::{
        Enrollment, IdentityError as E, MembershipInput, OrganizationInput, Subject, Verifier,
    },
    now,
    planner::FixturePlanner,
};
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
    db.execute_batch("DROP TABLE identity_memberships; DROP TABLE identity_subjects; DROP TABLE identity_organizations; DROP TABLE identity_users; PRAGMA user_version=9;").unwrap();
    drop(db);
    let upgraded = Service::open(dir.path()).unwrap();
    assert_eq!(upgraded.verify_evidence().unwrap().head, before.head);
    let db = rusqlite::Connection::open(dir.path().join("transactions.sqlite")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        10
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
    db.execute_batch("DROP TABLE identity_memberships;")
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
