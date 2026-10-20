# Instructions for coding agents

Project Fabric is a safety tool: a bug here can destroy the work it exists to protect. Read
`docs/ARCHITECTURE.md` and `docs/STORAGE_FORMAT.md` before changing storage or recovery code.

## Rules

- Work in small, reviewable increments, one roadmap stage at a time.
- Before finishing, run and report the results of:
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
- Never weaken, skip or delete a failing test to make CI pass.
- No `unsafe` (the workspace forbids it).
- Never change the on-disk format, manifest schema or JSON output schema without bumping its
  version and updating `docs/STORAGE_FORMAT.md`.
- Destructive tests use temporary directories and synthetic fixtures only — never the
  repository itself or real user data.
- Never claim a guarantee the code and tests do not establish. If a guarantee is
  platform-specific, say which platforms.
- No cloud services, telemetry, paid APIs or mandatory model-provider dependencies in the core.
- Do not store secrets, credentials or environment dumps in snapshots or logs.
- If a requirement is ambiguous or a recovery guarantee cannot be met safely, stop and
  document the decision instead of guessing.

## Not in scope yet

Desktop app, multi-model orchestration, cloud sync, a hosted team server.

In scope since the finance pivot (`docs/PIVOT_FINANCE.md`): local approvals, tamper-evident
evidence export and finance rule packs. Evidence is audit material: never claim it proves
more than its verifier checks.
