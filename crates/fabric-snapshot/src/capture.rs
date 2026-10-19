use std::collections::HashSet;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::SystemTime;

use fabric_store::{ObjectId, PutOutcome, PutResult, Store, StoreError};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use serde::Serialize;

use crate::{Entry, Manifest, PathError, RelPath, SnapshotError, StatCache};

/// Project-specific exclusions, in `.gitignore` syntax, read in addition to `.gitignore`.
pub const IGNORE_FILE: &str = ".fabricignore";

/// Where an in-progress restore keeps its journal and staged files, at the project root.
pub const RESTORE_DIR: &str = ".fabric-restore";

/// Always excluded: Git's own database (Git is its own recovery system), Fabric's restore
/// staging area, and common secret files, which must never be copied into the store.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".git/",
    "/.fabric-restore/",
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

/// Files up to this size are read into memory whole; larger ones are streamed.
const IN_MEMORY_LIMIT: u64 = 8 * 1024 * 1024;

/// Threads reading files in parallel. Bounded so memory stays at most a few files' worth.
const MAX_WORKERS: usize = 8;

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
    /// Files not read at all because the stat cache showed them unchanged.
    pub cache_hits: u64,
}

/// Captures the project at `root` into `store` without modifying the project.
///
/// Respects `.gitignore`, `.git/info/exclude` and [`IGNORE_FILE`] (but not the user's global
/// Git excludes, so results don't depend on per-machine configuration), always applies
/// [`DEFAULT_EXCLUDES`], and never follows symlinks.
pub fn capture(store: &Store, root: &Path) -> Result<Snapshot, SnapshotError> {
    capture_inner(store, root, None)
}

/// Like [`capture`], but skips reading files the cache shows are unchanged, and updates the
/// cache for next time. Produces exactly the same snapshot as [`capture`].
pub fn capture_cached(
    store: &Store,
    root: &Path,
    cache: &mut StatCache,
) -> Result<Snapshot, SnapshotError> {
    capture_inner(store, root, Some(cache))
}

fn capture_inner(
    store: &Store,
    root: &Path,
    cache: Option<&mut StatCache>,
) -> Result<Snapshot, SnapshotError> {
    let started = SystemTime::now();
    let mut next_cache = StatCache::new();
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
    let mut files = Vec::new();

    // Walk first, then read files in parallel: storing new content is dominated by waiting
    // for the disk, which overlaps well across threads.
    for result in build_walker(&root)? {
        let walk_entry = result.map_err(|e| SnapshotError::Walk(e.to_string()))?;
        if walk_entry.depth() == 0 {
            continue;
        }
        let path = walk_entry.path();
        let mut skip = |reason| skipped.push(skipped_entry(&root, path, reason));

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
            files.push((rel, path.to_path_buf()));
        } else {
            skip(SkipReason::UnsupportedFileType);
        }
    }

    let outcomes = capture_files(store, &files, cache.as_deref())?;
    // Two threads storing identical files at once can both report `Stored` (on Unix the
    // second rename harmlessly replaces the first), so count distinct new content.
    let mut stored = HashSet::new();
    for ((rel, path), outcome) in files.into_iter().zip(outcomes) {
        match outcome {
            Ok(captured) => {
                stats.files += 1;
                stats.bytes += captured.put.size;
                stats.cache_hits += u64::from(captured.from_cache);
                match captured.put.outcome {
                    PutOutcome::Stored if stored.insert(captured.put.id) => stats.new_objects += 1,
                    _ => stats.reused_objects += 1,
                }
                if cache.is_some() {
                    next_cache.remember(rel.clone(), &captured.meta, captured.put.id, started);
                }
                entries.push(Entry::File {
                    path: rel,
                    id: captured.put.id,
                    size: captured.put.size,
                    executable: is_executable(&captured.meta),
                });
            }
            Err(reason) => skipped.push(skipped_entry(&root, &path, reason)),
        }
    }

    let manifest = Manifest::new(entries)?;
    let id = manifest.store(store)?;
    // Paths that no longer exist drop out of the cache.
    if let Some(cache) = cache {
        *cache = next_cache;
    }
    Ok(Snapshot {
        id,
        manifest,
        skipped,
        stats,
    })
}

/// A skipped path, shown relative to the root. Components are joined rather than separators
/// replaced: on Unix, `\` is a legal character inside a name, not a separator.
fn skipped_entry(root: &Path, path: &Path, reason: SkipReason) -> Skipped {
    let shown = path.strip_prefix(root).unwrap_or(path);
    let shown: Vec<_> = shown
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    Skipped {
        path: shown.join("/"),
        reason,
    }
}

struct CapturedFile {
    put: PutResult,
    meta: Metadata,
    from_cache: bool,
}

/// Captures `files` on a small pool of threads, returning outcomes in the same order.
fn capture_files(
    store: &Store,
    files: &[(RelPath, PathBuf)],
    cache: Option<&StatCache>,
) -> Result<Vec<Result<CapturedFile, SkipReason>>, SnapshotError> {
    let workers = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, MAX_WORKERS)
        .min(files.len().max(1));
    let next = AtomicUsize::new(0);
    let mut outcomes: Vec<Option<_>> = (0..files.len()).map(|_| None).collect();
    thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((rel, path)) = files.get(index) else {
                            break;
                        };
                        done.push((index, capture_one(store, rel, path, cache)));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            for (index, outcome) in handle.join().expect("capture worker panicked") {
                outcomes[index] = Some(outcome);
            }
        }
    });
    outcomes
        .into_iter()
        .map(|outcome| outcome.expect("every file is captured exactly once"))
        .collect()
}

fn capture_one(
    store: &Store,
    rel: &RelPath,
    path: &Path,
    cache: Option<&StatCache>,
) -> Result<Result<CapturedFile, SkipReason>, SnapshotError> {
    // A cache hit is only trusted if the content it points to is still stored.
    if let Some(cache) = cache
        && let Ok(meta) = fs::symlink_metadata(path)
        && let Some((id, size)) = cache.lookup(rel, &meta)
        && store.contains(&id)
    {
        let put = PutResult {
            id,
            size,
            outcome: PutOutcome::AlreadyPresent,
        };
        return Ok(Ok(CapturedFile {
            put,
            meta,
            from_cache: true,
        }));
    }
    Ok(capture_file(store, path)?.map(|(put, meta)| CapturedFile {
        put,
        meta,
        from_cache: false,
    }))
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
) -> Result<Result<(PutResult, Metadata), SkipReason>, SnapshotError> {
    let unreadable = |e: io::Error| SkipReason::Unreadable {
        detail: e.to_string(),
    };
    for _ in 0..CAPTURE_ATTEMPTS {
        let before = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => return Ok(Err(unreadable(e))),
        };
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(e) => return Ok(Err(unreadable(e))),
        };
        // Small files are read whole and hashed before anything is written, so content the
        // store already has costs no disk writes at all. Large files are streamed.
        let put = if before.len() <= IN_MEMORY_LIMIT {
            let mut content = Vec::with_capacity(before.len() as usize);
            if let Err(e) = file.read_to_end(&mut content) {
                return Ok(Err(unreadable(e)));
            }
            store.put_bytes(&content)?
        } else {
            match store.put_reader(file) {
                Ok(put) => put,
                Err(StoreError::SourceRead(e)) => return Ok(Err(unreadable(e))),
                Err(e) => return Err(e.into()),
            }
        };
        let after = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => return Ok(Err(unreadable(e))),
        };
        if put.size == after.len() && unchanged(&before, &after) {
            return Ok(Ok((put, after)));
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
