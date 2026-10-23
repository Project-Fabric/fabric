# Cosigna: first Android voice purchase test

This milestone connects the Expo app to the existing local Rust transaction service and
independent test merchant. It does **not** integrate Google Assistant/Gemini, Swiggy or Zomato.
No real money or physical orders. The phone uses the computer's models/payment integration.

## Before connecting the Nokia

- Check Settings > About phone > Android version. SDK 57 supports Android 7+; actual handset
  compatibility, performance and microphone capture have not been verified.
- Install Android Studio's **Android SDK Platform-Tools** through SDK Manager > SDK Tools.
  ADB/Android SDK and a JDK were not found in PATH/the usual SDK locations during this build.
- On the phone, enable Developer options (tap Build number seven times), then USB debugging.
- Use an Expo Go release compatible with **SDK 57**. On Android older compatible Expo Go
  versions are available through https://expo.dev/go. A latest-store SDK mismatch does not
  mean our project should be upgraded blindly.

## 1. Start the isolated phone lab on the computer

```powershell
cd C:\Users\Harsha\Pictures\project\testbeds\voice-merchant
npm run dev:mobile:stripe
```

This uses **3097/3098** and the independent `mobile-stripe-v1` profile. Existing browser labs
on 3093–3096 are left running. The launcher uses the existing Stripe test secret, if present,
and rejects live keys. `npm run dev:mobile` instead uses simulated acceptance with no Stripe
account. These commands use the offline fixture planner, not an AI model.

For microphone transcription, set the server key **before** starting the lab:

```powershell
$env:COSIGNA_OPENAI_API_KEY = [System.Net.NetworkCredential]::new('', (Read-Host 'OpenAI API key' -AsSecureString)).Password
npm run dev:mobile:stripe
```

Do not start a second copy on the same ports. If switching modes, stop only that phone-lab
terminal with Ctrl+C first; keep its profile intact. Audio is a paid hosted operation when
configured. Do not put this key into Expo, an EXPO_PUBLIC variable, a QR code or the phone.
Use `npm run dev:mobile:ai` for hosted natural-language planning plus Stripe sandbox; this
requires that server key and separate planning consent in the app. Without a key, typed
fixture examples work but the recording control is disabled.

### Groq: voice and planning without OpenAI API credits

Create a Groq API key at https://console.groq.com/keys. Stop the existing phone lab with
Ctrl+C; keep Expo running. In the same `testbeds/voice-merchant` terminal:

```powershell
$env:COSIGNA_GROQ_API_KEY = [System.Net.NetworkCredential]::new('', (Read-Host 'Groq API key' -AsSecureString)).Password
npm run dev:mobile:groq
```

This explicitly selects `whisper-large-v3-turbo` for audio and `openai/gpt-oss-120b` for
planning, hosted by Groq. The `openai/` model prefix does not use OpenAI billing. The server
reports Groq at startup. The phone's two consent switches must name Groq. The key stays in
the computer terminal environment; do not add it to Expo configuration. Groq's free plan is
quota limited; availability and accuracy still need testing with the actual account/voice.

Before testing the phone, an optional live check can run in a separate terminal with that
key configured:

```powershell
npm run check:groq
```

On Windows this generates synthetic speech for “Order one oat latte”, sends it to Groq,
uses the Rust Groq planner for a quote, and approves exactly one $7 Stripe sandbox payment
as the test driver. It checks replay and over-budget blocking and writes a report in `reports`.
It consumes real Groq quota, creates only Stripe test objects, uses separate temporary ports
and data, and does not test the physical microphone. On other platforms, pass a short recording
with `npm run check:groq -- --audio /path/to/order.wav`. Failed checks preserve their temporary
profile; inspect unresolved outcomes before rerunning.

Open **http://127.0.0.1:3097** in the computer browser. This is the merchant's staff counter
and the place to issue/revoke phone pairing codes.

## 2. Connect by USB and start Expo

### Alternative: Expo Go over trusted Wi-Fi without USB

On the computer and Android phone, use the same trusted Wi-Fi network. From `mobile`, run
`npm run start:wifi`. The launcher selects the computer's private IPv4 address (or accepts
`-WifiAddress` when invoked as a PowerShell script), asks privately for a Groq server key
if needed, and starts/reuses the matching Groq/Stripe sandbox backend. It starts an opt-in
relay bound only to that private address on port 3099 and Expo Go/Metro on port 8081.

Scan Expo's QR code from Expo Go, or enter `exp://<computer-wifi-ip>:8081`. Choose **Open
connected order lab**. On the computer, visit `http://127.0.0.1:3097`, create a six-digit
pairing code and enter it in Cosigna. The screen must show **WI-FI DEVELOPMENT LAB** and
the computer's Wi-Fi address. Set a test allowance and allow Groq transcription, planning
and device speech before starting voice checkout. Test payment confirmation still requires
a fresh post-quote response. Do not create a replacement purchase after an unknown result.

