#!/usr/bin/env bash
# Device checks for Krunk for Android (since 0.2.0), driven over adb against the sandbox emulator.
#   a3.sh gate [test...]     the blocker's gate and start order:
#                            idle | allow | blocked | update | starts | hold | off | guardlost | noguard
#                            (no test = all, in that order). Run at every uBlock Origin or GeckoView bump.
#                            Extra, not in "all": idle10 (the 10-minute capture and one page load),
#                            afterupdate (the first start after a list update), startlinks (8 cold starts by a link)
#   a3.sh ui [test...]       through the real UI: toolbar | menu | shield | private | settings
#   a3.sh links [test...]    rewrite | handoff | selection
#   a3.sh prompts [test...]  date time datetime month week colour leave repost share file photo recreate
#   a3.sh themes             styles and accent (picker, toolbar, switcher, light and dark, private)
#   a3.sh small [test...]    switcher | desktop | rows
#   a3.sh upgrade            the published APK ($KRUNK_OLD_APK) with data, then this release installed over it
# REL=1 tests the release APK instead of the debug one. A release build has no debug hooks, so
# checks that read the blocker's internal state are skipped by name ("SKIP ... (needs a debug
# build)") and the tests that are only possible with the hooks are replaced by ones that go through
# the UI and the packet capture (see the gate group).
# Evidence (logs, captures, screenshots) goes to $EV. The test server must be running:
#   python3 -I android/scripts/a3/server.py android/scripts/a3/www 18080 >> "$KRUNK_A3_ROOT/server.log"
# Debug builds are driven through the org.krunk.browser.DEBUG broadcast (DebugHooks.kt) and answer
# in the log (tag KrunkGuard). Separate groups with Home, 3 s, force-stop: a force-stop in the
# foreground makes the next start ask "Restore tabs?".
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$HERE/lib.sh"
mkdir -p "$EV"
PASS=0; FAIL=0; SKIP=0
REL=${REL:-}; APK=$DEBUG_APK; [ -n "$REL" ] && APK=$RELEASE_APK
dcheck() { # a check that reads state only a debug build exposes
  if [ -n "$REL" ]; then echo "SKIP  $1 (needs a debug build)"; SKIP=$((SKIP + 1)); else check "$@"; fi
}
skip() { echo "SKIP  $*"; SKIP=$((SKIP + 1)); }
ok() { echo "PASS  $*"; PASS=$((PASS + 1)); }
bad() { echo "FAIL  $*"; FAIL=$((FAIL + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }   # check <label> <shell condition>
dbg() { A "am broadcast -a org.krunk.browser.DEBUG --es cmd $*" > /dev/null; }
alive() { # the device answers and Krunk's process exists; a dead device ends the run (exit 4)
  A "service check activity" 2> /dev/null | grep -q ": found" || { echo "ABORT: the device is not answering (graphics stack wedged?). Results so far: passed $PASS, failed $FAIL"; exit 4; }
  [ -n "$(A "pidof $PKG" 2> /dev/null)" ]
}
front() { # front <label>: Krunk is running and in the foreground, or the test fails here
  if alive && fg "$1" > /dev/null; then return 0; fi
  bad "$1: Krunk is not running in the foreground"; return 1
}
glog() { T "$E" adb logcat -d -v time -s Krunk:I KrunkGuard:I; }
status() { [ -n "$REL" ] && { echo "(no status on a release build)"; return 0; }; dbg status; sleep 1; glog | grep "debug: status" | tail -1 | sed 's/.*debug: status //'; }
packets() { head -1 "$EV/$1-pcap-summary.txt" | grep -oE '[0-9]+ packets' | cut -d' ' -f1; }
since() { tail -n +"$(($1 + 1))" "$ROOT/server.log"; }   # the test server's log lines after mark $1 (see now)
served() { since "$1" | awk '{print $3}' | sed 's/[?&]t=[0-9]*//' | sort | tr '\n' ' '; }   # the paths the test server got since the mark
now() { wc -l < "$ROOT/server.log"; }   # a mark in the server log (a line count: clock times repeat after midnight)
wait_ready() { # wait_ready [seconds]: until the blocker says READY (or OFF / UNAVAILABLE); a release build cannot be asked: wait
  [ -n "$REL" ] && { sleep 8; return 0; }
  local n=0; until status | grep -qE "status=(READY|OFF|UNAVAILABLE)"; do n=$((n + 1)); [ "$n" -gt "${1:-30}" ] && return 1; sleep 1; done
}
save() { glog > "$EV/$1-logcat.txt"; }

# ----- UI helpers (uiautomator; Compose text is "text", contentDescription is "desc") -----
UI=""
WIN=""   # WIN=1: dump every window (the menu over selected text is a window of its own)
dump() { A "uiautomator dump ${WIN:+--windows} /sdcard/ui.xml" > /dev/null 2>&1; UI=$(A "cat /sdcard/ui.xml"); }
has() { echo "$UI" | python3 -I "$HERE/uix.py" has "$@"; }                  # has text|desc VALUE [--contains]
attr() { echo "$UI" | python3 -I "$HERE/uix.py" attr "$@"; }                # attr text|desc VALUE ATTRIBUTE
tapui() { # tapui text|desc VALUE [wait] [--contains]: dump, find, tap; fails the test when it is not there
  dump; local xy; xy=$(echo "$UI" | python3 -I "$HERE/uix.py" find "$1" "$2" ${4:-}) || { bad "no '$2' on screen"; return 1; }
  # shellcheck disable=SC2086
  tap $xy "${3:-2}"
}
longtap() { dump; local xy; xy=$(echo "$UI" | python3 -I "$HERE/uix.py" find "$1" "$2") || { bad "no '$2' on screen"; return 1; }
  # shellcheck disable=SC2086
  A "input swipe $xy $xy 900"; sleep 2; }
menu() { tapui desc "Menu" 2; }
scrollto() { # scrollto text|desc VALUE [--contains]: swipe up until it is on screen, clear of the bottom edge
  local i y
  for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
    dump
    if has "$@"; then
      y=$(echo "$UI" | python3 -I "$HERE/uix.py" find "$@" | cut -d' ' -f2)
      [ "${y:-0}" -le 1900 ] && return 0
      A "input swipe 540 1600 540 1200 300"; sleep 1
    else
      A "input swipe 540 1700 540 900 300"; sleep 1
    fi
  done
  bad "no '$2' found by scrolling"; return 1
}
tapto() { scrollto "$1" "$2" ${4:-} && tapui "$1" "$2" "${3:-2}" ${4:-}; }   # scroll to it, then tap
privacy() { menu && tapui text "Settings" 3 && scrollto text "Block ads and trackers"; }   # phones list every section on one page

t_idle() { # 1: fresh install, 2 minutes idle: 0 packets; blocker ready; time to ready
  fresh "$APK"; A "logcat -c"; cap_start gate-idle; launch 15; front idle || return
  wait_ready 30; local st; st=$(status); echo "  $st" | cut -c1-200
  dcheck "fresh install: blocker READY" 'echo "$st" | grep -q "status=READY"'
  sleep 105; front idle-end || return; shot gate-idle-after-2min || exit 4; cap_stop gate-idle > /dev/null; save gate-idle
  check "fresh install, 2 min idle: 0 packets (got $(packets gate-idle))" '[ "$(packets gate-idle)" = 0 ]'
  grep -oE "blocker ready after.*" "$EV/gate-idle-logcat.txt" | tail -1 | sed 's/^/  first start: /'
  echo "  gate: $(grep -c 'blocker request cancelled' "$EV/gate-idle-logcat.txt") blocker requests cancelled, $(grep -c 'blocker request passed' "$EV/gate-idle-logcat.txt") passed, $(grep -c 'engine request' "$EV/gate-idle-logcat.txt") engine requests seen"
}

t_idle10() { # acceptance: fresh install, 10 minutes idle: 0 packets; then one page load: only the page's own traffic
  fresh "$APK"; A "logcat -c"; cap_start gate-idle10; launch 15; front idle10 || return
  local i; for i in 1 2 3 4 5 6 7 8 9; do sleep 60; alive || { bad "idle10: Krunk is gone after $i min"; return; }; done; sleep 45
  front idle10-end || return; shot gate-idle10-after-10min || exit 4; cap_stop gate-idle10 > /dev/null; save gate-idle10
  check "fresh install, 10 min idle: 0 packets (got $(packets gate-idle10))" '[ "$(packets gate-idle10)" = 0 ]'
  grep -oE "blocker ready after.*" "$EV/gate-idle10-logcat.txt" | tail -1 | sed 's/^/  first start: /'
  local t; t=$(now); cap_start gate-idle10-page; view "$B/ads.html" 12; dump; shot gate-idle10-page; sleep 20; cap_stop gate-idle10-page > /dev/null
  check "then one page load: filtered ($(served "$t")), shield 5" '[ "$(served "$t")" = "/ads.html /ok.png " ] && has desc "5 requests blocked on this page"'
  check "the page load: packets only to the test server ($(packets gate-idle10-page) packets)" '[ "$(packets gate-idle10-page)" -gt 0 ] && ! grep -E "sni|quic" "$EV/gate-idle10-page-pcap-summary.txt" && [ "$(grep -cE "^ +[0-9.]+s +[0-9a-f.:]+" "$EV/gate-idle10-page-pcap-summary.txt")" = "$(grep -cE "^ +[0-9.]+s +${KRUNK_TEST_HOST:-10.250.136.3}:18080" "$EV/gate-idle10-page-pcap-summary.txt")" ]'
}

LIST_HOSTS="cdn.jsdelivr.net filters.adtidy.org malware-filter.gitlab.io pgl.yoyo.org publicsuffix.org ublockorigin.github.io ublockorigin.pages.dev"
t_relupdate() { # a user-started update through Settings, captured: only the filter lists' hosts are contacted
  launch 8; front relupdate || return; wait_ready 20
  cap_start gate-relupdate; privacy || return; tapto text "Update filter lists" 3; dump; check "update from Settings: running" 'has text "Updating…"'
  local n=0; until dump && has text "Last updated" --contains; do n=$((n + 1)); [ "$n" -gt 40 ] && break; sleep 3; done
  shot gate-relupdate-done; check "update from Settings: a last-updated time is shown" 'has text "Last updated" --contains'
  sleep 5; cap_stop gate-relupdate > /dev/null; back 2
  local hosts other=""; hosts=$(grep -oE "sni [^ ]+" "$EV/gate-relupdate-pcap-summary.txt" | cut -d' ' -f2 | sort -u | tr '\n' ' ')
  local h; for h in $hosts; do case " $LIST_HOSTS " in *" $h "*) ;; *) other="$other $h" ;; esac; done
  check "update: only the filter lists' hosts were contacted ($hosts; others:${other:- none})" '[ -n "$hosts" ] && [ -z "$other" ]'
  # The update's connections are closed by the engine up to two minutes later (a few FIN packets to the same addresses).
  cap_start gate-relupdate-closing; sleep 120; front relupdate-closing || return; cap_stop gate-relupdate-closing > /dev/null
  local ips_update ips_closing new=""; ips_update=$(grep -oE "^ +[0-9.-]+s +[0-9a-f.:]+" "$EV/gate-relupdate-pcap-summary.txt" | awk '{print $2}' | sort -u)
  ips_closing=$(grep -oE "^ +[0-9.-]+s +[0-9a-f.:]+" "$EV/gate-relupdate-closing-pcap-summary.txt" | awk '{print $2}' | sort -u)
  for h in $ips_closing; do echo "$ips_update" | grep -qxF "$h" || new="$new $h"; done
  check "after the update: no new connection ($(packets gate-relupdate-closing) packets closing the update's connections; new addresses:${new:- none})" '[ -z "$new" ] && ! grep -qE "sni|quic" "$EV/gate-relupdate-closing-pcap-summary.txt"'
  cap_start gate-relupdate-after; sleep 60; front relupdate-after || return; cap_stop gate-relupdate-after > /dev/null
  check "then quiet again (60 s, $(packets gate-relupdate-after) packets)" '[ "$(packets gate-relupdate-after)" = 0 ]'
}

