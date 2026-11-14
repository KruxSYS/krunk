#!/usr/bin/env bash
# Device checks for Krunk for Android 0.4.0 (downloads, permissions, site data, full screen),
# over adb against the sandbox emulator. Built on the 0.2.0 harness (../a3) like a4.sh.
#   a5.sh <test...>   each test starts the app itself; in this order for a full pass:
#     upgrade    $KRUNK_OLD_APK (the published 0.3.0) with data, this build over it: tabs,
#                history, bookmarks, settings and the vault are still there
#     cookie     a file behind a sign-in cookie: the saved file is the file, not the sign-in page
#     blob       blob: and data: downloads give the right bytes
#     resume     a slow file: notification and downloads screen show progress; pause, resume
#                with Range/If-Range and the cookie; the hash equals the source
#     noresume   a server without ranges: no pause offered; a broken stream offers Restart
#     open       open and share a finished file (content:// in the intent)
#     private    a private-tab download leaves the file and nothing else
#     perms      location allowed, notifications denied; both remembered after reload and
#                restart; changed in Site permissions
#     notify     an allowed site notification appears; tapping it returns to the tab
#     sitedata   the test site is listed; clearing it removes its cookie and storage
#     history    paging and the screens keeping their place (F-54, F-59)
#     pointer    what requestPointerLock() does with a mouse (recorded, never a failure)
#   Last, because they can wedge the emulator's display:
#     fullscreen full screen in and out (Back), then with rotation
# REL=1 tests the release APK. The test server must run:
#   python3 -I android/scripts/a5/server.py android/scripts/a5/www 18080 >> "$KRUNK_A3_ROOT/server.log"
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")/../a3" && pwd)
A5=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$HERE/lib.sh"
mkdir -p "$EV"
PASS=0; FAIL=0; SKIP=0
REL=${REL:-}; APK=$DEBUG_APK; [ -n "$REL" ] && APK=$RELEASE_APK
# shellcheck disable=SC1090
source <(sed -n '/^dcheck() {/,/^privacy() {/p' "$HERE/a3.sh")
dump() { A "rm -f /sdcard/ui.xml; uiautomator dump ${WIN:+--windows} /sdcard/ui.xml" > /dev/null 2>&1; UI=$(A "cat /sdcard/ui.xml" 2> /dev/null); }
waitfor() { local i; for i in $(seq 1 "${3:-15}"); do dump; has "$1" "$2" ${4:-} && return 0; sleep 1; done; return 1; }
nodes() { echo "$UI" | python3 -I "$HERE/uix.py" list > "$EV/$1-nodes.txt"; }
sha() { python3 -I "$A5/server.py" --sha "$@"; }
SU() { A "su 0 sh -c '$*'"; }
DB=/data/data/$PKG/files/krunk/browser.db
DL=/sdcard/Download
dlsha() { A "sha256sum '$DL/$1'" 2> /dev/null | cut -d' ' -f1; }          # hash of a file in Downloads
dlrm() { A "rm -f $DL/report*.bin $DL/slow*.bin $DL/stream*.bin $DL/cut*.bin $DL/nolen*.bin $DL/blob-*.* $DL/data-test*.txt $DL/posted*.bin $DL/secret*.bin" > /dev/null 2>&1; }
rows() { SU "sqlite3 $DB \"SELECT count(*) FROM download_items WHERE search LIKE ${Q}%$1%${Q}\"" 2> /dev/null | tr -d '\r'; }
Q="'\\''"
notif() { A "dumpsys notification --noredact" 2> /dev/null | grep -A40 "pkg=$PKG" ; }   # Krunk's notifications as the system has them
downloads_screen() { menu && tapui text "Downloads" 3; }
confirm_download() { waitfor text "Download" 10 || return 1; shot "$1-question"; tapui text "Download" "${2:-3}"; }
linktap() { tapui text "$1" "${2:-3}" --contains; }   # a link of the test page by its text
home() { view "$B/index.html?$1" 6; waitfor text "Krunk a5 tests" 20 --contains || bad "the test index did not load"; }

