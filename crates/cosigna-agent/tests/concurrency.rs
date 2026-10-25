//! The real binary keeps answering while some requests are stuck: a slow client (or, in the same
//! way, a slow rail or planner call) holds one worker, not the whole gateway. Temporary stores only.
use std::{
    io::Write,
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn stalled_requests_do_not_block_status_approvals_or_revocation() {
    let directory = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_cosigna-agent");
    assert!(
        Command::new(binary)
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
    let owner = format!("Bearer {}", credentials["owner"].as_str().unwrap());
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let _process = Process(
        Command::new(binary)
            .args(["serve", "--data-dir"])
            .arg(directory.path())
            .args(["--port", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let client = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .new_agent();
    let address = format!("http://127.0.0.1:{port}/gateway/v1");
    let post = |path: &str, body: Value| -> Value {
        client
            .post(&format!("{address}{path}"))
            .header("Authorization", &owner)
            .send_json(body)
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap()
    };
    let budget = post(
        "/budgets",
        json!({"currency":"USD","limit_minor":10000,"per_purchase_minor":10000,
            "merchants":["fixture-cafe"],"expires_at":4_000_000_000_i64}),
    )["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent = post(
        "/agents",
        json!({"name":"refunds-bot","budget_id":budget,"permissions":["execute"],
            "expires_at":4_000_000_000_i64,
            "operations":{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":1000}}),
    )["data"]["agent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Three clients send complete headers and then stop halfway through a payment request body
    // (larger than the HTTP library reads ahead, so a worker is really stuck reading it).
    let stalled: Vec<TcpStream> = (0..3)
        .map(|_| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                stream,
                "POST /gateway/v1/operations HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
                Authorization: {owner}\r\nContent-Type: application/json\r\n\
                Content-Length: 10000\r\n\r\n{{\"budget_id\":"
            )
            .unwrap();
            stream.flush().unwrap();
            stream
        })
        .collect();
    std::thread::sleep(Duration::from_millis(300));

    // Everything else still answers promptly: status, the inbox, the exposure report, revocation.
    let quick = Instant::now();
    let health = client
        .get(&format!("http://127.0.0.1:{port}/v1/health"))
        .call()
        .unwrap();
    assert_eq!(health.status(), 200);
    for path in ["/operations?stage=held", "/exposure"] {
        let response = client
            .get(&format!("{address}{path}"))
            .header("Authorization", &owner)
            .call()
            .unwrap();
        assert_eq!(response.status(), 200, "{path}");
    }
    let revoked = post(&format!("/agents/{agent}/revoke"), json!({}));
    assert_eq!(revoked["data"]["revoked"], true);
    assert!(
        quick.elapsed() < Duration::from_secs(2),
        "answered in {:?} while three requests were stalled",
        quick.elapsed()
    );
    drop(stalled);
}
