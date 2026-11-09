#!/usr/bin/env python3
"""Krunk 0.10.0 (P1) VM helper, installed as C:\\pipeline\\krunk\\report-serve.py.

    python report-serve.py ROOT PORT LOGFILE

Serves the test pages in ROOT (threaded, keep-alive) and appends every `/m?<report>` request to
LOGFILE as `<unix ms> <decoded report>`, opening and closing the file per line so it can be read
while the server runs. Reports answer 204 with no-store. `/slow?mb=N&secs=S` sends N MB of
zeros spread over S seconds (a download that takes a while; P3). A minimal STUN server on UDP
PORT (binding requests only) gives WebRTC a candidate: Krunk's WebRTC policy gathers none without
one. `/stun-host` names the address to reach it at.
"""
import functools
import socket
import struct
import threading
import sys
import time
import urllib.parse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT, PORT, LOG = sys.argv[1], int(sys.argv[2]), sys.argv[3]


class Handler(SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path.startswith("/m?"):
            line = urllib.parse.unquote(self.path[3:])
            with open(LOG, "a", encoding="utf-8") as f:
                f.write(f"{int(time.time() * 1000)} {line}\n")
            self.send_response(204)
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if self.path == "/stun-host":
            body = outbound_ip().encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.startswith("/slow?"):
            q = urllib.parse.parse_qs(self.path[6:])
            mb = min(int(q.get("mb", ["20"])[0]), 200)
            secs = min(float(q.get("secs", ["20"])[0]), 120)
            chunk = b"\0" * (64 * 1024)
            count = mb * 16
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Disposition", 'attachment; filename="spike-slow.bin"')
            self.send_header("Content-Length", str(count * len(chunk)))
            self.end_headers()
            for _ in range(count):
                self.wfile.write(chunk)
                time.sleep(secs / count)
            return
        super().do_GET()

    def log_message(self, *args):
        pass


def outbound_ip():
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.connect(("8.8.8.8", 53))
        return s.getsockname()[0]


def stun(port):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    # Only the address the pages are told to use (/stun-host): the test machine's own.
    sock.bind((outbound_ip(), port))
    cookie = 0x2112A442
    while True:
        data, (ip, sport) = sock.recvfrom(2048)
        if len(data) < 20 or struct.unpack("!HHI", data[:8])[0] != 0x0001 or struct.unpack("!I", data[4:8])[0] != cookie:
            continue
        xaddr = struct.pack("!BBH4s", 0, 1, sport ^ (cookie >> 16),
                            bytes(a ^ b for a, b in zip(socket.inet_aton(ip), struct.pack("!I", cookie))))
        attr = struct.pack("!HH", 0x0020, len(xaddr)) + xaddr
        sock.sendto(struct.pack("!HHI", 0x0101, len(attr), cookie) + data[8:20] + attr, (ip, sport))


threading.Thread(target=stun, args=(PORT,), daemon=True).start()
ThreadingHTTPServer.daemon_threads = True
ThreadingHTTPServer(("0.0.0.0", PORT), functools.partial(Handler, directory=ROOT)).serve_forever()
