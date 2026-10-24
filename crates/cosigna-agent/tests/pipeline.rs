use cosigna_agent::{
    Actor, Budget, BudgetInput, Offer, Plan, Result, Role, Service, Stage, TaskInput,
    api::Credentials,
    gateway::{self, Permission, Registration},
    pipeline::{self, Status},
    planner::{FixturePlanner, Planner},
    provider::Scenario,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingPlanner(AtomicUsize);
impl Planner for CountingPlanner {
    fn plan(&self, task: &TaskInput, budget: &Budget, offers: &[Offer]) -> Result<Plan> {
        self.0.fetch_add(1, Ordering::SeqCst);
        FixturePlanner.plan(task, budget, offers)
    }
    fn name(&self) -> &str {
        "counted fixtures"
    }
}
struct Setup {
    directory: tempfile::TempDir,
    service: Service,
    owner: Actor,
    agent: gateway::IssuedAgent,
    budget: String,
    at: i64,
}
impl Setup {
    fn new(permissions: Vec<Permission>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path()).unwrap();
        let at = cosigna_agent::now();
        let owner = Actor {
            owner: "pipeline-owner".into(),
            role: Role::Owner,
        };
        let budget = service
            .create_budget(
                &owner,
                BudgetInput {
                    currency: "USD".into(),
                    limit_minor: 1000,
                    per_purchase_minor: 1000,
                    merchants: vec!["fixture-cafe".into()],
                    expires_at: at + 3600,
                },
                at,
            )
            .unwrap()
            .id;
        let agent = service
            .register_agent(
                &owner,
                Registration {
                    name: "pipeline-test".into(),
                    budget_id: budget.clone(),
                    permissions,
                    expires_at: at + 1800,
                    operations: None,
                },
                at,
            )
            .unwrap();
        Self {
            directory,
            service,
            owner,
            agent,
            budget,
            at,
        }
    }
    fn credentials(&self) -> Credentials {
        Credentials::initialize(&self.directory.path().join("credentials.json")).unwrap()
    }
    fn call(&self, planner: &dyn Planner, method: &str, path: &str, body: Value) -> Result<Value> {
        gateway::dispatch(
            &self.service,
            planner,
            &self.credentials(),
            &format!("Bearer {}", self.agent.token),
            method,
            &format!("/gateway/v1{path}"),
            &serde_json::to_vec(&body).unwrap(),
            self.at,
        )
    }
    fn start(&self, planner: &dyn Planner, key: &str, request: &str) -> Value {
        self.call(
            planner,
            "POST",
            "/pipelines",
            json!({"budget_id":self.budget,"request":request,"idempotency_key":key}),
        )
        .unwrap()
    }
    fn approve(&self, pipeline: &Value) {
        self.service
            .approve(
                &self.owner,
                pipeline["pipeline_id"].as_str().unwrap(),
                pipeline["approval_digest"].as_str().unwrap(),
                self.at,
            )
            .unwrap();
    }
}