t_afterupdate() { # the first start after a list update: the blocker has new lists to compile; the first page must still be filtered
  fresh "$APK"; launch 15; front afterupdate || return; wait_ready 20   # a fresh install: the update brings new lists for certain
  privacy || return; tapto text "Update filter lists" 3
  local n=0; until dump && has text "Last updated" --contains; do n=$((n + 1)); [ "$n" -gt 40 ] && break; sleep 3; done
  check "update from Settings: done" 'has text "Last updated" --contains'
  back 2; sleep 20; stop_clean; A "logcat -c"
  local t; t=$(now); view "$B/ads.html" 12; front afterupdate-start || return; dump; shot gate-afterupdate-first-page; save gate-afterupdate
  grep -oE "blocker ready after.*" "$EV/gate-afterupdate-logcat.txt" | tail -1 | sed 's/^/  start after the update: /'
  check "first start after an update: the first page is filtered ($(served "$t"))" '[ "$(served "$t")" = "/ads.html /ok.png " ]'
  check "first start after an update: the shield counts 5 and does not warn ($(echo "$UI" | grep -oE "content-desc=\"[^\"]*(block|Block)[^\"]*\"" | head -1))" 'has desc "5 requests blocked on this page"'
}

t_startlinks() { # cold starts by a link from another app: every first page filtered; how long the blocker takes each time
  local i t ms slow=0 unfiltered=0 times=""
  launch 8; front startlinks || return; wait_ready 20
  for i in 1 2 3 4 5 6 7 8; do
    stop_clean; A "logcat -c"; t=$(now); view "$B/ads.html" 14; alive || break; dump
    ms=$(glog | grep -oE "blocker ready after [0-9]+" | tail -1 | grep -oE "[0-9]+$"); times="$times ${ms:-?}"
    [ "${ms:-0}" -gt 5000 ] && { slow=$((slow + 1)); T "$E" adb logcat -d -v time > "$EV/gate-startlinks-slow-$i-logcat-full.txt"; glog > "$EV/gate-startlinks-slow-$i-logcat.txt"; }
    # The restored tab may be the same page: what counts is that nothing but the page and its control image was fetched.
    { [ -z "$(served "$t" | tr ' ' '\n' | grep -vxE "/ads.html|/ok.png|")" ] && has desc "5 requests blocked on this page"; } || { unfiltered=$((unfiltered + 1)); shot "gate-startlinks-unfiltered-$i"; glog > "$EV/gate-startlinks-unfiltered-$i-logcat.txt"; echo "  start $i: served $(served "$t"); $(echo "$UI" | grep -oE 'content-desc="[^"]*(block|Block)[^"]*"' | head -1)"; }
  done
  echo "  blocker ready after (ms):$times; slower than 5 s: $slow of 8"
  check "8 cold starts by a link: the first page is filtered every time ($unfiltered not)" '[ "$unfiltered" = 0 ]'
}

t_allow() { # 4: count and "off" title; allow list set, read back, effective after reload
  A "logcat -c"; local t; t=$(now); view "$B/ads.html" 10; front allow || return; shot gate-allow-1-blocked
  local st; st=$(status); echo "  shields: $(echo "$st" | grep -oE 'shields=\{[^}]*\}')"
  check "local page: count 5" 'echo "$st" | grep -q "count=5"'
  check "local page: server got the page and the control only ($(served "$t"))" '[ "$(served "$t")" = "/ads.html /ok.png " ]'
  dbg allow --es host 10.250.136.3; sleep 2
  check "allow: stored and taken by the blocker" 'glog | grep -q "debug: allow 10.250.136.3 -> 10.250.136.3, blocker took it: true"'
  t=$(now); sleep 1; view "$B/ads.html" 10; shot gate-allow-2-allowed; st=$(status)
  check "allowed site: all 6 requests reach the server" '[ "$(served "$t" | wc -w)" = 7 ]'
  check "allowed site: shield reads off" 'echo "$st" | grep -q "off=true"'
  dbg unallow --es host 10.250.136.3; sleep 2
  check "unallow: taken by the blocker" 'glog | grep -q "debug: unallow 10.250.136.3 -> true, blocker took it: true"'
  t=$(now); sleep 1; view "$B/ads.html" 10; shot gate-allow-3-blocked-again
  check "removed: blocked again ($(served "$t"))" '[ "$(served "$t")" = "/ads.html /ok.png " ]'
  save gate-allow
}

