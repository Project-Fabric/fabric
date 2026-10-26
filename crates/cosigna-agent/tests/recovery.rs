//! Refund recovery: the worker settles money operations whose rail result is unknown, by
//! lookup only. It never sends an attempt again, never touches one still in flight, adds no
//! evidence while nothing changes, and says when a person is needed. Fixture rail only;
//! synthetic stores in temporary folders; one test kills a real child process.
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

use cosigna_agent::{
    Actor, Error, Service,
    api::Credentials,
    gateway,
    operations::OperationStage,
    provider::Scenario,
    worker::{RECOVERY_CHECKS_BEFORE_PERSON, RECOVERY_GRACE, Recovery, Worker},
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;
const READY: &str = "COSIGNA_SYNTHETIC_RECOVERY_CHILD_STARTED";

struct Lab {
    directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    owner: String,
    budget: String,
    bot: String,
}

impl Lab {
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
            bot: String::new(),
        };
        lab.budget = lab
            .call(
                &lab.owner,
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,
                "per_purchase_minor":70000,"merchants":["fixture-cafe"],"expires_at":AT+86400*2}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        lab.bot = format!(
            "Bearer {}",
            lab.call(
                &lab.owner,
                "POST",
                "/agents",
                json!({"name":"refunds-bot","budget_id":lab.budget,"permissions":["execute"],
                "expires_at":AT+86400,"operations":{"kinds":["refund"],"rails":["fixture"],
                "auto_approve_minor":1000}})
            )
            .unwrap()["data"]["token"]
                .as_str()
                .unwrap()
        );
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

    fn owner_actor(&self) -> Actor {
        self.credentials.authenticate(&self.owner).unwrap()
    }

    /// A $300 refund on pay_fixture_2049, held for a person (above the agent's $10 limit).
    fn held(&self) -> (String, String) {
        let held = self
            .call(
                &self.bot,
                "POST",
                "/operations",
                json!({"budget_id":self.budget,"kind":"refund","rail":"fixture",
                "source":"pay_fixture_2049","amount_minor":30000,"currency":"USD",
                "reason":"Ticket","idempotency_key":"recover"}),
            )
            .unwrap();
        (
            held["data"]["id"].as_str().unwrap().to_string(),
            held["proposal_digest"].as_str().unwrap().to_string(),
        )
    }

    /// Approves and sends it, with the fixture rail behaving as `scenario`.
    fn sent(&self, scenario: Scenario) -> String {
        let (id, digest) = self.held();
        let owner = self.owner_actor();
        self.service
            .approve_operation(&owner, &id, &digest, AT)
            .unwrap();
        let op = self
            .service
            .execute_operation(&owner, &id, scenario, AT)
            .unwrap();
        assert_eq!(op.stage, OperationStage::Unknown);
        id
    }

    fn stage(&self, id: &str) -> OperationStage {
        self.service
            .operation(&self.owner_actor(), id)
            .unwrap()
            .stage
    }

    fn refunded(&self) -> i64 {
        self.service
            .rails
            .fixture_rail()
            .unwrap()
            .refunded_minor("pay_fixture_2049")
            .unwrap()
    }

    fn attempts(&self) -> i64 {
        self.service
            .rails
            .fixture_rail()
            .unwrap()
            .attempt_count()
            .unwrap()
    }

    fn evidence_records(&self) -> i64 {
        rusqlite::Connection::open(self.directory.path().join("transactions.sqlite"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM evidence", [], |r| r.get(0))
            .unwrap()
    }

    fn timeline_steps(&self, id: &str) -> Vec<String> {
        self.call(
            &self.owner,
            "GET",
            &format!("/operations/{id}/timeline"),
            json!({}),
        )
        .unwrap()["data"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["step"].as_str().unwrap().to_string())
            .collect()
    }
}

#[test]
fn a_lost_answer_is_settled_from_the_rail_without_a_second_refund() {
    let lab = Lab::new();
    // The rail paid, but its answer was lost.
    let id = lab.sent(Scenario::TimeoutAfterAccept);
    assert_eq!(lab.refunded(), 30000);
    let mut worker = Worker::open(&lab.service).unwrap();
    // Too recent: it may still be in flight in the gateway, so it is not touched.
    assert!(
        worker
            .recover_operations(AT + RECOVERY_GRACE - 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(lab.stage(&id), OperationStage::Unknown);
    let found = worker.recover_operations(AT + RECOVERY_GRACE).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].operation_id, id);
    assert_eq!(found[0].stage, OperationStage::Succeeded);
    assert!(!found[0].needs_person);
    assert_eq!(lab.stage(&id), OperationStage::Succeeded);
    // Paid once: one attempt at the rail, the money counted once, the hold spent.
    assert_eq!(lab.refunded(), 30000);
    assert_eq!(lab.attempts(), 1);
    assert_eq!(
        lab.timeline_steps(&id).last().map(String::as_str),
        Some("rail_result")
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
    // Settled operations are not checked again.
    assert!(worker.recover_operations(AT + 3600).unwrap().is_empty());
}

#[test]
fn with_no_result_it_checks_with_backoff_records_nothing_and_asks_for_a_person() {
    let lab = Lab::new();
    // The attempt never reached the rail: there is nothing to find.
    let id = lab.sent(Scenario::TimeoutBeforeAccept);
    let records = lab.evidence_records();
    let mut worker = Worker::open(&lab.service).unwrap();
    let mut reports: Vec<(i64, Recovery)> = Vec::new();
    // Every second for a minute: backoff keeps this to a few checks, none needing a person.
    for t in 0..60 {
        let at = AT + RECOVERY_GRACE + t;
        reports.extend(
            worker
                .recover_operations(at)
                .unwrap()
                .into_iter()
                .map(|r| (at, r)),
        );
    }
    assert!(reports.is_empty(), "{reports:?}");
    // Then every five minutes for two hours.
    for t in 1..=24 {
        let at = AT + RECOVERY_GRACE + 60 + t * 300;
        reports.extend(
            worker
                .recover_operations(at)
                .unwrap()
                .into_iter()
                .map(|r| (at, r)),
        );
    }
    assert_eq!(reports.len(), 1, "a person is asked for once: {reports:?}");
    let (_, report) = &reports[0];
    assert!(report.needs_person);
    assert_eq!(report.checks, RECOVERY_CHECKS_BEFORE_PERSON);
    assert_eq!(report.stage, OperationStage::Unknown);
    // No money moved, nothing was sent again, and repeated checks added no evidence.
    assert_eq!(lab.refunded(), 0);
    assert_eq!(lab.attempts(), 0);
    assert_eq!(lab.evidence_records(), records);
    // The owner settles it through the rail; the worker then leaves it alone.
    let owner = lab.owner_actor();
    let settled = lab
        .service
        .resolve_operation(&owner, &id, AT + 9000)
        .unwrap();
    assert_eq!(settled.stage, OperationStage::Failed);
    assert!(worker.recover_operations(AT + 99_999).unwrap().is_empty());
    assert!(
        lab.service
            .unsettled_operations(AT + 99_999, RECOVERY_GRACE)
            .unwrap()
            .is_empty()
    );
    assert!(lab.service.verify_evidence().unwrap().ok());
}

/// Child process: approves (which executes) and gets stuck at the rail, so the parent can
/// kill it with the operation claimed but no result recorded. Not a standalone test.
#[test]
#[ignore = "subprocess driver; invoked by the crash recovery test"]
fn recovery_crash_child() {
    let directory = std::path::PathBuf::from(std::env::var_os("COSIGNA_RECOVERY_STORE").unwrap());
    let service = Service::open(&directory).unwrap();
    let credentials = Credentials::load(&directory.join("credentials.json")).unwrap();
    let owner = credentials
        .authenticate(&std::env::var("COSIGNA_RECOVERY_OWNER").unwrap())
        .unwrap();
    let id = std::env::var("COSIGNA_RECOVERY_OPERATION").unwrap();
    let digest = std::env::var("COSIGNA_RECOVERY_DIGEST").unwrap();
    service.approve_operation(&owner, &id, &digest, AT).unwrap();
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    // Go on once the parent holds the rail's database: the submission then waits in the
    // rail until this process is killed.
    let mut stdin = std::io::stdin();
    stdin.read_exact(&mut [0_u8]).unwrap();
    let _ = service.execute_operation(&owner, &id, Scenario::Success, AT);
    stdin.read_exact(&mut [0_u8]).unwrap();
    panic!("the parent must kill this process before the rail answers");
}

#[test]
fn a_payment_interrupted_by_a_crash_is_recovered_without_being_sent_again() {
    let lab = Lab::new();
    let (id, digest) = lab.held();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "recovery_crash_child",
            "--nocapture",
        ])
        .env("COSIGNA_RECOVERY_STORE", lab.directory.path())
        .env("COSIGNA_RECOVERY_OWNER", &lab.owner)
        .env("COSIGNA_RECOVERY_OPERATION", &id)
        .env("COSIGNA_RECOVERY_DIGEST", &digest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = BufReader::new(stdout)
            .lines()
            .any(|line| line.is_ok_and(|l| l.contains(READY)));
        let _ = send.send(started);
    });
    let started = receive.recv_timeout(Duration::from_secs(20));
    if started != Ok(true) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("child did not start");
    }
    // Hold the rail's database so the child's submission cannot proceed, then let it go on.
    let rail =
        rusqlite::Connection::open(lab.directory.path().join("fixture-provider.sqlite")).unwrap();
    rail.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&[1]).unwrap();
    stdin.flush().unwrap();
    // Wait until the claim is durable: the operation is `submitting` in the ledger.
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while lab.stage(&id) != OperationStage::Submitting && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let killed = child.kill();
    let exit = child.wait().unwrap();
    drop(stdin);
    assert!(killed.is_ok() && !exit.success(), "child must be killed");
    rail.execute_batch("ROLLBACK").unwrap();
    drop(rail);

    // After the crash: claimed, money held, nothing at the rail.
    let service = Service::open(lab.directory.path()).unwrap();
    assert_eq!(lab.stage(&id), OperationStage::Submitting);
    assert_eq!(lab.attempts(), 0);
    let mut worker = Worker::open(&service).unwrap();
    assert!(
        worker
            .recover_operations(AT + RECOVERY_GRACE)
            .unwrap()
            .is_empty()
    );
    // No result: the operation becomes `unknown` (recorded once), and is never resubmitted.
    assert_eq!(lab.stage(&id), OperationStage::Unknown);
    let records = lab.evidence_records();
    for t in 1..=20 {
        worker
            .recover_operations(AT + RECOVERY_GRACE + t * 300)
            .unwrap();
    }
    assert_eq!(lab.evidence_records(), records);
    assert_eq!(lab.attempts(), 0);
    assert_eq!(lab.refunded(), 0);
    assert_eq!(
        lab.timeline_steps(&id),
        [
            "held",
            "owner_approved",
            "submission_started",
            "submission_unknown"
        ]
    );
    assert!(service.verify_evidence().unwrap().ok());
}
