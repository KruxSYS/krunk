#!/usr/bin/env bash
# Device checks for the password vault of Krunk for Android (0.3.0), over adb against the sandbox
# emulator. Built on the 0.2.0 harness (../a3): same helpers, same test server.
#   a4.sh <test...>   in this order for a full pass (each test starts the app itself):
#     create     fresh install: create the vault (too short, mismatch), lock, wrong password,
#                rate-limit message, unlock
#     save       sign in on the http test page: warning, save; on return the picker fills;
#                a changed password offers "Update"
#     https      a public https login form: save without a warning, fill on return
#     locked     locked vault: quiet on load, unlock asked at the tap on the field
#     private    private tab: no picker, no save offer; "Fill from vault" fills
#     auth       HTTP sign-in dialog: save after success, saved login offered next time
#     screen     vault screen: search, view, show, edit, delete; blank screenshot
#     timer      lock after 1 minute without use
#     grace      lock after leaving Krunk: back within 30 s still open, later locked
#     keylost    (debug build) the Keystore key removed: the "cannot be opened" screen, delete
#     fixture    (debug build) writes the vault file and its inner twin for the format test
#     nobio      no fingerprint enrolled: the switch is disabled with its reason
#   Last, because they can wedge the emulator's display:
#     config     rotation with the unlock dialog open, dark mode
#   upgrade      $KRUNK_OLD_APK (the published 0.2.0) with data, this release over it, then a vault
# REL=1 tests the release APK (no debug hooks: keylost and fixture are skipped by name).
# The test server must run:  python3 -I android/scripts/a3/server.py android/scripts/a4/www 18080 >> "$KRUNK_A3_ROOT/server.log"
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")/../a3" && pwd)
A4=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$HERE/lib.sh"
mkdir -p "$EV"
PASS=0; FAIL=0; SKIP=0
REL=${REL:-}; APK=$DEBUG_APK; [ -n "$REL" ] && APK=$RELEASE_APK
# The 0.2.0 harness's UI helpers (check, dump, has, tapui, menu, scrollto, ...), taken from its
# script so both stay the same.
# shellcheck disable=SC1090
source <(sed -n '/^dcheck() {/,/^privacy() {/p' "$HERE/a3.sh")
# A failed dump must not leave the previous screen's nodes to be read again.
dump() { A "rm -f /sdcard/ui.xml; uiautomator dump ${WIN:+--windows} /sdcard/ui.xml" > /dev/null 2>&1; UI=$(A "cat /sdcard/ui.xml" 2> /dev/null); }
PW=fixture-password-1
PW2=second-password-22
LOGIN="$B/login.html"
HTTPS_LOGIN=https://the-internet.herokuapp.com/login

type_text() { A "input text '$1'"; sleep 1; }
field() { tapui text "$1" 1 && type_text "$2"; }
refield() { tapui text "$1" 1 && A "input keycombination 113 29" && A "input keyevent KEYCODE_DEL" && type_text "$2"; }   # clear first            # field LABEL VALUE: tap a text field by its label, type
hidekb() { dump; A "dumpsys input_method" 2> /dev/null | grep -q "mInputShown=true" && { A "input keyevent KEYCODE_BACK"; sleep 1; }; return 0; }
waitfor() { # waitfor text|desc VALUE [seconds] [--contains]
  local i; for i in $(seq 1 "${3:-15}"); do dump; has "$1" "$2" ${4:-} && return 0; sleep 1; done; return 1
}
shotblack() { # shotblack NAME: the screenshot must be blank (FLAG_SECURE): mean brightness 0
  shot "$1" || return 4
  local mean; mean=$(convert "$EV/$1.png" -colorspace Gray -format "%[fx:mean*1000]" info: 2> /dev/null | cut -d. -f1)
  check "screenshot of $1 is blank (secure window)" "[ \"${mean:-999}\" -le 2 ]"
}
passwords() { menu && tapui text "Passwords" 3; }
unlock_dialog() { waitfor text "Unlock vault" 8 || waitfor text "The vault is locked" 3; field "Vault password" "${1:-$PW}"; hidekb; tapui text "Unlock" 4; }
dbgv() { dbg "$@"; sleep 2; T "$E" adb logcat -d -v time -s KrunkGuard:I | grep "debug: vault" | tail -1 | sed 's/.*debug: //'; }
vstate() { dbgv vault-status | grep -oE 'state=[A-Z_]+' | cut -d= -f2; }
pagevals() { dump; echo "$UI" | python3 -I "$HERE/uix.py" list | grep -oE "u=[^' ]* p=[^' ]*" | head -1; }
open_login() { view "$1" 8; waitfor text "Sign in" 20 --contains || bad "login page did not load ($1)"; }
# The test login page has a fixed layout (toolbar at the top); its fields have no names for
# the accessibility tree, so they are tapped by position.
tap_user() { tap 518 514 3; }
tap_pass() { tap 518 688 2; }
submit() { tap 480 862 "${1:-4}"; }

