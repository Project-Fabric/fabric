# Self-hosting Cosigna with Docker

Before going live, work through the [production checklist](../../docs/SECURITY_OVERVIEW.md#production-checklist).

Runs Cosigna in your own infrastructure: the gateway API, the console (where people approve
payments), the worker (owner-enrolled tasks, lookup-only recovery of refunds whose result is
unknown, and alerts) and a TLS proxy (Caddy).
Payment-system and help-desk keys stay on your machines; nothing is sent to Cosigna.

```sh
cd deploy/docker
COSIGNA_PUBLIC_HOST=cosigna.example.com docker compose up -d --wait
```

The default uses the synthetic fixture rail, so you can try it without any keys. With no
`COSIGNA_PUBLIC_HOST`, it serves `https://localhost` with Caddy's local certificate authority.
Use `COSIGNA_HTTPS_PORT` and `COSIGNA_HTTP_PORT` to publish other ports; then also set
`COSIGNA_CONSOLE_ORIGIN` to the address people type, with the port (for example
`https://localhost:8443`), because the console accepts only that exact origin.

## What runs

| Service | Role | Reachable from |
|---|---|---|
| `proxy` | TLS termination; redirects HTTP to HTTPS | the published ports |
| `gateway` | the authenticated API (`serve --listen 0.0.0.0:3092 --public-host ... --trust-local-clients`) | the proxy, and the console beside it |
| `console` | the web console, in the gateway's network namespace | the proxy only |
| `worker` | scheduled tasks, refund recovery, alerts | nothing (no port) |

The proxy sends `/gateway/*`, `/v1/*` and `/identity/*` to the gateway and everything else to
the console. The gateway and worker share the `cosigna-data` volume (SQLite ledger, role
credentials); the console mounts it read-only to read the owner credential, which it never
sends to the browser. All run as an unprivileged user with a read-only root filesystem, no
Linux capabilities and `no-new-privileges`. A health check (`cosigna-agent probe`) runs inside
the gateway container.

The console shares the gateway container's network and calls it at `127.0.0.1:3092`.
`--trust-local-clients` lets the gateway accept that local name without the proxy's headers,
but only from its own loopback address, which nothing outside that network namespace can use.

**The gateway refuses any request that does not name the public host and arrive with
`X-Forwarded-Proto: https`.** That is what the proxy sends; a request that bypasses it,
names another host or came over plain HTTP gets 403, even with a valid credential. Browser
`Origin` headers are refused as before. Health checks may also use the local name inside the
container.

## Credentials

On first start the gateway creates `/data/credentials.json` with the development role
credentials (owner, planner, executor). Read the owner credential once and keep it in your
password manager:

```sh
docker compose exec gateway cat /data/credentials.json
```

Agents get their own scoped tokens from the owner (`POST /gateway/v1/agents`). Never give an
agent the owner credential.

**The console.** On each start it prints a one-time owner link, valid for 10 minutes:

```sh
docker compose logs console | grep token=
```

Open it once in your browser; it signs you in as the owner (`COSIGNA_CONSOLE_USER` names you)
and then stops working. Restart the console for a new one. Approvers open `/signin` and paste
the `approverkey_` the owner gave them. Sessions are cookies marked `Secure`, `HttpOnly` and
`SameSite=Strict`, and the console accepts requests only for its public origin over HTTPS.

## Stripe test mode and help-desk case records

```sh
mkdir -p secrets config
# Write each secret to its own file, with your editor or a password manager. Not on the
# command line: it would stay in your shell history.
#   secrets/stripe_test_key   a Stripe TEST secret key (sk_test_...); live keys are refused
#   secrets/cases_secret      the help-desk token named by the cases file
cp ../../examples/cases/zendesk-oauth.json config/cases.json   # then edit it
docker compose -f compose.yaml -f compose.stripe-test.yaml up -d --wait
```

`secrets/` and `config/` are git-ignored. The containers read the secrets through
`COSIGNA_STRIPE_TEST_SECRET_KEY_FILE` and `COSIGNA_CASES_SECRET_FILE` (any variable the
gateway reads a secret from also accepts a `_FILE` form) and never print them. Compose mounts
secret files with their host permissions: the container user (uid 10001) must be able to read
them, for example `chmod 644` in a directory only you can open, or `chown 10001`.

## Your own certificate or an internal name

For a public DNS name, Caddy obtains a certificate automatically (ports 80 and 443 must be
reachable from the internet). For an internal name, edit `Caddyfile` and add `tls internal`
(Caddy's CA) or `tls /certs/cert.pem /certs/key.pem` with your company's certificate mounted
into the proxy. Any other TLS proxy works if it keeps the `Host` header and sets
`X-Forwarded-Proto: https`; do not publish the gateway's port 3092 directly.

## Backups and upgrades

Everything that matters is in the `cosigna-data` volume. Stop the stack (or at least the
gateway and worker) before copying it, so the SQLite files are consistent. To upgrade,
rebuild the image and `docker compose up -d --wait`; the ledger format is upgraded on start
when needed (see `docs/STORAGE_FORMAT.md`). Verify the evidence after an upgrade or restore:

```sh
docker compose exec gateway cosigna-agent evidence verify --data-dir /data
```

## What this is not yet

- **One server.** SQLite on one volume; no clustering or failover. The gateway handles 32
  requests at once by default (`--max-concurrent`) and answers `busy` beyond that. Each
  credential gets 20 requests per second (`--rate-limit`); an address sending wrong tokens is
  blocked for the rest of the minute after 30 (`--refusals-per-minute`). Limits are per
  gateway process and reset on restart.
- **Development credentials.** Role tokens in a file; no single sign-on with a company account
  yet: the owner signs in with the one-time link and approvers with their keys. Alerts need
  `--alerts` on the worker (`examples/alerts`).
- **Test mode only** for Stripe. Live payment rails are not connected.

Checked on 27 October 2026 with Docker 29.8 on Windows (Linux containers): the stack started
healthy; through the proxy, health answered and an unauthenticated request was refused;
the owner listed operations; an agent's refund within its limit succeeded and one above
what was refundable was denied; plain HTTP redirected to HTTPS; the gateway port was not
reachable from the host; no credential appeared in the logs; operations survived a restart;
the evidence verified; the Stripe-test overlay read both secret files without printing them.
On 28 October 2026, with the console added: all four services started; through the proxy
only, an agent's refund was held, the console refused requests without a session, the one-time
link from the logs signed the owner in, the console listed the held refund with no errors and
without the owner credential, approving it there paid it, the console's port was not reachable
from the host, no token appeared in the logs, and the evidence verified.
After the move to axum (same day), `docker compose stop` made the gateway log that it was
finishing requests in progress and exit with code 0 in about half a second.
The automated tests (`crates/cosigna-agent/tests/self_host.rs`) cover the host and HTTPS
checks, the listen refusals, the probe and secrets read from files, without Docker.
