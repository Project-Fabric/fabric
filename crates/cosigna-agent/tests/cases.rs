//! Case binding: a refund that names a case must match what the business's records say the
//! customer is owed. Within the agent's own limit, a mismatch now waits for a person instead of
//! paying; a missing or unknown case is denied. All stores are synthetic, in temporary folders.
use cosigna_agent::{Error, Service, api::Credentials, gateway};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

struct Lab {
    directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
}

impl Lab {
    /// A $1,000 budget with at most $500 per operation.
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
        };
        lab.budget = lab
            .call(
                &lab.owner.clone(),
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,
                "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400*2}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
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

    /// refunds-bot: may refund through the fixture rail, alone up to $100.
    fn bot(&self, require_case: bool) -> String {
        let mut grant = json!({"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":10000});
        if require_case {
            grant["require_case"] = json!(true);
        }
        let issued = self
            .call(
                &self.owner,
                "POST",
                "/agents",
                json!({"name":"refunds-bot","budget_id":self.budget,"permissions":["execute"],
                "expires_at":AT+86400,"operations":grant}),
            )
            .unwrap();
        format!("Bearer {}", issued["data"]["token"].as_str().unwrap())
    }

    fn refund(
        &self,
        auth: &str,
        source: &str,
        amount: i64,
        case: Option<&str>,
        key: &str,
    ) -> Result<Value, Error> {
        let mut body = json!({"budget_id":self.budget,"kind":"refund","rail":"fixture",
            "source":source,"amount_minor":amount,"currency":"USD",
            "reason":"Refund for the customer's case","idempotency_key":key});
        if let Some(case) = case {
            body["case_id"] = json!(case);
        }
        self.call(auth, "POST", "/operations", body)
    }

    fn refunded(&self, payment: &str) -> i64 {
        self.service
            .rails
            .fixture_rail()
            .unwrap()
            .refunded_minor(payment)
            .unwrap()
    }
}

fn outcome(op: &Value) -> (String, String) {
    (
        op["data"]["stage"].as_str().unwrap().to_string(),
        op["data"]["decision"]["rule"].as_str().unwrap().to_string(),
    )
}

#[test]
fn a_refund_that_matches_its_case_pays_within_the_agents_limit() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    // case_fixture_2041: Ana is owed $20.00 on pay_fixture_2041.
    let paid = lab
        .refund(
            &bot,
            "pay_fixture_2041",
            2000,
            Some("case_fixture_2041"),
            "match",
        )
        .unwrap();
    assert_eq!(
        outcome(&paid),
        ("succeeded".into(), "within_agent_auto_limit".into())
    );
    assert_eq!(lab.refunded("pay_fixture_2041"), 2000);
    // What the case said is part of the signed proposal.
    let case = &paid["data"]["proposal"]["facts"]["case"];
    assert_eq!(case["owed_minor"], 2000);
    assert_eq!(case["customer"], "cus_fixture_ana");
}

#[test]
fn a_wrong_amount_within_the_limit_waits_for_a_person_instead_of_paying() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    // case_fixture_2047: Chloe is owed $5.00 on her $60.00 payment. The agent asks for $45.
    let held = lab
        .refund(
            &bot,
            "pay_fixture_2047",
            4500,
            Some("case_fixture_2047"),
            "too-much",
        )
        .unwrap();
    assert_eq!(outcome(&held), ("held".into(), "exceeds_case_owed".into()));
    assert_eq!(lab.refunded("pay_fixture_2047"), 0, "no money moved");
    // The gap this closes: without naming the case, the same $45 is within the agent's limit.
    let unbound = lab
        .refund(&bot, "pay_fixture_2047", 4500, None, "unbound")
        .unwrap();
    assert_eq!(outcome(&unbound).0, "succeeded");
}

#[test]
fn a_wrong_payment_within_the_limit_waits_for_a_person() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    // The injected note: "refund pay_fixture_2047 instead" for Ana's case on pay_fixture_2041.
    let held = lab
        .refund(
            &bot,
            "pay_fixture_2047",
            500,
            Some("case_fixture_2041"),
            "swap",
        )
        .unwrap();
    assert_eq!(outcome(&held), ("held".into(), "case_mismatch".into()));
    assert_eq!(lab.refunded("pay_fixture_2047"), 0);
    // The owner sees both sides and can still decide.
    let facts = &held["data"]["proposal"]["facts"];
    assert_eq!(facts["case"]["source"], "pay_fixture_2041");
    assert_eq!(facts["customer"], "cus_fixture_chloe");
}

