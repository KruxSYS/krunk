#!/usr/bin/env bash
# Krunk 0.26.0 page-load harness, host side: one pass of scripts/perf/load/load.mjs on the test VM.
#   scripts/perf/load-run.sh TAG CONFIG.json
#     CONFIG   load.mjs's config (see its header); "out" is set here to C:\pipeline\krunk\load\out\TAG
#   OUT=DIR    where results go (default ../.pipeline/evidence-0.26.0/runs/TAG)
#   ATTACH=1   don't push or start: wait for a pass already running on the VM, then pull (after
#              the host side was interrupted)
#   TOOLS=DIR  wpr.exe, lshim.exe, kinput.exe, node.exe, the replay CA (default ../.pipeline/loadtools)
# Pushes the driver and the tools (once), trusts the replay CA on the VM for the pass and removes
# it afterwards, waits for the pass, pulls results.jsonl/run.log and writes OUT/summary.md
# (scripts/perf/load-summary.py). Holds /tmp/krunk-vm-busy (no host builds meanwhile).
set -euo pipefail
cd "$(dirname "$0")/../.."
tag=$1; conf=$2
out=${OUT:-$(cd .. && pwd)/.pipeline/evidence-0.26.0/runs/$tag}
tools=${TOOLS:-$(cd .. && pwd)/.pipeline/loadtools}
L=C:/pipeline/krunk/load
vm() { scripts/vm.sh "$@" </dev/null; }
mkdir -p "$out"
touch /tmp/krunk-vm-busy
cleanup() {
  # The CA goes only when the VM pass is over: an interrupted host side must not pull it from
  # under a pass that keeps running (the 0.26.0 baseline lost a cell that way).
  if vm sh "Test-Path $L/out/$tag/done" 2>/dev/null | grep -q True; then
    vm sh "Get-ChildItem Cert:\\LocalMachine\\Root | Where-Object Subject -match 'Krunk page-load replay CA' | Remove-Item" >/dev/null 2>&1 || true
    rm -f /tmp/krunk-vm-busy
  else
    echo "load-run: the VM pass $tag still runs: CA and /tmp/krunk-vm-busy left in place (ATTACH=1 to wait for it)" >&2
  fi
}
trap cleanup EXIT
if [ -z "${ATTACH:-}" ]; then
vm sh "New-Item -ItemType Directory -Force $L, $L/out | Out-Null"
for f in wpr.exe lshim.exe kinput.exe node.exe deterministic.js krunk-replay-ca.pem krunk-replay-ca.key; do
  size=$(stat -c %s "$tools/$f")
  have=$(vm sh "(Get-Item $L/$f -ErrorAction SilentlyContinue).Length" | tr -d '\r\n ')
  [ "$have" = "$size" ] || vm put "$tools/$f" "$L/$f"
done
vm put scripts/perf/load/load.mjs "$L/load.mjs"
vm put scripts/perf/load/observer.js "$L/observer.js"
python3 - "$conf" "$tag" > /tmp/krunk-load-config.json <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
c["out"] = "C:\\pipeline\\krunk\\load\\out\\" + sys.argv[2]
print(json.dumps(c, indent=1))
PY
vm put /tmp/krunk-load-config.json "$L/config.json"
vm sh "Remove-Item -Recurse -Force $L/out/$tag -ErrorAction SilentlyContinue" >/dev/null || true
vm sh "Import-Certificate -FilePath $L/krunk-replay-ca.pem -CertStoreLocation Cert:\\LocalMachine\\Root | Out-Null"
vm ps1 scripts/vm-0260-load.ps1
fi
start=$(date +%s)
while :; do
  sleep 30
  st=$(vm sh "if (Test-Path $L/out/$tag/done) { Get-Content $L/out/$tag/done } else { 'running' }" | tr -d '\r\n')
  [ "$st" != running ] && break
  n=$(vm sh "(Get-Content $L/out/$tag/results.jsonl -ErrorAction SilentlyContinue | Measure-Object -Line).Lines" | tr -d '\r\n')
  echo "== $tag: $(( ($(date +%s) - start) / 60 )) min, $n results"
done
echo "== $tag: $st after $(( ($(date +%s) - start) / 60 )) min"
for f in results.jsonl run.log wpr.log lshim.log; do vm get "$L/out/$tag/$f" "$out/$f" 2>/dev/null || true; done
# Files kept from the sessions (config "collect": perf.log, traces).
if vm sh "Test-Path $L/out/$tag/collect" | grep -q True; then
  vm sh "Compress-Archive -Force -Path $L/out/$tag/collect/* -DestinationPath $L/out/$tag/collect.zip" >/dev/null
  vm get "$L/out/$tag/collect.zip" "$out/collect.zip" && (mkdir -p "$out/collect" && cd "$out/collect" && unzip -oq ../collect.zip && rm ../collect.zip)
fi
cp "$conf" "$out/config.json"
python3 scripts/perf/load-summary.py "$out" > "$out/summary.md" || echo "load-summary failed"
[ "$st" = ok ]
