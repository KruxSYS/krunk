#!/usr/bin/env bash
# Fetches the NSIS version pinned in nsis.lock (Ubuntu packages, SHA-256 checked) into
# ~/.cache/krunk-toolchain/nsis-<version>/ and prints the makensis path. NSISDIR must point
# at <that folder>/usr/share/nsis when running it (build-dist.sh does).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=/dev/null
source "$root/nsis.lock"
dest="${KRUNK_TOOLCHAIN:-$HOME/.cache/krunk-toolchain}/nsis-$NSIS_VERSION"
if [ ! -x "$dest/usr/bin/makensis" ]; then
  work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
  for pair in "$NSIS_DEB:$NSIS_DEB_SHA256" "$NSIS_COMMON_DEB:$NSIS_COMMON_DEB_SHA256"; do
    deb=${pair%%:*}; sum=${pair##*:}
    curl -fsSL --retry 3 -o "$work/$deb" "$NSIS_BASE/$deb"
    echo "$sum  $work/$deb" | sha256sum -c --quiet - || { echo "checksum mismatch: $deb" >&2; exit 1; }
  done
  mkdir -p "$dest"
  for deb in "$work"/*.deb; do dpkg-deb -x "$deb" "$dest"; done
fi
echo "$dest/usr/bin/makensis"
