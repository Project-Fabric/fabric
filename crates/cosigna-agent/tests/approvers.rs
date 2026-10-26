//! Named approvers: people with their own credential and a limit who decide held operations.
//! Every decision names who made it, limits and revocation are enforced inside the deciding
//! transaction, and approvers can do nothing else. Synthetic stores in temporary folders only.
use cosigna_agent::{Error, Service, api::Credentials, gateway};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

struct Lab {
    directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
    bot: String,
}

impl Lab {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path()).unwrap();
        let credentials =
            Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
        let private: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("credentials.json")).unwrap(),
        )
        .unwrap();
        let owner = format!("Bearer {}", private["owner"].as_str().unwrap());
        let mut lab = Self {
            directory,
            service,
            credentials,
            owner,
            budget: String::new(),
            bot: String::new(),
        };
        lab.budget = lab
            .call(
                &lab.owner,
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,"per_purchase_minor":50000,
                    "merchants":["fixture-cafe"],"expires_at":AT+86400*2}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let issued = lab
            .call(
                &lab.owner,
                "POST",
                "/agents",
                json!({"name":"refunds-bot","budget_id":lab.budget,"permissions":["execute"],
                    "expires_at":AT+86400,
                    "operations":{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":1000}}),
            )
            .unwrap();
        lab.bot = format!("Bearer {}", issued["data"]["token"].as_str().unwrap());
        lab
    }

    fn call(&self, auth: &str, method: &str, path: &str, body: Value) -> Result<Value, Error> {
        let bytes = if method == "GET" {
            Vec::new()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        gateway::dispatch(
            &self.service,
            &cosigna_agent::planner::FixturePlanner,
            &self.credentials,
            auth,
            method,
            &format!("/gateway/v1{path}"),
            &bytes,
            AT,
        )
    }

    /// Registers an approver; returns (id, bearer).
    fn approver(&self, name: &str, currency: &str, max: i64) -> (String, String) {
        let issued = self
            .call(
                &self.owner,
                "POST",
                "/approvers",
                json!({"name":name,"currency":currency,"max_approval_minor":max}),
            )
            .unwrap();
        assert_eq!(issued["schema"], "cosigna.approver/v1");
        (
            issued["data"]["approver"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
            format!("Bearer {}", issued["data"]["token"].as_str().unwrap()),
        )
    }

    /// A held refund: (operation id, proposal digest).
    fn held(&self, source: &str, amount: i64, key: &str) -> (String, String) {
        let op = self
            .call(
                &self.bot,
                "POST",
                "/operations",
                json!({"budget_id":self.budget,"kind":"refund","rail":"fixture","source":source,
                    "amount_minor":amount,"currency":"USD","reason":"Refund","idempotency_key":key}),
            )
            .unwrap();
        assert_eq!(op["data"]["stage"], "held");
        (
            op["data"]["id"].as_str().unwrap().to_string(),
            op["proposal_digest"].as_str().unwrap().to_string(),
        )
    }

    fn decide(&self, auth: &str, id: &str, approve: Option<&str>) -> Result<Value, Error> {
        match approve {
            Some(digest) => self.call(
                auth,
                "POST",
                &format!("/operations/{id}/approve"),
                json!({"proposal_digest":digest}),
            ),
            None => self.call(auth, "POST", &format!("/operations/{id}/reject"), json!({})),
        }
    }

    fn refunded(&self, payment: &str) -> i64 {
        self.service
            .rails
            .fixture_rail()
            .unwrap()
            .refunded_minor(payment)
            .unwrap()
    }

    fn events(&self) -> Vec<Value> {
        let out = self.directory.path().join("evidence.jsonl");
        let _ = std::fs::remove_file(&out);
        self.service.export_evidence(&out).unwrap();
        std::fs::read_to_string(&out)
            .unwrap()
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l.split_once(' ').unwrap().1).unwrap()["event"]
                    .clone()
            })
            .collect()
    }
}

