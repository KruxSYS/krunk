# Page-load pass: /work/projects/extra/browser/.pipeline/evidence-0.26.0/runs/baseline, /work/projects/extra/browser/.pipeline/evidence-0.26.0/runs/baseline-fill

## Headline: corpus median (p90) over cold + warm typed loads

Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.

| browser | ttfb | fcp | lcp | dcl | load | nav | fcp_ns | lcp_ns | load_ns | pages |
|---|---|---|---|---|---|---|---|---|---|---|
| krunk-a | 149 (172) | 460 (661) | 662 (837) | 726 (894) | 866 (1000) | 14 (19) | 396 (552) | 450 (644) | 744 (914) | 9 |
| krunk-b | 148 (172) | 461 (636) | 496 (1046) | 538 (1229) | 777 (1985) | 13 (19) | 382 (540) | 454 (752) | 640 (1107) | 9 |
| chrome | 115 (147) | 490 (731) | 620 (840) | 800 (977) | 1121 (1346) | 18 (43) | 366 (564) | 418 (640) | 652 (810) | 9 |
| edge | 395 (550) | 838 (1002) | 1056 (1444) | 1248 (1383) | 1278 (1672) | 327 (501) | 390 (540) | 412 (608) | 586 (741) | 9 |
| krunk | 148 (170) | 460 (651) | 602 (817) | 621 (869) | 857 (1000) | 13 (18) | 388 (540) | 450 (644) | 696 (894) | 9 |

- krunk-a: fcp -6.2 % vs chrome, lcp +6.8 % vs chrome, load -22.8 % vs chrome, fcp -45.1 % vs edge, lcp -37.3 % vs edge, load -32.3 % vs edge
- krunk-b: fcp -5.8 % vs chrome, lcp -19.9 % vs chrome, load -30.6 % vs chrome, fcp -44.9 % vs edge, lcp -53.0 % vs edge, load -39.2 % vs edge
- krunk: fcp -6.1 % vs chrome, lcp -2.8 % vs chrome, load -23.5 % vs chrome, fcp -45.1 % vs edge, lcp -42.9 % vs edge, load -32.9 % vs edge

Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).

## cold

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-a | 17 / 19 | 170 / 172 | 661 / 661 | 817 / 837 | 869 / 894 | 911 / 949 | load 18, timeout 27 |
| krunk-b | 17 / 18 | 170 / 172 | 530 / 558 | 530 / 558 | 615 / 758 | 763 / 840 | load 21, timeout 24 |
| chrome | 17 / 24 | 145 / 149 | 627 / 658 | 713 / 799 | 720 / 854 | 1345 / 1437 | load 40, timeout 5 |
| edge | 254 / 293 | 376 / 414 | 877 / 990 | 899 / 1178 | 1193 / 1322 | 1377 / 1466 | load 40, timeout 5 |
| krunk | 18 / 19 | 170 / 173 | 596 / 661 | 794 / 837 | 865 / 894 | 906 / 949 | load 39, timeout 51 |

Per page, lcp (median / p90, n):

| page | krunk-a | krunk-b | chrome | edge | krunk |
|---|---|---|---|---|---|
| news | 1251 / 1346 (3) | 1260 / 1339 (4) | 800 / 853 (5) | 899 / 1037 (5) | 1251 / 1346 (7) |
| spa | 527 / 550 (2) | 514 / 544 (3) | 441 / 551 (5) | 510 / 571 (5) | 514 / 550 (5) |
| search | 2597 / 2912 (2) | 2160 / 2328 (4) | 1660 / 1950 (5) | 2400 / 3154 (5) | 2258 / 2912 (6) |
| shop | 1126 / 1326 (2) | 895 / 1046 (2) | 876 / 1054 (5) | 1341 / 1502 (5) | 986 / 1326 (4) |
| youtube | 3539 / 5707 (2) | 5485 / 5654 (2) | 2486 / 2526 (4) | 2804 / 3066 (4) | 5485 / 5707 (4) |
| wiki | 569 / 569 (1) | 523 / 558 (2) | 542 / 581 (4) | 665 / 742 (4) | 558 / 569 (3) |
| ads | 817 / 837 (3) | 495 / 495 (1) | 713 / 799 (4) | 1150 / 1266 (4) | 794 / 837 (4) |
| docs | 661 / 661 (1) | 530 / 530 (1) | 627 / 731 (4) | 882 / 1178 (4) | 596 / 661 (2) |
| hn | 352 / 352 (2) | 359 / 363 (2) | 314 / 328 (4) | 326 / 375 (4) | 354 / 363 (4) |

