#!/usr/bin/env bash
# Krunk 0.26.0 final pass: 0.26.0 (app260r: harness build of the release commit, no
# KRUNK_PERF_OFF, i.e. the shipped behaviour) vs 0.25.5 (app260h), typed (cold+warm) and link
# (back/forward), 5 runs. Krunk only (director, 2026-11-21): Chrome and Edge auto-updated on the
# VM to 158/157 after the baseline and no longer take the harness's input; Chrome 157 / Edge 156
# come from the baseline main pass. WAIT_FOCUS doesn't help 0.25.5 (its shell never takes the
# focus on the first Ctrl+L). Output: evidence-0.26.0/final/summary.md
#   scripts/perf/final-0260.sh [RUNS]
set -uo pipefail
cd "$(dirname "$0")/../.."
runs=${1:-5}
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
export KRUNK_NEW_APP=app260r KRUNK_OLD_APP=app260h
python3 scripts/perf/load/mkconfig.py replay "$runs" krunk-new,krunk-old typed,link > /tmp/final.json
OUT=$E/final scripts/perf/load-run.sh final-0260 /tmp/final.json || echo "final pass reported a problem"
python3 scripts/perf/load-summary.py "$E/final" --ab krunk-old=krunk-new --band "$E/baseline/numbers.json" > "$E/final/summary.md"
echo "summary: $E/final/summary.md"
