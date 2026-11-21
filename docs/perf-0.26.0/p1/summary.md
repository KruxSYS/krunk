# P1: where page-load time goes (Krunk 0.25.5 vs Chrome 157 / Edge 156)

Status: **done** (2026-11-21): passes 1 and 2 measured; results in §6 and `../fixes.md`.

## 1. Runs

All runs use the same replay corpus and 40 ms RTT shim as the baseline (`scripts/perf/README-load.md`).

### 1a. Krunk's own steps (load trace)
- **Build:** `app260t`, the harness build of e17afe8: 0.25.5 plus the load trace d0c68e4, with the
  trace off unless asked for.
- **Command:**
  ```sh
  KRUNK_T_APP=app260t KRUNK_ENV='{"KRUNK_PERF_TRACE":"1"}' COLLECT='Krunk\perf.log' \
    python3 scripts/perf/load/mkconfig.py replay 3 krunk-t typed,link > /tmp/p1t.json
  OUT=../.pipeline/evidence-0.26.0/p1/trace scripts/perf/load-run.sh p1-trace /tmp/p1t.json
  for f in ../.pipeline/evidence-0.26.0/p1/trace/collect/*perf.log; do
    python3 scripts/perf/load-trace-summary.py "$f"
  done
  ```
- **Gives:** per navigation, input → `load_url` → `before_browse` → main request → commit →
  painted → load end; UI handler time by name; requests, blocked, filter µs (total and max);
  cookie-filter calls.

### 1-method. QUIC in the replay
Web Page Replay serves TCP only. Every browser runs with QUIC off, so none of them races a QUIC
attempt that the replay can't answer:
- Chrome and Edge get `--disable-quic` on their command line (`scripts/perf/load/load.mjs`
  `launch`, the fixed argument list);
- Krunk gets it as an extra flag in its base profile `lp-base` (`scripts/vm-0260-prep.ps1`:
  `setkv {"extraFlags": "--disable-quic"}`), which reaches the engine through
  `Settings::browser_switches`.

- **Check on the VM after the pass:** Krunk's `krunk.log` line "Krunk's switches" must show
  `--disable-quic`. For Chrome and Edge the switch is in the launch arguments the driver passes.
- **Live set:** the same flags apply there, so the live runs are QUIC-off for all three browsers
  too. Real users get QUIC; this is noted as a limit of the live numbers.

### 1b. Engine traces, Krunk vs Chrome (Perfetto / Chrome JSON)
- **Pages:** spa, news, ads. Typed scenario, so one cold and one warm load per trace. 3 runs each.
- **Categories (the same for both browsers):**
  `toplevel,navigation,loading,blink,blink.user_timing,devtools.timeline,v8,v8.execute,disabled-by-default-v8.compile,cc,gpu,renderer_host,content,benchmark,disabled-by-default-devtools.timeline,disabled-by-default-devtools.timeline.frame`
- **Chrome / Edge** (extra switches through the driver; `{profile}` is the session's profile):
  ```sh
  BROWSER_ARGS='["--trace-startup=CATS","--trace-startup-file={profile}\\trace.json","--trace-startup-duration=40"]' \
  COLLECT='trace.json' python3 scripts/perf/load/mkconfig.py replay 3 chrome typed spa,news,ads > /tmp/p1c.json
  ```
- **Krunk:** the same three switches as extra flags in a separate base profile `lp-trace`, a copy of
  `lp-base`:
  ```sh
  python setkv.py lp-trace '{"extraFlags": "--disable-quic --trace-startup=CATS --trace-startup-file=C:\pipeline\krunk\lp\krunk-tr\trace.json --trace-startup-duration=40"}'
  ```
  Then run a browser named `krunk-tr` with `KRUNK_TR_APP=app260h KRUNK_TR_BASE=lp-trace`
  and `COLLECT='trace.json'`. None of the three switches is on Krunk's forbidden list.
- **Analysis:**
  ```sh
  python3 scripts/perf/trace-compare.py krunk=…/krunk-tr-spa-1-typed-trace.json chrome=…/chrome-spa-1-typed-trace.json
  ```
  `trace-compare.py` is the comparison tool. It is a new script rather than an extension of
  `trace-summary.py`, which reads Krunk's own `perf.log`. Per process kind it totals:
  - context creation: `LocalWindowProxy::CreateContext`, `V8.NewContext`,
    `V8ContextSnapshot::CreateContextFromSnapshot`;
  - renderer start: `RenderProcessHostImpl::Init`, `ChildProcessLauncher::Launch`;
  - commit: `NavigationRequest`, `RenderFrameHostImpl::CommitNavigation`;
  - parse: `ParseHTML`, `HTMLDocumentParser::PumpTokenizer`;
  - script: `v8.compile`, `v8.run`, `FunctionCall`, `EvaluateScript`, GC;
  - style/layout/paint: `UpdateLayoutTree`, `Layout`, `Paint`, `PrePaint`, `Commit`;
  - then the largest differences.
- **V8 snapshot decision:** Krunk's `LocalWindowProxy::CreateContext` per context against Chrome's,
  times the contexts per page. Go only if ≥ 10 ms per main-frame load or ≥ 3 % of LCP.

### 1c. Krunk's page scripts (perf-probe)
One harness build with `perf-harness,perf-probe`, `KRUNK_PERF_OFF=all` against the default, typed
scenario, all pages, 3 runs.

### 1d. Back/forward cache
The baseline's `bfcacheNotUsed` reasons per browser and page (`../baseline/summary.md`).

## 1z. Runbook after the baseline (exact order)
1. **Baseline wrap-up** (no build):
   - fill pass and live set by the v2 chain;
   - `lost-keys.py runs/baseline runs/baseline-fill`;
   - Krunk's switches line from `runs/baseline-fill/collect/*krunk.log`;
   - `baseline/summary.md` filled and committed.
2. **Only if the fill pass lost keys again:** the focus-waiting typed pass for all three browsers
   (director-approved):
   `WAIT_FOCUS=1 mkconfig.py replay 5 krunk-a,krunk-b,chrome,edge typed > /tmp/wf.json; OUT=…/runs/baseline-waitfocus load-run.sh baseline-waitfocus /tmp/wf.json`
   (about 70 min).
3. **One harness build** of HEAD: the P1 spans d0c68e4/85a0474, F-filter 8e83f6b, F-pre
   ae4f6d2/bace84f, the switches 71f5c88. Then
   `scripts/build-dist.sh --harness; KRUNK_DIST=target/harness-dist KRUNK_APP=app260n scripts/vm.sh deploy`.
   - The engine is still the 0.25.5 CEF archive: the engine patches (F-omni, F-block) come with
     the engine build after P1.
   - On the old engine `krunk.preconnect` and `krunk.blocked_hosts` are refused, with one warning
     each in `krunk.log`.
4. **P1 attribution + A/B, Krunk only, two interleaved passes.** All configurations are app
   `app260n`, base `lp-base`, `KRUNK_PERF_TRACE=1`, `COLLECT='Krunk\perf.log'`, 5 runs, all
   pages. They differ only in `KRUNK_PERF_OFF`:

   | config | KRUNK_PERF_OFF | what is on | role |
   |---|---|---|---|
   | krunk-base | `preload,omnibox-preconnect,preconnect-block,filter-fast,login-defer` | nothing (0.25.5 behaviour + trace) | attribution; A of every A/B |
   | krunk-pre | `omnibox-preconnect,preconnect-block,filter-fast,login-defer` | F-pre | B for F-pre |
   | krunk-filter | `preload,omnibox-preconnect,preconnect-block,login-defer` | F-filter | B for F-filter |
   | krunk-scripts | `preload,omnibox-preconnect,preconnect-block,filter-fast` | F-scripts | B for F-scripts |

   - **Pass A**, typed + link: krunk-base, krunk-pre. Back/forward and its NotUsed reasons are
     here, for F-bfcache (no code change: reasons only). About 80 min.
   - **Pass B**, typed only: krunk-base, krunk-filter, krunk-scripts. About 55 min.

   Run: `scripts/perf/p1-ab.sh app260n 5`. It makes both configs, runs both passes, writes the
   A/B verdicts against `baseline/numbers.json`'s band (written by the baseline summary step with
   `--json`), and summarises krunk-base's load trace into `p1/load-trace.md`.