t_create() {
  fresh "$APK"; launch 9; front create || return
  passwords; waitfor text "Create vault" 10 || { missing "Create vault"; return; }
  shotblack create-none
  dump; check "the form says there is no recovery" 'has text "cannot be opened" --contains'
  field "Vault password" short1; field "Repeat the password" short1; hidekb; tapui text "Create vault" 2
  dump; check "a 6-character password is refused" 'has text "Use at least" --contains'
  refield "Vault password" "$PW"; refield "Repeat the password" "${PW}x"; hidekb; tapui text "Create vault" 2
  dump; check "two different passwords are refused" 'has text "The two passwords are not the same"'
  refield "Repeat the password" "$PW"; hidekb; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains; check "vault created and open (empty list)" 'has text "No logins yet" --contains'
  [ -z "$REL" ] && check "debug: state UNLOCKED" '[ "$(vstate)" = UNLOCKED ]'
  shotblack create-open
  tapui desc "More" 2; tapui text "Lock now" 3
  waitfor text "The vault is locked" 6 --contains; check "Lock now locks" 'has text "The vault is locked" --contains'
  # wrong password, then at once again: the second attempt is held back by the rate limit
  field "Vault password" wrong-password-1; hidekb; tapui text "Unlock" 4
  waitfor text "Wrong password" 8 --contains; check "wrong password is said" 'has text "Wrong password" --contains'
  tapui text "Unlock" 3; tapui text "Unlock" 1
  dump; check "rate-limit message" 'has text "Too many attempts" --contains'
  echo "$UI" | python3 -I "$HERE/uix.py" list > "$EV/create-ratelimit-nodes.txt"
  shotblack create-ratelimit
  sleep 8
  refield "Vault password" "$PW"; hidekb; tapui text "Unlock" 5
  waitfor text "No logins yet" 15 --contains
  check "the right password unlocks after the wait" 'has text "No logins yet" --contains'
  stop_clean
}

t_save() { # plain http page: warning before save and before fill
  launch 8; front save || return
  open_login "$LOGIN?save"
  tap_user; type_text alice; hidekb; tap_pass; type_text s3cret-one; hidekb
  local m; m=$(now); submit 4
  waitfor text "Save this sign-in?" 10; check "save offer after submit" 'has text "Save this sign-in?"'
  dump; check "the offer names user and site, not the password" 'has text "alice on" --contains && ! has text "s3cret-one" --contains'
  shot save-offer
  tapui text "Save" 2
  waitfor text "This page is not encrypted" 5; check "http: warning before saving" 'has text "This page is not encrypted"'
  shot save-http-warning
  tapui text "Save anyway" 3
  dump; has text "Unlock vault" && unlock_dialog "$PW"
  sleep 3
  # on return: nothing filled by itself, the picker at the tap, warning first
  open_login "$LOGIN?return"
  check "nothing is filled without asking" '[ "$(pagevals)" = "u= p=" ]'
  tap_user
  waitfor text "This page is not encrypted" 8; check "http: warning before the picker" 'has text "This page is not encrypted"'
  tapui text "Fill in anyway" 3
  dump; has text "Unlock vault" && unlock_dialog "$PW"
  waitfor text "Choose a saved sign-in:" 8; check "picker lists the saved login" 'has text "alice"'
  shotblack save-picker
  tapui text "alice" 4; hidekb
  check "the form is filled" '[ "$(pagevals)" = "u=alice p=s3cret-one" ]'
  shot save-filled
  # same login again: no offer
  submit 5; dump
  check "an unchanged login is not offered again" '! has text "Save this sign-in?" && ! has text "Update the saved password?"'
  # changed password: update
  open_login "$LOGIN?update"
  tap_user; sleep 2; dump; has text "Fill in anyway" && { A "input keyevent KEYCODE_BACK"; sleep 1; }
  tap_user; type_text alice; hidekb; tap_pass; type_text s3cret-new; hidekb; submit 4
  waitfor text "Update the saved password?" 10; check "a changed password offers Update" 'has text "Update the saved password?"'
  shot save-update
  tapui text "Update" 2; dump; has text "Save anyway" && tapui text "Save anyway" 3
  stop_clean
}

