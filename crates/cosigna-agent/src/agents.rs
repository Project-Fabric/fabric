//! Durable agent identities. A bearer conveys only its registered scope, never ownership.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{Actor, Error, Result, Role, Service, Task, evidence, new_id, storage};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Plan,
    Execute,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub name: String,
    pub budget_id: String,
    pub permissions: Vec<Permission>,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredAgent {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub budget_id: String,
    pub permissions: Vec<Permission>,
    pub expires_at: i64,
    pub revoked: bool,
}

// Intentionally no Debug: the credential is returned once to the owner, never listed.
#[derive(Serialize)]
pub struct IssuedAgent {
    pub agent: RegisteredAgent,
    pub token: String,
}

fn load(db: &Connection, id: &str) -> Result<RegisteredAgent> {
    let row: (String, String, String, String, i64, bool) = db
        .query_row(
            "SELECT owner,name,budget_id,permissions,expires_at,revoked FROM agents WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(RegisteredAgent {
        id: id.into(),
        owner: row.0,
        name: row.1,
        budget_id: row.2,
        permissions: serde_json::from_str(&row.3)?,
        expires_at: row.4,
        revoked: row.5,
    })
}

fn active(agent: &RegisteredAgent, at: i64) -> Result<()> {
    if agent.revoked || agent.expires_at <= at {
        return Err(Error::Forbidden);
    }
    Ok(())
}

impl Service {
    pub(crate) fn require_unscoped(&self) -> Result<()> {
        if self.agent_scope.is_some() {
            Err(Error::Forbidden)
        } else {
            Ok(())
        }
    }

