//! Where Fabric keeps its data, and how projects are registered and found.
//!
//! Everything lives under one per-user directory (`FABRIC_HOME`, by default the platform's
//! local data directory + `fabric`), outside every project, so nothing an agent does inside a
//! project can damage the history of that project:
//!
//! ```text
//! <home>/store/                       shared content-addressed object store
//! <home>/projects/<id>/project.json   registration: id and root path
//! <home>/projects/<id>/checkpoints.jsonl
//! <home>/hook-errors.log              failures inside agent hooks, which must never block an agent
//! ```

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fabric_snapshot::{Snapshot, StatCache, capture_cached};
use fabric_store::{ObjectId, Store};
use serde::{Deserialize, Serialize};

use crate::error::{CliError, exit};
use crate::timefmt;

/// Agent hooks can fire while another Fabric command holds the store.
const STORE_LOCK_WAIT: Duration = Duration::from_secs(30);
const PROJECT_FORMAT: &str = "fabric-project";
const PROJECT_VERSION: u32 = 1;

pub struct Home {
    pub root: PathBuf,
}

pub struct Project {
    pub id: String,
    pub root: PathBuf,
    pub dir: PathBuf,
}

const STAT_CACHE_FILE: &str = "stat-cache.json";

impl Project {
    /// Captures the project, skipping files the stat cache shows are unchanged, and saves
    /// the updated cache. Callers hold the store open, which serializes concurrent captures
    /// (and therefore cache updates) through the store lock.
    pub fn capture(&self, store: &Store) -> Result<Snapshot, CliError> {
        let path = self.dir.join(STAT_CACHE_FILE);
        let mut cache = StatCache::load(&path);
        let snapshot = capture_cached(store, &self.root, &mut cache)?;
        // A cache that fails to save only costs speed next time.
        let _ = cache.save(&path);
        Ok(snapshot)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    format: String,
    version: u32,
    id: String,
    root: PathBuf,
    created_at: String,
}

impl Home {
    pub fn locate() -> Result<Self, CliError> {
        if let Some(dir) = std::env::var_os("FABRIC_HOME") {
            return Ok(Self { root: dir.into() });
        }
        let base = dirs::data_local_dir().ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "no_home",
                "cannot determine a data directory; set FABRIC_HOME",
            )
        })?;
        Ok(Self {
            root: base.join("fabric"),
        })
    }

    pub fn store_path(&self) -> PathBuf {
        self.root.join("store")
    }

    pub fn open_store(&self) -> Result<Store, CliError> {
        let path = self.store_path();
        let store = if Store::exists(&path) {
            Store::open_waiting(&path, STORE_LOCK_WAIT)?
        } else {
            Store::init(&path)?
        };
        Ok(store)
    }

    fn project_dir(&self, id: &str) -> PathBuf {
        self.root.join("projects").join(id)
    }

    /// Registers `root` as a project. Returns the project and whether it was newly registered.
    pub fn register(&self, root: &Path) -> Result<(Project, bool), CliError> {
        let root = canonical_dir(root)?;
        if self.root.starts_with(&root)
            || canonical_dir(&self.root).is_ok_and(|h| h.starts_with(&root))
        {
            return Err(CliError::new(
                exit::ERROR,
                "home_inside_project",
                format!(
                    "Fabric's data directory {} is inside {}; set FABRIC_HOME to a directory outside the project",
                    display_path(&self.root),
                    display_path(&root)
                ),
            ));
        }
        let id = project_id(&root);
        let dir = self.project_dir(&id);
        let file = dir.join("project.json");
        if file.exists() {
            return Ok((self.load(&file)?, false));
        }
        fs::create_dir_all(&dir).map_err(|e| CliError::io(&dir, e))?;
        let record = ProjectFile {
            format: PROJECT_FORMAT.to_owned(),
            version: PROJECT_VERSION,
            id: id.clone(),
            root: root.clone(),
            created_at: timefmt::now_rfc3339(),
        };
        let bytes = serde_json::to_vec_pretty(&record).expect("project records always serialize");
        write_durably(&file, &bytes)?;
        Ok((Project { id, root, dir }, true))
    }

    /// Finds the registered project containing `start`.
    pub fn find_project(&self, start: &Path) -> Result<Project, CliError> {
        let start = canonical_dir(start)?;
        for dir in start.ancestors() {
            let file = self.project_dir(&project_id(dir)).join("project.json");
            if file.exists() {
                return self.load(&file);
            }
        }
        Err(CliError::new(
            exit::ERROR,
            "not_a_project",
            format!(
                "{} is not inside a Fabric project; run `fabric init` in the project root",
                display_path(&start)
            ),
        ))
    }

    fn load(&self, file: &Path) -> Result<Project, CliError> {
        let bytes = fs::read(file).map_err(|e| CliError::io(file, e))?;
        let record: ProjectFile = serde_json::from_slice(&bytes).map_err(|e| {
            CliError::new(
                exit::ERROR,
                "invalid_project",
                format!("{}: {e}", display_path(file)),
            )
        })?;
        if record.format != PROJECT_FORMAT || record.version != PROJECT_VERSION {
            return Err(CliError::new(
                exit::ERROR,
                "invalid_project",
                format!("{}: unsupported project record", display_path(file)),
            ));
        }
        Ok(Project {
            dir: self.project_dir(&record.id),
            id: record.id,
            root: record.root,
        })
    }
}

