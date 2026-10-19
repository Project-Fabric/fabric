use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use fabric_store::{CorruptReason, ObjectId, PutOutcome, Store, StoreError};
use tempfile::TempDir;

fn new_store() -> (TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::init(dir.path().join("store")).unwrap();
    (dir, store)
}

/// Deterministic, incompressible test content.
fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

#[allow(clippy::permissions_set_readonly_false)]
fn make_writable(path: &Path) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    #[cfg(unix)]
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o644);
    #[cfg(not(unix))]
    perms.set_readonly(false);
    fs::set_permissions(path, perms).unwrap();
}

/// Simulates on-disk corruption of a stored object.
fn tamper(store: &Store, id: &ObjectId, edit: impl FnOnce(&mut Vec<u8>)) {
    let path = store.object_path(id);
    make_writable(&path);
    let mut bytes = fs::read(&path).unwrap();
    edit(&mut bytes);
    fs::write(&path, bytes).unwrap();
}

fn tmp_dir_entries(store: &Store) -> usize {
    fs::read_dir(store.root().join("tmp")).unwrap().count()
}

fn corrupt_reason(err: StoreError, expected_id: &ObjectId) -> CorruptReason {
    match err {
        StoreError::Corrupt { id, reason } => {
            assert_eq!(&id, expected_id);
            reason
        }
        other => panic!("expected a corruption error, got {other:?}"),
    }
}

#[test]
fn round_trips_empty_small_and_large_content() {
    let (_dir, store) = new_store();
    for content in [
        Vec::new(),
        b"hello fabric".to_vec(),
        pseudo_random(3 * 1024 * 1024 + 7, 1),
    ] {
        let put = store.put_bytes(&content).unwrap();
        assert_eq!(put.outcome, PutOutcome::Stored);
        assert_eq!(put.id, ObjectId::of(&content));
        assert_eq!(put.size, content.len() as u64);
        assert_eq!(store.read(&put.id).unwrap(), content);
        assert_eq!(store.verify(&put.id).unwrap(), content.len() as u64);
    }
    assert_eq!(tmp_dir_entries(&store), 0);
}

#[test]
fn streamed_and_in_memory_puts_agree() {
    let (_dir, store) = new_store();
    let content = pseudo_random(200_000, 2);
    let streamed = store.put_reader(Cursor::new(&content)).unwrap();
    assert_eq!(streamed.outcome, PutOutcome::Stored);
    assert_eq!(streamed.id, ObjectId::of(&content));

    let in_memory = store.put_bytes(&content).unwrap();
    assert_eq!(in_memory.outcome, PutOutcome::AlreadyPresent);
    assert_eq!(in_memory.id, streamed.id);
}

#[test]
fn duplicate_content_is_stored_once_and_left_untouched() {
    let (_dir, store) = new_store();
    let content = pseudo_random(50_000, 3);
    let first = store.put_bytes(&content).unwrap();
    let on_disk_before = fs::read(store.object_path(&first.id)).unwrap();

    let again = store.put_bytes(&content).unwrap();
    let streamed_again = store.put_reader(Cursor::new(&content)).unwrap();

    assert_eq!(first.outcome, PutOutcome::Stored);
    assert_eq!(again.outcome, PutOutcome::AlreadyPresent);
    assert_eq!(streamed_again.outcome, PutOutcome::AlreadyPresent);
    assert_eq!(again.id, first.id);
    assert_eq!(streamed_again.id, first.id);
    assert_eq!(store.list().unwrap(), vec![first.id]);
    assert_eq!(
        fs::read(store.object_path(&first.id)).unwrap(),
        on_disk_before
    );
    assert_eq!(tmp_dir_entries(&store), 0);
}

#[test]
fn distinct_content_gets_distinct_objects() {
    let (_dir, store) = new_store();
    let a = store.put_bytes(b"a").unwrap();
    let b = store.put_bytes(b"b").unwrap();
    assert_ne!(a.id, b.id);
    let mut expected = vec![a.id, b.id];
    expected.sort();
    assert_eq!(store.list().unwrap(), expected);
}

#[test]
fn compressible_content_is_compressed() {
    let (_dir, store) = new_store();
    store.put_bytes(&vec![0; 1024 * 1024]).unwrap();
    let stats = store.stats().unwrap();
    assert_eq!(stats.object_count, 1);
    assert!(
        stats.stored_bytes < 10 * 1024,
        "stored {} bytes",
        stats.stored_bytes
    );
}

#[test]
fn stored_objects_are_read_only() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(b"immutable").unwrap();
    assert!(
        fs::metadata(store.object_path(&put.id))
            .unwrap()
            .permissions()
            .readonly()
    );
}

#[test]
fn flipped_payload_byte_is_detected() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(&pseudo_random(100_000, 4)).unwrap();
    tamper(&store, &put.id, |bytes| {
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
    });

    // Depending on where the flip lands, zstd rejects the frame or the hash mismatches;
    // either way the object must not verify and its content must not be returned.
    corrupt_reason(store.verify(&put.id).unwrap_err(), &put.id);
    corrupt_reason(store.read(&put.id).unwrap_err(), &put.id);
}

#[test]
fn object_swapped_for_other_valid_content_is_detected() {
    let (_dir, store) = new_store();
    let a = store.put_bytes(b"content a").unwrap();
    let b = store.put_bytes(b"content b").unwrap();
    let b_bytes = fs::read(store.object_path(&b.id)).unwrap();
    tamper(&store, &a.id, |bytes| *bytes = b_bytes);

    let reason = corrupt_reason(store.verify(&a.id).unwrap_err(), &a.id);
    assert_eq!(reason, CorruptReason::HashMismatch { actual: b.id });
}

