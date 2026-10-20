//! Change requests: one authorization for a piece of work, one review of its result.
//!
//! Approving every held edit one by one is how a single feature ends up with half a dozen
//! approvals. Change management in a SOX setting works in two steps instead: a change is
//! *authorized* before work starts (what, where, by whom), and the result is *reviewed* before
//! it is accepted. Fabric records both:
//!
//! 1. `fabric approve <n> --change "<title>"` approves the held request and opens a change for
//!    the agent's session covering the given paths (by default, the requested files). Further
//!    edits by that session to those paths proceed without separate approvals; each is recorded
//!    in the evidence as `change_edit`. The change starts at the prompt that began the task.
//! 2. `fabric review <change>` shows everything that changed under the change's paths since it
//!    started, and records the reviewer's decision bound to that exact content (the
//!    reviewed state is a checkpoint, and each file's content id is in the evidence). Review
//!    closes the change: later edits need a new approval.
//!
//! A change never covers shell commands that a rule holds (for example direct SQL on the
//! books), nor files under a `deny` rule; those keep their own controls.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write as _};
use std::path::Path;

use fabric_snapshot::{Entry, Manifest, RelPath, diff};
use fabric_store::Store;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use similar::TextDiff;

use crate::approvals::{git_identity, in_agent_environment};
use crate::checkpoint::{CheckpointKind, Meta, read_log, record};
use crate::error::{CliError, exit};
use crate::evidence::{self, Record};
use crate::home::{Project, display_path, write_durably};
use crate::timefmt;

const FILE: &str = "changes.json";
const LOCK_FILE: &str = "changes.lock";
const FORMAT: &str = "fabric-changes";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Authorized; the agent's edits to its paths proceed and are recorded.
    Open,
    /// Reviewed and accepted.
    Accepted,
    /// Reviewed and not accepted; the work should be reverted.
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: u64,
    pub title: String,
    /// Globs over project-relative paths that the change covers.
    pub paths: Vec<String>,
    pub session: Option<String>,
    /// The approval requests whose approval opened the change.
    pub requests: Vec<u64>,
    /// Where the change starts: the session's latest prompt before it was authorized (else the
    /// latest checkpoint). The review compares against it.
    pub base_checkpoint: u64,
    pub status: Status,
    pub authorized_by: String,
    pub authorized_os_user: Option<String>,
    pub authorized_at: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub reviewed_by: Option<String>,
    #[serde(default)]
    pub reviewed_os_user: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub review_note: Option<String>,
    /// The checkpoint holding exactly the state that was reviewed.
    #[serde(default)]
    pub reviewed_checkpoint: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangesFile {
    format: String,
    version: u32,
    changes: Vec<Change>,
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Open => "open",
        Status::Accepted => "accepted",
        Status::Rejected => "rejected",
    }
}

/// Compiles path globs the way project rules do (`*` stays within one directory).
pub fn globs(patterns: &[String]) -> Result<GlobSet, String> {
    let mut set = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| format!("invalid path pattern {pattern:?}: {e}"))?;
        set.add(glob);
    }
    set.build().map_err(|e| e.to_string())
}

fn with_changes<T>(
    project: &Project,
    f: impl FnOnce(&mut Vec<Change>) -> Result<(T, bool), CliError>,
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
    let mut changes = load(&path)?;
    let (result, changed) = f(&mut changes)?;
    if changed {
        let file = ChangesFile {
            format: FORMAT.to_owned(),
            version: VERSION,
            changes,
        };
        let bytes = serde_json::to_vec_pretty(&file).expect("changes always serialize");
        write_durably(&path, &bytes)?;
    }
    drop(lock);
    Ok(result)
}

