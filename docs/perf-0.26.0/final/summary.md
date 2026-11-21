# Page-load pass: /work/projects/extra/browser/.pipeline/evidence-0.26.0/final

## Headline: corpus median (p90) over cold + warm typed loads

Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.

| browser | ttfb | fcp | lcp | dcl | load | nav | fcp_ns | lcp_ns | load_ns | pages |
|---|---|---|---|---|---|---|---|---|---|---|
| krunk-new | 60 (65) | 375 (420) | 430 (492) | 468 (534) | 571 (636) | 9 (11) | 268 (364) | 370 (412) | 496 (543) | 9 |
| krunk-old | 153 (156) | 491 (525) | 581 (637) | 612 (677) | 670 (822) | 12 (13) | 416 (460) | 488 (516) | 615 (644) | 9 |

Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).

## cold

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-new | 11 / 13 | 64 / 67 | 419 / 424 | 472 / 501 | 511 / 553 | 535 / 577 | load 45 |
| krunk-old | – | – | – | – | – | – | timeout 45 |

Per page, lcp (median / p90, n):

| page | krunk-new | krunk-old |
|---|---|---|
| news | 822 / 859 (5) | – |
| spa | 299 / 308 (5) | – |
| search | 1525 / 1545 (5) | – |
| shop | 832 / 860 (5) | – |
| youtube | 4251 / 4660 (5) | – |
| wiki | 333 / 338 (5) | – |
| ads | 472 / 501 (5) | – |
| docs | 419 / 424 (5) | – |
| hn | 189 / 194 (5) | – |

## warm

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-new | 9 / 11 | 60 / 61 | 244 / 270 | 366 / 400 | 388 / 409 | 458 / 475 | load 45 |
| krunk-old | 12 / 13 | 153 / 156 | 491 / 525 | 581 / 637 | 612 / 677 | 670 / 822 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-new | krunk-old |
|---|---|---|
| news | 685 / 716 (5) | 916 / 947 (5) |
| spa | 241 / 269 (5) | 389 / 409 (5) |
| search | 366 / 400 (5) | 1587 / 2058 (5) |
| shop | 666 / 711 (5) | 889 / 1020 (5) |
| youtube | 709 / 3926 (5) | 4544 / 4710 (5) |
| wiki | 276 / 287 (5) | 428 / 477 (5) |
| ads | 418 / 428 (5) | 581 / 637 (5) |
| docs | 321 / 389 (5) | 491 / 525 (5) |
| hn | 156 / 168 (5) | 286 / 288 (5) |

## link

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-new | 6 / 7 | 137 / 140 | 490 / 512 | 562 / 588 | 600 / 613 | 622 / 648 | load 45 |
| krunk-old | 10 / 14 | 158 / 162 | 488 / 520 | 559 / 594 | 601 / 623 | 636 / 801 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-new | krunk-old |
|---|---|---|
| news | 886 / 890 (5) | 914 / 927 (5) |
| spa | 384 / 397 (5) | 381 / 391 (5) |
| search | 1562 / 1654 (5) | 1592 / 2009 (5) |
| shop | 853 / 880 (5) | 849 / 874 (5) |
| youtube | 4329 / 4460 (5) | 4328 / 4447 (5) |
| wiki | 406 / 431 (5) | 430 / 448 (5) |
| ads | 562 / 588 (5) | 559 / 594 (5) |
| docs | 490 / 512 (5) | 488 / 520 (5) |
| hn | 268 / 276 (5) | 290 / 295 (5) |

## back

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-new | 18 / 22 | 37 / 38 | – | – | – | bfcache 45 |
| krunk-old | 18 / 24 | 36 / 38 | – | – | – | bfcache 45 |

Per page, frame (median / p90, n):

| page | krunk-new | krunk-old |
|---|---|---|
| news | 28 / 37 (5) | 31 / 36 (5) |
| spa | 27 / 34 (5) | 29 / 38 (5) |
| search | 44 / 47 (5) | 43 / 60 (5) |
| shop | 39 / 44 (5) | 38 / 41 (5) |
| youtube | 76 / 88 (5) | 66 / 90 (5) |
| wiki | 32 / 32 (5) | 30 / 32 (5) |
| ads | 49 / 50 (5) | 47 / 50 (5) |
| docs | 35 / 36 (5) | 30 / 37 (5) |
| hn | 37 / 38 (5) | 36 / 38 (5) |

## forward

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-new | 11 / 13 (8p) | 26 / 29 (8p) | 335 / 423 (1p) | 726 / 935 (1p) | 1864 / 2385 (1p) | bfcache 40, load 5 |
| krunk-old | 11 / 14 (8p) | 27 / 29 (8p) | 322 / 342 (1p) | 3922 / 4246 (1p) | 1897 / 4242 (1p) | bfcache 40, load 5 |

Per page, frame (median / p90, n):

| page | krunk-new | krunk-old |
|---|---|---|
| news | 30 / 32 (5) | 30 / 35 (5) |
| spa | 32 / 34 (5) | 36 / 40 (5) |
| search | 26 / 28 (5) | 25 / 29 (5) |
| shop | 27 / 29 (5) | 33 / 35 (5) |
| youtube | 726 / 935 (5) | 3922 / 4246 (5) |
| wiki | 26 / 27 (5) | 26 / 26 (5) |
| ads | 20 / 38 (5) | 25 / 26 (5) |
| docs | 27 / 28 (5) | 27 / 27 (5) |
| hn | 26 / 28 (5) | 27 / 28 (5) |

Back/forward cache not used:

- krunk-new: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-old: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5

## A/B verdicts (corpus median (p90), ms from the input)

| A → B | scenario | metric | A | B | change | band | verdict |
|---|---|---|---|---|---|---|---|
| krunk-old → krunk-new | warm | fcp | 491 (525) | 244 (270) | -50.3 % | 7.4 % | **beats the band** |
| krunk-old → krunk-new | warm | lcp | 581 (637) | 366 (400) | -37.0 % | 18.4 % | **beats the band** |
| krunk-old → krunk-new | warm | load | 670 (822) | 458 (475) | -31.7 % | 26.9 % | **beats the band** |
| krunk-old → krunk-new | warm | ttfb | 153 (156) | 60 (61) | -61.0 % | 7.6 % | **beats the band** |
| krunk-old → krunk-new | warm | nav | 12 (13) | 9 (11) | -20.7 % | 20.2 % | **beats the band** |
| krunk-old → krunk-new | link | fcp | 488 (520) | 490 (512) | +0.5 % | 7.6 % | within the band |
| krunk-old → krunk-new | link | lcp | 559 (594) | 562 (588) | +0.6 % | 19.9 % | within the band |
| krunk-old → krunk-new | link | load | 636 (801) | 622 (648) | -2.2 % | 4.1 % | within the band |
| krunk-old → krunk-new | link | ttfb | 158 (162) | 137 (140) | -13.5 % | 3.4 % | **beats the band** |
| krunk-old → krunk-new | link | nav | 10 (14) | 6 (7) | -41.7 % | 9.2 % | **beats the band** |
| krunk-old → krunk-new | back | frame | 36 (38) | 37 (38) | +1.7 % | 3.4 % | within the band |
| krunk-old → krunk-new | forward | frame | 27 (29) | 26 (29) | -1.9 % | 26.8 % | within the band |
| krunk-old → krunk-new | forward | lcp | 3922 (4246) | 726 (935) | -81.5 % | 6.0 % | **beats the band** |
