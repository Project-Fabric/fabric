# Cosigna

**The control layer between a company's AI agents and its money.** Agents ask Cosigna to make
refunds and payouts instead of holding payment keys. Cosigna checks each request against the
payment system's own records and the business's case records, applies per-agent limits, has a
person sign the exact payment when it is above them, pays it at most once, and records every
step as tamper-evident evidence.

Status: working end to end on a synthetic payment rail, with a Stripe test-mode refund rail.
Not production software yet; see [what is missing](#not-yet-built).

## What it does

- **Agents never hold the payment key.** They call Cosigna's MCP tools or HTTP API with their
  own revocable token; the gateway holds the payment credentials.
- **Checks against real records first.** The payment exists, has that much left to refund, the
  payee is registered, and (when the agent names a case) the refund matches what the customer
  is actually owed, even inside the agent's own limit.
- **Limits per agent.** Small payments go through on their own; larger ones wait for a person;
  anything outside the agent's grant or budget is denied.
- **Exact approval.** A person approves one exact payment (a digest of it); any change voids it.
- **Exactly once.** Retries, lost responses and crashes never pay twice; an unclear result stays
  held until the payment system says what happened.
- **Worst-case exposure.** The most the agents can still move with nobody signing, and what sets
  each agent's limit.
- **Named approvers and two-person approval.** Each person signs in with their own key and
  limit; above a set amount, two different people must approve.
- **Replay before switching it on.** Run a proposed policy over past refunds (a CSV, locally)
  with the gateway's own rules: what it would have paid, held or stopped, and which known
  mistakes it would have caught.
- **Evidence.** A hash-chained record of every decision, checkable by an
  [independent verifier](tools/evidence-verifier/README.md) that shares no code with the gateway.

## Results so far

- **Real agents, with and without Cosigna** ([Northwind 2.0](docs/EXPERIMENT_NORTHWIND_2.md)):
  Claude Code agents handling ten support tickets refunded **$1,168 / $1,168 / $528** wrongly
  across three runs without Cosigna (a prompt injection, an agent with no authority, a refund
  beyond budget) and **$0** with it, while making the same five correct refunds. Three runs, one
  model, a synthetic payment ledger.
- **Attack, race and crash lab** ([benchmark](docs/GATEWAY_ATTACK_BENCHMARK.md)): 30 of 30
  attacks blocked in every run; 60 concurrent refunds against $20 moved exactly $20; servers
  killed mid-payment left every cent accounted for. One laptop, synthetic rail.

## Try it

No keys or accounts are needed; nothing here moves real money.

```sh
cargo run -p cosigna-agent -- gateway-demo      # permissions, exact approval, revocation
cd console && npm ci && npm run dev             # owner console with captured sample data
node site/server.mjs                            # landing page at http://127.0.0.1:3091/next
cargo run -p cosigna-agent -- replay --history examples/replay/refunds-90-days.csv   --policy examples/replay/policy.json --cases examples/replay/cases.csv --text
                                                # a policy replayed over 90 days of refunds
```

Before a demo, `node scripts/demo-check.mjs` plays the demo's story against the real gateway in a
temporary profile (routine refund, injected refund denied, wrong amount held, exact approval,
no double payment, exposure, both evidence verifiers, console, landing page) and lists anything
that would fail on stage.

Then read the [gateway API](docs/COSIGNA_GATEWAY.md), the
[agent tools](crates/cosigna-agent/README.md) and the [storage and API formats](docs/STORAGE_FORMAT.md).

## Not yet built

- Production identity: named approvers, sign-in and per-person signatures (one owner key today).
- A production server: the gateway is a loopback development server (eight concurrent requests).
- Live connectors beyond Stripe test mode, and a real case source (help desk or store).
- Alerts, and connectors that export a company's history directly (replay reads a CSV today).

Progress and order: the [roadmap board](https://github.com/orgs/Project-Fabric/projects/2).

## Where things are

```
crates/cosigna-agent        The gateway: policy, rails, case binding, exposure, evidence, MCP tools
console/                    Owner console: approval inbox, exposure, evidence
site/                       Landing pages
tools/evidence-verifier     Independent Python verifier of the evidence chain
testbeds/gateway-attack-lab Attack, race and crash harness against the real binary
experiments/northwind2      Real-agent experiment harness and raw results
backend/                    Rails application foundation (early)
```

Other work kept in this repository: the [voice commerce lab](testbeds/voice-merchant/README.md)
and the legacy AgentAuth integration ([consolidation plan](docs/COSIGNA_UNIFICATION.md)).

## The Fabric engine (also in this repository)

The project recording and recovery engine Cosigna grew from; its command is `cosigna` (or
the compatible `fabric` alias).

**A flight recorder for AI coding agents.** Fabric records what every coding agent actually
did to your project — step by step, across Claude Code, Codex, Cursor and others — lets any
agent query that history, and lets you undo any single step with a verified result.

> **Direction:** change control for AI agents on financially significant systems: see
> [`docs/PIVOT_FINANCE.md`](docs/PIVOT_FINANCE.md).
>
> Status: early development. Works end to end with Claude Code; not yet tuned for large
> repositories. Not ready for production use.

### Try the engine

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

### Change control for financial systems

For code under SOX change management, Fabric holds agent changes until a person approves them,
records who approved what, and exports evidence an auditor can verify:

```sh
fabric guard pack finance --scope "src/billing/**" --scope "migrations/**"
fabric approvals                 # changes the agent is waiting on
fabric approve 4                 # review the exact diff, approve (or --reject)
fabric approve 4 --change "CHG-1042: late fees" --path "src/billing/**"   # or authorize the task
fabric review 1                  # ...and review its whole result once, when the agent is done
fabric evidence verify           # chain intact? any controlled change made without approval?
fabric evidence report --change 1        # one page per change ticket, for the auditor
fabric evidence export --out audit-q3/   # signed pack; check with `fabric evidence verify-pack`
```

Changes made around the hold (a script editing a controlled file) are rolled back at once.
For segregation of duties, approvers can sign approvals and reviews with their own key on
their own machine (`fabric approver sign`), and a project can require it.

Agents that operate the books through tools get the same controls: `fabric mcp-proxy` sits
in front of any MCP server (an ERP, a ledger, a payments API) for any agent, and the
`finance-ops` pack holds postings and money movements for approval, blocks deletions, and
records every call (`fabric evidence calls`).
Committed migrations cannot be edited, direct SQL writes to financial tables need approval,
and production configuration is off limits. See [docs/PIVOT_FINANCE.md](docs/PIVOT_FINANCE.md)
and the testbed in [testbeds/ledgerly](testbeds/ledgerly).

See [docs/CLI.md](docs/CLI.md) for every command, JSON output and exit codes.

### Why

Agents make hundreds of edits nobody explicitly chose. Today you see only the end state, each
agent's checkpoints live inside that agent, and handing work from one agent to another means
pasting chat logs. Fabric is the vendor-neutral record underneath:

1. **Checkpoint every agent action** automatically, fast enough to run on every tool call.
2. **Undo one step, keep the rest** — not just "rewind everything after this point".
3. **Find the step that broke the tests** by bisecting over recorded steps.
4. **Shared memory across agents** via MCP: what was tried, by whom, and whether tests passed —
   built from what actually happened, not from what an agent said happened.

See [docs/PRODUCT_SPEC.md](docs/PRODUCT_SPEC.md) for the full direction and roadmap.

### Engine layout

```
crates/fabric-store     Content-addressed object store (BLAKE3 + zstd, verified reads)
crates/fabric-snapshot  Project capture, versioned manifests, diffs
crates/fabric-recovery  Planned, journaled, verified, resumable restore
crates/fabric-cli       The `fabric` command and agent integrations
docs/                   Product spec, architecture, storage format, threat model
testbeds/ledgerly       A billing and general-ledger service used to test finance controls
experiments/            Experiments with real agents (lessons, finance controls)
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
