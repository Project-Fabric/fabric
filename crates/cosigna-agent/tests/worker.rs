use std::sync::atomic::{AtomicUsize, Ordering};

use cosigna_agent::{
    Actor, Budget, BudgetInput, Error, Offer, Plan, Result, Role, Service, Stage, TaskInput,
    api::Credentials,
    gateway::{self, Permission, Registration},
    planner::{FixturePlanner, Planner},
    provider::Scenario,
    worker::{self, Action, Worker},
};
use serde_json::{Value, json};

struct Count(AtomicUsize);
impl Planner for Count {
    fn plan(&self, task: &TaskInput, budget: &Budget, offers: &[Offer]) -> Result<Plan> {
        self.0.fetch_add(1, Ordering::SeqCst);
        FixturePlanner.plan(task, budget, offers)
    }
    fn name(&self) -> &str {
        "counting fixtures"
    }
}
struct Setup {
    dir: tempfile::TempDir,
    service: Service,
    owner: Actor,
    budget: String,
    at: i64,
}
impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(dir.path()).unwrap();
        let owner = Actor {
            owner: "local-user".into(),
            role: Role::Owner,
        };
        let at = cosigna_agent::now();
        let budget = service
            .create_budget(
                &owner,
                BudgetInput {
                    currency: "USD".into(),
                    limit_minor: 2000,
                    per_purchase_minor: 1000,
                    merchants: vec!["fixture-cafe".into()],
                    expires_at: at + 3600,
                },
                at,
            )
            .unwrap()
            .id;
        Self {
            dir,
            service,
            owner,
            budget,
            at,
        }
    }
    fn task(&self, key: &str) -> String {
        self.service
            .create_task(&self.owner, self.input(key), self.at)
            .unwrap()
            .id
    }
    fn input(&self, key: &str) -> TaskInput {
        TaskInput {
            budget_id: self.budget.clone(),
            request: "Order one oat latte".into(),
            idempotency_key: key.into(),
            required_features: vec![],
        }
    }
    fn enroll(&self, id: &str) {
        worker::control(&self.service, &self.owner, id, Action::Enroll, self.at).unwrap();
    }
    fn approve(&self, id: &str) {
        let task = self.service.task(&self.owner, id).unwrap();
        self.service
            .approve(
                &self.owner,
                id,
                &task.quote.unwrap().digest().unwrap(),
                self.at,
            )
            .unwrap();
    }
    fn executor(&self) -> Actor {
        Actor {
            owner: self.owner.owner.clone(),
            role: Role::Executor,
        }
    }
    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.dir.path().join("transactions.sqlite")).unwrap()
    }
}

#[test]
fn opt_in_worker_waits_then_restarts_and_executes_exactly_one_fixture_order() {
    let s = Setup::new();
    let id = s.task("one");
    let planner = Count(AtomicUsize::new(0));
    let mut worker = Worker::open(&s.service).unwrap();
    assert!(worker.tick(&planner, s.at).unwrap().is_none());
    s.enroll(&id);
    assert_eq!(
        worker.tick(&planner, s.at).unwrap().unwrap().state,
        "pending"
    );
    for step in 1..4 {
        worker.tick(&planner, s.at + step * 5).unwrap();
    }
    assert_eq!(planner.0.load(Ordering::SeqCst), 1);
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
    s.approve(&id);
    drop(worker);
    let reopened = Service::open(s.dir.path()).unwrap();
    let mut worker = Worker::open(&reopened).unwrap();
    assert_eq!(
        worker.tick(&planner, s.at + 20).unwrap().unwrap().state,
        "done"
    );
    assert!(worker.tick(&planner, s.at + 25).unwrap().is_none());
    assert_eq!(reopened.provider.order_count().unwrap(), 1);
    assert_eq!(
        reopened.balance(&s.owner, &s.budget).unwrap().spent_minor,
        700
    );
}

