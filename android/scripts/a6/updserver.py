#!/usr/bin/env python3
"""The update server the QA build talks to (Krunk for Android 0.5.0, a6.sh).

    updserver.py PORT --proxy http://dind:PORT     forward every GET to the staging site
    updserver.py PORT --root DIR                   serve DIR (the tampered fixtures)

Every request is logged on stdout as one line: time, client, method, path, status, bytes,
User-Agent. That log is the evidence for "no request until the user taps Check" (9e). The mode
can be switched while running by writing "proxy URL" or "root DIR" into the file named by
--control (re-read at every request), so the device keeps one address.
"""
import argparse
import http.server
import os
import socketserver
import sys
import time
import urllib.error
import urllib.request

ARGS = None


def mode():
    if ARGS.control and os.path.exists(ARGS.control):
        with open(ARGS.control, encoding="utf-8") as f:
            kind, _, value = f.read().strip().partition(" ")
            if kind in ("proxy", "root") and value:
                return kind, value
    return ("proxy", ARGS.proxy) if ARGS.proxy else ("root", ARGS.root)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def note(self, status, size):
        print(
            "%s %s %s %s %d %d %r"
            % (time.strftime("%H:%M:%S"), self.client_address[0], self.command, self.path, status, size,
               self.headers.get("User-Agent", "")),
            flush=True,
        )

    def send(self, status, body, ctype):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)
        self.note(status, len(body))

    def stream_file(self, path):
        size = os.path.getsize(path)
        ctype = "application/json" if path.endswith(".json") else (
            "application/vnd.android.package-archive" if path.endswith(".apk") else "text/plain; charset=utf-8")
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(size))
        self.end_headers()
        with open(path, "rb") as f:
            while True:
                chunk = f.read(1 << 16)
                if not chunk:
                    break
                self.wfile.write(chunk)
                if ARGS.slow:
                    time.sleep(ARGS.slow)
        self.note(200, size)

    def do_GET(self):
        kind, value = mode()
        path = self.path.split("?", 1)[0]
        if ".." in path:
            return self.send(400, b"bad path", "text/plain")
        if kind == "root":
            local = os.path.join(value, path.lstrip("/"))
            if os.path.isfile(local):
                return self.stream_file(local)
            return self.send(404, b"404 Not Found", "text/plain")
        url = value.rstrip("/") + path
        req = urllib.request.Request(url, headers={"User-Agent": self.headers.get("User-Agent", "")})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                status, ctype, size = r.status, r.headers.get("Content-Type", "application/octet-stream"), r.headers.get("Content-Length")
                self.send_response(status)
                self.send_header("Content-Type", ctype)
                if size:
                    self.send_header("Content-Length", size)
                self.end_headers()
                n = 0
                while True:
                    chunk = r.read(1 << 16)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    n += len(chunk)
                self.note(status, n)
        except urllib.error.HTTPError as e:
            self.send(e.code, ("%d" % e.code).encode(), "text/plain")
        except Exception as e:  # noqa: BLE001 - logged and answered
            self.send(502, str(e).encode(), "text/plain")

    do_HEAD = do_GET


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main():
    global ARGS
    p = argparse.ArgumentParser()
    p.add_argument("port", type=int)
    p.add_argument("--proxy")
    p.add_argument("--root")
    p.add_argument("--control")
    p.add_argument("--slow", type=float, default=0.0, help="seconds to wait after each 64 KiB (cancel tests)")
    ARGS = p.parse_args()
    if not (ARGS.proxy or ARGS.root):
        sys.exit("give --proxy URL or --root DIR")
    print("updserver on :%d %s" % (ARGS.port, mode()), flush=True)
    Server(("0.0.0.0", ARGS.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
