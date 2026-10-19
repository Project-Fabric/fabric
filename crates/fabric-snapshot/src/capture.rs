use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use fabric_store::{ObjectId, PutOutcome, PutResult, Store, StoreError};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use serde::Serialize;

use crate::{Entry, Manifest, PathError, RelPath, SnapshotError};

/// Project-specific exclusions, in `.gitignore` syntax, read in addition to `.gitignore`.
pub const IGNORE_FILE: &str = ".fabricignore";

/// Always excluded: Git's own database (Git is its own recovery system) and common secret
/// files, which must never be copied into the store.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".git/",
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "id_rsa",
    "id_rsa.*",
    "id_ed25519",
    "id_ed25519.*",
];

/// How many times to re-read a file that changes while it is being captured.
const CAPTURE_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub id: ObjectId,
    pub manifest: Manifest,
    /// Paths that exist but could not be captured. They are not part of the snapshot, so a
    /// restore of it cannot bring them back.
    pub skipped: Vec<Skipped>,
    pub stats: CaptureStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    /// Relative to the project root; lossy if the name is not UTF-8.
    pub path: String,
    #[serde(flatten)]
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipReason {
    NonUtf8Name,
    NonPortableName { detail: String },
    UnsupportedFileType,
    Unreadable { detail: String },
    ChangedDuringCapture,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CaptureStats {
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    /// Total uncompressed size of captured files.
    pub bytes: u64,
    /// Files whose content was not yet in the store.
    pub new_objects: u64,
    /// Files whose content was already stored and was reused.
    pub reused_objects: u64,
}

/// Captures the project at `root` into `store` without modifying the project.
///
/// Respects `.gitignore`, `.git/info/exclude` and [`IGNORE_FILE`] (but not the user's global
/// Git excludes, so results don't depend on per-machine configuration), always applies
/// [`DEFAULT_EXCLUDES`], and never follows symlinks.
pub fn capture(store: &Store, root: &Path) -> Result<Snapshot, SnapshotError> {
    let root = fs::canonicalize(root).map_err(io_at(root))?;
    if !root.is_dir() {
        return Err(SnapshotError::NotADirectory { path: root });
    }
    let store_root = fs::canonicalize(store.root()).map_err(io_at(store.root()))?;
    if store_root.starts_with(&root) {
        return Err(SnapshotError::StoreInsideProject {
            store: store_root,
            project: root,
        });
    }

    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut stats = CaptureStats::default();

    for result in build_walker(&root)? {
        let walk_entry = result.map_err(|e| SnapshotError::Walk(e.to_string()))?;
        if walk_entry.depth() == 0 {
            continue;
        }
        let path = walk_entry.path();
        let mut skip = |reason| {
            let shown = path.strip_prefix(&root).unwrap_or(path);
            skipped.push(Skipped {
                path: shown.to_string_lossy().replace('\\', "/"),
                reason,
            });
        };

        let rel = match RelPath::from_fs(&root, path) {
            Ok(rel) => rel,
            Err(PathError::NonUtf8(_)) => {
                skip(SkipReason::NonUtf8Name);
                continue;
            }
            Err(e) => {
                skip(SkipReason::NonPortableName {
                    detail: e.to_string(),
                });
                continue;
            }
        };
        let Some(file_type) = walk_entry.file_type() else {
            continue;
        };

        if file_type.is_symlink() {
            match fs::read_link(path) {
                Ok(target) => match target.to_str() {
                    Some(target) => {
                        stats.symlinks += 1;
                        entries.push(Entry::Symlink {
                            path: rel,
                            target: target.to_owned(),
                        });
                    }
                    None => skip(SkipReason::Unreadable {
                        detail: "symlink target is not valid UTF-8".to_owned(),
                    }),
                },
                Err(e) => skip(SkipReason::Unreadable {
                    detail: e.to_string(),
                }),
            }
        } else if file_type.is_dir() {
            stats.dirs += 1;
            entries.push(Entry::Dir { path: rel });
        } else if file_type.is_file() {
            match capture_file(store, path)? {
                Ok((put, executable)) => {
                    stats.files += 1;
                    stats.bytes += put.size;
                    match put.outcome {
                        PutOutcome::Stored => stats.new_objects += 1,
                        PutOutcome::AlreadyPresent => stats.reused_objects += 1,
                    }
                    entries.push(Entry::File {
                        path: rel,
                        id: put.id,
                        size: put.size,
                        executable,
                    });
                }
                Err(reason) => skip(reason),
            }
        } else {
            skip(SkipReason::UnsupportedFileType);
        }
    }

    let manifest = Manifest::new(entries)?;
    let id = manifest.store(store)?;
    Ok(Snapshot {
        id,
        manifest,
        skipped,
        stats,
    })
}

fn build_walker(root: &Path) -> Result<ignore::Walk, SnapshotError> {
    let walk_error = |e: ignore::Error| SnapshotError::Walk(e.to_string());
    let mut overrides = OverrideBuilder::new(root);
    for glob in DEFAULT_EXCLUDES {
        overrides.add(&format!("!{glob}")).map_err(walk_error)?;
    }
    Ok(WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .overrides(overrides.build().map_err(walk_error)?)
        .add_custom_ignore_filename(IGNORE_FILE)
        .build())
}

/// Stores one file's content. The file is re-read if its size or modification time changes
/// during the read, so a captured file is never a mix of two versions — within the limits of
/// the filesystem's timestamp resolution.
fn capture_file(
    store: &Store,
    path: &Path,
) -> Result<Result<(PutResult, bool), SkipReason>, SnapshotError> {
    let unreadable = |e: io::Error| SkipReason::Unreadable {
        detail: e.to_string(),
    };
    for _ in 0..CAPTURE_ATTEMPTS {
        let before = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => return Ok(Err(unreadable(e))),
        };
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) => return Ok(Err(unreadable(e))),
        };
        let put = match store.put_reader(file) {
            Ok(put) => put,
            Err(StoreError::SourceRead(e)) => return Ok(Err(unreadable(e))),
            Err(e) => return Err(e.into()),
        };
        let after = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => return Ok(Err(unreadable(e))),
        };
        if put.size == after.len() && unchanged(&before, &after) {
            return Ok(Ok((put, is_executable(&after))));
        }
    }
    Ok(Err(SkipReason::ChangedDuringCapture))
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}

#[cfg(unix)]
fn is_executable(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &Metadata) -> bool {
    false
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> SnapshotError + use<> {
    let path: PathBuf = path.to_path_buf();
    move |source| SnapshotError::Io { path, source }
}
