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
server, tool as `server/tool`, agent and whether it was authenticated (`agent_authenticated`),
session, action digest, decision, rule, and either
`arguments` with outcome `forwarded`, `held` or `blocked`, or the later record of a forwarded
call's outcome `ok` or `error`; `result` is the server's text, cut to 2,000 characters), and
`agent_registered` (name, scopes, token hash, by), `agent_revoked`, `payment_authorization`
(tool, agent and whether authenticated, digest, and `payment`: amount, currency, merchant,
AgentAuth's `decision` — `ALLOW`, `DENY`, `STEP_UP` or `UNAVAILABLE` — authorization code,
consent and reason; a forwarded `tool_call` carries the same `payment`),
`mandate_granted` (number, terms, granted by, OS account or key, `signed` when signed),
`mandate_used` (number, tool, agent, digest, amount, UTC day, the day's total and count,
via), `mandate_revoked`, and `policy_changed`. `approval_used` records `via` (`hook` or
`proxy`). A decision
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

## Agents (`agents.json`, format `fabric-agents`, version 1)

Per project: `{"format","version","required","agents":[…]}`. Each agent has `name`,
`token_hash` (BLAKE3 of the token; tokens are `fat_` and 64 hex characters, never stored),
`tools` (globs over `server/tool`; empty means any), `created_by`, `created_at` and
`revoked_at`.

## AgentAuth connection (`agentauth.json`, format `fabric-agentauth`, version 2)

Per project: `url`, `api_key_secret` (the name of a stored credential, never the key), and
`payments`: each with `tool` (`server/tool`), `amount` (argument path, dotted for nested
fields), `minor_units`, `currency`, `default_currency` and `merchant` (argument paths), and, since
version 2, `when`: conditions `{field, equals}` on argument values that must all hold.
Version 1 files are read.

## Credentials (`<home>/secrets/<name>`)

