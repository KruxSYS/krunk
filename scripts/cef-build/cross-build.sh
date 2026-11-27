#!/usr/bin/env bash
# Krunk's CEF cross build (Linux host → Windows x64), see README.md in this folder.
#
#   scripts/cef-build/cross-build.sh checkout     depot_tools + CEF/Chromium checkout (pinned)
#   scripts/cef-build/cross-build.sh toolchain    register the Windows toolchain package, win deps
#   scripts/cef-build/cross-build.sh configure    patch CEF's platform detection, gn gen
#   scripts/cef-build/cross-build.sh build        Krunk's CEF patches (patches/*.patch), logo, ninja
#   scripts/cef-build/cross-build.sh distrib      minimal distribution archive
#
# Everything lives under $CEF_BUILD_ROOT (default /work/projects/extra/browser/.pipeline/cef-build), outside the repository. No
# FUSE is needed: the toolchain's case problem is handled by casefix.py (see README.md).
set -euo pipefail

ROOT=${CEF_BUILD_ROOT:-/work/projects/extra/browser/.pipeline/cef-build}
CEF_BRANCH=8078                    # Chromium 156
CEF_COMMIT=cf671c0                 # CEF 156.0.15
CEF_VERSION=156.0.15
CHROMIUM_VERSION=156.0.8078.80
TOOLCHAIN_HASH_KEY=3bfcb536c8      # TOOLCHAIN_HASH in src/build/vs_toolchain.py at this Chromium
# The archive's flavour: krunk-codecs (0.5.2–0.9.0); -hidden since 0.10.0 (patches/); -brand
# since 0.12.1 (Krunk's logo as the engine's product logo, see brand()); -private since 0.12.3
# (patches/chromium-private-media.patch); -nav since 0.18.0; -preconnect since 0.26.0
# (patches/chromium-preconnect-blocked-hosts.patch, krunk-preconnect-pref.patch); -ua since 0.27.0
# (patches/chromium-ua-brand.patch); -audible-chips since 0.28.0 (patches/krunk-audible-state.patch,
# patches/krunk-chips-cookies.patch); -kx since 2026-11-24 (the KruxSYS-style Krunk logo as the
# product logo, brand(); only resources.pak changes); -xml since 2026-11-24
# (patches/chromium-xml-viewer-csp.patch: the XML tree viewer keeps its style under a strict CSP);
# -cast since 0.30.0 (patches/krunk-cast-bridge.patch, chromium-cast-dialog.patch: Krunk's own
# device picker on Chromium's Media Router).
CEF_FLAVOR=krunk-codecs-hidden-brand-private-nav-preconnect-ua-audible-chips-kx-xml-cast${CEF_FLAVOR_SUFFIX:-}
PATCHES=$(cd "$(dirname "$0")" && pwd)/patches
REPO=$(cd "$(dirname "$0")/../.." && pwd)
SRC=$ROOT/chromium_git/chromium/src
export PATH=$ROOT/chromium_git/depot_tools:$ROOT/depot_tools:$PATH
export DEPOT_TOOLS_UPDATE=0
export GN_DEFINES=${GN_DEFINES:-"is_official_build=true proprietary_codecs=true ffmpeg_branding=Chrome target_os=\"win\" target_cpu=\"x64\" enable_platform_hevc=true enable_platform_ac3_eac3_audio=true symbol_level=0"}

