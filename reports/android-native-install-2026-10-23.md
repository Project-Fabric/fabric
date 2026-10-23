# Installed Android build — 23 October 2026

Built and installed the actual Cosigna Android APK on the Google Play-enabled
`Medium_Phone_API_37.0` emulator (Android 17, `emulator-5554`). Application ID
`com.cosigna.preview`, version `0.1.0`, minimum SDK 24, target/compile SDK 36.
Artifact: `mobile/dist/native/cosigna-test.apk` (65,166,953 bytes), ARM64 and x86_64.

## Build changes

- Added a separate publisher-checksum-verified Temurin 21 build toolchain. Studio's
  bundled Java 25 produced a restricted-method error during CMake configuration.
  Build selection and first-use installation are handled by the PowerShell scripts.
- Gradle installs missing SDK components only using already accepted SDK licenses.
  SDK Platform 36, NDK 27.1.12297006 and CMake 3.22.1 are now installed.
- A maintained Expo config plugin puts native app build intermediates in a short,
  per-project `%USERPROFILE%/.cxx` path and sets CMake object-path hashing to 250.
  This fixes the observed Ninja filename-over-260 error. The failing object is now
  235 characters. Generated native source is still owned by config/plugins.
- Gradle heap/metaspace increased to 3 GB/1 GB with four workers; release bundling
  uses `NODE_ENV=production`. This changes build configuration, not transaction authority.

## Verification

- Native Gradle release build: **successful**, 507 tasks, including native C++ for both
  architectures, application Kotlin/Java, manifest merge, release lint, signing and APK packaging.
- `apksigner verify`: passed. Signing remains the generated development key for internal
  testing, not a production/store signing setup.
- APK inspected with `aapt`: label Cosigna, expected package/SDKs and both CPU architectures.
  Embedded `assets/index.android.bundle` present (2,323,652 bytes).
- ADB install: successful. Cold launch rendered the connected order/pairing screen while
  no Metro process listened on 8081. This verifies a self-contained installed frontend.
- Direct native assistant-activity invocation preserved the exact request
  `Order one black coffee` and displayed it as an unapproved draft.
- A separate `I confirm` invocation with an `approved=true` extra remained only a draft.
  The actual Android Button for continuing was disabled while unpaired. This checks the
  native draft boundary, not physical speech or production payment authentication.
- Sampled app logs showed the React Native app starting and no fatal application exception.
- Mobile typecheck/lint: passed. Native configuration tests: **3 passed**. All PowerShell
  scripts parsed successfully. Rust fmt/clippy and workspace tests: **195 passed**.

## Runtime and remaining validation

Opened the native project in Android Studio and the managed test launcher in an interactive
PowerShell window. The old Groq backend had stopped during the interruption; its key was
not recovered from process memory or written to files. The launcher requests a key privately
when it needs to start the hosted backend. Server readiness still needs checking after entry.

Only the emulator was attached at final installation; the Nokia was disconnected. Its
installation and fresh physical microphone run remain pending. The user confirmed Play
Console access. Studio/device same-account login, app package test registration, compatible
Assistant plugin and Google's account-bound App Actions preview still need completion.
Direct Android intent testing is not proof that the Assistant wake-word/invocation works.

No purchase, live merchant order or Stripe sandbox payment was submitted in this native
verification. Existing API/schema, budget, exact-quote approval and reconciliation semantics
were unchanged. The current native handoff still requires an explicit Continue action and
processing consent. Phone-only use without PC/USB remains a separate secure hosted deployment.

Instructions: [Android Studio setup](../mobile/ANDROID_STUDIO.md).
