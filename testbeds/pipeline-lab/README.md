# Local Apache verification pipeline

This optional Apache Airflow 3.3.2 lab runs real fixture regressions as a batch DAG:

```text
process crash recovery -> authorization/budget/replay -> API/worker restart -> summary
```

Cosigna's Rust ledger and worker still operate customer transaction states. This DAG runs
only isolated test stores; it has no customer tasks, owner credentials, provider keys or
permission to approve a purchase. Airflow is not a mandatory core dependency. Cloud signup
and deployment are deferred while the transaction foundation is built locally.

## Run on Windows

From `C:\Users\Harsha\Pictures\project`:

```powershell
docker desktop start
New-Item -ItemType Directory -Force testbeds/pipeline-lab/.runs/reports | Out-Null
docker compose -f testbeds/pipeline-lab/compose.yaml run --build --rm verify
```

Docker Desktop must use Linux containers. The first build downloads pinned upstream images
and Rust dependencies. Runtime is limited to 2 CPUs and 4 GB RAM, has no external networking,
uses a read-only image and temporary `/tmp`, exposes no ports and mounts only its report
directory. The test API and worker communicate over the container's own loopback interface.
The build context is allowlisted; it excludes environment files, keys, runtime DBs and builds.
No Docker socket, host credential directory or existing transaction ledger is mounted.

The process exits after the DAG. This command does not start a permanent Airflow UI/server
or an always-running customer transaction service. Reports persist under the ignored
`.runs/reports/<hashed-run-id>/` directory. Airflow's disposable metadata lives in `/tmp`;
it is not a transaction ledger or an authoritative audit history.

```powershell
Get-ChildItem testbeds/pipeline-lab/.runs/reports -Recurse -Filter summary.json |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1 | Get-Content
```

On Linux, create the report directory and give UID 50000 (the pinned Airflow image user)
write access before running Compose. The manual GitHub workflow does this explicitly.
Do not mount a real ledger or supply model/processor secrets to this lab.

## What is checked

- Real child-process kills during inference, after a durable submission claim, after
  fixture acceptance but before ledger completion, and after confirmation is recorded.
- Scope/revocation, exact approval, budget reservations, tampered quotes, concurrent claims,
  replay, restart and unknown-result handling from existing gateway/transaction/pipeline suites.
- Two real processes: an API and worker discover approval committed during downtime, complete
  a fixture purchase, and preserve the confirmed attempt/balance on terminal replay.
- Missing/failed/timed-out checks produce a failed report and fail the final DAG task. No
  automatic retries are configured. The crash child-driver test is intentionally ignored
  as a standalone case; its four parent tests explicitly invoke and kill it.

Per-suite stdout/stderr are limited to 64 KiB of valid UTF-8 per log. Check subprocesses
receive an allowlist of nonsecret execution environment variables, not provider credentials.
Reports contain synthetic identifiers and timing metadata. They remain local unless the
explicit manual GitHub workflow is run, which uploads them with seven-day retention.

Fast runner checks without Docker/Airflow:

```powershell
python -m unittest discover -s testbeds/pipeline-lab/tests
cargo test -p cosigna-agent --test crash_recovery
```

Full container verification is also available through the manually triggered
`Local Apache verification lab` GitHub Action. Standard Rust CI runs the crash tests;
Product checks run the six runner regressions. No paid model call or financial write occurs.

## Practical limits

These are controlled fixtures and fixed kill boundaries. They do not establish power-loss,
full-disk or network-partition recovery for every processor, universal exactly-once payment,
production identity, signed permits, resistance to every prompt injection, throughput or
commercial readiness. Timing fields measure this verification run, not a customer latency SLA.

Local evidence, 23 October 2026: all 230 Rust workspace regressions passed on Windows
(plus the intentionally ignored subprocess driver, exercised by four parent regressions),
six Python runner tests passed, and the complete DAG passed in the Linux Docker container.
A separate isolated run overlaid the planner test executable with an `exit 7` fixture:
Airflow returned exit code 1 and a failed summary, with the service check left unrun.
No production payment or external model API was invoked by these runs.

See [the local-first build plan](../../docs/LOCAL_PIPELINE_PLAN.md) for the next product work
and the distinct potential roles of Kafka and Camel. No Kafka broker or Camel connector is
implemented here. [Airflow's batch scope](https://airflow.apache.org/docs/apache-airflow/stable/index.html)
and [local DAG testing](https://airflow.apache.org/docs/apache-airflow/stable/core-concepts/debug.html)
explain this choice; its full Docker quickstart is a development setup, not a production payment gateway.
