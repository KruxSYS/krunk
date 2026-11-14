#!/usr/bin/env bash
# Publishes a Krunk for Android release (signed APKs) to the download server's release store.
#
#   dist/server/publish-android.sh VERSION (--dest DIR | --volume NAME) [--apks DIR]
#       [--changelog FILE | --notes FILE] [--released YYYY-MM-DD] [--cert-sha256 HEX] [--key PATH]
#       [--force]
#
#   VERSION       the Android app version, e.g. 0.1.0 (android/app/build.gradle.kts versionName)
#   --apks DIR    where the APKs are (default dist/android):
#                 Krunk-android-<VERSION>-{arm64-v8a,universal,x86_64}-release.apk
#   --dest DIR    a plain folder (mounted into the container as /srv/releases)
#   --volume NAME a Docker named volume, written through a throwaway pinned alpine container
#   --changelog FILE  release notes source (default CHANGELOG-android.md): its "## VERSION — DATE"
#                 section becomes NOTES.md and DATE the release date
#   --notes FILE  use this Markdown file as NOTES.md instead
#   --released    the release date (default: the changelog section's date, else today, UTC)
#   --cert-sha256 the expected signing certificate fingerprint; it must equal the release
#                 keystore's (~/.config/krunk/android-release.properties, or KRUNK_ANDROID_SIGNING),
#                 which every APK must carry
#   --key PATH    the Ed25519 update key (default ~/.config/krunk/update-signing.key): signs the
#                 in-app update manifest; its public half must be assets/keys/update-signing.pub,
#                 the key built into the app
#   --force       publish VERSION again although it is already in the store (its files are
#                 replaced; the edge cache may serve the old ones for a while)
#
# The APKs are signed by Gradle with Krunk's Android release key (never in a container or the
# image); this script only checks each APK before it copies anything: signature and certificate
# (apksigner), and that it really is VERSION of the app and not debuggable (aapt2). Since 0.5.0
# it also writes the in-app update manifest (channel android-arm64) and signs it on this machine
# with the update key (the Windows scheme and key), checks the signature, and after copying reads
# the files back from the destination and compares their SHA-256 with the manifest. It refuses to
# start without either key: the APK keystore (for the certificate) and the update key.
#
# Layout written, in this order:
#   download/android/<VERSION>/Krunk-android-<VERSION>-<abi>-release.apk, NOTES.md, SHA256SUMS
#   download/android.json                       (the website's Android download box)
#   updates/android-arm64/latest.json.sig, then latest.json   (the app's update check)
# The Windows files (download/release.json, updates/win-x64/) are never touched.
set -euo pipefail
ALPINE="alpine:3.22@sha256:2e0b81d60597f449baa56dc25d355367869d2dca4cd57e866f1e2b52af3eb55f"
ABIS="arm64-v8a universal x86_64"

usage() { sed -n '2,38p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
[ $# -ge 1 ] || usage
version=$1; shift
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dest="" volume="" apks="$root/dist/android" notes="" released="" cert="" force=0
key="$HOME/.config/krunk/update-signing.key"
pubkey="$root/assets/keys/update-signing.pub"
changelog="$root/CHANGELOG-android.md"
while [ $# -gt 0 ]; do
  case "$1" in
    --dest) dest=$2; shift 2 ;;
    --volume) volume=$2; shift 2 ;;
    --apks) apks=$2; shift 2 ;;
    --notes) notes=$2; shift 2 ;;
    --changelog) changelog=$2; shift 2 ;;
    --released) released=$2; shift 2 ;;
    --cert-sha256) cert=$2; shift 2 ;;
    --key) key=$2; shift 2 ;;
    --force) force=1; shift ;;
    *) usage ;;
  esac
