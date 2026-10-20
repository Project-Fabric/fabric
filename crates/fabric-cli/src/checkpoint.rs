//! The checkpoint log: an append-only, per-project timeline of snapshots with context about
//! who took them and why. Snapshots say *what* the project looked like; checkpoints say
//! *when*, *by whom* and *after what*.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};

use fabric_snapshot::{Diff, Manifest, Snapshot, diff};
use fabric_store::{ObjectId, Store};
use serde::{Deserialize, Serialize};

use crate::error::{CliError, exit};
use crate::evidence;
use crate::home::Project;
use crate::timefmt;

const LOG_FILE: &str = "checkpoints.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub seq: u64,
    pub snapshot: ObjectId,
    pub created_at: String,
    pub kind: CheckpointKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Changes relative to the previous checkpoint.
    pub changes: ChangeCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKind {
    /// Taken when the project was registered.
    Init,
    /// Taken by `fabric checkpoint`.
    Manual,
    /// Taken when a prompt was sent to an agent, before it acted.
    Prompt,
    /// Taken after an agent tool call that changed the project.
    AgentStep,
    /// The state just before a restore; restoring it undoes the restore.
    PreRestore,
    /// The state a restore produced.
    Restore,
    /// The state after undoing one checkpoint's changes.
    Revert,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeCounts {
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    pub renamed: usize,
}

impl From<&Diff> for ChangeCounts {
    fn from(d: &Diff) -> Self {
        Self {
            added: d.added.len(),
            modified: d.modified.len(),
            removed: d.removed.len(),
            renamed: d.renamed.len(),
        }
    }
}

/// Context for a new checkpoint.
pub struct Meta {
    pub kind: CheckpointKind,
    pub label: String,
    pub agent: Option<String>,
    pub session: Option<String>,
    pub tool: Option<String>,
}

impl Meta {
    pub fn new(kind: CheckpointKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
            agent: None,
            session: None,
            tool: None,
        }
    }
}

pub fn read_log(project: &Project) -> Result<Vec<Checkpoint>, CliError> {
    let path = project.dir.join(LOG_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CliError::io(&path, e)),
    };
    let complete = complete_prefix(&text);
    complete
        .lines()
        .enumerate()
        .map(|(n, line)| {
            serde_json::from_str(line).map_err(|e| {
                CliError::new(
                    exit::INTEGRITY,
                    "corrupt_log",
                    format!("checkpoint log line {}: {e}", n + 1),
                )
            })
        })
        .collect()
}

/// The log up to its last newline. Anything after it is a record whose write was interrupted,
/// which never completed and is ignored.
fn complete_prefix(text: &str) -> &str {
    text.rfind('\n').map_or("", |end| &text[..=end])
}

/// Records a captured snapshot as a new checkpoint. With `if_changed`, nothing is recorded
/// when the project is identical to the latest checkpoint.
pub fn record(
    store: &Store,
    project: &Project,
    snapshot: &Snapshot,
    meta: Meta,
    if_changed: bool,
) -> Result<Option<Checkpoint>, CliError> {
    record_manifest(
        store,
        project,
        snapshot.id,
        &snapshot.manifest,
        meta,
        if_changed,
    )
}

pub fn record_manifest(
    store: &Store,
    project: &Project,
    id: ObjectId,
    manifest: &Manifest,
    meta: Meta,
    if_changed: bool,
) -> Result<Option<Checkpoint>, CliError> {
    let log = read_log(project)?;
    let previous = log.last();
    if if_changed && previous.is_some_and(|p| p.snapshot == id) {
        return Ok(None);
    }
    let changes = match previous {
        Some(p) => (&diff(&Manifest::load(store, &p.snapshot)?, manifest)).into(),
        None => ChangeCounts {
            added: manifest.entries().len(),
            ..ChangeCounts::default()
        },
    };
    let checkpoint = Checkpoint {
        seq: previous.map_or(1, |p| p.seq + 1),
        snapshot: id,
        created_at: timefmt::now_rfc3339(),
        kind: meta.kind,
        label: meta.label,
        agent: meta.agent,
        session: meta.session,
        tool: meta.tool,
        changes,
    };
    append(project, &checkpoint)?;
    evidence::append(
        project,
        evidence::checkpoint_event(&checkpoint, previous.map(|p| &p.snapshot)),
    )?;
    Ok(Some(checkpoint))
}

fn append(project: &Project, checkpoint: &Checkpoint) -> Result<(), CliError> {
    let path = project.dir.join(LOG_FILE);
    // Drop an interrupted final record first, so the new one starts on its own line.
    if let Ok(text) = fs::read_to_string(&path) {
        let keep = complete_prefix(&text).len();
        if keep < text.len() {
            let file = OpenOptions::new()
                .write(true)
                .open(&path)
                .map_err(|e| CliError::io(&path, e))?;
            file.set_len(keep as u64)
                .map_err(|e| CliError::io(&path, e))?;
        }
    }
    let mut line = serde_json::to_string(checkpoint).expect("checkpoints always serialize");
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| CliError::io(&path, e))?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.sync_data())
        .map_err(|e| CliError::io(&path, e))
}

/// Resolves what a user typed into a snapshot id: `latest`, a checkpoint number (`12` or
/// `#12`), or a prefix of a snapshot id (at least 6 hex characters, `blake3:` optional).
pub fn resolve(log: &[Checkpoint], reference: &str) -> Result<(ObjectId, Option<u64>), CliError> {
    let not_found = || {
        CliError::new(
            exit::ERROR,
            "unknown_checkpoint",
            format!("no checkpoint matches {reference:?}; see `fabric log`"),
        )
    };
    if reference == "latest" {
        return log
            .last()
            .map(|c| (c.snapshot, Some(c.seq)))
            .ok_or_else(not_found);
    }
    if let Ok(seq) = reference.trim_start_matches('#').parse::<u64>() {
        return log
            .iter()
            .find(|c| c.seq == seq)
            .map(|c| (c.snapshot, Some(seq)))
            .ok_or_else(not_found);
    }
    let prefix = reference.trim_start_matches("blake3:");
    if prefix.len() < 6 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(not_found());
    }
    let prefix = prefix.to_ascii_lowercase();
    let mut matches: Vec<ObjectId> = log
        .iter()
        .map(|c| c.snapshot)
        .filter(|id| id.to_hex().starts_with(&prefix))
        .collect();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [id] => Ok((
            *id,
            log.iter().rev().find(|c| c.snapshot == *id).map(|c| c.seq),
        )),
        [] => Err(not_found()),
        _ => Err(CliError::new(
            exit::ERROR,
            "ambiguous_checkpoint",
            format!("{reference:?} matches several snapshots; use more characters"),
        )),
    }
}
