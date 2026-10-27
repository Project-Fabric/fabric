# Customer identity and transaction authority

27 October 2026: first G3/#72 application-identity increment, also tracked under Rails #78.
The founder selected a provider-neutral implementation; no identity vendor is selected.

## Implemented boundary

The Rails application can verify a configured issuer's signed JWT access token, identify an
operator-enrolled user by `(issuer, subject)`, and read that user's active organizations.
Organization membership and role come from the application database, never from an incoming
owner id, email address or token role/organization claim.

Only these new routes are implemented:

| Route | Response | Authorization |
| --- | --- | --- |
| `GET /api/v1/me` | Public user id and display name. | Verified access token, enrolled active identity and active user. |
| `GET /api/v1/organizations` | At most 50 active memberships, with organization id/name, stored role and optional next cursor. | Same user boundary; query always starts from that user's memberships. |
| `GET /api/v1/organizations/:id` | The user's membership view for one active organization. | Active membership; unrelated and nonexistent organizations both return the same 404. |

Membership roles are `owner`, `admin`, `member` and `viewer`. In this increment they describe
application membership. They grant no gateway payment, administrative or approval authority.
Enrollment, identity linking, membership mutation and role changes have no public API.

The envelope is `cosigna.identity/v1`. Platform metadata advances to `cosigna.backend/v2`
to describe this new boundary and the still-unimplemented gateway owner mapping. The existing
credential-free gateway-health envelope stays `cosigna.backend/v1` and retains its behavior.

## Trusted token profile

This is a resource-server verifier, not a login server or token issuer. It supports the
following restricted profile based on RFC 9068:

- Compact, signed JWT access tokens with RS256 and an RSA public key of 2048-4096 bits.
- Explicit `typ: at+jwt` or `application/at+jwt` (media type comparison is case-insensitive),
  and a known key id. OIDC ID tokens (`typ: JWT`), unsigned tokens, HMAC tokens and extra
  header parameters are refused. In particular `jku`, `jwk`, `x5u`, `x5c` and `crit` cannot
  select a key or cause a network request.
- Exact configured HTTPS issuer and this API's configured audience. Array audiences are
  allowed only as a bounded array of strings containing that audience.
- Required `sub`, `client_id`, `iat`, `exp`, `jti` and `scope`, in addition to `iss` and `aud`.
  Client id must be in an operator-defined allowlist and scope must include `cosigna:read`.
- Integer dates; current time must be before expiry, at/after optional `nbf`, and not before
  issued-at. Maximum lifetime is one hour. This increment uses no clock-skew allowance.
- Bounded header/token/key-set and claim sizes. Cryptographic verification is performed by
  the locked `jwt` gem; there is no application cryptographic signing implementation.

This does not establish compatibility with every provider's token defaults. A provider must
be configured to issue this access-token profile before a real sign-in integration is tested.
Opaque access tokens, automatic discovery/introspection, login redirects, PKCE callbacks,
passkeys, refresh-token handling, token issuance and a login UI are not implemented here.

## Public-key configuration and rotation

All four settings are operator-controlled and required together:

| Setting | Meaning |
| --- | --- |
| `COSIGNA_IDENTITY_ISSUER` | Exact HTTPS issuer string, including any trailing slash. |
| `COSIGNA_IDENTITY_AUDIENCE` | Audience for this application API. |
| `COSIGNA_IDENTITY_CLIENT_IDS` | JSON array of 1-10 permitted client ids. |
| `COSIGNA_IDENTITY_JWKS_PATH` | Absolute path of a local public RSA JWKS, at most 64 KiB and 10 keys. |

Default configuration has no identity provider. Protected routes return 503 rather than
accepting a development token or self-declared user. Incomplete, malformed or unsafe key
configuration also fails closed. Public platform/application/gateway-health routes remain.

The key file contains public verification keys only. Private RSA parameters, duplicate key
ids, encryption keys, weak keys or incompatible algorithms/operations are rejected. Unknown
key ids return 401; they never trigger discovery or remote key fetching.

The public file is reread for each authenticated request in this first increment. Operators
can publish overlapping old/new public keys, then atomically replace the file to retire an
old key. A removed key fails on the next request; a partial or unreadable update produces 503.
There is no provider-managed key-refresh job or production throughput claim for this loader.

Native Ruby can use the settings directly. Docker setup is described by
[`backend/compose.identity.example.yaml`](../backend/compose.identity.example.yaml), an
optional override that mounts only a public file read-only. The default Compose setup mounts
no identity keys. Never use a payment key, provider client secret or private signing key as
the public JWKS file.

## User enrollment and revocation

Enrollment is performed by a trusted operator using the Rails console/administration process.
For a synthetic development account, the data relationship is:

