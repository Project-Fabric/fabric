//! Case sources: the gateway reads case records from the connected system, chosen by a
//! `cosigna.cases/v1` file. The help-desk tests run against a local stand-in shaped like
//! Zendesk's documented ticket API (`GET /api/v2/tickets/{id}.json`, custom fields as
//! `{id, value}`), not a real Zendesk account. All stores are synthetic, in temporary folders.
use std::sync::{Arc, Mutex};

use cosigna_agent::{
    Error, Service,
    api::Credentials,
    cases::{CaseSource, HttpCases},
    gateway,
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;
const SECRET: &str = "zd-test-token-123";
const CUSTOMER: u64 = 360_001;
const PAYMENT: u64 = 360_002;
const OWED: u64 = 360_003;

#[derive(Default)]
struct Desk {
    tickets: std::collections::HashMap<String, Value>,
    /// Every request: (path, authorization header).
    seen: Vec<(String, Option<String>)>,
    /// Answer every request with this status instead.
    fail_with: Option<u16>,
}

fn ticket(id: u64, customer: &str, payment: &str, owed: Value) -> Value {
    json!({"ticket": {"id": id, "status": "open", "requester_id": 9001, "custom_fields": [
        {"id": CUSTOMER, "value": customer}, {"id": PAYMENT, "value": payment},
        {"id": OWED, "value": owed}, {"id": 360_004, "value": null}]}})
}

/// A stand-in help desk on a loopback port.
fn desk() -> (u16, Arc<Mutex<Desk>>) {
    let desk = Arc::new(Mutex::new(Desk::default()));
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let shared = desk.clone();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let auth = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.as_str().to_string());
            let path = request.url().to_string();
            let mut desk = shared.lock().unwrap();
            desk.seen.push((path.clone(), auth.clone()));
            let expected = format!("Basic {}", basic("agent@acme.test/token", SECRET));
            let (status, body) = if let Some(status) = desk.fail_with {
                (status, "{}".to_string())
            } else if auth.as_deref() != Some(&expected) {
                (401, "{}".into())
            } else {
                match path
                    .strip_prefix("/api/v2/tickets/")
                    .and_then(|p| p.strip_suffix(".json"))
                    .and_then(|id| desk.tickets.get(id))
                {
                    Some(t) => (200, t.to_string()),
                    None => (404, r#"{"error":"RecordNotFound"}"#.into()),
                }
            };
            let _ =
                request.respond(tiny_http::Response::from_string(body).with_status_code(status));
        }
    });
    (port, desk)
}

