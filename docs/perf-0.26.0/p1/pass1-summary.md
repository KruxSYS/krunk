# Page-load pass: /work/projects/extra/browser/.pipeline/evidence-0.26.0/p1/ab-a

## Headline: corpus median (p90) over cold + warm typed loads

Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.

| browser | ttfb | fcp | lcp | dcl | load | nav | fcp_ns | lcp_ns | load_ns | pages |
|---|---|---|---|---|---|---|---|---|---|---|
| krunk-base | 100 (154) | 369 (470) | 425 (525) | 463 (569) | 560 (723) | 8 (13) | 282 (396) | 362 (464) | 499 (567) | 9 |
| krunk-pre | 93 (142) | 370 (463) | 432 (510) | 462 (556) | 564 (587) | 9 (14) | 304 (384) | 364 (452) | 475 (561) | 9 |
| krunk-omni | 60 (65) | 321 (379) | 400 (426) | 434 (468) | 524 (566) | 10 (14) | 248 (296) | 314 (368) | 456 (486) | 9 |
| krunk-block | 58 (66) | 323 (372) | 378 (429) | 416 (470) | 523 (546) | 10 (13) | 248 (304) | 316 (368) | 436 (487) | 9 |

Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).

## cold

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-base | 11 / 17 | 151 / 164 | 465 / 488 | 516 / 567 | 563 / 578 | 696 / 744 | load 45 |
| krunk-pre | 12 / 17 | 141 / 146 | 462 / 466 | 505 / 513 | 552 / 557 | 573 / 590 | load 45 |
| krunk-omni | 11 / 16 | 63 / 69 | 372 / 390 | 414 / 435 | 458 / 472 | 494 / 506 | load 45 |
| krunk-block | 10 / 15 | 65 / 68 | 372 / 391 | 418 / 454 | 465 / 476 | 492 / 517 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-base | krunk-pre | krunk-omni | krunk-block |
|---|---|---|---|---|
| news | 865 / 1637 (5) | 826 / 868 (5) | 749 / 789 (5) | 756 / 779 (5) |
| spa | 369 / 377 (5) | 352 / 370 (5) | 292 / 302 (5) | 279 / 314 (5) |
| search | 1419 / 1494 (5) | 1401 / 1455 (5) | 1326 / 1404 (5) | 1354 / 1373 (5) |
| shop | 771 / 1168 (5) | 791 / 823 (5) | 705 / 834 (5) | 762 / 1062 (5) |
| youtube | 4089 / 4372 (5) | 3969 / 4216 (5) | 3982 / 4102 (5) | 3889 / 4241 (5) |
| wiki | 409 / 421 (5) | 397 / 413 (5) | 312 / 372 (5) | 317 / 321 (5) |
| ads | 516 / 567 (5) | 505 / 513 (5) | 414 / 435 (5) | 418 / 454 (5) |
| docs | 465 / 488 (5) | 462 / 466 (5) | 372 / 390 (5) | 372 / 391 (5) |
| hn | 271 / 280 (5) | 261 / 269 (5) | 182 / 189 (5) | 182 / 190 (5) |

## warm

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-base | 8 / 11 | 54 / 56 | 236 / 252 | 324 / 337 | 357 / 366 | 404 / 433 | load 45 |
| krunk-pre | 9 / 12 | 54 / 58 | 242 / 251 | 333 / 349 | 348 / 356 | 402 / 416 | load 45 |
| krunk-omni | 9 / 11 | 55 / 60 | 236 / 255 | 336 / 394 | 356 / 394 | 399 / 444 | load 45 |
| krunk-block | 8 / 10 | 54 / 60 | 236 / 244 | 328 / 340 | 355 / 362 | 399 / 412 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-base | krunk-pre | krunk-omni | krunk-block |
|---|---|---|---|---|
| news | 621 / 627 (5) | 601 / 620 (5) | 606 / 687 (5) | 589 / 625 (5) |
| spa | 222 / 228 (5) | 238 / 251 (5) | 234 / 252 (5) | 230 / 244 (5) |
| search | 324 / 337 (5) | 335 / 349 (5) | 336 / 398 (5) | 328 / 340 (5) |
| shop | 597 / 814 (5) | 623 / 629 (5) | 601 / 643 (5) | 602 / 892 (5) |
| youtube | 3757 / 4080 (5) | 333 / 3614 (5) | 345 / 3752 (5) | 3591 / 4116 (5) |
| wiki | 273 / 302 (5) | 267 / 279 (5) | 272 / 301 (5) | 271 / 296 (5) |
| ads | 348 / 368 (5) | 355 / 369 (5) | 345 / 394 (5) | 334 / 349 (5) |
| docs | 290 / 300 (5) | 271 / 293 (5) | 282 / 291 (5) | 286 / 292 (5) |
| hn | 152 / 159 (5) | 152 / 156 (5) | 149 / 166 (5) | 153 / 166 (5) |

