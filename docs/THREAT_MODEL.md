# Threat model

Fabric protects developers from **accidents and mistakes** — by agents, tools and people —
on a machine they control. It is not a sandbox and does not defend against a deliberately
malicious agent running with the user's privileges.

## Assets

1. The user's project files, especially uncommitted work.
2. Stored snapshots and the record of agent actions.
3. The correctness of what Fabric reports (a false "restored and verified" is a failure).

## Threats and status

| Threat | Mitigation | Status |
|---|---|---|
| Interrupted write (crash, kill, power loss) exposes a partial object | temp file + fsync + atomic rename; stale temps removed on open | implemented; real-kill test pending |
| On-disk corruption or tampering returns wrong content | every read verifies header, length and BLAKE3 | implemented, tested |
| Missing object treated as empty | distinct `Missing` error | implemented, tested |
| Two processes write the same store | exclusive OS file lock | implemented, tested |
| Store created inside / over a user directory | `init` refuses non-empty foreign directories | implemented, tested |
| Agent or tool overwrites stored objects | objects read-only; store outside project root | partial — read-only implemented; deliberate deletion is out of scope |
| Restore overwrites user work | snapshot-before-restore (undo point), conflicts for any uncaptured file it would touch, re-check that nothing changed since planning | implemented, tested |
| Restore with corrupt or missing stored content | missing refused at planning; every file staged and verified before the project is touched | implemented, tested |
| Interrupted restore leaves a half-restored project | journal + idempotent operations + resume; tested with a simulated crash before every operation | implemented; real-kill test pending |
| Restore reported as successful but wrong | post-restore snapshot compared entry by entry with the target | implemented, tested |
| Malicious paths in a manifest (`..`, absolute, `\`, `:`) | validated on construction and on every load | implemented, tested |
| Reserved Windows names (`CON`, `NUL`), case collisions on restore | reported as conflicts when planning | implemented, tested |
| Symlinks escaping the project during capture | symlinks recorded, never followed | implemented, tested |
| Symlinks escaping the project during restore | every parent is checked to be a real directory before each operation | implemented; no dedicated race test yet |
| Secrets captured in snapshots or logs | default exclusions (`.env*`, `*.pem`, `*.key`, SSH keys); no environment dumps | implemented for snapshots, tested |
| Verification runs untrusted project scripts | only explicitly configured commands; never during restore | planned |
| Agent runs destructive Fabric commands (`restore apply`, `gc`) | human-issued authorization for destructive operations in agent mode | planned |

## Out of scope

A compromised OS or administrator; an agent intentionally deleting the store with the user's
privileges; changes outside the tracked project (databases, cloud resources, global packages) —
these can at most be reported, not undone; network filesystems.
