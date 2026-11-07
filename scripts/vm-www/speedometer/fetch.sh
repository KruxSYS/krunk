#!/usr/bin/env bash
# Puts Speedometer 3.1 (WebKit/Speedometer, branch release/3.1, pinned) on the test VM as
# C:\pipeline\krunk\speedometer, with the result reporter and its server. Not vendored.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
src=$here/../../..
COMMIT=1386415be8fef2f6b6bbdbe1828872471c5d802a
work=${SPEEDOMETER_DIR:-/work/projects/browser/analysis/speedometer-3.1}
if [ ! -d "$work/.git" ]; then
  git clone -q --depth 1 -b release/3.1 https://github.com/WebKit/Speedometer.git "$work"
fi
[ "$(git -C "$work" rev-parse HEAD)" = "$COMMIT" ] || { echo "speedometer: expected $COMMIT"; exit 1; }
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/speedometer"
(cd "$work" && tar --exclude=.git --exclude=tests --exclude=node_modules -cf - .) | (cd "$stage/speedometer" && tar xf -)
cp "$here/report.mjs" "$here/sp-serve.py" "$stage/speedometer/"
sed -i 's#<script src="resources/main.mjs" type="module"></script>#&\n        <script src="report.mjs" type="module"></script>#' "$stage/speedometer/index.html"
grep -q report.mjs "$stage/speedometer/index.html" || { echo "speedometer: index.html patch failed"; exit 1; }
(cd "$stage" && zip -qr speedometer.zip speedometer)
"$src/scripts/vm.sh" put "$stage/speedometer.zip" C:/pipeline/krunk/speedometer.zip
"$src/scripts/vm.sh" sh "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue C:/pipeline/krunk/speedometer; Expand-Archive -Force C:/pipeline/krunk/speedometer.zip C:/pipeline/krunk; Remove-Item C:/pipeline/krunk/speedometer.zip; 'ok'"
