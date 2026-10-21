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
        // Tests may themselves run inside an agent; approvals must behave as for a person.
        for marker in [
            "CLAUDECODE",
            "CODEX_SANDBOX",
            "CODEX_SANDBOX_NETWORK_DISABLED",
        ] {
            command.env_remove(marker);
        }
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
    fx.write("status.txt", "service status: green");
    fx.write("app/a.txt", "a1");
    fx.ok_json(&["init", "--json"]);
    let steps = [
        ("app/b.txt", "b1", "add b"),
        ("app/c.txt", "c1", "add c"),
        ("status.txt", "service status: red", "refactor status"),
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
    assert_eq!(fx.read("status.txt"), "service status: red");
    assert_eq!(fx.read("app/a.txt"), "a2");
    assert_eq!(fx.read("app/d.txt"), "d1");

    let preview = fx.run(&["revert", "4", "--json"]);
    assert_eq!(preview.code, 3, "{}", preview.stdout);
    assert_eq!(fx.read("status.txt"), "service status: red");

    let reverted = fx.ok_json(&["revert", "4", "--yes", "--json"]);
    assert_eq!(reverted["schema"], "fabric.revert/v1");
    assert_eq!(reverted["status"], "applied");
    assert_eq!(reverted["reverted"], json!(["status.txt"]));

    // Only the breaking step is gone; work from later steps is kept.
    assert_eq!(fx.read("status.txt"), "service status: green");
    assert_eq!(fx.read("app/a.txt"), "a2");
    assert_eq!(fx.read("app/d.txt"), "d1");
    assert_eq!(fx.log()[0]["kind"], "revert");

    // And the revert itself can be undone.
    let undo = reverted["undo_checkpoint"].as_u64().unwrap().to_string();
    fx.ok_json(&["restore", &undo, "--yes", "--json"]);
    assert_eq!(fx.read("status.txt"), "service status: red");
}

