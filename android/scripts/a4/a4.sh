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
PW=fixture-password-1
PW2=second-password-22
LOGIN="$B/login.html"
HTTPS_LOGIN=https://the-internet.herokuapp.com/login

type_text() { A "input text '$1'"; sleep 1; }
field() { tapui text "$1" 1 && type_text "$2"; }            # field LABEL VALUE: tap a text field by its label, type
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
pagevals() { dump; echo "$UI" | python3 -I "$HERE/uix.py" list | grep -oE 'u=[^ ]* p=[^ ]*' | head -1; }
open_login() { view "$1" 8; waitfor text "Sign in" 20 --contains || bad "login page did not load ($1)"; }
tap_user() { tapui text "user" 3 || true; }

t_create() {
  fresh "$APK"; launch 9; front create || return
  passwords; waitfor text "Create vault" 10 || { missing "Create vault"; return; }
  shotblack create-none
  tapui text "Create vault" 2
  field "Vault password" short1; field "Repeat the password" short1; hidekb; tapui text "Create vault" 2
  dump; check "a 6-character password is refused" 'has text "Use at least" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1; tapui text "Create vault" 2
  field "Vault password" "$PW"; field "Repeat the password" "${PW}x"; hidekb; tapui text "Create vault" 2
  dump; check "two different passwords are refused" 'has text "The two passwords are not the same"'
  A "input keyevent KEYCODE_BACK"; sleep 1; tapui text "Create vault" 2
  dump; check "the dialog says there is no recovery" 'has text "cannot be opened" --contains'
  field "Vault password" "$PW"; field "Repeat the password" "$PW"; hidekb; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains; check "vault created and open (empty list)" 'has text "No logins yet" --contains'
  [ -z "$REL" ] && check "debug: state UNLOCKED" '[ "$(vstate)" = UNLOCKED ]'
  shotblack create-open
  tapui desc "More" 2 --contains || tapui desc "Menu" 2; tapui text "Lock now" 3
  waitfor text "The vault is locked" 6; check "Lock now locks" 'has text "The vault is locked" --contains'
  # wrong password, twice: the second attempt is held back by the rate limit
  field "Vault password" wrong-password-1; hidekb; tapui text "Unlock" 4
  waitfor text "Wrong password" 8; check "wrong password is said" 'has text "Wrong password" --contains'
  tapui text "Unlock" 3; tapui text "Unlock" 1
  dump; check "rate-limit message" 'has text "Too many attempts" --contains'
  shotblack create-ratelimit
  [ -z "$REL" ] && { A "am broadcast -a org.krunk.browser.DEBUG --es cmd vault-shot --es name create-ratelimit" > /dev/null; }
  sleep 6
  A "input keyevent KEYCODE_BACK"; sleep 1; passwords
  unlock_dialog "$PW"; waitfor text "No logins yet" 15 --contains
  check "the right password unlocks after the wait" 'has text "No logins yet" --contains'
  stop_clean
}

t_save() { # plain http page: warning before save and before fill
  launch 8; front save || return
  open_login "$LOGIN?save"
  tap_user; type_text alice; hidekb; tapui text "password" 2; type_text s3cret-one; hidekb
  local m; m=$(now); tapui text "Sign in" 4
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
  tapui text "Sign in" 5; dump
  check "an unchanged login is not offered again" '! has text "Save this sign-in?" && ! has text "Update the saved password?"'
  # changed password: update
  open_login "$LOGIN?update"
  tap_user; sleep 2; dump; has text "Fill in anyway" && { A "input keyevent KEYCODE_BACK"; sleep 1; }
  tap_user; type_text alice; hidekb; tapui text "password" 2; type_text s3cret-new; hidekb; tapui text "Sign in" 4
  waitfor text "Update the saved password?" 10; check "a changed password offers Update" 'has text "Update the saved password?"'
  shot save-update
  tapui text "Update" 2; dump; has text "Save anyway" && tapui text "Save anyway" 3
  stop_clean
}

