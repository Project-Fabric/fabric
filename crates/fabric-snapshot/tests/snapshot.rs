use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fabric_snapshot::{Entry, Manifest, RelPath, Snapshot, SnapshotError, capture, diff};
use fabric_store::{ObjectId, Store};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    project: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        let store = Store::init(dir.path().join("store")).unwrap();
        Self {
            _dir: dir,
            project,
            store,
        }
    }

    fn write(&self, rel: &str, content: &[u8]) {
        let path = self.project.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn capture(&self) -> Snapshot {
        capture(&self.store, &self.project).unwrap()
    }
}

fn paths(manifest: &Manifest) -> Vec<&str> {
    manifest
        .entries()
        .iter()
        .map(|e| e.path().as_str())
        .collect()
}

fn rel(path: &str) -> RelPath {
    RelPath::new(path).unwrap()
}

#[test]
fn captures_files_directories_and_empty_files() {
    let fx = Fixture::new();
    fx.write("README.md", b"# demo");
    fx.write("src/main.rs", b"fn main() {}");
    fx.write("src/empty.rs", b"");
    fs::create_dir_all(fx.project.join("assets/empty-dir")).unwrap();

    let snap = fx.capture();
    assert_eq!(
        paths(&snap.manifest),
        [
            "README.md",
            "assets",
            "assets/empty-dir",
            "src",
            "src/empty.rs",
            "src/main.rs"
        ]
    );
    assert_eq!(
        snap.manifest.get(&rel("src/main.rs")),
        Some(&Entry::File {
            path: rel("src/main.rs"),
            id: ObjectId::of(b"fn main() {}"),
            size: 12,
            executable: false,
        })
    );
    assert!(snap.skipped.is_empty(), "{:?}", snap.skipped);
    assert_eq!((snap.stats.files, snap.stats.dirs), (3, 3));
    assert_eq!(snap.stats.bytes, 18);

    // Every referenced object is present and intact.
    for entry in snap.manifest.entries() {
        if let Entry::File { id, .. } = entry {
            fx.store.verify(id).unwrap();
        }
    }
}

#[test]
fn manifest_round_trips_through_the_store() {
    let fx = Fixture::new();
    fx.write("a/b/c.txt", b"nested");
    let snap = fx.capture();
    assert_eq!(Manifest::load(&fx.store, &snap.id).unwrap(), snap.manifest);
}

#[test]
fn unchanged_project_produces_the_same_snapshot_and_reuses_everything() {
    let fx = Fixture::new();
    fx.write("one.txt", b"1");
    fx.write("dir/two.txt", b"2");
    let first = fx.capture();
    let second = fx.capture();
    assert_eq!(first.id, second.id);
    assert_eq!(second.stats.new_objects, 0);
    assert_eq!(second.stats.reused_objects, 2);
}

#[test]
fn identical_files_share_one_object() {
    let fx = Fixture::new();
    fx.write("a.txt", b"same");
    fx.write("b/a-copy.txt", b"same");
    let snap = fx.capture();
    assert_eq!(snap.stats.new_objects, 1);
    assert_eq!(snap.stats.reused_objects, 1);
}

#[test]
fn snapshot_does_not_modify_the_project() {
    fn listing(root: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    stack.push(entry.path());
                }
                // NTFS updates directory timestamps lazily, so only file timestamps are a
                // reliable signal; directories are compared by existence.
                let modified = if meta.is_dir() {
                    None
                } else {
                    meta.modified().ok()
                };
                out.insert(entry.path(), (meta.len(), modified));
            }
        }
        out
    }

    let fx = Fixture::new();
    fx.write("src/lib.rs", b"pub fn x() {}");
    fx.write(".gitignore", b"target/\n");
    fx.write("target/debug/out", b"build output");
    let before = listing(&fx.project);
    fx.capture();
    assert_eq!(listing(&fx.project), before);
}

