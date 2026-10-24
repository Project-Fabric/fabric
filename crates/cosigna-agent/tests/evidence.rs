use cosigna_agent::{
    Actor, BudgetInput, Error, Role, Service, TaskInput, api::Credentials, evidence, gateway,
    planner::FixturePlanner, provider::Scenario,
};
use serde_json::{Value, json};

const AT: i64 = 1_800_000_000;

fn owner() -> Actor {
    Actor {
        owner: "alice".into(),
        role: Role::Owner,
    }
}

fn budget_input(limit: i64) -> BudgetInput {
    BudgetInput {
        currency: "USD".into(),
        limit_minor: limit,
        per_purchase_minor: limit,
        merchants: vec!["fixture-cafe".into()],
        expires_at: AT + 3600,
    }
}

fn task_input(budget: &str, key: &str) -> TaskInput {
    TaskInput {
        budget_id: budget.into(),
        request: "Order one oat latte".into(),
        idempotency_key: key.into(),
        required_features: vec![],
    }
}

fn db(directory: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(directory.path().join("transactions.sqlite")).unwrap()
}

fn events(directory: &tempfile::TempDir) -> Vec<Value> {
    let db = db(directory);
    let mut stmt = db
        .prepare("SELECT body FROM evidence ORDER BY seq")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|body| serde_json::from_str::<Value>(&body.unwrap()).unwrap()["event"].clone())
        .collect()
}

/// One approved purchase by the owner, returning (directory, service, budget, task).
fn purchased() -> (tempfile::TempDir, Service, String, String) {
    let directory = tempfile::tempdir().unwrap();
    let service = Service::open(directory.path()).unwrap();
    let budget = service
        .create_budget(&owner(), budget_input(1000), AT)
        .unwrap();
    let task = service
        .create_task(&owner(), task_input(&budget.id, "one"), AT)
        .unwrap();
    let quoted = service
        .plan(&owner(), &task.id, &FixturePlanner, AT)
        .unwrap();
    let digest = quoted.quote.unwrap().digest().unwrap();
    service.approve(&owner(), &task.id, &digest, AT).unwrap();
    let executor = Actor {
        owner: "alice".into(),
        role: Role::Executor,
    };
    service
        .execute(&executor, &task.id, Scenario::Success, AT)
        .unwrap();
    (directory, service, budget.id, task.id)
}

