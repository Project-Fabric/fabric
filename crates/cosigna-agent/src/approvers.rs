//! Named approvers (ledger version 7): people the owner authorizes to decide held money
//! operations, each with their own credential and a limit. An approver can list, read, approve
//! and reject operations, and nothing else: approving above their limit or in another currency
//! is refused; rejecting is always allowed because it never moves money. Every decision is
//! recorded with the approver's id. Owner approval is unchanged.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{Actor, Error, MAX_MINOR, Result, Role, Service, evidence, new_id, storage};

pub const API_VERSION: &str = "cosigna.approver/v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproverInput {
    pub name: String,
    /// The currency of `max_approval_minor`; operations in another currency are refused.
    pub currency: String,
    /// The largest single operation this person may approve.
    pub max_approval_minor: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredApprover {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub currency: String,
    pub max_approval_minor: i64,
    pub created_at: i64,
    pub revoked: bool,
}

// Intentionally no Debug: the credential is returned once to the owner, never listed.
#[derive(Serialize)]
pub struct IssuedApprover {
    pub approver: RegisteredApprover,
    pub token: String,
}

pub(crate) fn load(db: &Connection, id: &str) -> Result<RegisteredApprover> {
    db.query_row(
        "SELECT owner,name,currency,max_approval_minor,created_at,revoked FROM approvers WHERE id=?1",
        [id],
        |r| {
            Ok(RegisteredApprover {
                id: id.into(),
                owner: r.get(0)?,
                name: r.get(1)?,
                currency: r.get(2)?,
                max_approval_minor: r.get(3)?,
                created_at: r.get(4)?,
                revoked: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// The approver's authority for one decision, read inside the deciding transaction so a
/// revocation committed earlier always wins. `None` when the actor is not an approver.
pub(crate) fn authority(
    db: &Connection,
    service: &Service,
    actor: &Actor,
) -> Result<Option<RegisteredApprover>> {
    if actor.role != Role::Approver {
        return Ok(None);
    }
    let id = service.approver_scope.as_deref().ok_or(Error::Forbidden)?;
    let approver = load(db, id)?;
    actor.owns(&approver.owner)?;
    if approver.revoked {
        return Err(Error::Forbidden);
    }
    Ok(Some(approver))
}

impl Service {
    pub fn register_approver(
        &self,
        owner: &Actor,
        input: ApproverInput,
        at: i64,
    ) -> Result<IssuedApprover> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let name = input.name.trim();
        if name.is_empty()
            || name.len() > 100
            || name.chars().any(char::is_control)
            || input.currency.len() != 3
            || !input.currency.bytes().all(|b| b.is_ascii_uppercase())
            || !(1..=MAX_MINOR).contains(&input.max_approval_minor)
        {
            return Err(Error::Invalid("invalid approver registration".into()));
        }
        let approver = RegisteredApprover {
            id: new_id("approver")?,
            owner: owner.owner.clone(),
            name: name.into(),
            currency: input.currency,
            max_approval_minor: input.max_approval_minor,
            created_at: at,
            revoked: false,
        };
        let token = new_id("approverkey")?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO approvers(id,owner,name,currency,max_approval_minor,created_at,token_hash) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![approver.id, approver.owner, approver.name, approver.currency,
                approver.max_approval_minor, at, blake3::hash(token.as_bytes()).to_hex().to_string()],
        )?;
        evidence::approver(&tx, "registered", &approver.id, at, self.by(owner))?;
        tx.commit()?;
        Ok(IssuedApprover { approver, token })
    }

    pub fn approvers(&self, owner: &Actor) -> Result<Vec<RegisteredApprover>> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let db = storage::connect(&self.ledger, false)?;
        let mut statement = db.prepare("SELECT id FROM approvers WHERE owner=?1 ORDER BY id")?;
        let ids = statement
            .query_map([&owner.owner], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter().map(|id| load(&db, id)).collect()
    }

    /// Permanent. Decisions already recorded stay valid; new ones are refused.
    pub fn revoke_approver(&self, owner: &Actor, id: &str, at: i64) -> Result<RegisteredApprover> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut approver = load(&tx, id)?;
        owner.owns(&approver.owner)?;
        if !approver.revoked {
            tx.execute("UPDATE approvers SET revoked=1 WHERE id=?1", [id])?;
            evidence::approver(&tx, "revoked", id, at, self.by(owner))?;
            approver.revoked = true;
        }
        tx.commit()?;
        Ok(approver)
    }

    pub(crate) fn authenticate_approver(&self, authorization: &str) -> Result<RegisteredApprover> {
        self.require_unscoped()?;
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or(Error::Forbidden)?;
        let suffix = token.strip_prefix("approverkey_").ok_or(Error::Forbidden)?;
        if suffix.len() != 64
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::Forbidden);
        }
        let db = storage::connect(&self.ledger, false)?;
        let id: String = db
            .query_row(
                "SELECT id FROM approvers WHERE token_hash=?1",
                [blake3::hash(token.as_bytes()).to_hex().to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Forbidden)?;
        let approver = load(&db, &id)?;
        if approver.revoked {
            return Err(Error::Forbidden);
        }
        Ok(approver)
    }

    pub(crate) fn scoped_approver(&self, approver: &RegisteredApprover) -> Self {
        let mut service = self.clone();
        service.approver_scope = Some(approver.id.clone());
        service
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BudgetInput,
        agents::{Permission, Registration},
        operations::{OperationGrant, OperationKind, OperationRequest},
    };

    #[test]
    fn a_revocation_after_authentication_still_wins_inside_the_decision() {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path()).unwrap();
        let at = 1_800_000_000;
        let actor = |role| Actor {
            owner: "owner".into(),
            role,
        };
        let budget = service
            .create_budget(
                &actor(Role::Owner),
                BudgetInput {
                    currency: "USD".into(),
                    limit_minor: 100_000,
                    per_purchase_minor: 50_000,
                    merchants: vec!["fixture-cafe".into()],
                    expires_at: at + 86_400,
                },
                at,
            )
            .unwrap();
        let agent = service
            .register_agent(
                &actor(Role::Owner),
                Registration {
                    name: "refunds-bot".into(),
                    budget_id: budget.id.clone(),
                    permissions: vec![Permission::Execute],
                    expires_at: at + 3600,
                    operations: Some(OperationGrant {
                        kinds: vec![OperationKind::Refund],
                        rails: vec!["fixture".into()],
                        auto_approve_minor: 0,
                        require_case: false,
                    }),
                },
                at,
            )
            .unwrap();
        let op = service
            .scoped_agent(&agent.agent)
            .request_operation(
                &actor(Role::Planner),
                OperationRequest {
                    budget_id: budget.id,
                    kind: OperationKind::Refund,
                    rail: "fixture".into(),
                    source: Some("pay_fixture_2045".into()),
                    payee: None,
                    amount_minor: 45_000,
                    currency: "USD".into(),
                    reason: "Refund".into(),
                    idempotency_key: "held".into(),
                    case_id: None,
                },
                at,
            )
            .unwrap();
        let issued = service
            .register_approver(
                &actor(Role::Owner),
                ApproverInput {
                    name: "Priya".into(),
                    currency: "USD".into(),
                    max_approval_minor: 50_000,
                },
                at,
            )
            .unwrap();
        // Authenticated while active...
        let session = service
            .authenticate_approver(&format!("Bearer {}", issued.token))
            .unwrap();
        let scoped = service.scoped_approver(&session);
        // ...revoked before deciding.
        service
            .revoke_approver(&actor(Role::Owner), &issued.approver.id, at)
            .unwrap();
        let digest = op.proposal.as_ref().unwrap().digest().unwrap();
        assert!(matches!(
            scoped.approve_operation(&actor(Role::Approver), &op.id, &digest, at),
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            scoped.reject_operation(&actor(Role::Approver), &op.id, at),
            Err(Error::Forbidden)
        ));
    }
}
