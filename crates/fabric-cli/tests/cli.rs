use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    home: PathBuf,
    project: PathBuf,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", self.stdout))
    }
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("fabric-home");
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        Self {
            _dir: dir,
            home,
            project,
        }
    }

    fn run_with_stdin(&self, args: &[&str], stdin: &str) -> Run {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        Run {
            code: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }

    /// `fabric`, isolated from the real user configuration: its own data directory, a Codex
    /// home inside the fixture, and no access to the real `claude` CLI.
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fabric"));
        command
            .current_dir(&self.project)
            .env("FABRIC_HOME", &self.home)
            .env("CODEX_HOME", self.home.with_file_name("codex-home"))
            .env(
                "FABRIC_CLAUDE_BIN",
                self.home.with_file_name("no-such-claude"),
            );
        command
    }

    fn run(&self, args: &[&str]) -> Run {
        self.run_with_stdin(args, "")
    }

    fn ok_json(&self, args: &[&str]) -> Value {
        let run = self.run(args);
        assert_eq!(run.code, 0, "{args:?} failed: {}{}", run.stdout, run.stderr);
        run.json()
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.project.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.project.join(rel)).unwrap()
    }

    fn log(&self) -> Vec<Value> {
        self.ok_json(&["log", "--json", "-n", "1000"])["checkpoints"]
            .as_array()
            .unwrap()
            .clone()
    }
}