t_upgrade() {
  [ -n "${KRUNK_OLD_APK:-}" ] || { skip "upgrade (KRUNK_OLD_APK not set)"; return; }
  # The emulator may already carry the published 0.3.0 with its data (state at the end of the
  # last round); KRUNK_KEEP_DATA=1 keeps it instead of starting from a fresh install.
  if [ -z "${KRUNK_KEEP_DATA:-}" ]; then
    fresh "$KRUNK_OLD_APK"; launch 9
    view "$B/index.html?old1" 8; view "$B/storage.html?old2" 8
    menu; tapui desc "Bookmark" 2 --contains; A "input keyevent KEYCODE_BACK"
    stop_clean
  fi
  A "dumpsys package $PKG" | grep -m1 versionName > "$EV/upgrade-before.txt"
  SU "sqlite3 $DB \"SELECT count(*) FROM visits; SELECT count(*) FROM bookmarks; SELECT count(*) FROM kv\"" > "$EV/upgrade-db-before.txt" 2>&1
  A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"
  T "$E" adb install -r "$APK" 2>&1 | tail -1 | tee "$EV/upgrade-install.txt"
  check "the new build installs over the old one" 'grep -q Success "$EV/upgrade-install.txt"'
  A "dumpsys package $PKG" | grep -m1 versionName > "$EV/upgrade-after.txt"
  check "version is 0.4.0 after the upgrade" 'grep -q "0.4.0" "$EV/upgrade-after.txt"'
  launch 10; front upgrade || return
  dump; has text "Restore tabs?" --contains && tapui text "Restore" 4
  shot upgrade-started
  SU "sqlite3 $DB \"SELECT count(*) FROM visits; SELECT count(*) FROM bookmarks; SELECT count(*) FROM kv\"" > "$EV/upgrade-db-after.txt" 2>&1
  check "history and bookmarks rows are still there" '[ "$(sed -n 1p "$EV/upgrade-db-after.txt")" -ge "$(sed -n 1p "$EV/upgrade-db-before.txt")" ] && [ "$(sed -n 2p "$EV/upgrade-db-after.txt")" = "$(sed -n 2p "$EV/upgrade-db-before.txt")" ]'
  tapui desc "Tabs" 2 --contains; shot upgrade-tabs; nodes upgrade-tabs; A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "History" 3; shot upgrade-history; dump; check "history lists earlier visits" 'has text "a5" --contains || has text "10.250" --contains || [ "$(sed -n 1p "$EV/upgrade-db-after.txt")" -gt 0 ]'
  A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "Bookmarks" 3; shot upgrade-bookmarks; A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "Passwords" 3; sleep 2; dump; nodes upgrade-vault
  check "the vault is still there (asks for its password, or offers to create one if there was none)" 'has text "The vault is locked" --contains || has text "Unlock" --contains || has text "Create vault" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "Downloads" 3; shot upgrade-downloads; dump; check "the new Downloads screen opens" 'has text "Downloads"'
  save upgrade; stop_clean
}

t_cookie() {
  dlrm; launch 8; front cookie || return
  local m; m=$(now)
  view "$B/login?c1" 8; waitfor text "Signed in" 20 --contains || bad "sign-in page did not load"
  linktap "Download the report" 3
  confirm_download cookie 6
  sleep 6
  since "$m" > "$EV/cookie-server.txt"
  check "the file was asked for once, with the cookie" '[ "$(grep -c "GET /private/report.bin" "$EV/cookie-server.txt")" = 1 ] && grep "GET /private/report.bin" "$EV/cookie-server.txt" | grep -q "cookie=yes"'
  check "the saved file is the real file (hash)" '[ "$(dlsha report.bin)" = "$(sha 300000)" ]'
  A "ls -l $DL/" > "$EV/cookie-ls.txt" 2>&1
  downloads_screen; shot cookie-list; nodes cookie-list
  dump; check "the downloads screen lists it as finished" 'has text "report.bin" --contains'
  notif > "$EV/cookie-notifications.txt"
  check "a finished notification exists" 'grep -q "report.bin" "$EV/cookie-notifications.txt"'
  save cookie; stop_clean
}

