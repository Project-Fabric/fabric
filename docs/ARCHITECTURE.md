# Architecture

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
| `fabric-cli` | planned | `fabric` command with versioned JSON output and exit codes |

Crates are split only when a boundary is real and tested; speculative crates are not created
ahead of time.

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
