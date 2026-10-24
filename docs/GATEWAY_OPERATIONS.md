# Persistent gateway sandbox

23 October update: cloud signup and deployment are deferred at the user's request. Build and
verify the transaction foundation locally first; see the [local pipeline plan](LOCAL_PIPELINE_PLAN.md)
and optional [Apache verification lab](../testbeds/pipeline-lab/README.md). The Oracle provisioning
instructions below are retained as a future reference, not an active deployment.

The one-shot `pipeline-demo` exits deliberately. An always-running sandbox now has two
processes: the authenticated loopback API and an opt-in transaction worker, both using the
same durable ledger on local disk. Linux systemd templates restart failed processes and
start them at boot. No hosting account, public deployment or live connector is provisioned
by this change. The supplied units select fixtures and make no model/processor calls.

This remains the independent `cosigna-agent` development ledger. The older Fabric MCP proxy
already integrates with AgentAuth `/v1/authorize`; this transaction service does **not** yet
use that mandate service. Consolidating the authoritative permission boundary, production
owner identity, signed transaction permits and live-connector eligibility remain release gates.

## Try on Windows

Use a separate profile; leave the phone/Stripe lab's existing profile and processes alone.
From the repository root, terminal 1:

```powershell
$gatewayData = Join-Path $env:LOCALAPPDATA 'Cosigna\gateway-server-dev'
cargo run -p cosigna-agent -- init --data-dir $gatewayData
cargo run -p cosigna-agent -- serve --data-dir $gatewayData --port 3100 --planner fixtures
```

Terminal 2 (same repository):

```powershell
$gatewayData = Join-Path $env:LOCALAPPDATA 'Cosigna\gateway-server-dev'
cargo run -p cosigna-agent -- worker --data-dir $gatewayData --planner fixtures
```

The worker is idle until an owner enrolls a task. Create a budget and task through the
[gateway API](COSIGNA_GATEWAY.md), then enroll it as described below. A phone is unnecessary.
`worker --once` handles at most one currently due job and exits; no output means none was due.
Both processes must select the same connector/profile. A hosted planner is optional: configure
the supported provider key only in trusted backend processes and select the matching planner
on both API and worker. Provider quota is billed normally; fixtures require no key.

## Explicit scheduling

Only the authenticated local owner can GET or POST
`/gateway/v1/pipelines/{task_id}/worker`. The strict POST body is one of:

```json
{"action":"enroll"}
```

```json
{"action":"pause"}
```

```json
{"action":"resume"}
```

The response is `cosigna.worker/v1` with `simulated_provider:true` and `data` containing
`task_id`, `state`, `revision`, `next_at`, `reconcile_count`, `reason`, `updated_at`.
`pending`, `running`, `paused`, `done` are scheduler states; task/payment state stays in the
existing pipeline API. `enroll` is idempotent and never resumes paused jobs. `resume` only
accepts paused jobs and resets the lookup retry allowance; the owner must review the reason
first, especially `planning_interrupted`. No action here approves a quote or changes a budget.

For a task already created using the gateway guide, in a private PowerShell session:

```powershell
$creds = Get-Content (Join-Path $gatewayData 'credentials.json') -Raw | ConvertFrom-Json
$ownerHeaders = @{ Authorization = "Bearer $($creds.owner)" }
$taskId = '<the existing task id>'
$workerUrl = "http://127.0.0.1:3100/gateway/v1/pipelines/$taskId/worker"
Invoke-RestMethod $workerUrl -Method Post -Headers $ownerHeaders -ContentType 'application/json' -Body '{"action":"enroll"}'
```

Create a draft via `POST /gateway/v1/tasks` to let the worker perform planning. Creating via
`POST /gateway/v1/pipelines` already performs the first step synchronously; it may still be
enrolled afterward. After reviewing the actual complete quote, the owner approves its exact
digest using the existing `/tasks/{id}/approve` endpoint. The worker notices that committed
approval and executes; there is no second execute click. Budgets alone never authorize payment.

## Scheduling and recovery

- One worker per ledger holds an OS file lock for its lifetime. One job per tick, paced at
  one tick/second in the CLI; at most 1,000 pending/running jobs. This is a bounded pilot,
  not a horizontally scalable queue or a latency SLA.
- Waiting for approval polls every five seconds without inference. Clarification, expired
  authority/quote and permission failures pause. Historical tasks are not auto-enrolled.
