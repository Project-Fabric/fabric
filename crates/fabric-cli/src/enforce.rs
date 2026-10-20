//! Enforcement after the fact: an agent step that changed controlled files without an
//! approval is rolled back at once.
//!
//! The guard holds `Edit`/`Write` calls on controlled paths before they run, but a shell
//! command can change any file without naming it (`python fix.py`, `sed -i`). After every agent
//! step, the files it changed are checked against `approve`/`deny` project rules and the
//! approvals used in that step (see `report::exceptions_for`). Each unapproved controlled file
//! is put back exactly as it was before the step: its earlier content is rewritten, or a file
//! the step created is removed. The agent is told why and how to make the change properly.
//!
//! Nothing is lost: the step's own checkpoint, recorded before the rollback, holds what the
//! agent wrote, and the rollback is itself a checkpoint, so `fabric revert` of it restores the
//! agent's version. This matters if a person edited a controlled file by hand during the same
//! step: their edit is rolled back too, and recoverable the same way.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use fabric_snapshot::{Entry, Manifest, RelPath};
use fabric_store::Store;
use serde_json::{Value, json};

use crate::checkpoint::{Checkpoint, CheckpointKind, Meta, read_log, record};
use crate::error::{CliError, exit};
use crate::evidence;
use crate::guard::Policy;
use crate::home::Project;
use crate::report;

/// Writes `contents` to `path` through a temporary file and an atomic rename, creating parent
/// directories as needed. The temporary name cannot collide with a project file's.
fn write_file(path: &Path, contents: &[u8]) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CliError::io(parent, e))?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".fabric-rollback");
    let temp = std::path::PathBuf::from(temp);
    let mut file = File::create(&temp).map_err(|e| CliError::io(&temp, e))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|e| CliError::io(&temp, e))?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| CliError::io(path, e))
}

/// Rolls back the controlled files `checkpoint` changed without an approval. Returns the
/// message for the agent, or `None` if there was nothing to roll back.
pub fn rollback_unapproved(
    store: &Store,
    project: &Project,
    checkpoint: &Checkpoint,
) -> Result<Option<Value>, CliError> {
    if checkpoint.kind != CheckpointKind::AgentStep {
        return Ok(None);
    }
    let policy = Policy::load(project)?;
    let log = read_log(project)?;
    let (_, records) = evidence::verify_chain(&evidence::read_chain(project)?);
    let exceptions = report::exceptions_for(store, project, &policy, &log, &records, checkpoint)?;
    if exceptions.is_empty() {
        return Ok(None);
    }
    let Some(previous) = log.iter().take_while(|c| c.seq < checkpoint.seq).last() else {
        return Ok(None);
    };
    let before = Manifest::load(store, &previous.snapshot)?;

    let mut rolled_back = Vec::new();
    let mut left = Vec::new();
    for exception in &exceptions {
        let rel = RelPath::new(exception.path.clone())
            .map_err(|e| CliError::new(exit::INTEGRITY, "invalid_path", e.to_string()))?;
        let target = rel.to_fs_path(&project.root);
        match before.get(&rel) {
            Some(Entry::File { id, .. }) => {
                write_file(&target, &store.read(id)?)?;
                rolled_back.push(exception.path.clone());
            }
            None => match fs::symlink_metadata(&target) {
                Ok(meta) if meta.is_file() => {
                    fs::remove_file(&target).map_err(|e| CliError::io(&target, e))?;
                    rolled_back.push(exception.path.clone());
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                _ => left.push(exception.path.clone()),
            },
            // Directories and symlinks are not rolled back automatically.
            Some(_) => left.push(exception.path.clone()),
        }
    }
    let rules: Vec<String> = {
        let mut rules: Vec<String> = exceptions.iter().flat_map(|e| e.rules.clone()).collect();
        rules.sort();
        rules.dedup();
        rules
    };
    evidence::append(
        project,
        json!({
            "kind": "rollback",
            "step": checkpoint.seq,
            "paths": rolled_back,
            "not_rolled_back": left,
            "rules": rules,
            "reason": "controlled files changed without an approval",
        }),
    )?;
    let snapshot = project.capture(store)?;
    let mut meta = Meta::new(
        CheckpointKind::Revert,
        format!(
            "Fabric rolled back unapproved change to {}",
            rolled_back.join(", ")
        ),
    );
    meta.agent = Some("fabric".to_owned());
    meta.session = checkpoint.session.clone();
    record(store, project, &snapshot, meta, true)?;

    let mut reason = format!(
        "Fabric rolled back your change to {}: these files are under change control ({}) and changing them needs a recorded approval. \
         Make the change with the Edit or Write tool so it can be held for the user's approval; do not change these files through shell commands or scripts.",
        rolled_back.join(", "),
        rules.join(", ")
    );
    if !left.is_empty() {
        reason.push_str(&format!(
            " These could not be rolled back automatically and need the user's attention: {}.",
            left.join(", ")
        ));
    }
    Ok(Some(json!({ "decision": "block", "reason": reason })))
}