#[test]
fn checkpoint_diff_restore_and_undo() {
    let fx = Fixture::new();
    fx.write("src/main.rs", "fn main() { ok() }");
    fx.write("README.md", "# app");

    let init = fx.ok_json(&["init", "--json"]);
    assert_eq!(init["schema"], "fabric.init/v1");
    assert_eq!(init["newly_registered"], true);
    assert_eq!(init["checkpoint"]["seq"], 1);
    assert_eq!(init["checkpoint"]["kind"], "init");

    // An agent makes a mess.
    fx.write("src/main.rs", "fn main() { broken( }");
    fx.write("scratch.py", "print('hmm')");
    let cp = fx.ok_json(&[
        "checkpoint",
        "-m",
        "agent work",
        "--agent",
        "codex",
        "--json",
    ]);
    assert_eq!(cp["checkpoint"]["seq"], 2);
    assert_eq!(cp["checkpoint"]["agent"], "codex");
    assert_eq!(
        cp["checkpoint"]["changes"],
        json!({"added": 1, "modified": 1, "removed": 0, "renamed": 0})
    );

    let diff = fx.ok_json(&["diff", "1", "2", "--json"]);
    assert_eq!(diff["schema"], "fabric.diff/v1");
    assert_eq!(diff["diff"]["added"], json!(["scratch.py"]));
    assert_eq!(diff["diff"]["modified"], json!(["src/main.rs"]));

    // Without --yes nothing happens and the exit code says confirmation is needed.
    let preview = fx.run(&["restore", "1", "--json"]);
    assert_eq!(preview.code, 3);
    let preview = preview.json();
    assert_eq!(preview["status"], "requires_confirmation");
    assert_eq!(preview["plan"]["destructive"], true);
    assert_eq!(fx.read("src/main.rs"), "fn main() { broken( }");

    let restored = fx.ok_json(&["restore", "1", "--yes", "--json"]);
    assert_eq!(restored["status"], "applied");
    assert_eq!(fx.read("src/main.rs"), "fn main() { ok() }");
    assert!(!fx.project.join("scratch.py").exists());

    // The pre-restore state was checkpointed, so the restore can be undone.
    let undo = restored["undo_checkpoint"].as_u64().unwrap().to_string();
    fx.ok_json(&["restore", &undo, "--yes", "--json"]);
    assert_eq!(fx.read("src/main.rs"), "fn main() { broken( }");
    assert_eq!(fx.read("scratch.py"), "print('hmm')");

    let kinds: Vec<_> = fx
        .log()
        .iter()
        .rev()
        .map(|c| c["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds, ["init", "manual", "restore", "restore"]);
}

#[test]
fn checkpoint_if_changed_skips_identical_states() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.ok_json(&["init", "--json"]);
    let cp = fx.ok_json(&["checkpoint", "--if-changed", "--json"]);
    assert_eq!(cp["recorded"], false);
    assert_eq!(fx.log().len(), 1);
}

#[test]
fn conflicts_block_restore_with_exit_code_4() {
    let fx = Fixture::new();
    fx.write("notes.txt", "tracked");
    fx.ok_json(&["init", "--json"]);
    fx.write(".gitignore", "notes.txt\n");
    fx.write("notes.txt", "precious untracked work");
    fx.ok_json(&["checkpoint", "--json"]);

    let run = fx.run(&["restore", "1", "--yes", "--json"]);
    assert_eq!(run.code, 4);
    assert_eq!(run.json()["status"], "conflicts");
    assert_eq!(fx.read("notes.txt"), "precious untracked work");
}

#[test]
fn errors_are_reported_with_kinds_and_exit_codes() {
    let fx = Fixture::new();
    let run = fx.run(&["log", "--json"]);
    assert_eq!(run.code, 1);
    assert_eq!(run.json()["error"]["kind"], "not_a_project");

    fx.ok_json(&["init", "--json"]);
    let run = fx.run(&["restore", "99", "--json"]);
    assert_eq!(run.code, 1);
    assert_eq!(run.json()["error"]["kind"], "unknown_checkpoint");

    let human = fx.run(&["restore", "99"]);
    assert!(human.stderr.starts_with("error: "), "{}", human.stderr);
}

#[test]
fn works_from_subdirectories_of_the_project() {
    let fx = Fixture::new();
    fx.write("deep/nested/file.txt", "x");
    fx.ok_json(&["init", "--json"]);
    let output = fx
        .command()
        .args(["log", "--json"])
        .current_dir(fx.project.join("deep/nested"))
        .output()
        .unwrap();
    assert!(output.status.success());
}

fn hook_event(fx: &Fixture, event: Value) -> Run {
    let run = fx.run_with_stdin(&["hook", "claude-code"], &event.to_string());
    assert_eq!(run.code, 0, "hooks must never fail: {}", run.stderr);
    assert_eq!(run.stdout, "", "hooks must print nothing");
    run
}

#[test]
fn claude_code_hooks_record_prompts_and_changing_steps() {
    let fx = Fixture::new();
    fx.write("src/main.rs", "fn main() {}");
    fx.ok_json(&["init", "--json"]);
    let root = fx.project.to_string_lossy().into_owned();

    hook_event(
        &fx,
        json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "s-1",
            "cwd": root,
            "prompt": "Refactor main.rs\nand add tests",
        }),
    );

    // A tool call that changed nothing is not recorded.
    let edit = |file: &str| {
        json!({
            "hook_event_name": "PostToolUse",
            "session_id": "s-1",
            "cwd": root,
            "tool_name": "Edit",
            "tool_input": { "file_path": fx.project.join(file).to_string_lossy() },
        })
    };
    hook_event(&fx, edit("src/main.rs"));
    assert_eq!(fx.log().len(), 2);

    fx.write("src/main.rs", "fn main() { refactored() }");
    hook_event(&fx, edit("src/main.rs"));

    let log = fx.log();
    assert_eq!(log.len(), 3);
    let (step, prompt) = (&log[0], &log[1]);
    assert_eq!(prompt["kind"], "prompt");
    assert_eq!(prompt["label"], "prompt: Refactor main.rs");
    assert_eq!(prompt["agent"], "claude-code");
    assert_eq!(prompt["session"], "s-1");
    assert_eq!(step["kind"], "agent_step");
    assert_eq!(step["tool"], "Edit");
    assert_eq!(step["label"], "Edit src/main.rs");
    assert_eq!(step["changes"]["modified"], 1);

    // Undo everything since the prompt.
    let seq = prompt["seq"].as_u64().unwrap().to_string();
    fx.ok_json(&["restore", &seq, "--yes", "--json"]);
    assert_eq!(fx.read("src/main.rs"), "fn main() {}");
}

