# Cosigna Rails application backend

27 October 2026: Rails 8.1.4 now provides a configured signed-access-token boundary,
operator-enrolled users, organizations and membership-scoped metadata reads. No identity
vendor is selected. Real provider sign-in, gateway owner binding, customer transaction APIs,
agent/integration setup, notifications and application billing remain unfinished.

The Rust gateway remains authoritative for budgets, exact approval, reservations,
submission attempts, provider results and reconciliation. Rails does not open its SQLite
files or maintain a second copy of that ledger. The existing Python authorization service
and Console/mobile interfaces remain intact; consolidation requires the shared G3 contract.

## Start on Windows with Docker Desktop

Run from the project directory:

```powershell
docker compose -f backend/compose.yaml build web
docker compose -f backend/compose.yaml run --rm web bundle exec rails db:prepare
docker compose -f backend/compose.yaml up -d web
```

Open **http://127.0.0.1:3101/api/v1/platform**. Its `cosigna.backend/v2` metadata reports the
implemented identity boundary, operator-only enrollment and the absence of gateway owner
mapping and transaction writes. `/up` reports Rails application health only. Protected
identity routes return 503 until all provider trust settings are supplied.

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

These credential-free status routes do not establish owner identity or spending authority.
The separate authenticated identity routes below expose only application metadata.

## Provider-neutral identity and organization reads

`GET /api/v1/me`, `GET /api/v1/organizations` and `GET /api/v1/organizations/:id` require a
verified RFC 9068 access token and an active, operator-enrolled `(issuer, subject)` identity.
Organization roles/membership come from the application database. Query parameters, email
claims and claimed token roles cannot grant membership. Cross-organization access returns
the same 404 as a nonexistent organization; listing is bounded to 50 records per page.

Configure issuer, API audience, allowed client ids and an absolute path to a public RSA JWKS.
The `.env.example` lists public settings; no token or private key belongs there. The optional
`compose.identity.example.yaml` override mounts the public file read-only. After setting its
required variables, apply both files to the same local Compose project:

```powershell
docker compose -f backend/compose.yaml -f backend/compose.identity.example.yaml up --build -d web
```

The verifier supports RS256 access tokens with a known key id, exact issuer/audience,
approved client id, `cosigna:read` scope and at most one-hour lifetime. It never performs
key discovery from token input. Removing an identity/key or revoking membership prevents
subsequent corresponding reads. Tokens and raw claims are not persisted or returned.

No login/token-issuance, self-service enrollment, public membership mutation, gateway task
access or financial delegation is implemented. Even an application `owner` role grants no
gateway payment authority. See the [identity/authority contract](../docs/IDENTITY_AND_AUTHORITY_CONTRACT.md)
for exact configuration, enrollment, key rotation, errors and remaining acceptance work.

## Storage and next increments

Active Record uses this application's separate `storage/development.sqlite3` and disposable
`storage/test.sqlite3`. Migration `20261027010000` adds users, external identities,
organizations and memberships, with unique indexes, foreign keys and a stored-role check.
There are no payment records or gateway ledger migrations. Generated keys, credentials,
logs, local databases and runtime files are excluded from Git and image build context.

1. Select/onboard a real identity provider and validate the supported access-token profile,
   enrollment, account lifecycle and revocation in that system.
2. Implement a trusted gateway-instance/owner mapping, then delegated read access with
   cross-owner denial before exposing gateway task data. Public gateway health is not proof
   of owner identity.
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
