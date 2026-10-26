//! Replay must decide exactly as the live gateway does. The same history goes through the real
//! gateway (fixture rail, people approving what is held) and through replay; every decision
//! must match. Then: renewing budgets, labelled mistakes, input errors and the CLI.
use cosigna_agent::{Error, Service, api::Credentials, gateway, replay};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

/// (id, payment, amount, case) in time order; payments and cases are the fixture rail's own.
const HISTORY: [(&str, &str, i64, Option<&str>); 11] = [
    ("r01", "pay_fixture_2041", 2000, Some("case_fixture_2041")), // automatic
    ("r02", "pay_fixture_2047", 4500, Some("case_fixture_2047")), // more than the case owes
    ("r03", "pay_fixture_2045", 45000, Some("case_fixture_2045")), // two people
    ("r04", "pay_fixture_2049", 64000, None),                     // over the per-operation limit
    ("r05", "pay_fixture_2049", 30000, None),                     // above the agent's limit
    ("r06", "pay_fixture_2041", 500, None),                       // nothing left to refund
    ("r07", "pay_fixture_2051", 2000, Some("case_fixture_2051")), // automatic
    ("r08", "pay_fixture_2051", 1000, Some("case_fixture_2051")), // case nearly settled
    ("r09", "pay_fixture_2047", 500, Some("case_fixture_2041")),  // wrong payment for the case
    ("r10", "pay_fixture_2049", 9000, None),                      // automatic
    ("r11", "pay_fixture_2049", 20000, None),                     // budget used up
];
const PAYMENTS: [(&str, i64, &str); 5] = [
    ("pay_fixture_2041", 2000, "cus_fixture_ana"),
    ("pay_fixture_2045", 45000, "cus_fixture_ben"),
    ("pay_fixture_2047", 6000, "cus_fixture_chloe"),
    ("pay_fixture_2049", 64000, "cus_fixture_dev"),
    ("pay_fixture_2051", 9900, "cus_fixture_eli"),
];
const CASES: &str = "id,customer,source,currency,owed_minor
case_fixture_2041,cus_fixture_ana,pay_fixture_2041,USD,2000
case_fixture_2045,cus_fixture_ben,pay_fixture_2045,USD,45000
case_fixture_2047,cus_fixture_chloe,pay_fixture_2047,USD,500
case_fixture_2051,cus_fixture_eli,pay_fixture_2051,USD,2500
";

fn policy() -> replay::Policy {
    serde_json::from_value(json!({
        "budget": {"currency":"USD","limit_minor":100000,"per_operation_minor":50000,
            "second_approval_above_minor":40000},
        "grant": {"kinds":["refund"],"auto_approve_minor":10000}
    }))
    .unwrap()
}

fn history_csv(mistakes: &[&str]) -> String {
    let mut csv = String::from(
        "id,at,kind,source,amount_minor,currency,customer,case_id,payment_amount_minor,mistake\n",
    );
    for (i, (id, source, amount, case)) in HISTORY.iter().enumerate() {
        let (_, paid, customer) = PAYMENTS.iter().find(|p| p.0 == *source).unwrap();
        csv += &format!(
            "{id},{},refund,{source},{amount},USD,{customer},{},{paid},{}\n",
            AT + i as i64 * 3600,
            case.unwrap_or(""),
            if mistakes.contains(id) { "yes" } else { "" }
        );
    }
    csv
}

