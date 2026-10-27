//! Tamper-evident record of every gateway decision (ledger version 4).
//!
//! Records use Fabric's evidence record format: the body is compact JSON
//! `{"v":1,"seq":N,"prev":"<hex>","at":"<RFC 3339 UTC>","event":{…}}` and its hash is the
//! lowercase hex BLAKE3 of exactly the body's bytes. Each record is appended in the same
//! SQLite transaction as the state change it describes, so a committed change always has
//! its record. The chain detects edited, removed or reordered records and state changed
//! outside the service; it does not stop someone with write access from rewriting the whole
//! chain consistently. Signing and anchoring are separate, later steps.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{Actor, Budget, Error, Result, Role, Service, Task, storage};

pub const VERIFY_SCHEMA: &str = "cosigna.ledger-evidence/v1";
const RECORD_VERSION: u64 = 1;
const GENESIS_PREV: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const MAX_EXCEPTIONS: usize = 100;

#[derive(Serialize)]
struct Body<'a> {
    v: u64,
    seq: i64,
    prev: &'a str,
    at: String,
    event: Value,
}

pub(crate) const SCHEMA: &str = "CREATE TABLE evidence (
    seq INTEGER PRIMARY KEY CHECK(seq > 0), hash TEXT NOT NULL UNIQUE, body TEXT NOT NULL);
    CREATE TRIGGER evidence_no_update BEFORE UPDATE ON evidence
        BEGIN SELECT RAISE(ABORT, 'evidence is append-only'); END;
    CREATE TRIGGER evidence_no_delete BEFORE DELETE ON evidence
        BEGIN SELECT RAISE(ABORT, 'evidence is append-only'); END;";

fn digest(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// Appends one record. Callers hold an IMMEDIATE transaction, which serializes appends.
pub(crate) fn append(db: &Connection, at: i64, event: Value) -> Result<()> {
    let last: Option<(i64, String)> = db
        .query_row(
            "SELECT seq,hash FROM evidence ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (seq, prev) = last.map_or((1, GENESIS_PREV.to_string()), |(seq, hash)| (seq + 1, hash));
    let body = serde_json::to_string(&Body {
        v: RECORD_VERSION,
        seq,
        prev: &prev,
        at: rfc3339(at),
        event,
    })?;
    db.execute(
        "INSERT INTO evidence(seq,hash,body) VALUES (?1,?2,?3)",
        params![seq, digest(&body), body],
    )?;
    Ok(())
}

/// Who made a change: the authenticated owner and role, and the agent grant if scoped.
pub(crate) fn by(actor: &Actor, agent: Option<&str>, approver: Option<&str>) -> Value {
    let role = match actor.role {
        Role::Owner => "owner",
        Role::Planner => "planner",
        Role::Executor => "executor",
        Role::Approver => "approver",
    };
    let mut by = json!({"owner": actor.owner, "role": role, "agent": agent});
    // Present only for a named approver, so earlier records keep their exact shape.
    if let (Some(approver), Some(map)) = (approver, by.as_object_mut()) {
        map.insert("approver".into(), json!(approver));
    }
    by
}

/// The first record of a ledger: created empty, or upgraded with pre-existing state that has
/// no earlier evidence. Pre-upgrade history is listed as unevidenced, never reconstructed.
pub(crate) fn start(db: &Connection, from_version: i64, at: i64) -> Result<()> {
    let ids = |table: &str| -> Result<Vec<String>> {
        let mut stmt = db.prepare(&format!("SELECT id FROM {table} ORDER BY id"))?;
        Ok(stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?)
    };
    let (budgets, tasks, agents) = (ids("budgets")?, ids("tasks")?, ids("agents")?);
    let event = if budgets.is_empty() && tasks.is_empty() && agents.is_empty() {
        json!({"kind": "gateway.ledger.created", "ledger_version": storage::LEDGER_VERSION})
    } else {
        json!({"kind": "gateway.ledger.upgraded", "from_version": from_version, "ledger_version": storage::LEDGER_VERSION,
            "unevidenced": {"budgets": budgets, "tasks": tasks, "agents": agents}})
    };
    append(db, at, event)
}

fn reservation(db: &Connection, task: &str) -> Result<Value> {
    Ok(db
        .query_row(
            "SELECT amount,state FROM reservations WHERE task_id=?1",
            [task],
            |r| Ok(json!({"amount_minor": r.get::<_, i64>(0)?, "state": r.get::<_, String>(1)?})),
        )
        .optional()?
        .unwrap_or(Value::Null))
}

fn assigned_agent(db: &Connection, task: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT agent_id FROM task_agents WHERE task_id=?1",
            [task],
            |r| r.get(0),
        )
        .optional()?)
}

