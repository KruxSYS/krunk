#!/usr/bin/env bash
# Krunk for Android 0.10.0 acceptance pass for the in-browser VPN (plan §8, T1–T8) on emulator-5554.
#
#   android/scripts/a9/a9.sh up | down          test servers + relays on this sandbox, VPN testbed on dind
#   android/scripts/a9/a9.sh profiles           device copies of the testbed profiles -> /sdcard/Download
#   android/scripts/a9/a9.sh <step>...          t1 t2 t3 t4 t5 t6 t7 t8 leakgrep (see each function)
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
  sleep 1
  pgrep -af "a9/(testserver|relay)" | grep -v pgrep
}

down() {
  for p in $(pgrep -f "a9/(testserver|relay).py"); do kill "$p"; done
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

[ $# -gt 0 ] || { sed -n '2,20p' "$0"; exit 2; }
for step in "$@"; do "$step"; done
