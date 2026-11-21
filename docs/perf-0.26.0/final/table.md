# 0.26.0 final pass: before / after (2026-11-21)

0.26.0 = app260r (harness build of the release commit 1bb8753, no switches). 0.25.5 = app260h. Same session, 9 corpus pages x 5 runs, 40 ms RTT replay, QUIC off.
Chrome/Edge from baseline pass 11-20, same corpus, VM, harness and 40 ms RTT. They auto-updated afterwards and the harness can't drive input into Chrome 158 / Edge 157.
Corpus median (p90), ms from the input; `_ns` = from navigation start. Sources: `summary.md` (A/B), `combined.md`.

| scenario / metric | Krunk 0.25.5 | Krunk 0.26.0 | change | band | Chrome 157 (11-20) | Edge 156 (11-20) |
|---|---|---|---|---|---|---|
| cold typed LCP | biased: 0/45 started this session; 817 (837) in the 11-20 baseline, 39/82 measured | 472 (501) | −42 % vs baseline reference | 35.1 % | 713 (799) | 899 (1178) |
| cold typed TTFB | (11-20) 170 (173) | 64 (67) | −62 % vs baseline reference | 1.2 % | 145 (149) | 376 (414) |
| cold typed FCP | (11-20) 596 (661) | 419 (424) | −30 % vs baseline reference | 19.8 % | 627 (658) | 877 (990) |
| warm typed LCP | not comparable (each "warm" load followed a cold load that never started, so it was a first visit): 581 (637) | 366 (400) | – | 18.4 % | 436 (665) | 1233 (1444) |
| link LCP | 559 (594) | 562 (588) | +0.6 % (within band) | 19.9 % | 669 (762) | 590 (711) |
| link FCP | 488 (520) | 490 (512) | +0.5 % (within band) | 7.6 % | 546 (628) | 508 (630) |
| link TTFB | 158 (162) | 137 (140) | −13.5 % (beats band) | 3.4 % | 142 (145) | 144 (146) |
| back frame | 36 (38) | 37 (38) | +1.7 % (within band) | 3.4 % | 55 (58) | 53 (72) |
| forward frame | 27 (29) | 26 (29) | −1.9 % (within band) | 26.8 % | 38 (40) | 44 (50) |
| typed cold+warm LCP, headline | 581 (637) (biased) | 430 (492) | – | – | 620 (840) | 1056 (1444) |
| typed cold+warm LCP_ns, headline | 488 (516) (biased) | 370 (412) | – | – | 418 (640) | 412 (608) |

**Session drift** (0.25.5 in this pass vs 0.25.5 in the 11-20 baseline, which ran with the load trace on): link LCP 559 vs 801 (−30 %), link TTFB 158 vs 177 (−11 %), back frame 36 vs 40 (−10 %). That is beyond the A/A band, so every cross-session figure (Chrome, Edge, cold typed vs the baseline reference) is **indicative**.

**Targets, as measured:**
- **−15 % median LCP vs 0.25.5:**
  - same session: **not met**. Link LCP +0.6 % (within the band). Warm typed is not comparable. Back/forward is unchanged.
  - cold typed, against the 11-20 baseline reference (biased, indicative, drift up to −30 %): **472 vs 817 ms, −42 %**. Most of it is cold TTFB 170 → 64 ms (F-omni) plus 0.25.5's lost-key bias.
- **Within 5 % of Edge 156 (indicative, cross-session):**
  - from the input: **met**. Typed headline 430 vs 1056 ms, link 562 vs 590 ms (−5 %). Edge's input-to-navigation hand-off is 327 ms.
  - from navigation start: **met**. LCP_ns 370 vs 412 ms (−10 %).
