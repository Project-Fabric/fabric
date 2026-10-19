//! Lets a capture skip reading files that have not changed since the last capture.
//!
//! Like Git's index, the cache remembers each file's size, modification time and (on Unix)
//! inode and change time, together with the content id computed when it was last read. A file
//! whose stat information still matches is not read again.
//!
//! The one way a stat match can lie is a change within the filesystem's timestamp
//! resolution: a file rewritten in the same tick as the read that cached it keeps the same
//! modification time. Files modified less than [`RACY_WINDOW`] before the capture began are
//! therefore never cached, so they are always re-read until their timestamp is safely in the
//! past. The cache is only an optimization: a missing, unreadable or corrupt cache file just
//! means files are read again.

use std::collections::HashMap;
use std::fs::{self, File, Metadata};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fabric_store::ObjectId;
use serde::{Deserialize, Serialize};

use crate::RelPath;

/// Longer than the coarsest timestamp resolution in common use (FAT's 2 seconds).
pub const RACY_WINDOW: Duration = Duration::from_secs(3);
const FORMAT: &str = "fabric-stat-cache";
const VERSION: u32 = 1;

#[derive(Debug, Default)]
pub struct StatCache {
    entries: HashMap<RelPath, Stat>,
}

/// What must still be true of a file for its cached content id to be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stat {
    size: u64,
    modified_ns: u128,
    /// Unix inode and status-change time; zero elsewhere.
    inode: u64,
    changed_ns: i128,
    id: ObjectId,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    format: String,
    version: u32,
    entries: Vec<(RelPath, Stat)>,
}

impl StatCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a cache saved by [`save`](Self::save). Anything unreadable yields an empty cache.
    pub fn load(path: &Path) -> Self {
        let parsed = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CacheFile>(&bytes).ok())
            .filter(|file| file.format == FORMAT && file.version == VERSION);
        Self {
            entries: parsed
                .map(|file| file.entries.into_iter().collect())
                .unwrap_or_default(),
        }
    }

    /// Saves the cache atomically. Failure is harmless (the next capture reads files again),
    /// so it is reported but never needs handling.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut entries: Vec<_> = self.entries.iter().map(|(p, s)| (p.clone(), *s)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let file = CacheFile {
            format: FORMAT.to_owned(),
            version: VERSION,
            entries,
        };
        let bytes = serde_json::to_vec(&file).expect("stat caches always serialize");
        let temp = path.with_extension("tmp");
        let mut out = File::create(&temp)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&temp, path)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The cached content id for `path`, if its current metadata still matches.
    pub(crate) fn lookup(&self, path: &RelPath, meta: &Metadata) -> Option<(ObjectId, u64)> {
        let cached = self.entries.get(path)?;
        let current = stat(meta, cached.id)?;
        (current == *cached).then_some((cached.id, cached.size))
    }

    /// Remembers `path`'s content id, unless its timestamp is too recent to be trusted.
    pub(crate) fn remember(
        &mut self,
        path: RelPath,
        meta: &Metadata,
        id: ObjectId,
        capture_started: SystemTime,
    ) {
        let settled = meta.modified().ok().is_some_and(|modified| {
            capture_started
                .duration_since(modified)
                .is_ok_and(|age| age > RACY_WINDOW)
        });
        if let (true, Some(stat)) = (settled, stat(meta, id)) {
            self.entries.insert(path, stat);
        }
    }
}

fn stat(meta: &Metadata, id: ObjectId) -> Option<Stat> {
    let modified_ns = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let (inode, changed_ns) = unix_identity(meta);
    Some(Stat {
        size: meta.len(),
        modified_ns,
        inode,
        changed_ns,
        id,
    })
}

#[cfg(unix)]
fn unix_identity(meta: &Metadata) -> (u64, i128) {
    use std::os::unix::fs::MetadataExt;
    (
        meta.ino(),
        i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()),
    )
}

#[cfg(not(unix))]
fn unix_identity(_meta: &Metadata) -> (u64, i128) {
    (0, 0)
}
