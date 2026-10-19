//! Content-addressed object store for Project Fabric.
//!
//! Objects are identified by the BLAKE3 hash of their uncompressed content, stored
//! zstd-compressed, written through a temp-file-and-rename protocol, and verified against
//! their identity on every read. See `docs/STORAGE_FORMAT.md` for the on-disk format.

mod error;
mod id;
mod store;

pub use error::{CorruptReason, StoreError};
pub use id::ObjectId;
pub use store::{PutOutcome, PutResult, Store, StoreStats};

pub type Result<T, E = StoreError> = std::result::Result<T, E>;
