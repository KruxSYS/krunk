#!/usr/bin/env bash
# Signs and publishes a Krunk release to the download/update server's release store.
#
#   dist/server/publish-release.sh INSTALLER ZIP VERSION (--dest DIR | --volume NAME)
#       [--key PATH] [--changelog FILE | --notes FILE] [--min-supported VERSION]
#       [--released YYYY-MM-DD]
#
#   INSTALLER   dist/Krunk-Setup-<VERSION>.exe
#   ZIP         dist/Krunk-win-x64.zip
#   --dest DIR      a plain folder (mounted into the container as /srv/releases)
#   --volume NAME   a Docker named volume, written through a throwaway pinned alpine container
#   --key PATH      Ed25519 private key (default ~/.config/krunk/update-signing.key)
#   --changelog FILE  where the release notes come from (default: the repository's
#                   CHANGELOG.md): its "## VERSION — DATE" section becomes NOTES.md, and DATE
#                   the release date. Publishing fails when the section is missing.
#   --notes FILE    use this Markdown file as NOTES.md instead (as before 0.13.0)
#   --min-supported the oldest version the installer updates in place (default 0.2.0)
#   --released      the release date (default: the changelog section's date, else today, UTC)
#
# Signing happens here, on this machine: the key never goes into a container or the image.
# The signature is checked again with the public key before anything is copied, and the update
# manifest is written last, so a client never sees a manifest for files that are not there yet.
#
# Layout written:
#   download/<VERSION>/Krunk-Setup-<VERSION>.exe, Krunk-win-x64.zip, NOTES.md, SHA256SUMS,
#                      files.json (every file of the release with its SHA-256, from the zip;
#                      Krunk 0.7.0+ checks its installed files against it, core/src/integrity.rs)
#   download/release.json              (the website's download box)
#   updates/win-x64/latest.json(.sig)  (Krunk's update check, see core/src/update.rs)
set -euo pipefail
ALPINE="alpine:3.22@sha256:2e0b81d60597f449baa56dc25d355367869d2dca4cd57e866f1e2b52af3eb55f"

usage() { sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
[ $# -ge 3 ] || usage
installer=$1 zip=$2 version=$3; shift 3
dest="" volume="" key="$HOME/.config/krunk/update-signing.key" notes="" min_supported="0.2.0"
changelog="$(dirname "$0")/../../CHANGELOG.md" released=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dest) dest=$2; shift 2 ;;
    --volume) volume=$2; shift 2 ;;
    --key) key=$2; shift 2 ;;
    --notes) notes=$2; shift 2 ;;
    --changelog) changelog=$2; shift 2 ;;
    --min-supported) min_supported=$2; shift 2 ;;
    --released) released=$2; shift 2 ;;
    *) usage ;;
  esac
