use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use fabric_snapshot::{Manifest, RESTORE_DIR, RelPath, capture};
use fabric_store::{ObjectId, Store};
use serde::{Deserialize, Serialize};

use crate::error::io_at;
use crate::{Operation, RecoveryError, RestorePlan};

const JOURNAL_FORMAT: &str = "fabric-restore-journal";
const JOURNAL_VERSION: u32 = 1;
const JOURNAL_FILE: &str = "journal.json";
const PROGRESS_FILE: &str = "progress";
const STAGED_DIR: &str = "staged";
const RENAME_ATTEMPTS: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub target: ObjectId,
    /// Restore this snapshot to undo the restore.
    pub undo_snapshot: ObjectId,
    pub operations_applied: usize,
    /// Files that were not part of the pre-restore snapshot (for example, ignored by a
    /// `.gitignore` the restore removed) and were left untouched.
    pub untracked_kept: Vec<RelPath>,
}

#[derive(Debug, Default, Clone)]
pub struct ApplyOptions {
    /// Testing aid: stop as if the process died just before applying operation N.
    #[doc(hidden)]
    pub simulate_crash_before_op: Option<usize>,
}

/// The durable record of a restore in progress, written before the project is touched.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    format: String,
    version: u32,
    target: ObjectId,
    base: ObjectId,
    operations: Vec<Operation>,
}

/// Applies a plan produced by [`plan_restore`](crate::plan_restore).
pub fn apply_restore(
    store: &Store,
    root: &Path,
    plan: &RestorePlan,
) -> Result<RestoreReport, RecoveryError> {
    apply_restore_with(store, root, plan, &ApplyOptions::default())
}

pub fn apply_restore_with(
    store: &Store,
    root: &Path,
    plan: &RestorePlan,
    options: &ApplyOptions,
) -> Result<RestoreReport, RecoveryError> {
    if !plan.conflicts.is_empty() {
        return Err(RecoveryError::Conflicts(plan.conflicts.clone()));
    }
    let root = fs::canonicalize(root).map_err(io_at(root))?;
    let dir = root.join(RESTORE_DIR);
    if dir.exists() {
        return Err(RecoveryError::RestorePending { path: dir });
    }

    // The plan is only valid for the exact state it was computed from.
    let current = capture(store, &root)?;
    if current.id != plan.base {
        return Err(RecoveryError::ProjectChanged {
            planned: plan.base,
            current: current.id,
        });
    }

    let journal = Journal {
        format: JOURNAL_FORMAT.to_owned(),
        version: JOURNAL_VERSION,
        target: plan.target,
        base: plan.base,
        operations: plan.operations.clone(),
    };
    fs::create_dir(&dir).map_err(io_at(&dir))?;
    let bytes = serde_json::to_vec(&journal).expect("journals always serialize");
    write_durably(&dir.join(JOURNAL_FILE), &bytes)?;

    run(store, &root, &journal, 0, options)
}

/// Finishes a restore that was interrupted. Returns `None` if no restore was pending.
pub fn resume_restore(store: &Store, root: &Path) -> Result<Option<RestoreReport>, RecoveryError> {
    let root = fs::canonicalize(root).map_err(io_at(root))?;
    let dir = root.join(RESTORE_DIR);
    let journal_path = dir.join(JOURNAL_FILE);
    let bytes = match fs::read(&journal_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Interrupted before the journal was committed, so the project was never
            // touched; only the staging area needs cleaning up.
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(io_at(&dir))?;
            }
            return Ok(None);
        }
        Err(e) => return Err(io_at(&journal_path)(e)),
    };
    let journal: Journal =
        serde_json::from_slice(&bytes).map_err(|e| RecoveryError::InvalidJournal(e.to_string()))?;
    if journal.format != JOURNAL_FORMAT || journal.version != JOURNAL_VERSION {
        return Err(RecoveryError::InvalidJournal(format!(
            "unsupported journal {} v{}",
            journal.format, journal.version
        )));
    }
    let done = completed_operations(&dir.join(PROGRESS_FILE), journal.operations.len())?;
    run(store, &root, &journal, done, &ApplyOptions::default()).map(Some)
}

