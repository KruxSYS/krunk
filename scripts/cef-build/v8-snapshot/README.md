# V8 context snapshot for the cross-built engine (0.26.0 candidate F-snap, draft, not run)

**Status: prepared, not built or run.** It runs only if P1
(`.pipeline/evidence-0.26.0/p1/summary.md` §5) measures the plain-snapshot context cost at
≥ 10 ms more per main-frame load than Chrome's, or ≥ 3 % of LCP. Otherwise it stays here as
"tried in analysis, dropped", with the numbers.

## Why
- Chromium switches the Blink context snapshot off for Windows builds made on Linux
  (`tools/v8_context_snapshot/v8_context_snapshot.gni`: `!(is_win && host_os != "win")`). The
  generator is a Blink program that must run on the target OS.
- Krunk's engine therefore loads V8's plain `snapshot_blob.bin`. Every new page context (each frame
  and iframe) then builds the `Window` global from templates and runs `v8::Context::New`, instead
  of deserialising a ready context (`LocalWindowProxy::CreateContext`,
  `third_party/blink/renderer/bindings/core/v8/local_window_proxy.cc:322-371`).

## How (three steps, `v8-snapshot.sh`)
1. **`build`:**
   - Applies `chromium-v8-context-snapshot-external.patch` (idempotent). It adds the GN arg
     `krunk_external_v8_context_snapshot`: with it, `v8_context_snapshot_generator` is built for
     the **target** (Windows) toolchain and the `generate_v8_context_snapshot` action, which would
     run it on the Linux host, is left out.
   - Runs `cross-build.sh configure` with the usual `GN_DEFINES` plus
     `use_v8_context_snapshot=true krunk_external_v8_context_snapshot=true`.
   - Then `cross-build.sh build` with `KRUNK_EXTRA_TARGETS=v8_context_snapshot_generator`.
   - **Cost:** the flag changes `tools/v8_context_snapshot/buildflags.h`, which
     `content_features.h` and `content_switches.h` include, so this is a near-full rebuild of
     content, Blink, chrome and CEF plus the libcef ThinLTO link. Estimate 3–6 h on the 32-core
     host (`.pipeline/cef-codecs-build.md`), plus a second large link for the generator.
   - **PGO:** the profiles stay valid for unchanged code; the few functions behind the flag get
     none. Later engine builds are incremental again.
2. **`generate`:**
   - Pushes `v8_context_snapshot_generator.exe`, `snapshot_blob.bin` and `icudtl.dat` from the out
     dir to the Windows VM (`C:\pipeline\krunk\v8snap`) and runs
     `v8_context_snapshot_generator.exe --snapshot_blob=snapshot_blob.bin --output_file=v8_context_snapshot.bin`
     (under a minute).
   - Pulls the file into the out dir and records its SHA-256 next to libcef.dll's
     (`v8_context_snapshot.pair.sha256`).
3. **`distrib`:**
   - Checks that pair, then runs `cross-build.sh distrib`. The patched `make_distrib.py` already
     treats `v8_context_snapshot.bin` as a conditional file, so it goes into `Release/`.
   - The archive gets the flavour suffix `-snap` (`CEF_FLAVOR_SUFFIX`). Then re-pin as usual:
     `cef.lock`, `engine-files.sha256`, `CEF_PATCHES` with this patch's hash.

## Rules once it is on
- **The snapshot belongs to exactly one libcef.dll.**
  - V8 checks its version and a checksum at start (`v8/src/snapshot/snapshot.cc:165-183`) and
    aborts on a mismatch.
  - With the flag, gin loads only `v8_context_snapshot.bin`, with no fallback to
    `snapshot_blob.bin` (`gin/v8_initializer.cc:618-641`, `LOG(FATAL)`).
  - So **every engine rebuild needs `generate` again**, and the package check in
    `app/src/ffi/cef/entry.rs` must then require `v8_context_snapshot.bin` (today it accepts
    either file).
- **ICF stays off for the generator** (`/OPT:NOICF`, `:disable_icf` in the BUILD.gn); ICF in
  libcef doesn't matter (the reference tables are by index).
- **The VM smoke must start Krunk** with the new archive: a bad snapshot only shows as a crash at
  start.
- **Disk:** the build is in place in `out/Release_GN_x64` (13 GB; `/work` had 98 GB free on
  2026-11-20). A second out dir is not needed.
