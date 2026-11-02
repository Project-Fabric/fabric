//! Explicit organization/owner/approver bindings. A token's role claim is never authority.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{IdentityError as E, Subject, registry};
use crate::{Actor, Error, Result, Role, Service, approvers::RegisteredApprover, new_id, storage};

pub(crate) const BINDING_SCHEMA: &str = "
CREATE TABLE identity_gateway_bindings (
 id TEXT PRIMARY KEY, owner TEXT NOT NULL UNIQUE, organization_id TEXT NOT NULL UNIQUE,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
 FOREIGN KEY(owner,organization_id) REFERENCES identity_organizations(owner,id));
CREATE TABLE identity_approver_bindings (
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, user_id TEXT NOT NULL,
 membership_id TEXT NOT NULL REFERENCES identity_memberships(id),
 approver_id TEXT NOT NULL UNIQUE REFERENCES approvers(id),
 gateway_binding_id TEXT NOT NULL REFERENCES identity_gateway_bindings(id),
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), UNIQUE(owner,user_id),
 FOREIGN KEY(owner,user_id) REFERENCES identity_users(owner,id));";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayBindingInput {
    pub organization_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproverBindingInput {
    pub membership_id: String,
    pub approver_id: String,
}

/// Only non-secret, verified identity facts; never the bearer token or signing material.
#[derive(Clone)]
pub(crate) struct Context {
    subject: Subject,
    owner: String,
    user_id: String,
    subject_id: String,
    organization_id: String,
    membership_id: String,
    gateway_binding_id: String,
    approver_binding_id: String,
    approver_id: String,
    verified_at: i64,
}

impl Context {
    pub(crate) fn evidence(&self) -> Value {
        json!({"issuer":self.subject.issuer,"subject":self.subject.subject,
            "subject_id":self.subject_id,"user_id":self.user_id,"organization_id":self.organization_id,
            "membership_id":self.membership_id,"gateway_binding_id":self.gateway_binding_id,
            "approver_binding_id":self.approver_binding_id,
            "verified_at":self.verified_at,"token_expires_at":self.subject.expires_at})
    }
}

pub(crate) fn has_binding(db: &Connection, approver_id: &str) -> Result<bool> {
    // Includes revoked bindings. Revocation never reactivates an earlier key.
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM identity_approver_bindings WHERE approver_id=?1)",
        [approver_id],
        |r| r.get(0),
    )?)
}

fn lookup(db: &Connection, subject: &Subject, organization: &str, at: i64) -> Result<Context> {
    if subject.expires_at <= crate::now() {
        return Err(E::InvalidToken.into());
    }
    db.query_row("SELECT s.id,u.id,u.owner,m.id,g.id,b.id,b.approver_id
        FROM identity_subjects s
        JOIN identity_users u ON u.id=s.user_id AND u.owner=s.owner
        JOIN identity_memberships m ON m.user_id=u.id AND m.owner=u.owner
        JOIN identity_organizations o ON o.id=m.organization_id AND o.owner=m.owner
        JOIN identity_gateway_bindings g ON g.organization_id=o.id AND g.owner=o.owner
        JOIN identity_approver_bindings b ON b.membership_id=m.id AND b.user_id=u.id AND b.owner=u.owner AND b.gateway_binding_id=g.id
        WHERE s.issuer=?1 AND s.subject=?2 AND o.id=?3 AND s.revoked=0 AND u.revoked=0
          AND m.revoked=0 AND o.revoked=0 AND g.revoked=0 AND b.revoked=0",
        params![subject.issuer,subject.subject,organization],|r| Ok(Context {
            subject:subject.clone(), subject_id:r.get(0)?,user_id:r.get(1)?,owner:r.get(2)?,
            organization_id:organization.into(),membership_id:r.get(3)?,gateway_binding_id:r.get(4)?,
            approver_binding_id:r.get(5)?,approver_id:r.get(6)?,verified_at:at,
        })).optional()?.ok_or(E::NotFound.into())
}

/// Called inside each approval/rejection/submission-claim transaction. A revocation committed
/// before the lock was acquired wins, even if authentication happened earlier.
pub(crate) fn check_authority(
    db: &Connection,
    context: &Context,
    approver: &RegisteredApprover,
) -> Result<()> {
    if !context.subject.can_approve {
        return Err(E::InsufficientApprovalScope.into());
    }
    let current = lookup(
        db,
        &context.subject,
        &context.organization_id,
        context.verified_at,
    )?;
    if current.owner != approver.owner
        || current.approver_id != approver.id
        || current.user_id != context.user_id
        || current.subject_id != context.subject_id
        || current.approver_binding_id != context.approver_binding_id
        || current.membership_id != context.membership_id
        || current.gateway_binding_id != context.gateway_binding_id
    {
        return Err(Error::Forbidden);
    }
    Ok(())
}

