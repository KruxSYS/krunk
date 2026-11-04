#!/usr/bin/env bash
# Fetches the CEF binary distribution pinned in cef.lock, verifies it and exports it in the
# layout cef-dll-sys expects. Prints the resulting CEF_PATH on the last line.
#
#   scripts/fetch-cef.sh            download (if needed), verify, export
#   scripts/fetch-cef.sh --pin      first-time pinning: record the archive's SHA-256 in cef.lock
#
# The archive is an untrusted download: it is kept in its own cache directory, checked against
# the SHA-1 published in the CEF build index (cef-builds.spotifycdn.com/index.json) *and* the
# SHA-256 pinned in cef.lock, and only then extracted.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lock="$root/cef.lock"
# shellcheck source=/dev/null
source "$lock"
cache=${LB_CEF_CACHE:-$HOME/.cache/lightbrowser-cef}
downloads="$cache/downloads"
out="$cache/$CEF_VERSION"
cdn=${CEF_DOWNLOAD_URL:-https://cef-builds.spotifycdn.com}
mkdir -p "$downloads"

# 1. The index must list exactly this archive with this SHA-1.
index="$downloads/index.json"
curl -fsS -o "$index" "$cdn/index.json"
python3 -I - "$index" "$CEF_ARCHIVE" "$CEF_SHA1" <<'EOF'
import json, sys
index, name, sha1 = sys.argv[1:]
files = [f for v in json.load(open(index))["windows64"]["versions"] for f in v["files"]]
match = [f for f in files if f["name"] == name]
if not match:
    sys.exit(f"{name} is not in the CEF build index")
if match[0]["sha1"] != sha1:
    sys.exit(f"index SHA-1 {match[0]['sha1']} != pinned {sha1}")
EOF

# 2. Download (once) and verify both hashes.
archive="$downloads/$CEF_ARCHIVE"
if [ ! -f "$archive" ]; then
  curl -fSL --retry 3 -o "$archive.part" "$cdn/${CEF_ARCHIVE//+/%2B}"
  mv "$archive.part" "$archive"
fi
echo "$CEF_SHA1  $archive" | sha1sum -c --quiet -
sha256=$(sha256sum "$archive" | cut -d' ' -f1)
if [ "${1:-}" = "--pin" ]; then
  sed -i "s/^CEF_SHA256=.*/CEF_SHA256=$sha256/" "$lock"
  echo "pinned CEF_SHA256=$sha256" >&2
elif [ "$sha256" != "$CEF_SHA256" ]; then
  echo "SHA-256 mismatch: $sha256 != pinned $CEF_SHA256" >&2
  exit 1
fi

# 3. Export once per version (export-cef-dir must match the cef crate version).
if [ ! -f "$out/.lightbrowser-exported" ] || [ "$(cat "$out/.lightbrowser-exported")" != "$sha256" ]; then
  export-cef-dir --force --target x86_64-pc-windows-msvc --version "$CEF_VERSION" \
    --archive "$archive" "$out" >&2
  # CEF's own licence file is not part of the exported layout.
  tar -xjf "$archive" -O "${CEF_ARCHIVE%.tar.bz2}/LICENSE.txt" > "$out/LICENSE.txt"
  echo "$sha256" > "$out/.lightbrowser-exported"
fi
echo "$out"