5. **Engine traces:** `scripts/perf/p1-traces.sh 3 spa,news,ads`, Krunk 0.25.5 vs Chrome, about
   20 min.
6. **Results:** §6 re-ranked; the measured fixes' follow-up commits ("Measured: F-…") or reverts;
   `../fixes.md`; ask_director with the numbers.

## 2. CEF 156 against Chrome 156 defaults (from the source tree, `cef-tree.md`)

| Feature | Chrome 156 | CEF 156, Alloy style | Krunk 0.25.5 | Evidence |
|---|---|---|---|---|
| Spare RenderProcessHost | on (desktop, site-per-process, > 1 GB RAM) | on, not overridden; one spare for the last context used | on | `content/browser/renderer_host/render_process_host_impl.cc:3558-3584`; `chrome_content_browser_client.cc:2058-2096` (CEF's client doesn't override it) |
| Back/forward cache | on (6 entries) | on; `AlloyBrowserHostImpl::IsBackForwardCacheSupported` = true; CEF handles frames entering and leaving | on; Krunk's page scripts add no `unload` listener (grep) | `cef/libcef/browser/alloy/alloy_browser_host_impl.cc:1149-1152`; `content_features.cc:187` |
| Prerender2 | on (speculation rules, omnibox) | **hard-disabled** in both browser styles | off | `alloy_browser_host_impl.cc:1154`; `cef/libcef/browser/chrome/chrome_browser_web_contents_delegate.cc:120` |
| NoStatePrefetch (`<link rel=prerender>`) | on, ignores the prediction pref | on, runs in a browserless WebContents | **was on and unfiltered**; holdback since 602f01e | `components/no_state_prefetch/browser/no_state_prefetch_manager.cc:473-487`; `content_features.cc:776` |
| LoadingPredictor / PreconnectManager | on, pref "standard" | attached for Alloy style (`TabHelpers::AttachTabHelpers`); follows the pref | **off** (`net.network_prediction_options = 2`); its learning writes `Network Action Predictor` anyway | `cef/libcef/browser/alloy/browser_platform_delegate_alloy.cc:206`; `chrome/browser/predictors/loading_predictor.cc:453-455`; `predictor_database.cc:31-32` |
| Page `<link rel=preconnect / dns-prefetch>` hints | on | follows the pref | off (pref 2) | `chrome/browser/predictors/network_hints_handler_impl.cc:49-113` |
| Speculation-rules prefetch | on | on, through CEF's request interception | off (pref 2) | `content/browser/preloading/prefetch/prefetch_url_loader_factory_utils.cc:44-113` |
| HTTP / code cache size | heuristic, ≤ 320 MB | the same | the same | `cef-tree.md` Q9 |

## 3. Review of the written, not yet built 0.26.0 code (before the first build)

### 3a. Speculative requests take the normal blocking and proxy path