One file per credential, its value only, written with owner-only permissions on Unix (on
Windows, the user's profile ACLs). Names are lowercase letters, digits, `-` and `_`. Not
versioned, never recorded in evidence.

## Mandates (`mandates.json`, format `fabric-mandates`, version 1)

Per project: `{"format","version","mandates":[…]}`, rewritten durably under `mandates.lock`.
Each mandate has an `id`, its `terms` (`title`, `tools` globs over `server/tool`, optional
`agent`, `max_per_call` and `max_per_day` as decimal strings, `max_calls_per_day`, and
`expires` as `YYYY-MM-DD`), `granted_by`, `granted_at`, `granter_key` (fingerprint, when
signed), `revoked_at`, and `uses`: `at_unix`, `day` (UTC), `digest`, `tool`, `amount` (an
integer scaled by 10^6, or `null`) and `via` (`hook`, `proxy`).

## Sign-off files (version 1)

- `fabric-approval-request` (from `fabric approvals export`): `project` (`id`, `root`),
  `exported_at`, and `requests`, each with `id`, `digest`, `rule`, `reason`, `agent`,
  `session`, `tool`, `target`, `input` (exactly as the agent sent it), `requested_at` and
  `change_text` (the diff as the requester's Fabric rendered it).
- `fabric-review-request` (from `fabric review N --export`): `project`, `exported_at`,
  `change` (`id`, `title`, `paths`, `session`, `authorized_by`, `base_checkpoint`), `steps`,
  `snapshot`, `files` (path to content id, `null` if removed) and `diff`.
- `fabric-mandate-proposal` (from `fabric mandate grant ... --out`): `project`,
  `exported_at` and `terms`.
- `fabric-signed-decision` (from `fabric approver sign`): `payload`, `public_key` and
  `signature` (hex Ed25519 over the payload serialized as compact JSON with sorted keys). The
  payload is `{"kind":"approval","project","requests":[{"id","digest"}],"decision":"approve"|
  "reject","change":{"title","paths"}|null,"note","signed_at"}` or `{"kind":"review",
  "project","change","snapshot","files","decision":"accept"|"reject","note","signed_at"}` or
  `{"kind":"mandate","project","terms","decision":"grant"|"decline","note","signed_at"}`.

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

## Optional Cosigna agent development stores (ledger version 9, provider version 4)

These stores belong to `cosigna-agent`, not the Fabric object/manifest layout above. Default
directory: the OS local-data directory followed by `Cosigna/agent-dev`. No existing project
store is migrated. SQLite is authoritative for this isolated experiment, not a rebuildable
manifest index. Initialization accepts an empty database only, or a matching application ID
and version; foreign databases and unsupported versions are refused.

| File | `PRAGMA application_id` | `PRAGMA user_version` | Tables |
|---|---|---|---|
| `transactions.sqlite` | `0x43535458` | 9 | `budgets`, `tasks`, `reservations`, `events`, `agents`, `task_agents`, `agent_events`, `worker_jobs`, `evidence`, `operations`, `operation_holds`, `approvers` |
| `fixture-provider.sqlite` | `0x43534658` | 4 | `offers`, `orders`, `voids`, `fixture_payments`, `fixture_payees`, `money_attempts`, `money_voids`, `fixture_cases` |

Connections enable foreign keys, WAL journaling, `synchronous=FULL`, and a five-second busy
timeout. Mutations that need a shared state check use `BEGIN IMMEDIATE`. The service tests
reopening, explicit interruption boundaries and real process termination at four fixed
fixture planning/claim/acceptance/confirmation boundaries. Physical power-loss/full-disk
and arbitrary process-kill durability across live providers have not been established.

Ledger version 2 added the three agent tables. Version 3 added `worker_jobs` and its due-job
index. Version 4 added `evidence` (below). Version 5 added money operations, their holds and
agent operation grants. Version 6 adds case binding: the nullable `operations.case_id` column
with index `operations_case(owner, case_id)`, and the optional `case_id` request field and
`require_case` grant field (below). Version 7 adds the `approvers` table (named approvers,
below). Version 8 adds no tables: budget JSON may carry `second_approval_above_minor` and
operation JSON `first_approval` (two-person approval, below). Version 9 adds no tables: a
proposal's `facts.case` may carry `system`, the case source it was read from (case sources,
below). Opening an application-matching v1-v8 ledger creates the missing tables and columns
and updates `user_version` to 9 in one transaction. Existing
budget/task/reservation/event/agent values, quote digests and attempt keys are unchanged; no
historical task is automatically enrolled. Provider version 2 added `voids`, version 3 added
the synthetic money rail and version 4 adds `fixture_cases`; older provider stores gain them in
one transaction, and provider versions above 4 are refused. Versions above 9, foreign
application IDs and unidentified nonempty databases are refused. Older executables refuse v9
(and may refuse grants or operations that carry the new fields); do not downgrade
or drop tables in user data. Stop all users of a profile and back it up before upgrading.
Use a separate gateway development profile instead of repurposing an active phone-lab profile.

- `worker_jobs`: `task_id` primary key/FK, `owner`, `state` constrained to `pending`,
  `running`, `paused`, `done`, integer `revision`, Unix-seconds `next_at`/`updated_at`,
  integer `reconcile_count`, nullable fixed-code `reason`. Index `worker_due` covers
  `(state,next_at,task_id)`. Owner enrollment creates the record, not task approval.
  Claims commit `running` before external work. Completion uses the claim's revision so
  a later owner pause cannot be overwritten. A reopened worker pauses a running draft
  as `planning_interrupted`; other interrupted jobs return to pending and derive their
  next action from the existing task/approval/attempt. Unknown holds are unchanged.
  At most 1,000 pending/running jobs; CLI ticks one job/second, waiting polls five seconds,
  unresolved attempts back off from ten to 300 seconds and pause after twelve unresolved
  steps. Explicit owner resume resets that allowance, never approval or attempt keys.
- `worker.lock` is an OS-held exclusive lock for one local worker per ledger; `planner.lock`
  bounds concurrent inference across API/worker processes using this ledger. They contain
  no authority or secrets, are not removed on exit, and must not be deleted while a holder
  is alive. OS lock release on process exit enables recovery. Network filesystems are unsupported.

- `agents`: `id` primary key, `owner`, `name`, `budget_id` FK, JSON `permissions`
  (unique values `plan` and/or `execute`), Unix-seconds `expires_at`, Boolean `revoked`,
  unique `token_hash`. The token is `agentkey_` plus 64 random hex characters. Only its
  BLAKE3 hex digest is stored; plaintext is returned once in the owner's registration
  response and omitted from metadata/list responses. Permissions are immutable; revoke
  and register a replacement to change them. Agent expiry cannot exceed its budget expiry.
- `task_agents`: task ID primary key/FK and agent ID FK. Association is created in the
  same transaction as the task and its creation event. Existing tasks have no association.
  A registered credential can access only associated tasks and its one registered budget.
  Owner-scoped idempotency remains unchanged; a collision with another agent's task returns
  an error, never that other task. Gateway task envelopes expose association separately;
  the existing v1 task JSON is unchanged.
- `agent_events`: globally increasing sequence, agent ID FK, kind (`registered`, `revoked`)
  and Unix-seconds timestamp. Revocation is idempotent. No credential/hash is recorded here.
  These events are unsigned local records, not cryptographically signed consent receipts.

Revocation/expiry are checked on authentication and again at scope-sensitive writes, including
the atomic submission claim. Bound tasks cannot gain a new approval, plan or submission after
their agent expires/revokes, even through privileged legacy roles. Already claimed external
work can complete and be recorded. Revocation does not release a held budget or undo payment;
the owner can cancel an unsubmitted task or reconcile an existing attempt. Status/recovery by
the owner remains possible after revocation. The legacy local planner/executor credentials
remain privileged development roles; never hand those keys or the credential file to agents.

- `budgets`: primary key `id`, owner, JSON budget terms (`currency`, integer `limit_minor`,
  integer `per_purchase_minor`, merchant allowlist, Unix-seconds expiry) and revoked flag.
- `tasks`: primary key `id`, owner, `idem`, fingerprint, JSON task state. `(owner, idem)` is
  unique. Fingerprint is BLAKE3 of the serialized typed `TaskInput`, including budget ID,
  request, idempotency key and required features. A key replay returns its existing state
  only when the fingerprint matches. State includes a monotonic optimistic version, stage,
  input, optional clarification/quote/approved digest/stable attempt key/receipt.
- `reservations`: task primary key (FK), budget ID (FK), positive integer amount, state
  `held`, `spent` or `released`. Approval checks available funds and inserts/updates the
  held reservation in the same transaction. Confirmed acceptance changes held to spent;
  definite rejection or pre-submission cancellation changes held to released. Unknown
  results remain held, including an absent provider lookup result. Reservations are not
  released merely because a quote expires or a budget is revoked.
- `events`: globally increasing integer sequence, task ID (FK), event kind and Unix-seconds
  timestamp. No model reasoning, credentials or raw HTTP payloads. These rows are unsigned
  local history; the `evidence` chain below is the tamper-evident record.
- `evidence` (version 4): `seq` integer primary key starting at 1, unique lowercase hex
  `hash`, `body` text. Triggers abort every `UPDATE` and `DELETE`. Each body uses the engine's
  evidence record format (`{"v":1,"seq":N,"prev":"<hex>","at":"<RFC 3339 UTC>","event":{…}}`,
  `hash` = BLAKE3 of the body's exact bytes, `prev` of the first record = 64 zeros) and is
  appended in the same transaction as the change it records. Record 1 is
  `gateway.ledger.created`, or `gateway.ledger.upgraded` with `from_version` and the IDs of
  budgets, tasks and agents that predate evidence (`unevidenced`); their earlier history is
  not reconstructed. Event kinds:
  - `gateway.budget.{created,revoked}`: `by`, `budget`, `owner`, `terms`, `revoked`,
    `budget_digest` (BLAKE3 of the stored budget JSON).
  - `gateway.agent.{registered,revoked}`: `by`, `agent`, `grant` (`owner`, `name`, `budget`,
    `permissions`, `expires_at`, `revoked`) and `agent_digest` (BLAKE3 of the compact JSON
    array of the agent row's owner, name, budget ID, permissions text, expiry, revoked flag
    and token hash). Neither the token nor its hash appears in any record.
  - `gateway.task.<step>` for `task_created`, `plan_applied`, `owner_approved`,
    `submission_started`, `provider_result`, `provider_rejected_quote`, `submission_unknown`
    and `owner_cancelled`: `by`, `task`, `budget`, `agent` (assigned agent or null),
    `version`, `stage`, `request_digest` (BLAKE3 of the request text, which is not stored),
    `idempotency_key`, `quote` (`digest`, `offer`, `merchant`, `quantity`, `total_minor`,
    `currency`, `expires_at`) or null, `approved_digest`, `attempt_key`, `receipt`
    (`order`, `accepted`, `total_minor`, `currency`, `simulated`) or null, `reservation`
    (`amount_minor`, `state`) or null, and `task_digest` (BLAKE3 of the stored task JSON).
  - `gateway.worker.{enrolled,paused,resumed}`: `by`, `task`. Scheduler delegation only.
  - `by` is `{"owner", "role": "owner"|"planner"|"executor", "agent"}`; `agent` is the
    registered agent whose credential made the change, or null.
  Refused operations change nothing and record nothing. `cosigna-agent evidence verify`
  re-hashes every record, checks `seq`/`prev` and the first record's kind, then compares each
  current budget, task (with its reservation and agent assignment) and agent with its latest
  record: state changed outside the service, unrecorded rows and recorded rows that are gone
  are exceptions (exit code 5). It cannot detect a consistent rewrite of the whole chain by
  someone with write access to the database; signing and external anchoring are not yet
  implemented. `cosigna-agent evidence export --out FILE` writes `<hash> <body>` lines (the
  engine's `evidence.jsonl` layout) to a new file. The verify report schema is
  `cosigna.ledger-evidence/v1`: `records`, `head`, `chain_intact`, `state_matches`,
  `unevidenced` counts, `exceptions` (`seq`, `subject`, `problem`; at most 100) and
  `exceptions_truncated`.
- `offers`: ID and typed JSON synthetic offer, including all-in unit price, currency,
  seller, features, fulfillment, terms, revision and `simulated: true`.
- `orders`: stable attempt key primary key, quote digest and typed JSON synthetic receipt.
  Reuse with a different digest fails. No live provider state is present.
- `voids` (provider version 2): attempt key primary key and quote digest. Written only by an
  owner's `resolve` for an attempt with no order; a later submission with that key returns
  `Voided` and creates no order. An attempt that has an order returns its receipt instead.

Owner resolution (`POST /v1/tasks/{id}/resolve`, `POST /gateway/v1/tasks/{id}/resolve`,
owner only, `{}`) applies only to `submitting`/`unknown` tasks; others are returned
unchanged. The gateway asks the provider to void the attempt: an observed receipt settles
the task as usual (`provider_result`); a void or definite rejection makes it `declined` and
releases the reservation (`provider_voided` / `provider_rejected_quote`); a provider that
cannot answer leaves the task unknown and held. Lookup alone never releases a hold.

### Money operations (ledger version 5)

Refunds and payouts requested by registered agents or the owner, through
`POST /gateway/v1/operations` (envelope schema `cosigna.operation/v1`). Tables:

- `operations`: `id` (`op_` + 64 hex) primary key, owner, budget FK, nullable agent FK,
  `idem`, request fingerprint (BLAKE3 of the typed request), JSON operation; `(owner, idem)`
  unique. A replay with the same key and fingerprint returns the stored operation; a different
  request under the same key is a conflict.
- `operation_holds`: operation primary key/FK, budget FK, positive amount, state `held`,
  `spent` or `released`. Budget totals add these holds to purchase reservations: purchases and
  operations draw on one budget.
- `agents.operation_grant`: nullable JSON `{kinds, rails, auto_approve_minor, require_case?}`.
  Absent grants no operation rights; a grant requires the `execute` permission and is immutable
  like other permissions. Agents registered before v5 have none. `require_case` (v6) is written
  only when true; absent means false, so grants written before v6 serialize identically.

Request fields (unknown fields rejected): `budget_id`, `kind` (`refund` with `source`, or
`payout` with `payee`), `rail`, `amount_minor`, `currency`, `reason` (at most 500 characters),
`idempotency_key`, and optionally `case_id` (v6; refunds only; written only when present, so
earlier requests keep their fingerprints). The rail checks the request against its own records before any decision
(the payment exists and has that much left to refund; the payee is registered; the currency
matches). A failed check is recorded as `denied` with the rail's reason (`source_not_found`,
`exceeds_refundable`, `currency_mismatch`, `payee_unknown`). Otherwise policy decides, in order:
`not_granted` (agent without a grant covering kind and rail), `currency_not_in_budget`,
`over_per_operation_limit` (budget `per_purchase_minor`), `budget_exhausted`; then
`within_agent_auto_limit` approves automatically and anything else is held
(`needs_owner_approval`; owner-initiated requests are always held). The decision is stored as
`{"outcome": "auto_approved"|"held"|"denied", "rule": ...}`.

Case binding (v6) is then applied, and can only tighten that decision. A case is the
business's own record of what a customer is owed: `id`, `customer`, `source` (the payment),
`currency`, `owed_minor`. Cases come from the gateway's one connected case source (below); by
default that is the synthetic `fixture_cases` table (provider version 4). In order: a refund with no
`case_id` from an agent whose grant has `require_case` is denied (`case_required`); a `case_id`
with no case record is denied (`case_not_found`); a refund whose `source`, `currency` or the
rail's verified `customer` differs from the case is held (`case_mismatch`); a refund larger
than the case's `owed_minor` minus what earlier operations of this owner already hold or spent
against the case (`committed_minor`) is held (`exceeds_case_owed`). A matching refund keeps the
earlier decision, so the agent's limits and the budget still apply. When a case is named, the
proposal's `facts.case` records `{id, system, customer, source, owed_minor, currency,
committed_minor}` (`system` from v9: `fixture`, `csv` or the configured name), so the owner
signs what the case said and where it came from. Case binding narrows, but does not remove, the class of
mistakes within an agent's limit: it catches a wrong payment or an amount above what is owed,
not an amount below it, and it is only as correct as the case records.

The proposal is the exact operation (`operation_id`, `kind`, `rail`, `source`, `payee`,
`amount_minor`, `currency`, the rail's verified `facts`, `issued_at`, `expires_at` at most 24
hours later and never past the budget). Its digest is lowercase BLAKE3 hex of its compact
serialization in that field order; owner approval (`POST .../approve {"proposal_digest"}`) must
match it. Approval (automatic or by the owner) places the hold in the same transaction. The
gateway then claims one attempt (`opattempt_` key) and submits to the rail with server-held
credentials; the rail re-checks its records and may still reject. Stages: `held`, `approved`,
`submitting`, `unknown`, `succeeded`, `failed` (rail declined, rejected or voided; no money
moved), `denied`, `rejected` (by the owner before submission). Unknown results stay held;
`reconcile` only looks up, and only the owner's `resolve` can settle one, through the rail's
void as for purchases. Revoking the agent or the budget prevents new requests and new
submission claims.

Owner routes: `GET /gateway/v1/operations[?stage=held|unknown]`, `POST .../{id}/approve`,
`.../reject`, `.../resolve`. Owner or originating agent: `GET .../{id}`, `.../execute`,
`.../reconcile`. Other agents cannot see an operation.

Evidence kinds `gateway.operation.{auto_approved,held,denied,owner_approved,owner_rejected,
approver_approved,approver_rejected,first_approved,
submission_started,rail_result,rail_rejected,rail_voided,submission_unknown}` record `by`,
`operation`, `budget`, `agent`, `version`, `stage`, `decision`, `request` (with
`reason_digest` instead of the reason text, and `case_id` from v6), `proposal` (`digest`, `facts`, `expires_at`),
`approved_digest`, `approved_by` (`policy`, `owner` or, from v7, `approver`), `attempt_key`, `receipt`, `hold` and
`operation_digest` (BLAKE3 of the stored operation JSON). `evidence verify` checks each
operation and its hold against its latest record. An agent's `agent_digest` includes its
operation grant only when one exists, so digests recorded before v5 remain valid.

The fixture rail lives in `fixture-provider.sqlite` (provider version 4): `fixture_payments`
(synthetic payments with refunded totals), `fixture_payees`, `money_attempts` (attempt key,
proposal digest, typed receipt with `simulated: true`), `money_voids` and `fixture_cases`
(synthetic case records: `id`, `customer`, `source`, `currency`, `owed_minor > 0`).

The `stripe_test` rail (Stripe test-mode refunds; payouts are refused with
`kind_not_supported_on_rail`) is connected with `serve --stripe-test` or `worker --stripe-test`.
The key comes from `COSIGNA_STRIPE_TEST_SECRET_KEY` or the engine's stored `stripe-test`
credential (`<FABRIC_HOME>/secrets/stripe-test`); only `sk_test_`/`rk_test_` keys are
accepted and the key is never printed or recorded. Requests go only to
`https://api.stripe.com`; `COSIGNA_STRIPE_API_BASE` may only name a numeric loopback address
(for tests). The check retrieves the PaymentIntent (`succeeded`, test mode, currency) and its
refunds (succeeded, pending and requires-action refunds count as already refunded; more than
one page is refused rather than guessed). Submission creates a refund with idempotency key
`cosigna:<attempt key>` and metadata `cosigna_attempt`, `cosigna_proposal_digest`,
`cosigna_operation`, then retrieves the refund and accepts it only if amount, currency,
PaymentIntent and metadata match the proposal. `succeeded` settles as succeeded; `failed`
or `canceled` as failed; `pending` stays unknown and held. A Stripe validation refusal
(`invalid_request_error`, for example more than is refundable) fails the operation and
releases its hold; rate limits, server errors, idempotency conflicts and network failures
are unknown. Reconcile lists the PaymentIntent's refunds for the attempt's metadata. Stripe
cannot void a refund that was never created, so `resolve` first looks the attempt up and, if
it is absent and the attempt was claimed less than 23 hours ago (Stripe keeps idempotency
keys for 24 hours), repeats the original request with the same key and parameters: Stripe
returns the existing refund or completes the approved refund exactly once. After that window
the operation stays unknown and held. Receipts say `simulated: true` (test mode moves no real
funds). Operation JSON includes `submitted_at` (Unix seconds of the attempt claim).

Quote digest: lowercase BLAKE3 hex of compact `serde_json` serialization of the `Quote`
struct in declaration order: `id`, `task_id`, `offer`, `quantity`, `total_minor`, `issued_at`,
`expires_at`. The nested offer field order is `id`, `merchant_id`, `title`, `currency`,
`unit_total_minor`, `features`, `fulfillment`, `terms`, `revision`, `simulated`. This binds
approval to the complete quote and its expiry. API clients receive the digest directly;
arbitrarily reserializing JSON is not an equivalent hashing protocol.

A quote is valid for 300 seconds from planning (never past the budget). Approving or submitting
it at or after `expires_at` is refused with a conflict whose message starts with `quote_expired`
(the worker pauses with that reason). Planning a `quoted` or `approved` task again after its
quote expired, and before any attempt, issues a fresh quote with a new digest, releases any
reservation and needs a new owner approval; before expiry, planning again returns the task
unchanged. The pipeline's `advance` does not re-quote: an expired quote stays `needs_review`
until a person plans again or cancels.

`credentials.json`: strict JSON `{schema, owner, planner, executor}`, with
`schema: "cosigna.agent/v1"` and independent role tokens (`owner_`, `planner_`, `executor_`,
then 64 random hex characters). Created exclusively and synced without overwriting an existing
file. Unix mode 0600; Windows inherits parent ACLs. This is a local developer credential file,
not production user identity. Store it outside source trees and do not log or snapshot it.

HTTP response schema `cosigna.agent/v1`: success envelope has `schema`,
`simulated_provider: true`, `data` and `approval_digest` (null unless data contains a quote).
Public health has `schema`, `simulated_provider: true` and `status`. Error envelope has
`schema` and `error: {code, message}`. Input schemas reject unknown fields; purchase plan
fields are only `offer_id`, `quantity`, `clarification`. No plan can contain spending authority.

### Named approvers (ledger version 7, `cosigna.approver/v1`)

People the owner authorizes to decide held money operations, each with their own credential and
a limit. Table `approvers`: `id` (`approver_` + 64 hex) primary key, `owner`, `name` (1-100
characters, no control characters), `currency` (three uppercase letters), `max_approval_minor`
(1 to the maximum amount), `created_at` (Unix seconds), `revoked` (0/1), unique `token_hash`.
The credential is `approverkey_` plus 64 random hex characters; only its BLAKE3 hex digest is
stored, and the plaintext is returned once in the registration response. Revocation is
permanent.

Owner routes: `POST /gateway/v1/approvers {name, currency, max_approval_minor}` (unknown fields
rejected) returns `{approver, token}`; `GET /gateway/v1/approvers` lists without credentials;
`POST /gateway/v1/approvers/{id}/revoke`. Responses use envelope schema `cosigna.approver/v1`.

An approver credential reaches only: `GET /gateway/v1/approvers/me`, `GET /gateway/v1/agents`
(each agent's `id` and `name` only, so an inbox can say which agent asked), `GET /gateway/v1/operations
[?stage=held|unknown]`, `GET .../operations/{id}`, `POST .../{id}/approve {proposal_digest}`
(which then executes, as for the owner) and `POST .../{id}/reject`, and for purchases
`GET /gateway/v1/tasks/{id}` and `POST .../tasks/{id}/approve {quote_digest}` (approval only;
the agent or worker executes, as after an owner's approval), for the owner who registered
them. Everything else is not found. Approval checks, inside the deciding transaction: the
approver still exists and is not revoked (a revocation committed earlier wins even if the
approver authenticated before it), the proposal currency equals the approver's currency and its
amount is at most `max_approval_minor`; otherwise the conflict message starts with
`above_approver_limit`. Rejection is allowed at any amount, since refusing never moves money.
The owner's own authority is unchanged. An approver's approval sets `approved_by: "approver"`
and the operation's `approver` field (omitted otherwise, so earlier operation JSON and digests
are unchanged), and is recorded as `gateway.operation.approver_approved` or
`approver_rejected`, with `by: {owner, role: "approver", agent: null, approver}`. The
`approver` key in `by` appears only on an approver's records.

Evidence kinds `gateway.approver.{registered,revoked}` record `by`, `approver`, `terms`
(`owner`, `name`, `currency`, `max_approval_minor`, `created_at`, `revoked`) and
`approver_digest` (BLAKE3 of the compact JSON array of the stored row, including the token
hash). `evidence verify` checks every approver row against its latest record, so a limit raised
or a revocation undone outside the service is reported. The independent verifier checks that
each approver decision was made by an approver registered earlier in the chain, not revoked,
of the same owner, and that an approval is in their currency and within their limit.

A purchase quote approved by an approver is checked the same way (the quote's currency and
total against the approver's) and recorded as `gateway.task.approver_approved`; the task JSON
is unchanged. The two-person threshold applies to money operations, not to purchase quotes.
The console signs approvers in with their own key (see `console/README.md`).

### Two-person approval (ledger version 8)

A budget may set `second_approval_above_minor` (0 to the maximum amount; written only when
present, so earlier budget JSON and digests are unchanged). For an operation on that budget
whose amount is above it:

- Policy never approves it automatically, even within the agent's own limit: after the budget
  checks, `decide` holds it with rule `needs_two_approvals`. The exposure report's
  `largest_single_minor` is capped at the threshold accordingly.
- The first person's approval (the owner, or a named approver within their currency and
  limit) is stored as the operation's `first_approval: {by: "owner"|"approver", approver?,
  digest, at}`, recorded as `gateway.operation.first_approved`, and leaves the operation
  `held` with no budget hold. The approve route then returns without executing.
- The completing approval must be for the same digest, by a different person, also within
  their own limit; the owner counts as one person and each approver as one. The same person
  again is refused with a conflict starting `second_approver_required`. The completing
  approval places the hold, records `owner_approved` or `approver_approved` as before (with
  `first_approval` in the record), and executes.
- Either person, or any other, may reject before submission.

Operation records carry `first_approval` only when it is set, so records of operations that
need one person keep their earlier shape. The independent verifier checks that no automatic
approval exceeds the threshold, and that each approval above it follows a `first_approved`
record of the same operation by a different person, both within their limits.

### Policy replay (`cosigna.replay/v1`; no database)

`cosigna-agent replay --history <csv> --policy <json> [--cases <csv>] [--text]` runs a proposed
policy over past operations, locally; it reads files only and opens no ledger or network
connection. Each operation, in time order (then id), goes through the gateway's own
`operations::decide` and `apply_case` with a synthetic budget and grant built from the policy,
so replay and the live gateway decide identically (a test runs one history through both).
Before deciding, a refund larger than its payment's amount less what replay has already paid on
that payment is denied `exceeds_refundable`, as the rail would; this is checked only when the
row gives `payment_amount_minor`. Operations held for a person are assumed approved (they
happened) and draw on the budget and case like automatic ones; denied operations are not paid.

Policy JSON (unknown fields rejected): `budget` `{currency, limit_minor, per_operation_minor,
second_approval_above_minor?, renew_every_days? (1-3660; a fresh budget every N days from the
first operation)}` and `grant` `{kinds, auto_approve_minor, require_case?}`.

History CSV (RFC 4180, UTF-8, optional BOM; header names matter, column order does not):
`id`, `at` (Unix seconds or RFC 3339 UTC `YYYY-MM-DDTHH:MM:SS[.fff]Z`), `kind` (`refund` with
`source`, or `payout` with `payee`), `amount_minor`, `currency`, and optionally `customer`,
`case_id`, `payment_amount_minor`, `mistake` (`yes`, `true` or `1`). Identifiers are 1-100 of
`A-Z a-z 0-9 _ - . :`. Errors name the row and the field. Cases CSV: `id`, `customer`,
`source`, `currency`, `owed_minor`.

Output `{"schema":"cosigna.replay/v1","data":{...}}`: `policy`, `operations`, `from`, `to`,
`automatic`, `held_for_a_person` and `denied` (`{count, amount_minor}`, amounts in the budget's
currency only), `other_currency` (count), `paid_without_a_record` (`{count, amount_minor}` of
operations paid automatically with no `case_id` to check them against), `held_per_week` (one decimal), `by_rule`
(`{outcome, rule, count, amount_minor}`), `mistakes` when any row is labelled (`labelled`,
`stopped`, `stopped_amount_minor`, `paid_automatically` ids), `replayed` (per operation: `id`,
`at`, `amount_minor`, `currency`, `outcome`, `rule`, `case_check`: `checked` when it names a
case found in the case records, else `unknown`, meaning whether it was owed cannot be told from
the data, whatever the outcome) and three fixed `notes` stating the
assumptions. `--text` prints a short summary instead. Example data: `examples/replay/`.

### Operation timeline (`cosigna.timeline/v1`; no database migration)

`GET /gateway/v1/operations/{id}/timeline` (the owner, or a named approver of the same owner;
agents get not found) tells one operation's story from its evidence records only: the records
whose event names the operation, in sequence order. Envelope `{"schema":"cosigna.timeline/v1",
"data":{...}}`; `data` has `operation`, `kind`, `amount_minor`, `currency`, `counterparty`,
`case_id`, `stage`, `entries`, `now` (one sentence on where it stands), `next` (what can happen
and who can do it; empty once final) and `evidence_ok` (`evidence verify` passes for the whole
ledger). Each entry cites its record: `seq`, `hash`, `at` (the record's time), `step` (the
event kind without `gateway.operation.`), `who` (`role`: `agent`, `owner`, `approver` or
`gateway`, with the agent's or approver's `id` and current `name` where there is one) and a
plain-language `summary` that names the rule in words (for example "above the two-person limit:
two different people must approve"). The summaries are derived text, not stored authority;
the records they cite are the evidence.

### Case sources (`cosigna.cases/v1` configuration; ledger version 9)

`serve --cases <file>` and `worker --cases <file>` read case records from the source a JSON
file describes, instead of the synthetic case book; without the flag nothing changes. One
source is connected at a time. The file has `schema: "cosigna.cases/v1"` and `source`; unknown
keys are refused.

- `source: "csv"`, `path` (relative to the file): columns `id`, `customer`, `source`,
  `currency`, `owed_minor`, as for replay. Read once at startup; a repeated `id` is refused.
  Listable, so exposure counts its open cases. `system` is `csv`.
- `source: "http"`: `name` (letters, digits, `_`, `-`; recorded as `system`), `url` with
  `{id}` exactly once in its path, `auth`, optional `id_prefix`, and `fields`. `url` must be
  `https://` with no user information (plain `http://` only on `127.0.0.1:<port>`, for
  tests); redirects are not followed; responses are capped at 512 KiB and 10 seconds. `auth`
  is `{scheme: "basic", username, secret_env}` (no `:` in the username), `{scheme: "bearer",
  secret_env}` or `{scheme: "header", name, secret_env}`; the secret is read once from the
  named environment variable at startup and is never written to files, logs or evidence. A
  case id must start with `id_prefix`; the rest (already limited to letters, digits, `_`, `-`)
  replaces `{id}`. Ids without the prefix are not looked up and count as missing.
  `fields.customer`, `fields.source` and `fields.owed` are paths; `fields.owed_unit` is
  `minor` (integer) or `major` (digits with at most two decimals, read exactly, no float);
  exactly one of `fields.currency` (a path) and `fields.currency_fixed` (three uppercase
  letters). Paths are JSON pointers whose segments may also be `[key=value]`: the one array
  element whose `key`, as text, equals `value`. Two matches are ambiguous and not read.

An HTTP answer of 404 is a missing case (`case_not_found`). Any other failure fails the
request with `case_records_unavailable` (HTTP 503, `Error::CaseSource`) before anything is
decided or recorded: no answer, any other status, a body that is not JSON, a missing or
non-text value, an id-like value (`customer`, `source`) outside letters, digits, `_` and `-`,
a currency that is not three letters, or an amount outside 1 to 10^12 minor units. An HTTP
source cannot be listed: the exposure report then counts case-bound agents up to their budget
and adds a fourth note saying so. The check is only as independent as the fields it reads:
they must be fields the paying agent cannot write.

### Worst-case exposure (`cosigna.exposure/v1`; no database migration)

`GET /gateway/v1/exposure` (owner only; agents get not found) reports the most money the
owner's agents can still move without a person, from one read transaction over budgets, holds,
agents and case commitments. It is derived from the operation rules above, so no sequence of
agent requests can move more while grants, budgets and case records stay as they are: owner
requests are always held; an agent draws only on its own budget; a budget never pays past its
limit; an agent whose grant is refunds-only with `require_case` can pay only what open cases
still owe. It is an upper bound, not a forecast: rails refuse more than it counts (a refund
needs a payment with that much left), and new budgets, agents or cases can raise it. Purchases
(tasks) always need the owner's signature and add nothing.

Envelope `{"schema":"cosigna.exposure/v1","data":{...}}`; `data` has `at`, `notes` (three
fixed sentences stating these limits, and a fourth when the case source cannot be listed),
and:

- `agents[]`: `agent_id`, `name`, `budget_id`, `currency`, `exposure_minor` (this agent alone;
  agents on one budget share it), `largest_single_minor` (`min(auto_approve_minor,
  per_purchase_minor, exposure_minor)`), `limited_by`, `must_name_case`, optional
  `if_case_required_minor` (refunds-only agents that need not name a case: their exposure if
  they had to) and `until` (earlier of agent and budget expiry, present when exposure > 0).
- `budgets[]`: `budget_id`, `currency`, `available_minor` (limit less held and spent),
  `approved_minor` (operation holds still `held`: approved, not settled), `exposure_minor` (the
  largest of its agents'), `limited_by`, `agents` (ids with exposure > 0), optional `until`.
- `currencies[]`: `currency`, `without_a_person_minor`, `approved_minor`, `open_cases_minor`
  (sum over cases in that currency of `owed_minor` less this owner's committed amount, floored
  at 0). `without_a_person_minor` is the sum over budgets any non-case-bound agent can reach,
  plus the sum over budgets only case-bound agents can reach capped at `open_cases_minor`
  (all case-bound agents share one case book).

`limited_by` is one of `budget`, `cases`, `no_grant`, `agent_inactive` (revoked, expired or
without `execute`), `budget_inactive` (revoked or expired), `no_automatic_room` (nothing fits
under the automatic limit, or nothing is left). Amounts are integer minor units, never summed
across currencies. With no case book connected, `open_cases_minor` is 0 and case-bound agents
can move nothing (their requests are refused).

### Transaction pipeline API version 1

The pipeline adds no database tables, task fields, stored stages or ledger migration. Its ID
is the existing task ID. `POST /gateway/v1/pipelines` accepts existing strict `TaskInput`;
GET `/gateway/v1/pipelines/{id}` reads; POST `.../{id}/advance` accepts only `{}`. Creation
is owner-scoped idempotent task creation followed by one allowed step. Replays recover the
same task; quote/input waiting states do not repeatedly call a model. An approved task may
execute, while submitting/unknown tasks only reconcile. Exact owner approval is still a
separate existing operation. Core scope checks, reservations and attempt claims are unchanged.

Success envelope `cosigna.pipeline/v1` contains `schema`, `simulated_provider:true`,
`pipeline_id`, nullable `agent_id`, `status`, `next_action`, nullable `blocked_reason`,
nullable `approval_digest`, and `data` with unchanged task JSON. Status values:
`ready_to_plan`, `needs_input`, `awaiting_approval`, `ready_to_execute`, `reconciling`,
`needs_review`, `confirmed`, `declined`, `cancelled`. Next actions are respectively
`advance`, `provide_input`, `owner_approve_quote`, `advance`, `advance`, `review_or_cancel`,
`none`, `none`, `none`. Review reasons are `budget_inactive`, `agent_inactive` or
`quote_expired`. These are derived views, not independently stored authority. Review applies
only before submission; an existing attempt stays recoverable after authority/quote expiry.
Errors on pipeline routes retain `{schema,error:{code,message}}` with the pipeline schema.
Other gateway, mobile and legacy envelopes retain their existing schema identifiers.

`cosigna.pipeline-demo/v1` reports a simulated fixture flow: schema, simulated flag,
planner, synthetic owner approval source, pipeline ID, ordered stage names, replay status,
synthetic order count and existing balance object. No bearer/hash/payment credential appears.
`cosigna.roadmap-sync/v1` is a timestamped repository project snapshot (`captured_at`, project
URL and item title/state/URL/item ID), not payment state or evidence of feature completeness.

### Worker API version 1

Owner-only GET/POST `/gateway/v1/pipelines/{id}/worker` uses `cosigna.worker/v1`.
POST accepts only `{action}` with `enroll`, `pause` or `resume`. Success contains `schema`,
`simulated_provider:true`, and `data` with the worker job fields above (excluding owner).
Errors use `{schema,error:{code,message}}` with the worker schema. Registered agent/legacy
planner/executor credentials cannot control or inspect jobs. Enrollment replay never resumes
a paused job; only paused jobs can resume. No worker route creates approval, changes budgets
or replaces attempt keys. Existing pipeline/task envelopes retain their versions.
The worker CLI emits the same success envelope for processed jobs, and no request, model
response, reasoning, bearer/hash or payment credentials. See [operations](GATEWAY_OPERATIONS.md).

Refund recovery (`cosigna.recovery/v1`; no database migration). Each loop, the worker also
checks money operations sent to a rail whose result is not recorded (`submitting` or
`unknown`, `submitted_at` at least 30 seconds ago, so an execute still in flight in the
gateway is not touched), for every owner, without enrollment. A check is a rail lookup by
attempt key only: nothing is sent again, voided or resolved. A result is recorded as
`rail_result`, as a manual reconcile would; an operation still `submitting` with no result
(left by a crash) becomes `unknown`, recorded once as `submission_unknown`; an `unknown` one
with no result is left unchanged and adds no evidence record. Checks per operation back off
5, 10, 20, 40, 80, 160 seconds, then every 300 seconds while unsettled. The pacing state is
in memory: a restart starts checking again, which is safe because checks only read. The
worker prints one line `{"schema":"cosigna.recovery/v1","data":{operation_id, stage, checks,
needs_person}}` when an operation settles, and once at the 12th check without a result
(`needs_person: true`); settling it then is the owner's `resolve`.

### Optional batch verification report version 1

The Apache lab writes non-authoritative artifacts below its ignored
`.runs/reports/<first-24-hex-of-SHA256-Airflow-run-id>/` directory. These are test reports,
not payment state, production telemetry or signed audit evidence. No existing ledger/API
schema changes. `crash.json`, `rules.json`, `service.json` use `cosigna.verification/v1`:
`schema`, `simulated:true`, `suite`, `status` (`passed`/`failed`), Boolean `log_truncated`,
and `checks` containing `check`, `status`, nullable `exit_code`, nullable fixed-code `reason`
(`nonzero_exit`, `timeout`, `cannot_start`) and integer `duration_ms`. Durations describe the
verification run, not an application latency guarantee. Per-suite UTF-8 logs are at most
65,536 bytes; runtime command environments are allowlisted and stores are synthetic/temp.
`summary.json` uses the same schema/simulated/status fields, `suite:"summary"`, and `suites`
containing name/status for `crash`, `rules`, `service`. Missing, malformed, incomplete or
failed suite results fail the summary task. Reports are replaced through a sibling temporary
file and rename; no transaction-durability or tamper-resistance claim applies to artifacts.

### Headless gateway API version 1

`/gateway/v1/` is a new API, not a change to `/v1/` or the mobile/test merchant envelopes.
Its success envelope is `{schema:"cosigna.gateway/v1",simulated_provider:true,agent_id,
approval_digest,data}`. `agent_id` is populated for associated task responses, otherwise
null; `approval_digest` is populated for a task quote, otherwise null. Errors have `schema`
and `error:{code,message}` with the gateway schema. All POST input schemas reject unknown
fields and are bounded to 16 KiB. The loopback server retains exact Host, browser-Origin,
JSON content-type and no-store response controls. It serves up to eight requests at once, so
a slow client, rail or planner call holds one worker rather than the whole gateway; SQLite still
serializes writes. A connection to a store already at the current version opens without a write
lock; creation and upgrades take it. Eight clients that stall mid-request can still occupy every
worker: a shared deployment needs a front proxy with request timeouts (G13).

Owner control endpoints create budgets, register/list/read/revoke agents, and approve or
cancel tasks. Registration input is `{name,budget_id,permissions,expires_at}` and returns
`data:{agent,token}` once. No API can recover that plaintext token later. Registered agent
credentials can access their budget, filtered catalog, own task reads/events, creation and
planning with `plan`, and exact-approved execution with `execute`. They cannot approve,
manage agents/budgets or authenticate to the legacy API. Execute accepts only `{}`; models
cannot select a connector URL, payment amount or a fixture failure scenario through it.
The owner can explicitly execute an approved task, including one from a plan-only agent.
Owner or legacy privileged executor operations still honor the task's agent revocation.

This is local bearer authentication to a development service, not production user identity,
a scoped payment credential, a signed one-time authorization token or AP2 conformance.
`cosigna.gateway-demo/v1` is an isolated fixture report: schema, simulated flag, planner,
synthetic approval source, public agent metadata, Boolean checks, synthetic order count and
the existing balance object. It never contains the issued bearer, token hash or role keys.

## Voice merchant lab (independent schema version 1)

The optional `testbeds/voice-merchant` application keeps a separate profile, normally
`<local-data>/Cosigna/voice-merchant-lab/v1`, outside source trees. Its `agent/` subdirectory
uses the unchanged agent transaction schema v1 above. The default fixture profile is not
migrated or reused.

`agent/test-merchant-connection.json`: strict JSON `{format,version,url,instance}`, where
format is `cosigna-test-merchant-connection` and version is 1. It is created exclusively and
synced before the HTTP ledger is initialized. It contains the configured numeric loopback
URL and the merchant database instance ID, not an API credential. A fresh HTTP connection
refuses an existing unbound transaction ledger. Existing bindings must match exactly on
reopen. Opening a bound ledger in default fixture mode is refused. Every merchant HTTP
request after initial discovery carries its pinned instance ID.

`merchant.sqlite`: SQLite `application_id=0x43534d54`, `user_version=1`, foreign keys, WAL,
`synchronous=FULL` and five-second busy timeout. Empty databases initialize; foreign or future
schemas are refused. Schema:

- `settings(key PRIMARY KEY,value)`: stable random instance ID, private random merchant bearer
  token, and optional current Cosigna budget ID. The token stays server-side. Restrict profile
  access and never snapshot/log/export this database as public data.
- `products(id PRIMARY KEY,offer,stock CHECK stock>=0)`: typed JSON offer compatible with the
  agent `Offer`, plus nonnegative integer available stock.
- `attempts(key PRIMARY KEY,fingerprint,result)`: canonical sorted-key JSON of `{quote,digest}`
  hashed with SHA-256, and the saved outcome. Transport test scenario is excluded from the
  fingerprint. Replays with a different purchase conflict. Definitive quote/stock rejection
  is recorded. A `timeout_before_accept` test scenario records no result.
- `orders(id PRIMARY KEY,attempt_key UNIQUE REFERENCES attempts(key),quote,receipt,state,created_at)`:
  full quote/receipt JSON, fulfillment `preparing`, `ready`, `collected` or `declined`, Unix seconds.
  Order creation, attempt outcome and accepted stock decrement share one immediate transaction.
  A fulfillment state is separate from the receipt's simulated payment acceptance.
- `lab_tasks(task_id PRIMARY KEY,request_key UNIQUE)`: local UI task membership and recovery
  references. The agent ledger owns the transcript/request and transaction state.

The merchant accepts the BLAKE3 quote digest supplied by its authenticated caller as a
correlation value; it does not independently prove the user's grant. The SHA-256 idempotency
fingerprint is the merchant's own payload identity. Receipt validation remains in Cosigna.
The HTTP envelope is `cosigna.test-merchant/v1` with `data`, or `error:{code,message}`.
The browser API envelope is now `cosigna.voice-lab/v3` with `data` or `error:{code,message}`.
Details and exact endpoint input schemas live in the testbed README and route definitions.

Raw microphone audio is not saved by this application; transcription text persists as an
agent task request. HTTP cookie/CSRF session values exist in process memory and rotate on
restart. Unix profile directories use owner-only permissions and the merchant database mode
is 0600. Windows inherits directory ACLs. Logical reopen/HTTP failure tests do not establish
physical power-loss durability or protection against a compromised local user.

## Stripe sandbox merchant profile (independent schema version 1)

The optional Stripe mode lives in `<local-data>/Cosigna/voice-merchant-lab/stripe-v1`.
Existing simulation profiles are not migrated or retrospectively charged. Its `merchant.sqlite`
has `application_id=0x43534d53`, `user_version=1`; it uses the same `settings`, `products`,
`attempts`, `orders` and `lab_tasks` definitions as the voice merchant schema v1 plus:

- `stripe_attempts`: `key` PK/FK to attempts, immutable request JSON, unique order ID, prepared
  timestamp, unique optional PaymentIntent ID, optional confirmation-claim timestamp,
  stock state (`held`, `consumed`, `released`), payment status and optional Charge ID.
- `stripe_events`: Stripe event ID PK, attempt key FK and received timestamp, for deduplication
  after verified event handling. Raw provider payloads/client secrets are not stored.
- `settings.stripe_key_fingerprint`: SHA-256 of the backend test credential, a profile binding
  that refuses switching to a different key. The key itself remains outside this database.

Preparation and stock decrement are one immediate transaction before any Stripe mutation.
The unconfirmed PaymentIntent ID is saved before the one-time confirmation claim. Verified
success records a receipt/order and consumes the stock hold in one transaction. Verified
zero-received cancellation releases stock once. Unknown states retain their reservations.
Attempt and quote metadata link Stripe's record to the separate agent ledger; no distributed
database atomicity is claimed. On failure after processor acceptance, reconciliation can
finish the local transaction from an authoritative matching Stripe retrieval.

Stripe mode originally exposed browser envelope `cosigna.voice-lab/v2`, adding `payment_mode: stripe-test`,
sanitized pending payment records and webhook-configuration state to `/api/state`; each order
adds `payment: {provider,environment,payment_intent_id,status,charge_id,livemode}`. No API key,
webhook secret or PaymentIntent client secret is returned. Browser version 3 below replaces
both that envelope and the earlier simulation v1 envelope. The merchant HTTP envelope and agent receipt stay v1: no real funds move in Stripe
test mode, so `simulated:true` remains accurate; the separate processor fields distinguish it
from an in-process simulated acceptance.

`cosigna.stripe-protocol-check/v1` reports record timestamp, test-driver approval source,
Stripe PaymentIntent/Charge IDs, amount/currency, approved digest, check outcomes and a budget
summary. Reports contain no credentials, card numbers, client secrets or raw webhook bodies.
The profile also contains a copy of the built agent executable (`agent-runtime[.exe]`), which
is replaceable build output and not payment state.

## Paired phone lab (API version 3; no database migration)

Optional trusted-LAN development transport: `wifi-runtime.mjs` verifies/reuses or starts the
Groq/Stripe-test mobile backend, then exposes only `/mobile/v3/` through a relay bound to an
actual private IPv4 interface on port 3099. It leaves all stores, API payloads, pairing,
session expiry/revocation, approvals and provider consent versions unchanged. The phone's
nonsecret `EXPO_PUBLIC_COSIGNA_LAB_URL` accepts only the existing loopback URL or an explicit
RFC1918 numeric IPv4 relay URL on 3099, with no credentials/path/query. Its default is still
USB loopback. The native APK network policy is unchanged; Wi-Fi preview uses Expo Go.

The relay checks exact Host and local subnet, refuses browser Origin/Cookie/Sec-Fetch-Site
headers on mobile routes, bounds bodies/responses, forwards only authorization/content-type/
provider consent to a fixed loopback destination, and never follows redirects or retries.
No merchant/staff/core/credential-management routes or cookies are exposed. GET `/health`
returns `{schema:"cosigna.wifi-relay/v1",transport:"trusted-lan",payment_mode:"stripe-test"}`
without session data. The health response describes the launcher's validated sandbox mode;
it is not payment evidence. This HTTP transport does not protect a bearer/audio from a
network attacker and is restricted to explicitly selected trusted-LAN sandbox testing.
Production remote access still requires a distinct authenticated HTTPS deployment.

Optional `--mobile` mode uses independent `mobile-v1` or `mobile-stripe-v1` profiles under the
same local data root, on 3097/3098. They use the unchanged merchant/Stripe SQLite formats and
Rust ledger schemas above. The Expo app's paired lab state/token/consent switches live in
memory only. Its audio recorder creates a temporary native cache file; cleanup is attempted
after send/discard/background/unmount, but force termination may leave cache files. No raw
audio is persisted on the Node server. Submitted request text remains in the Rust ledger.

`cosigna.mobile-lab/v3` envelopes contain `data` or `error:{code,message}`. The `/mobile/v3/`
routes support POST `pair` (`{code}`), `disconnect` (`{}`), `budget`, `revoke-budget` (`{}`),
`plan`, `approve`, `reconcile`, `cancel`, `transcribe`, and GET `state` or `tasks/<task_id>`.
Budget/task bodies and nested `cosigna.agent/v1` task results match the browser equivalents.
Transcribe accepts bounded raw audio, returns `{text}`, and requires
`X-Cosigna-Hosted-Consent: voice-v1` for OpenAI or `voice-groq-v1` for Groq. Hosted-mode plan
requires `planner-v1` for OpenAI or `planner-groq-v1` for Groq; fixture planning
does not contact a model. These flags express a trusted app's explicit choice, not proof of
human identity. The planner exposes no approval or execution tool. The optional phone voice
checkout maps an explicit fresh confirmation, captured only after a completed spoken review,
to the existing `{task_id,quote_digest}` approval request. It checks current quote/expiry,
foreground state and measured microphone activity and consumes its local confirmation before
submission. This is trusted development-client intent, not verified speaker identity.
The ledger stores the existing owner approval event; it does not independently attest
whether a physical owner spoke or tapped. No approval or receipt schema changes.
Version 2 replaced the version 1 hexadecimal pairing code with exactly six decimal digits
(a string, preserving leading zeroes). Version 1 clients must reload and re-pair; no ledger
or database format changes. Version 3 retains six-digit pairing and adds `mode: groq` and
`ai_provider: openai|groq` to state. It binds hosted consent to the selected provider. Both
browser modes now use `cosigna.voice-lab/v3` and expose the same provider discriminator.
Old phone clients must reload and re-pair. The nested `cosigna.agent/v1` task schema, merchant
API and all database formats retain their versions. Hosted provider selection is startup
configuration only, not persisted transaction authority; no automatic provider fallback occurs.

State omits browser CSRF and includes mode/voice availability, payment mode, existing products,
orders/tasks/balance, Stripe test metadata where available, and human-readable privacy fields.
It contains no provider, merchant or Rust role credentials. The paired phone shares the lab
owner's state and cannot invoke merchant staff fulfillment. This is not multi-user isolation.

The browser's existing cookie+Origin+CSRF checks protect POST `/api/device-pairing` (`{}`)
and `/api/revoke-devices` (`{}`). Pairing returns a cryptographically random six-digit one-use
code, valid for five minutes/five failed attempts. The server stores only its SHA-256 in process memory. Successful
pairing returns a random 256-bit bearer session valid for twelve hours and replaces the prior
phone. Only the SHA-256 token is kept on the server. Neither code nor raw bearer is logged
or persisted by the server. The app's encrypted scoped-session format is described below.
Native routes refuse Origin, Sec-Fetch-Site and Cookie headers and enforce exact loopback Host.
Disconnect/revoke/restart invalidates new calls; already-authorized in-flight work can finish.
Pairing does not grant production identity, real spending or consent to a merchant integration.
USB loopback HTTP is a developer transport; remote use needs a different HTTPS/auth design.

Clarification replies compose the existing request, merchant question and user answer into
a new ordinary task request string, bounded by the existing 4096-character limit. The older
needs-input task remains recoverable; no quote, approval or reservation is copied into the
new task. Existing task `input.request` is read by the mobile UI; no new stored field is added.

`cosigna.groq-protocol-check/v1` reports the real Groq speech/planning and Stripe sandbox
check. It contains the same payment/digest/budget/check fields as the existing Stripe report,
plus `transcription`, `transcript` and `audio_source`; `planner` names Groq/GPT-OSS. Approval is
explicitly attributed to the test driver. It is not a physical microphone or identity test.
No key or raw audio is included. A successful check removes its isolated temporary profile;
failures preserve the profile for reconciliation and do not authorize a replacement payment.

## Scoped mobile session (`cosigna.device-session/v1`)

The native/Expo app now stores the paired lab connection in Expo SecureStore under
`cosigna.paired-lab.v1`. The JSON record contains only `schema`, `token` (the scoped
`mobile_` bearer), and `expiresAt` (Unix seconds from pairing). Native storage is encrypted
using Android Keystore/iOS Keychain, with `WHEN_UNLOCKED_THIS_DEVICE_ONLY` accessibility
where applicable. Android app backup is disabled; SecureStore also excludes its preferences
from backup. Unknown versions, malformed records, invalid token formats and expired records
are deleted rather than used. Storage errors surface to the user. This adds a new device-only
format; it does not change any API envelope or ledger/database version.

The service still keeps only the session hash in process memory and invalidates access on
restart, revoke, replacement or expiry. App restart can reuse a still-valid scoped connection
after checking server state. A 401 clears the local record. Disconnect revokes access on the
server before clearing local storage; deletion failure cannot revive a revoked session.
No quote approval, recording consent, provider consent, task text, model key, payment key or
merchant key persists in this record. Consent is renewed when reopening the screen. iOS
Keychain records may survive reinstall; server checks and the stored expiry still apply.

Assistant/native link input is a bounded untrusted draft, never a stored approval proof.
The Android bridge accepts only `request`; authentication, role credentials, spending limits
and confirmation commands in extras cannot authorize payment. Continuing the draft requires
an explicit voice-session action and processing permissions; actual approval still requires
a fresh spoken turn after the exact quote has finished reading.

## Rails application foundation: `cosigna.backend/v1`

The separate `backend` Rails API introduces a new metadata envelope, `cosigna.backend/v1`.
`GET /api/v1/platform` reports the product, foundation phase, Rails application backend,
Rust transaction authority, unimplemented customer authentication and absent write endpoints.
`GET /api/v1/gateway/health` returns that schema and a whitelisted verified
`cosigna.agent/v1` sandbox health object, or HTTP 503 and one of `gateway_unconfigured`,
`gateway_configuration_invalid`, `gateway_unavailable`, or `gateway_unverified`. It accepts
no client-selected destination and forwards no credentials. `/up` is Rails' own health route.

This increment adds no customer tables, payment records or financial migrations. Active
Record's local development/test databases live under the Rails app's separate `storage`
directory (a dedicated Compose volume in Docker). Rails never opens or changes the Rust
ledger, provider store, project manifests or Python consent database. Their versions and
existing wire contracts are unchanged. This is a local development API, not a tenant identity
or signed spending-authority contract.
