//! Per-client limits on the real binary: a request budget per credential, blocking an address
//! after repeated refusals (behind the proxy, the address the proxy reports), and slots kept
//! for people when agents fill the gateway. Temporary stores and loopback only.
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use cosigna_agent::{Service, api::Credentials, operations::OperationStage, worker::Worker};
use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_cosigna-agent");

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct Gateway {
    port: u16,
    credentials: Value,
    // Stop the server before deleting its store (fields drop in declaration order).
    _process: Process,
    _directory: tempfile::TempDir,
}

fn start(args: &[&str]) -> Gateway {
    let directory = tempfile::tempdir().unwrap();
    assert!(
        Command::new(BINARY)
            .args(["init", "--data-dir"])
            .arg(directory.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    let credentials: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("credentials.json")).unwrap())
            .unwrap();
    let port = free_port();
    let mut command = Command::new(BINARY);
    command
        .args(["serve", "--data-dir"])
        .arg(directory.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !args.contains(&"--listen") {
        command.args(["--port", &port.to_string()]);
    }
    let args: Vec<String> = args
        .iter()
        .map(|a| a.replace("{port}", &port.to_string()))
        .collect();
    let process = Process(command.args(&args).spawn().unwrap());
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    Gateway {
        port,
        credentials,
        _directory: directory,
        _process: process,
    }
}

impl Gateway {
    fn bearer(&self, role: &str) -> String {
        format!("Bearer {}", self.credentials[role].as_str().unwrap())
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn post(&self, authorization: &str, path: &str, body: Value) -> (u16, Value) {
        let mut response = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .new_agent()
            .post(format!("http://{}/gateway/v1{path}", self.host()))
            .header("Authorization", authorization)
            .send_json(body)
            .unwrap();
        let status = response.status().as_u16();
        let value = response.body_mut().read_json().unwrap();
        (status, value)
    }

    fn refund_agent(&self, budget_limit: i64, auto_limit: i64) -> (String, String) {
        let owner = self.bearer("owner");
        let (status, budget) = self.post(
            &owner,
            "/budgets",
            json!({"currency":"USD","limit_minor":budget_limit,
                "per_purchase_minor":budget_limit,"merchants":["fixture-cafe"],
                "expires_at":cosigna_agent::now()+3600}),
        );
        assert_eq!(status, 200);
        let budget = budget["data"]["id"].as_str().unwrap().to_string();
        let (status, issued) = self.post(
            &owner,
            "/agents",
            json!({"name":"synthetic-load-agent","budget_id":budget,
                "permissions":["execute"],"expires_at":cosigna_agent::now()+3600,
                "operations":{"kinds":["refund"],"rails":["fixture"],
                    "auto_approve_minor":auto_limit,"require_case":true}}),
        );
        assert_eq!(status, 200);
        let auth = format!("Bearer {}", issued["data"]["token"].as_str().unwrap());
        (budget, auth)
    }

    /// One GET with exactly these headers; returns the status and the whole response.
    fn get(&self, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = format!("GET {path} HTTP/1.1\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        (status, response)
    }

    /// A request that announces a body and never sends it, holding a slot until dropped.
    fn stall(&self, authorization: &str) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .write_all(
                format!(
                    "POST /gateway/v1/operations HTTP/1.1\r\nHost: {}\r\nAuthorization: {authorization}\r\n\
                     Content-Type: application/json\r\nContent-Length: 10\r\n\r\n",
                    self.host()
                )
                .as_bytes(),
            )
            .unwrap();
        stream
    }
}

fn refund(budget: &str, amount: i64, key: &str) -> Value {
    json!({"budget_id":budget,"kind":"refund","rail":"fixture",
        "source":"pay_fixture_2045","case_id":"case_fixture_2045",
        "amount_minor":amount,"currency":"USD","reason":"Synthetic overload regression",
        "idempotency_key":key})
}

#[test]
fn a_preclaim_database_failure_under_admission_saturation_recovers_without_resending() {
    // A deterministic fault at the same boundary as the earlier overload incident:
    // approval committed, SQLite's writer unavailable, no attempt claimed. This does not
    // establish the original incident's cause or a throughput limit.
    let gateway = start(&[
        "--max-concurrent",
        "4",
        "--request-timeout",
        "12",
        "--rate-limit",
        "1000",
    ]);
    let (budget, agent) = gateway.refund_agent(1000, 0);
    let request = refund(&budget, 500, "interrupted-under-load");
    let (status, held) = gateway.post(&agent, "/operations", request.clone());
    assert_eq!(status, 200);
    assert_eq!(held["data"]["stage"], "held");
    let id = held["data"]["id"].as_str().unwrap();
    let digest = held["proposal_digest"].as_str().unwrap();
    let service = Service::open(gateway._directory.path()).unwrap();
    let credentials =
        Credentials::initialize(&gateway._directory.path().join("credentials.json")).unwrap();
    let owner = credentials.authenticate(&gateway.bearer("owner")).unwrap();
    let at = cosigna_agent::now();
    service.approve_operation(&owner, id, digest, at).unwrap();

    // Occupy all three shared slots with real authenticated agent connections. A fourth
    // agent is rejected, while the owner's separately reserved slot remains available.
    let stalled: Vec<_> = (0..3).map(|_| gateway.stall(&agent)).collect();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = gateway
            .get(
                &format!("/gateway/v1/operations/{id}"),
                &[("Host", &gateway.host()), ("Authorization", &agent)],
            )
            .0;
        if status == 503 {
            break;
        }
        assert_eq!(status, 200);
        assert!(
            Instant::now() < deadline,
            "shared admission slots did not fill"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let blocker =
        rusqlite::Connection::open(gateway._directory.path().join("transactions.sqlite")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    // Keep the writer locked until the server's five-second SQLite busy timeout fails.
    // The HTTP deadline is longer, so the actual internal error is observable, not hidden
    // behind a client timeout. No payment or model provider is contacted.
    let (status, failure) = gateway.post(
        &gateway.bearer("owner"),
        &format!("/operations/{id}/execute"),
        json!({}),
    );
    assert_eq!(status, 500);
    assert_eq!(failure["error"]["code"], "internal_error");
    assert_eq!(
        gateway.get("/v1/health", &[("Host", &gateway.host())]).0,
        200
    );
    blocker.execute_batch("ROLLBACK").unwrap();
    drop(stalled);

    let op = service.operation(&owner, id).unwrap();
    assert_eq!(op.stage, OperationStage::Approved);
    assert!(op.attempt_key.is_none());
    assert_eq!(
        service.balance(&owner, &budget).unwrap().reserved_minor,
        500
    );
    let rail = service.rails.fixture_rail().unwrap();
    assert_eq!(rail.attempt_count().unwrap(), 0);

    let mut worker = Worker::open(&service).unwrap();
    let recovered = worker
        .recover_operations(at + cosigna_agent::worker::RECOVERY_GRACE + 30)
        .unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(recovered[0].needs_person);
    let op = service.operation(&owner, id).unwrap();
    assert_eq!(op.stage, OperationStage::Held);
    assert!(op.approved_digest.is_none() && op.attempt_key.is_none());
    let balance = service.balance(&owner, &budget).unwrap();
    assert_eq!(balance.available_minor, 1000);
    assert_eq!(balance.reserved_minor, 0);
    assert_eq!(balance.spent_minor, 0);
    assert_eq!(rail.attempt_count().unwrap(), 0);

    // Neither the original POST replay nor a stale execution may restart spending.
    let (status, replay) = gateway.post(&agent, "/operations", request);
    assert_eq!(status, 200);
    assert_eq!(replay["data"]["id"], id);
    assert_eq!(replay["data"]["stage"], "held");
    assert_eq!(
        gateway
            .post(&agent, &format!("/operations/{id}/execute"), json!({}))
            .0,
        409
    );
    assert_eq!(rail.attempt_count().unwrap(), 0);
    // A fresh exact-proposal approval still permits one legitimate refund.
    let (status, resumed) = gateway.post(
        &gateway.bearer("owner"),
        &format!("/operations/{id}/approve"),
        json!({"proposal_digest":digest}),
    );
    assert_eq!(status, 200);
    assert_eq!(resumed["data"]["stage"], "succeeded");
    assert_eq!(rail.attempt_count().unwrap(), 1);
    assert_eq!(rail.refunded_minor("pay_fixture_2045").unwrap(), 500);
    let balance = service.balance(&owner, &budget).unwrap();
    assert_eq!(balance.spent_minor, 500);
    assert_eq!(balance.reserved_minor, 0);
    assert!(service.verify_evidence().unwrap().ok());
}

#[test]
fn concurrent_http_refunds_and_replays_cannot_overspend_or_duplicate_effects() {
    let gateway = start(&["--max-concurrent", "8", "--rate-limit", "1000"]);
    let (budget, agent) = gateway.refund_agent(1000, 1000);
    let barrier = std::sync::Barrier::new(48);
    let statuses = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..48)
            .map(|i| {
                let request = refund(&budget, 100, &format!("concurrent-{}", i % 24));
                let gateway = &gateway;
                let agent = &agent;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    gateway.post(agent, "/operations", request).0
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    // Bounded admission may refuse excess work; successful admission must not become an
    // unexplained internal failure. Replay only the exact original key and payload.
    assert!(
        statuses.iter().all(|s| matches!(s, 200 | 503 | 504)),
        "{statuses:?}"
    );
    let mut succeeded = 0;
    let mut denied = 0;
    for i in 0..24 {
        let (status, op) = gateway.post(
            &agent,
            "/operations",
            refund(&budget, 100, &format!("concurrent-{i}")),
        );
        assert_eq!(status, 200);
        match op["data"]["stage"].as_str().unwrap() {
            "succeeded" => succeeded += 1,
            "denied" => denied += 1,
            stage => panic!("unexpected final stage: {stage}"),
        }
    }
    assert_eq!((succeeded, denied), (10, 14));
    let service = Service::open(gateway._directory.path()).unwrap();
    let credentials =
        Credentials::initialize(&gateway._directory.path().join("credentials.json")).unwrap();
    let owner = credentials.authenticate(&gateway.bearer("owner")).unwrap();
    let ops = service.operations(&owner, None).unwrap();
    assert_eq!(ops.len(), 24);
    let balance = service.balance(&owner, &budget).unwrap();
    assert_eq!(balance.available_minor, 0);
    assert_eq!(balance.spent_minor, 1000);
    assert_eq!(balance.reserved_minor, 0);
    let rail = service.rails.fixture_rail().unwrap();
    assert_eq!(rail.refunded_minor("pay_fixture_2045").unwrap(), 1000);
    assert_eq!(rail.attempt_count().unwrap(), 10);
    assert!(service.verify_evidence().unwrap().ok());
}

#[test]
fn each_credential_has_its_own_request_budget() {
    let gateway = start(&["--rate-limit", "2"]);
    let host = gateway.host();
    let owner = gateway.bearer("owner");
    let list = |auth: &str| {
        gateway.get(
            "/gateway/v1/operations",
            &[("Host", &host), ("Authorization", auth)],
        )
    };
    // A burst of twice the rate, then the client is told when to come back.
    for _ in 0..4 {
        assert_eq!(list(&owner).0, 200);
    }
    let (status, response) = list(&owner);
    assert_eq!(status, 429, "{response}");
    assert!(response.contains("rate_limited"));
    assert!(
        response.to_ascii_lowercase().contains("retry-after: 1"),
        "{response}"
    );
    // Another credential, and health checks, are not affected.
    let planner = gateway.bearer("planner");
    assert_eq!(
        gateway
            .get(
                "/v1/catalog",
                &[("Host", &host), ("Authorization", &planner)]
            )
            .0,
        200
    );
    assert_eq!(gateway.get("/v1/health", &[("Host", &host)]).0, 200);
    // The budget refills at the configured rate.
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(list(&owner).0, 200);
}

#[test]
fn an_address_that_keeps_guessing_tokens_is_blocked() {
    let gateway = start(&["--refusals-per-minute", "3"]);
    let host = gateway.host();
    for i in 0..3 {
        let wrong = format!("Bearer agentkey_guess{i}");
        let (status, _) = gateway.get(
            "/gateway/v1/operations",
            &[("Host", &host), ("Authorization", &wrong)],
        );
        assert_eq!(status, 403);
    }
    // Now even the right token is refused from this address for the rest of the minute.
    let owner = gateway.bearer("owner");
    let (status, response) = gateway.get(
        "/gateway/v1/operations",
        &[("Host", &host), ("Authorization", &owner)],
    );
    assert_eq!(status, 429, "{response}");
    assert!(response.contains("refused requests"));
    assert_eq!(gateway.get("/v1/health", &[("Host", &host)]).0, 200);
}

#[test]
fn behind_the_proxy_the_address_it_reports_is_the_one_counted() {
    let gateway = start(&[
        "--listen",
        "127.0.0.1:{port}",
        "--public-host",
        "cosigna.example.test",
        "--refusals-per-minute",
        "2",
    ]);
    let owner = gateway.bearer("owner");
    let call = |forwarded: &str, auth: &str| {
        gateway
            .get(
                "/gateway/v1/operations",
                &[
                    ("Host", "cosigna.example.test"),
                    ("X-Forwarded-Proto", "https"),
                    ("X-Forwarded-For", forwarded),
                    ("Authorization", auth),
                ],
            )
            .0
    };
    assert_eq!(call("198.51.100.1", "Bearer owner_wrong1"), 403);
    assert_eq!(call("198.51.100.1", "Bearer owner_wrong2"), 403);
    // That client is blocked; another client behind the same proxy is not.
    assert_eq!(call("198.51.100.1", &owner), 429);
    assert_eq!(call("198.51.100.2", &owner), 200);
    // Only the entry the proxy appended last counts: a client cannot pick its own address.
    assert_eq!(call("198.51.100.2, 198.51.100.1", &owner), 429);
    assert_eq!(call("198.51.100.1, 198.51.100.2", &owner), 200);
}

#[test]
fn people_can_still_act_when_agents_fill_the_gateway() {
    // Eight slots: seven for anyone, one kept for people.
    let gateway = start(&["--max-concurrent", "8"]);
    let host = gateway.host();
    let held: Vec<TcpStream> = (0..7)
        .map(|i| gateway.stall(&format!("Bearer agentkey_flood{i}")))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let (status, _) = gateway.get(
        "/gateway/v1/operations",
        &[("Host", &host), ("Authorization", "Bearer agentkey_flood8")],
    );
    assert_eq!(status, 503, "an eighth agent request was admitted");
    let owner = gateway.bearer("owner");
    assert_eq!(
        gateway
            .get(
                "/gateway/v1/operations",
                &[("Host", &host), ("Authorization", &owner)]
            )
            .0,
        200
    );
    drop(held);
}

#[test]
fn zero_rate_limits_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    assert!(
        Command::new(BINARY)
            .args(["init", "--data-dir"])
            .arg(directory.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    for args in [["--rate-limit", "0"], ["--refusals-per-minute", "0"]] {
        let output = Command::new(BINARY)
            .args(["serve", "--data-dir"])
            .arg(directory.path())
            .args(["--port", &free_port().to_string()])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} was accepted");
    }
}

#[test]
fn unauthenticated_stalls_cannot_take_the_slots_kept_for_people() {
    // Eight slots: seven for anyone, one kept for people.
    let gateway = start(&["--max-concurrent", "8"]);
    let host = gateway.host();
    // Junk credentials that are not agent keys stall their bodies: they may fill the shared
    // slots, but not the one kept for people.
    let held: Vec<TcpStream> = (0..8)
        .map(|i| gateway.stall(&format!("Bearer junk{i}")))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let owner = gateway.bearer("owner");
    assert_eq!(
        gateway
            .get(
                "/gateway/v1/operations",
                &[("Host", &host), ("Authorization", &owner)]
            )
            .0,
        200
    );
    drop(held);
}

#[test]
fn on_the_trusted_console_path_one_credentials_refusals_do_not_block_the_owner() {
    let gateway = start(&[
        "--listen",
        "127.0.0.1:{port}",
        "--public-host",
        "cosigna.example.test",
        "--trust-local-clients",
        "--refusals-per-minute",
        "2",
    ]);
    let host = gateway.host();
    let call = |auth: &str| {
        gateway
            .get(
                "/gateway/v1/operations",
                &[("Host", &host), ("Authorization", auth)],
            )
            .0
    };
    // A revoked or wrong key used through the console is refused, then blocked on its own.
    assert_eq!(call("Bearer approverkey_revoked"), 403);
    assert_eq!(call("Bearer approverkey_revoked"), 403);
    assert_eq!(call("Bearer approverkey_revoked"), 429);
    // The owner, on the same loopback address, is not.
    assert_eq!(call(&gateway.bearer("owner")), 200);
}

#[test]
fn a_valid_machine_credential_is_told_busy_but_not_counted_as_refused() {
    let gateway = start(&["--max-concurrent", "8", "--refusals-per-minute", "2"]);
    let host = gateway.host();
    let held: Vec<TcpStream> = (0..7)
        .map(|i| gateway.stall(&format!("Bearer agentkey_flood{i}")))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let planner = gateway.bearer("planner");
    for _ in 0..4 {
        let (status, response) = gateway.get(
            "/gateway/v1/operations",
            &[("Host", &host), ("Authorization", &planner)],
        );
        assert_eq!(status, 503, "{response}");
    }
    // A wrong credential under the same load does count, even though it is only told busy.
    for i in 0..2 {
        let (status, response) = gateway.get(
            "/gateway/v1/operations",
            &[
                ("Host", &host),
                ("Authorization", &format!("Bearer junk{i}")),
            ],
        );
        assert_eq!(status, 503, "{response}");
    }
    let owner = gateway.bearer("owner");
    assert_eq!(
        gateway
            .get(
                "/gateway/v1/operations",
                &[("Host", &host), ("Authorization", &owner)]
            )
            .0,
        429
    );
    drop(held);
}
