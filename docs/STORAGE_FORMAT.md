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
- Always excluded: `.git/`, the restore staging area `/.fabric-restore/`, `.env`, `.env.*`,
  `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`.
- Files are read without being modified. A file whose size or modification time changes while
  it is read is retried, then skipped. Skipped paths (unreadable, non-UTF-8 or non-portable
  names, special files) are reported with a reason and are not part of the snapshot.
- Objects referenced by a new manifest are known to exist; ones reused from earlier snapshots
  are not re-verified at capture time (that would mean re-reading every file). Restores verify
  every object they use.
- The CLI keeps a **stat cache** per project (`stat-cache.json` in the project's Fabric data
  directory, format `fabric-stat-cache` v1). A file whose size, modification time and, on
  Unix, inode and change time match the cache is not read; its cached content id is used if
  that object still exists. Files modified less than 3 seconds before a capture are never
  cached, so a rewrite within one timestamp tick is always noticed. A file changed while
  keeping exactly the same size and modification time (possible only by deliberately
  resetting the timestamp) is not noticed until it changes again; this is the same trade-off
  Git's index makes. The cache is disposable: deleting it only makes the next capture read
  every file.

## Restore journal (format `fabric-restore-journal`, version 1)

While a restore is in progress the project root contains:

```
.fabric-restore/
├── journal.json     {"format","version","target","base","operations":[…]}, written durably
│                    before any staging or project change
├── progress         one line per completed operation index, appended and fsynced in order
└── staged/<index>   verified content for each write_file operation (<index>.partial while
                     being written)
```

Operations are `remove`, `create_dir`, `write_file` (`id`, `size`, `executable`) and
`create_symlink` (`target`), each with a `path`. Each is idempotent so the operation in flight
during a crash can be repeated. The directory is deleted once all operations are applied, and
excluded from captures.

## Evidence chain (`evidence.jsonl`, record version 1)

Per project, in `<home>/projects/<id>/evidence.jsonl`. Append-only; one record per line:

```
<hash> <body>\n
```

`body` is compact JSON `{"v":1,"seq":N,"prev":"<hex>","at":"<RFC 3339 UTC>","event":{…}}` and
`hash` is the lowercase hex BLAKE3 hash of exactly the body's bytes. `seq` starts at 1 and
increases by one; `prev` is the previous record's hash, or 64 zeros for the first record. A
verifier needs only the file: it re-hashes each body and checks every `seq` and `prev`.
Appends are serialized by `evidence.lock` (an exclusive OS file lock) and fsynced; a final line
without a newline is an interrupted write, removed before the next append and reported by
`fabric evidence verify`.

