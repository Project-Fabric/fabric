# Northwind 2.0

Real Claude agents handle the 10 Northwind support tickets twice: once with a direct refund
tool, and once through Cosigna's agent tools on the gateway. It runs on the synthetic fixture
rail, with no real money. Write-up: [docs/EXPERIMENT_NORTHWIND_2.md](../../docs/EXPERIMENT_NORTHWIND_2.md).

```powershell
cargo build --release -p cosigna-agent
python -I experiments/northwind2/run.py --binary "$PWD\target\release\cosigna-agent.exe" --runs 3 --out results\<date>
```

It needs the `claude` CLI, signed in, and costs about $1.75 of agent usage per round (both worlds).
Each ticket runs in a fresh temporary directory outside the repository, with only the
experiment's MCP tools allowed.

## Codex runtime

Use `--runtime codex` to run both worlds with the existing signed-in Codex CLI instead of
Claude. Codex runs with only the supplied Northwind/Cosigna MCP servers; shell, web, apps,
hooks and multi-agent tools are disabled. It receives scoped tool tokens, never the Stripe
test key. Optional `--model` pins a Codex model; otherwise the CLI default is used in both
worlds. There is no fallback to Claude.

```powershell
python -I experiments/northwind2/run.py --binary "$PWD\target\release\cosigna-agent.exe" --runs 3 --out experiments\northwind2\results\<unique-date>-codex-stripe --rail stripe_test --runtime codex
```

Keep Codex and Claude results separate. Codex records token usage with `cost_usd: null`, since
the signed-in subscription does not report an API dollar charge. Reports identify the runtime
and completion state. Runtime errors abort the campaign, preserving a sanitized
`runN-WORLD-incomplete.json`; they are never scored as successful zero-payment outcomes.
Every campaign requires a new output folder, so previous results cannot be overwritten.
Incomplete v3 artifacts also retain payment/refund records and safe tool identifiers; guarded
failures export evidence before temporary-store cleanup. An unexpected tool still stops the
run, even if the model's turn completed. Diagnostics never include tool arguments or raw events.
During execution/measurement failures, the checkpoint preserves payment identifiers and the
last observed state. A failed payment-record lookup is marked unknown, not zero movement.
The checkpoint attempts evidence export offline before provider diagnostics. This does not
guarantee recovery of failures before a world finishes initialization.
Codex's built-in metadata listings (`list_mcp_resources` and `list_mcp_resource_templates`)
are counted separately from financial MCP calls. They do not grant resource reads, new
servers or additional financial tools. Shell commands and unknown capabilities still fail.
Successful guarded worlds retain `runN-guarded-evidence.jsonl` before temporary-store cleanup.
The export uses the engine's framed evidence format. Check it independently with
`python -I tools/evidence-verifier/verify_evidence.py <results>/run1-guarded-evidence.jsonl`
using a Python environment with `blake3` installed. An export failure stops the run.

Adapter/error-path tests (no hosted inference or real Stripe requests):
`python -I experiments/northwind2/test_codex_runtime.py`.

## On Stripe test mode

`--rail stripe_test` runs both worlds on real Stripe **test** payments: for each world, one test
customer and one test-card payment per order. Without Cosigna, the agents' refund tool calls
Stripe directly (only Stripe's own rules apply). With Cosigna, the gateway runs with
`--stripe-test` and the agents use its tools. Refunds are measured from Stripe's own refund list,
and every refund made with Cosigna must carry the gateway's operation tag.

The test key is read from `COSIGNA_STRIPE_TEST_SECRET_KEY` and reaches only the runner and the
gateway. The agents' Northwind tools reach Stripe through the runner's loopback service, with a
per-run token, and only for that run's payments; the Claude CLI's environment has no key, and the
runner stops if the key appears in any run or result file. Live keys are refused.

```powershell
$s = Read-Host "Stripe test key" -AsSecureString
$env:COSIGNA_STRIPE_TEST_SECRET_KEY = [System.Net.NetworkCredential]::new("", $s).Password
cargo build --release -p cosigna-agent
python -I experiments/northwind2/run.py --binary "$PWD\target\release\cosigna-agent.exe" --runs 3 --out experiments\northwind2\results\<date>-stripe --rail stripe_test
```

Before spending agent usage, check the wiring offline (a loopback stand-in for Stripe and a
scripted, deliberately naive stand-in for the agent; no account, no model calls, not an AI
result):

```powershell
python -I experiments/northwind2/offline_check.py --binary "$PWD\target\debug\cosigna-agent.exe"
```

