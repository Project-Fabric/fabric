use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{
    Actor, Balance, Budget, BudgetInput, Error, Event, Plan, Quote, Receipt, Result, Role, Stage,
    Task, TaskInput, evidence,
    merchant::{HttpTestMerchant, Provider},
    new_id,
    planner::Planner,
    provider::{FixtureProvider, Outcome, Scenario},
    storage,
};

#[derive(Clone)]
pub struct Service {
    pub(crate) ledger: PathBuf,
    pub(crate) agent_scope: Option<String>,
    /// The named approver this service acts for, if any (see `approvers`).
    pub(crate) approver_scope: Option<String>,
    pub provider: Provider,
    pub rails: crate::rails::Rails,
}

impl Service {
    pub fn open(directory: &Path) -> Result<Self> {
        if directory.join("test-merchant-connection.json").exists() {
            return Err(Error::Conflict(
                "this ledger requires its configured HTTP test merchant".into(),
            ));
        }
        std::fs::create_dir_all(directory)?;
        let ledger = directory.join("transactions.sqlite");
        storage::connect(&ledger, false)?;
        let fixture = directory.join("fixture-provider.sqlite");
        Ok(Self {
            ledger,
            agent_scope: None,
            approver_scope: None,
            provider: Provider::Fixture(FixtureProvider::open(&fixture)?),
            rails: crate::rails::Rails::fixture(&fixture)?,
        })
    }

    pub fn open_test_merchant(directory: &Path, url: &str, token: String) -> Result<Self> {
        let provider = HttpTestMerchant::connect(directory, url, token)?;
        let ledger = directory.join("transactions.sqlite");
        storage::connect(&ledger, false)?;
        Ok(Self {
            ledger,
            agent_scope: None,
            approver_scope: None,
            provider: Provider::TestMerchant(provider),
            rails: crate::rails::Rails::none(),
        })
    }

