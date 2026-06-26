# SpiritChat — mobile

The active app: a decentralized **P2P, end-to-end encrypted** messenger for Android
(iOS later). React Native + Expo frontend, with the security-critical core (libp2p +
crypto + key storage) to be added as a native module. **No cloud servers.**

This starts as a minimal build skeleton. Work is layered incrementally:

1. **Skeleton + APK pipeline** (current) — minimal Expo app that builds to an Android
   APK via GitHub Actions. Proves the build/distribution pipeline end to end.
2. **Reused UI** — port screens/components from `legacy/spiritchat/apps/mobile`
   (Liquid Glass design, chat/conversations/contacts/profile/settings, QR), stripped
   of all cloud (Supabase/API) coupling.
3. **Native core** — Rust + libp2p for P2P transport, identity keypair, and E2E
   encryption, exposed to JS as a native module. Requires an Expo **development build**
   (Expo Go cannot load custom native code).

## Build

CI builds a debug APK on every push (`.github/workflows/android-build.yml`) and uploads
it as the `app-debug-apk` artifact — no expo.dev cloud required.

Locally (needs Android SDK + JDK 17):

```bash
npm install
npx expo prebuild --platform android
cd android && ./gradlew assembleDebug
# APK: android/app/build/outputs/apk/debug/app-debug.apk
```

The generated `android/` (and `ios/`) folders are not committed (Continuous Native
Generation) — they are recreated by `expo prebuild`.