t_newvault() { # a vault with the test password on the installed app (after keylost)
  launch 8; front newvault || return
  passwords; waitfor text "Create vault" 10 || { missing "Create vault"; return; }
  field "Vault password" "$PW"; field "Repeat the password" "$PW"; hidekb; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains; check "vault created" 'has text "No logins yet" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  stop_clean
}

t_novault() { # no vault yet: the save offer leads to "Create vault", and the login lands in the new vault
  fresh "$APK"; launch 9; front novault || return
  open_login "$LOGIN?novault"
  tap_user; type_text carol; hidekb; tap_pass; type_text first-pass-1; hidekb; submit 4
  waitfor text "Save this sign-in?" 10; check "no vault: the offer is to create one" 'has text "Create vault" && has text "encrypted vault" --contains'
  shot novault-offer
  tapui text "Create vault" 2; dump; has text "Save anyway" && tapui text "Save anyway" 3
  waitfor text "Repeat the password" 8 || { missing "create form"; return; }
  field "Vault password" "$PW"; field "Repeat the password" "$PW"; hidekb; tapui text "Create vault" 6
  waitfor text "carol" 15; check "the login that waited is in the new vault" 'has text "carol"'
  # change the vault password; the old one no longer unlocks
  tapui desc "More" 2; tapui text "Change password" 2
  field "Current password" "$PW"; field "New password" "$PW2"; field "Repeat the password" "$PW2"; hidekb
  dump; echo "$UI" | python3 -I "$HERE/uix.py" list > "$EV/novault-change-nodes.txt"
  tapui text "Change password" 6 || tapui text "Change" 6
  tapui desc "More" 2; tapui text "Lock now" 3
  field "Vault password" "$PW"; hidekb; tapui text "Unlock" 5
  dump; check "the old password is refused after a change" 'has text "Wrong password" --contains'
  sleep 3; refield "Vault password" "$PW2"; hidekb; tapui text "Unlock" 5
  waitfor text "carol" 10; check "the new password unlocks" 'has text "carol"'
  stop_clean
}

t_https() { # a public https login form (fields tapped by position: 1080x2400, toolbar at the top)
  launch 8; front https || return
  view "$HTTPS_LOGIN" 12; waitfor text "Login Page" 30 || { skip "https test site not reachable"; stop_clean; return; }
  tap 500 1195 2; type_text tomsmith; hidekb; tap 500 1360 2; type_text 'SuperSecretPassword!'; hidekb
  tapui text " Login" 6
  waitfor text "Save this sign-in?" 10; check "https: save offer" 'has text "Save this sign-in?"'
  shot https-offer
  tapui text "Save" 3
  dump; check "https: no warning" '! has text "This page is not encrypted"'
  has text "Unlock vault" && unlock_dialog "$PW"; sleep 2
  view "$HTTPS_LOGIN" 12; waitfor text "Login Page" 30
  tap 500 1195 3
  dump; check "https: no warning before the picker" '! has text "This page is not encrypted"'
  has text "Unlock vault" && unlock_dialog "$PW"
  waitfor text "Choose a saved sign-in:" 8; check "https: the picker shows" 'has text "Choose a saved sign-in:"'
  shotblack https-picker
  tapui text "tomsmith" 4; hidekb; shot https-filled
  tapui text " Login" 8; waitfor text "Secure Area" 15 --contains
  check "https: signed in with the filled login" 'has text "Secure Area" --contains'
  dump; check "https: no offer for the unchanged login" '! has text "Save this sign-in?"'
  stop_clean
}

