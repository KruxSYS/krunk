#!/usr/bin/env bash
# Krunk 0.10.0 (P5): runs VM flows one by one with vm-batch.sh and keeps their minutes.
#   scripts/perf/regression.sh OUTDIR LIMIT_SECONDS 'name|script|sed' ...
# Each flow's wall time goes to OUTDIR/minutes.txt; before a flow starts, the run stops if the
# minutes used so far plus the flow's reserve (180 s) would pass LIMIT_SECONDS.
set -uo pipefail
cd "$(dirname "$0")/../.."
out=$1; limit=$2; shift 2
mkdir -p "$out" /tmp/vmrun
used=$(awk '{s += $2} END {print s + 0}' "$out/minutes.txt" 2>/dev/null || echo 0)
for spec in "$@"; do
  name=${spec%%|*}
  if [ $((used + 180)) -gt "$limit" ]; then
    echo "stopped before $name: $used s used, limit $limit s" | tee -a "$out/minutes.txt"
    exit 3
  fi
  s=$(date +%s)
  scripts/vm-batch.sh "$out" "$spec" | tail -1
  d=$(( $(date +%s) - s ))
  used=$((used + d))
  echo "$name $d" >> "$out/minutes.txt"
done
echo "total $used s"
