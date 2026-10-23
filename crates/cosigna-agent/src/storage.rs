use std::{path::Path, time::Duration};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{Budget, Error, Event, Result, Task};

// Separate stores from Fabric; versions are documented in docs/STORAGE_FORMAT.md.
const LEDGER_APP: i64 = 0x43535458;
const PROVIDER_APP: i64 = 0x43534658;

pub fn connect(path: &Path, provider: bool) -> Result<Connection> {
    let mut db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
    let expected = if provider { PROVIDER_APP } else { LEDGER_APP };
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let app: i64 = tx.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    if app == 0 && version == 0 && count == 0 {
        tx.pragma_update(None, "application_id", expected)?;
        tx.pragma_update(None, "user_version", 1)?;
        if provider {
            tx.execute_batch("CREATE TABLE offers (id TEXT PRIMARY KEY, data TEXT NOT NULL);
                CREATE TABLE orders (attempt_key TEXT PRIMARY KEY, digest TEXT NOT NULL, data TEXT NOT NULL);")?;
        } else {
            tx.execute_batch("CREATE TABLE budgets (id TEXT PRIMARY KEY, owner TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE tasks (id TEXT PRIMARY KEY, owner TEXT NOT NULL, idem TEXT NOT NULL, fingerprint TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(owner, idem));
                CREATE TABLE reservations (task_id TEXT PRIMARY KEY REFERENCES tasks(id), budget_id TEXT NOT NULL REFERENCES budgets(id), amount INTEGER NOT NULL CHECK(amount>0), state TEXT NOT NULL CHECK(state IN ('held','spent','released')));
                CREATE TABLE events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL REFERENCES tasks(id), kind TEXT NOT NULL, at INTEGER NOT NULL);")?;
        }
    } else if app != expected || !(version == 1 || (!provider && (2..=3).contains(&version))) {
        return Err(Error::StoreFormat);
    }
    // Additive, transactional upgrade; existing task JSON and provider format stay v1.
    if !provider && version < 2 {
        tx.execute_batch("CREATE TABLE agents (
            id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL,
            budget_id TEXT NOT NULL REFERENCES budgets(id), permissions TEXT NOT NULL,
            expires_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
            token_hash TEXT NOT NULL UNIQUE);
            CREATE TABLE task_agents (task_id TEXT PRIMARY KEY REFERENCES tasks(id), agent_id TEXT NOT NULL REFERENCES agents(id));
            CREATE TABLE agent_events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, agent_id TEXT NOT NULL REFERENCES agents(id), kind TEXT NOT NULL, at INTEGER NOT NULL);")?;
        tx.pragma_update(None, "user_version", 2)?;
    }
    if !provider && version < 3 {
        tx.execute_batch(
            "CREATE TABLE worker_jobs (
            task_id TEXT PRIMARY KEY REFERENCES tasks(id), owner TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('pending','running','paused','done')),
            revision INTEGER NOT NULL DEFAULT 0, next_at INTEGER NOT NULL,
            reconcile_count INTEGER NOT NULL DEFAULT 0, reason TEXT,
            updated_at INTEGER NOT NULL);
            CREATE INDEX worker_due ON worker_jobs(state,next_at,task_id);",
        )?;
        tx.pragma_update(None, "user_version", 3)?;
    }
    tx.commit()?;
    db.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(db)
}

pub fn budget(db: &Connection, id: &str) -> Result<Budget> {
    let data: String = db
        .query_row("SELECT data FROM budgets WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn task(db: &Connection, id: &str) -> Result<Task> {
    let data: String = db
        .query_row("SELECT data FROM tasks WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn save(tx: &Transaction<'_>, task: &mut Task, kind: &str, at: i64) -> Result<()> {
    task.version += 1;
    tx.execute(
        "UPDATE tasks SET data=?1 WHERE id=?2",
        params![serde_json::to_string(task)?, task.id],
    )?;
    tx.execute(
        "INSERT INTO events(task_id,kind,at) VALUES (?1,?2,?3)",
        params![task.id, kind, at],
    )?;
    Ok(())
}

pub fn totals(db: &Connection, id: &str) -> Result<(i64, i64)> {
    Ok(db.query_row("SELECT COALESCE(SUM(CASE WHEN state='held' THEN amount ELSE 0 END),0), COALESCE(SUM(CASE WHEN state='spent' THEN amount ELSE 0 END),0) FROM reservations WHERE budget_id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?)
}

pub fn release(tx: &Transaction<'_>, task: &str) -> Result<()> {
    tx.execute(
        "UPDATE reservations SET state='released' WHERE task_id=?1 AND state='held'",
        [task],
    )?;
    Ok(())
}

pub fn events(db: &Connection, task_id: &str) -> Result<Vec<Event>> {
    let mut stmt =
        db.prepare("SELECT sequence,kind,at FROM events WHERE task_id=?1 ORDER BY sequence")?;
    Ok(stmt
        .query_map([task_id], |r| {
            Ok(Event {
                sequence: r.get(0)?,
                kind: r.get(1)?,
                at: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?)
}