- Before an operation the job's `running` state commits. The existing core persists a
  stable attempt before calling the connector. Submission/unknown recovery only looks up
  that attempt, including after grant revocation; it never creates a replacement purchase.
- Unresolved outcomes back off 10, 20, 40, 80, 160, then 300 seconds (capped). After twelve
  unresolved steps the job pauses for operator review. Holds remain reserved; missing lookup
  results, elapsed time and retry exhaustion are not proof of failure.
- To settle a paused unknown attempt, the owner calls `POST /v1/tasks/{id}/resolve` (or the
  gateway route). The provider either reports the real outcome or voids the attempt so it can
  never succeed; only a void or rejection releases the hold. An `unknown` answer means retry
  later. Then resume the job (or leave it) so the worker records it as done.
- Model failures pause without automatic paid retries. A restart with a running draft
  pauses as `planning_interrupted`; a committed quote or existing attempt can resume.
  A separate planner lock bounds concurrent inference across API/worker processes on the
  same disk. Explicit clarification replans and independent stores can still incur new calls;
  there is no provider-wide inference billing guarantee.
- An owner pause increments a revision so late worker completion cannot undo the pause.
  An operation already started may finish. Pausing is not cancellation, revocation or refund.
  Core results remain authoritative; inspect the task before deciding what to do next.
- Logs contain job metadata and fixed reason codes, not credentials, request/audio text,
  provider response bodies or model reasoning. Task IDs/timestamps remain sensitive metadata.

Stop with Ctrl+C locally or `systemctl stop` on Linux. Termination can interrupt an operation;
there is no graceful-drain implementation yet. Restart uses the durable boundaries above.
Do not clear a ledger/queue, remove a live lock file, create replacement idempotency keys or
release unknown holds by editing storage; use `resolve`, which goes through the provider and
is recorded as evidence (`evidence verify` reports direct edits). Stop both services before upgrading or restoring storage.

## Oracle pilot provisioning

Previously considered 23 October 2026: Oracle Cloud Always Free for the private sandbox.
Account creation and the VM are deferred. No paid resource, model quota or live payment is authorized
by this hosting choice. The backend stays accessible through SSH forwarding initially.

