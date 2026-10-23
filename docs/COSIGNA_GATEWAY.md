# Cosigna Gateway build plan

Current focus: the permission/budget and transaction execution engine, independent of voice.
The user-provided **Cosigna Focus Areas and Build Plan** (reviewed 23 October 2026) sets the
priorities: permission reliability, one complete purchase flow, adversarial tests, then the
customer interface. This follows the [two-product direction](COSIGNA_REBUILD.md).

## Working increment

Registered agents now have separate credentials, immutable `plan`/`execute` permissions,
one owner-controlled budget, expiry and revocation. Creation, planning and execution use the
existing durable task/quote/reservation/attempt machinery. A task is atomically attributed to
its originating agent. That credential cannot read another agent's tasks, change its budget,
register agents, approve its own transaction or access the legacy role-token API.

The owner sets merchant/currency/per-purchase/total limits in the budget. Agent authority
never exceeds that budget. Several agents can intentionally share one budget; reservations
and spend count against the same total. The planner proposes only a catalog offer and quantity
or a clarification. Owner approval binds the complete quote; permission to execute does not
remove that requirement. Privileged owner-controlled execution can submit a plan-only agent's
approved task. Revocation/expiry are rechecked at the submission claim. The owner can still
recover status after revocation; an already claimed external request can finish.

The gateway still binds to loopback, uses development owner authentication and submits only
synthetic/test purchases. These reusable agent bearer credentials are not the document's
proposed signed, single-use transaction permits. Those are a separate unfinished control.

## Run without voice or model credentials

```powershell
cd C:\Users\Harsha\Pictures\project
cargo run -p cosigna-agent -- gateway-demo
```

The synthetic owner test driver approves exactly one fixture purchase; it is not an AI
approving itself. Expected report: one synthetic order, 700 minor units spent, 300 available,
and all checks true. No API key, raw bearer or payment credential is printed.

For the headless HTTP API, keep a separate gateway profile:

```powershell
$gatewayData = Join-Path $env:LOCALAPPDATA 'Cosigna\gateway-dev'
cargo run -p cosigna-agent -- init --data-dir $gatewayData
cargo run -p cosigna-agent -- serve --data-dir $gatewayData --port 3100 --planner fixtures
```

The server exposes both the existing `/v1/` interface and `/gateway/v1/`. In an authorized
owner process, load only the owner token from this profile's private `credentials.json`.
Register an agent and deliver its returned token only to that agent's trusted runtime.
Never send tokens to a model or print the full registration response in logs. Keep the
original phone lab on its existing profile and ports.

| Endpoint under `/gateway/v1` | Caller | Behavior |
| --- | --- | --- |
| `POST /budgets` | Owner | Existing capped merchant/currency allowance input |
| `GET /budgets/{id}` | Owner or bound agent | Balance and reserved/spent authority |
| `POST /budgets/{id}/revoke` | Owner | Stop future spending claims |
| `POST /agents` | Owner | Register `{name,budget_id,permissions,expires_at}`; return token once |
| `GET /agents`, `GET /agents/{id}` | Owner | Metadata without credentials/hashes |
| `POST /agents/{id}/revoke` | Owner | Idempotent revocation; no cancellation or refund implied |
| `GET /catalog` | Owner or agent | Agent catalog filtered to its budget's merchants/currency |
| `POST /tasks` | Owner or agent with `plan` | Existing strict `TaskInput`; identity is derived from the credential |
| `GET /tasks/{id}`, `GET /tasks/{id}/events` | Owner or originating agent | Owned task and local event history |
| `POST /tasks/{id}/plan` | Owner or agent with `plan` | `{}`; server-selected fixture/hosted planner proposes an offer |
| `POST /tasks/{id}/approve` | Owner | `{quote_digest}`; exact quote and atomic budget hold |
| `POST /tasks/{id}/execute` | Owner or agent with `execute` | `{}`; current approval/scope checked before one submission claim |
| `POST /tasks/{id}/reconcile` | Owner or originating active agent | `{}`; authoritative lookup, no replacement purchase |
| `POST /tasks/{id}/cancel` | Owner | `{}`; only before submission |

