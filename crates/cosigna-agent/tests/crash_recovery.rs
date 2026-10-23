//! Kill real child processes at fixed fixture-only durability boundaries.
//! This is process termination evidence, not a power-loss or live-processor claim.
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use cosigna_agent::{
    Actor, Budget, BudgetInput, Offer, Plan, Result, Role, Service, Stage, TaskInput,
    planner::{FixturePlanner, Planner},
    provider::Scenario,
    worker::{self, Action, Worker},
};

const READY: &str = "COSIGNA_SYNTHETIC_CRASH_BOUNDARY_READY";
fn actor(role: Role) -> Actor {
    Actor {
        owner: "crash-fixture-owner".into(),
        role,
    }
}

struct Setup {
    dir: tempfile::TempDir,
    budget: String,
    task: String,
}
impl Setup {
    fn new(approved: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(dir.path()).unwrap();
        let at = cosigna_agent::now();
        let owner = actor(Role::Owner);
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
        let task = service
            .create_task(
                &owner,
                TaskInput {
                    budget_id: budget.clone(),
                    request: "Order one oat latte".into(),
                    idempotency_key: "synthetic-crash".into(),
                    required_features: vec![],
                },
                at,
            )
            .unwrap()
            .id;
        if approved {
            let quoted = service.plan(&owner, &task, &FixturePlanner, at).unwrap();
            service
                .approve(&owner, &task, &quoted.quote.unwrap().digest().unwrap(), at)
                .unwrap();
        }
        worker::control(&service, &owner, &task, Action::Enroll, at).unwrap();
        Self { dir, budget, task }
    }
    fn kill_at(&self, phase: &str) {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "crash_boundary_child",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .envs(
                [
                    "SystemRoot",
                    "WINDIR",
                    "PATH",
                    "TEMP",
                    "TMP",
                    "LD_LIBRARY_PATH",
                ]
                .iter()
                .filter_map(|key| std::env::var_os(key).map(|value| (*key, value))),
            )
            .env("COSIGNA_CRASH_STORE", self.dir.path())
            .env("COSIGNA_CRASH_TASK", &self.task)
            .env("COSIGNA_CRASH_PHASE", phase)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let child = command.spawn().unwrap();
        let mut process = ChildGuard(child);
        let stdout = process.0.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.is_ok_and(|value| value.contains(READY)) {
                    let _ = send.send(());
                    break;
                }
            }
        });
        let ready = receive.recv_timeout(Duration::from_secs(20));
        // Always terminate/reap before assertions, including startup failure.
        let killed = process.0.kill();
        let exit = process.0.wait();
        reader.join().unwrap();
        ready.expect("fixture child did not reach the requested crash boundary");
        assert!(
            killed.is_ok(),
            "fixture child must be killed at the boundary"
        );
        assert!(
            !exit.unwrap().success(),
            "fixture child must not exit normally"
        );
    }
    fn reopen(&self) -> Service {
        Service::open(self.dir.path()).unwrap()
    }
}

