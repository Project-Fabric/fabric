# Installed Android app preparation — 22 October 2026

Update: [23 October native build and emulator installation](android-native-install-2026-10-23.md)
records the now-successful APK build, toolchain fixes and remaining Assistant validation.

User requested an Android Studio/emulator app with Google Assistant entry and a smooth
test startup, retaining Cosigna intelligence and transaction enforcement.

Implemented:

- A regenerable native Android entry activity and GET_THING App Actions capability.
  Request text becomes a bounded draft in the existing app; it cannot supply credentials,
  limits or payment approval. Unsupported generic order/commute invocation is not claimed.
- The draft can be continued through a deliberate voice-session action after pairing,
  audio/planner consent and active allowance. It uses Cosigna's existing constrained Groq
  planner and Rust approval/execution path. A fresh post-quote spoken turn still approves.
- Expo SecureStore persistence for the scoped expiring connection only, with schema
  `cosigna.device-session/v1`, restore/server validation, expiry/version handling and revoke
  cleanup. Audio/provider permission and approval do not persist. Native Android backup is
  disabled. No server/API/payment/ledger schema changed.
- Native APK build script selecting a bundled release UI, internally signed using the
  generated development key. The native build opens the connected commerce screen.
- One test launcher for APK build/reuse, named AVD boot/reuse, installation, forwarding,
  managed/reused Groq/Stripe test backend and app launch. It refuses incompatible existing
  services, never uninstalls to resolve a signing failure, and only stops a backend it owns.
- Setup, assistant-account preview instructions and remaining decisions in
  `mobile/ANDROID_STUDIO.md` and `mobile/QUESTIONS.md`.

Validation:

| Check | Result |
| --- | --- |
| TypeScript / mobile lint | Passed |
| Full Android suite | 87 passed |
| Full iOS suite | 86 passed, one existing Android-specific skip |
| Native configuration tests | 2 passed |
| Backend suite, including runtime compatibility boundary | 37 passed |
| Android prebuild | Passed twice; generated activity, capability metadata, SecureStore and backup/network config inspected |
| Android JS/Hermes export, installed-app mode | Passed |
| PowerShell parser checks | Passed |
| cargo fmt --all --check | Passed |
| cargo clippy --workspace --all-targets --target-dir target/fixture-status-check -- -D warnings | Passed |
| cargo test --workspace --target-dir target/fixture-status-check --quiet | 195 passed |

The actual APK build initially stopped at the missing-JDK prerequisite. Android Studio's
bundled OpenJDK 25 was detected later; Android SDK Platform 36, emulator and command-line
tools were still absent. A second build attempt passed JDK discovery and stopped at the
explicit missing-Platform-36 check. Android Studio's Setup Wizard was open at the final
inspection. No APK has been compiled or installed, no emulator run completed,
and the Google Assistant preview/account/Play registration has not been validated. No live
merchant order or new Stripe payment was submitted by the agent during this stage.

The single-command PC runtime is local development orchestration, not a deployed backend.
Removing PC/USB dependence needs authenticated HTTPS hosting and production identity.
Location/route selection, external café access, live payment setup and all-native hands-free
assistant fulfillment remain separate work. The public intent deliberately does not start
recording on its own; the current handoff has one explicit Continue action and renewed
processing consents. Tests and JS export do not establish native compilation, OS voice
matching, physical microphone behavior or end-to-end hands-free payment.
