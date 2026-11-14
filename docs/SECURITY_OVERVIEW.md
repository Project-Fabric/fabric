# Cosigna security overview and production checklist

For the IT and security people reviewing a Cosigna deployment. It says what runs, what is
stored, who can do what, what has been tested, and what has not. Status as of 28 October 2026;
every claim here is backed by the code and tests named, and the gaps are listed as plainly as
the controls. Details: [THREAT_MODEL.md](THREAT_MODEL.md), [STORAGE_FORMAT.md](STORAGE_FORMAT.md),
[deploy/docker](../deploy/docker/README.md).

## In one paragraph

Cosigna sits between a company's AI agents and its payment systems. An agent asks for a refund
or payout; Cosigna checks the agent's permission, its budget, the customer's case record and
the payment itself, then pays automatically, holds it for a person, or refuses, and records each
step in a hash-chained evidence log. **It runs entirely in the customer's infrastructure.**
No data, credentials or telemetry are sent to Cosigna the company.

## What runs and what it connects to

```
 browsers (owner, approvers) ─┐
 AI agents (API, MCP)  ───────┤ HTTPS :443
                              ▼
                       ┌─────────────┐
                       │ proxy (Caddy)│  TLS; only published service
                       └──────┬──────┘
          /gateway /v1 /identity │ everything else
                 ┌───────────────┴──────────────┐
                 ▼                              ▼
          ┌────────────┐  loopback 127.0.0.1  ┌─────────┐
          │  gateway   │◄─────────────────────│ console │ (same network namespace)
          └─────┬──────┘                      └─────────┘
                │ SQLite volume (cosigna-data)
          ┌─────┴──────┐
          │   worker   │  recovery lookups, alerts
          └────────────┘
```

**Inbound:** only the proxy's ports (443, and 80 which redirects to HTTPS). The gateway and
console ports are not published.

**Outbound,** only what the operator configures:

| To | When | Credential |
|---|---|---|
| `api.stripe.com` | Stripe rail enabled (`--stripe-test`) | Stripe **test** key; live keys are refused |
| The help desk (e.g. `*.zendesk.com`) | Case source configured (`--cases`) | Read-only token (Zendesk: OAuth `tickets:read`) |
| An EVM test-network node | Stablecoin rail enabled (`--evm-test`) | Node address (may hold an API key); a test wallet's private key; test chains only |
| Slack / webhook endpoints | Alerts configured (`--alerts`) | Slack webhook URL; HMAC signing secret |
| `api.openai.com` / `api.groq.com` | Only if the hosted planner is enabled (off by default; not used by money operations) | Model API key |

All outbound calls are HTTPS-only (loopback HTTP is accepted only for tests), follow no
redirects, and have size and time limits. Identity tokens are verified against keys in a local
file; no call is made to an identity provider.

## What is stored, and where

Everything is in the `cosigna-data` volume. **Cosigna does not encrypt it at rest; use an
encrypted disk or volume.**

| Data | Where | Notes |
|---|---|---|
| Budgets, agent grants, approvers, policies | `transactions.sqlite` | Agent and approver tokens are stored only as BLAKE3 hashes |
| Money operations | `transactions.sqlite` | Amount, currency, payment id, payee or source, case id, decision, approvals, and **the agent's free-text reason in plain text** (it may contain personal data) |
| Evidence log | `transactions.sqlite` (`evidence`, append-only by trigger) | Each step hash-chained; the reason is stored only as a digest |
| Identity records (when used) | `transactions.sqlite` | Issuer and subject of each enrolled person, organizations, memberships, approver bindings |
| Alert queue | `transactions.sqlite` | Operation id, event, delivery state; no message bodies |
| Owner, planner and executor credentials | `credentials.json` | **Plain text**, file mode 0600 on Unix; treat it like a root password |
| Stripe, help-desk and alert secrets | Environment or mounted files (`NAME_FILE`) only | Never written to the ledger, logs or evidence (tested) |
| Console sessions | Console process memory | Lost on restart; approver keys held only for the session |

Logs are JSON lines on stdout with no tokens or secrets (checked in tests and in the Docker
runs). Alerts carry the amount, reason category, agent name, case id and operation id, never a
credential, the customer, or the agent's free-text reason.

## Who can do what

- **Owner** (credential in `credentials.json`): creates budgets, agents and approvers, revokes
  them, approves or rejects, settles unknown results, reads everything. Today the owner signs
  in to the console with a one-time link printed in the console's log.
- **Agents**: one token each, scoped to one budget, the operation kinds and rails granted, an
  automatic limit, an expiry, and optionally "must name a case". They cannot see other agents'
  operations, approve anything, or change policy. Revocable at once (`POST /gateway/v1/agents/{id}/revoke`).
- **Approvers**: own key, a currency and a per-payment limit; approve or reject only. Above a
  budget's two-person threshold, two different people must approve the exact same proposal.
  Revocable at once.
- **Identity-bound approvers** (newer, see [GATEWAY_IDENTITY.md](GATEWAY_IDENTITY.md)): a
  verified OIDC subject explicitly bound by the owner to a named approver; membership alone
  grants nothing; everything is rechecked inside the approval transaction.

Every approval is bound to a digest of the exact proposal (amount, currency, payment, payee,
case facts); a changed proposal needs a new approval.

## Money safety controls

