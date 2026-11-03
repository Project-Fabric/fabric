use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use cosigna_agent::{
    Actor, Error, Role, Service,
    api::Credentials,
    gateway,
    monitor::{
        MISSING_GRACE, Monitor, RAIL_FAILURES_BEFORE_FINDING, RECONCILE_EVERY,
        RECONCILIATION_UNAVAILABLE, REFUND_MISSING, REFUND_REVERSED, UNRECORDED_REFUND,
    },
    operations::{OperationKind, OperationRequest, OperationStage},
    provider::Scenario,
    stripe_rail::StripeRail,
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;
const KEY: &str = "sk_test_synthetic_fixture_key";

/// A local stand-in for the Stripe API: PaymentIntents, refunds, idempotency keys and the
/// over-refund rule. `drop_next_create` refuses to process the next create (it never
/// arrived); `lose_next_create` processes it but loses the response.
#[derive(Default)]
struct Fake {
    intents: HashMap<String, Value>,
    refunds: Vec<Value>,
    idempotency: HashMap<String, (String, String)>,
    drop_next_create: bool,
    lose_next_create: bool,
    refund_status: String,
    creates: Vec<(String, HashMap<String, String>)>,
    unauthorized: u32,
    /// The account-wide listing has no answer; lookups by payment still work.
    listing_down: bool,
    /// Refunds per page of the account-wide listing. Zero means 100.
    page_size: usize,
}

fn decode(text: &str) -> String {
    let bytes = text.replace('+', " ").into_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            out.push(
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap(), 16).unwrap(),
            );
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

impl Fake {
    fn refunded(&self, intent: &str) -> i64 {
        self.refunds
            .iter()
            .filter(|r| {
                r["payment_intent"] == intent
                    && r["status"] != "failed"
                    && r["status"] != "canceled"
            })
            .map(|r| r["amount"].as_i64().unwrap())
            .sum()
    }

    fn handle(
        &mut self,
        method: &str,
        url: &str,
        idem: Option<String>,
        body: &str,
    ) -> (u16, Value) {
        let invalid = |message: &str| {
            (
                400,
                json!({"error":{"type":"invalid_request_error","message":message}}),
            )
        };
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        match (method, path) {
            ("GET", p) if p.starts_with("/v1/payment_intents/") => {
                match self.intents.get(&p["/v1/payment_intents/".len()..]) {
                    Some(intent) => (200, intent.clone()),
                    None => (404, json!({"error":{"type":"invalid_request_error"}})),
                }
            }
            ("GET", "/v1/refunds") => {
                let mut query = form(query);
                if !query.contains_key("payment_intent") {
                    if self.listing_down {
                        return (500, json!({}));
                    }
                    let from: i64 = query["created[gte]"].parse().unwrap();
                    assert_eq!(query["limit"], "100");
                    let all: Vec<&Value> = self
                        .refunds
                        .iter()
                        .filter(|r| r["created"].as_i64().unwrap() >= from)
                        .collect();
                    let skip = query.get("starting_after").map_or(0, |id| {
                        all.iter().position(|r| r["id"] == id.as_str()).unwrap() + 1
                    });
                    let size = if self.page_size == 0 {
                        100
                    } else {
                        self.page_size
                    };
                    let page: Vec<&Value> = all.iter().skip(skip).take(size).copied().collect();
                    let more = skip + page.len() < all.len();
                    return (200, json!({"object":"list","has_more":more,"data":page}));
                }
                let intent = query.remove("payment_intent").unwrap_or_default();
                let data: Vec<Value> = self
                    .refunds
                    .iter()
                    .filter(|r| r["payment_intent"] == intent)
                    .cloned()
                    .collect();
                (200, json!({"object":"list","has_more":false,"data":data}))
            }
            ("GET", p) if p.starts_with("/v1/refunds/") => {
                match self
                    .refunds
                    .iter()
                    .find(|r| r["id"] == p["/v1/refunds/".len()..])
                {
                    Some(refund) => (200, refund.clone()),
                    None => (404, json!({})),
                }
            }
            ("POST", "/v1/refunds") => {
                let params = form(body);
                let idem = idem.unwrap_or_default();
                if let Some((saved_body, id)) = self.idempotency.get(&idem) {
                    if saved_body != body {
                        return (400, json!({"error":{"type":"idempotency_error"}}));
                    }
                    let refund = self
                        .refunds
                        .iter()
                        .find(|r| r["id"] == id.as_str())
                        .unwrap()
                        .clone();
                    return (200, refund);
                }
                let intent_id = params.get("payment_intent").cloned().unwrap_or_default();
                let Some(intent) = self.intents.get(&intent_id).cloned() else {
                    return invalid("No such payment_intent");
                };
                let amount: i64 = params
                    .get("amount")
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(0);
                if amount > intent["amount_received"].as_i64().unwrap() - self.refunded(&intent_id)
                {
                    return invalid("Refund amount is greater than unrefunded amount");
                }
                let mut metadata = serde_json::Map::new();
                for (k, v) in &params {
                    if let Some(key) = k
                        .strip_prefix("metadata[")
                        .and_then(|k| k.strip_suffix(']'))
                    {
                        metadata.insert(key.into(), json!(v));
                    }
                }
                let refund = json!({"id": format!("re_test_{}", self.refunds.len() + 1), "object":"refund",
                    "amount": amount, "currency": intent["currency"], "payment_intent": intent_id,
                    "status": self.refund_status, "metadata": metadata, "created": AT});
                self.idempotency.insert(
                    idem,
                    (body.to_string(), refund["id"].as_str().unwrap().into()),
                );
                self.refunds.push(refund.clone());
                self.creates
                    .push((refund["id"].as_str().unwrap().into(), params));
                (200, refund)
            }
            _ => (404, json!({})),
        }
    }
}

struct Lab {
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
    fake: Arc<Mutex<Fake>>,
    _directory: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        let fake = Arc::new(Mutex::new(Fake {
            refund_status: "succeeded".into(),
            ..Fake::default()
        }));
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let shared = fake.clone();
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let headers: Vec<(String, String)> = request
                    .headers()
                    .iter()
                    .map(|h| {
                        (
                            h.field.as_str().as_str().to_ascii_lowercase(),
                            h.value.as_str().to_string(),
                        )
                    })
                    .collect();
                let header = |name: &str| {
                    headers
                        .iter()
                        .find(|(k, _)| k == name)
                        .map(|(_, v)| v.clone())
                };
                let auth = header("authorization");
                let idem = header("idempotency-key");
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let method = request.method().as_str().to_string();
                let url = request.url().to_string();
                let mut fake = shared.lock().unwrap();
                if auth.as_deref() != Some(&format!("Bearer {KEY}")) {
                    fake.unauthorized += 1;
                    let _ = request
                        .respond(tiny_http::Response::from_string("{}").with_status_code(401));
                    continue;
                }
                if method == "POST" && fake.drop_next_create {
                    fake.drop_next_create = false;
                    drop(request); // never processed: the client sees a server error
                    continue;
                }
                let (status, value) = fake.handle(&method, &url, idem, &body);
                if method == "POST" && fake.lose_next_create {
                    fake.lose_next_create = false;
                    drop(request); // processed, but the response is lost
                    continue;
                }
                let _ = request.respond(
                    tiny_http::Response::from_string(value.to_string()).with_status_code(status),
                );
            }
        });
        let directory = tempfile::tempdir().unwrap();
        let mut service = Service::open(directory.path()).unwrap();
        service.rails = service.rails.clone().with_stripe_test(
            StripeRail::new(KEY, Some(&format!("http://127.0.0.1:{port}"))).unwrap(),
        );
        let credentials =
            Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
        let private: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("credentials.json")).unwrap(),
        )
        .unwrap();
        let owner = format!("Bearer {}", private["owner"].as_str().unwrap());
        let mut lab = Self {
            service,
            credentials,
            owner,
            budget: String::new(),
            fake,
            _directory: directory,
        };
        lab.budget = lab
            .call(
                &lab.owner.clone(),
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,
                "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400*3}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        for (id, amount, status, livemode) in [
            ("pi_test_paid", 5000, "succeeded", false),
            ("pi_test_big", 45000, "succeeded", false),
            ("pi_test_live", 5000, "succeeded", true),
            ("pi_test_unpaid", 5000, "requires_payment_method", false),
        ] {
            lab.fake.lock().unwrap().intents.insert(id.into(), json!({"id":id,"object":"payment_intent",
                "livemode":livemode,"status":status,"currency":"usd","amount":amount,
                "amount_received": if status == "succeeded" {amount} else {0},"customer":"cus_test_ana"}));
        }
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

    fn agent(&self, kinds: Value) -> String {
        let issued = self
            .call(
                &self.owner,
                "POST",
                "/agents",
                json!({"name":"refunds-bot","budget_id":self.budget,
                "permissions":["execute"],"expires_at":AT+86400,
                "operations":{"kinds":kinds,"rails":["stripe_test"],"auto_approve_minor":10000}}),
            )
            .unwrap();
        format!("Bearer {}", issued["data"]["token"].as_str().unwrap())
    }

    fn refund(&self, auth: &str, source: &str, amount: i64, key: &str) -> Value {
        self.call(
            auth,
            "POST",
            "/operations",
            json!({"budget_id":self.budget,"kind":"refund",
            "rail":"stripe_test","source":source,"amount_minor":amount,"currency":"USD",
            "reason":"Order cancelled","idempotency_key":key}),
        )
        .unwrap()
    }

    fn owner_actor() -> Actor {
        Actor {
            owner: "local-user".into(),
            role: Role::Owner,
        }
    }

    fn approved(&self, source: &str, amount: i64, key: &str) -> String {
        let owner = Self::owner_actor();
        let op = self
            .service
            .request_operation(
                &owner,
                OperationRequest {
                    budget_id: self.budget.clone(),
                    kind: OperationKind::Refund,
                    rail: "stripe_test".into(),
                    source: Some(source.into()),
                    payee: None,
                    amount_minor: amount,
                    currency: "USD".into(),
                    reason: "r".into(),
                    idempotency_key: key.into(),
                    case_id: None,
                },
                AT,
            )
            .unwrap();
        let digest = op.proposal.as_ref().unwrap().digest().unwrap();
        self.service
            .approve_operation(&owner, &op.id, &digest, AT)
            .unwrap();
        op.id
    }

    fn creates(&self) -> usize {
        self.fake.lock().unwrap().creates.len()
    }
}

