//! Worst-case exposure: the most an owner's agents can move without a person. The report must
//! be an upper bound no sequence of agent requests can beat, and it should be reachable when
//! the rail has the money. All stores are synthetic, in temporary folders.
use cosigna_agent::{Error, Service, api::Credentials, gateway};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

struct Lab {
    _directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
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
        Self {
            _directory: directory,
            service,
            credentials,
            owner,
        }
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

    fn budget(&self, limit: i64, per_operation: i64) -> String {
        self.call(
            &self.owner,
            "POST",
            "/budgets",
            json!({"currency":"USD","limit_minor":limit,"per_purchase_minor":per_operation,
                "merchants":["fixture-cafe"],"expires_at":AT+86400*2}),
        )
        .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Registers an agent and returns (id, bearer).
    fn agent(&self, name: &str, budget: &str, grant: Value) -> (String, String) {
        let issued = self
            .call(
                &self.owner,
                "POST",
                "/agents",
                json!({"name":name,"budget_id":budget,"permissions":["execute"],
                    "expires_at":AT+86400,"operations":grant}),
            )
            .unwrap();
        (
            issued["data"]["agent"]["id"].as_str().unwrap().to_string(),
            format!("Bearer {}", issued["data"]["token"].as_str().unwrap()),
        )
    }

    fn exposure(&self) -> Value {
        let report = self
            .call(&self.owner, "GET", "/exposure", json!({}))
            .unwrap();
        assert_eq!(report["schema"], "cosigna.exposure/v1");
        report["data"].clone()
    }

    /// Money held or spent against a budget so far.
    fn used(&self, budget: &str) -> i64 {
        let balance = self
            .call(&self.owner, "GET", &format!("/budgets/{budget}"), json!({}))
            .unwrap();
        balance["data"]["reserved_minor"].as_i64().unwrap()
            + balance["data"]["spent_minor"].as_i64().unwrap()
    }
}

fn usd(report: &Value) -> &Value {
    report["currencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["currency"] == "USD")
        .unwrap()
}

fn find<'a>(report: &'a Value, list: &str, key: &str, id: &str) -> &'a Value {
    report[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e[key] == id)
        .unwrap()
}

const REFUNDS: &str = r#"{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":10000}"#;

fn grant(auto: i64, kinds: &[&str], require_case: bool) -> Value {
    let mut grant: Value = serde_json::from_str(REFUNDS).unwrap();
    grant["auto_approve_minor"] = json!(auto);
    grant["kinds"] = json!(kinds);
    if require_case {
        grant["require_case"] = json!(true);
    }
    grant
}

#[test]
fn the_report_names_the_bound_and_what_sets_it() {
    let lab = Lab::new();
    let budget = lab.budget(100_000, 50_000);
    let (bot, _) = lab.agent("refunds-bot", &budget, grant(10_000, &["refund"], false));
    let report = lab.exposure();
    // The budget is the only limit: $1,000 can move without anyone signing.
    assert_eq!(usd(&report)["without_a_person_minor"], 100_000);
    let agent = find(&report, "agents", "agent_id", &bot);
    assert_eq!(agent["limited_by"], "budget");
    assert_eq!(agent["largest_single_minor"], 10_000);
    assert_eq!(agent["until"], AT + 86400);
    // Requiring a case would cap it at what the open cases owe ($20 + $450 + $5 + $25).
    assert_eq!(usd(&report)["open_cases_minor"], 50_000);
    assert_eq!(agent["if_case_required_minor"], 50_000);
    assert_eq!(report["notes"].as_array().unwrap().len(), 3);
}

#[test]
fn agents_that_cannot_pay_alone_add_nothing() {
    let lab = Lab::new();
    let budget = lab.budget(100_000, 50_000);
    let (none, _) = lab.agent("held-only", &budget, grant(0, &["refund"], false));
    let (revoked, _) = lab.agent("revoked", &budget, grant(10_000, &["payout"], false));
    lab.call(
        &lab.owner,
        "POST",
        &format!("/agents/{revoked}/revoke"),
        json!({}),
    )
    .unwrap();
    let report = lab.exposure();
    assert_eq!(usd(&report)["without_a_person_minor"], 0);
    assert_eq!(
        find(&report, "agents", "agent_id", &none)["limited_by"],
        "no_automatic_room"
    );
    assert_eq!(
        find(&report, "agents", "agent_id", &revoked)["limited_by"],
        "agent_inactive"
    );
    assert_eq!(
        find(&report, "budgets", "budget_id", &budget)["limited_by"],
        "no_automatic_room"
    );
    // A revoked budget closes every agent on it.
    let other = lab.budget(50_000, 50_000);
    let (open, _) = lab.agent("payer", &other, grant(5_000, &["payout"], false));
    assert_eq!(usd(&lab.exposure())["without_a_person_minor"], 50_000);
    lab.call(
        &lab.owner,
        "POST",
        &format!("/budgets/{other}/revoke"),
        json!({}),
    )
    .unwrap();
    let report = lab.exposure();
    assert_eq!(usd(&report)["without_a_person_minor"], 0);
    assert_eq!(
        find(&report, "agents", "agent_id", &open)["limited_by"],
        "budget_inactive"
    );
}

