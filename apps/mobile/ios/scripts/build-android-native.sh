#!/usr/bin/env bash
#
# Builds the Rust crypto core for Android and generates the Kotlin that
# calls into it, placing both where the native module's build.gradle
# expects them.
#
# This is the Android equivalent of what Xcode does for the iOS half of
# the module, and it exists as a script rather than a Gradle task for one
# reason: it is slow and it is not always needed. A Rust release build of
# this crate for two ABIs takes minutes; the JavaScript above it changes
# many times an hour. Wiring it into every Gradle run would tax every
# rebuild for a dependency that changes rarely.
#
# Run it after changing anything under `packages/`, and on a fresh
# checkout. Both outputs are build products and are not checked in.
#
#   ANDROID_NDK_HOME=... apps/mobile/ios/scripts/build-android-native.sh
#
# The commands mirror the ones the Compose app's workflow has been using
# successfully, deliberately — this is not a second, differently-broken
# way to build the same library.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../../../.." && pwd)"
ffi_crate="$repo_root/packages/crypto-core-ffi"
module_android="$repo_root/apps/mobile/ios/modules/spiritchat-crypto-core/android"

jni_libs="$module_android/generated/jniLibs"
kotlin_out="$module_android/generated/java"

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  echo "ANDROID_NDK_HOME is not set — cargo-ndk needs it to find the toolchain." >&2
  echo "On a machine with Android Studio it is usually ~/Library/Android/sdk/ndk/<version>" >&2
  exit 1
fi

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk is missing. Install it with:" >&2
  echo "  cargo install cargo-ndk --locked" >&2
  exit 1
fi

echo "==> Cross-compiling the crypto core (arm64-v8a, x86_64)"
cd "$ffi_crate"
# arm64-v8a is every real phone; x86_64 is the emulator, which is the only
# way most people will ever run this on a desktop.
cargo ndk -t arm64-v8a -t x86_64 -o "$jni_libs" build --release --lib

echo "==> Generating Kotlin bindings"
cargo build --bin uniffi-bindgen
rm -rf "$ffi_crate/bindgen-out"
# Generating from the built library rather than the .udl keeps the
# bindings honest: they describe what was actually compiled.
"./target/debug/uniffi-bindgen" generate \
  --library "$jni_libs/arm64-v8a/libspiritchat_crypto_core_ffi.so" \
  --language kotlin \
  --out-dir bindgen-out \
  --no-format

rm -rf "$kotlin_out/uniffi"
mkdir -p "$kotlin_out/uniffi"
cp -r "$ffi_crate/bindgen-out/uniffi/spiritchat_crypto_core_ffi" "$kotlin_out/uniffi/"

echo
echo "Done."
echo "  native libraries: $jni_libs"
echo "  kotlin bindings:  $kotlin_out/uniffi/spiritchat_crypto_core_ffi"