#[test]
fn an_agent_refund_runs_through_stripe_with_the_gateways_key_and_an_idempotency_key() {
    let lab = Lab::new();
    let bot = lab.agent(json!(["refund"]));
    let done = lab.refund(&bot, "pi_test_paid", 2000, "r1");
    assert_eq!(done["data"]["stage"], "succeeded");
    assert_eq!(done["data"]["proposal"]["facts"]["refundable_minor"], 5000);
    assert!(
        done["data"]["receipt"]["rail_reference"]
            .as_str()
            .unwrap()
            .starts_with("re_test_")
    );
    let fake = lab.fake.lock().unwrap();
    assert_eq!(fake.creates.len(), 1);
    let (_, params) = &fake.creates[0];
    let attempt = done["data"]["attempt_key"].as_str().unwrap();
    assert_eq!(params["metadata[cosigna_attempt]"], attempt);
    assert_eq!(
        params["metadata[cosigna_proposal_digest]"],
        done["proposal_digest"].as_str().unwrap()
    );
    assert_eq!(
        fake.idempotency.keys().next().unwrap(),
        &format!("cosigna:{attempt}")
    );
    assert_eq!(fake.unauthorized, 0);
    drop(fake);
    let replay = lab.refund(&bot, "pi_test_paid", 2000, "r1");
    assert_eq!(replay["data"]["id"], done["data"]["id"]);
    assert_eq!(lab.creates(), 1);
    let report = lab.service.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
}

