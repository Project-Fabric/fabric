# Self-hosting Cosigna with Docker

Runs the Cosigna gateway in your own infrastructure: the API, the worker (owner-enrolled
tasks and lookup-only recovery of refunds whose result is unknown) and a TLS proxy (Caddy).
Payment-system and help-desk keys stay on your machines; nothing is sent to Cosigna.

```sh
cd deploy/docker
COSIGNA_PUBLIC_HOST=cosigna.example.com docker compose up -d --wait
```

The default uses the synthetic fixture rail, so you can try it without any keys. With no
`COSIGNA_PUBLIC_HOST`, it serves `https://localhost` with Caddy's local certificate authority.
Use `COSIGNA_HTTPS_PORT` and `COSIGNA_HTTP_PORT` to publish other ports.

## What runs

| Service | Role | Reachable from |
|---|---|---|
| `proxy` | TLS termination; redirects HTTP to HTTPS | the published ports |
| `gateway` | the authenticated API (`serve --listen 0.0.0.0:3092 --public-host ...`) | the proxy only |
| `worker` | scheduled tasks and refund recovery | nothing (no port) |

The gateway and worker share the `cosigna-data` volume (SQLite ledger, role credentials).
Both run as an unprivileged user with a read-only root filesystem, no Linux capabilities and
`no-new-privileges`. A health check (`cosigna-agent probe`) runs inside the gateway container.

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

Agents get their own scoped tokens from the owner (`POST /gateway/v1/agents`); approvers sign
in with their own tokens. Never give an agent the owner credential.

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
- **Development credentials.** Role tokens in a file; no single sign-on yet. Rate limiting,
  alerts and the console are not part of this stack yet.
- **Test mode only** for Stripe. Live payment rails are not connected.

Checked on 27 October 2026 with Docker 29.8 on Windows (Linux containers): the stack started
healthy; through the proxy, health answered and an unauthenticated request was refused;
the owner listed operations; an agent's refund within its limit succeeded and one above
what was refundable was denied; plain HTTP redirected to HTTPS; the gateway port was not
reachable from the host; no credential appeared in the logs; operations survived a restart;
the evidence verified; the Stripe-test overlay read both secret files without printing them.
After the move to axum (same day), `docker compose stop` made the gateway log that it was
finishing requests in progress and exit with code 0 in about half a second.
The automated tests (`crates/cosigna-agent/tests/self_host.rs`) cover the host and HTTPS
checks, the listen refusals, the probe and secrets read from files, without Docker.