#[test]
fn pipeline_waits_for_owner_then_resumes_after_reopen_and_replays_without_another_order() {
    let s = Setup::new(vec![Permission::Plan, Permission::Execute]);
    let planner = CountingPlanner(AtomicUsize::new(0));
    let started = s.start(&planner, "restart", "Order one oat latte");
    let id = started["pipeline_id"].as_str().unwrap();
    assert_eq!(started["schema"], pipeline::API_VERSION);
    assert_eq!(started["status"], "awaiting_approval");
    assert_eq!(started["agent_id"], s.agent.agent.id);
    assert_eq!(started["next_action"], "owner_approve_quote");
    for _ in 0..3 {
        assert_eq!(
            s.call(
                &planner,
                "POST",
                &format!("/pipelines/{id}/advance"),
                json!({})
            )
            .unwrap()["status"],
            "awaiting_approval"
        );
    }
    assert_eq!(
        s.start(&planner, "restart", "Order one oat latte")["pipeline_id"],
        id
    );
    assert_eq!(planner.0.load(Ordering::SeqCst), 1);
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    s.approve(&started);
    let reopened = Service::open(s.directory.path()).unwrap();
    let call = || {
        gateway::dispatch(
            &reopened,
            &planner,
            &s.credentials(),
            &format!("Bearer {}", s.agent.token),
            "POST",
            &format!("/gateway/v1/pipelines/{id}/advance"),
            b"{}",
            s.at,
        )
        .unwrap()
    };
    let complete = call();
    assert_eq!(complete["status"], "confirmed");
    assert_eq!(complete["next_action"], "none");
    assert_eq!(
        call()["data"]["attempt_key"],
        complete["data"]["attempt_key"]
    );
    assert_eq!(reopened.provider.order_count().unwrap(), 1);
    assert_eq!(planner.0.load(Ordering::SeqCst), 1);
}

#[test]
fn unclear_input_never_loops_inference_or_grants_approval() {
    let s = Setup::new(vec![Permission::Plan, Permission::Execute]);
    let planner = CountingPlanner(AtomicUsize::new(0));
    let started = s.start(&planner, "clarify", "Something nice");
    let id = started["pipeline_id"].as_str().unwrap();
    for _ in 0..3 {
        assert_eq!(
            s.call(
                &planner,
                "POST",
                &format!("/pipelines/{id}/advance"),
                json!({})
            )
            .unwrap()["status"],
            "needs_input"
        );
    }
    assert_eq!(
        s.start(&planner, "clarify", "Something nice")["status"],
        "needs_input"
    );
    assert_eq!(planner.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.service
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
}

#[test]
fn response_loss_is_reconciled_even_after_quote_expiry_and_revocation() {
    let s = Setup::new(vec![Permission::Plan, Permission::Execute]);
    let started = s.start(&FixturePlanner, "lost", "Order one oat latte");
    s.approve(&started);
    let id = started["pipeline_id"].as_str().unwrap();
    let executor = Actor {
        owner: s.owner.owner.clone(),
        role: Role::Executor,
    };
    assert_eq!(
        s.service
            .execute(&executor, id, Scenario::TimeoutAfterAccept, s.at)
            .unwrap()
            .stage,
        Stage::Unknown
    );
    s.service
        .revoke_agent(&s.owner, &s.agent.agent.id, s.at)
        .unwrap();
    let reopened = Service::open(s.directory.path()).unwrap();
    assert_eq!(
        pipeline::inspect(&reopened, &s.owner, id, s.at + 600)
            .unwrap()
            .status,
        Status::Reconciling
    );
    assert_eq!(
        pipeline::advance(&reopened, &s.owner, &FixturePlanner, id, s.at + 600)
            .unwrap()
            .status,
        Status::Confirmed
    );
    assert_eq!(reopened.provider.order_count().unwrap(), 1);
    assert_eq!(
        reopened.balance(&s.owner, &s.budget).unwrap().spent_minor,
        700
    );
}

#[test]
fn absent_provider_result_keeps_reservation_and_never_resubmits() {
    let s = Setup::new(vec![Permission::Plan, Permission::Execute]);
    let started = s.start(&FixturePlanner, "absent", "Order one oat latte");
    s.approve(&started);
    let id = started["pipeline_id"].as_str().unwrap();
    let executor = Actor {
        owner: s.owner.owner.clone(),
        role: Role::Executor,
    };
    s.service
        .execute(&executor, id, Scenario::TimeoutBeforeAccept, s.at)
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            pipeline::advance(&s.service, &s.owner, &FixturePlanner, id, s.at + 600)
                .unwrap()
                .status,
            Status::Reconciling
        );
    }
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    assert_eq!(
        s.service
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
}