/// A durable first approval does not require its old token to remain unexpired. Its exact
/// enrolled subject, membership and bindings must still be active when a second person acts.
pub(crate) fn check_first_approval(
    db: &Connection,
    operation: &crate::operations::Operation,
) -> Result<()> {
    let Some(first) = operation
        .first_approval
        .as_ref()
        .and_then(|first| first.approver.as_deref())
    else {
        return Ok(());
    };
    check_recorded_approval(
        db,
        operation,
        first,
        "gateway.operation.first_approved",
        "first_approval_no_longer_authorized",
    )
}

pub(crate) fn check_submission_authority(
    db: &Connection,
    operation: &crate::operations::Operation,
) -> Result<()> {
    check_first_approval(db, operation)?;
    if operation.approved_by.as_deref() == Some("approver") {
        check_recorded_approval(
            db,
            operation,
            operation.approver.as_deref().ok_or(Error::StoreFormat)?,
            "gateway.operation.approver_approved",
            "approval_no_longer_authorized",
        )?;
    }
    Ok(())
}

fn check_recorded_approval(
    db: &Connection,
    operation: &crate::operations::Operation,
    approver_id: &str,
    kind: &str,
    reason: &str,
) -> Result<()> {
    let data: Option<String>=db.query_row("SELECT body FROM evidence WHERE json_extract(body,'$.event.kind')=?1 AND json_extract(body,'$.event.operation')=?2 ORDER BY seq DESC LIMIT 1",params![kind,operation.id],|r|r.get(0)).optional()?;
    let event: Value = data
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or(Value::Null);
    let proof = &event["event"]["by"]["identity"];
    let invalid = || Error::Conflict(format!("{reason}: request a fresh proposal"));
    // Key-based and identity-bound approvals alike only count while the person who gave them
    // is still an active approver for this owner.
    let approver = match crate::approvers::load(db, approver_id) {
        Ok(approver) => approver,
        Err(Error::NotFound) => return Err(invalid()),
        Err(error) => return Err(error),
    };
    if approver.revoked || approver.owner != operation.owner {
        return Err(invalid());
    }
    if proof.is_null() {
        return if has_binding(db, approver_id)? {
            Err(invalid())
        } else {
            Ok(())
        };
    }
    let value = |key: &str| proof[key].as_str().ok_or_else(invalid);
    let subject = Subject {
        issuer: value("issuer")?.into(),
        subject: value("subject")?.into(),
        expires_at: crate::now().saturating_add(60),
        can_approve: true,
    };
    let current =
        lookup(db, &subject, value("organization_id")?, crate::now()).map_err(|_| invalid())?;
    if current.owner != operation.owner
        || current.approver_id != approver_id
        || current.subject_id != value("subject_id")?
        || current.user_id != value("user_id")?
        || current.membership_id != value("membership_id")?
        || current.gateway_binding_id != value("gateway_binding_id")?
        || current.approver_binding_id != value("approver_binding_id")?
    {
        return Err(invalid());
    }
    Ok(())
}

