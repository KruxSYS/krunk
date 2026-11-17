#!/usr/bin/env bash
# Krunk for Android 0.10.0 acceptance pass for the in-browser VPN (plan §8, T1–T8) on emulator-5554.
#
#   android/scripts/a9/a9.sh up | down          test servers + relays on this sandbox, VPN testbed on dind
#   android/scripts/a9/a9.sh profiles           device copies of the testbed profiles -> /sdcard/Download
#   android/scripts/a9/a9.sh <step>...          t1 t2 t3 t4 t5 t6 t7 t8 ovpn leakgrep (see each function)
#
# Environment: EV evidence folder (default .pipeline/evidence-android-0.10.0/run), APK (release
# x86_64 APK for `fresh`). Results: one line per check in $EV/results.txt.
#
# Network shape (NOTES-android-0.10.0.md §S2): the emulator reaches this sandbox at 10.250.136.3,
# the testbed (dind) reaches it at 10.250.137.3 and the emulator cannot reach 10.250.137.3. The
# test server (0.0.0.0:18080, UDP sink 3478) logs the source of every request: 10.250.137.2 =
# through the tunnel (dind NAT, full-tunnel profile), 10.250.136.2 = direct from the emulator.
# WireGuard: device -> 10.250.136.3:18551/udp (relay.py) -> dind:8551. Split profile: inside.test
# (10.77.0.10, tunnel DNS 10.77.0.53) answers "client=<tunnel IP>".
#
# Keep emulator time short (memory: graphics wedge): one flow per launch, config changes last,
# never build while this runs.
set -uo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SRC=$(cd "$HERE/../../.." && pwd)
PIPE=$(cd "$SRC/.." && pwd)/.pipeline
EV=${EV:-$PIPE/evidence-android-0.10.0/run}
PROF=$PIPE/evidence-android-0.10.0/profiles
mkdir -p "$EV" "$PROF"
PKG=org.krunk.browser
DEV=${DEV:-emulator-5554}
A() { adb -s "$DEV" shell "$@"; }
R() { adb -s "$DEV" shell "su 0 sh -c '$*'"; }
RES=$EV/results.txt
SLOG=$EV/server.log
TB="docker compose -p vpn-testbed -f $SRC/tools/vpn-testbed/compose.yaml"

result() { # result T-id PASS|FAIL|INFO text
  printf '%s %-5s %-4s %s\n' "$(date -u +%H:%M:%S)" "$1" "$2" "$3" | tee -a "$RES"
}
mark() { wc -l < "$SLOG" 2> /dev/null || echo 0; }
since() { tail -n +"$(( $1 + 1 ))" "$SLOG"; }
shot() { adb -s "$DEV" exec-out screencap -p > "$EV/$1.png"; }
view() { A "am start -a android.intent.action.VIEW -d '$1' $PKG" > /dev/null 2>&1; sleep "${2:-10}"; }
stop_clean() { A input keyevent KEYCODE_HOME; sleep 3; A am force-stop $PKG; }
launch() { A "am start -W -n $PKG/.MainActivity" > /dev/null 2>&1; sleep "${1:-6}"; }
app_uid() { A "pm list packages -U $PKG" | grep -oE "uid:[0-9]+" | head -1 | cut -d: -f2; }

# Loopback listeners owned by Krunk (root reads /proc/net/tcp; release builds have no hooks).
krunk_ports() {
  local uid; uid=$(app_uid)
  R "cat /proc/net/tcp" | awk -v u="$uid" '$4 == "0A" && $8 == u { split($2, a, ":"); if (a[1] == "0100007F") print a[2] }' |
    while read -r hex; do printf '%d\n' "0x$hex"; done
}

