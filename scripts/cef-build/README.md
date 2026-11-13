# Building Krunk's engine: CEF 156.0.15 with proprietary codecs (0.5.2)

Krunk 0.5.2 ships a self-built CEF instead of the Spotify "minimal" build, because that build
has no H.264/HEVC/AAC/AC-3 decoders (see the README's "Media codecs" section). The pin stays
the same: CEF commit `cf671c0` (156.0.15), Chromium 156.0.8078.80 (tag), so the C API is
unchanged and the `cef` crate pin (`=156.3.0`) and Krunk's ffi stay as they are.

The build is a **Linux → Windows cross build** (Chromium's `target_os="win"` on a Linux host),
done because the Windows test VM is far too small to build Chromium. It needs:

- a Linux host with ~32 cores, 64 GB+ RAM and 300 GB+ free (checkout ≈ 130 GB, build tree
  ≈ 60 GB, toolchain package 1.4 GB);
- a **Windows toolchain package** made on a Windows machine with Visual Studio 2026 Build
  Tools (C++ workload, ATL/MFC) and the Windows 11 SDK 10.0.28000 (+ Debugging Tools):
  `toolchain-package.ps1` runs depot_tools' `win_toolchain/package_from_installed.py 2026
  -w 10.0.28000.0 --noarm --allow_multiple_vs_installs` (its vswhere call needs
  `-products *` to see Build Tools). The result, `<hash>.zip`, is not in git. The package
  used for 0.5.2 is `b3a2d895cc.zip` (MSVC 14.51.36231, SDK 10.0.28000.0), SHA-256
  `a477229ce2576bcd4202759c6edfddd8095206c852c36c1623483cdfda16a79f`;
- **no FUSE**: Chromium mounts the extracted toolchain through `ciopfs` for a case-insensitive
  view. This host has no FUSE, so `cross-build.sh toolchain` skips that mount (a local edit of
  `build/vs_toolchain.py`, enabled by `KRUNK_NO_CIOPFS=1`) and `casefix.py` makes the case
  irrelevant instead:
  - a clang VFS overlay (`casefix/vfs-include.yaml`, `case-sensitive: false`) over the MSVC
    and SDK include trees, so `#include <Windows.h>` and `<windows.h>` both resolve;
  - a second overlay (`vfs-lib.yaml`) for lld-link's `/vfsoverlay:` plus a mirror directory
    of lowercase library symlinks (`casefix/lib/x64`, searched after the real directories);
  - `HostX64`/`HostX86` directory symlinks (VS 2026 names them `Hostx64`; Chromium's
    `setup_toolchain.py` and `SetEnv.x64.json` say `HostX64`);
  - the toolchain's `clang-cl` and `lld-link` (symlinks to `clang`/`lld` in
    `third_party/llvm-build/Release+Asserts/bin`) replaced **in place** by two-line wrappers
    that add the overlays — in place because Chromium's `build/toolchain/win/rc/rc.py`
    hard-codes that path. The lld-link wrapper also passes `/ignore:4099`: MSVC 14.51's
    desktop `libcmt.lib` has objects whose debug records point at the *onecore* PDB, which is
    not next to the library, and Chromium's `/WX` turns that warning into an error (the CRT's
    debug info is of no use here: `symbol_level=0`, no PDBs shipped).
  Nothing is written inside the toolchain directory itself: Chromium hashes its file list and
  would re-extract the package (directory symlinks are not part of that list).

Steps (`cross-build.sh` does them; every step is idempotent, all paths under `/work/cef-build`
= `$CEF_BUILD_ROOT`, outside the repository):

