//! Alerts: held, second-approval and unknown-result payments reach Slack and a signed webhook
//! once; anything no longer waiting is skipped; history is not alerted; a failing destination
//! retries with backoff and gives up without blocking others. Fixture rail; loopback stand-ins;
//! temporary stores.
use std::sync::{Arc, Mutex};

use cosigna_agent::{
    Actor, Error, Service,
    alerts::{Alerts, MAX_ATTEMPTS, signature},
    api::Credentials,
    gateway,
    provider::Scenario,
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;
const KEY: &str = "webhook-signing-secret-for-tests";

/// One received request: its headers (lowercase names) and body.
type Received = (Vec<(String, String)>, String);

/// A loopback receiver that records each request's headers and body and answers `status`.
struct Receiver {
    url: String,
    seen: Arc<Mutex<Vec<Received>>>,
    status: Arc<Mutex<u16>>,
}

impl Receiver {
    fn start() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/hook", server.server_addr());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(Mutex::new(200));
        let (log, answer) = (seen.clone(), status.clone());
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let headers = request
                    .headers()
                    .iter()
                    .map(|h| {
                        (
                            h.field.to_string().to_ascii_lowercase(),
                            h.value.to_string(),
                        )
                    })
                    .collect();
                log.lock().unwrap().push((headers, body));
                let code = *answer.lock().unwrap();
                let _ =
                    request.respond(tiny_http::Response::from_string("ok").with_status_code(code));
            }
        });
        Self { url, seen, status }
    }

    fn bodies(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b.clone())
            .collect()
    }

    fn header(&self, index: usize, name: &str) -> String {
        self.seen.lock().unwrap()[index]
            .0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

struct Lab {
    _directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
    bot: String,
    slack: Receiver,
    webhook: Receiver,
    alerts: Alerts,
}

impl Lab {
    /// An agent named with Slack markup, an automatic limit of USD 10.00, and two people
    /// needed above USD 400.00.
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
        let (slack, webhook) = (Receiver::start(), Receiver::start());
        let config = json!({"schema":"cosigna.alerts/v1","console_url":"https://cosigna.example.com",
            "destinations":[{"kind":"slack","name":"finance-slack","url_env":"SLACK"},
                {"kind":"webhook","name":"ops","url":webhook.url,"secret_env":"SIGNING"}]});
        let slack_url = slack.url.clone();
        let alerts = Alerts::from_json(&config.to_string(), |name| match name {
            "SLACK" => Some(slack_url.clone()),
            "SIGNING" => Some(KEY.to_string()),
            _ => None,
        })
        .unwrap();
        let mut lab = Self {
            _directory: directory,
            service,
            credentials,
            owner,
            budget: String::new(),
            bot: String::new(),
            slack,
            webhook,
            alerts,
        };
        lab.budget = lab
            .call(
                &lab.owner,
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":200000,"per_purchase_minor":70000,
                "merchants":["fixture-cafe"],"expires_at":AT+86400*2,
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
                json!({"name":"<!channel> bot","budget_id":lab.budget,"permissions":["execute"],
                "expires_at":AT+86400,"operations":{"kinds":["refund"],"rails":["fixture"],
                "auto_approve_minor":1000}})
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

    fn owner_actor(&self) -> Actor {
        self.credentials.authenticate(&self.owner).unwrap()
    }

    /// A refund on pay_fixture_2049 (USD 640.00), held for approval.
    fn held(&self, amount: i64, key: &str) -> (String, String) {
        let held = self
            .call(
                &self.bot,
                "POST",
                "/operations",
                json!({"budget_id":self.budget,"kind":"refund","rail":"fixture",
                "source":"pay_fixture_2049","amount_minor":amount,"currency":"USD",
                "reason":"Customer Jane Doe, card 4242","idempotency_key":key,"case_id":null}),
            )
            .unwrap();
        assert_eq!(held["data"]["stage"], "held");
        (
            held["data"]["id"].as_str().unwrap().to_string(),
            held["proposal_digest"].as_str().unwrap().to_string(),
        )
    }

    fn cycle(&self, at: i64) -> Vec<(String, String, &'static str)> {
        self.service.queue_alerts(&self.alerts, at).unwrap();
        self.service
            .deliver_alerts(&self.alerts, at)
            .unwrap()
            .into_iter()
            .map(|d| (d.destination, d.event, d.outcome))
            .collect()
    }
}

#[test]
fn recovery_of_an_unsubmitted_approval_notifies_reviewers_once() {
    let lab = Lab::new();
    let (id, digest) = lab.held(30000, "interrupted-approval");
    assert!(lab.cycle(AT).is_empty());
    lab.service
        .approve_operation(&lab.owner_actor(), &id, &digest, AT)
        .unwrap();
    assert!(lab.cycle(AT + 1).is_empty());
    let mut worker = cosigna_agent::worker::Worker::open(&lab.service).unwrap();
    worker
        .recover_operations(AT + cosigna_agent::worker::RECOVERY_GRACE)
        .unwrap();
    let delivered = lab.cycle(AT + cosigna_agent::worker::RECOVERY_GRACE);
    assert_eq!(delivered.len(), 2);
    assert!(
        delivered
            .iter()
            .all(|(_, event, outcome)| event == "held" && *outcome == "sent")
    );
    assert!(lab.cycle(AT + 100).is_empty());
}

#[test]
fn a_held_refund_alerts_slack_and_a_signed_webhook_once_and_history_is_not_alerted() {
    let lab = Lab::new();
    // Recorded before alerts were ever run: not alerted.
    lab.held(30000, "history");
    assert!(lab.cycle(AT).is_empty());

    let (id, _) = lab.held(30000, "new");
    let delivered = lab.cycle(AT);
    assert_eq!(delivered.len(), 2, "{delivered:?}");
    assert!(
        delivered
            .iter()
            .all(|(_, event, outcome)| *event == "held" && *outcome == "sent")
    );

    let slack = lab.slack.bodies();
    assert_eq!(slack.len(), 1);
    let text = serde_json::from_str::<Value>(&slack[0]).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains("Refund of USD 300.00 is waiting for approval"),
        "{text}"
    );
    assert!(text.contains("above the agent's automatic limit"), "{text}");
    assert!(
        text.contains(&id) && text.contains("https://cosigna.example.com"),
        "{text}"
    );
    // The agent's name cannot ping the channel; the agent's free-text reason is not sent.
    assert!(
        text.contains("&lt;!channel&gt; bot") && !text.contains("<!channel>"),
        "{text}"
    );
    assert!(!text.contains("Jane") && !text.contains("4242"), "{text}");

    let webhook = lab.webhook.bodies();
    assert_eq!(webhook.len(), 1);
    let body: Value = serde_json::from_str(&webhook[0]).unwrap();
    assert_eq!(body["schema"], "cosigna.alert/v1");
    assert_eq!(body["event"], "held");
    assert_eq!(body["operation_id"], id.as_str());
    assert_eq!(body["amount_minor"], 30000);
    assert_eq!(body["rule"], "needs_owner_approval");
    assert!(!webhook[0].contains("Jane"));
    // The signature covers the timestamp and the exact body, with the shared key.
    let timestamp: i64 = lab
        .webhook
        .header(0, "x-cosigna-timestamp")
        .parse()
        .unwrap();
    assert_eq!(timestamp, AT);
    assert_eq!(
        lab.webhook.header(0, "x-cosigna-signature"),
        signature(KEY.as_bytes(), timestamp, webhook[0].as_bytes())
    );

    // Nothing is sent twice.
    assert!(lab.cycle(AT + 60).is_empty());
    assert_eq!(lab.slack.bodies().len(), 1);
    assert_eq!(lab.webhook.bodies().len(), 1);
}

#[test]
fn a_payment_approved_before_its_alert_goes_out_is_skipped() {
    let lab = Lab::new();
    lab.cycle(AT);
    let (id, digest) = lab.held(30000, "quick");
    lab.service.queue_alerts(&lab.alerts, AT).unwrap();
    lab.service
        .approve_operation(&lab.owner_actor(), &id, &digest, AT)
        .unwrap();
    let delivered = lab.service.deliver_alerts(&lab.alerts, AT).unwrap();
    assert_eq!(delivered.len(), 2);
    assert!(delivered.iter().all(|d| d.outcome == "skipped"));
    assert!(lab.slack.bodies().is_empty() && lab.webhook.bodies().is_empty());
}

#[test]
fn a_second_approver_is_asked_for_after_the_first_approval() {
    let lab = Lab::new();
    lab.cycle(AT);
    let (id, digest) = lab.held(45000, "two");
    lab.cycle(AT);
    let first = lab
        .call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":digest}),
        )
        .unwrap();
    assert_eq!(first["data"]["stage"], "held");
    let delivered = lab.cycle(AT + 1);
    assert!(
        delivered
            .iter()
            .any(|(_, e, o)| e == "first_approved" && *o == "sent"),
        "{delivered:?}"
    );
    let text = lab.slack.bodies().last().unwrap().clone();
    assert!(
        text.contains("has one approval and needs a second, different person"),
        "{text}"
    );
}