    pub fn register_agent(
        &self,
        owner: &Actor,
        input: Registration,
        at: i64,
    ) -> Result<IssuedAgent> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        if input.name.trim().is_empty()
            || input.name.len() > 100
            || input.name.chars().any(char::is_control)
            || input.expires_at <= at
            || input.permissions.is_empty()
            || input.permissions.len() > 2
            || (input.permissions.len() == 2 && input.permissions[0] == input.permissions[1])
        {
            return Err(Error::Invalid("invalid agent registration".into()));
        }
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let budget = storage::budget(&tx, &input.budget_id)?;
        owner.owns(&budget.owner)?;
        if budget.revoked || input.expires_at > budget.terms.expires_at {
            return Err(Error::Invalid(
                "agent expiry must fit an active budget".into(),
            ));
        }
        let agent = RegisteredAgent {
            id: new_id("agent")?,
            owner: owner.owner.clone(),
            name: input.name.trim().into(),
            budget_id: input.budget_id,
            permissions: input.permissions,
            expires_at: input.expires_at,
            revoked: false,
        };
        let token = new_id("agentkey")?;
        tx.execute("INSERT INTO agents(id,owner,name,budget_id,permissions,expires_at,token_hash) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![agent.id, agent.owner, agent.name, agent.budget_id, serde_json::to_string(&agent.permissions)?, agent.expires_at,
                blake3::hash(token.as_bytes()).to_hex().to_string()])?;
        tx.execute(
            "INSERT INTO agent_events(agent_id,kind,at) VALUES (?1,'registered',?2)",
            params![agent.id, at],
        )?;
        evidence::agent(&tx, "registered", &agent.id, at, self.by(owner))?;
        tx.commit()?;
        Ok(IssuedAgent { agent, token })
    }

    pub fn registered_agent(&self, owner: &Actor, id: &str) -> Result<RegisteredAgent> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let agent = load(&storage::connect(&self.ledger, false)?, id)?;
        owner.owns(&agent.owner)?;
        Ok(agent)
    }

    pub fn registered_agents(&self, owner: &Actor) -> Result<Vec<RegisteredAgent>> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let db = storage::connect(&self.ledger, false)?;
        let mut statement = db.prepare("SELECT id FROM agents WHERE owner=?1 ORDER BY id")?;
        let ids = statement
            .query_map([&owner.owner], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter().map(|id| load(&db, id)).collect()
    }

    pub fn revoke_agent(&self, owner: &Actor, id: &str, at: i64) -> Result<RegisteredAgent> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut agent = load(&tx, id)?;
        owner.owns(&agent.owner)?;
        if !agent.revoked {
            tx.execute("UPDATE agents SET revoked=1 WHERE id=?1", [id])?;
            tx.execute(
                "INSERT INTO agent_events(agent_id,kind,at) VALUES (?1,'revoked',?2)",
                params![id, at],
            )?;
            evidence::agent(&tx, "revoked", id, at, self.by(owner))?;
            agent.revoked = true;
        }
        tx.commit()?;
        Ok(agent)
    }

    pub(crate) fn authenticate_agent(
        &self,
        authorization: &str,
        at: i64,
    ) -> Result<RegisteredAgent> {
        self.require_unscoped()?;
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or(Error::Forbidden)?;
        let suffix = token.strip_prefix("agentkey_").ok_or(Error::Forbidden)?;
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
                "SELECT id FROM agents WHERE token_hash=?1",
                [blake3::hash(token.as_bytes()).to_hex().to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Forbidden)?;
        let agent = load(&db, &id)?;
        active(&agent, at)?;
        Ok(agent)
    }

    pub(crate) fn scoped_agent(&self, agent: &RegisteredAgent) -> Self {
        let mut service = self.clone();
        service.agent_scope = Some(agent.id.clone());
        service
    }

    pub(crate) fn authenticate_scope(
        &self,
        actor: &Actor,
        id: &str,
        at: i64,
    ) -> Result<RegisteredAgent> {
        if self.agent_scope.as_deref() != Some(id) {
            return Err(Error::Forbidden);
        }
        let agent = load(&storage::connect(&self.ledger, false)?, id)?;
        actor.owns(&agent.owner)?;
        active(&agent, at)?;
        Ok(agent)
    }

    pub(crate) fn check_agent_budget(
        &self,
        db: &Connection,
        actor: &Actor,
        budget: &str,
        permission: Option<Permission>,
        at: i64,
    ) -> Result<()> {
        if let Some(id) = &self.agent_scope {
            let agent = load(db, id)?;
            actor.owns(&agent.owner)?;
            if agent.budget_id != budget {
                return Err(Error::NotFound);
            }
            active(&agent, at)?;
            if permission.is_some_and(|value| !agent.permissions.contains(&value)) {
                return Err(Error::Forbidden);
            }
        }
        Ok(())
    }

    pub(crate) fn check_agent_task(
        &self,
        db: &Connection,
        actor: &Actor,
        task: &Task,
        permission: Option<Permission>,
        at: i64,
    ) -> Result<()> {
        let assigned: Option<String> = db
            .query_row(
                "SELECT agent_id FROM task_agents WHERE task_id=?1",
                [&task.id],
                |r| r.get(0),
            )
            .optional()?;
        if self.agent_scope.is_some() && self.agent_scope != assigned {
            return Err(Error::NotFound);
        }
        self.check_agent_budget(db, actor, &task.input.budget_id, permission, at)?;
        // Owner/admin status recovery remains possible after revocation. New approval,
        // planning and submission claims must honor the task's original agent grant too.
        if let Some(id) = assigned.filter(|_| permission.is_some()) {
            let agent = load(db, &id)?;
            if agent.owner != task.owner || agent.budget_id != task.input.budget_id {
                return Err(Error::StoreFormat);
            }
            active(&agent, at)?;
        }
        Ok(())
    }

    pub(crate) fn task_agent_id(&self, actor: &Actor, task: &str) -> Result<Option<String>> {
        self.task(actor, task)?;
        Ok(storage::connect(&self.ledger, false)?
            .query_row(
                "SELECT agent_id FROM task_agents WHERE task_id=?1",
                [task],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub(crate) fn task_agent_active(&self, actor: &Actor, task: &Task, at: i64) -> Result<bool> {
        let Some(id) = self.task_agent_id(actor, &task.id)? else {
            return Ok(true);
        };
        let agent = load(&storage::connect(&self.ledger, false)?, &id)?;
        if agent.owner != task.owner || agent.budget_id != task.input.budget_id {
            return Err(Error::StoreFormat);
        }
        Ok(!agent.revoked && agent.expires_at > at)
    }
}
