# Krunk 0.26.0: end-to-end verification (2026-11-21)

## Clean build and tests (fresh `git clone` of e3e45c4, own target dir; `clean-clone.txt`)

| step | result |
|---|---|
| `cargo fmt --check` | OK |
| clippy, Windows target, `-D warnings` | OK |
| `scripts/build-dist.sh` (cross `--release`) | OK; the engine files match the published dist and `engine-files.sha256` |
| `cargo test` core, vpn, ffi | 1185 passed, 0 failed, 5 ignored |
| `cargo test -p app` | **not run**: Windows-only tests, no Wine |
| test-ui | 3123 passed, 0 failed |

## Main flows on the Windows VM (the real user path; `vm-run.txt`)
- **In-app update:** an installed 0.25.5 updated itself from https://krunk.209.wf.
  - Result: 0.26.0 running after 40 s, no crash events.
  - Installed files: Krunk.dll `480CE8BD…` (= dist), libcef `33BBBE35…` (= the -preconnect engine).
  - Shot: `up0260-1-first-start-35s.png`.
- **Flows on the installed 0.26.0** (user profile, real network):
  - Krunk was never hung (`IsHungAppWindow` false at 5 s and at the end).
  - The blocked-host push took 131 ms (86 308 hosts).
  - 0 PANIC lines, 0 crash events.

| shot | flow | seen |
|---|---|---|
| `flow-01-new-tab.png` | Ctrl+T | new tab page with tiles |
| `flow-02-typed-guardian.png` | typed address | the Guardian loaded |
| `flow-03-typed-wikipedia.png` | typed address | Wikipedia "Web browser" loaded |
| `flow-04-link-click.png` | link click | **not shown**: the click hit Wikipedia's donation banner and the page didn't change. Link navigation is covered by the final pass's 45 measured link loads (`../final/table.md`). |
| `flow-05-back.png` / `flow-06-forward.png` | Alt+Left / Alt+Right | back to the Guardian, forward to Wikipedia |
| `flow-07-omnibox-suggestions.png` | typing "wik" | the suggestion dropdown |
| `flow-08-private-window.png` | Ctrl+Shift+N + address | BBC News in a window with the Private badge |
| `flow-09-ad-blocker.png` | ad test page | page loaded, shield badge on |
| `flow-10-settings-preload.png` | Settings search "preload" | "Preload pages for faster browsing", on |

## Acceptance criteria

| criterion | verdict | evidence |
|---|---|---|
| −15 % corpus median LCP vs 0.25.5, or within 5 % of Edge (from the input and from navigation start); if neither, say why | **−15 %: not met.** Same-session link LCP 559 → 562 ms (+0.6 %, within the band); cold typed 817 → 472 ms (−42 %) only against the 11-20 baseline session (its own drift −30 %), so indicative. **Within 5 % of Edge 156: met, indicative** (11-20 numbers): typed LCP 430 vs 1056 ms from the input, 370 vs 412 ms from navigation start; link 562 vs 590 ms. The report says why LCP moved no more (connection time saved; script/layout speed is the engine's). | `../final/table.md`, report |
| Every kept fix has a measured before → after beyond the band, or is a blocking/privacy fix with a netlog proof | met | F-omni cold TTFB −55 %; F-pre link TTFB −9.0 %, link FCP −9.3 %, cold TTFB −6.6 %; F-block privacy fix, netlog proof `../smoke/rerun/nl-*.txt` (its +2 ms cold TTFB, outside the 1.2 % TTFB band, is stated); F-nsp netlog (`rel=prerender` starts nothing). Dropped fixes reverted with their numbers (`../fixes.md`). |
| All gates green | met, except `cargo test -p app` (Windows-only, not run) | clean clone above |
| No regression beyond the band in the item-8 checks | met after the two fixes (37e76ac hang, 9fe3e5a kill switch) | `../smoke/summary.md`: tab switch p95 49.2 vs 49.8 ms, CPU 5.5 vs 5.6 %, omnibox 9–29 vs 11–27 ms, ad blocker 6 = 6, VPN + kill switch, private tab |
| Live manifest lists 0.26.0; VM 0.25.5 sees the update | met | re-checked now: live latest.json 0.26.0 (`86f2f76d…`), "Signature Verified Successfully" with `assets/keys/update-signing.pub`; `../release/update-found.png`; and the in-app update above |
| Staging HEALTHY, `/changelog` shows 0.26.0, update proposed (and whether it awaits Apply) | met | `../release/site-staging.txt`; update 4c4744c auto-applied (auto-apply on, no operator Apply needed); live `/changelog` leads with 0.26.0 (re-checked now) |
| Engine patches in `scripts/cef-build/patches/`, `cef.lock`, `engine-files.sha256`, README | met | `cef.lock` pins both new patches by SHA-256; the clean-clone engine files match `engine-files.sha256` |
| `fixes.md`, before/after table, smoke shots, CHANGELOG, README, report complete | met | no TODO/TBD left (grep) |

## Found during verification (not fixed; for the next release)
- **Settings wording is too broad.** The Settings description of "Preload pages" (shipped in 0.26.0,
  `ui/pages/settings.html`) says early connections "never go to hosts the ad blocker blocks". The
  CHANGELOG and README now say "whole-host rules" and name the kill switch; the in-app text
  doesn't. A wording fix only; it needs a release to reach users.
