# Installed Cosigna Android test application

This stage keeps the shared React Native UI and existing Rust transaction service, and
generates a native Android Studio project. A bundled APK does not need Expo Go or Metro.
The local test backend still needs to run: one launcher manages it and the device connection.
It is not a deployed, phone-only service yet. All purchases remain test merchant/Stripe sandbox.

## One-time Android Studio setup

1. Install [Android Studio](https://developer.android.com/studio) and complete its setup wizard.
   The normal Windows installation includes its own JDK. Current Studio bundles Java 25,
   which hit a restricted-method failure in this app's native CMake build. The build script
   automatically installs/selects a separate checksum-verified Temurin JDK 21 under
   `%LOCALAPPDATA%\Cosigna\toolchains`; it does not change Studio's bundled runtime.
2. In **SDK Manager**, install Android SDK Platform **36**, Android SDK Build-Tools,
   Android SDK Platform-Tools, Android SDK Command-line Tools (latest), and Android Emulator.
   Accept/review the required SDK licenses in Android Studio. Gradle may request additional
   NDK/CMake components used by React Native; install the exact versions it reports.
   Gradle can also install the required components automatically after their SDK licenses
   have been accepted in Android Studio. The build script permits that path.
   The generated build configuration uses a shorter per-project native cache in
   `%USERPROFILE%\.cxx` and CMake object-path hashing to avoid Windows filename limits.
   Gradle uses a 3 GB heap, 1 GB metaspace and at most four workers for this native build.
3. In **Device Manager**, create a Pixel virtual phone using a **Google Play-enabled**
   x86_64 system image. Choose a model/image marked with Google Play. Name the AVD
   `Cosigna_API_36`, or use its actual AVD name below. An AOSP-only image cannot establish
   the Google Assistant/Google-account integration. Physical Nokia testing remains supported.
4. Sign in to Google Play/Google app and set up Assistant if that image/device supports it.
   Emulator voice/account features vary; use the Nokia to validate actual wake-word behavior.

## One command for local testing

From the repository root:

```powershell
.\mobile\scripts\start-android.ps1 -AvdName Cosigna_API_36
```

The launcher builds the APK when missing, starts/reuses the named AVD, waits for boot,
installs without uninstalling, forwards port 3097, starts/reuses the matching Groq/Stripe
test backend, opens Cosigna and opens the private desktop pairing page. A Groq key is
requested through a hidden terminal prompt only if a new hosted backend needs it.
The existing local Stripe test credential is required; no live key is accepted.
Provider/payment/merchant keys stay on the computer, never in the APK.

If exactly one emulator/physical phone is already connected:

```powershell
.\mobile\scripts\start-android.ps1
```

Use `-DeviceSerial emulator-5554` or the actual phone serial when several devices are present.
This computer's existing Google Play AVD is named `Medium_Phone_API_37.0`; it can be used
instead of creating `Cosigna_API_36`:

```powershell
.\mobile\scripts\start-android.ps1 -AvdName Medium_Phone_API_37.0
```

Use `-Rebuild` after code/native configuration changes. Without it, an existing APK is reused.
`-Fixtures` selects typed offline planning with Stripe sandbox; it does not provide speech.
The launcher refuses a backend with different mode/identity rather than stopping another
process. Ctrl+C stops only a backend that this launcher started. If reusing an existing backend,
leave that original terminal running. No task/payment is retried on shutdown/restart.

Pair using the six-digit code from the desktop page. The scoped connection is saved encrypted
for its existing twelve-hour lifetime. Reopening the app restores and checks it; expiry,
revocation, replacement or server restart requires re-pairing. Processing permissions remain
per screen/session, and quote approval never persists. Task history remains server-side.

## Work directly in Android Studio

Generated project: `C:\Users\Harsha\Pictures\project\mobile\android`.
Use **Open** on that folder and let Gradle sync. Native changes belong in app configuration
or `mobile/plugins`, not hand edits to generated files. Regenerate with:

```powershell
cd C:\Users\Harsha\Pictures\project\mobile
npx expo prebuild --platform android --no-install
```

For builds launched inside Studio, set **Settings > Build, Execution, Deployment >
Build Tools > Gradle > Gradle JDK** to the same JDK 21 installed by the launcher
(on this machine: `%LOCALAPPDATA%\Cosigna\toolchains\jdk-21.0.12.1+1`). The standalone
launcher selects it automatically. Keep the IDE's own bundled runtime unchanged.

For a self-contained APK, choose the **release** build variant or run `npm run android:build`.
The generated release variant currently uses the development signing key for local testing;
it is not a production signing setup. Output: `mobile\dist\native\cosigna-test.apk`.
The regular debug variant still expects Metro. The script sets
`EXPO_PUBLIC_COSIGNA_NATIVE_APP=1` while bundling so the installed app opens the connected
commerce screen instead of the fictional shopping/travel preview.

### Device speech engine

The Android build applies a pinned compatibility patch to `expo-speech` 57.0.3 through
`scripts/patch-expo-speech.cjs` (also run by `npm install`). That release destroys its
cached TTS engine when an Activity is destroyed, although the React Native module can
remain alive. Later voice lookups then reject with “Unable to get voices” / “not bound
to TTS engine”. The patch releases the engine when its owning module is destroyed.
Recheck/remove the patch when upgrading Expo Speech; changed versions/source stop the build.
This native fix requires rebuilding/reinstalling the APK and cannot change Expo Go's binary.

If speech is unavailable, close/reopen Cosigna and check **Android Settings > System >
Languages > Text-to-speech output**. Enable Google's speech engine and install/select an
English voice; the system's **Play** sample should work. Cosigna stops voice checkout when
speech fails or times out. It listens for payment confirmation only after the complete
quote finishes speaking; failure never substitutes for a spoken review.

### Emulator microphone and repeated questions

Android recording permission and Windows microphone access do not by themselves route
the computer's microphone into the emulator. Enable **Extended controls (⋮) > Microphone >
Enable Host Microphone Access** (older versions call it **Virtual microphone uses host audio
input**). Alternatively, explicitly enable it when starting this local test:

```powershell
.\mobile\scripts\start-android.ps1 -AvdName Medium_Phone_API_37.0 -EnableHostMicrophone
```

Windows **Settings > System > Sound > Input** should show your microphone level moving.
Allow microphone/desktop app access in Windows privacy settings and recording permission
in Android. Headphones help keep Cosigna's spoken prompt out of the microphone.
Cosigna shows a signal indicator while listening, rejects automatic turns with no detected
input before uploading, and pauses if the same clarification remains unanswered (or after
three different clarification questions). A signal is an audio-level check, not proof of
speech or identity. Start again or edit the request manually after fixing input.

## Google Assistant handoff

The config plugin generates `cosigna_shortcuts.xml` and `CosignaAssistantActivity.kt`.
It implements **GET_THING** for searching an offer through Cosigna, mapping `thing.name`
to a bounded editable request. This is the currently documented initial capability:
“Hey Google, search for black coffee on Cosigna.” The specific “order coffee on my way
to office” invocation, route discovery and real merchant checkout are not implemented here.

Google Assistant performs wake-word/request routing. It does not switch its own model or
payment system to Cosigna. The native activity forwards request text into Cosigna; continuing
it uses the existing Groq planner and Rust quote/budget/approval/execution pipeline. It never
uses Google Pay as a substitute for that authorization layer.

To exercise actual Assistant routing, follow Google's
[App Actions setup](https://developer.android.com/develop/devices/assistant/get-started)
and [test codelab](https://codelabs.developers.google.com/codelabs/appactions-2):

1. Configure Play Console/app test registration as required by Google and install the
   compatible Google Assistant plugin in Android Studio.
2. Use the same Google account for the Google app/test device, Android Studio and Play Console.
3. Open **Tools > Google Assistant > App Actions Test Tool**, create the account-bound preview,
   choose `actions.intent.GET_THING`, set `thing.name` to `Order one black coffee`, and run it.
4. Cosigna must show the matching request as an unapproved draft. Pair/allow processing if
   needed, then tap **Continue assistant request by voice**. This explicit action starts
   microphone use; a public link/intent alone never opens the microphone or pays.
5. Cosigna plans the request, reads the exact quote and listens for a fresh **I confirm**.
   Verify a single Stripe sandbox PaymentIntent and one merchant order. Check status without
   creating another purchase. Test missing text, negation, backgrounding, expiry and USB loss.

Generating an APK or sending a direct Android intent is not proof of a successful Google
Assistant voice invocation. Google controls account/device/locale eligibility and preview/review.
No Play Console app, preview or publishing step has been created automatically.

## Remaining product work

Normal phone-only use needs an authenticated HTTPS deployment, durable production identity,
device/session revocation and payment-method setup. This single-owner loopback lab must not
be publicly tunneled. Route/location permission and merchant ordering access are separate
work; see [the real coffee pilot](../docs/VOICE_COMMUTE_PILOT.md).