#[test]
fn bad_magic_is_detected() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(b"magic").unwrap();
    tamper(&store, &put.id, |bytes| bytes[0] = b'X');
    let reason = corrupt_reason(store.verify(&put.id).unwrap_err(), &put.id);
    assert_eq!(reason, CorruptReason::InvalidHeader("bad magic bytes"));
}

#[test]
fn unsupported_format_version_is_detected() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(b"version").unwrap();
    tamper(&store, &put.id, |bytes| bytes[4] = 99);
    let reason = corrupt_reason(store.verify(&put.id).unwrap_err(), &put.id);
    assert_eq!(reason, CorruptReason::UnsupportedVersion(99));
}

#[test]
fn file_shorter_than_header_is_detected() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(b"short").unwrap();
    tamper(&store, &put.id, |bytes| bytes.truncate(5));
    let reason = corrupt_reason(store.verify(&put.id).unwrap_err(), &put.id);
    assert!(
        matches!(reason, CorruptReason::InvalidHeader(_)),
        "{reason:?}"
    );
}

#[test]
fn truncated_payload_is_detected() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(&pseudo_random(100_000, 5)).unwrap();
    tamper(&store, &put.id, |bytes| bytes.truncate(bytes.len() / 2));
    corrupt_reason(store.verify(&put.id).unwrap_err(), &put.id);
}

#[test]
fn wrong_recorded_length_is_detected() {
    let (_dir, store) = new_store();
    let put = store.put_bytes(b"twelve bytes").unwrap();
    tamper(&store, &put.id, |bytes| {
        bytes[8..16].copy_from_slice(&5u64.to_le_bytes())
    });
    let reason = corrupt_reason(store.verify(&put.id).unwrap_err(), &put.id);
    assert!(
        matches!(reason, CorruptReason::LengthMismatch { expected: 5, .. }),
        "{reason:?}"
    );
}

#[test]
fn missing_objects_are_reported_as_missing() {
    let (_dir, store) = new_store();
    let never_stored = ObjectId::of(b"never stored");
    assert!(
        matches!(store.verify(&never_stored), Err(StoreError::Missing(id)) if id == never_stored)
    );

    let put = store.put_bytes(b"deleted later").unwrap();
    let path = store.object_path(&put.id);
    make_writable(&path);
    fs::remove_file(&path).unwrap();
    assert!(!store.contains(&put.id));
    assert!(matches!(store.read(&put.id), Err(StoreError::Missing(id)) if id == put.id));
}

/// Yields `limit` bytes, then fails, like a file that becomes unreadable mid-copy.
struct FailingReader {
    remaining: usize,
}

impl Read for FailingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("simulated read failure"));
        }
        let n = buf.len().min(self.remaining);
        buf[..n].fill(7);
        self.remaining -= n;
        Ok(n)
    }
}

#[test]
fn interrupted_write_leaves_no_object_and_no_temp_file() {
    let (_dir, store) = new_store();
    let err = store
        .put_reader(FailingReader { remaining: 300_000 })
        .unwrap_err();
    assert!(matches!(err, StoreError::SourceRead(_)), "{err:?}");
    assert!(store.list().unwrap().is_empty());
    assert_eq!(tmp_dir_entries(&store), 0);
}

#[test]
fn temp_files_from_a_crashed_writer_are_removed_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let kept = {
        let store = Store::init(&root).unwrap();
        store.put_bytes(b"committed before the crash").unwrap().id
    };
    // What a writer killed between staging and rename leaves behind.
    fs::write(root.join("tmp").join("4242-1-0.tmp"), b"partial zstd frame").unwrap();

    let store = Store::open(&root).unwrap();
    assert_eq!(store.removed_temp_files(), 1);
    assert_eq!(tmp_dir_entries(&store), 0);
    assert_eq!(store.list().unwrap(), vec![kept]);
    store.verify(&kept).unwrap();
}

#[test]
fn second_open_is_refused_while_the_store_is_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let store = Store::init(&root).unwrap();
    assert!(matches!(Store::open(&root), Err(StoreError::Locked { .. })));
    drop(store);
    Store::open(&root).unwrap();
}

#[test]
fn reinit_preserves_existing_objects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let id = Store::init(&root)
        .unwrap()
        .put_bytes(b"persisted")
        .unwrap()
        .id;
    let store = Store::init(&root).unwrap();
    assert_eq!(store.read(&id).unwrap(), b"persisted");
}

#[test]
fn refuses_directories_that_are_not_stores() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), b"someone's project").unwrap();
    assert!(matches!(
        Store::init(dir.path()),
        Err(StoreError::NotAStore { .. })
    ));
    assert!(matches!(
        Store::open(dir.path()),
        Err(StoreError::NotAStore { .. })
    ));
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "nothing may be written"
    );

    let empty = tempfile::tempdir().unwrap();
    Store::init(empty.path()).unwrap();
}

#[test]
fn object_ids_have_one_canonical_spelling() {
    let id = ObjectId::of(b"id");
    let text = id.to_string();
    assert!(text.starts_with("blake3:"));
    assert_eq!(text.len(), "blake3:".len() + 64);
    assert_eq!(text.parse::<ObjectId>().unwrap(), id);

    for bad in [
        text.to_uppercase(),
        text.replace("blake3:", "sha256:"),
        text[..text.len() - 1].to_owned(),
        id.to_hex(),
    ] {
        assert!(
            matches!(bad.parse::<ObjectId>(), Err(StoreError::InvalidId(_))),
            "{bad}"
        );
    }

    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, format!("\"{text}\""));
    assert_eq!(serde_json::from_str::<ObjectId>(&json).unwrap(), id);
}
