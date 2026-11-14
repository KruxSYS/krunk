#!/usr/bin/env bash
# End-to-end update checks for Krunk for Android 0.5.0 (A6), over adb against the sandbox
# emulator. Built on the a3 and a5 harnesses.
#   a6.sh <test...>   in this order for a full pass:
#     installover  9a: the published 0.4.0 ($KRUNK_OLD_APK) with history, a bookmark, a vault
#                  login and a finished download; the 0.5.0 direct APK ($KRUNK_NEW_APK) installed
#                  over it: all still there, same certificate
#     inapp        9b: 0.4.0 with data, the 0.5.0-qa build ($KRUNK_QA_APK) over it, then in the
#                  app: Check, Download, "install unknown apps", Android's confirmation -> 0.5.0
#                  (versionCode 7) runs with the data. The update server is updserver.py in proxy
#                  mode to the staging site, which holds 0.5.0 published with the real key.
#     quiet        9e: the qa build: start, 3 minutes idle, two pages; no request to the update
#                  server and no packet to it until Check is tapped, then exactly the manifest and
#                  its signature
#     tampersig    9c: one byte of the manifest changed; a valid signature from another key
#     tamperapk    9d: one byte of the APK changed (hash); an APK signed with another certificate
#                  (refused before the installer)
#   (9f, the Play bundle, is play.sh.)
# Needs: the a5 test server on 18080 (data seeding), and
#   python3 -I android/scripts/a6/updserver.py 18090 --proxy http://dind:<staging port> \
#       --control $KRUNK_A3_ROOT/upd.control >> $KRUNK_A3_ROOT/upd.log
# $FIX holds the tampered trees (make-fixtures.sh); the tests switch updserver by writing
# "root DIR" or "proxy URL" into the control file.
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")/../a3" && pwd)
A5=$(cd "$(dirname "${BASH_SOURCE[0]}")/../a5" && pwd)
A6=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$HERE/lib.sh"
mkdir -p "$EV"
PASS=0; FAIL=0; SKIP=0
REL=1; APK=${KRUNK_NEW_APK:-$RELEASE_APK}
# shellcheck disable=SC1090
source <(sed -n '/^dcheck() {/,/^privacy() {/p' "$HERE/a3.sh")
# shellcheck disable=SC1090
source <(sed -n '/^dump() {/,/^home() {/p' "$A5/a5.sh")
CONTROL=${KRUNK_UPD_CONTROL:-$ROOT/upd.control}
UPDLOG=${KRUNK_UPD_LOG:-$ROOT/upd.log}
PROXY=${KRUNK_UPD_PROXY:-}
FIX=${FIX:-$ROOT/fixtures}
CERT=c94b8915cd8b393a5fc48bc132a6de026374e76337c4341c59dd60328373e624
updmode() { echo "$1" > "$CONTROL"; }
updmark() { wc -l < "$UPDLOG"; }
updsince() { tail -n +"$(( $1 + 1 ))" "$UPDLOG"; }
vcode() { A "dumpsys package $PKG" | grep -m1 -oE 'versionCode=[0-9]+' | cut -d= -f2; }
vname() { A "dumpsys package $PKG" | grep -m1 -oE 'versionName=[^ ]+' | cut -d= -f2; }
certof() { A "dumpsys package $PKG" | grep -m1 -oiE 'signatures=.*' ; }
counts() { SU "sqlite3 $DB \"SELECT count(*) FROM visits; SELECT count(*) FROM bookmarks; SELECT count(*) FROM download_items\""; }
about() { menu; tapto text "Settings" 2; tapto text "About" 3; }   # Settings > About
checktap() { about; tapto text "Check for updates" 6; }

