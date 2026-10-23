//! Opt-in local scheduler. Queue state conveys no spending authority.
use std::fs::{File, OpenOptions};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{Actor, Error, Result, Role, Service, Stage, pipeline, planner::Planner, storage};

pub const API_VERSION: &str = "cosigna.worker/v1";
const MAX_ACTIVE: i64 = 1000;
const MAX_RECONCILES: u32 = 12;

#[derive(Debug, Serialize)]
pub struct Job {
    pub task_id: String,
    pub state: String,
    pub revision: i64,
    pub next_at: i64,
    pub reconcile_count: u32,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub action: Action,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Enroll,
    Pause,
    Resume,
}

fn load(db: &Connection, id: &str) -> Result<Job> {
    db.query_row("SELECT task_id,state,revision,next_at,reconcile_count,reason,updated_at FROM worker_jobs WHERE task_id=?1", [id], |r| {
        Ok(Job { task_id:r.get(0)?, state:r.get(1)?, revision:r.get(2)?, next_at:r.get(3)?, reconcile_count:r.get(4)?, reason:r.get(5)?, updated_at:r.get(6)? })
    }).optional()?.ok_or(Error::NotFound)
}

pub fn inspect(service: &Service, owner: &Actor, id: &str) -> Result<Job> {
    service.require_unscoped()?;
    owner.require(&[Role::Owner])?;
    service.task(owner, id)?;
    load(&storage::connect(&service.ledger, false)?, id)
}

/// Enrollment is explicit owner delegation to the local scheduler, not quote approval.
pub fn control(service: &Service, owner: &Actor, id: &str, action: Action, at: i64) -> Result<Job> {
    service.require_unscoped()?;
    owner.require(&[Role::Owner])?;
    let mut db = storage::connect(&service.ledger, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let task = storage::task(&tx, id)?;
    owner.owns(&task.owner)?;
    let existing = match load(&tx, id) {
        Ok(job) => Some(job),
        Err(Error::NotFound) => None,
        Err(error) => return Err(error),
    };
    match action {
        Action::Enroll if existing.is_some() => {} // Replay never resumes paused work.
        Action::Pause => {
            existing.ok_or(Error::NotFound)?;
            tx.execute("UPDATE worker_jobs SET state='paused',revision=revision+1,reason='owner_paused',updated_at=?2 WHERE task_id=?1 AND state!='done'", params![id,at])?;
        }
        Action::Resume => {
            let job = existing.ok_or(Error::NotFound)?;
            if job.state != "paused" {
                return Err(Error::Conflict("only paused worker jobs can resume".into()));
            }
            capacity(&tx)?;
            tx.execute("UPDATE worker_jobs SET state='pending',revision=revision+1,next_at=?2,reconcile_count=0,reason=NULL,updated_at=?2 WHERE task_id=?1", params![id,at])?;
        }
        Action::Enroll => {
            capacity(&tx)?;
            tx.execute("INSERT INTO worker_jobs(task_id,owner,state,next_at,updated_at) VALUES (?1,?2,'pending',?3,?3)", params![id,owner.owner,at])?;
        }
    }
    let job = load(&tx, id)?;
    tx.commit()?;
    Ok(job)
}

fn capacity(db: &Connection) -> Result<()> {
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM worker_jobs WHERE state IN ('pending','running')",
        [],
        |r| r.get(0),
    )?;
    if count >= MAX_ACTIVE {
        return Err(Error::Conflict("worker queue capacity reached".into()));
    }
    Ok(())
}

/// One scheduler per ledger, held for its entire lifetime. Local disk only.
/// The OS releases this lock on process exit, including a forced termination.
pub struct Worker {
    service: Service,
    _lock: File,
}