#[test]
fn stripe_records_are_checked_before_any_decision() {
    let lab = Lab::new();
    let bot = lab.agent(json!(["refund", "payout"]));
    lab.fake
        .lock()
        .unwrap()
        .refunds
        .push(json!({"id":"re_external","object":"refund","amount":4000,
        "currency":"usd","payment_intent":"pi_test_paid","status":"succeeded","metadata":{}}));
    for (source, amount, rule) in [
        ("pi_test_missing", 100, "source_not_found"),
        ("pi_test_live", 100, "live_mode_refused"),
        ("pi_test_unpaid", 100, "payment_not_settled"),
        ("pi_test_paid", 2000, "exceeds_refundable"),
        ("ch_not_a_payment_intent", 100, "source_not_found"),
    ] {
        let denied = lab.refund(&bot, source, amount, source);
        assert_eq!(
            denied["data"]["decision"],
            json!({"outcome":"denied","rule":rule}),
            "{source}"
        );
    }
    let payout = lab
        .call(&bot, "POST", "/operations", json!({"budget_id":lab.budget,"kind":"payout","rail":"stripe_test",
            "payee":"ba_test_1","amount_minor":100,"currency":"USD","reason":"r","idempotency_key":"p"}))
        .unwrap();
    assert_eq!(
        payout["data"]["decision"]["rule"],
        "kind_not_supported_on_rail"
    );
    assert_eq!(lab.creates(), 0, "nothing was sent to Stripe");
}

