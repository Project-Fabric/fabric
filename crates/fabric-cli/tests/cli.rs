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
        let mut child = Command::new(env!("CARGO_BIN_EXE_fabric"))
            .args(args)
            .current_dir(&self.project)
            .env("FABRIC_HOME", &self.home)
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
    let output = Command::new(env!("CARGO_BIN_EXE_fabric"))
        .args(["log", "--json"])
        .current_dir(fx.project.join("deep/nested"))
        .env("FABRIC_HOME", &fx.home)
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

#[test]
fn integrate_refuses_to_overwrite_invalid_settings() {
    let fx = Fixture::new();
    fx.ok_json(&["init", "--json"]);
    fx.write(".claude/settings.local.json", "{ not json");
    let run = fx.run(&["integrate", "claude-code", "--json"]);
    assert_eq!(run.code, 1);
    assert_eq!(fx.read(".claude/settings.local.json"), "{ not json");
}
