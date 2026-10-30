# Cosigna private analysis and secure refund delivery tracker

Live parent: [#109](https://github.com/Project-Fabric/fabric/issues/109). Board: [Cosigna roadmap](https://github.com/orgs/Project-Fabric/projects/2).

30 October 2026. Proposed implementation plan; production work requires the C01 design decision.
The source of live status is the linked issues/project, not this dated document. Filter the board by `label:"track: private-analysis"`.

## Scope and verified baseline

Refunds first, Stripe TEST and synthetic/explicitly permitted records only. The running gateway uses Rust/tiny_http/SQLite; Tokio/Axum/PostgreSQL remain a proposed migration. Existing scoped grants, case checks, approvals, reservations, attempt recovery and evidence are a starting point, not a production certification.

The local 30 October public-derived challenge matched all 240 declared policy effects, while the same 24 wrong-intent probes affected both Cosigna and the case-aware reference. A mixed overload run left one approved USD 1 reservation unsettled after HTTP 500. Those local results motivate C02/C03; they are not merged-release guarantees or historical fraud savings. See local `benchmarks/assurance/2026-10-30` before claiming publication of the original evidence.

The protected analysis/egress stack is proposed. Named development credentials are not verified enterprise identity. The monitoring sandbox contains isolated fixture data and does not monitor a real customer or the Windows console ledger.

## Work packages

| ID | Workstream | Work package | Issue | Phase | Hours | Dependencies |
|---|---|---|---|---|---:|---|
| C01 | Architecture | Approve the private analysis boundary and versioned contracts | [#110](https://github.com/Project-Fabric/fabric/issues/110) | Design | 8–12 | None (design first) |
| C02 | Transaction reliability | Recover approved reservations interrupted before provider submission | [#111](https://github.com/Project-Fabric/fabric/issues/111) | Foundation | 8–16 | #110 |
| C03 | Authorization | Bind each agent request to a trusted workflow intent | [#112](https://github.com/Project-Fabric/fabric/issues/112) | Foundation | 12–20 | #110, #111 |
| C04 | Authorization | Unify transaction policy authority with legacy adapters | [#113](https://github.com/Project-Fabric/fabric/issues/113) | Foundation | 16–28 | #110, #112 |
| C05 | Identity | Production approver identity and tenant/workload boundaries | [#72](https://github.com/Project-Fabric/fabric/issues/72) | Foundation | 16–28 | #110 |
| C06 | Data connectors | Validate authoritative refund sources and defend connector fetches | [#114](https://github.com/Project-Fabric/fabric/issues/114) | Private data | 8–16 | #110, #112 |
| C07 | Data analysis | Implement pure Rust refund analyzers with provenance | [#115](https://github.com/Project-Fabric/fabric/issues/115) | Private data | 12–20 | #114 |
| C08 | Privacy | Project minimal typed outcomes and prove field non-disclosure | [#116](https://github.com/Project-Fabric/fabric/issues/116) | Private data | 8–16 | #110, #115 |
| C09 | Privacy | Bind analysis contexts to exact proposals and enforce freshness | [#117](https://github.com/Project-Fabric/fabric/issues/117) | Private data | 8–16 | #112, #116 |
| C10 | Security | Define encryption endpoints and customer-owned secret lifecycle | [#118](https://github.com/Project-Fabric/fabric/issues/118) | Security | 12–24 | #110, #72 |
| C11 | Security | Enforce a single model egress path and process isolation | [#119](https://github.com/Project-Fabric/fabric/issues/119) | Security | 12–20 | #116, #117, #118 |
| C12 | AI advisory | Add one optional structured classifier and versioned skill loading | [#120](https://github.com/Project-Fabric/fabric/issues/120) | Evaluation | 8–16 | #117, #119 |
| C13 | Evidence | Protect analysis evidence and anchor financial checkpoints | [#79](https://github.com/Project-Fabric/fabric/issues/79) | Security | 12–24 | #117, #118 |
| C14 | Supply chain | Verify releases, dependencies, parser hardening and rollback | [#68](https://github.com/Project-Fabric/fabric/issues/68) | Security | 12–20 | #110 |
| C15 | Verification | Evaluate disclosure, intent accuracy and failure recovery against baselines | [#75](https://github.com/Project-Fabric/fabric/issues/75) | Evaluation | 12–24 | #111, #112, #116, #119, #120 |
| C16 | Performance | Measure bounded admission, control responsiveness and settlement capacity | [#85](https://github.com/Project-Fabric/fabric/issues/85) | Evaluation | 12–24 | #111, #112 |
| C17 | Observability | Monitor actual transaction state, exposure and recovery age | [#121](https://github.com/Project-Fabric/fabric/issues/121) | Operations | 8–16 | #111, #72, #117 |
| C18 | Console | Explain held decisions, source freshness and safe operator actions | [#122](https://github.com/Project-Fabric/fabric/issues/122) | Operations | 8–16 | #72, #117, #79, #121 |
| C19 | Deployment | Deliver a reproducible sandbox with limits, restore and lifecycle checks | [#73](https://github.com/Project-Fabric/fabric/issues/73) | Operations | 12–20 | #111, #72, #118, #119, #68, #121 |
| C20 | Customer validation | Run a narrow external refund sandbox pilot with measured value | [#76](https://github.com/Project-Fabric/fabric/issues/76) | Pilot | 12–24 | #113, #72, #79, #68, #75, #85, #122, #73 |

## Delivery gates and order

1. Design: C01. Record approval of authority, trusted endpoints, schemas, model role and source fields. No production feature implementation before this decision.
2. Foundation: C02/C03 first; C04/C05 next. Reproduce/fix interrupted approved reservations, bind exact intent, and establish current identity/policy authority.
3. Private data: C06 → C07 → C08 → C09. Ship a local deterministic projection using synthetic inputs before allowing hosted inference.
4. Security: C10/C11/C13/C14. Verify actual egress, crypto endpoints, credential boundaries and release integrity. A packet/log scanner alone is insufficient.
5. Evaluation: C12 in shadow, C15/C16. Require independent labels, positive controls, disclosure tests and useful improvement over rules-only. Reject-all and model agreement are not success criteria.
6. Operations/pilot: C17 → C18, C19 and C20 after their explicit dependencies. Prove supported-host restore, review workflows and actual external value. Live money stays outside this plan.

Rough total: 216–400 focused hours, approximately 18–40 weeks at 10–12 hours/week. This includes remaining work on reused gates; it is not all new implementation. Estimates exclude external review, waiting for identities/provider access, production migration, customer scheduling and unexpected defects. Do not add work to the current cycle without reassessing scope.

First 10–12-hour cycle: complete the C01 decision package and use remaining time to reproduce C02. Next cycle: C02 regression/fix before opening the next implementation. Keep one production implementation item in progress at a time.

## Tracking rules

- Owner: solo founder/engineering; new issues are assigned to the authenticated founder account. No outside person has been assigned work.
- New issue status starts Todo. Existing issues retain their current status/checklists. C01 design review is the first active work package.
- Proposed transitions: Todo → In Progress → verification evidence → Done. Dependencies/open decisions remain visible even when the board has only three status columns.
- Each issue has scope, acceptance checkboxes, dependencies, hour range and evidence required. Close only after a merged PR, exact checks/results and limitations are linked.
- Update the issue and board after each implementation; monthly documents are not autonomous progress monitoring. No scheduled AI/payment run is introduced.
- Failed, incomplete and skipped checks are distinct. Local uncommitted files, a design document, a demo, or a successful CI job alone do not complete a feature.
- Any chosen model must justify extra latency, cost and review burden versus deterministic rules. Keys, prompts, raw customer data and private runtime folders never enter GitHub issues or artifacts.

## Completion evidence required across all fields

### C01 — Approve the private analysis boundary and versioned contracts

Tracking: #110. Dependencies: None (design first).

- [ ] Review threat actors, trusted plaintext endpoints, raw-data scope and refund-only exclusions; record founder decision before production implementation.
- [ ] Agree feature/output schemas with unknown states, provenance, freshness, minimal vocabulary and no arbitrary fields.
- [ ] Document which decisions require a model and which remain rules-only; preserve a usable no-model mode.

### C02 — Recover approved reservations interrupted before provider submission

Tracking: #111. Dependencies: #110.

- [ ] Reproduce the observed approved/unsettled reservation after overload using synthetic data; retain failing evidence.
- [ ] Specify safe approved/submitting/unknown recovery and transactional work creation; no new attempt after an uncertain submission.
- [ ] Pass process-kill and saturation regressions at each boundary with no duplicate effect, unexplained reservation or premature release.

### C03 — Bind each agent request to a trusted workflow intent

Tracking: #112. Dependencies: #110, #111.

- [ ] A trusted issuer binds tenant/principal, workflow, case, payment, operation, amount constraints and expiry; agent cannot broaden assignment.
- [ ] Reject wrong-but-valid case/payment and excessive intended amount even when a broader grant would permit them.
- [ ] Negative tests cover assignment swaps, replay, expiry, source changes and another agent/tenant; legitimate assigned refunds still complete.

### C04 — Unify transaction policy authority with legacy adapters

Tracking: #113. Dependencies: #110, #112.

- [ ] Define one versioned transaction authority model with grants, denies, caps, currencies, case/intent requirements and approval thresholds.
- [ ] Inventory engine mandates, AgentAuth consents and gateway budgets; explicit adapters fail closed on unmapped/conflicting authority; never union permissions.
- [ ] Migrate in shadow/compare mode without resetting balances, holds, attempts, evidence or revocation; exercise rollback and compatibility.

### C05 — Production approver identity and tenant/workload boundaries

Tracking: #72. Dependencies: #110.

- [ ] Integrate one chosen identity provider after provider-neutral issuer/audience/expiry verification and server-derived tenant mapping.
- [ ] Bind approver identity and membership to exact-proposal approvals; enforce two-person separation and current revocation.
- [ ] Test key rotation, invalid tokens, forged tenant/membership, expired sessions and cross-tenant reads/approvals.

### C06 — Validate authoritative refund sources and defend connector fetches

Tracking: #114. Dependencies: #110, #112.

- [ ] Map business-controlled entitlement/payment fields and document who can edit them; agent-authored content cannot establish authority.
- [ ] Bound source bodies/timeouts/parsing; enforce administrator-approved destinations, redirects and DNS/IP validation with deployment egress rules.
- [ ] Test source outage, stale/missing data, wrong customer/currency, schema drift, SSRF/metadata URLs and redirect leakage.

### C07 — Implement pure Rust refund analyzers with provenance

Tracking: #115. Dependencies: #114.

- [ ] Compute exact entitlement, cumulative commitments, duplicates, window eligibility and freshness without a model.
- [ ] Unknown remains explicit; malformed/ambiguous input never becomes invented entitlement.
- [ ] Boundary/property tests cover rounding/overflow, split amounts, concurrent commitments and false-positive rejection of legitimate cases.

### C08 — Project minimal typed outcomes and prove field non-disclosure

Tracking: #116. Dependencies: #110, #115.

- [ ] Construct a new strict outbound struct from an approved feature catalog; never serialize-and-redact a raw source object.
- [ ] Reject extra fields, URLs, identifiers and unapproved values; synthetic canaries stay absent in request/log/error/trace captures.
- [ ] Test raw-only field invariance, repeated-query inference risks, schema versions and sensitive feature combinations.

### C09 — Bind analysis contexts to exact proposals and enforce freshness

Tracking: #117. Dependencies: #112, #116.

- [ ] Store local request/tenant binding, exact facts, source revision, policy/analyzer version, nonce and expiry; send no unnecessary identifiers to a model.
- [ ] Stale or miscorrelated output cannot apply to another operation; approval/execution rechecks current authority and accounting.
- [ ] Exercise out-of-order replies, replay, concurrent approvals, changed facts/policies and context expiry.

### C10 — Define encryption endpoints and customer-owned secret lifecycle

Tracking: #118. Dependencies: #110, #72.

- [ ] Document every TLS/plaintext endpoint, threat actor, key holder, backup and crash-dump path; no computation-on-ciphertext or root-compromise claim.
- [ ] Use reviewed TLS/mTLS at real service boundaries and a secret-provider interface with one customer deployment adapter; no new mandatory cloud dependency.
- [ ] Verify least-privilege secrets, rotation/revocation, certificate expiry, backup handling and absence of keys in agents/logs.

### C11 — Enforce a single model egress path and process isolation

Tracking: #119. Dependencies: #116, #117, #118.

- [ ] Only the approved client can send the validated feature contract and reviewed instructions; model has no browser/shell/upload/payment tools.
- [ ] Enforce deployment network allowlists, bounded calls and no redirects; separate analyzer source credentials from model/provider access where a process boundary exists.
- [ ] Attack tests prove unapproved destinations, encoded raw values, parser tricks and canary exfiltration fail; document administrator bypass limits.

### C12 — Add one optional structured classifier and versioned skill loading

Tracking: #120. Dependencies: #117, #119.

- [ ] Strict schemas allow defer_to_policy or require_review only; hard denies/holds cannot be loosened; amounts/destinations never come from output.
- [ ] Pin/review skill and adapter versions; log available exact model metadata; required-model timeout/malformed/stale result holds safely.
- [ ] Start with mock transport and shadow mode; paid calls require usable access/contract and an explicit cost-bounded run; no model mandatory in core.

### C13 — Protect analysis evidence and anchor financial checkpoints

Tracking: #79. Dependencies: #117, #118.

- [ ] Record allowed payload/digests, context/policy/analyzer/skill/model versions and decisions without raw records or secrets.
- [ ] Add reviewed signing and independently stored checkpoints with key rotation; distinguish consistency from protection against full-store replacement.
- [ ] Independent verifier detects altered records/checkpoints and rollback; retention/export permissions and canary scans pass.

### C14 — Verify releases, dependencies, parser hardening and rollback

Tracking: #68. Dependencies: #110.

- [ ] Lock dependencies, review security advisories, produce SBOM/provenance and verify release/skill artifacts before promotion.
- [ ] Fuzz bounded parsers; forbid input-driven eval/shell/plugins; non-root/read-only deployment with minimal mounts and no Docker socket.
- [ ] Tampered artifact and malicious-input tests fail safely; exercise restoration of the last approved artifact; scope independent review before live money.

### C15 — Evaluate disclosure, intent accuracy and failure recovery against baselines

Tracking: #75. Dependencies: #111, #112, #116, #119, #120.

- [ ] Add independent held-out labels, positive controls and raw-context-versus-outcomes comparison using synthetic or permissioned records only.
- [ ] Compare rules-only, outcomes plus classifier and competent rules wrapper; execute Stripe restricted-key comparisons only with documented actual configurations/access.
- [ ] Test races, retries, lost replies, poisoned data, stale approvals, tenant leakage, canaries and malformed outputs; preserve all failures and denominators.
- [ ] Promote only after measured usefulness/disclosure benefit, shadow evidence, limited canary and tested immediate disable; no zero-harm or uncopyability claim.

### C16 — Measure bounded admission, control responsiveness and settlement capacity

Tracking: #85. Dependencies: #111, #112.

- [ ] Profile queue/lock/case/provider/commit latency before changing runtime; async Rust/PostgreSQL migration remains a separate reviewed design gate.
- [ ] Report offered/submitted/completed requests and confirmed effects separately, warm/populated ledgers and independent-host load; preserve FULL durability.
- [ ] Explicit correctness gate: no overspend, duplicate effect or unexplained holds; protect revocation/status/recovery under overload. 50ms gate and 1k/10k RPS remain targets, not delivered claims.

### C17 — Monitor actual transaction state, exposure and recovery age

Tracking: #121. Dependencies: #111, #72, #117.

- [ ] Keep live gateway traffic separate from historical benchmarks and isolated synthetic monitoring; label target environment and data freshness.
- [ ] Add unique financial outcomes, reserved/unknown exposure by currency, oldest unresolved age, queue depth/age and source/provider failures without sensitive labels.
- [ ] Test actionable alerts, bounded admission/circuit breakers and recovery visibility; readiness must not be inferred from HTTP 200 alone.

### C18 — Explain held decisions, source freshness and safe operator actions

Tracking: #122. Dependencies: #72, #117, #79, #121.

- [ ] Show source provenance, typed analysis, intended proposal, deterministic reason and separate model recommendation with explicit unknown states.
- [ ] Only authorized identities approve exact digests; two-person flow and cross-tenant restrictions enforced server-side.
- [ ] Operator can trace/resolve uncertain outcomes through supported provider checks without fresh sends or manual ledger edits; browser task tests pass.

### C19 — Deliver a reproducible sandbox with limits, restore and lifecycle checks

Tracking: #73. Dependencies: #111, #72, #118, #119, #68, #121.

- [ ] Install/restart on a clean supported host, enforce network and secret boundaries, timeouts/body/quota limits and graceful lifecycle.
- [ ] Exercise backup/restore and abrupt loss with in-flight work; credentials excluded from published artifacts and no uncertainty released prematurely.
- [ ] Document SQLite/local mode versus future PostgreSQL service mode and per-environment metric targets; no unimplemented production certification.

### C20 — Run a narrow external refund sandbox pilot with measured value

Tracking: #76. Dependencies: #113, #72, #79, #68, #75, #85, #122, #73.

- [ ] Obtain a willing team and explicit dataset permission; no customers or willingness-to-pay claimed before actual adoption.
- [ ] Compare setup/maintenance, correct completion, unnecessary reviews and recovery effort against their current workflow.
- [ ] Define go/no-go criteria and record willingness-to-pay; Stripe TEST only, live money requires a separate reviewed release gate.

## Open decisions before implementation

Business-owned entitlement field and editor permissions; trusted workflow issuer; supported identity provider/deployment; treatment of ambiguous free text; acceptable outcome disclosure; model usefulness and funded access; independent checkpoint owner; future PostgreSQL migration trigger. Defaults: no arbitrary code execution, no raw text to the outcome-only model, no mandatory model/cloud dependency, no live money.

Related broader issues #87, #88 and #89 remain in place. Voice, mobile, purchases and payouts are deferred, not silently marked complete. Rails is not added to the transaction path; retiring earlier application scaffolding is a separate compatibility decision.
