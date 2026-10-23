# Android application integration decisions

The installed-app implementation reuses the existing cross-platform UI and Rust spending
boundary. Android Studio is the native build/debug environment, not a separate UI rewrite.

Still needed for end-to-end validation:

- Toolchain setup is now complete: Platform 36, NDK/CMake and a separate build JDK 21 are
  installed. The Google Play AVD `Medium_Phone_API_37.0` runs Android 17. A signed internal
  APK has been built, installed and launched there. The Nokia was disconnected at the
  final installation check and still needs the APK installed when reconnected.
- The user confirmed Play Console access on 23 October. App package registration, same-account
  Studio/device login, compatible Assistant plugin and preview setup still need validation.
  No app publication or account registration has been completed by the agent.
- For a phone-only experience: hosting account/domain and production identity design.
  The current one-command PC test runtime is not an internet backend.
- For a real coffee purchase: target city, consenting merchant and actual ordering/payment
  integration access. Google Places discovery does not grant merchant checkout capability.

No API keys, payment credentials or office/home address should be written here.