```ruby
ApplicationRecord.transaction do
  user = ApplicationUser.create!(display_name: "Development user")
  ExternalIdentity.create!(
    application_user: user,
    issuer: ENV.fetch("COSIGNA_IDENTITY_ISSUER"),
    subject: "the-subject-verified-in-the-provider-admin-system"
  )
  organization = Organization.create!(name: "Development organization")
  OrganizationMembership.create!(application_user: user, organization: organization, role: "viewer")
end
```

The operator must obtain the issuer/subject association from a trusted enrollment process.
A claimed email address is not identity proof. The API never automatically registers users,
links accounts by matching email, accepts roles from a token or creates a membership while
reading. Two identities with the same email and different subjects stay distinct unless an
operator explicitly performs a reviewed link.

External identity `(issuer, subject)`, public user ids, public organization ids and each
user/organization membership pair have database uniqueness constraints. Memberships and
identities have foreign keys; membership roles have a database check constraint. Public ids
are random opaque application identifiers, immutable through the models.

Revoking an identity (`revoked_at`), disabling a user, revoking a membership or disabling an
organization takes effect on subsequent requests. An already-started read may finish.
Per-token `jti` revocation and customer self-service lifecycle APIs are not implemented.
Tokens expire within one hour; no reusable application token carries spending permission.

Only public ids, display names, organization names and stored roles are returned. Tokens,
issuer subjects, raw claims, payment credentials and gateway role keys are not returned or
persisted by these routes. Errors use fixed codes; token/claim-sensitive parameters are
filtered. The deployment/operator must still secure application data, logs and backups.

## Error contract

Errors are `{schema: "cosigna.identity/v1", error: <code>}` with `Cache-Control: no-store`.

| HTTP | Code | Meaning |
| --- | --- | --- |
| 401 | `invalid_token` | Invalid/expired/untrusted token, unknown or revoked identity, or inactive user. Returns a Bearer challenge. |
| 403 | `insufficient_scope` | A valid token lacks `cosigna:read`. Returns the required scope in the Bearer challenge. |
| 404 | `not_found` | Organization nonexistent, inactive, or outside the current user's active memberships. |
| 400 | `invalid_cursor` | Organization-list cursor is not a valid opaque organization id. |
| 503 | `identity_unconfigured` | Provider trust is not configured. |
| 503 | `identity_configuration_invalid` | Configuration/public keys cannot be safely used. |
| 503 | `identity_unavailable` | The application identity store is unavailable. |

Tokens are accepted only from the Authorization header, not cookies or query parameters.
Pagination uses fixed pages of 50 and an optional `after` public organization id. A cursor
selects a point within the authenticated user's query; it does not grant organization access.

## Gateway ownership and spending: next boundary, not implemented

The current Rust development owner credential authenticates as `local-user`. Rails cannot
turn its user/organization id into a Rust owner by copying an incoming field. It also cannot
derive ownership from successful access to the public gateway-health route.

Before exposing any customer transaction read or write, implement and test a binding with:

1. A stable, authenticated gateway-instance identity and the actual owner/principal id.
2. A trusted operator/provider-approved association from one application organization to
   that instance/principal, with uniqueness and explicit revocation/migration semantics.
3. A server-selected gateway destination and narrowly scoped credential reference; callers
   cannot supply a URL, profile path, owner id or upstream role key.
4. Rust-side authentication and authorization of each delegated request, with cross-owner
   denial. Rails membership is application context; the Rust gateway remains the financial
   decision and accounting authority.
5. Exact transaction approval/permit binding, audience, expiry, key lifecycle, revocation,
   durable single-use consumption and attempt correlation before any delegated spending.

No gateway ownership-binding table or financial delegation is added in this increment.
The Rails identity database stores no budgets, balances, approvals, transactions or copies
of gateway state. The Rust ledger, console sign-in and legacy AgentAuth service are unchanged.
Existing engine mandates, legacy consents and gateway budgets are not yet one authority.

## Verification and remaining release gates

Tests use generated in-memory RSA keys, a temporary public JWKS file and disposable Rails
test databases. They exercise signature/algorithm/token-type confusion, issuer/audience/client
binding, expiry and lifetime, key rotation, server-derived roles, cross-organization denial,
revocation, pagination, safe errors and database constraints. They can run with networking
disabled and do not contact an identity provider, model or payment rail.

Still required: real provider onboarding/sign-in validation, trusted enrollment administration,
customer lifecycle, per-token/session revocation policy, membership-change audit, public API
operating controls, authenticated gateway owner binding, signed transaction authority and
security review. G3 and Rails #78 stay open; this is a tested development boundary, not a
production identity certification or permission to use live funds.

Primary contracts: [RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html),
[ruby-jwt](https://github.com/jwt/ruby-jwt),
[Rails security guide](https://guides.rubyonrails.org/security.html).