Events (`event.kind`): `genesis` (project id, root, Fabric version), `checkpoint` (checkpoint
number, snapshot id, parent snapshot id, type, label, agent, session, tool, change counts),
`guard` (decision, rule, tool, target), `approval_requested`, `approval_granted`,
`approval_rejected`, `approval_used` (request number, action digest, approver and OS
accounts), `rollback` (the step, the paths put back, paths that could not be, and the rules
involved), `change_opened` (change number, title, path globs, session, the requests it was
approved with, the starting checkpoint (the session's latest prompt before authorization),
approver and OS accounts), `change_edit` (change
number, tool, action digest, target), `change_reviewed` (change number, outcome `accepted` or
`rejected`, reviewer and OS account, the checkpoint and snapshot reviewed, and the content id
of every file under the change's paths that changed, `null` if removed), `approver_added` and
`approver_removed` (name, public key, fingerprint, by), `tool_call` (from `fabric mcp-proxy`:
server, tool as `server/tool`, agent, session, action digest, decision, rule, and either
`arguments` with outcome `forwarded`, `held` or `blocked`, or the later record of a forwarded
call's outcome `ok` or `error`; `result` is the server's text, cut to 2,000 characters), and
`policy_changed`. `approval_used` records `via` (`hook` or `proxy`). A decision
applied from a signed file adds to its `approval_granted`, `approval_rejected`,
`change_opened` or `change_reviewed` event `approver_key` or `reviewer_key` (fingerprint) and
`signed` (`public_key`, `signature`, `payload`, `same_installation`), so it can be verified
again from the chain alone. Commands and paths in events are cut to 2,000 characters;
full content is in the referenced snapshots. Projects recorded before evidence existed start
their chain at the first checkpoint taken afterwards.

The chain shows that records were not changed, removed or reordered after later records were
written, to anyone holding a later head (for example from an exported pack). It cannot stop
someone with write access to Fabric's data directory from rewriting the whole chain before any
head has been shared.

## Approvals (`approvals.json`, format `fabric-approvals`, version 2)

Per project, next to the evidence chain: `{"format","version","requests":[…]}`, rewritten
durably under `approvals.lock`. Each request has a number, the action's digest (BLAKE3 of
`{"input":…,"tool":…}` serialized with sorted keys), rule, tool, target, the tool input as the
agent sent it, the requesting session and OS account, and its status: `pending`, `approved`,
`rejected`, `used` (an approval is used by exactly one action) or `expired` (approved but
unused for 4 hours). Version 2 added `used_via` (the checkpoints that used it: `hook`,
`proxy`) and `used_unix`; one call may pass both within two minutes. Version 1 files are read.

## Approvers (`approvers.json`, format `fabric-approvers`, version 1)

Per project, next to the approvals: `{"format","version","required","approvers":[…]}`. Each
approver has `name`, `public_key` (Ed25519, 64 hex characters), `fingerprint` (first 16 hex
characters of the key's BLAKE3 hash), `added_by` (OS account) and `added_at`. `required`: unsigned
approvals and reviews, and decisions signed with this installation's own approver key
(`<home>/keys/approver-ed25519.key`), are refused.

## Sign-off files (version 1)

- `fabric-approval-request` (from `fabric approvals export`): `project` (`id`, `root`),
  `exported_at`, and `requests`, each with `id`, `digest`, `rule`, `reason`, `agent`,
  `session`, `tool`, `target`, `input` (exactly as the agent sent it), `requested_at` and
  `change_text` (the diff as the requester's Fabric rendered it).
- `fabric-review-request` (from `fabric review N --export`): `project`, `exported_at`,
  `change` (`id`, `title`, `paths`, `session`, `authorized_by`, `base_checkpoint`), `steps`,
  `snapshot`, `files` (path to content id, `null` if removed) and `diff`.
- `fabric-signed-decision` (from `fabric approver sign`): `payload`, `public_key` and
  `signature` (hex Ed25519 over the payload serialized as compact JSON with sorted keys). The
  payload is `{"kind":"approval","project","requests":[{"id","digest"}],"decision":"approve"|
  "reject","change":{"title","paths"}|null,"note","signed_at"}` or `{"kind":"review",
  "project","change","snapshot","files","decision":"accept"|"reject","note","signed_at"}`.

## Changes (`changes.json`, format `fabric-changes`, version 1)

Per project, next to the approvals: `{"format","version","changes":[…]}`, rewritten durably
under `changes.lock`. Each change has a number, title, path globs (as in project rules), the
agent session it covers, the approval requests it was opened with, the starting checkpoint,
its status (`open`, `accepted`, `rejected`), who authorized it and when, and once reviewed,
the reviewer, OS account, time, note and the checkpoint holding the reviewed state. Whether a
past agent step was covered by a change is decided from the evidence chain
(`change_opened`/`change_reviewed`), not from this file.

## Guard policy (`guard-policy.json`, format `fabric-guard-policy`, version 3)

`{"format","version","overrides":{rule: decision},"custom":[…]}`. Version 2 added `custom`,
project rules with `id`, `description`, `decision` (`allow`, `ask`, `approve`, `deny`), `paths`
(globs over project-relative paths, `*` not crossing `/`), `commands` (case-insensitive
regular expressions) and `committed_only`. Version 3 added `tools` (globs over MCP tool calls
as `server/tool`). Older files are read (version 1 as having no project rules); Fabric writes
version 3.

## Evidence pack (format `fabric-evidence-pack`, version 1)

A directory written by `fabric evidence export`: `evidence.jsonl` (the chain's complete
records), `changes/<checkpoint, 6 digits>.md` (one report per evidenced checkpoint),
`tickets/change-<change, 4 digits>.md` (one report per change authorized with `--change`),
`README.md`, `summary.json` and `summary.sig`. `summary.json` lists the project, the chain's
record count and head, the evidenced checkpoints, the change numbers (`changes`, added within
version 1; older packs have none), the exceptions found, the BLAKE3 hash of every other file, and the
Ed25519 public key and its fingerprint (the first 16 hex characters of the key's BLAKE3
hash). `summary.sig` is the hex Ed25519 signature of `summary.json`'s exact bytes. The signing
key is generated on first export and kept in `<home>/keys/evidence-ed25519.key`.

## Not yet defined

Garbage collection, store format migrations.
