"""VM helper (Krunk 0.11.0): an HTTP server on 127.0.0.1:8104 whose pages answer after 30 s
(a slow site), each request in its own thread: python slowserve.py [port] [delay]."""
import http.server
import socketserver
import sys
import time

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8104
DELAY = float(sys.argv[2]) if len(sys.argv) > 2 else 30


class Slow(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        time.sleep(DELAY)
        body = b"<!doctype html><title>Slow page</title><h1>Finally loaded after %d s</h1>" % DELAY
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


Server(("127.0.0.1", PORT), Slow).serve_forever()