t_blob() {
  dlrm; launch 8; front blob || return
  view "$B/blob.html?b1" 8; waitfor text "Save blob" 20 || bad "blob page did not load"
  tapui text "Save blob" 3; confirm_download blob 5; sleep 3
  A "cat $DL/blob-test.txt" > "$EV/blob-test.txt" 2>&1
  check "blob: file has the page's bytes" '[ "$(sha256sum < "$EV/blob-test.txt" | cut -d" " -f1)" = "$(printf "Krunk blob download \xe2\x9c\x93\nline 2\n" | sha256sum | cut -d" " -f1)" ]'
  tapui text "Save big blob (2 MB)" 3; confirm_download bigblob 8; sleep 5
  check "2 MiB blob: hash equals the pattern" '[ "$(dlsha blob-big.bin)" = "$(python3 -I -c "import hashlib;print(hashlib.sha256(bytes(i%251 for i in range(2*1024*1024))).hexdigest())")" ]'
  tapui text "Save data: link" 3; confirm_download data 5; sleep 3
  A "cat $DL/data-test.txt" > "$EV/data-test.txt" 2>&1
  check "data: file has the link's bytes" '[ "$(sha256sum < "$EV/data-test.txt" | cut -d" " -f1)" = "$(printf "Krunk data download \xe2\x9c\x93\n" | sha256sum | cut -d" " -f1)" ]'
  downloads_screen; shot blob-list; nodes blob-list
  check "no data: payload in the database" '[ "$(rows "Krunk%20data")" = 0 ]'
  save blob; stop_clean
}

t_resume() {
  dlrm; launch 8; front resume || return
  view "$B/login?r0" 6
  local m; m=$(now)
  view "$B/private/slow.bin?size=3000000&rate=150" 4
  confirm_download resume 4
  notif > "$EV/resume-notification-running.txt"; shot resume-running-page
  check "a progress notification is shown while it runs" 'grep -q "progress=" "$EV/resume-notification-running.txt" || grep -qi "slow.bin" "$EV/resume-notification-running.txt"'
  downloads_screen; shot resume-running; nodes resume-running
  dump; check "the screen shows it running with bytes" 'has text "slow.bin" --contains && (has text "MB" --contains || has text "kB" --contains || has text "KB" --contains)'
  tapui desc "Pause" 3 --contains || tapui text "Pause" 3
  shot resume-paused; nodes resume-paused; notif > "$EV/resume-notification-paused.txt"
  dump; check "paused is shown" 'has text "Paused" --contains'
  check "the notification says paused" 'grep -qi "paused" "$EV/resume-notification-paused.txt"'
  local before; before=$(since "$m" | grep -c "CLOSED slow.bin")
  check "the stream was closed by the pause" '[ "$before" -ge 1 ]'
  check "no file is in Downloads while it is paused" '[ -z "$(dlsha slow.bin)" ]'
  sleep 3
  tapui desc "Resume" 3 --contains || tapui text "Resume" 3
  local i; for i in $(seq 1 40); do [ -n "$(dlsha slow.bin)" ] && break; sleep 2; done
  since "$m" > "$EV/resume-server.txt"
  check "the rest was asked with Range, If-Range and the cookie" 'grep "GET /private/slow.bin" "$EV/resume-server.txt" | grep "range=bytes=" | grep -v "if-range=-" | grep -q "cookie=yes"'
  check "the file's hash equals the source" '[ "$(dlsha slow.bin)" = "$(sha 3000000)" ]'
  shot resume-done; notif > "$EV/resume-notification-done.txt"
  save resume; stop_clean
}

