# Gateway identity: first Rust increment

27 October 2026. Implements the resource-server and identity-read portion of PR #97's
contract in `crates/cosigna-agent`, following the founder's one-backend decision on main
(`docs/DECISION_ONE_BACKEND_2026-10-27.md`). Rails is frozen. This increment does not implement
browser sign-in or make identities financial approvers.

## Verified boundary

The gateway verifies an operator-configured provider's JWT access token, then looks up an
active enrolled user by the exact `(issuer, subject)` pair. Users, external subjects,
organizations and memberships live in the same SQLite ledger as the gateway. A read uses a
single consistent transaction. Subsequent requests see committed revocations.

| Route | Authentication and behavior |
| --- | --- |
| `GET /identity/v1/me` | Valid access token plus active enrolled subject/user. Returns public user id and display name. |
| `GET /identity/v1/organizations` | Same boundary. Returns only active stored memberships/organizations, sorted by id, at most 50; optional `?after=org_<64 lowercase hex>` and `next_cursor`. |
| `GET /identity/v1/organizations/:id` | Active membership required; foreign, revoked and missing organizations all return `not_found`. |

Responses use `cosigna.identity/v1` and `Cache-Control: no-store`. Roles (`owner`, `admin`,
`member`, `viewer`) are stored application metadata. Neither membership nor any token email,
role, organization or owner claim grants access to budgets, payments, approvals, agents,
provider keys or the legacy `/v1/` API. The existing owner/agent/approver-key paths retain
their existing behavior. A JWT is refused on those routes.

This is an isolated identity-read boundary, not a completed multi-tenant payment service.
The installed owner credential is a trusted operator credential, never a customer login.

## Provider trust and rotation

All four operator environment settings are required together:

| Variable | Meaning |
| --- | --- |
| `COSIGNA_IDENTITY_ISSUER` | Exact HTTPS issuer, including trailing slash if present; no credentials, query or fragment. |
| `COSIGNA_IDENTITY_AUDIENCE` | This resource server's audience. |
| `COSIGNA_IDENTITY_CLIENT_IDS` | JSON array of 1–10 distinct permitted client ids. |
| `COSIGNA_IDENTITY_JWKS_PATH` | Absolute local path to public RSA JWKS; regular file, at most 64 KiB, 1–10 keys. |

The file is reread on every request. Publish overlapping old/new public keys, then atomically
replace the file to retire an old key. An unknown/retired key never causes remote discovery.
Incomplete, malformed or unreadable configuration fails closed. Default installations have
no configured provider; identity routes return 503 and normal gateway routes remain usable.
In Docker, explicitly mount the public JWKS read-only and set the variables in an operator
override; no private signing key, provider client secret or payment key belongs in this file.

The restricted access-token profile follows [RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html):

- RS256 only, `typ` equal to `at+jwt` or `application/at+jwt` (case-insensitive), and a known
  `kid`. Additional JOSE header fields are rejected, including token-selected key URLs/keys.
  Signature verification uses locked `jsonwebtoken` 11.1.0 with its RustCrypto backend.
- Public RSA modulus 2048–4096 bits, exponent 65537. Duplicate key ids, private parameters,
  incompatible algorithms, encryption use and incompatible key operations are refused.
- Exact issuer, configured audience (string or bounded string array containing it), allowed
  client id, nonempty bounded `sub`/`jti`, `cosigna:read` scope, integer `iat` and `exp`.
  `iat <= now < exp`, lifetime at most one hour, no clock-skew allowance. Optional `nbf` must
  be an integer at/before now and before expiry; explicit null is invalid.
- Authorization is a single `Bearer <compact JWT>` header, at most 16 KiB. Cookies and query
  parameters cannot authenticate. Tokens and raw claims are not persisted or returned.

No provider has yet been configured/tested against this profile. OIDC ID tokens, opaque
tokens, discovery/introspection, authorization-code/PKCE redirects, refresh tokens, session
issuance and browser login are not implemented.

