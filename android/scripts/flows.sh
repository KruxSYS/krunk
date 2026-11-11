#!/usr/bin/env bash
# The round-1 acceptance flows (brief R10), driven over adb against the debug APK, one screenshot
# per flow into $KRUNK_SHOTS (default .pipeline/screenshots/android). Coordinates assume a
# 1080x2400 @ 420 dpi phone (the sandbox emulator and the local pixel_6 AVD) with the toolbar at
# the top (the default since 0.1.1). TOOLBAR=bottom drives an install that has it at the bottom.
#
#   android/scripts/flows.sh                 all flows, in order
#   android/scripts/flows.sh search restore  only these
#
# Flows: load-url, search, suggestions, open-link-new-tab, tab-switcher, private-no-history,
#        bookmark, settings-theme, restore, (tablet)
# Environment: KRUNK_LOCAL_ADB=1 for a locally started emulator; SLOW=1 doubles every wait
# (software-emulated AVD); TOOLBAR=top|bottom (default top).
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
E="$HERE/emu.sh"
APP=org.krunk.browser
SLOW_FACTOR="${SLOW:-1}"
PAGE_WAIT=$((14 * SLOW_FACTOR))

A() { "$E" adb shell "$@"; }
wait_s() { sleep "$(( $1 * SLOW_FACTOR ))"; }
tap() { A input tap "$1" "$2"; wait_s 1; }
longpress() { A input swipe "$1" "$2" "$1" "$2" 1200; wait_s 2; }
typetext() { A input text "$1"; wait_s 1; }
enter() { A input keyevent 66; }
back() { A input keyevent 4; wait_s 1; }
shot() { "$E" shot "$1" > /dev/null; echo "  shot $1"; }
# The toolbar row (y) and the ⋮ menu's rows (y) as laid out on this screen size: the menu opens
# downward from a top toolbar and upward from a bottom one. A top toolbar also moves the page
# down by its height (PAGE_SHIFT) for taps on page content.
case "${TOOLBAR:-top}" in
  top) TB_Y=210; MENU_SHIFT=-493; PAGE_SHIFT=147 ;;
  bottom) TB_Y=2263; MENU_SHIFT=0; PAGE_SHIFT=0 ;;
  *) echo "TOOLBAR must be top or bottom" >&2; exit 2 ;;
esac
MENU_NEW_TAB=$((850 + MENU_SHIFT)); MENU_PRIVATE=$((977 + MENU_SHIFT)); MENU_BOOKMARK=$((1106 + MENU_SHIFT))
MENU_FIND=$((1357 + MENU_SHIFT)); MENU_BOOKMARKS=$((1738 + MENU_SHIFT)); MENU_HISTORY=$((1866 + MENU_SHIFT))
MENU_SETTINGS=$((1990 + MENU_SHIFT))

address() { tap 600 "$TB_Y"; wait_s 1; }
go() { address; typetext "$1"; enter; wait_s "$PAGE_WAIT"; }
menu() { tap 1005 "$TB_Y"; wait_s 1; }
tabs_button() { tap 880 "$TB_Y"; wait_s 3; }
# One string for the device shell, so the SQL's parentheses and spaces reach sqlite3 intact.
# Debug builds only (run-as); an error is printed, not hidden.
db_count() { A "run-as $APP sqlite3 files/krunk/browser.db \"$1\"" | tr -d '\r'; }

fresh() {
  A am force-stop $APP; A pm clear $APP > /dev/null
  A am start -W -n $APP/.MainActivity > /dev/null; wait_s 6
}

flow_load_url() {
  echo "== load-url"; fresh; go "example.com"; shot flow-load-url
}

flow_search() {
  echo "== search"; go "krunk%sbrowser"; shot flow-search
}

flow_suggestions() {
  echo "== suggestions"; address; typetext "exa"; wait_s 3; shot flow-suggestions; back
}

flow_open_link_new_tab() {
  echo "== open-link-new-tab"; go "krunk.209.wf"; longpress 696 $((230 + PAGE_SHIFT)); shot flow-context-menu
  tap 300 1465; wait_s 3; shot flow-open-in-new-tab   # "Open in new tab" row; counter shows 2
}

flow_tab_switcher() {
  echo "== tab-switcher"; tabs_button; shot flow-tab-switcher
  # Close the second card's × then reopen it through the menu's "Reopen closed tab".
  back; menu; shot flow-tab-menu; back
}

flow_private_no_history() {
  echo "== private-no-history"
  before=$(db_count "select count(*) from visits")
  menu; tap 600 $MENU_PRIVATE; wait_s 3; go "neverssl.com"; shot flow-private-no-history
  after=$(db_count "select count(*) from visits")
  echo "  visits before=$before after=$after (must be equal)"
  [[ $before =~ ^[0-9]+$ ]] || { echo "  FAILED: the visit count could not be read" >&2; return 1; }
  [ "$before" = "$after" ] || { echo "  FAILED: a private visit was recorded" >&2; return 1; }
}

flow_bookmark() {
  echo "== bookmark"; tabs_button; back   # back to the switcher and return
  go "example.com"; menu; tap 600 $MENU_BOOKMARK; wait_s 2; shot flow-bookmark-added
  menu; tap 600 $MENU_BOOKMARKS; wait_s 3; shot flow-bookmark-manager; back
}

flow_settings_theme() {
  echo "== settings-theme"; menu; tap 600 $MENU_SETTINGS; wait_s 3; shot flow-settings
  # Appearance > Theme is the first row of the second section: open it and pick Dark.
  A input swipe 540 1800 540 900 300; wait_s 2; shot flow-settings-scrolled
}

flow_restore() {
  echo "== restore"; A am force-stop $APP; wait_s 2
  A am start -W -n $APP/.MainActivity > /dev/null; wait_s 8; shot flow-restore-after
  tabs_button; shot flow-restore-after-switcher; back
}

flow_tablet() {
  echo "== tablet (sw600dp via wm size)"
  trap 'A wm size reset; A wm density reset' EXIT
  A wm size 1600x2560; A wm density 320; wait_s 6
  A am start -n $APP/.MainActivity > /dev/null; wait_s 6; shot shell-tablet
  A wm size reset; A wm density reset; trap - EXIT; wait_s 4
}

if [ $# -eq 0 ]; then
  set -- load-url search suggestions open-link-new-tab tab-switcher private-no-history bookmark settings-theme restore
fi
for f in "$@"; do
  fn="flow_${f//-/_}"
  if declare -f "$fn" > /dev/null; then "$fn"; else echo "unknown flow: $f" >&2; exit 2; fi
done
echo "done"
