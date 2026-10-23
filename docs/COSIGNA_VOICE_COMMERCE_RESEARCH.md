# Cosigna personal assistant: understanding, evidence and build plan

> **Latest voice/model follow-up:** [COSIGNA_VOICE_AGENT_STACK.md](COSIGNA_VOICE_AGENT_STACK.md) records the iOS and Android scope, Voxtral/Grok weight availability, GPT/open-weight planner comparison, inspected development hardware and a supervised real-purchase test sequence. It updates platform/model recommendations without claiming live execution is built.

Researched 21–22 October 2026. This is a proposal for the user's new consumer/enterprise idea,
not an implemented feature, committed launch market, or replacement for the existing roadmap.
The landing page and pixel concept are unchanged by this research.

## Scope clarification: 22 October 2026

The user clarified that Cosigna's intended product is a general personal assistant that
researches, plans and carries out authorized tasks across services. Shopping on Amazon and
other vendors, booking flights within an approved budget, retrieving internet information
and identity-related checks are explicitly part of the vision. Voice is an interface;
coffee is one example, not the product boundary or a confirmed first launch vertical.

Product definition: **a personal AI assistant that turns requests into completed tasks,
within permissions and budgets the user controls.** "Jarvis" communicates the ambition;
universal capability and superiority are not established product guarantees.

The earlier coffee flow and pilot stages below remain a worked example. They do not commit
the product to coffee first. Select the first executable vertical based on reachable
provider access and customers, while designing the shared task system for multiple domains.

### Capabilities and integration evidence