/// Records a task's state as just written (`data` is the exact stored task JSON).
pub(crate) fn task(
    db: &Connection,
    kind: &str,
    task: &Task,
    data: &str,
    at: i64,
    by: Value,
) -> Result<()> {
    let quote = match &task.quote {
        Some(quote) => json!({"digest": quote.digest()?, "offer": quote.offer.id,
            "merchant": quote.offer.merchant_id, "quantity": quote.quantity,
            "total_minor": quote.total_minor, "currency": quote.offer.currency,
            "expires_at": rfc3339(quote.expires_at)}),
        None => Value::Null,
    };
    let receipt = task.receipt.as_ref().map(|r| {
        json!({"order": r.order_id, "accepted": r.accepted, "total_minor": r.total_minor,
            "currency": r.currency, "simulated": r.simulated})
    });
    append(
        db,
        at,
        json!({"kind": format!("gateway.task.{kind}"), "by": by, "task": task.id,
            "budget": task.input.budget_id, "agent": assigned_agent(db, &task.id)?,
            "version": task.version, "stage": task.stage,
            // The request may hold personal details; the record binds it without repeating it.
            "request_digest": digest(&task.input.request),
            "idempotency_key": task.input.idempotency_key,
            "quote": quote, "approved_digest": task.approved_digest,
            "attempt_key": task.attempt_key, "receipt": receipt,
            "reservation": reservation(db, &task.id)?, "task_digest": digest(data)}),
    )
}

/// Records a budget's terms and status as just written.
pub(crate) fn budget(
    db: &Connection,
    kind: &str,
    budget: &Budget,
    data: &str,
    at: i64,
    by: Value,
) -> Result<()> {
    append(
        db,
        at,
        json!({"kind": format!("gateway.budget.{kind}"), "by": by, "budget": budget.id,
            "owner": budget.owner, "terms": budget.terms, "revoked": budget.revoked,
            "budget_digest": digest(data)}),
    )
}

/// The agent row as evidence sees it. The digest covers the token hash, so a swapped
/// credential is detected, but no token or token hash is ever written into a record.
fn agent_state(db: &Connection, id: &str) -> Result<Option<(Value, String)>> {
    Ok(db
        .query_row(
            "SELECT owner,name,budget_id,permissions,expires_at,revoked,token_hash,operation_grant FROM agents WHERE id=?1",
            [id],
            |r| {
                let (owner, name, budget, permissions, expires, revoked, token): (
                    String, String, String, String, i64, bool, String,
                ) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?);
                let grant: Option<String> = r.get(7)?;
                let mut row = json!([owner, name, budget, permissions, expires, revoked, token]);
                if let (Some(grant), Some(items)) = (&grant, row.as_array_mut()) {
                    items.push(json!(grant));
                }
                Ok((
                    json!({"owner": owner, "name": name, "budget": budget,
                        "permissions": serde_json::from_str::<Value>(&permissions).unwrap_or(Value::Null),
                        "expires_at": rfc3339(expires), "revoked": revoked,
                        "operations": grant.as_deref().map(serde_json::from_str::<Value>).transpose().unwrap_or(None)}),
                    digest(&row.to_string()),
                ))
            },
        )
        .optional()?)
}

pub(crate) fn agent(db: &Connection, kind: &str, id: &str, at: i64, by: Value) -> Result<()> {
    let (state, agent_digest) = agent_state(db, id)?.ok_or(Error::NotFound)?;
    append(
        db,
        at,
        json!({"kind": format!("gateway.agent.{kind}"), "by": by, "agent": id,
            "grant": state, "agent_digest": agent_digest}),
    )
}

fn approver_state(db: &Connection, id: &str) -> Result<Option<(Value, String)>> {
    Ok(db
        .query_row(
            "SELECT owner,name,currency,max_approval_minor,created_at,revoked,token_hash FROM approvers WHERE id=?1",
            [id],
            |r| {
                let (owner, name, currency, max, created, revoked, token): (
                    String, String, String, i64, i64, bool, String,
                ) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?);
                let row = json!([owner, name, currency, max, created, revoked, token]);
                Ok((
                    json!({"owner": owner, "name": name, "currency": currency,
                        "max_approval_minor": max, "created_at": rfc3339(created), "revoked": revoked}),
                    digest(&row.to_string()),
                ))
            },
        )
        .optional()?)
}

