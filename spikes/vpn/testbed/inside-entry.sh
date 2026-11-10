#!/bin/sh
# "Inside" network services (reachable only through the tunnels):
#   inside-entry.sh echo  - HTTP on :80 that answers "client=<source IP>" (10.77.0.10)
#   inside-entry.sh dns   - dnsmasq answering inside.test -> 10.77.0.10 (10.77.0.53)
# Replies for the tunnel subnets go back via the servers (no NAT, so the source IP is visible).
set -eu
ip route replace 10.66.0.0/24 via 10.77.0.2
ip route replace 10.88.0.0/24 via 10.77.0.3
ip route replace 10.89.0.0/24 via 10.77.0.3
case "${1:-echo}" in
dns)
  # --interface also lifts dnsmasq's default "local subnets only" rule (tunnel clients are remote).
  exec dnsmasq --keep-in-foreground --no-resolv --no-hosts --interface=eth0 \
    --address=/inside.test/10.77.0.10 --local=/inside.test/ \
    --log-queries --log-facility=- ;;
*)
  # One process per connection: read the request head, then answer "client=<peer IP>"
  # (socat exports the peer as SOCAT_PEERADDR).
  cat > /usr/local/bin/echo-ip <<'SH'
#!/bin/sh
while read -r l && [ "$(printf %s "$l" | tr -d '\r')" != "" ]; do :; done
b="client=$SOCAT_PEERADDR"
printf 'HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: %s\r\n\r\n%s\n' $((${#b} + 1)) "$b"
SH
  chmod 755 /usr/local/bin/echo-ip
  exec socat TCP-LISTEN:80,fork,reuseaddr EXEC:/usr/local/bin/echo-ip ;;
esac
