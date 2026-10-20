//! Recorded approvals: a person signs off on one exact agent action before it runs.
//!
//! When a rule decides `approve`, the agent's action is held and an approval request is
//! recorded, identified by a number and by a digest of the exact action (tool and input). The
//! agent is told to ask the person, who reviews the proposed change and runs
//! `fabric approve <n>` in their own terminal. When the agent retries the *same* action, the
//! approval is used up and the action proceeds. A different action needs its own approval.
//!
//! Requests, approvals, rejections and uses are all recorded in the evidence chain, with the
//! approver's identity and OS account.
//!
//! Segregation of duties, and its limits: an agent cannot approve its own action through the
//! guarded tools — `fabric approve` run by an agent is denied by the guard, and refused when it
//! runs inside a known agent environment (`AGENT_MARKERS`). Both are defences against an agent
//! acting by mistake, not against one set on evading them, and the person who prompted the
//! agent can approve its work. Evidence records whether the approval came from the same OS
//! account as the agent, so a reviewer can tell. Approval by a second person on a separate
//! machine is not provided yet.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use similar::TextDiff;

use crate::error::{CliError, exit};
use crate::evidence;
use crate::guard::{Verdict, hook_decision};
use crate::home::{Project, display_path, project_relative, write_durably};
use crate::timefmt;

const FILE: &str = "approvals.json";
const LOCK_FILE: &str = "approvals.lock";
const FORMAT: &str = "fabric-approvals";
const VERSION: u32 = 1;
/// An approval not used within this time expires; the action must be approved again.
const VALID_FOR_SECS: u64 = 4 * 60 * 60;
/// Environment variables set by coding agents in the shells they run commands in.
const AGENT_MARKERS: &[&str] = &[
    "CLAUDECODE",
    "CODEX_SANDBOX",
    "CODEX_SANDBOX_NETWORK_DISABLED",
];
const AGENT: &str = "claude-code";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Approved,
    Rejected,
    Used,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub digest: String,
    pub rule: String,
    pub reason: String,
    pub agent: String,
    #[serde(default)]
    pub session: Option<String>,
    pub tool: String,
    pub target: String,
    /// The tool's input exactly as the agent sent it: what is being approved.
    pub input: Value,
    pub requested_at: String,
    #[serde(default)]
    pub requested_os_user: Option<String>,
    pub status: Status,
    #[serde(default)]
    pub approver: Option<String>,
    #[serde(default)]
    pub approver_os_user: Option<String>,
    #[serde(default)]
    pub decided_at: Option<String>,
    #[serde(default)]
    pub decided_unix: Option<u64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalsFile {
    format: String,
    version: u32,
    requests: Vec<Request>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The digest of an exact action. Equal inputs give equal digests: JSON object keys are
/// serialized in sorted order.
pub fn digest(tool: &str, input: &Value) -> String {
    let bytes = serde_json::to_vec(&json!({ "tool": tool, "input": input }))
        .expect("tool input always serializes");
    evidence::hash_hex(&bytes)
}

/// Runs `f` on the project's approvals with an exclusive lock held, saving if it returns
/// `true` alongside its result.
fn with_approvals<T>(
    project: &Project,
    f: impl FnOnce(&mut Vec<Request>) -> Result<(T, bool), CliError>,
) -> Result<T, CliError> {
    let lock_path = project.dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| CliError::io(&lock_path, e))?;
    lock.lock().map_err(|e| CliError::io(&lock_path, e))?;
    let path = project.dir.join(FILE);
    let mut requests = load(&path)?;
    expire(&mut requests);
    let (result, changed) = f(&mut requests)?;
    if changed {
        let file = ApprovalsFile {
            format: FORMAT.to_owned(),
            version: VERSION,
            requests,
        };
        let bytes = serde_json::to_vec_pretty(&file).expect("approvals always serialize");
        write_durably(&path, &bytes)?;
    }
    drop(lock);
    Ok(result)
}

fn load(path: &Path) -> Result<Vec<Request>, CliError> {
    match fs::read(path) {
        Ok(bytes) => {
            let file: ApprovalsFile = serde_json::from_slice(&bytes).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "invalid_approvals",
                    format!("{}: {e}", display_path(path)),
                )
            })?;
            if file.format != FORMAT || file.version != VERSION {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_approvals",
                    format!("{}: unsupported approvals file", display_path(path)),
                ));
            }
            Ok(file.requests)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CliError::io(path, e)),
    }
}

