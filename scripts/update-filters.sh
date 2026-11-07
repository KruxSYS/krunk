#!/usr/bin/env bash
# Fetches the bundled filter lists into assets/filters/ (committed to git so
# builds need no network). Run from anywhere; review the diff before committing.
# uBO lists that `!#include` sub-lists (filters.txt, privacy.txt) get them
# inlined in place, as the daily update in the browser does
# (core::preprocess::inline_includes).
set -euo pipefail
cd "$(dirname "$0")/../assets/filters"

get() { # url out
  curl -fsSL --retry 3 -A "Krunk-filter-update" -o "$2" "$1"
}

fetch() { # name url
  echo "fetching $1"
  get "$2" "$1.tmp"
  # Sanity check: a real list has many lines and no HTML error page.
  if [ "$(wc -l < "$1.tmp")" -lt 50 ] || head -c 200 "$1.tmp" | grep -qi '<html'; then
    echo "  $1 looks wrong, keeping the old copy" >&2; rm -f "$1.tmp"; return 1
  fi
  local base="${2%/*}"
  if grep -q '^!#include ' "$1.tmp"; then
    : > "$1.inl"
    while IFS= read -r line || [ -n "$line" ]; do
      if [[ "$line" =~ ^\!#include\ ([A-Za-z0-9_.-]+\.txt)$ ]]; then
        sub="${BASH_REMATCH[1]}"
        echo "  including $sub"
        get "$base/$sub" "$1.sub"
        { echo "! >>> $sub"; grep -v '^!#include ' "$1.sub" || true; echo "! <<< $sub"; } >> "$1.inl"
      else
        printf '%s\n' "$line" >> "$1.inl"
      fi
    done < "$1.tmp"
    mv "$1.inl" "$1.tmp"; rm -f "$1.sub"
  fi
  mv "$1.tmp" "$1"
}

UBO=https://ublockorigin.github.io/uAssets/filters
fetch easylist.txt           https://easylist.to/easylist/easylist.txt
fetch nocoin.txt             https://raw.githubusercontent.com/hoshsadiq/adblock-nocoin-list/master/nocoin.txt
fetch easyprivacy.txt        https://easylist.to/easylist/easyprivacy.txt
fetch ublock-filters.txt     $UBO/filters.txt
fetch ublock-privacy.txt     $UBO/privacy.txt
fetch ublock-unbreak.txt     $UBO/unbreak.txt
fetch ublock-quick-fixes.txt $UBO/quick-fixes.txt
fetch ublock-badware.txt     $UBO/badware.txt

curl -fsSL -o LICENSE-GPL-3.0.txt https://www.gnu.org/licenses/gpl-3.0.txt
curl -fsSL -o LICENSE-CC-BY-SA-3.0.txt https://creativecommons.org/licenses/by-sa/3.0/legalcode.txt || true
date -u +%Y-%m-%d > FETCHED
echo "done: $(cat FETCHED)"
