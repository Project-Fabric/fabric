use std::fmt;
use std::io;
use std::path::Path;

use fabric_recovery::RecoveryError;
use fabric_snapshot::SnapshotError;
use fabric_store::StoreError;

/// Process exit codes. Documented in `docs/CLI.md`; changing them is a breaking change.
pub mod exit {
    pub const OK: u8 = 0;
    pub const ERROR: u8 = 1;
    // 2 is clap's usage error.
    pub const NEEDS_CONFIRMATION: u8 = 3;
    pub const CONFLICTS: u8 = 4;
    pub const INTEGRITY: u8 = 5;
    pub const STATE_CHANGED: u8 = 6;
}

/// An error with a stable machine-readable `kind` and an exit code.
#[derive(Debug)]
pub struct CliError {
    pub code: u8,
    pub kind: &'static str,
    pub message: String,
}

impl CliError {
    pub fn new(code: u8, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            kind,
            message: message.into(),
        }
    }

    pub fn io(path: &Path, source: io::Error) -> Self {
        Self::new(
            exit::ERROR,
            "io",
            format!("{}: {source}", crate::home::display_path(path)),
        )
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<StoreError> for CliError {
    fn from(e: StoreError) -> Self {
        let (code, kind) = match e {
            StoreError::Corrupt { .. } => (exit::INTEGRITY, "corrupt_object"),
            StoreError::Missing(_) => (exit::INTEGRITY, "missing_object"),
            StoreError::Locked { .. } => (exit::ERROR, "store_busy"),
            _ => (exit::ERROR, "store"),
        };
        Self::new(code, kind, e.to_string())
    }
}

impl From<SnapshotError> for CliError {
    fn from(e: SnapshotError) -> Self {
        match e {
            SnapshotError::Store(e) => e.into(),
            SnapshotError::InvalidManifest { .. } => {
                Self::new(exit::INTEGRITY, "invalid_manifest", e.to_string())
            }
            _ => Self::new(exit::ERROR, "snapshot", e.to_string()),
        }
    }
}

impl From<RecoveryError> for CliError {
    fn from(e: RecoveryError) -> Self {
        match e {
            RecoveryError::Snapshot(inner) => inner.into(),
            RecoveryError::Store(inner) => inner.into(),
            e => {
                let (code, kind) = match &e {
                    RecoveryError::Conflicts(_) => (exit::CONFLICTS, "conflicts"),
                    RecoveryError::VerificationFailed { .. } => {
                        (exit::INTEGRITY, "verification_failed")
                    }
                    RecoveryError::ProjectChanged { .. } => {
                        (exit::STATE_CHANGED, "project_changed")
                    }
                    RecoveryError::RestorePending { .. } => {
                        (exit::STATE_CHANGED, "restore_pending")
                    }
                    _ => (exit::ERROR, "recovery"),
                };
                Self::new(code, kind, e.to_string())
            }
        }
    }
}
