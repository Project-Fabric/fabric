# One product: Cosigna

Decision confirmed with Harsha on 23 October 2026: **one product now, staged code merge**.
Delivery and remaining acceptance work: [#77](https://github.com/Project-Fabric/fabric/issues/77).
Fabric and AgentAuth are legacy implementation names within Cosigna, not separately marketed
products. The landing page, console, gateway and assistant use Cosigna. This decision does
not merge financial ledgers, change service URLs or make separate authorization paths one.

## Current code boundaries

| Cosigna component | Existing source | Current boundary |
| --- | --- | --- |
| Recording/recovery engine | `crates/fabric-store`, `fabric-snapshot`, `fabric-recovery` | Offline objects, manifests and recovery journal; not a payment ledger |
| CLI and tool gateway | `crates/fabric-cli`; `cosigna` and compatible `fabric` command | Existing local mandates/evidence; payment tools call the existing authorization API |
| Authorization service | Separate `agentauth-io/agentauth` repository; local sibling `agentauth` | Python API/consent service with its own schemas and migrations |
| Transaction engine | `crates/cosigna-agent` | Version 3 budget/approval/reservation/attempt ledger; not yet integrated with that mandate service |
| Console | `console` | Existing fixture/engine interfaces, one-time sign-in, session and CSRF controls |
| Assistant | `mobile` and `testbeds/voice-merchant` | Local voice/order prototype using the sandbox transaction path |

The private `Project-Fabric/fabric` repository and public `agentauth-io/agentauth` repository
retain their current locations and visibility. No repository transfer, public publication,
domain change, key rotation, package rename or database migration is part of the landing refresh.

## Stage 1 — one identity and honest product surface

24 October status: Harsha requested restoration of the earlier dark landing page, before the
blue redesign. The later subpages, theme switch and editorial refresh are retained in Git
history only. Their design/implementation notes below describe that earlier attempt, not
the active preview. The one-product decision and staged backend consolidation still apply.

- Public-facing product name: Cosigna. Gateway, Console and Assistant are components/interfaces.
- One landing page explains the authority boundary and links About, Architecture and Benchmarks.
- Separate implemented sandbox capabilities, illustrative UI and planned integrations.
- Preserve existing runtime names/addresses so tests, scripts and the Northwind harness work.
- Capture the code-consolidation boundary here and in G3/G6 tracking.

## Stage 2 — one authoritative permission contract (G3)

Define the owner/agent identity, mandate and exact-transaction approval decision shared by the
existing tool gateway and transaction engine. Specify issuer/audience, canonical transaction
binding, budget/merchant/currency semantics, mandate revision, expiry/revocation, signed
single-use permit consumption and stable attempt correlation. Define fail-closed behavior
when the authorization service is unavailable and distinguish offline development fixtures
from production authorization. Add contract tests before switching either execution path.

The authorization service supplies authority; transaction reservations/attempts supply atomic
accounting and execution state. Avoid two independently authoritative production rule stores.
Current agent bearer keys and unsigned quote digests do not implement this permit contract.

## Stage 3 — consolidate reviewed source

After the contract and dependency/ownership review, bring the active authorization service,
SDK, migrations and tests into a dedicated service directory in the Cosigna workspace, retaining
history and licenses. Review packaging and CI together. Do not bulk-copy the entire sibling
repository: exclude credentials, environment files, runtime DBs, archives, build outputs,
screenshots and unrelated experiments. The source/destination visibility must be explicit.

Add Cosigna configuration/package entrypoints with compatibility tests, then deprecate legacy
names deliberately. Do not silently change existing `FABRIC_*`/`AGENTAUTH_*` settings, API
routes, idempotency scopes, CLI behavior or provider bindings during a visual rename.

## Stage 4 — migrate and prove the unified execution path

Test old/new clients against the shared contract, including owner/scope escalation, permit
tampering/replay, shared budgets, revocation and crash recovery. Review and version any data
migration, with complete backups and a reconciliation-aware rollback plan. Object manifests,
authorization records and the transaction ledger may remain separate stores with different
responsibilities; one product does not require combining unlike records in one database.

Only retire old service entrypoints after contract/migration acceptance. A name change does
not establish production identity, live payment eligibility or independent security review.

## Landing-page references

Mobbin MCP was connected and attempted, but both section searches returned a paid-plan
requirement. Harsha explicitly selected public references and original visuals instead.
Public structural references: [Ramp](https://ramp.com/), [Linear](https://linear.app/),
[Stripe](https://stripe.com/). No Mobbin screen, third-party artwork, testimonial, logo strip
or customer statistic was copied. The blue gateway SVG and all Cosigna UI examples are original.

Browser connectivity was unavailable during implementation; functional DOM, route and motion
checks are recorded separately from any future browser visual review.
