# Helpers for the Android device checks (a3.sh). Source from bash.
#   KRUNK_A3_ROOT   where the run's files go: server.log and, by default, the evidence (EV)
#                   (default android/build/a3, git-ignored)
#   EV              evidence folder of this run (logs, captures, screenshots)
#   KRUNK_TEST_HOST the address of the machine running server.py as the device sees it
LIB=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SRC=$(cd "$LIB/../../.." && pwd)
ROOT=${KRUNK_A3_ROOT:-$SRC/android/build/a3}
mkdir -p "$ROOT"; touch "$ROOT/server.log"
E=$SRC/android/scripts/emu.sh
EV=${EV:-$ROOT/run}
B="http://${KRUNK_TEST_HOST:-10.250.136.3}:18080"
PKG=org.krunk.browser
DEBUG_APK=$SRC/android/app/build/outputs/apk/direct/debug/app-direct-x86_64-debug.apk
RELEASE_APK=$SRC/android/app/build/outputs/apk/direct/release/app-direct-x86_64-release.apk
T() { timeout "${KRUNK_ADB_TIMEOUT:-45}" "$@"; }
A() { T "$E" adb shell "$@"; }
# itext TEXT: `input text` on the device. KRUNK_SLOW_TYPE=1 sends one character per call (the
# software-emulated API 29 image drops characters of a burst); the timeout grows with the length.
itext() {
  [ "${KRUNK_SLOW_TYPE:-0}" = 1 ] || { A "input text '$1'"; return; }
  local s=$1 i c cmd=""
  for ((i = 0; i < ${#s}; i++)); do
    c=${s:i:1}
    case $c in ' ') cmd+="input text %s; " ;; "'") cmd+="input text \"'\"; " ;; *) cmd+="input text '$c'; " ;; esac
  done
  timeout $(( ${#s} * 12 + 30 )) "$E" adb shell "$cmd"
}
shot() { KRUNK_SHOTS=$EV T "$E" shot "$1" > /dev/null || { echo "NO SCREENSHOT $1 (display not answering)"; return 4; }; }
fg() { A "dumpsys activity activities" 2> /dev/null | grep -m1 -E 'topResumedActivity|mResumedActivity' | grep -q "$PKG" || { echo "NOT IN FOREGROUND ($1)"; return 9; }; }
launch() { A "am start -W -n $PKG/.MainActivity" > /dev/null; sleep "${1:-6}"; }
view() { A "am start -W -a android.intent.action.VIEW -d '$1' $PKG" > /dev/null; sleep "${2:-10}"; }
stop_clean() { A "input keyevent KEYCODE_HOME"; sleep 3; A "am force-stop $PKG"; }
tap() { A "input tap $1 $2"; sleep "${3:-2}"; }
uid() { A "pm list packages -U $PKG" | grep -oE "uid:[0-9]+" | head -1 | cut -d: -f2; }
fresh() { # fresh <apk>: remove the app and install the given APK
  A "input keyevent KEYCODE_HOME"; sleep 1; A "am force-stop $PKG"; T "$E" uninstall > /dev/null 2>&1
  T "$E" adb install "$1" 2>&1 | tail -1
}
klog() { T "$E" adb logcat -d -v time -s Krunk:V KrunkGuard:V; }
# Per-UID capture: every packet the app's UID sends is copied to NFLOG group 30 and written by
# tcpdump on the device (root). cap_start <name>, cap_stop <name> -> $EV/<name>.pcap and
# $EV/<name>-pcap-summary.txt (t0 = the time cap_start ran).
cap_start() {
  local u; u=$(uid); [ -n "$u" ] || { echo "no uid"; return 1; }
  A "su 0 sh -c 'pkill tcpdump; iptables -D OUTPUT -m owner --uid-owner $u -j NFLOG --nflog-group 30 2>/dev/null; ip6tables -D OUTPUT -m owner --uid-owner $u -j NFLOG --nflog-group 30 2>/dev/null; iptables -I OUTPUT 1 -m owner --uid-owner $u -j NFLOG --nflog-group 30 && ip6tables -I OUTPUT 1 -m owner --uid-owner $u -j NFLOG --nflog-group 30; rm -f /data/local/tmp/$1.pcap; nohup tcpdump -i nflog:30 -U -w /data/local/tmp/$1.pcap >/data/local/tmp/$1.tcpdump.log 2>&1 &'"
  sleep 1; date -u +%T > "$EV/$1-t0.txt"
}
cap_stop() {
  local u; u=$(uid)
  A "su 0 sh -c 'pkill -INT tcpdump; sleep 1; iptables -D OUTPUT -m owner --uid-owner $u -j NFLOG --nflog-group 30; ip6tables -D OUTPUT -m owner --uid-owner $u -j NFLOG --nflog-group 30; chmod 644 /data/local/tmp/$1.pcap'" > /dev/null 2>&1
  T "$E" adb pull "/data/local/tmp/$1.pcap" "$EV/$1.pcap" > /dev/null 2>&1 && A "su 0 rm -f /data/local/tmp/$1.pcap /data/local/tmp/$1.tcpdump.log"
  python3 -I "$LIB/pcap_summary.py" "$EV/$1.pcap" --t0 "$(cat "$EV/$1-t0.txt")" > "$EV/$1-pcap-summary.txt"
  head -1 "$EV/$1-pcap-summary.txt" | grep -oE '[0-9]+ packets sent by the UID'
}
