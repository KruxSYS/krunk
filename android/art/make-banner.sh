#!/usr/bin/env bash
# Renders the Android TV banner (tv_banner.svg -> res/drawable-xhdpi/tv_banner.png, 320x180).
set -euo pipefail
cd "$(dirname "$0")"
out=../app/src/main/res/drawable-xhdpi/tv_banner.png
inkscape tv_banner.svg --export-type=png --export-width=320 --export-height=180 --export-filename="$out"
optipng -quiet -o5 "$out"
