# Cosigna commute purchase: first real merchant pilot

Updated 22 October 2026. This is the next delivery slice, not a claim that Siri,
location search or live merchant ordering is already connected.

## Target experience

1. User: “Hey Siri, order me a black coffee on my way to office with Cosigna.”
   The exact supported invocation must be tested on the target OS. Do not promise
   that an unqualified generic request will automatically select Cosigna.
2. Cosigna uses a saved, user-confirmed office destination and one permissioned
   device location fix. Ask for the destination when missing, rather than infer it.
3. Find open, connected cafés along the route; rank by added journey time, stock,
   fulfillment readiness and the user's saved preferences/limits. A café found on a
   map is not necessarily a café Cosigna can order from.
4. Fetch its actual catalog/availability and all-in quote. Prepare the cart without
   committing the purchase. Ask one brief question if size, item or location matters.
5. Speak a grounded review, for example: “One black coffee at [connected café],
   [total including fees], [pickup estimate from merchant]. Shall I place it?”
6. A fresh “I confirm” after that review authorizes only that quote. Revalidate its
   digest, expiry, user/session, allowance and current merchant state before submit.
   Voice is a command, not identity; OS authentication or payment authentication can
   still require interaction. Do not promise a voice-only checkout for every payment.
7. Reconcile the accepted order and payment; speak the receipt and pickup information.
   Unknown results require lookup, never a replacement purchase. Say “ready” only
   after the merchant reports readiness, and distinguish that from an estimate.

## Working foundation and missing pieces

| Capability | Current state | Next work |
| --- | --- | --- |
| Speech request, clarification, quote readout, fresh voice approval | Paired Expo lab; user reports working Nokia flow | Reduce recording latency and evaluate conversational turn-taking/interruptions |
| “I confirm” | Added as an exact fresh post-quote command; existing confirmation remains accepted | Device retry; retain negation/change/replay checks |
| Budget, exact quote, idempotency and reconciliation | Existing Rust service and test merchant | Apply the same checks to the live merchant connector |
| Payment | Stripe sandbox in current phone lab | Supported customer payment setup and explicit, capped live pilot |
| Assistant entry | Not connected | iOS App Intents/App Shortcuts; supported Android App Actions, tested per device/account |
| Native application | Expo Go preview, not a standalone Cosigna assistant integration | Native development build, generated config/native module integration and signed install |
| Location and commute | Not implemented; native config currently blocks location | Foreground permission, freshness/accuracy checks, saved destination and manual fallback |
| Café discovery | Fixed local test catalog | Routes/places provider; match discovered businesses to authorized connector locations |
| Real fulfillment | Local test merchant only | One consenting café, actual ordering access, status and cancellation semantics |
| Device-to-server transport | Android USB loopback | Authenticated HTTPS with production identity and scoped device sessions |

## Delivery order

First connect one merchant and validate its sandbox catalog, quote, payment, order and
fulfillment records. Keep this behind the existing approval/transaction boundary.
Then add foreground location and route ranking across the connected locations.
Finally expose the complete workflow through native assistant entry and test locked
phone, permission denial, interruptions, stale quotes, changed prices and lost responses.
The system assistant starts a task; it does not supply payment authority. An incoming
intent/deep link may prepare an order but must never auto-approve it.

Use the current cross-platform app and Rust service. Do not replace the core with a
model-controlled payment tool or start a second mobile rewrite. Merchant onboarding
and a public secure backend are prerequisites to a live end-to-end demonstration.

Pilot decisions still needed: target phone/OS, city, consenting merchant and its actual
API access, permitted payment method/currency, spend ceiling and the saved destination.
No API key, office address or payment credential belongs in this document.

## Integration references checked

- [Apple App Intents](https://developer.apple.com/documentation/appintents) and
  [confirmation workflow](https://developer.apple.com/videos/play/tech-talks/10168/):
  native app actions, Siri/Shortcuts discovery and system-managed confirmation.
- [Android App Actions](https://developer.android.com/develop/devices/assistant/overview):
  registered capabilities launch a specific action with parameters; invocation is
  subject to Assistant matching. This is not arbitrary conversational-agent hosting.
- [Expo development builds](https://docs.expo.dev/develop/development-builds/introduction/):
  native customization requires our own build rather than modifying Expo Go.
- [Google Places search along a route](https://developers.google.com/maps/documentation/places/web-service/search-along-route):
  route-aware discovery; orderability and fulfillment require a separate merchant API.
- [Square pickup fulfillment](https://developer.squareup.com/docs/orders-api/fulfillments):
  one possible connector for a consenting supported seller, not universal café access.