/// A project's id is derived from its canonical root path, so it can be found again from any
/// directory inside it. Moving a project therefore starts a new history.
fn project_id(root: &Path) -> String {
    let id = ObjectId::of(root.to_string_lossy().as_bytes()).to_hex();
    id[..16].to_owned()
}

fn canonical_dir(path: &Path) -> Result<PathBuf, CliError> {
    let canonical = fs::canonicalize(path).map_err(|e| CliError::io(path, e))?;
    if !canonical.is_dir() {
        return Err(CliError::new(
            exit::ERROR,
            "not_a_directory",
            format!("{} is not a directory", display_path(path)),
        ));
    }
    Ok(canonical)
}

/// A path as an agent or person gave it, made relative to the project root with `/`
/// separators — the form used in manifests and timelines. Returned unchanged (apart from
/// separators) if it is not inside the project.
///
/// Agents report paths as they see them, which can differ from the project's canonical root
/// (macOS `/var` vs `/private/var`, Windows 8.3 short names), so the path is resolved first.
/// Paths that do not exist yet (a new file in a new directory) are resolved through their
/// nearest existing ancestor.
pub fn project_relative(target: &str, root: &Path) -> String {
    resolve_lenient(Path::new(target))
        .as_deref()
        .and_then(|resolved| resolved.strip_prefix(root).ok())
        .map(|relative| {
            relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|| {
            if cfg!(windows) {
                target.replace('\\', "/")
            } else {
                target.to_owned()
            }
        })
}

/// Canonicalizes a path that may not exist yet: its nearest existing ancestor is resolved
/// and the remaining components appended. Returns `None` if no ancestor can be resolved or a
/// missing component is `..`, which cannot be resolved without the directory existing.
pub fn resolve_lenient(path: &Path) -> Option<PathBuf> {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(resolved) = fs::canonicalize(existing) {
            let mut out = resolved;
            out.extend(missing.iter().rev());
            return Some(out);
        }
        missing.push(existing.file_name()?);
        existing = existing.parent()?;
        if existing.as_os_str().is_empty() {
            existing = Path::new(".");
        }
    }
}

/// Paths as people expect to read them: without Windows' `\\?\` verbatim prefix.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\")
        .filter(|rest| !rest.starts_with("UNC\\"))
        .unwrap_or(&text)
        .to_owned()
}

pub fn write_durably(path: &Path, contents: &[u8]) -> Result<(), CliError> {
    let temp = path.with_extension("tmp");
    let mut file = File::create(&temp).map_err(|e| CliError::io(&temp, e))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|e| CliError::io(&temp, e))?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| CliError::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_relative_resolves_paths_before_comparing() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "").unwrap();

        // The non-canonical form an agent may report (e.g. /var vs /private/var on macOS).
        let reported = dir.path().join("src").join("main.rs");
        assert_eq!(
            project_relative(&reported.to_string_lossy(), &root),
            "src/main.rs"
        );
        let deleted = dir.path().join("src").join("gone.rs");
        assert_eq!(
            project_relative(&deleted.to_string_lossy(), &root),
            "src/gone.rs"
        );
        let in_new_dirs = dir
            .path()
            .join("src")
            .join("new")
            .join("deeper")
            .join("mod.rs");
        assert_eq!(
            project_relative(&in_new_dirs.to_string_lossy(), &root),
            "src/new/deeper/mod.rs"
        );
        assert_eq!(project_relative("src/lib.rs", &root), "src/lib.rs");
        assert_eq!(
            project_relative("/elsewhere/x.rs", &root),
            "/elsewhere/x.rs"
        );
    }
}
