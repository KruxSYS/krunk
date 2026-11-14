#!/usr/bin/env python3
"""Test server for Krunk for Android 0.4.0 (downloads, permissions, site data).
Serves ./www and logs every request with what matters for the checks (cookie seen, Range,
If-Range), one line each.

  /login                 sets the cookie "krunk_session=ok", then shows the download links
  /logout                removes it
  /private/<name>        a file only for signed-in requests; without the cookie the answer is
                         a sign-in page (200, text/html): what a second fetch without cookies saves
  /file/<name>?size=N    N bytes of fixed content (default 300000), as an attachment. Options:
        rate=K           K KiB per second (default: as fast as it goes)
        ranges=0         no Range support (always the whole file, Accept-Ranges: none)
        etag=0           no ETag and no Last-Modified (cannot be continued)
        nolen=1          no Content-Length (chunked): size unknown
        cut=M            the first request closes the connection after M bytes (once per name)
        v=2              another version of the file (other bytes, other ETag)
        name=...         the Content-Disposition file name (default: <name>)
  /post-file             POST only: answers with an attachment
  /sha?size=N[&v=2]      the SHA-256 of that content, as text
Usage: python3 -I server.py <www-dir> [port]      |      python3 -I server.py --sha N [v]
"""
import hashlib
import mimetypes
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


def content(size, v="1"):
    """size bytes that differ at every offset block (so a wrong splice changes the hash)."""
    out = bytearray()
    n = 0
    while len(out) < size:
        out += hashlib.sha256(b"krunk-a5-%s-%d" % (v.encode(), n)).digest()
        n += 1
    return bytes(out[:size])


if len(sys.argv) > 1 and sys.argv[1] == "--sha":
    print(hashlib.sha256(content(int(sys.argv[2]), sys.argv[3] if len(sys.argv) > 3 else "1")).hexdigest())
    sys.exit(0)

WWW = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 18080
CUT_DONE = set()
VIEWPORT = "<meta name=viewport content='width=device-width'>"
LINKS = (VIEWPORT + "<h2 id=in>Signed in</h2><p><a id=dl href='/private/report.bin'>Download the report</a></p>"
         "<p><a href='/logout'>Sign out</a></p>").encode()
SIGNIN = (VIEWPORT + "<h2 id=out>Please sign in</h2><p>This file is for members.</p>"
          "<p><a href='/login'>Sign in</a></p>").encode()


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def _log(self, note=""):
        h = self.headers
        print(time.strftime("%H:%M:%S"), self.command, self.path,
              "cookie=%s" % ("yes" if "krunk_session=ok" in (h.get("Cookie") or "") else "no"),
              "range=%s" % (h.get("Range") or "-"), "if-range=%s" % (h.get("If-Range") or "-"),
              "referer=%s" % (h.get("Referer") or "-"), note, flush=True)

    def _send(self, code, body, ctype="text/html; charset=utf-8", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _file(self, name, q):
        size = int(q.get("size", ["300000"])[0])
        v = q.get("v", ["1"])[0]
        rate = int(q.get("rate", ["0"])[0]) * 1024
        ranges = q.get("ranges", ["1"])[0] != "0"
        validators = q.get("etag", ["1"])[0] != "0"
        nolen = q.get("nolen", ["0"])[0] == "1"
        cut = int(q.get("cut", ["0"])[0])
        data = content(size, v)
        etag = '"a5-%s-%d"' % (v, size)
        start, code = 0, 200
        rng = self.headers.get("Range")
        if_range = self.headers.get("If-Range")
        if ranges and rng and rng.startswith("bytes=") and (if_range is None or (validators and if_range == etag)):
            try:
                start = int(rng[6:].split("-")[0])
            except ValueError:
                start = 0
            if start >= size:
                self.send_response(416)
                self.send_header("Content-Range", "bytes */%d" % size)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            code = 206
        body = data[start:]
        self.send_response(code)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", 'attachment; filename="%s"' % q.get("name", [name])[0])
        self.send_header("Cache-Control", "no-store")
        self.send_header("Accept-Ranges", "bytes" if ranges else "none")
        if validators:
            self.send_header("ETag", etag)
            self.send_header("Last-Modified", "Sat, 14 Nov 2026 00:00:00 GMT")
        if code == 206:
            self.send_header("Content-Range", "bytes %d-%d/%d" % (start, size - 1, size))
        if nolen:
            self.send_header("Transfer-Encoding", "chunked")
        else:
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command == "HEAD":
            return
        limit = len(body)
        key = (name, v)
        if cut and key not in CUT_DONE and code == 200:
            CUT_DONE.add(key)
            limit = min(cut, limit)
        sent = 0
        step = 16 * 1024
        try:
            while sent < limit:
                chunk = body[sent:min(sent + step, limit)]
                if nolen:
                    self.wfile.write(b"%x\r\n" % len(chunk) + chunk + b"\r\n")
                else:
                    self.wfile.write(chunk)
                self.wfile.flush()
                sent += len(chunk)
                if rate:
                    time.sleep(len(chunk) / rate)
            if limit < len(body):
                print(time.strftime("%H:%M:%S"), "CUT", name, "after", start + sent, flush=True)
                self.close_connection = True
                self.connection.shutdown(2)
                return
            if nolen:
                self.wfile.write(b"0\r\n\r\n")
            print(time.strftime("%H:%M:%S"), "DONE", name, "%d-%d" % (start, start + sent), flush=True)
        except OSError:
            print(time.strftime("%H:%M:%S"), "CLOSED", name, "by the client after", start + sent, flush=True)
            self.close_connection = True

    def _handle(self):
        u = urlsplit(self.path)
        q = parse_qs(u.query)
        signed_in = "krunk_session=ok" in (self.headers.get("Cookie") or "")
        self._log()
        if u.path == "/login":
            return self._send(200, LINKS, extra=[("Set-Cookie", "krunk_session=ok; Path=/; Max-Age=86400; HttpOnly")])
        if u.path == "/logout":
            return self._send(200, SIGNIN, extra=[("Set-Cookie", "krunk_session=; Path=/; Max-Age=0")])
        if u.path.startswith("/private/"):
            if not signed_in:
                return self._send(200, SIGNIN)
            return self._file(u.path.rsplit("/", 1)[1], q)
        if u.path.startswith("/file/"):
            return self._file(u.path.rsplit("/", 1)[1], q)
        if u.path == "/post-file":
            n = int(self.headers.get("Content-Length") or 0)
            if n:
                self.rfile.read(n)
            if self.command != "POST":
                return self._send(405, b"POST only", "text/plain")
            return self._send(200, content(5000, "post"), "application/octet-stream",
                              extra=[("Content-Disposition", 'attachment; filename="posted.bin"')])
        if u.path == "/sha":
            return self._send(200, hashlib.sha256(content(int(q.get("size", ["300000"])[0]), q.get("v", ["1"])[0])).hexdigest().encode(), "text/plain")
        rel = u.path.lstrip("/") or "index.html"
        full = os.path.abspath(os.path.join(WWW, rel))
        if full.startswith(WWW + os.sep) and os.path.isfile(full):
            with open(full, "rb") as f:
                data = f.read()
            return self._send(200, data, mimetypes.guess_type(full)[0] or "application/octet-stream")
        return self._send(404, b"not found", "text/plain")

    do_GET = _handle
    do_POST = _handle
    do_HEAD = _handle


ThreadingHTTPServer(("0.0.0.0", PORT), H).serve_forever()
