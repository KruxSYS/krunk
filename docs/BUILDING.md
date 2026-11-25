# Building Krunk from source

Cross-building the Windows release on Linux, updating the engine, and the checks. Part of the [Krunk README](../README.md); the other pages are listed there under *Documentation*.

The release is cross-compiled on Linux for `x86_64-pc-windows-msvc` with `cargo-xwin`. CEF's
Rust bindings (`cef` crate) build CEF's C++ wrapper library and support only the MSVC target:
`x86_64-pc-windows-gnu` fails with "Unsupported target triplet". One command builds and
packages everything:

```sh
scripts/build-dist.sh            # -> dist/Krunk-win-x64/, the zip, Krunk-Setup-<version>.exe, SHA256SUMS
scripts/build-dist.sh --debug    # same layout with a debug build (testing only)
scripts/build-dist.sh --dev      # dev-updates build (update check accepts http://) -> target/dev-dist/
UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh   # another update server
```
`UPDATE_BASE_URL` must be `https://` (the build fails otherwise, except with `--dev`).
`KRUNK_UPDATE_PUBKEY_FILE` replaces the trusted public key (test keys for dev builds only).

What it does:
1. `scripts/fetch-cef.sh` downloads the CEF archive pinned in `cef.lock` (the "minimal"
   Windows x64 distribution, about 175 MB) into its own cache directory
   (`~/.cache/krunk-cef/downloads`). It checks the SHA-1 published in CEF's build index
   (`cef-builds.spotifycdn.com/index.json`) **and** the SHA-256 pinned in `cef.lock`, and only
   then extracts it with `export-cef-dir`. Nothing from CEF is committed to the repository.
2. `cargo xwin build --release --target x86_64-pc-windows-msvc -p app --lib` builds
   `Krunk.dll` with the static CRT (`.cargo/config.toml`) and `libcef.dll` delay-loaded.
3. It assembles `dist/Krunk-win-x64/`: `Krunk.exe` (the CEF bootstrap, branded by
   `tools/pebrand`), `Krunk.dll`, the CEF DLLs, `.pak` files, `icudtl.dat`,
   `v8_context_snapshot.bin`, all `locales/`, `licenses/` and `THIRD-PARTY-NOTICES.txt`. No
   Markdown: the build fails if any `*.md` file ends up in the folder (README and SECURITY are
   in the source and on the website). Then it writes the zip (fixed timestamps and order) and
   `SHA256SUMS`.
4. `scripts/fetch-nsis.sh` fetches NSIS 3.09 (Ubuntu's `nsis` and `nsis-common` packages,
   SHA-256 pinned in `nsis.lock`, unpacked with `dpkg-deb` into `~/.cache/krunk-toolchain`),
   and `makensis` builds `Krunk-Setup-<version>.exe` from `installer/krunk.nsi`. The installer
   deletes `*.md` from the program folder before copying (0.2.x shipped README.md and
   SECURITY.md); it never touches the data folder.

For a quick test build, `scripts/build-dist.sh --fast` uses the `fast` profile (the release
settings without whole-program LTO, incremental, every core): `target/fast-dist/Krunk-win-x64`,
no installer and no zip, about a minute after a change in `app/` instead of the release build's
ten. `KRUNK_DIST=target/fast-dist KRUNK_APP=apptest scripts/vm.sh deploy` puts it on the test
machine next to the release build. With `sccache` installed the build scripts use it, so
compiled dependencies are shared between target directories.

Requirements (versions used for the release):
- Rust stable (1.87 or newer; built with 1.99.1) with `rustup target add x86_64-pc-windows-msvc`
- `cargo install cargo-xwin --version 0.23.1`
- `cargo install export-cef-dir --version 156.3.0` (must match the `cef` crate version)
- LLVM **20.1.8** (`clang-cl`, `lld-link`, `llvm-lib`; the MSVC C++ headers need clang 19 or
  newer), unpacked from the official release tarball. The script expects it in
  `~/.cache/krunk-toolchain/llvm-20.1.8` or in `$LB_LLVM`.