impl Service {
    /// Explicitly exposes this owner's financial namespace (including history) to one org.
    pub fn bind_identity_gateway(
        &self,
        owner: &Actor,
        input: GatewayBindingInput,
        at: i64,
    ) -> Result<Value> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM identity_organizations WHERE id=?1 AND owner=?2 AND revoked=0)",params![input.organization_id,owner.owner],|r|r.get(0))?;
        if !active {
            return Err(Error::NotFound);
        }
        let existing: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM identity_gateway_bindings WHERE owner=?1 OR organization_id=?2)",params![owner.owner,input.organization_id],|r|r.get(0))?;
        if existing {
            return Err(Error::Conflict(
                "gateway owner or organization is already bound".into(),
            ));
        }
        let id = new_id("gbo")?;
        tx.execute(
            "INSERT INTO identity_gateway_bindings(id,owner,organization_id) VALUES (?1,?2,?3)",
            params![id, owner.owner, input.organization_id],
        )?;
        registry::record(&tx, &id, "gateway_bindings", "enrolled", at, self.by(owner))?;
        tx.commit()?;
        Ok(json!({"id":id,"organization_id":input.organization_id}))
    }

    /// One enrolled user can occupy only one approver slot under an owner, even after revoke.
    pub fn bind_identity_approver(
        &self,
        owner: &Actor,
        input: ApproverBindingInput,
        at: i64,
    ) -> Result<Value> {
        self.require_unscoped()?;
        owner.require(&[Role::Owner])?;
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (user, gateway): (String,String)=tx.query_row("SELECT m.user_id,g.id FROM identity_memberships m
            JOIN identity_users u ON u.id=m.user_id AND u.owner=m.owner
            JOIN identity_organizations o ON o.id=m.organization_id AND o.owner=m.owner
            JOIN identity_gateway_bindings g ON g.organization_id=o.id AND g.owner=o.owner
            WHERE m.id=?1 AND m.owner=?2 AND m.revoked=0 AND u.revoked=0 AND o.revoked=0 AND g.revoked=0",
            params![input.membership_id,owner.owner],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(Error::NotFound)?;
        let approver = crate::approvers::load(&tx, &input.approver_id)?;
        owner.owns(&approver.owner)?;
        if approver.revoked {
            return Err(Error::Forbidden);
        }
        let existing: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM identity_approver_bindings WHERE (owner=?1 AND user_id=?2) OR approver_id=?3)",params![owner.owner,user,input.approver_id],|r|r.get(0))?;
        if existing {
            return Err(Error::Conflict("user or approver is already bound".into()));
        }
        let id = new_id("iab")?;
        tx.execute("INSERT INTO identity_approver_bindings(id,owner,user_id,membership_id,approver_id,gateway_binding_id) VALUES (?1,?2,?3,?4,?5,?6)",params![id,owner.owner,user,input.membership_id,input.approver_id,gateway])?;
        registry::record(
            &tx,
            &id,
            "approver_bindings",
            "enrolled",
            at,
            self.by(owner),
        )?;
        tx.commit()?;
        Ok(
            json!({"id":id,"user_id":user,"membership_id":input.membership_id,"approver_id":input.approver_id}),
        )
    }

    fn prepare_identity_approver(
        &self,
        subject: &Subject,
        organization: &str,
        at: i64,
    ) -> Result<(Self, RegisteredApprover)> {
        let mut db = storage::connect(&self.ledger, false)?;
        let tx = db.transaction()?;
        let context = lookup(&tx, subject, organization, at)?;
        let approver = crate::approvers::load(&tx, &context.approver_id)?;
        if approver.revoked || approver.owner != context.owner {
            return Err(E::NotFound.into());
        }
        let mut scoped = self.scoped_approver(&approver);
        scoped.identity_scope = Some(context);
        Ok((scoped, approver))
    }

    /// Identity tokens use only organization-qualified, explicitly allowlisted money routes.
    pub fn identity_request(
        &self,
        subject: &Subject,
        method: &str,
        path: &str,
        body: &[u8],
        at: i64,
    ) -> Result<Value> {
        if body.len() > 16_384 {
            return Err(Error::Invalid("body exceeds 16 KiB".into()));
        }
        if subject.expires_at <= crate::now() {
            return Err(E::InvalidToken.into());
        }
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let parts: Vec<_> = route.split('/').collect();
        let (organization, gateway_path) = match (method, parts.as_slice()) {
            ("GET", ["", "identity", "v1", "organizations", org, "approver"])
                if query.is_empty() =>
            {
                (*org, "/gateway/v1/approvers/me".to_string())
            }
            ("GET", ["", "identity", "v1", "organizations", org, "agents"]) if query.is_empty() => {
                (*org, "/gateway/v1/agents".to_string())
            }
            ("GET", ["", "identity", "v1", "organizations", org, "operations"]) => (
                *org,
                format!(
                    "/gateway/v1/operations{}",
                    if query.is_empty() {
                        String::new()
                    } else {
                        format!("?{query}")
                    }
                ),
            ),
            ("GET", ["", "identity", "v1", "organizations", org, "operations", id])
                if query.is_empty() =>
            {
                (*org, format!("/gateway/v1/operations/{id}"))
            }
            (
                "GET",
                [
                    "",
                    "identity",
                    "v1",
                    "organizations",
                    org,
                    "operations",
                    id,
                    "timeline",
                ],
            ) if query.is_empty() => (*org, format!("/gateway/v1/operations/{id}/timeline")),
            (
                "POST",
                [
                    "",
                    "identity",
                    "v1",
                    "organizations",
                    org,
                    "operations",
                    id,
                    action,
                ],
            ) if matches!(*action, "approve" | "reject") && query.is_empty() => {
                if !subject.can_approve {
                    return Err(E::InsufficientApprovalScope.into());
                }
                (*org, format!("/gateway/v1/operations/{id}/{action}"))
            }
            _ => {
                return self
                    .identity_read(subject, method, path)
                    .map_err(Error::Identity);
            }
        };
        let (scoped, approver) = self.prepare_identity_approver(subject, organization, at)?;
        crate::gateway::approver_routes(&scoped, &approver, method, &gateway_path, body, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BudgetInput,
        approvers::ApproverInput,
        operations::{Operation, OperationStage},
        provider::Scenario,
    };

    struct Lab {
        _dir: tempfile::TempDir,
        service: Service,
        owner: Actor,
        organization: String,
        membership: String,
        approver: RegisteredApprover,
        op: Operation,
    }
    impl Lab {
        fn new(two: bool) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let service = Service::open(dir.path()).unwrap();
            let at = crate::now();
            let owner = Actor {
                owner: "synthetic-operator".into(),
                role: Role::Owner,
            };
            let user = service
                .enroll_identity(
                    &owner,
                    super::super::Enrollment {
                        display_name: "Development user".into(),
                        issuer: "https://identity.example/".into(),
                        subject: "subject-a".into(),
                    },
                    at,
                )
                .unwrap();
            let org = service
                .create_identity_organization(
                    &owner,
                    super::super::OrganizationInput {
                        name: "Company".into(),
                    },
                    at,
                )
                .unwrap();
            let organization = org["id"].as_str().unwrap().to_string();
            let membership = service
                .create_identity_membership(
                    &owner,
                    super::super::MembershipInput {
                        user_id: user["user"]["id"].as_str().unwrap().into(),
                        organization_id: organization.clone(),
                        role: "viewer".into(),
                    },
                    at,
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            service
                .bind_identity_gateway(
                    &owner,
                    GatewayBindingInput {
                        organization_id: organization.clone(),
                    },
                    at,
                )
                .unwrap();
            let approver = service
                .register_approver(
                    &owner,
                    ApproverInput {
                        name: "Approver".into(),
                        currency: "USD".into(),
                        max_approval_minor: 2000,
                    },
                    at,
                )
                .unwrap()
                .approver;
            let budget = service
                .create_budget(
                    &owner,
                    BudgetInput {
                        currency: "USD".into(),
                        limit_minor: 10_000,
                        per_purchase_minor: 10_000,
                        merchants: vec!["fixture-cafe".into()],
                        expires_at: at + 3600,
                        second_approval_above_minor: two.then_some(500),
                    },
                    at,
                )
                .unwrap();
            let op=service.request_operation(&owner,serde_json::from_value(json!({"budget_id":budget.id,"kind":"refund","rail":"fixture","source":"pay_fixture_2041","amount_minor":1000,"currency":"USD","reason":"Synthetic refund","idempotency_key":"race"})).unwrap(),at).unwrap();
            Self {
                _dir: dir,
                service,
                owner,
                organization,
                membership,
                approver,
                op,
            }
        }
        fn bind(&self) {
            self.service
                .bind_identity_approver(
                    &self.owner,
                    ApproverBindingInput {
                        membership_id: self.membership.clone(),
                        approver_id: self.approver.id.clone(),
                    },
                    crate::now(),
                )
                .unwrap();
        }
        fn subject(&self) -> Subject {
            Subject {
                issuer: "https://identity.example/".into(),
                subject: "subject-a".into(),
                expires_at: crate::now() + 300,
                can_approve: true,
            }
        }
        fn scoped(&self) -> Service {
            self.service
                .prepare_identity_approver(&self.subject(), &self.organization, crate::now())
                .unwrap()
                .0
        }
        fn actor(&self) -> Actor {
            Actor {
                owner: self.owner.owner.clone(),
                role: Role::Approver,
            }
        }
        fn approve(&self, service: &Service) -> Result<Operation> {
            service.approve_operation(
                &self.actor(),
                &self.op.id,
                &self.op.proposal.as_ref().unwrap().digest().unwrap(),
                crate::now(),
            )
        }
        fn refunded(&self) -> i64 {
            self.service
                .rails
                .fixture_rail()
                .unwrap()
                .refunded_minor("pay_fixture_2041")
                .unwrap()
        }
    }

    #[test]
    fn revocation_committed_after_identity_authentication_wins_inside_approval() {
        let lab = Lab::new(false);
        lab.bind();
        let scoped = lab.scoped();
        lab.service
            .revoke_identity_record(&lab.owner, "memberships", &lab.membership, crate::now())
            .unwrap();
        assert!(lab.approve(&scoped).is_err());
        assert_eq!(lab.refunded(), 0);
        assert_eq!(
            lab.service.operation(&lab.owner, &lab.op.id).unwrap().stage,
            OperationStage::Held
        );
    }

    #[test]
    fn a_key_authenticated_before_binding_cannot_race_the_binding_transaction() {
        let lab = Lab::new(false);
        let legacy_scoped = lab.service.scoped_approver(&lab.approver);
        lab.bind();
        assert!(matches!(lab.approve(&legacy_scoped), Err(Error::Forbidden)));
        assert_eq!(lab.refunded(), 0);
    }

    #[test]
    fn revocation_between_approval_and_submission_keeps_the_hold_without_payment() {
        let lab = Lab::new(false);
        lab.bind();
        let scoped = lab.scoped();
        assert_eq!(
            lab.approve(&scoped).unwrap().stage,
            OperationStage::Approved
        );
        lab.service
            .revoke_identity_record(&lab.owner, "memberships", &lab.membership, crate::now())
            .unwrap();
        assert!(
            scoped
                .execute_operation(&lab.actor(), &lab.op.id, Scenario::Success, crate::now())
                .is_err()
        );
        // Another executor cannot bypass the revoked identity by using a different credential.
        assert!(
            lab.service
                .execute_operation(&lab.owner, &lab.op.id, Scenario::Success, crate::now())
                .is_err()
        );
        assert_eq!(lab.refunded(), 0);
        let op = lab.service.operation(&lab.owner, &lab.op.id).unwrap();
        assert_eq!(op.stage, OperationStage::Approved);
        assert!(op.attempt_key.is_none());
        assert_eq!(
            lab.service
                .balance(&lab.owner, &lab.op.request.budget_id)
                .unwrap()
                .reserved_minor,
            1000
        );
    }

    #[test]
    fn token_expiry_and_read_only_scope_are_rechecked_in_the_write_transaction() {
        let lab = Lab::new(false);
        lab.bind();
        let mut expired = lab.scoped();
        expired.identity_scope.as_mut().unwrap().subject.expires_at = crate::now() - 1;
        assert!(matches!(
            lab.approve(&expired),
            Err(Error::Identity(E::InvalidToken))
        ));
        let mut read_only = lab.scoped();
        read_only
            .identity_scope
            .as_mut()
            .unwrap()
            .subject
            .can_approve = false;
        assert!(matches!(
            lab.approve(&read_only),
            Err(Error::Identity(E::InsufficientApprovalScope))
        ));
        assert_eq!(lab.refunded(), 0);
    }

    #[test]
    fn first_identity_revocation_after_final_approval_still_blocks_submission_by_another_executor()
    {
        let lab = Lab::new(true);
        lab.bind();
        lab.approve(&lab.scoped()).unwrap();
        let at = crate::now();
        let user = lab
            .service
            .enroll_identity(
                &lab.owner,
                super::super::Enrollment {
                    display_name: "Second person".into(),
                    issuer: "https://identity.example/".into(),
                    subject: "subject-b".into(),
                },
                at,
            )
            .unwrap();
        let membership = lab
            .service
            .create_identity_membership(
                &lab.owner,
                super::super::MembershipInput {
                    user_id: user["user"]["id"].as_str().unwrap().into(),
                    organization_id: lab.organization.clone(),
                    role: "member".into(),
                },
                at,
            )
            .unwrap();
        let approver = lab
            .service
            .register_approver(
                &lab.owner,
                ApproverInput {
                    name: "Second approver".into(),
                    currency: "USD".into(),
                    max_approval_minor: 2000,
                },
                at,
            )
            .unwrap();
        lab.service
            .bind_identity_approver(
                &lab.owner,
                ApproverBindingInput {
                    membership_id: membership["id"].as_str().unwrap().into(),
                    approver_id: approver.approver.id,
                },
                at,
            )
            .unwrap();
        let mut second = lab.subject();
        second.subject = "subject-b".into();
        let (scoped, _) = lab
            .service
            .prepare_identity_approver(&second, &lab.organization, at)
            .unwrap();
        assert_eq!(
            lab.approve(&scoped).unwrap().stage,
            OperationStage::Approved
        );
        lab.service
            .revoke_identity_record(&lab.owner, "memberships", &lab.membership, crate::now())
            .unwrap();
        assert!(
            lab.service
                .execute_operation(&lab.owner, &lab.op.id, Scenario::Success, crate::now())
                .is_err()
        );
        assert!(
            scoped
                .execute_operation(&lab.actor(), &lab.op.id, Scenario::Success, crate::now())
                .is_err()
        );
        assert_eq!(lab.refunded(), 0);
        assert!(
            lab.service
                .operation(&lab.owner, &lab.op.id)
                .unwrap()
                .attempt_key
                .is_none()
        );
        assert!(lab.service.verify_evidence().unwrap().ok());
    }
}
