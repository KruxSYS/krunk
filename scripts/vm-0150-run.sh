#!/usr/bin/env bash
# Runs one phase of scripts/vm-0150.ps1 on the VM and collects its screenshots, captures and log
# lines into .pipeline/evidence-0.15.0/vm/.
#   scripts/vm-0150-run.sh PHASE [extra ps1 args, e.g. "-TileX 300 -TileY 250"]
set -euo pipefail
cd "$(dirname "$0")/.."
phase=$1; extra=${2:-}
out=/work/projects/browser/.pipeline/evidence-0.15.0/vm
mkdir -p "$out"
scripts/vm.sh put scripts/vm-0150.ps1 C:/pipeline/krunk/vm-0150.ps1
w=$(mktemp --suffix=.ps1)
printf '& C:\\pipeline\\krunk\\vm-0150.ps1 -Phase %s %s\n' "$phase" "$extra" > "$w"
start=$(date +%s)
scripts/vm.sh ps1 "$w"
rm -f "$w"
echo "phase $phase: $(( $(date +%s) - start )) s"
scripts/vm.sh sh "if (Test-Path C:\\pipeline\\krunk\\shots\\v0150-$phase-*) { Compress-Archive -Force -Path C:\\pipeline\\krunk\\shots\\v0150-$phase-* -DestinationPath C:\\pipeline\\krunk\\v0150-$phase.zip }" >/dev/null
if scripts/vm.sh get "C:/pipeline/krunk/v0150-$phase.zip" "$out/v0150-$phase.zip" 2>/dev/null; then
  (cd "$out" && unzip -oq "v0150-$phase.zip" && rm "v0150-$phase.zip")
fi
scripts/vm.sh sh "Get-Content C:\\pipeline\\krunk\\shots\\run.log | Select-String '\\[v0150-$phase\\]' | ForEach-Object { \$_.Line }" | iconv -f cp1252 -t utf-8 > "$out/v0150-$phase-run.log" || true
ls "$out" | grep "v0150-$phase" | head -40