up() {
  $TB up -d --build > /dev/null && $TB ps --format '{{.Service}} {{.Status}}'
  pgrep -f "a9/testserver.py 18080" > /dev/null || nohup python3 -I "$HERE/testserver.py" 18080 "$SLOG" 3478 > "$EV/testserver.out" 2>&1 &
  pgrep -f "relay.py udp 18551" > /dev/null || nohup python3 -I "$HERE/relay.py" udp 18551 dind 8551 > "$EV/relay-wg.out" 2>&1 &
  pgrep -f "relay.py tcp 18194" > /dev/null || nohup python3 -I "$HERE/relay.py" tcp 18194 dind 8194 > "$EV/relay-ovpn.out" 2>&1 &
  pgrep -f "relay.py udp 18195" > /dev/null || nohup python3 -I "$HERE/relay.py" udp 18195 dind 8194 > "$EV/relay-ovpn-udp.out" 2>&1 &
  # https behind the tunnel (T1): a self-signed server from the 0.6.1 certificate tests.
  local bc=$PIPE/evidence-android-0.6.1/tools
  pgrep -f "badcert.py 18443" > /dev/null || nohup python3 -I "$bc/badcert.py" 18443 "$bc/cert1.pem" "$bc/key1.pem" > "$EV/badcert.out" 2>&1 &
  sleep 1
  pgrep -af "a9/(testserver|relay)|badcert" | grep -v pgrep
}

down() {
  for p in $(pgrep -f "a9/(testserver|relay).py|badcert.py 18443"); do kill "$p"; done
  echo "servers stopped (testbed left up; '$TB down -v --rmi local' removes it)"
}