fn run(
    store: &Store,
    root: &Path,
    journal: &Journal,
    already_done: usize,
    options: &ApplyOptions,
) -> Result<RestoreReport, RecoveryError> {
    let dir = root.join(RESTORE_DIR);
    if let Err(e) = stage(store, &dir, &journal.operations, already_done) {
        if already_done == 0 {
            // Nothing in the project has been touched; abandon the restore entirely.
            let _ = fs::remove_dir_all(&dir);
        }
        return Err(e);
    }

    let progress_path = dir.join(PROGRESS_FILE);
    let mut progress = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&progress_path)
        .map_err(io_at(&progress_path))?;
    for (index, op) in journal.operations.iter().enumerate().skip(already_done) {
        if options.simulate_crash_before_op == Some(index) {
            return Err(RecoveryError::Interrupted(index));
        }
        apply_operation(root, &dir, index, op)?;
        writeln!(progress, "{index}")
            .and_then(|()| progress.sync_data())
            .map_err(io_at(&progress_path))?;
    }
    drop(progress);
    fs::remove_dir_all(&dir).map_err(io_at(&dir))?;

    verify(store, root, journal)
}

/// Checks the restored project against the target. Every target entry must match exactly,
/// and nothing from the pre-restore snapshot that the target lacks may remain. Anything else
/// present was never captured, was never touched, and is reported rather than failed.
fn verify(store: &Store, root: &Path, journal: &Journal) -> Result<RestoreReport, RecoveryError> {
    let failed = |reason: String| RecoveryError::VerificationFailed {
        target: journal.target,
        undo_snapshot: journal.base,
        reason,
    };
    let target = Manifest::load(store, &journal.target)?;
    let base = Manifest::load(store, &journal.base)?;
    let result = capture(store, root)?;

    for entry in target.entries() {
        if result.manifest.get(entry.path()) != Some(entry) {
            return Err(failed(format!("{} does not match", entry.path())));
        }
    }
    let mut untracked_kept = Vec::new();
    for entry in result.manifest.entries() {
        if target.get(entry.path()).is_some() {
            continue;
        }
        if base.get(entry.path()).is_some() {
            return Err(failed(format!("{} should have been removed", entry.path())));
        }
        untracked_kept.push(entry.path().clone());
    }
    Ok(RestoreReport {
        target: journal.target,
        undo_snapshot: journal.base,
        operations_applied: journal.operations.len(),
        untracked_kept,
    })
}

fn staged_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(STAGED_DIR).join(index.to_string())
}

