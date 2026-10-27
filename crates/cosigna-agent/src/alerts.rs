//! Alerts to people when money is waiting for them: a payment held for approval, waiting for a
//! second approver, or sent with no confirmed result. The worker reads new evidence records,
//! queues one alert per destination in the ledger, and delivers it to Slack or a signed
//! webhook. Alerts never sit on the payment path: a destination that is down delays only its
//! own alerts. Delivery is at least once: a crash between sending and recording can repeat an
//! alert, never lose one. Alerts carry no credential and not the agent's free-text reason.
use std::{path::Path, time::Duration};

use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, Result, Service, storage};

/// The configuration file's schema.
pub const CONFIG_SCHEMA: &str = "cosigna.alerts/v1";
/// The webhook body's schema.
pub const API_VERSION: &str = "cosigna.alert/v1";

pub(crate) const SCHEMA: &str = "
    CREATE TABLE alert_cursor (id INTEGER PRIMARY KEY CHECK(id = 1), seq INTEGER NOT NULL);
    CREATE TABLE alert_outbox (
        evidence_seq INTEGER NOT NULL, destination TEXT NOT NULL, operation_id TEXT NOT NULL,
        event TEXT NOT NULL CHECK(event IN ('held','first_approved','unknown')),
        due_at INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
        state TEXT NOT NULL CHECK(state IN ('pending','sent','skipped','failed')),
        updated_at INTEGER NOT NULL, PRIMARY KEY (evidence_seq, destination));
    CREATE INDEX alert_outbox_due ON alert_outbox(state, due_at);";

/// Attempts per alert and destination before it is marked failed.
pub const MAX_ATTEMPTS: i64 = 6;
/// Alerts delivered per call, so a backlog cannot starve the worker's other jobs.
const BATCH: i64 = 20;
const MAX_CONFIG: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    schema: String,
    /// Where people review operations; linked in every alert.
    #[serde(default)]
    console_url: Option<String>,
    /// How long a result may stay unknown before anyone is alerted (the worker often
    /// confirms it within seconds). Default 120.
    #[serde(default)]
    unknown_after_seconds: Option<i64>,
    destinations: Vec<DestinationConfig>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum DestinationConfig {
    /// A Slack incoming webhook. Its URL is the secret, so it comes from the environment.
    Slack { name: String, url_env: String },
    /// Any HTTPS endpoint, signed with HMAC-SHA256 so the receiver can check the sender.
    Webhook {
        name: String,
        url: String,
        secret_env: String,
    },
}

struct Destination {
    name: String,
    url: String,
    /// The webhook signing key; none for Slack.
    signing: Option<Vec<u8>>,
}

/// Loaded alert settings, with their secrets.
pub struct Alerts {
    console_url: Option<String>,
    unknown_after: i64,
    destinations: Vec<Destination>,
}

/// One delivery attempt's result, for the worker's log.
#[derive(Debug, Serialize)]
pub struct Delivery {
    pub operation_id: String,
    pub event: String,
    pub destination: String,
    /// `sent`, `skipped` (no longer waiting), `retrying` or `failed` (gave up).
    pub outcome: &'static str,
}

fn bad(message: &str) -> Error {
    Error::Invalid(format!("alerts configuration: {message}"))
}

fn name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// An address alerts may go to: HTTPS, or a loopback port for tests. No credentials in it.
fn https_or_loopback(url: &str) -> bool {
    match url.strip_prefix("https://") {
        Some(rest) => rest
            .split('/')
            .next()
            .is_some_and(|host| !host.is_empty() && !host.contains('@')),
        None => url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.split('/').next())
            .and_then(|port| port.parse::<u16>().ok())
            .is_some_and(|port| port > 0),
    }
}

