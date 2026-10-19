use std::fs::{self, File, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{CorruptReason, ObjectId, Result, StoreError};

const LAYOUT_FILE: &str = "FABRIC_STORE";
const LAYOUT_CONTENTS: &str = "fabric-store layout 1\n";
const LOCK_FILE: &str = "LOCK";
const OBJECTS_DIR: &str = "objects";
const TMP_DIR: &str = "tmp";
const TMP_SUFFIX: &str = ".tmp";

const OBJECT_MAGIC: [u8; 4] = *b"FBOB";
const OBJECT_FORMAT_VERSION: u8 = 1;
const CODEC_ZSTD: u8 = 1;
const HEADER_LEN: usize = 16;

const ZSTD_LEVEL: i32 = 3;
const IO_BUF_LEN: usize = 64 * 1024;
const RENAME_ATTEMPTS: u32 = 6;

/// A content-addressed object store rooted at a directory.
///
/// An open `Store` holds an exclusive lock on the store directory for its lifetime, so at
/// most one process writes to a store at a time. Any temp file present when the lock is
/// acquired therefore belongs to a writer that died, and is removed on open.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    removed_temp_files: u64,
    // Released when the store is dropped, or by the OS when the process exits.
    _lock: File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    /// The content was new and has been durably written.
    Stored,
    /// An object with this identity already existed; nothing was written.
    AlreadyPresent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutResult {
    pub id: ObjectId,
    /// Uncompressed content size in bytes.
    pub size: u64,
    pub outcome: PutOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStats {
    pub object_count: u64,
    /// Bytes on disk used by object files, including headers.
    pub stored_bytes: u64,
}

impl Store {
    /// Creates a store at `root`, or opens it if one already exists there.
    ///
    /// Refuses to turn a non-empty directory that is not already a store into one.
    pub fn init(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let layout = root.join(LAYOUT_FILE);
        if !layout.exists() {
            if dir_has_entries(root)? {
                return Err(StoreError::NotAStore {
                    path: root.to_path_buf(),
                });
            }
            fs::create_dir_all(root).map_err(io_at(root))?;
            write_file_durably(&layout, LAYOUT_CONTENTS.as_bytes())?;
        }
        Self::open(root)
    }

    /// Opens an existing store, taking its process lock and removing temp files left behind
    /// by interrupted writers.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let layout = root.join(LAYOUT_FILE);
        match fs::read_to_string(&layout) {
            Ok(contents) if contents == LAYOUT_CONTENTS => {}
            Ok(_) => return Err(StoreError::NotAStore { path: root }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(StoreError::NotAStore { path: root });
            }
            Err(e) => return Err(io_at(&layout)(e)),
        }

        let lock_path = root.join(LOCK_FILE);
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(io_at(&lock_path))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(StoreError::Locked { path: root }),
            Err(TryLockError::Error(e)) => return Err(io_at(&lock_path)(e)),
        }

        for dir in [OBJECTS_DIR, TMP_DIR] {
            let dir = root.join(dir);
            fs::create_dir_all(&dir).map_err(io_at(&dir))?;
        }

        let mut store = Self {
            root,
            removed_temp_files: 0,
            _lock: lock,
        };
        store.removed_temp_files = store.remove_stale_temp_files()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// How many incomplete writes from earlier, interrupted processes were cleaned up when
    /// this store was opened.
    pub fn removed_temp_files(&self) -> u64 {
        self.removed_temp_files
    }

    /// Where the object with this identity lives, whether or not it exists.
    pub fn object_path(&self, id: &ObjectId) -> PathBuf {
        let hex = id.to_hex();
        self.root.join(OBJECTS_DIR).join(&hex[..2]).join(&hex[2..])
    }

    /// Whether an object file exists for `id`. This does not verify its content.
    pub fn contains(&self, id: &ObjectId) -> bool {
        self.object_path(id).is_file()
    }

    /// Stores `content`, skipping the write entirely if it is already present.
    pub fn put_bytes(&self, content: &[u8]) -> Result<PutResult> {
        let id = ObjectId::of(content);
        if self.contains(&id) {
            return Ok(PutResult {
                id,
                size: content.len() as u64,
                outcome: PutOutcome::AlreadyPresent,
            });
        }
        let staged = self.stage(content)?;
        debug_assert_eq!(staged.id, id);
        self.commit(staged)
    }

    /// Stores everything `source` yields, streaming it so large files need not fit in
    /// memory. If `source` fails part way, nothing is stored.
    pub fn put_reader(&self, source: impl Read) -> Result<PutResult> {
        let staged = self.stage(source)?;
        if self.contains(&staged.id) {
            return Ok(PutResult {
                id: staged.id,
                size: staged.size,
                outcome: PutOutcome::AlreadyPresent,
            });
        }
        self.commit(staged)
    }

    /// Reads and fully verifies an object. Corrupt content is never returned.
    pub fn read(&self, id: &ObjectId) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        self.read_into(id, &mut content)?;
        Ok(content)
    }

    /// Streams an object's content into `sink`, verifying it, and returns its size.
    ///
    /// Bytes reach `sink` as they are decoded, before the final hash check. If this returns
    /// an error, whatever `sink` received is unverified and must be discarded.
    pub fn read_into(&self, id: &ObjectId, sink: &mut impl Write) -> Result<u64> {
        let path = self.object_path(id);
        let corrupt = |reason| StoreError::Corrupt { id: *id, reason };

        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(StoreError::Missing(*id)),
            Err(e) => return Err(io_at(&path)(e)),
        };
        let mut header = [0; HEADER_LEN];
        if let Err(e) = file.read_exact(&mut header) {
            return Err(if e.kind() == io::ErrorKind::UnexpectedEof {
                corrupt(CorruptReason::InvalidHeader(
                    "file is shorter than the header",
                ))
            } else {
                io_at(&path)(e)
            });
        }
        let expected = decode_header(&header).map_err(corrupt)?;

        let tracked = TrackIoErrors {
            inner: file,
            error: None,
        };
        let mut decoder = zstd::Decoder::new(tracked).map_err(io_at(&path))?;
        let mut hasher = blake3::Hasher::new();
        let mut size = 0u64;
        let mut buf = vec![0; IO_BUF_LEN];
        loop {
            let n = match decoder.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                // A failure reading the file is an I/O problem; anything else the decoder
                // reports means the bytes on disk are not a valid payload.
                Err(e) => {
                    return Err(match decoder.get_mut().get_mut().error.take() {
                        Some(source) => StoreError::Io { path, source },
                        None => corrupt(CorruptReason::Decode(e.to_string())),
                    });
                }
            };
            size += n as u64;
            if size > expected {
                return Err(corrupt(CorruptReason::LengthMismatch {
                    expected,
                    actual: size,
                }));
            }
            hasher.update(&buf[..n]);
            sink.write_all(&buf[..n]).map_err(StoreError::SinkWrite)?;
        }

        if size != expected {
            return Err(corrupt(CorruptReason::LengthMismatch {
                expected,
                actual: size,
            }));
        }
        let actual = ObjectId::from(hasher.finalize());
        if actual != *id {
            return Err(corrupt(CorruptReason::HashMismatch { actual }));
        }
        Ok(size)
    }

    /// Verifies that an object exists and its content matches its identity. Returns its size.
    pub fn verify(&self, id: &ObjectId) -> Result<u64> {
        self.read_into(id, &mut io::sink())
    }

    /// Lists every object in the store, sorted. Files that are not named like objects are
    /// ignored.
    pub fn list(&self) -> Result<Vec<ObjectId>> {
        let mut ids = Vec::new();
        for fanout in read_dir(&self.root.join(OBJECTS_DIR))? {
            let name = fanout.file_name();
            let Some(prefix) = name.to_str().filter(|p| p.len() == 2) else {
                continue;
            };
            for entry in read_dir(&fanout.path())? {
                let name = entry.file_name();
                let Some(rest) = name.to_str() else { continue };
                if let Some(id) = ObjectId::from_hex(&format!("{prefix}{rest}")) {
                    ids.push(id);
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn stats(&self) -> Result<StoreStats> {
        let mut stats = StoreStats {
            object_count: 0,
            stored_bytes: 0,
        };
        for id in self.list()? {
            let path = self.object_path(&id);
            stats.object_count += 1;
            stats.stored_bytes += fs::metadata(&path).map_err(io_at(&path))?.len();
        }
        Ok(stats)
    }

    /// Compresses `source` into a temp file while hashing it. The temp file is removed if
    /// anything fails or the result is dropped without being committed.
    fn stage(&self, mut source: impl Read) -> Result<Staged> {
        let (temp, mut file) = TempFile::create(&self.root.join(TMP_DIR))?;
        let path = temp.path.clone();

        // Placeholder until the content length is known.
        file.write_all(&[0; HEADER_LEN]).map_err(io_at(&path))?;
        let mut encoder = zstd::Encoder::new(file, ZSTD_LEVEL).map_err(io_at(&path))?;
        let mut hasher = blake3::Hasher::new();
        let mut size = 0u64;
        let mut buf = vec![0; IO_BUF_LEN];
        loop {
            let n = match source.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(StoreError::SourceRead(e)),
            };
            hasher.update(&buf[..n]);
            encoder.write_all(&buf[..n]).map_err(io_at(&path))?;
            size += n as u64;
        }

        let mut file = encoder.finish().map_err(io_at(&path))?;
        file.seek(SeekFrom::Start(0)).map_err(io_at(&path))?;
        file.write_all(&encode_header(size)).map_err(io_at(&path))?;
        file.sync_all().map_err(io_at(&path))?;
        Ok(Staged {
            temp,
            id: ObjectId::from(hasher.finalize()),
            size,
        })
    }

    /// Moves a staged object into place. The object is read-only once visible.
    fn commit(&self, staged: Staged) -> Result<PutResult> {
        let Staged { temp, id, size } = staged;
        let dest = self.object_path(&id);
        let dir = dest
            .parent()
            .expect("object paths always have a fan-out directory");
        fs::create_dir_all(dir).map_err(io_at(dir))?;

        let mut perms = fs::metadata(&temp.path)
            .map_err(io_at(&temp.path))?
            .permissions();
        perms.set_readonly(true);
        fs::set_permissions(&temp.path, perms).map_err(io_at(&temp.path))?;

        rename_with_retry(&temp.path, &dest).map_err(io_at(&dest))?;
        temp.disarm();
        sync_dir(dir)?;
        Ok(PutResult {
            id,
            size,
            outcome: PutOutcome::Stored,
        })
    }

    fn remove_stale_temp_files(&self) -> Result<u64> {
        let mut removed = 0;
        for entry in read_dir(&self.root.join(TMP_DIR))? {
            if entry.file_name().to_string_lossy().ends_with(TMP_SUFFIX) {
                let path = entry.path();
                remove_file_forcing(&path).map_err(io_at(&path))?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

struct Staged {
    temp: TempFile,
    id: ObjectId,
    size: u64,
}

/// A temp file that deletes itself on drop unless disarmed.
struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    fn create(dir: &Path) -> Result<(Self, File)> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{}-{nanos}-{n}{TMP_SUFFIX}", process::id()));
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(io_at(&path))?;
        Ok((Self { path, armed: true }, file))
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = remove_file_forcing(&self.path);
        }
    }
}

/// Remembers the last real I/O error from the object file, so a decoder failure can be
/// attributed to the disk rather than to corrupt content.
struct TrackIoErrors<R> {
    inner: R,
    error: Option<io::Error>,
}

impl<R: Read> Read for TrackIoErrors<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf).inspect_err(|e| {
            if e.kind() != io::ErrorKind::Interrupted {
                self.error = Some(io::Error::new(e.kind(), e.to_string()));
            }
        })
    }
}