#[test]
fn a_lost_response_is_reconciled_from_stripe_without_a_second_refund() {
    let lab = Lab::new();
    let owner = Lab::owner_actor();
    let id = lab.approved("pi_test_paid", 2000, "lost-response");
    lab.fake.lock().unwrap().lose_next_create = true;
    let unknown = lab
        .service
        .execute_operation(&owner, &id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(unknown.stage, OperationStage::Unknown);
    let settled = lab.service.reconcile_operation(&owner, &id, AT).unwrap();
    assert_eq!(settled.stage, OperationStage::Succeeded);
    assert_eq!(lab.creates(), 1);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn resolve_completes_an_unsent_refund_exactly_once_inside_stripes_idempotency_window() {
    let lab = Lab::new();
    let owner = Lab::owner_actor();
    let id = lab.approved("pi_test_paid", 2000, "never-sent");
    lab.fake.lock().unwrap().drop_next_create = true;
    assert_eq!(
        lab.service
            .execute_operation(&owner, &id, Scenario::Success, AT)
            .unwrap()
            .stage,
        OperationStage::Unknown
    );
    // Lookup alone finds nothing and settles nothing.
    assert_eq!(
        lab.service
            .reconcile_operation(&owner, &id, AT)
            .unwrap()
            .stage,
        OperationStage::Unknown
    );
    assert_eq!(lab.creates(), 0);
    let resolved = lab
        .call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/resolve"),
            json!({}),
        )
        .unwrap();
    assert_eq!(resolved["data"]["stage"], "succeeded");
    assert_eq!(lab.creates(), 1);
    lab.call(
        &lab.owner,
        "POST",
        &format!("/operations/{id}/resolve"),
        json!({}),
    )
    .unwrap();
    assert_eq!(lab.creates(), 1, "resolving again changes nothing");
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn after_the_idempotency_window_an_unknown_refund_stays_held() {
    let lab = Lab::new();
    let owner = Lab::owner_actor();
    let id = lab.approved("pi_test_paid", 2000, "too-late");
    lab.fake.lock().unwrap().drop_next_create = true;
    lab.service
        .execute_operation(&owner, &id, Scenario::Success, AT)
        .unwrap();
    let later = lab
        .service
        .resolve_operation(&owner, &id, AT + 24 * 3600)
        .unwrap();
    assert_eq!(later.stage, OperationStage::Unknown);
    assert_eq!(
        lab.creates(),
        0,
        "no blind retry after Stripe forgets the key"
    );
}

#[test]
fn stripe_refusing_at_execution_fails_the_operation_and_releases_its_hold() {
    let lab = Lab::new();
    let owner = Lab::owner_actor();
    let id = lab.approved("pi_test_big", 45000, "raced");
    // Someone refunds most of the payment in Stripe's dashboard after approval.
    lab.fake
        .lock()
        .unwrap()
        .refunds
        .push(json!({"id":"re_dashboard","object":"refund","amount":40000,
        "currency":"usd","payment_intent":"pi_test_big","status":"succeeded","metadata":{}}));
    let failed = lab
        .service
        .execute_operation(&owner, &id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(failed.stage, OperationStage::Failed);
    let balance = lab
        .call(
            &lab.owner,
            "GET",
            &format!("/budgets/{}", lab.budget),
            Value::Null,
        )
        .unwrap();
    assert_eq!(balance["data"]["available_minor"], 100000);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn a_pending_refund_stays_held_until_stripe_settles_it() {
    let lab = Lab::new();
    let owner = Lab::owner_actor();
    lab.fake.lock().unwrap().refund_status = "pending".into();
    let id = lab.approved("pi_test_paid", 2000, "pending");
    assert_eq!(
        lab.service
            .execute_operation(&owner, &id, Scenario::Success, AT)
            .unwrap()
            .stage,
        OperationStage::Unknown
    );
    lab.fake.lock().unwrap().refunds[0]["status"] = json!("succeeded");
    assert_eq!(
        lab.service
            .reconcile_operation(&owner, &id, AT)
            .unwrap()
            .stage,
        OperationStage::Succeeded
    );
}

#[test]
fn live_keys_and_non_loopback_addresses_are_refused() {
    for key in [
        "sk_live_real",
        "rk_live_real",
        "pk_test_public",
        "sk_test_",
        "",
        "sk_test_has space",
    ] {
        assert!(StripeRail::new(key, None).is_err(), "{key}");
    }
    assert!(StripeRail::new(KEY, None).is_ok());
    for base in [
        "https://evil.example",
        "http://localhost:1234",
        "http://127.0.0.1:0",
        "http://127.0.0.1:99999",
    ] {
        assert!(StripeRail::new(KEY, Some(base)).is_err(), "{base}");
    }
}

/// One comparison with Stripe, as a freshly started worker would make it.
fn compare(lab: &Lab, at: i64) -> Vec<(String, String)> {
    Monitor::new()
        .run(&lab.service, None, at)
        .unwrap()
        .into_iter()
        .map(|f| (f.kind, f.subject))
        .collect()
}

/// A refund made in Stripe directly, with no Cosigna attempt behind it.
fn outside_refund(lab: &Lab, id: &str, status: &str, created: i64) {
    lab.fake
        .lock()
        .unwrap()
        .refunds
        .push(json!({"id":id,"object":"refund","amount":700,
        "currency":"usd","payment_intent":"pi_test_paid","status":status,"metadata":{},
        "created":created}));
}

#[test]
fn a_refund_made_outside_cosigna_is_reported_once_and_earlier_ones_are_not() {
    let lab = Lab::new();
    outside_refund(&lab, "re_before", "succeeded", AT - 500);
    // The first pass only records where to start.
    assert!(compare(&lab, AT - 100).is_empty());
    assert!(compare(&lab, AT).is_empty());

    outside_refund(&lab, "re_outside", "succeeded", AT);
    assert_eq!(
        compare(&lab, AT + 100),
        vec![(UNRECORDED_REFUND.to_string(), "re_outside".to_string())]
    );
    assert!(compare(&lab, AT + 200).is_empty());
    let status = lab
        .service
        .monitor_status(&Lab::owner_actor(), AT + 200)
        .unwrap();
    assert_eq!(status.open_findings, 1);
    assert_eq!(status.reconciliation.from, Some(AT - 100));
    assert_eq!(status.reconciliation.last_completed_at, Some(AT + 200));
    assert_eq!(status.findings[0].detail["amount_minor"], 700);
    assert_eq!(status.findings[0].detail["currency"], "USD");
    assert_eq!(status.findings[0].detail["source"], "pi_test_paid");

    // One that moved no money is not a problem; one still in flight is.
    outside_refund(&lab, "re_failed", "failed", AT);
    outside_refund(&lab, "re_pending", "pending", AT);
    assert_eq!(
        compare(&lab, AT + 300),
        vec![(UNRECORDED_REFUND.to_string(), "re_pending".to_string())]
    );
    // The comparison only reads: nothing was created, and the evidence still verifies.
    assert_eq!(lab.creates(), 0);
    let report = lab.service.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
}

#[test]
fn refunds_made_through_cosigna_are_not_reported() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    let bot = lab.agent(json!(["refund"]));
    assert_eq!(
        lab.refund(&bot, "pi_test_paid", 2000, "r1")["data"]["stage"],
        "succeeded"
    );
    assert!(compare(&lab, AT + MISSING_GRACE + 1).is_empty());
}

#[test]
fn a_refund_still_settling_belongs_to_recovery_and_is_not_reported() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    lab.fake.lock().unwrap().refund_status = "pending".into();
    let bot = lab.agent(json!(["refund"]));
    assert_eq!(
        lab.refund(&bot, "pi_test_paid", 2000, "r1")["data"]["stage"],
        "unknown"
    );
    assert!(compare(&lab, AT + MISSING_GRACE + 1).is_empty());
}

#[test]
fn a_recorded_refund_that_later_fails_is_reported_against_its_operation() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    let bot = lab.agent(json!(["refund"]));
    let done = lab.refund(&bot, "pi_test_paid", 2000, "r1");
    let id = done["data"]["id"].as_str().unwrap().to_string();
    lab.fake.lock().unwrap().refunds[0]["status"] = json!("failed");
    assert_eq!(
        compare(&lab, AT + 100),
        vec![(REFUND_REVERSED.to_string(), id.clone())]
    );
    assert!(compare(&lab, AT + 200).is_empty());
    // It reports; it does not rewrite what the ledger says happened.
    let op = lab.service.operation(&Lab::owner_actor(), &id).unwrap();
    assert_eq!(op.stage, OperationStage::Succeeded);
}

#[test]
fn copying_an_operations_metadata_onto_another_refund_does_not_make_it_recorded() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    let bot = lab.agent(json!(["refund"]));
    lab.refund(&bot, "pi_test_paid", 2000, "r1");
    let genuine = lab.fake.lock().unwrap().refunds[0].clone();
    {
        let mut fake = lab.fake.lock().unwrap();
        // The same terms under a second refund id: a second payment for one approval.
        let mut twin = genuine.clone();
        twin["id"] = json!("re_twin");
        fake.refunds.push(twin);
        // A different amount.
        let mut larger = genuine.clone();
        larger["id"] = json!("re_larger");
        larger["amount"] = json!(9000);
        fake.refunds.push(larger);
        // A different payment.
        let mut elsewhere = genuine.clone();
        elsewhere["id"] = json!("re_elsewhere");
        elsewhere["payment_intent"] = json!("pi_test_big");
        fake.refunds.push(elsewhere);
    }
    let mut found = compare(&lab, AT + 100);
    found.sort();
    let expected: Vec<(String, String)> = ["re_elsewhere", "re_larger", "re_twin"]
        .iter()
        .map(|id| (UNRECORDED_REFUND.to_string(), id.to_string()))
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn a_recorded_failure_that_stripe_shows_as_paid_is_reported() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    lab.fake.lock().unwrap().refund_status = "failed".into();
    let bot = lab.agent(json!(["refund"]));
    assert_eq!(
        lab.refund(&bot, "pi_test_paid", 2000, "r1")["data"]["stage"],
        "failed"
    );
    assert!(compare(&lab, AT + 100).is_empty());
    lab.fake.lock().unwrap().refunds[0]["status"] = json!("succeeded");
    assert_eq!(
        compare(&lab, AT + 200),
        vec![(UNRECORDED_REFUND.to_string(), "re_test_1".to_string())]
    );
}