#[test]
fn control_is_owner_only_strict_and_enrollment_replay_cannot_resume() {
    let s = Setup::new();
    let credentials = Credentials::initialize(&s.dir.path().join("credentials.json")).unwrap();
    let private: Value =
        serde_json::from_slice(&std::fs::read(s.dir.path().join("credentials.json")).unwrap())
            .unwrap();
    let agent = s
        .service
        .register_agent(
            &s.owner,
            Registration {
                name: "agent".into(),
                budget_id: s.budget.clone(),
                permissions: vec![Permission::Plan, Permission::Execute],
                expires_at: s.at + 1800,
            },
            s.at,
        )
        .unwrap();
    let call = |auth: &str, method: &str, path: &str, body: Value| {
        gateway::dispatch(
            &s.service,
            &FixturePlanner,
            &credentials,
            auth,
            method,
            path,
            &serde_json::to_vec(&body).unwrap(),
            s.at,
        )
    };
    let agent_auth = format!("Bearer {}", agent.token);
    let task = call(
        &agent_auth,
        "POST",
        "/gateway/v1/tasks",
        serde_json::to_value(s.input("agent")).unwrap(),
    )
    .unwrap();
    let id = task["data"]["id"].as_str().unwrap();
    let path = format!("/gateway/v1/pipelines/{id}/worker");
    assert!(call(&agent_auth, "POST", &path, json!({"action":"enroll"})).is_err());
    assert!(call(&agent_auth, "GET", &path, json!(null)).is_err());
    let owner_auth = format!("Bearer {}", private["owner"].as_str().unwrap());
    assert!(
        call(
            &owner_auth,
            "POST",
            &path,
            json!({"action":"enroll","approved":true})
        )
        .is_err()
    );
    let response = call(&owner_auth, "POST", &path, json!({"action":"enroll"})).unwrap();
    assert_eq!(response["schema"], worker::API_VERSION);
    worker::control(&s.service, &s.owner, id, Action::Pause, s.at).unwrap();
    s.enroll(id);
    assert_eq!(
        worker::inspect(&s.service, &s.owner, id).unwrap().state,
        "paused"
    );
    let other = Actor {
        owner: "stranger".into(),
        role: Role::Owner,
    };
    assert!(worker::control(&s.service, &other, id, Action::Resume, s.at).is_err());
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
}

#[test]
fn worker_preserves_agent_execute_permission_and_rechecks_revocation() {
    for revoke in [false, true] {
        let s = Setup::new();
        let credentials = Credentials::initialize(&s.dir.path().join("credentials.json")).unwrap();
        let agent = s
            .service
            .register_agent(
                &s.owner,
                Registration {
                    name: "limited".into(),
                    budget_id: s.budget.clone(),
                    permissions: if revoke {
                        vec![Permission::Plan, Permission::Execute]
                    } else {
                        vec![Permission::Plan]
                    },
                    expires_at: s.at + 1800,
                },
                s.at,
            )
            .unwrap();
        let task = gateway::dispatch(
            &s.service,
            &FixturePlanner,
            &credentials,
            &format!("Bearer {}", agent.token),
            "POST",
            "/gateway/v1/tasks",
            &serde_json::to_vec(&s.input("limited")).unwrap(),
            s.at,
        )
        .unwrap();
        let id = task["data"]["id"].as_str().unwrap();
        s.enroll(id);
        let mut worker = Worker::open(&s.service).unwrap();
        worker.tick(&FixturePlanner, s.at).unwrap();
        s.approve(id);
        if revoke {
            s.service
                .revoke_agent(&s.owner, &agent.agent.id, s.at)
                .unwrap();
        }
        let job = worker.tick(&FixturePlanner, s.at + 5).unwrap().unwrap();
        assert_eq!(job.state, "paused");
        assert_eq!(
            job.reason.as_deref(),
            Some(if revoke {
                "agent_inactive"
            } else {
                "permission_denied"
            })
        );
        assert_eq!(s.service.provider.order_count().unwrap(), 0);
        assert_eq!(
            s.service
                .balance(&s.owner, &s.budget)
                .unwrap()
                .reserved_minor,
            700
        );
    }
}

