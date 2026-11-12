#!/usr/bin/env python3
"""Reconcile the request observer's log with a per-UID packet capture.

Every destination in the capture must be explained by a request the observer attributed to
uBlock Origin, to the engine (system principal) or to a page load. Anything else is printed as
UNATTRIBUTED.

Usage: python3 -I reconcile.py <logcat.txt> <capture.pcap> <t0 HH:MM:SS UTC> <idle seconds> [page-host ...] [ip=host ...]
Output: markdown tables (uBO hosts, engine hosts, capture destinations with attribution, split
into the idle part and the page-load part).
"""
import os
import re
import subprocess
import sys
from collections import OrderedDict
from urllib.parse import urlsplit

logcat, pcap, t0, idle = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
page_hosts = {a for a in sys.argv[5:] if "=" not in a}
# "ip=host": a destination that carried no name in the capture (for example a SYN that was never
# answered), named by a DNS lookup done by hand and recorded in the evidence.
aliases = dict(a.split("=", 1) for a in sys.argv[5:] if "=" in a)
here = os.path.dirname(os.path.abspath(__file__))


def secs(hms):
    h, m, s = hms.split(":")
    return int(h) * 3600 + int(m) * 60 + float(s)


T0 = secs(t0)
req = re.compile(r"^\d\d-\d\d (\d\d:\d\d:\d\d\.\d+) .*REQ who=(\S+) cancel=(\w+) type=(\d+) (\S+)")
who_hosts = {"ubo": OrderedDict(), "system": OrderedDict()}
with open(logcat, encoding="utf-8", errors="replace") as f:
    for line in f:
        m = req.match(line)
        if not m:
            continue
        t, who, cancel, _typ, url = m.groups()
        u = urlsplit(url)
        key = "ubo" if who.startswith("uBlock0") else "system"
        r = who_hosts[key].setdefault(u.hostname, {"n": 0, "cancelled": 0, "first": secs(t) - T0, "paths": OrderedDict()})
        r["n"] += 1
        r["cancelled"] += cancel == "true"
        r["paths"].setdefault(u.path[:70], 0)
        r["paths"][u.path[:70]] += 1

out = subprocess.run(
    [sys.executable, "-I", os.path.join(here, "pcap_summary.py"), pcap, "--t0", t0], capture_output=True, text=True, check=True
).stdout.splitlines()
dests, cur = [], None
for line in out[1:]:
    m = re.match(r"\s*([\d.]+)s\s+(\S+)\s+(tcp|udp|icmp6?|\d+)\s+(\d+) pkts\s+(\d+) bytes", line)
    if m:
        cur = {"t": float(m.group(1)), "dst": m.group(2), "proto": m.group(3), "pkts": int(m.group(4)), "bytes": int(m.group(5)), "names": []}
        dests.append(cur)
        continue
    m = re.match(r"\s+([\d.]+)s\s+(sni|http|dns|quic/udp-443)\s*(\S*)", line)
    if m and cur:
        cur["names"].append((m.group(2), m.group(3)))

ip_host = dict(aliases)
for d in dests:
    for kind, name in d["names"]:
        if kind in ("sni", "http") and name:
            ip_host.setdefault(d["dst"].rsplit(":", 1)[0], name.split(":")[0])


def attribute(d):
    """All names seen on the destination, and who they belong to (every name must be explained)."""
    ip = d["dst"].rsplit(":", 1)[0]
    hosts = list(OrderedDict.fromkeys(n.split(":")[0] for k, n in d["names"] if k in ("sni", "http") and n))
    if not hosts and ip in ip_host:
        hosts = [ip_host[ip]]
    if not hosts:
        return ip, "page load" if ip in page_hosts else "UNATTRIBUTED"
    who = []
    for host in hosts:
        if ip in page_hosts or host in page_hosts:
            w = "page load"
        elif host in who_hosts["ubo"] and host in who_hosts["system"]:
            w = "uBO and engine"
        elif host in who_hosts["ubo"]:
            w = "uBO"
        elif host in who_hosts["system"]:
            w = "engine"
        else:
            w = "UNATTRIBUTED"
        if w not in who:
            who.append(w)
    return ", ".join(hosts), " / ".join(who)


print(out[0])
for key, title in (("ubo", "Requests the observer attributed to uBlock Origin"), ("system", "Requests the observer attributed to the engine (system principal)")):
    print("\n### %s\n" % title)
    print("| Host | Requests | Cancelled by the gate | First seen | Paths |")
    print("|---|---|---|---|---|")
    for h, r in who_hosts[key].items():
        paths = "; ".join("%s ×%d" % (p, n) for p, n in list(r["paths"].items())[:4])
        more = len(r["paths"]) - 4
        print("| `%s` | %d | %d | %.0f s | %s%s |" % (h, r["n"], r["cancelled"], r["first"], paths, " (+%d more)" % more if more > 0 else ""))
    if not who_hosts[key]:
        print("| (none) | 0 | 0 | | |")

for part, lo, hi in (("idle (0 to %.0f s)" % (idle + 15), -1e9, idle + 15), ("page load (after %.0f s)" % (idle + 15), idle + 15, 1e9)):
    rows = [d for d in dests if lo <= d["t"] < hi]
    print("\n### Capture, connections first opened during: %s — %d packets\n" % (part, sum(d["pkts"] for d in rows)))
    print("| First packet | Destination | Proto | Packets | Bytes sent | Name | Attributed to |")
    print("|---|---|---|---|---|---|---|")
    for d in rows:
        host, who = attribute(d)
        print("| %.1f s | %s | %s | %d | %d | %s | %s |" % (d["t"], d["dst"], d["proto"], d["pkts"], d["bytes"], host, who))
    if not rows:
        print("| (none) | | | 0 | 0 | | |")
un = [d for d in dests if "UNATTRIBUTED" in attribute(d)[1]]
print("\nUnattributed destinations: %d" % len(un))