    pub fn create_budget(&self, actor: &Actor, terms: BudgetInput, at: i64) -> Result<Budget> {
        self.require_unscoped()?;
        actor.require(&[Role::Owner])?;
        terms.validate(at)?;
        let budget = Budget {
            id: new_id("budget")?,
            owner: actor.owner.clone(),
            terms,
            revoked: false,
        };
        let data = serde_json::to_string(&budget)?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO budgets(id,owner,data) VALUES (?1,?2,?3)",
            params![budget.id, budget.owner, data],
        )?;
        evidence::budget(&tx, "created", &budget, &data, at, self.by(actor))?;
        tx.commit()?;
        Ok(budget)
    }

    pub fn balance(&self, actor: &Actor, id: &str) -> Result<Balance> {
        actor.require(&[Role::Owner, Role::Planner, Role::Executor])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction()?;
        let budget = storage::budget(&tx, id)?;
        actor.owns(&budget.owner)?;
        self.check_agent_budget(&tx, actor, id, None, crate::now())?;
        let (reserved_minor, spent_minor) = storage::totals(&tx, id)?;
        let available_minor = budget.terms.limit_minor - reserved_minor - spent_minor;
        Ok(Balance {
            budget,
            reserved_minor,
            spent_minor,
            available_minor,
        })
    }

    pub fn revoke_budget(&self, actor: &Actor, id: &str) -> Result<Budget> {
        self.revoke_budget_at(actor, id, crate::now())
    }

    pub fn revoke_budget_at(&self, actor: &Actor, id: &str, at: i64) -> Result<Budget> {
        self.require_unscoped()?;
        actor.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut budget = storage::budget(&tx, id)?;
        actor.owns(&budget.owner)?;
        if budget.revoked {
            return Ok(budget);
        }
        budget.revoked = true;
        let data = serde_json::to_string(&budget)?;
        tx.execute("UPDATE budgets SET data=?1 WHERE id=?2", params![data, id])?;
        evidence::budget(&tx, "revoked", &budget, &data, at, self.by(actor))?;
        // Revocation prevents new submissions. It cannot undo an already claimed request.
        // Pending reservations remain held until cancellation or reconciliation.
        tx.commit()?;
        Ok(budget)
    }

    pub fn create_task(&self, actor: &Actor, input: TaskInput, at: i64) -> Result<Task> {
        actor.require(&[Role::Owner, Role::Planner])?;
        input.validate()?;
        let fingerprint = blake3::hash(&serde_json::to_vec(&input)?)
            .to_hex()
            .to_string();
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.check_agent_budget(
            &tx,
            actor,
            &input.budget_id,
            Some(crate::agents::Permission::Plan),
            at,
        )?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT fingerprint,data FROM tasks WHERE owner=?1 AND idem=?2",
                params![actor.owner, input.idempotency_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((saved, data)) = existing {
            let task: Task = serde_json::from_str(&data)?;
            self.check_agent_task(&tx, actor, &task, None, at)?;
            return if saved == fingerprint {
                Ok(task)
            } else {
                Err(Error::Conflict(
                    "idempotency key was used for a different request".into(),
                ))
            };
        }
        let budget = storage::budget(&tx, &input.budget_id)?;
        actor.owns(&budget.owner)?;
        active(&budget, at)?;
        let task = Task {
            id: new_id("task")?,
            owner: actor.owner.clone(),
            input,
            version: 1,
            stage: Stage::Draft,
            clarification: None,
            quote: None,
            approved_digest: None,
            attempt_key: None,
            receipt: None,
        };
        let data = serde_json::to_string(&task)?;
        tx.execute(
            "INSERT INTO tasks(id,owner,idem,fingerprint,data) VALUES (?1,?2,?3,?4,?5)",
            params![
                task.id,
                task.owner,
                task.input.idempotency_key,
                fingerprint,
                data
            ],
        )?;
        tx.execute(
            "INSERT INTO events(task_id,kind,at) VALUES (?1,'task_created',?2)",
            params![task.id, at],
        )?;
        if let Some(agent) = &self.agent_scope {
            tx.execute(
                "INSERT INTO task_agents(task_id,agent_id) VALUES (?1,?2)",
                params![task.id, agent],
            )?;
        }
        evidence::task(&tx, "task_created", &task, &data, at, self.by(actor))?;
        tx.commit()?;
        Ok(task)
    }

    pub fn task(&self, actor: &Actor, id: &str) -> Result<Task> {
        actor.require(&[Role::Owner, Role::Planner, Role::Executor])?;
        let db = storage::connect(&self.ledger, false)?;
        let task = storage::task(&db, id)?;
        actor.owns(&task.owner)?;
        self.check_agent_task(&db, actor, &task, None, crate::now())?;
        Ok(task)
    }

    pub fn events(&self, actor: &Actor, id: &str) -> Result<Vec<Event>> {
        self.task(actor, id)?;
        storage::events(&storage::connect(&self.ledger, false)?, id)
    }

    pub fn plan(&self, actor: &Actor, id: &str, planner: &dyn Planner, at: i64) -> Result<Task> {
        actor.require(&[Role::Owner, Role::Planner])?;
        // Bound inference concurrency across API and worker processes sharing this
        // local ledger. Acquire before rereading task state; concurrent callers
        // fail with conflict rather than invoke a second model request.
        let planning_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.ledger.with_file_name("planner.lock"))?;
        planning_lock
            .try_lock()
            .map_err(|_| Error::Conflict("another planning operation is in progress".into()))?;
        let task = self.task(actor, id)?;
        self.check_agent_task(
            &storage::connect(&self.ledger, false)?,
            actor,
            &task,
            Some(crate::agents::Permission::Plan),
            at,
        )?;
        // Replays of plan do not repeat a paid request once a quote or attempt exists, unless
        // the quote expired before any attempt: then planning again issues a fresh quote (new
        // digest), releases any hold and needs a new approval.
        let expired = matches!(task.stage, Stage::Quoted | Stage::Approved)
            && task
                .quote
                .as_ref()
                .is_some_and(|quote| quote.expires_at <= at);
        if task.stage != Stage::Draft && task.stage != Stage::NeedsInput && !expired {
            return Ok(task);
        }
        let balance = self.balance(actor, &task.input.budget_id)?;
        active(&balance.budget, at)?;
        let started = std::time::Instant::now();
        let plan = planner.plan(&task.input, &balance.budget, &self.provider.offers()?)?;
        let finished_at =
            at.saturating_add(started.elapsed().as_secs().min(i64::MAX as u64) as i64);
        self.apply_plan(actor, id, task.version, plan, finished_at)
    }

    pub fn apply_plan(
        &self,
        actor: &Actor,
        id: &str,
        expected_version: u64,
        plan: Plan,
        at: i64,
    ) -> Result<Task> {
        actor.require(&[Role::Owner, Role::Planner])?;
        plan.validate()?;
        let offers = self.provider.offers()?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = storage::task(&tx, id)?;
        actor.owns(&task.owner)?;
        self.check_agent_task(&tx, actor, &task, Some(crate::agents::Permission::Plan), at)?;
        if task.version != expected_version
            || task.stage.has_attempt()
            || task.stage == Stage::Cancelled
        {
            return Err(Error::Conflict(
                "task changed or can no longer be planned".into(),
            ));
        }
        let budget = storage::budget(&tx, &task.input.budget_id)?;
        active(&budget, at)?;
        let quote = if let Some(offer_id) = &plan.offer_id {
            let offer = offers
                .into_iter()
                .find(|o| o.id == *offer_id)
                .ok_or(Error::Planner)?;
            let total_minor = offer
                .unit_total_minor
                .checked_mul(i64::from(plan.quantity))
                .ok_or(Error::Planner)?;
            let quote = Quote {
                id: new_id("quote")?,
                task_id: task.id.clone(),
                offer,
                quantity: plan.quantity,
                total_minor,
                issued_at: at,
                expires_at: at.saturating_add(300).min(budget.terms.expires_at),
            };
            validate_quote(&task, &budget, &quote, at)?;
            Some(quote)
        } else {
            None
        };
        storage::release(&tx, id)?;
        task.stage = if quote.is_some() {
            Stage::Quoted
        } else {
            Stage::NeedsInput
        };
        task.quote = quote;
        task.clarification = plan.clarification;
        task.approved_digest = None;
        storage::save(&tx, &mut task, "plan_applied", at, self.by(actor))?;
        tx.commit()?;
        Ok(task)
    }

    pub fn approve(&self, actor: &Actor, id: &str, digest: &str, at: i64) -> Result<Task> {
        self.require_unscoped()?;
        actor.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = storage::task(&tx, id)?;
        actor.owns(&task.owner)?;
        self.check_agent_task(
            &tx,
            actor,
            &task,
            Some(crate::agents::Permission::Execute),
            at,
        )?;
        // A repeat of the approval already recorded (two clicks, a retried request) returns
        // the task as it is, even once payment has started. It changes nothing.
        if task.stage.has_attempt() && task.approved_digest.as_deref() == Some(digest) {
            return Ok(task);
        }
        if !matches!(task.stage, Stage::Quoted | Stage::Approved) {
            return Err(Error::Conflict("task has no approvable quote".into()));
        }
        let quote = task.quote.as_ref().ok_or(Error::StoreFormat)?;
        if quote.digest()? != digest {
            return Err(Error::Conflict("quote digest does not match".into()));
        }
        let budget = storage::budget(&tx, &task.input.budget_id)?;
        validate_quote(&task, &budget, quote, at)?;
        if task.stage == Stage::Approved {
            return Ok(task);
        }
        let (held, spent) = storage::totals(&tx, &budget.id)?;
        if quote.total_minor > budget.terms.limit_minor - held - spent {
            return Err(Error::BudgetExceeded);
        }
        tx.execute(
            "INSERT INTO reservations(task_id,budget_id,amount,state) VALUES (?1,?2,?3,'held')
            ON CONFLICT(task_id) DO UPDATE SET amount=excluded.amount,state='held'",
            params![id, budget.id, quote.total_minor],
        )?;
        task.stage = Stage::Approved;
        task.approved_digest = Some(digest.to_string());
        storage::save(&tx, &mut task, "owner_approved", at, self.by(actor))?;
        tx.commit()?;
        Ok(task)
    }

    /// Persists a single submission claim before any external effect. The boolean is true
    /// only for its first claimant. A crash after this point requires reconciliation.
    pub fn begin_submission(&self, actor: &Actor, id: &str, at: i64) -> Result<(Task, bool)> {
        actor.require(&[Role::Executor])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = storage::task(&tx, id)?;
        actor.owns(&task.owner)?;
        self.check_agent_task(
            &tx,
            actor,
            &task,
            Some(crate::agents::Permission::Execute),
            at,
        )?;
        if task.stage.has_attempt() {
            return Ok((task, false));
        }
        if task.stage != Stage::Approved {
            return Err(Error::Conflict("owner approval is required".into()));
        }
        let quote = task.quote.as_ref().ok_or(Error::StoreFormat)?;
        let budget = storage::budget(&tx, &task.input.budget_id)?;
        validate_quote(&task, &budget, quote, at)?;
        if task.approved_digest.as_deref() != Some(quote.digest()?.as_str()) {
            return Err(Error::StoreFormat);
        }
        let held: Option<i64> = tx
            .query_row(
                "SELECT amount FROM reservations WHERE task_id=?1 AND state='held'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if held != Some(quote.total_minor) {
            return Err(Error::StoreFormat);
        }
        task.attempt_key = Some(new_id("attempt")?);
        task.stage = Stage::Submitting;
        storage::save(&tx, &mut task, "submission_started", at, self.by(actor))?;
        tx.commit()?;
        Ok((task, true))
    }

    pub fn execute(&self, actor: &Actor, id: &str, scenario: Scenario, at: i64) -> Result<Task> {
        let (task, claimed) = self.begin_submission(actor, id, at)?;
        if !claimed {
            return Ok(task);
        }
        let outcome = self
            .provider
            .submit(
                task.attempt_key.as_deref().ok_or(Error::StoreFormat)?,
                task.quote.as_ref().ok_or(Error::StoreFormat)?,
                scenario,
                at,
            )
            .unwrap_or(Outcome::Unknown);
        self.finish(actor, &task, outcome, at)
    }

    pub fn reconcile(&self, actor: &Actor, id: &str, at: i64) -> Result<Task> {
        let task = self.task(actor, id)?;
        if !matches!(task.stage, Stage::Submitting | Stage::Unknown) {
            return Ok(task);
        }
        let key = task.attempt_key.as_deref().ok_or(Error::StoreFormat)?;
        let outcome = match self.provider.lookup(key) {
            Ok(Some(receipt)) => Outcome::Observed(receipt),
            _ => Outcome::Unknown,
        };
        self.finish(actor, &task, outcome, at)
    }

    /// Owner-only settlement of an attempt whose result is unknown. The provider either
    /// reports what happened or records that the attempt can no longer succeed; only then is
    /// the reservation released. If the provider cannot say, the task stays unknown and held.
    pub fn resolve(&self, actor: &Actor, id: &str, at: i64) -> Result<Task> {
        self.require_unscoped()?;
        actor.require(&[Role::Owner])?;
        let task = self.task(actor, id)?;
        if !matches!(task.stage, Stage::Submitting | Stage::Unknown) {
            return Ok(task);
        }
        let key = task.attempt_key.as_deref().ok_or(Error::StoreFormat)?;
        let digest = task.quote.as_ref().ok_or(Error::StoreFormat)?.digest()?;
        let outcome = self.provider.void(key, &digest).unwrap_or(Outcome::Unknown);
        self.finish(actor, &task, outcome, at)
    }

    fn finish(&self, actor: &Actor, submitted: &Task, outcome: Outcome, at: i64) -> Result<Task> {
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = storage::task(&tx, &submitted.id)?;
        actor.owns(&task.owner)?;
        if !matches!(task.stage, Stage::Submitting | Stage::Unknown) {
            return Ok(task);
        }
        if task.attempt_key != submitted.attempt_key {
            return Err(Error::StoreFormat);
        }
        let kind = match outcome {
            Outcome::Observed(receipt) if receipt_matches(&task, &receipt)? => {
                task.stage = if receipt.accepted {
                    Stage::Confirmed
                } else {
                    Stage::Declined
                };
                tx.execute(
                    "UPDATE reservations SET state=?1 WHERE task_id=?2 AND state='held'",
                    params![
                        if receipt.accepted {
                            "spent"
                        } else {
                            "released"
                        },
                        task.id
                    ],
                )?;
                task.receipt = Some(receipt);
                "provider_result"
            }
            Outcome::Rejected => {
                task.stage = Stage::Declined;
                storage::release(&tx, &task.id)?;
                "provider_rejected_quote"
            }
            Outcome::Voided => {
                task.stage = Stage::Declined;
                storage::release(&tx, &task.id)?;
                "provider_voided"
            }
            _ => {
                task.stage = Stage::Unknown;
                "submission_unknown"
            }
        };
        storage::save(&tx, &mut task, kind, at, self.by(actor))?;
        tx.commit()?;
        Ok(task)
    }

    pub fn cancel(&self, actor: &Actor, id: &str, at: i64) -> Result<Task> {
        self.require_unscoped()?;
        actor.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task = storage::task(&tx, id)?;
        actor.owns(&task.owner)?;
        if task.stage.has_attempt() {
            return Err(Error::Conflict(
                "submission must be reconciled; cancellation cannot undo a purchase".into(),
            ));
        }
        if task.stage == Stage::Cancelled {
            return Ok(task);
        }
        storage::release(&tx, id)?;
        task.stage = Stage::Cancelled;
        task.approved_digest = None;
        storage::save(&tx, &mut task, "owner_cancelled", at, self.by(actor))?;
        tx.commit()?;
        Ok(task)
    }
}

