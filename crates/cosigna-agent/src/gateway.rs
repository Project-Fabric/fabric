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
fn empty(body: &[u8]) -> Result<()> {
    decode::<Empty>(body)?;
    Ok(())
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
    };
    let executor = Actor {
        owner: actor.owner.clone(),
        role: Role::Executor,
    };
    let parts: Vec<_> = path.split('/').collect();
    let data = match (method, parts.as_slice()) {
        ("POST", ["", "gateway", "v1", "budgets"]) if owner => serde_json::to_value(
            service.create_budget(&actor, decode::<BudgetInput>(body)?, at)?,
        )?,
        ("GET", ["", "gateway", "v1", "budgets", id]) => {
            serde_json::to_value(service.balance(&actor, id)?)?
        }
        ("POST", ["", "gateway", "v1", "budgets", id, "revoke"]) if owner => {
            empty(body)?;
            serde_json::to_value(service.revoke_budget(&actor, id)?)?
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
