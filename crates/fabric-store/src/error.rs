use std::io;
use std::path::PathBuf;

use crate::ObjectId;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("object {0} is missing from the store")]
    Missing(ObjectId),

    #[error("object {id} is corrupt: {reason}")]
    Corrupt { id: ObjectId, reason: CorruptReason },

    #[error("invalid object id {0:?}; expected blake3:<64 lowercase hex chars>")]
    InvalidId(String),

    #[error("{path} is not a Fabric store")]
    NotAStore { path: PathBuf },

    #[error("store at {path} is in use by another Fabric process")]
    Locked { path: PathBuf },

    #[error("failed to read source content: {0}")]
    SourceRead(#[source] io::Error),

    #[error("failed to write object content to its destination: {0}")]
    SinkWrite(#[source] io::Error),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Why a stored object failed verification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorruptReason {
    #[error("invalid header: {0}")]
    InvalidHeader(&'static str),

    #[error("unsupported object format version {0}")]
    UnsupportedVersion(u8),

    #[error("payload could not be decoded: {0}")]
    Decode(String),

    #[error("header records {expected} bytes but payload decoded to {actual}")]
    LengthMismatch { expected: u64, actual: u64 },

    #[error("content hashes to {actual}")]
    HashMismatch { actual: ObjectId },
}
