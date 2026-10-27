# Decision: one backend, in the Rust gateway (27 October 2026)

**Decided by:** Ashok (founder), on Claude's recommendation. **Supersedes:** building identity
and organizations in the Rails app (`backend/`, draft PR #97).

## Decision

Cosigna has one backend: the Rust gateway (`crates/cosigna-agent`). Identity (OpenID Connect
sign-in), organizations, memberships and approver identity are built there, next to the
approvals and money operations they authorize. The console stays the user interface and talks
to the gateway's API. The Rails app is frozen: no new features.

## Why

1. **Approvals happen in the gateway.** "This person approved this payment" must be checked by
   the system that moves the money. Identity in a second service would need a signed hand-off
   (the "spending permit" in `IDENTITY_AND_AUTHORITY_CONTRACT.md`) that one backend avoids.
2. **Self-hosting stays one install** (`deploy/docker`): one gateway image and a TLS proxy. A
   second runtime and database would be more for a customer's security team to review and
   patch, and `AGENTS.md` keeps the core free of cloud dependencies.
3. **Fewer stacks to secure.** With agents writing the code, the cost is the number of
   languages and services to secure, test and deploy, not typing speed.

## Consequences

- The gateway's HTTP layer moves from `tiny_http` to `axum` (request timeouts, rate limits,
  more than eight requests at once, graceful shutdown). Claude does this first, on
  `enterprise/axum`, without changing the API.
- Codex ports #97's identity contract and its test cases to the gateway: provider-neutral OIDC
  token verification (issuer, audience, expiry, signature from configured keys), users,
  organizations and memberships in the ledger, cross-organization isolation, revocation, and
  approvers bound to verified identities, with the provider and subject in the evidence.
  Ledger format version bumped and `STORAGE_FORMAT.md` updated, as usual.
- SQLite stays for self-hosted single servers. Postgres only when a hosted, multi-customer
  service exists.
- A hosted dashboard (billing, team settings) can later sit on top of the gateway API, in any
  framework, without holding identity or money logic.
- `docs/IDENTITY_AND_AUTHORITY_CONTRACT.md` (on #97) is kept as the specification; PR #97 is
  closed unmerged once the port lands.
