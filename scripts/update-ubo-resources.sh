#!/usr/bin/env bash
# Rebuilds assets/filters/ubo-resources.json from the uBlock Origin release pinned in
# assets/filters/ubo-resources.lock: scriptlets (+js filters) and redirect resources
# ($redirect filters), in the adblock crate's Resource format. The tarball is checked
# against the pinned SHA-256 and unpacked into a fresh temporary folder.
#   scripts/update-ubo-resources.sh          (needs curl, tar, node 18+)
# To move to a newer uBO release: change UBO_VERSION/UBO_TARBALL, run with --pin to
# record the new SHA-256, review the diff of ubo-resources.json, commit.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=/dev/null
source "$root/assets/filters/ubo-resources.lock"
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 3 -o "$work/ubo.tar.gz" "$UBO_TARBALL"
have=$(sha256sum "$work/ubo.tar.gz" | cut -d' ' -f1)
if [ "${1:-}" = "--pin" ]; then
  sed -i "s/^UBO_SHA256=.*/UBO_SHA256=$have/" "$root/assets/filters/ubo-resources.lock"
  echo "pinned $UBO_VERSION: $have"
elif [ "$have" != "$UBO_SHA256" ]; then
  echo "uBO tarball hash mismatch: got $have, expected $UBO_SHA256" >&2
  exit 1
fi
mkdir "$work/src"
tar -xzf "$work/ubo.tar.gz" -C "$work/src" --strip-components=1
node "$root/scripts/build-ubo-resources.mjs" "$work/src" "$root/assets/filters/ubo-resources.json" "$UBO_VERSION"
