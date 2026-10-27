//! Self-hosting behind a TLS proxy: `serve --listen --public-host`, `probe`, and secrets read
//! from files. The real binary, temporary stores and loopback only.
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command, Output, Stdio},
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

fn init() -> (tempfile::TempDir, String) {
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
    let owner = credentials["owner"].as_str().unwrap().to_string();
    (directory, owner)
}

fn wait_for(port: u16) {
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One HTTP/1.1 request with exactly these headers; returns the status code.
fn status(port: u16, path: &str, headers: &[(&str, &str)]) -> u16 {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = format!("GET {path} HTTP/1.1\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

/// Starts `serve` and returns its output if it exits within 5 seconds. A server that is still
/// running then was accepted: it is stopped and reported as a success, so a missing refusal
/// fails the test instead of hanging it.
fn serve(directory: &std::path::Path, args: &[&str]) -> Output {
    let mut child = Command::new(BINARY)
        .args(["serve", "--data-dir"])
        .arg(directory)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let mut output = child.wait_with_output().unwrap();
            output.status = Command::new(BINARY).arg("--version").status().unwrap();
            return output;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn behind_a_proxy_only_the_public_host_over_https_is_served() {
    let (directory, owner) = init();
    let port = free_port();
    let child = Command::new(BINARY)
        .args(["serve", "--data-dir"])
        .arg(directory.path())
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--public-host",
            "cosigna.example.test",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _process = Process(child);
    wait_for(port);
    let auth = format!("Bearer {owner}");
    let local = format!("127.0.0.1:{port}");
    let api = "/gateway/v1/operations";

    // Through the proxy: the public name and https, with or without the proxy's port.
    for host in [
        "cosigna.example.test",
        "cosigna.example.test:443",
        "cosigna.example.test:8443",
    ] {
        assert_eq!(
            status(
                port,
                api,
                &[
                    ("Host", host),
                    ("X-Forwarded-Proto", "https"),
                    ("Authorization", &auth)
                ]
            ),
            200,
            "{host}"
        );
    }
    // Plain HTTP, a missing proxy header, another name, or the local name are refused, even
    // with the owner's credential.
    for headers in [
        vec![("Host", "cosigna.example.test"), ("Authorization", &auth)],
        vec![
            ("Host", "cosigna.example.test"),
            ("X-Forwarded-Proto", "http"),
            ("Authorization", &auth),
        ],
        vec![
            ("Host", "evil.example.test"),
            ("X-Forwarded-Proto", "https"),
            ("Authorization", &auth),
        ],
        // Names that merely start with the public host, or carry a bad port.
        vec![
            ("Host", "cosigna.example.test.evil.test"),
            ("X-Forwarded-Proto", "https"),
            ("Authorization", &auth),
        ],
        vec![
            ("Host", "cosigna.example.test:0"),
            ("X-Forwarded-Proto", "https"),
            ("Authorization", &auth),
        ],
        vec![
            ("Host", "cosigna.example.test:x"),
            ("X-Forwarded-Proto", "https"),
            ("Authorization", &auth),
        ],
        vec![
            ("Host", local.as_str()),
            ("X-Forwarded-Proto", "https"),
            ("Authorization", &auth),
        ],
        vec![
            ("Host", "cosigna.example.test"),
            ("X-Forwarded-Proto", "https"),
            ("Authorization", &auth),
            ("Origin", "https://cosigna.example.test"),
        ],
    ] {
        assert_eq!(status(port, api, &headers), 403, "{headers:?}");
    }
    // Health answers container probes on the local name, and through the proxy.
    assert_eq!(status(port, "/v1/health", &[("Host", &local)]), 200);
    assert_eq!(
        status(
            port,
            "/v1/health",
            &[
                ("Host", "cosigna.example.test"),
                ("X-Forwarded-Proto", "https")
            ]
        ),
        200
    );
    assert_eq!(
        status(port, "/v1/health", &[("Host", "cosigna.example.test")]),
        403
    );

    let probe = |port: u16| {
        Command::new(BINARY)
            .args(["probe", "--port", &port.to_string()])
            .status()
            .unwrap()
            .success()
    };
    assert!(probe(port));
    assert!(!probe(free_port()));
}

#[test]
fn unsafe_listen_settings_are_refused_before_binding() {
    let (directory, _) = init();
    let port = free_port().to_string();
    let refusals = [
        // Beyond this machine without naming the proxy's host.
        vec!["--listen".to_string(), format!("0.0.0.0:{port}")],
        vec!["--listen".to_string(), format!("[::]:{port}")],
        // Host names with a scheme, port, path or capitals.
        vec![
            "--listen".to_string(),
            format!("0.0.0.0:{port}"),
            "--public-host".to_string(),
            "https://cosigna.example.test".to_string(),
        ],
        vec![
            "--public-host".to_string(),
            "cosigna.example.test:8443".to_string(),
        ],
        vec!["--public-host".to_string(), "Cosigna.Example".to_string()],
        vec!["--public-host".to_string(), "a/b".to_string()],
        vec!["--listen".to_string(), "127.0.0.1:0".to_string()],
        // Both ways of choosing the port at once.
        vec![
            "--port".to_string(),
            port.clone(),
            "--listen".to_string(),
            format!("127.0.0.1:{port}"),
        ],
    ];
    for args in refusals {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = serve(directory.path(), &args);
        assert!(!output.status.success(), "{args:?} was accepted");
    }
    let output = serve(directory.path(), &["--listen", &format!("0.0.0.0:{port}")]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("--public-host"));
}

#[test]
fn provider_secrets_can_come_from_mounted_files() {
    let (directory, _) = init();
    let secrets = tempfile::tempdir().unwrap();
    let start = |key: &str| {
        let file = secrets.path().join("stripe");
        std::fs::write(&file, key).unwrap();
        let port = free_port();
        let mut child = Command::new(BINARY)
            .args(["serve", "--data-dir"])
            .arg(directory.path())
            .args(["--port", &port.to_string(), "--stripe-test"])
            .env_remove("COSIGNA_STRIPE_TEST_SECRET_KEY")
            .env("COSIGNA_STRIPE_TEST_SECRET_KEY_FILE", &file)
            .env("FABRIC_HOME", secrets.path().join("no-fabric-home"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(exit) = child.try_wait().unwrap() {
                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                return (Some(exit.success()), stderr);
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                return (None, String::from_utf8_lossy(&output.stderr).into_owned());
            }
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // A file with a trailing newline, as editors and `docker secret` write them.
    let key = "sk_test_51FileMountedKeyForTests";
    let (exited, stderr) = start(&format!("{key}\n"));
    assert_eq!(exited, None, "the gateway did not start: {stderr}");
    assert!(stderr.contains("stripe_test"));
    assert!(!stderr.contains(key), "the key was printed");
    // A live key in the file is refused as it would be from the environment.
    let (exited, stderr) = start("sk_live_51NotAllowedEvenFromAFile\n");
    assert_eq!(exited, Some(false));
    assert!(stderr.contains("test-mode"));
    // An empty file counts as no key at all, not as an invalid one.
    let (exited, stderr) = start("\n");
    assert_eq!(exited, Some(false));
    assert!(
        stderr.contains("set COSIGNA_STRIPE_TEST_SECRET_KEY"),
        "{stderr}"
    );
}