t_https() {
  launch 8; front https || return
  view "$HTTPS_LOGIN" 12; waitfor text "Login Page" 30 --contains || { skip "https test site not reachable"; stop_clean; return; }
  tapui text "Username" 3 --contains || A "input tap 400 900"; type_text tomsmith; hidekb
  tapui text "Password" 2 --contains; type_text 'SuperSecretPassword!'; hidekb
  A "input keyevent KEYCODE_ENTER"; sleep 6
  waitfor text "Save this sign-in?" 10; check "https: save offer" 'has text "Save this sign-in?"'
  tapui text "Save" 3
  dump; check "https: no warning" '! has text "This page is not encrypted"'
  has text "Unlock vault" && unlock_dialog "$PW"; sleep 2
  view "$HTTPS_LOGIN" 12; waitfor text "Login Page" 30 --contains
  tapui text "Username" 3 --contains
  dump; has text "Unlock vault" && unlock_dialog "$PW"
  waitfor text "Choose a saved sign-in:" 8; check "https: picker without warning" 'has text "tomsmith"'
  shotblack https-picker
  tapui text "tomsmith" 4; hidekb; shot https-filled
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
  tapui desc "Address" 2 --contains || A "input tap 400 210"; type_text "${LOGIN#http://}?private"; A "input keyevent KEYCODE_ENTER"; sleep 9
  [ -z "$REL" ] && A "setprop debug.krunk.show_private 1"
  tap_user; sleep 4; dump
  check "private tab: no picker, no warning at the tap" '! has text "Choose a saved sign-in:" && ! has text "This page is not encrypted" && ! has text "Unlock vault"'
  type_text privuser; hidekb; tapui text "password" 2; type_text privpw99; hidekb; tapui text "Sign in" 5; dump
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
  A "am force-stop $PKG"; sleep 1; launch 8   # the engine forgets the session's credentials
  dump; has text "Restore" --contains && tapui text "Restore" 3 --contains
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"; sleep 2; A "input keyevent KEYCODE_BACK"; sleep 1
  view "$B/auth?2" 6; waitfor text "Sign in to" 10 --contains
  waitfor text "Use krunk" 6; check "HTTP sign-in: the saved login is offered" 'has text "Use krunk"'
  shotblack auth-offer || true; shot auth-offer-raw
  tapui text "Use krunk" 2
  waitfor text "This page is not encrypted" 4; check "HTTP sign-in over http: confirmation" 'has text "Use anyway"'
  tapui text "Use anyway" 2; tapui text "Sign in" 6
  waitfor text "Signed in as krunk" 10; check "signed in with the saved login" 'has text "Signed in as krunk"'
  dump; check "no offer for an unchanged login" '! has text "Save this sign-in?"'
  stop_clean
}

t_screen() {
  launch 8; front screen || return
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"
  waitfor text "Search logins" 8
  shotblack screen-list
  dump; check "the list has the saved logins" 'has text "alice" && has text "krunk"'
  field "Search logins" kru; hidekb; dump
  check "search narrows the list" 'has text "krunk" && ! has text "alice"'
  tapui text "krunk" 3
  dump; check "the password is hidden at first" '! has text "test"'
  tapui text "Show" 2; dump; check "Show reveals it" 'has text "test"'
  tapui text "Edit" 2 || tapui desc "Edit" 2
  field "User name" 2; hidekb; tapui text "Save" 3
  dump; check "edited user name is shown" 'has text "krunk2" --contains'
  tapui text "Delete" 2 || tapui desc "Delete" 2
  dump; check "delete asks first" 'has text "Delete this login?"'
  tapui text "Delete" 3
  A "input keyevent KEYCODE_BACK"; sleep 1; dump
  stop_clean
}

t_timer() { # 1 minute
  launch 8; front timer || return
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"
  tapui desc "More" 2 --contains; tapui text "Settings" 2; tapui text "1 minute" 2 --contains; A "input keyevent KEYCODE_BACK"; sleep 1
  shot timer-setting
  sleep 80; dump
  check "locked after a minute without use" 'has text "The vault is locked" --contains'
  shotblack timer-locked
  unlock_dialog "$PW"; waitfor text "Search logins" 8; check "unlocks again" 'has text "Search logins"'
  stop_clean
}

