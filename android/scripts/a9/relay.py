"""Plain TCP or UDP port forwarder for the device tests (the emulator reaches this sandbox at
10.250.136.3, the VPN testbed listens on the dind daemon).
    python3 -I relay.py tcp|udp LISTEN_PORT TARGET_HOST TARGET_PORT [LOGFILE]
UDP: one upstream socket per client address (WireGuard needs a stable 5-tuple). With LOGFILE every
TCP connection's source address is logged (one JSON line), so a test can tell tunnel from direct."""
import asyncio, json, socket, sys, time

proto, listen, host, port = sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4])
LOG = sys.argv[5] if len(sys.argv) > 5 else None


async def pipe(r, w):
    try:
        while d := await r.read(65536):
            w.write(d)
            await w.drain()
    except Exception:
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass


async def tcp_client(cr, cw):
    if LOG:
        with open(LOG, "a") as f:
            f.write(json.dumps({"t": round(time.time(), 3), "relay": listen, "src": cw.get_extra_info("peername")[0]}) + "\n")
    try:
        ur, uw = await asyncio.open_connection(host, port)
    except OSError:
        cw.close()
        return
    await asyncio.gather(pipe(cr, uw), pipe(ur, cw))


class Up(asyncio.DatagramProtocol):
    def __init__(self, down, client):
        self.down, self.client = down, client

    def datagram_received(self, data, _):
        self.down.sendto(data, self.client)


class Down(asyncio.DatagramProtocol):
    def __init__(self):
        self.ups = {}

    def connection_made(self, transport):
        self.transport = transport

    def datagram_received(self, data, client):
        up = self.ups.get(client)
        if up is None:
            loop = asyncio.get_running_loop()
            target = socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)[0][4]
            up = loop.create_task(loop.create_datagram_endpoint(lambda: Up(self.transport, client), remote_addr=target))
            self.ups[client] = up
            up.add_done_callback(lambda t: t.result()[0].sendto(data))
            return
        if up.done():
            up.result()[0].sendto(data)
        else:
            up.add_done_callback(lambda t: t.result()[0].sendto(data))


async def main():
    if proto == "udp":
        await asyncio.get_running_loop().create_datagram_endpoint(Down, local_addr=("0.0.0.0", listen))
        await asyncio.Event().wait()
    srv = await asyncio.start_server(tcp_client, "0.0.0.0", listen)
    async with srv:
        await srv.serve_forever()


asyncio.run(main())
