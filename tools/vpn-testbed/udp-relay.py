#!/usr/bin/env python3
"""UDP over one TCP connection, for the Windows VM tests only (the VM reaches the testbed on
dind only through `ssh -R`, which carries TCP). Datagrams are framed as a 2-byte big-endian
length plus the payload.

  VM side:    udp-relay.py client --udp 127.0.0.1:51820 --tcp 127.0.0.1:18551
              (Krunk's WireGuard profile uses Endpoint = 127.0.0.1:51820; replies go to the
              address the last datagram came from)
  Linux side: udp-relay.py server --tcp 127.0.0.1:18551 --udp dind:8551
              (with `ssh -R 18551:127.0.0.1:18551 windows`)
"""
import argparse
import select
import socket
import struct


def addr(s):
    host, port = s.rsplit(":", 1)
    return host, int(port)


def read_exact(conn, n):
    buf = b""
    while len(buf) < n:
        part = conn.recv(n - len(buf))
        if not part:
            raise ConnectionError("tcp closed")
        buf += part
    return buf


def pump(tcp, udp, send_udp):
    while True:
        ready, _, _ = select.select([tcp, udp], [], [])
        if udp in ready:
            data, src = udp.recvfrom(65535)
            send_udp.update(src)
            tcp.sendall(struct.pack(">H", len(data)) + data)
        if tcp in ready:
            (n,) = struct.unpack(">H", read_exact(tcp, 2))
            data = read_exact(tcp, n)
            dst = send_udp.target()
            if dst:
                udp.sendto(data, dst)


class Peer:
    """Where UDP replies go: fixed (server side) or the last sender (client side)."""

    def __init__(self, fixed=None):
        self.fixed, self.last = fixed, None

    def update(self, src):
        if not self.fixed:
            self.last = src

    def target(self):
        return self.fixed or self.last


def main():
    p = argparse.ArgumentParser()
    p.add_argument("mode", choices=["client", "server"])
    p.add_argument("--udp", required=True)
    p.add_argument("--tcp", required=True)
    a = p.parse_args()
    if a.mode == "client":
        udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        udp.bind(addr(a.udp))
        while True:
            try:
                tcp = socket.create_connection(addr(a.tcp))
                pump(tcp, udp, Peer())
            except (ConnectionError, OSError) as e:
                print("client:", e, flush=True)
                select.select([], [], [], 1)
    else:
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(addr(a.tcp))
        srv.listen(1)
        target = socket.getaddrinfo(*addr(a.udp), socket.AF_INET, socket.SOCK_DGRAM)[0][4]
        while True:
            tcp, _ = srv.accept()
            udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            try:
                pump(tcp, udp, Peer(target))
            except (ConnectionError, OSError) as e:
                print("server:", e, flush=True)
            finally:
                udp.close()
                tcp.close()


if __name__ == "__main__":
    main()
