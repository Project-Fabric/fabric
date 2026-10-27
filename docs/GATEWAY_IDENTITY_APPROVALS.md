# Identity-backed money approvals in Rust

27 October 2026, E18 second increment. Builds on [token verification and enrollment](GATEWAY_IDENTITY.md).
The Rust gateway remains the only financial authority; Rails stays frozen. This adds the
authenticated money-operation API, not browser login or a deployed identity-provider setup.

## Authority comes from explicit bindings

An installed owner explicitly binds **one organization to that gateway owner namespace**.
The binding exposes that namespace's existing and future financial records to its authorized
approvers. Another organization cannot share that owner namespace. There is no caller-selected
gateway destination or owner field and no mapping inferred from a successful health check.

The operator then binds an active membership in that organization to an existing named
approver belonging to the same gateway owner. The existing approver's currency and per-operation
limit remain authoritative. `owner`/`admin`/`member`/`viewer` membership roles do not create
spending authority; this separate, explicit approver grant does.

One enrolled user can occupy only one approver slot per owner. Each approver can have only one
identity binding. Uniqueness includes revoked rows, so revoking and rebinding cannot give the
same enrolled person another slot in a two-person decision. A bound approver's former key is
permanently refused, including after binding revocation. A key authenticated before the binding
was committed is also refused at the deciding transaction. Unbound approver keys retain their
existing behavior. The installed owner credential remains privileged operator authority.

These constraints establish distinct enrolled accounts/approver slots, not proof that two
different real-world humans control them. The operator must validate enrollment and avoid
duplicate human accounts. Owner/key-based legacy approvals also rely on operator custody.

## Operator-only administration

The existing owner credential is required; unknown request fields are rejected:

| Route | Body |
| --- | --- |
| `POST /gateway/v1/identity/gateway_bindings` | `{ "organization_id": "org_..." }` |
| `POST /gateway/v1/identity/approver_bindings` | `{ "membership_id": "mem_...", "approver_id": "approver_..." }` |
| `POST /gateway/v1/identity/gateway_bindings/:id/revoke` | `{}` |
| `POST /gateway/v1/identity/approver_bindings/:id/revoke` | `{}` |

Responses use the `cosigna.identity/v1` envelope. The owner is derived from authenticated
operator authority. The API verifies active ownership, membership, user, organization and
approver records inside the registration transaction. Bindings are permanent and revocation
is idempotent. There is no reactivation, reassignment or automatic organization migration API.

## Identity-token money routes

Tokens must pass the existing restricted provider-neutral RS256 access-token verifier.
`cosigna:read` remains required. Approval and rejection additionally require `cosigna:approve`.
Tokens are accepted only in one Authorization header, not cookies, body fields or query strings.

Every route below also requires an active explicitly bound approver for the selected org:

| Route after `/identity/v1/organizations/:org` | Behavior |
| --- | --- |
| `GET /approver` | The caller's named approver metadata, without credentials. |
| `GET /agents` | Agent names/ids only, scoped to that gateway owner. |
| `GET /operations` | Existing operation list; optional `stage=held` or `stage=unknown`. |
| `GET /operations/:id` | One same-owner operation. |
| `GET /operations/:id/timeline` | Its operator timeline. |
| `POST /operations/:id/approve` | `{ "proposal_digest": "..." }`; exact proposal approval, then server-side single-attempt execution when all approvals are complete. |
| `POST /operations/:id/reject` | `{}`; rejects an unsubmitted operation without moving funds. |

The money-operation and timeline response envelopes retain their existing versions. The
original `/gateway/v1/` and `/v1/` paths still refuse JWTs. Identity callers cannot create
operations, execute arbitrary requests, register agents, change budgets, reconcile/resolve
operations or call administration endpoints. Identity-based purchase-quote approval is not
implemented here. Company agents continue to use their separate budget-bound agent tokens.

