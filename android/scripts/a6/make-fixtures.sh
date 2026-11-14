#!/usr/bin/env bash
# The tampered update trees for a6.sh (9c, 9d), made from a good release tree written by
#   dist/server/publish-android.sh 0.5.0 --dest GOOD
#   make-fixtures.sh GOOD OUT
# OUT/onebyte    the manifest with one byte changed, the original signature
# OUT/otherkey   the manifest signed with a throwaway Ed25519 key made here (test only, deleted)
# OUT/apkbyte    the manifest as published, one byte of every APK changed
# OUT/othercert  the x86_64 APK re-signed with the Android debug key (~/.android/debug.keystore)
#                and a manifest naming its correct size and hash, signed with the real update key
#                (so only the certificate check can refuse it). Kept under .pipeline, never
#                published, never committed.
set -euo pipefail
good=$1 out=$2
src=$(cd "$(dirname "$0")/../../.." && pwd)
# shellcheck disable=SC1091
source "$src/android/toolchain.lock"
sdk="${ANDROID_HOME:-/opt/android-sdk}"
apksigner="$sdk/build-tools/$BUILD_TOOLS/apksigner"
key="$HOME/.config/krunk/update-signing.key"
m=updates/android-arm64/latest.json
rm -rf "$out"; mkdir -p "$out"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

copy() { mkdir -p "$out/$1"; cp -a "$good/." "$out/$1/"; }
sign() { openssl pkeyutl -sign -rawin -inkey "$1" -in "$2" | base64 -w0 > "$2.sig"; }

copy onebyte
python3 -I - "$out/onebyte/$m" <<'PY'
import sys
p = sys.argv[1]; b = bytearray(open(p, "rb").read())
i = b.index(b'"version_code":') + len(b'"version_code":')
b[i] = ord("8") if b[i] != ord("8") else ord("9")
open(p, "wb").write(bytes(b))
PY

copy otherkey
openssl genpkey -algorithm ed25519 -out "$tmp/other.key" 2>/dev/null
sign "$tmp/other.key" "$out/otherkey/$m"

copy apkbyte
for f in "$out"/apkbyte/download/android/*/*.apk; do
  python3 -I -c 'import os,sys; p=sys.argv[1]; n=os.path.getsize(p); f=open(p,"r+b"); f.seek(n//2); c=f.read(1); f.seek(n//2); f.write(bytes([c[0]^1]))' "$f"
done

copy othercert
apk=$(ls "$out"/othercert/download/android/*/*-x86_64-release.apk)
[ -f "$HOME/.android/debug.keystore" ] || { echo "no ~/.android/debug.keystore" >&2; exit 1; }
"$apksigner" sign --ks "$HOME/.android/debug.keystore" --ks-pass pass:android --ks-key-alias androiddebugkey \
  --key-pass pass:android --out "$tmp/resigned.apk" "$apk"
cp "$tmp/resigned.apk" "$apk"
python3 -I - "$out/othercert/$m" "$apk" <<'PY'
import hashlib, json, os, sys
p, apk = sys.argv[1], sys.argv[2]
m = json.load(open(p))
for f in m["files"]:
    if f["abi"] == "x86_64":
        f["size"] = os.path.getsize(apk)
        f["sha256"] = hashlib.sha256(open(apk, "rb").read()).hexdigest()
open(p, "w").write(json.dumps(m, separators=(",", ":"), ensure_ascii=False) + "\n")
PY
sign "$key" "$out/othercert/$m"
echo "fixtures in $out: onebyte otherkey apkbyte othercert"
