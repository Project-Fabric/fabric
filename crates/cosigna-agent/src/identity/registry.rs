use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    API_VERSION, IdentityError as E, IdentityResult, Subject,
    tokens::{issuer_valid, text},
};
use crate::{Actor, Error, Result, Role, Service, evidence, new_id, storage};

pub(crate) const SCHEMA: &str = "
CREATE TABLE identity_users (
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, display_name TEXT NOT NULL,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), UNIQUE(owner,id));
CREATE TABLE identity_subjects (
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, user_id TEXT NOT NULL,
 issuer TEXT NOT NULL, subject TEXT NOT NULL,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), UNIQUE(issuer,subject),
 FOREIGN KEY(owner,user_id) REFERENCES identity_users(owner,id));
CREATE TABLE identity_organizations (
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), UNIQUE(owner,id));
CREATE TABLE identity_memberships (
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, user_id TEXT NOT NULL, organization_id TEXT NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('owner','admin','member','viewer')),
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), UNIQUE(user_id,organization_id),
 FOREIGN KEY(owner,user_id) REFERENCES identity_users(owner,id),
 FOREIGN KEY(owner,organization_id) REFERENCES identity_organizations(owner,id));
CREATE INDEX identity_memberships_user ON identity_memberships(user_id,organization_id);";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub display_name: String,
    pub issuer: String,
    pub subject: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationInput {
    pub name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembershipInput {
    pub user_id: String,
    pub organization_id: String,
    pub role: String,
}

fn id_valid(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn public_name(s: &str) -> bool {
    text(s, 100, true) && !s.trim().is_empty() && s.trim() == s
}

fn table(resource: &str) -> Result<&'static str> {
    match resource {
        "users" => Ok("identity_users"),
        "subjects" => Ok("identity_subjects"),
        "organizations" => Ok("identity_organizations"),
        "memberships" => Ok("identity_memberships"),
        "gateway_bindings" => Ok("identity_gateway_bindings"),
        "approver_bindings" => Ok("identity_approver_bindings"),
        _ => Err(Error::NotFound),
    }
}

/// Exact private row state, including revocation and binding. Evidence stores only a digest.
pub(crate) fn record_digest(db: &Connection, id: &str) -> Result<Option<String>> {
    let query = if id_valid(id, "usr_") {
        "SELECT json_array(id,owner,display_name,revoked) FROM identity_users WHERE id=?1"
    } else if id_valid(id, "idn_") {
        "SELECT json_array(id,owner,user_id,issuer,subject,revoked) FROM identity_subjects WHERE id=?1"
    } else if id_valid(id, "org_") {
        "SELECT json_array(id,owner,name,revoked) FROM identity_organizations WHERE id=?1"
    } else if id_valid(id, "mem_") {
        "SELECT json_array(id,owner,user_id,organization_id,role,revoked) FROM identity_memberships WHERE id=?1"
    } else if id_valid(id, "gbo_") {
        "SELECT json_array(id,owner,organization_id,revoked) FROM identity_gateway_bindings WHERE id=?1"
    } else if id_valid(id, "iab_") {
        "SELECT json_array(id,owner,user_id,membership_id,approver_id,gateway_binding_id,revoked) FROM identity_approver_bindings WHERE id=?1"
    } else {
        return Ok(None);
    };
    let row: Option<String> = db.query_row(query, [id], |r| r.get(0)).optional()?;
    Ok(row.map(|s| blake3::hash(s.as_bytes()).to_hex().to_string()))
}

pub(crate) fn record_ids(db: &Connection) -> Result<Vec<String>> {
    let mut stmt = db.prepare("SELECT id FROM identity_users UNION ALL SELECT id FROM identity_subjects UNION ALL SELECT id FROM identity_organizations UNION ALL SELECT id FROM identity_memberships UNION ALL SELECT id FROM identity_gateway_bindings UNION ALL SELECT id FROM identity_approver_bindings ORDER BY id")?;
    Ok(stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}

pub(super) fn record(
    db: &Connection,
    id: &str,
    resource: &str,
    kind: &str,
    at: i64,
    by: Value,
) -> Result<()> {
    evidence::append(
        db,
        at,
        json!({"kind": format!("gateway.identity.{kind}"), "by": by,
        "resource": resource, "identity_record": id, "identity_digest": record_digest(db, id)?.ok_or(Error::NotFound)?}),
    )
}

fn owner_only(service: &Service, actor: &Actor) -> Result<()> {
    service.require_unscoped()?;
    actor.require(&[Role::Owner])
}

impl Service {
    /// Trusted operator enrollment. No token email/role claim creates or links an account.
    pub fn enroll_identity(&self, owner: &Actor, input: Enrollment, at: i64) -> Result<Value> {
        owner_only(self, owner)?;
        if !public_name(&input.display_name)
            || !issuer_valid(&input.issuer)
            || !text(&input.subject, 255, false)
        {
            return Err(Error::Invalid("invalid identity enrollment".into()));
        }
        let (user, identity) = (new_id("usr")?, new_id("idn")?);
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM identity_subjects WHERE issuer=?1 AND subject=?2)",
            params![input.issuer, input.subject],
            |r| r.get(0),
        )?;
        if exists {
            return Err(Error::Conflict("identity already enrolled".into()));
        }
        tx.execute(
            "INSERT INTO identity_users(id,owner,display_name) VALUES (?1,?2,?3)",
            params![user, owner.owner, input.display_name],
        )?;
        tx.execute("INSERT INTO identity_subjects(id,owner,user_id,issuer,subject) VALUES (?1,?2,?3,?4,?5)", params![identity,owner.owner,user,input.issuer,input.subject])?;
        record(&tx, &user, "users", "enrolled", at, self.by(owner))?;
        record(&tx, &identity, "subjects", "enrolled", at, self.by(owner))?;
        tx.commit()?;
        Ok(
            json!({"user": {"id": user, "display_name": input.display_name}, "identity_id": identity}),
        )
    }

    pub fn create_identity_organization(
        &self,
        owner: &Actor,
        input: OrganizationInput,
        at: i64,
    ) -> Result<Value> {
        owner_only(self, owner)?;
        if !public_name(&input.name) {
            return Err(Error::Invalid("invalid organization name".into()));
        }
        let id = new_id("org")?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO identity_organizations(id,owner,name) VALUES (?1,?2,?3)",
            params![id, owner.owner, input.name],
        )?;
        record(&tx, &id, "organizations", "enrolled", at, self.by(owner))?;
        tx.commit()?;
        Ok(json!({"id":id,"name":input.name}))
    }

    pub fn create_identity_membership(
        &self,
        owner: &Actor,
        input: MembershipInput,
        at: i64,
    ) -> Result<Value> {
        owner_only(self, owner)?;
        if !matches!(input.role.as_str(), "owner" | "admin" | "member" | "viewer") {
            return Err(Error::Invalid("invalid membership role".into()));
        }
        let id = new_id("mem")?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (table, target) in [
            ("identity_users", &input.user_id),
            ("identity_organizations", &input.organization_id),
        ] {
            let exists: bool = tx.query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1 AND owner=?2 AND revoked=0)"
                ),
                params![target, owner.owner],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error::NotFound);
            }
        }
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM identity_memberships WHERE user_id=?1 AND organization_id=?2)", params![input.user_id,input.organization_id], |r| r.get(0))?;
        if exists {
            return Err(Error::Conflict("membership already exists".into()));
        }
        tx.execute("INSERT INTO identity_memberships(id,owner,user_id,organization_id,role) VALUES (?1,?2,?3,?4,?5)", params![id,owner.owner,input.user_id,input.organization_id,input.role])?;
        record(&tx, &id, "memberships", "enrolled", at, self.by(owner))?;
        tx.commit()?;
        Ok(
            json!({"id":id,"user_id":input.user_id,"organization_id":input.organization_id,"role":input.role}),
        )
    }

    /// Permanent revocation; old decisions and evidence remain. No public user mutation API.
    pub fn revoke_identity_record(
        &self,
        owner: &Actor,
        resource: &str,
        id: &str,
        at: i64,
    ) -> Result<Value> {
        owner_only(self, owner)?;
        let table = table(resource)?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revoked: bool = tx
            .query_row(
                &format!("SELECT revoked FROM {table} WHERE id=?1 AND owner=?2"),
                params![id, owner.owner],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if !revoked {
            tx.execute(&format!("UPDATE {table} SET revoked=1 WHERE id=?1"), [id])?;
            record(&tx, id, resource, "revoked", at, self.by(owner))?;
        }
        tx.commit()?;
        Ok(json!({"id":id,"revoked":true}))
    }

    pub fn identity_read(
        &self,
        subject: &Subject,
        method: &str,
        path: &str,
    ) -> IdentityResult<Value> {
        // One read transaction gives a consistent membership/user/revocation snapshot.
        // A subsequent request sees a revocation committed after this read began.
        let read = || -> std::result::Result<Value, ReadError> {
            let mut db = storage::connect(&self.ledger, false)?;
            let tx = db.transaction()?;
            let user: Option<(String,String)> = tx.query_row("SELECT u.id,u.display_name FROM identity_subjects s JOIN identity_users u ON u.id=s.user_id AND u.owner=s.owner WHERE s.issuer=?1 AND s.subject=?2 AND s.revoked=0 AND u.revoked=0", params![subject.issuer,subject.subject], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let (user_id, name) = user.ok_or(ReadError::Public(E::InvalidToken))?;
            if method != "GET" {
                return Err(ReadError::Public(E::NotFound));
            }
            let (path, query) = path.split_once('?').unwrap_or((path, ""));
            if path == "/identity/v1/me" && query.is_empty() {
                return Ok(json!({"schema":API_VERSION,"data":{"id":user_id,"display_name":name}}));
            }
            let (after, target) = if path == "/identity/v1/organizations" {
                let after = if query.is_empty() {
                    ""
                } else {
                    query
                        .strip_prefix("after=")
                        .filter(|id| id_valid(id, "org_"))
                        .ok_or(ReadError::Public(E::InvalidCursor))?
                };
                (after, None)
            } else if let Some(id) = path.strip_prefix("/identity/v1/organizations/") {
                if !query.is_empty() || !id_valid(id, "org_") {
                    return Err(ReadError::Public(E::NotFound));
                }
                ("", Some(id))
            } else {
                return Err(ReadError::Public(E::NotFound));
            };
            let mut stmt = tx.prepare("SELECT o.id,o.name,m.role FROM identity_memberships m JOIN identity_organizations o ON o.id=m.organization_id AND o.owner=m.owner WHERE m.user_id=?1 AND m.revoked=0 AND o.revoked=0 AND o.id>?2 AND (?3 IS NULL OR o.id=?3) ORDER BY o.id LIMIT 51")?;
            let mut items = stmt.query_map(params![user_id,after,target], |r| Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"role":r.get::<_,String>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
            if target.is_some() {
                return items
                    .pop()
                    .map(|item| json!({"schema":API_VERSION,"data":item}))
                    .ok_or(ReadError::Public(E::NotFound));
            }
            let next = if items.len() > 50 {
                items.truncate(50);
                items.last().map(|item| item["id"].clone())
            } else {
                None
            };
            Ok(json!({"schema":API_VERSION,"data":items,"next_cursor":next}))
        };
        read().map_err(|e| match e {
            ReadError::Public(e) => e,
            ReadError::Store => E::Unavailable,
        })
    }
}

enum ReadError {
    Public(E),
    Store,
}
impl From<Error> for ReadError {
    fn from(_: Error) -> Self {
        Self::Store
    }
}
impl From<rusqlite::Error> for ReadError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Store
    }
}

