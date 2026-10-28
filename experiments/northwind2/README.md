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

