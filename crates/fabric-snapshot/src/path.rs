use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("path is empty")]
    Empty,

    #[error("{0:?} is not valid UTF-8")]
    NonUtf8(PathBuf),

    #[error("{0:?} is not inside the project root")]
    OutsideRoot(PathBuf),

    #[error("{path:?} has an invalid component: {reason}")]
    InvalidComponent { path: String, reason: &'static str },
}

/// A validated path relative to a project root, with `/` separators on every platform.
///
/// Construction guarantees the path cannot escape the root: no absolute paths, no `.` or `..`
/// components, no empty components. Characters that are path syntax on some platform (`\`,
/// `:`) and control characters are rejected too, so a manifest written on one OS is safe to
/// interpret on another.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    pub fn new(path: impl Into<String>) -> Result<Self, PathError> {
        let path = path.into();
        if path.is_empty() {
            return Err(PathError::Empty);
        }
        for component in path.split('/') {
            if let Err(reason) = check_component(component) {
                return Err(PathError::InvalidComponent { path, reason });
            }
        }
        Ok(Self(path))
    }

    /// Converts a filesystem path under `root` into a `RelPath`.
    pub fn from_fs(root: &Path, path: &Path) -> Result<Self, PathError> {
        let outside = || PathError::OutsideRoot(path.to_path_buf());
        let relative = path.strip_prefix(root).map_err(|_| outside())?;
        let mut parts = Vec::new();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(outside());
            };
            let name = name
                .to_str()
                .ok_or_else(|| PathError::NonUtf8(path.to_path_buf()))?;
            parts.push(name);
        }
        Self::new(parts.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The containing directory, or `None` for a top-level entry.
    pub fn parent(&self) -> Option<RelPath> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| Self(parent.to_owned()))
    }

    /// The path under `root` on this platform.
    pub fn to_fs_path(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        path.extend(self.0.split('/'));
        path
    }
}

fn check_component(component: &str) -> Result<(), &'static str> {
    if component.is_empty() {
        Err("empty component (leading, trailing or repeated '/')")
    } else if component == "." || component == ".." {
        Err("'.' and '..' are not allowed")
    } else if component.chars().any(char::is_control) {
        Err("control characters are not allowed")
    } else if component.contains(['\\', ':']) {
        Err("'\\' and ':' are not allowed")
    } else {
        Ok(())
    }
}

impl TryFrom<String> for RelPath {
    type Error = PathError;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl From<RelPath> for String {
    fn from(path: RelPath) -> Self {
        path.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelPath({:?})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_relative_paths() {
        for ok in [
            "a",
            "a/b.txt",
            "..hidden",
            ".env.example",
            "dir with spaces/ü.rs",
        ] {
            assert_eq!(RelPath::new(ok).unwrap().as_str(), ok);
        }
    }

    #[test]
    fn rejects_paths_that_could_escape_or_are_ambiguous() {
        for bad in [
            "", "/a", "a/", "a//b", ".", "..", "a/../b", "./a", "a\\b", "C:x", "a\0b", "a\nb",
        ] {
            assert!(RelPath::new(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn parent_and_fs_conversion() {
        let path = RelPath::new("src/lib/mod.rs").unwrap();
        assert_eq!(path.parent().unwrap().as_str(), "src/lib");
        assert_eq!(RelPath::new("top").unwrap().parent(), None);

        let root = Path::new("project");
        let fs_path = path.to_fs_path(root);
        assert_eq!(RelPath::from_fs(root, &fs_path).unwrap(), path);
        assert!(RelPath::from_fs(root, Path::new("elsewhere/x")).is_err());
    }
}