/// A named approver registered or revoked by the owner (ledger version 7).
pub(crate) fn approver(db: &Connection, kind: &str, id: &str, at: i64, by: Value) -> Result<()> {
    let (state, approver_digest) = approver_state(db, id)?.ok_or(Error::NotFound)?;
    append(
        db,
        at,
        json!({"kind": format!("gateway.approver.{kind}"), "by": by, "approver": id,
            "terms": state, "approver_digest": approver_digest}),
    )
}

fn operation_hold(db: &Connection, id: &str) -> Result<Value> {
    Ok(db
        .query_row(
            "SELECT amount,state FROM operation_holds WHERE operation_id=?1",
            [id],
            |r| Ok(json!({"amount_minor": r.get::<_, i64>(0)?, "state": r.get::<_, String>(1)?})),
        )
        .optional()?
        .unwrap_or(Value::Null))
}

/// Records a money operation's state as just written (`data` is the exact stored JSON).
pub(crate) fn operation(
    db: &Connection,
    step: &str,
    op: &crate::operations::Operation,
    data: &str,
    at: i64,
    by: Value,
) -> Result<()> {
    let proposal = match &op.proposal {
        Some(p) => json!({"digest": p.digest()?, "facts": p.facts,
            "expires_at": rfc3339(p.expires_at)}),
        None => Value::Null,
    };
    let request = &op.request;
    let mut event = json!({"kind": format!("gateway.operation.{step}"), "by": by, "operation": op.id,
            "budget": request.budget_id, "agent": op.agent, "version": op.version,
            "stage": op.stage, "decision": op.decision,
            "request": {"kind": request.kind, "rail": request.rail, "source": request.source,
                "payee": request.payee, "amount_minor": request.amount_minor,
                "currency": request.currency, "idempotency_key": request.idempotency_key,
                "case_id": request.case_id,
                // The reason may hold personal details; the record binds it without repeating it.
                "reason_digest": digest(&request.reason)},
            "proposal": proposal, "approved_digest": op.approved_digest,
            "approved_by": op.approved_by, "attempt_key": op.attempt_key,
            "receipt": op.receipt, "hold": operation_hold(db, &op.id)?,
            "operation_digest": digest(data)});
    // Present only when two approvals are needed, so earlier records keep their exact shape.
    if let (Some(first), Some(map)) = (&op.first_approval, event.as_object_mut()) {
        map.insert("first_approval".into(), serde_json::to_value(first)?);
    }
    append(db, at, event)
}

/// Owner delegation to the local scheduler (enroll, pause, resume). Not spending authority.
pub(crate) fn worker(db: &Connection, action: &str, task: &str, at: i64, by: Value) -> Result<()> {
    append(
        db,
        at,
        json!({"kind": format!("gateway.worker.{action}"), "by": by, "task": task}),
    )
}

#[derive(Debug, Serialize)]
pub struct Exception {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    pub subject: String,
    pub problem: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Unevidenced {
    pub budgets: usize,
    pub tasks: usize,
    pub agents: usize,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub records: i64,
    pub head: Option<String>,
    /// Every record hashes to its stored hash and links to its predecessor.
    pub chain_intact: bool,
    /// Current budgets, tasks, reservations and agents equal their last recorded state.
    pub state_matches: bool,
    /// State that predates ledger version 4 and so has no records.
    pub unevidenced: Unevidenced,
    pub exceptions: Vec<Exception>,
    pub exceptions_truncated: bool,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.chain_intact && self.state_matches
    }
}

struct Checker {
    exceptions: Vec<Exception>,
    truncated: bool,
}

impl Checker {
    fn flag(&mut self, seq: Option<i64>, subject: impl Into<String>, problem: impl Into<String>) {
        if self.exceptions.len() < MAX_EXCEPTIONS {
            self.exceptions.push(Exception {
                seq,
                subject: subject.into(),
                problem: problem.into(),
            });
        } else {
            self.truncated = true;
        }
    }
}