#[test]
fn case_bound_agents_share_one_case_book_across_budgets() {
    let lab = Lab::new();
    let a = lab.budget(80_000, 50_000);
    let b = lab.budget(60_000, 50_000);
    lab.agent("cases-a", &a, grant(50_000, &["refund"], true));
    lab.agent("cases-b", &b, grant(3_000, &["refund"], true));
    let report = lab.exposure();
    // Each budget alone is capped at the $500 the cases owe, and both together still are.
    assert_eq!(
        find(&report, "budgets", "budget_id", &a)["exposure_minor"],
        50_000
    );
    assert_eq!(
        find(&report, "budgets", "budget_id", &a)["limited_by"],
        "cases"
    );
    assert_eq!(
        find(&report, "budgets", "budget_id", &b)["exposure_minor"],
        50_000
    );
    assert_eq!(usd(&report)["without_a_person_minor"], 50_000);
    // An agent that may also pay out is not bound by cases, whatever its grant says.
    let (mixed, _) = lab.agent("mixed", &b, grant(3_000, &["refund", "payout"], true));
    let report = lab.exposure();
    assert_eq!(
        find(&report, "agents", "agent_id", &mixed)["must_name_case"],
        false
    );
    assert_eq!(usd(&report)["without_a_person_minor"], 60_000 + 50_000);
}

#[test]
fn a_refund_against_a_case_lowers_what_is_left() {
    let lab = Lab::new();
    let budget = lab.budget(80_000, 50_000);
    let (_, bot) = lab.agent("cases", &budget, grant(50_000, &["refund"], true));
    lab.call(
        &bot,
        "POST",
        "/operations",
        json!({"budget_id":budget,"kind":"refund","rail":"fixture","source":"pay_fixture_2051",
            "amount_minor":2000,"currency":"USD","reason":"Partial refund","idempotency_key":"k1",
            "case_id":"case_fixture_2051"}),
    )
    .unwrap();
    let report = lab.exposure();
    assert_eq!(usd(&report)["open_cases_minor"], 48_000);
    assert_eq!(usd(&report)["without_a_person_minor"], 48_000);
}

#[test]
fn agents_cannot_read_the_owners_exposure() {
    let lab = Lab::new();
    let budget = lab.budget(10_000, 10_000);
    let (_, bot) = lab.agent("refunds-bot", &budget, grant(1_000, &["refund"], false));
    assert!(matches!(
        lab.call(&bot, "GET", "/exposure", json!({})),
        Err(Error::NotFound)
    ));
}

#[test]
fn a_greedy_agent_can_reach_the_case_bound_but_not_pass_it() {
    let lab = Lab::new();
    let budget = lab.budget(100_000, 50_000);
    let (_, bot) = lab.agent("cases", &budget, grant(50_000, &["refund"], true));
    let bound = usd(&lab.exposure())["without_a_person_minor"]
        .as_i64()
        .unwrap();
    for (case, source, owed) in [
        ("case_fixture_2041", "pay_fixture_2041", 2000),
        ("case_fixture_2045", "pay_fixture_2045", 45000),
        ("case_fixture_2047", "pay_fixture_2047", 500),
        ("case_fixture_2051", "pay_fixture_2051", 2500),
    ] {
        // Ask for one cent more first: it must wait for a person.
        let over = lab
            .call(
                &bot,
                "POST",
                "/operations",
                json!({"budget_id":budget,"kind":"refund",
                "rail":"fixture","source":source,"amount_minor":owed+1,"currency":"USD",
                "reason":"Refund","idempotency_key":format!("{case}-over"),"case_id":case}),
            )
            .unwrap();
        assert_ne!(over["data"]["stage"], "succeeded");
        let paid = lab
            .call(
                &bot,
                "POST",
                "/operations",
                json!({"budget_id":budget,"kind":"refund",
                "rail":"fixture","source":source,"amount_minor":owed,"currency":"USD",
                "reason":"Refund","idempotency_key":case,"case_id":case}),
            )
            .unwrap();
        assert_eq!(paid["data"]["stage"], "succeeded");
    }
    assert_eq!(lab.used(&budget), bound, "the bound is reached exactly");
    assert_eq!(usd(&lab.exposure())["without_a_person_minor"], 0);
}

/// xorshift64*: deterministic, so a failure reproduces.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() % items.len() as u64) as usize]
    }
}

