use cosigna_agent::{
    Actor, Budget, BudgetInput, Error, Offer, Plan, Result, Role, Service, TaskInput,
    api::{self, Credentials},
    gateway::{self, Permission, Registration},
    planner::{FixturePlanner, Planner},
    provider::Scenario,
};
use serde_json::{Value, json};

struct Setup {
    directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: Actor,
    authorization: String,
    budget: String,
    at: i64,
}
impl Setup {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path()).unwrap();
        let credentials =
            Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
        let raw: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("credentials.json")).unwrap(),
        )
        .unwrap();
        let authorization = format!("Bearer {}", raw["owner"].as_str().unwrap());
        let owner = credentials.authenticate(&authorization).unwrap();
        let at = cosigna_agent::now();
        let budget = service
            .create_budget(
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
            )
            .unwrap()
            .id;
        Self {
            directory,
            service,
            credentials,
            owner,
            authorization,
            budget,
            at,
        }
    }
    fn call(&self, auth: &str, method: &str, path: &str, body: Value) -> Result<Value> {
        gateway::dispatch(
            &self.service,
            &FixturePlanner,
            &self.credentials,
            auth,
            method,
            &format!("/gateway/v1{path}"),
            &serde_json::to_vec(&body).unwrap(),
            self.at,
        )
    }
    fn agent(&self, permissions: &[Permission]) -> (String, String) {
        let result = self.call(&self.authorization, "POST", "/agents", json!({"name":"purchase-agent", "budget_id":self.budget,"permissions":permissions,"expires_at":self.at+1800})).unwrap();
        (
            result["data"]["agent"]["id"].as_str().unwrap().into(),
            format!("Bearer {}", result["data"]["token"].as_str().unwrap()),
        )
    }
    fn draft(&self, auth: &str, key: &str) -> Value {
        self.call(
            auth,
            "POST",
            "/tasks",
            json!({"budget_id":self.budget,"request":"Order one oat latte","idempotency_key":key}),
        )
        .unwrap()
    }
    fn quote(&self, auth: &str, key: &str) -> Value {
        let task = self.draft(auth, key);
        self.call(
            auth,
            "POST",
            &format!("/tasks/{}/plan", task["data"]["id"].as_str().unwrap()),
            json!({}),
        )
        .unwrap()
    }
    fn approve(&self, quote: &Value) {
        self.call(
            &self.authorization,
            "POST",
            &format!("/tasks/{}/approve", quote["data"]["id"].as_str().unwrap()),
            json!({"quote_digest":quote["approval_digest"]}),
        )
        .unwrap();
    }
}

#[test]
fn registered_agent_plans_and_executes_only_after_exact_owner_approval_and_replay_is_safe() {
    let s = Setup::new();
    let (id, auth) = s.agent(&[Permission::Plan, Permission::Execute]);
    let quote = s.quote(&auth, "gateway-one");
    assert_eq!(quote["schema"], gateway::API_VERSION);
    assert_eq!(quote["agent_id"], id);
    let task = quote["data"]["id"].as_str().unwrap();
    assert!(
        s.call(&auth, "POST", &format!("/tasks/{task}/execute"), json!({}))
            .is_err()
    );
    assert!(
        s.call(
            &auth,
            "POST",
            &format!("/tasks/{task}/approve"),
            json!({"quote_digest":quote["approval_digest"]})
        )
        .is_err()
    );
    assert!(
        s.call(
            &s.authorization,
            "POST",
            &format!("/tasks/{task}/approve"),
            json!({"quote_digest":"tampered"})
        )
        .is_err()
    );
    s.approve(&quote);
    let result = s
        .call(&auth, "POST", &format!("/tasks/{task}/execute"), json!({}))
        .unwrap();
    assert_eq!(result["data"]["stage"], "confirmed");
    let replay = s
        .call(&auth, "POST", &format!("/tasks/{task}/execute"), json!({}))
        .unwrap();
    assert_eq!(result["data"]["attempt_key"], replay["data"]["attempt_key"]);
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    assert_eq!(
        s.service.balance(&s.owner, &s.budget).unwrap().spent_minor,
        700
    );
}

