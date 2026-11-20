#!/usr/bin/env bash
# Krunk 0.26.0: the baseline's generated parts, once the main pass, the fill pass and the live
# set are pulled (no VM, no build):
#   baseline/generated.md   load-summary.py over main + fill (fill sessions replace failed ones),
#                           krunk-a/b pooled as krunk; baseline/numbers.json (with the noise band)
#   baseline/live.md        the live set (noisy)
#   baseline/lost-keys.md   typed navigations that never started, and why
#   baseline/cells.md       Krunk's cold typed cells: measured / failed / filled / still missing
#   baseline/switches.txt   the "Krunk's switches" lines of the collected krunk.log files
set -euo pipefail
cd "$(dirname "$0")/../.."
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
R=$E/runs
dirs=("$R/baseline")
[ -f "$R/baseline-fill/results.jsonl" ] && dirs+=("$R/baseline-fill")
[ -f "$R/baseline-waitfocus/results.jsonl" ] && echo "note: a focus-waiting pass exists; summarise it separately" >&2
python3 scripts/perf/load-summary.py "${dirs[@]}" --pool krunk=krunk-a,krunk-b --json "$E/baseline/numbers.json" > "$E/baseline/generated.md"
[ -f "$R/baseline-live/results.jsonl" ] && python3 scripts/perf/load-summary.py "$R/baseline-live" > "$E/baseline/live.md"
python3 scripts/perf/lost-keys.py "${dirs[@]}" > "$E/baseline/lost-keys.md"
python3 - "${dirs[@]}" > "$E/baseline/cells.md" <<'PY'
import json, sys
main, fill = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else None)
def rows(d):
    try:
        return [json.loads(l) for l in open(f"{d}/results.jsonl", encoding="utf-8") if l.strip()]
    except FileNotFoundError:
        return []
def cold(rs, b):
    return {(r["page"], r["run"]): r.get("kind") for r in rs if r["browser"] == b and r.get("scenario") == "cold"}
print("| Krunk cold typed cells | krunk-a | krunk-b |\n|---|---|---|")
out = {}
for b in ("krunk-a", "krunk-b"):
    m, f = cold(rows(main), b), cold(rows(fill), b) if fill else {}
    ok_main = sum(1 for k in m.values() if k == "load")
    failed = [c for c, k in m.items() if k != "load"]
    filled = [c for c in failed if f.get(c) == "load"]
    missing = sorted(c for c in failed if f.get(c) != "load")
    out[b] = (len(m), ok_main, len(failed), len(filled), missing)
for label, i in (("rows in the main pass", 0), ("measured in the main pass", 1), ("failed in the main pass", 2), ("measured in the fill pass", 3)):
    print(f"| {label} | {out['krunk-a'][i]} | {out['krunk-b'][i]} |")
print(f"| still missing (excluded on both sides) | {len(out['krunk-a'][4])} | {len(out['krunk-b'][4])} |")
for b in out:
    if out[b][4]:
        print(f"\n{b} still missing: " + ", ".join(f"{p} run {r}" for p, r in out[b][4]))
PY
grep -h "Krunk's switches" "$R"/baseline-fill/collect/*krunk.log 2>/dev/null | sort | uniq -c > "$E/baseline/switches.txt" || true
echo "wrote $E/baseline/{generated,live,lost-keys,cells}.md, numbers.json, switches.txt"