struct ChildGuard(Child);
impl ChildGuard {
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wait_for_termination() -> ! {
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    // Parent holds this pipe open, then kills the process without unwinding.
    std::io::stdin().read_exact(&mut [0_u8]).unwrap();
    panic!("fixture parent must terminate the child at this boundary");
}

/// A subprocess driver, deliberately not a standalone regression case. Each
/// parent regression invokes it explicitly with only synthetic paths/identifiers.
#[test]
#[ignore = "subprocess fixture driver; invoked by the four crash regressions"]
fn crash_boundary_child() {
    let service = Service::open(std::path::Path::new(
        &std::env::var_os("COSIGNA_CRASH_STORE").unwrap(),
    ))
    .unwrap();
    let id = std::env::var("COSIGNA_CRASH_TASK").unwrap();
    let phase = std::env::var("COSIGNA_CRASH_PHASE").unwrap();
    let at = cosigna_agent::now();
    if phase == "during_planning" {
        struct InterruptedPlanner;
        impl Planner for InterruptedPlanner {
            fn name(&self) -> &str {
                "synthetic interrupted inference"
            }
            fn plan(&self, _: &TaskInput, _: &Budget, _: &[Offer]) -> Result<Plan> {
                wait_for_termination()
            }
        }
        Worker::open(&service)
            .unwrap()
            .tick(&InterruptedPlanner, at)
            .unwrap();
    } else if phase == "after_confirmation" {
        service
            .execute(&actor(Role::Executor), &id, Scenario::Success, at)
            .unwrap();
        wait_for_termination();
    } else {
        let (task, claimed) = service
            .begin_submission(&actor(Role::Executor), &id, at)
            .unwrap();
        assert!(claimed);
        if phase == "after_acceptance" {
            service
                .provider
                .submit(
                    task.attempt_key.as_deref().unwrap(),
                    task.quote.as_ref().unwrap(),
                    Scenario::Success,
                    at,
                )
                .unwrap();
        } else {
            assert_eq!(phase, "after_claim");
        }
        wait_for_termination();
    }
}

#[test]
fn process_kill_during_planning_pauses_without_automatic_inference_replay() {
    let s = Setup::new(false);
    s.kill_at("during_planning");
    let service = s.reopen();
    let mut worker = Worker::open(&service).unwrap();
    let job = worker::inspect(&service, &actor(Role::Owner), &s.task).unwrap();
    assert_eq!(job.state, "paused");
    assert_eq!(job.reason.as_deref(), Some("planning_interrupted"));
    assert!(
        worker
            .tick(&FixturePlanner, cosigna_agent::now())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        service.task(&actor(Role::Owner), &s.task).unwrap().stage,
        Stage::Draft
    );
    assert_eq!(service.provider.order_count().unwrap(), 0);
    assert_eq!(
        service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .reserved_minor,
        0
    );
    // OS planning lock is also released; deliberate operator resume can recover.
    worker::control(
        &service,
        &actor(Role::Owner),
        &s.task,
        Action::Resume,
        cosigna_agent::now(),
    )
    .unwrap();
    worker.tick(&FixturePlanner, cosigna_agent::now()).unwrap();
    assert_eq!(
        service.task(&actor(Role::Owner), &s.task).unwrap().stage,
        Stage::Quoted
    );
}

#[test]
fn process_kill_after_claim_preserves_attempt_and_hold_without_replacement_submission() {
    let s = Setup::new(true);
    s.kill_at("after_claim");
    let service = s.reopen();
    let before = service.task(&actor(Role::Owner), &s.task).unwrap();
    assert_eq!(before.stage, Stage::Submitting);
    assert!(before.attempt_key.is_some());
    let mut worker = Worker::open(&service).unwrap();
    let first = worker
        .tick(&FixturePlanner, cosigna_agent::now())
        .unwrap()
        .unwrap();
    worker.tick(&FixturePlanner, first.next_at).unwrap();
    let after = service.task(&actor(Role::Owner), &s.task).unwrap();
    assert_eq!(after.stage, Stage::Unknown);
    assert_eq!(after.attempt_key, before.attempt_key);
    assert_eq!(service.provider.order_count().unwrap(), 0);
    let balance = service.balance(&actor(Role::Owner), &s.budget).unwrap();
    assert_eq!(
        (
            balance.reserved_minor,
            balance.spent_minor,
            balance.available_minor
        ),
        (700, 0, 300)
    );
}

#[test]
fn process_kill_after_acceptance_reconciles_one_order_after_budget_revocation() {
    let s = Setup::new(true);
    s.kill_at("after_acceptance");
    let service = s.reopen();
    let before = service.task(&actor(Role::Owner), &s.task).unwrap();
    assert_eq!(before.stage, Stage::Submitting);
    assert_eq!(service.provider.order_count().unwrap(), 1);
    service
        .revoke_budget(&actor(Role::Owner), &s.budget)
        .unwrap();
    Worker::open(&service)
        .unwrap()
        .tick(&FixturePlanner, cosigna_agent::now())
        .unwrap();
    let confirmed = service.task(&actor(Role::Owner), &s.task).unwrap();
    assert_eq!(confirmed.stage, Stage::Confirmed);
    assert_eq!(confirmed.attempt_key, before.attempt_key);
    let replay = service
        .execute(
            &actor(Role::Executor),
            &s.task,
            Scenario::Success,
            cosigna_agent::now(),
        )
        .unwrap();
    assert_eq!(replay.attempt_key, confirmed.attempt_key);
    assert_eq!(service.provider.order_count().unwrap(), 1);
    let balance = service.balance(&actor(Role::Owner), &s.budget).unwrap();
    assert_eq!((balance.reserved_minor, balance.spent_minor), (0, 700));
}

#[test]
fn process_kill_after_recording_preserves_confirmation_on_worker_restart() {
    let s = Setup::new(true);
    s.kill_at("after_confirmation");
    let service = s.reopen();
    let before = service.task(&actor(Role::Owner), &s.task).unwrap();
    assert_eq!(before.stage, Stage::Confirmed);
    let job = Worker::open(&service)
        .unwrap()
        .tick(&FixturePlanner, cosigna_agent::now())
        .unwrap()
        .unwrap();
    assert_eq!(job.state, "done");
    assert_eq!(
        service
            .task(&actor(Role::Owner), &s.task)
            .unwrap()
            .attempt_key,
        before.attempt_key
    );
    assert_eq!(service.provider.order_count().unwrap(), 1);
    assert_eq!(
        service
            .balance(&actor(Role::Owner), &s.budget)
            .unwrap()
            .spent_minor,
        700
    );
}
