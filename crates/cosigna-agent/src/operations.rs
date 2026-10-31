//! Money operations: refunds and payouts requested by agents or owners.
//!
//! A request is checked by its rail against the rail's own records, becomes an exact
//! digest-bound proposal, and is decided by policy: denied, approved automatically within the
//! agent's grant, or held for the owner. Approval holds budget atomically; execution claims a
//! single attempt before any external effect; unknown results stay held until reconciled or
//! resolved through the rail. Every step is recorded as evidence.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Actor, Budget, Error, MAX_MINOR, Result, Role, Service,
    agents::Permission,
    evidence, new_id,
    provider::Scenario,
    rails::{Check, RailOutcome},
    storage,
};

pub const API_VERSION: &str = "cosigna.operation/v1";
/// How long a held proposal can wait for approval and execution.
const PROPOSAL_LIFETIME: i64 = 24 * 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Refund,
    Payout,
}

/// What an agent may do with money operations. Absent on an agent grants nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationGrant {
    pub kinds: Vec<OperationKind>,
    pub rails: Vec<String>,
    /// Operations up to this amount are approved without the owner; above it they are held.
    pub auto_approve_minor: i64,
    /// Refunds must name a case (what the customer is owed, from the business's records).
    /// Absent in grants written before ledger version 6, which means false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_case: bool,
}

