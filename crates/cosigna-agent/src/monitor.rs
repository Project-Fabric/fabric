//! What the worker watches besides the payments it handles: the evidence chain, whether alerts
//! are getting through, and whether Stripe's refunds agree with the ledger. Problems become
//! findings: recorded in evidence, alerted once while open, listed by `GET /gateway/v1/monitor`.
//!
//! This only reports. No decision, approval or payment reads it, and a monitor failure never
//! stops the worker. It has limits: a stopped worker cannot report itself (use the outbound
//! heartbeat), and someone with write access to the ledger can edit findings like anything else
//! in it.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Actor, Error, Result, Role, Service, alerts::Alerts, evidence, operations::Operation,
    operations::OperationStage, storage, stripe_rail::StripeRail,
};

pub const API_VERSION: &str = "cosigna.monitor/v1";
/// Evidence kinds: a finding opened, and one closed by a later clean check.
pub(crate) const FINDING_EVENT: &str = "gateway.monitor.finding";
const RESOLVED_EVENT: &str = "gateway.monitor.resolved";

pub const EVIDENCE_FAILED: &str = "evidence_failed";
pub const ALERT_DELIVERY_FAILED: &str = "alert_delivery_failed";
pub const UNRECORDED_REFUND: &str = "unrecorded_refund";
pub const REFUND_REVERSED: &str = "refund_reversed";
pub const REFUND_MISSING: &str = "refund_missing";
pub const RECONCILIATION_UNAVAILABLE: &str = "reconciliation_unavailable";

/// Seconds between evidence checks, which read the whole chain.
pub const VERIFY_EVERY: i64 = 900;
/// Seconds between comparisons with the payment system.
pub const RECONCILE_EVERY: i64 = 300;
/// Seconds between the worker's writes of its heartbeat.
const BEAT_EVERY: i64 = 30;
/// A worker not seen for this long is reported as not recently seen.
pub const WORKER_STALE_AFTER: i64 = 120;
/// How far back refunds are read each pass. A refund can fail days after it succeeded.
pub const RECONCILE_LOOKBACK: i64 = 35 * 86_400;
/// Recorded successes this recent may not be listed by the payment system yet.
pub const MISSING_GRACE: i64 = 300;
/// Consecutive failed passes before the payment system counts as unavailable.
pub const RAIL_FAILURES_BEFORE_FINDING: u32 = 3;
const MAX_FINDINGS_LISTED: i64 = 100;
const STRIPE_RAIL: &str = "stripe_test";

/// Ledger version 13. `alert_outbox` is rebuilt because its CHECK names the events it allows.
pub(crate) const SCHEMA: &str = "
    CREATE TABLE alert_outbox_new (
        evidence_seq INTEGER NOT NULL, destination TEXT NOT NULL, operation_id TEXT NOT NULL,
        event TEXT NOT NULL CHECK(event IN ('held','first_approved','unknown','finding')),
        due_at INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
        state TEXT NOT NULL CHECK(state IN ('pending','sent','skipped','failed')),
        updated_at INTEGER NOT NULL, PRIMARY KEY (evidence_seq, destination));
    INSERT INTO alert_outbox_new SELECT evidence_seq,destination,operation_id,event,due_at,attempts,
        state,updated_at FROM alert_outbox;
    DROP TABLE alert_outbox;
    ALTER TABLE alert_outbox_new RENAME TO alert_outbox;
    CREATE INDEX alert_outbox_due ON alert_outbox(state, due_at);
    CREATE TABLE monitor_findings (
        id INTEGER PRIMARY KEY, kind TEXT NOT NULL, subject TEXT NOT NULL, detail TEXT NOT NULL,
        opened_at INTEGER NOT NULL, resolved_at INTEGER);
    CREATE UNIQUE INDEX monitor_findings_open ON monitor_findings(kind, subject)
        WHERE resolved_at IS NULL;
    CREATE TABLE monitor_state (
        id INTEGER PRIMARY KEY CHECK(id = 1), worker_seen_at INTEGER, alerts_configured INTEGER,
        verified_at INTEGER, verified_ok INTEGER, reconcile_from INTEGER, reconciled_at INTEGER);
    INSERT INTO monitor_state(id) VALUES (1);";

/// A problem the monitor found. `detail` holds no credential and no customer data.
#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub kind: String,
    pub subject: String,
    pub detail: Value,
    pub opened_at: i64,
}