fn encode_header(len: u64) -> [u8; HEADER_LEN] {
    let mut header = [0; HEADER_LEN];
    header[..4].copy_from_slice(&OBJECT_MAGIC);
    header[4] = OBJECT_FORMAT_VERSION;
    header[5] = CODEC_ZSTD;
    // Bytes 6..8 are reserved and must be zero.
    header[8..].copy_from_slice(&len.to_le_bytes());
    header
}

fn decode_header(header: &[u8; HEADER_LEN]) -> Result<u64, CorruptReason> {
    if header[..4] != OBJECT_MAGIC {
        return Err(CorruptReason::InvalidHeader("bad magic bytes"));
    }
    if header[4] != OBJECT_FORMAT_VERSION {
        return Err(CorruptReason::UnsupportedVersion(header[4]));
    }
    if header[5] != CODEC_ZSTD {
        return Err(CorruptReason::InvalidHeader("unknown codec"));
    }
    if header[6..8] != [0, 0] {
        return Err(CorruptReason::InvalidHeader("reserved bytes are not zero"));
    }
    let len: [u8; 8] = header[8..]
        .try_into()
        .expect("header length field is 8 bytes");
    Ok(u64::from_le_bytes(len))
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> StoreError + use<> {
    let path = path.to_path_buf();
    move |source| StoreError::Io { path, source }
}

fn read_dir(path: &Path) -> Result<Vec<fs::DirEntry>> {
    fs::read_dir(path)
        .and_then(|entries| entries.collect())
        .map_err(io_at(path))
}

fn dir_has_entries(path: &Path) -> Result<bool> {
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_some()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_at(path)(e)),
    }
}