# The 0.4.0 profile: two visits, a bookmark, a vault with one login, a finished download.
seed_old() {
  fresh "$KRUNK_OLD_APK"; A "pm grant $PKG android.permission.POST_NOTIFICATIONS" > /dev/null 2>&1
  launch 12
  view "$B/index.html?a6old1" 8; view "$B/storage.html?a6old2" 8
  menu; tapui text "Bookmark this page" 3
  menu; tapto text "Passwords" 3; waitfor text "Create vault" 10
  tapui text "Vault password" 1 && type_text "upgrade-pass-6"; tapui text "Repeat the password" 1 && type_text "upgrade-pass-6"
  A "input keyevent KEYCODE_BACK"; sleep 1; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains
  tapany "Add login" 3
  tapui text "Site" 1 && type_text "https://a6.example"; tapui text "User name" 1 && type_text "a6user"
  tapui text "Password" 1 && type_text "a6-secret-1"; A "input keyevent KEYCODE_BACK"; sleep 1
  tapany "Save" 3; dump; nodes seed-vault; shot seed-vault
  check "0.4.0: a vault with one login" 'has text "a6.example" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1; A "input keyevent KEYCODE_BACK"; sleep 1
  local name; name="a6seed-$(date +%s).bin"
  view "$B/file/$name?size=200000" 6; confirm_download seed; sleep 8
  check "0.4.0: the seed download finished" '[ "$(rows a6seed)" -ge 1 ]'
  SEEDFILE=$name
  counts > "$EV/$1-db-before.txt" 2>&1
  certof > "$EV/$1-cert-before.txt"; vcode > "$EV/$1-vcode-before.txt"
  stop_clean
}

# After an upgrade: the data of seed_old is all there.
verify_data() {
  counts > "$EV/$1-db-after.txt" 2>&1
  check "$1: history, bookmarks and downloads rows survived" \
    '[ "$(sed -n 1p "$EV/$1-db-after.txt")" -ge "$(sed -n 1p "$EV/$1-db-before.txt")" ] && [ "$(sed -n 2p "$EV/$1-db-after.txt")" = "$(sed -n 2p "$EV/$1-db-before.txt")" ] && [ "$(sed -n 3p "$EV/$1-db-after.txt")" -ge 1 ]'
  certof > "$EV/$1-cert-after.txt"
  check "$1: same signing certificate" 'cmp -s "$EV/$1-cert-before.txt" "$EV/$1-cert-after.txt"'
  launch 12; front "$1" || return
  dump; has text "Restore tabs?" --contains && tapui text "Restore" 4
  shot "$1-started"
  menu; tapui text "History" 3; shot "$1-history"; dump; check "$1: history shows the old visits" 'has text "a6old" --contains || has text "a5 index" --contains || has text "10.250" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "Bookmarks" 3; shot "$1-bookmarks"; dump; check "$1: the bookmark is there" 'has text "a5" --contains || has text "10.250" --contains || has text "storage" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  downloads_screen; shot "$1-downloads"; dump; check "$1: the download is listed" 'has text "a6seed" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "Passwords" 3; sleep 2
  tapui text "Vault password" 1 && type_text "upgrade-pass-6"; A "input keyevent KEYCODE_BACK"; sleep 1; tapui text "Unlock" 5
  dump; nodes "$1-vault"; shot "$1-vault"
  check "$1: the vault opens with its password and holds the login" 'has text "a6.example" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  save "$1"; stop_clean
}

t_installover() {
  [ -n "${KRUNK_OLD_APK:-}" ] || { skip "installover (KRUNK_OLD_APK not set)"; return; }
  seed_old installover
  A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"
  T "$E" adb install -r "$APK" 2>&1 | tail -1 | tee "$EV/installover-install.txt"
  check "9a: the 0.5.0 direct APK installs over 0.4.0" 'grep -q Success "$EV/installover-install.txt"'
  check "9a: versionCode 7 (0.5.0)" '[ "$(vcode)" = 7 ]'
  verify_data installover
}

# Android's installer confirmation (system UI): tap its Install/Update button.
confirm_install() {
  local i
  for i in $(seq 1 20); do
    WIN=1 dump
    if has text "Update" || has text "Install" || has text "UPDATE" || has text "INSTALL"; then
      shot "$1-system-confirm"; nodes "$1-system-confirm"
      if has text "Update"; then tapbutton "Update" 3; elif has text "UPDATE"; then tapbutton "UPDATE" 3;
      elif has text "Install"; then tapbutton "Install" 3; else tapbutton "INSTALL" 3; fi
      return 0
    fi
    sleep 2
  done
  missing "Android's install confirmation"; return 1
}