#[test]
fn respects_gitignore_and_fabricignore_without_a_git_repo() {
    let fx = Fixture::new();
    fx.write(".gitignore", b"node_modules/\n*.log\n");
    fx.write(".fabricignore", b"scratch/\n");
    fx.write("node_modules/pkg/index.js", b"dependency");
    fx.write("debug.log", b"noise");
    fx.write("scratch/notes.txt", b"temporary");
    fx.write("app.js", b"kept");
    fx.write("sub/.gitignore", b"generated.rs\n");
    fx.write("sub/generated.rs", b"generated");
    fx.write("sub/handwritten.rs", b"kept");

    let snap = fx.capture();
    assert_eq!(
        paths(&snap.manifest),
        [
            ".fabricignore",
            ".gitignore",
            "app.js",
            "sub",
            "sub/.gitignore",
            "sub/handwritten.rs"
        ]
    );
}

#[test]
fn excludes_git_directory_and_secrets_by_default() {
    let fx = Fixture::new();
    fx.write(".git/HEAD", b"ref: refs/heads/main");
    fx.write(".env", b"API_KEY=secret");
    fx.write(".env.production", b"API_KEY=secret");
    fx.write("certs/server.pem", b"-----BEGIN PRIVATE KEY-----");
    fx.write("deploy/id_ed25519", b"private key");
    fx.write("config/settings.toml", b"kept");

    let snap = fx.capture();
    assert_eq!(
        paths(&snap.manifest),
        ["certs", "config", "config/settings.toml", "deploy"]
    );
}

#[test]
fn refuses_a_store_inside_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let store = Store::init(project.join(".fabric-store")).unwrap();
    assert!(matches!(
        capture(&store, &project),
        Err(SnapshotError::StoreInsideProject { .. })
    ));
}

#[test]
fn refuses_a_file_as_the_project_root() {
    let fx = Fixture::new();
    fx.write("file.txt", b"not a directory");
    assert!(matches!(
        capture(&fx.store, &fx.project.join("file.txt")),
        Err(SnapshotError::NotADirectory { .. })
    ));
}

/// Creating symlinks needs Developer Mode or admin rights on Windows; skip if unavailable.
fn try_symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    match result {
        Ok(()) => true,
        Err(e) => {
            eprintln!("skipping symlink assertions: {e}");
            false
        }
    }
}

#[test]
fn symlinks_are_recorded_but_never_followed() {
    let fx = Fixture::new();
    let outside = fx.project.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"must not be captured").unwrap();
    fx.write("inside.txt", b"kept");
    if !try_symlink_dir(&outside, &fx.project.join("escape")) {
        return;
    }

    let snap = fx.capture();
    assert_eq!(paths(&snap.manifest), ["escape", "inside.txt"]);
    let Some(Entry::Symlink { target, .. }) = snap.manifest.get(&rel("escape")) else {
        panic!("escape should be recorded as a symlink");
    };
    assert_eq!(Path::new(target), outside);
    assert!(!fx.store.contains(&ObjectId::of(b"must not be captured")));
}

#[cfg(unix)]
#[test]
fn records_the_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    fx.write("run.sh", b"#!/bin/sh\n");
    fs::set_permissions(fx.project.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    let snap = fx.capture();
    assert!(matches!(
        snap.manifest.get(&rel("run.sh")),
        Some(Entry::File {
            executable: true,
            ..
        })
    ));
}

#[cfg(unix)]
#[test]
fn non_portable_names_are_skipped_and_reported() {
    use fabric_snapshot::SkipReason;
    let fx = Fixture::new();
    fx.write("back\\slash.txt", b"x");
    fx.write("ok.txt", b"y");
    let snap = fx.capture();
    assert_eq!(paths(&snap.manifest), ["ok.txt"]);
    assert_eq!(snap.skipped.len(), 1);
    assert_eq!(snap.skipped[0].path, "back\\slash.txt");
    assert!(matches!(
        snap.skipped[0].reason,
        SkipReason::NonPortableName { .. }
    ));
}

fn load_raw(store: &Store, json: &str) -> Result<Manifest, SnapshotError> {
    let id = store.put_bytes(json.as_bytes()).unwrap().id;
    Manifest::load(store, &id)
}

