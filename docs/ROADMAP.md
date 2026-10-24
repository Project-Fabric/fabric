# Cosigna delivery roadmap

Current priority: ship a trustworthy transaction gateway. Cosigna Assistant remains a separate
application above it. Existing mobile, CLI and web prototypes are preserved; they are not
evidence that production payment or identity requirements are complete.

Product identity update: one **Cosigna** product, with [staged code consolidation](COSIGNA_UNIFICATION.md).
24 October: the landing page was restored to the earlier dark version from before the blue
redesign. The subsequent subpages, theme switch and Griffin-style refresh were removed from
the active preview. Product consolidation remains staged; commands, contracts and ledgers remain compatible.
Identity delivery and consolidation gates are tracked in [#77](https://github.com/Project-Fabric/fabric/issues/77).

24 October: Rails is selected for application/business workflows. The [initial backend](../backend/README.md)
adds local status and a bounded, read-only Rust health adapter. Customer authentication,
organization/tenant mapping and authorized transaction operations remain unimplemented G3
work. Existing Rust, Python, console and mobile paths are preserved.
Rails delivery and remaining acceptance gates: [#78](https://github.com/Project-Fabric/fabric/issues/78).

23 October decision: local-first build and verification; cloud account creation/deployment is
deferred. The [local pipeline plan](LOCAL_PIPELINE_PLAN.md) prioritizes authorization, crash
recovery, concurrency and merchant/payment disagreement, with optional Apache batch tooling.

The private [Cosigna road to YC board](https://github.com/orgs/agentauth-io/projects/1) is the
live work tracker. G-series issues below track the current gateway-first plan. Earlier M-series
items describe the legacy engine/control-plane roadmap; their completion does not imply the
new production gateway is finished. Update issue checklists/status with implementation and
verification evidence rather than inferring completion from a demo.

| Milestone | Current delivery | Acceptance / live tracking |
| --- | --- | --- |
| G1 Reproducible product baseline | Published; GitHub Rust CI passed on Windows, Linux and macOS | [#70](https://github.com/Project-Fabric/fabric/issues/70), baseline `b896759` |
| G2 Transaction-agent pipeline | First caller-driven pipeline implemented; separate background operations remain G4 | [#71](https://github.com/Project-Fabric/fabric/issues/71): typed state, approval boundary, replay, scope, restart and recovery tests |
| G3 Signed permits and production owner identity | Planned; existing agent bearers are development credentials | [#72](https://github.com/Project-Fabric/fabric/issues/72): transaction binding, expiry, audience, replay, verified owner proof and key lifecycle |
| G4 Durable workers and operating controls | Local worker/recovery implemented; cloud provisioning deferred while local core gates advance | [#73](https://github.com/Project-Fabric/fabric/issues/73): target-host smoke/reboot/restore tests, public API limits, alerting and operating hardening remain |
| G5 One merchant/payment connector pilot | Test merchant and Stripe sandbox exist; live access/eligibility unproven | [#74](https://github.com/Project-Fabric/fabric/issues/74): select merchant/country/method; authentication, failures, webhook and compensation evidence |
| G6 Adversarial regression and release evidence | Active; four fixed-boundary process-kill regressions and optional local Airflow verification DAG added | [#75](https://github.com/Project-Fabric/fabric/issues/75): generated invariants, additional crash/provider failures, held-out model attacks and independent review remain |
| G7 Design-partner adoption | Planned; test payments are not revenue or customer traction | [#76](https://github.com/Project-Fabric/fabric/issues/76): one narrow adopted workflow and measured operational outcomes |
| G8 Gateway decisions as audit evidence | Increment 1 done: ledger v4 hash-chained records, `evidence verify`/`export` | [#79](https://github.com/Project-Fabric/fabric/issues/79): signed packs, one verifier with the engine chain, anchoring |
| G9 Resolve stuck unknown payments | Owner `resolve` through provider void implemented; live Stripe test-mode run pending | [#80](https://github.com/Project-Fabric/fabric/issues/80): owner resolution backed by provider lookup, recorded as evidence |
| G10 One approval inbox | Planned; tool-call and purchase approvals live in different places | [#82](https://github.com/Project-Fabric/fabric/issues/82): console decides gateway quotes under one approval contract |
| G11 Phone approval security | Planned; LAN relay is plain HTTP, voice confirmation is not identity | [#83](https://github.com/Project-Fabric/fabric/issues/83): TLS, passkey-signed quote digest, narrower phone scope |
| G12 Quote window | Planned | [#84](https://github.com/Project-Fabric/fabric/issues/84): approval near quote expiry must not fail silently |
| G13 Gateway concurrency | Planned; one request at a time | [#85](https://github.com/Project-Fabric/fabric/issues/85): worker pool, read-only connects, per-task planning lock, measured latency |
| D1 First customer | Founder decision | [#81](https://github.com/Project-Fabric/fabric/issues/81): finance-ops wedge vs developer gateway vs consumer voice |
| P1 End-to-end money operations | E1 operations, E2 policy and E3 Stripe test-mode refund rail done (fake-Stripe tested); live test-mode run, MCP tools, inbox and Northwind 2.0 next | [#87](https://github.com/Project-Fabric/fabric/issues/87) |
| P2 Gateway security | Planned, after P1 | [#88](https://github.com/Project-Fabric/fabric/issues/88) |
| P3 Agent control layer (pre-approval evidence probes) | Planned, after P2 | [#89](https://github.com/Project-Fabric/fabric/issues/89) |
| H1 Housekeeping | Planned | [#86](https://github.com/Project-Fabric/fabric/issues/86): build warning, docs, CI minutes, README |

## Monitoring

- **Rust CI:** push/PR formatting, Clippy and workspace tests across Windows, macOS and Linux.
- **Product checks:** push/PR/manual and weekday 04:00 UTC runs for the Rails foundation, gateway/merchant regressions,
  credential-pattern checks, console/site, mobile contracts/Expo bundle and Go CLI.
- **Roadmap:** issue checklists and the board show delivery progress, blockers and remaining
  acceptance work. Passing CI does not automatically mark a feature production-ready.
- **Release readiness:** production identity, signed permits, external merchant access, supported
  payment authentication, operating controls and security review are still explicit gates.

Actions run on GitHub's configured runners; a configured schedule is not proof that a given
run succeeded. Check [the actual Actions runs](https://github.com/Project-Fabric/fabric/actions).
No scheduled job performs live payments, calls a paid model or autonomously develops features.
The repository does not yet enforce required PR checks or production deployment approval rules.

## Product boundaries

The planner interprets a request and proposes typed choices. Owner authority, policy, budget,
exact approval and execution claims are enforced by code. The gateway keeps payment/order/
fulfillment outcomes distinct and reconciles uncertainty. The assistant will later provide
voice ordering, merchant readiness and pickup coordination over that same boundary.

Do not add broad assistant integrations or train a model before a measured task-quality gap
and the relevant transaction/merchant prerequisites exist. No general marketplace access,
universal exactly-once execution, zero queue time or compliance certification is claimed.

Implementation guides: [gateway](COSIGNA_GATEWAY.md), [transaction pipeline](TRANSACTION_PIPELINES.md),
[product/component direction](COSIGNA_REBUILD.md), [storage/API contracts](STORAGE_FORMAT.md),
[private server operation](GATEWAY_OPERATIONS.md).
