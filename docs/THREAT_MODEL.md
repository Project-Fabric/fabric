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
| Agent runs destructive Fabric commands (`restore`, `revert`, `guard set`) | guard denies them for hooked agents; MCP exposes no destructive tools | implemented for Claude Code, tested |
| Agent deletes files, rewrites Git history, drops data, destroys infrastructure or publishes without consent | guard asks the person first (pattern-based, not a sandbox); checkpoints make in-project damage undoable | implemented for Claude Code, tested; other agents: undo only |
| Agent disables its own guard (edits hooks, Fabric data, `.git`) | writes to agent config ask; writes to Fabric data and `.git` are denied; policy lives outside the project | implemented, tested |
| Agent approves its own held change | `fabric approve` denied by the guard; refused where a known agent marks its shell (`CLAUDECODE`, `CODEX_SANDBOX`); approval bound to the exact action's digest and used once | implemented, tested; not proof against an agent deliberately evading both, nor against the prompting developer approving (recorded: approver, OS account, same-account flag) unless signed approvals are required |
| The developer approves or reviews their own agent's change | with `fabric approver require on`, approvals and reviews must be signed by a registered approver's key from another Fabric installation, checked against the exact actions or content; signatures re-verified offline from the evidence | implemented, tested; shows that the holder of a registered key decided, not who holds it; someone with write access to Fabric's data directory can register their own key (the registration is recorded) |
| Controlled file changed without approval (e.g. by a shell command the guard cannot attribute to a path) | after every agent step, changed files are checked against `approve`/`deny` rules and that step's approvals; unapproved controlled files are rolled back at once (content kept in the step's checkpoint) and anything that cannot be rolled back is an evidence exception | implemented, tested; the change exists on disk between the command and the rollback, so a process reading the file in that window can see it |
| A change authorization used beyond what was reviewed | a change covers one agent session and its path globs only, never held shell commands or `deny` rules; review binds the decision to the exact state shown (re-captured before recording) and closes the change | implemented, tested; while a change is open, its paths are controlled by the later review, not by per-edit approval, so a change rejected at review must still be reverted by a person |
| Evidence edited, removed or reordered after the fact | hash-chained records; exported packs signed with Ed25519 and hash every file | implemented, tested; a writer to Fabric's data directory can rewrite the whole chain before any head or pack has been shared |

## Out of scope

A compromised OS or administrator; an agent intentionally deleting the store with the user's
privileges; changes outside the tracked project (databases, cloud resources, global packages) —
these can at most be reported, not undone; network filesystems.