## link

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-base | 12 / 16 | 158 / 162 | 488 / 506 | 523 / 553 | 567 / 578 | 733 / 747 | load 45 |
| krunk-pre | 10 / 17 | 144 / 148 | 442 / 457 | 498 / 518 | 542 / 562 | 577 / 585 | load 45 |
| krunk-omni | 11 / 19 | 144 / 151 | 459 / 474 | 497 / 505 | 541 / 550 | 572 / 589 | load 45 |
| krunk-block | 8 / 9 | 140 / 145 | 455 / 485 | 504 / 521 | 548 / 558 | 580 / 610 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-base | krunk-pre | krunk-omni | krunk-block |
|---|---|---|---|---|
| news | 840 / 868 (5) | 841 / 847 (5) | 830 / 858 (5) | 826 / 850 (5) |
| spa | 382 / 394 (5) | 362 / 372 (5) | 355 / 392 (5) | 352 / 412 (5) |
| search | 1426 / 1468 (5) | 1400 / 1439 (5) | 1390 / 1829 (5) | 1412 / 1491 (5) |
| shop | 814 / 914 (5) | 802 / 831 (5) | 809 / 907 (5) | 793 / 1354 (5) |
| youtube | 4101 / 4254 (5) | 4082 / 4117 (5) | 3957 / 4616 (5) | 3986 / 4439 (5) |
| wiki | 407 / 447 (5) | 394 / 409 (5) | 399 / 412 (5) | 382 / 398 (5) |
| ads | 523 / 553 (5) | 498 / 518 (5) | 497 / 505 (5) | 504 / 521 (5) |
| docs | 488 / 506 (5) | 442 / 457 (5) | 459 / 474 (5) | 455 / 485 (5) |
| hn | 287 / 292 (5) | 272 / 280 (5) | 271 / 279 (5) | 270 / 284 (5) |

## back

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-base | 17 / 20 | 33 / 35 | – | – | – | bfcache 45 |
| krunk-pre | 17 / 18 | 31 / 38 | – | – | – | bfcache 45 |
| krunk-omni | 17 / 18 | 33 / 37 | – | – | – | bfcache 45 |
| krunk-block | 18 / 21 | 33 / 38 | – | – | – | bfcache 45 |

Per page, frame (median / p90, n):

| page | krunk-base | krunk-pre | krunk-omni | krunk-block |
|---|---|---|---|---|
| news | 32 / 35 (5) | 31 / 33 (5) | 32 / 37 (5) | 30 / 35 (5) |
| spa | 31 / 34 (5) | 31 / 34 (5) | 35 / 39 (5) | 28 / 35 (5) |
| search | 33 / 44 (5) | 30 / 38 (5) | 39 / 44 (5) | 33 / 40 (5) |
| shop | 33 / 41 (5) | 29 / 40 (5) | 24 / 30 (5) | 29 / 48 (5) |
| youtube | 61 / 67 (5) | 67 / 71 (5) | 60 / 67 (5) | 63 / 65 (5) |
| wiki | 25 / 32 (5) | 25 / 31 (5) | 27 / 32 (5) | 29 / 34 (5) |
| ads | 44 / 46 (5) | 33 / 46 (5) | 44 / 46 (5) | 42 / 44 (5) |
| docs | 33 / 35 (5) | 36 / 38 (5) | 30 / 35 (5) | 33 / 38 (5) |
| hn | 32 / 35 (5) | 33 / 39 (5) | 33 / 35 (5) | 33 / 36 (5) |

## forward

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-base | 10 / 13 (8p) | 25 / 28 (8p) | 302 / 319 (1p) | 3726 / 3855 (1p) | 1762 / 1865 (1p) | bfcache 40, load 5 |
| krunk-pre | 10 / 10 (8p) | 24 / 28 (8p) | 313 / 327 (1p) | 3643 / 3736 (1p) | 1751 / 1946 (1p) | bfcache 40, load 5 |
| krunk-omni | 10 / 12 (8p) | 25 / 31 (8p) | 322 / 338 (1p) | 3674 / 3810 (1p) | 1754 / 1776 (1p) | bfcache 40, load 5 |
| krunk-block | 10 / 12 (8p) | 25 / 28 (8p) | 306 / 358 (1p) | 3729 / 3826 (1p) | 1747 / 1900 (1p) | bfcache 40, load 5 |

