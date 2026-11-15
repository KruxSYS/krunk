# Helpers for the Android 0.6.0 sync checks against a staging stack (source from bash).
#   EV  evidence folder (default .pipeline/evidence-android-0.6.0/run)
# Builds on a3/lib.sh (adb, per-UID capture). The staging stack is reached through
# .pipeline/evidence-android-0.6.0/tools/relay.py on 0.0.0.0:18082 (10.250.136.3:18082 on the device).
HERE7=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
EV=${EV:-$(cd "$HERE7/../../../.." && pwd)/.pipeline/evidence-android-0.6.0/run}
mkdir -p "$EV"
# shellcheck disable=SC1091
source "$HERE7/../a3/lib.sh"
U="$HERE7/../a3/uix.py"
dump() { A "uiautomator dump /sdcard/ui.xml" > /dev/null 2>&1; UI=$(A "cat /sdcard/ui.xml"); }
has() { echo "$UI" | python3 -I "$U" has "$@"; }
nodes() { dump; echo "$UI" | python3 -I "$U" list; }
tapt() { # tapt text|desc VALUE [wait] [--contains]
  dump; local xy; xy=$(echo "$UI" | python3 -I "$U" find "$1" "$2" ${4:-}) || { echo "MISSING $2"; return 1; }
  # shellcheck disable=SC2086
  tap $xy "${3:-2}"
}
waitt() { # waitt text|desc VALUE seconds [--contains]
  local i; for ((i = 0; i < $3; i += 2)); do dump; has "$1" "$2" ${4:-} && return 0; sleep 2; done; echo "TIMEOUT $2"; return 1
}
synclog() { T "$E" adb logcat -d -v time -s KrunkSyncNet:V KrunkSync:V; }
openSync() { launch 8; A "am start -W -a android.intent.action.VIEW -d 'krunk://sync' $PKG" > /dev/null 2>&1; sleep 3; }