/// The same history through the real gateway: (outcome, rule) per operation.
fn live() -> Vec<(String, String)> {
    let directory = tempfile::tempdir().unwrap();
    let service = Service::open(directory.path()).unwrap();
    let credentials = Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
    let private: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("credentials.json")).unwrap())
            .unwrap();
    let owner = format!("Bearer {}", private["owner"].as_str().unwrap());
    let call = |auth: &str, method: &str, path: &str, body: Value| -> Result<Value, Error> {
        gateway::dispatch(
            &service,
            &cosigna_agent::planner::FixturePlanner,
            &credentials,
            auth,
            method,
            &format!("/gateway/v1{path}"),
            &serde_json::to_vec(&body).unwrap(),
            AT,
        )
    };
    let budget = call(
        &owner,
        "POST",
        "/budgets",
        json!({"currency":"USD","limit_minor":100000,
        "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400*30,
        "second_approval_above_minor":40000}),
    )
    .unwrap()["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let bot = format!(
        "Bearer {}",
        call(
            &owner,
            "POST",
            "/agents",
            json!({"name":"refunds-bot","budget_id":budget,
            "permissions":["execute"],"expires_at":AT+86400*30,
            "operations":{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":10000}})
        )
        .unwrap()["data"]["token"]
            .as_str()
            .unwrap()
    );
    let priya = format!(
        "Bearer {}",
        call(
            &owner,
            "POST",
            "/approvers",
            json!({"name":"Priya","currency":"USD","max_approval_minor":50000})
        )
        .unwrap()["data"]["token"]
            .as_str()
            .unwrap()
    );
    HISTORY
        .iter()
        .map(|(id, source, amount, case)| {
            let mut body = json!({"budget_id":budget,"kind":"refund","rail":"fixture","source":source,
                "amount_minor":amount,"currency":"USD","reason":"History","idempotency_key":id});
            if let Some(case) = case {
                body["case_id"] = json!(case);
            }
            let op = call(&bot, "POST", "/operations", body).unwrap();
            // What was held was approved by people when it happened (the replay assumption).
            if op["data"]["stage"] == "held" {
                let path = format!("/operations/{}/approve", op["data"]["id"].as_str().unwrap());
                let digest = json!({"proposal_digest": op["proposal_digest"]});
                if op["data"]["decision"]["rule"] == "needs_two_approvals" {
                    call(&priya, "POST", &path, digest.clone()).unwrap();
                }
                assert_eq!(call(&owner, "POST", &path, digest).unwrap()["data"]["stage"], "succeeded");
            }
            let outcome = match op["data"]["decision"]["outcome"].as_str().unwrap() {
                "auto_approved" => "automatic",
                "held" => "held_for_a_person",
                other => other,
            };
            (outcome.to_string(), op["data"]["decision"]["rule"].as_str().unwrap().to_string())
        })
        .collect()
}

#[test]
fn replay_decides_every_operation_exactly_as_the_live_gateway() {
    let live = live();
    let report = replay::replay(
        policy(),
        replay::read_history(&history_csv(&[])).unwrap(),
        &replay::read_cases(CASES).unwrap(),
    )
    .unwrap();
    let replayed: Vec<(String, String)> = report
        .replayed
        .iter()
        .map(|r| (r.outcome.to_string(), r.rule.clone()))
        .collect();
    assert_eq!(replayed, live);
    // Every route is exercised, so the agreement means something.
    let rules: Vec<&str> = live.iter().map(|(_, rule)| rule.as_str()).collect();
    for rule in [
        "within_agent_auto_limit",
        "exceeds_case_owed",
        "needs_two_approvals",
        "over_per_operation_limit",
        "needs_owner_approval",
        "exceeds_refundable",
        "case_mismatch",
        "budget_exhausted",
    ] {
        assert!(rules.contains(&rule), "{rule} not exercised: {rules:?}");
    }
    assert_eq!(report.automatic.count, 3);
    assert_eq!(report.automatic.amount_minor, 2000 + 2000 + 9000);
    assert_eq!(report.held_for_a_person.count, 5);
    assert_eq!(report.denied.count, 3);
}

#[test]
fn known_mistakes_are_reported_as_stopped_or_still_paid() {
    // r02 (wrong amount) and r09 (wrong payment) were mistakes; so was r10, which the policy
    // would still have paid automatically: the report says so instead of hiding it.
    let report = replay::replay(
        policy(),
        replay::read_history(&history_csv(&["r02", "r09", "r10"])).unwrap(),
        &replay::read_cases(CASES).unwrap(),
    )
    .unwrap();
    let mistakes = report.mistakes.as_ref().unwrap();
    assert_eq!(mistakes.labelled, 3);
    assert_eq!(mistakes.stopped, 2);
    assert_eq!(mistakes.stopped_amount_minor, 4500 + 500);
    assert_eq!(mistakes.paid_automatically, vec!["r10".to_string()]);
    let text = replay::summary(&report);
    assert!(text.contains("Known mistakes: 2 of 3"), "{text}");
    assert!(text.contains("Still paid automatically: r10"), "{text}");
}

#[test]
fn a_renewing_budget_resets_each_period() {
    let csv = "id,at,kind,source,amount_minor,currency\n\
        a,2026-09-01T09:00:00Z,refund,p1,6000,USD\n\
        b,2026-09-02T09:00:00Z,refund,p2,6000,USD\n\
        c,2026-09-09T09:00:00Z,refund,p3,6000,USD\n";
    let mut policy: replay::Policy = serde_json::from_value(json!({
        "budget": {"currency":"USD","limit_minor":10000,"per_operation_minor":10000},
        "grant": {"kinds":["refund"],"auto_approve_minor":10000}
    }))
    .unwrap();
    let rules = |policy: replay::Policy| -> Vec<String> {
        replay::replay(policy, replay::read_history(csv).unwrap(), &[])
            .unwrap()
            .replayed
            .into_iter()
            .map(|r| r.rule)
            .collect()
    };
    // One budget for the whole history: the second and third exhaust it.
    assert_eq!(
        rules(policy.clone()),
        [
            "within_agent_auto_limit",
            "budget_exhausted",
            "budget_exhausted"
        ]
    );
    // A fresh budget every week: the third falls in a new week.
    policy.budget.renew_every_days = Some(7);
    assert_eq!(
        rules(policy),
        [
            "within_agent_auto_limit",
            "budget_exhausted",
            "within_agent_auto_limit"
        ]
    );
}

#[test]
fn bad_input_names_the_row_and_the_problem() {
    let header = "id,at,kind,source,amount_minor,currency\n";
    for (row, expected) in [
        ("a,yesterday,refund,p,100,USD", "row 2: invalid at"),
        ("a,1800000000,refund,p,0,USD", "row 2: invalid amount_minor"),
        (
            "a,1800000000,refund,,100,USD",
            "row 2: a refund needs `source`",
        ),
        (
            "a,1800000000,chargeback,p,100,USD",
            "row 2: kind must be refund or payout",
        ),
        ("a,1800000000,refund,p,100,usd", "row 2: currency must be"),
    ] {
        match replay::read_history(&format!("{header}{row}\n")) {
            Err(Error::Invalid(message)) => assert!(message.contains(expected), "{message}"),
            other => panic!("{row}: {other:?}"),
        }
    }
    assert!(matches!(
        replay::read_history("id,at,kind\n"),
        Err(Error::Invalid(m)) if m.contains("`amount_minor`")
    ));
    let invalid: Result<replay::Policy, _> = serde_json::from_value(json!({
        "budget": {"currency":"USD","limit_minor":1,"per_operation_minor":1},
        "grant": {"kinds":["refund"],"auto_approve_minor":1,"admin":true}
    }));
    assert!(invalid.is_err(), "unknown policy fields are refused");
}

#[test]
fn the_cli_prints_a_summary_and_a_versioned_report() {
    let directory = tempfile::tempdir().unwrap();
    let history = directory.path().join("history.csv");
    let cases = directory.path().join("cases.csv");
    let policy_file = directory.path().join("policy.json");
    std::fs::write(&history, history_csv(&["r02"])).unwrap();
    std::fs::write(&cases, CASES).unwrap();
    std::fs::write(&policy_file, serde_json::to_vec(&policy()).unwrap()).unwrap();
    let run = |extra: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
            .arg("replay")
            .arg("--history")
            .arg(&history)
            .arg("--policy")
            .arg(&policy_file)
            .arg("--cases")
            .arg(&cases)
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let text = run(&["--text"]);
    assert!(text.contains("Replayed 11 operations"), "{text}");
    assert!(text.contains("Known mistakes: 1 of 1"), "{text}");
    let report: Value = serde_json::from_str(&run(&[])).unwrap();
    assert_eq!(report["schema"], "cosigna.replay/v1");
    assert_eq!(report["data"]["replayed"].as_array().unwrap().len(), 11);
}

#[test]
fn the_shipped_example_tells_the_story_its_readme_describes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/replay");
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    let report = replay::replay(
        serde_json::from_str(&read("policy.json")).unwrap(),
        replay::read_history(&read("refunds-90-days.csv")).unwrap(),
        &replay::read_cases(&read("cases.csv")).unwrap(),
    )
    .unwrap();
    assert_eq!(report.operations, 122);
    assert_eq!(
        (
            report.automatic.count,
            report.held_for_a_person.count,
            report.denied.count
        ),
        (91, 28, 3)
    );
    let mistakes = report.mistakes.unwrap();
    assert_eq!((mistakes.labelled, mistakes.stopped), (6, 5));
    assert_eq!(mistakes.paid_automatically, vec!["rf_097".to_string()]);
}
