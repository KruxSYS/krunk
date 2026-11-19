#!/usr/bin/env bash
# Builds the shared core for the APK and generates its Kotlin bindings:
#   1. host build of krunk_ffi (libkrunk_ffi.so for Linux): uniffi-bindgen reads the exported
#      API from it, and the JVM unit tests load it through JNA
#   2. Kotlin bindings  -> android/app/build/rust/<profile>/kotlin/org/krunk/ffi/krunk_ffi.kt
#   3. cargo-ndk builds -> android/app/build/rust/<profile>/jniLibs/{arm64-v8a,armeabi-v7a,x86_64}/libkrunk_ffi.so
#
# Gradle runs this (tasks cargoBuildDebug / cargoBuildRelease) before compiling; by hand:
#   android/scripts/build-rust.sh [debug|release] [jniLibs out dir] [kotlin out dir]
# Environment: KRUNK_ABIS (default arm64-v8a,armeabi-v7a,x86_64), CARGO_BUILD_JOBS (default 4),
#              ANDROID_HOME, and the NDK version from android/toolchain.lock.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ANDROID_DIR=$(cd "$HERE/.." && pwd)
SRC_DIR=$(cd "$ANDROID_DIR/.." && pwd)
# shellcheck disable=SC1091
source "$ANDROID_DIR/toolchain.lock"

PROFILE="${1:-debug}"
case "$PROFILE" in
  debug) CARGO_PROFILE_FLAG=() ;;
  release) CARGO_PROFILE_FLAG=(--release) ;;
  *) echo "usage: build-rust.sh [debug|release]" >&2; exit 2 ;;
esac
ABIS="${KRUNK_ABIS:-arm64-v8a,armeabi-v7a,x86_64}"
JOBS="${CARGO_BUILD_JOBS:-4}"
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/android-sdk}}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$SDK/ndk/$NDK}"
[ -d "$ANDROID_NDK_HOME" ] || { echo "NDK $NDK not found at $ANDROID_NDK_HOME (run android/scripts/setup-sdk.sh)" >&2; exit 1; }

JNI_OUT="${2:-$ANDROID_DIR/app/build/rust/$PROFILE/jniLibs}"
KT_OUT="${3:-$ANDROID_DIR/app/build/rust/$PROFILE/kotlin}"
TARGET_DIR="${CARGO_TARGET_DIR:-$SRC_DIR/target}"

cd "$SRC_DIR"

# The host library is only read by uniffi-bindgen (and the JVM tests): always the debug build,
# because the workspace's release profile strips it and the UniFFI metadata with it.
echo "[build-rust] host library (debug, for the bindings generator)"
cargo build --locked -j"$JOBS" -p krunk_ffi
HOST_LIB="$TARGET_DIR/debug/libkrunk_ffi.so"
[ -f "$HOST_LIB" ] || { echo "host library missing: $HOST_LIB" >&2; exit 1; }

echo "[build-rust] Kotlin bindings -> $KT_OUT"
# Both are build output folders handed over by Gradle; never delete anything else.
for out in "$KT_OUT" "$JNI_OUT"; do
  [[ $out == */build/* ]] || { echo "refusing to clear $out: not inside a build folder" >&2; exit 2; }
done
rm -rf "$KT_OUT" "$JNI_OUT"
mkdir -p "$KT_OUT" "$JNI_OUT"
cargo run --locked -j"$JOBS" -q -p krunk_ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language kotlin --out-dir "$KT_OUT"
[ -f "$KT_OUT/org/krunk/ffi/krunk_ffi.kt" ] || { echo "bindings were not generated" >&2; exit 1; }

echo "[build-rust] Android libraries ($ABIS, $PROFILE, minSdk $MIN_SDK) -> $JNI_OUT"
TARGET_FLAGS=()
IFS=',' read -r -a abi_list <<< "$ABIS"
for abi in "${abi_list[@]}"; do
  TARGET_FLAGS+=(--target "$abi")
done
# OpenVPN 3 Core (vpn/openvpn) runs bindgen on its C header. cargo-ndk passes the sysroot through
# BINDGEN_EXTRA_CLANG_ARGS_<triple_with_underscores> but bindgen's unversioned --target then fails
# in NDK 30's sys/cdefs.h ("Unversioned target triples are not supported"). bindgen reads the
# hyphenated form first and cargo-ndk leaves it alone: give it a versioned target (Android 0.10.0
# spike S1, .pipeline/NOTES-android-0.10.0.md).
SYSROOT="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
BINDGEN_ENV=()
for triple in aarch64-linux-android x86_64-linux-android; do
  BINDGEN_ENV+=("BINDGEN_EXTRA_CLANG_ARGS_$triple=--target=$triple$MIN_SDK --sysroot=$SYSROOT -I$SYSROOT/usr/include/$triple")
done
# 32-bit ARM (0.12.0, Android TV devices such as the Chromecast with Google TV): Rust says
# armv7-linux-androideabi, clang armv7a-linux-androideabi<api>, the sysroot arm-linux-androideabi.
BINDGEN_ENV+=("BINDGEN_EXTRA_CLANG_ARGS_armv7-linux-androideabi=--target=armv7a-linux-androideabi$MIN_SDK --sysroot=$SYSROOT -I$SYSROOT/usr/include/arm-linux-androideabi")
env "${BINDGEN_ENV[@]}" cargo ndk "${TARGET_FLAGS[@]}" --platform "$MIN_SDK" --output-dir "$JNI_OUT" \
  build --locked -j"$JOBS" -p krunk_ffi "${CARGO_PROFILE_FLAG[@]}"
# Debug builds carry ~130 MB of DWARF per ABI, which AGP does not strip for debug variants: strip
# here (the host library keeps its symbols for backtraces in JVM tests). Release builds are
# already stripped by the workspace profile.
STRIP="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
for abi in "${abi_list[@]}"; do
  f="$JNI_OUT/$abi/libkrunk_ffi.so"
  [ -f "$f" ] || { echo "missing $f" >&2; exit 1; }
  if [ -x "$STRIP" ]; then "$STRIP" --strip-unneeded "$f"; fi
  printf '  %-10s %s\n' "$abi" "$(du -h "$f" | cut -f1)"
done
echo "[build-rust] done"