- `cmake`, `ninja`, `curl`, `python3`, `zip`, `dpkg-deb` (for NSIS)

**Microsoft's licence:** cargo-xwin downloads the MSVC CRT and the Windows SDK (headers and
import libraries) from Microsoft. Building therefore means accepting Microsoft's licence terms
for them; `build-dist.sh` sets `XWIN_ACCEPT_LICENSE=1` for this (and only there). The versions
are pinned so that builds are reproducible: `XWIN_VERSION=17`, CRT `14.44.17.14` (MSVC 14.44.35220),
SDK `10.0.26100`; the download is cached in `~/.cache/krunk-xwin`.

A clean build from a fresh clone took 1 min 27 s on the build machine (CEF's C++ wrapper and
LTO), plus the one-time downloads of CEF (175 MB) and the MSVC CRT/SDK; it has no warnings. The
release profile uses LTO, `opt-level = "s"`, strip and `panic = "abort"`. Two clean builds
from the same checkout path give the same `Krunk.dll` except for a few bytes (12 to 20
measured): the PE time stamp and the debug-directory record (GUID). `Krunk.exe` and the CEF
files are always identical. The zip is byte-identical for the same `Krunk.dll` and
`SOURCE_DATE_EPOCH` (default: the HEAD commit time, which stamps every file). The installer is
not byte-reproducible: NSIS's compressed data differs between runs, while the files it
installs are identical.

**Alternative: native build on Windows** (documented, not used for the release). With Visual
Studio 2022 Build Tools ("Desktop development with C++", MSVC 14.44, Windows SDK 10.0.26100),
Rust, CMake and Ninja: set `CEF_PATH` to the folder produced by `scripts/fetch-cef.sh` (or
run `export-cef-dir` on the verified archive) and run
`cargo build --release --target x86_64-pc-windows-msvc -p app --lib`. Then assemble the folder
as `build-dist.sh` does.