done
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'
[[ $version =~ $semver ]] || { echo "not a semver version: $version" >&2; exit 2; }
[[ $min_supported =~ $semver ]] || { echo "not a semver version: $min_supported" >&2; exit 2; }
[[ -z $released || $released =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || { echo "--released must be YYYY-MM-DD" >&2; exit 2; }
{ [ -n "$dest" ] && [ -z "$volume" ]; } || { [ -z "$dest" ] && [ -n "$volume" ]; } ||
  { echo "give exactly one of --dest DIR or --volume NAME" >&2; exit 2; }
[[ -z $volume || $volume =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { echo "bad volume name: $volume" >&2; exit 2; }
for f in "$installer" "$zip"; do [ -f "$f" ] || { echo "missing: $f" >&2; exit 1; }; done
[ -f "$key" ] || { echo "signing key not found: $key (see gen-signing-key.sh)" >&2; exit 1; }
[ -z "$notes" ] || [ -f "$notes" ] || { echo "missing notes file: $notes" >&2; exit 1; }
command -v openssl >/dev/null || { echo "openssl (3.x) is required" >&2; exit 1; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
# The release notes: the version's CHANGELOG.md section (the same parser renders /changelog),
# unless --notes names a file.
if [ -z "$notes" ]; then
  [ -f "$changelog" ] || { echo "changelog not found: $changelog (or give --notes FILE)" >&2; exit 1; }
  changelog_py="$(dirname "$0")/changelog.py"
  section=$(python3 -I "$changelog_py" section "$changelog" "$version") ||
    { echo "no release notes for $version (see above): add or fix its section in $changelog" >&2; exit 1; }
  printf '# Krunk %s\n\n%s\n' "$version" "$section" > "$work/NOTES.md"
  notes="$work/NOTES.md"
  if [ -z "$released" ]; then
    released=$(python3 -I "$changelog_py" date "$changelog" "$version")
    echo "release date $released (from $changelog)"
  fi
fi
[ -n "$released" ] || released=$(date -u +%Y-%m-%d)
stage="$work/releases"
rel="$stage/download/$version"
mkdir -p "$rel" "$stage/updates/win-x64"
setup_name="Krunk-Setup-$version.exe"
cp "$installer" "$rel/$setup_name"
cp "$zip" "$rel/Krunk-win-x64.zip"
cp "$notes" "$rel/NOTES.md"
(cd "$rel" && sha256sum "$setup_name" Krunk-win-x64.zip > SHA256SUMS)
setup_sha=$(sha256sum "$rel/$setup_name" | cut -d' ' -f1)
setup_size=$(stat -c %s "$rel/$setup_name")
zip_sha=$(sha256sum "$rel/Krunk-win-x64.zip" | cut -d' ' -f1)
zip_size=$(stat -c %s "$rel/Krunk-win-x64.zip")

# The release's file list, from the zip (the installer holds the same folder). Its SHA-256 goes
# into the signed manifest, so it needs no signature of its own.
command -v unzip >/dev/null || { echo "unzip is required" >&2; exit 1; }
mkdir -p "$work/unpacked"
unzip -q "$rel/Krunk-win-x64.zip" -d "$work/unpacked"
[ -d "$work/unpacked/Krunk-win-x64" ] || { echo "the zip has no Krunk-win-x64 folder" >&2; exit 1; }
(cd "$work/unpacked/Krunk-win-x64" && find . -type f | sed 's#^\./##' | LC_ALL=C sort | while read -r f; do
  printf '%s\t%s\n' "$f" "$(sha256sum "$f" | cut -d' ' -f1)"; done) |
  python3 -c 'import json,sys; v=sys.argv[1]; files=dict(l.rstrip("\n").split("\t") for l in sys.stdin); print(json.dumps({"version": v, "files": files}, separators=(",", ":")))' "$version" > "$rel/files.json"
files_sha=$(sha256sum "$rel/files.json" | cut -d' ' -f1)

# The update manifest: compact, one line, signed byte for byte.
manifest="$stage/updates/win-x64/latest.json"
printf '{"version":"%s","released":"%s","url":"/download/%s/%s","sha256":"%s","size":%s,"notes_url":"/download/%s/NOTES.md","min_supported":"%s","files_url":"/download/%s/files.json","files_sha256":"%s"}\n' \
  "$version" "$released" "$version" "$setup_name" "$setup_sha" "$setup_size" "$version" "$min_supported" "$version" "$files_sha" > "$manifest"
openssl pkeyutl -sign -rawin -inkey "$key" -in "$manifest" | base64 -w0 > "$manifest.sig"
# Check it as Krunk will, with the public half of the same key.
openssl pkey -in "$key" -pubout -out "$work/pub.pem"
base64 -d "$manifest.sig" > "$work/sig.bin"
openssl pkeyutl -verify -pubin -inkey "$work/pub.pem" -rawin -in "$manifest" -sigfile "$work/sig.bin" >/dev/null ||
  { echo "the new signature does not verify" >&2; exit 1; }

cat > "$stage/download/release.json" <<EOF
{
  "version": "$version",
  "released": "$released",
  "notes": "/download/$version/NOTES.md",
  "files": [
    {"name": "$setup_name", "kind": "installer", "url": "/download/$version/$setup_name", "size": $setup_size, "sha256": "$setup_sha"},
    {"name": "Krunk-win-x64.zip", "kind": "portable", "url": "/download/$version/Krunk-win-x64.zip", "size": $zip_size, "sha256": "$zip_sha"}
  ]
}
EOF
chmod -R a+rX "$stage"

if [ -n "$dest" ]; then
  mkdir -p "$dest"
  # Files first, then the site metadata, then the update manifest and its signature.
  mkdir -p "$dest/download/$version" "$dest/updates/win-x64"
  cp -f "$rel"/* "$dest/download/$version/"
  cp -f "$stage/download/release.json" "$dest/download/release.json.tmp"
  mv -f "$dest/download/release.json.tmp" "$dest/download/release.json"
  cp -f "$manifest.sig" "$dest/updates/win-x64/latest.json.sig.tmp"
  cp -f "$manifest" "$dest/updates/win-x64/latest.json.tmp"
  mv -f "$dest/updates/win-x64/latest.json.sig.tmp" "$dest/updates/win-x64/latest.json.sig"
  mv -f "$dest/updates/win-x64/latest.json.tmp" "$dest/updates/win-x64/latest.json"
  where=$dest
else
  command -v docker >/dev/null || { echo "docker is required for --volume" >&2; exit 1; }
  # The files travel as a tar stream on stdin, so this works with a remote Docker daemon too.
  tar -C "$stage" -cf - . | docker run --rm -i --network none -v "$volume:/dest" "$ALPINE" sh -eu -c '
    mkdir -p /in && tar -C /in -xf -
    v=$(ls /in/download | grep -v release.json)
    mkdir -p "/dest/download/$v" /dest/updates/win-x64
    cp -f /in/download/"$v"/* "/dest/download/$v/"
    cp -f /in/download/release.json /dest/download/.release.json.tmp
    mv -f /dest/download/.release.json.tmp /dest/download/release.json
    cp -f /in/updates/win-x64/latest.json.sig /dest/updates/win-x64/.sig.tmp
    cp -f /in/updates/win-x64/latest.json /dest/updates/win-x64/.json.tmp
    mv -f /dest/updates/win-x64/.sig.tmp /dest/updates/win-x64/latest.json.sig
    mv -f /dest/updates/win-x64/.json.tmp /dest/updates/win-x64/latest.json
    chmod -R a+rX /dest'
  where="volume $volume"
fi
echo "published Krunk $version to $where"
echo "  installer $setup_name  $setup_size bytes  sha256 $setup_sha"
echo "  manifest  $(cat "$manifest")"