t_locked() { # after a cold start the vault is locked: quiet on load, asks at the tap
  launch 8; front locked || return
  open_login "$LOGIN?locked"; sleep 3; dump
  check "locked vault is quiet on page load" '! has text "Unlock vault" && ! has text "This page is not encrypted"'
  tap_user; waitfor text "This page is not encrypted" 8; tapui text "Fill in anyway" 3
  waitfor text "Unlock vault" 6; check "unlock is asked at the tap" 'has text "Unlock vault"'
  shotblack locked-unlock
  unlock_dialog "$PW"; waitfor text "Choose a saved sign-in:" 8
  check "after unlock the picker shows" 'has text "alice"'
  tapui text "alice" 4; hidekb
  check "filled after unlock" 'pagevals | grep -q "u=alice p="'
  stop_clean
}

t_private() {
  launch 8; front private || return
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"; sleep 2; A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "New private tab" 3
  tapui desc "Edit address" 2; type_text "${LOGIN#http://}?private"; A "input keyevent KEYCODE_ENTER"; sleep 9
  [ -z "$REL" ] && A "setprop debug.krunk.show_private 1"
  tap_user; sleep 4; dump
  check "private tab: no picker, no warning at the tap" '! has text "Choose a saved sign-in:" && ! has text "This page is not encrypted" && ! has text "Unlock vault"'
  type_text privuser; hidekb; tap_pass; type_text privpw99; hidekb; submit 5; dump
  check "private tab: no save offer" '! has text "Save this sign-in?"'
  A "input keyevent KEYCODE_BACK"; sleep 4
  menu; tapui text "Fill from vault" 2
  tap_user; waitfor text "This page is not encrypted" 8; tapui text "Fill in anyway" 3
  waitfor text "Choose a saved sign-in:" 8; check "private tab: picker after Fill from vault" 'has text "alice"'
  tapui text "alice" 4; hidekb
  check "private tab: filled on request" 'pagevals | grep -q "u=alice p="'
  shot private-filled
  stop_clean
}

t_auth() {
  launch 8; front auth || return
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"; sleep 2; A "input keyevent KEYCODE_BACK"; sleep 1
  view "$B/auth?1" 6; waitfor text "Sign in to" 10 --contains || { missing "sign-in dialog"; return; }
  field "User name" krunk; field "Password" test; hidekb; tapui text "Sign in" 6
  waitfor text "Save this sign-in?" 10; check "HTTP sign-in: save offer after success" 'has text "Save this sign-in?"'
  tapui text "Save" 2; dump; has text "Save anyway" && tapui text "Save anyway" 3
  stop_clean; launch 8   # a new process: the engine has forgotten the session's sign-in, the vault is locked
  view "$B/auth?2" 6; waitfor text "Sign in to" 12 --contains || { missing "sign-in dialog"; return; }
  dump; check "HTTP sign-in, vault locked: unlock is offered, no login is shown" 'has text "Unlock vault for saved sign-ins" && ! has text "Use krunk"'
  shot auth-locked
  tapui text "Unlock vault for saved sign-ins" 2; unlock_dialog "$PW"
  waitfor text "Use krunk" 8; check "HTTP sign-in: the saved login is offered" 'has text "Use krunk"'
  shotblack auth-offer
  tapui text "Use krunk" 2
  waitfor text "This page is not encrypted" 4; check "HTTP sign-in over http: confirmation" 'has text "Use anyway"'
  tapui text "Use anyway" 2; tapui text "Sign in" 6
  waitfor text "Signed in as krunk" 10; check "signed in with the saved login" 'has text "Signed in as krunk"'
  dump; check "no offer for an unchanged login" '! has text "Save this sign-in?"'
  stop_clean
}

# Start for the tests that begin in the vault screen: a restored tab may be asking to sign in.
to_vault() {
  launch 8; dump; has text "Cancel" && has text "Sign in to" --contains && tapui text "Cancel" 2
  passwords; dump; has text "The vault is locked" --contains && { field "Vault password" "$PW"; hidekb; tapui text "Unlock" 5; }
  waitfor desc "Add login" 10
}
vault_settings() { tapui desc "More" 2 && tapui text "Settings" 2; }