A token missing approval scope receives HTTP 403 with `insufficient_approval_scope` and a
Bearer challenge naming `cosigna:approve`. Missing/foreign/revoked financial bindings are
`not_found`; cross-owner operation ids are also hidden. Other financial validation errors
retain the existing gateway codes. POST requires bounded JSON and `application/json`.

## Transaction and revocation semantics

The request first verifies the token and resolves its subject and explicit binding. Inside
each `BEGIN IMMEDIATE` approval/rejection transaction, the gateway rechecks the exact subject,
active user, organization, membership, owner binding, approver binding, named approver and
token expiry/scope. A revocation committed before that lock is acquired wins. Proposal
digest/expiry, currency, per-person limit and budget checks still run in the existing engine.
Identity proposal/claim expiry checks use the time after acquiring the write lock.

In a two-person operation, repeating one approver cannot complete it. Before counting a second
person, the first identity's exact subject and bindings must still be active. A durable first
approval does not need its earlier session token to remain unexpired; it remains bound to the
same proposal and enrolled authority. No new authority is obtained from an expired token.

Before any **new submission claim**, including one requested by another executor or the
operator, both recorded identity-backed approvals are revalidated against their current
records. Revocation cannot be bypassed by switching to an agent/owner credential to execute
an already-approved operation. A legacy key approval that predates a new identity binding
also cannot silently become an identity-backed approval. Failed checks create no attempt and
send no payment; an existing hold stays reserved. The operator must review/reject the old
request and obtain a fresh proposal/approval. No automatic resubmission or approval reset occurs.

Revocation after a submission claim cannot cancel a request already sent to a payment rail.
Its original attempt is completed/reconciled through the existing recovery path. A completed
refund is not undone by later identity revocation. Replayed successful approvals do not
create another payment, subject to the existing idempotency and recovery boundary.

## Ledger and evidence

Ledger v11 adds `identity_gateway_bindings` and `identity_approver_bindings`; v10 and older
valid ledgers gain empty tables with no inferred mapping. Binding creation/revocation commits
atomically with registry row-digest evidence. Existing transaction digests, operation JSON,
provider stores, credentials and attempt identifiers are unchanged. Older executables refuse
v11. Stop all processes using a profile and back it up before upgrade. Development tests use
temporary synthetic profiles; this change does not upgrade an existing demo or payment store.

Identity-backed operation decisions and subsequent submission/outcome events add `by.identity`:
verified `issuer`, `subject`, `subject_id`, `user_id`, `organization_id`, `membership_id`,
`gateway_binding_id`, `approver_binding_id`, `verified_at`, `token_expires_at`. The existing
`by.approver` identifies the named limit/grant. JWTs, signing keys and provider secrets are
never retained. Issuer/subject are now deliberately retained in this **private financial audit
trail**, so evidence exports and backups require appropriate access and retention controls.
Basic identity-read responses and enrollment row-digest events still do not repeat them.

Rust evidence verification checks chain integrity and registry/binding rows against recorded
digests. The independent Python verifier continues to replay existing financial rules. Neither
verifier re-authenticates a historical JWT or proves its signature from `by.identity` metadata;
provider token verification happens at runtime. These records do not establish an external
identity attestation or resist an attacker rewriting the whole database and evidence chain.

## Validation and remaining E18 work

Synthetic tests cover scoped identity refund/replay, amount and currency limits, separate
approval scope, exact digest, cross-owner denial, explicit binding uniqueness, old-key
invalidation, each revocation path, two-person identity separation, first-person revocation,
authentication/claim races, expiry, different-executor bypass, row tampering, additive v10
migration and the real HTTP boundary. Signing keys are generated in memory; no identity or
payment provider is contacted.

Still open: provider-neutral browser authorization-code/PKCE login and console sessions,
real IdP configuration/validation, enrollment administration UX, key/session operating policy,
independent identity-audit review, additional payment/merchant validation and production
security review. This increment does not make a production SSO or public payment launch claim.