#[test]
fn malformed_manifests_are_rejected() {
    let fx = Fixture::new();
    let id = ObjectId::of(b"x");
    let file = |path: &str| {
        format!(r#"{{"kind":"file","path":"{path}","id":"{id}","size":1,"executable":false}}"#)
    };
    let manifest = |entries: &[String]| {
        format!(
            r#"{{"format":"fabric-manifest","version":1,"entries":[{}]}}"#,
            entries.join(",")
        )
    };

    assert!(load_raw(&fx.store, &manifest(&[file("a.txt")])).is_ok());

    let bad = [
        ("not json", "{".to_owned()),
        (
            "wrong format",
            r#"{"format":"other","version":1,"entries":[]}"#.to_owned(),
        ),
        (
            "future version",
            r#"{"format":"fabric-manifest","version":2,"entries":[]}"#.to_owned(),
        ),
        (
            "unknown top-level field",
            r#"{"format":"fabric-manifest","version":1,"entries":[],"x":1}"#.to_owned(),
        ),
        ("path traversal", manifest(&[file("../escape.txt")])),
        ("absolute path", manifest(&[file("/etc/passwd")])),
        ("unsorted", manifest(&[file("b.txt"), file("a.txt")])),
        ("duplicate", manifest(&[file("a.txt"), file("a.txt")])),
        ("missing parent directory", manifest(&[file("dir/a.txt")])),
        (
            "unknown entry field",
            manifest(&[r#"{"kind":"dir","path":"d","mode":"0777"}"#.to_owned()]),
        ),
        (
            "unknown entry kind",
            manifest(&[r#"{"kind":"device","path":"d"}"#.to_owned()]),
        ),
        (
            "bad object id",
            manifest(&[
                r#"{"kind":"file","path":"a","id":"md5:00","size":1,"executable":false}"#
                    .to_owned(),
            ]),
        ),
    ];
    for (case, json) in bad {
        let result = load_raw(&fx.store, &json);
        assert!(
            matches!(
                result,
                Err(SnapshotError::InvalidManifest { id: Some(_), .. })
            ),
            "{case}: {result:?}"
        );
    }
}

#[test]
fn diff_reports_added_removed_modified_and_renamed() {
    let fx = Fixture::new();
    fx.write("keep.txt", b"unchanged");
    fx.write("edit.txt", b"before");
    fx.write("delete.txt", b"gone soon");
    fx.write("old_name.rs", b"moved content");
    let before = fx.capture();

    fx.write("edit.txt", b"after");
    fs::remove_file(fx.project.join("delete.txt")).unwrap();
    fs::rename(
        fx.project.join("old_name.rs"),
        fx.project.join("new_name.rs"),
    )
    .unwrap();
    fx.write("new/file.txt", b"brand new");
    let after = fx.capture();

    let d = diff(&before.manifest, &after.manifest);
    assert_eq!(d.added, [rel("new"), rel("new/file.txt")]);
    assert_eq!(d.removed, [rel("delete.txt")]);
    assert_eq!(d.modified, [rel("edit.txt")]);
    assert_eq!(d.renamed.len(), 1);
    assert_eq!(
        (&d.renamed[0].from, &d.renamed[0].to),
        (&rel("old_name.rs"), &rel("new_name.rs"))
    );
    assert!(diff(&after.manifest, &after.manifest).is_empty());
}

#[test]
fn diff_treats_a_kind_change_as_a_modification() {
    let fx = Fixture::new();
    fx.write("thing", b"a file");
    let before = fx.capture();
    fs::remove_file(fx.project.join("thing")).unwrap();
    fx.write("thing/inner.txt", b"now a directory");
    let after = fx.capture();

    let d = diff(&before.manifest, &after.manifest);
    assert_eq!(d.modified, [rel("thing")]);
    assert_eq!(d.added, [rel("thing/inner.txt")]);
}

#[test]
fn empty_files_are_not_paired_as_renames() {
    let fx = Fixture::new();
    fx.write("a.txt", b"");
    let before = fx.capture();
    fs::remove_file(fx.project.join("a.txt")).unwrap();
    fx.write("b.txt", b"");
    let after = fx.capture();

    let d = diff(&before.manifest, &after.manifest);
    assert!(d.renamed.is_empty());
    assert_eq!(
        (d.removed, d.added),
        (vec![rel("a.txt")], vec![rel("b.txt")])
    );
}
