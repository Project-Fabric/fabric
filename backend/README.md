# Cosigna Rails application backend

**Frozen, 27 October 2026:** the founder selected one backend in the Rust gateway. No new Rails
features should be added. PR #97's identity contract/test cases are a porting reference; the
first implemented Rust boundary is described in [Gateway identity](../docs/GATEWAY_IDENTITY.md).
The paragraphs below describe the earlier Rails foundation and are historical context.

First increment, 24 October 2026: a Rails 8.1.4 API application over the existing Rust
transaction foundation. Rails is selected for future customer accounts, organizations,
agent setup, integration management, notifications and application APIs. The current
increment is a local development foundation, not those completed workflows.

The Rust gateway remains authoritative for budgets, exact approval, reservations,
submission attempts, provider results and reconciliation. Rails does not open its SQLite
files or maintain a second copy of that ledger. The existing Python authorization service
and Console/mobile interfaces remain intact; consolidation requires the shared G3 contract.

## Start on Windows with Docker Desktop

Run from the project directory:

```powershell
docker compose -f backend/compose.yaml up --build -d web
```

Open **http://127.0.0.1:3101/api/v1/platform**. The JSON explicitly reports the Rails
foundation, Rust transaction authority, unimplemented customer authentication and absence
of transaction-write endpoints. `/up` reports Rails application health only.

Docker supplies Ruby; a Windows Ruby installation is not required. The Ruby image is pinned
by digest, Rails is pinned in the Gemfile, and transitive gems are locked in Gemfile.lock.
The container runs as an unprivileged user. Its HTTP port is published on loopback only.
This Dockerfile is for local development, not an approved production deployment.

```powershell
docker compose -f backend/compose.yaml logs --tail 50 web
docker compose -f backend/compose.yaml stop web
docker compose -f backend/compose.yaml run --build --rm test
```

## Read-only Rust health adapter

`GET /api/v1/gateway/health` calls only the gateway's public `GET /v1/health` route. It
needs no owner/agent/model/payment key. It accepts no destination from the incoming request,
disables environment proxies and retries, sets one-second connection/read/write timeouts,
refuses redirects, caps the response at 4 KiB, verifies the current sandbox envelope and
returns only whitelisted status fields. It does not read tasks, obtain approval or execute
payments. Application health and verified gateway health are deliberately separate.

The adapter is unconfigured by default and returns HTTP 503 with `gateway_unconfigured`.
To probe an already running local Rust gateway, supply its actual port before starting web:

```powershell
$env:COSIGNA_GATEWAY_HEALTH_PORT = '3100'
docker compose -f backend/compose.yaml up -d web
```

Compose uses Docker Desktop's `host.docker.internal` to reach the host. The adapter sends
the Rust server's required `Host: 127.0.0.1:<port>` header. Native Ruby development defaults
to `127.0.0.1`; only these two destinations are accepted. No model or provider request is
made. Invalid configuration, unavailable transport and unverified replies have distinct
503 codes; upstream content and exception details are not returned to callers.

Only these credential-free status routes exist. They do not establish customer identity,
tenant isolation, account enrollment or authorization. Those require dedicated implementations
and tests before customer data or transaction writes are added.

## Storage and next increments

Active Record is configured for this application's separate `storage/development.sqlite3`
and disposable `storage/test.sqlite3`. There are no customer domain tables or payment
migrations in this increment. Generated keys, credentials, logs, local databases and runtime
files are excluded from Git and image build context. Existing Cosigna stores are untouched.

1. Define verified customer identity and organization membership, including the mapping to
   the gateway's owner/agent authority. Coordinate this with signed permits and G3.
2. Implement authenticated, tenant-scoped read access through that contract; add cross-owner
   denial and credential-isolation tests before exposing gateway task data.
3. Add exact-quote approval proof and narrowly scoped write delegation through the same
   authority contract. Rails jobs and model output cannot create approval.
4. Add integration management, notifications and operator workflows without duplicating
   the authoritative transaction state machine.

Rails can implement a complete backend. Retaining Rust here preserves the existing tested
engine; any future migration must first reproduce its accounting and failure invariants.

References: [Rails API applications](https://guides.rubyonrails.org/api_app.html),
[Rails security guide](https://guides.rubyonrails.org/security.html),
[Cosigna unification](../docs/COSIGNA_UNIFICATION.md),
[gateway contract](../docs/COSIGNA_GATEWAY.md).
