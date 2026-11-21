#!/usr/bin/env python3
"""Krunk 0.26.0: which hosts a netlog shows DNS jobs, connect jobs or requests for.

    netlog-hosts.py NETLOG.json HOST [HOST...]
    netlog-hosts.py NETLOG.json --connects     (TCP connection addresses and proxy chains)

For each host: the netlog event types whose parameters name it, with counts. A host with no
HOST_RESOLVER_*, *CONNECT_JOB*, SOCKET* or URL_REQUEST* events got no DNS lookup, no socket
and no request. Exit status 0; the verdict line per host is "contacted" or "none".
"""
import json
import sys

CONTACT = ("HOST_RESOLVER", "CONNECT_JOB", "SOCKET", "URL_REQUEST", "HTTP_STREAM", "TCP_CONNECT", "SSL_")


def load(path):
    text = open(path, encoding="utf-8", errors="replace").read().rstrip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        # A netlog cut off at exit: close the events array.
        return json.loads(text.rstrip(",\n ") + "]}")


def main():
    log = load(sys.argv[1])
    names = {v: k for k, v in log["constants"]["logEventTypes"].items()}
    if sys.argv[2] == "--connects":
        # Every TCP connection the netlog shows (address) and every proxy chain used.
        seen = {}
        for ev in log.get("events", []):
            p = ev.get("params") or {}
            n = names.get(ev.get("type"), "")
            if n in ("TCP_CONNECT_ATTEMPT", "TCP_CONNECT") and "address" in p:
                seen[("connect", p["address"])] = seen.get(("connect", p["address"]), 0) + 1
            if "proxy_chain" in p or "proxy_server" in p:
                k = ("proxy", str(p.get("proxy_chain", p.get("proxy_server"))))
                seen[k] = seen.get(k, 0) + 1
        for (kind, v), c in sorted(seen.items()):
            print(f"{kind} {v} {c}")
        return
    for host in sys.argv[2:]:
        counts = {}
        for ev in log.get("events", []):
            params = ev.get("params")
            if params and host in json.dumps(params):
                name = names.get(ev.get("type"), str(ev.get("type")))
                counts[name] = counts.get(name, 0) + 1
        contact = {k: v for k, v in counts.items() if k.startswith(CONTACT)}
        print(f"{host}: {'contacted' if contact else 'none'}")
        for k, v in sorted(counts.items()):
            print(f"  {k} {v}")


if __name__ == "__main__":
    main()
