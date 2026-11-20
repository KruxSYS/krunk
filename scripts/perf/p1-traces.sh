#!/usr/bin/env bash
# Krunk 0.26.0 P1: Chrome-format startup traces of the same page loads in Krunk and Chrome.
#   scripts/perf/p1-traces.sh [RUNS] [PAGES]      (defaults: 3, spa,news,ads)
# Krunk (app260h, base profile lp-trace = lp-base + the trace switches as extra flags) and Chrome
# (the same switches on its command line), typed scenario (one cold and one warm load per
# trace), interleaved; each session's trace.json is kept (load.mjs "collect"). Then
# trace-compare.py per page and run: OUT/compare-<page>-<run>.md, and OUT/compare-all.md over
# every trace of each browser.
#   OUT=DIR   default ../.pipeline/evidence-0.26.0/p1/traces
set -euo pipefail
cd "$(dirname "$0")/../.."
runs=${1:-3}; pages=${2:-spa,news,ads}
out=${OUT:-$(cd .. && pwd)/.pipeline/evidence-0.26.0/p1/traces}
K=C:/pipeline/krunk
# Context creation and Krunk's scripts (v8), renderer start (startup), navigation and commit
# (navigation, loading), parse/script/style/layout/paint (devtools.timeline, blink, cc).
CATS=toplevel,startup,navigation,loading,v8,devtools.timeline,blink,cc,disabled-by-default-devtools.timeline
DUR=30
vm() { scripts/vm.sh "$@" </dev/null; }
# Krunk takes the switches as extra flags (Settings::browser_switches); the path is the
# session's profile (load.mjs copies lp-trace to lp\krunk-tr for every session).
flags="--disable-quic --trace-startup=$CATS --trace-startup-file=C:\\pipeline\\krunk\\lp\\krunk-tr\\trace.json --trace-startup-duration=$DUR"
vm sh "Remove-Item -Recurse -Force $K/lp-trace -ErrorAction SilentlyContinue; Copy-Item -Recurse $K/lp-base $K/lp-trace"
printf '{"extraFlags": "%s"}' "${flags//\\/\\\\}" > /tmp/p1-trace-flags.json
vm put /tmp/p1-trace-flags.json "$K/p1-trace-flags.json"
vm sh "python $K/setkv.py $K/lp-trace '@$K/p1-trace-flags.json'"
KRUNK_KRUNK_TR_APP=app260h KRUNK_KRUNK_TR_BASE=lp-trace COLLECT='trace.json' \
BROWSER_ARGS="[\"--trace-startup=$CATS\",\"--trace-startup-file={profile}\\\\trace.json\",\"--trace-startup-duration=$DUR\"]" \
  python3 scripts/perf/load/mkconfig.py replay "$runs" krunk-tr,chrome typed "$pages" > /tmp/p1-traces.json
OUT=$out scripts/perf/load-run.sh p1-traces /tmp/p1-traces.json || echo "p1-traces: the pass reported a problem (see $out/run.log)"
C=$out/collect
IFS=, read -ra list <<< "$pages"
for p in "${list[@]}"; do
  for r in $(seq 1 "$runs"); do
    k=$C/krunk-tr-$p-$r-typed-trace.json; c=$C/chrome-$p-$r-typed-trace.json
    [ -f "$k" ] && [ -f "$c" ] || { echo "missing trace for $p run $r"; continue; }
    python3 scripts/perf/trace-compare.py "krunk=$k" "chrome=$c" > "$out/compare-$p-$r.md"
  done
done
echo "traces in $C, comparisons in $out/compare-*.md"