#[test]
fn bisect_refuses_when_there_is_nothing_to_find_and_leaves_the_project_alone() {
    let fx = Fixture::new();
    fx.write("status.txt", "service status: green");
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
            "fabric_lessons",
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

/// What Claude Code sends before running a tool, and the guard's answer.
fn pre_tool_use(fx: &Fixture, tool: &str, input: Value) -> Option<Value> {
    let event = json!({
        "hook_event_name": "PreToolUse",
        "session_id": "s-1",
        "cwd": fx.project.to_string_lossy(),
        "tool_name": tool,
        "tool_input": input,
    });
    let run = fx.run_with_stdin(&["hook", "claude-code"], &event.to_string());
    assert_eq!(run.code, 0, "hooks must never fail: {}", run.stderr);
    (!run.stdout.trim().is_empty()).then(|| run.json())
}

fn decision(answer: &Option<Value>) -> &str {
    answer.as_ref().map_or("silent", |a| {
        a["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap()
    })
}

#[test]
fn guard_asks_before_destructive_actions_and_blocks_tampering() {
    let fx = Fixture::new();
    fx.write("src/app.py", "x = 1");
    fx.ok_json(&["init", "--json"]);
    let bash = |command: &str| pre_tool_use(&fx, "Bash", json!({ "command": command }));
    let write = |path: PathBuf| {
        pre_tool_use(
            &fx,
            "Write",
            json!({ "file_path": path.to_string_lossy(), "content": "x" }),
        )
    };

    // Everyday work is not interrupted: the guard stays silent.
    assert_eq!(decision(&bash("pytest -x")), "silent");
    assert_eq!(decision(&write(fx.project.join("src/app.py"))), "silent");
    assert_eq!(
        decision(&write(fx.project.join("src/payments/new_module.py"))),
        "silent",
        "a new file in a new directory is inside the project"
    );

    // Destructive actions wait for the person's approval, with a reason.
    let answer = bash("rm -rf src");
    assert_eq!(decision(&answer), "ask");
    let output = &answer.as_ref().unwrap()["hookSpecificOutput"];
    assert_eq!(output["hookEventName"], "PreToolUse");
    assert!(
        output["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("recursive-delete")
    );
    assert_eq!(decision(&bash("git push --force origin main")), "ask");
    assert_eq!(decision(&write(fx.project.join(".env"))), "ask");
    assert_eq!(
        decision(&write(fx.home.with_file_name("outside.txt"))),
        "ask"
    );

    // Catastrophic commands and attempts to undo Fabric itself are blocked outright.
    assert_eq!(decision(&bash("rm -rf /")), "deny");
    assert_eq!(decision(&bash("fabric restore 1 --yes")), "deny");
    assert_eq!(decision(&write(fx.project.join(".git/config"))), "deny");
    assert_eq!(decision(&write(fx.home.join("store/FABRIC_STORE"))), "deny");

    // Every ask and deny is in the audit log, newest first.
    let log = fx.ok_json(&["guard", "log", "--json"]);
    assert_eq!(log["total"], 8);
    assert_eq!(log["entries"][0]["decision"], "deny");
    assert_eq!(log["entries"][0]["rule"], "fabric-data");
    assert_eq!(log["entries"][7]["target"], "rm -rf src");
}

#[test]
fn guard_rules_can_be_changed_per_project_but_not_by_the_agent() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    let push = || pre_tool_use(&fx, "Bash", json!({ "command": "git push origin main" }));
    assert_eq!(decision(&push()), "ask");

    // The person relaxes a rule from their own terminal...
    let set = fx.ok_json(&["guard", "set", "git-push", "allow", "--json"]);
    assert_eq!(set["decision"], "allow");
    assert_eq!(decision(&push()), "silent");
    let rules = fx.ok_json(&["guard", "rules", "--json"]);
    let rule = rules["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == "git-push")
        .unwrap();
    assert_eq!(
        (&rule["decision"], &rule["default"]),
        (&json!("allow"), &json!("ask"))
    );

    // ...but an agent trying the same is blocked, and the setting lives outside the project.
    let tamper = pre_tool_use(
        &fx,
        "Bash",
        json!({ "command": "fabric guard set secrets allow" }),
    );
    assert_eq!(decision(&tamper), "deny");
    assert!(!fx.project.join("guard-policy.json").exists());

    let check = fx.ok_json(&["guard", "check", "rm -rf build", "--json"]);
    assert_eq!(check["verdict"]["decision"], "ask");
    assert_eq!(check["verdict"]["rule"], "recursive-delete");
    let bad = fx.run(&["guard", "set", "git-push", "sometimes", "--json"]);
    assert_eq!(bad.code, 1);
}

#[test]
fn integrate_installs_the_guard_hook() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["integrate", "claude-code", "--json"]);
    let settings: Value = serde_json::from_str(&fx.read(".claude/settings.local.json")).unwrap();
    let pre = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(pre["matcher"], "Write|Edit|MultiEdit|NotebookEdit|Bash");
    assert!(
        pre["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .ends_with("hook claude-code")
    );
}

/// Sends a hook event with experiment flags set, as Claude Code would run the hook.
fn hook_with(fx: &Fixture, flags: &[(&str, &str)], event: Value) -> Run {
    let mut command = fx.command();
    command
        .args(["hook", "claude-code"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in flags {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

#[test]
fn lessons_are_learned_from_the_breaking_step_and_reach_the_next_agent() {
    let fx = Fixture::new();
    agent_session_with_a_breaking_step(&fx);
    let found = fx.ok_json(&["bisect", "--test", status_test(), "--json"]);
    let bad = found["first_bad"]["seq"].as_u64().unwrap().to_string();

    let added = fx.ok_json(&[
        "lesson",
        "add",
        "--step",
        &bad,
        "--rule",
        "status.txt must say green; never write red",
        "--json",
    ]);
    let lesson = &added["lesson"];
    assert_eq!(lesson["from_checkpoint"], 4);
    assert_eq!(lesson["files"], json!(["status.txt"]));
    assert_eq!(
        fx.ok_json(&["lesson", "list", "--json"])["lessons"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let root = fx.project.to_string_lossy().into_owned();
    let prompt = json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "s-2",
        "cwd": root,
        "prompt": "update the status",
    });
    // Off by default: the hook stays silent.
    assert_eq!(hook_with(&fx, &[], prompt.clone()).stdout, "");
    // With the experiment on, the next agent is told the lesson with its evidence.
    let run = hook_with(&fx, &[("FABRIC_EXPERIMENT_LESSONS", "1")], prompt);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let context: Value = serde_json::from_str(&run.stdout).unwrap();
    let text = context["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert_eq!(
        context["hookSpecificOutput"]["hookEventName"],
        "UserPromptSubmit"
    );
    assert!(text.contains("never write red"), "{text}");
    assert!(text.contains("status.txt"), "{text}");
}

#[test]
fn the_learned_guard_stops_a_repeated_mistake_once() {
    let fx = Fixture::new();
    agent_session_with_a_breaking_step(&fx);
    fx.ok_json(&[
        "lesson",
        "add",
        "--step",
        "4",
        "--rule",
        "status.txt must stay green",
        "--json",
    ]);
    let flags = [("FABRIC_EXPERIMENT_LEARNED_GUARD", "1")];
    let edit = |session: &str, content: &str| {
        json!({
            "hook_event_name": "PreToolUse",
            "session_id": session,
            "cwd": fx.project.to_string_lossy(),
            "tool_name": "Write",
            "tool_input": { "file_path": fx.project.join("status.txt").to_string_lossy(), "content": content },
        })
    };

    // Writing "red" again repeats the reverted change: stopped, with the lesson as the reason.
    let first = hook_with(&fx, &flags, edit("s-3", "service status: red"));
    let answer: Value = serde_json::from_str(&first.stdout).unwrap();
    assert_eq!(answer["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = answer["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("status.txt must stay green"), "{reason}");
    assert!(reason.contains("#4"), "{reason}");

    // A retry in the same session is allowed: the agent has been told, the person decides.
    assert_eq!(
        hook_with(&fx, &flags, edit("s-3", "service status: red")).stdout,
        ""
    );
    // An unrelated edit is never stopped, and nothing fires with the experiment off.
    assert_eq!(
        hook_with(
            &fx,
            &flags,
            edit("s-4", "service status: green and healthy")
        )
        .stdout,
        ""
    );
    assert_eq!(
        hook_with(&fx, &[], edit("s-5", "service status: red")).stdout,
        ""
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

fn reason(answer: &Option<Value>) -> String {
    answer.as_ref().map_or(String::new(), |a| {
        a["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .to_owned()
    })
}

#[test]
fn finance_changes_need_a_recorded_approval_and_leave_verifiable_evidence() {
    let fx = Fixture::new();
    fx.write("ledger/journal.py", "def post(): pass\n");
    fx.write("tools/report.py", "print('hi')\n");
    fx.ok_json(&["init", "--json"]);
    let pack = fx.ok_json(&["guard", "pack", "finance", "--scope", "ledger/**", "--json"]);
    assert_eq!(pack["rules"][0], "finance-in-scope-code");
    let root = fx.project.to_string_lossy().into_owned();
    let journal = fx
        .project
        .join("ledger/journal.py")
        .to_string_lossy()
        .into_owned();
    let edit = |new: &str| {
        pre_tool_use(
            &fx,
            "Edit",
            json!({ "file_path": journal, "old_string": "pass", "new_string": new }),
        )
    };

    // Out-of-scope work is not interrupted.
    let tools = fx.project.join("tools/report.py");
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "Write",
            json!({ "file_path": tools.to_string_lossy(), "content": "x" })
        )),
        "silent"
    );

    // An in-scope change is held until a person approves it; retrying does not pile up requests.
    let held = edit("return 1");
    assert_eq!(decision(&held), "deny");
    assert!(
        reason(&held).contains("fabric approve 1"),
        "{}",
        reason(&held)
    );
    assert_eq!(decision(&edit("return 1")), "deny");
    let waiting = fx.ok_json(&["approvals", "--json"]);
    assert_eq!(waiting["requests"].as_array().unwrap().len(), 1);
    assert_eq!(waiting["requests"][0]["target"], "ledger/journal.py");

    // The agent cannot approve its own change: the guard blocks the command, and the command
    // itself refuses to run inside an agent session.
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "Bash",
            json!({ "command": "fabric approve 1 --yes" })
        )),
        "deny"
    );
    let output = fx
        .command()
        .args(["approve", "1", "--yes", "--json"])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("approval_from_agent"));
    // Without a terminal, approving needs --yes after reviewing.
    assert_eq!(fx.run(&["approve", "1"]).code, 3);

    let approved = fx.ok_json(&[
        "approve",
        "1",
        "--yes",
        "--as",
        "controller@example.com",
        "--note",
        "CHG-1042",
        "--json",
    ]);
    assert_eq!(approved["schema"], "fabric.approve/v2");
    assert_eq!(approved["requests"][0]["status"], "approved");
    assert_eq!(
        approved["requests"][0]["approver"],
        "controller@example.com"
    );

    // The approved action proceeds once; a different change needs its own approval.
    assert_eq!(decision(&edit("return 1")), "silent");
    fx.write("ledger/journal.py", "def post(): return 1\n");
    hook_event(
        &fx,
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s-1", "cwd": root,
            "tool_name": "Edit", "tool_input": { "file_path": journal },
        }),
    );
    assert_eq!(decision(&edit("return 1")), "deny");
    assert!(reason(&edit("return 2")).contains("fabric approve 3"));
    let rejected = fx.ok_json(&[
        "approve",
        "3",
        "--reject",
        "--yes",
        "--as",
        "c@example.com",
        "--json",
    ]);
    assert_eq!(rejected["requests"][0]["status"], "rejected");
    assert!(reason(&edit("return 2")).contains("rejected"));

    // Controlled files changed by a shell command, with no approval, are rolled back at once:
    // a new file is removed and an existing one gets its previous content back. The agent is
    // told why, and the rolled-back content stays recoverable in the step's checkpoint.
    fx.write("ledger/fees.py", "FEE = 3\n");
    fx.write("ledger/journal.py", "def post(): return 99\n");
    fx.write("tools/report.py", "print('changed by the same command')\n");
    let bash = json!({
        "hook_event_name": "PostToolUse", "session_id": "s-1", "cwd": root,
        "tool_name": "Bash", "tool_input": { "command": "python gen.py" },
    });
    let run = fx.run_with_stdin(&["hook", "claude-code"], &bash.to_string());
    assert_eq!(run.code, 0, "hooks must never fail: {}", run.stderr);
    let answer = run.json();
    assert_eq!(answer["decision"], "block");
    let why = answer["reason"].as_str().unwrap();
    assert!(
        why.contains("ledger/fees.py") && why.contains("ledger/journal.py"),
        "{why}"
    );
    assert!(!fx.project.join("ledger/fees.py").exists());
    assert_eq!(fx.read("ledger/journal.py"), "def post(): return 1\n");
    assert_eq!(
        fx.read("tools/report.py"),
        "print('changed by the same command')\n",
        "uncontrolled files are left alone"
    );
    let log = fx.log();
    assert_eq!(log[0]["kind"], "revert");
    assert_eq!(log[0]["agent"], "fabric");
    assert_eq!(log[1]["label"], "Bash python gen.py");

    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["chain"]["ok"], true);
    assert_eq!(
        verify["exceptions"],
        json!([]),
        "rolled-back changes are resolved"
    );
    let rollback_seq = log[0]["seq"].as_u64().unwrap();
    let rollback = fx.ok_json(&["evidence", "report", &rollback_seq.to_string(), "--json"]);
    assert!(
        rollback["markdown"]
            .as_str()
            .unwrap()
            .contains("rolled back** unapproved changes")
    );

    // The approved step's report shows the approval and no exception.
    let step = fx
        .log()
        .iter()
        .find(|c| c["label"] == "Edit ledger/journal.py")
        .unwrap()["seq"]
        .as_u64()
        .unwrap();
    let report = fx.ok_json(&["evidence", "report", &step.to_string(), "--json"]);
    let markdown = report["markdown"].as_str().unwrap();
    assert!(
        markdown.contains("approved** by controller@example.com"),
        "{markdown}"
    );
    assert!(markdown.contains("No exceptions"), "{markdown}");

    // A signed pack verifies anywhere, and any change to it is detected.
    let out = fx.home.with_file_name("pack");
    let exported = fx.ok_json(&[
        "evidence",
        "export",
        "--out",
        &out.to_string_lossy(),
        "--json",
    ]);
    let fingerprint = exported["fingerprint"].as_str().unwrap().to_owned();
    let checked = fx.ok_json(&[
        "evidence",
        "verify-pack",
        &out.to_string_lossy(),
        "--key",
        &fingerprint,
        "--json",
    ]);
    assert_eq!(checked["ok"], true, "{checked}");
    let wrong_key = fx.run(&[
        "evidence",
        "verify-pack",
        &out.to_string_lossy(),
        "--key",
        "0000",
    ]);
    assert_eq!(wrong_key.code, 5);
    let report_file = fs::read_dir(out.join("changes"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&report_file, "nothing to see here").unwrap();
    let tampered = fx.run(&["evidence", "verify-pack", &out.to_string_lossy(), "--json"]);
    assert_eq!(tampered.code, 5);
    assert!(
        tampered
            .stdout
            .contains("was changed after the pack was signed")
    );

    // Editing the project's own chain is detected too.
    let chain = fs::read_dir(fx.home.join("projects"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("evidence.jsonl");
    let text = fs::read_to_string(&chain).unwrap();
    fs::write(
        &chain,
        text.replacen("controller@example.com", "someone@else.com", 1),
    )
    .unwrap();
    let broken = fx.run(&["evidence", "verify", "--json"]);
    assert_eq!(broken.code, 5);
    assert_eq!(broken.json()["chain"]["ok"], false);
}

#[test]
fn committed_migrations_are_protected_but_new_ones_are_not() {
    let fx = Fixture::new();
    fx.write("migrations/0001_init.sql", "create table t (id int);\n");
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&fx.project)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-qm",
        "init",
    ]);
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance", "--json"]);
    let write = |name: &str| {
        pre_tool_use(
            &fx,
            "Write",
            json!({ "file_path": fx.project.join(name).to_string_lossy(), "content": "x" }),
        )
    };
    assert_eq!(decision(&write("migrations/0001_init.sql")), "deny");
    assert_eq!(decision(&write("migrations/0002_fees.sql")), "silent");

    // The same holds after the fact: a shell command that adds a migration keeps it, one that
    // edits an applied migration is rolled back.
    fx.write("migrations/0002_fees.sql", "create table fees (id int);\n");
    fx.write("migrations/0001_init.sql", "create table t (id bigint);\n");
    let event = json!({
        "hook_event_name": "PostToolUse", "session_id": "s-1",
        "cwd": fx.project.to_string_lossy(),
        "tool_name": "Bash", "tool_input": { "command": "python make_migrations.py" },
    });
    let run = fx.run_with_stdin(&["hook", "claude-code"], &event.to_string());
    assert_eq!(run.code, 0, "hooks must never fail: {}", run.stderr);
    let why = run.json()["reason"].as_str().unwrap().to_owned();
    assert!(
        why.contains("migrations/0001_init.sql") && !why.contains("0002"),
        "{why}"
    );
    assert_eq!(
        fx.read("migrations/0001_init.sql"),
        "create table t (id int);\n"
    );
    assert_eq!(
        fx.read("migrations/0002_fees.sql"),
        "create table fees (id int);\n"
    );
}

