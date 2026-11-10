#!/bin/sh
# Kernel WireGuard server on 51820/udp (published as 8551). Keys are generated on first start into /state/wg (a volume, never
# committed); the client config is /state/wg/client.conf (Endpoint = dind:8551).
set -eu
d=/state/wg; mkdir -p "$d"; umask 077
if [ ! -f "$d/server.key" ]; then
  wg genkey > "$d/server.key"; wg pubkey < "$d/server.key" > "$d/server.pub"
  wg genkey > "$d/client.key"; wg pubkey < "$d/client.key" > "$d/client.pub"
  cat > "$d/client.conf" <<CONF
[Interface]
PrivateKey = $(cat "$d/client.key")
Address = 10.66.0.2/32
DNS = 10.77.0.53

[Peer]
PublicKey = $(cat "$d/server.pub")
AllowedIPs = 10.77.0.0/24
Endpoint = ${WG_ENDPOINT:-dind:8551}
PersistentKeepalive = 25
CONF
fi
# A full-tunnel profile too (0.14.1): every address through the VPN, public names through 1.1.1.1
# inside it, like a VPN provider's profile. The server NATs that traffic out to the internet.
if [ ! -f "$d/client-full.conf" ]; then
  sed -e 's#^AllowedIPs = .*#AllowedIPs = 0.0.0.0/0, ::/0#' -e 's#^DNS = .*#DNS = 1.1.1.1#' \
    "$d/client.conf" > "$d/client-full.conf"
fi
ip link add wg0 type wireguard
wg set wg0 listen-port 51820 private-key "$d/server.key" \
  peer "$(cat "$d/client.pub")" allowed-ips 10.66.0.2/32
ip addr add 10.66.0.1/24 dev wg0
ip link set wg0 up
echo "wg server up: $(cat "$d/server.pub")"
iptables -t nat -A POSTROUTING -s 10.66.0.0/24 ! -d 10.64.0.0/11 -j MASQUERADE
# Forwarding wg0 <-> inside network without NAT, so the inside server sees 10.66.0.2.
while sleep 30; do wg show wg0 latest-handshakes; done