fn expire(requests: &mut [Request]) {
    let now = now_unix();
    for request in requests {
        if request.status == Status::Approved
            && request
                .decided_unix
                .is_some_and(|at| now.saturating_sub(at) > VALID_FOR_SECS)
        {
            request.status = Status::Expired;
        }
    }
}

/// Called by the guard when a rule decides `approve`. Lets the action through if it has an
/// unused approval; otherwise records (or reuses) a request and holds the action.
pub fn gate(
    project: &Project,
    event: &Value,
    verdict: &Verdict,
    target: &str,
) -> Result<Option<Value>, CliError> {
    let tool = event["tool_name"].as_str().unwrap_or_default().to_owned();
    let input = event["tool_input"].clone();
    let digest = digest(&tool, &input);
    let session = event["session_id"].as_str().map(str::to_owned);
    let rule = verdict.rule.clone().unwrap_or_default();

    enum Outcome {
        Proceed(u64),
        Rejected(Box<Request>),
        Held(u64, bool),
    }
    let outcome = with_approvals(project, |requests| {
        if let Some(request) = requests
            .iter_mut()
            .find(|r| r.digest == digest && r.status == Status::Approved)
        {
            request.status = Status::Used;
            return Ok((Outcome::Proceed(request.id), true));
        }
        let latest = requests.iter().rev().find(|r| r.digest == digest);
        match latest {
            Some(r) if r.status == Status::Pending => {
                return Ok((Outcome::Held(r.id, false), false));
            }
            Some(r) if r.status == Status::Rejected => {
                return Ok((Outcome::Rejected(Box::new(r.clone())), false));
            }
            _ => {}
        }
        let id = requests.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        requests.push(Request {
            id,
            digest: digest.clone(),
            rule: rule.clone(),
            reason: verdict.reason.clone(),
            agent: AGENT.to_owned(),
            session: session.clone(),
            tool: tool.clone(),
            target: target.to_owned(),
            input: input.clone(),
            requested_at: timefmt::now_rfc3339(),
            requested_os_user: evidence::os_user(),
            status: Status::Pending,
            approver: None,
            approver_os_user: None,
            decided_at: None,
            decided_unix: None,
            note: None,
        });
        Ok((Outcome::Held(id, true), true))
    })?;

    match outcome {
        Outcome::Proceed(id) => {
            evidence::append(
                project,
                json!({
                    "kind": "approval_used",
                    "request": id,
                    "digest": digest,
                    "session": session,
                    "tool": tool,
                    "target": evidence::clip(target),
                }),
            )?;
            // Silent: the agent's own permission rules still apply.
            Ok(None)
        }
        Outcome::Rejected(request) => Ok(Some(hook_decision(
            "deny",
            &format!(
                "Fabric: {} rejected this exact change (approval request #{}){}. Do not retry it; ask the user how to proceed.",
                request.approver.as_deref().unwrap_or("the reviewer"),
                request.id,
                request
                    .note
                    .as_deref()
                    .map(|n| format!(": \"{n}\""))
                    .unwrap_or_default(),
            ),
        ))),
        Outcome::Held(id, new) => {
            if new {
                evidence::append(
                    project,
                    json!({
                        "kind": "approval_requested",
                        "request": id,
                        "digest": digest,
                        "rule": rule,
                        "agent": AGENT,
                        "session": session,
                        "tool": tool,
                        "target": evidence::clip(target),
                        "os_user": evidence::os_user(),
                    }),
                )?;
            }
            Ok(Some(hook_decision(
                "deny",
                &format!(
                    "Fabric: this change needs a recorded approval ({rule}: {}). Approval request #{id} is waiting. \
                     Tell the user to review and approve it in their own terminal with `fabric approve {id}`, \
                     then retry exactly the same action. Do not try to approve it yourself.",
                    verdict.reason
                ),
            )))
        }
    }
}

// ---- `fabric approvals` and `fabric approve` ----------------------------------------------

fn find_request(requests: &[Request], id: u64) -> Result<&Request, CliError> {
    requests.iter().find(|r| r.id == id).ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_request",
            format!("no approval request #{id}; see `fabric approvals`"),
        )
    })
}