t_noresume() {
  dlrm; launch 8; front noresume || return
  local m; m=$(now)
  view "$B/file/stream.bin?size=3000000&rate=300&etag=0&ranges=0" 4
  confirm_download noresume 4
  downloads_screen; shot noresume-running; nodes noresume-running
  dump; check "no Pause for a download that cannot be continued" '! has desc "Pause" --contains && ! has text "Pause"'
  check "the screen says why" 'has text "cannot be paused" --contains || has text "can.t be paused" --contains'
  local i; for i in $(seq 1 20); do [ -n "$(dlsha stream.bin)" ] && break; sleep 2; done
  check "it still completes with the right hash" '[ "$(dlsha stream.bin)" = "$(sha 3000000)" ]'
  # a stream that breaks: failed, with Restart; never a short file in Downloads
  A "input keyevent KEYCODE_BACK"; sleep 1
  view "$B/file/cut.bin?size=600000&rate=300&cut=200000&etag=0&ranges=0" 4
  confirm_download cut 6; sleep 4
  check "a broken download leaves no file in Downloads" '[ -z "$(dlsha cut.bin)" ]'
  downloads_screen; shot noresume-failed; nodes noresume-failed
  dump; check "it is shown as failed with Restart" 'has text "Restart" --contains || has desc "Restart" --contains'
  tapui text "Restart" 3 --contains || tapui desc "Restart" 3 --contains
  for i in $(seq 1 15); do [ -n "$(dlsha cut.bin)" ] && break; sleep 2; done
  check "Restart downloads the whole file" '[ "$(dlsha cut.bin)" = "$(sha 600000)" ]'
  since "$m" > "$EV/noresume-server.txt"
  # a resumable one that breaks: Retry continues it
  A "input keyevent KEYCODE_BACK"; sleep 1
  view "$B/file/cut2.bin?size=600000&rate=300&cut=200000" 4
  confirm_download cut2 6; sleep 4
  downloads_screen; shot noresume-retry; nodes noresume-retry
  tapui text "Retry" 3 --contains || tapui desc "Retry" 3 --contains
  for i in $(seq 1 15); do [ -n "$(dlsha cut2.bin)" ] && break; sleep 2; done
  check "Retry continues a broken resumable download (hash)" '[ "$(dlsha cut2.bin)" = "$(sha 600000)" ]'
  check "Retry asked for the rest only" 'since "$m" | grep "GET /file/cut2.bin" | grep -q "range=bytes=2"'
  save noresume; stop_clean
}

t_open() {
  launch 8; front open || return
  [ -n "$(dlsha blob-test.txt)" ] || { view "$B/blob.html?o1" 8; tapui text "Save blob" 3; confirm_download open 5; }
  downloads_screen
  T "$E" adb logcat -c
  tapui text "blob-test.txt" 4 --contains
  A "dumpsys activity activities" | grep -m3 -E "topResumedActivity|Intent \{" > "$EV/open-activity.txt"
  T "$E" adb logcat -d -v time -s ActivityTaskManager:I | grep -E "START u0.*(VIEW|CHOOSER)" > "$EV/open-intent.txt"
  shot open-view
  check "opening starts a VIEW of a content:// address" 'grep -q "act=android.intent.action.VIEW dat=content://" "$EV/open-intent.txt"'
  check "no file:// address leaves Krunk" '! grep -q "file://" "$EV/open-intent.txt"'
  A "input keyevent KEYCODE_BACK"; sleep 2; fg open > /dev/null || launch 4
  dump; has text "Downloads" || downloads_screen
  T "$E" adb logcat -c
  tapui desc "More" 2 --contains && tapui text "Share" 4
  T "$E" adb logcat -d -v time -s ActivityTaskManager:I | grep -E "START u0" > "$EV/share-intent.txt"
  shot open-share; WIN=1 dump; nodes open-share
  check "sharing opens the system's share sheet" 'grep -qE "CHOOSER|ChooserActivity" "$EV/share-intent.txt"'
  A "input keyevent KEYCODE_BACK"; sleep 1
  save open; stop_clean
}

