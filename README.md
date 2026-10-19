# Project Fabric

**A flight recorder for AI coding agents.** Fabric records what every coding agent actually
did to your project — step by step, across Claude Code, Codex, Cursor and others — lets any
agent query that history, and lets you undo any single step with a verified result.

> Status: early development. Works end to end with Claude Code; not yet tuned for large
> repositories. Not ready for production use.

## Try it

```sh
cargo install --path crates/fabric-cli   # installs the `fabric` command

cd your-project
fabric init                      # start recording
fabric integrate claude-code     # checkpoint every Claude Code prompt and change
# ... let the agent work ...
fabric log                       # the timeline of what happened
fabric diff 3                    # what changed since checkpoint 3
fabric restore 3 --yes           # go back; verified, and undoable
```

See [docs/CLI.md](docs/CLI.md) for every command, JSON output and exit codes.

## Why

Agents make hundreds of edits nobody explicitly chose. Today you see only the end state, each
agent's checkpoints live inside that agent, and handing work from one agent to another means
pasting chat logs. Fabric is the vendor-neutral record underneath:

1. **Checkpoint every agent action** automatically, fast enough to run on every tool call.
2. **Undo one step, keep the rest** — not just "rewind everything after this point".
3. **Find the step that broke the tests** by bisecting over recorded steps.
4. **Shared memory across agents** via MCP: what was tried, by whom, and whether tests passed —
   built from what actually happened, not from what an agent said happened.

See [docs/PRODUCT_SPEC.md](docs/PRODUCT_SPEC.md) for the full direction and roadmap.

## Repository layout

```
crates/fabric-store     Content-addressed object store (BLAKE3 + zstd, verified reads)
crates/fabric-snapshot  Project capture, versioned manifests, diffs
crates/fabric-recovery  Planned, journaled, verified, resumable restore
crates/fabric-cli       The `fabric` command and agent integrations
docs/                 Product spec, architecture, storage format, threat model
```

## Development

Requirements: Rust via [rustup](https://rustup.rs). The pinned toolchain in
`rust-toolchain.toml` installs automatically. On Windows, the MSVC C++ build tools are
required (zstd is compiled from C).

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

All four must pass before a change is merged; CI runs them on Windows, macOS and Linux.