#[test]
fn an_approver_within_their_limit_approves_and_the_record_names_them() {
    let lab = Lab::new();
    let (priya, token) = lab.approver("Priya (finance ops)", "USD", 50000);
    // The credential is shown once; the list never repeats it.
    let listed = lab
        .call(&lab.owner, "GET", "/approvers", json!({}))
        .unwrap();
    assert_eq!(listed["data"][0]["id"], priya);
    assert!(!listed.to_string().contains("approverkey_"));
    let me = lab.call(&token, "GET", "/approvers/me", json!({})).unwrap();
    assert_eq!(me["data"]["name"], "Priya (finance ops)");

    let (id, digest) = lab.held("pay_fixture_2045", 45000, "big");
    let inbox = lab
        .call(&token, "GET", "/operations?stage=held", json!({}))
        .unwrap();
    assert_eq!(inbox["data"][0]["data"]["id"], id);
    let paid = lab.decide(&token, &id, Some(&digest)).unwrap();
    assert_eq!(paid["data"]["stage"], "succeeded");
    assert_eq!(paid["data"]["approved_by"], "approver");
    assert_eq!(paid["data"]["approver"], priya);
    assert_eq!(lab.refunded("pay_fixture_2045"), 45000);

    let events = lab.events();
    let approval = events
        .iter()
        .find(|e| e["kind"] == "gateway.operation.approver_approved")
        .unwrap();
    assert_eq!(approval["by"]["role"], "approver");
    assert_eq!(approval["by"]["approver"], priya);
    let registered = events
        .iter()
        .find(|e| e["kind"] == "gateway.approver.registered")
        .unwrap();
    assert_eq!(registered["terms"]["max_approval_minor"], 50000);
    // Earlier record shapes are unchanged: the owner's records carry no approver field.
    assert!(registered["by"].get("approver").is_none());
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn above_the_limit_or_in_another_currency_is_refused_but_rejecting_is_allowed() {
    let lab = Lab::new();
    let (_, small) = lab.approver("Sam (support lead)", "USD", 10000);
    let (_, euros) = lab.approver("Eva", "EUR", 1_000_000);
    let (id, digest) = lab.held("pay_fixture_2045", 45000, "too-big");
    for token in [&small, &euros] {
        let refused = lab.decide(token, &id, Some(&digest));
        assert!(
            matches!(refused, Err(Error::Conflict(ref m)) if m.starts_with("above_approver_limit")),
            "{refused:?}"
        );
    }
    assert_eq!(lab.refunded("pay_fixture_2045"), 0, "no money moved");
    // Anyone authorized to decide may stop a payment.
    let rejected = lab.decide(&small, &id, None).unwrap();
    assert_eq!(rejected["data"]["stage"], "rejected");
    assert!(
        lab.events()
            .iter()
            .any(|e| e["kind"] == "gateway.operation.approver_rejected")
    );
    // The owner keeps full authority.
    let (id, digest) = lab.held("pay_fixture_2049", 46000, "owner");
    let paid = lab.decide(&lab.owner, &id, Some(&digest)).unwrap();
    assert_eq!(paid["data"]["approved_by"], "owner");
    assert!(paid["data"].get("approver").is_none());
}

#[test]
fn a_revoked_approver_can_no_longer_decide() {
    let lab = Lab::new();
    let (priya, token) = lab.approver("Priya", "USD", 50000);
    let (id, digest) = lab.held("pay_fixture_2045", 45000, "revoked");
    lab.call(
        &lab.owner,
        "POST",
        &format!("/approvers/{priya}/revoke"),
        json!({}),
    )
    .unwrap();
    assert!(matches!(
        lab.decide(&token, &id, Some(&digest)),
        Err(Error::Forbidden)
    ));
    // (A revocation landing between authentication and the decision is covered by the unit
    // test in src/approvers.rs.)
    let listed = lab
        .call(&lab.owner, "GET", "/approvers", json!({}))
        .unwrap();
    assert_eq!(listed["data"][0]["revoked"], true);
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn approvers_can_do_nothing_but_read_and_decide_operations() {
    let lab = Lab::new();
    let (_, token) = lab.approver("Priya", "USD", 50000);
    for (method, path, body) in [
        (
            "POST",
            "/budgets",
            json!({"currency":"USD","limit_minor":1,"per_purchase_minor":1,
            "merchants":["fixture-cafe"],"expires_at":AT+60}),
        ),
        (
            "POST",
            "/approvers",
            json!({"name":"Mallory","currency":"USD","max_approval_minor":1}),
        ),
        ("GET", "/approvers", json!({})),
        ("GET", "/exposure", json!({})),
        (
            "POST",
            "/operations",
            json!({"budget_id":lab.budget,"kind":"refund","rail":"fixture",
            "source":"pay_fixture_2041","amount_minor":100,"currency":"USD","reason":"Self-serve",
            "idempotency_key":"approver-request"}),
        ),
        ("POST", "/tasks", json!({})),
    ] {
        assert!(
            matches!(lab.call(&token, method, path, body), Err(Error::NotFound)),
            "{method} {path}"
        );
    }
    // Agent names only, so the inbox can say who asked: no grants, budgets or credentials.
    let names = lab.call(&token, "GET", "/agents", json!({})).unwrap();
    assert_eq!(names["data"][0]["name"], "refunds-bot");
    assert_eq!(
        names["data"][0].as_object().unwrap().len(),
        2,
        "id and name only"
    );
    // An agent cannot use the approver routes, and a malformed approver token is refused.
    let (id, digest) = lab.held("pay_fixture_2045", 45000, "agent-tries");
    assert!(lab.decide(&lab.bot, &id, Some(&digest)).is_err());
    assert!(matches!(
        lab.decide("Bearer approverkey_nothex", &id, Some(&digest)),
        Err(Error::Forbidden)
    ));
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);
}

#[test]
fn raising_an_approvers_limit_outside_the_service_is_detected() {
    let lab = Lab::new();
    let (priya, _) = lab.approver("Priya", "USD", 10000);
    assert!(lab.service.verify_evidence().unwrap().ok());
    rusqlite::Connection::open(lab.directory.path().join("transactions.sqlite"))
        .unwrap()
        .execute(
            "UPDATE approvers SET max_approval_minor=99999999 WHERE id=?1",
            [&priya],
        )
        .unwrap();
    let report = lab.service.verify_evidence().unwrap();
    assert!(!report.state_matches);
    assert!(
        report
            .exceptions
            .iter()
            .any(|e| e.subject == format!("approver {priya}"))
    );
}

#[test]
fn approver_registration_is_validated() {
    let lab = Lab::new();
    for body in [
        json!({"name":"","currency":"USD","max_approval_minor":100}),
        json!({"name":"Priya","currency":"usd","max_approval_minor":100}),
        json!({"name":"Priya","currency":"USD","max_approval_minor":0}),
        json!({"name":"Priya","currency":"USD","max_approval_minor":100,"admin":true}),
    ] {
        assert!(matches!(
            lab.call(&lab.owner, "POST", "/approvers", body),
            Err(Error::Invalid(_))
        ));
    }
}
