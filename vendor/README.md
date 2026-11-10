# Vendored crates (Krunk 0.14.0)

OpenVPN 3 Core comes through `openvpn-connect`, a young crate with a single maintainer (see
`spikes/vpn/README.md`, supply chain). Both crates are vendored here, pinned, and used through
path dependencies from `vpn/Cargo.toml` (no `[patch]`, no network at build time). `vendor/` is
excluded from the workspace (root `Cargo.toml`), so workspace clippy and lints never run on
upstream code.

| Crate | Version | Licence | crates.io `.crate` SHA-256 |
| --- | --- | --- | --- |
| `openvpn-connect` | 0.1.2 | MPL-2.0 | `4ed58ad5905e9be8431d8c60b4d4cf413bf31a9a9768695b6fc647282ce36755` |
| `openvpn-connect-sys` | 0.1.2 | MPL-2.0 (bundles OpenVPN 3 Core 3.11.7, AGPL-3.0-only OR MPL-2.0, used under MPL-2.0; Asio, BSL-1.0) | `89788476adfc9bcb9440e35dc13fe3e8b627885246302961f0a4813527c29d63` |

The checksums are those of crates.io's index (the same as in the spike's `Cargo.lock`). Each
directory is the `.crate` archive unpacked as it is, plus only these changes:

- `openvpn-connect/Cargo.toml`: the `openvpn-connect-sys` dependency is
  `path = "../openvpn-connect-sys"`, `version = "=0.1.2"`.
- `openvpn-connect-sys/CMakeLists.txt`: on Windows, `win-include/` is added to the include
  path. OpenVPN's `openvpn/tun/win/tunutil.hpp` includes `<tap-windows.h>` even in
  external-TUN builds, and the crate does not ship it.
- `openvpn-connect-sys/win-include/tap-windows.h` (new, ours): a clean-room stand-in with the
  public TAP-Windows driver constants only (the real header is GPL-2.0 and is not vendored).
  Krunk never uses a TAP adapter. With it, `source scripts/xwin-env.sh && cargo xwin build`
  needs no extra flags.
- `openvpn-connect-sys/build.rs`: `cargo:rerun-if-changed=win-include`.
- `rustfmt.toml` in both (rustfmt defaults, so `cargo fmt --all -- --check` keeps the upstream
  formatting) and `openvpn-connect-sys/.gitignore` (`!build/`: the root `.gitignore` ignores
  `build/`, which here holds the build script's modules).

Check that nothing else changed:

```sh
d=$(mktemp -d) && cd "$d"
curl -sSfL -o c.crate https://crates.io/api/v1/crates/openvpn-connect/0.1.2/download
curl -sSfL -o s.crate https://crates.io/api/v1/crates/openvpn-connect-sys/0.1.2/download
sha256sum c.crate s.crate      # must match the table
tar xzf c.crate && tar xzf s.crate
diff -r openvpn-connect-0.1.2 <repo>/vendor/openvpn-connect
diff -r openvpn-connect-sys-0.1.2 <repo>/vendor/openvpn-connect-sys
```

MPL-2.0 obligations: the source of these files (with the changes above) is this directory;
the notice ships in `THIRD-PARTY-NOTICES.txt` (`vpn/THIRD-PARTY-NOTICES.txt`, appended by
`scripts/build-dist.sh`). Changes to MPL files must stay published here. Never use the AGPL
branch of OpenVPN 3 Core's dual licence.

To update: unpack the new `.crate` files over these directories, re-apply the changes above,
update the table, and re-run the `vpn` tests with the testbed (`vpn/README.md`).
