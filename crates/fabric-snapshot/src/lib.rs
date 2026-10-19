//! Project snapshots for Project Fabric.
//!
//! A snapshot walks a project directory, stores every captured file in the object store, and
//! records the complete tree as an immutable manifest — itself an object, so a snapshot's id
//! is the id of its manifest. See `docs/STORAGE_FORMAT.md` for the manifest format.

mod capture;
mod diff;
mod error;
mod manifest;
mod path;

pub use capture::{
    CaptureStats, DEFAULT_EXCLUDES, IGNORE_FILE, SkipReason, Skipped, Snapshot, capture,
};
pub use diff::{Diff, Rename, diff};
pub use error::SnapshotError;
pub use manifest::{Entry, MANIFEST_FORMAT, MANIFEST_VERSION, Manifest};
pub use path::{PathError, RelPath};