1. **Preconnect and DNS pre-resolve** (page hints, the predictor, pointer-down, the omnibox hook).
   They are socket-level, so `on_before_resource_load` never sees them. The engine patch covers
   that gap.
   - **Blocking:** every entry point drops hosts in Krunk's blocked set. These are tree lines with
     the patch applied:
     - `PreconnectManagerImpl::Start` (`content/browser/preloading/preconnect/preconnect_manager_impl.cc:198`);
     - `StartPreresolveHost` (`:234`), `StartPreresolveHosts` (`:260`), `StartPreconnectUrl` (`:291`, before the direct fast path);
     - Early Hints preconnect (`content/browser/loader/navigation_early_hints_manager.cc:521`);
     - the main navigation's own preconnect (`content/browser/renderer_host/navigation_request.cc:2162`).
   - **The set itself:** made in Krunk from the active lists' plain `||host^` rules
     (`core/src/preconnect_block.rs`), pushed from `Browser::push_preconnect_block`
     (`app/src/browser/blocking.rs:686`):
     - at start: `init_filters`, on both the enabled and the disabled path;
     - after every engine rebuild: `filters_ready`;
     - when ad blocking is switched on or off: `settings_host.rs`.
     It is empty with blocking off. This follows the director's decision: profile-wide, the
     per-site allowlist is ignored.
   - **Proxy:** the connections use the profile's NetworkContext
     (`PreconnectManagerImpl::GetNetworkContext` → the storage partition, `preconnect_manager_impl.cc:348/379/394`).
     The profile's proxy config comes from `ProfileNetworkContextService`, which is where Krunk's
     VPN puts its proxy (`app/src/ffi/cef/app.rs` `apply_vpn`). The pre-resolve does a proxy
     lookup first and resolves no DNS when a fixed proxy is set (`TryToLaunchPreresolveJobs` /
     `OnProxyLookupFinished`), so the VPN's names stay in the tunnel.
   - **Kill switch:**
     - **VPN down with the kill switch on:** Krunk makes its refusing stand-in the profile's proxy
       (`app/src/browser/vpn.rs` `vpn_block` → `engine_vpn_proxy`), so preconnects fail there.
     - **Stand-in could not even start ("offline"):** only the request filter refuses pages. Then
       the prediction pref goes to 2 (`engine::set_preload_offline`, `vpn.rs:226/752`;
       `prediction_value`, `app/src/ffi/cef/app.rs:198-200`), so page hints, the predictor and
       the omnibox hook are off. That includes the main-navigation preconnect, because
       `ShouldPreconnectNavigation` follows the pref. Early Hints need a server answer, and the
       filter refuses the request first.
2. **Speculation-rules prefetch.**
   - **Blocking:** these requests use `URLLoaderFactoryType::kPrefetch`. CEF proxies every
     factory type (`cef/libcef/browser/chrome/chrome_content_browser_client_cef.cc:482-571`), and
     renderer-triggered prefetches carry the referring frame, so Krunk's `TabRequests::on_before_resource_load`
     (`app/src/ffi/cef/net.rs:196`) gets them with the tab's browser. The ad blocker and tracking
     protection apply as to any request. A page later served from the prefetch is not checked a
     second time, but it was checked when fetched.
   - **Proxy:**
     - same-site prefetches use the profile context, so the VPN applies;
     - cross-site prefetches use an isolated context with a direct proxy, but `PrefetchService`
       refuses them when the profile has a proxy (`prefetch_service.cc:1208-1258`, `kExistingProxy`).
       With the VPN or the kill switch's stand-in that is always the case, so nothing goes direct.
     - In offline mode the pref is 2.
   - **Private tabs:** pref 2 and no predictor, so nothing.
3. **Prerender:**
   - Prerender2 is hard-disabled in CEF (table above), so there is no browserless prerender.
   - NoStatePrefetch is held back (602f01e).
   - Still to confirm on the VM (smoke): a speculation-rules `prerender` page triggers nothing,
     and a `prefetch` page shows up in Krunk's blocked log when it hits a blocked host.

### 3b. Private tabs never predict
- `apply_profile_prefs` sets `net.network_prediction_options` to `prediction_value(ctx.is_global())`,
  which is 2 for every non-global context (`app/src/ffi/cef/app.rs:198-200`). It runs in
  `on_request_context_initialized` for the private context.
- `set_preload` and `set_preload_offline` only ever touch the global context.
- An off-the-record profile has no LoadingPredictor at all (`cef-tree.md` Q2), so the omnibox hook
  is a no-op there.
- Krunk also never calls it for a private active tab (`app/src/browser/omnibox.rs:101`).
- Prefs set on the private (off-the-record) profile stay in its in-memory overlay: the prediction
  pref isn't in `kPersistentPrefNames` (`chrome/browser/prefs/pref_service_incognito_allowlist.cc:30`),
  so private=2 never leaks into the normal profile.

### 3c. The setting is wired everywhere
- **core:** `Settings::preload_pages` (default true; serde `preloadPages`).
- **`ui/dev/default-settings.json`:** **was missing.** Added; the core fixture test compares the
  two.
- **sync:** **was missing** from the classification test (`core/src/sync/coll.rs`). It is now
  local; the synced key set shared with Android is unchanged.
- **agent allowlist:** deliberately not on it, because it is network behaviour, like proxy or DNS.
- **Settings page:** General › Tabs and memory (`ui/pages/settings.html`, `data-key="preloadPages"`,
  bound generically like the other checkboxes).
- **app:** start-up value via `ffi::cef::init_preload` (`app/src/lib.rs:112`, before the engine,
  no CEF call), live change via `engine::set_preload` (`settings_host.rs:325`).
- **Omnibox preconnect:** no new shell request. The preconnect starts on the host side inside
  `address_suggest` (the `addressSuggest` request the shell already sends), so no new `mod.rs`
  router arm or Shell permit is needed. The request-router trap doesn't apply.
- **Clear history:** `engine::clear_predictor` in `library.rs` (clear browsing data) and in
  `settings_host.rs` (clear on exit).

### 3f. `--disable-background-networking` (BASE_SWITCHES): kept
- In this engine it gates (grep `kDisableBackgroundNetworking`, non-test `.cc`):
  - Intranet redirect detection (`chrome/browser/intranet_redirect_detector.cc:121`);
  - the Safe Browsing protocol config (`chrome/browser/safe_browsing/chrome_sb_protocol_config_provider.cc:48`);
  - translate's resource requests (`chrome/browser/translate/translate_service.cc:48`);
  - the variations (Finch) service (`chrome/browser/metrics/chrome_metrics_services_manager_client.cc:297`);
  - extension background updates (`chrome/browser/extensions/chrome_extensions_browser_client.cc:610-613`);
  - the upgrade detector (`chrome/browser/upgrade_detector/upgrade_detector_impl.cc:445`).
- None of these is on the page-load path, and none touches LoadingPredictor, PreconnectManager,
  prefetch or the HTTP cache. The switch stays for its privacy reason (no background contacts),
  with no load cost.

### 3g. Interception and proxy of speculative requests: short answer
Details with file:line are in §3a.
- **Speculation-rules prefetch:** goes through `CefResourceRequestHandler::OnBeforeResourceLoad`
  with the tab's browser (`kPrefetch` factory, proxied by CEF), so Krunk's blocker sees it.
  Same-site prefetch uses the profile's proxy. Cross-site prefetch is refused while any proxy is
  set (`kExistingProxy`).
