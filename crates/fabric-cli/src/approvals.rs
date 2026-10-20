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
//!
//! For work that needs many edits, `fabric approve <n> --change "<title>"` approves the
//! request and authorizes the whole change at once (see `changes`).

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use similar::TextDiff;

use crate::changes;
use crate::error::{CliError, exit};
use crate::evidence;
use crate::guard::{Verdict, hook_decision};
use crate::home::{Home, Project, display_path, project_relative, write_durably};
use crate::signoff;
use crate::timefmt;

const FILE: &str = "approvals.json";
const LOCK_FILE: &str = "approvals.lock";
const FORMAT: &str = "fabric-approvals";
/// Version 2 added `used_via` and `used_unix`. Version 1 files are still read.
const VERSION: u32 = 2;
/// A call that passes several of Fabric's checkpoints (the Claude Code hook, then
/// `fabric mcp-proxy`) uses its approval at each of them within this time.
const SAME_CALL_SECS: u64 = 120;
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
    /// Where the approval was used: `hook` and/or `proxy`. One approval covers one call, which
    /// may pass both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_via: Vec<String>,
    #[serde(default)]
    pub used_unix: Option<u64>,
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
            if file.format != FORMAT || !(1..=VERSION).contains(&file.version) {
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
fn is_file_tool(tool: &str) -> bool {
    matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit")
}

/// `via` names the checkpoint deciding: `hook` (Claude Code's pre-action hook) or `proxy`
/// (`fabric mcp-proxy`).
pub fn gate(
    project: &Project,
    event: &Value,
    verdict: &Verdict,
    target: &str,
    via: &str,
) -> Result<Option<Value>, CliError> {
    let tool = event["tool_name"].as_str().unwrap_or_default().to_owned();
    let input = event["tool_input"].clone();
    let digest = digest(&tool, &input);
    let session = event["session_id"].as_str().map(str::to_owned);
    let rule = verdict.rule.clone().unwrap_or_default();

    // File edits inside an authorized change proceed and are recorded; the change's review
    // covers their content. Shell commands and tool calls always need their own approval.
    // An exact approval waiting for this action is used first.
    let approved = with_approvals(project, |requests| {
        Ok((
            requests
                .iter()
                .any(|r| r.digest == digest && r.status == Status::Approved),
            false,
        ))
    })?;
    if is_file_tool(&tool)
        && !approved
        && let Some(change) = changes::covering(project, session.as_deref(), target)?
    {
        changes::record_edit(project, &change, &tool, &digest, target)?;
        return Ok(None);
    }

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
            request.used_via = vec![via.to_owned()];
            request.used_unix = Some(now_unix());
            return Ok((Outcome::Proceed(request.id), true));
        }
        // The same call reaching another of Fabric's checkpoints right after it was approved
        // at the first one. Sessions are not compared: the hook knows the agent's session,
        // the proxy only its own.
        let now = now_unix();
        if let Some(request) = requests.iter_mut().rev().find(|r| {
            r.digest == digest
                && r.status == Status::Used
                && !r.used_via.is_empty()
                && !r.used_via.iter().any(|v| v == via)
                && r.used_unix
                    .is_some_and(|at| now.saturating_sub(at) <= SAME_CALL_SECS)
        }) {
            request.used_via.push(via.to_owned());
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
            used_via: Vec::new(),
            used_unix: None,
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
                    "via": via,
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
            // Only file edits can be authorized together as a change.
            let change_hint = if is_file_tool(&tool) {
                format!(
                    " If this is one of several edits for the same task, tell the user they can authorize the whole task at once \
                     with `fabric approve {id} --change \"<title>\"` and review the result with `fabric review` when you are done."
                )
            } else {
                String::new()
            };
            Ok(Some(hook_decision(
                "deny",
                &format!(
                    "Fabric: this change needs a recorded approval ({rule}: {}). Approval request #{id} is waiting. \
                     Tell the user to review and approve it in their own terminal with `fabric approve {id}`, \
                     then retry exactly the same action. Do not try to approve it yourself.{change_hint}",
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

/// Writes held requests to a file an approver can review and sign elsewhere
/// (`fabric approver sign`).
pub fn export_cmd(
    project: &Project,
    ids: &[u64],
    out: Option<&Path>,
    json_output: bool,
) -> Result<u8, CliError> {
    let requests = with_approvals(project, |requests| Ok((requests.clone(), false)))?;
    let mut exported = Vec::new();
    for id in ids {
        let request = find_request(&requests, *id)?;
        if !decidable(request.status, false) {
            return Err(CliError::new(
                exit::ERROR,
                "request_closed",
                format!(
                    "approval request #{id} is already {}",
                    status_name(request.status)
                ),
            ));
        }
        exported.push(json!({
            "id": request.id,
            "digest": request.digest,
            "rule": request.rule,
            "reason": request.reason,
            "agent": request.agent,
            "session": request.session,
            "tool": request.tool,
            "target": request.target,
            "input": request.input,
            "requested_at": request.requested_at,
            "change_text": describe_change(project, request),
        }));
    }
    let file = json!({
        "format": signoff::REQUEST_FORMAT,
        "version": 1,
        "project": { "id": project.id, "root": display_path(&project.root) },
        "exported_at": timefmt::now_rfc3339(),
        "requests": exported,
    });
    let bytes = serde_json::to_vec_pretty(&file).expect("requests always serialize");
    match out {
        Some(path) => {
            write_durably(path, &bytes)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "schema": "fabric.approvals.export/v1",
                        "out": display_path(path),
                        "requests": ids,
                    }))
                    .expect("output always serializes")
                );
            } else {
                println!(
                    "Wrote {}. The approver reviews and signs it with `fabric approver sign {}` on their own machine; apply the result with `fabric approve --signed <file>`.",
                    display_path(path),
                    display_path(path)
                );
            }
        }
        None => println!("{}", String::from_utf8_lossy(&bytes)),
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

pub fn in_agent_environment() -> Option<&'static str> {
    AGENT_MARKERS
        .iter()
        .copied()
        .find(|name| std::env::var_os(name).is_some())
}

pub fn git_identity(root: &Path) -> Option<String> {
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
    /// Requests decided together: each change is shown, confirmed once, recorded separately.
    pub ids: &'a [u64],
    pub reject: bool,
    pub approver: Option<&'a str>,
    pub note: Option<&'a str>,
    pub yes: bool,
    /// Also authorize a change with this title, covering `paths` (default: the requested
    /// files) for the requests' session.
    pub change: Option<&'a str>,
    pub paths: &'a [String],
    /// A decision signed by a registered approver (`fabric approver sign`); it supplies the
    /// requests, the decision, the change and the note.
    pub signed: Option<&'a Path>,
}

fn status_name(status: Status) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn decidable(status: Status, reject: bool) -> bool {
    match (status, reject) {
        (Status::Pending, _) => true,
        // A person may change their mind about a rejection.
        (Status::Rejected, false) => true,
        _ => false,
    }
}

pub fn approve_cmd(
    home: &Home,
    project: &Project,
    d: &Decision<'_>,
    json_output: bool,
) -> Result<u8, CliError> {
    if let Some(marker) = in_agent_environment() {
        return Err(CliError::new(
            exit::ERROR,
            "approval_from_agent",
            format!(
                "approvals must come from a person's own terminal, not from inside an agent session ({marker} is set)"
            ),
        ));
    }
    let Some(file) = d.signed else {
        signoff::refuse_unsigned(project, "approvals")?;
        return decide(project, d, None, json_output);
    };
    if !d.ids.is_empty()
        || d.reject
        || d.change.is_some()
        || !d.paths.is_empty()
        || d.note.is_some()
        || d.approver.is_some()
    {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_signed",
            "with --signed, the requests, decision, change and note come from the signed file",
        ));
    }
    let verified = signoff::verify_file(home, project, file, "approval")?;
    let p = &verified.payload;
    let ids: Vec<u64> = p["requests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["id"].as_u64())
        .collect();
    let paths: Vec<String> = p["change"]["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str().map(str::to_owned))
        .collect();
    let signed = Decision {
        ids: &ids,
        reject: p["decision"] == "reject",
        approver: Some(&verified.approver),
        note: p["note"].as_str(),
        yes: true,
        change: p["change"]["title"].as_str(),
        paths: &paths,
        signed: None,
    };
    decide(project, &signed, Some(&verified), json_output)
}

/// Records a decision on held requests; `verified` when it comes from a signed file.
fn decide(
    project: &Project,
    d: &Decision<'_>,
    verified: Option<&signoff::Verified>,
    json_output: bool,
) -> Result<u8, CliError> {
    let mut ids = d.ids.to_vec();
    ids.dedup();
    let chosen = with_approvals(project, |requests| {
        let chosen = ids
            .iter()
            .map(|id| find_request(requests, *id).cloned())
            .collect::<Result<Vec<_>, _>>()?;
        Ok((chosen, false))
    })?;
    if let Some(closed) = chosen.iter().find(|r| !decidable(r.status, d.reject)) {
        return Err(CliError::new(
            exit::ERROR,
            "request_closed",
            format!(
                "approval request #{} is already {}; nothing recorded",
                closed.id,
                status_name(closed.status)
            ),
        ));
    }
    // A signed decision covers exactly the actions it was signed for.
    if let Some(v) = verified {
        let signed_digest = |id: u64| {
            v.payload["requests"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["id"].as_u64() == Some(id))
                .and_then(|r| r["digest"].as_str())
        };
        if let Some(r) = chosen
            .iter()
            .find(|r| signed_digest(r.id) != Some(r.digest.as_str()))
        {
            return Err(CliError::new(
                exit::INTEGRITY,
                "digest_mismatch",
                format!(
                    "request #{} is not the action that was signed for; nothing recorded",
                    r.id
                ),
            ));
        }
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
    if chosen.iter().any(|r| r.agent == approver) {
        return Err(CliError::new(
            exit::ERROR,
            "self_approval",
            "an agent cannot approve its own change",
        ));
    }
    let change_paths = match d.change {
        None if !d.paths.is_empty() => {
            return Err(CliError::new(
                exit::ERROR,
                "invalid_change",
                "--path is only used with --change",
            ));
        }
        None => Vec::new(),
        Some(_) if d.reject => {
            return Err(CliError::new(
                exit::ERROR,
                "invalid_change",
                "--change authorizes work; it cannot be used with --reject",
            ));
        }
        Some(_) => {
            if chosen.windows(2).any(|w| w[0].session != w[1].session) {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_change",
                    "a change covers one agent session; these requests come from several",
                ));
            }
            let mut paths = if d.paths.is_empty() {
                chosen
                    .iter()
                    .filter(|r| r.tool != "Bash")
                    .map(|r| globset::escape(&r.target))
                    .collect()
            } else {
                d.paths.to_vec()
            };
            paths.sort();
            paths.dedup();
            changes::validate_paths(&paths)?;
            paths
        }
    };

    let verb = if d.reject { "Reject" } else { "Approve" };
    if !json_output || !d.yes {
        for request in &chosen {
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
            eprintln!("{}", describe_change(project, request));
        }
        if let Some(title) = d.change {
            eprintln!(
                "This also authorizes change \"{title}\": further edits by this agent session to {} 
                 proceed without separate approvals until you review the result with `fabric review`.
",
                change_paths.join(", ")
            );
        }
    }
    let what = if chosen.len() == 1 {
        "this change".to_owned()
    } else {
        format!("these {} changes", chosen.len())
    };
    if !d.yes {
        if !io::stdin().is_terminal() {
            return Err(CliError::new(
                exit::NEEDS_CONFIRMATION,
                "needs_confirmation",
                format!(
                    "review {what} above, then rerun with --yes to {}",
                    verb.to_lowercase()
                ),
            ));
        }
        eprint!("{verb} {what} as {approver}? [y/N] ");
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

    // A signed decision was made elsewhere: the local OS account is not the approver's.
    let os_user = if verified.is_some() {
        None
    } else {
        evidence::os_user()
    };
    let status = if d.reject {
        Status::Rejected
    } else {
        Status::Approved
    };
    let decided_at = timefmt::now_rfc3339();
    // All or nothing: if any request changed while being reviewed, none is recorded.
    let updated = with_approvals(project, |requests| {
        if requests
            .iter()
            .filter(|r| ids.contains(&r.id))
            .any(|r| !decidable(r.status, d.reject))
        {
            return Err(CliError::new(
                exit::STATE_CHANGED,
                "state_changed",
                "a request changed while you were reviewing it; nothing recorded, run the command again",
            ));
        }
        let mut updated = Vec::new();
        for request in requests.iter_mut().filter(|r| ids.contains(&r.id)) {
            request.status = status;
            request.approver = Some(approver.clone());
            request.approver_os_user = os_user.clone();
            request.decided_at = Some(decided_at.clone());
            request.decided_unix = Some(now_unix());
            request.note = d.note.map(str::to_owned);
            updated.push(request.clone());
        }
        Ok((updated, true))
    })?;
    for request in &updated {
        evidence::append(
            project,
            json!({
                "kind": if d.reject { "approval_rejected" } else { "approval_granted" },
                "request": request.id,
                "digest": request.digest,
                "rule": request.rule,
                "target": evidence::clip(&request.target),
                "approver": approver,
                "approver_os_user": os_user,
                "same_os_account_as_agent": os_user.is_some() && os_user == request.requested_os_user,
                "note": d.note,
                "approver_key": verified.map(|v| v.fingerprint.clone()),
                "signed": verified.map(|v| v.evidence.clone()),
            }),
        )?;
    }

    let change = match d.change {
        Some(title) => Some(changes::open(
            project,
            changes::Authorization {
                title,
                paths: change_paths,
                session: updated.first().and_then(|r| r.session.clone()),
                requests: updated.iter().map(|r| r.id).collect(),
                approver: &approver,
                os_user: os_user.clone(),
                same_os_account_as_agent: os_user.is_some()
                    && updated.iter().any(|r| r.requested_os_user == os_user),
                note: d.note,
                signed: verified.map(|v| v.evidence.clone()),
                approver_key: verified.map(|v| v.fingerprint.clone()),
            },
        )?),
        None => None,
    };

    let numbers = updated
        .iter()
        .map(|r| format!("#{}", r.id))
        .collect::<Vec<_>>()
        .join(", ");
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": "fabric.approve/v2",
                "requests": updated,
                "change": change,
            }))
            .expect("output always serializes")
        );
    } else if d.reject {
        println!("Rejected {numbers} as {approver}.");
    } else {
        println!(
            "Approved {numbers} as {approver}. The agent can now retry exactly these changes (each once, within {} hours).",
            VALID_FOR_SECS / 3600
        );
        if let Some(change) = &change {
            println!(
                "Opened change #{} \"{}\" for {}. Review the result with `fabric review {}` when the agent is done.",
                change.id,
                change.title,
                change.paths.join(", "),
                change.id
            );
        }
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
            used_via: Vec::new(),
            used_unix: None,
            note: None,
        }];
        expire(&mut requests);
        assert_eq!(requests[0].status, Status::Expired);
    }
}
