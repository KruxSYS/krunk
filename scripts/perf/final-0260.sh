#!/usr/bin/env bash
# Krunk 0.26.0 final pass: one full matrix, 0.26.0 (app260r: harness build of the release commit,
# no KRUNK_PERF_OFF, i.e. the shipped behaviour) vs 0.25.5 (app260h) vs Chrome 157 vs Edge 156,
# typed (cold+warm) and link (back/forward), 5 runs, WAIT_FOCUS=1 for every browser (the driver
# types once the address bar has the focus, so 0.25.5's Ctrl+L focus loss doesn't empty its cold
# cells; Chrome/Edge get a fixed 1 s pause). Output: evidence-0.26.0/final/summary.md
#   scripts/perf/final-0260.sh [RUNS]
set -uo pipefail
cd "$(dirname "$0")/../.."
runs=${1:-5}
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
export KRUNK_NEW_APP=app260r KRUNK_OLD_APP=app260h WAIT_FOCUS=1
python3 scripts/perf/load/mkconfig.py replay "$runs" krunk-new,krunk-old,chrome,edge typed,link > /tmp/final.json
OUT=$E/final scripts/perf/load-run.sh final-0260 /tmp/final.json || echo "final pass reported a problem"
python3 scripts/perf/load-summary.py "$E/final" --ab krunk-old=krunk-new --band "$E/baseline/numbers.json" > "$E/final/summary.md"
echo "summary: $E/final/summary.md"