impl Alerts {
    pub fn load(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|_| bad("cannot read the file"))?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(file, MAX_CONFIG + 1), &mut text)
            .map_err(|_| bad("cannot read the file"))?;
        if text.len() as u64 > MAX_CONFIG {
            return Err(bad("the file is larger than 64 KiB"));
        }
        Self::from_json(&text, crate::env_secret)
    }

    /// The configuration from its JSON text, with secrets looked up by `secret` (the
    /// environment, or a test's own values).
    pub fn from_json(text: &str, secret: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let config: ConfigFile =
            serde_json::from_str(text).map_err(|_| bad("invalid JSON or unknown field"))?;
        Self::new(config, secret)
    }

    fn new(config: ConfigFile, secret: impl Fn(&str) -> Option<String>) -> Result<Self> {
        if config.schema != CONFIG_SCHEMA {
            return Err(bad("schema must be cosigna.alerts/v1"));
        }
        if let Some(url) = &config.console_url {
            let local = ["http://127.0.0.1:", "http://localhost:"]
                .iter()
                .any(|p| url.starts_with(p));
            if url.len() > 512 || !(url.starts_with("https://") || local) {
                return Err(bad(
                    "console_url must be https:// (or a local http:// address)",
                ));
            }
        }
        let unknown_after = config.unknown_after_seconds.unwrap_or(120);
        if !(0..=86_400).contains(&unknown_after) {
            return Err(bad("unknown_after_seconds must be between 0 and 86400"));
        }
        if !(1..=10).contains(&config.destinations.len()) {
            return Err(bad("name between 1 and 10 destinations"));
        }
        let mut destinations: Vec<Destination> = Vec::new();
        for destination in config.destinations {
            let (name, url, signing) = match destination {
                DestinationConfig::Slack { name, url_env } => {
                    let url = secret(&url_env)
                        .ok_or_else(|| bad("a Slack destination's url_env is not set"))?;
                    let slack = url.starts_with("https://hooks.slack.com/")
                        || url.starts_with("http://127.0.0.1:");
                    if !slack || !https_or_loopback(&url) {
                        return Err(bad(
                            "a Slack destination must be a https://hooks.slack.com/ URL",
                        ));
                    }
                    (name, url, None)
                }
                DestinationConfig::Webhook {
                    name,
                    url,
                    secret_env,
                } => {
                    if !https_or_loopback(&url) {
                        return Err(bad("a webhook url must be https://"));
                    }
                    let key = secret(&secret_env)
                        .ok_or_else(|| bad("a webhook's secret_env is not set"))?;
                    if key.len() < 16 {
                        return Err(bad("a webhook signing secret needs at least 16 characters"));
                    }
                    (name, url, Some(key.into_bytes()))
                }
            };
            if !name_ok(&name) || destinations.iter().any(|d| d.name == name) {
                return Err(bad(
                    "destination names must be unique: letters, digits, '-' or '_'",
                ));
            }
            destinations.push(Destination { name, url, signing });
        }
        Ok(Self {
            console_url: config.console_url,
            unknown_after,
            destinations,
        })
    }

    /// Names of the configured destinations, for the worker's banner (no secrets).
    pub fn names(&self) -> Vec<&str> {
        self.destinations.iter().map(|d| d.name.as_str()).collect()
    }
}

fn event_of(kind: &str) -> Option<&'static str> {
    match kind {
        "gateway.operation.held" => Some("held"),
        "gateway.operation.first_approved" => Some("first_approved"),
        "gateway.operation.submission_unknown" => Some("unknown"),
        _ => None,
    }
}