## warm

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-a | 12 / 15 | 145 / 154 | 400 / 534 | 611 / 665 | 634 / 728 | 866 / 1000 | load 45 |
| krunk-b | 13 / 16 | 144 / 154 | 389 / 534 | 498 / 965 | 540 / 1048 | 633 / 1365 | load 45 |
| chrome | 19 / 84 | 67 / 90 | 405 / 486 | 436 / 665 | 846 / 931 | 1025 / 1042 | load 40, timeout 5 |
| edge | 471 / 506 | 521 / 550 | 855 / 1002 | 1233 / 1444 | 1261 / 1436 | 1359 / 1672 | load 40, timeout 5 |
| krunk | 12 / 15 | 143 / 154 | 381 / 533 | 546 / 665 | 574 / 728 | 695 / 1000 | load 90 |

Per page, lcp (median / p90, n):

| page | krunk-a | krunk-b | chrome | edge | krunk |
|---|---|---|---|---|---|
| news | 1145 / 1227 (5) | 881 / 1148 (5) | 388 / 486 (5) | 765 / 878 (5) | 993 / 1226 (10) |
| spa | 360 / 632 (5) | 389 / 417 (5) | 405 / 452 (5) | 390 / 470 (5) | 378 / 417 (10) |
| search | 1370 / 1427 (5) | 558 / 1368 (5) | 1310 / 1399 (5) | 2039 / 2403 (5) | 611 / 1390 (10) |
| shop | 734 / 882 (5) | 752 / 965 (5) | 613 / 781 (5) | 1428 / 1883 (5) | 746 / 882 (10) |
| youtube | 4022 / 4653 (5) | 4023 / 5765 (5) | 1989 / 2610 (4) | 2751 / 3114 (4) | 4022 / 4846 (10) |
| wiki | 400 / 413 (5) | 378 / 385 (5) | 342 / 371 (4) | 693 / 777 (4) | 381 / 400 (10) |
| ads | 611 / 665 (5) | 498 / 770 (5) | 814 / 937 (4) | 1329 / 1607 (4) | 546 / 665 (10) |
| docs | 454 / 533 (5) | 455 / 3768 (5) | 436 / 665 (4) | 1233 / 1444 (4) | 455 / 533 (10) |
| hn | 272 / 277 (5) | 254 / 258 (5) | 220 / 234 (4) | 246 / 384 (4) | 256 / 276 (10) |

## link

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-a | 16 / 19 | 176 / 185 | 583 / 716 | 783 / 839 | 746 / 914 | 940 / 965 | load 45 |
| krunk-b | 16 / 19 | 176 / 184 | 627 / 752 | 627 / 849 | 688 / 925 | 903 / 968 | load 45 |
| chrome | 5 / 7 | 142 / 145 | 546 / 628 | 669 / 762 | 711 / 789 | 1266 / 1413 | load 40, timeout 5 |
| edge | 6 / 6 | 144 / 146 | 508 / 630 | 590 / 711 | 718 / 927 | 1300 / 1776 | load 40, timeout 5 |
| krunk | 16 / 19 | 175 / 182 | 605 / 724 | 661 / 839 | 713 / 917 | 910 / 964 | load 90 |

Per page, lcp (median / p90, n):