Per page, frame (median / p90, n):

| page | krunk-base | krunk-pre | krunk-omni | krunk-block |
|---|---|---|---|---|
| news | 22 / 27 (5) | 24 / 27 (5) | 28 / 39 (5) | 25 / 28 (5) |
| spa | 24 / 33 (5) | 27 / 31 (5) | 31 / 33 (5) | 33 / 34 (5) |
| search | 24 / 28 (5) | 24 / 27 (5) | 24 / 28 (5) | 26 / 28 (5) |
| shop | 26 / 31 (5) | 27 / 32 (5) | 25 / 34 (5) | 25 / 29 (5) |
| youtube | 3726 / 3855 (5) | 3643 / 3736 (5) | 3674 / 3810 (5) | 3729 / 3826 (5) |
| wiki | 25 / 27 (5) | 19 / 27 (5) | 23 / 33 (5) | 22 / 27 (5) |
| ads | 24 / 28 (5) | 21 / 29 (5) | 20 / 29 (5) | 25 / 35 (5) |
| docs | 25 / 29 (5) | 26 / 28 (5) | 26 / 28 (5) | 23 / 27 (5) |
| hn | 26 / 29 (5) | 23 / 27 (5) | 24 / 26 (5) | 24 / 26 (5) |

Back/forward cache not used:

- krunk-base: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-pre: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-omni: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-block: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5

## A/B verdicts (corpus median (p90), ms from the input)