## Updating the engine
Security fixes for the engine reach users only through new Krunk releases (see "Known
limitations"). To move to a newer CEF:
1. Set the new `cef` crate version (e.g. `=157.x.y`) in `app/Cargo.toml` and run
   `cargo update -p cef`. Install the matching `export-cef-dir` version.
2. Build the engine with the codecs: `scripts/cef-build/cross-build.sh` with the new CEF
   commit and Chromium tag (`scripts/cef-build/README.md`; about 2 h on 32 cores once the
   checkout is there). Copy the archive into `~/.cache/krunk-cef/downloads/` and the artifact
   store, set `CEF_VERSION`, `CHROMIUM_VERSION`, `CEF_ARCHIVE` and `CEF_ARTIFACT` in
   `cef.lock` (`CEF_SOURCE=self-built`) and run `scripts/fetch-cef.sh --pin` once to record
   the archive's SHA-256. (A stock Spotify "minimal" build, without H.264/AAC, works too:
   `CEF_SOURCE=spotify` plus `CEF_SHA1` from `https://cef-builds.spotifycdn.com/index.json`,
   but it lacks Krunk's patch: background tabs would run at full speed again.) Krunk's patches
   in `scripts/cef-build/patches/` must still apply (`cross-build.sh build` stops if one
   doesn't); re-record their SHA-256 in `cef.lock`.
3. `scripts/build-dist.sh`, then run the checks below and `docs/MANUAL_TEST.md` (including
   the media matrix: `scripts/vm-050-media.ps1`, `scripts/vm-052-engine.ps1`).

## Checks
```sh
cargo test --workspace                                                   # core: 323 unit tests
cargo clippy --workspace --all-targets -- -D warnings                    # Linux
cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo audit
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-ui.cjs [shots-dir]   # UI pages with a mock host
```
For `cargo xwin clippy`, export the same `CEF_PATH` and `XWIN_*` variables as `build-dist.sh`.
The app's own unit tests (DPAPI, message escaping) run on Windows:
`cargo xwin test --target x86_64-pc-windows-msvc -p app --no-run`, then run the test exe.

Testing on Windows:
- `scripts/vm.sh` deploys `dist/Krunk-win-x64` to a Windows test machine over SSH and
  drives it (start, stop, screenshots, keys, clicks).
- `scripts/vm-dpi-run.sh OUTDIR 1280x800:100 1920x1080:150 1920x1080:175 3840x2160:250
  1920x1080:150:rt` checks the layout at display scales (0.15.4): per mode it switches the
  test machine's display (`scripts/vm-display.ps1`; always back to 1280×800 at 100 %), takes
  about forty screenshots (`scripts/vm-dpi-shots.ps1`) and logs window, view and popup
  rectangles, hit-test codes and the page's `devicePixelRatio`; `:rt` changes the scale while
  Krunk runs, normal and maximized. See [docs/DPI.md](DPI.md).
- `scripts/vm-sandbox.ps1` shows the integrity level and token of every Krunk process.
- `scripts/vm-engine-check.ps1` shows which engine DLLs each process loaded and whether any
  `msedgewebview2.exe` was started by Krunk.
- `scripts/vm-mem.ps1` lists the process tree with memory figures.
- `scripts/vm-leak-scan.ps1` searches the data folder for strings that must never be stored,
  such as a test password.
- The Windows checklist is `docs/MANUAL_TEST.md`.

Page-load harness (0.26.0; details and every option in
[scripts/perf/README-load.md](../scripts/perf/README-load.md)):
- **What it measures:** the same recorded pages in Krunk, Chrome and Edge on the test VM. Each
  load is timed from the real input (Enter in the address bar, a link click, Alt+Left/Right) to
  navigation start, TTFB, FCP, LCP, DOMContentLoaded and load. Back/forward-cache restores are
  timed to the first frame. Median and p90 per page; the corpus value is the median over pages.
- **Replay:**
  - Web Page Replay (`wpr.exe`, cross-compiled with Go) serves the corpus. `lshim.exe`
    (`scripts/perf/load/kinput/src/bin/lshim.rs`) sits in front of it and adds a fixed 40 ms
    round-trip time.
  - A private test CA, valid 90 days, is trusted in the VM's machine root store only while a pass
    runs. `load-run.sh` removes it once the pass's `done` marker exists.
  - These tools live in `../.pipeline/loadtools`, outside git, and are pushed when they change.
- **Krunk under test:** a harness build, `scripts/build-dist.sh --harness`. It is the release
  build plus `perf-harness`: the DevTools port, `KRUNK_HOST_RULES` to reach the replay, and the
  `KRUNK_PERF_OFF` A/B switches. Release builds have none of these.
  - Install it with `KRUNK_DIST=target/harness-dist KRUNK_APP=app260h scripts/vm.sh deploy`.
  - Make the base profile once with `scripts/vm.sh ps1 scripts/vm-0260-prep.ps1`.
- **A pass:**
  ```sh
  python3 scripts/perf/load/mkconfig.py replay 5 krunk-a,krunk-b,chrome,edge typed,link > /tmp/c.json
  scripts/perf/load-run.sh TAG /tmp/c.json          # pushes, runs scripts/vm-0260-load.ps1 on the VM, waits, pulls
  python3 scripts/perf/load-summary.py ../.pipeline/evidence-0.26.0/runs/TAG --pool krunk=krunk-a,krunk-b
  ```
  - `load-run.sh` holds `/tmp/krunk-vm-busy`, so no host build runs meanwhile.
  - `ATTACH=1` waits for a pass that is already running.
  - `scripts/perf/load-fill.py` reruns incomplete sessions; `load-summary.py` takes the passes in
    order, and later sessions replace earlier ones.
  - With the same build twice (`krunk-a`, `krunk-b`), `load-summary.py` prints the noise band
    and, with `--ab A=B`, whether B's change beats it.
- **Pin Chrome and Edge** (auto-update off) on the test VM before comparison runs. In 0.26.0 both
  updated between two sessions (Chrome 157 → 158, Edge 156 → 157), and the harness's clicks and
  keys no longer reached them: every Chrome/Edge cell timed out. Krunk-only passes are unaffected.
- **Recording the corpus again:** `mkconfig.py record 1 chrome typed`, then `load-run.sh record`
  (Chrome, real network, about 7 minutes).
