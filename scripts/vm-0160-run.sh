#!/usr/bin/env bash
# Runs phases of scripts/vm-0160.ps1 on the test VM and collects screenshots and the log:
#   scripts/vm-0160-run.sh OUTDIR [--app DIR] PHASE...
# The AI phases (aimenu, scratch, password) get the mock provider (ui/dev/mock-provider.cjs on
# port 18111, log OUTDIR/mock.jsonl) behind an ssh tunnel for as long as they run.
set -euo pipefail
cd "$(dirname "$0")/.."
out=$1; shift
app=app
if [ "${1:-}" = --app ]; then app=$2; shift 2; fi
vm() { scripts/vm.sh "$@" </dev/null; }
R=C:/pipeline/krunk
mkdir -p "$out"
for f in vm-lib.ps1:lib.ps1 vm-windows.ps1:vm-windows.ps1 vm-0160.ps1:vm-0160.ps1 vm-setkv.py:setkv.py vm-serve.ps1:serve.ps1 \
         vm-0160-preset.py:preset0160.py vm-0160-presets.json:presets0160.json vm-ext/krunk-fixture.zip:vm-ext/krunk-fixture.zip; do
  vm put "scripts/${f%%:*}" "$R/${f##*:}"
done
for f in scripts/vm-www/8094/*.html; do vm put "$f" "$R/www/8094/$(basename "$f")"; done
errs=$(vm sh '$e = $null; $t = $null; [void][System.Management.Automation.Language.Parser]::ParseFile("C:\pipeline\krunk\vm-0160.ps1", [ref]$t, [ref]$e); $e | ForEach-Object { "$($_.Extent.StartLineNumber): $($_.Message)" }' | tr -d '\r')
[ -z "$errs" ] || { echo "vm-0160.ps1 does not parse: $errs" >&2; exit 2; }
w=$(mktemp --suffix=.ps1); mock=""; tunnel=""
cleanup() { rm -f "$w"; [ -n "$mock" ] && kill "$mock" 2>/dev/null; [ -n "$tunnel" ] && kill "$tunnel" 2>/dev/null; true; }
trap cleanup EXIT
for phase in "$@"; do
  case "$phase" in aimenu|scratch|password)
    if [ -z "$mock" ]; then
      node ui/dev/mock-provider.cjs --port 18111 --log "$out/mock.jsonl" > "$out/mock.out" 2>&1 & mock=$!
      command ssh -N -o ExitOnForwardFailure=yes -R 127.0.0.1:18111:127.0.0.1:18111 windows & tunnel=$!
      sleep 3
    fi ;;
  esac
  d="$out/$phase"; mkdir -p "$d"; rm -f "$d"/*
  printf '& C:\\pipeline\\krunk\\vm-0160.ps1 -Phase %s -App %s\n' "$phase" "$app" > "$w"
  vm sh "Remove-Item $R/shots/* -Force -ErrorAction SilentlyContinue" >/dev/null
  start=$(date +%s)
  vm ps1 "$w"
  vm sh "Compress-Archive -Force -Path $R/shots/* -DestinationPath $R/v0160.zip" >/dev/null
  vm get "$R/v0160.zip" "$d/shots.zip" || { echo "== $phase: nothing collected"; continue; }
  (cd "$d" && unzip -oq shots.zip && rm shots.zip)
  [ -f "$d/run.log" ] && iconv -f cp1252 -t utf-8 "$d/run.log" > "$d/run.log.u" && mv "$d/run.log.u" "$d/run.log"
  python3 scripts/dpi-sheet.py "$d" || true
  echo "== $phase ($(( $(date +%s) - start )) s): $(find "$d" -name '*.png' | wc -l) shots, $(grep -c 'FAILED\|ERROR' "$d/run.log" || true) errors"
done