fn verify_db(db: &Connection) -> Result<Report> {
    let mut check = Checker {
        exceptions: Vec::new(),
        truncated: false,
    };
    let mut chain_intact = true;
    let mut state_matches = true;
    // Last recorded event per subject, and everything listed as predating evidence.
    let mut last: BTreeMap<(&'static str, String), Value> = BTreeMap::new();
    let mut legacy: BTreeSet<(&'static str, String)> = BTreeSet::new();
    let mut records = 0;
    let mut head: Option<String> = None;
    {
        let mut stmt = db.prepare("SELECT seq,hash,body FROM evidence ORDER BY seq")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (seq, hash, body) = row?;
            records += 1;
            let mut broken = |problem: &str| {
                chain_intact = false;
                check.flag(Some(seq), "chain", problem);
            };
            if seq != records {
                broken("record is missing before this one");
            }
            if digest(&body) != hash {
                broken("record body does not match its hash");
            }
            let Ok(parsed) = serde_json::from_str::<Value>(&body) else {
                broken("record body is not JSON");
                head = Some(hash);
                continue;
            };
            if parsed["v"] != RECORD_VERSION || parsed["seq"] != seq {
                broken("record version or sequence field is wrong");
            }
            let expected_prev = head.as_deref().unwrap_or(GENESIS_PREV);
            if parsed["prev"] != expected_prev {
                broken("record does not link to the previous record");
            }
            if seq == 1
                && !matches!(
                    parsed["event"]["kind"].as_str(),
                    Some("gateway.ledger.created" | "gateway.ledger.upgraded")
                )
            {
                broken("chain does not start with a ledger record");
            }
            let event = &parsed["event"];
            if event["kind"] == "gateway.ledger.upgraded" {
                for (kind, list) in [
                    ("budget", "budgets"),
                    ("task", "tasks"),
                    ("agent", "agents"),
                ] {
                    for id in event["unevidenced"][list].as_array().into_iter().flatten() {
                        if let Some(id) = id.as_str() {
                            legacy.insert((kind, id.to_string()));
                        }
                    }
                }
            }
            for (kind, field, digest_field) in [
                ("task", "task", "task_digest"),
                ("budget", "budget", "budget_digest"),
                ("agent", "agent", "agent_digest"),
                ("operation", "operation", "operation_digest"),
                ("approver", "approver", "approver_digest"),
                ("identity", "identity_record", "identity_digest"),
            ] {
                if event.get(digest_field).is_some()
                    && let Some(id) = event[field].as_str()
                {
                    last.insert((kind, id.to_string()), event.clone());
                }
            }
            head = Some(hash);
        }
    }
    let mut unevidenced = Unevidenced::default();
    let mut mismatch = |check: &mut Checker, subject: String, problem: &str| {
        state_matches = false;
        check.flag(None, subject, problem);
    };

    let rows = |sql: &str| -> Result<Vec<(String, String)>> {
        let mut stmt = db.prepare(sql)?;
        Ok(stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?)
    };
    for (kind, sql, digest_field) in [
        (
            "budget",
            "SELECT id,data FROM budgets ORDER BY id",
            "budget_digest",
        ),
        (
            "task",
            "SELECT id,data FROM tasks ORDER BY id",
            "task_digest",
        ),
        (
            "operation",
            "SELECT id,data FROM operations ORDER BY id",
            "operation_digest",
        ),
    ] {
        for (id, data) in rows(sql)? {
            let key = (kind, id.clone());
            match last.get(&key) {
                Some(event) => {
                    if event[digest_field] != digest(&data) {
                        mismatch(
                            &mut check,
                            format!("{kind} {id}"),
                            "changed without a record",
                        );
                    }
                    if kind == "operation" && event["hold"] != operation_hold(db, &id)? {
                        mismatch(
                            &mut check,
                            format!("operation {id}"),
                            "budget hold changed without a record",
                        );
                    }
                    if kind == "task" {
                        if event["reservation"] != reservation(db, &id)? {
                            mismatch(
                                &mut check,
                                format!("task {id}"),
                                "reservation changed without a record",
                            );
                        }
                        if event["agent"] != json!(assigned_agent(db, &id)?) {
                            mismatch(
                                &mut check,
                                format!("task {id}"),
                                "agent assignment changed without a record",
                            );
                        }
                    }
                }
                None if legacy.contains(&key) => {
                    if kind == "budget" {
                        unevidenced.budgets += 1;
                    } else {
                        unevidenced.tasks += 1;
                    }
                }
                None => mismatch(
                    &mut check,
                    format!("{kind} {id}"),
                    "exists without any record",
                ),
            }
        }
    }
    let agent_ids: Vec<String> = {
        let mut stmt = db.prepare("SELECT id FROM agents ORDER BY id")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    for id in &agent_ids {
        let key = ("agent", id.clone());
        match last.get(&key) {
            Some(event) => {
                let current = agent_state(db, id)?.map(|(_, d)| d);
                if event["agent_digest"] != json!(current) {
                    mismatch(
                        &mut check,
                        format!("agent {id}"),
                        "grant changed without a record",
                    );
                }
            }
            None if legacy.contains(&key) => unevidenced.agents += 1,
            None => mismatch(
                &mut check,
                format!("agent {id}"),
                "exists without any record",
            ),
        }
    }
    let approver_ids: Vec<String> = {
        let mut stmt = db.prepare("SELECT id FROM approvers ORDER BY id")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    for id in &approver_ids {
        match last.get(&("approver", id.clone())) {
            Some(event) => {
                let current = approver_state(db, id)?.map(|(_, d)| d);
                if event["approver_digest"] != json!(current) {
                    mismatch(
                        &mut check,
                        format!("approver {id}"),
                        "changed without a record",
                    );
                }
            }
            None => mismatch(
                &mut check,
                format!("approver {id}"),
                "exists without any record",
            ),
        }
    }
    for id in crate::identity::record_ids(db)? {
        match last.get(&("identity", id.clone())) {
            Some(event)
                if event["identity_digest"] == json!(crate::identity::record_digest(db, &id)?) => {}
            _ => mismatch(
                &mut check,
                format!("identity {id}"),
                "identity state changed or exists without a record",
            ),
        }
    }
    // Recorded subjects must still exist: the service never deletes them.
    for (kind, id) in last.keys() {
        if *kind == "identity" {
            if crate::identity::record_digest(db, id)?.is_none() {
                mismatch(
                    &mut check,
                    format!("identity {id}"),
                    "recorded but missing from the ledger",
                );
            }
            continue;
        }
        let table = match *kind {
            "task" => "tasks",
            "budget" => "budgets",
            "operation" => "operations",
            "approver" => "approvers",
            _ => "agents",
        };
        let exists: Option<i64> = db
            .query_row(&format!("SELECT 1 FROM {table} WHERE id=?1"), [id], |r| {
                r.get(0)
            })
            .optional()?;
        if exists.is_none() {
            mismatch(
                &mut check,
                format!("{kind} {id}"),
                "recorded but missing from the ledger",
            );
        }
    }
    if records == 0 {
        chain_intact = false;
        check.flag(None, "chain", "ledger has no evidence records");
    }
    Ok(Report {
        schema: VERIFY_SCHEMA,
        records,
        head,
        chain_intact,
        state_matches,
        unevidenced,
        exceptions: check.exceptions,
        exceptions_truncated: check.truncated,
    })
}

impl Service {
    /// Checks the evidence chain and current state against it. Local ledger access only.
    pub fn verify_evidence(&self) -> Result<Report> {
        self.require_unscoped()?;
        verify_path(&self.ledger)
    }

    /// Writes the chain as Fabric-format `evidence.jsonl`; see [`export`].
    pub fn export_evidence(&self, out: &Path) -> Result<i64> {
        self.require_unscoped()?;
        export_path(&self.ledger, out)
    }
}

fn existing_ledger(directory: &Path) -> Result<std::path::PathBuf> {
    let ledger = directory.join("transactions.sqlite");
    if !ledger.is_file() {
        return Err(Error::NotFound);
    }
    Ok(ledger)
}

/// Verifies the ledger in `directory` without opening its merchant or provider. Upgrades an
/// older ledger to version 4 first (the upgrade lists pre-existing state as unevidenced).
pub fn verify(directory: &Path) -> Result<Report> {
    verify_path(&existing_ledger(directory)?)
}

/// Writes the chain of the ledger in `directory` as Fabric-format `evidence.jsonl`
/// (`<hash> <body>` per line) to a new file; an existing file is never overwritten.
/// Returns the number of records.
pub fn export(directory: &Path, out: &Path) -> Result<i64> {
    export_path(&existing_ledger(directory)?, out)
}

fn verify_path(ledger: &Path) -> Result<Report> {
    let mut db = storage::connect(ledger, false)?;
    // A read transaction gives one consistent snapshot of chain and state.
    let tx = db.transaction()?;
    verify_db(&tx)
}

fn export_path(ledger: &Path, out: &Path) -> Result<i64> {
    {
        let db = storage::connect(ledger, false)?;
        let mut stmt = db.prepare("SELECT hash,body FROM evidence ORDER BY seq")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)?;
        for (hash, body) in &rows {
            writeln!(file, "{hash} {body}")?;
        }
        file.sync_all()?;
        Ok(rows.len() as i64)
    }
}

/// A Unix time as RFC 3339 UTC, second precision (times before 1970 clamp to the epoch).
fn rfc3339(at: i64) -> String {
    let secs = at.max(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::rfc3339;

    #[test]
    fn formats_utc_times() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_800_000_000), "2027-01-15T08:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(-5), "1970-01-01T00:00:00Z");
    }
}