impl Service {
    /// The evidence actor for a change made through this (possibly agent-scoped) service.
    pub(crate) fn by(&self, actor: &Actor) -> serde_json::Value {
        evidence::by(
            actor,
            self.agent_scope.as_deref(),
            self.approver_scope.as_deref(),
        )
    }
}

fn active(budget: &Budget, at: i64) -> Result<()> {
    if budget.revoked || budget.terms.expires_at <= at {
        return Err(Error::Conflict("budget is revoked or expired".into()));
    }
    Ok(())
}

/// The error for an expired quote. Its prefix is the worker's pause reason.
pub const QUOTE_EXPIRED: &str =
    "quote_expired: plan the task again for a fresh quote, then approve that";

fn validate_quote(task: &Task, budget: &Budget, quote: &Quote, at: i64) -> Result<()> {
    active(budget, at)?;
    if quote.expires_at <= at {
        return Err(Error::Conflict(QUOTE_EXPIRED.into()));
    }
    if quote.task_id != task.id
        || quote.issued_at > at
        || quote.offer.currency != budget.terms.currency
        || !budget.terms.merchants.contains(&quote.offer.merchant_id)
        || !(1..=10).contains(&quote.quantity)
        || quote.offer.unit_total_minor <= 0
        || quote
            .offer
            .unit_total_minor
            .checked_mul(i64::from(quote.quantity))
            != Some(quote.total_minor)
        || !task
            .input
            .required_features
            .iter()
            .all(|f| quote.offer.features.contains(f))
        || !quote.offer.simulated
    {
        return Err(Error::Conflict(
            "quote is outside the authorized terms".into(),
        ));
    }
    if quote.total_minor > budget.terms.per_purchase_minor {
        return Err(Error::BudgetExceeded);
    }
    Ok(())
}

fn receipt_matches(task: &Task, receipt: &Receipt) -> Result<bool> {
    let quote = task.quote.as_ref().ok_or(Error::StoreFormat)?;
    Ok(
        Some(receipt.attempt_key.as_str()) == task.attempt_key.as_deref()
            && receipt.quote_digest == quote.digest()?
            && receipt.total_minor == quote.total_minor
            && receipt.currency == quote.offer.currency
            && !receipt.order_id.is_empty()
            && receipt.simulated,
    )
}