impl Service {
    /// Queues alerts for evidence recorded since the last call, one per destination. The
    /// first call on a ledger starts from its current end: history is not alerted.
    pub fn queue_alerts(&self, alerts: &Alerts, at: i64) -> Result<usize> {
        self.require_unscoped()?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let end: i64 = tx.query_row("SELECT COALESCE(MAX(seq),0) FROM evidence", [], |r| {
            r.get(0)
        })?;
        let Some(cursor) = tx
            .query_row("SELECT seq FROM alert_cursor WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        else {
            tx.execute("INSERT INTO alert_cursor(id,seq) VALUES (1,?1)", [end])?;
            tx.commit()?;
            return Ok(0);
        };
        let records = {
            let mut statement =
                tx.prepare("SELECT seq,body FROM evidence WHERE seq>?1 ORDER BY seq LIMIT 500")?;
            statement
                .query_map([cursor], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut queued = 0;
        let mut last = cursor;
        for (seq, body) in records {
            last = seq;
            let record: Value = serde_json::from_str(&body)?;
            let Some(event) = record["event"]["kind"].as_str().and_then(event_of) else {
                continue;
            };
            let Some(operation) = record["event"]["operation"].as_str() else {
                continue;
            };
            let due = if event == "unknown" {
                at.saturating_add(alerts.unknown_after)
            } else {
                at
            };
            for destination in &alerts.destinations {
                queued += tx.execute(
                    "INSERT OR IGNORE INTO alert_outbox
                    (evidence_seq,destination,operation_id,event,due_at,state,updated_at)
                    VALUES (?1,?2,?3,?4,?5,'pending',?6)",
                    params![seq, destination.name, operation, event, due, at],
                )?;
            }
        }
        tx.execute("UPDATE alert_cursor SET seq=?1 WHERE id=1", [last])?;
        tx.commit()?;
        Ok(queued)
    }

    /// Sends due alerts. One that is no longer waiting for anyone (approved, rejected, or
    /// settled in the meantime) is skipped. Failures retry with backoff, up to MAX_ATTEMPTS.
    pub fn deliver_alerts(&self, alerts: &Alerts, at: i64) -> Result<Vec<Delivery>> {
        self.require_unscoped()?;
        let db = storage::connect(&self.ledger, false)?;
        let due = {
            let mut statement = db.prepare(
                "SELECT evidence_seq,destination,operation_id,event,attempts FROM alert_outbox
                WHERE state='pending' AND due_at<=?1 ORDER BY due_at,evidence_seq LIMIT ?2",
            )?;
            statement
                .query_map(params![at, BATCH], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut deliveries = Vec::new();
        for (seq, name, operation, event, attempts) in due {
            let mark = |state: &str, attempts: i64, due_at: i64| -> Result<()> {
                db.execute(
                    "UPDATE alert_outbox SET state=?3,attempts=?4,due_at=?5,updated_at=?6
                    WHERE evidence_seq=?1 AND destination=?2",
                    params![seq, name, state, attempts, due_at, at],
                )?;
                Ok(())
            };
            let outcome = match alerts.destinations.iter().find(|d| d.name == name) {
                // A destination removed from the configuration: nothing to send it to.
                None => {
                    mark("skipped", attempts, at)?;
                    "skipped"
                }
                Some(destination) => {
                    match message(&db, alerts, seq, &operation, &event)? {
                        None => {
                            mark("skipped", attempts, at)?;
                            "skipped"
                        }
                        Some(message) => {
                            if send(destination, &message, at) {
                                mark("sent", attempts + 1, at)?;
                                "sent"
                            } else if attempts + 1 >= MAX_ATTEMPTS {
                                mark("failed", attempts + 1, at)?;
                                "failed"
                            } else {
                                // 30 s, 2 min, 8 min, 32 min, then hourly.
                                let wait = (30_i64 << (2 * attempts.min(5))).min(3600);
                                mark("pending", attempts + 1, at.saturating_add(wait))?;
                                "retrying"
                            }
                        }
                    }
                }
            };
            deliveries.push(Delivery {
                operation_id: operation,
                event,
                destination: name,
                outcome,
            });
        }
        Ok(deliveries)
    }
}

struct Message {
    text: String,
    body: Value,
}

fn money(minor: i64, currency: &str) -> String {
    format!("{currency} {}.{:02}", minor / 100, (minor % 100).abs())
}

/// The alert for an evidence record, or none if the operation is no longer waiting.
fn message(
    db: &Connection,
    alerts: &Alerts,
    seq: i64,
    operation: &str,
    event: &str,
) -> Result<Option<Message>> {
    let stage: Option<String> = db
        .query_row(
            "SELECT json_extract(data,'$.stage') FROM operations WHERE id=?1",
            [operation],
            |r| r.get(0),
        )
        .optional()?;
    let waiting = matches!(
        (event, stage.as_deref()),
        ("held" | "first_approved", Some("held")) | ("unknown", Some("unknown" | "submitting"))
    );
    if !waiting {
        return Ok(None);
    }
    let body: String = db.query_row("SELECT body FROM evidence WHERE seq=?1", [seq], |r| {
        r.get(0)
    })?;
    let record: Value = serde_json::from_str(&body)?;
    let e = &record["event"];
    let request = &e["request"];
    let kind = match request["kind"].as_str() {
        Some("payout") => "Payout",
        _ => "Refund",
    };
    let amount = money(
        request["amount_minor"].as_i64().unwrap_or(0),
        request["currency"].as_str().unwrap_or("?"),
    );
    let rule = e["decision"]["rule"].as_str().unwrap_or("");
    let agent_id = e["agent"].as_str();
    let agent = match agent_id {
        Some(id) => db
            .query_row("SELECT name FROM agents WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?,
        None => None,
    };
    let case = request["case_id"].as_str();
    let headline = match event {
        "held" => format!(
            "{kind} of {amount} is waiting for approval: {}.",
            crate::timeline::reason(rule)
        ),
        "first_approved" => {
            format!("{kind} of {amount} has one approval and needs a second, different person.")
        }
        _ => format!(
            "{kind} of {amount} was sent, but the payment system has not confirmed it. The money \
             stays held and the payment will not be sent again."
        ),
    };
    let mut details = Vec::new();
    if let Some(agent) = &agent {
        details.push(format!("Agent: {agent}."));
    }
    if let Some(case) = case {
        details.push(format!("Case: {case}."));
    }
    details.push(format!("Operation: {operation}."));
    if let Some(url) = &alerts.console_url {
        details.push(format!("Review: {url}"));
    }
    let text = format!("{headline} {}", details.join(" "));
    let body = json!({
        "schema": API_VERSION,
        "event": event,
        "operation_id": operation,
        "kind": kind.to_ascii_lowercase(),
        "amount_minor": request["amount_minor"],
        "currency": request["currency"],
        "rule": rule,
        "reason": if event == "held" { Some(crate::timeline::reason(rule)) } else { None },
        "agent": agent,
        "case_id": case,
        "recorded_at": record["at"],
        "evidence_seq": seq,
        "console_url": alerts.console_url,
        "text": text,
    });
    Ok(Some(Message { text, body }))
}

/// Slack treats `&`, `<` and `>` as markup (`<!channel>`, links); names and case ids come from
/// owners and agents, so they are shown as text.
fn slack_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `v1=` and the HMAC-SHA256 of `<timestamp>.<body>`, as hex.
pub fn signature(key: &[u8], timestamp: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("v1={}", hex(&mac.finalize().into_bytes()))
}

fn send(destination: &Destination, message: &Message, at: i64) -> bool {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let result = match &destination.signing {
        None => agent
            .post(&destination.url)
            .send_json(json!({"text": slack_escape(&message.text)})),
        Some(key) => {
            let body = message.body.to_string();
            agent
                .post(&destination.url)
                .header("Content-Type", "application/json")
                .header("X-Cosigna-Timestamp", at.to_string())
                .header("X-Cosigna-Signature", signature(key, at, body.as_bytes()))
                .send(body)
        }
    };
    result.is_ok_and(|response| response.status().is_success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_cover_the_timestamp_and_body() {
        let key = b"0123456789abcdef";
        let one = signature(key, 1_800_000_000, b"{}");
        assert!(one.starts_with("v1=") && one.len() == 3 + 64);
        assert_ne!(one, signature(key, 1_800_000_001, b"{}"));
        assert_ne!(one, signature(key, 1_800_000_000, b"{ }"));
        assert_ne!(one, signature(b"0123456789abcdeX", 1_800_000_000, b"{}"));
        assert_eq!(one, signature(key, 1_800_000_000, b"{}"));
    }

    #[test]
    fn slack_markup_from_names_is_shown_as_text() {
        assert_eq!(
            slack_escape("<!channel> & <https://evil.example|click>"),
            "&lt;!channel&gt; &amp; &lt;https://evil.example|click&gt;"
        );
    }
}