| Capability | Proposed behavior | Evidence and boundary |
| --- | --- | --- |
| Shopping | Compare products, variants, seller, delivered cost and arrival; buy under authority; track delivery and handle returns | [Amazon Business Ordering API](https://docs.business.amazon.com/docs/ordering-api) supports business purchasing, authorized account access and order status. This establishes a potential business integration, not access to arbitrary consumer Amazon accounts. Consumer checkout remains unverified. Other vendors need individual supported integrations. |
| Travel | Search, compare and book flights; track booking status and support changes/cancellations | [Duffel booking guide](https://duffel.com/docs/guides/getting-started-with-flights) documents search, offer selection and order creation. [Offers](https://duffel.com/docs/api/offers) expire and prices can change; refresh before execution. Production access, market coverage, funding and servicing arrangements still need validation. |
| Internet research | Retrieve current sources, compare claims and return linked evidence with dates and uncertainty | Proposed read-only research capability. Public search results are not authoritative inventory, booking confirmation or identity proof. Separate discovered claims from provider-confirmed facts. |
| Identity | Authenticate the user; verify specific identity claims when necessary and consented; assess merchant/account evidence | [Stripe Identity checks](https://docs.stripe.com/identity/verification-checks) document document, selfie and other checks. A provider result is evidence about a specific check, not proof that a person or merchant is trustworthy. Provider availability and supported use cases need validation. |
| Personal context | Use user-approved preferences, addresses, calendar, location and connected accounts | Proposed scoped connections and editable memory. Each new source needs explicit access scope and retention choices. No unrestricted account access is implied. |

Amazon Business also documents order safeguards and possible seller substitution. An adapter
must align those settings with the user's price and seller constraints and interpret partial
acceptance. A successful API response is not evidence that every requested item was purchased.
Do not treat Amazon's seller-facing SP-API as a consumer checkout API.

### Shared task architecture

```text
Voice / text / supported assistant entry
                    |
       Task planner + user-approved context
                    |
    Research tools + domain-specific proposals
                    |
    Independent permission and budget evaluation
                    |
      Durable workflow + scoped connectors
                    |
    Provider evidence + reconciliation + user status
```

Use existing model capabilities behind an adapter, rather than training a general model
as a prerequisite. Begin with one planner and typed domain tools; additional agents are an
implementation option, not required for a personal assistant. Keep hosted orchestration
separate from the offline Rust core. This proposal does not enable multi-model orchestration
in the core or change its existing scope.

Generalize the shared task record: goal, owner, constraints, permitted data access, proposed
steps, authority references, budget reservations, provider IDs and observed outcomes. These
are design concepts, not a new schema. Shopping and travel retain their own validated data
and state machines; a flight is not a coffee order with different labels.

Every connector should declare whether it can research, quote, execute, read status, cancel
or refund, plus required account scopes and recovery behavior. UI must distinguish research
only, user checkout required, and supported execution. Prefer supported APIs. Evaluate
browser automation only where supported and permitted, with isolated credentials and the
same authority checks. Login challenges or blocked automation require user handoff, not a
promise that every website can be operated automatically.

Retrieved web pages, product descriptions and tool outputs are untrusted data: they cannot
grant permissions, change budgets, select new credential recipients or override the user's
instructions. Keep payment credentials and identity documents outside model memory; tools
receive only the minimum data needed for their purpose. Memory must be inspectable,
correctable and deletable, and inferred preferences never become spending authority.

### Budget is one condition of authority

A flight under the spending limit can still be the wrong purchase. Bind authority to dates,
airports, traveler, cabin, baggage, layover limits, arrival deadline, fare conditions,
currency and all-in cost. A product purchase similarly needs variant, quantity, seller,
delivery address and substitution rules. Shared budgets must reserve pending commitments
across connectors; do not silently combine currencies or assume refunds restore authority.

Offer three explicit modes per task: research only; prepare and request exact approval;
execute within an authenticated, narrowly defined prior grant. Start real purchases with
exact approval. Introduce delegated execution only after each workflow's constraints and
failure handling are demonstrated. Changed terms and unsupported actions require handoff.

Identity needs a separate decision: user authentication, consented identity proofing, account
ownership, and merchant/business checks are different operations. Searching public profiles
does not verify a person's legal identity. Do not build a universal identity score or promise
that internet information identifies anyone. Record the claim checked, source, time and
limitations; use passkeys/device authentication for account access and transaction approval.

### Revised implementation recommendation

1. Build the shared task workspace in fixtures: request, evidence, plan, approval, status and
   permission controls. Demonstrate shopping and travel with explicit simulated labels.
2. Select one market and accessible provider; complete one real sandbox workflow end to end,
   including changed quotes, duplicate submissions, unknown outcomes and cancellation.
3. Add a second domain to prove reuse of authority and workflow infrastructure without forcing
   identical domain models. Then add voice and supported platform entry points.
4. Expand integrations according to measured completion and recovery quality. Enterprise
   packaging can expose the shared authority/execution service with tenant isolation.

"Better" should be measured as correctly completed tasks, fewer unwanted purchases, reliable
recovery, lower user effort and transparent control. Broad coverage alone is insufficient.

## 1. What the user wants

A person asks an assistant to accomplish an everyday purchase, starting with coffee on the
way to work. Cosigna understands the request, uses permissioned location and a destination,
finds an eligible merchant along the route, reads its actual menu, discusses choices if
needed, and completes the order within authority the person has granted. The result should
include a confirmed order and useful pickup status, not merely a generated recommendation.

Siri and Android assistants are desired entry points. Cosigna also needs its own voice
experience so platform integration availability does not determine whether the product works.
The long-term enterprise offering embeds the same permission and execution infrastructure in
wallets, financial apps and agent products. Consumer ordering and internal corporate finance
have different workflows, identities and obligations; shared policy primitives do not make
them interchangeable products.

Recommended product definition: **an assistant that completes purchases within authority you
control**, backed by an authorization and execution service that others can integrate.

## 2. What research establishes

| Area | Verified evidence | Consequence for Cosigna |
| --- | --- | --- |
| Siri | App Intents exposes application actions; intents have authentication policies and confirmation APIs. [Apple App Intents](https://developer.apple.com/documentation/appintents), [authentication](https://developer.apple.com/documentation/appintents/appintent/authenticationpolicy), [confirmation](https://developer.apple.com/documentation/appintents/appintent/requestconfirmation()) | Prototype a narrow request and status flow on an iPhone. Do not assume arbitrary continuous conversation or access to Siri's raw voice recordings. |
| Android | AppFunctions is experimental, Android 16+, and its overview describes Gemini integration as a private preview with trusted testers. [Android AppFunctions](https://developer.android.com/ai/appfunctions) | Build native in-app voice and app links first; treat Gemini discovery/execution as an integration gate. Older App Actions documentation is not proof of equivalent Gemini support. |
| Voice runtime | Gemini Live supports function calls; direct client access can use short-lived tokens. Tool availability and preview behavior need version-specific testing. [Live tools](https://ai.google.dev/gemini-api/docs/live-api/tools), [ephemeral tokens](https://ai.google.dev/gemini-api/docs/live-api/ephemeral-tokens) | Evaluate a voice provider behind an adapter. A speech/model session never receives payment credentials or bypasses server authorization. No model choice is committed here. |
| Route discovery | Places Text Search can use a route polyline produced by Routes. [Search along route](https://developers.google.com/maps/documentation/places/web-service/search-along-route) | Search along the journey, then filter against integrated merchants. Mapping data does not establish orderability, current menu stock or prep time. |
| Merchant operations | Square provides catalog-linked orders, pricing and pickup fulfillment; paid fulfillment orders can appear in merchant tools. [Orders](https://developer.squareup.com/docs/orders-api/what-it-does), [fulfillment](https://developer.squareup.com/docs/orders-api/fulfillments) | A plausible US pilot candidate if participating cafés use Square. Prove that orders reach the merchant's real workflow. API creation alone is not physical acceptance or coffee readiness. |
| Stored payment method | Square documents seller-scoped cards and shared cards for participating sellers, with specific customer mappings and OAuth permissions. [Cards API](https://developer.squareup.com/docs/cards-api/overview) | Start with one merchant. Validate the supported shared-card arrangement before promising one saved card across cafés. |
| Scoped payment credentials | Stripe SPTs support agent purchases scoped to a seller and transaction constraints. The current guide lists US, Canada and selected European markets. [Agent SPT documentation](https://docs.stripe.com/agentic-commerce/concepts/shared-payment-tokens.md?agent-seller=agent) | A candidate for a different merchant/payment integration, not a universal payment credential and not evidence of availability in India. It does not supply café fulfillment. |
| Authentication | NIST SP 800-63B-4 excludes voice biometric comparison in its authentication requirements. [NIST](https://pages.nist.gov/800-63-4/sp800-63b.html#biometric) | Use passkeys/device-backed authentication and explicit transaction binding. Do not build voiceprint authorization in the MVP. NIST is a security benchmark here, not a claim that one rule legally governs every customer. |
| Standards | AP2 describes signed checkout/payment mandates, including open constraints and closed transaction authorizations. [AP2](https://ap2-protocol.org/) | Evaluate compatibility later; protocol concepts do not establish acceptance by a given merchant, provider or bank. Do not label existing local mandates AP2-compatible. |
| Competition | Gemini documents food ordering and other multi-step tasks in selected apps/devices, with takeover/confirmation steps. [Google task automation](https://support.google.com/gemini/answer/16940971) | Voice ordering alone is not defensible differentiation. Test whether reliable fulfillment, control and integration create measurable value. |

These are public capabilities, not confirmation that Cosigna has production access, partner
approval, merchant contracts, distribution or a supported processor business model.

## 3. The initial product experience

Onboarding establishes an account, device/passkey, payment method through the provider's
supported SDK, saved preferences and optional destination. Ask for location when needed for
an active request. Do not begin with continuous background tracking or voice recording.

Example request: "Get my usual coffee on the way to the office, under eight dollars."

1. Resolve "usual" and "office" from preferences explicitly supplied by the user. Ask if
   uncertain. Never invent an ingredient, destination or menu option.
2. Find integrated, open cafés on the route. Rank eligible options by detour, estimated
   waiting, preferences and total price. Merchant prep estimates need their own data source.
3. Read the actual menu and quote a basket with item/modifier IDs, quantity, currency, taxes,
   fees, explicitly selected tip, merchant location, pickup estimate and quote expiry.
4. Speak a concise option, not the entire menu unless asked. Finish additions before payment.
5. In the first pilot, show the exact basket and authenticate approval. Later, allow execution
   under a previously established mandate only when every condition and required provider
   authentication is satisfied.
6. Reserve the permission budget, execute through the adapter, and track provider and merchant
   states independently. Announce acceptance or readiness only when supported by evidence.

A reusable allowance might limit one drink, USD 8 including fees, one order per weekday,
USD 35 per week, specified merchants, no substitutions, and a defined expiry/time zone.
This is permission to request purchases, not a stored-value wallet or payment authorization.
An authenticated request and all issuer/provider checks remain necessary. A nearby speaker
or an unlocked phone is not sufficient evidence of transaction-specific approval.

## 4. Proposed architecture

```text
Siri/App Intents    Android entry point    Cosigna voice UI
         \                 |                    /
              Authenticated request/session
                          |
             Voice + structured intent planner
                          |
         Route search + merchant catalog + fresh quote
                          |
          Purchase proposal (no authority to execute)
                          |
        Cosigna deterministic authorization service
            /              |                    \
   Device approval    Mandate/policy       Budget reservation
                          |
             Durable purchase workflow + adapter
                     /             \
             Merchant order      Provider payment
                     \             /
              Webhooks + reconciliation worker
                          |
              Status, receipt and decision evidence
```

Recommended implementation choices, subject to the first integration experiment:

- **Mobile:** one thin native app first: SwiftUI/App Intents for an iPhone pilot, or Kotlin
  for Android if that is the reachable audience. Do not build both complete apps first.
  iOS device/Siri work requires macOS/Xcode access; current Windows development can continue
  on the backend and web console.
- **Voice:** begin with push-to-talk, speech recognition, a structured tool-calling planner
  and speech output. Evaluate speech-to-speech for latency and interruption handling once
  tools work. Treat interruption as conversation control, not implicit cancellation of an
  already submitted payment. All write requests are checked by the backend.
- **Backend:** a separate Rust service and worker, with PostgreSQL for transactional order,
  reservation and workflow state. Extract reusable policy logic from the CLI deliberately;
  do not spawn a shell command per customer purchase. Keep the local engine independent of
  hosted or model services, consistent with existing repository requirements.
- **Console:** extend the existing Next.js interface with purchases, permission grants,
  merchant connections, exceptions and evidence. Agent drafts currently do not run workers.
- **Execution:** start with one adapter and a database-backed durable job/outbox mechanism.
  Add infrastructure only when observed volume or reliability requirements justify it.

Suggested model tools: `find_eligible_merchants`, `read_menu`, `quote_cart`,
`propose_purchase`, `read_purchase_status`. These are proposed contracts, not existing APIs.
The server supplies account identity and validates provider IDs. A model-created field such
as `approved: true` has no authority. Payment keys and user payment credentials never enter
the model context. Merchant descriptions and speech transcripts are untrusted input.

## 5. Permission and purchase correctness

Bind approval to a canonical purchase snapshot: account, merchant/location, items/modifiers,
quantity, exact currency/amount, policy version, quote version/expiry and a unique operation
identifier. Use a one-time challenge associated server-side with that snapshot. Changed
prices or baskets require reevaluation and, where required, new approval.

Keep three distinct lifecycles: permission/reservation, payment, and merchant fulfillment.
Suggested states are design sketches and must be reconciled with the selected provider:

- Permission: proposed -> approved/covered -> reserved -> consumed/released/review.
- Payment: not submitted -> pending/unknown -> authorized/captured/failed -> refund states.
- Fulfillment: draft -> submitted -> accepted/rejected -> preparing -> ready -> collected.

Do not hard-code "merchant accepts before payment" for every provider. The adapter must
document its real sequence and compensation path, including when a paid order is rejected.
Cancellation/refund is a separately authorized provider action, not guaranteed rollback.

Use atomic database reservation so simultaneous requests cannot each spend the last remaining
allowance. Track available, reserved and consumed authority separately, with explicit
policies for refunds and expired reservations. A timeout must not automatically release a
reservation or generate a new payment attempt while the original outcome remains unknown.

Persist a stable operation ID and provider idempotency keys before external writes. Verify
webhook signatures, deduplicate events, tolerate out-of-order delivery and reconcile uncertain
outcomes through provider reads. Recovery must survive process restarts. Avoid an unqualified
"exactly once" promise across independent systems.

## 6. What the repository already contains

Inspected this session:

- `crates/fabric-cli/src/identity.rs`, `approvals.rs`, `mandates.rs`, `mcp_proxy.rs`:
  existing agent identity, bounded tool mandates, approvals and proxy enforcement machinery.
- `agentauth.rs::Connection::authorize`: a client that calls a separate `/v1/authorize`
  service, supplying amount, currency and a merchant field. It handles ALLOW/DENY/STEP_UP.
  This is not proof that the separate service has a complete purchase reservation ledger.
- `mandates.rs::Terms`: existing local v1 terms include tool/agent scopes, per-call/day
  limits, daily call count and expiry. They lack explicit currency, merchant, basket and
  pickup constraints. The local usage model is not automatically a commerce settlement ledger.
- `console/src/lib/agent-drafts.ts`: drafts explicitly export `execution_enabled: false`.
- `docs/EXPERIMENT_NORTHWIND.md`: one Stripe test-mode refund comparison; useful evidence
  for scoped gateway behavior, not consumer buying or fulfillment validation.

New work includes mobile authentication, voice runtime, permissioned location, merchant
onboarding and menus, purchase quotes, provider tokenization, versioned consumer mandates,
durable payment/order workflows, concurrent budget reservations and support operations.
No existing storage/API schema should be silently repurposed; use explicit version changes.
The separate authorization-service implementation has not been audited in this research.

The existing internal-finance product plan remains a distinct opportunity. Its earlier
research should not be read as proving a gap in consumer commerce, where platform vendors
and payment protocols are already active.

## 7. Build sequence and evidence required

| Stage | Deliverable | Exit evidence |
| --- | --- | --- |
| 0. Reachable pilot | Choose country, merchant, POS, target phone and provider business model | A café agrees to participate; its supported account/API access and ordering workflow are confirmed |
| 1. Ordering without AI | One authenticated, explicitly approved sandbox order, payment and status flow | Order appears in merchant tools; duplicates, rejection, unknown payment outcome and cancellation are exercised |
| 2. Authority service | Exact purchase approvals, reservation ledger, revocation and evidence | Concurrent requests cannot exceed remaining authority; edited/expired/replayed approvals fail |
| 3. Voice + route | Voice prepares a valid merchant quote and resolves ambiguity | Wrong speech interpretation, unavailable menu, missing location and changed price never silently purchase |
| 4. Assistant entry | Narrow Siri intent or supported Android action, backed by native UI | Verified on target OS/device, including locked device and account switching; works without experimental discovery |
| 5. Live limited pilot | A small participating cohort and merchant group, explicit approvals initially | Actual acceptance/readiness/collection data; measured failure recovery, support load and cost |
| 6. Delegated routine | Opt-in tightly bounded routine purchases | Unauthorized-speaker, duplicate request, revoked grant and provider-authentication cases are handled |
| 7. Enterprise packaging | Tenant isolation, API/SDK, partner identity, scoped credentials, dashboard and support contracts | A design partner embeds the same core and demonstrates a paid, repeatable use case |

Stage 0 should determine the provider. For a US café already using Square, its combined
order/payment/fulfillment surface is a plausible first candidate. For an India pilot, select
a locally reachable café/POS and permitted payment flow; a payment gateway alone supplies
neither its kitchen integration nor blanket authority for voice-triggered purchases.

Measure basket accuracy, accepted/fulfilled orders, time saved versus the user's existing
ordering method, extra detour, pickup waiting, manual interventions, unauthorized attempts,
duplicate charges, unresolved outcomes and cost per completed order. Include speech/model,
maps, infrastructure, payment fees, refunds and support in costs. Do not quote unit economics
or a delivery date until these dependencies and a small benchmark are known.

## 8. Product focus and open decisions

The consumer promise is a successfully completed errand under the user's control. The
enterprise deliverable is reusable authorization/execution infrastructure. Voice, a pixel
landing page and a protocol wrapper are distribution or presentation features; durable
advantage would need demonstrated fulfillment quality, merchant access and operational trust.

Before implementation selection, resolve:

1. First country/city, reachable café and its POS/provider.
2. Consumer product first or one fintech design partner first; who pays and for what outcome?
3. First mobile OS, available test devices and macOS access if choosing iOS.
4. Explicit approval for all pilot orders versus the later scope of delegated routines.
5. Supported payment setup and who handles merchant rejection, refunds and customer support.
6. Location/audio retention, hosting region and acceptable model-provider processing terms.

These decisions are also listed in `console/QUESTIONS.md`. This research does not register
accounts, grant access, connect merchants, collect location/audio, or execute transactions.