| page | krunk-a | krunk-b | chrome | edge | krunk |
|---|---|---|---|---|---|
| news | 1067 / 1224 (5) | 1341 / 1380 (5) | 872 / 1018 (5) | 590 / 802 (5) | 1148 / 1362 (10) |
| spa | 541 / 554 (5) | 520 / 550 (5) | 493 / 545 (5) | 474 / 536 (5) | 524 / 550 (10) |
| search | 2168 / 2437 (5) | 1802 / 2185 (5) | 1592 / 2271 (5) | 1829 / 1860 (5) | 2130 / 2300 (10) |
| shop | 1203 / 1303 (5) | 1330 / 1371 (5) | 738 / 955 (5) | 850 / 978 (5) | 1262 / 1351 (10) |
| youtube | 4461 / 6088 (5) | 5022 / 5684 (5) | 1822 / 2484 (4) | 2794 / 3150 (4) | 4989 / 5684 (10) |
| wiki | 571 / 585 (5) | 591 / 597 (5) | 546 / 577 (4) | 497 / 546 (4) | 572 / 593 (10) |
| ads | 783 / 839 (5) | 540 / 849 (5) | 669 / 762 (4) | 668 / 711 (4) | 661 / 839 (10) |
| docs | 583 / 724 (5) | 627 / 758 (5) | 601 / 719 (4) | 508 / 630 (4) | 605 / 724 (10) |
| hn | 337 / 382 (5) | 352 / 367 (5) | 311 / 331 (4) | 317 / 338 (4) | 345 / 367 (10) |

## back

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-a | 28 / 35 | 39 / 55 | – | – | – | bfcache 45 |
| krunk-b | 29 / 34 | 40 / 55 | – | – | – | bfcache 45 |
| chrome | 35 / 38 | 55 / 58 | – | – | – | bfcache 40, timeout 5 |
| edge | 44 / 54 | 53 / 72 | – | – | – | bfcache 40, timeout 5 |
| krunk | 28 / 31 | 39 / 55 | – | – | – | bfcache 90 |

Per page, frame (median / p90, n):

| page | krunk-a | krunk-b | chrome | edge | krunk |
|---|---|---|---|---|---|
| news | 24 / 37 (5) | 37 / 39 (5) | 52 / 54 (5) | 52 / 70 (5) | 35 / 38 (10) |
| spa | 38 / 38 (5) | 39 / 52 (5) | 53 / 54 (5) | 53 / 80 (5) | 38 / 44 (10) |
| search | 62 / 66 (5) | 65 / 66 (5) | 64 / 65 (5) | 80 / 93 (5) | 64 / 66 (10) |
| shop | 48 / 58 (5) | 40 / 53 (5) | 60 / 64 (5) | 66 / 72 (5) | 45 / 55 (10) |
| youtube | 127 / 225 (5) | 125 / 230 (5) | 107 / 136 (4) | 56 / 141 (4) | 126 / 225 (10) |
| wiki | 34 / 38 (5) | 34 / 36 (5) | 55 / 58 (4) | 61 / 65 (4) | 34 / 36 (10) |
| ads | 46 / 54 (5) | 53 / 76 (5) | 97 / 101 (4) | 42 / 100 (4) | 49 / 55 (10) |
| docs | 37 / 58 (5) | 38 / 56 (5) | 40 / 54 (4) | 37 / 69 (4) | 38 / 56 (10) |
| hn | 39 / 55 (5) | 40 / 55 (5) | 49 / 54 (4) | 53 / 58 (4) | 39 / 55 (10) |

## forward

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-a | 15 / 19 (8p) | 25 / 36 (8p) | 383 / 585 (1p) | 4507 / 5289 (1p) | 2216 / 3401 (1p) | bfcache 40, load 5 |
| krunk-b | 16 / 18 (8p) | 32 / 34 (8p) | 487 / 586 (1p) | 4775 / 5470 (1p) | 2858 / 3353 (1p) | bfcache 40, load 5 |
| chrome | 19 / 22 (8p) | 38 / 40 (8p) | 438 / 500 (1p) | 2019 / 2736 (1p) | 2236 / 2952 (1p) | bfcache 36, load 4, timeout 5 |
| edge | 29 / 32 (8p) | 44 / 50 (8p) | 525 / 550 (1p) | 2538 / 2721 (1p) | 3178 / 4376 (1p) | bfcache 36, load 4, timeout 5 |
| krunk | 15 / 18 (8p) | 27 / 33 (8p) | 444 / 585 (1p) | 4754 / 5289 (1p) | 2787 / 3353 (1p) | bfcache 80, load 10 |

