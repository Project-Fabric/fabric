//! The monitor: scheduled evidence checks open one finding per problem, close it when the check
//! is clean, report the worker's heartbeat to the owner only, and survive the ledger upgrade.
//! Temporary stores and fixtures only. Stripe comparison is tested in `stripe_rail.rs`, alert
//! delivery of findings in `alerts.rs`.
use cosigna_agent::{
    Actor, Role, Service,
    api::Credentials,
    gateway,
    monitor::{EVIDENCE_FAILED, Monitor, VERIFY_EVERY, WORKER_STALE_AFTER},
    planner::FixturePlanner,
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

fn owner() -> Actor {
    Actor {
        owner: "local-user".into(),
        role: Role::Owner,
    }
}

struct Lab {
    directory: tempfile::TempDir,
    service: Service,
    credentials: Credentials,
    private: Value,
    budget: String,
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
        let mut lab = Self {
            directory,
            service,
            credentials,
            private,
            budget: String::new(),
        };
        lab.budget = lab
            .call(
                "owner",
                "POST",
                "/budgets",
                json!({"currency":"USD","limit_minor":100000,"per_purchase_minor":50000,
                    "merchants":["fixture-cafe"],"expires_at":AT+86400}),
            )
            .unwrap()["data"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        lab
    }

    fn call(
        &self,
        role: &str,
        method: &str,
        path: &str,
        body: Value,
    ) -> Result<Value, cosigna_agent::Error> {
        let bytes = if method == "GET" {
            Vec::new()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        gateway::dispatch(
            &self.service,
            &FixturePlanner,
            &self.credentials,
            &format!("Bearer {}", self.private[role].as_str().unwrap()),
            method,
            &format!("/gateway/v1{path}"),
            &bytes,
            AT,
        )
    }

    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.directory.path().join("transactions.sqlite")).unwrap()
    }

    /// Changes a budget outside the service, as someone with database access could.
    fn tamper(&self, data: &str) {
        self.db()
            .execute(
                "UPDATE budgets SET data=?1 WHERE id=?2",
                [data, self.budget.as_str()],
            )
            .unwrap();
    }

    fn budget_data(&self) -> String {
        self.db()
            .query_row(
                "SELECT data FROM budgets WHERE id=?1",
                [self.budget.as_str()],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn evidence_kinds(&self) -> Vec<String> {
        let db = self.db();
        let mut statement = db
            .prepare("SELECT body FROM evidence ORDER BY seq")
            .unwrap();
        statement
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|body| {
                let body: Value = serde_json::from_str(&body.unwrap()).unwrap();
                body["event"]["kind"].as_str().unwrap().to_string()
            })
            .collect()
    }
}

#[test]
fn a_clean_ledger_opens_no_finding_and_the_owner_sees_the_worker_was_here() {
    let lab = Lab::new();
    let status = lab.service.monitor_status(&owner(), AT).unwrap();
    assert_eq!(status.worker.last_seen_at, None);
    assert!(!status.worker.recently_seen);
    assert_eq!(status.evidence.ok, None);

    assert!(
        Monitor::new()
            .run(&lab.service, None, AT)
            .unwrap()
            .is_empty()
    );
    let status = lab.service.monitor_status(&owner(), AT).unwrap();
    assert_eq!(status.worker.last_seen_at, Some(AT));
    assert!(status.worker.recently_seen);
    assert!(!status.worker.alerts_configured);
    assert_eq!(status.evidence.last_verified_at, Some(AT));
    assert_eq!(status.evidence.ok, Some(true));
    assert_eq!(status.open_findings, 0);
    // With no Stripe rail connected, comparison never starts and does not claim to have run.
    assert_eq!(status.reconciliation.from, None);
    assert_eq!(status.reconciliation.last_completed_at, None);

    // A worker that stops writing is reported, by the API process, from the ledger.
    let later = lab
        .service
        .monitor_status(&owner(), AT + WORKER_STALE_AFTER + 1)
        .unwrap();
    assert!(!later.worker.recently_seen);
    assert_eq!(later.worker.last_seen_at, Some(AT));
}

#[test]
fn only_the_owner_can_read_the_monitor() {
    let lab = Lab::new();
    let status = lab.call("owner", "GET", "/monitor", json!({})).unwrap();
    assert_eq!(status["schema"], "cosigna.monitor/v1");
    assert_eq!(status["data"]["open_findings"], 0);
    assert!(lab.call("planner", "GET", "/monitor", json!({})).is_err());
    assert!(lab.call("executor", "GET", "/monitor", json!({})).is_err());
}

#[test]
fn changed_state_opens_one_finding_until_a_clean_check_closes_it() {
    let lab = Lab::new();
    let original = lab.budget_data();
    let tampered = original.replace("100000", "900000");
    assert_ne!(original, tampered);
    lab.tamper(&tampered);

    let mut monitor = Monitor::new();
    let found = monitor.run(&lab.service, None, AT).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, EVIDENCE_FAILED);
    assert_eq!(found[0].detail["state_matches"], false);
    // The finding names counts, not what changed; `evidence verify` has the details.
    assert_eq!(found[0].detail["chain_intact"], true);

    // Not checked again before it is due, and not reported twice while it stays open.
    assert!(
        monitor
            .run(&lab.service, None, AT + VERIFY_EVERY - 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        monitor
            .run(&lab.service, None, AT + VERIFY_EVERY)
            .unwrap()
            .is_empty()
    );
    let status = lab
        .service
        .monitor_status(&owner(), AT + VERIFY_EVERY)
        .unwrap();
    assert_eq!(status.evidence.ok, Some(false));
    assert_eq!(status.open_findings, 1);
    assert_eq!(status.findings[0].kind, EVIDENCE_FAILED);

    // Put right: the finding closes, and the same problem coming back is reported again.
    lab.tamper(&original);
    assert!(
        monitor
            .run(&lab.service, None, AT + 2 * VERIFY_EVERY)
            .unwrap()
            .is_empty()
    );
    let status = lab
        .service
        .monitor_status(&owner(), AT + 2 * VERIFY_EVERY)
        .unwrap();
    assert_eq!(status.evidence.ok, Some(true));
    assert_eq!(status.open_findings, 0);
    lab.tamper(&tampered);
    assert_eq!(
        monitor
            .run(&lab.service, None, AT + 3 * VERIFY_EVERY)
            .unwrap()
            .len(),
        1
    );

    // Opening and closing are themselves recorded, and leave the chain verifiable.
    lab.tamper(&original);
    let report = lab.service.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
    let kinds = lab.evidence_kinds();
    let count = |kind: &str| kinds.iter().filter(|k| *k == kind).count();
    assert_eq!(count("gateway.monitor.finding"), 2);
    assert_eq!(count("gateway.monitor.resolved"), 1);
}

#[test]
fn a_removed_record_is_found_by_the_scheduled_check() {
    let lab = Lab::new();
    lab.db()
        .execute_batch("DROP TRIGGER evidence_no_delete; DELETE FROM evidence WHERE seq=1;")
        .unwrap();
    let found = Monitor::new().run(&lab.service, None, AT).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, EVIDENCE_FAILED);
    assert_eq!(found[0].detail["chain_intact"], false);
}