impl OperationGrant {
    pub(crate) fn validate(&self) -> Result<()> {
        let mut kinds = self.kinds.clone();
        kinds.dedup();
        if self.kinds.is_empty()
            || self.kinds.len() > 2
            || kinds.len() != self.kinds.len()
            || (self.kinds.len() == 2 && self.kinds[0] == self.kinds[1])
            || self.rails.is_empty()
            || self.rails.len() > 10
            || self.rails.iter().any(|rail| !identifier(rail, 40))
            || !(0..=MAX_MINOR).contains(&self.auto_approve_minor)
        {
            return Err(Error::Invalid("invalid operation grant".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub budget_id: String,
    pub kind: OperationKind,
    pub rail: String,
    /// The payment being refunded (refunds).
    #[serde(default)]
    pub source: Option<String>,
    /// The registered destination (payouts).
    #[serde(default)]
    pub payee: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    pub reason: String,
    pub idempotency_key: String,
    /// The case this refund settles: the business's own record of what the customer is owed.
    /// When present, the refund must match it (payment, customer, currency, amount still owed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_id: Option<String>,
}

impl OperationRequest {
    fn validate(&self) -> Result<()> {
        let reference_ok = match self.kind {
            OperationKind::Refund => {
                self.source.as_deref().is_some_and(|s| identifier(s, 100)) && self.payee.is_none()
            }
            OperationKind::Payout => {
                self.payee.as_deref().is_some_and(|s| identifier(s, 100)) && self.source.is_none()
            }
        };
        let case_ok = match (&self.case_id, self.kind) {
            (None, _) => true,
            (Some(case), OperationKind::Refund) => identifier(case, 100),
            (Some(_), OperationKind::Payout) => false,
        };
        if !reference_ok
            || !case_ok
            || !identifier(&self.rail, 40)
            || !(1..=MAX_MINOR).contains(&self.amount_minor)
            || self.currency.len() != 3
            || !self.currency.bytes().all(|b| b.is_ascii_uppercase())
            || self.reason.trim().is_empty()
            || self.reason.len() > 500
            || self.reason.chars().any(char::is_control)
            || self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 128
            || !self.idempotency_key.is_ascii()
        {
            return Err(Error::Invalid("invalid operation fields".into()));
        }
        Ok(())
    }
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The exact operation that is approved and executed. Its digest binds every approval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub operation_id: String,
    pub kind: OperationKind,
    pub rail: String,
    pub source: Option<String>,
    pub payee: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    /// What the rail verified about the request when it was made.
    pub facts: Value,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl Proposal {
    pub fn digest(&self) -> Result<String> {
        Ok(blake3::hash(&serde_json::to_vec(self)?)
            .to_hex()
            .to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "rule")]
pub enum Decision {
    AutoApproved(String),
    Held(String),
    Denied(String),
}

impl Decision {
    /// The rule that decided it.
    pub fn rule(&self) -> &str {
        match self {
            Self::AutoApproved(rule) | Self::Held(rule) | Self::Denied(rule) => rule,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    /// Waiting for the owner.
    Held,
    Approved,
    Submitting,
    Unknown,
    Succeeded,
    /// The rail declined, rejected or voided the attempt; no money moved.
    Failed,
    Denied,
    /// Declined by the owner before submission.
    Rejected,
}

impl OperationStage {
    pub fn has_attempt(self) -> bool {
        matches!(
            self,
            Self::Submitting | Self::Unknown | Self::Succeeded | Self::Failed
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationReceipt {
    pub attempt_key: String,
    pub proposal_digest: String,
    pub rail_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub succeeded: bool,
    pub simulated: bool,
}

/// The first person's approval of an operation that needs two. Bound to the same digest the
/// second person must approve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstApproval {
    /// `owner` or `approver`.
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
    pub digest: String,
    pub at: i64,
}

impl FirstApproval {
    fn person(&self) -> &str {
        self.approver.as_deref().unwrap_or("owner")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub owner: String,
    pub agent: Option<String>,
    pub request: OperationRequest,
    pub version: u64,
    pub stage: OperationStage,
    pub proposal: Option<Proposal>,
    pub decision: Decision,
    pub approved_digest: Option<String>,
    /// `policy` for an automatic approval, `owner` or `approver` for a person's.
    pub approved_by: Option<String>,
    /// The named approver who approved it (ledger version 7); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
    /// The first of two required approvals (ledger version 8); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_approval: Option<FirstApproval>,
    pub attempt_key: Option<String>,
    /// When the attempt was claimed (bounds idempotent retries on some rails).
    #[serde(default)]
    pub submitted_at: Option<i64>,
    pub receipt: Option<OperationReceipt>,
}

/// Pure policy. Order matters: grants, then hard limits, then the agent's automatic limit.
pub fn decide(
    request: &OperationRequest,
    budget: &Budget,
    available_minor: i64,
    agent_grant: Option<Option<&OperationGrant>>,
) -> Decision {
    let denied = |rule: &str| Decision::Denied(rule.into());
    if let Some(grant) = agent_grant {
        match grant {
            Some(grant)
                if grant.kinds.contains(&request.kind) && grant.rails.contains(&request.rail) => {}
            _ => return denied("not_granted"),
        }
    }
    if request.currency != budget.terms.currency {
        return denied("currency_not_in_budget");
    }
    if request.amount_minor > budget.terms.per_purchase_minor {
        return denied("over_per_operation_limit");
    }
    if request.amount_minor > available_minor {
        return denied("budget_exhausted");
    }
    // Above the budget's two-person threshold nothing is approved automatically.
    if budget
        .terms
        .second_approval_above_minor
        .is_some_and(|above| request.amount_minor > above)
    {
        return Decision::Held("needs_two_approvals".into());
    }
    match agent_grant {
        Some(Some(grant)) if request.amount_minor <= grant.auto_approve_minor => {
            Decision::AutoApproved("within_agent_auto_limit".into())
        }
        _ => Decision::Held("needs_owner_approval".into()),
    }
}

/// A case from the business's system of record: who is owed how much, on which payment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub customer: String,
    pub source: String,
    pub owed_minor: i64,
    pub currency: String,
}

/// Pure case policy, applied after `decide`. It only ever tightens a decision: a refund that
/// does not match the case it names is held for the owner even within the agent's limit, and
/// a missing or unknown case is denied. `committed_minor` is what this case has already
/// approved or spent; `facts` are the rail's verified facts about the payment.
pub fn apply_case(
    decision: Decision,
    request: &OperationRequest,
    agent_grant: Option<Option<&OperationGrant>>,
    case: Option<&Case>,
    committed_minor: i64,
    facts: &Value,
) -> Decision {
    if matches!(decision, Decision::Denied(_)) || request.kind != OperationKind::Refund {
        return decision;
    }
    let required = matches!(agent_grant, Some(Some(grant)) if grant.require_case);
    match (&request.case_id, case) {
        (None, _) if required => Decision::Denied("case_required".into()),
        (None, _) => decision,
        (Some(_), None) => Decision::Denied("case_not_found".into()),
        (Some(_), Some(case)) => {
            let customer = facts.get("customer").and_then(Value::as_str);
            if request.source.as_deref() != Some(case.source.as_str())
                || request.currency != case.currency
                || customer.is_some_and(|c| c != case.customer)
            {
                Decision::Held("case_mismatch".into())
            } else if request.amount_minor > case.owed_minor - committed_minor {
                Decision::Held("exceeds_case_owed".into())
            } else {
                decision
            }
        }
    }
}

fn load(db: &Connection, id: &str) -> Result<Operation> {
    let data: String = db
        .query_row("SELECT data FROM operations WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&data)?)
}

fn save(db: &Connection, op: &mut Operation, step: &str, at: i64, by: Value) -> Result<()> {
    op.version += 1;
    let data = serde_json::to_string(op)?;
    db.execute(
        "UPDATE operations SET data=?1 WHERE id=?2",
        params![data, op.id],
    )?;
    evidence::operation(db, step, op, &data, at, by)
}

/// Money already approved or spent against a case by earlier operations of this owner.
pub(crate) fn committed_to_case(db: &Connection, owner: &str, case_id: &str) -> Result<i64> {
    Ok(db.query_row(
        "SELECT COALESCE(SUM(h.amount),0) FROM operation_holds h JOIN operations o ON o.id=h.operation_id
        WHERE o.owner=?1 AND o.case_id=?2 AND h.state IN ('held','spent')",
        params![owner, case_id],
        |r| r.get(0),
    )?)
}

/// A person approving a held refund signs the case as it stood when it was requested. Held
/// operations place no hold, so other refunds on the same case may have been approved since.
/// If they have, and this one no longer fits what the case owes, the signed facts are no longer
/// true: refuse, so that it is requested again and shows the current case. An excess the person
/// was already shown is still theirs to approve while nothing has changed.
fn check_case_unchanged(db: &Connection, op: &Operation, proposal: &Proposal) -> Result<()> {
    let Some(case_id) = op.request.case_id.as_deref() else {
        return Ok(());
    };
    let signed = proposal.facts.get("case").ok_or(Error::StoreFormat)?;
    let owed = signed["owed_minor"].as_i64().ok_or(Error::StoreFormat)?;
    let signed_committed = signed["committed_minor"]
        .as_i64()
        .ok_or(Error::StoreFormat)?;
    let committed = committed_to_case(db, &op.owner, case_id)?;
    if committed > signed_committed && proposal.amount_minor > owed - committed {
        return Err(Error::Conflict(
            "exceeds_case_owed: refunds approved since this was requested now cover the case; request it again"
                .into(),
        ));
    }
    Ok(())
}

fn hold(db: &Connection, op: &Operation, budget: &str, amount: i64) -> Result<()> {
    db.execute(
        "INSERT INTO operation_holds(operation_id,budget_id,amount,state) VALUES (?1,?2,?3,'held')
        ON CONFLICT(operation_id) DO UPDATE SET amount=excluded.amount,state='held'",
        params![op.id, budget, amount],
    )?;
    Ok(())
}

fn settle_hold(db: &Connection, id: &str, state: &str) -> Result<()> {
    db.execute(
        "UPDATE operation_holds SET state=?1 WHERE operation_id=?2 AND state='held'",
        params![state, id],
    )?;
    Ok(())
}

fn active(budget: &Budget, at: i64) -> Result<()> {
    if budget.revoked || budget.terms.expires_at <= at {
        return Err(Error::Conflict("budget is revoked or expired".into()));
    }
    Ok(())
}

impl Service {
    /// Records a money-operation request and its decision. A refused or invalid request is
    /// recorded as `denied`; it is never silently dropped. Replays of the same idempotency
    /// key return the original operation.
    pub fn request_operation(
        &self,
        actor: &Actor,
        request: OperationRequest,
        at: i64,
    ) -> Result<Operation> {
        actor.require(&[Role::Owner, Role::Planner])?;
        if self.agent_scope.is_none() {
            actor.require(&[Role::Owner])?;
        }
        request.validate()?;
        let fingerprint = blake3::hash(&serde_json::to_vec(&request)?)
            .to_hex()
            .to_string();
        // Rail and case checks outside the ledger lock: they read the rail's own records and
        // the business's case records only.
        let check = self.rails.check(&request)?;
        let case = match &request.case_id {
            Some(id) => self.rails.case(id)?,
            None => None,
        };
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.check_agent_budget(
            &tx,
            actor,
            &request.budget_id,
            Some(Permission::Execute),
            at,
        )?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT fingerprint,id FROM operations WHERE owner=?1 AND idem=?2",
                params![actor.owner, request.idempotency_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((saved, id)) = existing {
            let op = load(&tx, &id)?;
            if op.agent.as_deref() != self.agent_scope.as_deref() {
                return Err(Error::NotFound);
            }
            return if saved == fingerprint {
                Ok(op)
            } else {
                Err(Error::Conflict(
                    "idempotency key was used for a different request".into(),
                ))
            };
        }
        let budget = storage::budget(&tx, &request.budget_id)?;
        actor.owns(&budget.owner)?;
        active(&budget, at)?;
        let (held, spent) = storage::totals(&tx, &budget.id)?;
        let available = budget.terms.limit_minor - held - spent;
        let grant = match &self.agent_scope {
            Some(agent) => Some(crate::agents::grant(&tx, agent)?),
            None => None,
        };
        let id = new_id("op")?;
        let (decision, proposal) = match check {
            Check::Invalid(reason) => (Decision::Denied(reason.into()), None),
            Check::Valid(mut facts) => {
                let committed = match &request.case_id {
                    Some(case_id) => committed_to_case(&tx, &actor.owner, case_id)?,
                    None => 0,
                };
                let decision = apply_case(
                    decide(
                        &request,
                        &budget,
                        available,
                        grant.as_ref().map(Option::as_ref),
                    ),
                    &request,
                    grant.as_ref().map(Option::as_ref),
                    case.as_ref(),
                    committed,
                    &facts,
                );
                // The owner signs what the case said, alongside what the rail verified.
                if let (Some(case), Some(map)) = (&case, facts.as_object_mut()) {
                    map.insert(
                        "case".into(),
                        serde_json::json!({"id": case.id, "system": self.rails.case_source(), "customer": case.customer,
                            "source": case.source, "owed_minor": case.owed_minor,
                            "currency": case.currency, "committed_minor": committed}),
                    );
                }
                let proposal = (!matches!(decision, Decision::Denied(_))).then(|| Proposal {
                    operation_id: id.clone(),
                    kind: request.kind,
                    rail: request.rail.clone(),
                    source: request.source.clone(),
                    payee: request.payee.clone(),
                    amount_minor: request.amount_minor,
                    currency: request.currency.clone(),
                    facts,
                    issued_at: at,
                    expires_at: at
                        .saturating_add(PROPOSAL_LIFETIME)
                        .min(budget.terms.expires_at),
                });
                (decision, proposal)
            }
        };
        let digest = proposal.as_ref().map(Proposal::digest).transpose()?;
        let (stage, approved_by) = match decision {
            Decision::AutoApproved(_) => (OperationStage::Approved, Some("policy".to_string())),
            Decision::Held(_) => (OperationStage::Held, None),
            Decision::Denied(_) => (OperationStage::Denied, None),
        };
        let op = Operation {
            id,
            owner: actor.owner.clone(),
            agent: self.agent_scope.clone(),
            request,
            version: 1,
            stage,
            proposal,
            decision,
            approved_digest: approved_by.as_ref().and(digest),
            approved_by,
            approver: None,
            first_approval: None,
            attempt_key: None,
            submitted_at: None,
            receipt: None,
        };
        let data = serde_json::to_string(&op)?;
        tx.execute(
            "INSERT INTO operations(id,owner,budget_id,agent_id,idem,fingerprint,data,case_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![op.id, op.owner, budget.id, op.agent, op.request.idempotency_key, fingerprint, data, op.request.case_id],
        )?;
        if op.stage == OperationStage::Approved {
            hold(&tx, &op, &budget.id, op.request.amount_minor)?;
        }
        let step = match op.stage {
            OperationStage::Approved => "auto_approved",
            OperationStage::Held => "held",
            _ => "denied",
        };
        evidence::operation(&tx, step, &op, &data, at, self.by(actor))?;
        tx.commit()?;
        Ok(op)
    }

    pub fn operation(&self, actor: &Actor, id: &str) -> Result<Operation> {
        actor.require(&[Role::Owner, Role::Planner, Role::Executor, Role::Approver])?;
        let op = load(&storage::connect(&self.ledger, false)?, id)?;
        self.visible(actor, &op)?;
        Ok(op)
    }

    fn visible(&self, actor: &Actor, op: &Operation) -> Result<()> {
        actor.owns(&op.owner)?;
        if self.agent_scope.is_some() && self.agent_scope != op.agent {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    /// The owner's operations, newest first, optionally only those in one stage.
    pub fn operations(
        &self,
        owner: &Actor,
        stage: Option<OperationStage>,
    ) -> Result<Vec<Operation>> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner, Role::Approver])?;
        let db = storage::connect(&self.ledger, false)?;
        let mut stmt =
            db.prepare("SELECT data FROM operations WHERE owner=?1 ORDER BY rowid DESC LIMIT 200")?;
        let rows = stmt.query_map([&owner.owner], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let op: Operation = serde_json::from_str(&row?)?;
            if stage.is_none_or(|stage| op.stage == stage) {
                out.push(op);
            }
        }
        Ok(out)
    }

    /// Owner approval of a held proposal, bound to its exact digest.
    pub fn approve_operation(
        &self,
        owner: &Actor,
        id: &str,
        digest: &str,
        at: i64,
    ) -> Result<Operation> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner, Role::Approver])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let at = if self.identity_scope.is_some() {
            crate::now()
        } else {
            at
        };
        let mut op = load(&tx, id)?;
        owner.owns(&op.owner)?;
        let approver = crate::approvers::authority(&tx, self, owner)?;
        // A repeat of a person's recorded approval returns the operation as it is, even once
        // payment has started; it changes nothing. An automatic approval is not a person's.
        let by_person = matches!(op.approved_by.as_deref(), Some("owner" | "approver"));
        let repeat = op.approved_digest.as_deref() == Some(digest)
            && (op.stage == OperationStage::Approved || (op.stage.has_attempt() && by_person));
        if repeat {
            return Ok(op);
        }
        if op.stage != OperationStage::Held {
            return Err(Error::Conflict(
                "operation is not waiting for approval".into(),
            ));
        }
        crate::agents::check_operation_agent(
            &tx,
            &op.owner,
            &op.request.budget_id,
            op.agent.as_deref(),
            at,
        )?;
        let proposal = op.proposal.as_ref().ok_or(Error::StoreFormat)?;
        if proposal.digest()? != digest {
            return Err(Error::Conflict("proposal digest does not match".into()));
        }
        if proposal.expires_at <= at {
            return Err(Error::Conflict(
                "proposal has expired; request it again".into(),
            ));
        }
        if let Some(approver) = &approver
            && (proposal.currency != approver.currency
                || proposal.amount_minor > approver.max_approval_minor)
        {
            return Err(Error::Conflict(
                "above_approver_limit: this payment needs an approver with a higher limit".into(),
            ));
        }
        let budget = storage::budget(&tx, &op.request.budget_id)?;
        active(&budget, at)?;
        // Two-person rule: the first approval is recorded and the operation stays held; the
        // second must come from a different person, for the same digest.
        let needs_two = budget
            .terms
            .second_approval_above_minor
            .is_some_and(|above| proposal.amount_minor > above);
        let person = approver.as_ref().map_or("owner", |a| a.id.as_str());
        if needs_two {
            match &op.first_approval {
                None => {
                    op.first_approval = Some(FirstApproval {
                        by: if approver.is_some() {
                            "approver"
                        } else {
                            "owner"
                        }
                        .into(),
                        approver: approver.as_ref().map(|a| a.id.clone()),
                        digest: digest.to_string(),
                        at,
                    });
                    save(&tx, &mut op, "first_approved", at, self.by(owner))?;
                    tx.commit()?;
                    return Ok(op);
                }
                Some(first) if first.person() == person => {
                    return Err(Error::Conflict(
                        "second_approver_required: a different person must give the second approval"
                            .into(),
                    ));
                }
                Some(first) if first.digest != digest => return Err(Error::StoreFormat),
                Some(_) => crate::identity::check_first_approval(&tx, &op)?,
            }
        }
        check_case_unchanged(&tx, &op, proposal)?;
        let (held, spent) = storage::totals(&tx, &budget.id)?;
        if proposal.amount_minor > budget.terms.limit_minor - held - spent {
            return Err(Error::BudgetExceeded);
        }
        let amount = proposal.amount_minor;
        hold(&tx, &op, &budget.id, amount)?;
        op.stage = OperationStage::Approved;
        op.approved_digest = Some(digest.to_string());
        let step = if approver.is_some() {
            "approver_approved"
        } else {
            "owner_approved"
        };
        op.approved_by = Some(
            if approver.is_some() {
                "approver"
            } else {
                "owner"
            }
            .into(),
        );
        op.approver = approver.map(|a| a.id);
        save(&tx, &mut op, step, at, self.by(owner))?;
        tx.commit()?;
        Ok(op)
    }

    /// Owner refusal before submission. Releases any hold.
    /// A named approver may reject any unsubmitted operation: refusing never moves money.
    pub fn reject_operation(&self, owner: &Actor, id: &str, at: i64) -> Result<Operation> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner, Role::Approver])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut op = load(&tx, id)?;
        owner.owns(&op.owner)?;
        let approver = crate::approvers::authority(&tx, self, owner)?;
        match op.stage {
            OperationStage::Rejected => return Ok(op),
            OperationStage::Held | OperationStage::Approved => {}
            _ => {
                return Err(Error::Conflict(
                    "only an unsubmitted operation can be rejected".into(),
                ));
            }
        }
        settle_hold(&tx, &op.id, "released")?;
        op.stage = OperationStage::Rejected;
        op.approved_digest = None;
        let step = if approver.is_some() {
            "approver_rejected"
        } else {
            "owner_rejected"
        };
        save(&tx, &mut op, step, at, self.by(owner))?;
        tx.commit()?;
        Ok(op)
    }

    /// Claims the single attempt of an approved operation, then submits it to the rail.
    /// `scenario` only affects the synthetic fixture rail.
    pub fn execute_operation(
        &self,
        actor: &Actor,
        id: &str,
        scenario: Scenario,
        at: i64,
    ) -> Result<Operation> {
        // An approver's approval is executed on the server right after it, as an owner's is.
        actor.require(&[Role::Owner, Role::Planner, Role::Executor, Role::Approver])?;
        let (op, claimed, at) = {
            let mut db = storage::connect(&self.ledger, false)?;
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let at = if self.identity_scope.is_some() {
                crate::now()
            } else {
                at
            };
            let mut op = load(&tx, id)?;
            self.visible(actor, &op)?;
            if actor.role == Role::Approver {
                crate::approvers::authority(&tx, self, actor)?;
            }
            if op.stage.has_attempt() {
                (op, false, at)
            } else {
                if op.stage != OperationStage::Approved {
                    return Err(Error::Conflict("operation is not approved".into()));
                }
                crate::identity::check_submission_authority(&tx, &op)?;
                crate::agents::check_operation_agent(
                    &tx,
                    &op.owner,
                    &op.request.budget_id,
                    op.agent.as_deref(),
                    at,
                )?;
                if self.agent_scope.is_some() {
                    self.check_agent_budget(
                        &tx,
                        actor,
                        &op.request.budget_id,
                        Some(Permission::Execute),
                        at,
                    )?;
                }
                let proposal = op.proposal.as_ref().ok_or(Error::StoreFormat)?;
                if proposal.expires_at <= at {
                    return Err(Error::Conflict(
                        "proposal has expired; request it again".into(),
                    ));
                }
                active(&storage::budget(&tx, &op.request.budget_id)?, at)?;
                if op.approved_digest.as_deref() != Some(proposal.digest()?.as_str()) {
                    return Err(Error::StoreFormat);
                }
                let held: Option<i64> = tx
                    .query_row(
                        "SELECT amount FROM operation_holds WHERE operation_id=?1 AND state='held'",
                        [id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if held != Some(proposal.amount_minor) {
                    return Err(Error::StoreFormat);
                }
                op.attempt_key = Some(new_id("opattempt")?);
                op.submitted_at = Some(at);
                op.stage = OperationStage::Submitting;
                save(&tx, &mut op, "submission_started", at, self.by(actor))?;
                tx.commit()?;
                (op, true, at)
            }
        };
        if !claimed {
            return Ok(op);
        }
        let outcome = self
            .rails
            .submit(
                op.attempt_key.as_deref().ok_or(Error::StoreFormat)?,
                op.proposal.as_ref().ok_or(Error::StoreFormat)?,
                scenario,
            )
            .unwrap_or(RailOutcome::Unknown);
        self.finish_operation(actor, &op, outcome, at)
    }

    /// Lookup only: settles a submitted operation if the rail has its result.
    pub fn reconcile_operation(&self, actor: &Actor, id: &str, at: i64) -> Result<Operation> {
        let op = self.operation(actor, id)?;
        if !matches!(
            op.stage,
            OperationStage::Submitting | OperationStage::Unknown
        ) {
            return Ok(op);
        }
        let key = op.attempt_key.as_deref().ok_or(Error::StoreFormat)?;
        let proposal = op.proposal.as_ref().ok_or(Error::StoreFormat)?;
        let outcome = match self.rails.lookup(key, proposal) {
            Ok(Some(receipt)) => RailOutcome::Done(receipt),
            _ => RailOutcome::Unknown,
        };
        self.finish_operation(actor, &op, outcome, at)
    }

    /// Operations sent to a rail whose result is not recorded, submitted at least `grace`
    /// seconds before `at` (an execute still in flight is left alone): `(owner, id)`.
    pub fn unsettled_operations(&self, at: i64, grace: i64) -> Result<Vec<(String, String)>> {
        self.require_unscoped()?;
        let db = storage::connect(&self.ledger, false)?;
        let mut statement = db.prepare(
            "SELECT owner,id FROM operations
             WHERE json_extract(data,'$.stage') IN ('submitting','unknown')
               AND COALESCE(json_extract(data,'$.submitted_at'),0) <= ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map([at.saturating_sub(grace)], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Approved operations with no claimed attempt can be stranded between approval and
    /// execution. Use the latest approval event (not proposal issuance) for the grace period.
    pub(crate) fn unsubmitted_operations(
        &self,
        at: i64,
        grace: i64,
    ) -> Result<Vec<(String, String)>> {
        self.require_unscoped()?;
        let db = storage::connect(&self.ledger, false)?;
        let mut statement = db.prepare(
            "SELECT o.owner,o.id FROM operations o JOIN evidence e
             ON json_extract(e.body,'$.event.operation')=o.id
             WHERE json_extract(o.data,'$.stage')='approved'
               AND json_extract(e.body,'$.event.stage')='approved'
             GROUP BY o.owner,o.id
             HAVING MAX(unixepoch(json_extract(e.body,'$.at'))) <= ?1
             ORDER BY o.id LIMIT 100",
        )?;
        Ok(statement
            .query_map([at.saturating_sub(grace)], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Atomically withdraw an old approval only when no submission has been claimed.
    /// A competing executor holds the same write lock: either it claims an attempt and
    /// this is a no-op, or it subsequently sees Held and cannot send. Never calls a rail.
    pub(crate) fn pause_unsubmitted_operation(
        &self,
        owner: &Actor,
        id: &str,
        at: i64,
        grace: i64,
    ) -> Result<Option<Operation>> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut op = load(&tx, id)?;
        owner.owns(&op.owner)?;
        if op.stage != OperationStage::Approved {
            return Ok(None);
        }
        let approved_at: Option<i64> = tx.query_row(
            "SELECT MAX(unixepoch(json_extract(body,'$.at'))) FROM evidence
             WHERE json_extract(body,'$.event.operation')=?1
               AND json_extract(body,'$.event.stage')='approved'",
            [id],
            |r| r.get(0),
        )?;
        if approved_at.is_none_or(|t| t > at.saturating_sub(grace)) {
            return Ok(None);
        }
        if op.attempt_key.is_some() || op.submitted_at.is_some() || op.receipt.is_some() {
            return Err(Error::StoreFormat);
        }
        let proposal = op.proposal.as_ref().ok_or(Error::StoreFormat)?;
        let held: Option<i64> = tx
            .query_row(
                "SELECT amount FROM operation_holds WHERE operation_id=?1 AND state='held'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if held != Some(proposal.amount_minor)
            || op.approved_digest.as_deref() != Some(proposal.digest()?.as_str())
        {
            return Err(Error::StoreFormat);
        }
        settle_hold(&tx, id, "released")?;
        op.stage = OperationStage::Held;
        op.decision = Decision::Held("execution_interrupted".into());
        op.approved_digest = None;
        op.approved_by = None;
        op.approver = None;
        op.first_approval = None;
        save(&tx, &mut op, "execution_interrupted", at, self.by(owner))?;
        tx.commit()?;
        Ok(Some(op))
    }

    /// Recovery by lookup only; nothing is ever sent. If the rail has the attempt's result it
    /// is recorded; an attempt left `submitting` (say, by a crash) with no result becomes
    /// `unknown`, recorded once; an `unknown` one with no result is left as it is, with no
    /// new record, so repeated checks do not fill the evidence chain.
    pub fn recover_operation(&self, owner: &Actor, id: &str, at: i64) -> Result<Operation> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let op = self.operation(owner, id)?;
        if !matches!(
            op.stage,
            OperationStage::Submitting | OperationStage::Unknown
        ) {
            return Ok(op);
        }
        let key = op.attempt_key.as_deref().ok_or(Error::StoreFormat)?;
        let proposal = op.proposal.as_ref().ok_or(Error::StoreFormat)?;
        match self.rails.lookup(key, proposal) {
            Ok(Some(receipt)) => self.finish_operation(owner, &op, RailOutcome::Done(receipt), at),
            _ if op.stage == OperationStage::Submitting => {
                self.finish_operation(owner, &op, RailOutcome::Unknown, at)
            }
            _ => Ok(op),
        }
    }

    /// Owner settlement of an unknown result through the rail: its receipt, or a void that
    /// refuses any late copy of the attempt. A rail that cannot answer leaves it held.
    pub fn resolve_operation(&self, owner: &Actor, id: &str, at: i64) -> Result<Operation> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let op = self.operation(owner, id)?;
        if !matches!(
            op.stage,
            OperationStage::Submitting | OperationStage::Unknown
        ) {
            return Ok(op);
        }
        let key = op.attempt_key.as_deref().ok_or(Error::StoreFormat)?;
        let outcome = self
            .rails
            .settle_unknown(
                key,
                op.proposal.as_ref().ok_or(Error::StoreFormat)?,
                op.submitted_at,
                at,
            )
            .unwrap_or(RailOutcome::Unknown);
        self.finish_operation(owner, &op, outcome, at)
    }

    fn finish_operation(
        &self,
        actor: &Actor,
        submitted: &Operation,
        outcome: RailOutcome,
        at: i64,
    ) -> Result<Operation> {
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut op = load(&tx, &submitted.id)?;
        actor.owns(&op.owner)?;
        if !matches!(
            op.stage,
            OperationStage::Submitting | OperationStage::Unknown
        ) {
            return Ok(op);
        }
        if op.attempt_key != submitted.attempt_key {
            return Err(Error::StoreFormat);
        }
        let proposal = op.proposal.as_ref().ok_or(Error::StoreFormat)?;
        let step = match outcome {
            RailOutcome::Done(receipt)
                if Some(receipt.attempt_key.as_str()) == op.attempt_key.as_deref()
                    && receipt.proposal_digest == proposal.digest()?
                    && receipt.amount_minor == proposal.amount_minor
                    && receipt.currency == proposal.currency =>
            {
                settle_hold(
                    &tx,
                    &op.id,
                    if receipt.succeeded {
                        "spent"
                    } else {
                        "released"
                    },
                )?;
                op.stage = if receipt.succeeded {
                    OperationStage::Succeeded
                } else {
                    OperationStage::Failed
                };
                op.receipt = Some(receipt);
                "rail_result"
            }
            RailOutcome::Rejected => {
                settle_hold(&tx, &op.id, "released")?;
                op.stage = OperationStage::Failed;
                "rail_rejected"
            }
            RailOutcome::Voided => {
                settle_hold(&tx, &op.id, "released")?;
                op.stage = OperationStage::Failed;
                "rail_voided"
            }
            _ => {
                op.stage = OperationStage::Unknown;
                "submission_unknown"
            }
        };
        save(&tx, &mut op, step, at, self.by(actor))?;
        tx.commit()?;
        Ok(op)
    }
}