#[test]
fn an_unknown_result_is_alerted_only_if_still_unknown_after_two_minutes() {
    let lab = Lab::new();
    lab.cycle(AT);
    let owner = lab.owner_actor();
    let send = |key: &str, scenario| {
        let (id, digest) = lab.held(20000, key);
        lab.service
            .approve_operation(&owner, &id, &digest, AT)
            .unwrap();
        lab.service
            .execute_operation(&owner, &id, scenario, AT)
            .unwrap();
        id
    };
    // One stays unknown; the other is settled by a lookup before its alert is due.
    let stuck = send("stuck", Scenario::TimeoutBeforeAccept);
    let settled = send("settled", Scenario::TimeoutAfterAccept);
    // The worker queues new records every second; the wait counts from then.
    lab.service.queue_alerts(&lab.alerts, AT).unwrap();
    lab.service
        .reconcile_operation(&owner, &settled, AT + 30)
        .unwrap();

    let early: Vec<_> = lab
        .cycle(AT + 60)
        .into_iter()
        .filter(|(_, e, _)| e == "unknown")
        .collect();
    assert!(early.is_empty(), "{early:?}");
    let due: Vec<_> = lab
        .cycle(AT + 120)
        .into_iter()
        .filter(|(_, e, _)| e == "unknown")
        .collect();
    assert_eq!(
        due.iter().filter(|(_, _, o)| *o == "sent").count(),
        2,
        "{due:?}"
    );
    assert_eq!(
        due.iter().filter(|(_, _, o)| *o == "skipped").count(),
        2,
        "{due:?}"
    );
    let unknown: Vec<String> = lab
        .slack
        .bodies()
        .into_iter()
        .filter(|b| b.contains("has not confirmed"))
        .collect();
    assert_eq!(unknown.len(), 1);
    assert!(unknown[0].contains(&stuck) && !unknown[0].contains(&settled));
}

