//! A restartable, caller-driven pipeline over the authoritative transaction ledger.
//! Each advance performs at most one planning, execution or reconciliation operation.
use crate::{
    Actor, Result, Role, Service, Stage, Task, TaskInput, planner::Planner, provider::Scenario,
};
use serde::Serialize;

pub const API_VERSION: &str = "cosigna.pipeline/v1";

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    ReadyToPlan,
    NeedsInput,
    AwaitingApproval,
    ReadyToExecute,
    Reconciling,
    NeedsReview,
    Confirmed,
    Declined,
    Cancelled,
}

#[derive(Serialize)]
pub struct Pipeline {
    pub schema: &'static str,
    pub simulated_provider: bool,
    pub pipeline_id: String,
    pub agent_id: Option<String>,
    pub status: Status,
    pub next_action: &'static str,
    pub blocked_reason: Option<&'static str>,
    pub approval_digest: Option<String>,
    pub data: Task,
}

pub fn inspect(service: &Service, actor: &Actor, id: &str, at: i64) -> Result<Pipeline> {
    let task = service.task(actor, id)?;
    let balance = service.balance(actor, &task.input.budget_id)?;
    let blocked_reason = if !task.stage.has_attempt() && task.stage != Stage::Cancelled {
        if balance.budget.revoked || balance.budget.terms.expires_at <= at {
            Some("budget_inactive")
        } else if !service.task_agent_active(actor, &task, at)? {
            Some("agent_inactive")
        } else if task
            .quote
            .as_ref()
            .is_some_and(|quote| quote.expires_at <= at)
        {
            Some("quote_expired")
        } else {
            None
        }
    } else {
        None
    };
    let (status, next_action) = if blocked_reason.is_some() {
        (Status::NeedsReview, "review_or_cancel")
    } else {
        match task.stage {
            Stage::Draft => (Status::ReadyToPlan, "advance"),
            Stage::NeedsInput => (Status::NeedsInput, "provide_input"),
            Stage::Quoted => (Status::AwaitingApproval, "owner_approve_quote"),
            Stage::Approved => (Status::ReadyToExecute, "advance"),
            Stage::Submitting | Stage::Unknown => (Status::Reconciling, "advance"),
            Stage::Confirmed => (Status::Confirmed, "none"),
            Stage::Declined => (Status::Declined, "none"),
            Stage::Cancelled => (Status::Cancelled, "none"),
        }
    };
    Ok(Pipeline {
        schema: API_VERSION,
        simulated_provider: true,
        pipeline_id: task.id.clone(),
        agent_id: service.task_agent_id(actor, &task.id)?,
        status,
        next_action,
        blocked_reason,
        approval_digest: task
            .quote
            .as_ref()
            .map(|quote| quote.digest())
            .transpose()?,
        data: task,
    })
}

pub fn start(
    service: &Service,
    actor: &Actor,
    planner: &dyn Planner,
    input: TaskInput,
    at: i64,
) -> Result<Pipeline> {
    let task = service.create_task(actor, input, at)?;
    advance(service, actor, planner, &task.id, at)
}

pub fn advance(
    service: &Service,
    actor: &Actor,
    planner: &dyn Planner,
    id: &str,
    at: i64,
) -> Result<Pipeline> {
    let before = inspect(service, actor, id, at)?;
    // Approval remains a separate owner action. Retries and unclear prompts never
    // turn into authorization or an unbounded model/execution loop.
    match before.status {
        Status::ReadyToPlan => {
            service.plan(actor, id, planner, at)?;
        }
        Status::ReadyToExecute => {
            let executor = Actor {
                owner: actor.owner.clone(),
                role: Role::Executor,
            };
            service.execute(&executor, id, Scenario::Success, at)?;
        }
        Status::Reconciling => {
            service.reconcile(actor, id, at)?;
        }
        _ => return Ok(before),
    }
    inspect(service, actor, id, at)
}

/// A fixture owner test driver supplies approval; the pipeline never approves itself.
pub fn demo() -> Result<serde_json::Value> {
    use crate::{
        BudgetInput,
        gateway::{Permission, Registration},
        planner::FixturePlanner,
    };
    let directory = tempfile::tempdir()?;
    let service = Service::open(directory.path())?;
    let at = crate::now();
    let owner = Actor {
        owner: "synthetic-pipeline-owner".into(),
        role: Role::Owner,
    };
    let budget = service.create_budget(
        &owner,
        BudgetInput {
            currency: "USD".into(),
            limit_minor: 1000,
            per_purchase_minor: 1000,
            merchants: vec!["fixture-cafe".into()],
            expires_at: at + 3600,
            second_approval_above_minor: None,
        },
        at,
    )?;
    let issued = service.register_agent(
        &owner,
        Registration {
            name: "pipeline-agent".into(),
            budget_id: budget.id.clone(),
            permissions: vec![Permission::Plan, Permission::Execute],
            expires_at: at + 1800,
            operations: None,
        },
        at,
    )?;
    let actor = Actor {
        owner: owner.owner.clone(),
        role: Role::Planner,
    };
    let scoped = service.scoped_agent(&issued.agent);
    let started = start(
        &scoped,
        &actor,
        &FixturePlanner,
        TaskInput {
            budget_id: budget.id.clone(),
            request: "Order one oat latte".into(),
            idempotency_key: "pipeline-demo".into(),
            required_features: vec![],
        },
        at,
    )?;
    let waiting = advance(&scoped, &actor, &FixturePlanner, &started.pipeline_id, at)?;
    if waiting.status != Status::AwaitingApproval || service.provider.order_count()? != 0 {
        return Err(crate::Error::Conflict(
            "pipeline must wait for owner approval".into(),
        ));
    }
    service.approve(
        &owner,
        &started.pipeline_id,
        started
            .approval_digest
            .as_deref()
            .ok_or(crate::Error::StoreFormat)?,
        at,
    )?;
    let approved = inspect(&scoped, &actor, &started.pipeline_id, at)?;
    let reopened = Service::open(directory.path())?;
    let grant = reopened.authenticate_agent(&format!("Bearer {}", issued.token), at)?;
    let resumed = reopened.scoped_agent(&grant);
    let completed = advance(&resumed, &actor, &FixturePlanner, &started.pipeline_id, at)?;
    let replay = advance(&resumed, &actor, &FixturePlanner, &started.pipeline_id, at)?;
    Ok(
        serde_json::json!({"schema":"cosigna.pipeline-demo/v1","simulated":true,"planner":"fixture-script (not AI)",
        "approval_source":"synthetic-owner-test-driver","pipeline_id":started.pipeline_id,
        "stages":[started.status,approved.status,completed.status],"replay_status":replay.status,
        "synthetic_orders":reopened.provider.order_count()?,"balance":reopened.balance(&owner,&budget.id)?}),
    )
}
