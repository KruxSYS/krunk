#!/usr/bin/env bash
# Checks the libraries a Krunk for Android release ships against the OSV vulnerability database.
#
#   android/scripts/dep-audit.sh [--configuration NAME] [--out FILE]
#
# Resolves the app's runtime classpath with Gradle (default releaseRuntimeClasspath), then asks
# https://api.osv.dev about every Maven coordinate. Only "group:artifact" and the version are
# sent. Exit status: 0 = no known vulnerability, 1 = at least one, 2 = usage or tooling error.
# The Rust side is checked with `cargo audit` (see docs/ANDROID_DEV.md).
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ANDROID_DIR=$(cd "$HERE/.." && pwd)
configuration=releaseRuntimeClasspath
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --configuration) configuration=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
  esac
done

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
(cd "$ANDROID_DIR" && ./gradlew -q :app:dependencies --configuration "$configuration") > "$work/tree.txt" ||
  { echo "gradle could not resolve $configuration" >&2; exit 2; }

python3 -I - "$work/tree.txt" "$out" <<'PY'
import json
import re
import sys
import urllib.request

tree, out = sys.argv[1], sys.argv[2]
coords = set()
# Tree lines look like "+--- group:artifact:1.0 -> 1.2 (*)" or "\--- group:artifact -> 1.2"
# (version from a BOM); the version after "->" is the one that was selected.
line_re = re.compile(r"[+\\]--- ([\w.\-]+):([\w.\-]+)(?::([^\s]+))?(?: -> ([^\s]+))?")
for line in open(tree, encoding="utf-8"):
    m = line_re.search(line)
    if not m:
        continue
    group, artifact, declared, selected = m.groups()
    version = selected or declared
    if version:
        coords.add((f"{group}:{artifact}", version))
if not coords:
    print("no dependencies found in the Gradle output", file=sys.stderr)
    sys.exit(2)

ordered = sorted(coords)
queries = [{"package": {"name": n, "ecosystem": "Maven"}, "version": v} for n, v in ordered]
found = []
for start in range(0, len(queries), 200):
    body = json.dumps({"queries": queries[start:start + 200]}).encode()
    req = urllib.request.Request(
        "https://api.osv.dev/v1/querybatch",
        data=body,
        headers={"Content-Type": "application/json", "User-Agent": "krunk-dep-audit"},
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        results = json.load(resp)["results"]
    for (name, version), result in zip(ordered[start:start + 200], results):
        for vuln in result.get("vulns", []):
            found.append((name, version, vuln["id"]))

lines = [f"{len(ordered)} Maven coordinates checked against OSV"]
lines += [f"  {n} {v}" for n, v in ordered]
lines.append("")
if found:
    lines.append(f"{len(found)} known vulnerabilities:")
    lines += [f"  {n} {v}: {i} (https://osv.dev/vulnerability/{i})" for n, v, i in found]
else:
    lines.append("no known vulnerabilities")
text = "\n".join(lines) + "\n"
if out:
    open(out, "w", encoding="utf-8").write(text)
    print(lines[0])
    print("\n".join(lines[len(ordered) + 2:]))
else:
    print(text, end="")
sys.exit(1 if found else 0)
PY
