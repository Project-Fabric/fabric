//! The operator timeline: one payment's story, read only from its evidence records.
//! Synthetic stores in temporary folders.
use cosigna_agent::{Error, Service, api::Credentials, gateway, provider::Scenario};
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
                json!({"currency":"USD","limit_minor":100000,
                "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400*2,
                "second_approval_above_minor":40000}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        lab.bot = format!(
            "Bearer {}",
            lab.call(
                &lab.owner,
                "POST",
                "/agents",
                json!({"name":"refunds-bot","budget_id":lab.budget,
                "permissions":["execute"],"expires_at":AT+86400,
                "operations":{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":1000}})
            )
            .unwrap()["data"]["token"]
                .as_str()
                .unwrap()
        );
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

    fn refund(&self, source: &str, amount: i64, key: &str, case: Option<&str>) -> Value {
        let mut body = json!({"budget_id":self.budget,"kind":"refund","rail":"fixture",
            "source":source,"amount_minor":amount,"currency":"USD","reason":"Ticket","idempotency_key":key});
        if let Some(case) = case {
            body["case_id"] = json!(case);
        }
        self.call(&self.bot, "POST", "/operations", body).unwrap()
    }

    fn timeline(&self, auth: &str, id: &str) -> Result<Value, Error> {
        self.call(
            auth,
            "GET",
            &format!("/operations/{id}/timeline"),
            json!({}),
        )
        .map(|v| {
            assert_eq!(v["schema"], "cosigna.timeline/v1");
            v["data"].clone()
        })
    }
}

fn steps(timeline: &Value) -> Vec<String> {
    timeline["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["step"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_two_person_refund_reads_as_a_story_with_names_and_evidence() {
    let lab = Lab::new();
    let priya = lab
        .call(
            &lab.owner,
            "POST",
            "/approvers",
            json!({"name":"Priya","currency":"USD","max_approval_minor":50000}),
        )
        .unwrap();
    let priya_token = format!("Bearer {}", priya["data"]["token"].as_str().unwrap());
    let held = lab.refund("pay_fixture_2045", 45000, "big", Some("case_fixture_2045"));
    let id = held["data"]["id"].as_str().unwrap().to_string();
    let approve = |auth: &str| {
        lab.call(
            auth,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":held["proposal_digest"]}),
        )
        .unwrap()
    };
    // While waiting: the approver can read the story and sees what happens next.
    approve(&priya_token);
    let waiting = lab.timeline(&priya_token, &id).unwrap();
    assert_eq!(waiting["now"], "Waiting for a second person to approve.");
    assert_eq!(waiting["next"].as_array().unwrap().len(), 1);
    approve(&lab.owner);

    let story = lab.timeline(&lab.owner, &id).unwrap();
    assert_eq!(
        steps(&story),
        [
            "held",
            "first_approved",
            "owner_approved",
            "submission_started",
            "rail_result"
        ]
    );
    let entries = story["entries"].as_array().unwrap();
    assert_eq!(entries[0]["who"]["name"], "refunds-bot");
    assert!(entries[0]["summary"].as_str().unwrap().contains(
        "refunds-bot asked for a refund of USD 450.00 on pay_fixture_2045 for case case_fixture_2045. Held for a person: above the two-person limit"
    ));
    assert_eq!(entries[1]["who"]["name"], "Priya");
    assert!(
        entries[1]["summary"]
            .as_str()
            .unwrap()
            .starts_with("First approval by Priya")
    );
    assert!(
        entries[2]["summary"]
            .as_str()
            .unwrap()
            .starts_with("Approved by the owner")
    );
    assert!(
        entries[4]["summary"]
            .as_str()
            .unwrap()
            .starts_with("The payment system confirmed it")
    );
    assert_eq!(story["now"], "Paid.");
    assert!(story["next"].as_array().unwrap().is_empty());
    assert_eq!(story["evidence_ok"], true);

    // Each step cites a real record: its sequence and hash are in the exported chain.
    let out = lab.directory.path().join("evidence.jsonl");
    lab.service.export_evidence(&out).unwrap();
    let chain = std::fs::read_to_string(&out).unwrap();
    for entry in entries {
        let hash = entry["hash"].as_str().unwrap();
        let line = chain
            .lines()
            .find(|l| l.starts_with(hash))
            .expect("cited record exists");
        assert!(line.contains(&format!("\"seq\":{}", entry["seq"])));
    }
}

#[test]
fn a_lost_answer_shows_what_is_unknown_and_how_it_was_settled() {
    let lab = Lab::new();
    let held = lab.refund("pay_fixture_2049", 30000, "lost", None);
    let id = held["data"]["id"].as_str().unwrap().to_string();
    let owner = lab.credentials.authenticate(&lab.owner).unwrap();
    lab.service
        .approve_operation(&owner, &id, held["proposal_digest"].as_str().unwrap(), AT)
        .unwrap();
    lab.service
        .execute_operation(&owner, &id, Scenario::TimeoutBeforeAccept, AT)
        .unwrap();
    let unknown = lab.timeline(&lab.owner, &id).unwrap();
    assert!(
        unknown["now"]
            .as_str()
            .unwrap()
            .contains("answer is not known")
    );
    assert_eq!(unknown["next"].as_array().unwrap().len(), 2);
    assert!(
        unknown["entries"].as_array().unwrap().last().unwrap()["summary"]
            .as_str()
            .unwrap()
            .contains("will not be sent again")
    );
    lab.call(
        &lab.owner,
        "POST",
        &format!("/operations/{id}/resolve"),
        json!({}),
    )
    .unwrap();
    let settled = lab.timeline(&lab.owner, &id).unwrap();
    assert_eq!(steps(&settled).last().unwrap(), "rail_voided");
    assert!(settled["now"].as_str().unwrap().starts_with("Not paid"));
}

#[test]
fn a_denial_explains_itself_and_nothing_comes_next() {
    let lab = Lab::new();
    let denied = lab.refund("pay_fixture_2041", 2001, "too-much", None);
    let id = denied["data"]["id"].as_str().unwrap();
    let story = lab.timeline(&lab.owner, id).unwrap();
    assert_eq!(steps(&story), ["denied"]);
    assert!(
        story["entries"][0]["summary"]
            .as_str()
            .unwrap()
            .ends_with("Denied: it is more than is left to refund on that payment.")
    );
    assert!(story["next"].as_array().unwrap().is_empty());
}

#[test]
fn agents_cannot_read_timelines_and_tampering_shows() {
    let lab = Lab::new();
    let paid = lab.refund("pay_fixture_2041", 500, "small", None);
    let id = paid["data"]["id"].as_str().unwrap();
    assert!(matches!(lab.timeline(&lab.bot, id), Err(Error::NotFound)));
    assert_eq!(lab.timeline(&lab.owner, id).unwrap()["evidence_ok"], true);
    rusqlite::Connection::open(lab.directory.path().join("transactions.sqlite"))
        .unwrap()
        .execute_batch("UPDATE operation_holds SET amount=1")
        .unwrap();
    assert_eq!(lab.timeline(&lab.owner, id).unwrap()["evidence_ok"], false);
}