fn write_file_durably(path: &Path, contents: &[u8]) -> Result<()> {
    let temp = path.with_extension(TMP_SUFFIX.trim_start_matches('.'));
    let mut file = File::create(&temp).map_err(io_at(&temp))?;
    file.write_all(contents).map_err(io_at(&temp))?;
    file.sync_all().map_err(io_at(&temp))?;
    drop(file);
    rename_with_retry(&temp, path).map_err(io_at(path))?;
    sync_dir(path.parent().unwrap_or(Path::new(".")))
}

/// On Windows, antivirus scanners and search indexers briefly open newly written files,
/// which makes a rename fail with "access denied". Retry for a short while before giving up.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut delay = Duration::from_millis(5);
    for _ in 1..RENAME_ATTEMPTS {
        match fs::rename(from, to) {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                thread::sleep(delay);
                delay *= 2;
            }
            result => return result,
        }
    }
    fs::rename(from, to)
}

// Clearing the read-only flag here only touches the Windows file attribute, never Unix
// permission bits, which is what this lint guards against.
#[allow(clippy::permissions_set_readonly_false)]
fn remove_file_forcing(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        // Windows refuses to delete files that have the read-only attribute set.
        #[cfg(windows)]
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_readonly(false);
            fs::set_permissions(path, perms)?;
            fs::remove_file(path)
        }
        result => result,
    }
}

/// Makes a rename into `dir` durable. See `docs/STORAGE_FORMAT.md` for per-platform behavior.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_at(dir))
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<()> {
    // Windows offers no supported way to flush a directory entry; NTFS journals metadata
    // changes, and the object file's own data was flushed before the rename.
    Ok(())
}
