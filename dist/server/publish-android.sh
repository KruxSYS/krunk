#!/usr/bin/env bash
# Publishes a Krunk for Android release (signed APKs) to the download server's release store.
#
#   dist/server/publish-android.sh VERSION (--dest DIR | --volume NAME) [--apks DIR]
#       [--changelog FILE | --notes FILE] [--released YYYY-MM-DD] [--cert-sha256 HEX]
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
#   --cert-sha256 the expected signing certificate fingerprint (default: read from the keystore
#                 in ~/.config/krunk/android-release.properties); every APK must carry it
#
# The APKs are signed by Gradle with Krunk's Android release key (never in a container or the
# image); this script only checks each APK's signature and certificate with apksigner before it
# copies anything. There is no in-app update channel yet: the website is the only distribution.
#
# Layout written:
#   download/android/<VERSION>/Krunk-android-<VERSION>-<abi>-release.apk, NOTES.md, SHA256SUMS
#   download/android.json   (the website's Android download box)
set -euo pipefail
ALPINE="alpine:3.22@sha256:2e0b81d60597f449baa56dc25d355367869d2dca4cd57e866f1e2b52af3eb55f"
ABIS="arm64-v8a universal x86_64"

usage() { sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
[ $# -ge 1 ] || usage
version=$1; shift
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dest="" volume="" apks="$root/dist/android" notes="" released="" cert=""
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
[ -x "$apksigner" ] || { echo "apksigner not found: $apksigner (android/scripts/setup-sdk.sh)" >&2; exit 1; }

# The expected certificate: given, or the release keystore's.
if [ -z "$cert" ]; then
  props="${KRUNK_ANDROID_SIGNING:-$HOME/.config/krunk/android-release.properties}"
  [ -f "$props" ] || { echo "no signing properties at $props: give --cert-sha256" >&2; exit 1; }
  store=$(sed -n 's/^storeFile=//p' "$props"); pass=$(sed -n 's/^storePassword=//p' "$props"); alias=$(sed -n 's/^keyAlias=//p' "$props")
  cert=$(keytool -list -v -keystore "$store" -storepass "$pass" -alias "$alias" 2>/dev/null | sed -n 's/^[[:space:]]*SHA256: //p' | head -1 | tr -d ':' | tr 'A-F' 'a-f')
  [ -n "$cert" ] || { echo "could not read the certificate fingerprint from $store" >&2; exit 1; }
fi
cert=$(printf '%s' "$cert" | tr -d ': ' | tr 'A-F' 'a-f')
[[ $cert =~ ^[0-9a-f]{64}$ ]] || { echo "bad certificate fingerprint: $cert" >&2; exit 2; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
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
chmod -R a+rX "$stage"

if [ -n "$dest" ]; then
  mkdir -p "$dest/download/android/$version"
  cp -f "$rel"/* "$dest/download/android/$version/"
  cp -f "$stage/download/android.json" "$dest/download/.android.json.tmp"
  mv -f "$dest/download/.android.json.tmp" "$dest/download/android.json"
  where=$dest
else
  command -v docker >/dev/null || { echo "docker is required for --volume" >&2; exit 1; }
  tar -C "$stage" -cf - . | docker run --rm -i --network none -v "$volume:/dest" "$ALPINE" sh -eu -c '
    mkdir -p /in && tar -C /in -xf -
    v=$(ls /in/download/android)
    mkdir -p "/dest/download/android/$v"
    cp -f /in/download/android/"$v"/* "/dest/download/android/$v/"
    cp -f /in/download/android.json /dest/download/.android.json.tmp
    mv -f /dest/download/.android.json.tmp /dest/download/android.json
    chmod -R a+rX /dest'
  where="volume $volume"
fi
echo "published Krunk for Android $version to $where"
echo "  certificate sha256 $cert"
sed 's/^/  /' "$rel/SHA256SUMS"