done
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'
[[ $version =~ $semver ]] || { echo "not a semver version: $version" >&2; exit 2; }
[[ -z $released || $released =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || { echo "--released must be YYYY-MM-DD" >&2; exit 2; }
{ [ -n "$dest" ] && [ -z "$volume" ]; } || { [ -z "$dest" ] && [ -n "$volume" ]; } ||
  { echo "give exactly one of --dest DIR or --volume NAME" >&2; exit 2; }
[[ -z $volume || $volume =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { echo "bad volume name: $volume" >&2; exit 2; }

# apksigner from the pinned build-tools (android/toolchain.lock).
# shellcheck disable=SC1091
source "$root/android/toolchain.lock"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/android-sdk}}"
apksigner="$sdk/build-tools/$BUILD_TOOLS/apksigner"
aapt2="$sdk/build-tools/$BUILD_TOOLS/aapt2"
[ -x "$apksigner" ] || { echo "apksigner not found: $apksigner (android/scripts/setup-sdk.sh)" >&2; exit 1; }
[ -x "$aapt2" ] || { echo "aapt2 not found: $aapt2 (android/scripts/setup-sdk.sh)" >&2; exit 1; }

# Both keys must be here before anything is read or copied (0.5.0).
# 1. The APK release keystore: the certificate every APK must carry.
props="${KRUNK_ANDROID_SIGNING:-$HOME/.config/krunk/android-release.properties}"
[ -f "$props" ] || { echo "refusing: no Android release keystore properties at $props" >&2; exit 1; }
store=$(sed -n 's/^storeFile=//p' "$props"); pass=$(sed -n 's/^storePassword=//p' "$props"); alias=$(sed -n 's/^keyAlias=//p' "$props")
[ -f "$store" ] || { echo "refusing: the Android release keystore is missing: $store" >&2; exit 1; }
# The password goes through the environment, not the command line (visible in the process list).
store_cert=$(KRUNK_STOREPASS="$pass" keytool -list -v -keystore "$store" -storepass:env KRUNK_STOREPASS -alias "$alias" 2>/dev/null | sed -n 's/^[[:space:]]*SHA256: //p' | head -1 | tr -d ':' | tr 'A-F' 'a-f')
unset pass
[[ $store_cert =~ ^[0-9a-f]{64}$ ]] || { echo "could not read the certificate fingerprint from $store" >&2; exit 1; }
if [ -n "$cert" ]; then
  cert=$(printf '%s' "$cert" | tr -d ': ' | tr 'A-F' 'a-f')
  [ "$cert" = "$store_cert" ] || { echo "--cert-sha256 $cert is not the keystore's certificate $store_cert" >&2; exit 2; }
fi
cert=$store_cert
# 2. The update key: it signs the manifest the app trusts, so it must be the key built into it.
[ -f "$key" ] || { echo "refusing: the update signing key is missing: $key" >&2; exit 1; }
[ -f "$pubkey" ] || { echo "missing $pubkey" >&2; exit 1; }
command -v openssl >/dev/null || { echo "openssl is required" >&2; exit 1; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
openssl pkey -in "$key" -pubout -out "$work/pub.pem" 2>/dev/null || { echo "cannot read the update key $key" >&2; exit 1; }
cmp -s "$work/pub.pem" "$pubkey" ||
  { echo "refusing: $key is not the key the app trusts ($pubkey)" >&2; exit 1; }
if [ -z "$notes" ]; then
  [ -f "$changelog" ] || { echo "changelog not found: $changelog (or give --notes FILE)" >&2; exit 1; }
  section=$(python3 -I "$here/changelog.py" section "$changelog" "$version") ||
    { echo "no release notes for $version: add or fix its section in $changelog" >&2; exit 1; }
  printf '# Krunk for Android %s\n\n%s\n' "$version" "$section" > "$work/NOTES.md"
  notes="$work/NOTES.md"
  if [ -z "$released" ]; then
    released=$(python3 -I "$here/changelog.py" date "$changelog" "$version")
    echo "release date $released (from $changelog)"
  fi
fi
[ -f "$notes" ] || { echo "missing notes file: $notes" >&2; exit 1; }
[ -n "$released" ] || released=$(date -u +%Y-%m-%d)

stage="$work/releases"
rel="$stage/download/android/$version"
mkdir -p "$rel"
files_json=""
version_code=""
for abi in $ABIS; do
  name="Krunk-android-$version-$abi-release.apk"
  src="$apks/$name"
  [ -f "$src" ] || { echo "missing: $src" >&2; exit 1; }
  # Signed with the release key, v2 or v3 scheme, and the expected certificate.
  out=$("$apksigner" verify --print-certs -v "$src" 2>&1) || { echo "$name: signature check failed:"; echo "$out"; exit 1; } >&2
  # Here-strings, not pipes: grep -q closing early would fail the pipeline under pipefail.
  grep -qE 'Verified using v[23] scheme \(APK Signature Scheme v[23]\): true' <<<"$out" ||
    { echo "$name: not signed with APK Signature Scheme v2/v3" >&2; exit 1; }
  grep -qx 'Number of signers: 1' <<<"$out" || { echo "$name: expected exactly one signer" >&2; exit 1; }
  # apksigner prints one line per scheme ("Signer #1 …" or "V3.0 Signer: …"): all must match.
  got=$(sed -n 's/^.*[Ss]igner[^:]*:\{0,1\} \{0,1\}certificate SHA-256 digest: //p' <<<"$out" | tr 'A-F' 'a-f' | sort -u)
  [ "$got" = "$cert" ] || { echo "$name: signed with certificate ${got:-none}, expected $cert" >&2; exit 1; }
  # The file must be what its name says: this app, this version, a release build.
  badging=$("$aapt2" dump badging "$src" 2>/dev/null) || { echo "$name: aapt2 cannot read it" >&2; exit 1; }
  pkg=$(sed -n "s/^package: name='\([^']*\)'.*/\1/p" <<<"$badging")
  vname=$(sed -n "s/^package: .* versionName='\([^']*\)'.*/\1/p" <<<"$badging")
  [ "$pkg" = "$APPLICATION_ID" ] || { echo "$name: package is ${pkg:-unknown}, expected $APPLICATION_ID" >&2; exit 1; }
  [ "$vname" = "$version" ] || { echo "$name: versionName is ${vname:-unknown}, expected $version" >&2; exit 1; }
  if grep -q '^application-debuggable' <<<"$badging"; then echo "$name: debuggable build" >&2; exit 1; fi
  vcode=$(sed -n "s/^package: .* versionCode='\([0-9]*\)'.*/\1/p" <<<"$badging")
  [ -n "$vcode" ] || { echo "$name: no versionCode" >&2; exit 1; }
  [ -z "$version_code" ] || [ "$vcode" = "$version_code" ] ||
    { echo "$name: versionCode $vcode differs from the other APKs ($version_code)" >&2; exit 1; }
  version_code=$vcode
  # The website APK has the update check: its installer permission must be there.
  grep -q "^uses-permission: name='android.permission.REQUEST_INSTALL_PACKAGES'" <<<"$badging" ||
    { echo "$name: not the direct flavour (no REQUEST_INSTALL_PACKAGES)" >&2; exit 1; }
  cp "$src" "$rel/$name"
  sha=$(sha256sum "$rel/$name" | cut -d' ' -f1)
  size=$(stat -c %s "$rel/$name")
  kind="device"; label="arm64-v8a APK"
  case "$abi" in
    universal) kind="universal"; label="Universal APK" ;;
    x86_64) kind="emulator"; label="x86_64 APK (emulator)" ;;
  esac
  files_json="$files_json${files_json:+,}
    {\"name\": \"$name\", \"abi\": \"$abi\", \"kind\": \"$kind\", \"label\": \"$label\", \"url\": \"/download/android/$version/$name\", \"size\": $size, \"sha256\": \"$sha\"}"
done
cp "$notes" "$rel/NOTES.md"
(cd "$rel" && sha256sum ./*.apk | sed 's#\./##' > SHA256SUMS)

cat > "$stage/download/android.json" <<EOF
{
  "version": "$version",
  "released": "$released",
  "notes": "/download/android/$version/NOTES.md",
  "package": "$APPLICATION_ID",
  "min_android": "8.0",
  "min_sdk": $MIN_SDK,
  "signing_cert_sha256": "$cert",
  "files": [$files_json
  ]
}
EOF
python3 -I -c 'import json,sys; json.load(open(sys.argv[1]))' "$stage/download/android.json"

# The in-app update manifest (core/src/update_android.rs): compact, one line, signed byte for
# byte with the update key, and checked with its public half as the app will.
mkdir -p "$stage/updates/android-arm64"
manifest="$stage/updates/android-arm64/latest.json"
python3 -I - "$stage/download/android.json" "$notes" "$version_code" "$MIN_SDK" > "$manifest" <<'PY'
import json, sys
site = json.load(open(sys.argv[1]))
notes = open(sys.argv[2], encoding="utf-8").read()
# NOTES.md starts with its own title line; the app shows the changes only.
lines = notes.splitlines()
if lines and lines[0].startswith("# "):
    lines = lines[1:]
notes = "\n".join(lines).strip()
if len(notes) > 32 * 1024:
    sys.exit("the release notes are longer than the app accepts (32 KiB)")
m = {
    "channel": "android-arm64",
    "version": site["version"],
    "version_code": int(sys.argv[3]),
    "released": site["released"],
    "package": site["package"],
    "min_sdk": int(sys.argv[4]),
    "signing_cert_sha256": site["signing_cert_sha256"],
    "notes": notes,
    "notes_url": site["notes"],
    "files": [{"abi": f["abi"], "url": f["url"], "size": f["size"], "sha256": f["sha256"]} for f in site["files"]],
}
print(json.dumps(m, separators=(",", ":"), ensure_ascii=False))
PY
[ "$(stat -c %s "$manifest")" -le 65536 ] || { echo "the update manifest is larger than 64 KiB" >&2; exit 1; }
openssl pkeyutl -sign -rawin -inkey "$key" -in "$manifest" | base64 -w0 > "$manifest.sig"
base64 -d "$manifest.sig" > "$work/sig.bin"
openssl pkeyutl -verify -pubin -inkey "$pubkey" -rawin -in "$manifest" -sigfile "$work/sig.bin" >/dev/null ||
  { echo "the new update manifest signature does not verify" >&2; exit 1; }
echo "update manifest signed and verified (versionCode $version_code)"
# What the destination must hold afterwards: path and SHA-256 of every file written.
(cd "$stage" && find download/android/"$version" download/android.json updates/android-arm64 -type f | LC_ALL=C sort |
  xargs sha256sum) > "$work/expect.sha256"
chmod -R a+rX "$stage"

if [ -n "$dest" ]; then
  if [ -e "$dest/download/android/$version" ] && [ "$force" != 1 ]; then
    echo "$version is already published in $dest (use --force to replace it)" >&2; exit 3
  fi
  mkdir -p "$dest/download/android/$version"
  cp -f "$rel"/* "$dest/download/android/$version/"
  cp -f "$stage/download/android.json" "$dest/download/.android.json.tmp"
  mv -f "$dest/download/.android.json.tmp" "$dest/download/android.json"
  mkdir -p "$dest/updates/android-arm64"
  cp -f "$manifest.sig" "$dest/updates/android-arm64/.latest.json.sig.tmp"
  cp -f "$manifest" "$dest/updates/android-arm64/.latest.json.tmp"
  mv -f "$dest/updates/android-arm64/.latest.json.sig.tmp" "$dest/updates/android-arm64/latest.json.sig"
  mv -f "$dest/updates/android-arm64/.latest.json.tmp" "$dest/updates/android-arm64/latest.json"
  (cd "$dest" && sha256sum -c --quiet "$work/expect.sha256") ||
    { echo "the files in $dest do not match what was published" >&2; exit 1; }
  where=$dest
else
  command -v docker >/dev/null || { echo "docker is required for --volume" >&2; exit 1; }
  # The volume must exist: docker would create an empty one for a mistyped name and "publish"
  # into it without anyone noticing.
  docker volume inspect "$volume" > /dev/null 2>&1 || { echo "no such Docker volume: $volume" >&2; exit 1; }
  tar -C "$stage" -cf - . | docker run --rm -i --network none -e FORCE="$force" -v "$volume:/dest" "$ALPINE" sh -eu -c '
    mkdir -p /in && tar -C /in -xf -
    v=$(ls /in/download/android)
    if [ -e "/dest/download/android/$v" ] && [ "$FORCE" != 1 ]; then
      echo "$v is already published in this volume (use --force to replace it)" >&2; exit 3
    fi
    mkdir -p "/dest/download/android/$v"
    cp -f /in/download/android/"$v"/* "/dest/download/android/$v/"
    cp -f /in/download/android.json /dest/download/.android.json.tmp
    mv -f /dest/download/.android.json.tmp /dest/download/android.json
    mkdir -p /dest/updates/android-arm64
    cp -f /in/updates/android-arm64/latest.json.sig /dest/updates/android-arm64/.sig.tmp
    cp -f /in/updates/android-arm64/latest.json /dest/updates/android-arm64/.json.tmp
    mv -f /dest/updates/android-arm64/.sig.tmp /dest/updates/android-arm64/latest.json.sig
    mv -f /dest/updates/android-arm64/.json.tmp /dest/updates/android-arm64/latest.json
    chmod -R a+rX "/dest/download/android/$v" /dest/download/android.json /dest/updates/android-arm64'
  # Read back what the volume now holds.
  docker run --rm -i --network none -v "$volume:/dest:ro" "$ALPINE" sh -eu -c 'cd /dest && sha256sum -c' \
    < "$work/expect.sha256" > "$work/readback.txt" 2>&1 ||
    { echo "the files in volume $volume do not match what was published:" >&2; cat "$work/readback.txt" >&2; exit 1; }
  where="volume $volume"
fi
echo "published Krunk for Android $version to $where"
echo "  certificate sha256 $cert"
echo "  update manifest updates/android-arm64/latest.json (versionCode $version_code), signature verified, files read back"
sed 's/^/  /' "$rel/SHA256SUMS"