#[test]
fn unresolved_payment_has_bounded_backoff_and_keeps_hold_across_restart() {
    let s = Setup::new();
    let id = s.task("unknown");
    s.service
        .plan(&s.owner, &id, &FixturePlanner, s.at)
        .unwrap();
    s.approve(&id);
    s.service
        .execute(&s.executor(), &id, Scenario::TimeoutBeforeAccept, s.at)
        .unwrap();
    s.enroll(&id);
    let mut worker = Worker::open(&s.service).unwrap();
    let first = worker.tick(&FixturePlanner, s.at).unwrap().unwrap();
    assert_eq!(first.next_at, s.at + 10);
    assert!(worker.tick(&FixturePlanner, s.at + 9).unwrap().is_none());
    drop(worker);
    let reopened = Service::open(s.dir.path()).unwrap();
    let mut worker = Worker::open(&reopened).unwrap();
    let mut at = first.next_at;
    loop {
        let job = worker.tick(&FixturePlanner, at).unwrap().unwrap();
        if job.state == "paused" {
            assert_eq!(job.reason.as_deref(), Some("reconciliation_limit"));
            assert_eq!(job.reconcile_count, 12);
            break;
        }
        assert!(job.next_at - at <= 300);
        at = job.next_at;
    }
    assert_eq!(reopened.provider.order_count().unwrap(), 0);
    assert_eq!(reopened.task(&s.owner, &id).unwrap().stage, Stage::Unknown);
    assert_eq!(
        reopened
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
}

#[test]
fn interrupted_inference_pauses_but_persisted_quote_and_attempt_can_recover() {
    let s = Setup::new();
    let draft = s.task("draft");
    let quoted = s.task("quoted");
    let unknown = s.task("accepted");
    for id in [&quoted, &unknown] {
        s.service.plan(&s.owner, id, &FixturePlanner, s.at).unwrap();
    }
    s.approve(&unknown);
    s.service
        .execute(&s.executor(), &unknown, Scenario::TimeoutAfterAccept, s.at)
        .unwrap();
    for id in [&draft, &quoted, &unknown] {
        s.enroll(id);
    }
    // Model/connector interruption boundary reproduced in this synthetic DB.
    s.db()
        .execute("UPDATE worker_jobs SET state='running'", [])
        .unwrap();
    let mut worker = Worker::open(&s.service).unwrap();
    let interrupted = worker::inspect(&s.service, &s.owner, &draft).unwrap();
    assert_eq!(interrupted.state, "paused");
    assert_eq!(interrupted.reason.as_deref(), Some("planning_interrupted"));
    let planner = Count(AtomicUsize::new(0));
    for _ in 0..2 {
        worker.tick(&planner, s.at + 1).unwrap();
    }
    assert_eq!(planner.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.service.task(&s.owner, &unknown).unwrap().stage,
        Stage::Confirmed
    );
    assert_eq!(s.service.provider.order_count().unwrap(), 1);
    worker::control(&s.service, &s.owner, &draft, Action::Resume, s.at + 2).unwrap();
    worker.tick(&planner, s.at + 2).unwrap();
    assert_eq!(planner.0.load(Ordering::SeqCst), 1);
}

#[test]
fn inference_failure_pauses_without_a_paid_retry_loop() {
    struct Failed(AtomicUsize);
    impl Planner for Failed {
        fn name(&self) -> &str {
            "failure fixture"
        }
        fn plan(&self, _: &TaskInput, _: &Budget, _: &[Offer]) -> Result<Plan> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(Error::Planner)
        }
    }
    let s = Setup::new();
    let id = s.task("failure");
    s.enroll(&id);
    let planner = Failed(AtomicUsize::new(0));
    let mut worker = Worker::open(&s.service).unwrap();
    assert_eq!(
        worker
            .tick(&planner, s.at)
            .unwrap()
            .unwrap()
            .reason
            .as_deref(),
        Some("planner_failed")
    );
    assert!(worker.tick(&planner, s.at + 1000).unwrap().is_none());
    assert_eq!(planner.0.load(Ordering::SeqCst), 1);
}