#[test]
fn credentials_are_hashed_and_metadata_can_be_recovered_without_disclosing_them() {
    let s = Setup::new();
    let (id, auth) = s.agent(&[Permission::Plan]);
    let db = rusqlite::Connection::open(s.directory.path().join("transactions.sqlite")).unwrap();
    let hash: String = db
        .query_row("SELECT token_hash FROM agents WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        hash,
        blake3::hash(auth.strip_prefix("Bearer ").unwrap().as_bytes())
            .to_hex()
            .to_string()
    );
    let metadata = s
        .call(
            &s.authorization,
            "GET",
            &format!("/agents/{id}"),
            Value::Null,
        )
        .unwrap();
    assert!(!metadata.to_string().contains("agentkey_"));
    assert!(metadata["data"].get("token_hash").is_none());
    let listed = s
        .call(&s.authorization, "GET", "/agents", Value::Null)
        .unwrap();
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains("agentkey_"));
    let reopened = Service::open(s.directory.path()).unwrap();
    assert!(
        gateway::dispatch(
            &reopened,
            &FixturePlanner,
            &s.credentials,
            &auth,
            "GET",
            "/gateway/v1/catalog",
            b"null",
            s.at
        )
        .is_ok()
    );
    assert!(
        s.credentials.authenticate(&auth).is_err(),
        "registered credentials must not access the legacy API"
    );
}

#[test]
fn agents_cannot_escalate_ownership_or_manage_authority() {
    let s = Setup::new();
    let (id, auth) = s.agent(&[Permission::Plan, Permission::Execute]);
    for path in [
        "/agents".to_owned(),
        format!("/agents/{id}/revoke"),
        "/budgets".into(),
        format!("/budgets/{}/revoke", s.budget),
    ] {
        assert!(s.call(&auth, "POST", &path, json!({})).is_err());
    }
    assert!(s.call(&auth, "GET", "/agents", Value::Null).is_err());
    assert!(s.call(&auth,"POST","/tasks",json!({"budget_id":s.budget,"request":"Order one oat latte","idempotency_key":"spoofed","owner":"another-owner","permissions":["execute"]})).is_err());
    let stranger = Actor {
        owner: "another-owner".into(),
        role: Role::Owner,
    };
    assert!(matches!(
        s.service.registered_agent(&stranger, &id),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.service.revoke_agent(&stranger, &id, s.at),
        Err(Error::NotFound)
    ));
    assert!(
        s.service
            .register_agent(
                &stranger,
                Registration {
                    name: "spoofed".into(),
                    budget_id: s.budget.clone(),
                    permissions: vec![Permission::Plan],
                    expires_at: s.at + 60,
                    operations: None,
                },
                s.at
            )
            .is_err()
    );
}

#[test]
fn same_owner_agents_cannot_access_each_others_tasks_or_replay_their_idempotency_key() {
    let s = Setup::new();
    let (_, a) = s.agent(&[Permission::Plan, Permission::Execute]);
    let (_, b) = s.agent(&[Permission::Plan, Permission::Execute]);
    let task = s.quote(&a, "private-task");
    let id = task["data"]["id"].as_str().unwrap();
    for action in ["plan", "execute", "reconcile"] {
        assert!(
            s.call(&b, "POST", &format!("/tasks/{id}/{action}"), json!({}))
                .is_err()
        );
    }
    assert!(matches!(
        s.call(&b, "GET", &format!("/tasks/{id}"), Value::Null),
        Err(Error::NotFound)
    ));
    assert!(
        s.call(&b, "GET", &format!("/tasks/{id}/events"), Value::Null)
            .is_err()
    );
    assert!(s.call(&b,"POST","/tasks",json!({"budget_id":s.budget,"request":"Order one oat latte","idempotency_key":"private-task"})).is_err());
    let replay = s.draft(&a, "private-task");
    assert_eq!(replay["data"]["id"], task["data"]["id"]);
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
}

#[test]
fn an_agent_cannot_use_another_budget_even_under_the_same_owner() {
    let s = Setup::new();
    let (_, auth) = s.agent(&[Permission::Plan]);
    let other = s
        .service
        .create_budget(
            &s.owner,
            BudgetInput {
                currency: "USD".into(),
                limit_minor: 1000,
                per_purchase_minor: 1000,
                merchants: vec!["fixture-cafe".into()],
                expires_at: s.at + 3600,
                second_approval_above_minor: None,
            },
            s.at,
        )
        .unwrap();
    assert!(
        s.call(&auth, "GET", &format!("/budgets/{}", other.id), Value::Null)
            .is_err()
    );
    assert!(s.call(&auth,"POST","/tasks",json!({"budget_id":other.id,"request":"Order one oat latte","idempotency_key":"wrong-budget"})).is_err());
    let task = s
        .service
        .create_task(
            &s.owner,
            TaskInput {
                budget_id: s.budget.clone(),
                request: "Order one oat latte".into(),
                idempotency_key: "legacy-private".into(),
                required_features: vec![],
            },
            s.at,
        )
        .unwrap();
    assert!(
        s.call(&auth, "GET", &format!("/tasks/{}", task.id), Value::Null)
            .is_err()
    );
}

