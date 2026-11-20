# Page-load harness (0.26.0)

Measures how fast pages load in Krunk, Chrome and Edge on the Windows test VM. The same recorded
pages are replayed for every browser, so network noise drops out. Nothing here ships with Krunk.

## Parts
- **Web Page Replay** (`wpr.exe`, from chromium.googlesource.com/webpagereplay, cross-compiled
  with Go) serves a recorded corpus on 127.0.0.1:8080/8081. It uses a private test CA
  (`krunk-replay-ca.pem`/`.key`, 90 days). `load-run.sh` trusts the CA in the VM's machine Root
  store for the length of a pass and removes it afterwards.
- **`lshim.exe`** (`load/kinput/src/bin/lshim.rs`) sits in front of WPR and adds a fixed RTT
  (default 40 ms):
  - one RTT per connection (the TCP handshake);
  - RTT/2 per chunk in each direction.

  WPR speaks TCP only, so every browser runs with QUIC off.
- **Host mapping:** every browser is pointed at the shim with
  `--host-resolver-rules=MAP *:80 127.0.0.1:18080, MAP *:443 127.0.0.1:18443, EXCLUDE localhost`.
  Krunk forbids that flag, so it takes the rules from `KRUNK_HOST_RULES` in **harness builds**
  only (`scripts/build-dist.sh --harness` → `target/harness-dist/`: release build plus the
  `perf-harness` feature, which also opens the DevTools port).
- **Driver:** `load/load.mjs`, run with portable `node.exe` in the VM's desktop session by
  `scripts/vm-0260-load.ps1`.
  - It drives the browsers with real input through `kinput.exe` (`load/kinput`: SendInput, logging
    each input's wall-clock time and QPC).
  - It reads the times with CDP: `load/observer.js`, a buffered PerformanceObserver and Navigation
    Timing script, is installed the same way in all three browsers.
  - Times are reported in ms **from the input**, so Krunk's omnibox → engine hand-off is included.
- **Summary:** `load-summary.py` gives median and p90 per page and the corpus value (median of the
  per-page medians). Two copies of the same Krunk build (`krunk-a`, `krunk-b`) give the noise band.

## Scenarios
- **`typed`:** fresh profile → start → white page → click, Ctrl+L, type the URL, Enter (**cold**)
  → white page → the same again (**warm**: HTTP and code cache, the predictor's data).
- **`link`:** fresh profile → start → a page that is one full-window link → click (**link**) →
  Alt+Left (**back**) → Alt+Right (**forward**). Back/forward-cache restores count from the input
  to the pageshow and the next frame. `Page.backForwardCacheNotUsed` reasons are kept.

Runs are interleaved, page by page, with the browser order rotated each run.

## Running it
```sh
scripts/build-dist.sh --harness
KRUNK_DIST=target/harness-dist KRUNK_APP=app260h scripts/vm.sh deploy
scripts/vm.sh ps1 scripts/vm-0260-prep.ps1        # Krunk base profile lp-base (filter lists, QUIC off)
# once: record the corpus (Chrome, real network) into C:\pipeline\krunk\load\corpus.wprgo
python3 scripts/perf/load/mkconfig.py record 1 chrome typed > /tmp/rec.json
scripts/perf/load-run.sh record /tmp/rec.json
# a pass: 5 runs, all pages, three browsers
python3 scripts/perf/load/mkconfig.py replay 5 krunk,chrome,edge typed,link > /tmp/c.json
scripts/perf/load-run.sh baseline /tmp/c.json     # -> ../.pipeline/evidence-0.26.0/runs/baseline/summary.md
```
- **Other Krunk builds:** `KRUNK_<NAME>_APP` / `KRUNK_<NAME>_BASE` name the app folder and base
  profile on the VM for a browser called `krunk-<name>`.
- **Load trace:** `KRUNK_ENV='{"KRUNK_PERF_TRACE":"1"}'` turns it on (`perf.log` in the copied
  profile).
- **Live sites:** `mkconfig.py live …` uses the live set without replay. Those numbers are noisy
  and only a sanity check.

The tools (`wpr.exe`, `lshim.exe`, `kinput.exe`, `node.exe`, the CA) are kept in
`../.pipeline/loadtools` (not in git) and pushed by `load-run.sh` when they changed. The recorded
archive stays on the VM, with a copy in `evidence-0.26.0/corpus/`.