#[test]
fn api_cannot_start_another_inference_while_the_worker_is_planning() {
    struct Reentrant<'a>(&'a Setup, &'a str, AtomicUsize);
    impl Planner for Reentrant<'_> {
        fn name(&self) -> &str {
            "concurrent inference fixture"
        }
        fn plan(&self, t: &TaskInput, b: &Budget, o: &[Offer]) -> Result<Plan> {
            self.2.fetch_add(1, Ordering::SeqCst);
            assert!(matches!(
                self.0
                    .service
                    .plan(&self.0.owner, self.1, &FixturePlanner, self.0.at),
                Err(Error::Conflict(_))
            ));
            FixturePlanner.plan(t, b, o)
        }
    }
    let s = Setup::new();
    let id = s.task("inference-lock");
    s.enroll(&id);
    let planner = Reentrant(&s, &id, AtomicUsize::new(0));
    Worker::open(&s.service)
        .unwrap()
        .tick(&planner, s.at)
        .unwrap();
    assert_eq!(planner.2.load(Ordering::SeqCst), 1);
    assert_eq!(s.service.task(&s.owner, &id).unwrap().stage, Stage::Quoted);
}

#[test]
fn owner_pause_during_inference_wins_over_worker_completion() {
    struct Pausing<'a>(&'a Setup, &'a str);
    impl Planner for Pausing<'_> {
        fn name(&self) -> &str {
            "pause fixture"
        }
        fn plan(&self, t: &TaskInput, b: &Budget, o: &[Offer]) -> Result<Plan> {
            worker::control(
                &self.0.service,
                &self.0.owner,
                self.1,
                Action::Pause,
                self.0.at,
            )?;
            FixturePlanner.plan(t, b, o)
        }
    }
    let s = Setup::new();
    let id = s.task("pause");
    s.enroll(&id);
    let mut worker = Worker::open(&s.service).unwrap();
    let job = worker.tick(&Pausing(&s, &id), s.at).unwrap().unwrap();
    assert_eq!(job.state, "paused");
    assert_eq!(job.reason.as_deref(), Some("owner_paused"));
    s.approve(&id);
    assert!(worker.tick(&FixturePlanner, s.at + 5).unwrap().is_none());
    assert_eq!(s.service.provider.order_count().unwrap(), 0);
}

#[test]
fn v2_migration_preserves_approval_and_hold_and_does_not_enroll_historical_tasks() {
    let s = Setup::new();
    let id = s.task("v2");
    s.service
        .plan(&s.owner, &id, &FixturePlanner, s.at)
        .unwrap();
    s.approve(&id);
    let before = serde_json::to_value(s.service.task(&s.owner, &id).unwrap()).unwrap();
    // Exact old schema in an isolated fixture only.
    s.db()
        .execute_batch("DROP TABLE worker_jobs; PRAGMA user_version=2;")
        .unwrap();
    let reopened = Service::open(s.dir.path()).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.task(&s.owner, &id).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        reopened
            .balance(&s.owner, &s.budget)
            .unwrap()
            .reserved_minor,
        700
    );
    assert_eq!(
        s.db()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(
        Worker::open(&reopened)
            .unwrap()
            .tick(&FixturePlanner, s.at)
            .unwrap()
            .is_none()
    );
}

#[test]
fn worker_process_lock_is_exclusive_and_released_after_kill() {
    let s = Setup::new();
    Credentials::initialize(&s.dir.path().join("credentials.json")).unwrap();
    let worker = Worker::open(&s.service).unwrap();
    assert!(Worker::open(&s.service).is_err());
    drop(worker);
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .args(["worker", "--data-dir"])
        .arg(s.dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Reading one startup line proves the child acquired the lock before we kill it.
    let stderr = child.stderr.take().unwrap();
    let mut reader = std::io::BufReader::new(stderr);
    let mut line = String::new();
    std::io::BufRead::read_line(&mut reader, &mut line).unwrap();
    assert!(line.starts_with("Cosigna worker"));
    let locked = Worker::open(&s.service).is_err();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(locked);
    assert!(Worker::open(&s.service).is_ok());
}
