use cosigna_agent::{
    Actor, Error, Role, Service, api::Credentials, gateway, operations::OperationStage,
    provider::Scenario,
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

struct Lab {
    directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
}

impl Lab {
    /// A $1,000 budget with at most $500 per operation.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path()).unwrap();
        let credentials =
            Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
        let private: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("credentials.json")).unwrap(),
        )
        .unwrap();
        let owner = format!("Bearer {}", private["owner"].as_str().unwrap());
        let mut lab = Self {
            directory,
            service,
            credentials,
            owner,
            budget: String::new(),
        };
        lab.budget = lab
            .call(
                &lab.owner.clone(),
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,
                "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":AT+86400*2}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        lab
    }

    fn call(&self, auth: &str, method: &str, path: &str, body: Value) -> Result<Value, Error> {
        let bytes = if method == "GET" {
            Vec::new()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        gateway::dispatch(
            &self.service,
            &cosigna_agent::planner::FixturePlanner,
            &self.credentials,
            auth,
            method,
            &format!("/gateway/v1{path}"),
            &bytes,
            AT,
        )
    }

    /// Registers an agent and returns (bearer, agent id). `grant` is the operations grant.
    fn agent(&self, permissions: Value, grant: Value) -> (String, String) {
        let mut body = json!({"name":"refunds-bot","budget_id":self.budget,
            "permissions":permissions,"expires_at":AT+86400});
        if !grant.is_null() {
            body["operations"] = grant;
        }
        let issued = self.call(&self.owner, "POST", "/agents", body).unwrap();
        (
            format!("Bearer {}", issued["data"]["token"].as_str().unwrap()),
            issued["data"]["agent"]["id"].as_str().unwrap().to_string(),
        )
    }

    fn refunds_bot(&self) -> (String, String) {
        self.agent(
            json!(["execute"]),
            json!({"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":10000}),
        )
    }

    fn refund(&self, auth: &str, source: &str, amount: i64, key: &str) -> Result<Value, Error> {
        self.call(
            auth,
            "POST",
            "/operations",
            json!({"budget_id":self.budget,"kind":"refund",
            "rail":"fixture","source":source,"amount_minor":amount,"currency":"USD",
            "reason":"Customer cancelled before shipping","idempotency_key":key}),
        )
    }

    fn available(&self) -> i64 {
        self.call(
            &self.owner,
            "GET",
            &format!("/budgets/{}", self.budget),
            Value::Null,
        )
        .unwrap()["data"]["available_minor"]
            .as_i64()
            .unwrap()
    }

    fn refunded(&self, payment: &str) -> i64 {
        self.service
            .rails
            .fixture_rail()
            .unwrap()
            .refunded_minor(payment)
            .unwrap()
    }

    fn owner_actor(&self) -> Actor {
        Actor {
            owner: "local-user".into(),
            role: Role::Owner,
        }
    }

    fn events(&self) -> Vec<Value> {
        let db =
            rusqlite::Connection::open(self.directory.path().join("transactions.sqlite")).unwrap();
        let mut stmt = db
            .prepare("SELECT body FROM evidence ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|b| serde_json::from_str::<Value>(&b.unwrap()).unwrap()["event"].clone())
            .collect()
    }
}

#[test]
fn refunds_within_the_agents_limit_are_approved_and_executed_without_the_owner() {
    let lab = Lab::new();
    let (bot, agent_id) = lab.refunds_bot();
    let done = lab.refund(&bot, "pay_fixture_2041", 2000, "r1").unwrap();
    assert_eq!(done["schema"], "cosigna.operation/v1");
    assert_eq!(done["data"]["stage"], "succeeded");
    assert_eq!(done["data"]["approved_by"], "policy");
    assert_eq!(
        done["data"]["decision"],
        json!({"outcome":"auto_approved","rule":"within_agent_auto_limit"})
    );
    assert_eq!(done["data"]["agent"], json!(agent_id));
    assert_eq!(done["needs_owner"], false);
    assert_eq!(done["data"]["proposal"]["facts"]["refundable_minor"], 2000);
    assert_eq!(lab.refunded("pay_fixture_2041"), 2000);
    assert_eq!(lab.available(), 98000);

    // A replay of the same request returns the same operation and moves no more money.
    let replay = lab.refund(&bot, "pay_fixture_2041", 2000, "r1").unwrap();
    assert_eq!(replay["data"]["id"], done["data"]["id"]);
    assert_eq!(lab.refunded("pay_fixture_2041"), 2000);
    // The same key for a different request is refused.
    assert!(matches!(
        lab.refund(&bot, "pay_fixture_2041", 1000, "r1"),
        Err(Error::Conflict(_))
    ));

    let kinds: Vec<String> = lab
        .events()
        .iter()
        .filter(|e| e["operation"].is_string())
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "gateway.operation.auto_approved",
            "gateway.operation.submission_started",
            "gateway.operation.rail_result"
        ]
    );
    let first = lab
        .events()
        .into_iter()
        .find(|e| e["kind"] == "gateway.operation.auto_approved")
        .unwrap();
    assert_eq!(first["by"]["agent"], json!(agent_id));
    assert_eq!(first["hold"], json!({"amount_minor":2000,"state":"held"}));
    assert!(
        !serde_json::to_string(&lab.events())
            .unwrap()
            .contains("Customer cancelled")
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn refunds_above_the_limit_wait_for_the_owner_and_only_the_owner_can_approve() {
    let lab = Lab::new();
    let (bot, _) = lab.refunds_bot();
    let held = lab.refund(&bot, "pay_fixture_2045", 45000, "big").unwrap();
    assert_eq!(held["data"]["stage"], "held");
    assert_eq!(held["needs_owner"], true);
    assert_eq!(held["data"]["decision"]["rule"], "needs_owner_approval");
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);
    // Waiting holds no budget yet.
    assert_eq!(lab.available(), 100000);
    let id = held["data"]["id"].as_str().unwrap();
    let digest = held["proposal_digest"].as_str().unwrap();

    // The agent can neither approve nor execute its own held request.
    assert!(
        lab.call(
            &bot,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":digest})
        )
        .is_err()
    );
    assert!(matches!(
        lab.call(
            &bot,
            "POST",
            &format!("/operations/{id}/execute"),
            json!({})
        ),
        Err(Error::Conflict(_))
    ));
    // A wrong digest is refused.
    assert!(
        lab.call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":"0".repeat(64)})
        )
        .is_err()
    );

    let inbox = lab
        .call(&lab.owner, "GET", "/operations?stage=held", Value::Null)
        .unwrap();
    assert_eq!(inbox["data"].as_array().unwrap().len(), 1);

    let approved = lab
        .call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":digest}),
        )
        .unwrap();
    assert_eq!(approved["data"]["stage"], "succeeded");
    assert_eq!(approved["data"]["approved_by"], "owner");
    assert_eq!(lab.refunded("pay_fixture_2045"), 45000);
    assert_eq!(lab.available(), 55000);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn invalid_and_unauthorized_requests_are_denied_and_recorded_without_touching_the_rail() {
    let lab = Lab::new();
    let (bot, _) = lab.refunds_bot();
    let (reader, _) = lab.agent(json!(["plan", "execute"]), Value::Null);
    let (payouts_only, _) = lab.agent(
        json!(["execute"]),
        json!({"kinds":["payout"],"rails":["fixture"],"auto_approve_minor":10000}),
    );
    let cases = [
        (&bot, "pay_fixture_missing", 100, "source_not_found"),
        (&bot, "pay_fixture_2041", 2001, "exceeds_refundable"),
        (&bot, "pay_fixture_3001", 100, "currency_mismatch"),
        (&bot, "pay_fixture_2049", 64000, "over_per_operation_limit"),
        (&reader, "pay_fixture_2041", 100, "not_granted"),
        (&payouts_only, "pay_fixture_2041", 100, "not_granted"),
        // An agent without rights learns nothing from the rail: not whether the payment
        // exists, its currency, or how much is left to refund.
        (&reader, "pay_fixture_missing", 100, "not_granted"),
        (&reader, "pay_fixture_2041", 2001, "not_granted"),
        (&payouts_only, "pay_fixture_3001", 100, "not_granted"),
    ];
    for (i, (auth, source, amount, rule)) in cases.iter().enumerate() {
        let denied = lab.refund(auth, source, *amount, &format!("d{i}")).unwrap();
        assert_eq!(denied["data"]["stage"], "denied", "{rule}");
        assert_eq!(
            denied["data"]["decision"],
            json!({"outcome":"denied","rule":rule})
        );
        assert!(denied["data"]["proposal"].is_null());
    }
    assert_eq!(
        lab.service
            .rails
            .fixture_rail()
            .unwrap()
            .attempt_count()
            .unwrap(),
        0
    );
    assert_eq!(lab.available(), 100000);
    let denials = lab
        .events()
        .iter()
        .filter(|e| e["kind"] == "gateway.operation.denied")
        .count();
    assert_eq!(denials, cases.len());
    assert!(lab.service.verify_evidence().unwrap().ok());

    // Agents with an operations grant must hold `execute`; a malformed grant is refused.
    for (permissions, grant) in [
        (
            json!(["plan"]),
            json!({"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":0}),
        ),
        (
            json!(["execute"]),
            json!({"kinds":[],"rails":["fixture"],"auto_approve_minor":0}),
        ),
        (
            json!(["execute"]),
            json!({"kinds":["refund","refund"],"rails":["fixture"],"auto_approve_minor":0}),
        ),
        (
            json!(["execute"]),
            json!({"kinds":["refund"],"rails":["bad rail!"],"auto_approve_minor":0}),
        ),
        (
            json!(["execute"]),
            json!({"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":-1}),
        ),
    ] {
        let body = json!({"name":"x","budget_id":lab.budget,"permissions":permissions,
            "expires_at":AT+60,"operations":grant});
        assert!(matches!(
            lab.call(&lab.owner, "POST", "/agents", body),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn money_cannot_be_refunded_twice_even_when_two_approvals_race() {
    let lab = Lab::new();
    let (bot, _) = lab.refunds_bot();
    // Two held full refunds of the same $60 payment, both approved by the owner.
    let one = lab.refund(&bot, "pay_fixture_2047", 6000, "a").unwrap();
    let two = lab.refund(&bot, "pay_fixture_2047", 6000, "b").unwrap();
    assert_eq!(
        one["data"]["stage"], "succeeded",
        "within the $100 automatic limit"
    );
    assert_eq!(two["data"]["stage"], "denied");
    assert_eq!(two["data"]["decision"]["rule"], "exceeds_refundable");

    // Race through the library: approval of both before either executes.
    let owner = lab.owner_actor();
    let request = |key: &str| cosigna_agent::operations::OperationRequest {
        budget_id: lab.budget.clone(),
        kind: cosigna_agent::operations::OperationKind::Refund,
        rail: "fixture".into(),
        source: Some("pay_fixture_2051".into()),
        payee: None,
        amount_minor: 9900,
        currency: "USD".into(),
        reason: "Damaged in transit".into(),
        idempotency_key: key.into(),
        case_id: None,
    };
    let first = lab
        .service
        .request_operation(&owner, request("x"), AT)
        .unwrap();
    let second = lab
        .service
        .request_operation(&owner, request("y"), AT)
        .unwrap();
    for op in [&first, &second] {
        assert_eq!(
            op.stage,
            OperationStage::Held,
            "owner requests always wait for approval"
        );
        let digest = op.proposal.as_ref().unwrap().digest().unwrap();
        lab.service
            .approve_operation(&owner, &op.id, &digest, AT)
            .unwrap();
    }
    let a = lab
        .service
        .execute_operation(&owner, &first.id, Scenario::Success, AT)
        .unwrap();
    let b = lab
        .service
        .execute_operation(&owner, &second.id, Scenario::Success, AT)
        .unwrap();
    assert_eq!(a.stage, OperationStage::Succeeded);
    assert_eq!(
        b.stage,
        OperationStage::Failed,
        "the rail re-checks what is still refundable"
    );
    assert_eq!(lab.refunded("pay_fixture_2051"), 9900);
    assert_eq!(lab.available(), 100000 - 6000 - 9900);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn an_unknown_result_is_held_until_resolved_through_the_rail() {
    let lab = Lab::new();
    let owner = lab.owner_actor();
    let request = |source: &str, key: &str| cosigna_agent::operations::OperationRequest {
        budget_id: lab.budget.clone(),
        kind: cosigna_agent::operations::OperationKind::Refund,
        rail: "fixture".into(),
        source: Some(source.into()),
        payee: None,
        amount_minor: 2000,
        currency: "USD".into(),
        reason: "r".into(),
        idempotency_key: key.into(),
        case_id: None,
    };
    for (source, scenario, settled, refunded) in [
        (
            "pay_fixture_2049",
            Scenario::TimeoutBeforeAccept,
            OperationStage::Failed,
            0,
        ),
        (
            "pay_fixture_2047",
            Scenario::TimeoutAfterAccept,
            OperationStage::Succeeded,
            2000,
        ),
    ] {
        let op = lab
            .service
            .request_operation(&owner, request(source, source), AT)
            .unwrap();
        let digest = op.proposal.as_ref().unwrap().digest().unwrap();
        lab.service
            .approve_operation(&owner, &op.id, &digest, AT)
            .unwrap();
        let unknown = lab
            .service
            .execute_operation(&owner, &op.id, scenario, AT)
            .unwrap();
        assert_eq!(unknown.stage, OperationStage::Unknown);
        let before = lab.available();
        if matches!(scenario, Scenario::TimeoutBeforeAccept) {
            // Lookup alone cannot prove absence; the hold stays and rejection is refused.
            assert_eq!(
                lab.service
                    .reconcile_operation(&owner, &op.id, AT)
                    .unwrap()
                    .stage,
                OperationStage::Unknown
            );
            assert!(matches!(
                lab.service.reject_operation(&owner, &op.id, AT),
                Err(Error::Conflict(_))
            ));
        }
        let resolved = lab
            .call(
                &lab.owner,
                "POST",
                &format!("/operations/{}/resolve", op.id),
                json!({}),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_value::<OperationStage>(resolved["data"]["stage"].clone()).unwrap(),
            settled
        );
        assert_eq!(lab.refunded(source), refunded);
        if settled == OperationStage::Failed {
            assert_eq!(lab.available(), before + 2000);
            // The original attempt arriving late is refused by the rail.
            let late = lab
                .service
                .rails
                .submit(
                    unknown.attempt_key.as_deref().unwrap(),
                    unknown.proposal.as_ref().unwrap(),
                    Scenario::Success,
                )
                .unwrap();
            assert!(matches!(late, cosigna_agent::rails::RailOutcome::Voided));
            assert_eq!(lab.refunded(source), 0);
        } else {
            assert_eq!(lab.available(), before);
        }
    }
    // Resolve is owner-only.
    let (bot, _) = lab.refunds_bot();
    let any = lab.refund(&bot, "pay_fixture_2051", 100, "z").unwrap();
    let id = any["data"]["id"].as_str().unwrap();
    assert!(
        lab.call(
            &bot,
            "POST",
            &format!("/operations/{id}/resolve"),
            json!({})
        )
        .is_err()
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn owner_rejection_revocation_and_shared_budgets_are_enforced() {
    let lab = Lab::new();
    let owner = lab.owner_actor();
    let (bot, agent_id) = lab.refunds_bot();
    let held = lab.refund(&bot, "pay_fixture_2045", 45000, "h").unwrap();
    let id = held["data"]["id"].as_str().unwrap().to_string();
    let rejected = lab
        .call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/reject"),
            json!({}),
        )
        .unwrap();
    assert_eq!(rejected["data"]["stage"], "rejected");
    assert!(
        lab.call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest": held["proposal_digest"]})
        )
        .is_err()
    );

    // Another agent cannot see this agent's operations.
    let (other, _) = lab.refunds_bot();
    assert!(matches!(
        lab.call(&other, "GET", &format!("/operations/{id}"), Value::Null),
        Err(Error::NotFound)
    ));

    // A revoked agent can no longer request.
    lab.call(
        &lab.owner,
        "POST",
        &format!("/agents/{agent_id}/revoke"),
        json!({}),
    )
    .unwrap();
    assert!(
        lab.refund(&bot, "pay_fixture_2041", 100, "after-revoke")
            .is_err()
    );

    // Operations and purchases share one budget: a purchase hold reduces what can be refunded.
    let task = lab
        .service
        .create_task(
            &owner,
            cosigna_agent::TaskInput {
                budget_id: lab.budget.clone(),
                request: "Order one oat latte".into(),
                idempotency_key: "coffee".into(),
                required_features: vec![],
            },
            AT,
        )
        .unwrap();
    let quoted = lab
        .service
        .plan(
            &owner,
            &task.id,
            &cosigna_agent::planner::FixturePlanner,
            AT,
        )
        .unwrap();
    lab.service
        .approve(
            &owner,
            &task.id,
            &quoted.quote.unwrap().digest().unwrap(),
            AT,
        )
        .unwrap();
    assert_eq!(lab.available(), 100000 - 700);

    // A revoked budget stops execution of an approved operation.
    let op = lab
        .service
        .request_operation(
            &owner,
            cosigna_agent::operations::OperationRequest {
                budget_id: lab.budget.clone(),
                kind: cosigna_agent::operations::OperationKind::Refund,
                rail: "fixture".into(),
                source: Some("pay_fixture_2041".into()),
                payee: None,
                amount_minor: 500,
                currency: "USD".into(),
                reason: "r".into(),
                idempotency_key: "pre-revoke".into(),
                case_id: None,
            },
            AT,
        )
        .unwrap();
    let digest = op.proposal.as_ref().unwrap().digest().unwrap();
    let op = lab
        .service
        .approve_operation(&owner, &op.id, &digest, AT)
        .unwrap();
    assert_eq!(op.stage, OperationStage::Approved);
    lab.call(
        &lab.owner,
        "POST",
        &format!("/budgets/{}/revoke", lab.budget),
        json!({}),
    )
    .unwrap();
    assert!(matches!(
        lab.service
            .execute_operation(&owner, &op.id, Scenario::Success, AT),
        Err(Error::Conflict(_))
    ));
    assert_eq!(lab.refunded("pay_fixture_2041"), 0);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn a_revoked_agents_waiting_refunds_cannot_be_approved_or_sent_but_can_be_rejected() {
    let lab = Lab::new();
    let owner = lab.owner_actor();
    let (bot, agent_id) = lab.refunds_bot();
    // One refund waits for a person; another is approved but not yet sent.
    let held = lab
        .refund(&bot, "pay_fixture_2049", 30000, "waiting")
        .unwrap();
    let held_id = held["data"]["id"].as_str().unwrap().to_string();
    let other = lab
        .refund(&bot, "pay_fixture_2045", 20000, "approved")
        .unwrap();
    let other_id = other["data"]["id"].as_str().unwrap().to_string();
    let digest = other["proposal_digest"].as_str().unwrap().to_string();
    let approved = lab
        .service
        .approve_operation(&owner, &other_id, &digest, AT)
        .unwrap();
    assert_eq!(approved.stage, OperationStage::Approved);

    lab.call(
        &lab.owner,
        "POST",
        &format!("/agents/{agent_id}/revoke"),
        json!({}),
    )
    .unwrap();

    let approve = lab.call(
        &lab.owner,
        "POST",
        &format!("/operations/{held_id}/approve"),
        json!({"proposal_digest": held["proposal_digest"]}),
    );
    assert!(
        matches!(&approve, Err(Error::Conflict(m)) if m.starts_with("agent_inactive")),
        "{approve:?}"
    );
    assert!(matches!(
        lab.service
            .execute_operation(&owner, &other_id, Scenario::Success, AT),
        Err(Error::Conflict(m)) if m.starts_with("agent_inactive")
    ));
    assert_eq!(lab.refunded("pay_fixture_2049"), 0);
    assert_eq!(lab.refunded("pay_fixture_2045"), 0);

    // Rejecting still works and releases the approved one's hold.
    for id in [&held_id, &other_id] {
        let rejected = lab
            .call(
                &lab.owner,
                "POST",
                &format!("/operations/{id}/reject"),
                json!({}),
            )
            .unwrap();
        assert_eq!(rejected["data"]["stage"], "rejected");
    }
    assert_eq!(lab.available(), 100000);
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn a_hold_changed_outside_the_service_is_detected() {
    let lab = Lab::new();
    let (bot, _) = lab.refunds_bot();
    lab.refund(&bot, "pay_fixture_2041", 2000, "t").unwrap();
    assert!(lab.service.verify_evidence().unwrap().ok());
    rusqlite::Connection::open(lab.directory.path().join("transactions.sqlite"))
        .unwrap()
        .execute_batch("UPDATE operation_holds SET state='released'")
        .unwrap();
    let report = lab.service.verify_evidence().unwrap();
    assert!(!report.state_matches);
    assert!(
        report
            .exceptions
            .iter()
            .any(|e| e.problem == "budget hold changed without a record")
    );
}

#[test]
fn a_v4_ledger_upgrades_with_its_agents_evidence_intact_and_no_new_rights() {
    let lab = Lab::new();
    let (old_agent, _) = lab.agent(json!(["plan", "execute"]), Value::Null);
    assert!(lab.service.verify_evidence().unwrap().ok());
    // Reproduce a version 4 ledger in this synthetic store only.
    rusqlite::Connection::open(lab.directory.path().join("transactions.sqlite"))
        .unwrap()
        .execute_batch("DROP TABLE monitor_findings; DROP TABLE monitor_state; DROP TABLE alert_outbox; DROP TABLE alert_cursor; DROP TABLE identity_approver_bindings; DROP TABLE identity_gateway_bindings; DROP TABLE identity_memberships; DROP TABLE identity_subjects; DROP TABLE identity_organizations; DROP TABLE identity_users; DROP TABLE approvers; DROP TABLE operation_holds; DROP TABLE operations; ALTER TABLE agents DROP COLUMN operation_grant; PRAGMA user_version=4;")
        .unwrap();
    let reopened = Service::open(lab.directory.path()).unwrap();
    assert!(
        reopened.verify_evidence().unwrap().ok(),
        "earlier agent digests still match"
    );
    let lab = Lab {
        service: reopened,
        ..lab
    };
    let denied = lab
        .refund(&old_agent, "pay_fixture_2041", 100, "old")
        .unwrap();
    assert_eq!(denied["data"]["decision"]["rule"], "not_granted");
    assert!(lab.service.verify_evidence().unwrap().ok());
}

#[test]
fn a_repeated_owner_approval_after_payment_returns_the_paid_operation_and_pays_nothing_more() {
    let lab = Lab::new();
    let (bot, _) = lab.refunds_bot();
    // Two clicks on "approve": the second arrives after the first has already paid.
    let held = lab
        .refund(&bot, "pay_fixture_2045", 45000, "double-click")
        .unwrap();
    let id = held["data"]["id"].as_str().unwrap().to_string();
    let digest = held["proposal_digest"].as_str().unwrap().to_string();
    let approve = |digest: &str| {
        lab.call(
            &lab.owner,
            "POST",
            &format!("/operations/{id}/approve"),
            json!({"proposal_digest":digest}),
        )
    };
    assert_eq!(approve(&digest).unwrap()["data"]["stage"], "succeeded");
    let again = approve(&digest).unwrap();
    assert_eq!(again["data"]["stage"], "succeeded");
    assert_eq!(lab.refunded("pay_fixture_2045"), 45000, "paid once");
    // Any other digest is still refused once the operation is decided.
    assert!(matches!(approve(&"0".repeat(64)), Err(Error::Conflict(_))));
    // An automatic approval is not the owner's: approving its digest afterwards is refused.
    let auto = lab.refund(&bot, "pay_fixture_2041", 2000, "auto").unwrap();
    assert_eq!(auto["data"]["stage"], "succeeded");
    let auto_id = auto["data"]["id"].as_str().unwrap();
    assert!(matches!(
        lab.call(
            &lab.owner,
            "POST",
            &format!("/operations/{auto_id}/approve"),
            json!({"proposal_digest":auto["proposal_digest"]}),
        ),
        Err(Error::Conflict(_))
    ));
}
