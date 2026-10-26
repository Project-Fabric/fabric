//! Worst-case exposure: the most money an owner's agents can still move without a person.
//!
//! The bound follows from the rules `operations::decide` and `operations::apply_case` enforce,
//! so it holds for any sequence of agent requests while grants, budgets and case records stay
//! as they are now. Owner requests are always held; an agent can only draw on its own budget; a
//! budget never pays out past its limit; an agent limited to refunds that must name a case can
//! only pay what open cases still owe. It is an upper bound, not a forecast: rails refuse more
//! than it counts (a refund needs a payment with that much left to refund).
use std::collections::BTreeMap;

use rusqlite::params;
use serde::Serialize;

use crate::{
    Actor, Budget, Result, Role, Service,
    agents::{Permission, RegisteredAgent},
    operations::{Case, OperationKind},
    storage,
};

pub const API_VERSION: &str = "cosigna.exposure/v1";

/// What the figures assume. Shown with every report so no one reads more into them.
pub const NOTES: [&str; 3] = [
    "Counts only what agents can pay without a person; requests a person approves are excluded.",
    "Holds while grants, budgets and case records stay as they are now. New budgets, agents or cases can raise it.",
    "An upper bound: rails can refuse more than it counts, such as a refund larger than what is left on the payment.",
];

/// One budget as it stands.
pub struct BudgetState {
    pub budget: Budget,
    /// What policy may still approve: the limit less everything held or spent.
    pub available_minor: i64,
    /// Operations already approved (by policy or a person) whose money has not settled.
    pub approved_minor: i64,
}

/// One case and what it still owes after this owner's approved or spent refunds against it.
pub struct CaseState {
    pub case: Case,
    pub remaining_minor: i64,
}

/// What bounds a figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    /// The budget's remaining limit.
    Budget,
    /// What open cases still owe (refunds-only agents that must name a case).
    Cases,
    /// The agent has no money-operation grant.
    NoGrant,
    /// The agent is revoked, expired or cannot execute.
    AgentInactive,
    /// The budget is revoked or expired.
    BudgetInactive,
    /// Nothing fits under the automatic limit, or nothing is left.
    NoAutomaticRoom,
}

