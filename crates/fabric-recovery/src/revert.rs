use std::collections::{BTreeMap, BTreeSet};

use fabric_snapshot::{Entry, Manifest, RelPath};
use serde::Serialize;

use crate::RecoveryError;

/// The project state that results from undoing one step while keeping everything after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertOutcome {
    /// The state to restore to. Only meaningful when `conflicts` is empty.
    pub manifest: Manifest,
    /// Paths the revert changes back.
    pub reverted: Vec<RelPath>,
    /// Paths the step changed that are already back to how they were before it.
    pub already_reverted: Vec<RelPath>,
    /// Paths that cannot be reverted without discarding later work.
    pub conflicts: Vec<RevertConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevertConflict {
    pub path: RelPath,
    pub reason: RevertConflictReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevertConflictReason {
    /// A later step changed this path again, so undoing the step would discard that work.
    ChangedLater,
    /// The step created this directory, but later steps put things in it.
    DirectoryInUse,
    /// A path the revert must recreate is now occupied by a file where a directory belongs.
    ParentIsNotADirectory,
}

/// Undoes the step that turned `before` into `after`, applied to `current`.
///
/// Works path by path: a path the step changed is put back only if it is still exactly as the
/// step left it. If anything changed it since, that path is a conflict — Fabric never merges
/// or guesses. Paths the step did not touch keep their current state.
pub fn revert_step(
    before: &Manifest,
    after: &Manifest,
    current: &Manifest,
) -> Result<RevertOutcome, RecoveryError> {
    let mut entries: BTreeMap<RelPath, Entry> = current
        .entries()
        .iter()
        .map(|e| (e.path().clone(), e.clone()))
        .collect();
    let changed: BTreeSet<&RelPath> = before
        .entries()
        .iter()
        .chain(after.entries())
        .map(Entry::path)
        .filter(|path| before.get(path) != after.get(path))
        .collect();

    let mut reverted = Vec::new();
    let mut already_reverted = Vec::new();
    let mut conflicts = Vec::new();
    let mut removed_dirs = Vec::new();

    for path in changed {
        let (old, new, now) = (before.get(path), after.get(path), current.get(path));
        if now == old {
            already_reverted.push(path.clone());
        } else if now != new {
            conflicts.push(RevertConflict {
                path: path.clone(),
                reason: RevertConflictReason::ChangedLater,
            });
        } else {
            match old {
                Some(entry) => {
                    entries.insert(path.clone(), entry.clone());
                }
                None => {
                    if matches!(new, Some(Entry::Dir { .. })) {
                        removed_dirs.push(path.clone());
                    }
                    entries.remove(path);
                }
            }
            reverted.push(path.clone());
        }
    }

    // A directory the step created stays if later work lives inside it.
    for dir in removed_dirs {
        let prefix = format!("{dir}/");
        if entries.keys().any(|p| p.as_str().starts_with(&prefix)) {
            entries.insert(dir.clone(), Entry::Dir { path: dir.clone() });
            reverted.retain(|p| p != &dir);
            conflicts.push(RevertConflict {
                path: dir,
                reason: RevertConflictReason::DirectoryInUse,
            });
        }
    }

    // Recreated paths need their parent directories, which a later step may have removed.
    let paths: Vec<RelPath> = entries.keys().cloned().collect();
    for path in paths {
        let mut parent = path.parent();
        while let Some(dir) = parent {
            match entries.get(&dir) {
                Some(Entry::Dir { .. }) => break,
                Some(_) => {
                    conflicts.push(RevertConflict {
                        path: path.clone(),
                        reason: RevertConflictReason::ParentIsNotADirectory,
                    });
                    break;
                }
                None => {
                    entries.insert(dir.clone(), Entry::Dir { path: dir.clone() });
                }
            }
            parent = dir.parent();
        }
    }

    if !conflicts.is_empty() {
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
        conflicts.dedup();
        return Ok(RevertOutcome {
            manifest: current.clone(),
            reverted,
            already_reverted,
            conflicts,
        });
    }
    Ok(RevertOutcome {
        manifest: Manifest::new(entries.into_values().collect())?,
        reverted,
        already_reverted,
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use fabric_store::ObjectId;

    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    /// Builds a manifest from `path=content` files and `path/` directories.
    fn manifest(spec: &[&str]) -> Manifest {
        let entries = spec
            .iter()
            .map(|item| match item.strip_suffix('/') {
                Some(dir) => Entry::Dir { path: rel(dir) },
                None => {
                    let (path, content) = item.split_once('=').unwrap();
                    Entry::File {
                        path: rel(path),
                        id: ObjectId::of(content.as_bytes()),
                        size: content.len() as u64,
                        executable: false,
                    }
                }
            })
            .collect();
        Manifest::new(entries).unwrap()
    }

    #[test]
    fn undoes_one_step_and_keeps_later_work() {
        let before = manifest(&["a=1", "c=x"]);
        let after = manifest(&["a=2", "b=new", "c=x"]);
        // Later steps edited c and added d.
        let current = manifest(&["a=2", "b=new", "c=y", "d=later"]);

        let outcome = revert_step(&before, &after, &current).unwrap();
        assert!(outcome.conflicts.is_empty());
        assert_eq!(outcome.manifest, manifest(&["a=1", "c=y", "d=later"]));
        assert_eq!(outcome.reverted, [rel("a"), rel("b")]);
    }

    #[test]
    fn a_later_change_to_the_same_path_is_a_conflict() {
        let before = manifest(&["a=1"]);
        let after = manifest(&["a=2"]);
        let current = manifest(&["a=3"]);
        let outcome = revert_step(&before, &after, &current).unwrap();
        assert_eq!(
            outcome.conflicts,
            [RevertConflict {
                path: rel("a"),
                reason: RevertConflictReason::ChangedLater
            }]
        );
        assert_eq!(outcome.manifest, current, "nothing changes on conflict");
    }

    #[test]
    fn recreates_deleted_files_and_their_directories() {
        let before = manifest(&["old/", "old/f=keep me", "x=1"]);
        let after = manifest(&["x=1"]);
        let current = manifest(&["x=2"]);
        let outcome = revert_step(&before, &after, &current).unwrap();
        assert_eq!(
            outcome.manifest,
            manifest(&["old/", "old/f=keep me", "x=2"])
        );
    }

    #[test]
    fn a_created_directory_that_later_work_uses_is_a_conflict() {
        let before = manifest(&["a=1"]);
        let after = manifest(&["a=1", "gen/", "gen/one=1"]);
        let current = manifest(&["a=1", "gen/", "gen/one=1", "gen/two=2"]);
        let outcome = revert_step(&before, &after, &current).unwrap();
        assert_eq!(outcome.conflicts.len(), 1);
        assert_eq!(outcome.conflicts[0].path, rel("gen"));
        assert_eq!(
            outcome.conflicts[0].reason,
            RevertConflictReason::DirectoryInUse
        );
    }

    #[test]
    fn paths_already_back_to_their_old_state_are_reported_not_touched() {
        let before = manifest(&["a=1", "b=1"]);
        let after = manifest(&["a=2", "b=2"]);
        let current = manifest(&["a=1", "b=2"]);
        let outcome = revert_step(&before, &after, &current).unwrap();
        assert_eq!(outcome.already_reverted, [rel("a")]);
        assert_eq!(outcome.reverted, [rel("b")]);
        assert_eq!(outcome.manifest, manifest(&["a=1", "b=1"]));
    }
}
