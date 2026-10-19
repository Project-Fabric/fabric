use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fabric_snapshot::{Entry, RelPath, StatCache, capture, capture_cached};
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

    /// Writes a file and dates it `age` in the past, as files usually are.
    fn write_aged(&self, rel: &str, content: &str, age: Duration) {
        let path = self.project.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        set_age(&path, age);
    }
}

fn set_age(path: &Path, age: Duration) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

const HOUR: Duration = Duration::from_secs(3600);

fn file_id(snapshot: &fabric_snapshot::Snapshot, path: &str) -> ObjectId {
    match snapshot.manifest.get(&RelPath::new(path).unwrap()) {
        Some(Entry::File { id, .. }) => *id,
        other => panic!("{path} is not a file: {other:?}"),
    }
}

#[test]
fn a_warm_cache_skips_reading_and_gives_the_same_snapshot() {
    let fx = Fixture::new();
    for i in 0..20 {
        fx.write_aged(&format!("src/m{i}.rs"), &format!("mod m{i};"), HOUR);
    }
    let mut cache = StatCache::new();

    let cold = capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(cold.stats.cache_hits, 0);
    assert_eq!(cache.len(), 20);

    let warm = capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(warm.stats.cache_hits, 20);
    assert_eq!(warm.id, cold.id);
    assert_eq!(warm.id, capture(&fx.store, &fx.project).unwrap().id);
}

#[test]
fn a_changed_file_is_read_again_even_at_the_same_size() {
    let fx = Fixture::new();
    fx.write_aged("a.txt", "version 1", HOUR);
    fx.write_aged("b.txt", "unchanged", HOUR);
    let mut cache = StatCache::new();
    capture_cached(&fx.store, &fx.project, &mut cache).unwrap();

    // Same length, new content, a different (still old) timestamp.
    fx.write_aged("a.txt", "version 2", HOUR / 2);
    let snapshot = capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(snapshot.stats.cache_hits, 1);
    assert_eq!(file_id(&snapshot, "a.txt"), ObjectId::of(b"version 2"));
    assert_eq!(snapshot.id, capture(&fx.store, &fx.project).unwrap().id);
}

#[test]
fn recently_modified_files_are_never_trusted_from_the_cache() {
    let fx = Fixture::new();
    let path = fx.project.join("hot.txt");
    fs::write(&path, "just written").unwrap();
    let mut cache = StatCache::new();
    capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert!(
        cache.is_empty(),
        "a file modified just now must not be cached"
    );

    // Rewritten within the same timestamp tick: only a re-read can notice.
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "just WRITTEN").unwrap();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let snapshot = capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(snapshot.stats.cache_hits, 0);
    assert_eq!(file_id(&snapshot, "hot.txt"), ObjectId::of(b"just WRITTEN"));
}

#[test]
fn a_cache_hit_whose_content_left_the_store_is_read_again() {
    let fx = Fixture::new();
    fx.write_aged("a.txt", "precious", HOUR);
    let mut cache = StatCache::new();
    capture_cached(&fx.store, &fx.project, &mut cache).unwrap();

    let object = fx.store.object_path(&ObjectId::of(b"precious"));
    let mut perms = fs::metadata(&object).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&object, perms).unwrap();
    fs::remove_file(&object).unwrap();

    let snapshot = capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(snapshot.stats.cache_hits, 0);
    fx.store.verify(&ObjectId::of(b"precious")).unwrap();
}

#[test]
fn deleted_files_leave_the_cache() {
    let fx = Fixture::new();
    fx.write_aged("keep.txt", "k", HOUR);
    fx.write_aged("gone.txt", "g", HOUR);
    let mut cache = StatCache::new();
    capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(cache.len(), 2);
    fs::remove_file(fx.project.join("gone.txt")).unwrap();
    capture_cached(&fx.store, &fx.project, &mut cache).unwrap();
    assert_eq!(cache.len(), 1);
}

#[test]
fn the_cache_survives_saving_and_tolerates_corruption() {
    let fx = Fixture::new();
    fx.write_aged("a.txt", "a", HOUR);
    let mut cache = StatCache::new();
    capture_cached(&fx.store, &fx.project, &mut cache).unwrap();

    let path = fx.project.parent().unwrap().join("stat-cache.json");
    cache.save(&path).unwrap();
    let mut loaded = StatCache::load(&path);
    assert_eq!(loaded.len(), 1);
    let snapshot = capture_cached(&fx.store, &fx.project, &mut loaded).unwrap();
    assert_eq!(snapshot.stats.cache_hits, 1);

    fs::write(&path, "{ not a cache").unwrap();
    assert!(StatCache::load(&path).is_empty());
    assert!(StatCache::load(&path.with_file_name("missing.json")).is_empty());
}