- **Checked before any decision:** the payment system's own record (refundable amount,
  currency, live mode refused), the budget, the agent's grant and limits, and, when required,
  the customer's case (customer, payment, amount still owed). A help desk that cannot be read
  makes the request fail closed; nothing is decided.
- **Paid at most once:** each operation has a single attempt key used as the payment system's
  idempotency key; a repeat returns the same result.
- **Unknown results stay held:** if the payment system's answer is lost, the money stays
  reserved; the worker only looks the result up and never sends the payment again.
- **Limits per client:** each credential has a request budget; an address with repeated refused
  tokens is blocked for the rest of the minute; capacity is kept for people so approvals and
  revocations still work when agents flood the gateway.

## Evidence and audit

Every budget, grant, decision, approval and payment result is appended to a hash-chained log.
`cosigna-agent evidence verify` re-hashes the chain and checks current state against it;
`evidence export` writes it out; an independent verifier in Python (`tools/evidence-verifier`)
checks an export without Cosigna's code.

**Limit:** the chain is not yet signed or anchored externally. It detects edits, deletions and
reordering, but someone with write access to the database could rewrite the whole chain
consistently. Keep exports somewhere the gateway cannot write.

## Hardening defaults (deploy/docker)

- TLS at the proxy; the gateway and console refuse requests that do not name the public host
  or did not arrive over HTTPS, even with valid credentials; no CORS; browser `Origin` headers
  refused by the gateway.
- Containers run as an unprivileged user, read-only root filesystem, no Linux capabilities,
  `no-new-privileges`; only the proxy publishes ports.
- Console cookies `Secure`, `HttpOnly`, `SameSite=Strict`; CSRF token on every change.
- Request timeouts, a concurrency limit that answers `busy` instead of queueing, and graceful
  shutdown that lets a payment in flight finish and be recorded.

## What has been tested

- 341 Rust tests, plus console, mobile and CLI suites (`node scripts/check-all.mjs`).
- An attack lab run against the real gateway: 30 attacks (authorization, authentication,
  isolation, grants, money, tampering, replay, input, transport, races), stress and a crash test,
  with the evidence verified after each phase.
- "Break checks": key security lines deliberately broken to confirm a test fails.
- By hand, once each: refunds through a real Stripe **test** account; case checks against a real
  Zendesk trial account; the full Docker stack through the proxy; alerts end to end.

## What is not done yet

Stated plainly, because a reviewer will ask:

- **No independent security review or penetration test,** and no SOC 2 or ISO 27001 report.
- **No sign-in with a company account in the console yet** (Google, Microsoft); the owner uses
  a one-time link and approvers use keys. Identity tokens are verified only in the strict
  RFC 9068 format, from a local key file.
- **No owner-credential rotation command.** Agents, approvers and budgets can be revoked;
  replacing the owner credential is a manual, offline step.
- **No encryption at rest** of the ledger (use encrypted storage).
- **Evidence is not signed or externally anchored** (see above).
- **One server:** SQLite on one volume; no clustering or failover. Limits are per process.
- **Stripe test mode only;** no live payment rail is connected.
- **Platforms:** developed and tested on Windows; the Linux containers were exercised with Docker
  Desktop. Hosted CI on Linux and macOS is currently paused, so local runs cover one OS.

## Production checklist

Before go-live:

1. **Address and TLS:** a DNS name for Cosigna; set `COSIGNA_PUBLIC_HOST` and, if the HTTPS port
   is not 443, `COSIGNA_CONSOLE_ORIGIN`. Use a public certificate, or your company's (edit the
   Caddyfile). Do not publish ports 3092 or 3090.
2. **Storage:** put the `cosigna-data` volume on encrypted storage, backed up.
3. **Owner credential:** read it once (`docker compose exec gateway cat /data/credentials.json`),
   store it in your password manager, and limit who can run commands on the host.
4. **Secrets:** Stripe and help-desk keys and alert secrets as files under `deploy/docker/secrets`
   (readable by uid 10001 only), never on the command line. Use a read-only help-desk token.
5. **Agents:** one agent per job, each with the smallest budget, kinds, rails and automatic limit
   that work, an expiry, and "must name a case" for refunds.
6. **People:** register approvers with currency limits; set a two-person threshold for large
   payments.
7. **Alerts:** configure Slack or a webhook (`examples/alerts`) so held payments reach a person.
8. **Check:** `docker compose ps` shows everything healthy;
   `docker compose exec gateway cosigna-agent evidence verify --data-dir /data` passes.

Running it:

- **Backups:** stop the gateway and worker (or the whole stack), copy the volume, restart; verify
  the evidence after any restore. Also export the evidence regularly to storage the gateway
  cannot write.
- **Upgrades:** back up first; rebuild and `docker compose up -d --wait`. The ledger upgrades
  itself on start; downgrading is not supported. Verify the evidence afterwards.
- **Monitoring:** the gateway health check; the worker's JSON lines (`cosigna.recovery/v2`
  `needs_person`, `cosigna.alert-delivery/v1` `failed`); payments held or unknown in the console.
- **Incidents:** revoke the agent (`POST /gateway/v1/agents/{id}/revoke`) or approver at once;
  revoke the budget to stop all its agents; rotate any provider key at the provider; review the
  operation's timeline and verify the evidence.

**Reporting a vulnerability:** contact address to be published by the founders before the first
customer deployment.