/// Opens a finding unless the same one is already open. Returns whether it was new.
pub(crate) fn open_finding(
    ledger: &Path,
    kind: &str,
    subject: &str,
    detail: Value,
    at: i64,
) -> Result<Option<Finding>> {
    let mut db = storage::connect(ledger, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let added = tx.execute(
        "INSERT OR IGNORE INTO monitor_findings(kind,subject,detail,opened_at) VALUES (?1,?2,?3,?4)",
        params![kind, subject, detail.to_string(), at],
    )?;
    if added == 0 {
        return Ok(None);
    }
    evidence::append(
        &tx,
        at,
        json!({"kind": FINDING_EVENT, "finding": kind, "subject": subject, "detail": detail}),
    )?;
    tx.commit()?;
    Ok(Some(Finding {
        kind: kind.into(),
        subject: subject.into(),
        detail,
        opened_at: at,
    }))
}

/// Closes an open finding, so the same problem alerts again if it comes back.
pub(crate) fn resolve_finding(ledger: &Path, kind: &str, subject: &str, at: i64) -> Result<()> {
    let mut db = storage::connect(ledger, false)?;
    // Read first: most passes have nothing to close and should not take the write lock.
    let open: Option<i64> = db
        .query_row(
            "SELECT 1 FROM monitor_findings WHERE kind=?1 AND subject=?2 AND resolved_at IS NULL",
            params![kind, subject],
            |r| r.get(0),
        )
        .optional()?;
    if open.is_none() {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let closed = tx.execute(
        "UPDATE monitor_findings SET resolved_at=?3
         WHERE kind=?1 AND subject=?2 AND resolved_at IS NULL",
        params![kind, subject, at],
    )?;
    if closed > 0 {
        evidence::append(
            &tx,
            at,
            json!({"kind": RESOLVED_EVENT, "finding": kind, "subject": subject}),
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// The worker's schedule. In memory: a restart checks everything again, which only reads.
#[derive(Default)]
pub struct Monitor {
    next_beat: i64,
    next_verify: i64,
    next_reconcile: i64,
    next_ping: i64,
    rail_failures: u32,
}

impl Monitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs whatever is due and returns findings opened by this pass.
    pub fn run(
        &mut self,
        service: &Service,
        alerts: Option<&Alerts>,
        at: i64,
    ) -> Result<Vec<Finding>> {
        service.require_unscoped()?;
        let mut found = Vec::new();
        if at >= self.next_beat {
            self.next_beat = at.saturating_add(BEAT_EVERY);
            storage::connect(&service.ledger, false)?.execute(
                "UPDATE monitor_state SET worker_seen_at=?1,alerts_configured=?2 WHERE id=1",
                params![at, alerts.is_some()],
            )?;
        }
        if at >= self.next_verify {
            self.next_verify = at.saturating_add(VERIFY_EVERY);
            found.extend(verify(service, at)?);
        }
        if at >= self.next_reconcile {
            self.next_reconcile = at.saturating_add(RECONCILE_EVERY);
            if let Some(rail) = service.rails.stripe_test() {
                found.extend(self.reconcile_pass(service, rail, at)?);
            }
        }
        if let Some(alerts) = alerts
            && at >= self.next_ping
            && let Some(every) = alerts.heartbeat_every()
        {
            self.next_ping = at.saturating_add(every);
            // A failed ping is what the outside service exists to notice.
            if !alerts.ping_heartbeat() {
                eprintln!("Cosigna monitor: the heartbeat could not be sent");
            }
        }
        Ok(found)
    }

    fn reconcile_pass(
        &mut self,
        service: &Service,
        rail: &StripeRail,
        at: i64,
    ) -> Result<Vec<Finding>> {
        match reconcile(service, rail, at) {
            Ok((found, true)) => {
                self.rail_failures = 0;
                resolve_finding(&service.ledger, RECONCILIATION_UNAVAILABLE, STRIPE_RAIL, at)?;
                Ok(found)
            }
            Ok((mut found, false)) => {
                found.extend(open_finding(
                    &service.ledger,
                    RECONCILIATION_UNAVAILABLE,
                    STRIPE_RAIL,
                    json!({"reason": "too_many_refunds"}),
                    at,
                )?);
                Ok(found)
            }
            // The ledger itself could not be read or written: not a payment system problem.
            Err(Error::Merchant) => {
                self.rail_failures = self.rail_failures.saturating_add(1);
                if self.rail_failures < RAIL_FAILURES_BEFORE_FINDING {
                    return Ok(Vec::new());
                }
                Ok(open_finding(
                    &service.ledger,
                    RECONCILIATION_UNAVAILABLE,
                    STRIPE_RAIL,
                    json!({"reason": "no_answer", "failed_passes": self.rail_failures}),
                    at,
                )?
                .into_iter()
                .collect())
            }
            Err(error) => Err(error),
        }
    }
}

/// Checks the chain and state. A check that cannot finish counts as failed.
fn verify(service: &Service, at: i64) -> Result<Option<Finding>> {
    let (ok, detail) = match service.verify_evidence() {
        Ok(report) => (
            report.ok(),
            json!({"chain_intact": report.chain_intact, "state_matches": report.state_matches,
                "exceptions": report.exceptions.len()}),
        ),
        Err(_) => (false, json!({"could_not_check": true})),
    };
    storage::connect(&service.ledger, false)?.execute(
        "UPDATE monitor_state SET verified_at=?1,verified_ok=?2 WHERE id=1",
        params![at, ok],
    )?;
    if ok {
        resolve_finding(&service.ledger, EVIDENCE_FAILED, "ledger", at)?;
        return Ok(None);
    }
    open_finding(&service.ledger, EVIDENCE_FAILED, "ledger", detail, at)
}

/// Whether money moved or may have. Only a definite failure counts as no: the rail's status can
/// be empty, and it can gain values this code has never seen.
fn moved(status: &str) -> bool {
    !matches!(status, "failed" | "canceled")
}

fn stripe_operations(db: &Connection) -> Result<Vec<Operation>> {
    let mut statement = db.prepare(
        "SELECT data FROM operations WHERE json_extract(data,'$.request.rail')=?1
         AND json_extract(data,'$.attempt_key') IS NOT NULL",
    )?;
    let rows = statement
        .query_map([STRIPE_RAIL], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.iter()
        .map(|data| Ok(serde_json::from_str(data)?))
        .collect()
}

/// Whether a refund is the one this operation's approved attempt created. Metadata can be set
/// by anyone holding the key, so every approved term has to agree.
fn is_attempt_of(refund: &Value, op: &Operation) -> Result<bool> {
    let Some(proposal) = &op.proposal else {
        return Ok(false);
    };
    Ok(
        refund["metadata"]["cosigna_attempt"].as_str() == op.attempt_key.as_deref()
            && refund["metadata"]["cosigna_proposal_digest"] == proposal.digest()?
            && refund["amount"].as_i64() == Some(proposal.amount_minor)
            && refund["currency"]
                .as_str()
                .map(str::to_ascii_uppercase)
                .as_deref()
                == Some(proposal.currency.as_str())
            && refund["payment_intent"].as_str() == proposal.source.as_deref(),
    )
}

/// Text from the payment system, checked before it enters evidence and alerts.
fn plain(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| {
        (1..=255).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}

/// Compares the rail's refunds with the ledger. Returns findings, and whether the listing was
/// complete. The first pass only records where to start: earlier refunds are not reported.
fn reconcile(service: &Service, rail: &StripeRail, at: i64) -> Result<(Vec<Finding>, bool)> {
    let start: Option<i64> = {
        let db = storage::connect(&service.ledger, false)?;
        db.query_row(
            "SELECT reconcile_from FROM monitor_state WHERE id=1",
            [],
            |r| r.get(0),
        )?
    };
    let Some(start) = start else {
        storage::connect(&service.ledger, false)?.execute(
            "UPDATE monitor_state SET reconcile_from=?1,reconciled_at=?1 WHERE id=1",
            [at],
        )?;
        return Ok((Vec::new(), true));
    };
    let from = start.max(at.saturating_sub(RECONCILE_LOOKBACK));
    // Successes recorded before listing must appear in it. Attempts are matched against the
    // ledger after listing, since an attempt is durable before its refund is created.
    let settled_before = stripe_operations(&storage::connect(&service.ledger, false)?)?;
    let (refunds, complete) = rail.refunds_since(from)?;
    let operations = stripe_operations(&storage::connect(&service.ledger, false)?)?;
    let by_attempt: HashMap<&str, &Operation> = operations
        .iter()
        .filter_map(|op| op.attempt_key.as_deref().map(|key| (key, op)))
        .collect();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for refund in &refunds {
        let Some(id) = plain(&refund["id"]) else {
            return Err(Error::Merchant);
        };
        let status = plain(&refund["status"]).unwrap_or("unknown");
        seen.insert(id.to_string());
        let detail = json!({
            "refund": id,
            "status": status,
            "amount_minor": refund["amount"].as_i64(),
            "currency": plain(&refund["currency"]).map(str::to_ascii_uppercase),
            "source": plain(&refund["payment_intent"]),
        });
        let attempt = refund["metadata"]["cosigna_attempt"].as_str();
        let matched = match attempt.and_then(|key| by_attempt.get(key)) {
            Some(op) if is_attempt_of(refund, op)? => Some(*op),
            _ => None,
        };
        let finding = match matched {
            None if moved(status) => Some((UNRECORDED_REFUND, id.to_string(), detail)),
            None => None,
            Some(op) => {
                let receipt = op.receipt.as_ref();
                match op.stage {
                    OperationStage::Succeeded
                        if receipt.is_some_and(|r| r.rail_reference != id) && moved(status) =>
                    {
                        Some((UNRECORDED_REFUND, id.to_string(), detail))
                    }
                    OperationStage::Succeeded
                        if receipt.is_some_and(|r| r.rail_reference == id) && !moved(status) =>
                    {
                        Some((REFUND_REVERSED, op.id.clone(), detail))
                    }
                    // Still settling: the worker's recovery owns these.
                    OperationStage::Submitting | OperationStage::Unknown => None,
                    OperationStage::Succeeded => None,
                    // The ledger says no money moved, and the rail says it did.
                    _ if moved(status) => Some((UNRECORDED_REFUND, id.to_string(), detail)),
                    _ => None,
                }
            }
        };
        if let Some((kind, subject, detail)) = finding {
            found.extend(open_finding(&service.ledger, kind, &subject, detail, at)?);
        }
    }
    if complete {
        for op in &settled_before {
            let Some(receipt) = op.receipt.as_ref() else {
                continue;
            };
            let in_window = op
                .submitted_at
                .is_some_and(|submitted| submitted >= from && submitted <= at - MISSING_GRACE);
            if op.stage == OperationStage::Succeeded
                && in_window
                && !seen.contains(&receipt.rail_reference)
            {
                found.extend(open_finding(
                    &service.ledger,
                    REFUND_MISSING,
                    &op.id,
                    json!({"refund": receipt.rail_reference, "amount_minor": receipt.amount_minor,
                        "currency": receipt.currency}),
                    at,
                )?);
            }
        }
        storage::connect(&service.ledger, false)?
            .execute("UPDATE monitor_state SET reconciled_at=?1 WHERE id=1", [at])?;
    }
    Ok((found, complete))
}

#[derive(Debug, Serialize)]
pub struct WorkerStatus {
    pub last_seen_at: Option<i64>,
    /// Seen within `WORKER_STALE_AFTER` seconds. False means alerts and recovery may be stopped.
    pub recently_seen: bool,
    pub alerts_configured: bool,
}

#[derive(Debug, Serialize)]
pub struct EvidenceStatus {
    pub last_verified_at: Option<i64>,
    /// Absent until the first check has run.
    pub ok: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ReconciliationStatus {
    /// Refunds from before this time are not compared. Absent until it has started.
    pub from: Option<i64>,
    pub last_completed_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AlertStatus {
    pub pending: i64,
    pub failed: i64,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub at: i64,
    pub worker: WorkerStatus,
    pub evidence: EvidenceStatus,
    pub reconciliation: ReconciliationStatus,
    pub alerts: AlertStatus,
    pub open_findings: i64,
    /// Newest first, at most 100.
    pub findings: Vec<Finding>,
}

impl Service {
    /// What the monitor last saw. Owner only; the figures cover this whole ledger.
    pub fn monitor_status(&self, owner: &Actor, at: i64) -> Result<Status> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let db = storage::connect(&self.ledger, false)?;
        let (seen, configured, verified_at, verified_ok, from, reconciled) = db.query_row(
            "SELECT worker_seen_at,alerts_configured,verified_at,verified_ok,reconcile_from,
             reconciled_at FROM monitor_state WHERE id=1",
            [],
            |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<bool>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<bool>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            },
        )?;
        let count = |state: &str| -> Result<i64> {
            Ok(db.query_row(
                "SELECT COUNT(*) FROM alert_outbox WHERE state=?1",
                [state],
                |r| r.get(0),
            )?)
        };
        let open_findings = db.query_row(
            "SELECT COUNT(*) FROM monitor_findings WHERE resolved_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        let mut statement = db.prepare(
            "SELECT kind,subject,detail,opened_at FROM monitor_findings
             WHERE resolved_at IS NULL ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement
            .query_map([MAX_FINDINGS_LISTED], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let findings = rows
            .into_iter()
            .map(|(kind, subject, detail, opened_at)| {
                Ok(Finding {
                    kind,
                    subject,
                    detail: serde_json::from_str(&detail)?,
                    opened_at,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Status {
            at,
            worker: WorkerStatus {
                last_seen_at: seen,
                recently_seen: seen.is_some_and(|s| at.saturating_sub(s) <= WORKER_STALE_AFTER),
                alerts_configured: configured.unwrap_or(false),
            },
            evidence: EvidenceStatus {
                last_verified_at: verified_at,
                ok: verified_ok,
            },
            reconciliation: ReconciliationStatus {
                from,
                last_completed_at: reconciled,
            },
            alerts: AlertStatus {
                pending: count("pending")?,
                failed: count("failed")?,
            },
            open_findings,
            findings,
        })
    }
}
