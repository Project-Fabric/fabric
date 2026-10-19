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
| `fabric-snapshot` | next | Walk a project, apply exclusion and symlink policy, write manifests |
| `fabric-recovery` | planned | Diff, recovery plans, conflict detection, journaled restore |
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

## Store location

The store lives outside the project root by default (per-user data directory), so project
operations — including a restore, `git clean`, or an agent deleting the project — cannot
touch it. Exact paths will be defined with project registration.