#[test]
fn every_gateway_decision_is_recorded_with_actor_and_amounts() {
    let directory = tempfile::tempdir().unwrap();
    let service = Service::open(directory.path()).unwrap();
    let credentials = Credentials::initialize(&directory.path().join("credentials.json")).unwrap();
    let private: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("credentials.json")).unwrap())
            .unwrap();
    let owner_auth = format!("Bearer {}", private["owner"].as_str().unwrap());
    let call = |auth: &str, path: &str, body: Value| {
        gateway::dispatch(
            &service,
            &FixturePlanner,
            &credentials,
            auth,
            "POST",
            &format!("/gateway/v1{path}"),
            &serde_json::to_vec(&body).unwrap(),
            AT,
        )
    };
    let budget = call(
        &owner_auth,
        "/budgets",
        json!({"currency":"USD","limit_minor":1000,"per_purchase_minor":1000,"merchants":["fixture-cafe"],"expires_at":AT+3600}),
    )
    .unwrap();
    let budget_id = budget["data"]["id"].as_str().unwrap().to_string();
    let issued = call(
        &owner_auth,
        "/agents",
        json!({"name":"buyer","budget_id":budget_id,"permissions":["plan","execute"],"expires_at":AT+1800}),
    )
    .unwrap();
    let token = issued["data"]["token"].as_str().unwrap().to_string();
    let agent_id = issued["data"]["agent"]["id"].as_str().unwrap().to_string();
    let agent_auth = format!("Bearer {token}");
    let task = call(
        &agent_auth,
        "/tasks",
        json!({"budget_id":budget_id,"request":"Order one oat latte","idempotency_key":"k1"}),
    )
    .unwrap();
    let id = task["data"]["id"].as_str().unwrap().to_string();
    let quote = call(&agent_auth, &format!("/tasks/{id}/plan"), json!({})).unwrap();
    let digest = quote["approval_digest"].as_str().unwrap().to_string();
    // Refused calls change nothing and so record nothing.
    let before = events(&directory).len();
    assert!(
        call(
            &agent_auth,
            &format!("/tasks/{id}/approve"),
            json!({"quote_digest":digest})
        )
        .is_err()
    );
    assert_eq!(events(&directory).len(), before);
    call(
        &owner_auth,
        &format!("/tasks/{id}/approve"),
        json!({"quote_digest":digest}),
    )
    .unwrap();
    call(&agent_auth, &format!("/tasks/{id}/execute"), json!({})).unwrap();
    call(
        &owner_auth,
        &format!("/agents/{agent_id}/revoke"),
        json!({}),
    )
    .unwrap();
    call(
        &owner_auth,
        &format!("/budgets/{budget_id}/revoke"),
        json!({}),
    )
    .unwrap();

    let recorded = events(&directory);
    let kinds: Vec<&str> = recorded
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "gateway.ledger.created",
            "gateway.budget.created",
            "gateway.agent.registered",
            "gateway.task.task_created",
            "gateway.task.plan_applied",
            "gateway.task.owner_approved",
            "gateway.task.submission_started",
            "gateway.task.provider_result",
            "gateway.agent.revoked",
            "gateway.budget.revoked",
        ]
    );
    let created = &recorded[3];
    assert_eq!(
        created["by"],
        json!({"owner":"local-user","role":"planner","agent":agent_id})
    );
    assert_eq!(created["agent"], json!(agent_id));
    let approved = &recorded[5];
    assert_eq!(approved["by"]["role"], "owner");
    assert_eq!(approved["by"]["agent"], Value::Null);
    assert_eq!(approved["approved_digest"], json!(digest));
    assert_eq!(approved["quote"]["digest"], json!(digest));
    assert_eq!(approved["quote"]["total_minor"], 700);
    assert_eq!(
        approved["reservation"],
        json!({"amount_minor":700,"state":"held"})
    );
    let started = &recorded[6];
    assert_eq!(started["by"]["role"], "executor");
    assert_eq!(started["by"]["agent"], json!(agent_id));
    let result = &recorded[7];
    assert_eq!(result["receipt"]["accepted"], true);
    assert_eq!(result["reservation"]["state"], "spent");
    assert_eq!(
        recorded[2]["grant"]["permissions"],
        json!(["plan", "execute"])
    );
    assert_eq!(recorded[8]["grant"]["revoked"], true);

    // Credentials and request text never enter the record.
    let all = db(&directory)
        .prepare("SELECT body FROM evidence")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|b| b.unwrap())
        .collect::<String>();
    assert!(!all.contains(&token));
    assert!(!all.contains(token.trim_start_matches("agentkey_")));
    assert!(!all.contains(private["owner"].as_str().unwrap()));
    assert!(!all.contains("oat latte"));

    let report = service.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
    assert_eq!(report.records, 10);
}

#[test]
fn records_form_a_hash_chain_in_fabric_format() {
    let (directory, service, ..) = purchased();
    let out = directory.path().join("evidence.jsonl");
    let records = service.export_evidence(&out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    let mut prev = "0".repeat(64);
    for (index, line) in text.lines().enumerate() {
        let (hash, body) = line.split_once(' ').unwrap();
        assert_eq!(hash, blake3::hash(body.as_bytes()).to_hex().as_str());
        let parsed: Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["v"], 1);
        assert_eq!(parsed["seq"], index as i64 + 1);
        assert_eq!(parsed["prev"], json!(prev));
        assert!(parsed["at"].as_str().unwrap().ends_with('Z'));
        prev = hash.to_string();
    }
    assert_eq!(text.lines().count() as i64, records);
    // An existing export is never overwritten.
    assert!(matches!(service.export_evidence(&out), Err(Error::Io(_))));
}

#[test]
fn state_changed_outside_the_service_is_detected() {
    // Each case starts from a fresh, verified ledger and edits it directly.
    let cases: [(&str, &str); 5] = [
        (
            "UPDATE reservations SET state='released'",
            "reservation changed without a record",
        ),
        (
            "UPDATE tasks SET data=replace(data,'\"confirmed\"','\"declined\"')",
            "changed without a record",
        ),
        (
            "UPDATE budgets SET data=replace(data,'\"limit_minor\":1000','\"limit_minor\":99000')",
            "changed without a record",
        ),
        (
            "INSERT INTO budgets(id,owner,data) SELECT 'budget_forged',owner,data FROM budgets",
            "exists without any record",
        ),
        (
            "DELETE FROM task_agents; INSERT INTO agents(id,owner,name,budget_id,permissions,expires_at,token_hash) \
             SELECT 'agent_forged',owner,'x',id,'[\"execute\"]',0,'h' FROM budgets",
            "exists without any record",
        ),
    ];
    for (sql, problem) in cases {
        let (directory, service, ..) = purchased();
        assert!(service.verify_evidence().unwrap().ok());
        db(&directory).execute_batch(sql).unwrap();
        let report = service.verify_evidence().unwrap();
        assert!(report.chain_intact, "{sql}");
        assert!(!report.state_matches, "{sql}");
        assert!(
            report.exceptions.iter().any(|e| e.problem == problem),
            "{sql}: {:?}",
            report.exceptions
        );
    }
}

