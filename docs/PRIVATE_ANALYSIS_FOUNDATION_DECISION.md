# C01 foundation decision and verified baseline

30 October 2026. The founder authorized starting the planned work. This document records
the narrow foundation decisions used for implementation; the broader design PR remains in
review and does not claim that customer-specific deployment settings have been agreed.

- Refunds first; fixture/Stripe TEST only. The model has advisory authority only; no model
  is required for deterministic payment processing. No raw business records enter the
  proposed outcome-only classifier. Ambiguous free text goes to human review initially.
- The analyzer is a trusted plaintext endpoint inside the customer boundary. Transport
  and storage encryption do not establish computation on ciphertext or administrator-proof
  confidentiality. Egress validation and deployment restrictions must be tested separately.
- The proposed v1 feature/output JSON Schemas are under docs/contracts. They admit only
  fixed scalars/enums, reject unknown fields, distinguish unknown values and expose no
  identifiers, exact amounts, free text or execution instructions. They are review artifacts,
  not a runtime implementation. Customer-specific band thresholds remain policy work.
- Trusted workflow assignment and exact facts remain local. Model timeouts or extra fields
  cannot loosen deterministic decisions. Source authority and freshness are explicit gates.
- C02 recovery decision: after the grace period, an approved operation with no claimed
  attempt can atomically return to review, release its unused reservation and clear prior
  approvals. Execution and recovery use the same lock. Claimed/unknown attempts retain their
  reservations and use provider lookup only. Recovery never creates an execution attempt.
- Correct baseline: main b669dbe already has Tokio/Axum, admission/time limits, graceful
  lifecycle, SQLite v12, identity resources/bindings and alerting. The old demo checkout
  lacked these. Do not duplicate them or use the old benchmark as current-main capacity.

Open before external classifier/deployment: business-owned entitlement mappings and editor
permissions, chosen identity-provider configuration, source version/freshness contract,
outcome disclosure thresholds, model usefulness/access/retention agreement, actual encryption
endpoints/key owner and independent checkpoint storage. No provider or customer values are
invented. A managed vault/cloud dependency is optional, not mandatory in the core.

C01 remains open until the design PR and its contracts receive final review and merge.
C02 is a repair to existing recovery behavior, not activation of the optional model path.
