//! Per-client limits on the real binary: a request budget per credential, blocking an address
//! after repeated refusals (behind the proxy, the address the proxy reports), and slots kept
//! for people when agents fill the gateway. Temporary stores and loopback only.
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::Value;

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
    _directory: tempfile::TempDir,
    _process: Process,
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