#[test]
fn hook_accepts_input_with_a_byte_order_mark() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    let event = json!({
        "hook_event_name": "UserPromptSubmit",
        "cwd": fx.project.to_string_lossy(),
        "prompt": "piped from PowerShell",
    });
    let run = fx.run_with_stdin(&["hook", "claude-code"], &format!("\u{feff}{event}"));
    assert_eq!(run.code, 0);
    assert_eq!(fx.log()[0]["label"], "prompt: piped from PowerShell");
}

#[test]
fn hook_failures_are_logged_never_surfaced() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    let run = fx.run_with_stdin(&["hook", "claude-code"], "not json");
    assert_eq!(run.code, 0);
    assert_eq!(run.stdout, "");
    let log = fs::read_to_string(fx.home.join("hook-errors.log")).unwrap();
    assert!(log.contains("bad_hook_input"), "{log}");
}

#[test]
fn hooks_in_unrecorded_projects_do_nothing() {
    let fx = Fixture::new();
    hook_event(
        &fx,
        json!({
            "hook_event_name": "UserPromptSubmit",
            "cwd": fx.project.to_string_lossy(),
            "prompt": "hi",
        }),
    );
    assert!(!fx.home.join("hook-errors.log").exists());
}

#[test]
fn integrate_installs_hooks_without_clobbering_settings() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    fx.write(
        ".claude/settings.local.json",
        r#"{"permissions":{"allow":["Bash(npm test)"]}}"#,
    );

    let first = fx.ok_json(&["integrate", "claude-code", "--json"]);
    assert_eq!(first["changed"], true);
    let second = fx.ok_json(&["integrate", "claude-code", "--json"]);
    assert_eq!(second["changed"], false);

    let settings: Value = serde_json::from_str(&fx.read(".claude/settings.local.json")).unwrap();
    assert_eq!(settings["permissions"]["allow"][0], "Bash(npm test)");
    let post = &settings["hooks"]["PostToolUse"][0];
    assert_eq!(post["matcher"], "Write|Edit|MultiEdit|NotebookEdit|Bash");
    assert!(
        post["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .ends_with("hook claude-code")
    );
    assert!(settings["hooks"]["UserPromptSubmit"][0]["matcher"].is_null());
}

/// A test command that passes while `status.txt` says "green", in each platform's shell.
fn status_test() -> &'static str {
    if cfg!(windows) {
        r#"findstr /C:"green" status.txt"#
    } else {
        "grep -q green status.txt"
    }
}

/// Five agent steps, the third of which breaks the tests.
fn agent_session_with_a_breaking_step(fx: &Fixture) {
    fx.write("status.txt", "green");
    fx.write("app/a.txt", "a1");
    fx.ok_json(&["init", "--json"]);
    let steps = [
        ("app/b.txt", "b1", "add b"),
        ("app/c.txt", "c1", "add c"),
        ("status.txt", "red", "refactor status"),
        ("app/d.txt", "d1", "add d"),
        ("app/a.txt", "a2", "edit a"),
    ];
    for (path, content, label) in steps {
        fx.write(path, content);
        fx.ok_json(&[
            "checkpoint",
            "-m",
            label,
            "--agent",
            "claude-code",
            "--json",
        ]);
    }
}

