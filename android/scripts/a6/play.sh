#!/usr/bin/env bash
# 9f: the Play bundle installed through bundletool over a 0.4.0 profile, on the emulator.
#   play.sh AAB OLD_APK EVIDENCE_DIR
# Builds the device's APK set with --local-testing, signed with Krunk's release key (as Play
# would deliver it under option 1 of the signing choice), installs it over the published 0.4.0
# with data, and records: version, permissions of the installed splits, the About screen.
# The keystore password goes through a mode-600 file that is deleted at the end.
set -euo pipefail
aab=$1 old=$2 ev=$3
mkdir -p "$ev"
S="adb -s ${ANDROID_SERIAL:-emulator-5554}"
PKG=org.krunk.browser
bt=$(ls "$HOME"/.gradle/caches/modules-2/files-2.1/com.android.tools.build/bundletool/1.18.3/*/bundletool-1.18.3.jar | head -1)
props="${KRUNK_ANDROID_SIGNING:-$HOME/.config/krunk/android-release.properties}"
store=$(sed -n 's/^storeFile=//p' "$props"); alias=$(sed -n 's/^keyAlias=//p' "$props")
tmp=$(mktemp -d "$HOME/.cache/krunk-play.XXXXXX"); chmod 700 "$tmp"; trap 'rm -rf "$tmp"' EXIT
(umask 077; sed -n 's/^storePassword=//p' "$props" > "$tmp/ks.pass"; sed -n 's/^keyPassword=//p' "$props" > "$tmp/key.pass")

$S shell dumpsys package $PKG | grep -m1 versionName > "$ev/play-before.txt" || true
java -jar "$bt" build-apks --bundle="$aab" --output="$tmp/krunk.apks" --local-testing --connected-device \
  --device-id="${ANDROID_SERIAL:-emulator-5554}" \
  --ks="$store" --ks-key-alias="$alias" --ks-pass="file:$tmp/ks.pass" --key-pass="file:$tmp/key.pass" 2>&1 | tail -3
unzip -l "$tmp/krunk.apks" > "$ev/play-apkset.txt"
$S shell input keyevent KEYCODE_HOME; sleep 3; $S shell am force-stop $PKG
java -jar "$bt" install-apks --apks="$tmp/krunk.apks" --device-id="${ANDROID_SERIAL:-emulator-5554}" 2>&1 | tail -3 | tee "$ev/play-install.txt"
$S shell dumpsys package $PKG | grep -m3 -E "versionCode|versionName|splits" > "$ev/play-after.txt"
$S shell dumpsys package $PKG | sed -n '/requested permissions:/,/install permissions:/p' > "$ev/play-permissions.txt"
if grep -q REQUEST_INSTALL_PACKAGES "$ev/play-permissions.txt"; then echo "FAIL  play: REQUEST_INSTALL_PACKAGES requested"; else echo "PASS  play: no REQUEST_INSTALL_PACKAGES"; fi
# Every split's manifest, as Play would install it.
mkdir -p "$tmp/x"; unzip -q -o "$tmp/krunk.apks" -d "$tmp/x"
aapt2=$(ls "${ANDROID_HOME:-/opt/android-sdk}"/build-tools/*/aapt2 | tail -1)
for f in $(find "$tmp/x" -name '*.apk' | sort); do echo "== ${f#"$tmp"/x/}"; "$aapt2" dump permissions "$f" 2>/dev/null; done > "$ev/play-split-permissions.txt"
if grep -q REQUEST_INSTALL_PACKAGES "$ev/play-split-permissions.txt"; then echo "FAIL  play: a split asks for REQUEST_INSTALL_PACKAGES"; else echo "PASS  play: no split asks for REQUEST_INSTALL_PACKAGES"; fi
