# `vpn`: Krunk's in-browser VPN (0.14.0)

Package `vpn`, library `krunk_vpn`. WireGuard and OpenVPN profiles, for **Krunk's own traffic
only**: no network adapter, no routes, no admin rights, other programs are not affected. The
contract with the app is `.pipeline/vpn-contract-0.14.0.md`.

## Architecture

```
engine / WinHTTP ──HTTP proxy──▶ proxy (127.0.0.1:random) ──▶ net (smoltcp TCP/UDP) ──IP packets──▶ tunnel ──▶ server
                                     │ names: dns (A/AAAA through the tunnel)            wg: boringtun noise over UDP
                                     │ owner-PID check: the caller's peer_allowed hook    ovpn: OpenVPN 3 Core, external TUN
```

- `wg`: the `.conf` parser and the tunnel: boringtun's `noise::Tunn` (BSD-3) over a tokio UDP
  socket, with its timers, keepalives and handshake retries. A silent peer is probed with a
  fresh handshake (after 4 s of silence since we sent data, or 30 s idle); two unanswered
  probes end the session (`reconnecting`).
- `ovpn`: the profile inspection and rewrite (below) and the tunnel: OpenVPN 3 Core 3.11.7
  through `openvpn-connect` 0.1.2 (vendored in the repository's `vendor/`, see
  `vendor/README.md`) with the `external-tun` feature. Core hands over IP packets; its
  blocking `connect()` runs on a thread of its own; events and log lines are classified for
  failures.
- `net`: smoltcp 0.14 (0BSD) with `Medium::Ip` on the tunnel's address and MTU (configured or
  pushed). TCP connections are tokio `AsyncRead`/`AsyncWrite` streams; at most 256 at once,
  256 KiB receive window each.
- `dns`: A (and AAAA when the tunnel has an IPv6 address) through the config's or the pushed
  DNS servers, UDP inside the tunnel, TCP when truncated or unanswered, a TTL cache (5 s to
  10 min, negative 30 s). No server configured: 1.1.1.1, still through the tunnel. The
  system resolver is used only for the VPN server's own name.
- `proxy`: the HTTP proxy (below).
- `Vpn` (`service.rs`): its own tokio runtime (2 workers `krunk-vpn`, driven by the
  `krunk-vpn-rt` thread), the connect/reconnect loop with back-off (1, 2, 4 … 60 s, reset
  after a connection), status callbacks (each change, and every 2 s while connected), stop.

Nothing in the crate logs. Keys, passwords and profile texts are `Zeroizing` and their
`Debug` output is redacted; the crate has no `unsafe` (`#![forbid(unsafe_code)]`).

## Proxy semantics

- `CONNECT host:port` opens a TCP connection through the tunnel and pipes both ways (data the
  client sends right after the CONNECT head is forwarded; idle limit 30 min).
- Absolute-form `http://` requests, any method, are rewritten to origin-form with `Host` set
  from the URL, lose their hop-by-hop headers (`Connection` and the headers it names,
  `Keep-Alive`, `Proxy-*`, `TE`, `Trailer`, `Upgrade`) and get `Connection: close`. One request
  per connection; the response head gets the same treatment, the body is piped until the
  server closes (idle limit 5 min). `https://` absolute-form and origin-form requests get 400.
- Strict, bounded request heads: at most 16 KiB (431), 100 headers (431), CRLF only, no bare
  CR/LF, no obs-fold, no whitespace before the colon, token methods and names, no control
  characters in values, one `Host`, at most one `Content-Length` (digits only), no
  `Content-Length` with `Transfer-Encoding`, `chunked` as the only transfer coding (else
  501), no `Transfer-Encoding` on HTTP/1.0. Bodies are forwarded by their framing: exactly
  `Content-Length` bytes, or chunk by chunk with each size line and trailer re-checked, so
  nothing after the body can reach the server as a second request.
- Timeouts: head 30 s, tunnel connect 20 s (504), response head 120 s (504). At most 512 client
  connections.
- Peers: loopback only, then `peer_allowed(peer)` (the Windows owner-PID check, run on a
  blocking thread); refused peers are closed without an answer.

