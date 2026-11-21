# 0.26.0 smoke and regressions (2026-11-21, one VM batch)

Builds: 0.26.0 = app260rel (release, dist/) and app260r (harness build of the same commit, for the
CDP-driven VPN steps); 0.25.5 = app260h (harness build). Batch: `scripts/perf/smoke-0260.sh`, log
`../smoke-batch.log`.

**Verdict: FAIL. Release blocker: 0.26.0 stops responding soon after start on a real network.**

| Check | 0.25.5 | 0.26.0 | Verdict |
|---|---|---|---|
| Release smoke shots (Guardian, Wikipedia, BBC, YouTube, back/forward) | – | all 6 show the new tab page: no typed address was opened; the back/forward shots' title is "New tab – Krunk (Not Responding)" | **FAIL** |
| Private-tab shot | – | bare desktop (Krunk window gone or behind) | **FAIL** |
| Ad blocker shot | – | new tab page | **FAIL** (same hang) |
| VPN-on shot (harness build) | – | preconnect test page loaded, VPN connected (OpenVPN TCP testbed) | pass |
| Kill-switch shot (harness build) | – | Settings page: the navigation was blocked by the import dialog | not a valid shot |
| Tab switch (vm-0250 switch, 40 tabs) | 25/25 switches, median 34.5, p95 49.8 ms | not measured: "could not focus Krunk", twice, on both builds; krunk.log stops at "engine initialized" (0.25.5's goes on to load tabs within 1–4 s) | **FAIL** |
| Background CPU (vm-0250 cpu, 40 tabs) | avg 5.6 %, max 9.8 % | not measured (same failure) | **FAIL** |
| Omnibox latency (vm-0255 check) | 8 dropdowns, on screen 11–27 ms after the query | 1 dropdown in 6 tries (26 ms) | **FAIL** (fits the hang) |
| Ad blocker page, blocked/redirected requests | 6 | 4: the linkedin `px.ads` and facebook `tr` images are missing from the log | **mismatch** (stray Firefox window in front in both runs; cause not examined) |
| Release netlog, F-block page: doubleclick.net | – | no DNS, no socket | pass |
| Release netlog, F-block page: example.org | – | no DNS, no socket (expected both) | **FAIL** |
| Private-tab netlog (release) | – | invalid: the page never loaded in Krunk (no connection to localhost:8092). The keys missed Krunk and the URL opened in Firefox. Pass 1's private-tab proof was made the same way and is invalid too | **not proven** |
| VPN on: preconnect routes | – | only the VPN proxy (127.0.0.1:65279) and the local page; plus one direct connectivity check to 142.251.150.120:80 | pass (routes) |
| VPN on: F-block | – | **preconnect to stats.g.doubleclick.net through the VPN proxy** (CONNECT tunnel headers in the netlog) | **FAIL** (blocked-host guarantee) |
| Kill switch engaged (kill switch on, no VPN): sockets | – | only the refusing stand-in (127.0.0.1:64855) and the local page; no direct socket | pass |
| Kill switch engaged: preconnects | – | preconnects to example.org and stats.g.doubleclick.net were attempted to the stand-in (refused); requirement: none | **FAIL** |

Screenshots: `s0260-*.png`, `ads-*.png`, `omni-*.png`; netlogs: `nl-rel-*.json`, `nl-vpn.json`, `nl-ks.json`.
Tab-pass logs: `../regress/tabs-{old,new,new-r}/`.

Not investigated (instruction: ask first).