pub fn list_cmd(project: &Project, all: bool, json_output: bool) -> Result<u8, CliError> {
    let requests = with_approvals(project, |requests| Ok((requests.clone(), false)))?;
    let shown: Vec<&Request> = requests
        .iter()
        .rev()
        .filter(|r| all || r.status == Status::Pending)
        .collect();
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": "fabric.approvals/v1",
                "requests": shown,
            }))
            .expect("output always serializes")
        );
        return Ok(exit::OK);
    }
    if shown.is_empty() {
        println!(
            "{}",
            if all {
                "No approval requests yet"
            } else {
                "No approvals waiting"
            }
        );
        return Ok(exit::OK);
    }
    for r in shown {
        println!(
            "#{:<4} {:<9} {}  {} {}  ({})",
            r.id,
            serde_json::to_value(r.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            r.requested_at,
            r.tool,
            r.target,
            r.rule
        );
    }
    if !all {
        println!("Review one with `fabric approve <n>`; see all with `fabric approvals --all`.");
    }
    Ok(exit::OK)
}

/// The change an approver is asked to approve, as they would review it.
pub fn describe_change(project: &Project, request: &Request) -> String {
    let input = &request.input;
    let mut out = String::new();
    match request.tool.as_str() {
        "Bash" => {
            let _ = writeln!(out, "$ {}", input["command"].as_str().unwrap_or_default());
        }
        "Write" => {
            let path = input["file_path"].as_str().unwrap_or_default();
            let current = fs::read_to_string(path).unwrap_or_default();
            let new = input["content"].as_str().unwrap_or_default();
            out.push_str(&unified(
                &project_relative(path, &project.root),
                &current,
                new,
            ));
        }
        "Edit" => {
            let path = input["file_path"].as_str().unwrap_or_default();
            out.push_str(&edit_diff(
                &project_relative(path, &project.root),
                input["old_string"].as_str().unwrap_or_default(),
                input["new_string"].as_str().unwrap_or_default(),
            ));
        }
        "MultiEdit" => {
            let path = input["file_path"].as_str().unwrap_or_default();
            let relative = project_relative(path, &project.root);
            for edit in input["edits"].as_array().into_iter().flatten() {
                out.push_str(&edit_diff(
                    &relative,
                    edit["old_string"].as_str().unwrap_or_default(),
                    edit["new_string"].as_str().unwrap_or_default(),
                ));
            }
        }
        _ => {
            let _ = writeln!(
                out,
                "{}",
                serde_json::to_string_pretty(input).unwrap_or_default()
            );
        }
    }
    out
}

fn unified(path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("{path} (now)"), &format!("{path} (proposed)"))
        .to_string()
}

fn edit_diff(path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("{path} (replace)"), &format!("{path} (with)"))
        .to_string()
}

fn in_agent_environment() -> Option<&'static str> {
    AGENT_MARKERS
        .iter()
        .copied()
        .find(|name| std::env::var_os(name).is_some())
}

fn git_identity(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "user.email"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let email = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !email.is_empty()).then_some(email)
}

pub struct Decision<'a> {
    pub id: u64,
    pub reject: bool,
    pub approver: Option<&'a str>,
    pub note: Option<&'a str>,
    pub yes: bool,
}

