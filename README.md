# Project Fabric

**A flight recorder for AI coding agents.** Fabric records what every coding agent actually
did to your project — step by step, across Claude Code, Codex, Cursor and others — lets any
agent query that history, and lets you undo any single step with a verified result.

> **Direction:** change control for AI agents on financially significant systems: see
> [`docs/PIVOT_FINANCE.md`](docs/PIVOT_FINANCE.md).
>
> Status: early development. Works end to end with Claude Code; not yet tuned for large
> repositories. Not ready for production use.

## Try it

```sh
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/Project-Fabric/fabric/main/install.sh | sh
# Windows (PowerShell)
irm https://raw.githubusercontent.com/Project-Fabric/fabric/main/install.ps1 | iex
# or from source
cargo install --path crates/fabric-cli

cd your-project
fabric init                      # start recording
fabric integrate claude-code     # checkpoint every Claude Code prompt and change
fabric integrate codex           # let Codex see what Claude did, and vice versa
# ... let the agent work ...
fabric log                       # the timeline of what happened
fabric diff 3                    # what changed since checkpoint 3
fabric restore 3 --yes           # go back; verified, and undoable

fabric bisect --test "pytest -x" # which agent step broke the tests?
fabric revert 7 --yes            # undo just that step, keep everything after it
fabric show 7                    # exactly what step 7 changed
fabric history src/auth.py       # every change to a file, and which agent made it
```

Connected agents get the same history through Fabric's MCP server: before touching a file,
Codex can ask what Claude already tried on it and whether the tests passed.

And agents ask before they destroy: deleting files, force-pushing, dropping tables or
`terraform destroy` waits for your approval, and attempts to wipe the system or undo Fabric
itself are blocked (`fabric guard rules`).

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
