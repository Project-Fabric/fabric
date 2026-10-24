# Gateway attack lab

Live attack, stress and crash simulation against the real `cosigna-agent` binary. It uses the
synthetic fixture rail only, with no real money, no network beyond 127.0.0.1 and no model calls.
It is written independently of the gateway in Python's standard library and uses only the public
HTTP API, the CLI and read-only SQLite inspection. The one exception is that it adds lab-only
synthetic payments to the fixture rail's database, so that capacity never limits a phase.

```powershell
cargo build --release -p cosigna-agent
python -I testbeds/gateway-attack-lab/attack_lab.py --binary "$PWD\target\release\cosigna-agent.exe" --out results.json
```

Phases:
1. 30 attacks (authorization, authentication, isolation, grants, money, tampering, replay, input, transport, races).
2. Concurrent races across several server processes on one ledger.
3. Hard kills of a server under load, then recovery.
4. Latency and throughput benchmarks.

`evidence verify` runs after each phase. The exit code is 0 only if everything is blocked and every
invariant holds. Results and their interpretation: [docs/GATEWAY_ATTACK_BENCHMARK.md](../../docs/GATEWAY_ATTACK_BENCHMARK.md).
