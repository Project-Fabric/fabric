use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_cosigna-agent");

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http(port: u16, auth: &str, method: &str, path: &str, body: Value) -> Value {
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let url = format!("http://127.0.0.1:{port}/gateway/v1{path}");
    let mut response = if method == "GET" {
        agent
            .get(&url)
            .header("Authorization", auth)
            .call()
            .unwrap()
    } else {
        agent
            .post(&url)
            .header("Authorization", auth)
            .send_json(body)
            .unwrap()
    };
    response.body_mut().read_json().unwrap()
}

/// A served gateway with one budget and one refunds agent ($100 automatic limit).
fn gateway() -> (tempfile::TempDir, Running, u16, String, String, String) {
    let directory = tempfile::tempdir().unwrap();
    assert!(
        Command::new(BIN)
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
    let port = free_port();
    let server = Running(
        Command::new(BIN)
            .args(["serve", "--port", &port.to_string(), "--data-dir"])
            .arg(directory.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for _ in 0..200 {
        if ureq::get(format!("http://127.0.0.1:{port}/v1/health"))
            .call()
            .is_ok()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let now = cosigna_agent::now();
    let budget = http(
        port,
        &owner,
        "POST",
        "/budgets",
        json!({"currency":"USD","limit_minor":100000,
        "per_purchase_minor":50000,"merchants":["fixture-cafe"],"expires_at":now+3600}),
    )["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let issued = http(
        port,
        &owner,
        "POST",
        "/agents",
        json!({"name":"support-agent","budget_id":budget,
        "permissions":["execute"],"expires_at":now+1800,
        "operations":{"kinds":["refund"],"rails":["fixture"],"auto_approve_minor":10000}}),
    );
    let token = issued["data"]["token"].as_str().unwrap().to_string();
    (directory, server, port, owner, budget, token)
}

struct Mcp {
    child: Running,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Mcp {
    fn start(port: u16, token: &str) -> Self {
        let mut child = Command::new(BIN)
            .args(["mcp", "--gateway-url", &format!("http://127.0.0.1:{port}")])
            .env("COSIGNA_AGENT_TOKEN", token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Self {
            child: Running(child),
            out,
            next: 0,
        }
    }

    fn send(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params});
        let stdin = self.child.0.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut reply = String::new();
        self.out.read_line(&mut reply).unwrap();
        let value: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(value["id"], self.next);
        value["result"].clone()
    }

    fn tool(&mut self, name: &str, arguments: Value) -> (Value, bool) {
        let result = self.send("tools/call", json!({"name":name,"arguments":arguments}));
        let text = result["content"][0]["text"].as_str().unwrap();
        (
            serde_json::from_str(text).unwrap(),
            result["isError"] == true,
        )
    }
}

#[test]
fn an_agent_moves_money_only_through_its_tools_and_its_grant() {
    let (directory, _server, port, owner, budget, token) = gateway();
    let mut mcp = Mcp::start(port, &token);
    let init = mcp.send(
        "initialize",
        json!({"protocolVersion":"2025-06-18","clientInfo":{"name":"claude-code"}}),
    );
    assert_eq!(init["serverInfo"]["name"], "cosigna");
    assert!(
        init["instructions"]
            .as_str()
            .unwrap()
            .contains("Never try to get around a denial")
    );
    // Notifications get no reply; the next request still lines up.
    let stdin = mcp.child.0.stdin.as_mut().unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    let tools = mcp.send("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "cosigna_request_refund",
            "cosigna_request_payout",
            "cosigna_operation_status",
            "cosigna_budget"
        ]
    );

    let refund = |amount: i64, source: &str, key: &str| {
        json!({"budget_id":budget,"rail":"fixture",
        "source":source,"amount_minor":amount,"currency":"USD","reason":"Order cancelled","idempotency_key":key})
    };

    let (paid, is_error) = mcp.tool(
        "cosigna_request_refund",
        refund(2000, "pay_fixture_2041", "t1"),
    );
    assert!(!is_error);
    assert_eq!(paid["stage"], "succeeded");
    assert!(
        paid["reference"]
            .as_str()
            .unwrap()
            .starts_with("re_fixture_")
    );
    assert!(
        paid.get("proposal_digest").is_none(),
        "agents never see approval digests"
    );

    let (held, is_error) = mcp.tool(
        "cosigna_request_refund",
        refund(45000, "pay_fixture_2045", "t2"),
    );
    assert!(!is_error);
    assert_eq!(held["stage"], "held");
    assert!(
        held["next"]
            .as_str()
            .unwrap()
            .contains("Waiting for a person")
    );

    let (denied, is_error) = mcp.tool(
        "cosigna_request_refund",
        refund(100, "pay_fixture_2041", "t3"),
    );
    assert!(is_error);
    assert_eq!(denied["reason_code"], "exceeds_refundable");

    let (status, _) = mcp.tool(
        "cosigna_operation_status",
        json!({"operation_id": held["operation_id"]}),
    );
    assert_eq!(status["stage"], "held");
    let (budget_left, _) = mcp.tool("cosigna_budget", json!({"budget_id": budget}));
    assert_eq!(budget_left["available_minor"], 98000);

    // The owner approves outside the agent's reach; the agent then sees it paid.
    let envelope = http(
        port,
        &owner,
        "GET",
        &format!("/operations/{}", held["operation_id"].as_str().unwrap()),
        Value::Null,
    );
    http(
        port,
        &owner,
        "POST",
        &format!(
            "/operations/{}/approve",
            held["operation_id"].as_str().unwrap()
        ),
        json!({"proposal_digest": envelope["proposal_digest"]}),
    );
    let (after, _) = mcp.tool(
        "cosigna_operation_status",
        json!({"operation_id": held["operation_id"]}),
    );
    assert_eq!(after["stage"], "succeeded");

    // Malformed and smuggled input fails without effect.
    let mut smuggled = refund(100, "pay_fixture_2049", "t4");
    smuggled["approved"] = json!(true);
    let (_, is_error) = mcp.tool("cosigna_request_refund", smuggled);
    assert!(is_error);
    let (_, is_error) = mcp.tool(
        "cosigna_operation_status",
        json!({"operation_id": "../budgets"}),
    );
    assert!(is_error);

    let report = Command::new(BIN)
        .args(["evidence", "verify", "--data-dir"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(report.status.success());
}

#[test]
fn the_tool_server_refuses_owner_tokens_and_remote_gateways() {
    for (url, token) in [
        ("http://127.0.0.1:1", "owner_abc"),
        ("https://gateway.example", "agentkey_abc"),
        ("http://localhost:3000", "agentkey_abc"),
    ] {
        let out = Command::new(BIN)
            .args(["mcp", "--gateway-url", url])
            .env("COSIGNA_AGENT_TOKEN", token)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{url} {token}");
    }
}
