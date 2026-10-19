# Storage format

This document is normative. Changing anything here requires a version bump.

## Store layout (layout version 1)

```
<store>/
├── FABRIC_STORE            "fabric-store layout 1\n" — identifies the directory as a store
├── LOCK                    held with an exclusive OS file lock by the process using the store
├── objects/
│   └── ab/                 first 2 hex chars of the object id
│       └── cdef…           remaining 62 hex chars
└── tmp/                    in-progress writes (*.tmp)
```

`init` refuses to create a store in a non-empty directory that lacks `FABRIC_STORE`, so it
can never take over an existing project directory.

## Object identity

`blake3:<64 lowercase hex>` — the BLAKE3-256 hash of the **uncompressed** content. Only
lowercase is valid, so each object has one path even on case-insensitive filesystems.

## Object file (format version 1)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | magic `FBOB` |
| 4 | 1 | format version, `1` |
| 5 | 1 | codec, `1` = zstd |
| 6 | 2 | reserved, must be zero |
| 8 | 8 | uncompressed length, little-endian u64 |
| 16 | … | zstd stream (level 3) |

## Write protocol

1. Create `tmp/<pid>-<nanos>-<counter>.tmp` with `create_new`.
2. Stream content through BLAKE3 and zstd into it, then write the header.
3. `fsync` the file.
4. If an object with that id already exists, discard the temp file (deduplication).
5. Mark the temp file read-only and rename it to `objects/xx/yyyy…`.
6. On Unix, `fsync` the fan-out directory.

An object path therefore either does not exist or holds a complete, flushed object. A crash
at any point leaves at most a stale temp file, which is deleted the next time the store is
opened (safe because the opener holds the exclusive lock, so no live writer can own it).

## Read protocol

Every read verifies: header fields, a decodable zstd stream, decoded length equal to the
header (decoding stops as soon as it is exceeded), and BLAKE3 of the content equal to the
requested id. Failures are reported as `Corrupt` (with a reason), `Missing`, or `Io` — never
as empty or partial content. Streaming reads deliver bytes before the final check; callers
must stage output and discard it on error (restores will write to a staging area first).

## Platform guarantees

| | Linux / macOS | Windows |
|---|---|---|
| Object data durable before rename | yes (`fsync`) | yes (`FlushFileBuffers`) |
| Rename durable after commit returns | yes (directory `fsync`) | relies on NTFS metadata journaling; not explicitly flushed |
| Rename atomic | yes | yes on NTFS (`MoveFileExW`) |
| Process lock | `flock` | `LockFileEx` |

macOS `fsync` does not force data through the drive cache (`F_FULLFSYNC` would); a power loss,
as opposed to a process crash, can lose recent writes there. Network filesystems and FAT/exFAT
are unsupported.

Windows: renames are retried briefly on "access denied", which antivirus scanners and indexers
cause by opening new files.

## Tested so far

Round trips (empty, small, 3 MiB), streaming vs in-memory agreement, deduplication without
rewriting, compression, read-only objects, corruption (flipped byte, swapped object, bad magic,
bad version, truncated header, truncated payload, wrong length), missing objects, a source that
fails mid-write, stale temp cleanup after a simulated crash, lock contention, refusing foreign
directories, id parsing.

Not yet tested: killing a real process mid-write (planned in `tests/crash-recovery`), power
loss, full disks.

## Manifest (format `fabric-manifest`, version 1)

A manifest is the complete captured tree of a project, stored as an ordinary object: compact
JSON, so **a snapshot id is the object id of its manifest**. It contains no timestamps or
capture metadata, so capturing an identical tree twice yields the same id. Metadata about a
capture (when, which agent, why) belongs in a separate checkpoint record (not yet defined).

```json
{"format":"fabric-manifest","version":1,"entries":[
  {"kind":"dir","path":"src"},
  {"kind":"file","path":"src/main.rs","id":"blake3:…","size":12,"executable":false},
  {"kind":"symlink","path":"link","target":"../elsewhere"}
]}
```

Rules, all enforced when a manifest is loaded; a violation is an error, never a partial result:

- `format` and `version` must match exactly. Unknown fields and unknown entry kinds are errors.
- Entries are sorted by `path` (byte order) with no duplicates.
- Every entry's parent directory has its own `dir` entry. All directories are recorded,
  including empty ones.
- Paths use `/`, are relative, and have no empty, `.` or `..` components. `\`, `:` and control
  characters are rejected so a manifest is safe to interpret on any OS.
- `executable` is the Unix owner/group/other execute bit; always `false` when captured on
  Windows.
- Symlinks are recorded with their target exactly as stored, and never followed. Windows
  junctions are treated as symlinks.

### What a capture includes

- Respected: `.gitignore` files (at any depth, with or without a Git repo),
  `.git/info/exclude`, and `.fabricignore` files (same syntax).
- Not respected: the user's global Git excludes, so results don't vary between machines.
- Always excluded: `.git/`, `.env`, `.env.*`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`.
- Files are read without being modified. A file whose size or modification time changes while
  it is read is retried, then skipped. Skipped paths (unreadable, non-UTF-8 or non-portable
  names, special files) are reported with a reason and are not part of the snapshot.
- Objects referenced by a new manifest are known to exist; ones reused from earlier snapshots
  are not re-verified at capture time (that would mean re-reading every file). Restores verify
  every object they use.

## Not yet defined

Checkpoint records, garbage collection, store format migrations.