#[test]
fn several_held_changes_are_approved_together_and_recorded_separately() {
    let fx = Fixture::new();
    fx.write(
        "ledger/a.py",
        "A = 1
",
    );
    fx.write(
        "ledger/b.py",
        "B = 1
",
    );
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance", "--scope", "ledger/**", "--json"]);
    let write = |name: &str| {
        pre_tool_use(
            &fx,
            "Write",
            json!({ "file_path": fx.project.join(name).to_string_lossy(), "content": "X = 2
" }),
        )
    };
    assert_eq!(decision(&write("ledger/a.py")), "deny");
    assert_eq!(decision(&write("ledger/b.py")), "deny");

    // A closed request in the batch means nothing is recorded.
    fx.ok_json(&[
        "approve",
        "2",
        "--reject",
        "--yes",
        "--as",
        "c@example.com",
        "--json",
    ]);
    let refused = fx.run(&[
        "approve",
        "1",
        "2",
        "--reject",
        "--yes",
        "--as",
        "c@example.com",
    ]);
    assert_eq!(refused.code, 1);
    assert_eq!(fx.ok_json(&["approvals", "--json"])["requests"][0]["id"], 1);

    let both = fx.ok_json(&[
        "approve",
        "1",
        "2",
        "--yes",
        "--as",
        "c@example.com",
        "--json",
    ]);
    let statuses: Vec<&Value> = both["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["status"])
        .collect();
    assert_eq!(statuses, [&json!("approved"), &json!("approved")]);
    assert_eq!(decision(&write("ledger/a.py")), "silent");
    assert_eq!(decision(&write("ledger/b.py")), "silent");
    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["chain"]["ok"], true);
}

#[test]
fn a_change_is_authorized_once_and_its_result_reviewed_once() {
    let fx = Fixture::new();
    fx.write("ledger/fees.py", "FEE = 0\n");
    fx.write("ledger/journal.py", "def post(): pass\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance", "--scope", "ledger/**", "--json"]);
    let root = fx.project.to_string_lossy().into_owned();
    let path = |name: &str| fx.project.join(name).to_string_lossy().into_owned();
    let write = |name: &str, content: &str| {
        pre_tool_use(
            &fx,
            "Write",
            json!({ "file_path": path(name), "content": content }),
        )
    };
    let after = |tool: &str, input: Value| {
        hook_event(
            &fx,
            json!({
                "hook_event_name": "PostToolUse", "session_id": "s-1", "cwd": root,
                "tool_name": tool, "tool_input": input,
            }),
        )
    };

    // The first held edit is approved together with the whole task.
    assert_eq!(decision(&write("ledger/fees.py", "FEE = 5\n")), "deny");
    assert_eq!(
        fx.run(&["approve", "1", "--path", "ledger/**", "--yes"])
            .code,
        2,
        "--path needs --change"
    );
    let approved = fx.ok_json(&[
        "approve",
        "1",
        "--change",
        "Late fees",
        "--path",
        "ledger/**",
        "--note",
        "CHG-12",
        "--yes",
        "--as",
        "controller@example.com",
        "--json",
    ]);
    assert_eq!(approved["schema"], "fabric.approve/v2");
    assert_eq!(approved["change"]["id"], 1);
    assert_eq!(approved["change"]["status"], "open");
    assert_eq!(approved["change"]["paths"], json!(["ledger/**"]));

    // The approved edit uses its own approval; further edits proceed under the change,
    // including a file the task creates and a change made through the shell.
    assert_eq!(decision(&write("ledger/fees.py", "FEE = 5\n")), "silent");
    fx.write("ledger/fees.py", "FEE = 5\n");
    after("Write", json!({ "file_path": path("ledger/fees.py") }));
    assert_eq!(decision(&write("ledger/late.py", "LATE = 1\n")), "silent");
    fx.write("ledger/late.py", "LATE = 1\n");
    after("Write", json!({ "file_path": path("ledger/late.py") }));
    fx.write("ledger/journal.py", "def post(): return 1\n");
    after("Bash", json!({ "command": "python gen.py" }));
    assert_eq!(fx.read("ledger/journal.py"), "def post(): return 1\n");
    let requests = fx.ok_json(&["approvals", "--all", "--json"]);
    assert_eq!(requests["requests"].as_array().unwrap().len(), 1);
    assert_eq!(requests["requests"][0]["status"], "used");

    // The change covers file edits of its own session only, and never shell commands that a
    // rule holds.
    let other_session = fx.run_with_stdin(
        &["hook", "claude-code"],
        &json!({
            "hook_event_name": "PreToolUse", "session_id": "s-2", "cwd": root,
            "tool_name": "Write",
            "tool_input": { "file_path": path("ledger/fees.py"), "content": "FEE = 9\n" },
        })
        .to_string(),
    );
    assert_eq!(decision(&Some(other_session.json())), "deny");
    let sql = pre_tool_use(
        &fx,
        "Bash",
        json!({ "command": "sqlite3 books.db \"delete from journal_lines\"" }),
    );
    assert_eq!(decision(&sql), "deny");
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "Bash",
            json!({ "command": "fabric review 1 --yes" })
        )),
        "deny",
        "agents cannot review their own change"
    );

    // Review shows the whole result and records the decision bound to it.
    let unconfirmed = fx.run(&["review", "1"]);
    assert_eq!(unconfirmed.code, 3);
    assert!(
        unconfirmed.stderr.contains("+FEE = 5"),
        "{}",
        unconfirmed.stderr
    );
    assert!(
        unconfirmed.stderr.contains("+LATE = 1"),
        "{}",
        unconfirmed.stderr
    );
    let reviewed = fx.ok_json(&[
        "review",
        "1",
        "--yes",
        "--as",
        "reviewer@example.com",
        "--json",
    ]);
    assert_eq!(reviewed["schema"], "fabric.review/v1");
    assert_eq!(reviewed["change"]["status"], "accepted");
    assert_eq!(reviewed["change"]["reviewed_by"], "reviewer@example.com");
    let files: Vec<&String> = reviewed["files"].as_object().unwrap().keys().collect();
    assert_eq!(
        files,
        ["ledger/fees.py", "ledger/journal.py", "ledger/late.py"]
    );
    assert_eq!(fx.run(&["review", "1", "--yes"]).code, 1, "reviewed once");

    // Review closes the change: the next edit needs a new approval.
    assert_eq!(decision(&write("ledger/fees.py", "FEE = 7\n")), "deny");

    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["chain"]["ok"], true);
    assert_eq!(verify["exceptions"], json!([]));
    let log = fx.log();
    let late = log
        .iter()
        .find(|c| c["label"] == "Write ledger/late.py")
        .unwrap();
    let report = fx.ok_json(&[
        "evidence",
        "report",
        &late["seq"].as_u64().unwrap().to_string(),
        "--json",
    ]);
    let markdown = report["markdown"].as_str().unwrap();
    assert!(markdown.contains("made under change #1"), "{markdown}");
    assert_eq!(log[0]["label"], "accepted change #1: Late fees");
    assert_eq!(
        fx.ok_json(&["changes", "--all", "--json"])["changes"][0]["status"],
        "accepted"
    );

    // One page per change, as an auditor samples change tickets, and in the signed pack.
    let ticket = fx.ok_json(&["evidence", "report", "--change", "1", "--json"]);
    assert_eq!(ticket["schema"], "fabric.evidence.change-report/v1");
    assert_eq!(ticket["exceptions"], json!([]));
    let page = ticket["markdown"].as_str().unwrap();
    for expected in [
        "# Change #1: Late fees",
        "**accepted**",
        "by controller@example.com",
        "by reviewer@example.com",
        "CHG-12",
        "Write ledger/late.py",
        "Bash python gen.py",
        "content recorded at review matches",
        "+FEE = 5",
        "+def post(): return 1",
    ] {
        assert!(page.contains(expected), "missing {expected:?}:\n{page}");
    }
    assert!(
        !page.contains("FEE = 9"),
        "other sessions are not part of it"
    );
    assert_eq!(
        fx.run(&["evidence", "report", "--change", "7"]).code,
        1,
        "unknown change"
    );
    let pack = fx.home.join("pack");
    let exported = fx.ok_json(&[
        "evidence",
        "export",
        "--out",
        &pack.to_string_lossy(),
        "--json",
    ]);
    let fingerprint = exported["fingerprint"].as_str().unwrap().to_owned();
    assert!(pack.join("tickets/change-0001.md").exists());
    let checked = fx.ok_json(&[
        "evidence",
        "verify-pack",
        &pack.to_string_lossy(),
        "--key",
        &fingerprint,
        "--json",
    ]);
    assert_eq!(checked["ok"], true);

    // The whole change can be undone at once: every file its session touched goes back to
    // how it was when the change was authorized, including the shell-made change.
    fx.write("notes.txt", "later work by a person\n");
    let reverted = fx.ok_json(&["revert", "--change", "1", "--yes", "--json"]);
    assert_eq!(reverted["status"], "applied");
    assert_eq!(reverted["change"], 1);
    assert_eq!(fx.read("ledger/fees.py"), "FEE = 0\n");
    assert_eq!(fx.read("ledger/journal.py"), "def post(): pass\n");
    assert!(!fx.project.join("ledger/late.py").exists());
    assert_eq!(fx.read("notes.txt"), "later work by a person\n");
    let page = fx.ok_json(&["evidence", "report", "--change", "1", "--json"])["markdown"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(page.contains("| Reverted |"), "{page}");
    assert_eq!(
        fx.run(&["revert", "--change", "1", "--yes"]).code,
        0,
        "reverting again changes nothing"
    );
}

#[test]
fn reverting_a_change_refuses_to_overwrite_later_edits_to_its_files() {
    let fx = Fixture::new();
    fx.write("ledger/fees.py", "FEE = 0\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance", "--scope", "ledger/**", "--json"]);
    let path = fx
        .project
        .join("ledger/fees.py")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "Write",
            json!({ "file_path": path, "content": "FEE = 5\n" })
        )),
        "deny"
    );
    fx.ok_json(&[
        "approve",
        "1",
        "--change",
        "Fees",
        "--yes",
        "--as",
        "c@example.com",
        "--json",
    ]);
    fx.write("ledger/fees.py", "FEE = 5\n");
    hook_event(
        &fx,
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s-1",
            "cwd": fx.project.to_string_lossy(),
            "tool_name": "Write", "tool_input": { "file_path": path },
        }),
    );
    fx.ok_json(&["review", "1", "--yes", "--as", "r@example.com", "--json"]);
    // A person edits the file after the review; reverting the change must not discard that.
    fx.write("ledger/fees.py", "FEE = 6\n");
    let refused = fx.run(&["revert", "--change", "1", "--yes", "--json"]);
    assert_eq!(refused.code, 4);
    assert_eq!(refused.json()["status"], "conflicts");
    assert_eq!(fx.read("ledger/fees.py"), "FEE = 6\n");
}

#[test]
fn a_change_starts_at_the_prompt_that_began_the_task() {
    let fx = Fixture::new();
    fx.write("ledger/tax.py", "MODE = 'half-even'\n");
    fx.write("tests/test_tax.py", "assert MODE == 'half-even'\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance", "--scope", "ledger/**", "--json"]);
    let root = fx.project.to_string_lossy().into_owned();
    let after = |tool: &str, file: &str| {
        hook_event(
            &fx,
            json!({
                "hook_event_name": "PostToolUse", "session_id": "s-1", "cwd": root,
                "tool_name": tool,
                "tool_input": { "file_path": fx.project.join(file).to_string_lossy() },
            }),
        )
    };
    hook_event(
        &fx,
        json!({
            "hook_event_name": "UserPromptSubmit", "session_id": "s-1", "cwd": root,
            "prompt": "Round tax half up",
        }),
    );
    // The agent changes a test first (not controlled), then is held on the tax code.
    fx.write("tests/test_tax.py", "assert MODE == 'half-up'\n");
    after("Edit", "tests/test_tax.py");
    let tax = fx
        .project
        .join("ledger/tax.py")
        .to_string_lossy()
        .into_owned();
    let write = json!({ "file_path": tax, "content": "MODE = 'half-up'\n" });
    assert_eq!(decision(&pre_tool_use(&fx, "Write", write.clone())), "deny");
    fx.ok_json(&[
        "approve",
        "1",
        "--change",
        "Tax",
        "--yes",
        "--as",
        "c@example.com",
        "--json",
    ]);
    assert_eq!(decision(&pre_tool_use(&fx, "Write", write)), "silent");
    fx.write("ledger/tax.py", "MODE = 'half-up'\n");
    after("Write", "ledger/tax.py");

    // Review covers the task from its prompt; reverting the change undoes the test edit too.
    let unconfirmed = fx.run(&["review", "1"]);
    assert!(
        unconfirmed.stderr.contains("Edit tests/test_tax.py"),
        "{}",
        unconfirmed.stderr
    );
    fx.ok_json(&[
        "review",
        "1",
        "--reject",
        "--yes",
        "--as",
        "r@example.com",
        "--json",
    ]);
    fx.ok_json(&["revert", "--change", "1", "--yes", "--json"]);
    assert_eq!(fx.read("ledger/tax.py"), "MODE = 'half-even'\n");
    assert_eq!(fx.read("tests/test_tax.py"), "assert MODE == 'half-even'\n");
}

/// `fabric` as a different person: their own Fabric installation, outside the project.
fn other_installation(home: &std::path::Path, cwd: &std::path::Path, args: &[&str]) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fabric"));
    command
        .args(args)
        .current_dir(cwd)
        .env("FABRIC_HOME", home)
        .stdin(Stdio::null());
    for marker in [
        "CLAUDECODE",
        "CODEX_SANDBOX",
        "CODEX_SANDBOX_NETWORK_DISABLED",
    ] {
        command.env_remove(marker);
    }
    let output = command.output().unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

#[test]
fn approvals_and_reviews_can_be_signed_by_an_approver_elsewhere() {
    let fx = Fixture::new();
    fx.write("ledger/fees.py", "FEE = 0\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance", "--scope", "ledger/**", "--json"]);
    let elsewhere = fx.home.with_file_name("approver-desk");
    fs::create_dir_all(&elsewhere).unwrap();
    let approver_home = fx.home.with_file_name("approver-home");
    let approver = |args: &[&str]| {
        let run = other_installation(&approver_home, &elsewhere, args);
        assert_eq!(run.code, 0, "{args:?}: {}{}", run.stdout, run.stderr);
        run.json()
    };
    let file = |name: &str| elsewhere.join(name).to_string_lossy().into_owned();
    let path = fx
        .project
        .join("ledger/fees.py")
        .to_string_lossy()
        .into_owned();
    let write = json!({ "file_path": path, "content": "FEE = 5\n" });
    assert_eq!(decision(&pre_tool_use(&fx, "Write", write.clone())), "deny");

    // The project registers the approver's public key and requires signed approvals.
    let key = approver(&["approver", "key", "--json"]);
    let public_key = key["public_key"].as_str().unwrap();
    fx.ok_json(&[
        "approver",
        "add",
        "Dana Controller",
        "--key",
        public_key,
        "--json",
    ]);
    assert_eq!(fx.run(&["approver", "require", "on", "--json"]).code, 0);
    let unsigned = fx.run(&["approve", "1", "--yes", "--as", "dev@example.com"]);
    assert_eq!(unsigned.code, 1);
    assert!(
        unsigned.stderr.contains("signed by a registered approver"),
        "{}",
        unsigned.stderr
    );

    // The developer exports the request; the approver signs it without the project.
    fx.ok_json(&[
        "approvals",
        "export",
        "1",
        "--out",
        &file("req.json"),
        "--json",
    ]);
    let mut tampered: Value =
        serde_json::from_str(&fs::read_to_string(file("req.json")).unwrap()).unwrap();
    tampered["requests"][0]["input"]["content"] = json!("FEE = 500\n");
    fs::write(file("tampered.json"), tampered.to_string()).unwrap();
    let refused = other_installation(
        &approver_home,
        &elsewhere,
        &["approver", "sign", &file("tampered.json"), "--yes"],
    );
    assert_eq!(
        refused.code, 1,
        "an edited request is refused: {}",
        refused.stderr
    );
    approver(&[
        "approver",
        "sign",
        &file("req.json"),
        "--change",
        "Fees",
        "--path",
        "ledger/**",
        "--note",
        "CHG-7",
        "--yes",
        "--out",
        &file("req.signed.json"),
        "--json",
    ]);

    // A decision changed after signing is refused; the genuine one is applied.
    let mut forged: Value =
        serde_json::from_str(&fs::read_to_string(file("req.signed.json")).unwrap()).unwrap();
    forged["payload"]["change"]["paths"] = json!(["**"]);
    fs::write(file("forged.json"), forged.to_string()).unwrap();
    assert_eq!(
        fx.run(&["approve", "--signed", &file("forged.json"), "--json"])
            .code,
        5
    );
    assert_eq!(
        fx.run(&["approve", "1", "--signed", &file("req.signed.json")])
            .code,
        1,
        "the signed file decides which requests"
    );
    let applied = fx.ok_json(&["approve", "--signed", &file("req.signed.json"), "--json"]);
    assert_eq!(applied["requests"][0]["approver"], "Dana Controller");
    assert_eq!(applied["requests"][0]["note"], "CHG-7");
    assert_eq!(applied["change"]["paths"], json!(["ledger/**"]));
    assert_eq!(applied["change"]["authorized_by"], "Dana Controller");

    assert_eq!(decision(&pre_tool_use(&fx, "Write", write)), "silent");
    fx.write("ledger/fees.py", "FEE = 5\n");
    hook_event(
        &fx,
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s-1",
            "cwd": fx.project.to_string_lossy(),
            "tool_name": "Write", "tool_input": { "file_path": path },
        }),
    );

    // The review goes the same way. A review signed with this installation's own key is
    // refused while signed approvals are required, as is one about content that changed.
    assert_eq!(
        fx.run(&["review", "1", "--yes", "--as", "dev@example.com"])
            .code,
        1
    );
    fx.ok_json(&["review", "1", "--export", &file("review.json"), "--json"]);
    let own = fx.ok_json(&["approver", "key", "--json"]);
    fx.ok_json(&[
        "approver",
        "add",
        "Developer",
        "--key",
        own["public_key"].as_str().unwrap(),
        "--json",
    ]);
    fx.ok_json(&[
        "approver",
        "sign",
        &file("review.json"),
        "--yes",
        "--out",
        &file("own.json"),
        "--json",
    ]);
    let same = fx.run(&["review", "1", "--signed", &file("own.json")]);
    assert_eq!(same.code, 1);
    assert!(
        same.stderr.contains("another installation"),
        "{}",
        same.stderr
    );
    approver(&[
        "approver",
        "sign",
        &file("review.json"),
        "--note",
        "looks right",
        "--yes",
        "--out",
        &file("review.signed.json"),
        "--json",
    ]);
    fx.write("ledger/fees.py", "FEE = 6\n");
    assert_eq!(
        fx.run(&["review", "1", "--signed", &file("review.signed.json")])
            .code,
        6
    );
    fx.write("ledger/fees.py", "FEE = 5\n");
    let reviewed = fx.ok_json(&[
        "review",
        "1",
        "--signed",
        &file("review.signed.json"),
        "--json",
    ]);
    assert_eq!(reviewed["change"]["status"], "accepted");
    assert_eq!(reviewed["change"]["reviewed_by"], "Dana Controller");
    assert_eq!(reviewed["change"]["review_note"], "looks right");

    // Every signed decision is re-checked from the evidence alone, here and in a pack.
    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["signed_decisions"]["checked"], 3);
    assert_eq!(verify["signed_decisions"]["problems"], json!([]));
    let pack = fx.home.with_file_name("pack");
    let exported = fx.ok_json(&[
        "evidence",
        "export",
        "--out",
        &pack.to_string_lossy(),
        "--json",
    ]);
    let checked = fx.ok_json(&[
        "evidence",
        "verify-pack",
        &pack.to_string_lossy(),
        "--key",
        exported["fingerprint"].as_str().unwrap(),
        "--json",
    ]);
    assert_eq!(checked["ok"], true);
    assert_eq!(checked["signed_decisions"], 3);

    // The change's report says the decisions were signed, and by which key.
    let page = fx.ok_json(&["evidence", "report", "--change", "1", "--json"])["markdown"]
        .as_str()
        .unwrap()
        .to_owned();
    let fingerprint = key["fingerprint"].as_str().unwrap();
    for row in ["| Authorized |", "| Reviewed |"] {
        let line = page.lines().find(|l| l.starts_with(row)).unwrap();
        assert!(line.contains("Dana Controller"), "{line}");
        assert!(
            line.contains(&format!("signed with approver key {fingerprint}")),
            "{line}"
        );
    }
}

#[test]
fn the_mcp_proxy_holds_and_records_tool_calls_for_any_agent() {
    let fx = Fixture::new();
    fx.write("src/app.py", "x = 1\n");
    fx.ok_json(&["init", "--json"]);
    // Fabric's own MCP server stands in for an ERP: one tool is held, one blocked.
    fx.ok_json(&[
        "guard",
        "add-rule",
        "erp-writes",
        "--decision",
        "approve",
        "--tool",
        "erp/fabric_checkpoint",
        "--json",
    ]);
    fx.ok_json(&[
        "guard",
        "add-rule",
        "erp-deletes",
        "--decision",
        "deny",
        "--tool",
        "erp/fabric_lessons",
        "--json",
    ]);
    let exe = env!("CARGO_BIN_EXE_fabric");
    let session = |messages: &[Value]| -> Vec<Value> {
        let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
        let run = fx.run_with_stdin(&["mcp-proxy", "--name", "erp", "--", exe, "mcp"], &input);
        assert_eq!(run.code, 0, "{}", run.stderr);
        run.stdout
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    };
    let call = |id: u64, tool: &str, args: Value| {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": tool, "arguments": args } })
    };
    let by_id = |responses: &[Value], id: u64| -> Value {
        responses.iter().find(|r| r["id"] == id).cloned().unwrap()
    };
    let checkpoint = call(4, "fabric_checkpoint", json!({ "message": "month end" }));
    let responses = session(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                            "clientInfo": { "name": "finance-agent", "version": "1" } } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        call(3, "fabric_timeline", json!({})),
        checkpoint.clone(),
        call(5, "fabric_lessons", json!({})),
    ]);

    // Tools under a rule are marked before the agent calls them.
    let tools = by_id(&responses, 2)["result"]["tools"].clone();
    let description = |name: &str| {
        tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["description"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(description("fabric_checkpoint").contains("needs a recorded approval"));
    assert!(description("fabric_lessons").contains("blocked"));
    assert!(!description("fabric_timeline").contains("[Fabric"));

    // Allowed calls go through; held and blocked ones never reach the server.
    assert_ne!(by_id(&responses, 3)["result"]["isError"], true);
    let held = by_id(&responses, 4);
    assert_eq!(held["result"]["isError"], true);
    assert!(
        held["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("fabric approve 1")
    );
    let blocked = by_id(&responses, 5);
    assert!(
        blocked["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Blocked")
    );
    assert_eq!(fx.log().len(), 1, "the held checkpoint was not taken");

    // After a person approves, the same call goes through once.
    fx.ok_json(&[
        "approve",
        "1",
        "--yes",
        "--as",
        "controller@example.com",
        "--json",
    ]);
    fx.write("src/app.py", "x = 2\n");
    let responses = session(std::slice::from_ref(&checkpoint));
    assert_ne!(by_id(&responses, 4)["result"]["isError"], true);
    assert_eq!(fx.log()[0]["label"], "month end");
    let again = session(&[checkpoint]);
    assert_eq!(
        by_id(&again, 4)["result"]["isError"],
        true,
        "approvals are used once"
    );

    // Every call is in the evidence: who called what, the decision and the outcome.
    let calls = fx.ok_json(&["evidence", "calls", "--json"])["calls"].clone();
    let summary: Vec<(String, String)> = calls
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|c| {
            (
                c["tool"].as_str().unwrap().to_owned(),
                c["outcome"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected = [
        ("erp/fabric_timeline", "ok"),
        ("erp/fabric_checkpoint", "held"),
        ("erp/fabric_lessons", "blocked"),
        ("erp/fabric_checkpoint", "ok"),
        ("erp/fabric_checkpoint", "held"),
    ];
    assert_eq!(
        summary,
        expected.map(|(t, o)| (t.to_owned(), o.to_owned())).to_vec()
    );
    assert_eq!(
        calls[1]["agent"], "mcp-client",
        "agent named per proxy session"
    );
    assert_eq!(calls[4]["agent"], "finance-agent");
    assert_eq!(calls[1]["arguments"], "{\"message\":\"month end\"}");
    assert_eq!(
        fx.ok_json(&["evidence", "verify", "--json"])["chain"]["ok"],
        true
    );
}

#[test]
fn claude_code_mcp_calls_to_the_books_need_approval() {
    let fx = Fixture::new();
    fx.write("README.md", "erp\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance-ops", "--json"]);
    assert_eq!(
        fx.run(&["guard", "pack", "finance-ops", "--scope", "x/**"])
            .code,
        1,
        "the finance-ops pack has no file scope"
    );
    let post = json!({ "date": "2026-10-31", "memo": "accrual", "lines": [] });
    let answer = pre_tool_use(&fx, "mcp__ledgerly__post_journal_entry", post.clone());
    assert_eq!(decision(&answer), "deny");
    assert!(
        reason(&answer).contains("fabric approve 1"),
        "{}",
        reason(&answer)
    );
    assert!(
        !reason(&answer).contains("--change"),
        "changes cover file edits, not tool calls"
    );
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__ledgerly__delete_journal_entry",
            json!({ "id": 4 })
        )),
        "deny"
    );
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__ledgerly__trial_balance",
            json!({})
        )),
        "silent"
    );
    fx.ok_json(&[
        "approve",
        "1",
        "--yes",
        "--as",
        "controller@example.com",
        "--json",
    ]);
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__ledgerly__post_journal_entry",
            post
        )),
        "silent"
    );
    let rules = fx.ok_json(&["guard", "rules", "--json"]);
    let text = rules.to_string();
    assert!(
        text.contains("finance-ops-postings") && text.contains("tool calls"),
        "{text}"
    );
}

#[test]
fn one_approval_covers_one_call_through_both_the_hook_and_the_proxy() {
    let fx = Fixture::new();
    fx.write("README.md", "erp\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&[
        "guard",
        "add-rule",
        "erp-writes",
        "--decision",
        "approve",
        "--tool",
        "erp/fabric_checkpoint",
        "--json",
    ]);
    let args = json!({ "message": "month end" });
    let exe = env!("CARGO_BIN_EXE_fabric");
    let through_proxy = || {
        let message = json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call",
                              "params": { "name": "fabric_checkpoint", "arguments": args } });
        let run = fx.run_with_stdin(
            &["mcp-proxy", "--name", "erp", "--", exe, "mcp"],
            &format!("{message}\n"),
        );
        let response: Value = serde_json::from_str(run.stdout.lines().next().unwrap()).unwrap();
        response["result"]["isError"] == true
    };
    // Claude Code: the hook holds the call; once approved, the hook lets it through and the
    // proxy behind it accepts the same approval for the same call.
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__erp__fabric_checkpoint",
            args.clone()
        )),
        "deny"
    );
    fx.ok_json(&["approve", "1", "--yes", "--as", "c@example.com", "--json"]);
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__erp__fabric_checkpoint",
            args.clone()
        )),
        "silent"
    );
    assert!(
        !through_proxy(),
        "the proxy accepts the call the hook just let through"
    );
    // A second call is a new action: both layers hold it.
    assert!(through_proxy(), "the approval is not reused by the proxy");
    assert_eq!(
        decision(&pre_tool_use(&fx, "mcp__erp__fabric_checkpoint", args)),
        "deny"
    );
    let requests = fx.ok_json(&["approvals", "--all", "--json"])["requests"].clone();
    let used = requests
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == 1)
        .unwrap()
        .clone();
    assert_eq!(used["used_via"], json!(["hook", "proxy"]));
}

#[test]
fn mandates_let_agents_act_within_limits_and_hold_the_rest() {
    let fx = Fixture::new();
    fx.write("README.md", "erp\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance-ops", "--json"]);
    assert_eq!(
        fx.run(&[
            "mandate",
            "grant",
            "No limits",
            "--tool",
            "*/issue_refund",
            "--expires",
            "2099-01-01"
        ])
        .code,
        1,
        "a mandate needs a limit"
    );
    assert_eq!(
        fx.run(&[
            "mandate",
            "grant",
            "Bad date",
            "--tool",
            "*/issue_refund",
            "--max-per-call",
            "5",
            "--expires",
            "2026-02-30"
        ])
        .code,
        1
    );
    let granted = fx.ok_json(&[
        "mandate",
        "grant",
        "Small refunds",
        "--tool",
        "*/issue_refund",
        "--max-per-call",
        "500",
        "--max-per-day",
        "800",
        "--expires",
        "2099-01-01",
        "--json",
    ]);
    assert_eq!(granted["mandate"]["id"], 1);
    let refund = |amount: &str| {
        decision(&pre_tool_use(
            &fx,
            "mcp__ledgerly__issue_refund",
            json!({ "payment_id": 1, "amount": amount, "date": "2026-03-20", "reason": "x" }),
        ))
        .to_owned()
    };
    assert_eq!(refund("300.00"), "silent", "within the mandate");
    assert_eq!(refund("400.00"), "silent", "700 of 800 today");
    assert_eq!(
        refund("200.00"),
        "deny",
        "would make 900 today: held for a person"
    );
    assert_eq!(refund("600.00"), "deny", "over the per-call limit");
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__ledgerly__post_journal_entry",
            json!({ "lines": [{ "account": "6000", "debit": "1.00" }] })
        )),
        "deny",
        "a tool the mandate does not cover"
    );

    // A mandate for one agent and one call a day, through the proxy. The agent must be
    // registered, and only calls authenticated with its token are covered.
    assert_eq!(
        fx.run(&[
            "mandate",
            "grant",
            "x",
            "--tool",
            "erp/fabric_checkpoint",
            "--agent",
            "finance-agent",
            "--max-calls-per-day",
            "1",
            "--expires",
            "2099-01-01",
        ])
        .code,
        1,
        "an unregistered agent cannot be named"
    );
    let token = fx.ok_json(&["agent", "add", "finance-agent", "--json"])["token"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.ok_json(&[
        "mandate",
        "grant",
        "One checkpoint a day",
        "--tool",
        "erp/fabric_checkpoint",
        "--agent",
        "finance-agent",
        "--max-calls-per-day",
        "1",
        "--expires",
        "2099-01-01",
        "--json",
    ]);
    fx.ok_json(&[
        "guard",
        "add-rule",
        "erp-writes",
        "--decision",
        "approve",
        "--tool",
        "erp/fabric_checkpoint",
        "--json",
    ]);
    let exe = env!("CARGO_BIN_EXE_fabric");
    let via_proxy = |client: &str, token: Option<&str>, message: &str| -> bool {
        let input = format!(
            "{}\n{}\n",
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                                "clientInfo": { "name": client, "version": "1" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": { "name": "fabric_checkpoint", "arguments": { "message": message } } })
        );
        let mut command = fx.command();
        command
            .args(["mcp-proxy", "--name", "erp", "--", exe, "mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(token) = token {
            command.env("FABRIC_AGENT_TOKEN", token);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let response = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .find(|r| r["id"] == 2)
            .unwrap();
        response["result"]["isError"] != true
    };
    assert!(
        !via_proxy("other-agent", None, "a"),
        "another agent is not covered"
    );
    assert!(
        !via_proxy("finance-agent", None, "a2"),
        "claiming to be the agent without its token is not enough"
    );
    assert!(
        via_proxy("anything", Some(&token), "b"),
        "the authenticated agent is covered"
    );
    assert!(!via_proxy("anything", Some(&token), "c"), "one call a day");

    // Revoking stops it at once.
    fx.ok_json(&["mandate", "revoke", "1", "--json"]);
    assert_eq!(refund("10.00"), "deny");
    let list = fx.ok_json(&["mandate", "list", "--json"]);
    assert!(list["mandates"][0]["revoked_at"].is_string());
    assert_eq!(list["mandates"][0]["uses"].as_array().unwrap().len(), 2);

    // Every use is replayed against the mandate's terms from the evidence alone.
    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["mandate_uses"]["checked"], 3);
    assert_eq!(verify["mandate_uses"]["problems"], json!([]));
    let calls = fx.ok_json(&["evidence", "calls", "--json"])["calls"].clone();
    assert!(
        calls
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["outcome"] == "ok")
    );
}

#[test]
fn a_mandate_can_require_an_approvers_signature() {
    let fx = Fixture::new();
    fx.write("README.md", "erp\n");
    fx.ok_json(&["init", "--json"]);
    fx.ok_json(&["guard", "pack", "finance-ops", "--json"]);
    let elsewhere = fx.home.with_file_name("approver-desk");
    fs::create_dir_all(&elsewhere).unwrap();
    let approver_home = fx.home.with_file_name("approver-home");
    let approver = |args: &[&str]| {
        let run = other_installation(&approver_home, &elsewhere, args);
        assert_eq!(run.code, 0, "{args:?}: {}{}", run.stdout, run.stderr);
        run.json()
    };
    let key = approver(&["approver", "key", "--json"]);
    fx.ok_json(&[
        "approver",
        "add",
        "CFO",
        "--key",
        key["public_key"].as_str().unwrap(),
        "--json",
    ]);
    fx.ok_json(&["approver", "require", "on", "--json"]);
    let terms = [
        "mandate",
        "grant",
        "Refunds",
        "--tool",
        "*/issue_refund",
        "--max-per-call",
        "250",
        "--expires",
        "2099-01-01",
    ];
    assert_eq!(fx.run(&terms).code, 1, "an unsigned mandate is refused");
    let proposal = elsewhere
        .join("mandate.json")
        .to_string_lossy()
        .into_owned();
    let mut with_out = terms.to_vec();
    with_out.extend(["--out", &proposal, "--json"]);
    fx.ok_json(&with_out);
    let signed = elsewhere
        .join("mandate.signed.json")
        .to_string_lossy()
        .into_owned();
    approver(&[
        "approver", "sign", &proposal, "--yes", "--out", &signed, "--json",
    ]);
    let granted = fx.ok_json(&["mandate", "grant", "--signed", &signed, "--json"]);
    assert_eq!(granted["mandate"]["granted_by"], "CFO");
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "mcp__ledgerly__issue_refund",
            json!({ "payment_id": 1, "amount": "99.00", "reason": "x" })
        )),
        "silent"
    );
    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["signed_decisions"]["checked"], 1);
    assert_eq!(verify["signed_decisions"]["problems"], json!([]));
}

