#!/usr/bin/env bash
# Krunk 0.26.0, VM session 1 (one VM session, no build meanwhile):
#   1. P1 pass A: the chain base -> pre -> omni -> block (typed cold+warm, link), app260n
#   2. the preconnect netlog proof (F-block, F-nsp; private tab): smoke/preload/
#   3. p1-traces.sh 3 spa,news,ads (0.25.5 vs Chrome: F-snap go/no-go)
# Results: p1/ab-a/summary.md, p1/load-trace.md, smoke/preload/netlog.md, p1/traces/compare-all.md
set -uo pipefail
cd "$(dirname "$0")/../.."
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
K=C:/pipeline/krunk
vm() { scripts/vm.sh "$@" </dev/null; }
echo "== session 1: pass A $(date +%T)"
PASS=a scripts/perf/p1-ab.sh app260n 5
echo "== session 1: netlog $(date +%T)"
P=$E/smoke/preload; mkdir -p "$P"
vm put scripts/vm-www/8092/preconnect.html "$K/www/8092/preconnect.html"
vm put scripts/vm-0260-netlog.ps1 "$K/vm-0260-netlog.ps1"
for c in on off private; do
  printf '& C:\\pipeline\\krunk\\vm-0260-netlog.ps1 -Case %s -App app260n\n' "$c" > /tmp/nl-$c.ps1
  vm ps1 /tmp/nl-$c.ps1
  vm get "$K/out/netlog/nl-$c.json" "$P/nl-$c.json" || echo "no netlog for $c"
done
{
  echo "# Preconnect netlog proof (app260n, $(date +%F))"
  echo "Page: scripts/vm-www/8092/preconnect.html. Expected: on = example.org contacted, doubleclick none,"
  echo "example.net none (NoStatePrefetch held back); off (control, block switched off) = doubleclick"
  echo "contacted; private = nothing contacted."
  for c in on off private; do
    echo; echo "## $c"; echo '```'
    python3 scripts/perf/netlog-hosts.py "$P/nl-$c.json" example.org doubleclick.net example.net 2>&1
    echo '```'
  done
} > "$P/netlog.md"
echo "== session 1: traces $(date +%T)"
scripts/perf/p1-traces.sh 3 spa,news,ads
echo "== session 1: done $(date +%T)"