#[test]
fn bisect_finds_the_breaking_step_and_revert_undoes_only_it() {
    let fx = Fixture::new();
    agent_session_with_a_breaking_step(&fx);

    let found = fx.ok_json(&["bisect", "--test", status_test(), "--json"]);
    assert_eq!(found["schema"], "fabric.bisect/v1");
    assert_eq!(found["first_bad"]["seq"], 4);
    assert_eq!(found["first_bad"]["label"], "refactor status");
    assert_eq!(found["last_good"], 3);
    assert_eq!(found["diff"]["modified"], json!(["status.txt"]));

    // Bisecting left the project exactly as it was.
    assert_eq!(fx.read("status.txt"), "red");
    assert_eq!(fx.read("app/a.txt"), "a2");
    assert_eq!(fx.read("app/d.txt"), "d1");

    let preview = fx.run(&["revert", "4", "--json"]);
    assert_eq!(preview.code, 3, "{}", preview.stdout);
    assert_eq!(fx.read("status.txt"), "red");

    let reverted = fx.ok_json(&["revert", "4", "--yes", "--json"]);
    assert_eq!(reverted["schema"], "fabric.revert/v1");
    assert_eq!(reverted["status"], "applied");
    assert_eq!(reverted["reverted"], json!(["status.txt"]));

    // Only the breaking step is gone; work from later steps is kept.
    assert_eq!(fx.read("status.txt"), "green");
    assert_eq!(fx.read("app/a.txt"), "a2");
    assert_eq!(fx.read("app/d.txt"), "d1");
    assert_eq!(fx.log()[0]["kind"], "revert");

    // And the revert itself can be undone.
    let undo = reverted["undo_checkpoint"].as_u64().unwrap().to_string();
    fx.ok_json(&["restore", &undo, "--yes", "--json"]);
    assert_eq!(fx.read("status.txt"), "red");
}

#[test]
fn bisect_refuses_when_there_is_nothing_to_find_and_leaves_the_project_alone() {
    let fx = Fixture::new();
    fx.write("status.txt", "green");
    fx.ok_json(&["init", "--json"]);
    fx.write("other.txt", "x");
    fx.ok_json(&["checkpoint", "--json"]);

    let run = fx.run(&["bisect", "--test", status_test(), "--json"]);
    assert_eq!(run.code, 1);
    assert_eq!(run.json()["error"]["kind"], "bisect_precondition");
    assert_eq!(fx.read("other.txt"), "x");
}

#[test]
fn revert_refuses_when_a_later_step_changed_the_same_file() {
    let fx = Fixture::new();
    fx.write("x.txt", "1");
    fx.ok_json(&["init", "--json"]);
    fx.write("x.txt", "2");
    fx.ok_json(&["checkpoint", "--json"]);
    fx.write("x.txt", "3");
    fx.ok_json(&["checkpoint", "--json"]);

    let run = fx.run(&["revert", "2", "--yes", "--json"]);
    assert_eq!(run.code, 4);
    let out = run.json();
    assert_eq!(out["status"], "conflicts");
    assert_eq!(out["conflicts"][0]["path"], "x.txt");
    assert_eq!(out["conflicts"][0]["reason"], "changed_later");
    assert_eq!(fx.read("x.txt"), "3");
}

/// Speaks MCP to `fabric mcp`: sends every request, closes stdin, returns responses by id.
fn mcp_session(fx: &Fixture, requests: &[Value]) -> std::collections::HashMap<u64, Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let run = fx.run_with_stdin(&["mcp"], &input);
    assert_eq!(run.code, 0, "{}", run.stderr);
    run.stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .map(|response| (response["id"].as_u64().unwrap(), response))
        .collect()
}

fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

