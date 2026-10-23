# Payment lifecycle, idempotency, retries and asynchronous checks in payment APIs (reference for AgentAuth)

Pages were fetched on 2026-10-21. Stripe and Adyen change their docs often, so re-check the cited page before relying on an exact number. "Secondary" marks any source that is not the API owner or the standards body.

## Stripe: PaymentIntent statuses, manual capture, refunds, payouts, webhooks and idempotency

### Takeaway
Stripe uses separate, asynchronous state machines for PaymentIntents, Refunds and Payouts. Several states can still reverse after they look final: a `succeeded` refund can become `failed` up to about 30 days later, and a `paid` payout can become `failed`. Webhooks with event-ID deduplication are the recommended way to track outcomes, and polling is described as "much less reliable". API v1 idempotency is per key, keeps keys for at least 24 hours, replays the stored response (including 500s), and is only safe for POST.

### Cited Findings

**PaymentIntent lifecycle**
- The statuses are `requires_payment_method`, `requires_confirmation`, `requires_action`, `processing`, `requires_capture`, `succeeded` and `canceled`. A failed attempt, such as a decline, returns the PaymentIntent to `requires_payment_method` so it can be retried. It does not move to a terminal "failed" state. — [Stripe: How PaymentIntents work](https://docs.stripe.com/payments/paymentintents/lifecycle)
- `processing` applies to asynchronous payment methods such as bank debits, which "can take up to a few days to process". With manual capture, the intent moves to `requires_capture` instead, and capturing it moves it to `processing` or `succeeded` depending on the payment method. — [Stripe lifecycle](https://docs.stripe.com/payments/paymentintents/lifecycle)
- `succeeded` means "the funds are now in your account and you can confidently fulfil the order". — [Stripe lifecycle](https://docs.stripe.com/payments/paymentintents/lifecycle)
- Cancellation:
  - It is allowed before `processing` or `succeeded`. It "releases any held funds and can't be undone".
  - It is allowed in `processing` for ACH, ACSS, AU BECS, BACS, NZ BECS and SEPA, but "might fail due to a limited and varying cancellation time window".
  - A PaymentIntent can auto-cancel if it is confirmed too many times. — [Stripe lifecycle](https://docs.stripe.com/payments/paymentintents/lifecycle)
- The refunds page gives a narrower list of cancellable statuses: `requires_payment_method`, `requires_capture`, `requires_confirmation`, `requires_action`, and `processing` "only when the associated payment method is US Bank Account". This conflicts slightly with the lifecycle page's list of debit methods. — [Stripe: Refund and cancel payments](https://docs.stripe.com/refunds)
- After `succeeded`, "Subsequent refunds, disputes and outcomes are reflected on the Charge… even though the PaymentIntent remains `succeeded`." The Dashboard maps `requires_capture` to **Uncaptured** or **Partial capture**, and `processing` to **Pending**. — [Stripe: Payment status updates](https://docs.stripe.com/payments/payment-intents/verifying-status)
- Stripe tells merchants not to fulfil on the client. It recommends the `payment_intent.succeeded` and `payment_intent.payment_failed` webhooks. The `processing` event means "wait for the initiated payment to succeed or fail", and `amount_capturable_updated` means "authorised and ready for capture". Polling is "much less reliable and might trigger rate limits". — [Stripe: Payment status updates](https://docs.stripe.com/payments/payment-intents/verifying-status)

**Manual capture (authorize, then capture)**
- Set `capture_method=manual`. After authorization the status is `requires_capture`. `amount_capturable` gives the capturable total, and `payment_method_details.card.capture_before` gives the authorization expiry. If the authorization expires before capture, "the funds are released and the payment status changes to `canceled`". — [Stripe: Place a hold](https://docs.stripe.com/payments/place-a-hold-on-a-payment-method)
- Card-not-present validity windows:

  | Card | Merchant-initiated | Customer-initiated |
  |---|---|---|
  | Visa | 5 days (exactly "4 days and 18 hours") | 7 days |
  | Mastercard | 7 days | 7 days |
  | Amex | 7 days | 7 days |
  | Discover | 7 days | 7 days |

  - Card-present: Visa 5 days; Mastercard, Amex and Discover 2 days.
  - Japan: JPY transactions can be held for 30 days.
  - Stripe classifies MIT and CIT from cardholder-participation signals, not only from the `off_session` flag.
  - Other methods: Klarna 28 days, PayPal 10 days plus an automatic 10-day extension (20 total), Afterpay 13 days, Affirm 30 days, Cash App Pay 7 days.
  - ACH and iDEAL do not support separate capture.
  — [Stripe: Place a hold](https://docs.stripe.com/payments/place-a-hold-on-a-payment-method)
- `amount_to_capture` allows a partial capture, which "automatically releases the remaining amount". Most payments allow only one capture. If you partially capture a payment, "you can't perform another capture for the difference". Multicapture and overcapture are separate, eligibility-gated features. — [Stripe: Place a hold](https://docs.stripe.com/payments/place-a-hold-on-a-payment-method)
- `capture_method=automatic_delayed` is in private preview. It captures about 6 hours before the authorization expires, and `capture_by` accepts `auth_expiry`, `end_of_day` or `target_delay` (30 days at most). — [Stripe: Place a hold](https://docs.stripe.com/payments/place-a-hold-on-a-payment-method)
- Incremental authorization:
  - Set `request_incremental_authorization=if_available`, then call `POST /v1/payment_intents/{id}/increment_authorization` with the new total amount.
  - There is a maximum of 10 attempts per PaymentIntent. Each increment is capped at 500 USD over, or 500% over, the previously authorized amount, whichever is higher.
  - A failed increment returns `card_declined`, and the PaymentIntent "remains capturable for the previously authorised amount".
  - Increments "don't extend the validity period".
  - Only Visa, Mastercard, Amex, Discover and UnionPay are supported, with merchant category restrictions. The feature is on IC+ pricing, or by request.
  - Each increment appears on the statement as a separate pending entry until capture.
  — [Stripe: Increment an authorisation](https://docs.stripe.com/payments/incremental-authorization.md?platform=web&ui=elements)
- An uncaptured (`requires_capture`) PaymentIntent cannot be refunded. You must cancel it. — [Stripe refunds](https://docs.stripe.com/refunds)

**Refunds**
- Refund `status` can be `pending`, `requires_action`, `succeeded`, `failed` or `canceled`.
  - `pending_reason` is `processing`, `insufficient_funds` or `charge_pending`.
  - `failure_reason` is `lost_or_stolen_card`, `expired_or_canceled_card`, `charge_for_pending_refund_disputed`, `insufficient_funds`, `declined`, `merchant_request` or `unknown`.
  - `failure_balance_transaction` reverses the balance impact of a failed refund.
  - Stripe can set `reason=expired_uncaptured_charge` internally.
  — [Stripe: Refund object](https://docs.stripe.com/api/refunds/object)
- You can create several refunds per charge, but "can't refund a total greater than the original charge amount". Refunds draw on the available balance. If the balance is too low, card refunds stay pending, while "refunds for other payment method types fail". — [Stripe refunds](https://docs.stripe.com/refunds)
- A refund can fail after it was initiated. The bank returns the funds "up to 30 days from the post date", the status moves to `failed`, and Stripe emits `refund.failed`. — [Stripe refunds](https://docs.stripe.com/refunds)
- Payment methods without native refunds (Konbini, PromptPay, Boleto, bank transfers) follow `requires_action` → `pending` → `succeeded`. If the bank returns the funds, the refund goes back to `requires_action`. It becomes `failed` if the customer does not respond before expiry, and `canceled` if cancelled. — [Stripe refunds](https://docs.stripe.com/refunds)
- Card refunds can be cancelled only briefly and only from the Dashboard. A cancellation is "a type of refund failure", so `failure_reason` is set. — [Stripe refunds](https://docs.stripe.com/refunds)
- A refund issued soon after the charge can be processed as a reversal (`destination_details.card.type = 'reversal'`), in which case the charge drops off the customer's statement. The customer sees an ordinary refund about 5–10 business days later. — [Stripe refunds](https://docs.stripe.com/refunds)
- Refund events: `refund.created` (Stripe's recommended minimum), `refund.updated`, `refund.failed` and `charge.refunded`. `charge.refund.updated` is deprecated. — [Stripe refunds](https://docs.stripe.com/refunds)
- Bank debit methods carry a double-refund risk if the merchant refunds while the customer's bank also raises a dispute. — [Stripe refunds](https://docs.stripe.com/refunds)

**Payouts**
- Payout `status` can be `paid`, `pending`, `in_transit`, `canceled` or `failed`. A payout is "`pending` until it's submitted to the bank, when it becomes `in_transit`… changes to `paid` if the transaction succeeds, or to `failed` or `canceled` (within 5 business days). Some payouts that fail might initially show as `paid`, then change to `failed`." A failed payout has a `failure_code` (for example `account_closed`, `insufficient_funds` or `could_not_process`) and a `failure_balance_transaction`. — [Stripe: Payout object](https://docs.stripe.com/api/payouts/object)

**Webhooks**
- In live mode, Stripe retries delivery "for up to three days with an exponential backoff". In a sandbox it retries 3 times over a few hours. Manual resend works for 15 days in the Dashboard and 30 days from the CLI. — [Stripe webhooks](https://docs.stripe.com/webhooks)
- Ordering is not guaranteed. Do not use `created` to order events or to detect duplicates. Track event IDs instead. Sometimes "two separate Event objects are generated"; deduplicate those by `data.object` ID plus `event.type`. — [Stripe webhooks](https://docs.stripe.com/webhooks)
- Return 2xx quickly and process events from a queue. Verify the HMAC-SHA256 `Stripe-Signature` (`t=` and `v1=`). The library default tolerance is 5 minutes, and a tolerance of 0 must not be used. — [Stripe webhooks](https://docs.stripe.com/webhooks)

**Idempotency (API v1)**
- Keys go in the `Idempotency-Key` header and can be up to 255 characters. V4 UUIDs are suggested. Do not put sensitive data in keys. — [Stripe: Idempotent requests](https://docs.stripe.com/api/idempotent_requests)
- Stripe "saves the status code and body of the first request… after endpoint execution begins. Retries with the same key return the saved response, including `500` errors." — [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests)
- Keys "can be removed… after they're at least 24 hours old". Reusing a key after it is pruned creates a new request. Stripe compares the parameters with the original request and returns an error on any mismatch. — [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests)
- No result is saved when validation fails, when the request conflicts with a concurrent request, or when it is rate-limited. These requests can be retried. — [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests); [Stripe advanced error handling](https://docs.stripe.com/error-low-level)
- All v1 POST requests accept keys. GET and DELETE are idempotent by definition, and a key has no effect on them. — [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests)
- API v2 behaves differently. Retries "don't repeat successful operations". Stripe retries failed or partial work to completion and returns an updated response, within a 30-day window. — [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests); [Stripe error-low-level](https://docs.stripe.com/error-low-level)
- Retry guidance:
  - A 500 is an indeterminate outcome: "the operation might have produced side effects".
  - "Don't retry with a new idempotency key until you've resolved the original outcome."
  - On a 409 from a key that is still executing, wait and retry with the same key and parameters, because "A new key can start a second operation".
  - Retry with the same key for up to 24 hours. After that, stop and reconcile through retrieval, the Dashboard and webhooks.
  - Put a local identifier in `metadata` so webhook objects can be matched to your records.
  - A replayed response carries `Idempotent-Replayed: true`. The `Stripe-Should-Retry: true/false` header overrides inference from the status code.
  - Keys can be random or "derived from a user-attached object, like the ID of a shopping cart". Persist the key across process restarts.
  — [Stripe: Advanced error handling](https://docs.stripe.com/error-low-level)
- 402 means "the parameters were valid but the request failed", 409 means a conflict "potentially due to using the same idempotent key", and 429 should be retried with exponential backoff. — [Stripe error-low-level](https://docs.stripe.com/error-low-level)

### Inferences
- A budget that counts a refund or payout as final when the API first responds will overstate the money actually moved. Commitments should be recorded as pending, and settled only on a terminal webhook state, with a later `failed` able to reverse a "settled" entry.
- Stripe has no "failed" PaymentIntent status. A gateway must treat `requires_payment_method` with `last_payment_error`, or the `payment_intent.payment_failed` event, as an attempt failure.
- When a gateway does not know an outcome, the safe action is to reconcile. Re-issuing with a new key could cause a double payout or double refund.
- The gateway's own idempotency key should map one approved authorization to exactly one upstream key, persisted before the upstream call is made.

### Gaps
- I did not fetch the Stripe pages on extended authorization, multicapture and overcapture, so their exact windows and limits are not covered here.
- I did not fetch the Stripe Payout create and cancel endpoint pages, so payout cancel rules are not covered beyond the status text.
- The exact backoff schedule for webhook retries is not published beyond "up to three days".

## Adyen: authorise, capture, cancel and refund; asynchronous modifications and webhooks; idempotency

### Takeaway
Every Adyen modification (capture, refund, cancel, reversal) is asynchronous. The API answers `status: "received"`, and the outcome arrives in a webhook (`CAPTURE`, `REFUND`, `CANCEL_OR_REFUND` and others) with `success` true or false. Even `success: true` can later be undone by `CAPTURE_FAILED`, `REFUND_FAILED` or `REFUNDED_REVERSED`. Adyen idempotency keys are valid for 7–14 days, scoped to the company account, and limited to 64 characters.

### Cited Findings
- Capture:
  - Capture is automatic by default, immediately after authorization. Manual and delayed capture are both available.
  - A capture response with status `received` means the request is queued, and "You will receive the result in a webhook".
  - `CAPTURE` with `success: true` means the request is valid (the authorization has not expired and the balance is available), but the scheme can still reject it.
  - `CAPTURE_FAILED` can arrive "a few days after you submitted the capture request".
  - After a single partial capture, "Any unclaimed amount… is automatically cancelled". Multiple partial captures must be enabled by Support.
  - An expired authorization leaves the remaining balance at 0.
  - Delayed capture does not send a `CAPTURE` webhook by default; you must enable it.
  — [Adyen: Capture](https://docs.adyen.com/online-payments/capture)
- Refunds:
  - Call `POST /payments/{paymentPspReference}/refunds`. The response gives a refund `pspReference`, and the outcome comes in the `REFUND` webhook.
  - `success: true` means "Adyen's validations were successful and we sent the refund request to the card scheme".
  - `REFUND_FAILED` can follow days later.
  - `REFUNDED_REVERSED` means the funds came back to the merchant account, for example for bank transfers, iDEAL or Bancontact.
  - Multiple partial refunds cannot exceed the captured amount.
  - "You can only refund a payment after it has been captured." If the capture state is unknown, use a reversal.
  - Funds reach the shopper in "up to 40 business days" depending on the payment method.
  — [Adyen: Refund](https://docs.adyen.com/online-payments/refund)
- A reversal cancels the payment if it is uncaptured and refunds it if it is captured. The response is `status: "received"`, and the outcome comes in the `CANCEL_OR_REFUND` webhook. — [Adyen: Reversal](https://docs.adyen.com/online-payments/reversal)
- Idempotency:
  - Send the header `idempotency-key` (case-insensitive). Keys can be up to 64 characters, and V4 UUIDs are recommended.
  - Keys are valid "7 to 14 days after first submission", stored at company-account level, and not checked across regions.
  - A retry of a processed request returns the first response "without duplication".
  - A concurrent duplicate returns HTTP 422 or 409 with error code 704, "request already processed or in progress".
  - A `transient-error: true` response header means retry later with the same key, using exponential backoff.
  - A 503 with code 703 means the idempotency store is unavailable.
  - Behaviour for the same key with a different payload is not documented on this page.
  — [Adyen: API idempotency](https://docs.adyen.com/development-resources/api-idempotency)
- Webhooks:
  - Acknowledge with a 2xx "before applying any business logic". If there is no success within 10 seconds, the webhook is marked Failing and retried.
  - Duplicates share `eventCode` and `pspReference`; use the latest one.
  - Check timestamps for ordering, and use `sequenceNumber` where it is present.
  — [Adyen: Handle webhook events](https://docs.adyen.com/development-resources/webhooks/handle-webhook-events); [Adyen: Webhooks](https://docs.adyen.com/development-resources/webhooks)

### Inferences
- In Adyen terms, a "received" response or a `success: true` webhook means the request was accepted. It does not mean money moved. A gateway should hold three separate states: request accepted, provider validated, and final.
- The deduplication key differs by provider: Stripe uses the event ID, Adyen uses `eventCode` + `pspReference`. A gateway should normalize both into its own event log.

### Gaps
- The Adyen Cancel page could not be fetched because the domain was blocked for that URL, so the `CANCELLATION` event semantics come only from the Capture page's mention of cancelled amounts.
- The Adyen authorization validity table and the webhook retry schedule live on linked pages I did not fetch.

## Card networks: authorization, capture, clearing, settlement, holds, expiry and reversals

### Takeaway
An authorization places a hold on the cardholder's open-to-buy funds. It does not move money. Money moves through clearing and settlement after capture. Visa requires reversals within 24 hours when a sale will not complete or when the final amount is lower than the amount authorized.

### Cited Findings
- "With each successful authorization, the issuer typically reduces the amount available to the cardholder for other purchases… commonly known as an authorization hold. Authorizations are meant to be settled." — [Visa: Estimated and Incremental Authorization and Reversal Processing Requirements (©2024)](https://usa.visa.com/content/dam/VCOM/regional/na/us/support-legal/documents/authorization-and-reversal-processing-best-practices-for-merchants.pdf)
- Reversals "notify the issuer that all, or part, of a transaction has been cancelled and that the hold on cardholder funds should be removed".
  - Transaction not completed: the "entire authorized amount must be reversed within 24 hours" of the merchant knowing, or by the end of the validity period.
  - Final amount below the estimated plus incremental total: the difference must be reversed within 24 hours of completion.
  - Missing or non-matching data can leave funds "held for a longer period or in duplicate".
  — [Visa (©2024)](https://usa.visa.com/content/dam/VCOM/regional/na/us/support-legal/documents/authorization-and-reversal-processing-best-practices-for-merchants.pdf)
- Estimated authorizations must be a "genuine estimate", the cardholder must be told that later authorizations may follow, and incremental authorizations can only follow an estimated or incremental authorization. — [Visa (©2024)](https://usa.visa.com/content/dam/VCOM/regional/na/us/support-legal/documents/authorization-and-reversal-processing-best-practices-for-merchants.pdf)
- Issuers match reversals to authorizations using STAN (field 11), RRN (37), Auth ID response (38) and Transaction ID (62.2). — Visa reversal guidance as summarized by search (secondary summary of [Visa authorization reversals PDF](https://usa.visa.com/content/dam/VCOM/download/merchants/authorization-reversals.pdf)); not verified against full text.
- Card statements may not distinguish authorizations from captured (settled) payments. — [Stripe: Place a hold](https://docs.stripe.com/payments/place-a-hold-on-a-payment-method)
- Debit holds can drop off after 1–4 days and credit holds can last up to 30 days, depending on the issuer. — Secondary: [Worldline docs](https://docs.connect.worldline-solutions.com/accepting-payments/payment-scenarios/authorization-reversal) (search snippet only).

### Inferences
- An authorization hold reserves money; it does not spend it. AgentAuth should count holds as reserved budget, and release them on cancel, expiry or partial capture.

### Gaps
- I did not retrieve primary Visa or Mastercard rulebook text defining clearing and settlement timelines (for example T+1 settlement). The authorization validity periods in network rules come only from Stripe's summary above.

## ISO 20022: pain.002 and pacs.002 status reporting

### Takeaway
ISO 20022 reports status as a sequence of graded acceptance codes, not a single success or failure. RCVD → ACTC → ACCP → ACSP → ACSC, with PDNG for holds and RJCT, which needs a reason code. Reports exist at group level and at transaction level, and banks implement subsets of the codes differently.

### Cited Findings
- pacs.002 is an FI-to-FI Payment Status Report. It responds to pacs.008, pacs.003 or pacs.004, and a payment can have several of them, which must follow lifecycle order.
  - Transaction-level codes: RCVD (received), ACCP (processed by instructed agent), ACSP (accepted, settlement in progress), ACSC (accepted, settlement completed), PDNG (on hold awaiting checks such as compliance), RJCT (rejected, with a mandatory reason as `ExternalStatusReason1Code` or proprietary).
  - Group-level codes add ACTC (technical validation), ACWC (accepted with change) and PART (partially accepted, with the rejected transactions listed).
  - Reason codes include AC01, AC04, AM04, AM05 (duplicate), MD01, FRAD and TECH.
  — Secondary (vendor guide): [Mambu Payments (formerly Numeral) pacs.002.001.10](https://docs.mambupayments.com/docs/pacs002-xsd-business-logic)
- Definitions vary between implementers. Token.io defines ACSC as "Settlement on the debtor's account has been completed" and ACCP as technical validation plus customer profile checks passed. — Secondary: [Token.io ISO 20022](https://docs.token.io/products/tpp/sip/sip-v2/iso-20022)
- pain.002 bank guides use ACTC for a file that passed technical validation, ACCP for one that also passed content validation, PART for partial acceptance, PDNG for pending, and RJCT for rejected. Banks agree which codes they use bilaterally. — Secondary (search snippets): [ING pain.002 format description](https://assets.ing.com/asset/1ef37150-b5b8-4b5b-96c3-ab1fb374d6bd/ING-Format-Description_Payment-Status-Report_Pain-002-001-003-Connectivity-Channels.pdf); [Handelsbanken guide](https://handelsbanken.com/tron/xgpu/info/contents/v1/document/72-111390)
- Payments Canada's RTR usage guideline restricts pacs.002.001.10 (RTR_Exchange_UG2025_v1.3, published 3 June 2025). One rule found: if the group status is PDNG, a transaction status must not be RJCT. — [Payments Canada RTR pacs.002 (PDF)](https://www.payments.ca/sites/default/files/RTR_FItoFI_PaymentStatusReport_pacs.002.pdf); the rule comes from the search snippet and I did not confirm it in the extracted text.

### Inferences
- ISO's graded statuses (accepted for processing, settlement in progress, settled, rejected with reason) are a well-established model that an authorization layer could mirror, rather than a boolean outcome.

### Gaps
- iso20022.org and the GS developer page returned 403, so I did not obtain the official ExternalPaymentTransactionStatus1Code definitions (including ACCC, ACWP, BLCK and CANC). The definitions above come from vendor guides.

## Retries, backoff, webhook deduplication and "pending until confirmed"

### Takeaway
The sources agree: use client-generated unique keys, return the original result for a duplicate, return a conflict while the original is in flight, reject a reused key with a different payload, back off exponentially with jitter, and treat webhooks as at-least-once and unordered.

### Cited Findings
- The IETF Idempotency-Key header draft (draft-ietf-httpapi-idempotency-key-header-07, revised 2025-10-15) is **expired and archived**. It is not an RFC. — [IETF datatracker](https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/)
- What the draft specifies:
  - The value is an RFC 8941 String. The key "MUST be unique and MUST NOT be reused with another request with a different request payload".
  - An optional payload fingerprint is allowed. Expiry is optional, but the server "SHOULD define such expiration policy and publish it".
  - Errors: 400 for a missing key, 409 for a concurrent retry, 422 for a key reused with a different payload.
  - It targets POST and PATCH.
  - Security: use high-entropy keys and consider a composite key that combines the client key with client identity.
  — [IETF draft-07](https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-idempotency-key-header-07)
- Brandur Leach's Stripe blog post (22 Feb 2017) names three failure modes: connection failure, mid-operation failure ("work in limbo") and response failure. It recommends idempotency keys, exponential backoff of about 2^n, and random jitter against a "thundering herd". — Engineering blog: [Stripe blog: idempotency](https://stripe.com/blog/idempotency)
- Stripe SDKs auto-generate keys and retry with exponential backoff. — [Stripe error-low-level](https://docs.stripe.com/error-low-level)
- Stripe delivers webhooks at-least-once and unordered, deduplicated by event ID. Adyen deduplicates by `eventCode` + `pspReference`. — see the citations in the Stripe and Adyen sections above.
- Stripe's asynchronous payment methods "can take up to several days to confirm… During this time, the payment can't be guaranteed". — [Stripe lifecycle](https://docs.stripe.com/payments/paymentintents/lifecycle)

### Inferences
- AgentAuth's gateway can follow the IETF and Stripe model directly:
  - persist the key and a request fingerprint before acting
  - return 409 while the upstream call is in flight
  - return 422 on a payload mismatch
  - replay the stored outcome
  - state the retention window, which must be at least the upstream window (Stripe 24 hours, Adyen 7–14 days)
- The agent's UX should show "submitted / pending confirmation" until the upstream webhook settles the outcome.

### Gaps
- I found no authoritative standard on webhook retry schedules; each provider defines its own.

## What an authorization layer should record so its budget reflects money actually moved

### Takeaway
None of the provider APIs exposes a single number for money moved. Each separates authorized, captured, refunded and failed amounts, and every one of these can change asynchronously. An authorization layer should therefore keep an append-only ledger per action and reconcile it with provider webhooks.

### Cited Findings
- Stripe fields:
  - PaymentIntent: `amount`, `amount_capturable`, `amount_received`, and `capture_before` on the charge.
  - Refund: `status`, `pending_reason`, `failure_reason`, `failure_balance_transaction`.
  - Payout: `status`, `failure_code`, `failure_balance_transaction`, `reconciliation_status`, `trace_id`.
  — [Stripe: Place a hold](https://docs.stripe.com/payments/place-a-hold-on-a-payment-method); [Refund object](https://docs.stripe.com/api/refunds/object); [Payout object](https://docs.stripe.com/api/payouts/object); [Stripe incremental authorization](https://docs.stripe.com/payments/incremental-authorization.md?platform=web&ui=elements)
- Stripe recommends storing a local identifier in `metadata` so it can match webhook objects, including ones created during Stripe's incident reconciliation. — [Stripe error-low-level](https://docs.stripe.com/error-low-level)
- Adyen identifiers: the original payment `pspReference`, each modification's own `pspReference`, and the webhook `originalReference`. Refusal details are in `additionalData.refusalReason`. — [Adyen: Refund](https://docs.adyen.com/online-payments/refund)

### Inferences
These are design suggestions derived from the findings above, not taken from any single source:

- **Per approved action, record:**
  - mandate and approval ID, agent identity
  - provider, endpoint, and a hash of the request parameters
  - the gateway's idempotency key and the upstream key
  - upstream object IDs (PaymentIntent, Charge, Refund, Payout, `pspReference`)
  - currency and amounts in minor units
  - a state history with event IDs and timestamps
- **Budget buckets:**
  - **reserved**: approved but not yet sent, plus authorized holds
  - **committed**: sent; the upstream state is pending, processing, in_transit, or `received`/`success: true`
  - **settled**: a terminal success, such as PaymentIntent `succeeded`, refund `succeeded` or payout `paid`, which can still be reversed later
  - **released**: cancel, expiry, the uncaptured remainder of a partial capture, failed or canceled outcomes, or `*_FAILED` / `REFUNDED_REVERSED` events
  - **unknown**: a 5xx or a timeout; block a re-issue until reconciled
- Spend limits should be checked against reserved + committed + settled, so that pending money cannot be spent twice. Reports should still show settled amounts separately.
- A `failed` event arriving after `paid` or `succeeded` must move money back out of settled. Late reversals are documented to arrive up to about 30 days later for Stripe refunds, within 5 business days for Stripe payouts, and days later for Adyen.
- Evidence exports should state what was verified: "provider reported X at time T". They should not claim that funds arrived.

### Gaps
- No primary source prescribes ledger design for a third-party authorization layer. This section is inference.
