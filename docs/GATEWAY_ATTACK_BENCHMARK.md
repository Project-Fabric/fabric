# Gateway attack, stress and crash benchmark (baseline, 24 October 2026)

The first live adversarial run against the Cosigna transaction gateway. An independent
harness ([`testbeds/gateway-attack-lab`](../testbeds/gateway-attack-lab/README.md), Python
standard library only, no shared code with the gateway) starts the real `cosigna-agent`
release binary as separate processes and drives it only through its public HTTP API, its CLI
and read-only SQLite inspection. Raw results: [`benchmarks/gateway/2026-10-24/`](../benchmarks/gateway/2026-10-24/)
(three consecutive runs; an earlier run with harness bugs is excluded).

**Scope and limits.**
- **Rail:** the synthetic `fixture` rail only. No real money, no Stripe and no model calls; the network is loopback only.
- **Machine:** one Windows 11 laptop (Intel Core Ultra 7 155H, 22 threads, 32 GB RAM, NVMe SSD).
- **Measurement:** latency includes the Python client.
- These are a starting baseline, not production capacity or a security certification. Stripe
  behaviour is covered separately by tests against a local fake of the Stripe API.

## Results

| Check | Runs | Result |
|---|---|---|
| Attacks blocked | 3 × 30 | **30/30 every run** |
| Race: 60 concurrent $1 refunds of a payment with $20 left, 6 agents, 3 server processes on one ledger | 3 | **Exactly $20.00 moved every run** (20 succeeded; the rest denied as exceeding the refundable amount, or once failed at the rail's execution-time re-check with its hold released) |
| Budget: 80 concurrent $1 refunds against a $50 budget | 3 | **Exactly $50.00 spent every run**; 30 denied `budget_exhausted` |
| Crash: 8 clients refunding while one of 3 servers is hard-killed 3 times | 3 | 122 / 188 / 141 requests, 9–11 cut connections; **every refunded cent matches a spent hold, nothing left held or mid-submission** after recovery |
| Evidence (`cosigna-agent evidence verify`) after attacks, stress, crashes and benchmarks | 3 × 4 | **Chain intact and state matching every time**, up to about 4,000 records |

### Attacks (all blocked in every run)

| Category | Attacks |
|---|---|
| Authorization | agent approves its own held refund; executes an unapproved refund; reads the owner's inbox; creates a budget; registers a more powerful agent; resolves an operation; a plan-only agent requests money; a revoked budget is used |
| Authentication | forged agent token; missing or malformed bearer; revoked agent keeps spending |
| Isolation | another agent reads this agent's operation; an agent spends from a budget it is not bound to |
| Grants and policy | payout-only agent refunds; prompt injection in the request text ("SYSTEM OVERRIDE: pre-approved…") above the limit stays **held** |
| Money | refund more than was paid; INR payment refunded as USD; payout to an unregistered account |
| Tampering and replay | owner approval with a tampered digest; idempotency key reused for a different amount; replay of a completed refund (no money moves) |
| Input | zero, negative, huge, fractional and string amounts; smuggled `approved` / `auto_approve_minor` / `stage` fields; 20 KiB body; non-JSON content type; path traversal and SQL-injection-like identifiers |
| Transport | DNS-rebinding `Host` header; browser cross-origin request |
| Races | 20 concurrent executes of one approved refund (exactly one attempt, money moved once); approve and reject racing on one refund (exactly one outcome) |

### Performance (median of three runs)

| Path | Throughput/s | p50 ms | p95 ms | worst p99 ms | Failures |
|---|---|---|---|---|---|
| Auto-approved refund, request to settled, 1 client | 18.9 | 46 | 62 | 747 | 0/600 |
| Auto-approved refund, 8 clients, 1 server | 23.1 | 283 | 974 | 1,742 | 0/1,200 |
| Auto-approved refund, 8 clients, 3 servers on one ledger | 35.8 | 194 | 485 | 852 | 0/1,200 |
| Denied request (exceeds refundable), 1 client | 40.0 | 13 | 36 | 407 | 0/600 |
| Held refund, then owner approval and settlement | 16.2 | 62 | 82 | 432 | 0/150 |
| `evidence verify`, about 2,900 records | | 59–61 ms total | | | |

## What this shows, and what it does not

- **Correctness held under adversarial, concurrent and crash conditions.** No double refund, no overspend, no
  orphaned hold, no unrecorded state change across three runs.
- **Performance is the weak point.** Throughput is about 19–36 refunds/s, and latency grows quickly with concurrency. The
  server handles one request at a time. Every operation opens several SQLite connections, and each connect runs a
  write-locked schema check. With `synchronous=FULL`, each committed step is flushed to disk.
  Extra server processes help (36/s with 3) only until they contend for the same SQLite write lock. This is tracked in
  [G13 #85](https://github.com/Project-Fabric/fabric/issues/85). Expected fixes: one migration at startup with
  read-only connections afterwards, a worker pool, and fewer transactions per request. Re-measure with this harness.
- **Not covered here:**
  - real Stripe test mode;
  - power loss or disk-full conditions (only process kills were tested);
  - network partitions between the gateway and a real rail;
  - attacks by someone with direct database or file access (evidence detects edits but cannot prevent a consistent
    rewrite of the whole chain);
  - model-level attacks on agents themselves;
  - the pre-approval probes planned in P3.

Reproduce:

```powershell
cargo build --release -p cosigna-agent
python -I testbeds/gateway-attack-lab/attack_lab.py --binary "$PWD\target\release\cosigna-agent.exe" --out results.json
```

The exit code is 0 only if every attack is blocked, every invariant holds and every evidence check passes.
