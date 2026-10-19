use std::io;
use std::path::PathBuf;

use fabric_store::{ObjectId, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error("{path} is not a directory")]
    NotADirectory { path: PathBuf },

    #[error("the store at {store} is inside the project at {project}; it must live outside")]
    StoreInsideProject { store: PathBuf, project: PathBuf },

    #[error("manifest{} is invalid: {reason}", describe_id(.id))]
    InvalidManifest {
        id: Option<ObjectId>,
        reason: String,
    },

    #[error("failed to walk the project: {0}")]
    Walk(String),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn describe_id(id: &Option<ObjectId>) -> String {
    id.map(|id| format!(" {id}")).unwrap_or_default()
}