#[test]
fn a_version_12_ledger_upgrades_and_keeps_its_waiting_alerts() {
    let directory = tempfile::tempdir().unwrap();
    drop(Service::open(directory.path()).unwrap());
    let path = directory.path().join("transactions.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    // Reproduce version 12 in this synthetic store only, with its narrower event list.
    db.execute_batch(
        "DROP TABLE monitor_findings; DROP TABLE monitor_state; DROP TABLE alert_outbox;
         CREATE TABLE alert_outbox (
            evidence_seq INTEGER NOT NULL, destination TEXT NOT NULL, operation_id TEXT NOT NULL,
            event TEXT NOT NULL CHECK(event IN ('held','first_approved','unknown')),
            due_at INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL CHECK(state IN ('pending','sent','skipped','failed')),
            updated_at INTEGER NOT NULL, PRIMARY KEY (evidence_seq, destination));
         CREATE INDEX alert_outbox_due ON alert_outbox(state, due_at);
         INSERT INTO alert_outbox VALUES (1,'ops','op_waiting','held',5,2,'pending',4);
         PRAGMA user_version=12;",
    )
    .unwrap();
    drop(db);

    let service = Service::open(directory.path()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 13);
    let row: (String, String, i64, i64, String) = db
        .query_row(
            "SELECT operation_id,event,due_at,attempts,state FROM alert_outbox
             WHERE evidence_seq=1 AND destination='ops'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "op_waiting".to_string(),
            "held".to_string(),
            5,
            2,
            "pending".to_string()
        )
    );
    // The new event is accepted, and an unknown one still is not.
    db.execute(
        "INSERT INTO alert_outbox VALUES (2,'ops','ledger','finding',5,0,'pending',4)",
        [],
    )
    .unwrap();
    assert!(
        db.execute(
            "INSERT INTO alert_outbox VALUES (3,'ops','ledger','other',5,0,'pending',4)",
            [],
        )
        .is_err()
    );
    let status = service.monitor_status(&owner(), AT).unwrap();
    assert_eq!(status.alerts.pending, 2);
    assert!(Monitor::new().run(&service, None, AT).unwrap().is_empty());
}

#[test]
fn the_real_worker_warns_when_alerts_are_off_and_records_its_heartbeat() {
    let directory = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_cosigna-agent");
    assert!(
        std::process::Command::new(binary)
            .args(["init", "--data-dir"])
            .arg(directory.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = cosigna_agent::now();
    let output = std::process::Command::new(binary)
        .args(["worker", "--once", "--data-dir"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("alerts are off"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let service = Service::open(directory.path()).unwrap();
    let status = service
        .monitor_status(&owner(), cosigna_agent::now())
        .unwrap();
    assert!(status.worker.last_seen_at.is_some_and(|at| at >= before));
    assert!(status.worker.recently_seen);
    assert_eq!(status.evidence.ok, Some(true));
}