t_screen() {
  to_vault; front screen || return
  shotblack screen-list
  dump; check "the list has the saved logins" 'has text "alice" && has text "krunk"'
  tapui desc "Search" 2; type_text kru; hidekb; dump
  check "search narrows the list" 'has text "krunk" && ! has text "alice"'
  tapui text "krunk" 3
  dump; check "the password is hidden at first" 'has desc "Password, hidden" && ! has text "test"'
  tapui text "Show" 2; dump; check "Show reveals it" 'has text "test"'
  tapui text "Edit" 2
  tapui text "krunk" 1; type_text 2; hidekb; tapui text "Save" 3
  A "input keyevent KEYCODE_BACK"; sleep 1; dump; has text "Close" && tapui text "Close" 1
  tapui desc "Close" 2; dump
  check "the edited user name is in the list" 'has text "krunk2" --contains'
  tapui text "krunk2" 3 --contains; tapui text "Delete" 2
  dump; check "delete asks first, naming site and user" 'has text "Delete this login?" && has text "krunk2" --contains'
  shot screen-delete-confirm
  tapui text "Delete" 3; sleep 1; dump; has text "Close" && tapui text "Close" 1
  dump; check "the login is gone from the list" '! has text "krunk2" --contains && has text "alice"'
  stop_clean
}

t_timer() { # 1 minute
  to_vault; front timer || return
  vault_settings; tapui text "1 minute" 2 --contains; A "input keyevent KEYCODE_BACK"; sleep 1
  shot timer-setting
  sleep 80; dump
  check "locked after a minute without use" 'has text "The vault is locked" --contains'
  shotblack timer-locked
  field "Vault password" "$PW"; hidekb; tapui text "Unlock" 5; waitfor desc "Add login" 8; check "unlocks again" 'has desc "Add login"'
  stop_clean
}

t_grace() { # 30 s grace
  to_vault; front grace || return
  vault_settings; tapto text "After 30 seconds" 2; tapui text "15 minutes" 2 --contains; A "input keyevent KEYCODE_BACK"; sleep 1
  A "input keyevent KEYCODE_HOME"; sleep 10
  shot grace-recents-home
  launch 3; dump; check "back after 10 s: still open" 'has desc "Add login"'
  A "input keyevent KEYCODE_HOME"; sleep 40
  launch 3; dump; check "back after 40 s: locked" 'has text "The vault is locked" --contains'
  shotblack grace-locked
  field "Vault password" "$PW"; hidekb; tapui text "Unlock" 5; waitfor desc "Add login" 8; check "unlocks again after the background lock" 'has desc "Add login"'
  A "input keyevent KEYCODE_APP_SWITCH"; sleep 3; shot grace-recents; A "input keyevent KEYCODE_HOME"; sleep 1
  stop_clean
}

t_nobio() {
  to_vault; front nobio || return
  vault_settings; scrollto text "Unlock with fingerprint"
  dump; check "no fingerprint enrolled: the reason is shown" 'has text "No fingerprint enrolled on this device"'
  check "the switch is disabled" '[ "$(attr text "Unlock with fingerprint" enabled --clickable)" = false ]'
  A "am broadcast -a org.krunk.browser.DEBUG --es cmd vault-shot" > /dev/null 2>&1; shot nobio-raw
  stop_clean
}

add_login() { # add_login SITE USER PASSWORD, in the open vault screen
  tapui desc "Add login" 2; field "Site" "$1"; field "User name" "$2"; field "Password" "$3"; hidekb; tapui text "Save" 3
  dump; has text "Close" && tapui text "Close" 1; return 0
}

