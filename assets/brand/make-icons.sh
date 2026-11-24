#!/usr/bin/env bash
# Renders krunk.svg to the PNG sizes and the multi-size krunk.ico
# (embedded in Krunk.exe and the installer; krunk-256.png is used by the website), and the
# installer's side image installer-side.bmp, and the website's icons (dist/server/site).
set -euo pipefail
cd "$(dirname "$0")"
sizes="16 20 24 32 40 48 64 128 256"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
for s in $sizes; do
  inkscape krunk.svg --export-type=png --export-filename="$tmp/$s.png" -w "$s" -h "$s" >/dev/null 2>&1
done
cp "$tmp/256.png" krunk-256.png
cp "$tmp/64.png" krunk-64.png
# PNG-compressed 256 px entry, BMP for the small ones (what Explorer expects).
convert $(for s in $sizes; do echo "$tmp/$s.png"; done) krunk.ico
optipng -quiet -o2 krunk-256.png krunk-64.png
# Installer side image (welcome/finish pages): 24-bit BMP, as NSIS requires.
inkscape installer-side.svg --export-type=png --export-filename="$tmp/side.png" -w 164 -h 314 >/dev/null 2>&1
convert "$tmp/side.png" -background white -alpha remove -type TrueColor BMP3:installer-side.bmp
# The website's copies (favicon, touch icon, header/footer logo).
site=../../dist/server/site
cp krunk.svg krunk-64.png "$site/"
inkscape krunk.svg --export-type=png --export-filename="$site/krunk-180.png" -w 180 -h 180 >/dev/null 2>&1
optipng -quiet -o2 "$site/krunk-180.png"
identify krunk.ico | awk '{print $3}' | tr '\n' ' '; echo