pub(crate) fn admin(
    service: &Service,
    actor: &Actor,
    method: &str,
    path: &str,
    body: &[u8],
    at: i64,
) -> Result<Value> {
    owner_only(service, actor)?;
    fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T> {
        if body.len() > 16_384 {
            return Err(Error::Invalid("body exceeds 16 KiB".into()));
        }
        serde_json::from_slice(body)
            .map_err(|_| Error::Invalid("invalid identity administration body".into()))
    }
    let parts: Vec<_> = path.split('/').collect();
    let data = match (method, parts.as_slice()) {
        ("POST", ["", "gateway", "v1", "identity", "users"]) => {
            service.enroll_identity(actor, decode(body)?, at)?
        }
        ("POST", ["", "gateway", "v1", "identity", "organizations"]) => {
            service.create_identity_organization(actor, decode(body)?, at)?
        }
        ("POST", ["", "gateway", "v1", "identity", "memberships"]) => {
            service.create_identity_membership(actor, decode(body)?, at)?
        }
        ("POST", ["", "gateway", "v1", "identity", "gateway_bindings"]) => {
            service.bind_identity_gateway(actor, decode(body)?, at)?
        }
        ("POST", ["", "gateway", "v1", "identity", "approver_bindings"]) => {
            service.bind_identity_approver(actor, decode(body)?, at)?
        }
        ("POST", ["", "gateway", "v1", "identity", resource, id, "revoke"]) => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Empty {}
            decode::<Empty>(body)?;
            service.revoke_identity_record(actor, resource, id, at)?
        }
        _ => return Err(Error::NotFound),
    };
    Ok(json!({"schema":API_VERSION,"data":data}))
}