t_grace() { # 30 s grace
  launch 8; front grace || return
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"
  tapui desc "More" 2 --contains; tapui text "Settings" 2; tapto text "After 30 seconds" 2; tapui text "15 minutes" 2 --contains; A "input keyevent KEYCODE_BACK"; sleep 1
  A "input keyevent KEYCODE_HOME"; sleep 10
  shot grace-recents-home
  launch 3; dump; check "back after 10 s: still open" 'has text "Search logins"'
  A "input keyevent KEYCODE_HOME"; sleep 40
  launch 3; dump; check "back after 40 s: locked" 'has text "The vault is locked" --contains'
  shotblack grace-locked
  unlock_dialog "$PW"; waitfor text "Search logins" 8; check "unlocks again after the background lock" 'has text "Search logins"'
  A "input keyevent KEYCODE_APP_SWITCH"; sleep 3; shot grace-recents; A "input keyevent KEYCODE_HOME"; sleep 1
  stop_clean
}

t_nobio() {
  launch 8; front nobio || return
  passwords; dump; has text "The vault is locked" --contains && unlock_dialog "$PW"
  tapui desc "More" 2 --contains; tapui text "Settings" 2; scrollto text "Unlock with fingerprint"
  dump; check "no fingerprint enrolled: the reason is shown" 'has text "No fingerprint enrolled on this device"'
  check "the switch is disabled" '[ "$(attr text "Unlock with fingerprint" enabled --clickable)" = false ]'
  A "am broadcast -a org.krunk.browser.DEBUG --es cmd vault-shot" > /dev/null 2>&1; shot nobio-raw
  stop_clean
}

t_fixture() {
  [ -n "$REL" ] && { skip "fixture (needs a debug build)"; return; }
  launch 8; front fixture || return
  echo "fixture: $(dbgv vault-fixture)"
  local d="$EV/fixture"; mkdir -p "$d"
  T "$E" adb exec-out run-as $PKG cat files/krunk-vault/vault.json > "$d/vault-android-keystore.json"
  T "$E" adb exec-out run-as $PKG cat files/krunk-vault/vault-inner.json > "$d/vault-android-inner.json"
  A "run-as $PKG rm files/krunk-vault/vault-inner.json"
  check "both files pulled" '[ -s "$d/vault-android-keystore.json" ] && [ -s "$d/vault-android-inner.json" ]'
  stop_clean
}

t_keylost() {
  [ -n "$REL" ] && { skip "keylost (needs a debug build)"; return; }
  launch 8; front keylost || return
  echo "drop key: $(dbgv vault-drop-key)"
  A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; launch 8
  passwords; waitfor text "This vault cannot be opened" 8
  check "key lost: the screen says so, no password is asked" 'has text "This vault cannot be opened" && ! has text "Unlock"'
  shotblack keylost
  check "the vault file is still there" 'A "run-as $PKG ls files/krunk-vault/" | grep -q vault.json'
  tapui text "Delete this vault" 2; field "Type DELETE" delete; hidekb; tapui text "Delete this vault" 2
  dump; check "lower-case confirmation is refused" 'has text "capital letters" --contains'
  field "Type DELETE" DELETE; hidekb; tapui text "Delete this vault" 3
  waitfor text "Create vault" 8; check "after deleting: no vault, Create offered" 'has text "Create vault"'
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
  passwords; waitfor text "Create vault" 10; tapui text "Create vault" 2
  field "Vault password" "$PW"; field "Repeat the password" "$PW"; hidekb; tapui text "Create vault" 6
  waitfor text "No logins yet" 15 --contains; check "upgrade: a vault can be created" 'has text "No logins yet" --contains'
  stop_clean
}

for t in "$@"; do
  echo "=== $t"; "t_$t"; alive || { echo "DEVICE NOT ANSWERING"; exit 4; }
done
echo "RESULT pass=$PASS fail=$FAIL skip=$SKIP"
[ "$FAIL" -eq 0 ]
