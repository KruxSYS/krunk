"""S2/T test server: python3 -I testserver.py PORT LOGFILE [UDP_PORT]
Logs every request (source IP, method, path, UA). Pages: /page, /ws (WebSocket echo), /dl.bin,
/sw.html + /sw.js, /rtc.html. A UDP sink on UDP_PORT logs every datagram's source."""
import asyncio, base64, hashlib, json, sys, time
PORT, LOG = int(sys.argv[1]), sys.argv[2]
UDP = int(sys.argv[3]) if len(sys.argv) > 3 else 0
TUN = "http://10.250.137.3:%d" % PORT
def log(**kw):
    kw["t"] = round(time.time(), 3)
    with open(LOG, "a") as f: f.write(json.dumps(kw) + "\n")
PAGES = {
 "/page": ("text/html", "<!doctype html><meta name=viewport content='width=device-width'><title>tun page</title><h1 id=h>page ok</h1><p id=src>SRC</p><img src='/img.png'>"),
 "/sw.html": ("text/html", """<!doctype html><meta name=viewport content='width=device-width'><title>sw</title><h1 id=s>sw: waiting</h1><script>
navigator.serviceWorker.register('/sw.js').then(()=>navigator.serviceWorker.ready).then(async()=>{
 if(!navigator.serviceWorker.controller){location.reload();return}
 const r=await fetch('/sw-probe'); document.getElementById('s').textContent='sw: '+await r.text();
}).catch(e=>document.getElementById('s').textContent='sw error '+e)</script>"""),
 "/sw.js": ("text/javascript", """self.addEventListener('install',e=>self.skipWaiting());
self.addEventListener('activate',e=>e.waitUntil(clients.claim()));
self.addEventListener('fetch',e=>{ if(new URL(e.request.url).pathname==='/sw-probe'){
 e.respondWith(fetch('""" + TUN + """/from-sw',{mode:'no-cors'}).then(r=>new Response('fetched type='+r.type)).catch(x=>new Response('failed '+x)));}});"""),
 "/ws.html": ("text/html", """<!doctype html><meta name=viewport content='width=device-width'><title>ws</title><h1 id=s>ws: waiting</h1><script>
const w=new WebSocket('ws://10.250.137.3:%d/ws');w.onopen=()=>w.send('hello');
w.onmessage=e=>document.getElementById('s').textContent='ws: echo '+e.data;w.onerror=e=>document.getElementById('s').textContent='ws: error'</script>""" % PORT),
 "/rtc.html": ("text/html", """<!doctype html><meta name=viewport content='width=device-width'><title>rtc</title><h1 id=s>rtc: gathering</h1><pre id=c></pre><script>
const out=[];try{const pc=new RTCPeerConnection({iceServers:[{urls:'stun:10.250.136.3:%d'}]});pc.createDataChannel('x');
pc.onicecandidate=e=>{if(e.candidate){out.push(e.candidate.candidate)}else{done()}};
pc.createOffer().then(o=>pc.setLocalDescription(o));setTimeout(done,8000)}catch(e){out.push('error '+e);done()}
let sent=0;function done(){if(sent++)return;document.getElementById('s').textContent='rtc: '+out.length+' candidates';document.getElementById('c').textContent=out.join('\\n');
fetch('/rtc-report?n='+out.length+'&c='+encodeURIComponent(out.join('|')).slice(0,1500))}</script>""" % (UDP or 3478)),
}
def ws_frame(data):
    return bytes([0x81, len(data)]) + data
async def handle(r, w):
    peer = w.get_extra_info("peername")[0]
    while True:
        try: head = await asyncio.wait_for(r.readuntil(b"\r\n\r\n"), 60)
        except Exception: break
        lines = head.decode("latin-1").split("\r\n"); method, path = lines[0].split(" ")[:2]
        h = {l.split(":",1)[0].lower(): l.split(":",1)[1].strip() for l in lines[1:] if ":" in l}
        log(src=peer, method=method, path=path[:300], host=h.get("host",""), ua=h.get("user-agent","")[:60])
        p = path.split("?")[0]
        if p == "/ws" and "sec-websocket-key" in h:
            acc = base64.b64encode(hashlib.sha1((h["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
            w.write(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n" % acc).encode()); await w.drain()
            hdr = await r.readexactly(2); n = hdr[1] & 0x7f; mask = await r.readexactly(4); data = bytes(b ^ mask[i % 4] for i, b in enumerate(await r.readexactly(n)))
            log(src=peer, ws_message=data.decode()); w.write(ws_frame(data)); await w.drain(); await asyncio.sleep(2); break
        if p in PAGES:
            ct, body = PAGES[p]; body = body.replace("SRC", "seen from " + peer).encode()
            extra = ""
        elif p == "/dl.bin":
            ct, body, extra = "application/octet-stream", b"x" * 4096, "Content-Disposition: attachment; filename=s2-download.bin\r\n"
        else:
            ct, body, extra = "text/plain", ("client=%s path=%s\n" % (peer, p)).encode(), ""
        w.write(("HTTP/1.1 200 OK\r\nContent-Type: %s\r\nContent-Length: %d\r\nCache-Control: no-store\r\n%s\r\n" % (ct, len(body), extra)).encode() + body)
        await w.drain()
    w.close()
class Udp(asyncio.DatagramProtocol):
    def datagram_received(self, d, a): log(udp_from=a[0], port=a[1], n=len(d))
async def main():
    if UDP: await asyncio.get_running_loop().create_datagram_endpoint(Udp, local_addr=("0.0.0.0", UDP))
    srv = await asyncio.start_server(handle, "0.0.0.0", PORT)
    async with srv: await srv.serve_forever()
asyncio.run(main())