## Trusted operator enrollment

These JSON routes require the existing installed **owner credential**. Agent, approver and
identity tokens cannot call them. Unknown request fields are refused.

| Route | JSON body |
| --- | --- |
| `POST /gateway/v1/identity/users` | `{ "display_name": "Development user", "issuer": "https://identity.example/", "subject": "provider-verified-subject" }` |
| `POST /gateway/v1/identity/organizations` | `{ "name": "Development company" }` |
| `POST /gateway/v1/identity/memberships` | `{ "user_id": "usr_...", "organization_id": "org_...", "role": "viewer" }` |
| `POST /gateway/v1/identity/{users,subjects,organizations,memberships}/:id/revoke` | `{}` |

User enrollment returns `{user: {id, display_name}, identity_id}`; organization and membership
creation return their public metadata. Owner namespace is derived from authenticated authority,
never accepted from the request. Membership targets must be active and owned by that same
operator; composite foreign keys enforce the owner relationship. External issuer/subject is
globally unique; each user/organization membership is unique. Enrollment and each evidence
record commit atomically. Repeated revocation is idempotent and permanent in this increment.

Obtain issuer/subject from a trusted provider administration/enrollment process. A claimed
email address is not proof of identity. Accounts are never automatically created or linked
by email, and existing users cannot add another subject through a public API. The operator
API is not yet a customer self-service administration experience. Secure the owner credential,
identity database, enrollment transport and backups as privileged data.

## Storage and evidence

Ledger v10 adds four empty tables to older valid ledgers. No historical person, approver,
organization or membership is inferred. Existing operations, approvals, digests, reservations,
attempt keys and provider stores are unchanged. Older gateway binaries refuse v10; stop all
processes using a profile and back it up before upgrading. Tests use disposable profiles only.

`gateway.identity.enrolled` and `gateway.identity.revoked` events record the operator actor,
resource type, opaque record id and a BLAKE3 digest of the exact stored row. They do not repeat
issuer/subject or token data. `evidence verify` compares all four identity tables to their
latest recorded digests and detects unrecorded inserts, changes and deletion. This provides
the existing local tamper-evident boundary, not protection against an attacker rewriting the
database and its whole evidence chain. Export/anchor evidence independently as appropriate.

## Errors

| HTTP | Fixed `error` string |
| --- | --- |
| 401 | `invalid_token` (includes unknown/revoked identity or inactive user); Bearer challenge. |
| 403 | `insufficient_scope`; challenge names `cosigna:read`. |
| 404 | `not_found`. |
| 400 | `invalid_cursor`. |
| 503 | `identity_unconfigured`, `identity_configuration_invalid`, or `identity_unavailable`. |

Existing HTTP Host, Origin, TLS-proxy, concurrency and timeout checks still apply. Their
transport errors retain their existing envelope. Operator routes use the normal gateway
administration error envelope. Errors never include claims, JWTs, keys or private paths.

## Next increment: identity-backed approvals

The remaining E18 work must explicitly bind an enrolled identity and active organization
membership to an existing named approver and gateway owner, with no automatic email matching
or role-to-owner escalation. Check revocation, scope, proposal digest, currency and per-person
limit inside the deciding transaction; prevent one identity counting twice under two-person
approval. Evidence must bind provider issuer/subject to that decision without retaining tokens.
Then integrate a provider-neutral browser sign-in flow into the console and test a real IdP.
Those approvals and sign-in workflows are **not** implemented by this increment. PR #97 remains
unmerged as the reference until its complete Rust replacement is reviewed.

Verification: synthetic in-memory RSA signing keys, temporary public-key files and ledgers,
strict token/profile rejection, scoped reads, revocation, pagination, role/owner-claim
non-authority, schema constraints/migration, row-tampering detection and the real HTTP binary.
No model, identity-provider or payment API is called by these tests.