fn load(path: &Path) -> Result<Vec<Change>, CliError> {
    match fs::read(path) {
        Ok(bytes) => {
            let file: ChangesFile = serde_json::from_slice(&bytes).map_err(|e| {
                CliError::new(
                    exit::ERROR,
                    "invalid_changes",
                    format!("{}: {e}", display_path(path)),
                )
            })?;
            if file.format != FORMAT || file.version != VERSION {
                return Err(CliError::new(
                    exit::ERROR,
                    "invalid_changes",
                    format!("{}: unsupported changes file", display_path(path)),
                ));
            }
            Ok(file.changes)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CliError::io(path, e)),
    }
}

/// What `fabric approve --change` needs to open a change.
pub struct Authorization<'a> {
    pub title: &'a str,
    pub paths: Vec<String>,
    pub session: Option<String>,
    pub requests: Vec<u64>,
    pub approver: &'a str,
    pub os_user: Option<String>,
    pub same_os_account_as_agent: bool,
    pub note: Option<&'a str>,
}

/// Checks the paths of a change before anything is recorded.
pub fn validate_paths(paths: &[String]) -> Result<(), CliError> {
    if paths.is_empty() {
        return Err(CliError::new(
            exit::ERROR,
            "invalid_change",
            "a change must cover at least one path",
        ));
    }
    globs(paths)
        .map(drop)
        .map_err(|e| CliError::new(exit::ERROR, "invalid_change", e))
}

/// Opens a change and records it in the evidence.
pub fn open(project: &Project, a: Authorization<'_>) -> Result<Change, CliError> {
    validate_paths(&a.paths)?;
    // The change starts where the task did: at the session's latest prompt (the state before
    // the agent acted on it), so edits made before its first held change belong to it too.
    let log = read_log(project)?;
    let base_checkpoint = log
        .iter()
        .rev()
        .find(|c| c.kind == CheckpointKind::Prompt && c.session == a.session)
        .or(log.last())
        .map_or(0, |c| c.seq);
    let change = with_changes(project, |changes| {
        let change = Change {
            id: changes.iter().map(|c| c.id).max().unwrap_or(0) + 1,
            title: a.title.to_owned(),
            paths: a.paths.clone(),
            session: a.session.clone(),
            requests: a.requests.clone(),
            base_checkpoint,
            status: Status::Open,
            authorized_by: a.approver.to_owned(),
            authorized_os_user: a.os_user.clone(),
            authorized_at: timefmt::now_rfc3339(),
            note: a.note.map(str::to_owned),
            reviewed_by: None,
            reviewed_os_user: None,
            reviewed_at: None,
            review_note: None,
            reviewed_checkpoint: None,
        };
        changes.push(change.clone());
        Ok((change, true))
    })?;
    evidence::append(
        project,
        json!({
            "kind": "change_opened",
            "change": change.id,
            "title": evidence::clip(&change.title),
            "paths": change.paths,
            "session": change.session,
            "requests": change.requests,
            "base_checkpoint": change.base_checkpoint,
            "approver": change.authorized_by,
            "approver_os_user": change.authorized_os_user,
            "same_os_account_as_agent": a.same_os_account_as_agent,
            "note": change.note,
        }),
    )?;
    Ok(change)
}

/// The open change of `session` that covers file `target` (project-relative), if any.
pub fn covering(
    project: &Project,
    session: Option<&str>,
    target: &str,
) -> Result<Option<Change>, CliError> {
    let changes = with_changes(project, |changes| Ok((changes.clone(), false)))?;
    Ok(changes.into_iter().find(|c| {
        c.status == Status::Open
            && c.session.as_deref() == session
            && globs(&c.paths).is_ok_and(|g| g.is_match(target))
    }))
}

/// Records an edit made under change `change`.
pub fn record_edit(
    project: &Project,
    change: &Change,
    tool: &str,
    digest: &str,
    target: &str,
) -> Result<(), CliError> {
    evidence::append(
        project,
        json!({
            "kind": "change_edit",
            "change": change.id,
            "session": change.session,
            "tool": tool,
            "digest": digest,
            "target": evidence::clip(target),
        }),
    )
    .map(drop)
}

