# Architecture

Current transaction-product direction: [Cosigna Gateway and Cosigna Assistant](COSIGNA_REBUILD.md).
The gateway is the shared transaction foundation; the assistant is an application above it.
Gateway hardening precedes further voice capability expansion; the unified marketing site
is active under [the staged consolidation plan](COSIGNA_UNIFICATION.md). The sections below document existing
implementation, including the separate offline project engine and local commerce experiments;
the rebuild brief does not change their storage or API contracts.

## Layers

```
 Interfaces     fabric CLI (JSON output) · agent hooks · MCP server · later: desktop app
                                 │
 Core           snapshot · diff · restore (plan → journal → apply → verify) · recorder
                                 │
 Storage        manifests (authoritative)        object store (content-addressed)
                                 │
 Index          SQLite or similar — rebuildable from manifests, never the source of truth
```

All state-changing operations go through the core library. Interfaces never touch storage
directly.

## Crates

| Crate | Status | Responsibility |
|-------|--------|----------------|
| `fabric-store` | implemented | Immutable, content-addressed, verified object storage |
| `fabric-snapshot` | implemented | Walk a project, apply exclusion and symlink policy, write and validate manifests, diff |
| `fabric-recovery` | implemented | Restore plans, conflict detection, journaled apply, resume, verification |
| `fabric-cli` | implemented | `fabric` command, project registry, checkpoint log, Claude Code hooks |
| `cosigna-agent` | development experiment | Optional consumer task service: hosted planner, exact quote approval, atomic budget reservations, simulated merchant and reconciliation |

Crates are split only when a boundary is real and tested; speculative crates are not created
ahead of time.

`cosigna-agent` is a separate application service, not part of the offline recording/recovery
engine. Its optional model adapter uses the network; fixture mode and the existing engine need
no model provider. It owns an independent, versioned SQLite ledger and fixture provider store,
neither of which is an index of Fabric manifests. Existing project stores, evidence and mandates
are unchanged. It does not yet consume the enterprise mandate implementation or execute live
payments. See [the service README](../crates/cosigna-agent/README.md) for its tested boundary.

The headless [gateway API](COSIGNA_GATEWAY.md) adds local, individually revocable agent bearer
credentials bound to one budget and explicit plan/execute permissions. Task attribution is
atomic with task creation; scope and revocation are checked again at planning/submission.
The transaction ledger advances to version 2 through an additive migration; existing task
JSON, quote digests, provider storage and legacy API envelopes retain their versions. This
development agent registry does not supply production user identity or signed transaction
permits. The supplied Focus Areas and Build Plan is mapped to remaining work in that guide.

The [transaction pipeline](TRANSACTION_PIPELINES.md) coordinates planning,
approval waiting, execution and reconciliation over those same persisted task records. Each
advance performs one allowed operation; there is no independent workflow state to drift from
the ledger and no implicit owner approval. An opt-in, owner-controlled durable queue now
adds a separate single-worker process with bounded polling/backoff and local OS locks.
The additive ledger v3 migration leaves existing tasks and approvals unchanged. Private
Linux service templates and recovery limits are in [operations](GATEWAY_OPERATIONS.md).
Production identity, signed permits, AgentAuth authorization consolidation and public server
hardening remain separate [roadmap gates](ROADMAP.md).

The optional [Apache verification lab](../testbeds/pipeline-lab/README.md) runs fixture
crash/authorization/replay/service checks as an Airflow batch DAG. It uses no existing user
ledger, provider keys or public network at runtime. Airflow does not schedule customer
payments or grant authority; Docker/Airflow are not dependencies of the offline engine or
Rust gateway. Kafka/Camel are candidates documented in the [local plan](LOCAL_PIPELINE_PLAN.md),
not implemented transaction integrations.

The optional [voice merchant lab](../testbeds/voice-merchant/README.md) adds a Node development
UI/voice backend and an independent merchant HTTP API with its own stock/order store. The
Rust service's fixed loopback connector pins merchant identity and verifies receipt fields
before settling its separate ledger. All merchant payments remain simulated. This adapter
does not grant general Internet merchant access or change the offline engine's dependencies.