toolchain_env() {
  local pkg
  pkg=$(ls "$ROOT"/toolchain/*.zip 2>/dev/null | head -1) || true
  [ -n "$pkg" ] || { echo "no toolchain package in $ROOT/toolchain (see toolchain-package.ps1)" >&2; exit 2; }
  export DEPOT_TOOLS_WIN_TOOLCHAIN=1
  export DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL=$ROOT/toolchain
  export "GYP_MSVS_HASH_${TOOLCHAIN_HASH_KEY}=$(basename "$pkg" .zip)"
  export CEF_CROSS_TARGET=windows
  export KRUNK_NO_CIOPFS=1
}

# Krunk's own patches (0.10.0), applied once, idempotently: patches/chromium-*.patch to the
# Chromium checkout (0.12.3), every other one to the CEF checkout.
apply_patches() {
  local p dir
  for p in "$PATCHES"/*.patch; do
    case "$(basename "$p")" in chromium-*) dir=$SRC ;; *) dir=$SRC/cef ;; esac
    if git -C "$dir" apply --check "$p" 2>/dev/null; then
      git -C "$dir" apply "$p"
      echo "applied $(basename "$p") ($(sha256sum "$p" | cut -c1-16))"
    elif git -C "$dir" apply -R --check "$p" 2>/dev/null; then
      echo "already applied: $(basename "$p")"
    else
      echo "error: $(basename "$p") does not apply to $dir" >&2
      exit 1
    fi
  done
}

# Krunk's logo in place of Chromium's (0.12.1): the engine shows its product logo as the media
# overlay's artwork when a site gives none (SystemMediaControlsNotifier::UpdateIcon ->
# GetProductLogo -> IDR_PRODUCT_LOGO_256 in resources.pak). Copied over the source PNG, so only
# the resource pack changes; a `git checkout` of the file undoes it before a gclient sync.
brand() {
  local logo=$SRC/chrome/app/theme/chromium/product_logo_256.png
  if cmp -s "$REPO/assets/brand/krunk-256.png" "$logo"; then
    echo "already branded: product_logo_256.png"
  else
    cp "$REPO/assets/brand/krunk-256.png" "$logo"
    echo "branded: product_logo_256.png <- assets/brand/krunk-256.png"
  fi
}

case "${1:-}" in
  checkout)
    mkdir -p "$ROOT" && cd "$ROOT"
    [ -d depot_tools ] || git clone --depth 1 https://chromium.googlesource.com/chromium/tools/depot_tools.git depot_tools
    [ -f automate-git.py ] || curl -sSLO https://bitbucket.org/chromiumembedded/cef/raw/master/tools/automate/automate-git.py
    python3 automate-git.py --download-dir="$ROOT/chromium_git" --branch=$CEF_BRANCH --checkout=$CEF_COMMIT \
      --no-build --no-distrib --x64-build
    ;;
  toolchain)
    toolchain_env
    cd "$ROOT/chromium_git/chromium"
    # Windows-only deps (clang's Windows runtime library, dxc, win_build_output, ...) and the
    # Windows PGO profile come with target_os=['win'] (gclient keeps the host OS too) and
    # checkout_pgo_profiles; both need a sync, which refuses to run over local edits.
    grep -q "target_os" .gclient || printf "\ntarget_os = ['win']\n" >> .gclient
    grep -q "'win'" .gclient || sed -i "s/target_os = \[/target_os = ['win', /" .gclient
    sed -i "s/'checkout_pgo_profiles': False/'checkout_pgo_profiles': True/" .gclient
    cat .gclient
    (cd src && git checkout -- build/vs_toolchain.py)
    gclient sync --nohooks --with_branch_heads --revision src@refs/tags/$CHROMIUM_VERSION --jobs ${GCLIENT_JOBS:-8}
    # No FUSE here: Chromium's vs_toolchain.py would mount the toolchain through ciopfs for a
    # case-insensitive view. Skip that (local, idempotent edit) and fix the case with casefix.py.
    python3 - <<'EOF'
p = "src/build/vs_toolchain.py"
s = open(p).read()
old = "        if sys.platform.startswith('linux') and not os.path.ismount(\n            toolchain_dir\n        ):"
new = "        if sys.platform.startswith('linux') and not os.environ.get('KRUNK_NO_CIOPFS') and not os.path.ismount(\n            toolchain_dir\n        ):"
if new not in s:
    assert old in s, "vs_toolchain.py changed; update the ciopfs skip"
    open(p, "w").write(s.replace(old, new, 1))
    print("patched", p)
EOF
    gclient runhooks
    python3 src/build/vs_toolchain.py update --force
    tc=$(python3 src/build/vs_toolchain.py get_toolchain_dir | sed -n 's/^vs_path = "\(.*\)"$/\1/p')
    echo "toolchain dir: $tc"
    python3 "$REPO/scripts/cef-build/casefix.py" "$tc" "$SRC/third_party/llvm-build/Release+Asserts" "$ROOT/casefix"
    ;;
  configure)
    toolchain_env
    cd "$SRC/cef"
    # CEF's tools decide the platform from sys.platform: let CEF_CROSS_TARGET=windows override it
    # (a local change of the checkout, idempotent, never committed to CEF).
    python3 - <<'EOF'
import re
for p, old, new in [
  ("tools/gn_args.py", "if sys.platform == 'win32':\n  platform = 'windows'", "if sys.platform == 'win32' or os.environ.get('CEF_CROSS_TARGET') == 'windows':\n  platform = 'windows'"),
  ("tools/gn_args.py", "machine = 'arm64' if python_platform.machine() == 'ARM64' else 'amd64'", "machine = 'arm64' if python_platform.machine() in ('ARM64', 'aarch64') else 'amd64'"),
  ("tools/gclient_hook.py", "if sys.platform == 'win32':\n  platform = 'windows'", "if sys.platform == 'win32' or os.environ.get('CEF_CROSS_TARGET') == 'windows':\n  platform = 'windows'"),
  ("tools/make_distrib.py", "  if sys_platform == 'win32':\n    return 'windows'", "  if sys_platform == 'win32' or os.environ.get('CEF_CROSS_TARGET') == 'windows':\n    return 'windows'"),
  # No V8 context snapshot in a cross build (see README.md): V8's plain snapshot_blob.bin goes
  # into the distribution instead.
  ("tools/make_distrib.py", "        {'path': 'v8_context_snapshot.bin'},\n",
   "        {'path': 'v8_context_snapshot.bin', 'conditional': True},\n        {'path': 'snapshot_blob.bin', 'conditional': True},\n"),
]:
    s = open(p).read()
    if new in s:
        continue
    assert old in s, (p, old)
    s = s.replace(old, new, 1)
    if "import os" not in s:
        s = s.replace("import sys", "import os\nimport sys", 1)
    open(p, "w").write(s)
    print("patched", p)
EOF
    # CEF's Windows configuration turns on Widevine CDM host verification, storage ID and RLZ.
    # GN evaluates args.gn once for every toolchain, and two Chromium asserts only accept those
    # on Windows/Mac: in a native build host == target, in a cross build the Linux *host*
    # toolchains (which only build generator tools) trip them. Relax the two asserts for the
    # host toolchains (current_os != target_os; local, idempotent edit of the checkout).
    python3 - <<'EOF2'
for p, old, new in [
  ("../media/media_options.gni",
   "assert(!enable_cdm_host_verification || is_mac || is_win,",
   "assert(!enable_cdm_host_verification || is_mac || is_win ||\n           current_os != target_os,"),
  ("../rlz/buildflags/buildflags.gni",
   "assert(!enable_rlz || enable_rlz_support,",
   "assert(!enable_rlz || enable_rlz_support || current_os != target_os,"),
  # Chromium turns the V8 context snapshot off in a Windows cross build (its generator is a
  # Windows program), so the engine loads snapshot_blob.bin instead. mini_installer_archive
  # (never built by CEF, but evaluated by gn gen) lists that file without a dependency path to
  # the target that makes it, which gn rejects. Give it the dependency.
  ("../chrome/installer/mini_installer/BUILD.gn",
   '      inputs += [ "$root_out_dir/snapshot_blob.bin" ]\n      args += [ "--include_snapshotblob=1" ]',
   '      inputs += [ "$root_out_dir/snapshot_blob.bin" ]\n      deps += [ "//v8:run_mksnapshot_default" ]\n      args += [ "--include_snapshotblob=1" ]'),
]:
    s = open(p).read()
    if new in s:
        continue
    assert old in s, (p, old)
    open(p, "w").write(s.replace(old, new, 1))
    print("patched", p)
EOF2
    echo "GN_DEFINES=$GN_DEFINES"
    python3 tools/gclient_hook.py
    cat "$SRC/out/Release_GN_x64/args.gn"
    ;;
  build)
    toolchain_env
    apply_patches
    brand
    cd "$SRC"
    # KRUNK_EXTRA_TARGETS: more ninja targets (v8-snapshot/v8-snapshot.sh adds the generator).
    # shellcheck disable=SC2086
    autoninja -C out/Release_GN_x64 cefsimple bootstrap bootstrapc ${KRUNK_EXTRA_TARGETS:-}
    ls -la out/Release_GN_x64/libcef.dll out/Release_GN_x64/bootstrap.exe
    ;;
  distrib)
    toolchain_env
    cd "$SRC/cef"
    rm -rf "$ROOT/distrib"
    python3 tools/make_distrib.py --output-dir="$ROOT/distrib" --allow-partial --ninja-build --x64-build --minimal --no-archive --no-docs
    # The folder inside the archive must carry the archive's own name: the cef crate's
    # export-cef-dir derives one from the other.
    name="cef_binary_${CEF_VERSION}+g${CEF_COMMIT}+chromium-${CHROMIUM_VERSION}_windows64_minimal_${CEF_FLAVOR}"
    cd "$ROOT/distrib" && dir=$(ls -d cef_binary_* | head -1) && { [ "$dir" = "$name" ] || mv "$dir" "$name"; }
    # No V8 context snapshot in a cross build (see README.md): ship V8's plain snapshot (the
    # patched make_distrib.py copies it; this is the fallback).
    [ -f "$name/Release/v8_context_snapshot.bin" ] || [ -f "$name/Release/snapshot_blob.bin" ] || cp "$SRC/out/Release_GN_x64/snapshot_blob.bin" "$name/Release/"
    ls -la "$name/Release" "$name/Resources"
    tar -cjf "$ROOT/$name.tar.bz2" "$name"
    sha256sum "$ROOT/$name.tar.bz2"
    ;;
  *) sed -n 2,13p "$0"; exit 2 ;;
esac