# Device copies of the testbed profiles (test-only keys). Endpoint = the UDP relay; OpenVPN TCP via
# the TCP relay. They go to /sdcard/Download for the SAF import and are deleted by `cleanup`.
profiles() {
  $TB exec -T wg cat /state/wg/client.conf > "$PROF/wg-split.conf"
  $TB exec -T wg cat /state/wg/client-full.conf > "$PROF/wg-full.conf"
  sed -i 's/^Endpoint = .*/Endpoint = 10.250.136.3:18551/' "$PROF"/wg-*.conf
  $TB exec -T ovpn cat /state/ovpn/client-tcp.ovpn | sed 's/^remote .*/remote 10.250.136.3 18194 tcp/' > "$PROF/ovpn-tcp.ovpn"
  $TB exec -T ovpn cat /state/ovpn/client-tcp-badca.ovpn | sed 's/^remote .*/remote 10.250.136.3 18194 tcp/' > "$PROF/ovpn-badca.ovpn"
  $TB exec -T ovpn cat /state/ovpn/client-udp.ovpn | sed 's/^remote .*/remote 10.250.136.3 18195 udp/' > "$PROF/ovpn-udp.ovpn"
  cp "$PROF/ovpn-tcp.ovpn" "$PROF/ovpn-wrongpw.ovpn"
  chmod 600 "$PROF"/*
  for f in "$PROF"/*; do adb -s "$DEV" push "$f" /sdcard/Download/ > /dev/null; done
  A ls -l /sdcard/Download | grep -E "conf|ovpn"
}

cleanup() { A "rm -f /sdcard/Download/wg-*.conf /sdcard/Download/ovpn-*.ovpn"; }

# T2: another uid (shell) finds the port and tries HTTP CONNECT and SOCKS5 without the credential:
# the proxy must close without a single byte.
t2() {
  local ports p got; ports=$(krunk_ports)
  [ -n "$ports" ] || { result T2 FAIL "no loopback listener of Krunk found (VPN not started?)"; return 1; }
  for p in $ports; do
    got=$(A "printf 'CONNECT 10.250.136.3:18080 HTTP/1.1\r\nHost: x\r\n\r\n' | toybox timeout 8 toybox nc 127.0.0.1 $p | toybox wc -c" | tr -d '\r ')
    [ "$got" = 0 ] && result T2 PASS "port $p: HTTP CONNECT without credential closed, 0 bytes" ||
      result T2 FAIL "port $p: HTTP CONNECT without credential got $got bytes"
    got=$(A "printf '\005\001\000' | toybox timeout 8 toybox nc 127.0.0.1 $p | toybox wc -c" | tr -d '\r ')
    [ "$got" = 0 ] && result T2 PASS "port $p: SOCKS5 no-auth greeting closed, 0 bytes" ||
      result T2 FAIL "port $p: SOCKS5 no-auth greeting got $got bytes"
  done
}

# T7: no VPN network, no key icon (VpnService not used), other apps direct.
t7() {
  local n; n=$(A dumpsys connectivity | grep -c "VPN CONNECTED\|type: VPN\|NetworkAgentInfo.*VPN")
  [ "$n" = 0 ] && result T7 PASS "dumpsys connectivity: no VPN network" || result T7 FAIL "dumpsys connectivity shows $n VPN lines"
  A dumpsys connectivity > "$EV/t7-dumpsys-connectivity.txt"
  local m; m=$(mark)
  A "printf 'GET /t7-shell HTTP/1.0\r\n\r\n' | toybox timeout 5 toybox nc 10.250.136.3 18080" > /dev/null
  sleep 1
  since "$m" | grep -q '"10.250.136.2".*t7-shell' && result T7 PASS "adb shell (other uid) reaches the sandbox direct" ||
    result T7 FAIL "adb shell request not seen direct"
}

# Secrets must not be in logcat or in plaintext under the app's data directory.
leakgrep() {
  local key pat hits
  key=$(grep -m1 '^PrivateKey' "$PROF/wg-split.conf" | sed 's/^PrivateKey = //')
  for pat in "$key" "PrivateKey" "BEGIN PRIVATE KEY" "krunk-test" "\[Interface\]"; do
    hits=$(R "grep -rIl -F -- \"$pat\" /data/data/$PKG /data/user_de/0/$PKG 2>/dev/null | head -5")
    [ -z "$hits" ] && result LEAK PASS "no plaintext '${pat:0:12}…' under the app data" || result LEAK FAIL "'${pat:0:12}…' in: $hits"
  done
  adb -s "$DEV" logcat -d > "$EV/logcat-full.txt"
  for pat in "$key" "PrivateKey" "Proxy-Authorization" "krunk-test"; do
    grep -qF -- "$pat" "$EV/logcat-full.txt" && result LEAK FAIL "logcat contains '${pat:0:12}…'" || result LEAK PASS "logcat clean of '${pat:0:12}…'"
  done
  # The session token: 64 hex chars next to "krunk" would be the SOCKS password.
  grep -qE 'krunk:[0-9a-f]{64}|[0-9a-f]{64}' "$EV/logcat-full.txt" && result LEAK INFO "64-hex string in logcat (check: $(grep -oE '[0-9a-f]{64}' "$EV/logcat-full.txt" | head -1 | cut -c1-8)…)" ||
    result LEAK PASS "no 64-hex token in logcat"
}

# ---------------------------------------------------------------- UI helpers (uiautomator)
U="$HERE/../a3/uix.py"
dump() { A "uiautomator dump /sdcard/ui.xml" > /dev/null 2>&1; UI=$(A "cat /sdcard/ui.xml"); }
has() { echo "$UI" | python3 -I "$U" has "$@"; }
tapt() { # tapt text|desc VALUE [--contains]
  dump; local xy; xy=$(echo "$UI" | python3 -I "$U" find "$1" "$2" ${3:-}) || { echo "MISSING $2"; return 1; }
  # shellcheck disable=SC2086
  A input tap $xy; sleep 2
}
waitt() { # waitt text|desc VALUE seconds [--contains]: 0 when seen
  local i; for ((i = 0; i < $3; i += 2)); do dump; has "$1" "$2" ${4:-} && return 0; sleep 2; done; return 1
}

# Krunk's sockets (uid of the app, from /proc/net as root): every remote must be the loopback
# proxy or the VPN endpoint (the tunnel runs in Krunk's process). Anything else is a direct
# connection = a leak. Samples every second into $EV/sockets-<tag>.txt until the step ends.
ENDPOINT_RE=${ENDPOINT_RE:-'^0388FA0A:'}   # 10.250.136.3 little-endian hex = the WG/OpenVPN relay
sockwatch_start() {
  local uid tag=$1; uid=$(app_uid)
  ( while :; do
      R "cat /proc/net/tcp /proc/net/tcp6 /proc/net/udp /proc/net/udp6" 2> /dev/null |
        awk -v u="$uid" '$8 == u && $3 !~ /^0+:0000$/ && $3 !~ /^0+:0000$/ { print $3 }' |
        grep -vE '^0100007F:|^0000000000000000FFFF00000100007F:|^00000000000000000000000001000000:|^00000000000000000000000000000000:' |
        grep -vE "$ENDPOINT_RE" | sed "s/^/$(date +%T) /"
      sleep 1
    done ) > "$EV/sockets-$tag.txt" 2>&1 &
  SOCKWATCH=$!
}
sockwatch_stop() { # sockwatch_stop TID: PASS when no foreign remote was seen
  kill "$SOCKWATCH" 2> /dev/null; wait "$SOCKWATCH" 2> /dev/null
  local f="$EV/sockets-$2.txt"
  [ -s "$f" ] && result "$1" FAIL "Krunk had direct sockets: $(sort -u -k2 "$f" | head -3 | tr '\n' ' ')" ||
    result "$1" PASS "Krunk's sockets: only the proxy and the VPN endpoint ($2)"
}

# Import a profile pushed by `profiles` through the share path (content:// of MediaStore).
share_import() { # share_import FILE
  local id uri
  id=$(A "content query --uri content://media/external/downloads --projection _id --where \"_display_name='$1'\"" | grep -oE '_id=[0-9]+' | tail -1 | cut -d= -f2)
  [ -n "$id" ] || { echo "no MediaStore row for $1"; return 1; }
  uri="content://media/external/downloads/$id"
  A "am start -a android.intent.action.SEND -t application/octet-stream --eu android.intent.extra.STREAM $uri --grant-read-uri-permission -n $PKG/.MainActivity" > /dev/null
  waitt text "Import this VPN profile?" 20 || { result IMP FAIL "no import dialog for $1"; shot "import-$1-missing"; return 1; }
  shot "import-$1"
  tapt text "Save"
}

vpn_screen() { A "am start -a android.intent.action.VIEW -d krunk://vpn $PKG" > /dev/null 2>&1; sleep 3; dump; has text "Use the VPN" || {
  tapt desc "Menu" && tapt text "Settings" && tapt text "Privacy & security" --contains; tapt text "VPN"; }; }
vpn_on() { # vpn_on PROFILE_NAME: select and switch on; waits for "Connected"
  vpn_screen; tapt text "Use $1" --contains || tapt desc "Use $1" --contains
  dump; [ "$(echo "$UI" | python3 -I "$U" attr text "Use the VPN" checked --clickable)" = true ] || tapt text "Use the VPN"
  waitt text "Connected" 40 --contains && { shot "vpn-connected-$1"; return 0; }
  shot "vpn-not-connected-$1"; return 1
}
vpn_off() { vpn_screen; dump; [ "$(echo "$UI" | python3 -I "$U" attr text "Use the VPN" checked --clickable)" = true ] && tapt text "Use the VPN"; }

page_has() { # page_has TEXT [seconds]
  waitt text "$1" "${2:-30}" --contains
}

# T1: split profile; inside.test exists only behind the tunnel's DNS; http + https through it.
t1() {
  stop_clean; launch; vpn_on "wg-split" || { result T1 FAIL "VPN did not connect (wg-split)"; return 1; }
  sockwatch_start t1
  view "http://inside.test/" 12
  page_has "client=10.66.0" && result T1 PASS "http://inside.test/ answered from the tunnel (client=10.66.0.x)" || result T1 FAIL "inside.test not loaded through the tunnel"
  shot t1-inside-http
  # The system resolver does not know the name (it exists only in the tunnel's DNS).
  A "toybox nslookup inside.test 2>&1" | grep -qiE "can't|not found|NXDOMAIN|fail|timed" &&
    result T1 PASS "system resolver does not know inside.test" || result T1 INFO "system nslookup inside.test: $(A 'toybox nslookup inside.test 2>&1' | tail -1)"
  sockwatch_stop T1 t1
}

# T1b (https) + T3 with the full-tunnel profile: the sandbox sees tunnel traffic from 10.250.137.2.
t3() {
  stop_clean; launch; vpn_on "wg-full" || { result T3 FAIL "VPN did not connect (wg-full)"; return 1; }
  sockwatch_start t3
  local m; m=$(mark)
  view "http://10.250.137.3:18080/page?t3" 10
  view "http://10.250.137.3:18080/ws.html" 10
  page_has "ws: echo hello" 10 && result T3 PASS "WebSocket echo through the tunnel" || result T3 FAIL "WebSocket echo missing"
  A "nohup toybox nc -s 127.0.0.1 -p 18081 -L toybox nc 10.250.136.3 18080 > /dev/null 2>&1 &"
  view "http://localhost:18081/sw.html" 15
  page_has "sw: fetched" 15 && result T3 PASS "service worker fetch answered" || result T3 FAIL "service worker fetch missing"
  view "http://10.250.137.3:18080/dl.bin" 6; tapt text "Download"; sleep 6
  since "$m" > "$EV/t3-server.txt"
  for path in '/page?t3' '/ws' '/from-sw' '/dl.bin'; do
    grep -F "\"path\": \"$path" "$EV/t3-server.txt" | grep -q '"10.250.137.2"' && result T3 PASS "$path arrived from the tunnel" || result T3 FAIL "$path not seen from the tunnel"
  done
  grep -c '"/dl.bin"' "$EV/t3-server.txt" | grep -qE '^[2-9]' && result T3 PASS "download fetched again by Krunk's downloader through the tunnel" || result T3 INFO "dl.bin requests: $(grep -c '/dl.bin' "$EV/t3-server.txt")"
  grep '"10.250.136.2"' "$EV/t3-server.txt" | grep -v 'localhost:18081' | grep -q . && result T3 FAIL "direct requests seen: $(grep '"10.250.136.2"' "$EV/t3-server.txt" | grep -v localhost | head -2)" || result T3 PASS "no direct request (apart from the localhost page, direct by design)"
  # Private tab: new private tab, type the address.
  tapt desc "Menu" && tapt text "New private tab"; tapt desc "Edit address"; A "input text 10.250.137.3:18080/private-t3"; A input keyevent ENTER; sleep 10
  since "$m" | grep '/private-t3' | grep -q '"10.250.137.2"' && result T3 PASS "private tab through the tunnel" || result T3 FAIL "private tab request not seen from the tunnel"
  # https through the tunnel: a self-signed server reachable only via the tunnel → its warning page.
  view "https://10.250.137.3:18443/" 12; shot t1-https-badcert
  dump; has text "Continue anyway" --contains || has text "certificate" --contains && result T1 PASS "https server behind the tunnel reached (certificate warning shown)" || result T1 FAIL "https through the tunnel: no answer"
  sockwatch_stop T3 t3
}

# T4: Krunk's own requests through the VPN, and nothing direct when it is down. Needs the qa APK
# (release code) built with -Pkrunk.qaSyncBase=http://10.250.137.3:18082
# -Pkrunk.qaUpdateBase=http://10.250.137.3:18080 (addresses reachable only through the tunnel),
# and STAGE_PORT = the staging stack's port on dind (service_stage); the relay logs sources.
t4() {
  [ -n "${STAGE_PORT:-}" ] || { result T4 FAIL "STAGE_PORT not set (service_stage first)"; return 1; }
  local RLOG=$EV/relay-sync.log
  pgrep -f "relay.py tcp 18082" > /dev/null || nohup python3 -I "$HERE/relay.py" tcp 18082 dind "$STAGE_PORT" "$RLOG" > "$EV/relay-sync.out" 2>&1 &
  sleep 1; touch "$RLOG"
  stop_clean; launch; vpn_on "wg-full" || { result T4 FAIL "VPN did not connect"; return 1; }
  sockwatch_start t4
  local m r0; m=$(mark); r0=$(wc -l < "$RLOG")
  # Favicon: Krunk fetches the icon of a page it shows.
  view "http://10.250.137.3:18080/favpage" 12
  since "$m" | grep '"/fav.png"' | grep -q '"10.250.137.2"' && result T4 PASS "favicon fetched through the tunnel ($(since "$m" | grep -c '/fav.png') requests)" ||
    result T4 FAIL "favicon not seen from the tunnel"
  since "$m" | grep '"/fav.png"' | grep -q '"10.250.136.2"' && result T4 FAIL "favicon fetched direct"
  # Search suggestions (public engine: only path is the tunnel; proof = they appear + sockwatch).
  tapt desc "Edit address"; A "input text krunkvpn"; sleep 6; dump; shot t4-suggestions-up
  [ "$(echo "$UI" | grep -o 'text="krunkvpn[^"]*"' | sort -u | wc -l)" -ge 2 ] && result T4 PASS "search suggestions arrive while connected" || result T4 INFO "no engine suggestions visible (setting off or engine silent)"
  A input keyevent KEYCODE_BACK; A input keyevent KEYCODE_BACK
  # Update check (qaUpdateBase = the test server through the tunnel).
  A "am start -a android.intent.action.VIEW -d krunk://about $PKG" > /dev/null 2>&1; sleep 3
  tapt text "Check for updates" --contains; sleep 6; shot t4-update-check-up
  since "$m" | grep -E '"/updates/' | grep -q '"10.250.137.2"' && result T4 PASS "update check through the tunnel" || result T4 FAIL "update check not seen from the tunnel"
  # Sync: "Sign in" makes the first request (link start) to the staging stack.
  A "am start -a android.intent.action.VIEW -d krunk://sync $PKG" > /dev/null 2>&1; sleep 3
  tapt text "Sign in"; waitt text "Approve this device" 20 --contains && result T4 PASS "staging answered sync's first request" || result T4 FAIL "no answer from staging"
  shot t4-sync-up
  tail -n +"$((r0 + 1))" "$RLOG" | grep -q '"10.250.137.2"' && result T4 PASS "sync reached staging through the tunnel" || result T4 FAIL "sync connection not seen from the tunnel"
  tapt text "Cancel"
  # Server down: every one of them fails, nothing direct.
  $TB stop wg > /dev/null 2>&1; sleep 3
  m=$(mark); r0=$(wc -l < "$RLOG")
  A "am start -a android.intent.action.VIEW -d krunk://about $PKG" > /dev/null 2>&1; sleep 3
  tapt text "Check for updates" --contains; sleep 25; shot t4-update-check-down
  A "am start -a android.intent.action.VIEW -d krunk://sync $PKG" > /dev/null 2>&1; sleep 3
  tapt text "Sign in"; sleep 25; shot t4-sync-down
  waitt text "Approve this device" 2 --contains && result T4 FAIL "sync answered while the VPN server is down"
  tapt desc "Edit address"; A "input text krunkdown"; sleep 8; dump; shot t4-suggestions-down
  [ "$(echo "$UI" | grep -o 'text="krunkdown[^"]*"' | sort -u | wc -l)" -le 1 ] && result T4 PASS "no engine suggestions while the server is down" || result T4 FAIL "suggestions arrived while the server is down"
  A input keyevent KEYCODE_BACK; A input keyevent KEYCODE_BACK
  [ "$(since "$m" | wc -l)" = 0 ] && result T4 PASS "test server saw nothing while down (update check failed)" || result T4 FAIL "requests arrived while down: $(since "$m" | head -2)"
  [ "$(tail -n +"$((r0 + 1))" "$RLOG" | wc -l)" = 0 ] && result T4 PASS "staging relay saw nothing while down (sync failed)" || result T4 FAIL "sync connections while down"
  sockwatch_stop T4 t4
  $TB start wg > /dev/null 2>&1
}

# M6: OpenVPN on Android — TCP and UDP through the tunnel, a wrong password, another CA.
# Testbed user krunk / krunk-test; OpenVPN pushes the inside network (10.77.0.0/24, DNS .53).
ovpn_creds() { # ovpn_creds USER PASS: fill the sign-in dialog that follows the import
  waitt text "Sign in to" 10 --contains || { result M6 FAIL "no sign-in dialog"; return 1; }
  shot "ovpn-credentials-dialog"
  tapt text "User name"; A "input text $1"; tapt text "Password"; A "input text $2"
  tapt text "Save"
}
ovpn() {
  stop_clean; launch
  share_import ovpn-tcp.ovpn && ovpn_creds krunk krunk-test
  share_import ovpn-udp.ovpn && ovpn_creds krunk krunk-test
  share_import ovpn-wrongpw.ovpn && ovpn_creds krunk wrong-password
  share_import ovpn-badca.ovpn && ovpn_creds krunk krunk-test
  sockwatch_start ovpn
  local proto net
  for proto in tcp:10.89.0 udp:10.88.0; do
    net=${proto#*:}; proto=${proto%%:*}
    if vpn_on "ovpn-$proto"; then
      view "http://inside.test/?ovpn-$proto" 12
      page_has "client=$net" && result M6 PASS "OpenVPN $proto: inside.test answered from $net.x" || result M6 FAIL "OpenVPN $proto: inside.test not loaded"
      shot "ovpn-$proto-inside"
    else result M6 FAIL "OpenVPN $proto did not connect"; fi
  done
  # Fail-closed with OpenVPN (UDP): the server stops → VPN not connected, nothing direct.
  local m; m=$(mark)
  $TB stop ovpn > /dev/null 2>&1
  view "http://10.250.136.3:18080/ovpn-down-bait?r=$RANDOM" 2
  page_has "VPN not connected" 60 && result M6 PASS "OpenVPN server down: VPN not connected page" || result M6 FAIL "OpenVPN server down: no VPN error page"
  shot ovpn-down
  since "$m" | grep -q ovpn-down-bait && result M6 FAIL "request arrived direct while OpenVPN was down" || result M6 PASS "nothing direct while OpenVPN was down"
  $TB start ovpn > /dev/null 2>&1; sleep 8
  vpn_on "ovpn-wrongpw"; dump; shot ovpn-wrong-password
  has text "Failed" --contains && result M6 PASS "wrong password: $(echo "$UI" | grep -o 'Failed: [^"]*' | head -1)" || result M6 FAIL "wrong password: no failure shown"
  vpn_on "ovpn-badca"; dump; shot ovpn-bad-ca
  has text "Failed" --contains && result M6 PASS "other CA: $(echo "$UI" | grep -o 'Failed: [^"]*' | head -1)" || result M6 FAIL "other CA: no failure shown"
  sockwatch_stop M6 ovpn
  vpn_off
}

# T5: server stopped → VPN not connected page within ~20 s, nothing direct; WebRTC gets nothing.
t5() {
  stop_clean; launch; vpn_on "wg-full" || { result T5 FAIL "VPN did not connect"; return 1; }
  local m u0; m=$(mark); u0=$(grep -c udp_from "$SLOG")
  view "http://10.250.137.3:18080/rtc.html?t5" 14
  page_has "rtc: 0 candidates" 6 && result T5 PASS "WebRTC: 0 ICE candidates while the VPN is on" || { dump; result T5 FAIL "WebRTC page: $(echo "$UI" | grep -o 'rtc: [^"]*' | head -1)"; }
  [ "$(grep -c udp_from "$SLOG")" = "$u0" ] && result T5 PASS "no UDP from the emulator at the STUN sink" || result T5 FAIL "UDP seen at the STUN sink"
  sockwatch_start t5
  $TB stop wg > /dev/null 2>&1
  local t0=$SECONDS
  view "http://10.250.137.3:18080/t5-after-stop?r=$RANDOM" 2
  if page_has "VPN not connected" 40; then result T5 PASS "VPN not connected page after $((SECONDS - t0)) s"; else result T5 FAIL "no VPN error page within 40 s"; fi
  shot t5-not-connected
  view "http://10.250.136.3:18080/t5-direct-bait?r=$RANDOM" 15
  since "$m" | grep -E 't5-after-stop|t5-direct-bait' | grep -q . && result T5 FAIL "a request arrived while the server was down" || result T5 PASS "the sandbox saw no request while the server was down"
  sockwatch_stop T5 t5
  $TB start wg > /dev/null 2>&1
}

# T6: restart with the VPN off → direct, no proxy prefs; restart with connect-at-start → the
# restored tab waits and loads through the tunnel.
t6() {
  vpn_off; local m; m=$(mark)
  view "http://10.250.136.3:18080/t6-off" 10; stop_clean
  R "grep -E 'network\\.proxy|peerconnection|krunk\\.vpn' /data/data/$PKG/files/mozilla/*/prefs.js" > "$EV/t6-prefs.txt"
  [ -s "$EV/t6-prefs.txt" ] && result T6 FAIL "prefs.js holds: $(head -2 "$EV/t6-prefs.txt")" || result T6 PASS "prefs.js has no proxy/VPN prefs"
  launch 10; view "http://10.250.136.3:18080/t6-off-again" 10
  since "$m" | grep '/t6-off-again' | grep -q '"10.250.136.2"' && result T6 PASS "VPN off after restart: direct" || result T6 FAIL "VPN off after restart: page not loaded direct"
  # Connect at start: on, the option on, a tunnel-only page as the current tab, restart.
  vpn_on "wg-full"; vpn_screen; dump
  [ "$(echo "$UI" | python3 -I "$U" attr text "Connect when Krunk starts" checked --clickable)" = true ] || tapt text "Connect when Krunk starts"
  view "http://10.250.137.3:18080/t6-restore" 10; stop_clean; m=$(mark)
  sockwatch_start t6; launch 25
  since "$m" | grep '/t6-restore' | grep -q '"10.250.137.2"' && result T6 PASS "restored tab loaded through the tunnel after start" || result T6 FAIL "restored tab not seen from the tunnel"
  sockwatch_stop T6 t6
}