- **Speculation-rules prerender:** none. Prerender2 is hard-disabled in CEF (§4 row 7).
- **`<link rel=prerender>` NoStatePrefetch:** skipped the handler (browserless WebContents). It is
  held back since 602f01e.
- **LoadingPredictor preconnects and DNS:** socket-level, so they never reach the handler; they go
  through the profile's network context and so the proxy, with no local DNS when a proxy is set.
  Krunk's guarantee for these is the blocked-host set (engine patch).
- **Conclusion:** no prefetch or prerender path skips the handler in 0.26.0, so nothing has to be
  feature-disabled. If a later engine version enables Prerender2 for Alloy style, it needs a check
  first: prerendered frames belong to the tab's WebContents, so they should reach the handler with
  a browser (`browser_info.cc:225-238`), but that has to be verified before enabling.

## 3d. How the code that landed unmeasured (ae4f6d2, 602f01e) gets measured, one record per fix

One harness build of the fix code (`build-dist.sh --harness`; `perf-harness` now includes
`perf-probe`). Every piece has its own switch, so each one gets an A/B pass against the same
binary, interleaved, on the scenarios it can affect (5 runs, all corpus pages unless noted). The
reference is the baseline 0.25.5 build (`app260h`) for the "before" column; the A side of each
pair is the new build with only that piece switched off.

| Fix | Switch (off side) | Scenarios | Expected effect | Commit that records the numbers |
|---|---|---|---|---|
| F-pre: preload setting = network prediction 0 in normal tabs (page hints, link-press preconnect, LoadingPredictor, speculation-rules prefetch) | base profile `lp-nopreload` (`setkv {"preloadPages": false}`) vs `lp-base` (default on) | typed cold+warm, link | warm/typed: learned subresource origins preconnected (1 RTT each); link: pointer-down preconnect; cold: page `rel=preconnect` hints | "Preload pages: network prediction in normal tabs (measured)" |
| F-omni: omnibox top-row preconnect (`krunk.preconnect`, engine patch) | `KRUNK_PERF_OFF=omnibox-preconnect` (preload on both sides) | typed cold+warm (the driver types char by char, so the top row exists before Enter) | input→TTFB −1 RTT + TLS on cold typed; less on warm (sockets may be pooled) | "Address bar: preconnect the top suggestion (measured)" |
| F-block: blocked-host set (engine patch, privacy) | `KRUNK_PERF_OFF=preconnect-block` | typed cold, ads page (the most tracker hints) | none on speed (it is a guarantee): the pass shows its cost is within the noise band; netlog proof separately | "Preconnect never to blocked hosts (cost measured)" |
| F-nsp: NoStatePrefetchHoldback (602f01e, privacy) | extra flag `--disable-features=NoStatePrefetchHoldback` in a base profile | typed cold | none expected (no corpus page uses `<link rel=prerender>`; checked by grep of the archive) | numbers added to the CHANGELOG entry; the fix commit is not amended |
| F-filter: cheaper request filtering (not written yet; only if the P1 histogram shows ≥ 1 ms per navigation in the policy) | `KRUNK_PERF_OFF=filter-fast` to switch back to the old path inside one build | typed cold, ads + news | ≤ the measured filter time | its own commit |

Rules:
- A fix counts only if the difference beats the A/A band from the baseline for that scenario and
  metric; otherwise it is reverted (speed fixes) or kept as a guarantee with "no measurable cost"
  (privacy fixes F-block and F-nsp).
- The fix commits that record numbers say that the code itself landed in ae4f6d2/602f01e and that
  the commit adds the measurement (and any change the measurement prompted).
- The per-fix passes run Krunk only (the new build twice, A and B, interleaved); Chrome/Edge come
  in again in the final matrix.

## 3d'. What the corpus asks of the browser (`scripts/perf/load/corpus-scan.cjs`, `../corpus/scan.txt`)

Decompressed bodies and headers of the 1408 recorded responses:
- `rel=preconnect`: 17 responses on 9 hosts. Every corpus page except hn has one: the Guardian,
  react.dev, Bing, IKEA, YouTube, Wikipedia, Allrecipes, plus fonts.googleapis.com.
- `rel=dns-prefetch`: 16 responses (Wikipedia, Bing, the Guardian, YouTube, GTM).
- So F-pre (prediction 0) can act on cold loads through the page's own hints, not only on warm
  loads through the predictor's learning.
- `rel=prerender` and speculation rules: none. F-nsp can't show a speed effect on this corpus (as
  expected: it is a blocking fix), and speculation-rules prefetch doesn't happen in the replay.
- `unload` listeners: Wikipedia (2 responses), which blocks the back/forward cache there in every
  browser.
- `no-store` documents: 18, among them Bing's and YouTube's main documents, so no back/forward
  cache for those pages in any browser unless the browser allows no-store pages in.

## 3e. Code read for injected scripts and navigation-time writes (no VM needed)

- **Injected scripts per page context** (`app/src/ffi/cef/renderer.rs` `on_context_created` →
  `run_scriptlets`, then `context_created`):
  - every frame's main world gets uBO's scriptlets and, as settings say, `gpc.js`, `dnt.js`,
    `nohints.js` (http frames), the audio tracker, the pointer-lock shim, the page bridge and
    `login.js`;
  - main frames also get `page-tokens.js`, `notify.js`, `media.js` and `painted.js`;
  - each one is a synchronous `V8Context::eval` before the page's first script.
  - `document_url(frame)` is read up to 5 times per context (a CEF call each).
  - 85a0474 makes the cost visible per navigation (`ctx`, `scriptlet_us`, `script_us`); the
    perf-probe A/B (`KRUNK_PERF_OFF=all`) gives the end-to-end effect. Candidate #7 only if the
    sum is ≥ ~2 ms per navigation on the corpus.