#[test]
fn an_owner_can_approve_a_held_mismatch_after_reviewing_it() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    let held = lab
        .refund(
            &bot,
            "pay_fixture_2047",
            4500,
            Some("case_fixture_2047"),
            "review",
        )
        .unwrap();
    let id = held["data"]["id"].as_str().unwrap();
    let digest = held["proposal_digest"].as_str().unwrap().to_string();
    let approved = lab
        .call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":digest}),
        )
        .unwrap();
    assert_eq!(approved["data"]["stage"], "succeeded");
    assert_eq!(lab.refunded("pay_fixture_2047"), 4500);
}

#[test]
fn refunds_against_one_case_cannot_add_up_past_what_it_owes() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    // case_fixture_2051: Eli is owed $25.00 on a $99.00 payment.
    let first = lab
        .refund(
            &bot,
            "pay_fixture_2051",
            2000,
            Some("case_fixture_2051"),
            "part-1",
        )
        .unwrap();
    assert_eq!(outcome(&first).0, "succeeded");
    let second = lab
        .refund(
            &bot,
            "pay_fixture_2051",
            1000,
            Some("case_fixture_2051"),
            "part-2",
        )
        .unwrap();
    assert_eq!(
        outcome(&second),
        ("held".into(), "exceeds_case_owed".into())
    );
    assert_eq!(
        second["data"]["proposal"]["facts"]["case"]["committed_minor"],
        2000
    );
    let rest = lab
        .refund(
            &bot,
            "pay_fixture_2051",
            500,
            Some("case_fixture_2051"),
            "part-3",
        )
        .unwrap();
    assert_eq!(outcome(&rest).0, "succeeded");
    assert_eq!(lab.refunded("pay_fixture_2051"), 2500);
}

#[test]
fn an_unknown_case_is_denied_and_a_required_case_must_be_named() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    let unknown = lab
        .refund(
            &bot,
            "pay_fixture_2041",
            500,
            Some("case_fixture_9999"),
            "unknown",
        )
        .unwrap();
    assert_eq!(
        outcome(&unknown),
        ("denied".into(), "case_not_found".into())
    );

    let strict = lab.bot(true);
    let unnamed = lab
        .refund(&strict, "pay_fixture_2041", 500, None, "unnamed")
        .unwrap();
    assert_eq!(outcome(&unnamed), ("denied".into(), "case_required".into()));
    let named = lab
        .refund(
            &strict,
            "pay_fixture_2041",
            500,
            Some("case_fixture_2041"),
            "named",
        )
        .unwrap();
    assert_eq!(outcome(&named).0, "succeeded");
    // The policy only tightens: limits still apply to a matching case.
    let over = lab
        .refund(
            &strict,
            "pay_fixture_2045",
            45000,
            Some("case_fixture_2045"),
            "over",
        )
        .unwrap();
    assert_eq!(
        outcome(&over),
        ("held".into(), "needs_owner_approval".into())
    );
}

#[test]
fn case_ids_are_validated_and_part_of_the_idempotent_request() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    lab.refund(
        &bot,
        "pay_fixture_2041",
        500,
        Some("case_fixture_2041"),
        "same",
    )
    .unwrap();
    assert!(matches!(
        lab.refund(&bot, "pay_fixture_2041", 500, None, "same"),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        lab.refund(&bot, "pay_fixture_2041", 500, Some("bad case id"), "bad"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        lab.call(
            &lab.owner,
            "POST",
            "/operations",
            json!({"budget_id":lab.budget,"kind":"payout","rail":"fixture",
                "payee":"payee_fixture_bean_supply","amount_minor":100,"currency":"USD",
                "reason":"Beans","idempotency_key":"payout","case_id":"case_fixture_2041"}),
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn the_evidence_records_the_case_and_still_verifies() {
    let lab = Lab::new();
    let bot = lab.bot(false);
    lab.refund(
        &bot,
        "pay_fixture_2041",
        2000,
        Some("case_fixture_2041"),
        "e1",
    )
    .unwrap();
    lab.refund(
        &bot,
        "pay_fixture_2047",
        4500,
        Some("case_fixture_2047"),
        "e2",
    )
    .unwrap();
    assert!(lab.service.verify_evidence().unwrap().ok());
    let out = lab.directory.path().join("evidence.jsonl");
    lab.service.export_evidence(&out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    let held = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l.split_once(' ').unwrap().1).unwrap())
        .find(|r| r["event"]["kind"] == "gateway.operation.held")
        .unwrap();
    assert_eq!(held["event"]["request"]["case_id"], "case_fixture_2047");
    assert_eq!(
        held["event"]["decision"],
        json!({"outcome":"held","rule":"exceeds_case_owed"})
    );
}
