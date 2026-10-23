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

## Optional Cosigna agent development stores (ledger version 2, provider version 1)

These stores belong to `cosigna-agent`, not the Fabric object/manifest layout above. Default
directory: the OS local-data directory followed by `Cosigna/agent-dev`. No existing project
store is migrated. SQLite is authoritative for this isolated experiment, not a rebuildable
manifest index. Initialization accepts an empty database only, or a matching application ID
and version; foreign databases and unsupported versions are refused.

| File | `PRAGMA application_id` | `PRAGMA user_version` | Tables |
|---|---|---|---|
| `transactions.sqlite` | `0x43535458` | 2 | `budgets`, `tasks`, `reservations`, `events`, `agents`, `task_agents`, `agent_events` |
| `fixture-provider.sqlite` | `0x43534658` | 1 | `offers`, `orders` |

Connections enable foreign keys, WAL journaling, `synchronous=FULL`, and a five-second busy
timeout. Mutations that need a shared state check use `BEGIN IMMEDIATE`. The service tests
reopening and explicit interruption boundaries; physical power-loss/full-disk and actual
process-kill durability have not been established.

Ledger version 2 is an additive, single-transaction upgrade from an application-matching
version 1 ledger. It creates the three agent tables and updates `user_version` together.
Existing budget/task/reservation/event values, quote digests and attempt keys are unchanged.
The fixture provider remains version 1. Versions above 2, foreign application IDs and
unidentified nonempty databases are refused. Old executables cannot open ledger version 2;
do not downgrade or delete agent tables in a user store. Use a separate `gateway-dev`
profile for gateway development instead of repurposing an active phone-lab profile.

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
  local history, not the engine's tamper-evident evidence chain.
- `offers`: ID and typed JSON synthetic offer, including all-in unit price, currency,
  seller, features, fulfillment, terms, revision and `simulated: true`.
- `orders`: stable attempt key primary key, quote digest and typed JSON synthetic receipt.
  Reuse with a different digest fails. No live provider state is present.

Quote digest: lowercase BLAKE3 hex of compact `serde_json` serialization of the `Quote`
struct in declaration order: `id`, `task_id`, `offer`, `quantity`, `total_minor`, `issued_at`,
`expires_at`. The nested offer field order is `id`, `merchant_id`, `title`, `currency`,
`unit_total_minor`, `features`, `fulfillment`, `terms`, `revision`, `simulated`. This binds
approval to the complete quote and its expiry. API clients receive the digest directly;
arbitrarily reserializing JSON is not an equivalent hashing protocol.

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

### Headless gateway API version 1

`/gateway/v1/` is a new API, not a change to `/v1/` or the mobile/test merchant envelopes.
Its success envelope is `{schema:"cosigna.gateway/v1",simulated_provider:true,agent_id,
approval_digest,data}`. `agent_id` is populated for associated task responses, otherwise
null; `approval_digest` is populated for a task quote, otherwise null. Errors have `schema`
and `error:{code,message}` with the gateway schema. All POST input schemas reject unknown
fields and are bounded to 16 KiB. The loopback server retains exact Host, browser-Origin,
JSON content-type and no-store response controls.

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
