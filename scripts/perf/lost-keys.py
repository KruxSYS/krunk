#!/usr/bin/env python3
"""Krunk 0.26.0: the share of typed navigations that never started, and why (F-keys).

  lost-keys.py DIR [DIR...]

Per browser and typed scenario (cold, warm): loads tried, loads that timed out, and of those how
many show keys lost to the page (the watched page counted keydowns), an address bar left empty or
partial (it doesn't hold the URL that was typed), or neither (unknown). Uses the diagnostics the
driver keeps in a timed-out row (`diag`, since c415092); older rows count as unknown.
"""
import json
import sys
from collections import defaultdict

tot = defaultdict(lambda: defaultdict(int))
for d in sys.argv[1:]:
    for line in open(f"{d}/results.jsonl", encoding="utf-8"):
        if not line.strip():
            continue
        r = json.loads(line)
        if r.get("scenario") not in ("cold", "warm"):
            continue
        k = (r["browser"], r["scenario"])
        tot[k]["tried"] += 1
        if r.get("kind") != "timeout":
            continue
        tot[k]["timeout"] += 1
        diag = r.get("diag") or {}
        keys = (diag.get("page") or {}).get("keys", 0)
        shell = diag.get("shell") or {}
        if keys:
            tot[k]["keys_in_page"] += 1
        elif shell and not shell.get("value"):
            tot[k]["address_empty"] += 1
        elif shell:
            tot[k]["address_partial_or_full"] += 1
        else:
            tot[k]["unknown"] += 1
print("| browser | scenario | tried | never started | rate | keys in the page | address bar empty | address bar has text | unknown |")
print("|---|---|---|---|---|---|---|---|---|")
for (b, s), c in sorted(tot.items()):
    rate = 100.0 * c["timeout"] / c["tried"] if c["tried"] else 0
    print(f"| {b} | {s} | {c['tried']} | {c['timeout']} | {rate:.1f} % | {c['keys_in_page']} | {c['address_empty']} | {c['address_partial_or_full']} | {c['unknown']} |")