All owner operations remain scoped to their authenticated owner. Gateway agents cannot choose
an owner/agent ID through task JSON. Development legacy planner/executor keys remain privileged
and are not agent credentials. A bearer proves possession of a scoped development token, not
the physical user's identity. Production authentication and network deployment remain pending.

## Current rule contract and decision flow

Owner budget input:

```json
{"currency":"USD","limit_minor":2000,"per_purchase_minor":1000,"merchants":["fixture-cafe"],"expires_at":1800000000}
```

Agent registration input (use an actual budget ID and future expiry within that budget):

```json
{"name":"purchase-agent","budget_id":"budget_example","permissions":["plan","execute"],"expires_at":1799999900}
```

These are the implemented strict data contracts, not a new general policy language. Read
them as: authenticate owner/agent; check ownership and agent/task/budget binding; check the
requested operation; validate grant expiry/revocation; obtain a merchant-backed quote;
require its exact owner-approved digest; check current quote and budget; atomically reserve;
recheck before the durable submission claim; execute through the fixed connector; record
verified success/failure or keep an unknown outcome reserved and reconcile it.

Cedar/OPA from the supplied plan are candidates for future policy authoring/evaluation.
Adopting one will not replace atomic accounting, transaction binding, idempotency or recovery.
Do not add a policy engine or rewrite the Rust core without a concrete rule requirement and
tests establishing the new boundary.

## Next increments from the supplied plan

1. **Complete gateway safety evidence:** extend the existing model-output/quote/replay tests
   with property-based budget invariants, simultaneous registered agents, actual process-kill
   boundaries and controlled failure injection. Run the offline suite on every build.
2. **Transaction permits:** define canonical signed, short-lived, single-use authorization,
   issuer/audience/key rotation and revocation, then bind consumption to the durable attempt.
   Decide how a verified owner approval supplies authority. Never treat an agent session
   bearer or a quote digest as that signed proof.
3. **Production identity and operations:** authenticated customers/agents, device/owner
   approval proof, HTTPS, bounded concurrent workers, request/rate limits, secret storage,
   redacted tracing and reconciliation operations. The current loopback server is not an
   Internet endpoint. Do not open or tunnel the phone development lab to simulate this stage.
4. **One processor/merchant sandbox flow:** merchant choice, actual price/availability,
   permission decision, customer authentication, order/payment IDs, receipt and verified
   recovery. Preserve payment/order/fulfillment distinctions and test compensation paths.
5. **Model evaluation:** use an existing hosted planner first; measure task correctness,
   clarification quality, latency, cost and unsafe action proposals on held-out cases.
   Consider training only after observing a specific gap and obtaining consented, redacted
   examples. Trained weights never enforce policy or carry payment credentials. No model has
   been trained or live inference run by this gateway increment.

The plan's native iOS/SwiftUI and India-first Razorpay/UPI suggestions are proposals to verify
before selecting the pilot, not instructions to discard the working test infrastructure.
Payment-method permissions, merchant access, customer authentication and applicable requirements
must be checked against current provider documentation for the selected country. Existing
USD Stripe sandbox tests do not establish Indian payment eligibility. Passport storage is
outside the purchase gateway's first milestone. The four-to-six-week schedule is a planning
estimate; merchant/provider access and security validation determine live-pilot readiness.

## Verification and remaining limits

`crates/cosigna-agent/tests/gateway.rs` covers agent isolation, permission escalation, exact
approval, hashed credentials, restart, idempotent execution, expiry/revocation, unknown-result
recovery, delayed model output after revocation, strict registration and v1 ledger migration.
The real HTTP test exercises issued agent credentials, legacy API refusal, Host/Origin checks,
owner approval, execution and revocation. Existing transaction/model/merchant tests still apply.

Storage upgrade and wire contracts are normative in [STORAGE_FORMAT.md](STORAGE_FORMAT.md).
No existing project store or merchant/provider schema is changed. This increment is not a
complete production gateway, cryptographic transaction protocol, real-money pilot, trained
payment model or demonstrated resistance to every prompt-injection/credential-theft attack.