/// The changes open just before evidence record `end`, from the evidence alone: (session,
/// paths) for each. Used to tell whether a file an agent step changed was covered.
pub fn open_in_evidence(records: &[Record], end: usize) -> Vec<(Option<String>, GlobSet)> {
    let mut open: BTreeMap<u64, (Option<String>, GlobSet)> = BTreeMap::new();
    for record in &records[..end.min(records.len())] {
        let event = &record.body.event;
        let Some(id) = event["change"].as_u64() else {
            continue;
        };
        match event["kind"].as_str() {
            Some("change_opened") => {
                let paths: Vec<String> = event["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect();
                if let Ok(set) = globs(&paths) {
                    let session = event["session"].as_str().map(str::to_owned);
                    open.insert(id, (session, set));
                }
            }
            Some("change_reviewed") => {
                open.remove(&id);
            }
            _ => {}
        }
    }
    open.into_values().collect()
}

/// Where a change starts and ends, from the evidence alone.
pub struct Span {
    pub title: String,
    pub session: Option<String>,
    /// The checkpoint when it was authorized.
    pub base: u64,
    /// The reviewed checkpoint, or the latest one if the change is still open.
    pub end: u64,
}

pub fn span(records: &[Record], latest: u64, id: u64) -> Result<Span, CliError> {
    let event = |kind: &str| {
        records
            .iter()
            .map(|r| &r.body.event)
            .find(|e| e["kind"] == kind && e["change"].as_u64() == Some(id))
    };
    let opened = event("change_opened").ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_change",
            format!("no change #{id} in the evidence; see `fabric changes --all`"),
        )
    })?;
    Ok(Span {
        title: opened["title"].as_str().unwrap_or_default().to_owned(),
        session: opened["session"].as_str().map(str::to_owned),
        base: opened["base_checkpoint"].as_u64().unwrap_or(0),
        end: event("change_reviewed")
            .and_then(|e| e["checkpoint"].as_u64())
            .unwrap_or(latest),
    })
}

// ---- `fabric changes` and `fabric review` -------------------------------------------------

pub fn list_cmd(project: &Project, all: bool, json_output: bool) -> Result<u8, CliError> {
    let changes = with_changes(project, |changes| Ok((changes.clone(), false)))?;
    let shown: Vec<&Change> = changes
        .iter()
        .rev()
        .filter(|c| all || c.status == Status::Open)
        .collect();
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({ "schema": "fabric.changes/v1", "changes": shown })
            )
            .expect("output always serializes")
        );
        return Ok(exit::OK);
    }
    if shown.is_empty() {
        println!(
            "{}",
            if all {
                "No changes yet"
            } else {
                "No open changes"
            }
        );
        return Ok(exit::OK);
    }
    for c in &shown {
        println!(
            "#{:<4} {:<8} {}  {}  [{}]  authorized by {}",
            c.id,
            status_name(c.status),
            c.authorized_at,
            c.title,
            c.paths.join(", "),
            c.authorized_by
        );
    }
    if !all {
        println!("Review one with `fabric review <n>`; see all with `fabric changes --all`.");
    }
    Ok(exit::OK)
}

pub struct Review<'a> {
    pub id: u64,
    pub reject: bool,
    pub reviewer: Option<&'a str>,
    pub note: Option<&'a str>,
    pub yes: bool,
}

/// Files under the change's paths that differ between `before` and `after`, with the content
/// id each has after (`None`: removed or not a regular file).
pub fn changed_files(
    before: &Manifest,
    after: &Manifest,
    scope: &GlobSet,
) -> Vec<(RelPath, Option<String>)> {
    let d = diff(before, after);
    let mut paths: Vec<RelPath> = d
        .added
        .iter()
        .chain(&d.modified)
        .chain(&d.removed)
        .cloned()
        .chain(
            d.renamed
                .iter()
                .flat_map(|r| [r.from.clone(), r.to.clone()]),
        )
        .filter(|p| scope.is_match(p.as_str()))
        .collect();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|p| {
            let id = match after.get(&p) {
                Some(Entry::File { id, .. }) => Some(id.to_string()),
                _ => None,
            };
            (p, id)
        })
        .collect()
}

