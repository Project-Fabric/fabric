//! Claude Code integration.
//!
//! `fabric integrate claude-code` installs two hooks in the project's
//! `.claude/settings.local.json` (per-user, not committed):
//!
//! - `UserPromptSubmit`: checkpoint the project before Claude acts on each prompt, labelled
//!   with the prompt, so "undo everything since I asked X" is always possible.
//! - `PostToolUse` on file-changing tools: checkpoint after each step that changed the project.
//!
//! It also registers `fabric mcp` with Claude Code, so Claude can query what other agents did.
//!
//! Hooks run `fabric hook claude-code`, which reads Claude Code's event JSON on stdin. It
//! must never disrupt the agent: it prints nothing (stdout from a prompt hook would be added
//! to Claude's context), always exits 0, and records failures in `<home>/hook-errors.log`.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fabric_snapshot::capture;
use serde_json::{Value, json};

use crate::checkpoint::{CheckpointKind, Meta, record};
use crate::error::{CliError, exit};
use crate::home::{Home, display_path, project_relative, write_durably};
use crate::timefmt;

const AGENT: &str = "claude-code";
const HOOK_ARGS: &str = "hook claude-code";
/// Tools that can change files. Read-only tools never need a checkpoint.
const CHANGING_TOOLS: &str = "Write|Edit|MultiEdit|NotebookEdit|Bash";
const LABEL_CHARS: usize = 100;

/// Entry point for the hook. Never fails from Claude Code's point of view.
pub fn hook(home: &Home) {
    let mut input = String::new();
    let result = io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))
        .and_then(|_| handle_event(home, &input));
    if let Err(e) = result {
        let line = format!("{} {}: {}\n", timefmt::now_rfc3339(), e.kind, e.message);
        let _ = fs::create_dir_all(&home.root);
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.root.join("hook-errors.log"))
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }
}

fn handle_event(home: &Home, input: &str) -> Result<(), CliError> {
    // Windows tools (PowerShell among them) often prefix piped text with a byte-order mark.
    let event: Value = serde_json::from_str(input.trim_start_matches('\u{feff}'))
        .map_err(|e| CliError::new(exit::ERROR, "bad_hook_input", e.to_string()))?;
    let cwd = match event["cwd"].as_str() {
        Some(cwd) => PathBuf::from(cwd),
        None => env::current_dir().map_err(|e| CliError::io(Path::new("."), e))?,
    };
    // Hooks may be installed in projects that are not (or no longer) recorded.
    let Ok(project) = home.find_project(&cwd) else {
        return Ok(());
    };

    let tool = event["tool_name"].as_str().map(str::to_owned);
    let (mut meta, if_changed) = match event["hook_event_name"].as_str() {
        Some("UserPromptSubmit") => {
            let prompt = event["prompt"].as_str().unwrap_or_default();
            (
                Meta::new(
                    CheckpointKind::Prompt,
                    format!("prompt: {}", summarize(prompt)),
                ),
                false,
            )
        }
        Some("PostToolUse") => {
            let input = &event["tool_input"];
            let target = ["file_path", "notebook_path", "path"]
                .iter()
                .find_map(|key| input[*key].as_str())
                .map(|path| project_relative(path, &project.root))
                .or_else(|| input["command"].as_str().map(str::to_owned))
                .unwrap_or_default();
            let name = tool.as_deref().unwrap_or("tool");
            (
                Meta::new(
                    CheckpointKind::AgentStep,
                    format!("{name} {}", summarize(&target)),
                ),
                true,
            )
        }
        _ => return Ok(()),
    };
    meta.agent = Some(AGENT.to_owned());
    meta.session = event["session_id"].as_str().map(str::to_owned);
    meta.tool = tool;

    let store = home.open_store()?;
    let snapshot = capture(&store, &project.root)?;
    record(&store, &project, &snapshot, meta, if_changed)?;
    Ok(())
}