#[test]
fn a_recorded_success_stripe_has_no_refund_for_is_reported_after_a_grace_period() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    let bot = lab.agent(json!(["refund"]));
    let id = lab.refund(&bot, "pi_test_paid", 2000, "r1")["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    lab.fake.lock().unwrap().refunds.clear();
    assert!(compare(&lab, AT + MISSING_GRACE - 1).is_empty());
    assert_eq!(
        compare(&lab, AT + MISSING_GRACE),
        vec![(REFUND_MISSING.to_string(), id)]
    );
}

#[test]
fn a_listing_that_is_cut_short_is_reported_and_never_counted_as_clean() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    let bot = lab.agent(json!(["refund"]));
    let id = lab.refund(&bot, "pi_test_paid", 2000, "r1")["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    {
        let mut fake = lab.fake.lock().unwrap();
        fake.page_size = 1;
        // A recorded refund past the end of the listing is not called missing.
        fake.refunds.clear();
    }
    for i in 0..11 {
        outside_refund(&lab, &format!("re_old{i}"), "failed", AT);
    }
    let found = compare(&lab, AT + MISSING_GRACE + 1);
    assert_eq!(
        found,
        vec![(
            RECONCILIATION_UNAVAILABLE.to_string(),
            "stripe_test".to_string()
        )]
    );
    let status = lab
        .service
        .monitor_status(&Lab::owner_actor(), AT + MISSING_GRACE + 1)
        .unwrap();
    assert_eq!(status.findings[0].detail["reason"], "too_many_refunds");
    assert_eq!(status.reconciliation.last_completed_at, Some(AT - 1));
    // Every page the limit allows was read, following the cursor.
    lab.fake.lock().unwrap().page_size = 100;
    assert_eq!(
        compare(&lab, AT + MISSING_GRACE + 2),
        vec![(REFUND_MISSING.to_string(), id)]
    );
    let status = lab
        .service
        .monitor_status(&Lab::owner_actor(), AT + MISSING_GRACE + 2)
        .unwrap();
    assert!(
        status
            .findings
            .iter()
            .all(|f| f.kind != RECONCILIATION_UNAVAILABLE)
    );
}

