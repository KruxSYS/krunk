"""Basic-auth test server for the Krunk 0.15.0 VM pass: 401 + WWW-Authenticate until it gets
krunk:test. Usage: python authserve.py PORT"""
import base64, http.server, sys

GOOD = "Basic " + base64.b64encode(b"krunk:test").decode()

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.headers.get("Authorization") == GOOD:
            body = b"<!doctype html><title>Signed in</title><h1>Signed in</h1>"
            self.send_response(200)
        else:
            body = b"<!doctype html><title>401</title><h1>401: sign in needed (server page)</h1>"
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="Krunk test"')
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass

http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
