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
