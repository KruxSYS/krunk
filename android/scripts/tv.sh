#!/usr/bin/env bash
# Krunk for Android on Android TV (0.12.0): the remote-control checks.
#
#   tv.sh gate                 badging of the release APKs (leanback, banner, armeabi-v7a, no
#                              required feature) and that the release dex has no TV override
#   tv.sh phone APK NAME       phone smoke on a phone emulator: launch, load a page, Settings
#                              (screenshots NAME-*.png; run it for 0.11.1 and for 0.12.0)
#   tv.sh tv APK               the D-pad pass (acceptance checks 2-9): a phone emulator switched to
#                              1920x1080 @ 320 dpi, landscape, with a qa/debug build forced into TV
#                              mode (debug.krunk.force_tv 1); keyevents only. Restores the display
#                              on exit (the emulator is shared).
#   tv.sh launcher APK         an Android TV AVD (TCG, slow): install, the launcher row with the
#                              banner, the leanback entry and a start without a crash (check 1)
#
# Environment: ANDROID_SERIAL (default emulator-5554; the TV AVD: emulator-5580, local adb),
# EV (screenshots, default .pipeline/evidence/a12), HOST_URL (the test pages, default
# http://10.250.136.3:18080; the local TV AVD reaches this machine as http://10.0.2.2:18080).
# The pages are android/scripts/tv/*.html, served by `python3 -I android/scripts/tv/server.py
# 18080 > server.log`; SERVER_LOG (default /tmp/tv-server.log) is read to confirm loads and clicks.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SRC=$(cd "$HERE/../.." && pwd)
EV="${EV:-$SRC/../.pipeline/evidence/a12}"
HOST_URL="${HOST_URL:-http://10.250.136.3:18080}"
SERVER_LOG="${SERVER_LOG:-/tmp/tv-server.log}"
export ANDROID_SERIAL="${ANDROID_SERIAL:-emulator-5554}"
PKG=org.krunk.browser
SDK="${ANDROID_HOME:-/opt/android-sdk}"
AAPT2=$(ls -d "$SDK"/build-tools/*/aapt2 | sort -V | tail -1)
mkdir -p "$EV"

ok=0 bad=0
pass() { echo "PASS $*"; ok=$((ok + 1)); }
fail() { echo "FAIL $*"; bad=$((bad + 1)); }
summary() { echo "== $ok passed, $bad failed"; [ "$bad" = 0 ]; }

key() { for k in "$@"; do adb shell input keyevent "$k"; sleep "${KEY_PAUSE:-0.6}"; done; }
times() { local n=$1 k=$2; for _ in $(seq "$n"); do key "$k"; done; }
shot() {
  # The emulator console's screenshot when there is one (screencap is black on the TCG AVD).
  if [ "${CONSOLE_SHOT:-0}" = 1 ]; then adb emu screenrecord screenshot "$EV/$1.png" >/dev/null; else adb exec-out screencap -p > "$EV/$1.png"; fi
  echo "  shot $1.png"
}
top() { adb shell dumpsys activity activities | sed -n 's/.*topResumedActivity=ActivityRecord{[^ ]* [^ ]* \([^ ]*\).*/\1/p' | head -1; }
mark() { wc -l < "$SERVER_LOG"; }
# wait_req MARK PATTERN [seconds]: a request line after MARK in the server log.
wait_req() {
  local from=$1 pat=$2 n=${3:-40}
  for _ in $(seq "$n"); do
    if tail -n +"$((from + 1))" "$SERVER_LOG" | grep -q "$pat"; then return 0; fi
    sleep 1
  done
  return 1
}
dump() { adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 && adb shell cat /sdcard/ui.xml; }
# The focused node's text: its own, else the texts and descriptions inside it ("" if none).
focused() { dump | python3 -I -c '
import sys, xml.etree.ElementTree as ET
try:
    root = ET.fromstring(sys.stdin.read())
except ET.ParseError:
    sys.exit()
for n in root.iter("node"):
    if n.get("focused") == "true":
        words = [m.get("text") or m.get("content-desc") for m in n.iter("node")]
        print(" ".join(w.strip() for w in words if w and w.strip())[:80] or n.get("class"))
'; }
# focus_to PATTERN KEY [max]: presses KEY until the focused node's text matches PATTERN.
focus_to() {
  local pat=$1 k=$2 n=${3:-15}
  for _ in $(seq "$n"); do
    if focused | grep -qiE "$pat"; then return 0; fi
    key "$k"
  done
  focused | grep -qiE "$pat"
}
# tap_text TEXT: taps the centre of the first node with that text or description (phone smoke only).
tap_text() { dump | python3 -I -c '
import re, sys
want = sys.argv[1]
for n in re.finditer(r"<node [^>]*>", sys.stdin.read()):
    s = n.group(0)
    if f"text=\"{want}\"" in s or f"content-desc=\"{want}\"" in s:
        a, b, c, d = map(int, re.search(r"bounds=\"\[(\d+),(\d+)\]\[(\d+),(\d+)\]\"", s).groups())
        print((a + c) // 2, (b + d) // 2)
        break
' "$1" | { read -r x y && adb shell input tap "$x" "$y"; }; }
fresh_install() {
  adb shell am force-stop "$PKG" || true
  adb uninstall "$PKG" >/dev/null 2>&1 || true
  adb install "$1" | tail -1
}
crash_check() {
  local log
  log=$(adb logcat -d -b crash,main 2>/dev/null | grep -E "FATAL EXCEPTION|ANR in $PKG|Fatal signal" | grep -E "$PKG|krunk" || true)
  if [ -z "$log" ]; then pass "9 no FATAL/ANR for $PKG in logcat"; else fail "9 logcat: $log"; fi
}

cmd=${1:-}; shift || true
case "$cmd" in
gate)
  dir="$SRC/android/app/build/outputs/apk/direct/release"
  for apk in "$dir"/*-universal-release.apk "$SRC"/android/app/build/outputs/apk/play/release/*-universal-release.apk; do
    [ -f "$apk" ] || { fail "missing $apk"; continue; }
    b=$("$AAPT2" dump badging "$apk")
    grep -q "^leanback-launchable-activity: name='$PKG.MainActivity'" <<<"$b" && pass "$(basename "$apk"): leanback-launchable-activity" || fail "$(basename "$apk"): no leanback activity"
    grep -q "^application: .* banner='" <<<"$b" && pass "banner" || fail "no banner"
    grep -q "^native-code: 'arm64-v8a' 'armeabi-v7a' 'x86_64'" <<<"$b" && pass "native-code with armeabi-v7a" || fail "native-code: $(grep native-code <<<"$b")"
    req=$(grep -E "^ *uses-feature: |^ *uses-implied-feature: " <<<"$b" | grep -v "glEsVersion" || true)
    [ -z "$req" ] && pass "no required feature but GLES" || fail "required: $req"
    # The TV override lives in debug/qa builds only: R8 drops it (BuildConfig.TV_OVERRIDE false).
    tmp=$(mktemp -d)
    unzip -q -o "$apk" 'classes*.dex' -d "$tmp"
    if grep -aq "debug.krunk.force_tv" "$tmp"/classes*.dex; then fail "release dex contains the TV override"; else pass "release dex has no TV override"; fi
    rm -rf "$tmp"
  done
  summary ;;

phone)
  apk=$1 name=$2
  fresh_install "$apk"
  adb logcat -c
  m=$(mark)
  adb shell am start -a android.intent.action.VIEW -d "$HOST_URL/long.html" -n "$PKG/.MainActivity" >/dev/null
  wait_req "$m" "GET /long.html" 60 && pass "phone: page loaded" || fail "phone: page not requested"
  sleep 6
  shot "$name-page"
  tap_text "Menu" || tap_text "More options"
  sleep 2
  shot "$name-menu"
  tap_text "Settings"
  sleep 3
  shot "$name-settings"
  [ "$(top)" = "$PKG/.MainActivity" ] && pass "phone: still in Krunk" || fail "phone: top is $(top)"
  crash_check
  adb shell input keyevent HOME; sleep 3; adb shell am force-stop "$PKG"
  summary ;;

tv)
  apk=$1
  # Display changes once, here, and always undone (the emulator is shared; wedge trap).
  restore() {
    adb shell setprop debug.krunk.force_tv 0 || true
    adb shell wm size reset || true
    adb shell wm density reset || true
    adb shell settings put system accelerometer_rotation 1 || true
    adb shell settings put system user_rotation 0 || true
  }
  trap restore EXIT
  fresh_install "$apk"
  adb shell setprop debug.krunk.force_tv 1
  adb shell settings put system accelerometer_rotation 0
  adb shell settings put system user_rotation 0
  adb shell wm size 1920x1080
  adb shell wm density 320
  adb logcat -c
  # 2. Launch from the leanback entry: a focused element is visible.
  adb shell am start -a android.intent.action.MAIN -c android.intent.category.LEANBACK_LAUNCHER -n "$PKG/.MainActivity" >/dev/null
  sleep 12
  # A first start may show a one-time question (restore tabs etc.); it has its own focus.
  shot tv2-launch
  f=$(focused)
  [ -n "$f" ] && pass "2 focused at launch: $f" || fail "2 nothing focused at launch"

  # 3. Address bar: Center opens the editor and the keyboard, type, Enter loads.
  key DPAD_CENTER
  sleep 2
  shot tv3-editing
  m=$(mark)
  adb shell input text "${HOST_URL#http://}/long.html"
  sleep 1
  shot tv3-typed
  key ENTER
  wait_req "$m" "GET /long.html" 60 && pass "3 typed address loaded" || fail "3 typed address not loaded"
  sleep 6
  shot tv3-loaded

  # 4. Into the page (Down from the toolbar), scroll, step to the link (Right), follow it.
  key DPAD_DOWN
  KEY_PAUSE=0.3 times 12 DPAD_DOWN
  sleep 2
  shot tv4-scrolled
  m=$(mark)
  # Tab order on the page: the text field, then the link.
  key DPAD_RIGHT DPAD_RIGHT
  sleep 2
  shot tv4-link-focused
  key DPAD_CENTER
  wait_req "$m" "GET /page2.html" 30 && pass "4 scrolled and followed the link" || fail "4 link not followed"
  sleep 4
  shot tv4-page2

  # 6. Back goes back in history (the long page again).
  key BACK
  sleep 5
  shot tv6-back
  # The page's own text is not in a UI dump; the address field shows where the tab is.
  dump | grep -q 'long.html' && pass "6 Back returned to the long page" || fail "6 Back did not return to long.html"

  # 5. Cursor mode (long-press Center), aim at the non-focusable box, Center clicks it.
  adb shell input keyevent --longpress DPAD_CENTER
  sleep 2
  shot tv5-cursor-on
  m=$(mark)
  # The pointer starts in the middle of the page. Pushed against the top edge it scrolls the
  # page to the top; the box is then about 230-470 px below the page's top, at the left.
  KEY_PAUSE=0.15 times "${CURSOR_UP:-60}" DPAD_UP
  KEY_PAUSE=0.15 times "${CURSOR_LEFT:-26}" DPAD_LEFT
  KEY_PAUSE=0.15 times "${CURSOR_DOWN:-14}" DPAD_DOWN
  sleep 1
  shot tv5-cursor-aimed
  key DPAD_CENTER
  wait_req "$m" "GET /clicked" 15 && pass "5 cursor clicked the non-focusable element" || fail "5 cursor click not received"
  sleep 2
  shot tv5-clicked
  key BACK   # leaves cursor mode

  # 7. Toolbar (Up at the top), menu, Settings; VPN, Passwords, Account; and back.
  key DPAD_UP
  sleep 1
  shot tv7-toolbar
  f=$(focused)
  echo "  focused after Up at the top: $f"
  KEY_PAUSE=0.5 times 6 DPAD_RIGHT
  key DPAD_CENTER
  sleep 2
  shot tv7-menu
  KEY_PAUSE=0.3 times 30 DPAD_DOWN
  sleep 1
  shot tv7-menu-bottom
  key DPAD_CENTER
  sleep 3
  shot tv7-settings
  d=$(dump)
  grep -q 'text="Settings"' <<<"$d" && pass "7 Settings open" || fail "7 Settings not open"
  # open_entry SECTION ENTRY NAME: the category (Down from the top), Center, Right into it, Down
  # to the entry, Center; a look, then Back to Settings.
  open_entry() {
    local section=$1 entry=$2 name=$3
    # Its section may already be open (the content follows the categories down the left).
    if [ "${4:-}" = direct ] && focus_to "^$entry" DPAD_DOWN 25; then
      key DPAD_CENTER; sleep 3; shot "tv7-$name"; pass "7 reached $name ($(focused))"; key BACK; sleep 2; return
    fi
    if focus_to "^$section" DPAD_DOWN 10; then
      key DPAD_CENTER DPAD_RIGHT
      if focus_to "^$entry" DPAD_DOWN 25; then
        key DPAD_CENTER
        sleep 3
        shot "tv7-$name"
        pass "7 reached $name ($(focused))"
        key BACK
        sleep 2
        return
      fi
    fi
    fail "7 $name not reached (focus: $(focused))"
    shot "tv7-$name-missing"
  }
  open_entry "General" "Account" account
  open_entry "Privacy" "Passwords" passwords
  # Back on Settings the focus starts at the top again.
  open_entry "Privacy" "VPN" vpn direct
  key BACK
  sleep 2
  shot tv7-back-in-browser
  [ "$(top)" = "$PKG/.MainActivity" ] && pass "7 back from Settings" || fail "7 left Krunk"

  # 8. New tab and close it: the tab counter, the switcher's New tab, then its close button.
  # find_in KEYS PATTERN: tries each key direction in turn until PATTERN has the focus.
  find_in() { local pat=$1; shift; for k in "$@"; do focus_to "$pat" "$k" 8 && return 0; done; return 1; }
  key DPAD_UP
  if find_in "^Tabs:" DPAD_RIGHT; then
    shot tv8-tabs-focused
    key DPAD_CENTER
    sleep 3
    shot tv8-switcher
    if find_in "^New tab$" DPAD_DOWN DPAD_RIGHT DPAD_UP DPAD_LEFT; then
      key DPAD_CENTER
      sleep 3
      shot tv8-new-tab
      dump | grep -q 'Tabs: 2 open' && pass "8 new tab opened (2 tabs)" || fail "8 no second tab"
      key DPAD_UP
      find_in "^Tabs:" DPAD_RIGHT DPAD_LEFT && key DPAD_CENTER
      sleep 3
      # The new tab's card, then Right to its close button.
      if find_in "^New tab, " DPAD_DOWN DPAD_RIGHT && key DPAD_RIGHT && focused | grep -q "^Close New tab"; then
        shot tv8-close-focused
        key DPAD_CENTER
        sleep 2
        shot tv8-closed
        key BACK
        sleep 2
        dump | grep -q 'Tabs: 1 open' && pass "8 new tab closed (1 tab)" || fail "8 tab not closed ($(dump | grep -o 'Tabs: [0-9]* open' | head -1))"
      else
        fail "8 close button not reached (focus: $(focused))"
      fi
    else
      fail "8 New tab not reached in the switcher (focus: $(focused))"
    fi
  else
    fail "8 tab counter not reached (focus: $(focused))"
  fi
  shot tv8-end
  crash_check
  summary ;;

launcher)
  apk=$1
  export CONSOLE_SHOT=1
  adb install -r "$apk" | tail -1
  adb shell cmd package query-activities --brief -a android.intent.action.MAIN -c android.intent.category.LEANBACK_LAUNCHER | grep -q "$PKG/.MainActivity" &&
    pass "1 leanback launcher lists Krunk" || fail "1 not in the leanback launcher list"
  # The launcher's Apps tab: Krunk's banner in the Installed Apps row.
  adb shell input keyevent HOME
  sleep 20
  KEY_PAUSE=3 key DPAD_UP DPAD_RIGHT DPAD_CENTER
  sleep 15
  shot tv1-apps-row
  adb logcat -c
  # Open it from its tile: Down into the row, Right until the tile is Krunk's (by the
  # launcher's focus, read from the dump), Center. Falls back to the leanback intent.
  key DPAD_DOWN
  for _ in 1 2 3 4 5 6; do
    if focused | grep -qi krunk; then break; fi
    KEY_PAUSE=2 key DPAD_RIGHT
  done
  if focused | grep -qi krunk; then
    pass "1 Krunk's tile has the launcher's focus"
    key DPAD_CENTER
  else
    echo "  (tile focus not readable; leanback intent instead)"
    adb shell am start -a android.intent.action.MAIN -c android.intent.category.LEANBACK_LAUNCHER -n "$PKG/.MainActivity" >/dev/null
  fi
  sleep 120
  shot tv1-launched
  echo "  top: $(top)"
  crash_check
  summary ;;

*) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
