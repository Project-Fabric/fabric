# Project Fabric

**A flight recorder for AI coding agents.** Fabric records what every coding agent actually
did to your project — step by step, across Claude Code, Codex, Cursor and others — lets any
agent query that history, and lets you undo any single step with a verified result.

> Status: early development. The object store and snapshot/diff engine exist; restore, the
> CLI and agent integrations do not yet. Nothing here is ready for real projects.

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
