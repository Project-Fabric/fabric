# Phone voice lab readiness — 2026-10-22

Implemented the Expo connected order screen and single-owner USB development API, with
one-use pairing, revocation, explicit hosted-transcription/planner consent, editable
transcript, test allowance creation/revocation, quote review/approval and durable task lookup.
The existing Rust transaction ledger remains authoritative. No Rust code/schema was changed.

## Checks performed

- Lab Node tests: **31 passed**, including actual local HTTP/Rust/merchant boundaries and a
  fake Stripe transport for the new paired-phone checkout tests. No actual new Stripe charge
  was made in this work. The earlier real sandbox protocol check remains separate evidence.
- Mobile Android full suite: **34 passed** before the last unknown-task recovery test was
  added. The updated order-lab suite then passed all **5** tests (including that new test).
  Together the current Android suite has **35** verified cases.
- Mobile iOS full suite: **33 passed**, one Android-specific test skipped, before that same
  last test; the updated iOS order-lab suite then passed all **5** tests. Current coverage is
  **34** verified iOS cases plus one platform skip. Native modules are mocked in these tests.
- `npm run typecheck`, `npm run lint`: passed after final source/test changes.
- Expo Doctor: **21/21 passed** after installing audio's required `expo-asset` peer dependency.
- Android and iOS JS/Hermes exports: passed. These are not APK/IPA files.
- Expo Android prebuild: passed; generated manifest/network XML inspected for microphone,
  location/contacts removal, no background audio service and localhost-only HTTP exception.
- PowerShell USB helper: syntax parsed successfully; invoking it failed with the intended
  actionable **ADB not found** message on this machine. No actual USB device was connected.
- Rust workspace: **194 tests passed**; `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets --target-dir target/fixture-status-check -- -D warnings`
  passed. Workspace tests used the same alternate target directory to avoid existing server
  executable locks. The pre-existing shared cosigna/fabric binary manifest warning remains.
- `git diff --check`: passed for tracked changes. Existing unrelated work was preserved.

## Running development service

Started `npm run dev:mobile:stripe` with the isolated `mobile-stripe-v1` data profile.
Browser UI/Node server: `http://127.0.0.1:3097`; Rust agent: loopback port 3098.
HTTP smoke: page/state 200, browser schema `cosigna.voice-lab/v2`, payment mode `stripe-test`,
fixture planner, zero orders, unpaired mobile state 401. Model keys/tokens were not printed.
The service reported **voice unavailable** because no transcription key was configured.
Existing services on 3093–3096 were not stopped or changed.

## Remaining gates

Install/find Android SDK Platform-Tools for USB forwarding. An Android SDK/JDK was not found
in the usual checked locations, so no APK build/signing/install was attempted. Follow
[the setup/device checklist](../docs/PHONE_VOICE_TEST.md) when the Nokia is available.

Actual microphone capture/permission behavior, physical rendering, device speech and
phone-to-server forwarding remain unverified. Hosted speech/model quality was not tested.
Source/bundle tests do not establish physical iOS connectivity. Google Assistant/Gemini,
Swiggy/Zomato, live-funds checkout, production identity/delegated grants and personal-data
deletion/export controls are not implemented by this milestone.

Dependency install reports **57 audit advisories (8 moderate, 49 high)** in the existing
Expo/React Native tree. Expo Doctor validates compatibility, not vulnerability remediation.
Audio cleanup is best effort; force termination may leave device cache audio. Submitted
text and payment records stay in the local ledger; disconnecting does not delete them.
