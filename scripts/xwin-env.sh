# Environment for cross-building Krunk.dll with cargo-xwin. Sourced by build-dist.sh; also
# usable by hand for Windows checks:
#   source scripts/xwin-env.sh && cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
root=${root:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
# shellcheck source=/dev/null
source "$root/cef.lock"
export PATH="${LB_LLVM:-$HOME/.cache/krunk-toolchain/llvm-20.1.8}/bin:$PATH"

# The engine: verified CEF binary distribution (see scripts/fetch-cef.sh). CEF_PATH must be
# the exported folder itself, or cef-dll-sys downloads its own unverified copy.
CEF_PATH=$("$root/scripts/fetch-cef.sh" | tail -n 1)
export CEF_PATH

# cargo-xwin downloads the MSVC CRT and the Windows SDK (headers and import libraries) from
# Microsoft. Setting XWIN_ACCEPT_LICENSE=1 accepts Microsoft's licence terms for them; it is
# set here only, for this build. The versions are pinned so builds are reproducible.
export XWIN_ACCEPT_LICENSE=1
export XWIN_CACHE_DIR="${XWIN_CACHE_DIR:-$HOME/.cache/krunk-xwin}"
export XWIN_VERSION=17
export XWIN_CRT_VERSION=14.44.17.14
export XWIN_SDK_VERSION=10.0.26100

# Compiled dependencies are shared between target directories (worktrees, the fast and the
# release profile after a clean) when sccache is installed; without it nothing changes.
if [ -z "${RUSTC_WRAPPER:-}" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache
fi
