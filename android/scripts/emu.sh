#!/usr/bin/env bash
# Drive the Android emulator for Krunk for Android.
#
#   emu.sh start            use the emulator already attached to adb (the sandbox provides one), or
#                           boot the local AVD from toolchain.lock headless (software emulation when
#                           /dev/kvm is missing) and wait for sys.boot_completed
#   emu.sh stop             stop the local emulator started by this script (never the shared one)
#   emu.sh wait             wait until the device has booted
#   emu.sh serial           print the adb serial in use
#   emu.sh shot <name>      screenshot to $KRUNK_SHOTS/<name>.png (default .pipeline/screenshots/android)
#   emu.sh install [apk]    install the debug APK (default: the universal debug APK)
#   emu.sh uninstall        remove org.krunk.browser
#   emu.sh launch [url]     start Krunk (optionally with a URL)
#   emu.sh logcat [extra]   Krunk + Gecko logcat (add "-d" to dump and exit)
#   emu.sh adb <args...>    adb against the selected device
#
# Device selection: $ANDROID_SERIAL if set, else the first "emulator-*" that is online. Other attached
# devices (the sandbox also has an offline TV) are ignored.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ANDROID_DIR=$(cd "$HERE/.." && pwd)
SRC_DIR=$(cd "$ANDROID_DIR/.." && pwd)
# shellcheck disable=SC1091
source "$ANDROID_DIR/toolchain.lock"

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/android-sdk}}"
ADB="${ADB:-$SDK/platform-tools/adb}"
[ -x "$ADB" ] || ADB=adb
APP_ID="${APPLICATION_ID:-org.krunk.browser}"
SHOTS="${KRUNK_SHOTS:-$SRC_DIR/../.pipeline/screenshots/android}"
PIDFILE="${TMPDIR:-/tmp}/krunk-emu-$AVD_NAME.pid"
DEFAULT_APK="$ANDROID_DIR/app/build/outputs/apk/debug/app-universal-debug.apk"

pick_serial() {
  if [ -n "${ANDROID_SERIAL:-}" ]; then
    echo "$ANDROID_SERIAL"
    return
  fi
  "$ADB" devices 2> /dev/null | awk '$1 ~ /^emulator-/ && $2 == "device" {print $1; exit}'
}

adbx() { # adb against the selected device
  local serial
  serial=$(pick_serial)
  [ -n "$serial" ] || { echo "no online emulator attached to adb (run: emu.sh start)" >&2; exit 1; }
  "$ADB" -s "$serial" "$@"
}

wait_boot() {
  local serial="$1" deadline=$(( $(date +%s) + ${BOOT_TIMEOUT:-2400} ))
  echo "waiting for $serial to boot (timeout ${BOOT_TIMEOUT:-2400} s)..."
  until [ "$("$ADB" -s "$serial" shell getprop sys.boot_completed 2> /dev/null | tr -d '\r')" = "1" ]; do
    if [ "$(date +%s)" -ge "$deadline" ]; then
      echo "boot timed out" >&2
      return 1
    fi
    sleep 5
  done
  # The launcher needs a moment after boot_completed before input works.
  sleep 3
  echo "booted: $("$ADB" -s "$serial" shell getprop ro.build.version.release | tr -d '\r') (API $("$ADB" -s "$serial" shell getprop ro.build.version.sdk | tr -d '\r'))"
}

cmd_start() {
  local serial
  serial=$(pick_serial)
  if [ -n "$serial" ]; then
    echo "using the emulator already attached to adb: $serial"
    wait_boot "$serial"
    return
  fi
  local emulator="$SDK/emulator/emulator"
  [ -x "$emulator" ] || { echo "no attached emulator and no local emulator installed (setup-sdk.sh --with-local-emulator)" >&2; exit 1; }
  "$SDK/cmdline-tools/latest/bin/avdmanager" list avd -c | grep -qx "$AVD_NAME" || { echo "AVD $AVD_NAME missing (setup-sdk.sh --with-local-emulator)" >&2; exit 1; }
  local accel=()
  if [ ! -e /dev/kvm ]; then
    echo "WARNING: /dev/kvm is missing: starting with software emulation (-accel off). Boot takes 10-30 minutes."
    accel=(-accel off)
  fi
  mkdir -p "$SHOTS"
  nohup "$emulator" -avd "$AVD_NAME" -no-window -no-audio -no-boot-anim -no-snapshot \
    -gpu swiftshader_indirect -memory 2048 -cores 2 "${accel[@]}" \
    > "${TMPDIR:-/tmp}/krunk-emu-$AVD_NAME.log" 2>&1 &
  echo $! > "$PIDFILE"
  echo "emulator pid $(cat "$PIDFILE"), log ${TMPDIR:-/tmp}/krunk-emu-$AVD_NAME.log"
  # Wait for adb to see it.
  local tries=0
  until serial=$(pick_serial); [ -n "$serial" ]; do
    tries=$((tries + 1))
    [ "$tries" -lt 120 ] || { echo "the emulator never appeared in adb devices" >&2; exit 1; }
    sleep 5
  done
  wait_boot "$serial"
}

cmd_stop() {
  if [ -f "$PIDFILE" ]; then
    local pid
    pid=$(cat "$PIDFILE")
    if kill -0 "$pid" 2> /dev/null; then
      adbx emu kill || kill "$pid" || true
      echo "stopped local emulator (pid $pid)"
    fi
    rm -f "$PIDFILE"
  else
    echo "no local emulator was started by this script; the shared emulator is left running"
  fi
}

cmd_shot() {
  local name="${1:?usage: emu.sh shot <name>}"
  mkdir -p "$SHOTS"
  adbx exec-out screencap -p > "$SHOTS/$name.png"
  echo "$SHOTS/$name.png"
}

cmd_install() {
  local apk="${1:-$DEFAULT_APK}"
  [ -f "$apk" ] || { echo "APK not found: $apk (run ./gradlew assembleDebug in android/)" >&2; exit 1; }
  adbx install -r -t "$apk"
}

cmd_launch() {
  if [ -n "${1:-}" ]; then
    adbx shell am start -W -a android.intent.action.VIEW -d "$1" "$APP_ID"
  else
    adbx shell am start -W -n "$APP_ID/.MainActivity"
  fi
}

cmd_logcat() {
  adbx logcat -v time "$@" '*:S' 'Krunk:V' 'KrunkFfi:V' 'GeckoView:I' 'GeckoViewActivity:V' 'GeckoConsole:I' 'Gecko:W' 'GeckoThread:I' 'AndroidRuntime:E' 'System.err:W' 'ActivityManager:I'
}

case "${1:-}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  wait) wait_boot "$(pick_serial)" ;;
  serial) pick_serial ;;
  shot) shift; cmd_shot "$@" ;;
  install) shift; cmd_install "$@" ;;
  uninstall) adbx uninstall "$APP_ID" ;;
  launch) shift; cmd_launch "$@" ;;
  logcat) shift; cmd_logcat "$@" ;;
  adb) shift; adbx "$@" ;;
  *) sed -n '2,20p' "$0"; exit 2 ;;
esac