/// Independent of the crate's encoder: the RFC 4648 alphabet, written out here.
fn basic(user: &str, secret: &str) -> String {
    let bytes = format!("{user}:{secret}").into_bytes();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(alphabet[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn zendesk_config(port: u16) -> Value {
    json!({"name": "zendesk", "url": format!("http://127.0.0.1:{port}/api/v2/tickets/{{id}}.json"),
        "auth": {"scheme": "basic", "username": "agent@acme.test/token", "secret_env": "UNUSED"},
        "id_prefix": "zd_",
        "fields": {
            "customer": format!("/ticket/custom_fields/[id={CUSTOMER}]/value"),
            "source": format!("/ticket/custom_fields/[id={PAYMENT}]/value"),
            "owed": format!("/ticket/custom_fields/[id={OWED}]/value"),
            "owed_unit": "major", "currency_fixed": "USD"}})
}

struct Lab {
    _directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
    bot: String,
}

impl Lab {
    fn new(source: CaseSource) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut service = Service::open(directory.path()).unwrap();
        service.rails = service.rails.clone().with_cases(source);
        let credentials =
            Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
        let private: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("credentials.json")).unwrap(),
        )
        .unwrap();
        let owner = format!("Bearer {}", private["owner"].as_str().unwrap());
        let mut lab = Self {
            _directory: directory,
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
                "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400*2}),
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
                json!({"name":"refunds-bot","budget_id":lab.budget,"permissions":["execute"],
                "expires_at":AT+86400,"operations":{"kinds":["refund"],"rails":["fixture"],
                "auto_approve_minor":50000,"require_case":true}})
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

    fn refund(&self, source: &str, amount: i64, case: &str, key: &str) -> Result<Value, Error> {
        self.call(
            &self.bot,
            "POST",
            "/operations",
            json!({"budget_id":self.budget,"kind":"refund","rail":"fixture","source":source,
                "amount_minor":amount,"currency":"USD","reason":"Ticket","idempotency_key":key,
                "case_id":case}),
        )
    }

    fn operations(&self) -> usize {
        self.call(&self.owner, "GET", "/operations", json!({}))
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .len()
    }
}

fn outcome(op: &Value) -> (&str, &str) {
    (
        op["data"]["stage"].as_str().unwrap(),
        op["data"]["decision"]["rule"].as_str().unwrap(),
    )
}

fn zendesk_lab() -> (Lab, Arc<Mutex<Desk>>) {
    let (port, desk) = desk();
    let source = CaseSource::from(HttpCases::from_json(&zendesk_config(port), SECRET).unwrap());
    (Lab::new(source), desk)
}

#[test]
fn a_refund_that_matches_the_help_desk_ticket_pays_and_records_what_the_ticket_said() {
    let (lab, desk) = zendesk_lab();
    desk.lock().unwrap().tickets.insert(
        "2045".into(),
        ticket(2045, "cus_fixture_ben", "pay_fixture_2045", json!("400.00")),
    );
    let paid = lab
        .refund("pay_fixture_2045", 40000, "zd_2045", "match")
        .unwrap();
    assert_eq!(outcome(&paid), ("succeeded", "within_agent_auto_limit"));
    let case = &paid["data"]["proposal"]["facts"]["case"];
    assert_eq!(case["system"], "zendesk");
    assert_eq!(case["id"], "zd_2045");
    assert_eq!(case["owed_minor"], 40000);
    // One read of the ticket, by its own id, with the configured credential.
    let seen = desk.lock().unwrap().seen.clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "/api/v2/tickets/2045.json");
    assert_eq!(
        seen[0].1.as_deref(),
        Some(format!("Basic {}", basic("agent@acme.test/token", SECRET)).as_str())
    );
    // The payment has $50 left, but the ticket has nothing left owed.
    let again = lab
        .refund("pay_fixture_2045", 1000, "zd_2045", "again")
        .unwrap();
    assert_eq!(outcome(&again), ("held", "exceeds_case_owed"));
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn a_refund_that_disagrees_with_the_ticket_waits_for_a_person() {
    let (lab, desk) = zendesk_lab();
    desk.lock().unwrap().tickets.insert(
        "2047".into(),
        ticket(2047, "cus_fixture_chloe", "pay_fixture_2047", json!(5)),
    );
    let more = lab
        .refund("pay_fixture_2047", 4500, "zd_2047", "more")
        .unwrap();
    assert_eq!(outcome(&more), ("held", "exceeds_case_owed"));
    let other = lab
        .refund("pay_fixture_2041", 300, "zd_2047", "other")
        .unwrap();
    assert_eq!(outcome(&other), ("held", "case_mismatch"));
}

#[test]
fn an_unknown_ticket_is_denied_and_foreign_ids_are_not_looked_up() {
    let (lab, desk) = zendesk_lab();
    let missing = lab
        .refund("pay_fixture_2041", 500, "zd_9999", "missing")
        .unwrap();
    assert_eq!(outcome(&missing), ("denied", "case_not_found"));
    // The synthetic case book is no longer consulted, and ids without the prefix are not sent.
    let fixture = lab
        .refund("pay_fixture_2041", 500, "case_fixture_2041", "fixture")
        .unwrap();
    assert_eq!(outcome(&fixture), ("denied", "case_not_found"));
    assert_eq!(desk.lock().unwrap().seen.len(), 1);
}

#[test]
fn an_unreachable_or_unreadable_help_desk_fails_closed_and_records_nothing() {
    let (lab, desk) = zendesk_lab();
    for status in [500, 429, 401, 302] {
        desk.lock().unwrap().fail_with = Some(status);
        let error = lab
            .refund("pay_fixture_2041", 500, "zd_2041", &format!("s{status}"))
            .unwrap_err();
        assert!(matches!(error, Error::CaseSource(_)), "{status}: {error}");
        assert_eq!(
            cosigna_agent::api::error_response(&error).0,
            503,
            "{status}"
        );
    }
    let mut d = desk.lock().unwrap();
    d.fail_with = None;
    // A ticket whose owed field is empty, negative, or has too many decimals is not guessed at.
    d.tickets.insert(
        "1".into(),
        ticket(1, "cus_fixture_ana", "pay_fixture_2041", Value::Null),
    );
    d.tickets.insert(
        "2".into(),
        ticket(2, "cus_fixture_ana", "pay_fixture_2041", json!("-5.00")),
    );
    d.tickets.insert(
        "3".into(),
        ticket(3, "cus_fixture_ana", "pay_fixture_2041", json!("5.001")),
    );
    d.tickets.insert(
        "4".into(),
        ticket(4, "cus_fixture_ana", "pay fixture 2041", json!("5.00")),
    );
    drop(d);
    for id in ["zd_1", "zd_2", "zd_3", "zd_4"] {
        let error = lab.refund("pay_fixture_2041", 500, id, id).unwrap_err();
        assert!(matches!(error, Error::CaseSource(_)), "{id}: {error}");
    }
    assert_eq!(lab.operations(), 0, "no operation was recorded");
    // An address nothing listens on.
    let mut config = zendesk_config(1);
    config["url"] = json!("http://127.0.0.1:1/api/v2/tickets/{id}.json");
    let closed = Lab::new(CaseSource::from(
        HttpCases::from_json(&config, SECRET).unwrap(),
    ));
    assert!(matches!(
        closed.refund("pay_fixture_2041", 500, "zd_1", "closed"),
        Err(Error::CaseSource(_))
    ));
}

#[test]
fn exposure_counts_case_bound_agents_up_to_their_budget_when_cases_cannot_be_listed() {
    let (lab, _desk) = zendesk_lab();
    let report = lab.call(&lab.owner, "GET", "/exposure", json!({})).unwrap();
    let agent = &report["data"]["agents"][0];
    assert_eq!(agent["must_name_case"], true);
    assert_eq!(agent["exposure_minor"], 100000);
    assert_eq!(
        report["data"]["currencies"][0]["without_a_person_minor"],
        100000
    );
    let notes = report["data"]["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 4);
    assert!(notes[3].as_str().unwrap().contains("cannot be totalled"));
}

#[test]
fn a_csv_export_replaces_the_synthetic_case_book() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("cases.csv"),
        "id,customer,source,currency,owed_minor\r\nord_1001,cus_fixture_ana,pay_fixture_2041,USD,1500\r\n",
    )
    .unwrap();
    let config = folder.path().join("cases.json");
    std::fs::write(
        &config,
        r#"{"schema":"cosigna.cases/v1","source":"csv","path":"cases.csv"}"#,
    )
    .unwrap();
    let source = CaseSource::load(&config).unwrap();
    assert_eq!(source.name(), "csv");
    let lab = Lab::new(source);
    let paid = lab
        .refund("pay_fixture_2041", 1500, "ord_1001", "csv")
        .unwrap();
    assert_eq!(outcome(&paid), ("succeeded", "within_agent_auto_limit"));
    assert_eq!(paid["data"]["proposal"]["facts"]["case"]["system"], "csv");
    let fixture = lab
        .refund("pay_fixture_2045", 500, "case_fixture_2045", "fixture")
        .unwrap();
    assert_eq!(outcome(&fixture), ("denied", "case_not_found"));
    // Listable: exposure is bounded by what the export says is still owed (nothing, now).
    let report = lab.call(&lab.owner, "GET", "/exposure", json!({})).unwrap();
    assert_eq!(report["data"]["agents"][0]["exposure_minor"], 0);
    assert_eq!(report["data"]["notes"].as_array().unwrap().len(), 3);
}

#[test]
fn configurations_that_could_leak_the_secret_or_guess_are_refused() {
    let folder = tempfile::tempdir().unwrap();
    let load = |text: &str| {
        let path = folder.path().join("c.json");
        std::fs::write(&path, text).unwrap();
        CaseSource::load(&path)
    };
    // Unknown schema, unknown source, unknown keys.
    assert!(load(r#"{"schema":"cosigna.cases/v2","source":"csv","path":"x.csv"}"#).is_err());
    assert!(load(r#"{"schema":"cosigna.cases/v1","source":"ftp","path":"x"}"#).is_err());
    assert!(
        load(r#"{"schema":"cosigna.cases/v1","source":"csv","path":"x.csv","extra":1}"#).is_err()
    );
    // A CSV that repeats a case id would make "what is owed" ambiguous.
    std::fs::write(
        folder.path().join("twice.csv"),
        "id,customer,source,currency,owed_minor\nord_1,c,p,USD,1\nord_1,c,p,USD,2\n",
    )
    .unwrap();
    assert!(load(r#"{"schema":"cosigna.cases/v1","source":"csv","path":"twice.csv"}"#).is_err());

    let http = |change: &dyn Fn(&mut Value)| {
        let mut config = zendesk_config(8080);
        config["url"] = json!("https://acme.zendesk.com/api/v2/tickets/{id}.json");
        change(&mut config);
        HttpCases::from_json(&config, SECRET)
    };
    assert!(http(&|_| {}).is_ok());
    for bad in [
        "http://acme.zendesk.com/api/v2/tickets/{id}.json",
        "http://localhost:8080/api/v2/tickets/{id}.json",
        "https://user@acme.zendesk.com/api/v2/tickets/{id}.json",
        "https://acme.zendesk.com/api/v2/tickets/",
        "https://acme.zendesk.com/{id}/{id}",
        "https://acme.zendesk.com/search?q={id}",
        "https:///{id}",
    ] {
        assert!(http(&|c| c["url"] = json!(bad)).is_err(), "{bad}");
    }
    assert!(http(&|c| c["fields"]["currency"] = json!("/ticket/currency")).is_err());
    assert!(
        http(&|c| {
            c["fields"]["currency_fixed"] = Value::Null;
        })
        .is_err()
    );
    assert!(http(&|c| c["fields"]["currency_fixed"] = json!("usd")).is_err());
    assert!(http(&|c| c["auth"]["username"] = json!("a:b")).is_err());
    assert!(http(&|c| c["id_prefix"] = json!("zd/")).is_err());
    assert!(HttpCases::from_json(&zendesk_config(8080), " ").is_err());
    // The secret comes only from the environment variable the file names.
    let config = folder.path().join("desk.json");
    let mut file = zendesk_config(8080);
    file["schema"] = json!("cosigna.cases/v1");
    file["source"] = json!("http");
    file["auth"]["secret_env"] = json!("COSIGNA_TEST_CASES_SECRET_NOT_SET");
    std::fs::write(&config, file.to_string()).unwrap();
    let error = CaseSource::load(&config).err().unwrap().to_string();
    assert!(error.contains("COSIGNA_TEST_CASES_SECRET_NOT_SET"));
}
