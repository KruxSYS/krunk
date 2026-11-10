# Krunk VPN testbed images (test only; tools/vpn-testbed, never deployed or shipped). The base is
# pinned by digest; the packages are whatever Alpine 3.22 has (exact apk pins would break as soon
# as Alpine publishes a security update, and this image never leaves the test daemon).
FROM alpine:3.22@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f AS servers
# hadolint ignore=DL3018
RUN apk add --no-cache wireguard-tools-wg iproute2 openvpn openssl
COPY wg-entry.sh ovpn-entry.sh /usr/local/bin/

FROM alpine:3.22@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f AS inside
# hadolint ignore=DL3018
RUN apk add --no-cache socat dnsmasq iproute2
COPY inside-entry.sh /usr/local/bin/
