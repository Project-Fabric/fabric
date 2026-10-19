use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};

use fabric_store::ObjectId;
use serde::Serialize;

use crate::{Entry, Manifest, RelPath};

/// The differences between two manifests. Every path appears in at most one list.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub added: Vec<RelPath>,
    pub removed: Vec<RelPath>,
    /// Same path, different content, kind, executable bit or symlink target.
    pub modified: Vec<RelPath>,
    /// A removed file and an added file with identical, non-empty content.
    pub renamed: Vec<Rename>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rename {
    pub from: RelPath,
    pub to: RelPath,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.modified.is_empty()
            && self.renamed.is_empty()
    }
}

pub fn diff(old: &Manifest, new: &Manifest) -> Diff {
    let mut diff = Diff::default();
    let (mut old_iter, mut new_iter) = (
        old.entries().iter().peekable(),
        new.entries().iter().peekable(),
    );
    loop {
        match (old_iter.peek(), new_iter.peek()) {
            (None, None) => break,
            (Some(o), None) => {
                diff.removed.push(o.path().clone());
                old_iter.next();
            }
            (None, Some(n)) => {
                diff.added.push(n.path().clone());
                new_iter.next();
            }
            (Some(o), Some(n)) => match o.path().cmp(n.path()) {
                Ordering::Less => {
                    diff.removed.push(o.path().clone());
                    old_iter.next();
                }
                Ordering::Greater => {
                    diff.added.push(n.path().clone());
                    new_iter.next();
                }
                Ordering::Equal => {
                    if o != n {
                        diff.modified.push(n.path().clone());
                    }
                    old_iter.next();
                    new_iter.next();
                }
            },
        }
    }
    detect_renames(&mut diff, old, new);
    diff
}

/// Pairs removed and added files with the same content. Empty files are never paired: any
/// two are identical, so a pairing would be arbitrary.
fn detect_renames(diff: &mut Diff, old: &Manifest, new: &Manifest) {
    let mut removed_by_content: HashMap<ObjectId, VecDeque<RelPath>> = HashMap::new();
    for path in &diff.removed {
        if let Some(id) = file_content(old, path) {
            removed_by_content
                .entry(id)
                .or_default()
                .push_back(path.clone());
        }
    }
    if removed_by_content.is_empty() {
        return;
    }
    let mut renamed = Vec::new();
    for path in &diff.added {
        if let Some(id) = file_content(new, path)
            && let Some(from) = removed_by_content
                .get_mut(&id)
                .and_then(VecDeque::pop_front)
        {
            renamed.push(Rename {
                from,
                to: path.clone(),
            });
        }
    }
    diff.removed
        .retain(|path| !renamed.iter().any(|r| &r.from == path));
    diff.added
        .retain(|path| !renamed.iter().any(|r| &r.to == path));
    diff.renamed = renamed;
}

fn file_content(manifest: &Manifest, path: &RelPath) -> Option<ObjectId> {
    match manifest.get(path) {
        Some(Entry::File { id, size, .. }) if *size > 0 => Some(*id),
        _ => None,
    }
}