#[test]
fn no_sequence_of_agent_requests_moves_more_than_the_bound() {
    let lab = Lab::new();
    let free = lab.budget(30_000, 20_000);
    let case_a = lab.budget(80_000, 50_000);
    let case_b = lab.budget(60_000, 50_000);
    let closed = lab.budget(100_000, 100_000);
    // (bearer, its own budget)
    let mut bots = vec![
        (
            lab.agent("refunds", &free, grant(10_000, &["refund"], false))
                .1,
            &free,
        ),
        (
            lab.agent("payouts", &free, grant(5_000, &["payout"], false))
                .1,
            &free,
        ),
        (
            lab.agent("cases-a", &case_a, grant(50_000, &["refund"], true))
                .1,
            &case_a,
        ),
        (
            lab.agent("cases-b", &case_b, grant(3_000, &["refund"], true))
                .1,
            &case_b,
        ),
        (
            lab.agent("held-only", &closed, grant(0, &["refund", "payout"], false))
                .1,
            &closed,
        ),
    ];
    let (revoked, token) = lab.agent("revoked", &closed, grant(100_000, &["payout"], false));
    lab.call(
        &lab.owner,
        "POST",
        &format!("/agents/{revoked}/revoke"),
        json!({}),
    )
    .unwrap();
    bots.push((token, &closed));
    let budgets = [&free, &case_a, &case_b, &closed];
    let report = lab.exposure();
    let bound = usd(&report)["without_a_person_minor"].as_i64().unwrap();
    assert_eq!(bound, 30_000 + 50_000);

    let sources = [
        "pay_fixture_2041",
        "pay_fixture_2045",
        "pay_fixture_2047",
        "pay_fixture_2049",
        "pay_fixture_2051",
        "pay_fixture_3001",
        "pay_fixture_none",
    ];
    let cases = [
        None,
        Some("case_fixture_2041"),
        Some("case_fixture_2045"),
        Some("case_fixture_2047"),
        Some("case_fixture_2051"),
        Some("case_fixture_9999"),
    ];
    // (case, its payment, what it owes): the requests a capable attacker would aim with.
    let owed = [
        ("case_fixture_2041", "pay_fixture_2041", 2000),
        ("case_fixture_2045", "pay_fixture_2045", 45000),
        ("case_fixture_2047", "pay_fixture_2047", 500),
        ("case_fixture_2051", "pay_fixture_2051", 2500),
    ];
    let payees = [
        "payee_fixture_bean_supply",
        "payee_fixture_cup_co",
        "payee_fixture_chai_wholesale",
    ];
    let amounts = [
        1, 499, 500, 2000, 2500, 3000, 5000, 10_000, 20_000, 45_000, 50_001,
    ];
    let mut random = Random(0x00C0_516A_2026_1025);
    let mut moved_checks = 0;
    for i in 0..600 {
        let (bot, own) = random.pick(&bots);
        let mut body = json!({"rail":"fixture","currency":"USD","reason":"Try",
            "idempotency_key":format!("try-{i}")});
        if random.next() % 5 < 3 {
            // Aimed: its own budget and requests that fit its grant and the rail's records.
            body["budget_id"] = json!(own);
            if *own == &free && random.next().is_multiple_of(2) {
                body["kind"] = json!("payout");
                body["payee"] = json!(random.pick(&payees[..2]));
                body["amount_minor"] = json!((random.next() % 6_000) as i64 + 1);
            } else if *own == &free {
                body["kind"] = json!("refund");
                body["source"] = json!("pay_fixture_2049");
                body["amount_minor"] = json!((random.next() % 12_000) as i64 + 1);
            } else {
                let (case, source, owes) = *random.pick(&owed);
                body["kind"] = json!("refund");
                body["source"] = json!(source);
                body["case_id"] = json!(case);
                body["amount_minor"] = json!((random.next() % (owes as u64 + 100)) as i64 + 1);
            }
        } else {
            body["budget_id"] = json!(random.pick(&budgets));
            let mut amount = *random.pick(&amounts);
            if random.next().is_multiple_of(4) {
                amount = (random.next() % 50_000) as i64 + 1;
            }
            body["amount_minor"] = json!(amount);
            if random.next().is_multiple_of(3) {
                body["kind"] = json!("payout");
                body["payee"] = json!(random.pick(&payees));
            } else {
                body["kind"] = json!("refund");
                body["source"] = json!(random.pick(&sources));
                if let Some(case) = random.pick(&cases) {
                    body["case_id"] = json!(case);
                }
            }
        }
        // Errors (wrong budget, unknown case book entries) and denials move nothing.
        let _ = lab.call(bot, "POST", "/operations", body);
        if i % 50 == 49 {
            let moved: i64 = budgets.iter().map(|b| lab.used(b)).sum();
            assert!(moved <= bound, "moved {moved} past the bound {bound}");
            // What is left plus what has moved never exceeds the original bound.
            let left = usd(&lab.exposure())["without_a_person_minor"]
                .as_i64()
                .unwrap();
            assert!(moved + left <= bound, "{moved} + {left} > {bound}");
            moved_checks += 1;
        }
    }
    assert_eq!(moved_checks, 12);
    let moved: i64 = budgets.iter().map(|b| lab.used(b)).sum();
    // The attacker gets close, so the bound is tested where it matters, not far below it.
    assert!(moved >= 70_000, "the run moved only {moved}");
    assert_eq!(lab.used(&closed), 0);
    assert!(lab.used(&case_a) + lab.used(&case_b) <= 50_000);
}
