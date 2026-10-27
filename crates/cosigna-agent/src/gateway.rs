//! Headless, loopback development gateway. The model never receives an owner credential.
pub use crate::agents::{IssuedAgent, Permission, RegisteredAgent, Registration};
use crate::{
    Actor, BudgetInput, Error, Quote, Result, Role, Service, TaskInput, api::Credentials,
    planner::Planner, provider::Scenario,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

pub const API_VERSION: &str = "cosigna.gateway/v1";

/// Isolated executable example. The synthetic owner driver approves; the agent cannot.
pub fn demo() -> Result<Value> {
    let directory = tempfile::tempdir()?;
    let service = Service::open(directory.path())?;
    let path = directory.path().join("credentials.json");
    let credentials = Credentials::initialize(&path)?;
    let private: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let owner_auth = format!(
        "Bearer {}",
        private["owner"].as_str().ok_or(Error::StoreFormat)?
    );
    let at = crate::now();
    let call = |auth: &str, path: &str, body: Value| {
        dispatch(
            &service,
            &crate::planner::FixturePlanner,
            &credentials,
            auth,
            "POST",
            &format!("/gateway/v1{path}"),
            &serde_json::to_vec(&body)?,
            at,
        )
    };
    let budget = call(
        &owner_auth,
        "/budgets",
        json!({"currency":"USD","limit_minor":1000,"per_purchase_minor":1000,"merchants":["fixture-cafe"],"expires_at":at+3600}),
    )?;
    let issued = call(
        &owner_auth,
        "/agents",
        json!({"name":"demo-purchase-agent","budget_id":budget["data"]["id"],"permissions":["plan","execute"],"expires_at":at+1800}),
    )?;
    let agent_auth = format!(
        "Bearer {}",
        issued["data"]["token"].as_str().ok_or(Error::StoreFormat)?
    );
    let task = call(
        &agent_auth,
        "/tasks",
        json!({"budget_id":budget["data"]["id"],"request":"Order one oat latte","idempotency_key":"gateway-demo-one"}),
    )?;
    let id = task["data"]["id"].as_str().ok_or(Error::StoreFormat)?;
    let quote = call(&agent_auth, &format!("/tasks/{id}/plan"), json!({}))?;
    let unapproved_blocked = call(&agent_auth, &format!("/tasks/{id}/execute"), json!({})).is_err();
    let self_approval_blocked = call(
        &agent_auth,
        &format!("/tasks/{id}/approve"),
        json!({"quote_digest":quote["approval_digest"]}),
    )
    .is_err();
    call(
        &owner_auth,
        &format!("/tasks/{id}/approve"),
        json!({"quote_digest":quote["approval_digest"]}),
    )?;
    let confirmed = call(&agent_auth, &format!("/tasks/{id}/execute"), json!({}))?;
    let replay = call(&agent_auth, &format!("/tasks/{id}/execute"), json!({}))?;
    let second = call(
        &agent_auth,
        "/tasks",
        json!({"budget_id":budget["data"]["id"],"request":"Order one oat latte","idempotency_key":"gateway-demo-two"}),
    )?;
    let second_id = second["data"]["id"].as_str().ok_or(Error::StoreFormat)?;
    let second_quote = call(&agent_auth, &format!("/tasks/{second_id}/plan"), json!({}))?;
    let budget_blocked = matches!(
        call(
            &owner_auth,
            &format!("/tasks/{second_id}/approve"),
            json!({"quote_digest":second_quote["approval_digest"]})
        ),
        Err(Error::BudgetExceeded)
    );
    let agent_id = issued["data"]["agent"]["id"]
        .as_str()
        .ok_or(Error::StoreFormat)?;
    call(
        &owner_auth,
        &format!("/agents/{agent_id}/revoke"),
        json!({}),
    )?;
    let revoked_blocked =
        call(&agent_auth, &format!("/tasks/{second_id}/plan"), json!({})).is_err();
    let owner = credentials.authenticate(&owner_auth)?;
    let agent_metadata = service.registered_agent(&owner, agent_id)?;
    let balance = service.balance(
        &owner,
        budget["data"]["id"].as_str().ok_or(Error::StoreFormat)?,
    )?;
    Ok(
        json!({"schema":"cosigna.gateway-demo/v1","simulated":true,"planner":"fixture-script (not AI)",
        "approval_source":"synthetic-owner-test-driver","agent":agent_metadata,
        "checks":{"unapproved_execution_blocked":unapproved_blocked,"agent_self_approval_blocked":self_approval_blocked,
            "confirmed":confirmed["data"]["stage"]=="confirmed","replay_preserves_attempt":confirmed["data"]["attempt_key"]==replay["data"]["attempt_key"],
            "over_budget_blocked":budget_blocked,"revoked_agent_blocked":revoked_blocked},
        "synthetic_orders":service.provider.order_count()?,"balance":balance}),
    )
}

pub(crate) enum Principal {
    Owner(Actor),
    Agent(RegisteredAgent),
    Approver(crate::approvers::RegisteredApprover),
}

pub(crate) fn authenticate(
    service: &Service,
    credentials: &Credentials,
    authorization: &str,
    at: i64,
) -> Result<Principal> {
    if let Ok(actor) = credentials.authenticate(authorization) {
        actor.require(&[Role::Owner])?;
        return Ok(Principal::Owner(actor));
    }
    if authorization.starts_with("Bearer approverkey_") {
        return Ok(Principal::Approver(
            service.authenticate_approver(authorization)?,
        ));
    }
    Ok(Principal::Agent(
        service.authenticate_agent(authorization, at)?,
    ))
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    if body.len() > 16_384 {
        return Err(Error::Invalid("body exceeds 16 KiB".into()));
    }
    serde_json::from_slice(body).map_err(|_| Error::Invalid("invalid gateway JSON body".into()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    quote_digest: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationApproval {
    proposal_digest: String,
}

/// The operation envelope: the operation, the digest an approver signs off, and whether a
/// person still has to decide.
fn operation_view(op: &crate::operations::Operation) -> Result<Value> {
    let digest = op
        .proposal
        .as_ref()
        .map(crate::operations::Proposal::digest)
        .transpose()?;
    Ok(
        json!({"schema": crate::operations::API_VERSION, "simulated_rail": op.request.rail == "fixture",
        "proposal_digest": digest,
        "needs_owner": op.stage == crate::operations::OperationStage::Held, "data": op}),
    )
}
fn empty(body: &[u8]) -> Result<()> {
    decode::<Empty>(body)?;
    Ok(())
}

/// A named approver's only routes: who am I, agent names, the operations, and deciding them.
pub(crate) fn approver_routes(
    service: &Service,
    approver: &crate::approvers::RegisteredApprover,
    method: &str,
    path: &str,
    body: &[u8],
    at: i64,
) -> Result<Value> {
    let actor = Actor {
        owner: approver.owner.clone(),
        role: Role::Approver,
    };
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let parts: Vec<_> = path.split('/').collect();
    match (method, parts.as_slice()) {
        ("GET", ["", "gateway", "v1", "approvers", "me"]) => {
            Ok(json!({"schema": crate::approvers::API_VERSION, "data": approver}))
        }
        // Which agent asked, by name only: no grants, budgets or credentials.
        ("GET", ["", "gateway", "v1", "agents"]) => {
            let names: Vec<Value> = service
                .agent_names(&actor)?
                .into_iter()
                .map(|(id, name)| json!({"id": id, "name": name}))
                .collect();
            Ok(json!({"schema": crate::approvers::API_VERSION, "data": names}))
        }
        ("GET", ["", "gateway", "v1", "operations"]) => {
            let stage = match query {
                "" => None,
                "stage=held" => Some(crate::operations::OperationStage::Held),
                "stage=unknown" => Some(crate::operations::OperationStage::Unknown),
                _ => return Err(Error::Invalid("unsupported operation filter".into())),
            };
            let ops = service.operations(&actor, stage)?;
            Ok(json!({"schema": crate::operations::API_VERSION,
                "data": ops.iter().map(operation_view).collect::<Result<Vec<_>>>()?}))
        }
        ("GET", ["", "gateway", "v1", "operations", id]) => {
            operation_view(&service.operation(&actor, id)?)
        }
        ("GET", ["", "gateway", "v1", "operations", id, "timeline"]) => Ok(
            json!({"schema": crate::timeline::API_VERSION, "data": service.timeline(&actor, id)?}),
        ),
        ("POST", ["", "gateway", "v1", "operations", id, "approve"]) => {
            let input: OperationApproval = decode(body)?;
            let op = service.approve_operation(&actor, id, &input.proposal_digest, at)?;
            // A first of two approvals leaves it held; only a complete approval executes.
            if op.stage == crate::operations::OperationStage::Held {
                return operation_view(&op);
            }
            operation_view(&service.execute_operation(&actor, id, Scenario::Success, at)?)
        }
        ("POST", ["", "gateway", "v1", "operations", id, "reject"]) => {
            empty(body)?;
            operation_view(&service.reject_operation(&actor, id, at)?)
        }
        // Purchase quotes: read one and approve its exact digest. Execution stays with the
        // agent or worker, as after an owner's approval.
        ("GET", ["", "gateway", "v1", "tasks", id]) => Ok(
            json!({"schema": API_VERSION, "simulated_provider": true, "data": service.task(&actor, id)?}),
        ),
        ("POST", ["", "gateway", "v1", "tasks", id, "approve"]) => {
            let input: Approval = decode(body)?;
            let task = service.approve(&actor, id, &input.quote_digest, at)?;
            Ok(json!({"schema": API_VERSION, "simulated_provider": true, "data": task}))
        }
        _ => Err(Error::NotFound),
    }
}

/// Authenticates the complete request; caller-supplied fields cannot select identity.
#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    service: &Service,
    planner: &dyn Planner,
    credentials: &Credentials,
    authorization: &str,
    method: &str,
    path: &str,
    body: &[u8],
    at: i64,
) -> Result<Value> {
    dispatch_authenticated(
        service,
        planner,
        authenticate(service, credentials, authorization, at)?,
        method,
        path,
        body,
        at,
    )
}

pub(crate) fn dispatch_authenticated(
    service: &Service,
    planner: &dyn Planner,
    principal: Principal,
    method: &str,
    path: &str,
    body: &[u8],
    at: i64,
) -> Result<Value> {
    let (service, actor, owner) = match principal {
        Principal::Owner(actor) => (service.clone(), actor, true),
        Principal::Agent(agent) => (
            service.scoped_agent(&agent),
            Actor {
                owner: agent.owner,
                role: Role::Planner,
            },
            false,
        ),
        Principal::Approver(approver) => {
            return approver_routes(
                &service.scoped_approver(&approver),
                &approver,
                method,
                path,
                body,
                at,
            );
        }
    };
    if path.starts_with("/gateway/v1/identity/") {
        return crate::identity::admin(&service, &actor, method, path, body, at);
    }
    let executor = Actor {
        owner: actor.owner.clone(),
        role: Role::Executor,
    };
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let parts: Vec<_> = path.split('/').collect();
    let data = match (method, parts.as_slice()) {
        // Money operations. Execution after an approval happens here, on the server, with
        // credentials the agent never holds.
        ("POST", ["", "gateway", "v1", "operations"]) => {
            let op = service.request_operation(&actor, decode(body)?, at)?;
            let op = if op.stage == crate::operations::OperationStage::Approved {
                service.execute_operation(&actor, &op.id, Scenario::Success, at)?
            } else {
                op
            };
            return operation_view(&op);
        }
        ("GET", ["", "gateway", "v1", "operations"]) if owner => {
            let stage = match query {
                "" => None,
                "stage=held" => Some(crate::operations::OperationStage::Held),
                "stage=unknown" => Some(crate::operations::OperationStage::Unknown),
                _ => return Err(Error::Invalid("unsupported operation filter".into())),
            };
            let ops = service.operations(&actor, stage)?;
            return Ok(json!({"schema": crate::operations::API_VERSION,
                "data": ops.iter().map(operation_view).collect::<Result<Vec<_>>>()?}));
        }
        ("GET", ["", "gateway", "v1", "operations", id]) => {
            return operation_view(&service.operation(&actor, id)?);
        }
        ("GET", ["", "gateway", "v1", "operations", id, "timeline"]) if owner => {
            return Ok(
                json!({"schema": crate::timeline::API_VERSION, "data": service.timeline(&actor, id)?}),
            );
        }
        ("POST", ["", "gateway", "v1", "operations", id, "approve"]) if owner => {
            let input: OperationApproval = decode(body)?;
            let op = service.approve_operation(&actor, id, &input.proposal_digest, at)?;
            if op.stage == crate::operations::OperationStage::Held {
                return operation_view(&op);
            }
            return operation_view(&service.execute_operation(
                &actor,
                id,
                Scenario::Success,
                at,
            )?);
        }
        ("POST", ["", "gateway", "v1", "operations", id, "reject"]) if owner => {
            empty(body)?;
            return operation_view(&service.reject_operation(&actor, id, at)?);
        }
        ("POST", ["", "gateway", "v1", "operations", id, "execute"]) => {
            empty(body)?;
            return operation_view(&service.execute_operation(
                &actor,
                id,
                Scenario::Success,
                at,
            )?);
        }
        ("POST", ["", "gateway", "v1", "operations", id, "reconcile"]) => {
            empty(body)?;
            return operation_view(&service.reconcile_operation(&actor, id, at)?);
        }
        ("POST", ["", "gateway", "v1", "operations", id, "resolve"]) if owner => {
            empty(body)?;
            return operation_view(&service.resolve_operation(&actor, id, at)?);
        }
        ("GET", ["", "gateway", "v1", "exposure"]) if owner => {
            return Ok(json!({"schema": crate::exposure::API_VERSION,
                "data": service.exposure(&actor, at)?}));
        }
        ("GET", ["", "gateway", "v1", "pipelines", id, "worker"]) if owner => {
            return Ok(
                json!({"schema":crate::worker::API_VERSION,"simulated_provider":true,
                "data":crate::worker::inspect(&service,&actor,id)?}),
            );
        }
        ("POST", ["", "gateway", "v1", "pipelines", id, "worker"]) if owner => {
            let control: crate::worker::Control = decode(body)?;
            return Ok(
                json!({"schema":crate::worker::API_VERSION,"simulated_provider":true,
                "data":crate::worker::control(&service,&actor,id,control.action,at)?}),
            );
        }
        ("POST", ["", "gateway", "v1", "pipelines"]) => {
            return Ok(serde_json::to_value(crate::pipeline::start(
                &service,
                &actor,
                planner,
                decode::<TaskInput>(body)?,
                at,
            )?)?);
        }
        ("GET", ["", "gateway", "v1", "pipelines", id]) => {
            return Ok(serde_json::to_value(crate::pipeline::inspect(
                &service, &actor, id, at,
            )?)?);
        }
        ("POST", ["", "gateway", "v1", "pipelines", id, "advance"]) => {
            empty(body)?;
            return Ok(serde_json::to_value(crate::pipeline::advance(
                &service, &actor, planner, id, at,
            )?)?);
        }
        ("POST", ["", "gateway", "v1", "budgets"]) if owner => serde_json::to_value(
            service.create_budget(&actor, decode::<BudgetInput>(body)?, at)?,
        )?,
        ("GET", ["", "gateway", "v1", "budgets", id]) => {
            serde_json::to_value(service.balance(&actor, id)?)?
        }
        ("POST", ["", "gateway", "v1", "budgets", id, "revoke"]) if owner => {
            empty(body)?;
            serde_json::to_value(service.revoke_budget_at(&actor, id, at)?)?
        }
        ("GET", ["", "gateway", "v1", "approvers"]) if owner => {
            return Ok(json!({"schema": crate::approvers::API_VERSION,
                "data": service.approvers(&actor)?}));
        }
        ("POST", ["", "gateway", "v1", "approvers"]) if owner => {
            return Ok(json!({"schema": crate::approvers::API_VERSION,
                "data": service.register_approver(&actor, decode(body)?, at)?}));
        }
        ("POST", ["", "gateway", "v1", "approvers", id, "revoke"]) if owner => {
            empty(body)?;
            return Ok(json!({"schema": crate::approvers::API_VERSION,
                "data": service.revoke_approver(&actor, id, at)?}));
        }
        ("GET", ["", "gateway", "v1", "agents"]) if owner => {
            serde_json::to_value(service.registered_agents(&actor)?)?
        }
        ("POST", ["", "gateway", "v1", "agents"]) if owner => serde_json::to_value(
            service.register_agent(&actor, decode::<Registration>(body)?, at)?,
        )?,
        ("GET", ["", "gateway", "v1", "agents", id]) if owner => {
            serde_json::to_value(service.registered_agent(&actor, id)?)?
        }
        ("POST", ["", "gateway", "v1", "agents", id, "revoke"]) if owner => {
            empty(body)?;
            serde_json::to_value(service.revoke_agent(&actor, id, at)?)?
        }
        ("GET", ["", "gateway", "v1", "catalog"]) => {
            let mut offers = service.provider.offers()?;
            if let Some(id) = &service.agent_scope {
                let metadata = service.authenticate_scope(&actor, id, at)?;
                let balance = service.balance(&actor, &metadata.budget_id)?;
                offers.retain(|offer| {
                    offer.currency == balance.budget.terms.currency
                        && balance.budget.terms.merchants.contains(&offer.merchant_id)
                });
            }
            serde_json::to_value(offers)?
        }
        ("POST", ["", "gateway", "v1", "tasks"]) => {
            serde_json::to_value(service.create_task(&actor, decode::<TaskInput>(body)?, at)?)?
        }
        ("GET", ["", "gateway", "v1", "tasks", id]) => {
            serde_json::to_value(service.task(&actor, id)?)?
        }
        ("GET", ["", "gateway", "v1", "tasks", id, "events"]) => {
            serde_json::to_value(service.events(&actor, id)?)?
        }
        ("POST", ["", "gateway", "v1", "tasks", id, "plan"]) => {
            empty(body)?;
            serde_json::to_value(service.plan(&actor, id, planner, at)?)?
        }
        ("POST", ["", "gateway", "v1", "tasks", id, "approve"]) if owner => {
            let input: Approval = decode(body)?;
            serde_json::to_value(service.approve(&actor, id, &input.quote_digest, at)?)?
        }
        ("POST", ["", "gateway", "v1", "tasks", id, "execute"]) => {
            empty(body)?;
            serde_json::to_value(service.execute(&executor, id, Scenario::Success, at)?)?
        }
        ("POST", ["", "gateway", "v1", "tasks", id, "reconcile"]) => {
            empty(body)?;
            serde_json::to_value(service.reconcile(&executor, id, at)?)?
        }
        ("POST", ["", "gateway", "v1", "tasks", id, "resolve"]) if owner => {
            empty(body)?;
            serde_json::to_value(service.resolve(&actor, id, at)?)?
        }
        ("POST", ["", "gateway", "v1", "tasks", id, "cancel"]) if owner => {
            empty(body)?;
            serde_json::to_value(service.cancel(&actor, id, at)?)?
        }
        _ => return Err(Error::NotFound),
    };
    let digest = data
        .get("quote")
        .filter(|q| !q.is_null())
        .map(|q| -> Result<String> { serde_json::from_value::<Quote>(q.clone())?.digest() })
        .transpose()?;
    let agent_id = if data.get("input").is_some() {
        service.task_agent_id(&actor, data["id"].as_str().ok_or(Error::StoreFormat)?)?
    } else {
        None
    };
    Ok(
        json!({"schema":API_VERSION,"simulated_provider":true,"agent_id":agent_id,"approval_digest":digest,"data":data}),
    )
}
