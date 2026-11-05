#!/usr/bin/env bash
# Builds the Windows release of Krunk on Linux and packages it:
#
#   scripts/build-dist.sh            release build -> dist/Krunk-win-x64/ + .zip +
#                                    Krunk-Setup-<version>.exe + SHA256SUMS
#   scripts/build-dist.sh --debug    debug build, same layout (for testing; not for release)
#   scripts/build-dist.sh --dev      release build with the dev-updates feature (the update
#                                    check also accepts http://) -> target/dev-dist/, never dist/
#
# UPDATE_BASE_URL (default https://krunk.209.wf, the live server) is where the update check looks
# and what the installer names as the homepage, and
# KRUNK_UPDATE_PUBKEY_FILE the public key it trusts (default assets/keys/update-signing.pub):
#   UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh
#
# Requirements (see README, "Building"): Rust with the x86_64-pc-windows-msvc target,
# cargo-xwin, export-cef-dir (same version as the cef crate), LLVM 20 (clang-cl, lld-link,
# llvm-lib) in $LB_LLVM or on PATH, cmake, ninja, curl, python3, zip, dpkg-deb (NSIS is
# fetched by scripts/fetch-nsis.sh).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
profile=release
dist="$root/dist"
features=()
case "${1:-}" in
  --debug) profile=debug ;;
  --dev) dist="$root/target/dev-dist"; features=(--features dev-updates) ;;
  "") ;;
  *) echo "usage: $0 [--debug | --dev]" >&2; exit 2 ;;
esac

# The update server, also passed to build.rs (app/build.rs has the same default).
export UPDATE_BASE_URL="${UPDATE_BASE_URL:-https://krunk.209.wf}"

# shellcheck source=scripts/xwin-env.sh
source "$root/scripts/xwin-env.sh"

target=x86_64-pc-windows-msvc
args=(--target "$target" -p app --lib "${features[@]}")
[ "$profile" = release ] && args+=(--release)
cargo xwin build "${args[@]}"

out="$dist/Krunk-win-x64"
# dist/ holds exactly the latest build (dist/server/ is source, tracked in git).
rm -rf "$out" "$dist"/LightBrowser-win-x64
rm -f "$dist"/*.zip "$dist"/*.exe "$dist"/SHA256SUMS
mkdir -p "$out/locales"
# Krunk.exe is CEF's sandbox bootstrap; it loads Krunk.dll from its folder.
# tools/pebrand gives it Krunk's version strings and manifest (the code is unchanged).
cargo build -q --release --manifest-path tools/pebrand/Cargo.toml
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
manifest=$(mktemp); trap 'rm -f "$manifest"' EXIT
sed "s/@VERSION@/$version.0/" app/res/Krunk.exe.manifest > "$manifest"
tools/pebrand/target/release/pebrand brand "$CEF_PATH/bootstrap.exe" "$out/Krunk.exe" \
  "$manifest" "$version" assets/brand/krunk.ico
cp "target/$target/$profile/Krunk.dll" "$out/"
for f in libcef.dll chrome_elf.dll d3dcompiler_47.dll dxcompiler.dll vk_swiftshader.dll \
         vk_swiftshader_icd.json vulkan-1.dll icudtl.dat v8_context_snapshot.bin resources.pak \
         chrome_100_percent.pak chrome_200_percent.pak; do
  cp "$CEF_PATH/$f" "$out/"
done
cp "$CEF_PATH"/locales/*.pak "$out/locales/"
mkdir -p "$out/licenses"
cp "$CEF_PATH/LICENSE.txt" "$out/licenses/CEF-LICENSE.txt"
cp "$CEF_PATH/CREDITS.html" "$out/licenses/Chromium-CREDITS.html"
{
  echo "Krunk third-party notices"
  echo "========================="
  echo
  echo "Krunk itself is MIT-licensed. It bundles or embeds the components below."
  echo
  echo "1. Chromium Embedded Framework (CEF) $CEF_VERSION and Chromium $CHROMIUM_VERSION"
  echo "--------------------------------------------------------------------------------"
  echo "libcef.dll, chrome_elf.dll, Krunk.exe (CEF's bootstrap.exe), the .pak, .dat and .bin"
  echo "files, locales/ and the graphics DLLs come unmodified from the CEF binary distribution"
  echo "($CEF_ARCHIVE)."
  echo "CEF licence (BSD): licenses/CEF-LICENSE.txt."
  echo "Chromium and the third-party code it contains: licenses/Chromium-CREDITS.html."
  echo
  echo "2. Bundled filter lists"
  echo "-----------------------"
  cat assets/filters/ATTRIBUTION.md
  echo
  echo "Filter lists fetched on: $(cat assets/filters/FETCHED)"
  echo
  echo "3. GNU General Public License v3.0 (applies to the uBlock Origin lists; EasyList/EasyPrivacy are dual GPL-3.0-or-later / CC BY-SA 3.0)"
  echo "------------------------------------------------------------------------------------------------"
  cat assets/filters/LICENSE-GPL-3.0.txt
  echo
  echo "4. Creative Commons Attribution-ShareAlike 3.0"
  echo "----------------------------------------------"
  cat assets/filters/LICENSE-CC-BY-SA-3.0.txt
} > "$out/THIRD-PARTY-NOTICES.txt"

# The package ships no Markdown: README and SECURITY are read on the website and in the
# repository. Fail rather than ship one that slipped in (checked before the installer and zip).
stray_md=$(find "$out" -type f -iname '*.md')
if [ -n "$stray_md" ]; then
  echo "error: Markdown files in the package (remove them or stop copying them):" >&2
  echo "$stray_md" >&2
  exit 1
fi

# Per-user installer (NSIS, see installer/krunk.nsi).
makensis=$("$root/scripts/fetch-nsis.sh")
setup="Krunk-Setup-$version.exe"
NSISDIR="$(dirname "$(dirname "$makensis")")/share/nsis" "$makensis" -V2 -NOCD \
  -DVERSION="$version" -DSRC="$out" -DOUT="$dist/$setup" -DICON="$root/assets/brand/krunk.ico" \
  -DSIDE_IMAGE="$root/assets/brand/installer-side.bmp" -DHOMEPAGE="$UPDATE_BASE_URL" \
  "$root/installer/krunk.nsi"

cd "$dist"
rm -f Krunk-win-x64.zip SHA256SUMS
# Fixed timestamps and order so the archive only changes when its contents do.
find Krunk-win-x64 -exec touch -h -d "@${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}" {} +
find Krunk-win-x64 -type f | LC_ALL=C sort | zip -X -q -9 Krunk-win-x64.zip -@
sha256sum "$setup" Krunk-win-x64.zip Krunk-win-x64/Krunk.exe Krunk-win-x64/Krunk.dll \
  Krunk-win-x64/libcef.dll > SHA256SUMS
du -sh Krunk-win-x64 Krunk-win-x64.zip "$setup"
echo "CEF $CEF_VERSION (Chromium $CHROMIUM_VERSION), $profile build: $out"