# Android's "install unknown apps" screen for Krunk: switch it on, come back.
allow_unknown() {
  local i
  for i in $(seq 1 10); do
    dump
    if has text "Allow from this source" --contains; then
      shot "$1-unknown-apps"; tapui text "Allow from this source" 2 --contains
      shot "$1-unknown-apps-on"; A "input keyevent KEYCODE_BACK"; sleep 3; return 0
    fi
    sleep 1
  done
  missing "Allow from this source"; return 1
}

t_inapp() {
  [ -n "${KRUNK_OLD_APK:-}" ] && [ -n "${KRUNK_QA_APK:-}" ] || { skip "inapp (KRUNK_OLD_APK / KRUNK_QA_APK)"; return; }
  [ -n "$PROXY" ] || { skip "inapp (KRUNK_UPD_PROXY: the staging site)"; return; }
  updmode "proxy $PROXY"
  seed_old inapp
  A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"
  T "$E" adb install -r "$KRUNK_QA_APK" 2>&1 | tail -1 | tee "$EV/inapp-qa-install.txt"
  check "9b: the 0.5.0-qa build installs over 0.4.0" 'grep -q Success "$EV/inapp-qa-install.txt"'
  check "9b: versionCode 6 (qa)" '[ "$(vcode)" = 6 ]'
  A "appops set $PKG REQUEST_INSTALL_PACKAGES default" > /dev/null 2>&1
  launch 12; front inapp || return
  dump; has text "Restore tabs?" --contains && tapui text "Restore" 4
  local m; m=$(updmark)
  checktap; sleep 4; dump; nodes inapp-offer; shot inapp-offer
  check "9b: 0.5.0 is offered" 'has text "Krunk 0.5.0 is available" --contains'
  updsince "$m" > "$EV/inapp-check-requests.txt"
  check "9b: the check fetched the manifest and its signature" 'grep -q "/updates/android-arm64/latest.json " "$EV/inapp-check-requests.txt" && grep -q "latest.json.sig" "$EV/inapp-check-requests.txt"'
  check "9b: User-Agent Krunk-Android/0.5.0-qa" 'grep -q "Krunk-Android/0.5.0-qa" "$EV/inapp-check-requests.txt"'
  tapui text "What's new" 2 --contains; shot inapp-notes
  tapui text "Download and install" 3
  shot inapp-downloading
  local i; for i in $(seq 1 60); do dump; has text "Allow Krunk to install the update?" && break; has text "Update" && break; sleep 3; done
  shot inapp-permission-dialog; nodes inapp-permission-dialog
  check "9b: Krunk explains the install permission first" 'has text "Allow Krunk to install the update?"'
  tapui text "Open settings" 3
  allow_unknown inapp
  confirm_install inapp
  sleep 20
  updsince "$m" > "$EV/inapp-requests.txt"
  check "9b: exactly one APK download (the x86_64 one)" '[ "$(grep -c "\.apk " "$EV/inapp-requests.txt")" = 1 ] && grep -q "x86_64-release.apk" "$EV/inapp-requests.txt"'
  A "dumpsys package $PKG" | grep -m2 -E "versionCode|versionName" > "$EV/inapp-after.txt"
  check "9b: Krunk is now versionCode 7 (0.5.0)" '[ "$(vcode)" = 7 ] && [ "$(vname)" = 0.5.0 ]'
  A "ls /data/data/$PKG/no_backup/updates/" > "$EV/inapp-leftover.txt" 2>&1
  verify_data inapp
  A "su 0 ls /data/data/$PKG/no_backup/updates/" > "$EV/inapp-leftover-after-start.txt" 2>&1
  check "9b: no APK left in app storage after the restart" '! grep -q "\.apk" "$EV/inapp-leftover-after-start.txt"'
}