/// Reconstructs every file still to be written into the staging area, verifying each against
/// its object id. Runs to completion before the project is modified.
fn stage(
    store: &Store,
    dir: &Path,
    operations: &[Operation],
    from: usize,
) -> Result<(), RecoveryError> {
    let staged_dir = dir.join(STAGED_DIR);
    fs::create_dir_all(&staged_dir).map_err(io_at(&staged_dir))?;
    for (index, op) in operations.iter().enumerate().skip(from) {
        let Operation::WriteFile { id, executable, .. } = op else {
            continue;
        };
        let staged = staged_path(dir, index);
        // Staged files only appear under their final name once complete.
        if staged.exists() {
            continue;
        }
        let partial = staged.with_extension("partial");
        let file = File::create(&partial).map_err(io_at(&partial))?;
        let mut writer = BufWriter::new(file);
        let result = store
            .read_into(id, &mut writer)
            .map_err(RecoveryError::from)
            .and_then(|_| {
                let file = writer
                    .into_inner()
                    .map_err(|e| io_at(&partial)(e.into_error()))?;
                set_executable(&file, *executable).map_err(io_at(&partial))?;
                file.sync_all().map_err(io_at(&partial))
            });
        if let Err(e) = result {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        rename_with_retry(&partial, &staged).map_err(io_at(&staged))?;
    }
    Ok(())
}

/// Applies one operation. Every operation is idempotent, so a resumed restore can safely
/// repeat the one that was in flight when it was interrupted.
fn apply_operation(
    root: &Path,
    dir: &Path,
    index: usize,
    op: &Operation,
) -> Result<(), RecoveryError> {
    let path = op.path();
    let live = path.to_fs_path(root);
    ensure_real_ancestors(root, path)?;
    match op {
        Operation::Remove { .. } => remove_entry(&live).map_err(io_at(&live)),
        Operation::CreateDir { .. } => match fs::create_dir(&live) {
            Ok(()) => Ok(()),
            Err(e)
                if e.kind() == io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&live).is_ok_and(|m| m.is_dir()) =>
            {
                Ok(())
            }
            Err(e) => Err(io_at(&live)(e)),
        },
        Operation::WriteFile { .. } => {
            if fs::symlink_metadata(&live).is_ok_and(|m| m.is_dir()) {
                return Err(RecoveryError::UnsafePath {
                    path: path.clone(),
                    reason: "a directory is in the way",
                });
            }
            let staged = staged_path(dir, index);
            rename_with_retry(&staged, &live).map_err(io_at(&live))
        }
        Operation::CreateSymlink { target, .. } => {
            if fs::read_link(&live).is_ok_and(|existing| existing == Path::new(target)) {
                return Ok(());
            }
            create_symlink(&live, target).map_err(io_at(&live))
        }
    }
}

/// Refuses to operate through a symlink or anything else that is not a real directory, so a
/// restore can never be redirected outside the project.
fn ensure_real_ancestors(root: &Path, path: &RelPath) -> Result<(), RecoveryError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut current = root.to_path_buf();
    for component in parent.as_str().split('/') {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(RecoveryError::UnsafePath {
                    path: path.clone(),
                    reason: "a parent is not a real directory (possibly a symlink)",
                });
            }
            // The operation itself will fail with a clear error.
            Err(_) => return Ok(()),
        }
    }
    Ok(())
}

fn remove_entry(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        // Windows directory symlinks and junctions are removed as directories.
        fs::remove_file(path).or_else(|_| fs::remove_dir(path))
    } else if meta.is_dir() {
        // Deliberately not recursive: everything inside was planned as its own removal.
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(unix)]
fn create_symlink(link: &Path, target: &str) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_symlink(link: &Path, target: &str) -> io::Result<()> {
    // Windows needs to know whether a link points at a directory.
    let resolved = link.parent().unwrap_or(link).join(target);
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(unix)]
fn set_executable(file: &File, executable: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    file.set_permissions(fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_executable(_file: &File, _executable: bool) -> io::Result<()> {
    Ok(())
}

fn completed_operations(progress: &Path, total: usize) -> Result<usize, RecoveryError> {
    let text = match fs::read_to_string(progress) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(io_at(progress)(e)),
    };
    // A crash can leave a partly written final line; the operation it names is repeated.
    let mut done = 0;
    for line in text.lines() {
        match line.parse::<usize>() {
            Ok(index) if index == done && index < total => done += 1,
            _ if !text.ends_with('\n') && text.ends_with(line) => break,
            _ => {
                return Err(RecoveryError::InvalidJournal(format!(
                    "bad progress line {line:?}"
                )));
            }
        }
    }
    Ok(done)
}

fn write_durably(path: &Path, contents: &[u8]) -> Result<(), RecoveryError> {
    let temp = path.with_extension("tmp");
    let mut file = File::create(&temp).map_err(io_at(&temp))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(io_at(&temp))?;
    drop(file);
    rename_with_retry(&temp, path).map_err(io_at(path))
}

/// Antivirus scanners and indexers briefly hold new files open on Windows, which makes
/// renames fail with "access denied"; retry for a short while.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut delay = Duration::from_millis(5);
    for _ in 1..RENAME_ATTEMPTS {
        match fs::rename(from, to) {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                thread::sleep(delay);
                delay *= 2;
            }
            result => return result,
        }
    }
    fs::rename(from, to)
}