t_fixture() { # the vault file for the cross-platform format test, made by the app with the real Keystore
  [ -n "$REL" ] && { skip "fixture (needs a debug build)"; return; }
  fresh "$APK"; launch 9; front fixture || return
  passwords; waitfor text "Create vault" 10
  field "Vault password" "fixture%spassword"; field "Repeat the password" "fixture%spassword"; hidekb; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains || { missing "open vault"; return; }
  add_login https://bank.example alice s3cret-one
  add_login https://mail.example:8443 bob s3cret-two
  dump; check "both logins are in the list" 'has text "alice" && has text "bob"'
  echo "fixture: $(dbgv vault-fixture)"
  local d="$EV/fixture"; mkdir -p "$d"
  T "$E" adb exec-out run-as $PKG cat files/krunk-vault/vault.json > "$d/vault-android-keystore.json"
  T "$E" adb exec-out run-as $PKG cat files/krunk-vault/vault-inner.json > "$d/vault-android-inner.json"
  A "run-as $PKG ls -la files/krunk-vault/" > "$d/files.txt"
  A "run-as $PKG rm files/krunk-vault/vault-inner.json"
  A "getprop ro.build.fingerprint" > "$d/device.txt"; A "dumpsys package $PKG" | grep -m2 -E "versionName|versionCode" >> "$d/device.txt"
  check "both files pulled" '[ -s "$d/vault-android-keystore.json" ] && [ -s "$d/vault-android-inner.json" ]'
  stop_clean
}

t_keylost() {
  [ -n "$REL" ] && { skip "keylost (needs a debug build)"; return; }
  launch 8; front keylost || return
  dump; has text "Cancel" && has text "Sign in to" --contains && tapui text "Cancel" 2
  echo "drop key: $(dbgv vault-drop-key)"
  A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; launch 8
  dump; has text "Cancel" && has text "Sign in to" --contains && tapui text "Cancel" 2
  passwords; waitfor text "This vault cannot be opened" 8
  check "key lost: the screen says so, no password is asked" 'has text "This vault cannot be opened" && ! has text "Unlock"'
  shotblack keylost
  check "the vault file is still there" 'A "run-as $PKG ls files/krunk-vault/" | grep -q vault.json'
  tapui text "Delete this vault" 2; field "Type DELETE" delete; hidekb; tapui text "Delete this vault" 2
  dump; check "lower-case confirmation is refused" 'has text "capital letters" --contains'
  refield "Type DELETE" DELETE; hidekb; tapui text "Delete this vault" 3
  waitfor text "Create vault" 8; check "after deleting: no vault, Create offered" 'has text "Create vault"'
  check "the vault file is gone" '! A "run-as $PKG ls files/krunk-vault/" | grep -q vault.json'
  stop_clean
}

t_config() {
  launch 8; front config || return
  passwords; waitfor text "Vault password" 8
  A "settings put system accelerometer_rotation 0"; A "settings put system user_rotation 1"; sleep 4
  dump; check "rotated: the unlock field is still there" 'has text "Vault password"'
  shotblack config-landscape
  A "settings put system user_rotation 0"; sleep 4
  A "cmd uimode night yes"; sleep 4; dump; check "dark mode: still the vault screen" 'has text "Vault password"'
  A "cmd uimode night no"; sleep 3
  stop_clean
}

t_upgrade() {
  [ -n "${KRUNK_OLD_APK:-}" ] || { skip "upgrade (KRUNK_OLD_APK not set)"; return; }
  fresh "$KRUNK_OLD_APK"; launch 14
  view "$B/landing.html?old=1" 8; view "$B/login.html?old=2" 8
  menu; tapui text "Bookmark this page" 3
  stop_clean
  T "$E" adb install -r "$APK" 2>&1 | tail -1
  launch 12; dump; has text "Restore" --contains && tapui text "Restore" 3 --contains
  A "dumpsys package $PKG" | grep -m1 versionName
  shot upgrade-started
  tapui desc "Tabs" 2 --contains; shot upgrade-tabs; A "input keyevent KEYCODE_BACK"; sleep 1
  menu; tapui text "Bookmarks" 3; dump; check "bookmark kept" 'has text "login" --contains'; A "input keyevent KEYCODE_BACK"; sleep 1
  passwords; waitfor text "Create vault" 10
  field "Vault password" "$PW"; field "Repeat the password" "$PW"; hidekb; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains; check "upgrade: a vault can be created" 'has text "No logins yet" --contains'
  stop_clean
}

for t in "$@"; do
  echo "=== $t"; "t_$t"; [ "$(A "echo up" 2> /dev/null | tr -d "\r")" = up ] || { echo "DEVICE NOT ANSWERING"; exit 4; }
done
echo "RESULT pass=$PASS fail=$FAIL skip=$SKIP"
[ "$FAIL" -eq 0 ]