#[test]
fn repeated_silence_from_stripe_is_reported_and_clears_when_it_answers() {
    let lab = Lab::new();
    let mut monitor = Monitor::new();
    assert!(monitor.run(&lab.service, None, AT).unwrap().is_empty());
    lab.fake.lock().unwrap().listing_down = true;
    let mut at = AT;
    for pass in 1..=RAIL_FAILURES_BEFORE_FINDING {
        at += RECONCILE_EVERY;
        let found = monitor.run(&lab.service, None, at).unwrap();
        if pass < RAIL_FAILURES_BEFORE_FINDING {
            assert!(found.is_empty(), "reported after {pass} failures");
        } else {
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].kind, RECONCILIATION_UNAVAILABLE);
        }
    }
    at += RECONCILE_EVERY;
    assert!(monitor.run(&lab.service, None, at).unwrap().is_empty());
    let owner = Lab::owner_actor();
    assert_eq!(
        lab.service
            .monitor_status(&owner, at)
            .unwrap()
            .open_findings,
        1
    );
    lab.fake.lock().unwrap().listing_down = false;
    at += RECONCILE_EVERY;
    assert!(monitor.run(&lab.service, None, at).unwrap().is_empty());
    assert_eq!(
        lab.service
            .monitor_status(&owner, at)
            .unwrap()
            .open_findings,
        0
    );
}

