//! Exercise the real binary and HTTP/auth boundary using only temporary stores.
use std::{
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
fn loopback_server_authenticates_roles_and_rejects_browser_origins() {
    let directory = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_cosigna-agent");
    let initialized = Command::new(binary)
        .args(["init", "--data-dir"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(initialized.status.success());
    let credentials: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("credentials.json")).unwrap())
            .unwrap();
    // Neither initialization nor startup may log the credentials.
    let init_output = String::from_utf8(initialized.stdout).unwrap();
    assert!(!init_output.contains(credentials["owner"].as_str().unwrap()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = Command::new(binary)
        .args(["serve", "--data-dir"])
        .arg(directory.path())
        .args(["--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _process = Process(child);
    let address = format!("http://127.0.0.1:{port}");
    let started = Instant::now();
    while std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err() {
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
    assert_eq!(
        client
            .get(format!("{address}/v1/health"))
            .call()
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!("{address}/v1/catalog"))
            .call()
            .unwrap()
            .status(),
        403
    );
    let planner_auth = format!("Bearer {}", credentials["planner"].as_str().unwrap());
    let owner_auth = format!("Bearer {}", credentials["owner"].as_str().unwrap());
    assert_eq!(
        client
            .get(format!("{address}/v1/catalog"))
            .header("Authorization", &planner_auth)
            .header("Origin", "https://untrusted.example")
            .call()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{address}/v1/health"))
            .header("Host", "untrusted.example")
            .call()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{address}/v1/catalog"))
            .header("Authorization", &planner_auth)
            .call()
            .unwrap()
            .status(),
        200
    );
    let budget = json!({"currency":"USD","limit_minor":1000,"per_purchase_minor":1000,
        "merchants":["fixture-cafe"],"expires_at":cosigna_agent::now()+3600});
    assert_eq!(
        client
            .post(format!("{address}/v1/budgets"))
            .header("Authorization", &planner_auth)
            .send_json(&budget)
            .unwrap()
            .status(),
        403
    );
    let mut response = client
        .post(format!("{address}/v1/budgets"))
        .header("Authorization", &owner_auth)
        .send_json(&budget)
        .unwrap();
    assert_eq!(response.status(), 200);
    let result: Value = response.body_mut().read_json().unwrap();
    assert_eq!(result["schema"], cosigna_agent::API_VERSION);
    let task = json!({"budget_id":result["data"]["id"],"request":"Order one oat latte","idempotency_key":"network-test"});
    let mut response = client
        .post(format!("{address}/v1/tasks"))
        .header("Authorization", &planner_auth)
        .send_json(&task)
        .unwrap();
    assert_eq!(response.status(), 200);
    let task_result: Value = response.body_mut().read_json().unwrap();
    let id = task_result["data"]["id"].as_str().unwrap();
    let mut response = client
        .post(format!("{address}/v1/tasks/{id}/plan"))
        .header("Authorization", &planner_auth)
        .send_json(json!({}))
        .unwrap();
    assert_eq!(response.status(), 200);
    let result: Value = response.body_mut().read_json().unwrap();
    assert_eq!(result["data"]["stage"], "quoted");
    assert_eq!(result["approval_digest"].as_str().unwrap().len(), 64);
    // The new headless gateway uses independent registered credentials. Existing
    // role tokens and all legacy envelopes above retain their original contract.
    let send_gateway = |auth: &str, path: &str, body: Value| {
        let mut response = client
            .post(format!("{address}/gateway/v1{path}"))
            .header("Authorization", auth)
            .send_json(body)
            .unwrap();
        let status = response.status().as_u16();
        assert_eq!(response.headers().get("Cache-Control").unwrap(), "no-store");
        let value: Value = response.body_mut().read_json().unwrap();
        assert_eq!(value["schema"], cosigna_agent::gateway::API_VERSION);
        (status, value)
    };
    let (status, issued) = send_gateway(
        &owner_auth,
        "/agents",
        json!({"name":"http-agent","budget_id":task["budget_id"],"permissions":["plan","execute"],"expires_at":cosigna_agent::now()+1800}),
    );
    assert_eq!(status, 200);
    let agent_id = issued["data"]["agent"]["id"].as_str().unwrap();
    let agent_auth = format!("Bearer {}", issued["data"]["token"].as_str().unwrap());
    assert_eq!(
        client
            .get(format!("{address}/v1/catalog"))
            .header("Authorization", &agent_auth)
            .call()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{address}/gateway/v1/catalog"))
            .header("Authorization", &agent_auth)
            .header("Origin", "https://untrusted.example")
            .call()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{address}/gateway/v1/catalog"))
            .header("Authorization", &agent_auth)
            .header("Host", "untrusted.example")
            .call()
            .unwrap()
            .status(),
        403
    );
    let (status, draft) = send_gateway(
        &agent_auth,
        "/tasks",
        json!({"budget_id":task["budget_id"],"request":"Order one oat latte","idempotency_key":"http-gateway-order"}),
    );
    assert_eq!(status, 200);
    assert_eq!(draft["agent_id"], agent_id);
    let gateway_task = draft["data"]["id"].as_str().unwrap();
    let (status, quote) = send_gateway(
        &agent_auth,
        &format!("/tasks/{gateway_task}/plan"),
        json!({}),
    );
    assert_eq!(status, 200);
    assert_eq!(
        send_gateway(
            &agent_auth,
            &format!("/tasks/{gateway_task}/approve"),
            json!({"quote_digest":quote["approval_digest"]})
        )
        .0,
        404
    );
    assert_eq!(
        send_gateway(
            &agent_auth,
            &format!("/tasks/{gateway_task}/execute"),
            json!({})
        )
        .0,
        409
    );
    assert_eq!(
        send_gateway(
            &owner_auth,
            &format!("/tasks/{gateway_task}/approve"),
            json!({"quote_digest":quote["approval_digest"]})
        )
        .0,
        200
    );
    let (status, confirmed) = send_gateway(
        &agent_auth,
        &format!("/tasks/{gateway_task}/execute"),
        json!({}),
    );
    assert_eq!(status, 200);
    assert_eq!(confirmed["data"]["stage"], "confirmed");
    let mut pipeline_response = client
        .get(format!("{address}/gateway/v1/pipelines/{gateway_task}"))
        .header("Authorization", &agent_auth)
        .call()
        .unwrap();
    assert_eq!(pipeline_response.status(), 200);
    let pipeline: Value = pipeline_response.body_mut().read_json().unwrap();
    assert_eq!(pipeline["schema"], cosigna_agent::pipeline::API_VERSION);
    assert_eq!(pipeline["status"], "confirmed");
    assert_eq!(pipeline["pipeline_id"], gateway_task);
    let mut rejected = client
        .post(format!(
            "{address}/gateway/v1/pipelines/{gateway_task}/advance"
        ))
        .header("Authorization", &agent_auth)
        .send_json(json!({"approved":true}))
        .unwrap();
    assert_eq!(rejected.status(), 400);
    let rejected_body: Value = rejected.body_mut().read_json().unwrap();
    assert_eq!(
        rejected_body["schema"],
        cosigna_agent::pipeline::API_VERSION
    );
    assert_eq!(
        send_gateway(
            &owner_auth,
            &format!("/agents/{agent_id}/revoke"),
            json!({})
        )
        .0,
        200
    );
    assert_eq!(
        send_gateway(
            &agent_auth,
            &format!("/tasks/{gateway_task}/execute"),
            json!({})
        )
        .0,
        403
    );
    let (status, recovered) = send_gateway(
        &owner_auth,
        &format!("/tasks/{gateway_task}/reconcile"),
        json!({}),
    );
    assert_eq!(status, 200);
    assert_eq!(
        recovered["data"]["receipt"]["order_id"],
        confirmed["data"]["receipt"]["order_id"]
    );
    assert_eq!(
        client
            .post(format!("{address}/v1/tasks"))
            .header("Authorization", &planner_auth)
            .header("Content-Type", "text/plain")
            .send("{}")
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .post(format!("{address}/v1/tasks"))
            .header("Authorization", &planner_auth)
            .header("Content-Type", "application/json")
            .send("x".repeat(16_385))
            .unwrap()
            .status(),
        400
    );
}