**Fail-closed.** While the tunnel is not connected every request gets
`HTTP/1.1 503 VPN not connected` (plain-text body `VPN not connected`); requests waiting for
DNS or a connect when the tunnel drops get the 503 at once. Destinations outside the WireGuard
AllowedIPs / OpenVPN routes (pushed `route`, `redirect-gateway`, the tunnel subnet, and the
profile's own `route`/`redirect-gateway`) get `502`, as do loopback, unspecified and
multicast addresses: a split config means "only those networks work". Nothing ever goes
direct. UDP is not proxied (HTTP CONNECT is TCP only; Chromium falls back from QUIC).

## OpenVPN import rewrite and failure detection

OpenVPN 3 Core 3.11.7 silently ignores `remote-cert-tls` (spike risk R1), so `import`:
- rewrites `remote-cert-tls server` to `remote-cert-eku "TLS Web Server Authentication"`
  (any other value is refused); adds that line with a warning when the profile has no EKU check;
- drops `script-security`, `up`, `down`, `route-up` and the other script, plugin and daemon
  options with a warning;
- takes an inline `<auth-user-pass>` pair out of the text (kept as credentials); a password
  file argument is dropped with a warning (`needsCredentials`);
- refuses file references (`ca ca.crt`, `tls-auth ta.key`, `config …`), TAP profiles,
  password-protected keys; warns about `static-challenge` (not supported);
- validates the result with Core's own parser (`evaluate_config`).

The rewrite is idempotent: importing a rewritten text gives the same text and no warnings.
The app stores the original file and imports it again at each connect.

`connect()` returns Ok even after `AUTH_FAILED`, and TLS/certificate errors only appear in
Core's log while it keeps reconnecting (spike risk R3). The session therefore watches the
events (`AUTH_FAILED`, `CERT_VERIFY_FAIL`, `CLIENT_HALT`, `NEED_CREDS`, `DYNAMIC_CHALLENGE`,
`RECONNECTING`, `CONNECTION_TIMEOUT` …) and classifies log lines (only the class is kept, the
line is never stored): the rustls verifier's `peer certificate …` and `… did not match the peer
certificate` messages. A refused password gives `failed` "The username or password was
refused", a certificate error `failed` "The server's certificate was not accepted", both
without a retry loop. `RECONNECTING` after a connection gives `reconnecting` (Core reconnects
by itself); Core's connection timeout (30 s) ends the session for a retry with back-off.
Compression is off (`compression_mode = "no"`).

## Tests

```sh
cargo test -p vpn                                   # unit tests; the testbed tests skip
cargo clippy -p vpn --all-targets -- -D warnings
cargo fmt --all -- --check

# Integration tests against the testbed (dind; ports 8551/udp, 8194/udp+tcp):
docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml up -d --build
KRUNK_VPN_TESTBED=1 cargo test -p vpn --test testbed
docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml down -v --rmi local

# Windows (cargo-xwin; no extra flags, the tap-windows.h stand-in is in the vendored crate):
source scripts/xwin-env.sh
cargo xwin clippy --target x86_64-pc-windows-msvc -p vpn --all-targets -- -D warnings
cargo xwin build --release --target x86_64-pc-windows-msvc -p vpn --example vpn-proxy
```

- Unit: the WireGuard and OpenVPN parsers (good, broken, PostUp-laden and hostile inputs, the
  rewrite), the proxy parser (CONNECT, absolute-form, oversized heads, smuggling patterns,
  chunked re-checks, response rewrite), DNS packets (building, parsing, CNAME chains,
  compression loops, truncation at every length), two smoltcp stacks back to back, and the
  503 while not connected (plus the PID hook refusing a peer and a clean stop).
- Testbed (`tests/testbed.rs`, serialised): `http://inside.test/` (names through the tunnel
  DNS) and CONNECT answer with the tunnel IP for WireGuard, OpenVPN UDP and OpenVPN TCP; 502
  outside the routes and for unknown names; a wrong password (UDP and TCP) and a CA that did
  not sign the server give `failed` and stay failed; stopping the WireGuard and the OpenVPN
  server gives 503 until it is back, then the VPN reconnects.

Manual check: `cargo run -p vpn --example vpn-proxy -- <file> [--user U] [--seconds N]`
(password from `KRUNK_VPN_PASS` or `--pass`), then `curl -x http://<proxy> http://inside.test/`.

## Licences and versions

| Component | Version | Licence |
| --- | --- | --- |
| boringtun (`noise` only) | =0.7.1 | BSD-3-Clause |
| smoltcp | =0.14.0 | 0BSD |
| openvpn-connect, openvpn-connect-sys (vendored) | =0.1.2 | MPL-2.0 |
| OpenVPN 3 Core (inside openvpn-connect-sys) | 3.11.7 | AGPL-3.0-only OR MPL-2.0, used under MPL-2.0 |
| Asio (inside openvpn-connect-sys) | 1.24.0 | BSL-1.0 |
| LZ4 (lz4-sys) | 1.10.0 | BSD-2-Clause |

The notices and licence texts are in `THIRD-PARTY-NOTICES.txt` (appended to the package's
notices by `scripts/build-dist.sh`). The vendored crates' checksums and our changes to them:
`../vendor/README.md`. boringtun and openvpn-connect-sys bring the older RustCrypto generation
(digest 0.10, hmac 0.12, sha2 0.10, chacha20poly1305 0.10, x25519-dalek 2) next to core's
exact pins, which are untouched.

## Known limits

- OpenVPN over UDP notices a dead server only through Core's keepalive (`ping-restart`, as
  pushed by the server, often 60–120 s); over TCP it is immediate. WireGuard notices in about
  16 s with traffic, 42 s idle.
- Not tested: IPv6 inside the tunnel, `<cert>`/`<key>` and `tls-auth`/`tls-crypt-v2` profiles,
  long sessions and rekeying, throughput, OpenVPN profiles with compression.
- One request per connection for plain `http://` through the proxy (Chromium copes; it costs a
  TCP handshake through the tunnel per request).