| A → B | scenario | metric | A | B | change | band | verdict |
|---|---|---|---|---|---|---|---|
| krunk-base → krunk-pre | cold | fcp | 465 (488) | 462 (466) | -0.7 % | 19.8 % | within the band |
| krunk-base → krunk-pre | cold | lcp | 516 (567) | 505 (513) | -2.0 % | 35.1 % | within the band |
| krunk-base → krunk-pre | cold | load | 696 (744) | 573 (590) | -17.7 % | 16.7 % | **beats the band** |
| krunk-base → krunk-pre | cold | ttfb | 151 (164) | 141 (146) | -6.6 % | 1.2 % | **beats the band** |
| krunk-base → krunk-pre | cold | nav | 11 (17) | 12 (17) | +16.0 % | 5.9 % | slower (beyond the band) |
| krunk-base → krunk-pre | warm | fcp | 236 (252) | 242 (251) | +2.6 % | 7.4 % | within the band |
| krunk-base → krunk-pre | warm | lcp | 324 (337) | 333 (349) | +2.9 % | 18.4 % | within the band |
| krunk-base → krunk-pre | warm | load | 404 (433) | 402 (416) | -0.3 % | 26.9 % | within the band |
| krunk-base → krunk-pre | warm | ttfb | 54 (56) | 54 (58) | +0.4 % | 7.6 % | within the band |
| krunk-base → krunk-pre | warm | nav | 8 (11) | 9 (12) | +16.0 % | 20.2 % | within the band |
| krunk-base → krunk-pre | link | fcp | 488 (506) | 442 (457) | -9.3 % | 7.6 % | **beats the band** |
| krunk-base → krunk-pre | link | lcp | 523 (553) | 498 (518) | -4.8 % | 19.9 % | within the band |
| krunk-base → krunk-pre | link | load | 733 (747) | 577 (585) | -21.3 % | 4.1 % | **beats the band** |
| krunk-base → krunk-pre | link | ttfb | 158 (162) | 144 (148) | -9.0 % | 3.4 % | **beats the band** |
| krunk-base → krunk-pre | link | nav | 12 (16) | 10 (17) | -15.3 % | 9.2 % | **beats the band** |
| krunk-base → krunk-pre | back | frame | 33 (35) | 31 (38) | -4.3 % | 3.4 % | **beats the band** |
| krunk-base → krunk-pre | forward | frame | 25 (28) | 24 (28) | -2.2 % | 26.8 % | within the band |
| krunk-base → krunk-pre | forward | lcp | 3726 (3855) | 3643 (3736) | -2.2 % | 6.0 % | within the band |
| krunk-pre → krunk-omni | cold | fcp | 462 (466) | 372 (390) | -19.3 % | 19.8 % | within the band |
| krunk-pre → krunk-omni | cold | lcp | 505 (513) | 414 (435) | -18.0 % | 35.1 % | within the band |
| krunk-pre → krunk-omni | cold | load | 573 (590) | 494 (506) | -13.9 % | 16.7 % | within the band |
| krunk-pre → krunk-omni | cold | ttfb | 141 (146) | 63 (69) | -55.2 % | 1.2 % | **beats the band** |
| krunk-pre → krunk-omni | cold | nav | 12 (17) | 11 (16) | -11.4 % | 5.9 % | **beats the band** |
| krunk-pre → krunk-omni | warm | fcp | 242 (251) | 236 (255) | -2.6 % | 7.4 % | within the band |
| krunk-pre → krunk-omni | warm | lcp | 333 (349) | 336 (394) | +0.9 % | 18.4 % | within the band |
| krunk-pre → krunk-omni | warm | load | 402 (416) | 399 (444) | -0.9 % | 26.9 % | within the band |
| krunk-pre → krunk-omni | warm | ttfb | 54 (58) | 55 (60) | +1.1 % | 7.6 % | within the band |
| krunk-pre → krunk-omni | warm | nav | 9 (12) | 9 (11) | +3.4 % | 20.2 % | within the band |
| krunk-pre → krunk-omni | link | fcp | 442 (457) | 459 (474) | +3.9 % | 7.6 % | within the band |
| krunk-pre → krunk-omni | link | lcp | 498 (518) | 497 (505) | -0.3 % | 19.9 % | within the band |
| krunk-pre → krunk-omni | link | load | 577 (585) | 572 (589) | -0.8 % | 4.1 % | within the band |
| krunk-pre → krunk-omni | link | ttfb | 144 (148) | 144 (151) | -0.3 % | 3.4 % | within the band |
| krunk-pre → krunk-omni | link | nav | 10 (17) | 11 (19) | +6.7 % | 9.2 % | within the band |
| krunk-pre → krunk-omni | back | frame | 31 (38) | 33 (37) | +4.8 % | 3.4 % | slower (beyond the band) |
| krunk-pre → krunk-omni | forward | frame | 24 (28) | 25 (31) | +2.9 % | 26.8 % | within the band |
| krunk-pre → krunk-omni | forward | lcp | 3643 (3736) | 3674 (3810) | +0.9 % | 6.0 % | within the band |
| krunk-omni → krunk-block | cold | fcp | 372 (390) | 372 (391) | -0.2 % | 19.8 % | within the band |
| krunk-omni → krunk-block | cold | lcp | 414 (435) | 418 (454) | +1.0 % | 35.1 % | within the band |
| krunk-omni → krunk-block | cold | load | 494 (506) | 492 (517) | -0.4 % | 16.7 % | within the band |
| krunk-omni → krunk-block | cold | ttfb | 63 (69) | 65 (68) | +2.9 % | 1.2 % | slower (beyond the band) |
| krunk-omni → krunk-block | cold | nav | 11 (16) | 10 (15) | -4.6 % | 5.9 % | within the band |
| krunk-omni → krunk-block | warm | fcp | 236 (255) | 236 (244) | +0.3 % | 7.4 % | within the band |
| krunk-omni → krunk-block | warm | lcp | 336 (394) | 328 (340) | -2.3 % | 18.4 % | within the band |
| krunk-omni → krunk-block | warm | load | 399 (444) | 399 (412) | -0.0 % | 26.9 % | within the band |
| krunk-omni → krunk-block | warm | ttfb | 55 (60) | 54 (60) | -1.8 % | 7.6 % | within the band |
| krunk-omni → krunk-block | warm | nav | 9 (11) | 8 (10) | -8.9 % | 20.2 % | within the band |
| krunk-omni → krunk-block | link | fcp | 459 (474) | 455 (485) | -1.0 % | 7.6 % | within the band |
| krunk-omni → krunk-block | link | lcp | 497 (505) | 504 (521) | +1.5 % | 19.9 % | within the band |
| krunk-omni → krunk-block | link | load | 572 (589) | 580 (610) | +1.4 % | 4.1 % | within the band |
| krunk-omni → krunk-block | link | ttfb | 144 (151) | 140 (145) | -2.4 % | 3.4 % | within the band |
| krunk-omni → krunk-block | link | nav | 11 (19) | 8 (9) | -32.1 % | 9.2 % | **beats the band** |
| krunk-omni → krunk-block | back | frame | 33 (37) | 33 (38) | +0.0 % | 3.4 % | within the band |
| krunk-omni → krunk-block | forward | frame | 25 (31) | 25 (28) | +0.8 % | 26.8 % | within the band |
| krunk-omni → krunk-block | forward | lcp | 3674 (3810) | 3729 (3826) | +1.5 % | 6.0 % | within the band |
