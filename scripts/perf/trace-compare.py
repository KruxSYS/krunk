#!/usr/bin/env python3
"""Krunk 0.26.0 P1: compares Chrome-format traces (--trace-startup JSON) of the same page loads.

  trace-compare.py NAME=TRACE.json [NAME=TRACE.json ...] [--top 40] [--match REGEX]

Per trace: the time (ms, wall duration of complete 'X' events and B/E pairs, nested events
counted in full) per event name, summed per process kind (browser, renderer, gpu, utility), and
the count. Prints the top names by the largest difference between the traces, plus a fixed list
of events that matter for page loads (context creation, parsing, script, style/layout, paint,
navigation). A trace may hold several loads: compare traces of the same pages and runs.
"""
import gzip
import json
import re
import sys
from collections import defaultdict

KEY_EVENTS = [
    # Context creation (the V8 context snapshot question): Initialize contains CreateContext
    # (from the snapshot, or a fresh context from the global template), the prototype chain,
    # the conditional features and ContextCreatedNotification, which is where CEF's
    # OnContextCreated runs Krunk's scriptlets and page scripts (local_window_proxy.cc:244-454).
    "V8.NewContext",
    "LocalWindowProxy::Initialize",
    "LocalWindowProxy::CreateContext",
    "LocalWindowProxy::SetupWindowPrototypeChain",
    "InstallConditionalFeatures",
    "ContextCreatedNotification",
    "LocalWindowProxy::UpdateDocumentProperty",
    "V8ContextSnapshot::CreateContextFromSnapshot",
    "WindowProxy::InitializeIfNeeded",
    "V8PerContextData::Create",
    # Loading and parsing.
    "NavigationRequest",
    "NavigationRequest::CommitNavigation",
    "NavigationURLLoaderImpl::StartNavigation",
    "RenderFrameHostImpl::CommitNavigation",
    "HTMLDocumentParser::PumpTokenizer",
    "ParseHTML",
    "ResourceFetcher::RequestResource",
    # Script.
    "v8.compile",
    "V8.CompileCode",
    "v8.run",
    "V8.Execute",
    "FunctionCall",
    "EvaluateScript",
    "v8.callFunction",
    "MajorGC",
    "MinorGC",
    "V8.GC_MC_BACKGROUND_MARKING",
    # Style, layout, paint.
    "Document::UpdateStyleAndLayoutTree",
    "UpdateLayoutTree",
    "Layout",
    "LocalFrameView::UpdateLifecyclePhases",
    "Paint",
    "PrePaint",
    "Commit",
    "ProxyMain::BeginMainFrame",
    "ThreadProxy::BeginMainFrame",
    # Renderer start.
    "ChildProcessLauncher",
    "RenderProcessHostImpl::Init",
    "ChildProcessLauncher::Launch",
    "RenderThreadImpl::Init",
]


def kind_of(pname):
    p = (pname or "").lower()
    if "renderer" in p:
        return "renderer"
    if "gpu" in p:
        return "gpu"
    if "browser" in p:
        return "browser"
    return "utility"


def load(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt", encoding="utf-8", errors="replace") as f:
        text = f.read()
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        # A trace cut off at the end (the engine stopped mid-write): close the array.
        data = json.loads(text.rstrip().rstrip(",") + "]}")
    return data["traceEvents"] if isinstance(data, dict) else data


def summarize(events):
    pnames = {}
    for e in events:
        if e.get("ph") == "M" and e.get("name") == "process_name":
            pnames[e["pid"]] = e.get("args", {}).get("name", "")
    tot = defaultdict(float)  # (kind, name) -> ms
    cnt = defaultdict(int)
    stacks = defaultdict(list)  # (pid, tid) -> [(name, ts)]
    for e in events:
        ph = e.get("ph")
        name = e.get("name", "")
        k = kind_of(pnames.get(e.get("pid")))
        if ph == "X" and "dur" in e:
            tot[(k, name)] += e["dur"] / 1000.0
            cnt[(k, name)] += 1
        elif ph == "B":
            stacks[(e["pid"], e["tid"])].append((name, e["ts"]))
        elif ph == "E":
            st = stacks[(e["pid"], e["tid"])]
            if st:
                n, ts = st.pop()
                tot[(k, n)] += (e["ts"] - ts) / 1000.0
                cnt[(k, n)] += 1
    return tot, cnt


def main():
    args = sys.argv[1:]
    top = 40
    match = None
    if "--top" in args:
        i = args.index("--top")
        top = int(args[i + 1])
        del args[i : i + 2]
    if "--match" in args:
        i = args.index("--match")
        match = re.compile(args[i + 1])
        del args[i : i + 2]
    named = [a.split("=", 1) for a in args]
    sums = {n: summarize(load(p)) for n, p in named}
    names = [n for n, _ in named]
    keys = set()
    for tot, _ in sums.values():
        keys.update(tot)

    def row(key):
        cells = []
        for n in names:
            tot, cnt = sums[n]
            cells.append(f"{tot.get(key, 0):.1f} ({cnt.get(key, 0)})")
        return cells

    print("## Page-load events (ms total (count))\n")
    print("| process | event | " + " | ".join(names) + " |")
    print("|---|---|" + "---|" * len(names))
    for ev in KEY_EVENTS:
        for k in ("browser", "renderer", "gpu"):
            if any((k, ev) in sums[n][0] for n in names):
                print(f"| {k} | {ev} | " + " | ".join(row((k, ev))) + " |")
    print(f"\n## Largest differences (top {top})\n")
    def spread(key):
        v = [sums[n][0].get(key, 0) for n in names]
        return max(v) - min(v)
    cand = [k for k in keys if not match or match.search(k[1])]
    cand.sort(key=spread, reverse=True)
    print("| process | event | " + " | ".join(names) + " |")
    print("|---|---|" + "---|" * len(names))
    for key in cand[:top]:
        print(f"| {key[0]} | {key[1]} | " + " | ".join(row(key)) + " |")


if __name__ == "__main__":
    main()
