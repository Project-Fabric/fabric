use fabric_store::{ObjectId, Store};
use serde::{Deserialize, Serialize};

use crate::{RelPath, SnapshotError};

pub const MANIFEST_FORMAT: &str = "fabric-manifest";
pub const MANIFEST_VERSION: u32 = 1;

/// The complete captured state of a project: every directory, file and symlink, sorted by
/// path. Manifests contain no timestamps or other metadata about the capture itself, so two
/// captures of an identical tree produce the same manifest and the same snapshot id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    format: String,
    version: u32,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entry {
    Dir {
        path: RelPath,
    },
    File {
        path: RelPath,
        id: ObjectId,
        size: u64,
        executable: bool,
    },
    /// Recorded, never followed. `target` is stored exactly as the link holds it.
    Symlink {
        path: RelPath,
        target: String,
    },
}

impl Entry {
    pub fn path(&self) -> &RelPath {
        match self {
            Entry::Dir { path } | Entry::File { path, .. } | Entry::Symlink { path, .. } => path,
        }
    }
}

impl Manifest {
    /// Builds a manifest from entries in any order.
    pub fn new(mut entries: Vec<Entry>) -> Result<Self, SnapshotError> {
        entries.sort_by(|a, b| a.path().cmp(b.path()));
        let manifest = Self {
            format: MANIFEST_FORMAT.to_owned(),
            version: MANIFEST_VERSION,
            entries,
        };
        manifest
            .validate()
            .map_err(|reason| SnapshotError::InvalidManifest { id: None, reason })?;
        Ok(manifest)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, path: &RelPath) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.path().cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Writes the manifest to the store and returns its id, which is the snapshot id.
    pub fn store(&self, store: &Store) -> Result<ObjectId, SnapshotError> {
        let bytes = serde_json::to_vec(self).expect("manifests always serialize");
        Ok(store.put_bytes(&bytes)?.id)
    }

    /// Reads, parses and validates a manifest. Anything malformed is an error.
    pub fn load(store: &Store, id: &ObjectId) -> Result<Self, SnapshotError> {
        let invalid = |reason| SnapshotError::InvalidManifest {
            id: Some(*id),
            reason,
        };
        let bytes = store.read(id)?;
        let manifest: Self = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        manifest.validate().map_err(invalid)?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), String> {
        if self.format != MANIFEST_FORMAT {
            return Err(format!("unknown format {:?}", self.format));
        }
        if self.version != MANIFEST_VERSION {
            return Err(format!("unsupported version {}", self.version));
        }
        for pair in self.entries.windows(2) {
            let (a, b) = (pair[0].path(), pair[1].path());
            if a >= b {
                return Err(format!("entries are unsorted or duplicated at {a} and {b}"));
            }
        }
        for entry in &self.entries {
            if let Some(parent) = entry.path().parent()
                && !matches!(self.get(&parent), Some(Entry::Dir { .. }))
            {
                return Err(format!(
                    "{} has no directory entry for its parent {parent}",
                    entry.path()
                ));
            }
            if let Entry::Symlink { path, target } = entry
                && (target.is_empty() || target.contains('\0'))
            {
                return Err(format!("symlink {path} has an invalid target"));
            }
        }
        Ok(())
    }
}
