#!/usr/bin/env python3
"""Summarise a per-UID capture made with `iptables ... -j NFLOG` + `tcpdump -i nflog:N -w`.

Only packets the app's UID *sent* are in such a capture. For each destination the script prints
packet and byte counts, the time of the first packet (seconds after the capture's first packet,
or after --t0 HH:MM:SS UTC), and what was seen: a new TCP connection (SYN), TLS SNI, HTTP Host +
request line, DNS queries. A destination without any of these got only packets of a connection
that was open before the capture began (keep-alive, closing).

Usage: python3 -I pcap_summary.py capture.pcap [--t0 HH:MM:SS] [--skip-host IP[:PORT]]...
No third-party modules (the sandbox has neither tshark nor scapy).
"""
import struct
import sys
import socket
import time
from collections import OrderedDict

NFULA_PAYLOAD = 9


def nflog_payload(data):
    off = 4
    while off + 4 <= len(data):
        length, typ = struct.unpack_from("<HH", data, off)
        if length < 4:
            break
        if (typ & 0x7FFF) == NFULA_PAYLOAD:
            return data[off + 4 : off + length]
        off += (length + 3) & ~3
    return None


def sni_of(payload):
    try:
        if len(payload) < 43 or payload[0] != 0x16 or payload[5] != 0x01:
            return None
        p = 5 + 4 + 2 + 32
        p += 1 + payload[p]
        p += 2 + struct.unpack_from(">H", payload, p)[0]
        p += 1 + payload[p]
        end = p + 2 + struct.unpack_from(">H", payload, p)[0]
        p += 2
        while p + 4 <= end and p + 4 <= len(payload):
            etype, elen = struct.unpack_from(">HH", payload, p)
            if etype == 0:
                n = struct.unpack_from(">H", payload, p + 7)[0]
                return payload[p + 9 : p + 9 + n].decode("ascii", "replace")
            p += 4 + elen
    except (IndexError, struct.error):
        return None
    return "(tls, no sni)"


def dns_name(payload):
    try:
        if len(payload) < 13:
            return None
        p, labels = 12, []
        while payload[p]:
            n = payload[p]
            labels.append(payload[p + 1 : p + 1 + n].decode("ascii", "replace"))
            p += 1 + n
        return ".".join(labels)
    except IndexError:
        return None


def main():
    args = sys.argv[1:]
    path = args.pop(0)
    t0 = None
    skip = set()
    while args:
        a = args.pop(0)
        if a == "--t0":
            hh, mm, ss = (int(x) for x in args.pop(0).split(":"))
            t0 = (hh, mm, ss)
        elif a == "--skip-host":
            skip.add(args.pop(0))
    with open(path, "rb") as f:
        data = f.read()
    magic = struct.unpack_from("<I", data, 0)[0]
    if magic not in (0xA1B2C3D4, 0xA1B23C4D):
        sys.exit("not a little-endian pcap file")
    linktype = struct.unpack_from("<I", data, 20)[0]
    off, first, rows, total, skipped = 24, None, OrderedDict(), 0, 0
    while off + 16 <= len(data):
        sec, usec, incl, _orig = struct.unpack_from("<IIII", data, off)
        pkt = data[off + 16 : off + 16 + incl]
        off += 16 + incl
        ts = sec + usec / 1e6
        if first is None:
            first = ts
            if t0:
                day = time.gmtime(sec)
                first = time.mktime((day.tm_year, day.tm_mon, day.tm_mday, *t0, 0, 0, 0)) - time.timezone
        ip = nflog_payload(pkt) if linktype == 239 else pkt
        if not ip:
            continue
        ver = ip[0] >> 4
        if ver == 4:
            ihl = (ip[0] & 15) * 4
            proto, dst = ip[9], socket.inet_ntoa(ip[16:20])
            l4 = ip[ihl:]
        elif ver == 6:
            proto, dst = ip[6], socket.inet_ntop(socket.AF_INET6, ip[24:40])
            l4 = ip[40:]
        else:
            continue
        port, names, payload = 0, [], b""
        if proto == 6 and len(l4) >= 20:
            port = struct.unpack_from(">H", l4, 2)[0]
            payload = l4[(l4[12] >> 4) * 4 :]
            if l4[13] & 0x12 == 0x02:  # SYN without ACK: the app opens a connection
                names.append("syn (new connection)")
            s = sni_of(payload)
            if s:
                names.append("sni " + s)
            elif payload[:4] in (b"GET ", b"POST", b"HEAD", b"CONN", b"PUT "):
                head = payload.split(b"\r\n\r\n")[0].decode("latin1").split("\r\n")
                host = next((h[6:] for h in head if h.lower().startswith("host: ")), "")
                names.append("http %s %s" % (host, head[0][:120]))
        elif proto == 17 and len(l4) >= 8:
            port = struct.unpack_from(">H", l4, 2)[0]
            payload = l4[8:]
            if port in (53, 5353):
                n = dns_name(payload)
                if n:
                    names.append("dns " + n)
            elif port == 443:
                names.append("quic/udp-443")
        key = "%s:%d" % (dst, port)
        if dst in skip or key in skip:
            skipped += 1
            continue
        total += 1
        r = rows.setdefault((dst, port, proto), {"n": 0, "bytes": 0, "t": ts - first, "names": OrderedDict()})
        r["n"] += 1
        r["bytes"] += len(ip)
        for n in names:
            r["names"].setdefault(n, round(ts - first, 1))
    print("%s: linktype %d, %d packets sent by the UID (%d more to skipped hosts)" % (path, linktype, total, skipped))
    for (dst, port, proto), r in rows.items():
        pname = {6: "tcp", 17: "udp", 1: "icmp", 58: "icmp6"}.get(proto, str(proto))
        print("%8.1fs  %-40s %-5s %5d pkts %8d bytes" % (r["t"], "%s:%d" % (dst, port), pname, r["n"], r["bytes"]))
        for n, t in r["names"].items():
            print("            %8.1fs  %s" % (t, n))


if __name__ == "__main__":
    main()
