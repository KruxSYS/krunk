#!/usr/bin/env python3
"""Spike test server (Android 0.2.0, M1). Serves ./www, logs every request line with its
query (the proof of what reached the network), and has a few endpoints:
  /redir            302 to /landing.html with tracking parameters
  /post             echoes method and URL (POST must arrive untouched)
Anything else under www/ is served as a file; unknown paths answer 200 with a tiny body so
"blocked" is never confused with "missing".
Usage: python3 -I server.py <www-dir> [port]
"""
import sys, os, time, mimetypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

WWW = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 18080
GIF = bytes.fromhex("47494638396101000100800000000000ffffff21f90401000000002c00000000010001000002024401003b")


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def _send(self, code, body, ctype="text/html; charset=utf-8", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def _handle(self):
        print(time.strftime("%H:%M:%S"), self.command, self.path, flush=True)
        u = urlsplit(self.path)
        if u.path == "/redir":
            return self._send(302, b"", extra=[("Location", "/landing.html?utm_campaign=y&keep=1&gclid=zz")])
        if u.path == "/post":
            n = int(self.headers.get("Content-Length") or 0)
            data = self.rfile.read(n) if n else b""
            body = ("<meta name=viewport content='width=device-width'><h2>%s</h2><p id=u>%s</p><p>body: %s</p>"
                    % (self.command, self.path, data.decode("utf-8", "replace"))).encode()
            return self._send(200, body)
        rel = u.path.lstrip("/") or "index.html"
        full = os.path.abspath(os.path.join(WWW, rel))
        if full.startswith(WWW + os.sep) and os.path.isfile(full):
            with open(full, "rb") as f:
                data = f.read()
            return self._send(200, data, mimetypes.guess_type(full)[0] or "application/octet-stream")
        if u.path.endswith((".gif", ".png")) or ".gif" in u.path:
            return self._send(200, GIF, "image/gif")
        if u.path.endswith(".js") or ".js" in u.path:
            return self._send(200, b"window.__loaded=(window.__loaded||0)+1;", "text/javascript")
        return self._send(200, b"ok", "text/plain")

    do_GET = _handle
    do_POST = _handle


ThreadingHTTPServer(("0.0.0.0", PORT), H).serve_forever()