t_blocked() { # 6: blocked page and "open once"
  A "logcat -c"; view "https://googleads.g.doubleclick.net/pagead/x" 8; front blocked || return; shot gate-blocked-1-panel
  A "uiautomator dump /sdcard/ui.xml" > /dev/null 2>&1; local ui; ui=$(A "cat /sdcard/ui.xml")
  check "blocked address: Krunk's panel is shown" 'echo "$ui" | grep -q "Krunk blocked this page"'
  check "blocked address: the panel names it" 'echo "$ui" | grep -q "googleads.g.doubleclick.net/pagead/x"'
  local xy; xy=$(echo "$ui" | python3 -I -c "
import re,sys
t=sys.stdin.read()
for label in ('Open once','Go back'):
    m=re.search(r'text=\"%s\"[^>]*?bounds=\"\[(\d+),(\d+)\]\[(\d+),(\d+)\]\"' % label, t)
    print((int(m.group(1))+int(m.group(3)))//2, (int(m.group(2))+int(m.group(4)))//2) if m else print('0 0')
")
  local once back; once=$(echo "$xy" | sed -n 1p); back=$(echo "$xy" | sed -n 2p)
  # shellcheck disable=SC2086
  tap $back 3; front blocked-back || return; shot gate-blocked-2-after-back
  view "https://googleads.g.doubleclick.net/pagead/x" 8; cap_start gate-blocked-once
  # shellcheck disable=SC2086
  tap $once 8; shot gate-blocked-3-open-once; cap_stop gate-blocked-once > /dev/null
  # Over TLS the capture shows the host's name; over QUIC (HTTP/3, once the engine has learnt the host offers it) only that a connection was made.
  dump; check "open once: the address is then requested ($(grep -oE "sni [^ ]+|quic/udp-443" "$EV/gate-blocked-once-pcap-summary.txt" | sort -u | tr '\n' ' '))" 'grep -qE "sni googleads.g.doubleclick.net|quic/udp-443" "$EV/gate-blocked-once-pcap-summary.txt" && ! has text "Open once"'
  save gate-blocked
}

t_update() { # 5: update window: lists fetched, gate closed again afterwards
  A "logcat -c"; A "input keyevent KEYCODE_HOME"; launch 4; cap_start gate-update
  dbg lists; sleep 1; dbg update; local n=0
  until glog | grep -q "update finished"; do n=$((n + 1)); [ "$n" -gt 100 ] && break; sleep 3; done
  dbg lists; dbg status; sleep 2
  local passed; passed=$(glog | grep -c "blocker request passed")
  check "update: lists fetched ($passed requests passed the gate; $(glog | grep -oE 'update finished.*' | tail -1))" '[ "$passed" -gt 5 ] && glog | grep -qE "update finished: [1-9]"'
  check "update: time stored" 'glog | grep "debug: status" | tail -1 | grep -qE "updated=[0-9]{13}"'
  A "logcat -c"; dbg blocker --es channel dashboard --es msg "'{\"what\":\"updateNow\"}'"; sleep 15
  local late; late=$(glog | grep -c "blocker request passed")
  check "after the update the gate is closed again ($(glog | grep -c 'blocker request cancelled') cancelled, $late passed)" '[ "$late" = 0 ]'
  cap_stop gate-update > /dev/null; glog > "$EV/gate-update-after-logcat.txt"
  grep -E "sni|http " "$EV/gate-update-pcap-summary.txt" | awk '{print $3}' | sort -u | tr '\n' ' ' | sed 's/^/  hosts contacted during the update: /'; echo
}

t_starts() { # 2: 5 cold starts with uBO's auto-update forced on: 0 packets, only cancelled requests
  launch 8; front starts || return; wait_ready 20   # its own start: the group before may have ended with a force-stop
  A "logcat -c"; dbg blocker --es channel dashboard --es msg "'{\"what\":\"userSettings\",\"name\":\"autoUpdate\",\"value\":true}'"; sleep 2
  menu && tapui text "New tab" 3   # a new tab, so no page is restored at start
  stop_clean
  local i sum=0 cancelled=0 passed=0 ran=0
  for i in 1 2 3 4 5; do
    A "logcat -c"; cap_start "gate-start$i"; launch 12; front "start$i" || break; ran=$((ran + 1))
    [ "$i" -le 2 ] && { dbg blocker --es channel dashboard --es msg "'{\"what\":\"userSettings\",\"name\":\"autoUpdate\",\"value\":true}'"; }
    sleep 40; cap_stop "gate-start$i" > /dev/null; save "gate-start$i"
    sum=$((sum + $(packets "gate-start$i"))); cancelled=$((cancelled + $(grep -c 'blocker request cancelled' "$EV/gate-start$i-logcat.txt"))); passed=$((passed + $(grep -c 'blocker request passed' "$EV/gate-start$i-logcat.txt")))
    grep -oE "blocker ready after.*" "$EV/gate-start$i-logcat.txt" | tail -1 | sed "s/^/  start $i: /"
    stop_clean
  done
  check "5 cold starts ($ran ran): 0 packets in total (got $sum; $cancelled blocker requests cancelled, $passed passed)" '[ "$ran" = 5 ] && [ "$sum" = 0 ] && [ "$passed" = 0 ]'
}

t_noguard() { # 3a: the guard cannot be installed (fresh install): uBO is never installed, 0 packets
  A "setprop debug.krunk.guard_uri resource://android/assets/extensions/none/"
  fresh "$APK"; A "logcat -c"; cap_start gate-noguard; launch 25; front noguard || return; local st; st=$(status); echo "  $st" | cut -c1-170
  check "guard missing: status UNAVAILABLE, blocker never installed" 'echo "$st" | grep -q "status=UNAVAILABLE" && echo "$st" | grep -q "blocker=null"'
  local t; t=$(now); view "$B/ads.html" 10; shot gate-noguard-page; sleep 30; cap_stop gate-noguard > /dev/null; save gate-noguard
  check "guard missing: pages load, unfiltered ($(served "$t" | wc -w) requests served)" '[ "$(served "$t" | wc -w)" = 7 ]'
  check "guard missing: no packets except to the test server" '! grep -E "sni|quic" "$EV/gate-noguard-pcap-summary.txt"'
  A "setprop debug.krunk.guard_uri ''"; stop_clean
}

t_guardlost() { # 3b: the guard is disabled at run time (existing install): uBO is disabled at once
  fresh "$APK"; A "logcat -c"; launch 15; front guardlost || return; wait_ready 30
  cap_start gate-guardlost; dbg guard-disable; sleep 4; local st; st=$(status); echo "  $st" | cut -c1-170
  check "guard disabled at run time: status UNAVAILABLE, blocker disabled" 'echo "$st" | grep -q "status=UNAVAILABLE" && echo "$st" | grep -q "blocker=false"'
  local t; t=$(now); view "$B/ads.html" 10; shot gate-guardlost-page
  check "guard disabled: pages load, unfiltered ($(served "$t" | wc -w) requests served)" '[ "$(served "$t" | wc -w)" = 7 ]'
  sleep 30; cap_stop gate-guardlost > /dev/null; save gate-guardlost
  check "guard disabled at run time: no packets except to the test server" '! grep -E "sni|quic" "$EV/gate-guardlost-pcap-summary.txt"'
  menu && tapui text "New tab" 3; stop_clean; A "logcat -c"; launch 15; front guardlost-next || return; wait_ready 30; st=$(status); echo "  $st" | cut -c1-170; glog > "$EV/gate-guardlost-next-start-logcat.txt"
  check "next start: the guard is enabled again and the blocker READY" 'echo "$st" | grep -q "status=READY" && echo "$st" | grep -q "blocker=true"'
}

t_off() { # blocking switched off: uBO disabled, the guard (rewriter, engine config) still works
  launch 8; front off || return; wait_ready 20
  A "logcat -c"; dbg setting --es name adblock --es value off; sleep 3; local st; st=$(status); echo "  $st" | cut -c1-160
  check "blocking off: status OFF, blocker disabled, guard up" 'echo "$st" | grep -q "status=OFF" && echo "$st" | grep -q "blocker=false" && echo "$st" | grep -q "guardUp=true"'
  local t; t=$(now); view "$B/ads.html" 10; shot gate-off-1-ads
  check "blocking off: nothing is blocked ($(served "$t" | wc -w) requests served)" '[ "$(served "$t" | wc -w)" = 7 ]'
  t=$(now); sleep 1; view "$B/links.html" 8; tap 420 597 6; shot gate-off-2-link-cleaned
  check "blocking off: the tracking parameters are still removed ($(served "$t"))" 'served "$t" | grep -q "/landing.html?id=5 "'
  stop_clean; cap_start gate-off-idle; A "logcat -c"; launch 15; sleep 60; front off-idle || return; cap_stop gate-off-idle > /dev/null
  check "blocking off: restart, restored page only: no packets to anything but the test server" '! grep -E "sni|quic" "$EV/gate-off-idle-pcap-summary.txt"'
  dbg setting --es name adblock --es value on; sleep 1; wait_ready 30; st=$(status)
  check "blocking on again: READY" 'echo "$st" | grep -q "status=READY"'
  save gate-off
}

t_hold() { # the page restored at start waits for the blocker and is filtered
  view "$B/ads.html" 8; stop_clean; A "logcat -c"; local t; t=$(now); sleep 1; launch 15; front hold || return; wait_ready 20
  shot gate-hold-restored; local st; st=$(status)
  check "restored page at start: filtered ($(served "$t"))" '[ "$(served "$t")" = "/ads.html /ok.png " ]'
  dcheck "restored page at start: count 5" 'echo "$st" | grep -q "count=5"'
  dcheck "start was not unprotected" 'echo "$st" | grep -q "unprotected=false"'
  dump; check "restored page at start: the shield counts 5 and does not warn" 'has desc "5 requests blocked on this page"'
  save gate-hold
}

t_toolbar() { # the toolbar has no Back/Forward; the address pill takes at least 60 % of the width
  fresh "$APK"; A "logcat -c"; launch 15; front toolbar || return; wait_ready 30
  view "$B/links.html" 8; shot ui-toolbar; dump
  check "toolbar: no Back or Forward button" '! has desc Back && ! has desc Forward'
  check "toolbar: reload, tab counter and menu are there" 'has desc Reload && has desc "Tabs:" --contains && has desc Menu'
  local pill reload; pill=$(echo "$UI" | python3 -I "$HERE/uix.py" size desc "Edit address" | cut -d' ' -f1); reload=$(echo "$UI" | python3 -I "$HERE/uix.py" size desc Reload --clickable)
  check "toolbar: the address pill is $((pill * 100 / 1080)) % of the width ($pill of 1080 px)" '[ "$((pill * 100 / 1080))" -ge 60 ]'
  check "reload target is 48 dp, 126 px at 420 dpi (got $reload)" '[ "${reload% *}" -ge 126 ] && [ "${reload#* }" -ge 126 ]'
  tapui desc "Edit address" 2; dump; check "a tap on the address opens the editor" 'has desc "Clear" --contains || has text "10.250.136.3" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1; dump; has desc Menu || { A "input keyevent KEYCODE_BACK"; sleep 2; }   # the keyboard first, then the editor
  # Requirement 11 speaks of a 360 dp phone; this screen is 411 dp (1080 px at 420 dpi = 2.625 px per dp). Everything
  # but the address field has a fixed width, so its share at 360 dp follows from what is measured here.
  share360() { python3 -I -c 'import sys; pill = int(sys.argv[1]) / 2.625; fixed = 1080 / 2.625 - pill; print("%.1f dp of 411.4 dp here (%.1f %%); fixed parts %.1f dp; at 360 dp: %.1f dp = %.1f %%" % (pill, pill * 100 / (1080 / 2.625), fixed, 360 - fixed, (360 - fixed) * 100 / 360))' "$1"; }
  share_ok() { python3 -I -c 'import sys; fixed = (1080 - int(sys.argv[1])) / 2.625; sys.exit(0 if (360 - fixed) / 360 >= 0.60 else 1)' "$1"; }
  check "address field without a theme: $(share360 "$pill")" 'share_ok "$pill"'
  local themed=""; pick_style Sunset && [ "$STYLE_CHECKED" = true ] && { dump; shot ui-toolbar-sunset; themed=$(echo "$UI" | python3 -I "$HERE/uix.py" size desc "Edit address" | cut -d' ' -f1); }
  check "address field with the Sunset style: $([ -n "$themed" ] && share360 "$themed")" '[ -n "$themed" ] && share_ok "$themed" && [ "$themed" = "$pill" ]'
  check "themed toolbar: reload target still 48 dp ($(echo "$UI" | python3 -I "$HERE/uix.py" size desc Reload --clickable))" '[ "$(echo "$UI" | python3 -I "$HERE/uix.py" size desc Reload --clickable)" = "126 126" ]'
  pick_style Classic
}

t_menu() { # first row of the menu: Back, Forward, Home; enabled from the history; long press lists the pages
  view "$B/links.html" 8; front menu || return
  menu; dump; shot ui-menu-1-first-page
  check "menu: Back, Forward and Home are in the first row" 'has desc Back && has desc Forward && has desc Home'
  check "menu: on a tab's first page Back and Forward are disabled" '[ "$(attr desc Back enabled)" = false ] && [ "$(attr desc Forward enabled)" = false ]'
  A "input keyevent KEYCODE_BACK"; sleep 1
  tap 220 1345 6   # link 4, a clean link to /landing.html?id=9
  menu; dump; check "menu: after a link Back is enabled, Forward is not" '[ "$(attr desc Back enabled)" = true ] && [ "$(attr desc Forward enabled)" = false ]'
  tapui desc Back 4; dump; check "menu Back: the links page is back" 'has text "Tracking parameters"'
  menu; dump; check "menu: now Forward is enabled" '[ "$(attr desc Forward enabled)" = true ]'
  tapui desc Forward 4; dump
  if ! has text "Landed on"; then   # seen once in three runs on 2026-11-12: say whether it is late (timing) or absent (app)
    shot ui-menu-forward-late; echo "$UI" | python3 -I "$HERE/uix.py" list > "$EV/ui-menu-forward-late-nodes.txt"; glog | tail -20 > "$EV/ui-menu-forward-late-logcat.txt"
    sleep 4; dump; echo "  NOTE menu Forward: the page was not in the first dump; 4 s later: $(has text "Landed on" && echo shown || echo "still missing")"
  fi
  check "menu Forward: the landing page" 'has text "Landed on"'
  A "input keyevent KEYCODE_BACK"; sleep 3; dump; check "system back still goes back in the page's history" 'has text "Tracking parameters"'
  tap 220 1345 6; menu; longtap desc Back; dump; shot ui-menu-2-history-list
  check "long press on Back: the list of earlier pages" 'has text "Go to" && has text "Links"'
  tapui text "Links" 4; dump; check "history list: jumps to the picked page" 'has text "Tracking parameters"'
  menu; tapui desc Home 10; dump; shot ui-menu-3-home; check "menu Home: the homepage (DuckDuckGo by default) is loaded" 'has text "duckduckgo.com"'
}

t_shield() { # count for this page; sheet with count, site and the switch; allow survives a restart; follows the tab
  A "logcat -c"; local t; t=$(now); view "$B/ads.html" 10; front shield || return; dump; shot ui-shield-1-count
  check "shield: 5 requests blocked on this page" 'has desc "5 requests blocked on this page"'
  tapui desc "5 requests blocked on this page" 2; dump; shot ui-shield-2-sheet
  check "sheet: count, site and the switch (on)" 'has text "5 requests blocked on this page" && has text "10.250.136.3" && [ "$(attr text "Block on this site" checked)" != "" ]'
  t=$(now); sleep 1; tapui text "Block on this site" 10; dump; shot ui-shield-3-allowed
  check "allow: the page reloaded with everything ($(served "$t"))" '[ "$(served "$t" | wc -w)" = 7 ]'
  check "allow: the shield shows the allowed state" 'has desc "Blocking is off for this site"'
  stop_clean; t=$(now); sleep 1; launch 15; front shield-restart || return; wait_ready 20; dump
  check "allow: still set after a restart ($(served "$t" | wc -w) requests, shield allowed)" '[ "$(served "$t" | wc -w)" = 7 ] && has desc "Blocking is off for this site"'
  menu; tapui text "New tab" 3; dump; check "another tab (new-tab page): no shield, no count" '! has desc "blocked on this page" --contains && ! has desc "Blocking is off for this site"'
  t=$(now); view "$B/ads.html" 10; tapui desc "Blocking is off for this site" 2; tapui text "Block on this site" 10; dump; shot ui-shield-4-blocking-again
  check "allow removed in the sheet: blocked again, count 5" 'has desc "5 requests blocked on this page"'
  view "$B/links.html" 8; dump; check "navigation: the count starts again (0 on the next page)" 'has desc "0 requests blocked on this page"'
  save ui-shield
}

t_private() { # private tab: blocked like a normal tab; the sheet has a hint and no switch
  launch 8; front private || return; wait_ready 20   # its own start: the group before may have ended with a stop
  menu; tapui text "New private tab" 4; front private || return
  local t; t=$(now); tapui desc "Edit address" 2; A "input text '10.250.136.3:18080/ads.html'"; A "input keyevent 66"; sleep 10; dump
  check "private tab: blocked ($(served "$t"))" '[ "$(served "$t")" = "/ads.html /ok.png " ]'
  check "private tab: count 5" 'has desc "5 requests blocked on this page"'
  tapui desc "5 requests blocked on this page" 2; dump
  check "private tab: the sheet has the hint and no switch" 'has text "Sites can be allowed from a normal tab" --contains && ! has text "Block on this site"'
  A "input keyevent KEYCODE_BACK"; sleep 1
}

t_settings() { # Settings > Privacy: the switch, the allowed sites, update, automatic updates
  fresh "$APK"; A "logcat -c"; launch 15; front settings || return; wait_ready 30   # a fresh install: nothing updated yet
  view "$B/ads.html" 10; tapui desc "5 requests blocked on this page" 2 && tapui text "Block on this site" 8   # allow the site in the sheet
  privacy || return; shot ui-settings-1-privacy
  check "privacy: block ads and trackers (a switch to a screen reader: checkable, F-52)" 'has text "Block ads and trackers" && echo "$UI" | grep -q "checkable=\"true\""'
  scrollto text "Sites with blocking off"; check "privacy: 1 allowed site" 'has text "1 site"'
  tapui text "Sites with blocking off" 2; dump; shot ui-settings-2-allowed; check "allowed sites: the entry with Remove" 'has text "10.250.136.3" && has text "Remove"'
  tapui text "Remove" 2; dump; check "allowed sites: removed" '! has text "10.250.136.3"'; tapui text "Done" 2
  scrollto text "Update filter lists"; check "update row: lists are from the package until the first update" 'has text "Lists from the app package" --contains'
  scrollto text "Update automatically"; shot ui-settings-3-update-rows; check "automatic updates: off by default" 'has text "Update automatically"'
  scrollto text "no updates of certificate revocation data" --contains; check "privacy: the default trade-off is stated" 'has text "no updates of certificate revocation data" --contains'
  tapto text "Update filter lists" 3; dump; check "update: running" 'has text "Updating…"'
  local n=0; until dump && has text "Last updated" --contains; do n=$((n + 1)); [ "$n" -gt 40 ] && break; sleep 3; done
  shot ui-settings-4-updated; check "update: a last-updated time is shown" 'has text "Last updated" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 1
  privacy && tapui text "Block ads and trackers" 3; A "input keyevent KEYCODE_BACK"; sleep 1
  local t; t=$(now); view "$B/ads.html" 10; dump; shot ui-settings-5-blocking-off
  check "blocking switched off in Settings: nothing blocked, shield says off" '[ "$(served "$t" | wc -w)" = 7 ] && has desc "Blocking is switched off"'
  privacy && tapui text "Block ads and trackers" 3; A "input keyevent KEYCODE_BACK"; sleep 2; wait_ready 20
  t=$(now); view "$B/ads.html" 10; dump; shot ui-settings-6-blocking-on; echo "  $(status | cut -c1-120)"
  check "blocking on again: count 5" 'has desc "5 requests blocked on this page"'
  save ui-settings
}

t_rewrite() { # tracking parameters: click, redirect, POST untouched; setting off; private tab
  A "logcat -c"; local t; t=$(now); view "$B/links.html" 8; front rewrite || return
  tapui text "1 link with utm_source, fbclid" 6; dump; shot links-1-click
  check "link click: the server got the cleaned address ($(served "$t"))" 'served "$t" | grep -q "/landing.html?id=5 " && ! served "$t" | grep -q utm_'
  check "link click: the page shows the cleaned address" 'has text "landing.html?id=5" --contains'
  A "input keyevent KEYCODE_BACK"; sleep 3; dump; check "back returns to the links page (one history entry, no loop)" 'has text "Tracking parameters"'
  t=$(now); sleep 1; tapui text "2 link to a 302" 6 --contains; dump; shot links-2-redirect
  check "redirect: /redir, then the cleaned target ($(served "$t"))" 'served "$t" | grep -q "/landing.html?keep=1 " && served "$t" | grep -q "/redir " && ! served "$t" | grep -q utm_'
  A "input keyevent KEYCODE_BACK"; sleep 3
  t=$(now); sleep 1; tapui text "3 POST form" 6 --contains; shot links-3-post
  check "POST: untouched ($(since "$t" | grep POST | awk '{print $2, $3}'))" 'since "$t" | grep -q "POST /post?utm_source=form&x=1"'
  privacy && tapto text "Strip tracking parameters" 3; A "input keyevent KEYCODE_BACK"; sleep 1
  t=$(now); view "$B/links.html" 8; tapui text "1 link with utm_source, fbclid" 6
  check "setting off: the address is left alone ($(served "$t"))" 'served "$t" | grep -q "utm_source=news"'
  privacy && tapto text "Strip tracking parameters" 3; A "input keyevent KEYCODE_BACK"; sleep 1
  t=$(now); view "$B/links.html" 8; tapui text "1 link with utm_source, fbclid" 6
  check "setting on again: cleaned ($(served "$t"))" 'served "$t" | grep -q "/landing.html?id=5 "'
  menu; tapui text "New private tab" 4; tapui desc "Edit address" 2; A "input text '10.250.136.3:18080/links.html'"; A "input keyevent 66"; sleep 12
  t=$(now); sleep 1; tapui text "1 link with utm_source, fbclid" 6
  check "private tab: cleaned too ($(served "$t"))" 'served "$t" | grep -q "/landing.html?id=5 " && ! served "$t" | grep -q utm_'
  save links-rewrite
}

top_app() { A "dumpsys activity activities" 2> /dev/null | grep -m1 -E 'topResumedActivity|mResumedActivity' | grep -oE '[a-z0-9.]+/[A-Za-z0-9.$]+' | head -1; }

t_handoff() { # mailto: and tel: go to Android; by script only after a question; intent: stays
  view "$B/handoff.html" 8; front handoff || return
  tapui text "1 mailto link (tap)" 4; local app; app=$(top_app); shot links-4-mailto
  check "mailto tap: another app (or Android's chooser) is in front: $app" '! echo "$app" | grep -q "^$PKG/"' || { dump; has text "No app on this device" --contains && ok "mailto tap: no mail app on this device, Krunk says so"; }
  A "input keyevent KEYCODE_BACK"; A "input keyevent KEYCODE_BACK"; sleep 2; view "$B/handoff.html" 6
  tapui text "2 tel link (tap)" 4; app=$(top_app); shot links-5-tel
  check "tel tap: the dialer is in front: $app" 'echo "$app" | grep -qi "dialer\|phone\|contacts"'
  A "input keyevent KEYCODE_BACK"; A "input keyevent KEYCODE_BACK"; sleep 2; view "$B/handoff.html" 6
  tapui text "3 tel by script" 11 --contains; dump; shot links-6-tel-asked
  check "tel by script: Krunk asks first, naming the number" 'has text "Open the phone app?" && has text "+15551234" --contains'
  tapui text "Cancel" 2; front handoff-cancel && ok "cancel: still in Krunk"
  tapui text "4 intent: link" 4 --contains; app=$(top_app)
  check "intent: link: not handed over ($app)" 'echo "$app" | grep -q "^$PKG/"'
  save links-handoff
}

sel_entry() { # sel_entry LABEL: Krunk's entry is in the menu over the selection; says whether it needed "More options"
  dump; SEL_WHERE="shown directly"
  has desc "$1" --contains && return 0
  has desc "More options" || return 1
  tapui desc "More options" 2; dump; SEL_WHERE="behind More options"
  has desc "$1" --contains
}
t_selection() { # the menu over selected text: search, and open for an address; not for a password
  local WIN=1 SEL_WHERE=""
  view "$B/select.html" 8; front selection || return
  longtap text "krunk"; shot links-7-selection-word
  check "selected word: Search DuckDuckGo for it" 'sel_entry "Search DuckDuckGo for"'; echo "  $SEL_WHERE"
  check "selected word: no Open entry (not an address)" '! has desc "Open krunk" --contains'
  tap 540 1900 2   # an empty part of the page: the selection goes
  longtap text "••••••••••"; shot links-7b-selection-password; dump
  check "password field: neither entry" '! has desc "Search DuckDuckGo" --contains && ! has desc "Open " --contains'
  # A long press selects one word ("com"): "Select all" on a page that holds only the address
  view "$B/select-address.html" 6
  longtap text "example.com/docs"; dump
  has desc "Select all" || tapui desc "More options" 2
  tapui desc "Select all" 3; shot links-8-selection-address
  check "selected address: Open example.com/docs" 'sel_entry "Open example.com"'; echo "  $SEL_WHERE"
  local tabs; tabs=$(echo "$UI" | grep -o "Tabs: [0-9]*" | head -1)
  tapui desc "Open example.com" 8 --contains; dump; shot links-9-opened
  check "open: a new tab on that address (before: $tabs)" 'has text "example.com" --contains && ! echo "$UI" | grep -q "$tabs open"'
  save links-selection
}

# ----- M7: page prompts (test pages www/p-*.html; each shows its state in #s and reports it to the server) -----
SITE="10.250.136.3:18080"
gone() { ! has text "asks for" --contains && ! has text "Cancel"; }   # no prompt dialog is left on screen
st() { echo "$UI" | grep -qE "$1"; }   # the page's status (a text node with one k=v per line) matches
open_picker() { tapui text "open picker" "${1:-3}"; }
back() { A "input keyevent KEYCODE_BACK"; sleep "${1:-2}"; }
ready() { # ready TEXT [seconds]: wait until the page shows TEXT (a cold start can take longer than the fixed wait)
  local n=0; dump; until has text "$1"; do n=$((n + 1)); [ "$n" -gt "${2:-10}" ] && return 1; sleep 2; dump; done
}
page_alive() { # the page still answers: its "report" button raises the counter (no prompt hangs it)
  local before; dump; before=$(echo "$UI" | grep -oE 'reported=[0-9]+' | cut -d= -f2); tapui text "report" 2; dump
  st "reported=$(( ${before:-0} + 1 ))"
}

t_date() { # date field: titled with the site; min..max; OK, Back, tap outside, Cancel, Clear
  A "logcat -c"; view "$B/p-date.html" 6; front date || return; ready "open picker" || bad "date: the test page did not load"; local t; t=$(now)
  open_picker; dump; shot prompts-date-1-picker
  check "date: the dialog is titled with the asking site" 'has text "$SITE asks for a date"'
  tapui desc "Change to next month" 2; dump
  check "date: a day after the field's max cannot be picked" '[ "$(attr text "Tuesday, December 1, 2026" enabled)" = false ]'
  tapui desc "Change to previous month" 2
  tapui text "Friday, November 13, 2026" 1; tapui text "OK" 3; dump; shot prompts-date-2-answered
  check "date: OK gives the page the picked day" 'st "value=\[2026-11-13\]" && st "changes=1" && gone'
  check "date: the page reported it to the server" 'since "$t" | grep -q "page=date&value=%5B2026-11-13%5D&changes=1"'
  open_picker; back; dump; check "date: Back leaves the field as it was" 'st "value=\[2026-11-13\]" && st "changes=1" && gone'
  open_picker; tap 540 300 2; dump; check "date: a tap outside leaves the field as it was" 'st "value=\[2026-11-13\]" && st "changes=1" && gone'
  open_picker; tapui text "Cancel" 2; dump; check "date: Cancel leaves the field as it was" 'st "value=\[2026-11-13\]" && st "changes=1" && gone'
  open_picker; tapui text "Clear" 3; dump; shot prompts-date-3-cleared; check "date: Clear empties the field" 'st "value=\[\]" && st "changes=2" && gone'
  check "date: the page still answers after the dismissals" 'page_alive'
  save prompts-date
}

t_time() { # time field
  A "logcat -c"; view "$B/p-time.html" 6; front time || return; ready "open picker" || bad "time: the test page did not load"
  open_picker; dump; shot prompts-time-1-picker
  check "time: the dialog is titled with the asking site" 'has text "$SITE asks for a time"'
  tapui text "OK" 3; dump; check "time: OK gives the page a time (HH:MM)" 'st "value=\[[0-9]{2}:[0-9]{2}\]" && st "changes=1" && gone'
  open_picker; back; dump; shot prompts-time-2-dismissed; check "time: Back leaves the field as it was" 'st "changes=1" && gone'
  view "$B/p-time2.html" 6
  open_picker; tapui text "Cancel" 2; dump; check "time: Cancel leaves the field as it was" 'st "value=\[07:30\]" && st "changes=0" && gone'
  open_picker; tapui text "Clear" 3; dump; check "time: Clear empties the field" 'st "value=\[\]" && st "changes=1" && gone'
  check "time: the page still answers" 'page_alive'
  save prompts-time
}

t_datetime() { # datetime-local: the day, then the time of day
  A "logcat -c"; view "$B/p-datetime.html" 6; front datetime || return; ready "open picker" || bad "datetime: the test page did not load"
  open_picker; dump; check "datetime: first the day, titled with the site" 'has text "$SITE asks for a date" && has text "Next"'
  tapui text "Friday, November 13, 2026" 1; tapui text "Next" 2; dump; shot prompts-datetime-1-time-step
  check "datetime: then the time, titled with the site" 'has text "$SITE asks for a time"'
  tapui text "OK" 3; dump; shot prompts-datetime-2-answered
  check "datetime: OK gives the page day and time" 'st "value=\[2026-11-13T07:30\]" && st "changes=1" && gone'
  open_picker; tapui text "Next" 2; back; dump; check "datetime: Back at the time step leaves the field as it was" 'st "value=\[2026-11-13T07:30\]" && st "changes=1" && gone'
  open_picker; back; dump; check "datetime: Back at the day step leaves the field as it was" 'st "changes=1" && gone'
  check "datetime: the page still answers" 'page_alive'
  save prompts-datetime
}

t_month() { # month field: Krunk's own small dialog
  A "logcat -c"; view "$B/p-month.html" 6; front month || return; ready "open picker" || bad "month: the test page did not load"
  open_picker; dump; shot prompts-month-1-picker
  check "month: the dialog is titled with the asking site, year 2026 shown" 'has text "$SITE asks for a month" && has text "2026"'
  tapui text "Mar" 1; tapui text "OK" 3; dump; check "month: OK gives the page the month" 'st "value=\[2026-03\]" && st "changes=1" && gone'
  open_picker; tapui desc "Next year" 1; tapui text "Dec" 1; back; dump; shot prompts-month-2-dismissed
  check "month: Back leaves the field as it was" 'st "value=\[2026-03\]" && st "changes=1" && gone'
  open_picker; tapui text "Clear" 3; dump; check "month: Clear empties the field" 'st "value=\[\]" && st "changes=2" && gone'
  check "month: the page still answers" 'page_alive'
  save prompts-month
}

t_week() { # week field: a day is picked, its ISO week is the answer
  A "logcat -c"; view "$B/p-week.html" 6; front week || return; ready "open picker" || bad "week: the test page did not load"
  open_picker; dump; check "week: the dialog is titled with the asking site" 'has text "$SITE asks for a week"'
  tapui text "Friday, November 13, 2026" 2; dump; shot prompts-week-1-picker
  check "week: the picked day's week is shown" 'has text "Week 46, 2026"'
  tapui text "OK" 3; dump; check "week: OK gives the page the ISO week" 'st "value=\[2026-W46\]" && st "changes=1" && gone'
  open_picker; back; dump; shot prompts-week-2-dismissed; check "week: Back leaves the field as it was" 'st "value=\[2026-W46\]" && st "changes=1" && gone'
  check "week: the page still answers" 'page_alive'
  save prompts-week
}

t_colour() { # colour field: swatches and a hex field; the page's own suggestions when it has some
  A "logcat -c"; view "$B/p-colour.html" 6; front colour || return; ready "open picker" || bad "colour: the test page did not load"
  open_picker; dump; shot prompts-colour-1-picker
  check "colour: the dialog is titled with the asking site" 'has text "$SITE asks for a colour"'
  check "colour: the field's own colour is offered and marked" '[ "$(attr desc "#336699" checked --clickable)" = true ]'
  tapui desc "#d32f2f" 1; tapui text "OK" 3; dump; check "colour: OK gives the page the colour" 'st "value=\[#d32f2f\]" && st "changes=1" && gone'
  open_picker; tapui desc "#388e3c" 1; back; dump; shot prompts-colour-2-dismissed
  check "colour: Back leaves the field as it was" 'st "value=\[#d32f2f\]" && st "changes=1" && gone'
  check "colour: the page still answers" 'page_alive'
  view "$B/p-colour2.html" 6
  open_picker; dump; shot prompts-colour-3-suggestions
  check "colour: a page's suggested colours replace the palette" 'has desc "#ff0000" && has desc "#0000ff" && ! has desc "#d32f2f"'
  tapui text "Cancel" 2; dump; check "colour: Cancel leaves the field as it was" 'st "value=\[#336699\]" && st "changes=0" && gone'
  save prompts-colour
}

t_leave() { # beforeunload: Stay, Back (= stay), Leave
  A "logcat -c"; view "$B/p-leave.html" 6; front leave || return; ready "arm" || bad "leave: the test page did not load"
  tapui text "arm" 2; tapui text "go away" 3; dump; shot prompts-leave-1-dialog
  check "leave: the dialog names the site and offers Leave and Stay" 'has text "Leave $SITE?" && has text "Leave" && has text "Stay"'
  tapui text "Stay" 3; dump; check "leave: Stay keeps the page" 'st "armed=1" && ! has text "Stay"'
  tapui text "go away" 3; back 3; dump; shot prompts-leave-2-dismissed; check "leave: Back keeps the page" 'st "armed=1" && ! has text "Stay"'
  tapui text "go away" 3; tap 540 300 3; dump; check "leave: a tap outside keeps the page" 'st "armed=1" && ! has text "Stay"'
  tapui text "go away" 3; tapui text "Leave" 5; dump; shot prompts-leave-3-left; check "leave: Leave goes to the next page" 'has text "Landed on"'
  save prompts-leave
}

t_repost() { # reloading the result of a form: Cancel, Back (= do not send), Send again
  A "logcat -c"; view "$B/p-repost.html" 6; front repost || return; ready "post the form" || bad "repost: the test page did not load"; local t; t=$(now)
  posts() { since "$t" | grep -c "POST /post?form=1"; }
  tapui text "post the form" 5; dump; check "repost: the form was sent once" '[ "$(posts)" = 1 ] && has text "POST"'
  tapui desc "Reload" 3; dump; shot prompts-repost-1-dialog
  check "repost: Reload asks first, naming the site" 'has text "Send the form again?" && has text "$SITE" --contains && has text "Send again"'
  tapui text "Cancel" 3; dump; check "repost: Cancel sends nothing" '[ "$(posts)" = 1 ] && ! has text "Send again"'
  tapui desc "Reload" 3; back 3; dump; shot prompts-repost-2-dismissed; check "repost: Back sends nothing" '[ "$(posts)" = 1 ] && ! has text "Send again"'
  tapui desc "Reload" 3; tapui text "Send again" 5; dump; check "repost: Send again sends the form a second time" '[ "$(posts)" = 2 ] && has text "POST"'
  save prompts-repost
}

LOCAL="http://localhost:18081"   # the test server as a secure context (navigator.share needs one)
relay() { # a relay on the device, localhost:18081 -> the test server (adb reverse would end at the adb server's host)
  A "ps -A -o ARGS" 2> /dev/null | grep -q "[n]c -s 127.0.0.1 -p 18081" && return 0
  (T "$E" adb shell "nohup toybox nc -s 127.0.0.1 -p 18081 -L toybox nc ${KRUNK_TEST_HOST:-10.250.136.3} 18080 > /dev/null 2>&1 &" > /dev/null 2>&1 &)
  sleep 2
}
t_share() { # navigator.share: Android's share sheet; closing it rejects the page's promise with AbortError
  relay; A "logcat -c"; view "$LOCAL/p-share.html" 6; front share || return; ready "share" || bad "share: the test page did not load"
  tapui text "share" 4; local app; app=$(top_app); shot prompts-share-1-sheet; dump
  check "share: Android's share sheet is in front ($app)" '! echo "$app" | grep -q "^$PKG/"'
  check "share: the sheet carries the page's text and address" 'has text "Shared text" --contains || has text "landing.html?id=7" --contains'
  back 4; front share-back || return; dump; shot prompts-share-2-dismissed
  check "share: closing the sheet answers the page (AbortError)" 'st "asked=1" && st "result=AbortError"'
  tapui text "share" 4; dump
  local target=""; for target in Messages Gmail Drive Bluetooth "Quick Share" Nearby ""; do [ -n "$target" ] && has text "$target" && break; done
  if [ -n "$target" ]; then
    tapui text "$target" 5; app=$(top_app); shot prompts-share-3-target; A "am start -W -n $PKG/.MainActivity" > /dev/null; sleep 4; dump
    check "share: picking an app ($target, $app) resolves the page's promise" 'st "asked=2" && st "result=shared"'
    A "am force-stop ${app%%/*}"
  else
    echo "SKIP  share: no app to pick in the sheet ($(echo "$UI" | python3 -I "$HERE/uix.py" list | wc -l) nodes)"; back 3
  fi
  save prompts-share
}

pick_file() { # pick_file NAME: in Android's picker, show Downloads if the file is not listed, then tap it
  dump
  if ! has text "$1"; then
    tapui desc "Show roots" 2 && tapui text "Downloads" 3; dump
  fi
  tapui text "$1" "${2:-5}"
}
t_file() { # file field: Android's picker; the page reads the file; closing the picker gives no file
  A "logcat -c"; view "$B/p-file.html" 6; front file || return; ready "open picker" || bad "file: the test page did not load"; local t; t=$(now)
  open_picker 4; local app; app=$(top_app); shot prompts-file-1-picker
  check "file: Android's document picker is in front ($app)" 'echo "$app" | grep -qi "documentsui"'
  pick_file "krunk-upload.txt"; front file-picked || return; sleep 2; dump; shot prompts-file-2-picked
  check "file: the page got the file (name, size, type)" 'st "files=1" && st "names=krunk-upload.txt:28:text/plain"'
  check "file: the page can read it" 'st "head=krunk upload test file"'
  check "file: the upload reached the server" 'since "$t" | grep -q "POST /post?upload=krunk-upload.txt&size=28"'
  open_picker 4; back 3; front file-back || return; dump; shot prompts-file-3-dismissed
  check "file: closing the picker gives the page no file (cancel event)" 'st "cancels=1" && st "changes=1"'
  open_picker 4; app=$(top_app); check "file: the picker opens again afterwards ($app)" 'echo "$app" | grep -qi "documentsui"'; back 3
  view "$B/p-files.html" 6; t=$(now)
  open_picker 4; dump; has text "krunk-upload.txt" || { tapui desc "Show roots" 2 && tapui text "Downloads" 3; }
  longtap text "krunk-upload.txt"; tapui text "krunk-upload-2.txt" 2; shot prompts-file-4-several; dump
  if has text "SELECT" || has text "Select" || has text "OPEN" || has text "Open"; then
    for b in SELECT Select OPEN Open; do has text "$b" && { tapui text "$b" 6; break; }; done
    front files-picked || return; sleep 2; dump; shot prompts-file-5-several-picked
    check "files: a field for several files gets both" 'st "files=2" && [ "$(since "$t" | grep -c "POST /post?upload=krunk-upload")" = 2 ]'
  else
    echo "SKIP  files: could not select two files in the system picker"; back 2; back 2
  fi
  save prompts-file
}

t_photo() { # a field that asks for a photo: Krunk asks camera or file, titled with the site
  A "logcat -c"; view "$B/p-photo.html" 6; front photo || return; ready "open picker" || bad "photo: the test page did not load"
  open_picker 3; dump; shot prompts-photo-1-question
  check "photo: the question is titled with the asking site" 'has text "$SITE asks for a photo" && has text "Take a photo" && has text "Choose a file"'
  tapui text "Cancel" 3; dump; check "photo: Cancel gives the page no file" 'st "cancels=1" && gone'
  open_picker 3; back 3; dump; check "photo: Back gives the page no file" 'st "cancels=2" && gone'
  open_picker 3; tapui text "Take a photo" 6; local app; app=$(top_app); shot prompts-photo-2-camera
  check "photo: the camera app is in front, no permission asked by Krunk ($app)" '! echo "$app" | grep -q "^$PKG/"'
  back 3; [ "$(top_app | cut -d/ -f1)" = "$PKG" ] || back 3; front photo-back || return; dump; shot prompts-photo-3-dismissed
  check "photo: leaving the camera without a picture gives the page no file" 'st "cancels=3" && st "changes=0"'
  open_picker 3; tapui text "Choose a file" 5; app=$(top_app)
  check "photo: Choose a file opens the picker ($app)" 'echo "$app" | grep -qi "documentsui"'
  back 3; front photo-picker-back || return; dump; check "photo: closing it gives the page no file" 'st "cancels=4"'
  open_picker 3; tapui text "Take a photo" 6; tapui desc "Shutter" 5; tapui desc "Done" 5; front photo-taken || return; sleep 3; dump; shot prompts-photo-4-taken
  check "photo: a taken picture reaches the page as a JPEG" 'st "files=1" && st "names=photo-[0-9a-f-]+\.jpg:[0-9]+:image/jpeg" && st "uploaded=photo-"'
  check "photo: nothing is left in the capture folder" '[ -z "$(A "su 0 sh -c \"ls /data/data/$PKG/cache/capture 2>/dev/null\"" | tr -d "[:space:]")" ]'
  check "photo: the copy for the page is in the tab's upload folder" '[ -n "$(A "su 0 sh -c \"ls /data/data/$PKG/cache/uploads 2>/dev/null\"" | tr -d "[:space:]")" ]'
  stop_clean; launch 8; front photo-restart || return
  check "uploads: the copies are gone after a restart" '[ -z "$(A "su 0 sh -c \"ls /data/data/$PKG/cache/uploads 2>/dev/null\"" | tr -d "[:space:]")" ]'
  save prompts-photo
}

t_recreate() { # "Don't keep activities": the activity is destroyed under an open prompt and under the pickers
  A "settings put global always_finish_activities 1"
  A "logcat -c"; view "$B/p-date.html" 6; front recreate || { A "settings put global always_finish_activities 0"; return; }; ready "open picker" || bad "recreate: the test page did not load"
  open_picker; A "input keyevent KEYCODE_HOME"; sleep 3; A "am start -W -n $PKG/.MainActivity" > /dev/null; sleep 5; dump; shot prompts-recreate-1-date-again
  check "recreate: the date dialog is shown again after the activity was rebuilt" 'has text "$SITE asks for a date"'
  tapui text "Friday, November 13, 2026" 1; tapui text "OK" 3; dump
  check "recreate: and it still answers the page" 'st "value=\[2026-11-13\]" && st "changes=1" && gone'
  local t; t=$(now); view "$B/p-file.html" 6; ready "open picker" || bad "recreate: the file page did not load"
  open_picker 4; pick_file "krunk-upload.txt"; front recreate-file || { A "settings put global always_finish_activities 0"; return; }; sleep 3; dump; shot prompts-recreate-2-file
  check "recreate: a file picked while the activity was gone reaches the page" 'st "files=1" && st "head=krunk upload test file" && since "$t" | grep -q "POST /post?upload=krunk-upload.txt"'
  open_picker 4; back 4; front recreate-file-back || { A "settings put global always_finish_activities 0"; return; }; dump
  check "recreate: a closed picker still gives the page its cancel" 'st "cancels=1"'
  A "settings put global always_finish_activities 0"
  save prompts-recreate
}


# ----- M8: themes -----
px() { convert "$EV/$1.png" -format '%[hex:p{'"$2,$3"'}]' info: 2> /dev/null | cut -c1-6; }   # px SHOT X Y: a pixel's colour
near() { # near HEX HEX: the same colour within what two separately drawn gradients differ by (3 of 255 per channel)
  python3 -I -c 'import sys; a, b = sys.argv[1:3]; sys.exit(0 if len(a) == 6 == len(b) and all(abs(int(a[i:i+2], 16) - int(b[i:i+2], 16)) <= 3 for i in (0, 2, 4)) else 1)' "$1" "$2"
}
greenish() { # greenish SHOT X0 X1 Y0 Y1: some pixel of that area is clearly green (a switch drawn in a green accent)
  convert "$EV/$1.png" -crop "$(($3 - $2))x$(($5 - $4))+$2+$4" -depth 8 txt:- 2> /dev/null | python3 -I -c '
import re, sys
for line in sys.stdin:
    m = re.search(r"#([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})", line)
    if m:
        r, g, b = (int(x, 16) for x in m.groups())
        if g > r + 40 and g > b + 25: sys.exit(0)
sys.exit(1)'
}
switch_green() { # switch_green SHOT ROW-TEXT: the switch at the right end of that row (from the last dump) is green in SHOT
  local b; b=$(attr text "$2" bounds --clickable | tr -c '0-9' ' '); set -- "$1" $b
  [ $# = 5 ] && greenish "$1" $(($4 - 300)) "$4" "$3" "$5"   # the whole row's height: a three-line row has its switch at the top
}
ntp_and_menu() { # ntp_and_menu LABEL BACKGROUND RAISED: the new-tab page and the open menu in the theme's colours
  menu; tapui text "New tab" 4; shot "themes-$1-newtab"; dump
  check "$1: new-tab page under the styled toolbar (strip $(px "themes-$1-newtab" 300 20), page $(px "themes-$1-newtab" 30 2000))" '[ "$(px "themes-'"$1"'-newtab" 300 20)" != "$plain" ] && near "$(px "themes-'"$1"'-newtab" 30 2000)" "'"$2"'" && has text "Add shortcut"'
  menu; shot "themes-$1-menu"; dump
  check "$1: the menu is drawn on the theme's raised surface ($(px "themes-$1-menu" 1010 550))" 'near "$(px "themes-'"$1"'-menu" 1010 550)" "'"$3"'" && has text "New private tab"'
  back 2; tapui desc "Tabs:" 3 --contains; tapui desc "Links, tab" 4   # close the menu, back to the page's tab
}
card_px() { # card_px SHOT DESC: a pixel of that tab card's title row, between title and close button (from the last dump)
  local b; b=$(attr desc "$2" bounds | tr -c '0-9' ' '); set -- "$1" $b
  [ $# = 5 ] && px "$1" $(($2 + ($4 - $2) * 6 / 10)) $(($3 + 55))
}
to_settings() { menu && tapui text "Settings" 3; }
pick_style() { # pick_style NAME: Settings > Style > NAME > Done (from the browser screen), back to the browser
  to_settings && scrollto text "Style" && tapui text "Style" 2 || return 1
  dump; has desc "$1" || { A "input swipe 540 1500 540 900 300"; sleep 1; }
  tapui desc "$1" 2; dump; STYLE_CHECKED=$(attr desc "$1" checked --clickable)
  [ -n "${2:-}" ] && shot "$2"
  tapui text "Done" 2; back 2
}
pick_mode() { to_settings && scrollto text "Theme" && tapui text "Theme" 2 && tapui text "$1" 2; back 2; }   # Light | Dark | Follow the system
switcher_shot() { tapui desc "Tabs:" 3 --contains; shot "$1"; dump; back 2; }
t_themes() {
  # A fresh profile: two tabs in the switcher, so the active card and another are both on screen.
  A "setprop debug.krunk.show_private 1"; stop_clean; A "pm clear $PKG" > /dev/null; A "logcat -c"
  launch 12; view "$B/links.html" 8; front themes || return; ready "1 link with utm_source, fbclid" || bad "themes: the test page did not load"
  pick_mode Light; shot themes-01-default-light; local plain; plain=$(px themes-01-default-light 300 20)
  to_settings; scrollto text "Style"; dump
  check "settings: Style and Accent colour rows (Default, Krunk blue)" 'has text "Style" && has text "Default" && has text "Accent colour" && has text "Krunk blue"'
  tapui text "Style" 2; dump; shot themes-02-picker-light
  local n missing=""; for n in Classic Sunset Ocean Glass Mono Aurora; do has desc "$n" || missing="$missing $n"; done
  A "input swipe 540 1500 540 800 300"; sleep 1; dump; shot themes-03-picker-light-more
  for n in Midnight Forest Ember Candy Frost Neon; do has desc "$n" || missing="$missing $n"; done
  check "picker: the twelve styles are offered by name (missing:$missing)" '[ -z "$missing" ]'
  tapui text "Done" 2; back 2
  pick_style Sunset themes-04-picker-sunset-chosen
  check "picker: the chosen style is marked" '[ "$STYLE_CHECKED" = true ]'
  shot themes-05-sunset-light; dump
  check "sunset, light: the strip behind the status bar is no longer the plain surface ($(px themes-05-sunset-light 300 20) vs $plain)" '[ "$(px themes-05-sunset-light 300 20)" != "$plain" ]'
  check "sunset, light: strip and toolbar are one piece ($(px themes-05-sunset-light 300 20) / $(px themes-05-sunset-light 300 141))" 'near "$(px themes-05-sunset-light 300 20)" "$(px themes-05-sunset-light 300 141)"'
  check "sunset, light: the gradient runs across the toolbar ($(px themes-05-sunset-light 60 141) to $(px themes-05-sunset-light 950 141))" '[ "$(px themes-05-sunset-light 60 141)" != "$(px themes-05-sunset-light 950 141)" ]'
  check "the page and the toolbar's controls are still there" 'has desc "Edit address" && has desc "Menu" && has text "1 link with utm_source, fbclid"'
  ntp_and_menu sunset-light F3F4F6 FFFFFF
  tapui desc "Tabs:" 3 --contains; shot themes-06-sunset-light-switcher; dump
  local on off; on=$(card_px themes-06-sunset-light-switcher "Links, current tab"); off=$(card_px themes-06-sunset-light-switcher "New tab, tab"); back 2
  check "sunset (fill): the active card's title row is filled ($on vs $off)" '[ -n "$on" ] && [ -n "$off" ] && [ "$on" != "$off" ]'
  to_settings; scrollto text "Style"; dump; check "settings: the row names the style" 'has text "Sunset"'; back 2
  pick_mode Dark; shot themes-07-sunset-dark; ntp_and_menu sunset-dark 1F2023 2B2D31
  switcher_shot themes-08-sunset-dark-switcher
  to_settings; scrollto text "Style"; tapui text "Style" 2; shot themes-09-picker-dark; tapui text "Done" 2; back 2
  pick_style Neon; check "picker: Neon can be chosen" '[ "$STYLE_CHECKED" = true ]'
  shot themes-10-neon-dark; switcher_shot themes-11-neon-dark-switcher
  pick_mode Light; shot themes-12-neon-light
  tapui desc "Tabs:" 3 --contains; shot themes-13-neon-light-switcher; dump
  on=$(card_px themes-13-neon-light-switcher "Links, current tab"); off=$(card_px themes-13-neon-light-switcher "New tab, tab"); back 2
  check "neon (outline): the active card's title row is not filled ($on = $off)" '[ -n "$on" ] && [ "$on" = "$off" ]'
  check "neon, light: a plain toolbar again ($(px themes-12-neon-light 60 141) = $(px themes-12-neon-light 950 141))" '[ "$(px themes-12-neon-light 60 141)" = "$(px themes-12-neon-light 950 141)" ]'
  to_settings; scrollto text "Accent colour"; tapui text "Accent colour" 2; dump; shot themes-14-accent-picker
  check "accent: seven named colours and another colour" 'has desc "Krunk blue" && has desc "Green" && has desc "Violet" && has text "Another colour"'
  tapui desc "Green" 2; dump; check "accent: the chosen colour is marked" '[ "$(attr desc Green checked --clickable)" = true ]'
  tapui text "Done" 2; dump; shot themes-15-settings-green; check "settings: the row names the accent" 'has text "Green"'
  A "input swipe 540 900 540 1800 300"; sleep 1; scrollto text "Search suggestions"; dump; shot themes-15b-settings-switch-green
  check "settings draws the accent: the switch of Search suggestions is green" 'switch_green themes-15b-settings-switch-green "Search suggestions"'
  back 2; switcher_shot themes-16-neon-green-switcher
  tapui desc "requests blocked on this page" 3 --contains; dump; shot themes-16b-shield-sheet-green
  check "the shield sheet draws the accent: its switch is green" 'has text "Block on this site" && switch_green themes-16b-shield-sheet-green "Block on this site"'
  back 2
  pick_style Glass; shot themes-17-glass-green-light
  menu; tapui text "New private tab" 4; shot themes-18-private-tab; local private; private=$(px themes-18-private-tab 300 20)
  dcheck "private tab: its own look ($private; glass $(px themes-17-glass-green-light 300 20), plain $plain)" '[ "$private" != "$plain" ] && [ "$private" != "$(px themes-17-glass-green-light 300 20)" ]'
  pick_style Sunset; shot themes-19-private-tab-sunset
  dcheck "private tab: the same under another style ($(px themes-19-private-tab-sunset 300 20))" '[ "$(px themes-19-private-tab-sunset 300 20)" = "$private" ]'
  stop_clean; launch 8; front themes-restart || return; to_settings; scrollto text "Style"; dump; shot themes-20-after-restart
  check "restart: style and accent are kept" 'has text "Sunset" && has text "Green"'
  back 2
  # Sunset with the toolbar at the bottom: the gradient continues behind the navigation bar, the status strip is plain.
  view "$B/links.html" 6; to_settings; scrollto text "Toolbar position"; tapui text "Toolbar position" 2; tapui text "Bottom" 2; back 2; shot themes-20b-sunset-bottom-toolbar; dump
  check "bottom toolbar: the navigation-bar area continues the toolbar ($(px themes-20b-sunset-bottom-toolbar 200 2390) / $(px themes-20b-sunset-bottom-toolbar 200 2200))" 'near "$(px themes-20b-sunset-bottom-toolbar 200 2390)" "$(px themes-20b-sunset-bottom-toolbar 200 2200)" && [ "$(px themes-20b-sunset-bottom-toolbar 60 2200)" != "$(px themes-20b-sunset-bottom-toolbar 950 2200)" ]'
  check "bottom toolbar: no gradient behind the status bar ($(px themes-20b-sunset-bottom-toolbar 300 20))" 'near "$(px themes-20b-sunset-bottom-toolbar 300 20)" F3F4F6'
  check "bottom toolbar: page and controls are there" 'has desc "Edit address" && has desc "Menu" && has text "1 link with utm_source, fbclid"'
  to_settings; scrollto text "Toolbar position"; tapui text "Toolbar position" 2; tapui text "Top" 2; back 2
  # Back to how it was.
  pick_style Classic; to_settings; scrollto text "Accent colour"; tapui text "Accent colour" 2; tapui desc "Krunk blue" 2; tapui text "Done" 2; back 2
  pick_mode "Follow the system"; shot themes-21-classic-again
  A "setprop debug.krunk.show_private ''"
  save themes
}

# ----- M9: switcher actions, desktop mode kept per tab, small rows -----
longtap_c() { dump; local xy; xy=$(echo "$UI" | python3 -I "$HERE/uix.py" find "$1" "$2" --contains) || { bad "no '$2' on screen"; return 1; }
  # shellcheck disable=SC2086
  A "input swipe $xy $xy 900"; sleep 2; }
t_switcher() { # long press on a tab card: Duplicate, Close other tabs (same kind only), Close tab
  stop_clean; A "pm clear $PKG" > /dev/null; A "logcat -c"; launch 12; view "$B/links.html" 8; front switcher || return
  ready "1 link with utm_source, fbclid" || bad "switcher: the test page did not load"
  tapui desc "Tabs:" 3 --contains; dump; check "two tabs to start with" 'has text "Tabs (2)"'
  longtap_c desc "Links, current tab"; dump; shot small-1-card-menu
  check "long press on a card: Duplicate, Close other tabs, Close tab" 'has text "Duplicate" && has text "Close other tabs" && has text "Close tab"'
  tapui text "Duplicate" 5; dump; has text "Tabs (3)" || { tapui desc "Tabs:" 3 --contains; dump; }; shot small-2-duplicated
  check "duplicate: three tabs, two of them the Links page ($(echo "$UI" | grep -o 'content-desc="Links, [a-z ]*tab"' | wc -l))" 'has text "Tabs (3)" && [ "$(echo "$UI" | grep -o "content-desc=\"Links, [a-z ]*tab\"" | wc -l)" = 2 ]'
  tapui desc "Links, current tab" 5 --contains; dump; check "duplicate: the copy shows the same page" 'has text "1 link with utm_source, fbclid"'
  menu; tapui text "New private tab" 4; tapui desc "Tabs:" 3 --contains; dump
  tapui text "Private (1)" 2; longtap_c desc ", current tab"; dump; shot small-3-private-card-menu
  check "private card: its entry closes private tabs only, and has nothing to close" 'has text "Close other private tabs" && [ "$(attr text "Close other private tabs" enabled --clickable)" = false ]'
  back 2; tapui text "Tabs (3)" 2; longtap_c desc "Links, tab"; dump
  check "normal card with two others: Close other tabs is enabled" '[ "$(attr text "Close other tabs" enabled --clickable)" = true ]'
  tapui text "Close other tabs" 4; dump; shot small-4-closed-others
  check "close others: one normal tab left, the private tab untouched" 'has text "Tabs (1)" && has text "Private (1)"'
  tapui desc "Recently closed" 3; dump; shot small-5-recently-closed
  check "close others: the closed tabs are in Recently closed" 'has text "Links"'
  back 2; save small-switcher
}

t_desktop() { # "Request desktop site" belongs to the tab: kept over a recreated session and a restart
  A "logcat -c"; local t; t=$(now); view "$B/ua.html" 8; front desktop || return; ready "User agent" || bad "desktop: the test page did not load"
  dump; check "a new tab asks for the mobile site" 'st "kind=mobile"'
  menu; dump; check "menu: the row is a check box, not checked" '[ "$(attr text "Request desktop site" checked --clickable)" = false ] && [ "$(attr text "Request desktop site" checkable --clickable)" = true ]'
  tapui text "Request desktop site" 6; dump; shot small-6-desktop
  check "desktop site: the page comes back with a desktop user agent" 'st "kind=desktop"'
  menu; dump; check "menu: the row is checked" '[ "$(attr text "Request desktop site" checked --clickable)" = true ]'; back 2
  dbg recreate-session; sleep 7; dump; shot small-7-desktop-recreated; glog | grep "recreate-session" | tail -1 | cut -c1-160
  dcheck "recreated session (F-43): still the desktop site" 'st "kind=desktop" && glog | grep -q "recreate-session: tab .* desktop=true"'
  dcheck "recreated session: its first load was already a desktop one (no mobile request in between)" '! since "$t" | sed -n "/kind=desktop/,\$p" | grep -q "kind=mobile"'
  menu; dump; dcheck "recreated session: the menu row is still checked" '[ "$(attr text "Request desktop site" checked --clickable)" = true ]'; back 2
  stop_clean; launch 10; front desktop-restart || return; ready "User agent" 15; dump; shot small-8-desktop-restart
  check "restart: the tab is still on the desktop site" 'st "kind=desktop"'
  menu; tapui text "Request desktop site" 6; dump; check "switched off: the mobile site again" 'st "kind=mobile"'
  save small-desktop
}

link_tabs() { to_settings && scrollto text "Tabs opened by links" && tapui text "Tabs opened by links" 2 && tapui text "$1" 2 --contains; back 2; }
t_rows() { # F-52 switch rows, F-61 reload target, the "tabs opened by links" setting
  A "logcat -c"; view "$B/links.html" 8; front rows || return; ready "1 link with utm_source, fbclid" || bad "rows: the test page did not load"
  dump; check "F-61: reload's touch target is 48 dp (126 px): $(echo "$UI" | python3 -I "$HERE/uix.py" size desc Reload --clickable)" '[ "$(echo "$UI" | python3 -I "$HERE/uix.py" size desc Reload --clickable)" = "126 126" ]'
  to_settings; scrollto text "Search suggestions"; dump
  check "F-52: a switch row is one checkable node with its state" '[ "$(attr text "Search suggestions" checkable --clickable)" = true ] && [ -n "$(attr text "Search suggestions" checked --clickable)" ]'
  back 2
  link_tabs "Always in front"; longtap text "4 clean link"; tapui text "Open in new tab" 6; dump; shot small-9-link-tab-front
  check "tabs opened by links, always in front: the new tab is shown" 'has text "Landed on"'
  view "$B/links.html" 6; link_tabs "Always behind"; local before; dump; before=$(echo "$UI" | grep -o "Tabs: [0-9]*" | head -1)
  longtap text "4 clean link"; tapui text "Open in new tab" 6; dump; shot small-10-link-tab-behind
  check "always behind: the page stays, one more tab (before: $before)" 'has text "4 clean link" && ! echo "$UI" | grep -q "$before open"'
  link_tabs "As the page asks"
  save small-rows
}

# ----- upgrade from the published release (release APKs: no debug hooks; the emulator's root reads the files) -----
Q="'"   # a single quote for SQL inside the checks' own single-quoted conditions
db() { A "su 0 sqlite3 /data/data/$PKG/files/krunk/browser.db \"$1\"" 2> /dev/null | tr -d '\r'; }
t_upgrade() { # 0.1.1 with tabs, a bookmark, history and a changed setting; 0.2.0 over it: all kept, blocking on, allow kept, nothing from a private tab
  local old="${KRUNK_OLD_APK:?the published APK to upgrade from}" t
  A "input keyevent KEYCODE_HOME"; sleep 1; A "am force-stop $PKG"; T "$E" uninstall > /dev/null 2>&1
  T "$E" adb install "$old" 2>&1 | tail -1; A "logcat -c"; launch 20; front upgrade-old || return
  echo "  installed: $(A "dumpsys package $PKG" | grep -m1 -oE "versionName=[^ ]+")"
  view "$B/links.html" 10; view "$B/ads.html" 10; dump; shot upgrade-1-old-version
  menu; tapui text "Bookmark this page" 3; dump
  menu; tapui text "Settings" 3; tapui text "Search engine" 2; tapui text "Startpage" 2; dump; shot upgrade-2-old-settings
  check "0.1.1: state made (3 tabs, a bookmark, history, search engine Startpage)" 'has text "Startpage" && [ "$(db "select count(*) from bookmarks where url like ${Q}%ads.html${Q}")" = 1 ] && [ "$(db "select count(*) from urls where url like ${Q}%18080/%${Q}")" -ge 2 ]'
  local hist marks; hist=$(db "select count(*) from visits"); marks=$(db "select count(*) from bookmarks")
  back 2; stop_clean
  T "$E" adb install -r "$APK" 2>&1 | tail -1
  check "0.2.0 installed over it: $(A "dumpsys package $PKG" | grep -m1 -oE "versionName=[^ ]+")" 'A "dumpsys package $PKG" | grep -q "versionName=0.2.0"'
  A "logcat -c"; t=$(now); cap_start upgrade-first-start; launch 25; front upgrade-new || return; dump; shot upgrade-3-first-start
  check "upgrade start: the restored page is filtered ($(served "$t")) and the shield counts 5" '[ "$(served "$t")" = "/ads.html /ok.png " ] && has desc "5 requests blocked on this page"'
  check "upgrade start: the three tabs are back" 'has desc "Tabs: 3 open" --contains'
  sleep 20; cap_stop upgrade-first-start > /dev/null; save upgrade-first-start
  grep -oE "blocker ready after.*" "$EV/upgrade-first-start-logcat.txt" | tail -1 | sed 's/^/  upgrade start: /'
  check "upgrade start: packets only to the test server ($(packets upgrade-first-start) packets)" '[ "$(packets upgrade-first-start)" -gt 0 ] && ! grep -E "sni|quic" "$EV/upgrade-first-start-pcap-summary.txt"'
  check "history and bookmarks are kept ($hist visits, $marks bookmarks before)" '[ "$(db "select count(*) from visits")" -ge "$hist" ] && [ "$(db "select count(*) from bookmarks")" = "$marks" ]'
  menu; tapui text "Bookmarks" 3; tapui text "Other bookmarks" 2; dump; has text "Ad test" || { back 2; tapui text "Bookmarks bar" 2; dump; }
  shot upgrade-4-bookmarks; check "the bookmark is in the manager" 'has text "Ad test"'; back 2; back 2
  menu; tapui text "History" 3; dump; shot upgrade-5-history; check "the history lists the pages visited with 0.1.1" 'has text "Ad test" && has text "Links"'; back 2
  to_settings; dump; shot upgrade-6-settings; check "the changed setting is kept (search engine Startpage)" 'has text "Startpage"'
  scrollto text "Block ads and trackers"; dump; check "blocking is on by default after the upgrade" '[ "$(attr text "Block ads and trackers" checked --clickable)" = true ]'
  scrollto text "Update automatically"; dump; check "automatic updates are off by default after the upgrade" '[ "$(attr text "Update automatically" checked --clickable)" = false ]'
  back 2
  # A per-site allow made in 0.2.0 is kept over a restart.
  tapui desc "5 requests blocked on this page" 2; t=$(now); tapui text "Block on this site" 10; dump; shot upgrade-7-allowed
  check "allow in 0.2.0: the page reloads unfiltered ($(served "$t" | wc -w) requests)" '[ "$(served "$t" | wc -w)" = 7 ] && has desc "Blocking is off for this site"'
  stop_clean; t=$(now); launch 15; front upgrade-restart || return; sleep 5; dump
  check "allow: still set after a restart ($(served "$t" | wc -w) requests; stored: $(db "select * from adblock_allowlist" | tr '\n' ' '))" '[ "$(served "$t" | wc -w)" = 7 ] && has desc "Blocking is off for this site" && [ -n "$(db "select * from adblock_allowlist")" ]'
  tapui desc "Blocking is off for this site" 2; tapui text "Block on this site" 10; dump
  check "allow removed again: filtered, nothing stored" 'has desc "5 requests blocked on this page" && [ -z "$(db "select * from adblock_allowlist")" ]'
  # A private tab: no allow switch; "Open once" on a blocked address is the only allow there is, and it leaves nothing behind.
  local blocked="https://googleads.g.doubleclick.net/pagead/x"
  menu; tapui text "New private tab" 4; tapui desc "Edit address" 2; A "input text '$B/ads.html'"; A "input keyevent 66"; sleep 10
  tapui desc "5 requests blocked on this page" 2; dump
  check "private tab: the sheet has no allow switch" '! has text "Block on this site" && has text "5 requests blocked on this page"'
  back 2; tapui desc "Edit address" 2; A "input text '$blocked'"; A "input keyevent 66"; sleep 8; dump
  check "private tab: a blocked address shows Krunk's page" 'has text "Open once"'
  tapui text "Open once" 8; dump; check "private tab: Open once goes on to the address" '! has text "Open once"'
  tapui desc "Tabs:" 3 --contains; tapui text "Private (1)" 2; tapui desc "Close " 3 --contains; dump
  check "the private tab is closed" 'has text "Private (0)"'
  back 2; local open_at; open_at=$(date +%s)
  to_settings; scrollto text "Sites with blocking off"; dump; shot upgrade-8-after-private
  check "after the private tab: Settings lists no site with blocking off" 'has text "None" || has text "0 sites" || ! has text "1 site"'
  back 2
  local traces; traces=$(A "su 0 sh -c \"cat /data/data/$PKG/files/krunk/browser.db /data/data/$PKG/files/krunk/browser.db-wal 2>/dev/null | strings | grep -c googleads.g.doubleclick\"" | tr -d "[:space:]")
  check "after the private tab: nothing in the allow table, the address nowhere in Krunk's database ($traces places; $(db "select count(*) from urls where url like ${Q}%doubleclick%${Q}") history rows)" '[ -z "$(db "select * from adblock_allowlist")" ] && [ "$(db "select count(*) from urls where url like ${Q}%doubleclick%${Q}")" = 0 ] && [ "$traces" = 0 ]'
  # "Open once" is the blocker's own short memory (not Krunk's, not stored): say how long it lasts after the private tab is gone.
  local now_s; while :; do now_s=$(( $(date +%s) - open_at )); [ "$now_s" -ge 150 ] && break; sleep 10; done
  view "$blocked" 8; dump; now_s=$(( $(date +%s) - open_at )); shot upgrade-9-blocked-again
  check "${now_s} s after the private tab was closed, without a restart: the address is blocked again" 'has text "Open once"'
  stop_clean; launch 12; view "$blocked" 8; dump
  check "after a restart the address is blocked as well" 'has text "Open once"'
  save upgrade
}

case "${1:-}" in
  gate)
    shift
    A "svc power stayon true"
    for t in "${@:-idle allow blocked update starts hold off guardlost noguard}"; do
      for one in $t; do
        echo "== $one $(date -u +%T)"
        if [ -n "$REL" ]; then
          case "$one" in
            allow) skip "gate allow: uses the debug hook; the same through the UI is 'ui shield' and 'ui settings'"; continue ;;
            update) one=relupdate; echo "   (release build: the update is started in Settings and captured)" ;;
            off) skip "gate off: uses the debug hook; the same through the UI is 'ui settings' (blocking switched off)"; continue ;;
            guardlost | noguard) skip "gate $one: only possible with the debug build's hooks (checked on the debug build of the same commit)"; continue ;;
          esac
        fi
        "t_$one"
      done
    done
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  ui)
    shift
    A "svc power stayon true"
    for t in "${@:-toolbar menu shield private settings}"; do
      for one in $t; do echo "== $one $(date -u +%T)"; "t_$one"; A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; sleep 1; A "am start -W -n $PKG/.MainActivity" > /dev/null; sleep 8; done
    done
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  links)
    shift
    A "svc power stayon true"
    for t in "${@:-rewrite handoff selection}"; do
      for one in $t; do echo "== $one $(date -u +%T)"; "t_$one"; A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; sleep 1; A "am start -W -n $PKG/.MainActivity" > /dev/null; sleep 8; done
    done
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  small)
    shift
    A "svc power stayon true"
    for t in "${@:-switcher desktop rows}"; do
      for one in $t; do echo "== $one $(date -u +%T)"; "t_$one"; A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; sleep 1; done
    done
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  upgrade)
    A "svc power stayon true"; REL=1; APK=$RELEASE_APK; echo "== upgrade $(date -u +%T)"; t_upgrade; A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  themes)
    A "svc power stayon true"; echo "== themes $(date -u +%T)"; t_themes; A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  prompts)
    shift
    A "svc power stayon true"
    for t in "${@:-date time datetime month week colour leave repost share file photo recreate}"; do
      for one in $t; do echo "== $one $(date -u +%T)"; "t_$one"; A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; sleep 1; done
    done
    echo "passed $PASS, failed $FAIL, skipped $SKIP"
    [ "$FAIL" = 0 ]
    ;;
  *) sed -n '2,11p' "$0"; exit 2 ;;
esac
