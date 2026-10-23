# Spoken checkout and clarification replies — 2026-10-22

User reports Groq transcription works and black coffee can be ordered manually. The current
test catalog contains black coffee, oat latte and butter croissant, not black tea. Its real
needs-input response asked whether black coffee should replace black tea; the mobile card
had no inline reply control.

Implemented:

- Your answer, Send answer and Speak your answer below a clarification. Hosted replies include
  the saved original request, merchant question and user answer in a new bounded task request.
  Offline mode instead accepts a complete supported fixture request. No approval is copied.
- Optional Start voice checkout after separate provider audio/planning and device speech
  opt-ins, with an active allowance. It can continue an existing question/quote or start a
  new spoken request. Manual ordering/review remains available.
- Automatic capture starts after the spoken prompt and a 500ms audio-tail delay. Measured
  activity followed by a 1.2-second pause ends a turn; native capture also stops at 12 seconds.
  The microphone is paused during quote/question readout. Each turn sends audio to the
  selected provider using the existing provider-bound consent header.
- Exact confirm-my-order command recognized only as a fresh turn after the complete quote
  readout. Requires current quote ID/digest/expiry, active allowance, foreground app and
  measured audio activity. Consumes confirmation before the request. Ambiguous/negated/
  changed responses cannot approve. Cancellation is distinct and cannot undo submitted orders.
- Parent submission recovery lock is set before the unchanged /approve call. Lost responses
  never automatically retry; budget reservations/reconciliation remain controlled by Rust.
- Cached recording URI and guarded cleanup avoid reading an already released Expo shared
  recorder during unmount. The actual Nokia log exposed an uncaught release error during
  reload; a regression test now simulates that ordering. Later sampled logs showed no new
  ReactNativeJS errors, but this is not a complete physical voice checkout validation.

Validation:

| Check | Result |
| --- | --- |
| Android full suite | 58 passed |
| iOS full suite | 57 passed, 1 existing Android-specific skip |
| Latest inline-answer adjustment | 9 Android UI tests passed |
| Mobile lint/typecheck | Passed |
| Android JS/Hermes export | Passed before final cleanup/fixture-copy adjustments |
| cargo fmt --all --check | Passed |
| cargo clippy --workspace --all-targets --target-dir target/fixture-status-check -- -D warnings | Passed |
| cargo test --workspace --target-dir target/fixture-status-check --quiet | Passed |

The optional voice flow uses the same trusted, single-owner paired test client. ASR and sound
level checks are not speaker identity or production payment authentication. The ledger does
not attest whether approval came from a physical owner speaking or tapping. No API envelope,
database, receipt or approval-body format changed. The new flow has not been exercised with
the user's physical speech through to a sandbox payment; no new payment was submitted by
the agent during this change. Native silence thresholds, speaker routing and interruptions
still need the Nokia run. The Groq phone backend remains running on 3097; Expo remains on 8081.

Instructions: [phone guide](../docs/PHONE_VOICE_TEST.md).