# T8: Wi-Fi ↔ mobile: the tunnel reconnects within a few seconds.
t8() {
  vpn_on "wg-full" || { result T8 FAIL "VPN did not connect"; return 1; }
  local m t0; A svc wifi disable; sleep 3; t0=$SECONDS; m=$(mark)
  view "http://10.250.137.3:18080/t8-mobile?r=$RANDOM" 2
  for ((i = 0; i < 30; i++)); do since "$m" | grep -q t8-mobile && break; sleep 1; done
  since "$m" | grep -q t8-mobile && result T8 PASS "after Wi-Fi off: page through the tunnel in $((SECONDS - t0)) s" || result T8 FAIL "after Wi-Fi off: no page in 30 s"
  A svc wifi enable; sleep 3; t0=$SECONDS; m=$(mark)
  view "http://10.250.137.3:18080/t8-wifi?r=$RANDOM" 2
  for ((i = 0; i < 30; i++)); do since "$m" | grep -q t8-wifi && break; sleep 1; done
  since "$m" | grep -q t8-wifi && result T8 PASS "after Wi-Fi on: page through the tunnel in $((SECONDS - t0)) s" || result T8 FAIL "after Wi-Fi on: no page in 30 s"
}

[ $# -gt 0 ] || { sed -n '2,20p' "$0"; exit 2; }
for step in "$@"; do "$step"; done
