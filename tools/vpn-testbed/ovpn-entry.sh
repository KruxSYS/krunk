#!/bin/sh
# OpenVPN 2.6 servers on UDP 1194 (published as 8194) (10.88.0.0/24) and TCP 1194 (10.89.0.0/24). A throw-away test
# PKI is generated on first start into /state/ovpn (a volume, never committed). Auth: CA +
# tls-crypt + username/password (krunk / krunk-test), no client certificate, like most providers.
set -eu
d=/state/ovpn; mkdir -p "$d"; cd "$d"; umask 077
if [ ! -f ca.crt ]; then
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 \
    -subj "/CN=Krunk Test CA" -keyout ca.key -out ca.crt 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj "/CN=krunk-test-server" \
    -keyout server.key -out server.csr 2>/dev/null
  printf 'basicConstraints=CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth\n' > ext.cnf
  openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 30 \
    -extfile ext.cnf -out server.crt 2>/dev/null
  openvpn --genkey tls-crypt tc.key
  printf 'krunk:krunk-test\n' > users
  for proto in udp tcp; do
    cat > "client-$proto.ovpn" <<CONF
client
dev tun
proto $proto
remote ${OVPN_REMOTE:-dind} ${OVPN_REMOTE_PORT:-8194}
nobind
auth-user-pass
remote-cert-tls server
verify-x509-name krunk-test-server name
data-ciphers AES-256-GCM:CHACHA20-POLY1305
verb 3
<ca>
$(cat ca.crt)
</ca>
<tls-crypt>
$(cat tc.key)
</tls-crypt>
CONF
  done
  # The same TCP profile with a CA that did not sign the server: Krunk must report a
  # certificate failure, not loop in RECONNECTING.
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 \
    -subj "/CN=Krunk Unrelated CA" -keyout other-ca.key -out other-ca.crt 2>/dev/null
  awk -v ca="$(cat other-ca.crt)" '/^<ca>$/{print; print ca; skip=1; next} /^<\/ca>$/{skip=0} !skip' \
    client-tcp.ovpn > client-tcp-badca.ovpn
fi
cat > /usr/local/bin/ovpn-auth <<'SH'
#!/bin/sh
# via-file: $1 holds username and password lines.
u=$(sed -n 1p "$1"); p=$(sed -n 2p "$1")
grep -qx "$u:$p" /state/ovpn/users
SH
chmod 755 /usr/local/bin/ovpn-auth
cat > common.conf <<'CONF'
dev-type tun
topology subnet
ca ca.crt
cert server.crt
key server.key
dh none
tls-crypt tc.key
verify-client-cert none
username-as-common-name
auth-user-pass-verify /usr/local/bin/ovpn-auth via-file
script-security 2
data-ciphers AES-256-GCM:CHACHA20-POLY1305
keepalive 10 60
verb 3
push "route 10.77.0.0 255.255.255.0"
push "dhcp-option DNS 10.77.0.53"
CONF
openvpn --cd "$d" --config common.conf --proto udp4 --port 1194 --dev tun-udp --server 10.88.0.0 255.255.255.0 &
openvpn --cd "$d" --config common.conf --proto tcp4-server --port 1194 --dev tun-tcp --server 10.89.0.0 255.255.255.0 &
wait
