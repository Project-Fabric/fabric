use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use fabric_snapshot::{Entry, Manifest, RESTORE_DIR, RelPath, capture};
use fabric_store::{ObjectId, Store, StoreError};
use serde::{Deserialize, Serialize};

use crate::error::io_at;
use crate::{RecoveryError, portability};

/// Everything a restore would do, computed against a snapshot of the current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestorePlan {
    /// The snapshot being restored.
    pub target: ObjectId,
    /// A snapshot of the project as it was when planned. Applying the plan requires the
    /// project to still be in exactly this state, and restoring this snapshot undoes the
    /// restore.
    pub base: ObjectId,
    /// In execution order: removals deepest-first, then creations shallowest-first.
    pub operations: Vec<Operation>,
    /// Anything that would make the restore lose data or produce the wrong result. A plan
    /// with conflicts cannot be applied.
    pub conflicts: Vec<Conflict>,
    /// Directories the target does not have that are kept anyway, because they hold files
    /// Fabric never recorded (for example, ignored ones). Their recorded contents are still
    /// removed; the unrecorded files are never touched.
    pub kept: Vec<RelPath>,
    pub summary: RestoreSummary,
    /// Whether applying would delete or overwrite anything in the project.
    pub destructive: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RestoreSummary {
    pub create: usize,
    pub modify: usize,
    pub delete: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Removes a file, symlink or empty directory. Never removes a directory recursively.
    Remove {
        path: RelPath,
    },
    CreateDir {
        path: RelPath,
    },
    /// Writes a file atomically from staged, verified content.
    WriteFile {
        path: RelPath,
        id: ObjectId,
        size: u64,
        executable: bool,
    },
    CreateSymlink {
        path: RelPath,
        target: String,
    },
}

impl Operation {
    pub fn path(&self) -> &RelPath {
        match self {
            Operation::Remove { path }
            | Operation::CreateDir { path }
            | Operation::WriteFile { path, .. }
            | Operation::CreateSymlink { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub path: String,
    #[serde(flatten)]
    pub kind: ConflictKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "conflict", rename_all = "snake_case")]
pub enum ConflictKind {
    /// Something not in the current snapshot (ignored, unreadable or otherwise uncaptured)
    /// exists where the restore needs to write.
    UntrackedWouldBeOverwritten,
    /// A directory the restore must replace with a file or symlink contains something not in
    /// the current snapshot.
    UntrackedInRemovedDirectory,
    /// The name cannot be created faithfully on this platform.
    UnrepresentableName { reason: &'static str },
    /// Two target paths differ only by letter case, which this platform cannot hold apart.
    CaseCollision { other: String },
}

/// Plans restoring the project at `root` to snapshot `target`.
///
/// This captures the current project into the store (that capture is the plan's `base` and
/// the undo point) but does not modify the project.
pub fn plan_restore(
    store: &Store,
    root: &Path,
    target: &ObjectId,
) -> Result<RestorePlan, RecoveryError> {
    let root = fs::canonicalize(root).map_err(io_at(root))?;
    let restore_dir = root.join(RESTORE_DIR);
    if restore_dir.exists() {
        return Err(RecoveryError::RestorePending { path: restore_dir });
    }

    let target_manifest = Manifest::load(store, target)?;
    // Content is fully verified while staging, before the project is touched; here we only
    // refuse plans that could not possibly be carried out.
    for entry in target_manifest.entries() {
        if let Entry::File { id, .. } = entry
            && !store.contains(id)
        {
            return Err(StoreError::Missing(*id).into());
        }
    }

    let current = capture(store, &root)?;
    let base = &current.manifest;
    let (operations, kept) = keep_directories_in_use(
        &root,
        base,
        &target_manifest,
        operations(base, &target_manifest),
    )?;

    let mut conflicts = untracked_conflicts(&root, base, &target_manifest, &operations)?;
    conflicts.extend(portability::conflicts(&target_manifest));

    let mut summary = summarize(base, &target_manifest);
    summary.delete -= kept.len();
    Ok(RestorePlan {
        target: *target,
        base: current.id,
        destructive: summary.modify > 0 || summary.delete > 0,
        operations,
        conflicts,
        kept,
        summary,
    })
}

/// Directories the target does not have, but which hold files Fabric never recorded, are kept
/// rather than reported as conflicts: their recorded contents are still removed, and the
/// unrecorded files stay exactly where they are. A directory that is ignored-content-only
/// (such as `.claude/` holding a local settings file) is common, and blocking every restore on
/// it would make restores unusable. Parents of kept directories are kept too.
fn keep_directories_in_use(
    root: &Path,
    base: &Manifest,
    target: &Manifest,
    operations: Vec<Operation>,
) -> Result<(Vec<Operation>, Vec<RelPath>), RecoveryError> {
    let removed_dirs: BTreeSet<&RelPath> = operations
        .iter()
        .filter_map(|op| match op {
            Operation::Remove { path }
                if matches!(base.get(path), Some(Entry::Dir { .. }))
                    && target.get(path).is_none() =>
            {
                Some(path)
            }
            _ => None,
        })
        .collect();

    let mut kept = BTreeSet::new();
    for &dir in &removed_dirs {
        if !untracked_children(root, base, dir)?.is_empty() {
            let mut current = Some(dir.clone());
            while let Some(path) = current {
                if !removed_dirs.contains(&path) {
                    break;
                }
                current = path.parent();
                kept.insert(path);
            }
        }
    }
    let operations = operations
        .into_iter()
        .filter(|op| !matches!(op, Operation::Remove { path } if kept.contains(path)))
        .collect();
    Ok((operations, kept.into_iter().collect()))
}

/// Entries inside the live directory `dir` that the snapshot `base` did not record.
fn untracked_children(
    root: &Path,
    base: &Manifest,
    dir: &RelPath,
) -> Result<Vec<String>, RecoveryError> {
    let live = dir.to_fs_path(root);
    let entries = match fs::read_dir(&live) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_at(&live)(e)),
    };
    let mut untracked = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_at(&live))?;
        let child = format!("{dir}/{}", entry.file_name().to_string_lossy());
        let tracked = RelPath::new(child.clone())
            .ok()
            .is_some_and(|child| base.get(&child).is_some());
        if !tracked {
            untracked.push(child);
        }
    }
    Ok(untracked)
}