pub fn approve_cmd(project: &Project, d: &Decision<'_>, json_output: bool) -> Result<u8, CliError> {
    if let Some(marker) = in_agent_environment() {
        return Err(CliError::new(
            exit::ERROR,
            "approval_from_agent",
            format!(
                "approvals must come from a person's own terminal, not from inside an agent session ({marker} is set)"
            ),
        ));
    }
    let request = with_approvals(project, |requests| {
        Ok((find_request(requests, d.id)?.clone(), false))
    })?;
    let allowed = match (request.status, d.reject) {
        (Status::Pending, _) => true,
        // A person may change their mind about a rejection.
        (Status::Rejected, false) => true,
        _ => false,
    };
    if !allowed {
        return Err(CliError::new(
            exit::ERROR,
            "request_closed",
            format!(
                "approval request #{} is already {}",
                request.id,
                serde_json::to_value(request.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            ),
        ));
    }
    let approver = d
        .approver
        .map(str::to_owned)
        .or_else(|| git_identity(&project.root))
        .or_else(evidence::os_user)
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "no_identity",
                "cannot tell who is approving; pass --as NAME",
            )
        })?;
    if approver == request.agent {
        return Err(CliError::new(
            exit::ERROR,
            "self_approval",
            "an agent cannot approve its own change",
        ));
    }

    let verb = if d.reject { "Reject" } else { "Approve" };
    if !json_output || !d.yes {
        eprintln!(
            "Approval request #{} ({}: {})\nAgent {} wants to run {} on {}\nRequested {}\n",
            request.id,
            request.rule,
            request.reason,
            request.agent,
            request.tool,
            request.target,
            request.requested_at
        );
        eprintln!("{}", describe_change(project, &request));
    }
    if !d.yes {
        if !io::stdin().is_terminal() {
            return Err(CliError::new(
                exit::NEEDS_CONFIRMATION,
                "needs_confirmation",
                format!(
                    "review the change above, then rerun with --yes to {}",
                    verb.to_lowercase()
                ),
            ));
        }
        eprint!("{verb} this change as {approver}? [y/N] ");
        let _ = io::stderr().flush();
        let mut answer = String::new();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| CliError::new(exit::ERROR, "io", e.to_string()))?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            eprintln!("Nothing recorded.");
            return Ok(exit::NEEDS_CONFIRMATION);
        }
    }

    let os_user = evidence::os_user();
    let same_os_account = os_user.is_some() && os_user == request.requested_os_user;
    let status = if d.reject {
        Status::Rejected
    } else {
        Status::Approved
    };
    let decided_at = timefmt::now_rfc3339();
    let updated = with_approvals(project, |requests| {
        let request = requests
            .iter_mut()
            .find(|r| r.id == d.id)
            .expect("the request was found above and requests are never removed");
        if !matches!(request.status, Status::Pending | Status::Rejected) {
            return Err(CliError::new(
                exit::STATE_CHANGED,
                "state_changed",
                "the request changed while you were reviewing it; run the command again",
            ));
        }
        request.status = status;
        request.approver = Some(approver.clone());
        request.approver_os_user = os_user.clone();
        request.decided_at = Some(decided_at.clone());
        request.decided_unix = Some(now_unix());
        request.note = d.note.map(str::to_owned);
        Ok((request.clone(), true))
    })?;
    evidence::append(
        project,
        json!({
            "kind": if d.reject { "approval_rejected" } else { "approval_granted" },
            "request": updated.id,
            "digest": updated.digest,
            "rule": updated.rule,
            "target": evidence::clip(&updated.target),
            "approver": approver,
            "approver_os_user": os_user,
            "same_os_account_as_agent": same_os_account,
            "note": d.note,
        }),
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": "fabric.approve/v1",
                "request": updated,
                "same_os_account_as_agent": same_os_account,
            }))
            .expect("output always serializes")
        );
    } else if d.reject {
        println!("Rejected request #{} as {approver}.", updated.id);
    } else {
        println!(
            "Approved request #{} as {approver}. The agent can now retry exactly this change (valid for {} hours, once).",
            updated.id,
            VALID_FOR_SECS / 3600
        );
    }
    Ok(exit::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_identify_exact_actions() {
        let a = digest(
            "Edit",
            &json!({ "file_path": "a.py", "old_string": "x", "new_string": "y" }),
        );
        let reordered = digest(
            "Edit",
            &json!({ "new_string": "y", "old_string": "x", "file_path": "a.py" }),
        );
        let changed = digest(
            "Edit",
            &json!({ "file_path": "a.py", "old_string": "x", "new_string": "z" }),
        );
        assert_eq!(a, reordered);
        assert_ne!(a, changed);
        assert_ne!(
            a,
            digest(
                "Write",
                &json!({ "file_path": "a.py", "old_string": "x", "new_string": "y" })
            )
        );
    }

    #[test]
    fn unused_approvals_expire() {
        let mut requests = vec![Request {
            id: 1,
            digest: String::new(),
            rule: String::new(),
            reason: String::new(),
            agent: AGENT.to_owned(),
            session: None,
            tool: "Bash".to_owned(),
            target: String::new(),
            input: Value::Null,
            requested_at: String::new(),
            requested_os_user: None,
            status: Status::Approved,
            approver: Some("a@example.com".to_owned()),
            approver_os_user: None,
            decided_at: None,
            decided_unix: Some(now_unix() - VALID_FOR_SECS - 1),
            note: None,
        }];
        expire(&mut requests);
        assert_eq!(requests[0].status, Status::Expired);
    }
}
