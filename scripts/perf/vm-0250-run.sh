#!/usr/bin/env bash
# Krunk 0.25.0 (M2): one tab pass of scripts/vm-0250-tabs.ps1 on the test VM, then the analysis.
#   scripts/perf/vm-0250-run.sh TAG N [PHASES] [EXTRA PS1 ARGS]
#     PHASES  comma list (default switch,rapid,far,discard,open,close,drag,cpu,flash); every phase
#             runs as its own task call (vm.sh ps1 waits at most 10 min per call)
#     EXTRA   e.g. "-App C:\pipeline\krunk\app250 -Theme light -Trace"
#   OUT=DIR   where results go (default ../.pipeline/evidence-0.25.0/vm/TAG)
#   SKIP_RUN=1  no VM run: only the analysis of what is in OUT
# e.g. scripts/perf/vm-0250-run.sh dry1 1 switch,open,cpu,flash "-Trace"
# Pushes the script, its helpers and the test pages; pulls shots\v0250-TAG; writes OUT/summary.md
# (analyze.py switch per recording, trace-summary.py per phase, flash, draglag, cpu).
set -euo pipefail
cd "$(dirname "$0")/../.."
tag=$1; n=$2
phases=${3:-switch,rapid,far,discard,open,close,drag,cpu,flash}
extra=${4:-}
out=${OUT:-$(cd .. && pwd)/.pipeline/evidence-0.25.0/vm/$tag}
R=C:/pipeline/krunk
vm() { scripts/vm.sh "$@" </dev/null; }
mkdir -p "$out"

if [ -z "${SKIP_RUN:-}" ]; then
  touch /tmp/krunk-vm-busy
  trap 'rm -f /tmp/krunk-vm-busy' EXIT
  for f in vm-lib.ps1:lib.ps1 vm-windows.ps1:vm-windows.ps1 vm-0250-tabs.ps1:vm-0250-tabs.ps1 vm-setkv.py:setkv.py \
           perf/report-serve.py:report-serve.py; do
    vm put "scripts/${f%%:*}" "$R/${f##*:}"
  done
  for f in heavy.html busy.html busy-frame.html newtabs.html marker.js test-h264-aac.mp4 test-vp9-opus.webm; do
    vm put "scripts/vm-www/8094/$f" "$R/www/8094/$f"
  done
  errs=$(vm sh '$e = $null; $t = $null; [void][System.Management.Automation.Language.Parser]::ParseFile("C:\pipeline\krunk\vm-0250-tabs.ps1", [ref]$t, [ref]$e); $e | ForEach-Object { "$($_.Extent.StartLineNumber): $($_.Message)" }' | tr -d '\r')
  [ -z "$errs" ] || { echo "vm-0250-tabs.ps1 does not parse: $errs" >&2; exit 2; }
  vm sh "Remove-Item -Recurse -Force $R/shots/v0250-$tag -ErrorAction SilentlyContinue" >/dev/null || true
  w=$(mktemp --suffix=.ps1)
  IFS=, read -ra list <<< "$phases"
  for ph in "${list[@]}"; do
    printf '& C:\\pipeline\\krunk\\vm-0250-tabs.ps1 -N %s -Tag %s -Phases %s %s\n' "$n" "$tag" "$ph" "$extra" > "$w"
    start=$(date +%s)
    vm ps1 "$w"
    # A phase past vm.sh's 10 min: wait for its marker.
    for _ in $(seq 1 90); do
      vm sh "Test-Path $R/shots/v0250-$tag/$ph.done" | grep -q True && break
      sleep 10
    done
    echo "== $ph: $(( $(date +%s) - start )) s"
  done
  rm -f "$w"
  vm sh "Compress-Archive -Force -Path $R/shots/v0250-$tag/* -DestinationPath $R/v0250-$tag.zip" >/dev/null
  vm get "$R/v0250-$tag.zip" "$out/results.zip"
  (cd "$out" && unzip -oq results.zip && rm results.zip)
  [ -f "$out/run.log" ] && iconv -f cp1252 -t utf-8 "$out/run.log" > "$out/run.log.u" && mv "$out/run.log.u" "$out/run.log"
  rm -f /tmp/krunk-vm-busy
fi

A=scripts/frames/analyze.py
T=scripts/perf/trace-summary.py
flash_line() { # PATH: the flash counts from analyze.py flash's JSON
  python3 "$A" flash "$1" --sheet "$1-flash.png" > "$1.flash.json" || true
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print("frames %(frames)s, white flash frames %(flash_frames)s at %(flashes_at)s, black flash frames %(black_flash_frames)s at %(black_flashes_at)s, uniform %(uniform_frames)s" % d)' "$1.flash.json" || echo "flash analysis failed"
}
{
  echo "# Krunk 0.25.0 tab pass $tag (N=$n)"
  echo
  echo "Phases: $phases; extra: ${extra:-none}; analysed $(date -u +%Y-%m-%dT%H:%MZ)"
  for ev in "$out"/*-events.csv; do
    [ -e "$ev" ] || continue
    rec=${ev%-events.csv}; name=$(basename "$rec"); phase=${name%%-*}
    echo; echo "## $name"; echo
    if [ -f "$rec.json" ]; then
      echo '```'; python3 "$A" switch "$rec" "$ev" --out "$rec.switch.json" || echo "analyze.py switch failed"; echo '```'
    else
      echo "no recording"
    fi
    if [ -f "$out/perf-$phase.log" ]; then
      echo
      python3 "$T" "$out/perf-$phase.log" "$ev" $( [ -f "$rec.switch.json" ] && echo --switch-json "$rec.switch.json" ) \
        --json "$rec.trace.json" || echo "trace-summary failed"
    fi
  done
  for rec in flash discard; do
    [ -f "$out/$rec.json" ] || continue
    echo; echo "## Flash check: $rec"; echo; flash_line "$out/$rec"
  done
  if [ -f "$out/drag-markers.txt" ]; then
    echo; echo "## Drag"
    while read -r rec m; do
      [ -f "$out/$rec.json" ] && [ -f "$out/$rec-moves.csv" ] || continue
      echo; echo "### $rec (marker $m)"; echo '```'
      python3 scripts/frames/draglag.py "$out/$rec" "$out/$rec-moves.csv" --marker "$m" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps({k: d.get(k) for k in ("tear_off_ms","run_ms","frames_in_run","lag_ms","gaps_ms","step_cv")}))' || echo "draglag failed"
      echo '```'
    done < <(tr -d '\r' < "$out/drag-markers.txt")
    if [ -f "$out/drag-reorder.json" ]; then
      echo; echo "### drag-reorder (strip)"; echo '```'
      python3 "$A" motion "$out/drag-reorder" | python3 -c 'import json,sys; d=json.load(sys.stdin); d.pop("path",None); print(json.dumps(d))' || echo "motion failed"
      echo '```'
    fi
    [ -f "$out/perf-drag.log" ] && { echo; python3 "$T" "$out/perf-drag.log" || true; }
  fi
  if [ -f "$out/cpu.txt" ]; then echo; echo "## Background CPU"; echo; tr -d '\r' < "$out/cpu.txt"; fi
  if [ -f "$out/run.log" ]; then echo; echo "## Run log (failures)"; echo; grep -E 'failed|lost|not found|no trace' "$out/run.log" || echo "none"; fi
} > "$out/summary.md"
echo "summary: $out/summary.md"
