#!/usr/bin/env bash
# Krunk 0.26.0 candidate F-snap (NOT run; only if P1 measures the plain-snapshot context cost at
# >= 10 ms per main-frame load or >= 3 % of LCP, evidence-0.26.0/p1/summary.md §5). See README.md
# in this folder.
#
#   scripts/cef-build/v8-snapshot/v8-snapshot.sh build      GN patch, gn args, full rebuild + generator
#   scripts/cef-build/v8-snapshot/v8-snapshot.sh generate   run the generator on the Windows VM
#   scripts/cef-build/v8-snapshot/v8-snapshot.sh distrib    the archive with v8_context_snapshot.bin
#
# Hours of host CPU (a near-full rebuild): never while a VM test pass runs (/tmp/krunk-vm-busy).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CB=$HERE/..
ROOT=${CEF_BUILD_ROOT:-/work/projects/extra/browser/.pipeline/cef-build}
SRC=$ROOT/chromium_git/chromium/src
OUT=$SRC/out/Release_GN_x64
PATCH=$HERE/chromium-v8-context-snapshot-external.patch
VMDIR=C:/pipeline/krunk/v8snap
vm() { "$CB/../vm.sh" "$@" </dev/null; }
# The engine's usual args plus the two snapshot args (cross-build.sh reads GN_DEFINES).
base_defines() {
  sed -n 's/^export GN_DEFINES=\${GN_DEFINES:-"\(.*\)"}$/\1/p' "$CB/cross-build.sh" | sed 's/\\"/"/g'
}

case "${1:-}" in
  build)
    [ -e /tmp/krunk-vm-busy ] && { echo "a VM pass runs: no engine build now" >&2; exit 2; }
    if git -C "$SRC" apply --check "$PATCH" 2>/dev/null; then
      git -C "$SRC" apply "$PATCH" && echo "applied $(basename "$PATCH")"
    elif git -C "$SRC" apply -R --check "$PATCH" 2>/dev/null; then
      echo "already applied: $(basename "$PATCH")"
    else
      echo "error: $(basename "$PATCH") does not apply" >&2; exit 1
    fi
    export GN_DEFINES="$(base_defines) use_v8_context_snapshot=true krunk_external_v8_context_snapshot=true"
    echo "GN_DEFINES=$GN_DEFINES"
    "$CB/cross-build.sh" configure
    KRUNK_EXTRA_TARGETS=v8_context_snapshot_generator "$CB/cross-build.sh" build
    ls -la "$OUT/v8_context_snapshot_generator.exe" "$OUT/snapshot_blob.bin" "$OUT/icudtl.dat"
    ;;
  generate)
    # The generator runs Blink in-process (no GPU, no window) with --predictable; it needs the
    # same build's snapshot_blob.bin (and icudtl.dat next to it, to be safe).
    vm sh "New-Item -ItemType Directory -Force $VMDIR | Out-Null; Remove-Item $VMDIR/v8_context_snapshot.bin -ErrorAction SilentlyContinue"
    for f in v8_context_snapshot_generator.exe snapshot_blob.bin icudtl.dat; do vm put "$OUT/$f" "$VMDIR/$f"; done
    vm sh "Set-Location $VMDIR; .\\v8_context_snapshot_generator.exe --snapshot_blob=snapshot_blob.bin --output_file=v8_context_snapshot.bin; if (\$LASTEXITCODE) { throw \"generator exit \$LASTEXITCODE\" }; (Get-Item v8_context_snapshot.bin).Length"
    vm get "$VMDIR/v8_context_snapshot.bin" "$OUT/v8_context_snapshot.bin"
    sha256sum "$OUT/v8_context_snapshot.bin" "$OUT/libcef.dll" | tee "$OUT/v8_context_snapshot.pair.sha256"
    ;;
  distrib)
    [ -f "$OUT/v8_context_snapshot.bin" ] || { echo "no v8_context_snapshot.bin: run generate first" >&2; exit 2; }
    # The snapshot belongs to exactly this libcef.dll (V8's version and checksum checks would
    # abort the engine at start with another one).
    sha256sum -c "$OUT/v8_context_snapshot.pair.sha256"
    CEF_FLAVOR_SUFFIX=-snap "$CB/cross-build.sh" distrib
    ;;
  *)
    echo "usage: $0 build | generate | distrib" >&2; exit 2 ;;
esac
