# Cosigna web product — current development priorities

Updated 21 October 2026 following the user's instruction to prioritize the landing page,
dashboard, payment integrations and Cosigna's own AI agents. This supersedes earlier UI
ordering; engine enforcement and existing API compatibility remain in place.

| Area | Current status | Next concrete outcome |
| --- | --- | --- |
| Go CLI | Under development; working local prototype | Retain source and checks; defer new CLI features |
| Landing page | Active | Explain the product, demonstrate the approval workflow, show provider readiness |
| Dashboard | Active | Existing approval/activity/evidence flows plus integration and agent entry points |
| Stripe | Test-mode experiment validated | Merchant connection, event ingestion and a bounded refund workflow |
| Other providers | Planned | Build adapters individually after validating API/account access |
| Cosigna agents | Agent configuration drafts | One executable refunds agent before expanding workflows |

## Product experience

1. Understand Cosigna on the landing page and explore an explicitly illustrative workflow.
2. Enter the dashboard through its existing local sign-in boundary.
3. Choose a provider and inspect its required access and implementation status.
4. Connect an account only after the real provider authorization flow exists.
5. Configure a Cosigna agent: purpose, tools, owner, limits and human escalation.
6. Test the agent against a sandbox account. Review the proposed action and recorded outcome.
7. Enable permitted actions only after account access, runtime and policy checks pass.

The initial managed-agent templates are refunds, collections preparation and reconciliation.
Configuration drafts do not register identities, run models, send messages or execute payments.
Refunds are the first proposed execution workflow because the Stripe comparison already exists.
The model proposes an action; the existing gateway evaluates authority before a provider write.

## Connector architecture

Provider support is implemented per adapter. A directory entry is not a working connection.
The shared design must support account authorization/revocation, capability discovery, payment
normalization, signed event ingestion, retry/deduplication, reconciliation and evidence links.
Amounts retain exact minor units and currency. Settlement state stays distinct from authorization.

```text
Merchant account → provider adapter → normalized events → dashboard / agent task
                                                           ↓
Provider outcome ← adapter ← idempotent execution ← gateway authority check
                                                           ↑
                                             mandate / human approval
```

Webhook delivery tells us about provider events. It does not intercept actions made directly
in the provider's dashboard or by an unrelated application. Prevention applies only to writes
that actually pass through Cosigna's execution path. Ambiguous provider outcomes must be
reconciled before another write is attempted.

Initial provider candidates: Stripe, PayPal, Razorpay, Adyen and Square; accounting/commerce
candidates: QuickBooks, Xero and Shopify. Ordering after Stripe is not committed. No current
claim of universal coverage, production connection, partner approval or OAuth completion.

## Verified provider references

- [Stripe Connect OAuth](https://docs.stripe.com/connect/oauth-reference): authorization model
  for eligible Connect account integrations; choose the correct Stripe integration type first.
- [Stripe event delivery](https://docs.stripe.com/webhooks): event endpoint and signature handling.
- [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests): avoid duplicate
  writes on retries; provider/version semantics must be respected.
- [PayPal seller onboarding](https://developer.paypal.com/platforms/seller-onboarding): applicable
  to its multiparty platform flow; partner eligibility must be established for our product.
- [Razorpay OAuth](https://razorpay.com/docs/partners/technology-partners/onboard-businesses/integrate-oauth/):
  technology-partner onboarding and authorized account access, with regional availability.
- [Adyen webhooks](https://docs.adyen.com/development-resources/webhooks): event notifications
  for payment and platform workflows; account/capability onboarding is separate.

These are implementation research, not confirmation that Cosigna has provider accounts or approvals.

## Open decisions

- Launch country, target customer, and the next provider after Stripe.
- Which merchant authorization model each provider permits for Cosigna's business model.
- Hosted application/authentication and the eventual production dashboard URL.
- Model/runtime for Cosigna-managed agents, credentials, budgets and background execution.
- Whether initial agents only prepare proposals or execute within explicitly delegated limits.
- Agent draft persistence and the backend contract before activating any draft.

No credentials or real account connections are requested by the current preview. The public
landing site is developed separately from the authenticated Console; no Console authentication
checks are removed to make the marketing site accessible. No external deployment is included.
