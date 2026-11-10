# M9 VPN spike (0.14.0): results

Time-boxed spike for the in-browser VPN (plan-0.13.0.md §6): a userspace tunnel inside the Krunk
process, smoltcp as the TCP/IP stack, and a local HTTP CONNECT proxy. No TUN/TAP adapter, no
routes, no admin rights. This folder is its own cargo workspace and is not part of the product build.

| Path | What it is |
| --- | --- |
| `ovpn-probe/` | OpenVPN 3 Core through `openvpn-connect` 0.1.2 (`external-tun` + `vendor`). Default mode: evaluate an inline profile. `connect <file.ovpn> [user pass]`: a real session whose external TUN injects an ICMP echo and waits for the reply. |
| `wg-probe/` | boringtun 0.7.1 `noise` + smoltcp 0.14. Default mode: two in-memory `Tunn`s handshake and carry a smoltcp TCP SYN. `connect <endpoint> <server_pub> <client_priv> <client_ip> <ip:port>`: a real handshake over UDP, then `GET /` through smoltcp TCP inside the tunnel. |
| `xwin-ovpn-env.sh`, `win-include/tap-windows.h` | The extra Windows cross-build env (see below). |
| `testbed/` | The compose stack: WireGuard server, OpenVPN 2.6 server (UDP + TCP), inside echo server, dnsmasq. |

## Recommendation: GO for OpenVPN 3 Core in 0.14.0 (with the conditions below)

It builds for Linux and for `x86_64-pc-windows-msvc` with cargo-xwin. The Windows exe runs on the
VM. The Windows exe also connected to the testbed's OpenVPN 2.6 server over TCP through `ssh -R`:
it got the pushed IP, and an ICMP packet made the round trip through the external TUN. Nothing
needed admin rights and no adapter was created.

**Conditions:**
1. Pin `openvpn-connect`/`openvpn-connect-sys` to `=0.1.2` and vendor the source into the repo
   (single maintainer, see the supply-chain notes).
2. Rewrite `remote-cert-tls server` in imported profiles into `remote-cert-eku "TLS Web Server
   Authentication"`, because Core silently ignores it (see risk R1).
3. Krunk must spot "stuck in reconnect" itself (see R3).

## 1. OpenVPN 3 Core cross-build

**Result: builds and runs on Linux and on Windows (msvc via xwin).**

### Linux (x86_64-unknown-linux-gnu)
- `cargo build --release -p ovpn-probe`. No extra env: system clang 18/libclang, cmake, and a C++17 g++.
- Clean build, including all deps: **26 s** wall on 32 cores (2m16 CPU).
- `ovpn-probe` output: `OpenVPN core 3.11.7 linux x86_64 64-bit GREMLIN`, `external_tun: true`, an
  empty crypto self-test (= ok), eval ok, and a broken profile rejected with `CONFIG_ERROR`.

### Windows (x86_64-pc-windows-msvc)
```sh
source scripts/xwin-env.sh            # from the repo root (LLVM 20 clang-cl/lld-link, pinned xwin CRT/SDK)
cd spikes/vpn && source ./xwin-ovpn-env.sh
env "CXXFLAGS_x86_64-pc-windows-msvc=$ovpn_cxxflags" \
  cargo xwin build --release --target x86_64-pc-windows-msvc
```
- **Only one problem**: `openvpn/tun/win/tunutil.hpp` does `#include <tap-windows.h>` (via
  `netconf/hwaddr.hpp`, so even external-tun builds include it). The crate does not ship it.
  - The real header is from tap-windows6 (GPL-2.0), so we don't vendor it.
  - `win-include/tap-windows.h` is a 30-line stand-in written for this spike. It holds only the
    public driver ABI constants (IOCTL numbers, registry keys). The TAP code path never runs with external-tun.
  - Exact error without it: `tunutil.hpp(45,10): fatal error: 'tap-windows.h' file not found`.
- **Not needed:**
  - `CMAKE_GENERATOR` (cargo-xwin already passes `-G Ninja` and its clang-cl toolchain file).
  - `BINDGEN_EXTRA_CLANG_ARGS` (`wrapper.h` is plain C and bindgen runs fine for the host libclang).
  - `/EHsc` (cargo-xwin adds it).
  - `-fms-compatibility`.
