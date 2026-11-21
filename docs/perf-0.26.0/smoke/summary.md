# 0.26.0 smoke and regressions (2026-11-21)

Builds after the fixes 37e76ac (hang) and 9fe3e5a (kill switch, proof page): 0.26.0 = app260rel
(release, dist/) and app260r (harness build of the same commit: CDP for the VPN steps); 0.25.5 =
app260h. Re-run batch `smoke-0260b.sh` (`../smoke2/`) and its corrective subset
(SUBSET=1, `../smoke3/`: proof host, focus). First run and its failures: git 2e40e10; diagnosis
`hang.md`.

**Verdict: PASS.**

| Check | 0.25.5 | 0.26.0 | Verdict |
|---|---|---|---|
| Start-up hang | – | blocked-host push 132–216 ms on the UI thread (was 39 s, twice); no "Not Responding" | pass |
| Tab switch (vm-0250, 40 tabs, 25 switches) | median 34.5, p95 49.8 ms | median 18.8, p95 49.2 ms | pass |
| Background CPU (vm-0250 cpu, 40 tabs) | avg 5.6 %, max 9.8 % | avg 5.5 %, max 10.3 % | pass |
| Omnibox (vm-0255 check) | 8/8 dropdowns, 11–27 ms | 8/8, 9–29 ms | pass |
| Ad blocker page, logged requests | 6 | 6 (the first run's 4: the hang delayed the page past the 15 s capture) | pass |
| Release netlog: example.org | – | DNS + socket | pass |
| Release netlog: pixel.quantserve.com (in the blocked set) | – | no DNS, no socket | pass |
| Release netlog: `rel=prerender` example.net | – | nothing | pass |
| Private-tab netlog | – | the page loaded in the private window (shot `nl-private-page.png`, title check); no DNS, no socket for any hinted host | pass |
| VPN on (OpenVPN TCP testbed) | – | connections only to the VPN proxy and the local page; pixel.quantserve.com none | pass |
| Kill switch engaged | – | no early connections at all (no stand-in connections for hinted hosts) | pass |
| Smoke shots (release): Guardian (corpus page, live), Wikipedia, BBC, YouTube, back, forward, private tab, ad blocker; VPN on | – | all show their page; no blank or #121212 frames (checked by eye) | pass |

Notes:
- **Earlier "leak" hosts weren't in the set.** stats.g.doubleclick.net and ad.doubleclick.net were
  not in the blocked set by design: an `@@` exception touches them (e.g.
  `@@||ad.doubleclick.net/…$popup`). That is the documented narrower-rule limitation, not a
  missed check.
- **The set covers fewer big trackers than expected.** Of 13 well-known ad/tracker domains, only
  pixel.quantserve.com is in it; the others either have no plain `||host^` rule or an `@@`
  exception touches them in the default lists. README Known
  limitations says so.
- **Harness fixes:**
  - the private-tab and live steps focus Krunk with an Alt tap and log the title;
  - the VPN profile is pre-imported by `KRUNK_VPN_IMPORT` (harness builds only);
  - no file dialog.
