# Console contract questions

## Proposed voice-commerce direction — research decisions

Research: [Cosigna personal assistant](../docs/COSIGNA_VOICE_COMMERCE_RESEARCH.md).
This is a proposed extension, not a replacement for the current committed web work.

Scope clarified 22 October 2026: the vision includes shopping across vendors, flights,
internet research and identity-related checks. Coffee is an example, not a committed first
vertical. The research document now records the broader task architecture.

The `/tasks` preview now demonstrates shopping and flights using fictional offers and page
memory. INR and Bengaluru-to-Delhi are example content, not a chosen launch market. No live
task API or persistence contract has been inferred. Before replacing fixtures, define task
identity, quote expiry/versioning, authenticated approval binding, connector status reads,
durable reservations and recovery behavior. The UI simulation does not implement these guarantees.

- Which country launches first, and which shopping, travel or local-commerce provider can
  supply supported execution access? Is Amazon intended for consumer accounts or Business?
- Which identity claim should be checked: the user's identity, connected account ownership,
  merchant/business details, or another consented workflow? Public search is not identity proof.
- Should the first validated buyer be a consumer cohort or an enterprise design partner?
- Which phone platform and physical devices can we test? Is macOS/Xcode available for iOS?
  **Resolved for the first mobile version:** Android first, per the user. `mobile/` contains
  an Expo/React Native fixture preview. Physical Android model/version is still needed for
  device QA; iOS is deferred. Expo account/project ownership and signing must be chosen before
  a standalone APK build. `com.cosigna.preview` is a provisional internal identifier.
- Start with exact authenticated approval for every pilot purchase; what narrowly bounded
  delegated routine, if any, should follow after validation?
- Which provider-supported payment setup fits the market, and who owns fulfillment failures,
  refunds and customer support?
- What location/audio retention and model-processing choices should the product adopt?

## Existing console and web contracts

Current web priority: landing page, dashboard, provider connection plans and Cosigna agent
drafts. The CLI is under development. Product decisions needed before enabling real managed
agents or provider connections are tracked in [WEB_PRODUCT_PLAN.md](../docs/WEB_PRODUCT_PLAN.md):
launch market, merchant authorization model, runtime/model provider, durable agent storage,
and whether the first execution mode proposes actions or acts within delegated limits.
The current UI does not infer these answers or present drafts as running agents.

1. **Live approval amount units.** Requests include raw tool input but no unit/currency metadata.
   The synthetic `stripe/issue_refund` inbox uses USD cents (45000 = $450.00 in the brief),
   but it has no matching payment declaration. Fixtures use that explicit example; live mode
   requires a matching declaration from `fabric payments list --json`. When units cannot be
   determined, show the raw input and an unavailable amount; do not silently assume cents.
   Should the engine add normalized payment context to approval requests?
2. **Mandate currency.** Terms and uses have no currency. Uses are scaled by 10^6, per
   `mandates.rs`. Display them as units, not dollars. Should mandates gain a currency field?
3. **Evidence export destination.** No destination-selection contract is specified. Live export
   is disabled until a person configures `CONSOLE_EXPORT_DIR`; then use a unique child directory
   and show its absolute path. Should the console offer a native folder picker later?
4. **Consent mutations.** The brief specifies listing API consents but only grant/revoke actions
   for engine mandates. Consent creation/revocation is not added without an API action contract.
5. **Activity completeness.** `calls -n 200` and the consent fixture are bounded samples.
   Spending totals are labelled as recorded in the loaded activity, not a complete ledger.
   What pagination/range contract should provide complete daily/weekly totals?
6. **Blocked payment context.** The captured three blocked Stripe calls have `payment: null`.
   Their raw gateway refusal remains visible, but the console does not invent a Cosigna
   decision, normalized amount or authorization code from that prose. Should `evidence calls`
   join the related payment-authorization event for blocked calls as it does for allowed ones?

Fixture actions simulate changes in per-session memory so the inbox can update, without
writing fixtures or invoking the engine. They are always labelled as simulated. This resolves
the brief's no-op actions / updated-list flow without presenting a fixture decision as real.

## Verification status

Dependencies were installed from the person's terminal. Typecheck, lint, 17 component/command
tests, four session/CSRF/contrast tests, production build and the client-bundle secret scan
pass. The production HTTP tests exercise both approval outcomes, session/CSRF rejection,
stale-decision refusal, per-session fixture changes, one-time token handling, the other action
simulations and every page route. No engine command is run by these fixture tests.

Actual browser rendering at 375 px and screen-reader inspection remain pending: the browser
controller reports no available browsers. The CSS has a phone layout and tested AA text
colors, but those checks do not establish the rendered layout or the full accessibility
experience. A read-only live console API check loaded Northwind's 100 tool calls, including
five successful money-tool calls and all three blocked ones. Other live sections reported
their actual configuration/permission errors in this agent session (consents, approvals,
mandates and verification). Live financial decisions have not been run by this agent.