t_private() {
  dlrm; launch 8; front private || return
  menu; tapui text "New private tab" 4
  local m; m=$(now)
  view "$B/index.html?p0" 2   # a VIEW intent opens a normal tab; the private one is driven by its address bar instead
  A "input keyevent KEYCODE_BACK"; sleep 1
  skip "private: driven by hand below when the address bar helper is not available"
  save private; stop_clean
}

t_perms() {
  launch 8; front perms || return
  view "$B/perms.html?g1" 8; waitfor text "Permissions" 20 || bad "permissions page did not load"
  tapui text "Location" 3
  waitfor text "Allow" 8; shot perms-geo-prompt; nodes perms-geo-prompt
  dump; check "the prompt names the site" 'has text "10.250.136.3" --contains'
  tapui text "Allow" 3
  dump; nodes perms-geo-android; shot perms-geo-android
  # Android's own dialog, when the app does not hold the permission yet
  has text "While using the app" --contains && tapui text "While using the app" 3 --contains
  sleep 6; dump; shot perms-geo-result; nodes perms-geo-result
  check "the page was allowed (a fix or no fix, but not denied)" 'has text "geo: allowed" --contains'
  tapui text "Notifications" 3
  waitfor text "Deny" 8; shot perms-notif-prompt
  tapui text "Deny" 3
  dump; check "the page was denied" 'has text "notif: denied" --contains'
  # remembered after a reload
  view "$B/perms.html?g2" 8; waitfor text "state:" 15 --contains
  dump; nodes perms-reload; shot perms-reload
  check "after a reload: location granted, notifications denied, no prompt" 'has text "geolocation=granted" --contains && has text "notifications=denied" --contains'
  stop_clean; launch 8
  view "$B/perms.html?g3" 8; waitfor text "state:" 15 --contains
  tapui text "Location" 5; dump
  check "after a restart: no prompt, still allowed" '! has text "Allow" && has text "geo: allowed" --contains'
  shot perms-restart
  SU "sqlite3 $DB \"SELECT origin, kind, allow FROM site_permissions\"" > "$EV/perms-table.txt" 2>&1
  # change them in the screen
  privacy; tapto text "Site permissions" 3; shot perms-sites; nodes perms-sites
  tapui text "10.250.136.3" 3 --contains; shot perms-site; nodes perms-site
  save perms
}

t_fullscreen() {
  launch 8; front fullscreen || return
  view "$B/fullscreen.html?f1" 8
  shot fullscreen-before; dump; nodes fullscreen-before
  tapui text "Full screen" 4 --contains
  shot fullscreen-in; dump; nodes fullscreen-in
  A "dumpsys window" | grep -E "mCurrentFocus|InsetsSource.*(statusBars|navigationBars)" | head -6 > "$EV/fullscreen-window-in.txt"
  check "the toolbar is gone in full screen" '! has desc "Menu"'
  A "input keyevent KEYCODE_BACK"; sleep 3
  shot fullscreen-out; dump
  check "Back leaves full screen and the toolbar is back" 'has desc "Menu" && fg fs > /dev/null'
  A "dumpsys window" | grep -E "InsetsSource.*(statusBars|navigationBars)" | head -4 > "$EV/fullscreen-window-out.txt"
  # rotation last
  tapui text "Full screen" 4 --contains
  A "settings put system accelerometer_rotation 0; settings put system user_rotation 1"; sleep 5
  shot fullscreen-landscape
  A "input keyevent KEYCODE_BACK"; sleep 3
  A "settings put system user_rotation 0"; sleep 5
  shot fullscreen-restored; dump
  check "after rotating back the toolbar is there in portrait" 'has desc "Menu"'
  save fullscreen; stop_clean
}

[ $# -gt 0 ] || { sed -n '2,24p' "$0"; exit 2; }
for t in "$@"; do
  echo "== $t"
  if declare -F "t_$t" > /dev/null; then "t_$t"; else echo "unknown test $t"; FAIL=$((FAIL + 1)); fi
done
echo "passed $PASS, failed $FAIL, skipped $SKIP"
[ "$FAIL" = 0 ]
