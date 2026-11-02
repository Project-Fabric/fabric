# C02: recovery under HTTP admission saturation

31 October 2026. Follow-up to merged PR #124 (`59e932b`) on main `f066deb`.
Tracking: #111 under #109. This increment adds regression coverage, not a new
payment rail, model path, schema or capacity claim.

## Failure boundary

The earlier local overload run left an approved operation holding budget without
a claimed payment attempt after HTTP 500. The merged recovery repair returns stale
unsubmitted work to review, releases its unused reservation and clears approvals.
Claimed/unknown attempts remain subject to lookup-only recovery.

The new real-binary regression reproduces this durable failure pattern under
admission saturation. It deliberately holds a SQLite writer transaction until the
server's five-second busy timeout returns HTTP 500 from an execution request.
Approval was recorded through the service before the fault. Three incomplete,
authenticated agent requests occupy the shared HTTP admission pool; an additional
agent request receives HTTP 503. The owner uses the separately reserved slot.

This is controlled fault injection at the approved-before-claim boundary. It does
not identify the original incident's cause, reproduce its complete traffic pattern,
or establish that organic overload alone produces this error on current main.

## Explicit pass criteria

### Interrupted execution

- The server returns the observed internal error while the operation remains
  approved, with USD 5 reserved and no claimed or provider-side attempt.
- Health remains reachable while shared admission is saturated.
- After releasing the database blocker, recovery returns the stale operation to
  review, clears its approval and releases the unused reservation.
- Replaying the exact original request does not restart spending. Explicit execute
  without fresh approval conflicts.
- A fresh exact-proposal owner approval completes precisely one USD 5 fixture refund.
- Ledger totals match provider effects and the evidence verifier passes.

The worker's grace period is advanced using its existing explicit test-time input;
the test does not wait 30 seconds or change the production clock.

### Concurrent refunds and idempotent replays

- Launch 48 synchronized HTTP clients for 24 distinct requests, two clients per key.
  Each requests USD 1 against the same USD 10 budget and authoritative fixture case.
- The first burst may return 200, admission 503 or server-deadline 504. Any internal
  error fails the test. A timeout is not interpreted as evidence of zero effect.
- After the burst, replay each original key and identical payload through the same
  HTTP endpoint. This is an idempotency test, not a new-key retry strategy for an
  uncertain payment.
- Exactly 24 logical operations remain: ten succeeded and fourteen denied.
- Exactly ten provider attempts move USD 10 total. No unexplained reservations
  remain, the budget is not exceeded and the evidence verifier passes.

The HTTP test harness now stops its child server before deleting the temporary
store, including on Windows. Generated development credentials stay in that store
and are never printed or included in this report.

## Validation

The implementation is in `crates/cosigna-agent/tests/limits.rs`. Checks run locally:

```text
cargo test -p cosigna-agent --test limits -- --nocapture
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python -I -W error::ResourceWarning tools/evidence-verifier/test_verify_evidence.py
node scripts/check-source-secrets.mjs
```

The targeted HTTP suite passed 7/7. Formatting and Clippy passed. The complete Rust
workspace passed 350 tests with no failures and two ignored child-process helper
entry points (invoked by their parent crash tests). The independent Python verifier
suite passed 5/5. The source credential-pattern scan passed over 651 tracked text
files. These results were obtained locally on Windows, not through hosted CI.

The Python suite exercises its own synthetic evidence scenarios; the two new HTTP
scenarios check the built-in Rust verifier, not a newly exported independent
verification of those exact histories.

## Limits and remaining work

Fixture rail and temporary loopback stores only. No paid inference, external
payment calls, customer records or live funds. These are correctness regressions,
not a throughput benchmark, long soak, disk-full test or production certification.
They do not demonstrate 1,000-10,000 requests per second. Current-main capacity
qualification remains #85. Keep #111 open until this follow-up is reviewed and
merged and its remaining acceptance criteria are explicitly reconciled.

The next foundation item is #112: a trusted workflow assignment must constrain
which otherwise-valid case/payment an agent may select and how much it may request.
The optional private-data/model-egress path remains disabled and subject to the
design decisions in #110/#123.

## Update, 2 November 2026: merged with the audit fixes

Main now includes #126 and #128. Two things changed for these tests:

- **Shared slots:** a stalled body now holds its slot for at most 10 seconds, not the full
  request timeout. The injected failure returns at about five seconds, so the three stalled
  agents still fill the shared pool while the owner's request runs.
- **A flaky test exposed a gateway bug:** the concurrent test failed intermittently with a
  connection abort (WSAECONNABORTED), both before and after the merge. The test's assertions
  were left unchanged and the gateway was fixed. A request refused before processing (`busy` or
  `rate_limited`) was answered without reading its body, so closing the connection could reset
  it and lose the answer. The client then saw an uncertain network failure for work that never
  started. The gateway now reads and discards that body for up to one second, holding no slot.
  A body arriving later than that can still produce a reset; the client must treat such a
  failure as uncertain and replay the same idempotency key.

Measured on Windows only, on loopback: the two overload tests failed 9 of 15 runs at `3c734f5`,
11 of 15 after merging main, and 0 of 20 with the fix. The new test that delays a POST body
fails 6 of 6 on the old server and passes 6 of 6 with the fix. This is not a capacity result,
and the reset's frequency on other platforms was not measured.

Separately, the test harness set the budget's and agent's expiry from two clock reads, so a
second boundary between them could make setup fail. They now share one value.

Filling the shared slots also had a race: a status probe could overtake a stalled request and
take its slot, leaving only two filled. Setup now adds a stalled request for as long as a probe
gets through.

With these changes the `limits` test file passed 69 of 70 runs, including every overload test.
The one failure was the older `people_can_still_act_when_agents_fill_the_gateway`, which relies
on a fixed 500 ms wait; it is unchanged here. In an earlier 70-run batch, before the last setup
fix, one overload test hit a connection reset (WSAECONNRESET) once.
It did not recur and its cause is unknown. POST errors in the harness now name the path.