Per page, frame (median / p90, n):

| page | krunk-a | krunk-b | chrome | edge | krunk |
|---|---|---|---|---|---|
| news | 41 / 53 (5) | 39 / 47 (5) | 38 / 51 (5) | 54 / 61 (5) | 40 / 47 (10) |
| spa | 25 / 26 (5) | 25 / 26 (5) | 24 / 42 (5) | 38 / 39 (5) | 25 / 26 (10) |
| search | 23 / 39 (5) | 23 / 26 (5) | 37 / 39 (5) | 46 / 52 (5) | 23 / 26 (10) |
| shop | 48 / 56 (5) | 39 / 60 (5) | 40 / 45 (5) | 52 / 54 (5) | 45 / 56 (10) |
| youtube | 4507 / 5289 (5) | 4775 / 5470 (5) | 2019 / 2736 (4) | 2538 / 2721 (4) | 4754 / 5289 (10) |
| wiki | 29 / 38 (5) | 39 / 40 (5) | 39 / 40 (4) | 37 / 38 (4) | 36 / 40 (10) |
| ads | 25 / 34 (5) | 38 / 54 (5) | 39 / 41 (4) | 46 / 96 (4) | 29 / 38 (10) |
| docs | 20 / 31 (5) | 24 / 26 (5) | 22 / 23 (4) | 42 / 48 (4) | 21 / 26 (10) |
| hn | 25 / 30 (5) | 19 / 28 (5) | 22 / 26 (4) | 36 / 37 (4) | 24 / 28 (10) |

Back/forward cache not used:

- krunk-a: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-b: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- chrome: Circumstantial:ServiceWorkerUnregistration ×4, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×4
- edge: Circumstantial:ServiceWorkerUnregistration ×4, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×4
- krunk: Circumstantial:ServiceWorkerUnregistration ×10, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×10

## Noise band (krunk-a vs krunk-b, same build)

Band = the larger of |corpus median diff %| and the median per-page |diff %|; a fix must beat it.

| scenario | metric | corpus median diff | per-page median |diff| (median, max) | band |
|---|---|---|---|---|
| cold | fcp | -131 ms (-19.8 %) | 12.8 %, 30.5 % | 19.8 % |
| cold | lcp | -286 ms (-35.1 %) | 16.8 %, 39.4 % | 35.1 % |
| cold | load | -148 ms (-16.3 %) | 16.7 %, 39.1 % | 16.7 % |
| cold | ttfb | +0 ms (+0.1 %) | 1.2 %, 15.1 % | 1.2 % |
| cold | nav | +0 ms (+0.3 %) | 5.9 %, 35.9 % | 5.9 % |
| warm | fcp | -11 ms (-2.7 %) | 7.4 %, 64.2 % | 7.4 % |
| warm | lcp | -112 ms (-18.4 %) | 6.4 %, 59.2 % | 18.4 % |
| warm | load | -233 ms (-26.9 %) | 4.3 %, 57.9 % | 26.9 % |
| warm | ttfb | -1 ms (-0.6 %) | 7.6 %, 93.6 % | 7.6 % |
| warm | nav | +0 ms (+1.6 %) | 20.2 %, 98.8 % | 20.2 % |
| link | fcp | +44 ms (+7.6 %) | 4.0 %, 15.6 % | 7.6 % |
| link | lcp | -156 ms (-19.9 %) | 9.5 %, 31.0 % | 19.9 % |
| link | load | -37 ms (-3.9 %) | 4.1 %, 37.1 % | 4.1 % |
| link | ttfb | -0 ms (-0.1 %) | 3.4 %, 17.8 % | 3.4 % |
| link | nav | -1 ms (-4.3 %) | 9.2 %, 50.3 % | 9.2 % |
| back | frame | +0 ms (+0.5 %) | 3.4 %, 33.8 % | 3.4 % |
| forward | frame | +7 ms (+26.8 %) | 16.5 %, 35.9 % | 26.8 % |
| forward | lcp | +268 ms (+6.0 %) | 5.6 %, 5.6 % | 6.0 % |