An optional Stripe sandbox merchant profile now replaces the local acceptance simulation with
actual test PaymentIntent creation, confirmation and authoritative retrieval. Its private
merchant DB records intent/stock holds before network writes and resolves through processor
lookup or verified test webhooks. The unchanged Rust approval/budget boundary authorizes the
merchant request before Stripe is invoked. No real funds move; fixture-only and offline engine
operation still require no processor or model account. See the testbed's Stripe run/check commands.

The Expo mobile app now has a separate connected order lab. An explicitly paired development
phone uses a bounded, versioned Node API over Android USB loopback forwarding, never the Rust
role/merchant/payment credentials directly. The app captures optional audio, obtains an editable
transcript and requires separate model-processing consent and exact-quote approval. Its client
state is not authoritative. The existing Rust ledger controls reservations, submission and
reconciliation. Server-backed records survive phone re-pairing; phone access expires/revokes
without deleting transaction evidence. This is a single-owner local test transport, not a
production identity or delegated wallet. See [phone test instructions](PHONE_VOICE_TEST.md).

Hosted planning/transcription can explicitly select OpenAI or Groq at startup. Groq uses
Whisper Large V3 Turbo for transcription and GPT-OSS 120B for the constrained purchase plan.
Provider-specific credentials stay on the computer; fixed HTTPS destinations, bounded
responses and redirect rejection prevent task/model text from selecting a credential's
destination. The phone names the active provider and sends separate provider-bound audio
and planning consent. Changing providers requires renewed consent; there is no automatic
fallback. The planner still only proposes a catalog item or clarification. Existing Rust
quote, approval, budget, execution and reconciliation code determines transaction authority.

## Key decisions

- **Whole-file objects first, no chunking.** Source files are small; content-defined chunking
  (FastCDC) adds complexity whose benefit is unproven for this workload. Revisit with
  benchmarks on large binary assets.
- **BLAKE3 identities over uncompressed content.** Identity is independent of compression
  settings, so the codec can change without rewriting identities.
- **One writer process per store**, enforced with an OS file lock. Removes a class of
  concurrency bugs and makes crash cleanup unambiguous. Revisit if concurrent agents need
  concurrent writes; readers may become lock-free later.
- **Objects are read-only on disk.** Defense in depth against tools and agents that write to
  arbitrary files. Not a security boundary.
- **Manifests are authoritative; any database is an index** that can be rebuilt from them.
- **Every restore snapshots the current state first**, making restore reversible instead of
  relying on conflict detection alone.

## Restore

1. **Plan** (`plan_restore`). Checks the target's objects exist, snapshots the current
   project (the *base*, which is also the undo point), and computes operations: removals
   deepest-first, then creations shallowest-first. Lists conflicts — anything the base
   snapshot did not capture that the restore would overwrite or delete (ignored or unreadable
   files), names Windows cannot hold, and case-only collisions on Windows and macOS. Does not
   modify the project.
2. **Apply** (`apply_restore`). Refuses if the plan has conflicts, a restore is pending, or a
   fresh snapshot differs from the base. Writes a journal, then stages every file outside the
   live tree (in `.fabric-restore/`, on the same volume so renames are atomic) and verifies it
   against its object id. Only then touches the project, one idempotent operation at a time,
   recording each in an append-only progress log. Files are replaced by rename; directories
   are only removed when empty, never recursively; nothing is written through a symlink.
3. **Verify.** Snapshots the result. Every target entry must match exactly and nothing from
   the base that the target lacks may remain. Files that were never captured (ignored) are
   left alone and reported as `untracked_kept`.
4. **Resume** (`resume_restore`). After a crash, finishes the journaled operations and
   verifies. If the crash came before the journal was committed, the project was untouched
   and the staging area is simply removed.

What this does **not** guarantee: whole-project atomicity. A crash mid-apply leaves a mix of
old and new files until `resume_restore` runs. Every restore can be undone by restoring the
reported `undo_snapshot`.

## Store location

The store lives outside the project root by default (per-user data directory), so project
operations — including a restore, `git clean`, or an agent deleting the project — cannot
touch it. Exact paths will be defined with project registration.