#[test]
fn a_failing_destination_backs_off_and_gives_up_without_blocking_the_other() {
    let lab = Lab::new();
    lab.cycle(AT);
    *lab.slack.status.lock().unwrap() = 500;
    lab.held(30000, "down");
    let first = lab.cycle(AT);
    assert!(
        first.contains(&("ops".to_string(), "held".to_string(), "sent")),
        "{first:?}"
    );
    assert!(first.contains(&("finance-slack".to_string(), "held".to_string(), "retrying")));
    // Not again before the backoff: 30 s, then 2, 8 and 32 minutes, then hourly.
    assert!(lab.cycle(AT + 29).is_empty());
    let mut at = AT;
    let mut outcomes = vec!["retrying"];
    for wait in [30, 120, 480, 1920, 3600] {
        at += wait;
        assert!(
            lab.cycle(at - 1).is_empty(),
            "retried early at +{}",
            at - 1 - AT
        );
        outcomes.extend(lab.cycle(at).into_iter().map(|(_, _, o)| o));
    }
    assert_eq!(outcomes.len() as i64, MAX_ATTEMPTS);
    assert_eq!(outcomes.last(), Some(&"failed"));
    assert_eq!(lab.slack.bodies().len() as i64, MAX_ATTEMPTS);
    assert!(lab.cycle(at + 86_400).is_empty());
    assert_eq!(lab.webhook.bodies().len(), 1);
}

#[test]
fn unsafe_or_incomplete_configurations_are_refused_without_echoing_secrets() {
    let secret = |name: &str| match name {
        "SLACK" => Some("https://hooks.slack.com/services/T0/B0/very-secret-path".to_string()),
        "NOT_SLACK" => Some("https://evil.example/collect?secret".to_string()),
        "PLAIN" => Some("http://hooks.slack.com/services/T0/B0/x".to_string()),
        "SHORT" => Some("short".to_string()),
        "SIGNING" => Some(KEY.to_string()),
        _ => None,
    };
    let slack = json!({"kind":"slack","name":"s","url_env":"SLACK"});
    let ok = json!({"schema":"cosigna.alerts/v1","destinations":[slack]});
    assert!(Alerts::from_json(&ok.to_string(), secret).is_ok());
    let refused = [
        json!({"schema":"cosigna.alerts/v2","destinations":[slack]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[slack],"extra":1}),
        json!({"schema":"cosigna.alerts/v1","destinations":[slack, slack]}),
        json!({"schema":"cosigna.alerts/v1","console_url":"http://cosigna.example.com","destinations":[slack]}),
        json!({"schema":"cosigna.alerts/v1","unknown_after_seconds":-1,"destinations":[slack]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"slack","name":"s","url_env":"NOT_SLACK"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"slack","name":"s","url_env":"PLAIN"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"slack","name":"s","url_env":"MISSING"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"webhook","name":"w","url":"http://hooks.example.com/x","secret_env":"SIGNING"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"webhook","name":"w","url":"https://user:pw@hooks.example.com/x","secret_env":"SIGNING"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"webhook","name":"w","url":"https://hooks.example.com/x","secret_env":"SHORT"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"webhook","name":"w","url":"https://hooks.example.com/x","secret_env":"MISSING"}]}),
        json!({"schema":"cosigna.alerts/v1","destinations":[{"kind":"webhook","name":"bad name","url":"https://hooks.example.com/x","secret_env":"SIGNING"}]}),
    ];
    for config in refused {
        let error = Alerts::from_json(&config.to_string(), secret).err();
        let message = format!("{error:?}");
        assert!(error.is_some(), "accepted: {config}");
        assert!(
            !message.contains("very-secret-path") && !message.contains(KEY),
            "{message}"
        );
    }
}
