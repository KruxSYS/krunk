#!/usr/bin/env bash
# Krunk 0.10.0 (P1): runs vm-0100-tabs.ps1 for each spec and prints the summary table.
#   scripts/perf/tabs-matrix.sh OUTDIR TAG:BROWSER:N:REAL[:FG[:APP[:MODE]]] ...
#     FG    latency (default) | speedometer: what the active tab runs
#     APP   Krunk's folder under C:\pipeline\krunk (default app)
#     MODE  KRUNK_HIDDEN_MODE for a spike engine (aura | direct | off; P3)
# e.g. k30:krunk:30:0  ch30:chrome:30:0  k10r:krunk:10:5  s30:krunk:30:0:latency:app-spike:aura
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$1; shift
mkdir -p "$out"
dirs=()
for spec in "$@"; do
  IFS=: read -r tag browser n real fg app mode <<< "$spec"
  fg=${fg:-latency}
  app_sed="C:\\\\pipeline\\\\krunk\\\\${app:-app}"
  scripts/vm-batch.sh "$out" "$tag|vm-0100-tabs.ps1|1s/.*/param([string]\$Browser = '$browser', [int]\$N = $n, [string]\$Tag = '$tag', [string]\$App = '$app_sed', [int]\$Real = $real, [string]\$Fg = '$fg', [string]\$Mode = '${mode:-}')/"
  dirs+=("$out/$tag")
done
python3 scripts/perf/tabs-summary.py "${dirs[@]}"
