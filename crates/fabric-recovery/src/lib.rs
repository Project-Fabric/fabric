//! Restoring a project to a snapshot, safely.
//!
//! A restore is split into a [`plan_restore`] step, which captures the current state (the
//! undo point) and lists every operation and conflict, and an [`apply_restore`] step, which
//! refuses to run if there are conflicts or the project changed since planning. Applying
//! stages and verifies all content first, journals each operation so an interrupted restore
//! can be finished with [`resume_restore`], and verifies the result. See
//! `docs/ARCHITECTURE.md` for the guarantees and their limits.

mod apply;
mod error;
mod plan;
mod portability;

pub use apply::{ApplyOptions, RestoreReport, apply_restore, apply_restore_with, resume_restore};
pub use error::RecoveryError;
pub use plan::{Conflict, ConflictKind, Operation, RestorePlan, RestoreSummary, plan_restore};