Only the paired native `/mobile/v3/` routes are relayed. The staff browser, merchant API,
Rust service, role credentials and model/payment keys remain on loopback. Requests from
browser origins/cookies, unexpected Host values, other subnets, arbitrary paths and oversized
bodies are refused. The relay has no automatic retries or redirects. A minimal `/health`
endpoint can be opened on the phone to check reachability. Node must be allowed through
Windows Firewall on the current trusted network; do not disable the firewall.

This is unencrypted, paired **sandbox development over a trusted LAN**, not a public/mobile
production deployment. The relay validates a matching Stripe-test backend before starting.
No tunnel is used. The computer's public IP is never accepted as a relay endpoint. Only its
nonsecret LAN URL is passed to Expo; model/payment secrets are removed from the Metro child
environment. The native APK retains its loopback-only cleartext policy; this flow uses Expo
Go. Close/reopen Expo Go if its native speech engine becomes unavailable. Ctrl+C closes
Expo/the relay and only a backend that this launcher started; existing reused backends remain.

### USB connection

Connect the phone, unlock it, and accept its USB debugging prompt. In a second terminal:

```powershell
cd C:\Users\Harsha\Pictures\project\mobile
powershell -ExecutionPolicy Bypass -File .\scripts\connect-android.ps1
npm run start:usb
```

The helper finds ADB in PATH or `%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe`.
For a custom installation, pass `-AdbPath 'C:\...\adb.exe'`. It refuses ambiguous/unauthorized
devices; `-DeviceSerial` selects a specific authorized handset. It forwards **3097** (lab)
and **8081** (Metro) from phone loopback to computer loopback. It does not expose a LAN
listener or disable the firewall. Services must remain running and the cable connected.

In Expo Go, open `exp://127.0.0.1:8081` (or scan the localhost QR from the terminal).
Choose **Open connected order lab** on the Cosigna home screen. In the computer browser,
click **Create one-use pairing code** and enter the six-digit code in the phone.
The code expires after five minutes/five invalid attempts. Successful pairing replaces the
previous phone; access expires after twelve hours. Code/token values are not saved to disk.
The six-digit code now uses mobile API version 3 with provider-bound permissions. After updating,
restart the phone-lab server, reload Expo Go, refresh the computer page and create a fresh code.

## 3. Complete one test purchase

### Voice checkout

In hosted Groq or OpenAI mode, enable audio transcription, AI planning and spoken results,
and set an active allowance. Tap **Start voice checkout**. The app reads an existing quote
or clarification if one is open; otherwise it asks what to order.

Speak after the prompt. Each turn uploads automatically after measured speech followed by
about 1.2 seconds of quiet, or at the 12-second capture limit. The microphone is paused while
the device reads the next question or complete quote. The initial request can include
“confirm my order”, but it does not authorize that quote: after the full quote is read,
say **Confirm my order** as a new turn, or **Cancel**. Generic “yes”, negated confirmation,
changes to the order, missing measured microphone activity, stale quotes or backgrounding
do not approve a payment. Use manual review if the device cannot detect audio activity.

The same paired development owner/session calls the unchanged exact-digest approval endpoint.
This is a local test voice intent, not speaker identity, voice biometrics or a production
payment authorization. ASR and microphone activity can be wrong. Test merchant/Stripe sandbox
restrictions still apply. The transcript is processed by the explicitly selected provider;
starting this mode enables automatic audio/plan processing only for the current session.

**Stop voice checkout** stops the conversation. It cannot undo an already submitted payment.
A lost submission response locks new purchases until current task status is refreshed.
Order confirmation is separate from staff marking a test pickup ready.

### Manual recording and approval

1. Read the data-sharing notice. Set a $20 test allowance and $10 per-order cap.
2. Allow hosted audio transcription if configured. Press **Speak your order** and say
   **Order one oat latte**. Native capture stops after 20 seconds; it does not auto-upload.
3. Press **Stop and transcribe** (or **Transcribe recording** after the 20-second limit).
   Check/edit the resulting text. In fixture mode, punctuation
   or additional wording may need removal to match exactly; the example buttons also work.
4. Press **Find a quote**. Review item, seller, all-in price, pickup terms and quote expiry.
5. Press **Review this quote for approval**, then **Approve and place test order**.
   This button is the manual approval path. Voice checkout uses the separate fresh spoken
   confirmation flow above; the planner has no approval tool or authority field.
6. Confirm one $7 Stripe **test** PaymentIntent ID on the phone and the corresponding order
   on the computer's staff counter. Verify that same ID in Stripe test Dashboard.
7. Staff marks the test order ready; refresh/check status on the phone to see pickup readiness.

Repeatedly checking status must not charge again. A timeout must retain the task/reservation
until lookup establishes an outcome. Do not create a replacement order while one is uncertain.
Use **Recover a previous request** after re-pairing. The scoped twelve-hour phone connection
is now saved encrypted and checked when reopening the app; consent and quote approval are
not saved. Server restart/revoke/expiry still requires re-pairing. Task history stays on the
computer. For an installed APK with one managed test launcher, see
[Android Studio setup](../mobile/ANDROID_STUDIO.md).

