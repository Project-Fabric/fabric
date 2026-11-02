//! Two-person approval: above a budget's threshold, nothing is approved automatically and a
//! payment needs two different people, each within their own limit. Synthetic stores only.
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
    /// $1,000 budget, $500 per operation, two people above $200; the agent may pay up to $500
    /// alone, so the threshold, not the agent's limit, is what holds larger refunds.
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
                    "merchants":["fixture-cafe"],"expires_at":AT+86400*2,
                    "second_approval_above_minor":20000}),
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
                    "operations":{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":50000}}),
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

    fn approver(&self, name: &str, max: i64) -> (String, String) {
        let issued = self
            .call(
                &self.owner,
                "POST",
                "/approvers",
                json!({"name":name,"currency":"USD","max_approval_minor":max}),
            )
            .unwrap();
        (
            issued["data"]["approver"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
            format!("Bearer {}", issued["data"]["token"].as_str().unwrap()),
        )
    }

    fn refund(&self, source: &str, amount: i64, key: &str) -> Value {
        self.call(
            &self.bot,
            "POST",
            "/operations",
            json!({"budget_id":self.budget,"kind":"refund","rail":"fixture","source":source,
                "amount_minor":amount,"currency":"USD","reason":"Refund","idempotency_key":key}),
        )
        .unwrap()
    }

    fn approve(&self, auth: &str, op: &Value) -> Result<Value, Error> {
        let id = op["data"]["id"].as_str().unwrap();
        self.call(
            auth,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":op["proposal_digest"]}),
        )
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
fn above_the_threshold_an_agent_cannot_pay_alone_even_within_its_own_limit() {
    let lab = Lab::new();
    let small = lab.refund("pay_fixture_2041", 2000, "small");
    assert_eq!(small["data"]["stage"], "succeeded");
    let large = lab.refund("pay_fixture_2045", 45000, "large");
    assert_eq!(large["data"]["stage"], "held");
    assert_eq!(large["data"]["decision"]["rule"], "needs_two_approvals");
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);
    // The exposure report's largest automatic payment is capped at the threshold.
    let exposure = lab.call(&lab.owner, "GET", "/exposure", json!({})).unwrap();
    assert_eq!(exposure["data"]["agents"][0]["largest_single_minor"], 20000);
}

#[test]
fn two_different_people_must_approve_the_same_payment() {
    let lab = Lab::new();
    let (priya, priya_token) = lab.approver("Priya", 50000);
    let held = lab.refund("pay_fixture_2045", 45000, "two");

    // First approval: recorded, still held, nothing paid.
    let first = lab.approve(&priya_token, &held).unwrap();
    assert_eq!(first["data"]["stage"], "held");
    assert_eq!(first["needs_owner"], true);
    assert_eq!(first["data"]["first_approval"]["approver"], priya);
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);

    // The same person cannot give the second approval.
    let again = lab.approve(&priya_token, &held);
    assert!(
        matches!(again, Err(Error::Conflict(ref m)) if m.starts_with("second_approver_required")),
        "{again:?}"
    );
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);

    // A different person completes it; it is paid once.
    let paid = lab.approve(&lab.owner, &held).unwrap();
    assert_eq!(paid["data"]["stage"], "succeeded");
    assert_eq!(paid["data"]["approved_by"], "owner");
    assert_eq!(paid["data"]["first_approval"]["approver"], priya);
    assert_eq!(lab.refunded("pay_fixture_2045"), 45000);
    // A repeat of the completing approval returns the result and pays nothing more.
    assert_eq!(
        lab.approve(&lab.owner, &held).unwrap()["data"]["stage"],
        "succeeded"
    );
    assert_eq!(lab.refunded("pay_fixture_2045"), 45000);

    let events = lab.events();
    let first_record = events
        .iter()
        .find(|e| e["kind"] == "gateway.operation.first_approved")
        .unwrap();
    assert_eq!(first_record["by"]["approver"], priya);
    let second_record = events
        .iter()
        .find(|e| e["kind"] == "gateway.operation.owner_approved")
        .unwrap();
    assert_eq!(second_record["first_approval"]["approver"], priya);
    // Operations that need one person keep their earlier record shape.
    assert!(
        events
            .iter()
            .filter(|e| e["kind"] == "gateway.operation.held")
            .all(|e| e.get("first_approval").is_none())
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn each_person_must_be_within_their_own_limit_and_the_owner_counts_once() {
    let lab = Lab::new();
    let (_, sam) = lab.approver("Sam", 10000);
    let (raj, raj_token) = lab.approver("Raj", 50000);
    let held = lab.refund("pay_fixture_2045", 45000, "limits");
    // Sam is below the amount, as a first or a second approver.
    assert!(
        matches!(lab.approve(&sam, &held), Err(Error::Conflict(ref m)) if m.starts_with("above_approver_limit"))
    );
    lab.approve(&lab.owner, &held).unwrap();
    assert!(
        matches!(lab.approve(&sam, &held), Err(Error::Conflict(ref m)) if m.starts_with("above_approver_limit"))
    );
    // The owner cannot be both people.
    assert!(
        matches!(lab.approve(&lab.owner, &held), Err(Error::Conflict(ref m)) if m.starts_with("second_approver_required"))
    );
    let paid = lab.approve(&raj_token, &held).unwrap();
    assert_eq!(paid["data"]["stage"], "succeeded");
    assert_eq!(paid["data"]["approver"], raj);
    assert_eq!(paid["data"]["first_approval"]["by"], "owner");
}

#[test]
fn either_person_can_still_stop_it_after_the_first_approval() {
    let lab = Lab::new();
    let (_, priya) = lab.approver("Priya", 50000);
    let held = lab.refund("pay_fixture_2045", 45000, "stop");
    lab.approve(&priya, &held).unwrap();
    let id = held["data"]["id"].as_str().unwrap();
    let rejected = lab
        .call(
            &priya,
            "POST",
            &format!("/operations/{id}/reject"),
            json!({}),
        )
        .unwrap();
    assert_eq!(rejected["data"]["stage"], "rejected");
    assert!(lab.approve(&lab.owner, &held).is_err());
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);
}

#[test]
fn the_threshold_is_validated() {
    let lab = Lab::new();
    assert!(matches!(
        lab.call(
            &lab.owner,
            "POST",
            "/budgets",
            json!({"currency":"USD","limit_minor":1000,"per_purchase_minor":1000,
                "merchants":["fixture-cafe"],"expires_at":AT+60,"second_approval_above_minor":-1}),
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn a_first_approval_stops_counting_once_its_approver_is_revoked() {
    let lab = Lab::new();
    let (priya, priya_token) = lab.approver("Priya", 50000);
    let held = lab.refund("pay_fixture_2045", 45000, "revoked-first");
    lab.approve(&priya_token, &held).unwrap();
    lab.call(
        &lab.owner,
        "POST",
        &format!("/approvers/{priya}/revoke"),
        json!({}),
    )
    .unwrap();
    // The owner's approval alone is not two people once Priya's approval no longer counts.
    let second = lab.approve(&lab.owner, &held);
    assert!(
        matches!(second, Err(Error::Conflict(ref m)) if m.starts_with("first_approval_no_longer_authorized")),
        "{second:?}"
    );
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);
}
