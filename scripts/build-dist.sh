#!/usr/bin/env bash
# Builds the Windows release of Krunk on Linux and packages it:
#
#   scripts/build-dist.sh            release build -> dist/Krunk-win-x64/ + .zip +
#                                    Krunk-Setup-<version>.exe + SHA256SUMS
#   scripts/build-dist.sh --debug    debug build, same layout (for testing; not for release)
#   scripts/build-dist.sh --dev      release build with the dev-updates feature (the update
#                                    check also accepts http://) -> target/dev-dist/, never dist/
#   scripts/build-dist.sh --probe    release build with the perf-probe feature (KRUNK_PERF_OFF
#                                    switches page scripts off, for benchmarks) -> target/probe-dist/
#   scripts/build-dist.sh --fast     quick test build (profile "fast": no LTO, incremental, all
#                                    cores) -> target/fast-dist/, folder and zip only, never dist/
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
  --probe) dist="$root/target/probe-dist"; features=(--features perf-probe) ;;
  --fast) profile=fast; dist="$root/target/fast-dist" ;;
  "") ;;
  *) echo "usage: $0 [--debug | --dev | --probe | --fast]" >&2; exit 2 ;;
esac

# The update server, also passed to build.rs (app/build.rs has the same default).
export UPDATE_BASE_URL="${UPDATE_BASE_URL:-https://krunk.209.wf}"

# shellcheck source=scripts/xwin-env.sh
source "$root/scripts/xwin-env.sh"

target=x86_64-pc-windows-msvc
args=(--target "$target" -p app --lib "${features[@]}")
[ "$profile" = release ] && args+=(--release)
[ "$profile" = fast ] && args+=(--profile fast)
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
         vk_swiftshader_icd.json vulkan-1.dll icudtl.dat resources.pak \
         chrome_100_percent.pak chrome_200_percent.pak; do
  cp "$CEF_PATH/$f" "$out/"
done
# The V8 start-up snapshot: CEF's binary distribution has the Blink context snapshot, Krunk's
# cross-built engine (scripts/cef-build) has V8's plain snapshot_blob.bin instead (Chromium
# cannot generate the context snapshot off Windows). Ship whichever the archive contains.
snapshot=""
for f in v8_context_snapshot.bin snapshot_blob.bin; do
  [ -f "$CEF_PATH/$f" ] && { snapshot=$f; cp "$CEF_PATH/$f" "$out/"; break; }
done
[ -n "$snapshot" ] || { echo "no V8 snapshot (v8_context_snapshot.bin or snapshot_blob.bin) in $CEF_PATH" >&2; exit 1; }
echo "V8 snapshot: $snapshot" >&2
# Widevine VMP signing (R25): only with EVS credentials on this host; never blocks the build.
scripts/vmp-sign.sh "$out"
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
  echo
  echo "5. New tab page wallpapers"
  echo "--------------------------"
  echo "The ten built-in wallpapers (Krunk.dll, shared/wallpapers/) were made for Krunk: eight"
  echo "generated with the z-image-turbo model from Krunk's own prompts, two drawn by Krunk's build"
  echo "script. No third-party image is included. Prompts and seeds: assets/wallpapers/SOURCES.txt"
  echo "in Krunk's source repository."
  echo
  echo "6. BIP39 English word list"
  echo "--------------------------"
  echo "The recovery phrase uses the BIP39 English word list and checksum (bitcoin/bips, BIP-0039,"
  echo "through the Rust crate bip39 by Steven Roose et al.), dedicated to the public domain"
  echo "under CC0 1.0 Universal: https://creativecommons.org/publicdomain/zero/1.0/"
  echo
  echo "7. In-browser VPN (OpenVPN 3 Core, Asio, LZ4, boringtun, smoltcp)"
  echo "-----------------------------------------------------------------"
  cat vpn/THIRD-PARTY-NOTICES.txt
} > "$out/THIRD-PARTY-NOTICES.txt"

# The engine files must be the ones Krunk checks itself against (engine-files.sha256, built into
# Krunk.dll by core/src/integrity.rs). After a CEF change: regenerate the list with
#   (cd dist/Krunk-win-x64 && find . -type f ! -name Krunk.exe ! -name Krunk.dll \
#      ! -name THIRD-PARTY-NOTICES.txt ! -path './licenses/*' | sed 's#^\./##' | LC_ALL=C sort | xargs sha256sum) > engine-files.sha256
# and rebuild.
(cd "$out" && sha256sum --quiet -c "$root/engine-files.sha256") || {
  echo "error: the engine files differ from engine-files.sha256 (see the comment above)" >&2; exit 1; }
listed=$(wc -l < "$root/engine-files.sha256")
present=$(cd "$out" && find . -type f ! -name Krunk.exe ! -name Krunk.dll ! -name THIRD-PARTY-NOTICES.txt ! -path './licenses/*' | wc -l)
[ "$listed" -eq "$present" ] || { echo "error: $present engine files in the package, $listed in engine-files.sha256" >&2; exit 1; }

# The package ships no Markdown: README and SECURITY are read on the website and in the
# repository. Fail rather than ship one that slipped in (checked before the installer and zip).
stray_md=$(find "$out" -type f -iname '*.md')
if [ -n "$stray_md" ]; then
  echo "error: Markdown files in the package (remove them or stop copying them):" >&2
  echo "$stray_md" >&2
  exit 1
fi

# A test build stops here: the folder is all the VM needs (scripts/vm.sh deploy copies
# Krunk.dll and Krunk.exe over an installed folder with the same engine).
if [ "$profile" = fast ]; then
  echo "CEF $CEF_VERSION (Chromium $CHROMIUM_VERSION), fast test build: $out"
  exit 0
fi

# Per-user installer (NSIS, see installer/krunk.nsi).
makensis=$("$root/scripts/fetch-nsis.sh")
setup="Krunk-Setup-$version.exe"
# Every file the installer writes, for its check before writing (installer/krunk.nsi, CheckFile).
files_nsh=$(mktemp --suffix=.nsh)
(cd "$out" && find . -type f | sed 's#^\./##' | LC_ALL=C sort) | sed 's#/#\\#g; s#.*#!insertmacro CheckFile "&"#' > "$files_nsh"
NSISDIR="$(dirname "$(dirname "$makensis")")/share/nsis" "$makensis" -V2 -NOCD \
  -DVERSION="$version" -DSRC="$out" -DOUT="$dist/$setup" -DICON="$root/assets/brand/krunk.ico" \
  -DSIDE_IMAGE="$root/assets/brand/installer-side.bmp" -DHOMEPAGE="$UPDATE_BASE_URL" -DFILES_NSH="$files_nsh" \
  "$root/installer/krunk.nsi"

rm -f "$files_nsh"

cd "$dist"
rm -f Krunk-win-x64.zip SHA256SUMS
# Fixed timestamps and order so the archive only changes when its contents do.
find Krunk-win-x64 -exec touch -h -d "@${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}" {} +
find Krunk-win-x64 -type f | LC_ALL=C sort | zip -X -q -9 Krunk-win-x64.zip -@
sha256sum "$setup" Krunk-win-x64.zip Krunk-win-x64/Krunk.exe Krunk-win-x64/Krunk.dll \
  Krunk-win-x64/libcef.dll > SHA256SUMS
du -sh Krunk-win-x64 Krunk-win-x64.zip "$setup"
echo "CEF $CEF_VERSION (Chromium $CHROMIUM_VERSION), $profile build: $out"