/// The first line, whitespace collapsed, cut to a readable length.
fn summarize(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > LABEL_CHARS {
        let cut: String = collapsed.chars().take(LABEL_CHARS - 1).collect();
        format!("{cut}…")
    } else {
        collapsed
    }
}

pub fn integrate(home: &Home, json_output: bool) -> Result<u8, CliError> {
    let cwd = env::current_dir().map_err(|e| CliError::io(Path::new("."), e))?;
    let project = home.find_project(&cwd)?;
    let dir = project.root.join(".claude");
    let path = dir.join("settings.local.json");

    let mut settings: Value = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            CliError::new(
                exit::ERROR,
                "invalid_settings",
                format!(
                    "{} is not valid JSON ({e}); fix it and try again — it was not changed",
                    display_path(&path)
                ),
            )
        })?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(CliError::io(&path, e)),
    };
    if !settings.is_object() {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_settings",
            format!(
                "{} is not a JSON object; it was not changed",
                display_path(&path)
            ),
        ));
    }

    let exe = fabric_exe()?;
    let command = format!("\"{exe}\" {HOOK_ARGS}");
    let mut changed = ensure_hook(&mut settings, "UserPromptSubmit", None, &command)?;
    changed |= ensure_hook(&mut settings, "PostToolUse", Some(CHANGING_TOOLS), &command)?;

    if changed {
        fs::create_dir_all(&dir).map_err(|e| CliError::io(&dir, e))?;
        let mut bytes = serde_json::to_vec_pretty(&settings).expect("settings always serialize");
        bytes.push(b'\n');
        write_durably(&path, &bytes)?;
    }
    let mcp = register_mcp_server(&exe);

    let shown = display_path(&path);
    if json_output {
        let value = json!({
            "schema": "fabric.integrate/v1",
            "agent": AGENT,
            "settings": shown,
            "changed": changed,
            "command": command,
            "mcp_server": mcp.status(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("output always serializes")
        );
        return Ok(exit::OK);
    }
    if changed {
        println!(
            "Claude Code will now checkpoint before each prompt and after each change.\nHooks added to {shown}"
        );
    } else {
        println!("Claude Code hooks are already installed in {shown}");
    }
    match &mcp {
        McpRegistration::Added => {
            println!("Claude Code can now ask Fabric what other agents did (MCP server added).");
        }
        McpRegistration::AlreadyPresent => println!("Fabric's MCP server is already registered."),
        McpRegistration::Manual { command, reason } => {
            println!("To let Claude Code query Fabric, run this ({reason}):\n  {command}")
        }
    }
    if changed || matches!(mcp, McpRegistration::Added) {
        println!("Restart Claude Code to pick up the changes.");
    }
    Ok(exit::OK)
}

/// The running `fabric` executable, with forward slashes, which work in every shell an agent
/// may launch it from on Windows.
pub fn fabric_exe() -> Result<String, CliError> {
    let exe = env::current_exe().map_err(|e| CliError::io(Path::new("fabric"), e))?;
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    Ok(display_path(&exe).replace('\\', "/"))
}

enum McpRegistration {
    Added,
    AlreadyPresent,
    /// Could not register automatically; the user can run `command` themselves.
    Manual {
        command: String,
        reason: String,
    },
}

impl McpRegistration {
    fn status(&self) -> Value {
        match self {
            Self::Added => json!({ "status": "added" }),
            Self::AlreadyPresent => json!({ "status": "already_present" }),
            Self::Manual { command, reason } => {
                json!({ "status": "manual", "command": command, "reason": reason })
            }
        }
    }
}

/// Registers `fabric mcp` for all of the user's projects through Claude Code's own CLI
/// (`claude mcp add --scope user`), rather than editing Claude Code's config files directly.
/// The server finds the project from its working directory, so one registration serves
/// every recorded project.
fn register_mcp_server(exe: &str) -> McpRegistration {
    let args = ["mcp", "add", "--scope", "user", "fabric", "--", exe, "mcp"];
    let manual = |reason: &str| McpRegistration::Manual {
        command: format!("claude mcp add --scope user fabric -- \"{exe}\" mcp"),
        reason: reason.to_owned(),
    };
    let Some(claude) = find_claude() else {
        return manual("the `claude` command was not found on PATH");
    };
    let quiet = |command: &mut Command| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    if quiet(Command::new(&claude).args(["mcp", "get", "fabric"])).is_ok_and(|s| s.success()) {
        return McpRegistration::AlreadyPresent;
    }
    match quiet(Command::new(&claude).args(args)) {
        Ok(status) if status.success() => McpRegistration::Added,
        Ok(status) => manual(&format!("`claude mcp add` exited with {status}")),
        Err(e) => manual(&format!("`claude` could not be run: {e}")),
    }
}

/// Finds Claude Code's CLI. `FABRIC_CLAUDE_BIN` overrides the search (used by tests, which
/// must never touch the real user configuration).
fn find_claude() -> Option<PathBuf> {
    if let Some(path) = env::var_os("FABRIC_CLAUDE_BIN") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    env::split_paths(&env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Adds Fabric's hook for `event`, or updates its command if Fabric is already there.
/// Leaves every other hook untouched. Returns whether anything changed.
fn ensure_hook(
    settings: &mut Value,
    event: &str,
    matcher: Option<&str>,
    command: &str,
) -> Result<bool, CliError> {
    let invalid = || {
        CliError::new(
            exit::ERROR,
            "invalid_settings",
            format!(
                "unexpected shape for hooks.{event} in Claude Code settings; it was not changed"
            ),
        )
    };
    let hooks = settings
        .as_object_mut()
        .ok_or_else(invalid)?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let groups = hooks
        .as_object_mut()
        .ok_or_else(invalid)?
        .entry(event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(invalid)?;

    for group in groups.iter_mut() {
        let Some(entries) = group["hooks"].as_array_mut() else {
            continue;
        };
        for entry in entries {
            let is_fabric = entry["command"]
                .as_str()
                .is_some_and(|c| c.ends_with(HOOK_ARGS));
            if is_fabric {
                if entry["command"] == command {
                    return Ok(false);
                }
                entry["command"] = command.into();
                return Ok(true);
            }
        }
    }
    let mut group = json!({ "hooks": [{ "type": "command", "command": command }] });
    if let Some(matcher) = matcher {
        group["matcher"] = matcher.into();
    }
    groups.push(group);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_keeps_labels_short_and_single_line() {
        assert_eq!(summarize("  fix   the\tbug \nsecond line"), "fix the bug");
        let long = "x".repeat(300);
        assert_eq!(summarize(&long).chars().count(), LABEL_CHARS);
        assert_eq!(
            summarize("ü".repeat(200).as_str()).chars().count(),
            LABEL_CHARS
        );
    }

    #[test]
    fn ensure_hook_preserves_other_hooks_and_is_idempotent() {
        let mut settings = json!({
            "permissions": { "allow": ["Bash(ls)"] },
            "hooks": { "PostToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "lint.sh" }] }
            ]}
        });
        assert!(
            ensure_hook(
                &mut settings,
                "PostToolUse",
                Some("Edit"),
                "\"/x/fabric\" hook claude-code"
            )
            .unwrap()
        );
        assert!(
            !ensure_hook(
                &mut settings,
                "PostToolUse",
                Some("Edit"),
                "\"/x/fabric\" hook claude-code"
            )
            .unwrap()
        );
        assert!(
            ensure_hook(
                &mut settings,
                "PostToolUse",
                Some("Edit"),
                "\"/y/fabric\" hook claude-code"
            )
            .unwrap()
        );

        let groups = settings["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["hooks"][0]["command"], "lint.sh");
        assert_eq!(
            groups[1]["hooks"][0]["command"],
            "\"/y/fabric\" hook claude-code"
        );
        assert_eq!(settings["permissions"]["allow"][0], "Bash(ls)");
    }
}
