# 0.26.0 fixes: commits, A/B switches, measurements

Rule:
- Every fix has its own A/B switch and is measured against the noise band from the baseline A/A
  pair (krunk-a vs krunk-b, `baseline/summary.md`).
- Each measured fix gets a follow-up commit whose message gives median/p90 before → after on the
  corpus.
- A speed fix that doesn't beat the band is reverted ("tried, dropped"). A blocking/privacy fix
  stays if its cost is within the band.

**Note on history:** the preload wiring, the blocked-host set and the omnibox preconnect were
written while the baseline ran (no build allowed). The pipeline's turn-end checkpoint **ae4f6d2**
committed them together, along with the core `preload_pages` field from checkpoint **ca9702b**.
That history is not rewritten. Each fix's measured follow-up commit names ae4f6d2 as where its
code first landed.

| ID | Fix | Code commits | Engine patch | A/B switch (off side) | Scenarios | Before median/p90 | After median/p90 | Noise band | Kept / dropped |
|---|---|---|---|---|---|---|---|---|---|
| F-nsp | `<link rel=prerender>` NoStatePrefetch held back (blocking hole) | 602f01e, e9475ba (fmt) | – (feature switch) | `--disable-features=NoStatePrefetchHoldback` in a base profile | typed cold | – | netlog (smoke/preload/netlog.md): `<link rel=prerender>` to example.net started nothing (no DNS, no socket, no request) in the on, off and private runs; private tab re-proven 2026-11-21 (page loaded in the private window, nothing contacted) | – | **kept (blocking fix)**: no corpus page uses `rel=prerender` (`corpus/scan.txt`), so no speed effect to measure; no control run with the holdback off |
| F-pre | "Preload pages for faster browsing": network prediction 0 in normal tabs, 2 in private tabs and while the VPN is offline; predictor cleared with history | ca9702b (field), ae4f6d2 (wiring), ba1f75a (fixture, sync), 71f5c88 (switches) | – | base profile `lp-nopreload` (`preloadPages: false`) | typed cold+warm, link | base: cold TTFB 151 (164), link FCP 488 (506), link TTFB 158 (162), cold/warm/link LCP 516/324/523 | cold TTFB 141 (146) −6.6 %, link FCP 442 (457) −9.3 %, link TTFB 144 (148) −9.0 %; LCP 505/333/498 (−2.0/+2.9/−4.8 %) | TTFB cold 1.2 %, link FCP 7.6 %, link TTFB 3.4 %; LCP 35.1/18.4/19.9 % | **kept**: beats the band on cold TTFB, link FCP and link TTFB (pass 1, `p1/ab-a/summary.md`); LCP within the band |
| F-omni | Address bar: preconnect the top suggestion | ae4f6d2 | `krunk-preconnect-pref.patch` (`krunk.preconnect`) | `KRUNK_PERF_OFF=omnibox-preconnect` | typed cold+warm | pre: cold TTFB 141 (146), cold FCP 462 (466), cold LCP 505 (513) | cold TTFB 63 (69) −55.2 %, cold FCP 372 (390) −19.3 %, cold LCP 414 (435) −18.0 %; warm/link within ±4 % | cold TTFB 1.2 %, FCP 19.8 %, LCP 35.1 % | **kept**: cold TTFB far beyond the band; FCP/LCP gains inside the (wide) cold band; back frame +2 ms (+4.8 %, band 3.4 %) not plausible for an address-bar hook |
| F-block | No preconnect or DNS pre-resolve to blocked hosts (guarantee) | ae4f6d2 (`core/src/preconnect_block.rs`, `push_preconnect_block`) | `chromium-preconnect-blocked-hosts.patch`, `krunk.blocked_hosts` | `KRUNK_PERF_OFF=preconnect-block` | typed cold (ads, news) | omni: cold TTFB 63 (69), LCP cold/warm/link 414/336/497 | cold TTFB 65 (68) +2.9 %; LCP 418/328/504 (+1.0/−2.3/+1.5 %); other 11 metrics within the band | cold TTFB 1.2 %; LCP 35.1/18.4/19.9 % | **kept (privacy)**: cold TTFB +2 ms is outside the 1.2 % TTFB band (p90 69→68); everything else within; netlog proof holds (`smoke/preload/netlog.md`); director: keep Option A, no fallback C |
| F-filter | Request filter: the page's address once per request (Arc<str>, one lock) | 8e83f6b | – | `KRUNK_PERF_OFF=filter-fast` | typed cold (ads, news) | base: LCP cold 572 (636), warm 368 (380); TTFB cold 153 (165), warm 59 (62) | LCP 567 (595) −1.0 %, 355 (419) −3.5 %; TTFB 153 (163) +0.1 %, 59 (61) +0.5 % | LCP 35.1/18.4 %, TTFB 1.2/7.6 % (cold/warm) | **dropped**, reverted b6224a1 |
| F-scripts | login.js watches the DOM from DOMContentLoaded, not from document creation | f9ab3f6 | – | `KRUNK_PERF_OFF=login-defer` | typed cold+warm (DOM-heavy: news, shop, ads) | base: LCP cold 572 (636), warm 368 (380); TTFB cold 153 (165), warm 59 (62) | LCP 594 (671) +3.7 %, 381 (405) +3.5 %; TTFB 153 (165) −0.1 %, 57 (61) −3.7 % | LCP 35.1/18.4 %, TTFB 1.2/7.6 % (cold/warm) | **dropped**, reverted 9812584 |
| F-writes | History visit written 1 s after load end | bd3a1bd, 240ff32 | – | `KRUNK_PERF_OFF=defer-writes` | typed cold+warm | base: LCP cold 572 (636), warm 368 (380); TTFB cold 153 (165), warm 59 (62) | LCP 590 (614) +3.1 %, 363 (394) −1.4 %; TTFB 154 (166) +0.3 %, 60 (61) +1.5 % | LCP 35.1/18.4 %, TTFB 1.2/7.6 % (cold/warm) | **dropped**, reverted a1a9921 |
| F-cosmetic | Cosmetic filters made after the navigation goes on, not inside on_before_browse | 50c232d | – | `KRUNK_PERF_OFF=cosmetic-sync` | typed cold+warm | base: LCP cold 572 (636), warm 368 (380); TTFB cold 153 (165), warm 59 (62) | LCP 573 (630) +0.0 %, 361 (385) −1.9 %; TTFB 152 (165) −0.7 %, 58 (63) −1.7 % | LCP 35.1/18.4 %, TTFB 1.2/7.6 % (cold/warm) | **dropped**, reverted 276462d |
| F-bfcache | Back/forward-cache blockers of Krunk's own | none (code audit found none: `p1/summary.md` §4a) | – | – | back, forward (NotUsed reasons) | – | – | – | a fix only if the P1 pass shows a Krunk-only NotUsed reason |
| F-keys | Keys typed after Ctrl+L/Alt+D/F6 before the shell has the keyboard go to the address bar (1 s cap, Esc cancels) | 6d41a90, 226d85f (switch) | – | `KRUNK_PERF_OFF=address-capture` (krunk-nokeys, pass 2) | typed cold (lost keys) | 0.25.5 baseline: cold typed never started 94/134 (70 %), every key in the page (`baseline/lost-keys.md`) | pass 1 (F-keys on): 180/180 cold typed started; pass 2 A/B: F-keys off 0/45 cold and 0/45 warm never started vs on 0/45, 0/45; LCP cold 579 (605) → 572 (636) −1.1 %, TTFB 153 → 153 +0.3 % | LCP 35.1/18.4 %, TTFB 1.2/7.6 % (cold/warm) | **dropped**, reverted 252d1b4: no measured effect in the 0.26.0 build (it doesn't lose keys without the fix; why 0.25.5 did is unexplained) |
| F-snap | V8 context snapshot (engine build) | 213955e (prepared) | – | – | – | Krunk `LocalWindowProxy::CreateContext` 19.5 ms per trace (6 contexts, 2 loads) vs Chrome 157 4.2 ms (4 contexts) | – | go if ≥ 10 ms more per main-frame load or ≥ 3 % LCP | **not done**: +7.7 ms per load, 2.0 % of LCP (378 ms, pass 1): below both thresholds (`p1/traces/fsnap.md`) |
| F-trace | Load trace: per-navigation spans, filter histogram, script time per context (measurement only) | d0c68e4, 85a0474 | – | `KRUNK_PERF_TRACE` (off by default) | – | – | – | – | measurement tool; off in releases |

## Tried, dropped (pass 2, `p1/ab-b/summary.md`; rule: beat the band on LCP or TTFB)
- **F-filter** (8e83f6b, reverted b6224a1): LCP cold −1.0 %, warm −3.5 %; TTFB +0.1/+0.5 %. All within the band.
- **F-scripts** (f9ab3f6, reverted 9812584): LCP +3.7/+3.5 %; TTFB −0.1/−3.7 %. Within the band.
- **F-writes** (bd3a1bd + 240ff32, reverted a1a9921): LCP +3.1/−1.4 %; TTFB +0.3/+1.5 %. Within the band.
- **F-cosmetic** (50c232d, reverted 276462d): LCP +0.0/−1.9 %; TTFB −0.7/−1.7 %. Within the band.
- **F-keys** (6d41a90 + switch, reverted 252d1b4): lost keys 0/45 with and without it in the 0.26.0 build; no LCP/TTFB change beyond the band.

## Not done
- **F-snap** (V8 context snapshot): +7.7 ms per main-frame load, 2.0 % of LCP, below the 10 ms / 3 % bar.
- **Pass 3 / an extra technique:** skipped (director): the load trace shows nothing the kept fixes leave uncovered beyond the band.

## F-block, stated plainly
**Smoke 2026-11-21:** the first run's set push hung the UI thread for 39 s (O(n²) list build; fixed 37e76ac, now 132–216 ms). The VPN-on "leak" hosts (stats.g/ad.doubleclick.net) are not in the set by design (`@@` exceptions); with a real member (pixel.quantserve.com) the release, private-tab and VPN-on netlogs show no DNS and no socket. Kill switch: prediction now off while it holds connections (9fe3e5a). Kept as a privacy fix. Its LCP cost is within the band in every scenario, but cold TTFB went 63 → 65 ms (+2.9 %), **outside** the 1.2 % TTFB band (p90 69 → 68). Director decision: keep Option A, no fallback C.
