#!/usr/bin/env bash
# Fetches the bundled filter lists into assets/filters/ (committed to git so
# builds need no network). Run from anywhere; review the diff before committing.
set -euo pipefail
cd "$(dirname "$0")/../assets/filters"

fetch() { # name url
  echo "fetching $1"
  curl -fsSL --retry 3 -A "Krunk-filter-update" -o "$1.tmp" "$2"
  # Sanity check: a real list has many lines and no HTML error page.
  if [ "$(wc -l < "$1.tmp")" -lt 50 ] || head -c 200 "$1.tmp" | grep -qi '<html'; then
    echo "  $1 looks wrong, keeping the old copy" >&2; rm -f "$1.tmp"; return 1
  fi
  mv "$1.tmp" "$1"
}

fetch easylist.txt        https://easylist.to/easylist/easylist.txt
fetch easyprivacy.txt     https://easylist.to/easylist/easyprivacy.txt
fetch ublock-filters.txt  https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt
fetch ublock-privacy.txt  https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/privacy.txt
fetch ublock-unbreak.txt  https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/unbreak.txt

curl -fsSL -o LICENSE-GPL-3.0.txt https://www.gnu.org/licenses/gpl-3.0.txt
curl -fsSL -o LICENSE-CC-BY-SA-3.0.txt https://creativecommons.org/licenses/by-sa/3.0/legalcode.txt || true
date -u +%Y-%m-%d > FETCHED
echo "done: $(cat FETCHED)"