#[test]
fn edited_removed_or_rewritten_records_are_detected() {
    let (directory, service, ..) = purchased();
    let ledger = db(&directory);
    // Appending is the only operation the database itself allows.
    assert!(
        ledger
            .execute("UPDATE evidence SET body='{}' WHERE seq=2", [])
            .is_err()
    );
    assert!(
        ledger
            .execute("DELETE FROM evidence WHERE seq=2", [])
            .is_err()
    );

    // Someone with write access can drop the guards; verification still catches the edit.
    ledger
        .execute_batch("DROP TRIGGER evidence_no_update; DROP TRIGGER evidence_no_delete;")
        .unwrap();
    ledger
        .execute(
            "UPDATE evidence SET body=replace(body,'\"total_minor\":700','\"total_minor\":1') WHERE seq=5",
            [],
        )
        .unwrap();
    let report = service.verify_evidence().unwrap();
    assert!(!report.chain_intact);
    assert_eq!(report.exceptions[0].seq, Some(5));

    // Removing the latest record leaves a valid prefix, but state no longer matches it.
    let (directory, service, ..) = purchased();
    let ledger = db(&directory);
    ledger
        .execute_batch("DROP TRIGGER evidence_no_delete; DELETE FROM evidence WHERE seq=(SELECT MAX(seq) FROM evidence);")
        .unwrap();
    let report = service.verify_evidence().unwrap();
    assert!(report.chain_intact);
    assert!(!report.state_matches);

    // A removed record in the middle breaks the sequence.
    let (directory, service, ..) = purchased();
    db(&directory)
        .execute_batch("DROP TRIGGER evidence_no_delete; DELETE FROM evidence WHERE seq=3;")
        .unwrap();
    assert!(!service.verify_evidence().unwrap().chain_intact);
}

#[test]
fn upgraded_ledgers_list_earlier_state_as_unevidenced_and_record_from_then_on() {
    let (directory, service, budget, task) = purchased();
    let second = service
        .create_task(&owner(), task_input(&budget, "two"), AT)
        .unwrap();
    // Reproduce a version 3 ledger in this synthetic store only.
    db(&directory)
        .execute_batch("DROP TABLE operation_holds; DROP TABLE operations; ALTER TABLE agents DROP COLUMN operation_grant; DROP TABLE evidence; PRAGMA user_version=3;")
        .unwrap();
    let upgraded = Service::open(directory.path()).unwrap();
    let recorded = events(&directory);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["kind"], "gateway.ledger.upgraded");
    assert_eq!(recorded[0]["from_version"], 3);
    assert_eq!(recorded[0]["unevidenced"]["budgets"], json!([budget]));
    let mut tasks = vec![task.clone(), second.id.clone()];
    tasks.sort();
    assert_eq!(recorded[0]["unevidenced"]["tasks"], json!(tasks));

    let report = upgraded.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        (report.unevidenced.budgets, report.unevidenced.tasks),
        (1, 2)
    );

    // Work continues on an old task; from then on it is evidenced and checked.
    upgraded.cancel(&owner(), &second.id, AT).unwrap();
    let report = upgraded.verify_evidence().unwrap();
    assert!(report.ok(), "{report:?}");
    assert_eq!(report.unevidenced.tasks, 1);
    db(&directory)
        .execute(
            "UPDATE tasks SET data=replace(data,'\"cancelled\"','\"draft\"') WHERE id=?1",
            [&second.id],
        )
        .unwrap();
    assert!(!upgraded.verify_evidence().unwrap().state_matches);
}

#[test]
fn verify_command_reports_through_its_exit_code() {
    let (directory, ..) = purchased();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
            .args(["evidence", "verify", "--data-dir"])
            .arg(directory.path())
            .output()
            .unwrap()
    };
    let ok = run();
    assert!(ok.status.success());
    let report: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(report["schema"], evidence::VERIFY_SCHEMA);
    assert_eq!(report["chain_intact"], true);

    db(&directory)
        .execute_batch("UPDATE reservations SET state='released'")
        .unwrap();
    let failed = run();
    assert_eq!(failed.status.code(), Some(5));

    // Verifying never creates a ledger where there was none.
    let empty = tempfile::tempdir().unwrap();
    let missing = std::process::Command::new(env!("CARGO_BIN_EXE_cosigna-agent"))
        .args(["evidence", "verify", "--data-dir"])
        .arg(empty.path())
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(!empty.path().join("transactions.sqlite").exists());
}
