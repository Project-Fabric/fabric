//! Names that cannot be restored faithfully on the current platform.

use std::collections::HashMap;

use fabric_snapshot::{Manifest, RelPath};

use crate::{Conflict, ConflictKind};

/// Device names Windows reserves in every directory, with or without an extension.
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

pub(crate) fn conflicts(target: &Manifest) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    if cfg!(windows) {
        for entry in target.entries() {
            if let Some(reason) = windows_name_problem(entry.path()) {
                conflicts.push(Conflict {
                    path: entry.path().to_string(),
                    kind: ConflictKind::UnrepresentableName { reason },
                });
            }
        }
    }
    // Windows and macOS filesystems are case-insensitive by default.
    if cfg!(any(windows, target_os = "macos")) {
        let mut seen: HashMap<String, &RelPath> = HashMap::new();
        for entry in target.entries() {
            let folded = entry.path().as_str().to_lowercase();
            if let Some(other) = seen.insert(folded, entry.path()) {
                conflicts.push(Conflict {
                    path: entry.path().to_string(),
                    kind: ConflictKind::CaseCollision {
                        other: other.to_string(),
                    },
                });
            }
        }
    }
    conflicts
}

fn windows_name_problem(path: &RelPath) -> Option<&'static str> {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    let stem = name.split('.').next().unwrap_or_default().to_lowercase();
    if WINDOWS_RESERVED.contains(&stem.trim_end()) {
        Some("reserved device name on Windows")
    } else if name.ends_with('.') || name.ends_with(' ') {
        Some("Windows strips trailing dots and spaces")
    } else if name.contains(['<', '>', '"', '|', '?', '*']) {
        Some("character not allowed in Windows file names")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_windows_reserved_and_invalid_names() {
        for bad in [
            "con",
            "CON.txt",
            "dir/nul",
            "lpt1.log",
            "aux.tar.gz",
            "trailing.",
            "q?.txt",
        ] {
            let path = RelPath::new(bad).unwrap();
            assert!(windows_name_problem(&path).is_some(), "{bad}");
        }
        for ok in ["console.txt", "connect", "a/b.rs", "com10", ".nul"] {
            let path = RelPath::new(ok).unwrap();
            assert!(windows_name_problem(&path).is_none(), "{ok}");
        }
    }
}
