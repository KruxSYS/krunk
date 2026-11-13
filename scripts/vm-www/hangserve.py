#!/usr/bin/env python3
"""Krunk 0.18.0: a server whose responses hang on purpose, for the stuck-loads measurements
(docs/NETWORK.md). Standard library only; one thread per connection; HTTP/1.1 with keep-alive.

    python hangserve.py [--port 8120] [--bind 127.0.0.1] [--log hangserve.log]

Paths (every one takes any extra query, so `&i=3` makes a distinct URL):
  /hang                 the request is read, nothing is ever answered (until the client closes)
  /slow-headers?s=N     the answer (headers and body) comes after N seconds
  /slow-body?s=N        headers and the first bytes at once, the rest after N seconds
  /sse                  text/event-stream, one event a second, for ever
  /ok                   a small answer at once, `Cache-Control: max-age=600`
  /nocache              a small answer at once, `Cache-Control: no-store`
  /page?...             an HTML page that fetches things and reports in document.title:
                          u=<url>      what to fetch (repeatable; relative or absolute)
                          n=<count>    fetch each `u` that many times (`&i=<k>` is appended
                                       unless same=1, then all go to the very same URL)
                          then=<url>   fetched after the others were started (the "seventh")
                          as=fetch|img how (default fetch)
                        title: "pending <k>" while waiting, then "done <ms> then=<ms|pending>"
  /log                  the access log so far as JSON (and /log?clear=1 empties it)
/slow-headers and /slow-body carry no cache headers; add `cache=1` for
`Cache-Control: max-age=600` or `nostore=1` for `Cache-Control: no-store`.

Access log: one line when a request arrives ("+"), one when its answer ended or the client went
away ("-"), each with seconds since the start, the client's port (= which connection) and the
path. The same lines go to stdout and to --log.
"""
import argparse
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

START = time.monotonic()
LOCK = threading.Lock()
EVENTS = []
LOGFILE = None

PAGE = """<!doctype html><meta charset="utf-8"><title>pending</title><body><pre id="o"></pre><script>
(function () {
  var q = new URLSearchParams(location.search), us = q.getAll('u'), n = +(q.get('n') || 1), then = q.get('then');
  var same = q.get('same') === '1', how = q.get('as') || 'fetch', t0 = performance.now(), left = 0, done = null, thenMs = 'none';
  function ms() { return Math.round(performance.now() - t0); }
  function show() { document.title = (left ? 'pending ' + left : 'done ' + done) + ' then=' + thenMs;
    document.getElementById('o').textContent = document.title; }
  function get(u, cb) {
    if (how === 'img') { var i = new Image(); i.onload = i.onerror = cb; i.src = u; document.body.appendChild(i); return; }
    fetch(u).then(function (r) { return r.text(); }).then(cb, cb);
  }
  us.forEach(function (u) {
    for (var k = 0; k < n; k++) {
      left++;
      get(same ? u : u + (u.indexOf('?') < 0 ? '?' : '&') + 'i=' + k, function () { left--; if (!left) done = ms(); show(); });
    }
  });
  if (then) { thenMs = 'pending'; get(then, function () { thenMs = ms(); show(); }); }
  if (!left) done = 0;
  show();
})();
</script>"""


def log(sign, handler, extra=""):
    t = round(time.monotonic() - START, 3)
    port = handler.client_address[1]
    line = "%9.3f %s conn %-5d %s%s" % (t, sign, port, handler.path, (" " + extra) if extra else "")
    with LOCK:
        EVENTS.append({"t": t, "ev": sign, "conn": port, "path": handler.path, "note": extra})
        print(line, flush=True)
        if LOGFILE:
            LOGFILE.write(line + "\n")
            LOGFILE.flush()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # the access log above replaces the default one
        pass

    def gone(self, seconds):
        """Wait up to `seconds` (None: for ever); True when the client closed meanwhile."""
        end = None if seconds is None else time.monotonic() + seconds
        self.connection.settimeout(0.25)
        try:
            while end is None or time.monotonic() < end:
                try:
                    if self.connection.recv(1, 2) == b"":  # MSG_PEEK
                        return True
                    time.sleep(0.25)  # pipelined bytes: leave them
                except (TimeoutError, BlockingIOError):
                    pass
                except OSError:
                    return True
            return False
        finally:
            self.connection.settimeout(None)

    def send(self, body, ctype="text/plain; charset=utf-8", cache=None, split=None):
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        if cache:
            self.send_header("Cache-Control", cache)
        self.end_headers()
        if split is None:
            self.wfile.write(body)
            return False
        self.wfile.write(body[:1])
        self.wfile.flush()
        if self.gone(split):
            return True
        self.wfile.write(body[1:])
        return False

    def do_GET(self):
        url = urlsplit(self.path)
        q = parse_qs(url.query)
        secs = float(q.get("s", ["10"])[0])
        cache = "max-age=600" if q.get("cache") == ["1"] else ("no-store" if q.get("nostore") == ["1"] else None)
        log("+", self)
        note = ""
        try:
            if url.path == "/hang":
                self.gone(None)
                note = "client closed"
                self.close_connection = True
            elif url.path == "/slow-headers":
                if self.gone(secs):
                    note, self.close_connection = "client closed", True
                else:
                    self.send(b"slow headers " + b"x" * 2000 + b"\n", cache=cache)
            elif url.path == "/slow-body":
                if self.send(b"slow body " + b"x" * 2000 + b"\n", cache=cache, split=secs):
                    note, self.close_connection = "client closed", True
            elif url.path == "/sse":
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Cache-Control", "no-store")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.send_header("Connection", "close")
                self.end_headers()
                self.close_connection = True
                n = 0
                while not self.gone(1):
                    n += 1
                    self.wfile.write(("data: %d\n\n" % n).encode())
                    self.wfile.flush()
                note = "client closed"
            elif url.path == "/ok":
                self.send(b"ok\n", cache="max-age=600")
            elif url.path == "/nocache":
                self.send(b"nocache\n", cache="no-store")
            elif url.path == "/page":
                self.send(PAGE.encode(), ctype="text/html; charset=utf-8", cache="no-store")
            elif url.path == "/log":
                with LOCK:
                    body = json.dumps(EVENTS).encode()
                    if q.get("clear") == ["1"]:
                        del EVENTS[:]
                self.send(body, ctype="application/json", cache="no-store")
                return
            else:
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            note, self.close_connection = "client closed", True
        log("-", self, note)


class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 128

    def handle_error(self, request, client_address):  # resets are this server's daily bread
        pass


def main():
    global LOGFILE
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--port", type=int, default=8120)
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--log", default="")
    a = ap.parse_args()
    if a.log:
        LOGFILE = open(a.log, "a", encoding="utf-8")
    srv = Server((a.bind, a.port), Handler)
    print("hangserve on %s:%d" % (a.bind, a.port), flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