#[test]
fn expired_quote_and_revoked_agent_require_review_without_releasing_money_or_execution() {
    let s = Setup::new(vec![Permission::Plan, Permission::Execute]);
    let started = s.start(&FixturePlanner, "expire", "Order one oat latte");
    s.approve(&started);
    let id = started["pipeline_id"].as_str().unwrap();
    let expired = pipeline::advance(&s.service, &s.owner, &FixturePlanner, id, s.at + 301).unwrap();
    assert_eq!(expired.status, Status::NeedsReview);
    assert_eq!(expired.blocked_reason, Some("quote_expired"));
    s.service
        .revoke_agent(&s.owner, &s.agent.agent.id, s.at)
        .unwrap();
    let revoked = pipeline::advance(&s.service, &s.owner, &FixturePlanner, id, s.at).unwrap();
    assert_eq!(revoked.status, Status::NeedsReview);
    assert_eq!(revoked.blocked_reason, Some("agent_inactive"));
    assert!(
        s.call(
            &FixturePlanner,
            "POST",
            &format!("/pipelines/{id}/advance"),
            json!({})
        )
        .is_err()
    );
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    assert_eq!(
        s.service
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
    s.service.cancel(&s.owner, id, s.at).unwrap();
    assert_eq!(
        pipeline::inspect(&s.service, &s.owner, id, s.at)
            .unwrap()
            .status,
        Status::Cancelled
    );
}

#[test]
fn pipeline_never_escalates_plan_only_credentials_or_uses_another_agents_task() {
    let s = Setup::new(vec![Permission::Plan]);
    let started = s.start(&FixturePlanner, "scope", "Order one oat latte");
    s.approve(&started);
    let id = started["pipeline_id"].as_str().unwrap();
    assert!(
        s.call(
            &FixturePlanner,
            "POST",
            &format!("/pipelines/{id}/advance"),
            json!({})
        )
        .is_err()
    );
    let other = s
        .service
        .register_agent(
            &s.owner,
            Registration {
                name: "other".into(),
                budget_id: s.budget.clone(),
                permissions: vec![Permission::Plan, Permission::Execute],
                expires_at: s.at + 1800,
                operations: None,
            },
            s.at,
        )
        .unwrap();
    for method in ["GET", "POST"] {
        let path = if method == "GET" {
            format!("/gateway/v1/pipelines/{id}")
        } else {
            format!("/gateway/v1/pipelines/{id}/advance")
        };
        assert!(
            gateway::dispatch(
                &s.service,
                &FixturePlanner,
                &s.credentials(),
                &format!("Bearer {}", other.token),
                method,
                &path,
                b"{}",
                s.at
            )
            .is_err()
        );
    }
    assert!(
        s.call(
            &FixturePlanner,
            "POST",
            &format!("/pipelines/{id}/advance"),
            json!({"approved":true,"quote_digest":started["approval_digest"]})
        )
        .is_err()
    );
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
}

#[test]
fn concurrent_pipeline_advances_create_only_one_attempt() {
    let s = Setup::new(vec![Permission::Plan, Permission::Execute]);
    let started = s.start(&FixturePlanner, "concurrent", "Order one oat latte");
    s.approve(&started);
    let id = started["pipeline_id"].as_str().unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    s.call(
                        &FixturePlanner,
                        "POST",
                        &format!("/pipelines/{id}/advance"),
                        json!({}),
                    )
                    .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    assert_eq!(
        s.service.balance(&s.owner, &s.budget).unwrap().spent_minor,
        700
    );
}

#[test]
fn pipeline_demo_reports_restartable_stages_without_credentials_or_real_money() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .arg("pipeline-demo")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "cosigna.pipeline-demo/v1");
    assert_eq!(
        report["stages"],
        json!(["awaiting_approval", "ready_to_execute", "confirmed"])
    );
    assert_eq!(report["synthetic_orders"], 1);
    assert_eq!(report["simulated"], true);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("agentkey_"));
    assert!(!text.contains("token_hash"));
}
