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
downloads_screen() { menu && tapto text "Downloads" 3; }
confirm_download() { waitfor text "Download this file?" 12 || { missing "Download this file?"; return 1; }; shot "$1-question"; sleep 1; tapui text "Download" "${2:-3}"; }   # the button waits 0.5 s (D-16)
clean() { A "am force-stop $PKG; pm clear $PKG" > /dev/null; A "pm grant $PKG android.permission.POST_NOTIFICATIONS"; }   # no restored tabs re-asking for an old download
go() { tapui desc "Edit address" 2 && type_text "${1#http://}" && A "input keyevent KEYCODE_ENTER" && sleep "${2:-8}"; }   # type an address into the tab on screen
type_text() { A "input text '$1'"; sleep 1; }
tapbutton() { # tapbutton TEXT [wait]: the last node whose text is exactly TEXT (a page button below a heading of the same words)
  dump; local xy; xy=$(echo "$UI" | python3 -I "$HERE/uix.py" list | python3 -I -c '
import re,sys
want=sys.argv[1]; last=None
for l in sys.stdin:
    m=re.match(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]\s+text=.(.*?). desc=", l)
    if m and m.group(5)==want: last=((int(m.group(1))+int(m.group(3)))//2,(int(m.group(2))+int(m.group(4)))//2)
print("%d %d"%last if last else "")' "$1")
  [ -n "$xy" ] || { missing "$1"; return 1; }
  # shellcheck disable=SC2086
  tap $xy "${2:-2}"
}
tapany() { dump; if has desc "$1" --contains; then tapui desc "$1" "${2:-2}" --contains; else tapui text "$1" "${2:-2}" --contains; fi; }   # an icon (description) or a text button
linktap() { tapui text "$1" "${2:-3}" --contains; }   # a link of the test page by its text
# A secure context for permission tests (geolocation and notifications need one; plain http on
# an IP is not): a relay on the device makes the test server http://localhost:18081.
L=http://localhost:18081
SITE=localhost
relay() { A "pgrep -f 'p 18081' > /dev/null" || { (timeout 7000 adb -s emulator-5554 shell "toybox nc -s 127.0.0.1 -p 18081 -L toybox nc $(echo ${B#http://} | tr : " ")" > /dev/null 2>&1 &); sleep 2; }; }
home() { view "$B/index.html?$1" 6; waitfor text "Krunk a5 tests" 20 --contains || bad "the test index did not load"; }

t_upgrade() {
  [ -n "${KRUNK_OLD_APK:-}" ] || { skip "upgrade (KRUNK_OLD_APK not set)"; return; }
  # The emulator may already carry the published 0.3.0 with its data (state at the end of the
  # last round); KRUNK_KEEP_DATA=1 keeps it instead of starting from a fresh install.
  if [ -z "${KRUNK_KEEP_DATA:-}" ]; then
    fresh "$KRUNK_OLD_APK"; launch 9
    view "$B/index.html?old1" 8; view "$B/storage.html?old2" 8
    menu; tapui text "Bookmark this page" 3
    # a vault on 0.3.0 (one login), to see it again after the upgrade
    menu; tapto text "Passwords" 3; waitfor text "Create vault" 10
    tapui text "Vault password" 1 && type_text "upgrade-pass-1"; tapui text "Repeat the password" 1 && type_text "upgrade-pass-1"
    A "input keyevent KEYCODE_BACK"; sleep 1; tapui text "Create vault" 6
    waitfor text "No logins yet" 15 --contains; dump; nodes upgrade-vault-before
    check "0.3.0: a vault was created" 'has text "No logins yet" --contains'
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
  check "the vault from 0.3.0 is still there (locked, asks for its password)" 'has text "The vault is locked" --contains || has text "Unlock" --contains'
  tapui text "Vault password" 1 && type_text "upgrade-pass-1"; A "input keyevent KEYCODE_BACK"; sleep 1; tapui text "Unlock" 5
  dump; nodes upgrade-vault-open
  check "the vault opens with its 0.3.0 password" 'has text "No logins yet" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  downloads_screen; shot upgrade-downloads; dump; check "the new Downloads screen opens" 'has text "Downloads"'
  save upgrade; stop_clean
}

t_cookie() {
  dlrm; clean; launch 8; front cookie || return
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
  dlrm; clean; launch 8; front blob || return
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
  dlrm; clean; launch 8; front resume || return
  view "$B/login?r0" 6
  local m; m=$(now)
  view "$B/private/slow.bin?size=3000000&rate=40" 4
  confirm_download resume 4
  notif > "$EV/resume-notification-running.txt"; shot resume-running-page
  check "a progress notification is shown while it runs" 'grep -q "progress=" "$EV/resume-notification-running.txt" || grep -qi "slow.bin" "$EV/resume-notification-running.txt"'
  downloads_screen; shot resume-running; nodes resume-running
  dump; check "the screen shows it running with bytes" 'has text "slow.bin" --contains && (has text "MB" --contains || has text "kB" --contains || has text "KB" --contains)'
  tapany Pause 3
  shot resume-paused; nodes resume-paused; notif > "$EV/resume-notification-paused.txt"
  dump; check "paused is shown" 'has text "Paused" --contains'
  check "the notification says paused" 'grep -qi "paused" "$EV/resume-notification-paused.txt"'
  local before; before=$(since "$m" | grep -c "CLOSED slow.bin")
  check "the stream was closed by the pause" '[ "$before" -ge 1 ]'
  check "no file is in Downloads while it is paused" '[ -z "$(dlsha slow.bin)" ]'
  sleep 3
  tapany Resume 3
  local i; for i in $(seq 1 60); do [ -n "$(dlsha slow.bin)" ] && break; sleep 2; done
  since "$m" > "$EV/resume-server.txt"
  check "the rest was asked with Range, If-Range and the cookie" 'grep "GET /private/slow.bin" "$EV/resume-server.txt" | grep "range=bytes=" | grep -v "if-range=-" | grep -q "cookie=yes"'
  check "the file's hash equals the source" '[ "$(dlsha slow.bin)" = "$(sha 3000000)" ]'
  shot resume-done; notif > "$EV/resume-notification-done.txt"
  save resume; stop_clean
}

t_noresume() {
  local r; r=$(date +%H%M%S)
  dlrm; clean; launch 8; front noresume || return
  local m; m=$(now)
  view "$B/file/stream.bin?size=3000000&rate=60&etag=0&ranges=0" 4
  confirm_download noresume 4
  downloads_screen; shot noresume-running; nodes noresume-running
  dump; check "no Pause for a download that cannot be continued" '! has desc "Pause" --contains && ! has text "Pause"'
  check "the screen says why" 'has text "cannot be paused" --contains'
  local i; for i in $(seq 1 45); do [ -n "$(dlsha stream.bin)" ] && break; sleep 2; done
  check "it still completes with the right hash" '[ "$(dlsha stream.bin)" = "$(sha 3000000)" ]'
  # a stream that breaks: failed, with Restart; never a short file in Downloads
  A "input keyevent KEYCODE_BACK"; sleep 1
  view "$B/file/cut$r.bin?size=600000&rate=300&cut=200000&etag=0&ranges=0" 4
  confirm_download cut 6; sleep 4
  check "a broken download leaves no file in Downloads" '[ -z "$(dlsha cut$r.bin)" ]'
  downloads_screen; shot noresume-failed; nodes noresume-failed
  dump; check "it is shown as failed with Restart" 'has text "Restart" --contains || has desc "Restart" --contains'
  tapany Restart 3
  for i in $(seq 1 15); do [ -n "$(dlsha cut$r.bin)" ] && break; sleep 2; done
  check "Restart downloads the whole file" '[ "$(dlsha cut$r.bin)" = "$(sha 600000)" ]'
  since "$m" > "$EV/noresume-server.txt"
  # a resumable one that breaks: Retry continues it
  A "input keyevent KEYCODE_BACK"; sleep 1
  view "$B/file/cut2$r.bin?size=600000&rate=300&cut=200000" 4
  confirm_download cut2 6; sleep 4
  downloads_screen; shot noresume-retry; nodes noresume-retry
  tapany Retry 3
  for i in $(seq 1 15); do [ -n "$(dlsha cut2$r.bin)" ] && break; sleep 2; done
  check "Retry continues a broken resumable download (hash)" '[ "$(dlsha cut2$r.bin)" = "$(sha 600000)" ]'
  check "Retry asked for the rest only" 'since "$m" | grep "GET /file/cut2$r.bin" | grep -q "range=bytes=2"'
  save noresume; stop_clean
}

t_open() {
  clean; launch 8; front open || return
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
  dlrm; clean; launch 8; front private || return
  menu; tapui text "New private tab" 3
  [ -z "$REL" ] && A "setprop debug.krunk.show_private 1"
  local m; m=$(now)
  go "$B/login?priv" 8
  go "$B/private/secret.bin?size=400000&rate=5" 4
  waitfor text "Download this file?" 12; dump; nodes private-question
  check "the question says what a private download leaves" 'has text "Krunk forgets this download" --contains'
  tapui text "Download" 2
  check "no row while it runs" '[ "$(rows secret)" = 0 ]'
  downloads_screen; dump; nodes private-running
  check "listed (marked private) while it runs" 'has text "secret.bin" --contains && has text "Private" --contains'
  local i; for i in $(seq 1 60); do [ -n "$(dlsha secret.bin)" ] && break; sleep 2; done
  sleep 2; dump; nodes private-done
  check "the file is the real file, fetched with the private tab's cookie" '[ "$(dlsha secret.bin)" = "$(sha 400000)" ] && since "$m" | grep "GET /private/secret.bin" | grep -q "cookie=yes"'
  check "gone from the list when done" '! has text "secret.bin" --contains'
  check "no row in the database" '[ "$(rows secret)" = 0 ]'
  SU "grep -c secret $DB $DB-wal 2>/dev/null" > "$EV/private-grep-db.txt" 2>&1
  check "the name is nowhere in the database files" '! grep -qE ":[1-9]" "$EV/private-grep-db.txt"'
  SU "ls /data/data/$PKG/files/downloads-part/ 2>/dev/null | wc -l" > "$EV/private-parts.txt"
  check "no part file is left" '[ "$(tr -d "\r\n " < "$EV/private-parts.txt")" = 0 ]'
  A "input keyevent KEYCODE_BACK"; sleep 1
  notif > "$EV/private-notification-before.txt"
  # close the private tabs: the notification goes
  tapui desc "Tabs" 2 --contains; dump; nodes private-tabs
  tapui text "Private" 2 --contains; tapui desc "Close all private tabs" 2 && { dump; nodes private-closeall; has text "Close" && tapui text "Close" 2; }
  sleep 2; notif > "$EV/private-notification-after.txt"
  check "its notification is gone once the private tabs are closed" '! grep -q "secret.bin" "$EV/private-notification-after.txt"'
  save private; stop_clean
}

t_perms() {
  relay; launch 8; front perms || return
  view "$L/perms.html?g1" 8; waitfor text "Permissions" 20 || bad "permissions page did not load"
  tapui text "Location" 3
  waitfor text "Allow" 8; sleep 1; shot perms-geo-prompt; nodes perms-geo-prompt
  dump; check "the prompt names the site" 'has text "$SITE" --contains'
  tapui text "Allow" 3
  dump; nodes perms-geo-android; shot perms-geo-android
  # Android's own dialog, when the app does not hold the permission yet
  has text "While using the app" --contains && tapui text "While using the app" 3 --contains
  sleep 6; dump; shot perms-geo-result; nodes perms-geo-result
  check "the page was allowed (a fix or no fix, but not denied)" 'has text "geo: allowed" --contains'
  tapui text "Notifications" 3
  waitfor text "Deny" 8; sleep 1; shot perms-notif-prompt
  tapui text "Deny" 3
  dump; check "the page was denied" 'has text "notif: denied" --contains'
  # remembered after a reload
  view "$L/perms.html?g2" 8; waitfor text "state:" 15 --contains
  dump; nodes perms-reload; shot perms-reload
  check "after a reload: location granted, notifications denied, no prompt" 'has text "geolocation=granted" --contains && has text "notifications=denied" --contains'
  stop_clean; launch 8
  view "$L/perms.html?g3" 8; waitfor text "state:" 15 --contains
  tapui text "Location" 5; dump
  check "after a restart: no prompt, still allowed" '! has text "Allow" && has text "geo: allowed" --contains'
  shot perms-restart
  SU "sqlite3 $DB \"SELECT origin, kind, allow FROM site_permissions\"" > "$EV/perms-table.txt" 2>&1
  # change them in the screen
  privacy; tapto text "Site permissions" 3; shot perms-sites; nodes perms-sites
  tapui text "$SITE" 3 --contains; shot perms-site; nodes perms-site
  save perms
}

t_fullscreen() {
  stop_clean; launch 8; front fullscreen || return
  view "$B/fullscreen.html?f1" 8
  shot fullscreen-before; dump; nodes fullscreen-before
  tapbutton "Full screen" 4
  shot fullscreen-in; dump; nodes fullscreen-in
  A "dumpsys window" | grep -E "mCurrentFocus|InsetsSource.*(statusBars|navigationBars)" | head -6 > "$EV/fullscreen-window-in.txt"
  check "the toolbar is gone in full screen" '! has desc "Menu"'
  A "input keyevent KEYCODE_BACK"; sleep 3
  shot fullscreen-out; dump
  check "Back leaves full screen and the toolbar is back" 'has desc "Menu" && fg fs > /dev/null'
  A "dumpsys window" | grep -E "InsetsSource.*(statusBars|navigationBars)" | head -4 > "$EV/fullscreen-window-out.txt"
  # an orientation lock from the page (full screen + landscape), Back restores what was there.
  # No UI dump while the screen is landscape: UiAutomation then fixes the system's rotation at 90
  # degrees (user_rotation=1), which would look like a lock Krunk did not give back.
  A "settings put system user_rotation 0"
  reqo() { A "dumpsys window" | grep -oE "mLastOrientation=[-0-9]+" | head -1 | cut -d= -f2; }
  tapbutton "Full screen + landscape" 6
  shot fullscreen-landscape-lock; echo "requested $(reqo)" > "$EV/fullscreen-rotation-locked.txt"
  check "the page's landscape lock is applied (requested landscape)" 'grep -qE "requested (0|6|8|11)$" "$EV/fullscreen-rotation-locked.txt"'
  A "input keyevent KEYCODE_BACK"; sleep 4
  echo "requested $(reqo)" > "$EV/fullscreen-rotation-after.txt"; shot fullscreen-unlocked
  A "settings put system user_rotation 0"; sleep 3; dump; nodes fullscreen-unlocked
  check "Back ends full screen and gives the orientation back (requested unspecified, portrait page)" 'grep -q "requested -1" "$EV/fullscreen-rotation-after.txt" && has desc "Menu" && has text "portrait-primary" --contains'
  # a lock outside full screen is refused
  tapbutton "Lock landscape only" 4; echo "requested $(reqo)" > "$EV/fullscreen-rotation-lockonly.txt"; dump; nodes fullscreen-lockonly
  check "a lock outside full screen is refused" 'grep -q "requested -1" "$EV/fullscreen-rotation-lockonly.txt"'
  # device rotation (last): full screen survives turning the device and returning
  tapbutton "Full screen" 4
  A "settings put system accelerometer_rotation 0; settings put system user_rotation 1"; sleep 5
  shot fullscreen-landscape
  A "settings put system user_rotation 0"; sleep 5
  A "input keyevent KEYCODE_BACK"; sleep 3
  shot fullscreen-restored; dump
  check "after rotating back and Back, the toolbar is there in portrait" 'has desc "Menu"'
  save fullscreen; stop_clean
}

t_notify() { # after perms: allow notifications on the site page, then show one and tap it
  stop_clean; relay; launch 8; front notify || return
  view "$L/perms.html?n1" 8; waitfor text "Permissions" 20
  tapui text "Notifications" 3
  dump; has text "Allow" && { shot notify-prompt; tapui text "Allow" 3; }
  dump; has text "Allow" --contains && has text "Krunk" --contains && tapui text "Allow" 3   # Android's own dialog
  sleep 2; dump; nodes notify-asked
  check "the page has the notification permission" 'has text "notif: granted" --contains'
  tapui text "Show a notification" 4
  notif > "$EV/notify-shown.txt"; shot notify-page
  check "an Android notification with the page's text exists" 'grep -q "Hello from the test page" "$EV/notify-shown.txt"'
  check "it names the site" 'grep -q "$SITE" "$EV/notify-shown.txt"'
  # another tab on top, then tap the notification
  view "https://example.com/?n2" 8
  A "cmd statusbar expand-notifications"; sleep 2; shot notify-shade
  WIN=1 dump; WIN=1 tapui text "Krunk a5" 4 --contains
  A "cmd statusbar collapse"; sleep 2
  dump; nodes notify-after-tap; shot notify-after-tap
  check "the tap brings the site's tab back" 'fg notify > /dev/null && has text "Permissions"'
  check "the page heard the click" 'has text "show: clicked" --contains'
  save notify; stop_clean
}

t_permchange() { # after perms: flip the stored answers in Settings and see the page follow
  stop_clean; relay; launch 8; front permchange || return
  privacy; tapto text "Site permissions" 3; shot permchange-sites; dump; nodes permchange-sites
  check "the site is listed with its answers" 'has text "$SITE" --contains'
  tapui text "$SITE" 3 --contains; shot permchange-site; dump; nodes permchange-site
  check "the site's page shows Allow / Deny / Ask per permission" 'has text "Notifications" && has text "Ask"'
  # Notifications: Deny (from t_perms) -> Allow. Rows are: label, then Allow | Deny | Ask under it.
  local y; y=$(echo "$UI" | python3 -I "$HERE/uix.py" find text "Notifications" | cut -d" " -f2)
  tap 193 $((y + 100)) 3
  dump; nodes permchange-android; shot permchange-android
  has text "Allow" && has text "Don" --contains && tapui text "Allow" 3   # Android's own dialog (P-10: asked here, once)
  shot permchange-after
  SU "sqlite3 $DB \"SELECT kind, allow FROM site_permissions WHERE origin = ${Q}$L${Q}\"" > "$EV/permchange-table.txt" 2>&1
  check "the table now allows notifications" 'grep -q "notifications|1" "$EV/permchange-table.txt"'
  A "input keyevent KEYCODE_BACK"; sleep 1; A "input keyevent KEYCODE_BACK"; sleep 1; A "input keyevent KEYCODE_BACK"; sleep 1
  view "$L/perms.html?pc" 8; waitfor text "state:" 15 --contains; dump; nodes permchange-page
  check "the page sees the change without a restart" 'has text "notifications=granted" --contains'
  # Location -> Ask: the site asks again
  privacy; tapto text "Site permissions" 3; tapui text "$L" 3; dump
  y=$(echo "$UI" | python3 -I "$HERE/uix.py" find text "Location" | cut -d" " -f2)
  tap 606 $((y + 100)) 3
  A "input keyevent KEYCODE_BACK"; sleep 1; A "input keyevent KEYCODE_BACK"; sleep 1; A "input keyevent KEYCODE_BACK"; sleep 1
  view "$L/perms.html?pc2" 8; tapui text "Location" 3; waitfor text "Allow" 8; dump; nodes permchange-ask; shot permchange-ask
  check "after Ask the site asks again" 'has text "Know your location" --contains'
  tapui text "Deny" 2
  save permchange
}

t_sitedata() {
  stop_clean; launch 8; front sitedata || return
  view "$B/storage.html?s1" 8; waitfor text "Store data" 20
  tapui text "Store data" 4; dump
  check "the page stored a cookie and localStorage" 'has text "cookie: present" && has text "localStorage: present"'
  sleep 5
  privacy; tapto text "Site data" 5; waitfor text "cookie" 25 --contains
  shot sitedata-list; dump; nodes sitedata-list
  check "the site is listed" 'has text "10.250.136.3" --contains'
  check "with a cookie count" 'has text "cookie" --contains'
  check "the engine answered (no fallback note)" '! has text "could not ask the browser engine" --contains'
  tapui desc "Clear data of 10.250.136.3" 3 --contains
  shot sitedata-confirm; dump; nodes sitedata-confirm
  check "the confirmation names the site and offers the permissions" 'has text "Clear data of 10.250.136.3?" && has text "Also reset its permissions"'
  tapui text "Clear" 6
  shot sitedata-cleared; dump; nodes sitedata-cleared
  A "input keyevent KEYCODE_BACK"; sleep 1; A "input keyevent KEYCODE_BACK"; sleep 2
  view "$B/storage.html?s2" 8; waitfor text "Store data" 20; dump; nodes sitedata-page-after; shot sitedata-page-after
  check "the page no longer sees its cookie or its storage" 'has text "cookie: none" && has text "localStorage: none"'
  save sitedata; stop_clean
}

t_history() { # F-54 / F-59: screens keep their place; the page is not rebuilt
  stop_clean; launch 8; front history || return
  local m0; m0=$(now)
  local n; for n in 1 2 3; do view "$B/index.html?h$n" 5; done
  menu; tapui text "History" 3; shot history-list; dump; nodes history-list
  check "history lists today's visits" 'has text "Today" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 2
  # Settings scrolled down to its privacy rows, Site permissions pushed on top, Back: still there
  privacy; scrollto text "Site data"; dump; nodes history-settings-scrolled
  local before; before=$(echo "$UI" | python3 -I "$HERE/uix.py" find text "Site data" | cut -d" " -f2)
  tapui text "Site permissions" 3; dump; check "Site permissions is on top" 'has text "Search sites" || has text "No site has a remembered answer" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 2; dump; nodes history-settings-back; shot history-settings-back
  local after; after=$(echo "$UI" | python3 -I "$HERE/uix.py" find text "Site data" | cut -d" " -f2)
  check "Settings kept its scroll position ($before -> $after)" '[ -n "$before" ] && [ "$before" = "$after" ]'
  A "input keyevent KEYCODE_BACK"; sleep 2; dump; shot history-page-back; nodes history-page-back
  check "back on the page with its toolbar, page not reloaded" 'has desc "Menu" && has text "Krunk a5 tests" --contains && [ "$(since "$m0" | grep -c "index.html?h3")" = 1 ]'
  # typing still reaches the page chrome after a covered screen (focus / keyboard)
  go "$B/storage.html?h9" 8; dump
  check "the address bar works after returning" 'has text "Store data"'
  save history; stop_clean
}

t_pointer() { # recorded, never a failure: what requestPointerLock() does with a mouse
  launch 8; front pointer || return
  view "$B/pointerlock.html?p1" 8; shot pointer-before; dump; nodes pointer-before
  A "input mouse tap 540 900"; sleep 2
  dump; nodes pointer-after-click; shot pointer-locked
  A "input mouse motionevent MOVE 600 1000" 2> /dev/null; A "input mouse swipe 540 900 800 1200 400" 2> /dev/null; sleep 2
  dump; nodes pointer-after-move; shot pointer-moved
  echo "$UI" | python3 -I "$HERE/uix.py" list | grep -iE "lock|movement|pointer" > "$EV/pointer-result.txt"
  cat "$EV/pointer-result.txt"
  A "input keyevent KEYCODE_BACK"; sleep 2; dump; nodes pointer-after-back
  save pointer; stop_clean
}

[ $# -gt 0 ] || { sed -n '2,24p' "$0"; exit 2; }
for t in "$@"; do
  echo "== $t"
  if declare -F "t_$t" > /dev/null; then "t_$t"; else echo "unknown test $t"; FAIL=$((FAIL + 1)); fi
done
echo "passed $PASS, failed $FAIL, skipped $SKIP"
[ "$FAIL" = 0 ]
