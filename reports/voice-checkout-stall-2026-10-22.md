# Nokia voice checkout stall — 2026-10-22

The connected Nokia was displaying “What would you like to order? Speak after I finish.”
with no recording control. Its Android log repeatedly reported
`TextToSpeech: stop failed: not bound to TTS engine`. The installed Google speech package
had `enabled=3` (disabled by user). Re-enabled only `com.google.android.tts` for user 0;
Android then resolved its TTS service. Restarted Expo Go to discard the stale native
speech instance. Re-pairing is required because the mobile session lives only in memory.

The app previously waited indefinitely for native speech callbacks. It now checks for
an English voice, bounds initialization/start to eight seconds and bounds completed
readout separately. Missing callbacks stop the voice session with recovery instructions;
they never arm approval. Late callbacks from a stopped session cannot resume capture.
Listening guidance appears after the speech completion callback. Native speech-stop
rejections during cleanup no longer become unhandled rejections.

Automatic recording has an independent 12-second JavaScript completion fallback and
handles metering exceptions. Neither missing metering nor a timer can satisfy the
microphone-activity requirement for payment confirmation.

Validation: Android 64 tests passed; mobile typecheck and lint passed. Rust workspace
fmt, clippy and all 195 tests passed. iOS: 63 passed, one existing Android-specific skip.
Android JavaScript/Hermes export passed with these fixes. After restarting Expo Go,
the Nokia loaded the preview home screen successfully; USB forwards remain present.
No API, storage, approval digest or payment authorization schema changed. No purchase
was submitted by the agent. Physical speech through to a Stripe sandbox payment still
requires the user to retry after re-pairing.