- **Trap:**
  - cargo-xwin sets the *hyphenated* `CXXFLAGS_x86_64-pc-windows-msvc`, which cc-rs/cmake-rs read first.
  - Setting the underscore form replaces cargo-xwin's flags: clang-cl loses `--target` and the
    xwin include dirs, and you get "Compiler family detection failed".
  - So `xwin-ovpn-env.sh` repeats cargo-xwin's flags, adds `/imsvc win-include`, and the cargo
    call is wrapped in `env` (bash cannot export a hyphenated name).
  - After changing these flags, delete `target/x86_64-pc-windows-msvc/release/build/openvpn-connect-sys-*`:
    the cmake cache keeps the old flags.
- The "Compiler family detection failed" cargo warnings that remain are harmless.
- Build time for openvpn-connect-sys + the probe: **12–17 s** wall with warm host build deps.
- **The VM** (Windows 11, `C:\pipeline\krunk\spike-vpn\`):
  - `ovpn-probe.exe` prints `OpenVPN core 3.11.7 win x86_64 64-bit GREMLIN` and evaluates the profile (exit 0).
  - `ovpn-probe.exe connect` over `ssh -R 127.0.0.1:11940:dind:1194` (TCP):
    `CONNECTED ... via /TCP on krunk-probe/10.89.0.2/`, and the injected ICMP echo to 10.77.0.10
    came back through Core.
  - The probe's only DLL imports are kernel32, ws2_32, ntdll and api-ms-win-core-synch.

### Size
- `openvpn3_core.lib` (static, /O2): 6.9 MB before linking.
- `ovpn-probe.exe` with the eval-only path: 1.15 MB. With the full connect path: 6.2 MB.
  An empty Rust exe is 0.13 MB.
- Breakdown of the 6.2 MB exe (lld /MAP):

  | Part | Size |
  | --- | --- |
  | OpenVPN Core C++ | 1.35 MB |
  | openvpn-connect-sys Rust backends (RustCrypto/TLS glue) | 0.92 MB |
  | rustls | 0.65 MB |
  | ring | 0.52 MB |
  | hickory (resolver/proto/net) | 0.75 MB |
  | tokio | 0.19 MB |
  | x509-parser | 0.17 MB |
  | idna | 0.15 MB |

- Krunk already links rustls 0.23.46, ring, tokio 1.54, idna and rustls-webpki (same versions).
  **The expected growth of Krunk.dll (15.9 MB) is about 3.5 MB.**
- boringtun + smoltcp add about 0.7 MB at most (`wg-probe.exe` 0.84 MB).

### Licences
| Component | Licence | Notes |
| --- | --- | --- |
| OpenVPN 3 Core 3.11.7 (`vendor/openvpn3`) | **AGPL-3.0-only OR MPL-2.0** (`LICENSE.md`, `LICENSES/`) | Use under MPL-2.0. |
| Asio 1.24.0 (`vendor/asio`) | BSL-1.0 | |
| openvpn-connect, openvpn-connect-sys (bridge, Rust backends, patches) | MPL-2.0 | |
| lz4 (via lz4-sys, `vendor`) | BSD-2-Clause lib, MIT crate | |
| hickory-*, rustls, RustCrypto | MIT/Apache-2.0 | |
| boringtun 0.7.1 | BSD-3-Clause | |
| smoltcp 0.14.0 | 0BSD | |
| `win-include/tap-windows.h` | ours | Clean-room constants, no GPL text. |

- MPL-2.0 is per-file copyleft. It is fine to link into MIT Krunk.
- What we owe:
  - ship the MPL notice;
  - tell recipients where the source of the MPL files is (crates.io, or our vendored copy);
  - publish any change we make to MPL files.
- Never pick the AGPL branch.
- Not checked: whether the Windows-only TAP/Wintun headers Core includes, beyond tap-windows.h,
  carry other licences. None were needed.

### Supply chain and safety
- **Maintenance:**
  - One maintainer (`richerfu`, crates.io owner and the only GitHub contributor).
  - Repo `github.com/networks-rs/openvpn-connect-rs`: created 2026-07-20, 9 commits, 0 stars.
  - Versions 0.1.0–0.1.2 (2026-07-31 to 2026-09-17), about 800 downloads.
  - `Cargo.toml` has no `repository` field.
  - This is a high bus-factor risk: plan to vendor it and to own fixes.
- **Upstream is pristine:**
  - I diffed the crate's `vendor/openvpn3/{openvpn,client,LICENSE.md}` against OpenVPN/openvpn3 at
    `18edfae7` (3.11.7), and `vendor/asio/asio/include` against chriskohlhoff/asio at `147f7225`.
    They are **identical**.
  - All changes are in `patches/openvpn3.patch` (271 lines) and `patches/asio.patch` (281 lines).
    `build.rs` applies them in `OUT_DIR` with its own diff applier, with no network or git.
  - The OpenVPN patch adds Gremlin/external-transport plumbing and switches the resolver, crypto,
    TLS and PEM to the crate's Rust backends. The Asio patch adds OpenVPN Connect hooks.
- **Custom security code (the real audit surface):**
  - Core's TLS and data-channel crypto do not use OpenVPN's mbedTLS/OpenSSL code. They use the
    author's C++ and Rust bridge:
    - `src/rust_tls.hpp` (1074 lines) and `src/rust_backend/tls.rs` (1311 lines): rustls sessions,
      certificate checks (ns-cert-type, KU/EKU, verify-x509-name, tls-remote), keying-material export;
    - `src/rust_crypto.hpp` (586 lines) and `src/rust_backend/crypto.rs` (734 lines): ciphers, HMAC, TLS PRF;
    - `src/bridge.cpp` (3289 lines): catches exceptions, ownership/vtables;
    - the safe crate: 4087 lines, of which `native.rs` has 230 `unsafe` uses.
  - Spot checks against the testbed:
    - a wrong CA fails (`UnknownIssuer`);
    - a wrong `verify-x509-name` fails;
    - `remote-cert-eku` is enforced in both directions;
    - a wrong password gives `AUTH_FAILED`.
  - Also compiled in: the Gremlin (fault-injection) code (`OPENVPN_GREMLIN`, visible in the
    platform string). It only acts when `Config::gremlin_config` is set; Krunk never sets it.
  - Also compiled in: the cipher dependencies `blowfish`, `des`, `md4` and `md-5`, for legacy profiles.
- **Dependency duplication:** openvpn-connect-sys pins the *old* RustCrypto generation, while core
  pins the new one. Two copies sit side by side, which works but costs size:

  | Crate | openvpn-connect-sys | core |
  | --- | --- | --- |
  | digest | 0.10 | 0.11 |
  | hmac | 0.12 | 0.13 |
  | sha2 | 0.10 | 0.11 |
  | chacha20poly1305 | 0.10 | 0.11 |
  | aes-gcm | 0.10 | 0.11 |

  It also brings `getrandom` 0.3 and `hickory-resolver` 0.26 (new to Krunk).
  rustls 0.23.46, ring 0.17.14 and tokio 1.54.1 resolve to Krunk's versions.

### API notes for M11
- Use `Client::new(handler)`, then `evaluate`, `provide_credentials` and a blocking `connect()` on its own thread.
  `stop()` comes from a clone. A `tokio` feature exists too; the spike used the sync API.
- `EventHandler::external_tun()` returns our `ExternalTun`:
  - `configure` must return `true`.
  - `start(cfg, io)` gets the pushed options as **newline-separated** text (`ifconfig 10.88.0.2
    255.255.255.0`, `route ...`, `dhcp-option DNS ...`, `tun-mtu`). The app must call
    `io.pre_tun_config()`, `io.pre_route_config()` and `io.connected()`, then keep `io`.
  - `io.receive(pkt)` sends a packet into the tunnel. `send(pkt)` delivers one from the tunnel.
  - `info()` reports the IP back (it shows in the CONNECTED event).
- Username/password-only profiles (no `<cert>`/`<key>`) need `Config::disable_client_cert = true`.
  Without it you get `Missing External PKI alias`. It is the same as `setenv CLIENT_CERT 0`.
- `connect()` returns `Ok(Status{""})` even after `AUTH_FAILED`. Failures come only as events
  (`error: true`) or, for TLS and certificate errors, **only as log lines** while Core keeps
  reconnecting (RECONNECTING ×N). See R3.

## 2. WireGuard + smoltcp

**Result: builds on Linux and Windows. Works against a real kernel WireGuard server.**

- Dependencies:
  - `boringtun = "=0.7.1"`, default features off (`device` is off, so only `noise`).
  - `smoltcp = "=0.14.0"` with `std, medium-ip, proto-ipv4, proto-ipv6, socket-tcp, socket-udp,
    socket-dns` (`socket-dns` is unused so far; we can drop it if our DNS module speaks UDP itself).
  - `base64` 0.22 and `rand_core` 0.6 (already in boringtun's tree).
- Plain `cargo build` and plain `cargo xwin build`; no extra env.
- Build times: Linux about 7 s; Windows 2 s after the shared deps.
- Results:
  - `wg-probe` self mode on Linux and on the VM: handshake initiation 148 B and response 92 B, and
    smoltcp's 52-byte SYN comes out of the peer's decapsulate byte-identical.
  - `wg-probe connect dind:51820 ...` from this sandbox: real handshake in about 1.5 ms, then
    `GET /` over smoltcp TCP inside the tunnel returned `client=10.66.0.2` from the inside echo.
    This is the whole M10 data path minus the proxy.
- Version notes against core's exact pins:
  - **boringtun 0.7.1 uses the old crypto generation:** x25519-dalek 2.0.1 / curve25519-dalek
    4.1.3, chacha20poly1305 0.10.1 (it asks for `0.10.0-pre.1`), aead 0.5.2, hmac 0.12,
    blake2 0.10, rand_core 0.6.
  - Core pins x25519-dalek =3.0.0, curve25519-dalek 5, hmac/hkdf =0.13 and chacha20poly1305 0.11.
  - **No conflicts:** different majors sit side by side. ring 0.17.14 is shared.
  - boringtun also pulls `parking_lot`, `tracing`, `ip_network(_table)`, and `nix` (unix only).
  - Its `crate-type` includes staticlib/cdylib; it does no harm as a dependency.
  - boringtun 0.7.1 (2025) is Cloudflare's crate, with low but present maintenance.

## 3. Testbed on dind

**Result: everything works.**
- Kernel WireGuard works in a container with only `NET_ADMIN` (`ip link add wg0 type wireguard`
  OK; the dind kernel is 6.8). `/dev/net/tun` is available.
- UDP published on dind is reachable from the sandbox (a UDP echo on `dind:51820` answered).
- `testbed/compose.yaml` (project `vpn-spike`; images built from `alpine:3.22@sha256:4934…898f`):
  - `wg`: kernel WireGuard on `51820/udp`, tunnel 10.66.0.0/24.
  - `ovpn`: OpenVPN 2.6.20 on `1194/udp` (10.88.0.0/24) and `1194/tcp` (10.89.0.0/24).
    - Auth is CA + tls-crypt + username/password `krunk`/`krunk-test`, with no client cert, like
      commercial providers.
    - It pushes `route 10.77.0.0/24` and DNS 10.77.0.53.
  - `inside` (10.77.0.10:80): a socat HTTP echo, `client=<source IP>`.
  - `dns` (10.77.0.53): dnsmasq, `inside.test` → 10.77.0.10 (AAAA NODATA).
  - Keys and the test PKI are generated at first start into the `state` volume, never committed.
    Read them out with `docker compose -p vpn-spike exec wg cat /state/wg/client.conf` or
    `... exec ovpn cat /state/ovpn/client-udp.ovpn`.
  - No NAT: the inside servers route the tunnel subnets back via the servers, so the echo shows the tunnel IP.
- Two traps that are already solved in the files:
  1. Docker drops forwarded traffic on `internal` networks whose source is outside the network
     subnet (the `DOCKER-INTERNAL` chain). So the inside network is `subnet 10.64.0.0/11,
     ip_range 10.77.0.0/24`, and the /24 routes win.
  2. dnsmasq 2.91 answers only local subnets unless `--interface` is given.
- Also: bind mounts refer to the dind filesystem, not the sandbox's. Pass files by env or stdin.
- Verified (a fresh `up --build` takes about 10 s with cached layers):
  - `wg-probe connect` gives `client=10.66.0.2`.
  - `wg-quick` in a container on `vpn-spike_outer` resolves `inside.test` via 10.77.0.53 and fetches `client=10.66.0.2`.
  - `ovpn-probe connect` works over UDP and TCP (ICMP round trip to 10.77.0.10).
  - The Windows `ovpn-probe.exe` works over TCP via `ssh -R`.
  - Direct `curl http://10.77.0.10/` from the sandbox times out, so the inside network is only reachable through the tunnels.
- Then `docker compose -p vpn-spike down -v --rmi local`. Nothing is left running.
- For M10/M11, move it to `tools/vpn-testbed/` as planned.
- Packages are not version-pinned (alpine 3.22 at build time: openvpn 2.6.20-r0,
  wireguard-tools 1.0.20250521, dnsmasq 2.91, socat 1.8.1.3). Only the base image is pinned by digest.
- The VM cannot reach dind:
  - OpenVPN-TCP works through `ssh -R`.
  - WireGuard (UDP) needs the planned udp-over-tcp relay.

## Risks
- **R1 `remote-cert-tls` is a no-op in OpenVPN 3 Core 3.11.7.**
  - In the TLS config `load()`, `KUParse::remote_cert_tls()` runs and then `remote_cert_ku()` /
    `remote_cert_eku()` clear `ku`/`eku` when those options are missing.
  - The upstream OpenSSL and mbedTLS contexts have the same order, by code reading; I did not
    check this at runtime against an upstream build.
  - Verified here: `remote-cert-tls client` still connected to a serverAuth-only server. An
    explicit `remote-cert-eku` is enforced.
  - Impact: without `verify-x509-name`, any client cert from the same CA could pose as the server.
  - Mitigation: rewrite the profile at import (`remote-cert-tls server` → `remote-cert-eku "TLS Web
    Server Authentication"`), or patch it in our vendored copy. Report it upstream.
- **R2 Single-maintainer, young crate** with a custom TLS and crypto bridge (about 4.7k lines of
  security code). Vendor it, pin it, review `rust_backend/tls.rs`'s verification order before
  shipping, and expect to carry fixes ourselves.
- **R3 Error reporting:**
  - TLS and certificate failures loop as `RECONNECTING` and are only visible in logs.
  - `connect()` returns `Ok` after `AUTH_FAILED`.
  - The `service` layer needs a failure classifier (events plus log patterns) and a reconnect limit
    for the error state in the UI.
- **R4 Size:** about +3.5 MB on Krunk.dll for OpenVPN and about +0.7 MB for WireGuard. There are
  two RustCrypto generations in the binary.
- **R5 Windows build fragility:**
  - The hyphenated CXXFLAGS override repeats cargo-xwin's internal flag list. A cargo-xwin upgrade
    that changes it would break the build silently; the build would fail loudly, not miscompile.
  - Better: put the stand-in header into a patched vendored copy (add `win-include` to
    `CMakeLists.txt`), so no env is needed.
- **R6 Not tested:**
  - long sessions and rekeying (`reneg-sec`);
  - data-channel throughput;
  - `comp-lzo`/compression profiles (Core supports LZ4; compression should stay off);
  - IPv6 in the tunnel;
  - `<cert>`/`<key>` profiles;
  - `tls-auth`, and `tls-crypt-v2`;
  - BF-CBC/legacy (`enable_legacy_algorithms`);
  - static-challenge/MFA.
- **R7** `hickory-resolver` is used for the *server* name. It reads the system resolver, which is
  fine (the tunnel endpoint must resolve outside the tunnel). Make sure the netstack DNS for
  pages never falls back to it.
