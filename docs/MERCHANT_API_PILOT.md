# Merchant API and voice-purchase pilot

Researched 22 October 2026. The immediate goal is one verifiable voice-to-order transaction,
followed by a small supervised purchase at a cooperating external merchant. We need separate
evidence for speech understanding, authorization, payment, order acceptance and fulfillment.

## Available routes

| Route | Official capability | Access boundary and fit |
|---|---|---|
| Square Catalog + Orders + Payments | Catalog-backed orders, pickup fulfillment, and payment applied to an order | Strong candidate for a cooperating café using Square. Requires access to the seller account and payment integration, not arbitrary café access. Start in Sandbox; account/region and live-payment eligibility still need checking. |
| Shopify Storefront Cart | Create/update a cart and obtain `checkoutUrl` | Useful for e-commerce with a participating shop. Standard cart flow redirects the buyer to Shopify checkout; creating a cart is not proof of a completed autonomous purchase. |
| Merchant we control | Implement catalog, order submission, idempotency, lookup, inventory and staff fulfillment | Fastest controllable test of the Cosigna boundary. It cannot establish external merchant access or live payment success. |

Sources: [Square order creation](https://developer.squareup.com/docs/orders-api/create-orders),
[Square order payments](https://developer.squareup.com/docs/orders-api/pay-for-orders),
[Square seller OAuth](https://developer.squareup.com/docs/oauth-api/overview),
[Shopify carts and checkout URL](https://shopify.dev/docs/storefronts/headless/building-with-the-storefront-api/cart/manage).

Square's documentation distinguishes paid pickup orders visible to the seller from simply
creating an order. Seller OAuth grants scoped access to a specific account. These are reasons
to validate seller visibility and payment/fulfillment status during the pilot rather than
announce success from a returned order ID alone. The choice of Square is an engineering
recommendation from its documented capabilities, not an agreement or approved production access.

## Decision and implemented increment

Build and use [the local voice commerce lab](../testbeds/voice-merchant/README.md) first. Its
merchant runs independently over HTTP, stores stock/orders in its own SQLite database, and has
a browser staff counter. The existing Rust transaction layer calls it through a fixed,
authenticated connector and retains budget reservations for uncertain outcomes.

This is more than the previous in-process script: the server receives an API request, commits
an order and stock change, returns a receipt, and supports lookup after a lost response. Its
default payment acceptance is simulated locally. The new `npm run dev:stripe` mode instead
calls Stripe's actual test API, persists PaymentIntent/Charge identifiers and verifies status
before a paid order is created. A separate explicit $7.00 sandbox check has passed for exact
approval, replay and over-budget blocking. Neither mode is an actual café booking or proof of
live-funds commerce/external adoption. See the testbed README and `reports/stripe-protocol-*.json`.

Use recorded-file transcription followed by the existing hosted planner for this first voice
slice. OpenAI's current transcription guidance recommends `gpt-transcribe` for completed audio
recordings; its voice documentation distinguishes this pipeline from a realtime speech agent.
The browser records at most 20 seconds, the backend transcribes, the planner proposes a typed
catalog selection, and the owner approves the exact quote. Speech is an input, not proof of
identity or spending authority. [File transcription](https://developers.openai.com/api/docs/guides/speech-to-text),
[voice architectures](https://developers.openai.com/api/docs/guides/audio).

This is an intentionally small first commerce slice: one fixed pickup product/quantity per
quote, USD all-in test prices, one merchant, one local owner. It does not implement a multi-item
cart, location routing, shipping, flight ticketing, continuous conversation or voice-only consent.
Those require their own provider contract and tests rather than a more permissive prompt.

## Next external merchant pilot

1. Select one consenting merchant in a supported market, confirm catalog/API access and define
   the exact item, currency, tax/fees, pickup location/time, cancellation terms and total ceiling.
2. Obtain the seller's scoped integration access through its supported account flow. Implement
   the payment provider's supported customer authorization/tokenization flow. Model prompts and
   mobile clients must never receive merchant secrets or reusable payment credentials.
3. Build a provider-specific quote adapter and sandbox connector. Reconcile the distinct payment,
   order and fulfillment records; account for payment authentication and partial failure between
   an order and its payment. A payment API is not a menu, stock or merchant fulfillment API.
4. Test duplicates, exact quote drift, stock loss, expiry, budget races, missing responses,
   cancellation/refund and authoritative lookup. Authenticate webhooks, deduplicate them, and
   bind them to the actual order/payment before changing spend or displaying success.
5. Review one concrete live quote and have the user approve the capped purchase. Check both
   the payment record and the seller's actual order system. Have the merchant physically fulfill
   the order before claiming successful pickup.

The current local HTTP connector intentionally rejects public merchant URLs and live/non-simulated
receipts. Connecting Square or a real payment processor requires a dedicated adapter and a tested
live-mode authority boundary; simply changing the URL or removing `simulated` checks is insufficient.

## Acceptance evidence

| Claim | Evidence required |
|---|---|
| Voice understanding works | Representative actual recordings and checked transcripts, quantities and clarifications |
| User authorized the purchase | Authenticated approval bound to exact item, total, currency, seller, terms and expiry |
| Payment succeeded | Authoritative processor status, not model text or a browser redirect |
| Merchant accepted | The merchant's persisted order associated with the payment and idempotency key |
| No duplicate or overspend in the tested cases | Concurrent/replay/failure tests and reconciled ledger/provider records |
| Item is ready or collected | Merchant fulfillment state and physical confirmation for a live pilot |

The local tests cover the controlled transaction boundary with synthetic audio/transcription and
payment. Hosted speech/model quality, an actual microphone session and a cooperating merchant's
live order are still separate validations. No external account, store, paid API session or live
purchase was created during implementation.