#[test]
fn planning_permission_does_not_confer_execution_permission_but_owner_can_execute_explicitly() {
    let s = Setup::new();
    let (_, auth) = s.agent(&[Permission::Plan]);
    let quote = s.quote(&auth, "plan-only");
    s.approve(&quote);
    let id = quote["data"]["id"].as_str().unwrap();
    assert!(matches!(
        s.call(&auth, "POST", &format!("/tasks/{id}/execute"), json!({})),
        Err(Error::Forbidden)
    ));
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    assert_eq!(
        s.call(
            &s.authorization,
            "POST",
            &format!("/tasks/{id}/execute"),
            json!({})
        )
        .unwrap()["data"]["stage"],
        "confirmed"
    );
}

#[test]
fn revocation_blocks_already_authenticated_submission_and_owner_can_release_unsubmitted_hold() {
    let s = Setup::new();
    let (agent, auth) = s.agent(&[Permission::Plan, Permission::Execute]);
    let quote = s.quote(&auth, "revoke");
    s.approve(&quote);
    s.service.revoke_agent(&s.owner, &agent, s.at).unwrap();
    s.service.revoke_agent(&s.owner, &agent, s.at).unwrap();
    let id = quote["data"]["id"].as_str().unwrap();
    assert!(
        s.call(&auth, "GET", &format!("/tasks/{id}"), Value::Null)
            .is_err()
    );
    let executor = Actor {
        owner: s.owner.owner.clone(),
        role: Role::Executor,
    };
    assert!(
        s.service.begin_submission(&executor, id, s.at).is_err(),
        "write boundary must recheck revocation even for an existing admin session"
    );
    assert_eq!(
        s.service
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
    s.service.cancel(&s.owner, id, s.at).unwrap();
    assert_eq!(
        s.service
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    let db = rusqlite::Connection::open(s.directory.path().join("transactions.sqlite")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM agent_events WHERE kind='revoked'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn expired_agent_blocks_submission_while_owner_can_reconcile_a_prior_unknown_result() {
    let s = Setup::new();
    let (agent, auth) = s.agent(&[Permission::Plan, Permission::Execute]);
    let quote = s.quote(&auth, "unknown");
    s.approve(&quote);
    let id = quote["data"]["id"].as_str().unwrap();
    let executor = Actor {
        owner: s.owner.owner.clone(),
        role: Role::Executor,
    };
    assert!(
        s.service
            .begin_submission(&executor, id, s.at + 1800)
            .is_err()
    );
    let submitted = s
        .service
        .execute(&executor, id, Scenario::TimeoutAfterAccept, s.at)
        .unwrap();
    assert_eq!(submitted.stage, cosigna_agent::Stage::Unknown);
    s.service.revoke_agent(&s.owner, &agent, s.at).unwrap();
    let reopened = Service::open(s.directory.path()).unwrap();
    assert_eq!(
        reopened.reconcile(&s.owner, id, s.at + 1801).unwrap().stage,
        cosigna_agent::Stage::Confirmed
    );
    assert_eq!(reopened.provider.order_count().unwrap(), 1);
}

struct RevokeDuringPlan {
    service: Service,
    owner: Actor,
    agent: String,
    at: i64,
}
impl Planner for RevokeDuringPlan {
    fn plan(&self, task: &TaskInput, budget: &Budget, offers: &[Offer]) -> Result<Plan> {
        self.service
            .revoke_agent(&self.owner, &self.agent, self.at)?;
        FixturePlanner.plan(task, budget, offers)
    }
    fn name(&self) -> &str {
        "synthetic revoke during inference"
    }
}
#[test]
fn a_delayed_model_cannot_apply_a_plan_after_the_agent_is_revoked() {
    let s = Setup::new();
    let (agent, auth) = s.agent(&[Permission::Plan]);
    let draft = s.draft(&auth, "slow-model");
    let id = draft["data"]["id"].as_str().unwrap();
    let planner = RevokeDuringPlan {
        service: s.service.clone(),
        owner: s.owner.clone(),
        agent,
        at: s.at,
    };
    assert!(
        gateway::dispatch(
            &s.service,
            &planner,
            &s.credentials,
            &auth,
            "POST",
            &format!("/gateway/v1/tasks/{id}/plan"),
            b"{}",
            s.at
        )
        .is_err()
    );
    let task = s.service.task(&s.owner, id).unwrap();
    assert_eq!(task.stage, cosigna_agent::Stage::Draft);
    assert!(task.quote.is_none());
    assert_eq!(
        s.service
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
}

#[test]
fn ledger_v1_upgrade_preserves_approved_tasks_and_reservations_and_refuses_future_versions() {
    let s = Setup::new();
    let task = s
        .service
        .create_task(
            &s.owner,
            TaskInput {
                budget_id: s.budget.clone(),
                request: "Order one oat latte".into(),
                idempotency_key: "pre-upgrade".into(),
                required_features: vec![],
            },
            s.at,
        )
        .unwrap();
    let quote = s
        .service
        .plan(&s.owner, &task.id, &FixturePlanner, s.at)
        .unwrap();
    let digest = quote.quote.as_ref().unwrap().digest().unwrap();
    s.service
        .approve(&s.owner, &task.id, &digest, s.at)
        .unwrap();
    // Reproduce the old schema only in this synthetic store; never downgrade user data.
    let db = rusqlite::Connection::open(s.directory.path().join("transactions.sqlite")).unwrap();
    db.execute_batch("DROP TABLE approvers; DROP TABLE operation_holds; DROP TABLE operations; DROP TABLE evidence; DROP TABLE worker_jobs; DROP TABLE task_agents; DROP TABLE agent_events; DROP TABLE agents; PRAGMA user_version=1;").unwrap();
    drop(db);
    let upgraded = Service::open(s.directory.path()).unwrap();
    assert_eq!(
        upgraded
            .task(&s.owner, &task.id)
            .unwrap()
            .approved_digest
            .as_deref(),
        Some(digest.as_str())
    );
    assert_eq!(
        upgraded
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
    let db = rusqlite::Connection::open(s.directory.path().join("transactions.sqlite")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM agents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.pragma_update(None, "user_version", 9).unwrap();
    drop(db);
    assert!(matches!(
        Service::open(s.directory.path()),
        Err(Error::StoreFormat)
    ));
}

#[test]
fn invalid_registration_and_unknown_permissions_fail_without_creating_agents() {
    let s = Setup::new();
    for permissions in [json!([]), json!(["approve"]), json!(["plan", "plan"])] {
        assert!(s.call(&s.authorization,"POST","/agents",json!({"name":"agent","budget_id":s.budget,"permissions":permissions,"expires_at":s.at+60})).is_err());
    }
    assert!(s.call(&s.authorization,"POST","/agents",json!({"name":"agent","budget_id":s.budget,"permissions":["plan"],"expires_at":s.at+3601})).is_err());
    assert_eq!(s.service.registered_agents(&s.owner).unwrap().len(), 0);
    let raw: Value = serde_json::from_slice(
        &std::fs::read(s.directory.path().join("credentials.json")).unwrap(),
    )
    .unwrap();
    assert!(
        s.call(
            &format!("Bearer {}", raw["planner"].as_str().unwrap()),
            "GET",
            "/catalog",
            Value::Null
        )
        .is_err()
    );
    assert!(
        api::dispatch(
            &s.service,
            &FixturePlanner,
            &Actor {
                owner: s.owner.owner.clone(),
                role: Role::Planner
            },
            "POST",
            "/v1/tasks/nonexistent/approve",
            br#"{"quote_digest":"x"}"#,
            s.at
        )
        .is_err()
    );
}

#[test]
fn gateway_demo_is_headless_offline_and_contains_no_credential_or_live_payment_claim() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .arg("gateway-demo")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "cosigna.gateway-demo/v1");
    assert_eq!(report["simulated"], true);
    assert_eq!(report["agent"]["revoked"], true);
    assert_eq!(report["synthetic_orders"], 1);
    assert_eq!(report["balance"]["spent_minor"], 700);
    assert_eq!(report["balance"]["available_minor"], 300);
    assert!(
        report["checks"]
            .as_object()
            .unwrap()
            .values()
            .all(|value| value == true)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("agentkey_"));
    assert!(!text.contains("token_hash"));
    assert_eq!(report["approval_source"], "synthetic-owner-test-driver");
}

#[test]
fn two_registered_agents_cannot_reserve_the_same_remaining_budget() {
    let s = Setup::new();
    let (_, first) = s.agent(&[Permission::Plan, Permission::Execute]);
    let (_, second) = s.agent(&[Permission::Plan, Permission::Execute]);
    let quotes = [s.quote(&first, "race-one"), s.quote(&second, "race-two")];
    let barrier = std::sync::Barrier::new(2);
    let accepted = std::thread::scope(|scope| {
        let handles: Vec<_> = quotes
            .iter()
            .map(|quote| {
                let state = &s;
                let start = &barrier;
                scope.spawn(move || {
                    start.wait();
                    state
                        .call(
                            &state.authorization,
                            "POST",
                            &format!("/tasks/{}/approve", quote["data"]["id"].as_str().unwrap()),
                            json!({"quote_digest":quote["approval_digest"]}),
                        )
                        .is_ok()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(accepted, 1);
    let balance = s.service.balance(&s.owner, &s.budget).unwrap();
    assert_eq!(balance.reserved_minor, 700);
    assert_eq!(balance.available_minor, 300);
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
}