fn file_text(store: &Store, entry: Option<&Entry>) -> Result<Option<String>, CliError> {
    match entry {
        Some(Entry::File { id, .. }) => {
            let bytes = store.read(id)?;
            Ok(String::from_utf8(bytes).ok().filter(|s| !s.contains('\0')))
        }
        _ => Ok(Some(String::new())),
    }
}

/// The full diff a reviewer approves: nothing is truncated.
pub fn review_diff(
    store: &Store,
    before: &Manifest,
    after: &Manifest,
    files: &[(RelPath, Option<String>)],
) -> Result<String, CliError> {
    let mut out = String::new();
    for (path, _) in files {
        let (Some(old), Some(new)) = (
            file_text(store, before.get(path))?,
            file_text(store, after.get(path))?,
        ) else {
            let _ = writeln!(out, "{path}: binary file changed");
            continue;
        };
        out.push_str(
            &TextDiff::from_lines(&old, &new)
                .unified_diff()
                .context_radius(3)
                .header(&format!("{path} (authorized)"), &format!("{path} (now)"))
                .to_string(),
        );
    }
    Ok(out)
}

pub fn review_cmd(
    store: &Store,
    project: &Project,
    r: &Review<'_>,
    json_output: bool,
) -> Result<u8, CliError> {
    if let Some(marker) = in_agent_environment() {
        return Err(CliError::new(
            exit::ERROR,
            "review_from_agent",
            format!(
                "reviews must come from a person's own terminal, not from inside an agent session ({marker} is set)"
            ),
        ));
    }
    let change = with_changes(project, |changes| {
        Ok((changes.iter().find(|c| c.id == r.id).cloned(), false))
    })?
    .ok_or_else(|| {
        CliError::new(
            exit::ERROR,
            "unknown_change",
            format!("no change #{}; see `fabric changes --all`", r.id),
        )
    })?;
    if change.status != Status::Open {
        return Err(CliError::new(
            exit::ERROR,
            "change_closed",
            format!(
                "change #{} was already reviewed ({})",
                change.id,
                status_name(change.status)
            ),
        ));
    }
    let reviewer = r
        .reviewer
        .map(str::to_owned)
        .or_else(|| git_identity(&project.root))
        .or_else(evidence::os_user)
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "no_identity",
                "cannot tell who is reviewing; pass --as NAME",
            )
        })?;

    let log = read_log(project)?;
    let base = log
        .iter()
        .find(|c| c.seq == change.base_checkpoint)
        .ok_or_else(|| {
            CliError::new(
                exit::INTEGRITY,
                "missing_checkpoint",
                format!(
                    "change #{} starts at checkpoint #{}, which is not in the log",
                    change.id, change.base_checkpoint
                ),
            )
        })?;
    let before = Manifest::load(store, &base.snapshot)?;
    let scope =
        globs(&change.paths).map_err(|e| CliError::new(exit::ERROR, "invalid_change", e))?;
    let shown = project.capture(store)?;
    let files = changed_files(&before, &shown.manifest, &scope);
    let steps: Vec<String> = log
        .iter()
        .filter(|c| c.seq > change.base_checkpoint && c.session == change.session)
        .filter(|c| c.kind == CheckpointKind::AgentStep || c.agent.as_deref() == Some("fabric"))
        .map(|c| format!("#{} {}", c.seq, c.label))
        .collect();

    let verb = if r.reject { "Reject" } else { "Accept" };
    if !json_output || !r.yes {
        eprintln!(
            "Change #{}: {}\nAuthorized by {} at {} for {}\nSession {}, since checkpoint #{}\n",
            change.id,
            change.title,
            change.authorized_by,
            change.authorized_at,
            change.paths.join(", "),
            change.session.as_deref().unwrap_or("-"),
            change.base_checkpoint
        );
        if steps.is_empty() {
            eprintln!("No agent steps recorded since the change was authorized.");
        } else {
            eprintln!("Agent steps:\n  {}\n", steps.join("\n  "));
        }
        if files.is_empty() {
            eprintln!("Nothing under these paths has changed.");
        } else {
            eprintln!("{}", review_diff(store, &before, &shown.manifest, &files)?);
        }
    }
    if !r.yes {
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
        eprint!("{verb} change #{} as {reviewer}? [y/N] ", change.id);
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

    // The decision is bound to exactly what was shown: if anything changed meanwhile, nothing
    // is recorded.
    let now = project.capture(store)?;
    if now.id != shown.id {
        return Err(CliError::new(
            exit::STATE_CHANGED,
            "state_changed",
            "the project changed while you were reviewing; nothing recorded, run the review again",
        ));
    }
    let mut meta = Meta::new(
        CheckpointKind::Manual,
        format!(
            "{} change #{}: {}",
            if r.reject { "rejected" } else { "accepted" },
            change.id,
            change.title
        ),
    );
    meta.session = change.session.clone();
    // Recorded even when nothing changed since the last checkpoint, so the review shows in
    // the timeline.
    let reviewed_checkpoint = record(store, project, &now, meta, false)?
        .map(|c| c.seq)
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "not_recorded",
                "the reviewed state could not be recorded",
            )
        })?;

    let os_user = evidence::os_user();
    let status = if r.reject {
        Status::Rejected
    } else {
        Status::Accepted
    };
    let reviewed_at = timefmt::now_rfc3339();
    let updated = with_changes(project, |changes| {
        let Some(c) = changes.iter_mut().find(|c| c.id == r.id) else {
            return Err(CliError::new(
                exit::STATE_CHANGED,
                "state_changed",
                "the change disappeared while you were reviewing; nothing recorded",
            ));
        };
        if c.status != Status::Open {
            return Err(CliError::new(
                exit::STATE_CHANGED,
                "state_changed",
                "the change was reviewed by someone else meanwhile; nothing recorded",
            ));
        }
        c.status = status;
        c.reviewed_by = Some(reviewer.clone());
        c.reviewed_os_user = os_user.clone();
        c.reviewed_at = Some(reviewed_at.clone());
        c.review_note = r.note.map(str::to_owned);
        c.reviewed_checkpoint = Some(reviewed_checkpoint);
        Ok((c.clone(), true))
    })?;
    let file_ids: BTreeMap<String, Option<String>> = files
        .iter()
        .map(|(p, id)| (p.as_str().to_owned(), id.clone()))
        .collect();
    evidence::append(
        project,
        json!({
            "kind": "change_reviewed",
            "change": updated.id,
            "outcome": status_name(status),
            "reviewer": reviewer,
            "reviewer_os_user": os_user,
            "same_os_account_as_authorizer": os_user.is_some() && os_user == updated.authorized_os_user,
            "checkpoint": reviewed_checkpoint,
            "snapshot": now.id.to_string(),
            "files": file_ids,
            "note": r.note,
        }),
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": "fabric.review/v1",
                "change": updated,
                "files": file_ids,
            }))
            .expect("output always serializes")
        );
    } else if r.reject {
        println!(
            "Rejected change #{} as {reviewer}. The work is still in the project: undo it with `fabric revert` on the agent steps above (newest first), or `fabric restore {} --yes` to return to the state before the change.",
            updated.id, updated.base_checkpoint
        );
    } else {
        println!(
            "Accepted change #{} as {reviewer} ({} files, recorded as checkpoint #{reviewed_checkpoint}). Further edits to these paths need a new approval.",
            updated.id,
            file_ids.len()
        );
    }
    Ok(exit::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_paths_use_the_same_globs_as_rules() {
        let set = globs(&["ledger/*.py".to_owned()]).unwrap();
        assert!(set.is_match("ledger/fees.py"));
        assert!(!set.is_match("ledger/sub/fees.py"));
        assert!(validate_paths(&[]).is_err());
        assert!(validate_paths(&["[".to_owned()]).is_err());
    }
}