#[test]
fn text_from_the_payment_system_cannot_reach_evidence_or_alerts_unchecked() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    lab.fake
        .lock()
        .unwrap()
        .refunds
        .push(json!({"id":"re_<!channel>","object":"refund",
        "amount":700,"currency":"usd","payment_intent":"pi_test_paid","status":"succeeded",
        "metadata":{},"created":AT}));
    // Not a finding with that text in it, and not silence: the pass counts as no answer.
    let mut monitor = Monitor::new();
    for pass in 0..RAIL_FAILURES_BEFORE_FINDING {
        let found = monitor
            .run(&lab.service, None, AT + i64::from(pass) * RECONCILE_EVERY)
            .unwrap();
        if pass + 1 < RAIL_FAILURES_BEFORE_FINDING {
            assert!(found.is_empty());
        } else {
            assert_eq!(found[0].kind, RECONCILIATION_UNAVAILABLE);
            assert!(!format!("{found:?}").contains("channel"));
        }
    }
}

#[test]
fn a_refund_with_an_empty_or_new_status_counts_as_money_that_may_have_moved() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    outside_refund(&lab, "re_new_status", "some_future_status", AT);
    outside_refund(&lab, "re_no_status", "failed", AT);
    lab.fake.lock().unwrap().refunds[1]["status"] = Value::Null;
    let mut found = compare(&lab, AT + 100);
    found.sort();
    assert_eq!(
        found,
        vec![
            (UNRECORDED_REFUND.to_string(), "re_new_status".to_string()),
            (UNRECORDED_REFUND.to_string(), "re_no_status".to_string()),
        ]
    );
    let status = lab
        .service
        .monitor_status(&Lab::owner_actor(), AT + 100)
        .unwrap();
    assert!(
        status
            .findings
            .iter()
            .any(|f| f.detail["status"] == "unknown")
    );
}

#[test]
fn a_forged_refund_is_reported_even_while_its_operation_is_still_settling() {
    let lab = Lab::new();
    assert!(compare(&lab, AT - 1).is_empty());
    lab.fake.lock().unwrap().refund_status = "pending".into();
    let bot = lab.agent(json!(["refund"]));
    assert_eq!(
        lab.refund(&bot, "pi_test_paid", 2000, "r1")["data"]["stage"],
        "unknown"
    );
    {
        let mut fake = lab.fake.lock().unwrap();
        let genuine = fake.refunds[0].clone();
        for (id, field, value) in [
            ("re_amount", "amount", json!(9000)),
            ("re_payment", "payment_intent", json!("pi_test_big")),
            ("re_currency", "currency", json!("eur")),
        ] {
            let mut forged = genuine.clone();
            forged["id"] = json!(id);
            forged[field] = value;
            fake.refunds.push(forged);
        }
        let mut forged = genuine.clone();
        forged["id"] = json!("re_digest");
        forged["metadata"]["cosigna_proposal_digest"] = json!("other");
        fake.refunds.push(forged);
    }
    let mut found: Vec<String> = compare(&lab, AT + 100).into_iter().map(|f| f.1).collect();
    found.sort();
    assert_eq!(
        found,
        ["re_amount", "re_currency", "re_digest", "re_payment"]
    );
}