/// Whether an existing entry must be removed before the target entry can be created, rather
/// than overwritten in place.
fn needs_replacement(current: &Entry, target: &Entry) -> bool {
    match (current, target) {
        (Entry::Dir { .. }, Entry::Dir { .. }) | (Entry::File { .. }, Entry::File { .. }) => false,
        (Entry::Symlink { target: a, .. }, Entry::Symlink { target: b, .. }) => a != b,
        _ => true,
    }
}

fn operations(base: &Manifest, target: &Manifest) -> Vec<Operation> {
    let mut removals: Vec<Operation> = base
        .entries()
        .iter()
        .filter(|entry| {
            target
                .get(entry.path())
                .is_none_or(|t| needs_replacement(entry, t))
        })
        .map(|entry| Operation::Remove {
            path: entry.path().clone(),
        })
        .collect();
    // Manifests are sorted, so reversing puts children before their parents.
    removals.reverse();

    let creations = target.entries().iter().filter_map(|entry| {
        let needed = base
            .get(entry.path())
            .is_none_or(|current| current != entry);
        needed.then(|| match entry.clone() {
            Entry::Dir { path } => Operation::CreateDir { path },
            Entry::File {
                path,
                id,
                size,
                executable,
            } => Operation::WriteFile {
                path,
                id,
                size,
                executable,
            },
            Entry::Symlink { path, target } => Operation::CreateSymlink { path, target },
        })
    });
    removals.into_iter().chain(creations).collect()
}

/// Finds anything on disk that the current snapshot did not capture but the plan would
/// destroy: ignored or unreadable files in the way of a write, or inside a directory that
/// must become a file or symlink.
fn untracked_conflicts(
    root: &Path,
    base: &Manifest,
    target: &Manifest,
    operations: &[Operation],
) -> Result<Vec<Conflict>, RecoveryError> {
    let mut conflicts = Vec::new();
    for op in operations {
        let path = op.path();
        let live = path.to_fs_path(root);
        match op {
            Operation::Remove { .. } => {
                // Directories the target lacks were already kept if in use; only a directory
                // that must be replaced by something else can still lose untracked files.
                if !matches!(base.get(path), Some(Entry::Dir { .. })) || target.get(path).is_none()
                {
                    continue;
                }
                for child in untracked_children(root, base, path)? {
                    conflicts.push(Conflict {
                        path: child,
                        kind: ConflictKind::UntrackedInRemovedDirectory,
                    });
                }
            }
            _ if base.get(path).is_some() => {}
            _ => {
                let Ok(meta) = fs::symlink_metadata(&live) else {
                    continue;
                };
                let reusable_dir = matches!(op, Operation::CreateDir { .. }) && meta.is_dir();
                if !reusable_dir {
                    conflicts.push(Conflict {
                        path: path.to_string(),
                        kind: ConflictKind::UntrackedWouldBeOverwritten,
                    });
                }
            }
        }
    }
    Ok(conflicts)
}

fn summarize(base: &Manifest, target: &Manifest) -> RestoreSummary {
    let mut summary = RestoreSummary::default();
    for entry in target.entries() {
        match base.get(entry.path()) {
            None => summary.create += 1,
            Some(current) if current != entry => summary.modify += 1,
            Some(_) => {}
        }
    }
    summary.delete = base
        .entries()
        .iter()
        .filter(|entry| target.get(entry.path()).is_none())
        .count();
    summary
}