#[derive(Debug, Serialize)]
pub struct AgentExposure {
    pub agent_id: String,
    pub name: String,
    pub budget_id: String,
    pub currency: String,
    /// The most this agent alone can move without a person. Agents sharing a budget share it.
    pub exposure_minor: i64,
    /// The largest single payment it can make without a person.
    pub largest_single_minor: i64,
    pub limited_by: Limit,
    /// Whether its refunds must match a case (refunds-only grant with `require_case`).
    pub must_name_case: bool,
    /// For a refunds-only agent that need not name a case: its exposure if it had to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_case_required_minor: Option<i64>,
    /// When this exposure ends at the latest (agent or budget expiry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct BudgetExposure {
    pub budget_id: String,
    pub currency: String,
    pub available_minor: i64,
    pub approved_minor: i64,
    pub exposure_minor: i64,
    pub limited_by: Limit,
    /// Agents that can move this budget's money without a person.
    pub agents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CurrencyExposure {
    pub currency: String,
    /// The most all agents together can still move without a person.
    pub without_a_person_minor: i64,
    /// Already approved, not yet settled: it can move without a further decision.
    pub approved_minor: i64,
    /// What open cases in this currency still owe.
    pub open_cases_minor: i64,
}

#[derive(Debug, Serialize)]
pub struct Exposure {
    pub at: i64,
    pub currencies: Vec<CurrencyExposure>,
    pub budgets: Vec<BudgetExposure>,
    pub agents: Vec<AgentExposure>,
    pub notes: [&'static str; 3],
}

/// Pure computation over one consistent snapshot.
pub fn compute(
    at: i64,
    budgets: &[BudgetState],
    agents: &[RegisteredAgent],
    cases: &[CaseState],
) -> Exposure {
    let mut open_cases: BTreeMap<&str, i64> = BTreeMap::new();
    for state in cases {
        *open_cases.entry(&state.case.currency).or_default() += state.remaining_minor.max(0);
    }
    let room = |currency: &str| open_cases.get(currency).copied().unwrap_or(0);
    let agents: Vec<AgentExposure> = agents
        .iter()
        .filter_map(|agent| {
            let state = budgets.iter().find(|b| b.budget.id == agent.budget_id)?;
            Some(assess(agent, state, at, room(&state.budget.terms.currency)))
        })
        .collect();

    // Per currency: budgets any agent can drain freely, and budgets only case-bound agents can
    // reach, which together can never pay more than the open cases owe.
    let mut totals: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    let mut out = Vec::new();
    for state in budgets {
        let currency = state.budget.terms.currency.clone();
        let able: Vec<&AgentExposure> = agents
            .iter()
            .filter(|a| a.budget_id == state.budget.id && a.exposure_minor > 0)
            .collect();
        let exposure = able.iter().map(|a| a.exposure_minor).max().unwrap_or(0);
        let free = able.iter().any(|a| !a.must_name_case);
        let entry = totals.entry(currency.clone()).or_default();
        if free {
            entry.0 += exposure;
        } else {
            entry.1 += exposure;
        }
        entry.2 += state.approved_minor;
        out.push(BudgetExposure {
            budget_id: state.budget.id.clone(),
            currency,
            available_minor: state.available_minor,
            approved_minor: state.approved_minor,
            exposure_minor: exposure,
            limited_by: match able.is_empty() {
                true => Limit::NoAutomaticRoom,
                false if exposure < state.available_minor => Limit::Cases,
                false => Limit::Budget,
            },
            agents: able.iter().map(|a| a.agent_id.clone()).collect(),
            until: able.iter().filter_map(|a| a.until).max(),
        });
    }
    for currency in open_cases.keys() {
        totals.entry((*currency).to_string()).or_default();
    }
    let currencies = totals
        .into_iter()
        .map(
            |(currency, (free, case_bound, approved))| CurrencyExposure {
                without_a_person_minor: free + case_bound.min(room(&currency)),
                approved_minor: approved,
                open_cases_minor: room(&currency),
                currency,
            },
        )
        .collect();
    Exposure {
        at,
        currencies,
        budgets: out,
        agents,
        notes: NOTES,
    }
}

fn assess(agent: &RegisteredAgent, state: &BudgetState, at: i64, room: i64) -> AgentExposure {
    let budget = &state.budget;
    let available = state.available_minor.max(0);
    let mut out = AgentExposure {
        agent_id: agent.id.clone(),
        name: agent.name.clone(),
        budget_id: budget.id.clone(),
        currency: budget.terms.currency.clone(),
        exposure_minor: 0,
        largest_single_minor: 0,
        limited_by: Limit::NoGrant,
        must_name_case: false,
        if_case_required_minor: None,
        until: None,
    };
    let Some(grant) = &agent.operations else {
        return out;
    };
    let refunds_only = grant.kinds.iter().all(|k| *k == OperationKind::Refund);
    out.must_name_case = refunds_only && grant.require_case;
    if agent.revoked || agent.expires_at <= at || !agent.permissions.contains(&Permission::Execute)
    {
        out.limited_by = Limit::AgentInactive;
        return out;
    }
    if budget.revoked || budget.terms.expires_at <= at {
        out.limited_by = Limit::BudgetInactive;
        return out;
    }
    let case_bound = available.min(room);
    let exposure = if out.must_name_case {
        case_bound
    } else {
        available
    };
    // Above a two-person threshold nothing is approved automatically.
    let largest = grant
        .auto_approve_minor
        .min(budget.terms.per_purchase_minor)
        .min(budget.terms.second_approval_above_minor.unwrap_or(i64::MAX))
        .min(exposure);
    if refunds_only && !grant.require_case {
        out.if_case_required_minor = Some(case_bound);
    }
    if largest < 1 {
        out.limited_by = if out.must_name_case && available > 0 && grant.auto_approve_minor > 0 {
            Limit::Cases
        } else {
            Limit::NoAutomaticRoom
        };
        return out;
    }
    out.exposure_minor = exposure;
    out.largest_single_minor = largest;
    out.limited_by = if exposure < available {
        Limit::Cases
    } else {
        Limit::Budget
    };
    out.until = Some(agent.expires_at.min(budget.terms.expires_at));
    out
}

impl Service {
    /// The owner's worst-case exposure now. Owner only; agents cannot see it.
    pub fn exposure(&self, owner: &Actor, at: i64) -> Result<Exposure> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let cases = self.rails.cases()?;
        let mut db = storage::connect(&self.ledger, false)?;
        // One read transaction: budgets, holds, agents and case commitments from one snapshot.
        let tx = db.transaction()?;
        let mut statement = tx.prepare("SELECT data FROM budgets WHERE owner=?1 ORDER BY id")?;
        let budgets = statement
            .query_map([&owner.owner], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let budgets = budgets
            .iter()
            .map(|data| {
                let budget: Budget = serde_json::from_str(data)?;
                let (held, spent) = storage::totals(&tx, &budget.id)?;
                let approved_minor = tx.query_row(
                    "SELECT COALESCE(SUM(amount),0) FROM operation_holds WHERE budget_id=?1 AND state='held'",
                    params![budget.id],
                    |r| r.get(0),
                )?;
                Ok(BudgetState {
                    available_minor: budget.terms.limit_minor - held - spent,
                    approved_minor,
                    budget,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let agents = crate::agents::owned(&tx, &owner.owner)?;
        let cases = cases
            .into_iter()
            .map(|case| {
                let committed = crate::operations::committed_to_case(&tx, &owner.owner, &case.id)?;
                Ok(CaseState {
                    remaining_minor: (case.owed_minor - committed).max(0),
                    case,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(compute(at, &budgets, &agents, &cases))
    }
}