/// A Python interpreter, for tests that need a tiny stand-in MCP server.
fn python() -> &'static str {
    for candidate in ["python3", "python"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return candidate;
        }
    }
    panic!("these tests need python3 or python on PATH");
}

/// An MCP server whose one tool reports what its environment holds.
const ENV_SERVER: &str = r#"
import json, os, sys
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m: continue
    if m["method"] == "tools/call":
        seen = {"secret": os.environ.get("UPSTREAM_KEY", "missing"),
                "agent_token_visible": "FABRIC_AGENT_TOKEN" in os.environ}
        result = {"content": [{"type": "text", "text": json.dumps(seen)}]}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": result}), flush=True)
"#;

#[test]
fn the_gateway_holds_credentials_and_authenticates_agents() {
    let fx = Fixture::new();
    fx.write("README.md", "erp\n");
    fx.ok_json(&["init", "--json"]);
    let server = fx.home.with_file_name("env_server.py");
    fs::write(&server, ENV_SERVER).unwrap();
    let server = server.to_string_lossy().into_owned();

    // Credentials go in through standard input and never come back out.
    let set = fx.run_with_stdin(
        &["secret", "set", "erp-key", "--json"],
        "sk_test_value_123\n",
    );
    assert_eq!(set.code, 0, "{}", set.stderr);
    let listed = fx.ok_json(&["secret", "list", "--json"]);
    assert_eq!(listed["names"], json!(["erp-key"]));
    assert!(!listed.to_string().contains("sk_test_value_123"));

    let token = fx.ok_json(&[
        "agent",
        "add",
        "billing-bot",
        "--tool",
        "erp/read_*",
        "--json",
    ])["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let call = |tool: &str, token: Option<&str>, secret_env: &[&str]| -> (i32, Value) {
        let mut args = vec!["mcp-proxy", "--name", "erp"];
        for pair in secret_env {
            args.extend(["--secret-env", pair]);
        }
        let py = python();
        args.extend(["--", py, server.as_str()]);
        let mut command = fx.command();
        command
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(token) = token {
            command.env("FABRIC_AGENT_TOKEN", token);
        }
        let mut child = command.spawn().unwrap();
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                              "params": { "name": tool, "arguments": {} } });
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{message}\n").as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let response = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .next()
            .map(|l| serde_json::from_str(l).unwrap())
            .unwrap_or(Value::Null);
        (output.status.code().unwrap(), response)
    };

    // The upstream server gets the credential; it never gets the agent's token.
    let (code, response) = call("read_ledger", Some(&token), &["UPSTREAM_KEY=erp-key"]);
    assert_eq!(code, 0);
    let seen: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    // The server received the credential, and the agent never sees it: a server echoing it
    // back (as error messages do) has it scrubbed from the response and the evidence.
    assert_eq!(seen["secret"], "[redacted credential erp-key]");
    assert_eq!(seen["agent_token_visible"], false);

    // Scopes: outside them, the call is refused before it reaches the server.
    let (_, response) = call("pay_bill", Some(&token), &[]);
    assert_eq!(response["result"]["isError"], true);
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("agent-scope")
    );

    // Fail closed: a missing credential or a bad token stops the gateway from starting.
    assert_ne!(
        call("read_ledger", Some(&token), &["UPSTREAM_KEY=nope"]).0,
        0
    );
    assert_ne!(call("read_ledger", Some("fat_forged"), &[]).0, 0);

    // With tokens required, an unauthenticated call is refused; a revoked token cannot start.
    fx.ok_json(&["agent", "require", "on", "--json"]);
    let (_, response) = call("read_ledger", None, &[]);
    assert_eq!(response["result"]["isError"], true);
    fx.ok_json(&["agent", "revoke", "billing-bot", "--json"]);
    assert_ne!(call("read_ledger", Some(&token), &[]).0, 0);

    // Agents cannot read the credential store, by tool or by shell.
    let secrets = fx.home.join("secrets").join("erp-key");
    let path = secrets.to_string_lossy().into_owned();
    assert_eq!(
        decision(&pre_tool_use(&fx, "Read", json!({ "file_path": path }))),
        "deny"
    );
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "Bash",
            json!({ "command": format!("cat '{path}'") })
        )),
        "deny"
    );
    assert_eq!(
        decision(&pre_tool_use(
            &fx,
            "Bash",
            json!({ "command": "fabric secret list" })
        )),
        "deny"
    );
    let verify = fx.ok_json(&["evidence", "verify", "--json"]);
    assert_eq!(verify["chain"]["ok"], true);
    // The credential is in no file Fabric wrote (evidence, logs, project data) outside the
    // credential store itself.
    fn files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut written = Vec::new();
    files(&fx.home, &mut written);
    let store = fx.home.join("secrets");
    let leaked: Vec<_> = written
        .iter()
        .filter(|p| !p.starts_with(&store))
        .filter(|p| String::from_utf8_lossy(&fs::read(p).unwrap()).contains("sk_test_value_123"))
        .collect();
    assert!(leaked.is_empty(), "credential found in {leaked:?}");
    assert!(written.len() > 3, "the walk found Fabric's files");
}