### If voice does not transcribe

If **Start voice checkout** stays on the spoken prompt without opening the microphone,
check Android's **Settings > Text-to-speech output**. Speech Services by Google must be
enabled and have an English voice available. On the Nokia, that app was disabled by the
user setting; Android logged `stop failed: not bound to TTS engine`. It was re-enabled
for testing. Fully close/reopen Expo Go after enabling the engine and pair again.
Voice checkout now stops with guidance if speech does not start within eight seconds,
or if a started prompt never finishes. A timeout never authorizes payment. Wait until
the quote has finished and the microphone says **Listening** before saying **I confirm**
or **confirm my order**. These are exact commands for the reviewed quote; additions,
conditions and negations do not approve it. Siri and real route-based merchant ordering
are the [next pilot](VOICE_COMMUTE_PILOT.md), not capabilities of this USB lab.
Automatic recording also finishes after 12 seconds even if Android omits its completion
event; confirmation still requires measured microphone activity.

The recording control now explains missing transcription consent or server configuration
beside the microphone. Transcription can run before setting an allowance; quoting and payment
still require an active allowance. Allow microphone access in Expo Go when prompted.

In manual recording mode, after speaking tap **Stop and transcribe**. If native capture reaches its 20-second limit,
tap **Transcribe recording**; audio is not uploaded automatically. Recording, file and upload
errors appear beside this control. A missing recording is an error rather than a silent return.

Provider failures show safe guidance for rejected API keys (401), project permissions (403),
unavailable transcription models (404), and rate/quota limits (429). These identify the active
provider; Groq failures do not fall back to OpenAI. No provider body or key is
returned. Key presence alone does not establish API access or available credits. Restart the
lab in the same terminal after changing its key; the phone must re-pair after a restart.

### Answering a clarification

The test café currently offers oat latte, black coffee and butter croissant; black tea is
not in its catalog. In **needs input**, use the **Your answer** field and **Send answer**,
or **Speak your answer**. Voice checkout reads the question and listens for a reply.
Replies include the original request and the merchant question in a new planning request;
they do not authorize a purchase. The earlier request remains in the ledger for recovery.

## Privacy and authority limits

- One trusted development phone shares the lab owner's tasks/allowance; this is **not**
  production user login, multi-user isolation, verified identity or cryptographic delegation.
- Phone receives a scoped lab session, not merchant, model, Stripe or Rust owner/executor keys.
  It cannot use the merchant staff fulfillment endpoint. Pair only your own trusted device.
- Audio transcription and planning have separate provider-bound, per-request server consent gates.
  Manual transcription fills an editable text field. Voice checkout explicitly enables automatic
  transcription/planning per turn and a fresh post-quote voice confirmation. Fixture planning sends no request to a model. Hosted
  planning sends the request/catalog to the explicitly selected OpenAI or Groq service. No voiceprint, contacts or location is used.

- Server does not save audio. The app attempts to delete its cache recording after transcription,
  discard, backgrounding or leaving the screen. Force termination/cleanup failures may leave
  a cache file; clear app cache to remove it. No zero-retention/provider-training guarantee.
- Submitted text, quotes and payment evidence remain in the computer's existing transaction
  ledger. Record deletion/export controls are not implemented. Resetting example tasks or
  disconnecting the phone does not erase this evidence.
- **Revoke test allowance** blocks future spending on that allowance; already submitted or
  unknown payments still require status reconciliation. It does not undo accepted orders.
- **Disconnect phone** or **Revoke phone access** on the computer invalidates new calls.
  Already-authorized in-flight requests/uploads can finish; backgrounding cannot unsend
  an upload that already started. Restart also invalidates phone access.
- USB loopback HTTP is a developer transport, not internet deployment. A native config plugin
  permits cleartext only for localhost/127.0.0.1. Remote/mobile-network use needs authenticated
  HTTPS and a production identity design; do not tunnel these development ports publicly.

## Native build and remaining device checks

The Android project was generated with Expo prebuild and inspected for microphone permission,
blocked location/contacts and loopback-only network security. JavaScript exports are **not APKs**.
The initial build did not include APK compilation, signing, installation or Google Assistant testing.
The user has since reported working Groq microphone transcription and manual black-coffee
checkout on the Nokia. The new voice confirmation flow still needs a physical-device run.
After installing Android Studio/JDK/SDK, `npm run android -- --device` builds/installs locally.
Cloud APK profile instructions remain in `mobile/README.md`; no cloud build/account was created.

On the Nokia check: mic denial and retry, record/discard, background during capture/approval,
app kill/re-pair/task recovery, USB loss after approval, repeated taps, changed/expired quote,
over-budget refusal, allowance revocation and staff-ready updates. Run the first phone test
with sandbox only. iOS source/bundle support does not establish physical iPhone connectivity.
