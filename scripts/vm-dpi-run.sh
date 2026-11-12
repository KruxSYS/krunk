#!/usr/bin/env bash
# Runs scripts/vm-dpi-shots.ps1 on the test VM for a list of display modes and scales and
# collects the screenshots and the log:
#   scripts/vm-dpi-run.sh OUTDIR [--app DIR] SPEC...
#   SPEC = WIDTHxHEIGHT:PERCENT[:core|all|rt]     (rt = the runtime-change pass)
#   e.g.  scripts/vm-dpi-run.sh ../.pipeline/evidence-0.15.4/before 1280x800:100 1920x1080:150 1920x1080:150:rt
# --app names the folder under C:\pipeline\krunk that holds the build (default: app).
# Each SPEC is one run of the VM's interactive task (under 10 minutes); the display is back at
# 1280x800 @ 100 % after every run. Results: OUTDIR/<mode>-<percent>[-rt]/ with the PNGs,
# run.log (this run's lines) and sheet.jpg (all shots, small, for a first look).
set -euo pipefail
cd "$(dirname "$0")/.."
out=$1; shift
app=app
if [ "${1:-}" = --app ]; then app=$2; shift 2; fi
vm() { scripts/vm.sh "$@" </dev/null; }
K=C:/pipeline/krunk
for f in vm-lib.ps1:lib.ps1 vm-windows.ps1:vm-windows.ps1 vm-dpi.ps1:vm-dpi.ps1 vm-display.ps1:vm-display.ps1 \
         vm-dpi-shots.ps1:vm-dpi-shots.ps1 vm-0110-seed.py:dpi-seed.py vm-setkv.py:setkv.py vm-serve.ps1:serve.ps1 \
         vm-0150-authserve.py:authserve.py vm-www/8094/dpi.html:www/8094/dpi.html vm-www/8094/nav.html:www/8094/nav.html \
         vm-www/8094/pip.html:www/8094/pip.html vm-www/8094/probe.png:www/8094/probe.png; do
  vm put "scripts/${f%%:*}" "$K/${f##*:}"
done
# A script that does not parse would run nothing and leave no trace: check first.
errs=$(vm sh '$e = $null; $t = $null; [void][System.Management.Automation.Language.Parser]::ParseFile("C:\pipeline\krunk\vm-dpi-shots.ps1", [ref]$t, [ref]$e); $e | ForEach-Object { "$($_.Extent.StartLineNumber): $($_.Message)" }' | tr -d '\r')
[ -z "$errs" ] || { echo "vm-dpi-shots.ps1 does not parse: $errs" >&2; exit 2; }
w=$(mktemp --suffix=.ps1); trap 'rm -f "$w"' EXIT
for spec in "$@"; do
  IFS=: read -r mode pct set <<< "$spec"
  width=${mode%x*}; height=${mode#*x}; set=${set:-all}
  tag="$mode-$pct"; extra="-Set $set"
  if [ "$set" = rt ]; then tag="$tag-rt"; extra="-Runtime"; fi
  d="$out/$tag"; mkdir -p "$d"; rm -f "$d"/*
  printf '& C:\\pipeline\\krunk\\vm-dpi-shots.ps1 -Width %s -Height %s -Percent %s -App %s %s\n' "$width" "$height" "$pct" "$app" "$extra" > "$w"
  vm sh "Remove-Item $K/shots/* -Force -ErrorAction SilentlyContinue" >/dev/null
  start=$(date +%s)
  vm ps1 "$w"
  vm sh "Compress-Archive -Force -Path $K/shots/* -DestinationPath $K/dpi-shots.zip" >/dev/null
  vm get "$K/dpi-shots.zip" "$d/shots.zip" || { echo "== $tag: nothing collected"; continue; }
  (cd "$d" && unzip -oq shots.zip && rm shots.zip)
  for l in run.log display.log; do
    [ -f "$d/$l" ] && iconv -f cp1252 -t utf-8 "$d/$l" > "$d/$l.u" && mv "$d/$l.u" "$d/$l"
  done
  python3 scripts/dpi-sheet.py "$d" || true
  echo "== $tag ($(( $(date +%s) - start )) s): $(find "$d" -name '*.png' | wc -l) shots, $(grep -c 'FAILED\|ERROR' "$d/run.log" || true) failures"
  grep -h 'display: restored\|RESTORE FAILED' "$d/run.log" | tail -1
done
