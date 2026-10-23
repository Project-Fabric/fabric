# Groq integration readiness — 2026-10-22

Implemented an explicit Groq option for the existing voice merchant/phone lab:

- Rust `--planner groq`, default `openai/gpt-oss-120b`, optional `openai/gpt-oss-20b`.
  Same constrained JSON plan, parser, budget/quote/approval/execution boundary.
- `COSIGNA_GROQ_API_KEY` is read only on the computer. Rust and speech use fixed Groq HTTPS
  destinations with bounded responses, timeouts and redirect rejection. No provider fallback.
- Whisper Large V3 Turbo transcription; errors identify the selected provider and safe
  key/access/rate-limit guidance without exposing provider response bodies.
- `dev:mobile:groq`, `dev:stripe:groq`, `dev:groq`; separate audio/planning opt-ins naming Groq.
  Native consent headers bind the provider. Existing audio cannot be sent to a changed provider.
- Browser envelope and paired mobile API now v3, with `ai_provider` in state. Six-digit
  pairing remains, with five-minute expiry/five attempts. Restart, reload and re-pair required.
  No merchant, agent task or SQLite format changes; existing test profiles remain in use.
- `npm run check:groq`: optional actual Groq speech/planning → exact test-driver $7 approval
  → Stripe sandbox. Generates synthetic speech on Windows or accepts `--audio <file>`.
  Reports replay/over-budget checks; failed checks preserve isolated state for reconciliation.

Validation completed:

| Check | Result |
| --- | --- |
| Android Jest | 45 passed |
| iOS Jest | 44 passed, 1 existing Android-specific test skipped |
| Node lab tests | 35 passed |
| Mobile typecheck and lint | Passed |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --target-dir target/fixture-status-check -- -D warnings` | Passed |
| `cargo test --workspace --target-dir target/fixture-status-check` | Passed |
| Diagnostic script syntax | Passed |
| Groq real-Rust startup with synthetic key and no inference | Passed |
| `check:groq` without a key | Correctly refused before network/build/profile creation |

The existing duplicate fabric/cosigna main target warning remains. The separate target
directory avoids locking the user's running development binaries on Windows.

No real Groq key was supplied to the agent process, so no successful hosted Groq transcription
or planning is claimed. No new Stripe API payment was made during this implementation;
automated payment tests use fake Stripe. Physical microphone/transcript quality and complete
phone checkout require the live key/device run. Existing dependency audit issues and
production identity/merchant limits described in the phone guide remain unresolved.

Instructions: [phone guide](../docs/PHONE_VOICE_TEST.md),
[testbed README](../testbeds/voice-merchant/README.md).

Provider references: [speech](https://console.groq.com/docs/speech-to-text),
[Responses](https://console.groq.com/docs/responses-api),
[structured output](https://console.groq.com/docs/structured-outputs),
[limits](https://console.groq.com/docs/rate-limits).
