#!/usr/bin/env bash
# Install or verify the toolchain for Krunk for Android. Idempotent: every step checks first and
# does nothing when the pinned version is already there. Versions come from android/toolchain.lock.
#
#   android/scripts/setup-sdk.sh                      SDK packages + Rust targets + cargo-ndk
#   android/scripts/setup-sdk.sh --with-local-emulator   also the emulator, a system image and an AVD
#   android/scripts/setup-sdk.sh --check               verify only, exit 1 if anything is missing
#
# Needs: ANDROID_HOME (or ANDROID_SDK_ROOT) with cmdline-tools, a JDK >= 17 on PATH, rustup/cargo.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ANDROID_DIR=$(cd "$HERE/.." && pwd)
LOCK="$ANDROID_DIR/toolchain.lock"
# shellcheck disable=SC1090
source "$LOCK"

WITH_EMULATOR=0
CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --with-local-emulator) WITH_EMULATOR=1 ;;
    --check) CHECK_ONLY=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [ -z "$SDK" ]; then
  echo "ANDROID_HOME is not set" >&2
  exit 1
fi
SDKMANAGER="$SDK/cmdline-tools/latest/bin/sdkmanager"
AVDMANAGER="$SDK/cmdline-tools/latest/bin/avdmanager"
[ -x "$SDKMANAGER" ] || { echo "sdkmanager not found at $SDKMANAGER (install cmdline-tools first)" >&2; exit 1; }

missing=0
say() { printf '%s\n' "$*"; }
need() { # need <label> <present:0|1>
  if [ "$2" = 1 ]; then say "  ok      $1"; else say "  MISSING $1"; missing=1; fi
}

# ---------- JDK ----------
say "JDK"
java_major=$(java -version 2>&1 | awk -F'"' '/version/ {split($2, v, "."); print (v[1] == 1 ? v[2] : v[1])}')
if [ -n "$java_major" ] && [ "$java_major" -ge "$JDK_MIN" ]; then
  need "java $java_major (>= $JDK_MIN)" 1
else
  need "java >= $JDK_MIN (found: ${java_major:-none})" 1
  echo "  install a JDK >= $JDK_MIN and put it first on PATH" >&2
  exit 1
fi

# ---------- Android SDK packages ----------
say "Android SDK at $SDK"
sdk_pkg_present() { # sdk_pkg_present <package id> -> 0 if its directory exists
  local dir="${1//;//}"
  [ -d "$SDK/$dir" ]
}
want_pkgs=("$PLATFORM_PACKAGE" "build-tools;$BUILD_TOOLS" "ndk;$NDK" "platform-tools")
if [ "$WITH_EMULATOR" = 1 ]; then
  want_pkgs+=("emulator" "$SYSTEM_IMAGE")
fi
to_install=()
for p in "${want_pkgs[@]}"; do
  if sdk_pkg_present "$p"; then
    say "  ok      $p"
  else
    say "  install $p"
    to_install+=("$p")
  fi
done
if [ "${#to_install[@]}" -gt 0 ]; then
  if [ "$CHECK_ONLY" = 1 ]; then
    missing=1
  else
    # Accept the SDK licences non-interactively (the same text the IDE shows).
    yes | "$SDKMANAGER" --licenses > /dev/null 2>&1 || true
    yes | "$SDKMANAGER" --install "${to_install[@]}"
    for p in "${to_install[@]}"; do
      sdk_pkg_present "$p" || { echo "  failed to install $p" >&2; exit 1; }
    done
  fi
fi
if [ -f "$SDK/ndk/$NDK/source.properties" ]; then
  say "  ndk     $(grep Pkg.Revision "$SDK/ndk/$NDK/source.properties" | cut -d= -f2 | tr -d ' ')"
fi

# ---------- Rust ----------
say "Rust"
command -v rustup > /dev/null || { echo "rustup not found" >&2; exit 1; }
installed_targets=$(rustup target list --installed)
IFS=',' read -r -a targets <<< "$RUST_TARGETS"
for t in "${targets[@]}"; do
  if grep -qx "$t" <<< "$installed_targets"; then
    say "  ok      target $t"
  elif [ "$CHECK_ONLY" = 1 ]; then
    need "target $t" 0
  else
    say "  install target $t"
    rustup target add "$t"
  fi
done
have_ndk=$(cargo ndk --version 2> /dev/null | awk '{print $2}' || true)
if [ "$have_ndk" = "$CARGO_NDK" ]; then
  say "  ok      cargo-ndk $CARGO_NDK"
elif [ "$CHECK_ONLY" = 1 ]; then
  need "cargo-ndk $CARGO_NDK (found: ${have_ndk:-none})" 0
else
  say "  install cargo-ndk $CARGO_NDK (found: ${have_ndk:-none})"
  cargo install --locked -j4 "cargo-ndk@$CARGO_NDK"
fi
# uniffi-bindgen is built from the workspace (ffi/src/bin/uniffi-bindgen.rs, feature "cli"), so the
# bindings generator always matches the uniffi crate version in Cargo.lock. Nothing to install.
say "  ok      uniffi $UNIFFI (bindgen built from ffi/, feature cli)"

# ---------- Local emulator (optional) ----------
if [ "$WITH_EMULATOR" = 1 ]; then
  say "Local emulator"
  if [ ! -e /dev/kvm ]; then
    say "  WARNING /dev/kvm is missing: the local emulator can only run with software emulation (-accel off)."
    say "          Expect a boot of 10-30 minutes. emu.sh prefers an emulator that is already attached to adb."
  fi
  if "$AVDMANAGER" list avd -c 2> /dev/null | grep -qx "$AVD_NAME"; then
    say "  ok      AVD $AVD_NAME"
  elif [ "$CHECK_ONLY" = 1 ]; then
    need "AVD $AVD_NAME" 0
  else
    say "  create  AVD $AVD_NAME ($SYSTEM_IMAGE, $AVD_DEVICE)"
    echo no | "$AVDMANAGER" create avd -n "$AVD_NAME" -k "$SYSTEM_IMAGE" -d "$AVD_DEVICE" --force > /dev/null
    cfg="$HOME/.android/avd/$AVD_NAME.avd/config.ini"
    if [ -f "$cfg" ]; then
      # Phone-sized, 2 GB RAM, no snapshots: enough for the debug APK and screenshots.
      sed -i 's/^hw.ramSize=.*/hw.ramSize=2048/' "$cfg"
      grep -q '^hw.ramSize=' "$cfg" || echo 'hw.ramSize=2048' >> "$cfg"
      echo 'disk.dataPartition.size=4G' >> "$cfg"
    fi
  fi
fi

# ---------- local.properties for Gradle / IDEs ----------
LP="$ANDROID_DIR/local.properties"
if [ "$CHECK_ONLY" = 0 ] && ! grep -qs "^sdk.dir=$SDK$" "$LP" 2> /dev/null; then
  printf 'sdk.dir=%s\n' "$SDK" > "$LP"
  say "wrote $LP"
fi

if [ "$missing" = 1 ]; then
  say "Toolchain incomplete."
  exit 1
fi
say "Toolchain complete: platform $PLATFORM, build-tools $BUILD_TOOLS, NDK $NDK, Rust targets $RUST_TARGETS, cargo-ndk $CARGO_NDK."