fn tool_text(response: &Value) -> &str {
    assert_ne!(response["result"]["isError"], true, "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

#[test]
fn mcp_server_lets_another_agent_see_what_claude_did() {
    let fx = Fixture::new();
    fx.write("src/auth.py", "def login():\n    return check(user)\n");
    fx.ok_json(&["init", "--json"]);
    fx.write(
        "src/auth.py",
        "def login():\n    return check(user, token)\n",
    );
    fx.ok_json(&[
        "checkpoint",
        "-m",
        "Edit src/auth.py",
        "--agent",
        "claude-code",
        "--json",
    ]);
    // Codex then fixes something, and records it through MCP.
    fx.write(
        "src/auth.py",
        "def login():\n    return check(user, token, mfa)\n",
    );

    let responses = mcp_session(
        &fx,
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "codex-test", "version": "1" } } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            tool_call(3, "fabric_file_history", json!({ "path": "src/auth.py" })),
            tool_call(4, "fabric_step", json!({ "checkpoint": 2 })),
            tool_call(5, "fabric_timeline", json!({ "limit": 5 })),
            tool_call(
                6,
                "fabric_checkpoint",
                json!({ "message": "add MFA to login" }),
            ),
            tool_call(7, "fabric_step", json!({})),
            json!({ "jsonrpc": "2.0", "id": 8, "method": "no/such/method" }),
        ],
    );

    let init = &responses[&1]["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "fabric");
    assert!(
        init["instructions"]
            .as_str()
            .unwrap()
            .contains("fabric_file_history")
    );

    let tools: Vec<_> = responses[&2]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        tools,
        [
            "fabric_timeline",
            "fabric_file_history",
            "fabric_step",
            "fabric_test_results",
            "fabric_checkpoint"
        ]
    );
    // No tool can restore, revert or delete: destructive actions stay with people.
    assert!(
        !tools
            .iter()
            .any(|t| t.contains("restore") || t.contains("revert"))
    );

    let history = tool_text(&responses[&3]);
    assert!(history.contains("History of src/auth.py"), "{history}");
    assert!(
        history.contains("#2") && history.contains("claude-code"),
        "{history}"
    );
    assert!(history.contains("modified"), "{history}");

    let step = tool_text(&responses[&4]);
    assert!(step.contains("-    return check(user)"), "{step}");
    assert!(step.contains("+    return check(user, token)"), "{step}");

    assert!(tool_text(&responses[&5]).contains("Edit src/auth.py"));
    assert!(tool_text(&responses[&6]).contains("Recorded checkpoint #3"));
    assert_eq!(responses[&7]["result"]["isError"], true);
    assert_eq!(responses[&8]["error"]["code"], -32601);

    let latest = &fx.log()[0];
    assert_eq!(latest["label"], "add MFA to login");
    assert_eq!(latest["agent"], "codex-test");
    assert_eq!(latest["kind"], "agent_step");
}

#[test]
fn show_and_history_explain_steps_to_people() {
    let fx = Fixture::new();
    fx.write("app.py", "x = 1\n");
    fx.ok_json(&["init", "--json"]);
    fx.write("app.py", "x = 2\n");
    fx.ok_json(&["checkpoint", "-m", "bump x", "--agent", "codex", "--json"]);

    let show = fx.run(&["show", "2"]);
    assert_eq!(show.code, 0, "{}", show.stderr);
    assert!(
        show.stdout.contains("-x = 1") && show.stdout.contains("+x = 2"),
        "{}",
        show.stdout
    );

    let history = fx.run(&["history", "app.py"]);
    assert_eq!(history.code, 0, "{}", history.stderr);
    assert!(history.stdout.contains("codex"), "{}", history.stdout);
    assert!(history.stdout.contains("created"), "{}", history.stdout);
}

#[test]
fn integrate_codex_adds_the_mcp_server_and_keeps_existing_config() {
    let fx = Fixture::new();
    let codex_home = fx.home.with_file_name("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"other-server\"\n",
    )
    .unwrap();

    let first = fx.ok_json(&["integrate", "codex", "--json"]);
    assert_eq!(first["changed"], true);
    assert_eq!(
        fx.ok_json(&["integrate", "codex", "--json"])["changed"],
        false
    );

    let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(
        config.starts_with("# my settings\nmodel = \"gpt-5\""),
        "{config}"
    );
    assert!(config.contains("[mcp_servers.other]"), "{config}");
    assert!(config.contains("[mcp_servers.fabric]"), "{config}");
    assert!(config.contains("args = [\"mcp\"]"), "{config}");
}

#[test]
fn integrate_claude_code_explains_how_to_add_the_mcp_server_when_claude_is_missing() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    let out = fx.ok_json(&["integrate", "claude-code", "--json"]);
    assert_eq!(out["mcp_server"]["status"], "manual");
    assert!(
        out["mcp_server"]["command"]
            .as_str()
            .unwrap()
            .starts_with("claude mcp add --scope user fabric -- ")
    );
}

#[test]
fn integrate_refuses_to_overwrite_invalid_settings() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    fx.write(".claude/settings.local.json", "{ not json");
    let run = fx.run(&["integrate", "claude-code", "--json"]);
    assert_eq!(run.code, 1);
    assert_eq!(fx.read(".claude/settings.local.json"), "{ not json");
}