- **Navigation-time writes on the UI thread** (from the load trace's `ui=` names): history visit
  and title (`library.rs` `record_visit`), the session log, `session_save`, the favicon cache, tab
  list and nav sends. They run in the engine's event handlers between commit and load end; the
  trace measures each. Candidate #8 only if `ui_ms` per navigation is ≥ ~5 ms or one step is a
  stall (≥ 16 ms).

## 4. §feature-diff: CEF 156 Alloy style vs Chrome 156 defaults, verdict per loading feature (#9, #5)

Read from the source in `.pipeline/cef-build/chromium_git/chromium/src`; details and more lines in
`cef-tree.md`. Verdicts: **on** (works as in Chrome, nothing to do), **off** (deliberately off,
stays off), **pref** (Krunk's own setting decides), **patch** (needs an engine patch).

| # | Item | Chrome 156 | CEF 156 Alloy style (file:line) | Krunk 0.25.5 | Verdict |
|---|---|---|---|---|---|
| 1 | Back/forward cache | on, 6 entries | supported: `cef/libcef/browser/alloy/alloy_browser_host_impl.cc:1149-1152` (`IsBackForwardCacheSupported` true); frames entering/leaving handled `cef/libcef/browser/browser_info.cc:244-260`, `browser_contents_delegate.cc:565`; feature `content/public/common/content_features.cc:187` | on; Krunk adds no blocker (§4a) | **on** |
| 2 | Spare RenderProcessHost | one spare kept (desktop, site-per-process, > 1077 MB RAM) | `content/browser/renderer_host/render_process_host_impl.cc:3558-3584`; CEF's content client overrides neither `ShouldUseSpareRenderProcessHost` (`chrome/browser/chrome_content_browser_client.cc:2058-2096`) nor anything in `cef/libcef/browser` (grep: no hits) | on | **on** (no work; one spare per last-used context, so a private tab in between costs the next normal navigation its spare) |
| 3 | Process reuse (`ShouldTryToUseExistingProcessHost`) | only top-Chrome WebUI shares | `chrome/browser/chrome_content_browser_client.cc:2551-2561`, no CEF override; no renderer-process limit set by CEF or Krunk (grep `renderer-process-limit`/`SetMaxRendererProcessCount`: none) | as Chrome | **on** |
| 4 | LoadingPredictor / PreconnectManager for Alloy profiles | created for every non-OTR profile, pref "standard" | Alloy browsers get the full `TabHelpers::AttachTabHelpers` (`cef/libcef/browser/alloy/browser_platform_delegate_alloy.cc:206`), so `LoadingPredictorTabHelper` exists; gated by the pref (`chrome/browser/predictors/loading_predictor.cc:453-455`, `loading_predictor_config.cc:20-25`) | **off**: `net.network_prediction_options = 2` (`app/src/ffi/cef/app.rs`, 0.25.5) | **pref** → 0.26.0 F-pre (setting, normal tabs only) |
| 5 | Page preconnect / dns-prefetch hints, pointer-down preconnect | on | through the predictor's PreconnectManager, same pref (`chrome/browser/predictors/network_hints_handler_impl.cc:49-113`) | off (pref 2) | **pref** + **patch** for the blocked-host guarantee (`chromium-preconnect-blocked-hosts.patch`) |
| 6 | Omnibox preconnect of the top match | on (Chrome's omnibox) | no Alloy-style omnibox; the predictor API exists (`loading_predictor.cc:150-169`) but no CEF API reaches it | none | **patch** (`krunk-preconnect-pref.patch`, `krunk.preconnect`) |
| 7 | Prerender2 / speculation-rules prerender | on | **hard-disabled**: `alloy_browser_host_impl.cc:1154`, `cef/libcef/browser/chrome/chrome_browser_web_contents_delegate.cc:120-121` (`IsPrerender2Supported` false) | off | **off** (stays; enabling it would need a CEF patch and a browserless-frame filter path: not in 0.26.0) |
| 8 | Speculation-rules prefetch | on | works, intercepted by CEF for the tab's browser (`chrome_content_browser_client_cef.cc:482-571`, `content/browser/preloading/prefetch/prefetch_url_loader_factory_utils.cc:44-113`) | off (pref 2) | **pref** (no corpus page uses speculation rules: `../corpus/scan.txt`) |
| 9 | NoStatePrefetch (`<link rel=prerender>`) | on, ignores the prediction pref | on, in a browserless WebContents that only the request-context handler sees (`components/no_state_prefetch/browser/no_state_prefetch_manager.cc:473-487`, `no_state_prefetch_contents.cc:351-355`) | **on and unfiltered** | **off** since 602f01e (`NoStatePrefetchHoldback`) |
| 10 | Early Hints (103) preconnect/preload | on (no feature gate in this tree: `content/browser/loader/navigation_url_loader_impl.cc:1390-1432`) | forwarded by CEF's proxy loader (`cef/libcef/browser/net_service/proxy_url_loader_factory.cc:669-672`) | on (not pref-gated) | **on** + **patch** (the blocked-host check in `NavigationEarlyHintsManager::MaybePreconnect`); WPR records no 1xx, so the corpus can't show it |
| 11 | HTTP disk cache size | heuristic, ≤ 320 MB | not overridden (`cef-tree.md` Q9) | as Chrome | **on** (revisit only if warm loads miss the cache) |
| 12 | JS code cache | on, same heuristic | not overridden | as Chrome | **on** |
| 13 | QUIC / HTTP/3 | on | on | on (users); off in the replay runs for every browser (§1, method) | **on** |

### 4a. Back/forward-cache blockers Krunk itself could add (to confirm in the back/forward pass)
- **Verdict from the code:** no blocker of Krunk's own, so **F-bfcache has no code change**.
  - None of `login.js`, `media.js`, `painted.js`, `pointer-lock.js`, the other `ui/inject/*.js`
    or the extension content-script bootstrap (`ext-content.js`) registers `unload` or
    `beforeunload`. `painted.js` uses `load` and `visibilitychange`; `login.js` uses `load`,
    `popstate`, `hashchange` and DOM events; `media.js` uses media events and `load`.
  - No script sets or changes `Cache-Control`. The only Cache-Control Krunk touches is the
    request header `no-cache` on a duplicate GET while another tab waits for the same address
    (`net.rs` `on_before_resource_load`, `docs/NETWORK.md`); that is a request header, not the
    document's response, so it doesn't affect eligibility.
  - The page bridge is a V8 function with no port or channel.
  - Krunk's DevTools domains are kept only for tabs with extension content scripts, and an
    attached session doesn't block the cache anyway (below).
  - A "pagehide instead of unload" change has nothing to replace.
  - **F-bfcache becomes a fix only if** the pass's `backForwardCacheNotUsed` reasons show a
    Krunk-only reason (list below). The back/forward cells of the P1 pass carry those reasons.
- **The harness's DevTools session does not block the cache:**
  - the code: `cef-tree.md` Q5, no `kInjectedJavascript` for DevTools new-document scripts and no
    DevTools reason in the eligibility checks;
  - measured: in the dry runs and the first baseline cells, all three browsers restored pages from
    the cache with the driver's session attached (Krunk news back 25.8 ms / forward 17.3 ms;
    Edge 44.7 / 29.1 ms). So the back/forward numbers are not distorted by the harness.
- **What the run's `backForwardCacheNotUsed` reasons must confirm:**
  1. no `UnloadHandlerExistsIn*` on pages without their own unload. Wikipedia has one (corpus
     scan), so it is expected there in every browser;
  2. no `InjectedJavascript`/`InjectedStyleSheet` in Krunk only;
  3. no `DisableForRenderFrameHostCalled` in Krunk only;
  4. no Krunk-only `BrowsingInstanceNotSwapped` / `RelatedActiveContentsExist` (the driver's link
     page opens nothing);
  5. `MainResourceHasCacheControlNoStore` for Bing and YouTube in all three browsers, as expected.

  Any Krunk-only reason becomes candidate F-bfcache.
- **`unload`/`beforeunload` listeners:** none in `ui/inject/*.js` (grep). The page bridge is a V8
  function handler (`app/src/ffi/cef/renderer.rs:490-507`) with no port or channel.
- **BroadcastChannel / WebSocket / Web Locks / IndexedDB holders:** none in the injected scripts.
  `wasm-probe.js:387` uses a MessageChannel, only when the user opens the WASM inspector.
- **DevTools sessions:** Krunk keeps `Page.enable`/`Runtime.enable` only on tabs with extension
  content scripts (`app/src/browser/cdp.rs:43-44`). An attached session doesn't block the cache, and
  DevTools new-document scripts don't register `kInjectedJavascript` (`cef-tree.md` Q5).
- **Openers:** pages opened with `window.open` keep `window.opener` (`app/src/ffi/cef/client.rs:210`),
  so their opener can't be cached (`RelatedActiveContentsExist`). That is the same as Chrome.
- **What the baseline shows so far:** Krunk restores the news page from the cache on back and
  forward (17–26 ms). The full NotUsed list per page is in `../baseline/summary.md`.

## 5. V8 context snapshot (#4): what doing it would take

- **Can the tree build the generator for Windows?** Yes, with a small GN patch.
  `tools/v8_context_snapshot/v8_context_snapshot.gni:32-40` turns the snapshot off for `is_win &&
  host_os != "win"`. `BUILD.gn:94-95` defines `v8_context_snapshot_generator` only in
  `v8_snapshot_toolchain`, which is the Linux host toolchain for a win/x64 build on linux/x64, and
  the action at `:42-65` would run it on the host. The patch (sketch in `cef-tree.md` Q8) adds an
  arg `krunk_external_v8_context_snapshot`: the generator is built in the default (Windows)
  toolchain and the action becomes an empty group.
- **Build cost:**
  - `use_v8_context_snapshot=true` changes `buildflags.h`, which `content_features.h` and
    `content_switches.h` include, so it is a near-full rebuild of content/blink/chrome/cef plus the
    libcef ThinLTO link.
  - Recorded estimate: 3–6 h on 16–32 cores (`.pipeline/cef-codecs-build.md:11`). The host has
    32 cores and 91 GB RAM.
  - Plus a second ThinLTO link for the generator (all of Blink and V8, about 30–60 min).
  - PGO: the profiles stay valid for unchanged functions. The few functions behind the buildflag
    get no profile data; the gain/loss there is negligible.
  - Every later engine build stays incremental, but each one needs the generator step again.
- **Disk:** `out/Release_GN_x64` is 13 GB and `/work` has 98 GB free (95 % used), so it can be built
  in place but not in a second out dir comfortably.
- **VM step:**
  - push `v8_context_snapshot_generator.exe` (static, about 150–200 MB), `snapshot_blob.bin` and
    `icudtl.dat`;
  - run `v8_context_snapshot_generator.exe --snapshot_blob=snapshot_blob.bin --output_file=v8_context_snapshot.bin`
    (under a minute);
  - pull the `.bin` (a few MB) and put it in `Release/` before `distrib`'s tar.
  - Scripted as `scripts/cef-build/snapshot-on-vm.sh`. Not written yet; written only on "go".
- **Risk:**
  - With the flag on, gin loads only `v8_context_snapshot.bin` and is `LOG(FATAL)` if it is
    missing (`gin/v8_initializer.cc:618-641`). The package check in `app/src/ffi/cef/entry.rs`
    must then require that file.
  - A snapshot from a mismatched generator is caught by V8's version and checksum checks
    (`v8/src/snapshot/snapshot.cc:165-183`), at the cost of a crash at start, so the VM smoke must
    start Krunk.
- **Go/no-go rule** (plan): go only if P1 shows Krunk's `LocalWindowProxy::CreateContext` (or
  `V8.NewContext`) costing ≥ 10 ms more per main-frame load than Chrome's, or ≥ 3 % of LCP,
  summed over the page's contexts. Otherwise: "tried in analysis, dropped" with the numbers.
  Cost of a go: about one working day (rebuild in the background, VM step, smoke). It blocks other
  engine builds meanwhile.

## 5b. §omnibox-preconnect: design of F-omni (written, not built)

The code is already written but not built: the app side is in checkpoint ae4f6d2, the engine side
in `scripts/cef-build/patches/krunk-preconnect-pref.patch`. It goes in with the engine build after
P1. Design, point by point:

- **CEF entry point:** no new C API. Krunk calls the existing
  `CefRequestContext::SetPreference(name, value)` on the **global** context with the reserved
  pseudo-preference `krunk.preconnect` (a URL string). The patch handles that name in
  `CefRequestContextImpl::CanSetPreference` / `SetPreference`
  (`cef/libcef/browser/request_context_impl.cc`) before the normal pref lookup. Nothing is
  registered or stored. App side: `ffi::cef::preconnect(url)` (`app/src/ffi/cef/mod.rs`) →
  `app::set_krunk_pref`, which warns once and does nothing on an engine without the patch.
- **The engine call:** `predictors::LoadingPredictorFactory::GetForProfile(profile)->PrepareForPageLoad(
  std::nullopt, url, predictors::HintOrigin::OMNIBOX, network::GetNoOpNetworkRestrictionsId(),
  /*preconnectable=*/true)`. That is the same call Chrome's omnibox makes for its default match
  (`chrome/browser/predictors/loading_predictor.cc:150-169` → `HandleHintByOrigin`, `:373-426`).
  - It preconnects to the URL's origin, and with learned data also to the origins the site usually
    needs.
  - It also warms up the site's service worker (`MaybeWarmUpServiceWorker`, `:145-148`), a speed-up
    of its own.
- **Network partition key:** `HandleHintByOrigin` builds the request with
  `net::NetworkAnonymizationKey::CreateSameSite(net::SchemefulSite(url))` (`:397-399`). That is the
  key the following top-level navigation to that site uses, so the navigation reuses the socket. A
  preconnect made from Krunk's own pages (`krunk.invalid`) would be keyed to that site and wasted:
  the reason a page-side `<link rel=preconnect>` trick doesn't work.
- **Private contexts:** skipped three times over.
  1. Krunk calls it only when the active tab is not private and the setting is on
     (`Browser::omnibox_preconnect`, `app/src/browser/omnibox.rs`; `preload_effective`).
  2. It always goes to the global context.
  3. An off-the-record profile has no LoadingPredictor (`GetForProfile` returns null), and the
     patch then returns without doing anything.
- **Prediction off:** `HandleHintByOrigin` checks `IsPreconnectEnabled()` (`:379`), i.e.
  `net.network_prediction_options`. With the setting off, while the VPN is offline (pref 2) or with
  `KRUNK_PERF_OFF=preload`, the call is a no-op.
- **Proxy/VPN path:** the predictor's PreconnectManager uses the profile's storage partition and
  NetworkContext (`content/browser/preloading/preconnect/preconnect_manager_impl.cc`
  `GetNetworkContext`). That context's proxy config is the one Krunk's VPN sets (`apply_vpn`). With
  a fixed proxy there is no local DNS (the pre-resolve does a proxy lookup first). With the kill
  switch's refusing stand-in, the connection fails there.
- **Blocking:** the patched `PreconnectManagerImpl` entry points drop hosts in the blocked-host set
  (`chromium-preconnect-blocked-hosts.patch`), the same as for page hints.
- **When it fires:** in `Browser::address_suggest`, after each suggestion query, for the row Enter
  would open (the inline completion, else the first row; a search row means the search engine's
  URL). The same origin is asked again only after 10 s (`PRECONNECT_AGAIN_MS`), and the engine has
  its own minimum delay per origin.
- **Router arm and Shell permit:** **none needed.** The preconnect starts on the host side from the
  `addressSuggest` request the shell already sends, which already has its router arm and permit.
  No new shell → host request exists, so the request-router trap doesn't apply. If a later version
  adds a shell-side trigger (for example hover over a suggestion row), that request needs both an
  arm in `mod.rs`'s router and the Shell permit.
- **A/B:** `KRUNK_PERF_OFF=omnibox-preconnect` (`perf_probe`). Measured in the per-fix pass after
  the engine build, typed cold and warm.

## 6. Findings and the re-ranked candidate list

### Candidate F-keys (likely product bug, to be confirmed by the fill pass): keys typed right after Ctrl+L are lost
- **Symptom:** 7 of the baseline's Krunk cold typed loads never started. The tab stayed on the
  page the typing began on; Chrome and Edge never miss. Analysis in `../baseline/notes.md`
  ("second reading").
- **Code path:**
  1. the tab's key hook gets Ctrl+L and maps it to `Command::FocusAddressBar`;
  2. `Browser::focus_address_bar` (`app/src/browser/mod.rs:2185-2190`) does `shell.focus()`, then
     posts the `FocusAddressBar` event;
  3. the shell's JS focuses and selects `#address`.

  Keys in between go to the page.
- **Fix sketch (browser side, no engine patch):** from the moment Krunk handles Ctrl+L (or F6,
  Alt+D) until the shell reports that `#address` has the focus (a new `AddressFocused` message),
  the tab's key hook keeps the keys from the page and buffers them:
  - character keys are sent to the shell as text (`Event::AddressType`), in order, after the
    select-all;
  - Enter waits for that text;
  - Escape cancels.

  A 1 s cap on the buffering prevents a stuck state. The router needs an arm for `AddressFocused`
  and the Shell permit (the request-router trap).
- **Measurement:** the typed cold scenario's timeouts go away, and the shell gets every key in the
  harness. Input → navigation start on cold typed is unchanged or better: with lost keys, no
  navigation happens at all.
- **Priority:** high if the fill pass confirms it, because it is a correctness bug on the main
  typed path. It is not a speed fix, so it is recorded with its own evidence.

### Provisional ranking from the 807 main-pass rows (host-only analysis, 2026-11-20 18:2x)

**Medians from navigationStart, ms:**

| scenario | Krunk TTFB | Chrome TTFB | Edge TTFB |
|---|---|---|---|
| cold | 152 | 127 | 121 |
| link | 160 | 136 | 138 |
| warm | 65 | 47 | 46 |

**Per page** (cold + warm pooled; Krunk's pool is biased, because about half its cold cells are
missing):

| page | TTFB→FCP, Krunk vs Chrome | FCP→LCP, Krunk vs Chrome |
|---|---|---|
| docs | 504 vs 346 | – |
| shop | 683 vs 432 | – |
| news | 618 vs 539 | 536 vs 0 |
| search | 233 vs 146 | – |
| wiki | 355 vs 284 | – |
| ads | 286 vs 256 | 368 vs 274 |
| spa | 335 vs 319 | – |
| hn | 180 vs 163 | – |
| youtube | – | 4558 vs 1782 (LCP 5.1 s vs 2.2 s) |

DCL and load trail by similar margins (docs DCL 518 vs 240; shop load 2068 vs 1543).

**Reading:**
- **TTFB:** a constant +20–25 ms in every scenario, warm included (where sockets are reused). So it
  is not connection setup: it is per-navigation overhead on Krunk's own path. Suspects, to settle
  with the load trace:
  - the synchronous `on_before_browse` (`navigation_starting` computes the cosmetic filters for
    the URL on the UI thread before the navigation goes on);
  - the main request's trip through CEF's interception;
  - Krunk's request policy.
- **The bulk of the gap is after the first byte:** TTFB→FCP and FCP→LCP, on pages with few
  frames too (docs +158 ms, search +87 ms, hn +17 ms). So it **doesn't grow with iframe count**.
  That points to the engine (the 0.7.0 JS/layout gap) plus what Krunk adds to every page: the
  injected scripts, and subresources without the page's preconnect hints (pref 2).
  - 8 of 9 pages carry preconnect/dns-prefetch hints (`../corpus/scan.txt`).
  - Prediction 2 also disables the engine's early preconnect of the main navigation.
- **F-snap is not at the top:** the gap isn't iframe-driven. The go/no-go still follows the trace's
  measured `LocalWindowProxy::CreateContext` cost.

**Provisional order:**
1. **F-pre:** page hints and the predictor, for the subresource connections in TTFB→LCP.
2. **The per-navigation overhead** (+20–25 ms TTFB): the load trace decides between
   before_browse/cosmetic work on the UI thread, interception and the filter. A fix follows the
   data (e.g. the cosmetic filters computed off the navigation's critical path).
3. **F-scripts** (login observer), then **F-writes** and **F-filter**: small, measured in the A/B.
4. **F-snap:** only if the trace shows ≥ 10 ms per main-frame load.
5. **F-keys:** a correctness fix, not a speed one. It needs the fill pass's evidence.
6. **YouTube's LCP** (5.1 s vs 2.2 s) has its own cause, probably media: to look at in the traces.

### F-keys hypothesis, from the code (to check against the fill pass's `krunk.log`/`perf.log`)
- **Ctrl+L:** the tab's key hook maps it to `Command::FocusAddressBar`. `focus_address_bar`
  (`app/src/browser/mod.rs:2185`) calls `shell.focus()` and posts `FocusAddressBar` to the shell's
  renderer; the shell's JS then focuses `#address`. Keys typed before that land in the page.
- **At start-up:** a new tab page holds the address bar's focus (`hold_address_focus`, `frame.rs`),
  so typing right after launch without Ctrl+L goes to the address bar. The bug needs focus in the
  page first (a click, or a page that took it), then Ctrl+L and fast typing.
- **What the evidence would show:**
  - Krunk drops them: the page counted keydowns, or the address bar is empty/partial, possibly
    with a UI stall in `perf.log` around the Ctrl+L.
  - The harness drops them: the address bar holds the whole URL and the page counted nothing.
- **Fix if it's Krunk:** buffer keys in the tab's key hook from Ctrl+L until the shell confirms
  `#address` has focus (a new `AddressFocused` message: router arm + Shell permit), then send them
  as text and the Enter after them.

### Re-ranked list (final, after passes 1 and 2)
Measured on app260n (engine 33bbbe35), 9 pages × 5 runs, against the A/A band (`../baseline/numbers.json`).
1. **F-omni** (kept): cold typed TTFB 141 → 63 ms (−55 %); cold FCP −19 %, LCP −18 % (inside the wide cold band).
2. **F-pre** (kept): link FCP −9.3 %, link TTFB −9.0 %, cold TTFB −6.6 %; LCP within the band.
3. **F-block / F-nsp** (kept, privacy): F-block LCP within the band, cold TTFB +2 ms (outside the 1.2 % TTFB band); netlog proof (`../smoke/preload/netlog.md`).
4. **F-filter, F-scripts, F-writes, F-cosmetic** (dropped): every LCP/TTFB change within the band (`ab-b/summary.md`). The load trace puts Krunk's own pre-request path at ~14 ms per typed navigation (input → before_browse ~7 ms, → main request ~7 ms); F-cosmetic moved part of it without a measurable effect.
5. **F-keys** (dropped): 0.25.5 lost 94/134 cold typed loads; the 0.26.0 build lost 0/45 with and without the fix.
6. **F-snap** (not done): CreateContext 19.5 ms vs Chrome 4.2 ms per two loads: +7.7 ms per load, 2.0 % of LCP (`traces/fsnap.md`).
7. **YouTube LCP** (5.1 s vs 2.2 s in the baseline): media-driven, not addressed in 0.26.0.


## 7. Proof plan: preload traffic takes the VPN proxy and the kill switch stops it (VM, with the engine build)
- **Setup:** a test page on the VM's local server, with `<link rel=preconnect>` and `dns-prefetch` hints to three hosts:
  - a normal one (`example.org`);
  - one on the ad-block list (`doubleclick.net`);
  - one used for a speculation-rules prefetch (same-site and cross-site).
- **Normal tab, no VPN:**
  - netlog (`--log-net-log` extra flag) shows a socket to example.org and the prefetch request;
  - Krunk's blocked log has the prefetch to a blocked host;
  - netlog shows no DNS lookup or socket to doubleclick.net.
- **VPN on** (the dind OpenVPN testbed from 0.14.0):
  - every preconnect socket in netlog is a proxy socket (`PROXY 127.0.0.1:<vpn port>`);
  - netlog shows no host resolution for the hinted hosts (proxy lookup only);
  - the testbed's server sees the CONNECTs.
- **Kill switch on, VPN disconnected:**
  - preconnects to the stand-in proxy are refused (netlog: `ERR_PROXY_CONNECTION_FAILED`/503);
  - pktmon shows no direct TCP from the engine to example.org.
- **Private tab:** netlog shows no preconnect or prefetch at all (pref 2).
- Evidence goes to `evidence-0.26.0/smoke/preload/`; this runs in the final smoke.