1. `checkout`: depot_tools + CEF's `automate-git.py --branch=8078 --checkout=cf671c0
   --no-build --no-distrib`. googlesource answers HTTP 429 under load: if it stops, finish
   with `gclient sync --nohooks --with_branch_heads --revision src@refs/tags/156.0.8078.80`
   in a retry loop (no `--delete_unversioned_trees` once `src/cef` is in place), then copy
   `chromium_git/cef` to `chromium/src/cef`. Run `ensure_bootstrap` in
   `chromium_git/depot_tools` once (the `gn` wrapper needs its bundled Python).
2. `toolchain`: put the package in `/work/cef-build/toolchain/`; the step sets
   `DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL` and `GYP_MSVS_HASH_3bfcb536c8=<package hash>`, adds
   `target_os = ['win']` and `checkout_pgo_profiles: True` to `.gclient`, re-syncs (the
   Windows-only deps, clang's Windows runtime library and the win64 PGO profile only come with
   those), runs `gclient runhooks` (extracts the toolchain), then `casefix.py`.
3. `configure`: CEF's `gn_args.py`, `gclient_hook.py` and `make_distrib.py` decide the platform
   from `sys.platform`; `CEF_CROSS_TARGET=windows` overrides that (local, idempotent edits of
   the checkout, never committed to CEF). Three more local Chromium edits, all needed only
   because the *host* toolchains of a cross build are evaluated with the Windows args:
   `media/media_options.gni` and `rlz/buildflags/buildflags.gni` asserts (CEF turns on CDM host
   verification and RLZ, which those asserts only accept on Windows/Mac — relaxed for
   `current_os != target_os`), and `chrome/installer/mini_installer/BUILD.gn` (a missing
   dependency on `//v8:run_mksnapshot_default`, see the snapshot note below). Then
   `gclient_hook.py` applies CEF's 117 patches and runs `gn gen`. `GN_DEFINES` =
   `is_official_build=true proprietary_codecs=true ffmpeg_branding=Chrome target_os="win"
   target_cpu="x64" enable_platform_hevc=true enable_platform_ac3_eac3_audio=true
   symbol_level=0`. Effective: ThinLTO, PGO phase 2 with the win64 profile, siso (local, no
   remote execution), `enable_platform_dolby_vision=true` (Chromium's default for
   `proprietary_codecs && is_win`), `use_v8_context_snapshot=false` (see below). The generated
   `args.gn` is kept with the evidence.
4. `build`: applies Krunk's own patches (`patches/*.patch`, with `git apply`: `chromium-*.patch`
   in the Chromium checkout, the others in the CEF checkout; skipped when already applied, an error when one doesn't apply), puts Krunk's logo
   in place of Chromium's product logo (`brand`, since 0.12.1, see below), then
   `autoninja -C out/Release_GN_x64 cefsimple bootstrap bootstrapc` (libcef.dll and the
   bootstraps are what Krunk packages). After a patch change on a kept tree this is an
   incremental build: about 2 minutes on the build host (0.10.0).