impl Worker {
    pub fn open(service: &Service) -> Result<Self> {
        service.require_unscoped()?;
        let path = service.ledger.with_file_name("worker.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        lock.try_lock().map_err(|_| {
            Error::Conflict("a worker already holds this ledger's scheduler lock".into())
        })?;
        let worker = Self {
            service: service.clone(),
            _lock: lock,
        };
        worker.recover()?;
        Ok(worker)
    }

    fn recover(&self) -> Result<()> {
        let mut db = storage::connect(&self.service.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = {
            let mut stmt = tx.prepare("SELECT task_id FROM worker_jobs WHERE state='running'")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for id in ids {
            let task = storage::task(&tx, &id)?;
            // The model might have billed/responded before the process stopped. No
            // automatic inference replay without an operator deliberately resuming.
            let interrupted = task.stage == Stage::Draft;
            tx.execute("UPDATE worker_jobs SET state=?2,reason=?3,revision=revision+1,next_at=?4,updated_at=?4 WHERE task_id=?1", params![id,if interrupted {"paused"} else {"pending"},if interrupted {Some("planning_interrupted")} else {None},crate::now()])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// At most one eligible job and one external operation per tick. Waiting jobs
    /// are polled without inference. Callers pace ticks (CLI: at most one/second).
    pub fn tick(&mut self, planner: &dyn Planner, at: i64) -> Result<Option<Job>> {
        let mut db = storage::connect(&self.service.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let candidate: Option<(String,String)> = tx.query_row("SELECT task_id,owner FROM worker_jobs WHERE state='pending' AND next_at<=?1 ORDER BY next_at,task_id LIMIT 1", [at], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((id, owner_id)) = candidate else {
            return Ok(None);
        };
        tx.execute("UPDATE worker_jobs SET state='running',revision=revision+1,updated_at=?2 WHERE task_id=?1", params![id,at])?;
        let claimed = load(&tx, &id)?;
        tx.commit()?; // Durable before model or payment access.
        let owner = Actor {
            owner: owner_id,
            role: Role::Owner,
        };
        let result = self.step(&owner, &id, planner, at);
        let (state, reason, delay, count) = match result {
            Ok(view) => match view.status {
                pipeline::Status::Confirmed
                | pipeline::Status::Declined
                | pipeline::Status::Cancelled => ("done", None, 0, claimed.reconcile_count),
                pipeline::Status::NeedsInput => {
                    ("paused", Some("needs_input"), 0, claimed.reconcile_count)
                }
                pipeline::Status::NeedsReview => {
                    ("paused", view.blocked_reason, 0, claimed.reconcile_count)
                }
                pipeline::Status::Reconciling => {
                    let count = claimed.reconcile_count.saturating_add(1);
                    if count >= MAX_RECONCILES {
                        ("paused", Some("reconciliation_limit"), 0, count)
                    } else {
                        (
                            "pending",
                            Some("outcome_unknown"),
                            (5_i64 * (1_i64 << count.min(6))).min(300),
                            count,
                        )
                    }
                }
                _ => ("pending", None, 5, claimed.reconcile_count),
            },
            Err(error) => {
                // No raw error/provider/model text enters scheduler records/logs.
                let reason = match error {
                    Error::Planner => "planner_failed",
                    Error::Forbidden => "permission_denied",
                    Error::Merchant => "merchant_unavailable",
                    _ => "operation_failed",
                };
                ("paused", Some(reason), 0, claimed.reconcile_count)
            }
        };
        db.execute("UPDATE worker_jobs SET state=?3,reason=?4,next_at=?5,reconcile_count=?6,updated_at=?7 WHERE task_id=?1 AND revision=?2 AND state='running'", params![id,claimed.revision,state,reason,at.saturating_add(delay),count,at])?;
        // A concurrent owner pause wins over this completion. Pausing does not
        // undo an operation that has already started; core state remains truth.
        Ok(Some(load(&db, &id)?))
    }

    fn step(
        &self,
        owner: &Actor,
        id: &str,
        planner: &dyn Planner,
        at: i64,
    ) -> Result<pipeline::Pipeline> {
        let before = pipeline::inspect(&self.service, owner, id, at)?;
        // Read-only recovery must remain possible after grant revocation/expiry.
        if before.status == pipeline::Status::Reconciling {
            return pipeline::advance(&self.service, owner, planner, id, at);
        }
        if !matches!(
            before.status,
            pipeline::Status::ReadyToPlan | pipeline::Status::ReadyToExecute
        ) {
            return Ok(before);
        }
        if let Some(agent_id) = &before.agent_id {
            let agent = self.service.registered_agent(owner, agent_id)?;
            let scoped = self.service.scoped_agent(&agent);
            let actor = Actor {
                owner: owner.owner.clone(),
                role: Role::Planner,
            };
            // Scoped core operations recheck both permission and revocation.
            pipeline::advance(&scoped, &actor, planner, id, at)
        } else {
            // Explicit enrollment lets the trusted local owner schedule its own
            // unassigned task; no approval is created by enrollment.
            pipeline::advance(&self.service, owner, planner, id, at)
        }
    }
}
