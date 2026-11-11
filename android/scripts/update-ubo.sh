#!/usr/bin/env bash
# The uBlock Origin XPI pinned in android/ubo.lock (modelled on scripts/update-ubo-resources.sh).
#
#   update-ubo.sh            fetch the XPI into the cache if it is not there, check its SHA-256
#                            against the lock, print the path of the checked file
#   update-ubo.sh --check    the same without the path (what the Gradle build runs); exit 1 on a
#                            hash mismatch, exit 2 when the file cannot be fetched
#   update-ubo.sh --pin      download the XPI named in the lock and write its SHA-256 into the lock
#                            (after changing UBO_VERSION / UBO_XPI for a new release)
#   update-ubo.sh --path     print where the cached file is expected (no network, no check)
#
# Cache: $KRUNK_UBO_CACHE, default ~/.cache/krunk/ubo; one folder per version. A cached file that
# does not match the lock is never used and never overwritten silently: delete it or fix the lock.
# KRUNK_UBO_LOCK points at another lock file (the build's hash-mismatch test uses it).
# Needs curl and sha256sum.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
lock="${KRUNK_UBO_LOCK:-$here/../ubo.lock}"
[ -f "$lock" ] || { echo "update-ubo: no lock file at $lock" >&2; exit 2; }
# shellcheck source=/dev/null
source "$lock"
: "${UBO_VERSION:?missing in $lock}" "${UBO_XPI:?missing in $lock}"
cache="${KRUNK_UBO_CACHE:-$HOME/.cache/krunk/ubo}/$UBO_VERSION"
file="$cache/$(basename "$UBO_XPI")"

fetch() { # fetch <destination>
  mkdir -p "$(dirname "$1")"
  local tmp="$1.part"
  if ! curl -fsSL --retry 3 -o "$tmp" "$UBO_XPI"; then
    rm -f "$tmp"
    echo "update-ubo: cannot download $UBO_XPI" >&2
    return 2
  fi
  mv "$tmp" "$1"
}

sum() { sha256sum "$1" | cut -d' ' -f1; }

case "${1:-}" in
  --path)
    echo "$file"
    ;;
  --pin)
    fetch "$file" || exit 2
    have=$(sum "$file")
    sed -i "s/^UBO_SHA256=.*/UBO_SHA256=$have/" "$lock"
    echo "pinned uBlock Origin $UBO_VERSION: $have"
    ;;
  "" | --check)
    : "${UBO_SHA256:?missing in $lock}"
    if [ ! -f "$file" ]; then
      fetch "$file" || exit 2
    fi
    have=$(sum "$file")
    if [ "$have" != "$UBO_SHA256" ]; then
      echo "update-ubo: hash mismatch for uBlock Origin $UBO_VERSION" >&2
      echo "  file:     $file" >&2
      echo "  got:      $have" >&2
      echo "  expected: $UBO_SHA256 ($lock)" >&2
      exit 1
    fi
    if [ "${1:-}" = "--check" ]; then
      echo "uBlock Origin $UBO_VERSION ok ($have)"
    else
      echo "$file"
    fi
    ;;
  *)
    sed -n '2,16p' "$0"
    exit 2
    ;;
esac