5. `distrib`: `make_distrib.py --minimal --allow-partial --ninja-build --x64-build --no-archive`,
   plus `snapshot_blob.bin` copied into `Release/` (CEF's file list does not know it), then
   `tar.bz2` named like the Spotify archive with the suffix `_krunk-codecs` (0.5.2–0.9.0) or,
   with the patches, `_krunk-codecs-hidden` (0.10.0–0.12.0) and `_krunk-codecs-hidden-brand`
   (0.12.1–0.12.2) and `_krunk-codecs-hidden-brand-private` (`CEF_FLAVOR`, since 0.12.3).
6. Re-pin in Krunk (and record each patch's SHA-256 in `cef.lock`, `CEF_PATCHES`): copy the archive into the CEF cache (`~/.cache/krunk-cef/downloads/`), set
   `CEF_SOURCE=self-built`, `CEF_ARCHIVE`, `CEF_SHA256` and `CEF_ARTIFACT` in `cef.lock`
   (`scripts/fetch-cef.sh` then skips CEF's index and checks the SHA-256 only), run
   `scripts/build-dist.sh`, then the VM regression and the media matrix.

**V8 snapshot.** Chromium turns the Blink context snapshot off in every Windows cross build
(`tools/v8_context_snapshot/v8_context_snapshot.gni`: the generator is a Blink binary that must
be built and run on the target OS). The engine therefore loads V8's plain `snapshot_blob.bin`
instead of `v8_context_snapshot.bin`; the only cost is a few milliseconds slower page-context
creation. `app/src/ffi/cef/entry.rs` accepts either file and `scripts/build-dist.sh` ships
whichever the archive contains.

Everything under `/work/cef-build` (checkout, build tree, toolchain package, archive) stays
outside git; the archive is kept with the release artifacts (`CEF_ARTIFACT` in `cef.lock`).

**Krunk's patches** (`patches/`, since 0.10.0):
- `chromium-private-media.patch` (0.12.3, Chromium checkout): media of a private tab never
  reaches Windows' media controls ("Now Playing", the volume flyout, the lock screen).
  `ChromeMediaSessionClient::ShouldHideMetadata` counts every off-the-record profile (Chrome
  meant only its primary incognito one; Krunk's private tabs use a CEF one), and
  `SystemMediaControlsNotifier` gives such a session nothing: it stops and clears the controls,
  turns them off while the session is active and drops its pending metadata, position and
  artwork. Chrome would show "Playing media in incognito" there. Media keys don't control a
  private tab's media either.
- `chromium-clone-navigation.patch` (0.18.0, Chromium checkout): the DevTools command
  `Page.krunkCloneNavigationFrom {targetId, entryId}`. Called on a tab that has loaded nothing
  yet (`about:blank`), it copies the back/forward list of the page `targetId` into it
  (`NavigationController::CopyStateFrom`, as Chrome's "Duplicate tab") and makes `entryId`
  current, which loads it. It refuses the same page, a page of another profile (a private
  tab's list never reaches a normal one), an unknown entry and a tab that already has a list.
  Krunk uses it for a middle-click in the Back/Forward menus; without the patch the host opens
  the entry's address only and says so once in `krunk.log`. Not yet compiled: written against
  the 156.0.8078.80 sources, it applies cleanly (`patch -p1 --dry-run`).
- `chromium-sockets-per-host.patch` (0.18.0, Chromium checkout): at most 16 connections to one
  host instead of 6 (`g_max_sockets_per_group` in `net/socket/client_socket_pool_manager.cc`;
  WebSockets keep 255, a proxy 128). Six requests that never finish no longer block every tab's
  loads from that host; sixteen still do (`docs/NETWORK.md`). Not yet built: it applies cleanly
  to the 156.0.8078.80 sources (`patch -p1 --dry-run`).
- `krunk-windowed-hidden.patch`: `CefBrowserHost::WasHidden` works for windowed (Alloy)
  browsers. The native Aura delegate hides or shows the WebContents' own aura window; the
  occlusion tracker then reports it HIDDEN and `WebContentsViewAura` calls `WasHidden()` itself,
  so a background tab is throttled as in Chrome (Krunk's tabs were parked shown, at 64 frames a
  second). Windowless (OSR) browsers are unchanged. Measured: 30 busy tabs 90 % → 1.2 % CPU on
  the 4-core VM; `.pipeline/evidence-0.10.0/p3/`.

**Krunk's logo** (`brand` in `cross-build.sh`, since 0.12.1): `assets/brand/krunk-256.png` is
copied over `chrome/app/theme/chromium/product_logo_256.png` (`IDR_PRODUCT_LOGO_256`). The engine
gives that image to Windows as the media overlay's artwork when a site sets none
(`SystemMediaControlsNotifier::UpdateIcon` → `GetProductLogo`), so the overlay showed Chromium's
logo under Krunk's name. Only `resources.pak` changes (an 8-second incremental build). The file
is a local edit of the Chromium checkout: `git -C chromium/src checkout
chrome/app/theme/chromium/product_logo_256.png` before a `gclient sync`.
