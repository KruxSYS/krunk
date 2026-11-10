# Krunk VPN testbed images (test only; tools/vpn-testbed). Base pinned by digest; packages come from Alpine 3.22.
FROM alpine:3.22@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f AS servers
RUN apk add --no-cache wireguard-tools-wg iproute2 openvpn openssl
COPY wg-entry.sh ovpn-entry.sh /usr/local/bin/

FROM alpine:3.22@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f AS inside
RUN apk add --no-cache socat dnsmasq iproute2
COPY inside-entry.sh /usr/local/bin/
