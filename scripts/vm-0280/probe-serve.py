"""Krunk 0.28.0 probe server (VM). Serves this folder's files, echoes request headers in order
at /headers, stores POST /report bodies as reports/<tag>-<role>.json and logs every request with
its headers to access.log. Usage: python probe-serve.py [port]"""
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

ROOT = os.path.dirname(os.path.abspath(__file__))
REPORTS = os.path.join(ROOT, "reports")
os.makedirs(REPORTS, exist_ok=True)
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript", ".mp4": "video/mp4", ".json": "application/json"}


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _log(self):
        with open(os.path.join(ROOT, "access.log"), "a", encoding="utf-8") as f:
            f.write(f"{time.strftime('%H:%M:%S')} {self.command} {self.path} {json.dumps(list(self.headers.items()))}\n")

    def _send(self, code, body, ctype="text/plain", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        for k, v in extra:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._log()
        u = urlparse(self.path)
        if u.path == "/headers":
            return self._send(200, json.dumps(list(self.headers.items())).encode(), "application/json")
        name = os.path.normpath(u.path.lstrip("/") or "probe.html")
        if name.startswith("..") or os.path.isabs(name):
            return self._send(403, b"no")
        path = os.path.join(ROOT, name)
        if not os.path.isfile(path):
            return self._send(404, b"not found")
        with open(path, "rb") as f:
            body = f.read()
        self._send(200, body, TYPES.get(os.path.splitext(path)[1], "application/octet-stream"))

    def do_POST(self):
        self._log()
        u = urlparse(self.path)
        q = parse_qs(u.query)
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        if u.path == "/report":
            tag = "".join(c for c in q.get("tag", ["x"])[0] if c.isalnum() or c in "-_")[:40]
            role = "".join(c for c in q.get("role", ["x"])[0] if c.isalnum())[:10]
            with open(os.path.join(REPORTS, f"{tag}-{role}.json"), "wb") as f:
                f.write(body)
        self._send(200, b"ok")


port = int(sys.argv[1]) if len(sys.argv) > 1 else 18111
ThreadingHTTPServer(("0.0.0.0", port), H).serve_forever()