t_quiet() {
  [ -n "${KRUNK_QA_APK:-}" ] || { skip "quiet (KRUNK_QA_APK)"; return; }
  updmode "proxy ${PROXY:-http://127.0.0.1:9}"
  if [ "$(vcode)" != 6 ]; then fresh "$KRUNK_QA_APK"; fi
  A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"
  local m; m=$(updmark)
  cap_start quiet
  launch 15; front quiet || { cap_stop quiet; return; }
  dump; has text "Restore tabs?" --contains && tapui text "Restore" 4
  shot quiet-started
  echo "idle 180 s from $(date -u +%T)" > "$EV/quiet-steps.txt"
  sleep 180
  view "$B/index.html?quiet1" 10; view "$B/storage.html?quiet2" 10
  shot quiet-browsed
  echo "browsed until $(date -u +%T)" >> "$EV/quiet-steps.txt"
  updsince "$m" > "$EV/quiet-before-check.txt"
  check "9e: no request to the update server before Check" '[ ! -s "$EV/quiet-before-check.txt" ]'
  local m2; m2=$(updmark)
  echo "Check tapped at $(date -u +%T)" >> "$EV/quiet-steps.txt"
  checktap; sleep 6; shot quiet-after-check
  cap_stop quiet
  updsince "$m2" > "$EV/quiet-after-check.txt"
  check "9e: Check made exactly two requests (manifest, signature)" '[ "$(wc -l < "$EV/quiet-after-check.txt")" = 2 ]'
  grep -E "18090" "$EV/quiet-pcap-summary.txt" > "$EV/quiet-pcap-update-host.txt"
  save quiet; stop_clean
}

# One refusal: the fixture tree $1 served, Check (and Download when $3), expect text $2.
refusal() {
  local name=$1 want=$2 download=${3:-}
  updmode "root $FIX/$name"
  launch 10; front "$name" || return
  dump; has text "Restore tabs?" --contains && tapui text "Restore" 4
  local m; m=$(updmark)
  checktap; sleep 4
  if [ -n "$download" ]; then
    dump; nodes "$name-offer"; shot "$name-offer"
    tapui text "Download and install" 3
    local i; for i in $(seq 1 60); do dump; has text "$want" --contains && break; sleep 3; done
  fi
  dump; nodes "$name-error"; shot "$name-error"
  check "$name: refused with \"$want\"" 'has text "$want" --contains'
  check "$name: Android's installer was never opened" '! A "dumpsys activity activities" | grep -q -iE "packageinstaller.*(Install|Confirm)"'
  updsince "$m" > "$EV/$name-requests.txt"
  A "su 0 ls /data/data/$PKG/no_backup/updates/" > "$EV/$name-leftover.txt" 2>&1
  check "$name: no APK kept in app storage" '! grep -q "\.apk" "$EV/$name-leftover.txt"'
  save "$name"; stop_clean
}

t_tampersig() {
  [ -d "$FIX/onebyte" ] || { skip "tampersig (no fixtures in $FIX)"; return; }
  [ "$(vcode)" = 6 ] || fresh "$KRUNK_QA_APK"
  refusal onebyte "not signed with Krunk's release key"
  refusal otherkey "not signed with Krunk's release key"
}

t_tamperapk() {
  [ -d "$FIX/apkbyte" ] || { skip "tamperapk (no fixtures in $FIX)"; return; }
  [ "$(vcode)" = 6 ] || fresh "$KRUNK_QA_APK"
  refusal apkbyte "does not match the signed update information" download
  refusal othercert "not signed with the certificate of the installed Krunk" download
}

[ $# -gt 0 ] || { sed -n '2,26p' "$0"; exit 2; }
for t in "$@"; do
  echo "== $t"
  if declare -F "t_$t" > /dev/null; then "t_$t"; else echo "unknown test $t"; FAIL=$((FAIL + 1)); fi
done
echo "passed $PASS, failed $FAIL, skipped $SKIP"
[ "$FAIL" = 0 ]
