use std::io;
use std::path::PathBuf;

use fabric_snapshot::{RelPath, SnapshotError};
use fabric_store::{ObjectId, StoreError};

use crate::Conflict;

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error("restore blocked by {} conflict(s); nothing was changed", .0.len())]
    Conflicts(Vec<Conflict>),

    #[error(
        "the project changed after the restore was planned (planned from {planned}, now {current}); plan again"
    )]
    ProjectChanged {
        planned: ObjectId,
        current: ObjectId,
    },

    #[error("an interrupted restore is pending in {path}; resume it before starting another")]
    RestorePending { path: PathBuf },

    #[error(
        "restore finished but the project does not match {target} ({reason}); the pre-restore state is snapshot {undo_snapshot}"
    )]
    VerificationFailed {
        target: ObjectId,
        undo_snapshot: ObjectId,
        reason: String,
    },

    #[error("refusing to touch {path}: {reason}")]
    UnsafePath { path: RelPath, reason: &'static str },

    #[error("restore journal is invalid: {0}")]
    InvalidJournal(String),

    #[error("restore interrupted before operation {0} (simulated)")]
    Interrupted(usize),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(crate) fn io_at(path: &std::path::Path) -> impl FnOnce(io::Error) -> RecoveryError + use<> {
    let path = path.to_path_buf();
    move |source| RecoveryError::Io { path, source }
}
