"""Test server for Krunk's downloads fetched in parts (VM only; scripts/vm-040-parts.ps1).

Serves the files in ROOT with Range support, a strong ETag and Last-Modified, one thread per
connection. Query options:
  norange=1     ignore Range (always 200 with the whole file)
  kbps=N        throttle every connection to N KB/s
  change=N      after N requests the file "changes" (another ETag: If-Range then gets 200)
  cookie=NAME   answer 403 unless the request has cookie NAME
  noetag=1      no ETag and no Last-Modified (no validator)
  gzip=1        claim Content-Encoding: gzip
Every request is logged to LOG: time, client port, path, Range, If-Range, Cookie, Referer.
"""
import email.utils
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

ROOT = sys.argv[1] if len(sys.argv) > 1 else r"C:\pipeline\krunk\www\8093"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8093
LOG = os.path.join(os.path.dirname(ROOT), "rangeserve.log")
lock = threading.Lock()
requests = {}


def log(line):
    with lock, open(LOG, "a", encoding="utf-8") as f:
        f.write(time.strftime("%H:%M:%S") + " " + line + "\n")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        url = urlsplit(self.path)
        q = {k: v[0] for k, v in parse_qs(url.query).items()}
        with lock:
            # Counted per address (path and query), so "change=N" counts this test's requests.
            requests[self.path] = requests.get(self.path, 0) + 1
            n = requests[self.path]
        log("port=%s %s range=[%s] if-range=[%s] cookie=[%s] referer=[%s] ua=[%s]" % (
            self.client_address[1], self.path, self.headers.get("Range", ""), self.headers.get("If-Range", ""),
            self.headers.get("Cookie", ""), self.headers.get("Referer", ""), (self.headers.get("User-Agent") or "")[:40]))
        name = os.path.basename(url.path)
        path = os.path.join(ROOT, name)
        if not name or not os.path.isfile(path):
            return self.reply(404, b"not found", {})
        if "cookie" in q and q["cookie"] + "=" not in (self.headers.get("Cookie") or ""):
            return self.reply(403, b"no cookie", {})
        size = os.path.getsize(path)
        mtime = os.path.getmtime(path)
        version = 2 if "change" in q and n > int(q["change"]) else 1
        etag = '"%x-%x-v%d"' % (size, int(mtime), version)
        if name.endswith(".html"):
            headers = {"Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store"}
        else:
            headers = {"Accept-Ranges": "bytes", "Content-Type": "application/octet-stream",
                       "Content-Disposition": 'attachment; filename="%s"' % name}
        if "noetag" not in q:
            headers["ETag"] = etag
            headers["Last-Modified"] = email.utils.formatdate(mtime + version, usegmt=True)
        if "gzip" in q:
            headers["Content-Encoding"] = "gzip"
        start, end, status = 0, size - 1, 200
        rng = self.headers.get("Range")
        if_range = self.headers.get("If-Range")
        if rng and "norange" not in q and (not if_range or if_range == etag or if_range == headers.get("Last-Modified")):
            spec = rng.replace("bytes=", "").split(",")[0]
            a, b = spec.split("-")
            start = int(a) if a else size - int(b)
            end = min(int(b), size - 1) if (a and b) else size - 1
            if start > end or start >= size:
                return self.reply(416, b"", {"Content-Range": "bytes */%d" % size})
            status = 206
            headers["Content-Range"] = "bytes %d-%d/%d" % (start, end, size)
        kbps = int(q.get("kbps", "0"))
        self.send_response(status)
        for k, v in headers.items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(end - start + 1))
        self.end_headers()
        with open(path, "rb") as f:
            f.seek(start)
            left = end - start + 1
            chunk = 16 * 1024
            t0 = time.time()
            sent = 0
            while left > 0:
                data = f.read(min(chunk, left))
                if not data:
                    break
                try:
                    self.wfile.write(data)
                except (ConnectionError, OSError):
                    log("port=%s closed by the client after %d bytes" % (self.client_address[1], sent))
                    return
                sent += len(data)
                left -= len(data)
                if kbps:
                    ahead = sent / (kbps * 1024.0) - (time.time() - t0)
                    if ahead > 0:
                        time.sleep(ahead)
        log("port=%s done %d bytes in %.2f s (status %d)" % (self.client_address[1], sent, time.time() - t0, status))

    def reply(self, status, body, headers):
        self.send_response(status)
        for k, v in headers.items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    ThreadingHTTPServer.daemon_threads = True
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