1. Create/sign in to [Oracle Cloud Free Tier](https://www.oracle.com/cloud/free/). Complete
   identity/payment verification directly with Oracle. If creating an account, choose Mumbai
   or Hyderabad if offered; home region cannot be changed after tenancy creation. Existing
   accounts must use their existing home region for Always Free compute/storage.
2. In Compute > Instances > Create instance, name it `cosigna-gateway-dev`. Select an
   Always Free eligible Ubuntu ARM image and `VM.Standard.A1.Flex`, **2 OCPUs / 12 GB RAM**,
   with a **50 GB boot volume**. These resources consume account-wide allowances; check
   existing usage and the console's current eligibility before creating anything. The
   documented total is 2 OCPUs/12 GB and 200 GB combined boot/block volume storage, not a
   separate allowance per VM. If free capacity is unavailable, retry later; do not silently
   substitute a paid shape or enable extra paid services.
3. For this pilot, use a VCN public subnet with a route to its Internet gateway and a public
   IP for SSH. Restrict incoming TCP 22 to the administrator's current public IP (`/32`).
   Keep TCP 3100 closed externally. The application API itself binds to loopback.
4. Add an SSH **public** key. Keep the corresponding private key on the administrator's
   computer, outside the repository. If Oracle generates a pair, save the private key
   locally when offered; do not send its contents in chat. Ubuntu images use the `ubuntu`
   SSH user. Record the VM's public IP and local key path for deployment.
5. Connect with `ssh -i <local-private-key-path> ubuntu@<vm-public-ip>`. Build the Linux ARM
   binary on the VM or another compatible ARM build host; the Windows executable is not
   deployable there. Install the reviewed source and services using the Linux section below,
   then test health, owner-approved fixture execution, service restart and reboot recovery.

Oracle may reclaim idle Always Free instances. Keep reviewed backups and treat this as a
sandbox availability constraint. Free hosting does not include hosted model API allowances.
Oracle ARM target-host build/runtime and backup recovery still require verification.

Sources checked 23 October 2026: [free limits and idle reclamation](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm),
[home region](https://docs.oracle.com/en-us/iaas/Content/Identity/Tasks/managingregions.htm),
[Linux instance creation](https://docs.oracle.com/en-us/iaas/Content/Compute/tutorials/first-linux-instance/overview.htm).

## Linux server pilot

Use one Linux machine with systemd and persistent **local** disk, an unprivileged service
account, and private access over SSH/VPN. No Docker, Kubernetes, Redis, PostgreSQL or GPU is
required for this first operating increment. SQLite WAL is not supported on a shared network
filesystem; do not replicate this directory across independent active hosts.

Build a Linux binary from the reviewed commit on the Linux build host (a Windows `.exe` cannot
be installed as the Linux service). From that checkout:

```sh
cargo build --release --locked -p cosigna-agent
sudo useradd --system --home-dir /var/lib/cosigna --shell /usr/sbin/nologin cosigna
sudo install -d -m 0755 /opt/cosigna/bin
sudo install -m 0755 target/release/cosigna-agent /opt/cosigna/bin/cosigna-agent
sudo install -d -o cosigna -g cosigna -m 0700 /var/lib/cosigna
sudo -u cosigna /opt/cosigna/bin/cosigna-agent init --data-dir /var/lib/cosigna
sudo install -m 0644 deploy/systemd/cosigna-api.service /etc/systemd/system/cosigna-api.service
sudo install -m 0644 deploy/systemd/cosigna-worker.service /etc/systemd/system/cosigna-worker.service
sudo systemctl daemon-reload
sudo systemctl enable --now cosigna-api cosigna-worker
```

If the account already exists, inspect its configuration instead of recreating it. These
are administrator-run deployment instructions, not commands executed on the current Windows
computer. Keep the root-owned binary separate from service-writable ledger data. The units
use `UMask=0077`, restricted filesystem writes and restart limits; this is defense in depth,
not a sandbox against a compromised service account. Both processes are trusted local code.

The API remains `127.0.0.1:3100`, with strict Host checking and no browser origins. Do not open
port 3100 to the Internet. From a client computer, access through an authenticated SSH tunnel:

```sh
ssh -N -L 3100:127.0.0.1:3100 <ssh-user>@<server-host>
```

Then use `http://127.0.0.1:3100` locally. Supply owner/agent credentials through a private
authenticated channel; do not put keys in URLs, source, shell-history commands or chat.
A public reverse proxy does not by itself supply production owner identity or signed permits.

## Operator checks and upgrades

```sh
systemctl is-active cosigna-api cosigna-worker
curl --fail http://127.0.0.1:3100/v1/health
journalctl -u cosigna-api -u cosigna-worker --since '15 minutes ago'
```

The HTTP health endpoint checks API reachability only. Check systemd separately for worker
liveness and authenticated job status for paused/stale work. No alerting service is configured.
Restart loops hit systemd's start limit; inspect the cause before `systemctl reset-failed`.

Ledger schema is now version 3, additively migrated from v1/v2. Before the first upgrade,
stop both services and copy the **entire** private data directory, including both SQLite
databases, any WAL/SHM files and credentials, to an access-restricted backup. Test recovery on
an isolated copy. Never copy only a live `.sqlite` file and assume it contains all committed
WAL state. An older binary refuses v3; rollback requires a compatible binary or a reviewed,
complete pre-upgrade backup. Never replace an active ledger with an old backup after new
external effects without reconciling those effects separately.

Verified locally on Windows: queue/approval/restart/replay, grant boundaries, unknown outcome
backoff/holds, paused inference, owner-pause races, v1/v2 migration, shared inference lock,
HTTP auth/schema and OS lock release after killing an idle worker process. Simulated
interruption boundaries are not proof of process-kill durability during every payment phase.
The local pipeline increment also adds four real process-kill tests at fixed fixture
planning/claim/acceptance/confirmation boundaries. Broader provider failures and power loss
remain unverified; see `crates/cosigna-agent/tests/crash_recovery.rs`.
The Linux systemd units still need an actual target-host smoke test, reboot test and backup
restore drill. Production identity/permits, AgentAuth authorization consolidation, public API
rate limits/timeouts, alerting and a live connector remain tracked in G3–G6.

The automated two-process fixture check is `node scripts/check-gateway-service.mjs` after
building `cosigna-agent` in the default debug target; an explicit binary path is accepted.
It starts its own temporary API/worker, waits for the quote, stops the worker, approves while
it is down, restarts it and checks confirmation/terminal replay without a new enqueue.
CI runs that check and validates systemd unit syntax on Linux; neither provisions a server.
