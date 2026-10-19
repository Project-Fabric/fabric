//! Measures how long a checkpoint takes on a synthetic project.
//!
//!     cargo run --release -p fabric-snapshot --example capture_bench -- [files] [bytes-per-file]
//!
//! Reports the first capture (everything new), a recapture reading every file, and a
//! recapture using the stat cache — the case agent hooks hit on every step.

use std::fs::{self, OpenOptions};
use std::time::{Duration, Instant, SystemTime};

use fabric_snapshot::{StatCache, capture, capture_cached};
use fabric_store::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let files: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let size: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(4_096);

    let dir = tempfile::tempdir().expect("temp dir");
    let project = dir.path().join("project");
    let old = SystemTime::now() - Duration::from_secs(3600);
    for i in 0..files {
        let path = project.join(format!("pkg{}/mod{}/file{i}.rs", i % 10, i % 100));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line = format!("// file {i}\n");
        fs::write(&path, line.repeat(size / line.len() + 1)).unwrap();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let store = Store::init(dir.path().join("store")).unwrap();
    let mut cache = StatCache::new();

    let time = |label: &str, run: &mut dyn FnMut() -> fabric_snapshot::Snapshot| {
        let start = Instant::now();
        let snapshot = run();
        let elapsed = start.elapsed();
        println!(
            "{label:<34} {:>8.1} ms   ({} files, {} read from cache)",
            elapsed.as_secs_f64() * 1000.0,
            snapshot.stats.files,
            snapshot.stats.cache_hits
        );
        snapshot.id
    };

    println!("{files} files of ~{size} bytes in 110 directories\n");
    let first = time("first capture (all new)", &mut || {
        capture_cached(&store, &project, &mut cache).unwrap()
    });
    let reread = time("recapture, reading every file", &mut || {
        capture(&store, &project).unwrap()
    });
    let cached = time("recapture with stat cache", &mut || {
        capture_cached(&store, &project, &mut cache).unwrap()
    });
    assert!(
        first == reread && reread == cached,
        "all captures must agree"
    );
}
