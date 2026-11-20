#!/usr/bin/env bash
# Krunk 0.26.0 P1: attribution and the per-fix A/B passes of one harness build (Krunk only).
#   scripts/perf/p1-ab.sh [APP] [RUNS]       (defaults: app260n, 5)
# Every configuration is the same build and base profile with the load trace on; they differ
# only in KRUNK_PERF_OFF (evidence-0.26.0/p1/summary.md §1z step 4):
#   krunk-base     every 0.26.0 fix off (0.25.5 behaviour + trace): attribution, A of each A/B
#   krunk-pre      F-pre on (network prediction in normal tabs)
#   krunk-filter   F-filter on (the page's address once per request)
#   krunk-scripts  F-scripts on (login.js watches the DOM from DOMContentLoaded)
#   krunk-writes   F-writes on (history visit 1 s after load end)
#   krunk-cosmetic F-cosmetic on (cosmetic filters off the navigation's critical path)
#   krunk-omni     F-pre + F-omni (address-bar preconnect; needs the patched engine)
#   krunk-block    F-pre + F-omni + F-block (blocked-host set; needs the patched engine)
# Pass A (typed + link, so back/forward and its NotUsed reasons too), a chain where each
# neighbouring pair is one A/B: base -> pre -> omni -> block.
# Pass B (typed): base vs filter, scripts, writes, cosmetic (one fix on each). Then the A/B verdicts against the baseline's noise band
# and the load-trace summary of krunk-base.
#   SKIP_RUN=1   only the analysis of what is in the output dirs
set -euo pipefail
cd "$(dirname "$0")/../.."
app=${1:-app260n}; runs=${2:-5}
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
ALL=preload,omnibox-preconnect,preconnect-block,filter-fast,login-defer,defer-writes,cosmetic-sync
off() { # the list without the given fix
  echo "$ALL" | tr , '\n' | grep -vx "$1" | paste -sd, -
}
keep() { # the list without the given fixes (comma-separated)
  echo "$ALL" | tr , '\n' | grep -vxF -f <(echo "$1" | tr , '\n') | paste -sd, -
}
envjson() { printf '{"KRUNK_PERF_OFF":"%s"}' "$1"; }
export KRUNK_ENV='{"KRUNK_PERF_TRACE":"1"}' COLLECT='Krunk\perf.log'
export KRUNK_BASE_APP=$app KRUNK_PRE_APP=$app KRUNK_FILTER_APP=$app KRUNK_SCRIPTS_APP=$app KRUNK_WRITES_APP=$app KRUNK_COSMETIC_APP=$app KRUNK_OMNI_APP=$app KRUNK_BLOCK_APP=$app
export KRUNK_BASE_ENV=$(envjson "$ALL")
export KRUNK_PRE_ENV=$(envjson "$(off preload)")
export KRUNK_OMNI_ENV=$(envjson "$(keep preload,omnibox-preconnect)")
export KRUNK_BLOCK_ENV=$(envjson "$(keep preload,omnibox-preconnect,preconnect-block)")
export KRUNK_FILTER_ENV=$(envjson "$(off filter-fast)")
export KRUNK_SCRIPTS_ENV=$(envjson "$(off login-defer)")
export KRUNK_WRITES_ENV=$(envjson "$(off defer-writes)")
export KRUNK_COSMETIC_ENV=$(envjson "$(off cosmetic-sync)")
python3 scripts/perf/load/mkconfig.py replay "$runs" krunk-base,krunk-pre,krunk-omni,krunk-block typed,link > /tmp/p1a.json
python3 scripts/perf/load/mkconfig.py replay "$runs" krunk-base,krunk-filter,krunk-scripts,krunk-writes,krunk-cosmetic typed > /tmp/p1b.json
#   PASS=a|b   run only that pass (default both)
if [ -z "${SKIP_RUN:-}" ]; then
  [ "${PASS:-a}" = a ] && { OUT=$E/p1/ab-a scripts/perf/load-run.sh p1-ab-a /tmp/p1a.json || echo "pass A reported a problem"; }
  [ "${PASS:-b}" = b ] && { OUT=$E/p1/ab-b scripts/perf/load-run.sh p1-ab-b /tmp/p1b.json || echo "pass B reported a problem"; }
fi
B=(--band "$E/baseline/numbers.json")
[ -f "$E/p1/ab-a/results.jsonl" ] && python3 scripts/perf/load-summary.py "$E/p1/ab-a" --ab krunk-base=krunk-pre --ab krunk-pre=krunk-omni --ab krunk-omni=krunk-block "${B[@]}" > "$E/p1/ab-a/summary.md"
[ -f "$E/p1/ab-b/results.jsonl" ] && python3 scripts/perf/load-summary.py "$E/p1/ab-b" --ab krunk-base=krunk-filter --ab krunk-base=krunk-scripts --ab krunk-base=krunk-writes --ab krunk-base=krunk-cosmetic "${B[@]}" > "$E/p1/ab-b/summary.md"
cat "$E"/p1/ab-*/collect/krunk-base-*perf.log > /tmp/p1-base-perf.log 2>/dev/null || true
python3 scripts/perf/load-trace-summary.py /tmp/p1-base-perf.log > "$E/p1/load-trace.md" || echo "no krunk-base perf.log"
echo "summaries: $E/p1/ab-a/summary.md $E/p1/ab-b/summary.md $E/p1/load-trace.md"
