# Page-load pass: ../.pipeline/evidence-0.26.0/final, ../.pipeline/evidence-0.26.0/runs/baseline

## Headline: corpus median (p90) over cold + warm typed loads

Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.

| browser | ttfb | fcp | lcp | dcl | load | nav | fcp_ns | lcp_ns | load_ns | pages |
|---|---|---|---|---|---|---|---|---|---|---|
| krunk-new | 60 (65) | 375 (420) | 430 (492) | 468 (534) | 571 (636) | 9 (11) | 268 (364) | 370 (412) | 496 (543) | 9 |
| krunk-old | 153 (156) | 491 (525) | 581 (637) | 612 (677) | 670 (822) | 12 (13) | 416 (460) | 488 (516) | 615 (644) | 9 |
| krunk-a | 168 (176) | 622 (737) | 665 (837) | 728 (894) | 965 (1020) | 16 (20) | 556 (592) | 644 (720) | 775 (914) | 9 |
| krunk-b | 167 (174) | 573 (671) | 822 (958) | 898 (998) | 936 (1124) | 16 (19) | 460 (540) | 664 (756) | 886 (1022) | 9 |
| chrome | 115 (147) | 490 (731) | 620 (840) | 800 (977) | 1121 (1346) | 18 (43) | 366 (564) | 418 (640) | 652 (810) | 9 |
| edge | 395 (550) | 838 (1002) | 1056 (1444) | 1248 (1383) | 1278 (1672) | 327 (501) | 390 (540) | 412 (608) | 586 (741) | 9 |
| krunk-0255-baseline | 168 (174) | 589 (732) | 771 (837) | 860 (904) | 907 (1020) | 16 (19) | 506 (572) | 648 (720) | 881 (926) | 9 |

- krunk-new: fcp -23.5 % vs chrome, lcp -30.5 % vs chrome, load -49.0 % vs chrome, fcp -55.2 % vs edge, lcp -59.2 % vs edge, load -55.3 % vs edge
- krunk-old: fcp +0.3 % vs chrome, lcp -6.3 % vs chrome, load -40.2 % vs chrome, fcp -41.3 % vs edge, lcp -45.0 % vs edge, load -47.6 % vs edge
- krunk-a: fcp +27.0 % vs chrome, lcp +7.4 % vs chrome, load -13.8 % vs chrome, fcp -25.7 % vs edge, lcp -37.0 % vs edge, load -24.5 % vs edge
- krunk-b: fcp +16.8 % vs chrome, lcp +32.7 % vs chrome, load -16.5 % vs chrome, fcp -31.6 % vs edge, lcp -22.1 % vs edge, load -26.7 % vs edge
- krunk-0255-baseline: fcp +20.2 % vs chrome, lcp +24.4 % vs chrome, load -19.1 % vs chrome, fcp -29.7 % vs edge, lcp -27.0 % vs edge, load -29.0 % vs edge

Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).

## cold

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-new | 11 / 13 | 64 / 67 | 419 / 424 | 472 / 501 | 511 / 553 | 535 / 577 | load 45 |
| krunk-old | – | – | – | – | – | – | timeout 45 |
| krunk-a | 17 / 19 | 170 / 172 | 661 / 661 | 817 / 837 | 869 / 894 | 911 / 949 | load 18, timeout 23 |
| krunk-b | 18 / 19 (8p) | 171 / 172 (8p) | 568 / 597 (8p) | 712 / 802 (8p) | 888 / 994 (8p) | 1339 / 1412 (8p) | load 21, timeout 20 |
| chrome | 17 / 24 | 145 / 149 | 627 / 658 | 713 / 799 | 720 / 854 | 1345 / 1437 | load 40 |
| edge | 254 / 293 | 376 / 414 | 877 / 990 | 899 / 1178 | 1193 / 1322 | 1377 / 1466 | load 40 |
| krunk-0255-baseline | 18 / 19 | 170 / 173 | 596 / 661 | 817 / 837 | 869 / 894 | 911 / 949 | load 39, timeout 43 |

Per page, lcp (median / p90, n):

| page | krunk-new | krunk-old | krunk-a | krunk-b | chrome | edge | krunk-0255-baseline |
|---|---|---|---|---|---|---|---|
| news | 822 / 859 (5) | – | 1251 / 1346 (3) | 1260 / 1339 (4) | 800 / 853 (5) | 899 / 1037 (5) | 1251 / 1346 (7) |
| spa | 299 / 308 (5) | – | 527 / 550 (2) | 514 / 544 (3) | 441 / 551 (5) | 510 / 571 (5) | 514 / 550 (5) |
| search | 1525 / 1545 (5) | – | 2597 / 2912 (2) | 2160 / 2328 (4) | 1660 / 1950 (5) | 2400 / 3154 (5) | 2258 / 2912 (6) |
| shop | 832 / 860 (5) | – | 1126 / 1326 (2) | 895 / 1046 (2) | 876 / 1054 (5) | 1341 / 1502 (5) | 986 / 1326 (4) |
| youtube | 4251 / 4660 (5) | – | 3539 / 5707 (2) | 5316 / 5654 (3) | 2486 / 2526 (4) | 2804 / 3066 (4) | 5316 / 5707 (5) |
| wiki | 333 / 338 (5) | – | 569 / 569 (1) | 523 / 558 (2) | 542 / 581 (4) | 665 / 742 (4) | 558 / 569 (3) |
| ads | 472 / 501 (5) | – | 817 / 837 (3) | – | 713 / 799 (4) | 1150 / 1266 (4) | 817 / 837 (3) |
| docs | 419 / 424 (5) | – | 661 / 661 (1) | 530 / 530 (1) | 627 / 731 (4) | 882 / 1178 (4) | 596 / 661 (2) |
| hn | 189 / 194 (5) | – | 352 / 352 (2) | 359 / 363 (2) | 314 / 328 (4) | 326 / 375 (4) | 354 / 363 (4) |

## warm

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-new | 9 / 11 | 60 / 61 | 244 / 270 | 366 / 400 | 388 / 409 | 458 / 475 | load 45 |
| krunk-old | 12 / 13 | 153 / 156 | 491 / 525 | 581 / 637 | 612 / 677 | 670 / 822 | load 45 |
| krunk-a | 16 / 20 | 146 / 175 | 518 / 737 | 634 / 737 | 679 / 801 | 991 / 1020 | load 39, timeout 2 |
| krunk-b | 15 / 19 | 114 / 174 | 455 / 671 | 701 / 958 | 600 / 998 | 910 / 1124 | load 39, timeout 2 |
| chrome | 19 / 84 | 67 / 90 | 405 / 486 | 436 / 665 | 846 / 931 | 1025 / 1042 | load 40 |
| edge | 471 / 506 | 521 / 550 | 855 / 1002 | 1233 / 1444 | 1261 / 1436 | 1359 / 1672 | load 40 |
| krunk-0255-baseline | 15 / 20 | 135 / 175 | 479 / 740 | 662 / 958 | 620 / 998 | 758 / 1124 | load 78, timeout 4 |

Per page, lcp (median / p90, n):

| page | krunk-new | krunk-old | krunk-a | krunk-b | chrome | edge | krunk-0255-baseline |
|---|---|---|---|---|---|---|---|
| news | 685 / 716 (5) | 916 / 947 (5) | 1226 / 1356 (5) | 1105 / 1395 (5) | 388 / 486 (5) | 765 / 878 (5) | 1146 / 1356 (10) |
| spa | 241 / 269 (5) | 389 / 409 (5) | 335 / 576 (4) | 417 / 529 (5) | 405 / 452 (5) | 390 / 470 (5) | 389 / 576 (9) |
| search | 366 / 400 (5) | 1587 / 2058 (5) | 1181 / 1867 (4) | 558 / 1801 (5) | 1310 / 1399 (5) | 2039 / 2403 (5) | 605 / 1867 (9) |
| shop | 666 / 711 (5) | 889 / 1020 (5) | 922 / 1200 (5) | 965 / 1330 (5) | 613 / 781 (5) | 1428 / 1883 (5) | 930 / 1266 (10) |
| youtube | 709 / 3926 (5) | 4544 / 4710 (5) | 4653 / 6168 (5) | 4962 / 5765 (4) | 1989 / 2610 (4) | 2751 / 3114 (4) | 4846 / 6168 (9) |
| wiki | 276 / 287 (5) | 428 / 477 (5) | 580 / 612 (4) | 326 / 479 (3) | 342 / 371 (4) | 693 / 777 (4) | 479 / 612 (7) |
| ads | 418 / 428 (5) | 581 / 637 (5) | 634 / 665 (4) | 822 / 958 (4) | 814 / 937 (4) | 1329 / 1607 (4) | 662 / 958 (8) |
| docs | 321 / 389 (5) | 491 / 525 (5) | 623 / 737 (4) | 701 / 773 (4) | 436 / 665 (4) | 1233 / 1444 (4) | 673 / 773 (8) |
| hn | 156 / 168 (5) | 286 / 288 (5) | 297 / 375 (4) | 288 / 340 (4) | 220 / 234 (4) | 246 / 384 (4) | 288 / 375 (8) |

## link

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-new | 6 / 7 | 137 / 140 | 490 / 512 | 562 / 588 | 600 / 613 | 622 / 648 | load 45 |
| krunk-old | 10 / 14 | 158 / 162 | 488 / 520 | 559 / 594 | 601 / 623 | 636 / 801 | load 45 |
| krunk-a | 16 / 19 | 176 / 185 | 610 / 716 | 801 / 839 | 746 / 914 | 940 / 965 | load 41 |
| krunk-b | 16 / 19 | 177 / 184 | 665 / 752 | 686 / 849 | 738 / 925 | 903 / 968 | load 39, timeout 1 |
| chrome | 5 / 7 | 142 / 145 | 546 / 628 | 669 / 762 | 711 / 789 | 1266 / 1413 | load 40 |
| edge | 6 / 6 | 144 / 146 | 508 / 630 | 590 / 711 | 718 / 927 | 1300 / 1776 | load 40 |
| krunk-0255-baseline | 16 / 20 | 177 / 184 | 647 / 752 | 801 / 849 | 882 / 925 | 927 / 968 | load 80, timeout 1 |

Per page, lcp (median / p90, n):

| page | krunk-new | krunk-old | krunk-a | krunk-b | chrome | edge | krunk-0255-baseline |
|---|---|---|---|---|---|---|---|
| news | 886 / 890 (5) | 914 / 927 (5) | 1067 / 1224 (5) | 1351 / 1380 (4) | 872 / 1018 (5) | 590 / 802 (5) | 1071 / 1380 (9) |
| spa | 384 / 397 (5) | 381 / 391 (5) | 541 / 554 (5) | 520 / 550 (5) | 493 / 545 (5) | 474 / 536 (5) | 524 / 550 (10) |
| search | 1562 / 1654 (5) | 1592 / 2009 (5) | 2168 / 2437 (5) | 1802 / 2185 (5) | 1592 / 2271 (5) | 1829 / 1860 (5) | 2130 / 2300 (10) |
| shop | 853 / 880 (5) | 849 / 874 (5) | 1203 / 1303 (5) | 1330 / 1371 (5) | 738 / 955 (5) | 850 / 978 (5) | 1262 / 1351 (10) |
| youtube | 4329 / 4460 (5) | 4328 / 4447 (5) | 4461 / 6088 (5) | 5300 / 5684 (4) | 1822 / 2484 (4) | 2794 / 3150 (4) | 5022 / 6088 (9) |
| wiki | 406 / 431 (5) | 430 / 448 (5) | 572 / 585 (4) | 592 / 597 (4) | 546 / 577 (4) | 497 / 546 (4) | 579 / 597 (8) |
| ads | 562 / 588 (5) | 559 / 594 (5) | 801 / 839 (4) | 686 / 849 (4) | 669 / 762 (4) | 668 / 711 (4) | 801 / 849 (8) |
| docs | 490 / 512 (5) | 488 / 520 (5) | 625 / 724 (4) | 665 / 758 (4) | 601 / 719 (4) | 508 / 630 (4) | 647 / 758 (8) |
| hn | 268 / 276 (5) | 290 / 295 (5) | 351 / 382 (4) | 356 / 367 (4) | 311 / 331 (4) | 317 / 338 (4) | 356 / 382 (8) |

## back

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-new | 18 / 22 | 37 / 38 | – | – | – | bfcache 45 |
| krunk-old | 18 / 24 | 36 / 38 | – | – | – | bfcache 45 |
| krunk-a | 29 / 35 | 40 / 55 | – | – | – | bfcache 41 |
| krunk-b | 30 / 34 | 46 / 55 | – | – | – | bfcache 39, timeout 1 |
| chrome | 35 / 38 | 55 / 58 | – | – | – | bfcache 40 |
| edge | 44 / 54 | 53 / 72 | – | – | – | bfcache 40 |
| krunk-0255-baseline | 29 / 35 | 40 / 55 | – | – | – | bfcache 80, timeout 1 |

Per page, frame (median / p90, n):

| page | krunk-new | krunk-old | krunk-a | krunk-b | chrome | edge | krunk-0255-baseline |
|---|---|---|---|---|---|---|---|
| news | 28 / 37 (5) | 31 / 36 (5) | 24 / 37 (5) | 37 / 39 (4) | 52 / 54 (5) | 52 / 70 (5) | 35 / 39 (9) |
| spa | 27 / 34 (5) | 29 / 38 (5) | 38 / 38 (5) | 39 / 52 (5) | 53 / 54 (5) | 53 / 80 (5) | 38 / 44 (10) |
| search | 44 / 47 (5) | 43 / 60 (5) | 62 / 66 (5) | 65 / 66 (5) | 64 / 65 (5) | 80 / 93 (5) | 64 / 66 (10) |
| shop | 39 / 44 (5) | 38 / 41 (5) | 48 / 58 (5) | 40 / 53 (5) | 60 / 64 (5) | 66 / 72 (5) | 45 / 55 (10) |
| youtube | 76 / 88 (5) | 66 / 90 (5) | 127 / 225 (5) | 136 / 230 (4) | 107 / 136 (4) | 56 / 141 (4) | 127 / 230 (9) |
| wiki | 32 / 32 (5) | 30 / 32 (5) | 35 / 38 (4) | 35 / 36 (4) | 55 / 58 (4) | 61 / 65 (4) | 35 / 38 (8) |
| ads | 49 / 50 (5) | 47 / 50 (5) | 48 / 54 (4) | 54 / 76 (4) | 97 / 101 (4) | 42 / 100 (4) | 53 / 76 (8) |
| docs | 35 / 36 (5) | 30 / 37 (5) | 38 / 58 (4) | 46 / 56 (4) | 40 / 54 (4) | 37 / 69 (4) | 38 / 58 (8) |
| hn | 37 / 38 (5) | 36 / 38 (5) | 40 / 55 (4) | 47 / 55 (4) | 49 / 54 (4) | 53 / 58 (4) | 40 / 55 (8) |

Back/forward cache not used:

- krunk-b: Circumstantial:HTTPStatusNotOK ×1, Circumstantial:ErrorDocument ×1
- krunk-0255-baseline: Circumstantial:HTTPStatusNotOK ×1, Circumstantial:ErrorDocument ×1

## forward

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk-new | 11 / 13 (8p) | 26 / 29 (8p) | 335 / 423 (1p) | 726 / 935 (1p) | 1864 / 2385 (1p) | bfcache 40, load 5 |
| krunk-old | 11 / 14 (8p) | 27 / 29 (8p) | 322 / 342 (1p) | 3922 / 4246 (1p) | 1897 / 4242 (1p) | bfcache 40, load 5 |
| krunk-a | 15 / 19 (8p) | 27 / 36 (8p) | 383 / 585 (1p) | 4507 / 5289 (1p) | 2216 / 3401 (1p) | bfcache 36, load 5 |
| krunk-b | 17 / 18 (8p) | 32 / 34 (8p) | 527 / 586 (1p) | 4897 / 5470 (1p) | 3075 / 3353 (1p) | bfcache 35, load 4, timeout 1 |
| chrome | 19 / 22 (8p) | 38 / 40 (8p) | 438 / 500 (1p) | 2019 / 2736 (1p) | 2236 / 2952 (1p) | bfcache 36, load 4 |
| edge | 29 / 32 (8p) | 44 / 50 (8p) | 525 / 550 (1p) | 2538 / 2721 (1p) | 3178 / 4376 (1p) | bfcache 36, load 4 |
| krunk-0255-baseline | 16 / 18 (8p) | 28 / 36 (8p) | 487 / 586 (1p) | 4775 / 5470 (1p) | 2858 / 3401 (1p) | bfcache 71, load 9, timeout 1 |

Per page, frame (median / p90, n):

| page | krunk-new | krunk-old | krunk-a | krunk-b | chrome | edge | krunk-0255-baseline |
|---|---|---|---|---|---|---|---|
| news | 30 / 32 (5) | 30 / 35 (5) | 41 / 53 (5) | 42 / 47 (4) | 38 / 51 (5) | 54 / 61 (5) | 41 / 53 (9) |
| spa | 32 / 34 (5) | 36 / 40 (5) | 25 / 26 (5) | 25 / 26 (5) | 24 / 42 (5) | 38 / 39 (5) | 25 / 26 (10) |
| search | 26 / 28 (5) | 25 / 29 (5) | 23 / 39 (5) | 23 / 26 (5) | 37 / 39 (5) | 46 / 52 (5) | 23 / 26 (10) |
| shop | 27 / 29 (5) | 33 / 35 (5) | 48 / 56 (5) | 39 / 60 (5) | 40 / 45 (5) | 52 / 54 (5) | 45 / 56 (10) |
| youtube | 726 / 935 (5) | 3922 / 4246 (5) | 4507 / 5289 (5) | 4897 / 5470 (4) | 2019 / 2736 (4) | 2538 / 2721 (4) | 4775 / 5470 (9) |
| wiki | 26 / 27 (5) | 26 / 26 (5) | 31 / 38 (4) | 39 / 40 (4) | 39 / 40 (4) | 37 / 38 (4) | 38 / 40 (8) |
| ads | 20 / 38 (5) | 25 / 26 (5) | 27 / 34 (4) | 38 / 54 (4) | 39 / 41 (4) | 46 / 96 (4) | 32 / 54 (8) |
| docs | 27 / 28 (5) | 27 / 27 (5) | 20 / 31 (4) | 22 / 26 (4) | 22 / 23 (4) | 42 / 48 (4) | 21 / 31 (8) |
| hn | 26 / 28 (5) | 27 / 28 (5) | 26 / 30 (4) | 19 / 28 (4) | 22 / 26 (4) | 36 / 37 (4) | 24 / 30 (8) |

Back/forward cache not used:

- krunk-new: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-old: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-a: Circumstantial:ServiceWorkerUnregistration ×5, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×5
- krunk-b: Circumstantial:ServiceWorkerUnregistration ×4, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×4
- chrome: Circumstantial:ServiceWorkerUnregistration ×4, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×4
- edge: Circumstantial:ServiceWorkerUnregistration ×4, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×4
- krunk-0255-baseline: Circumstantial:ServiceWorkerUnregistration ×9, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×9

## Noise band (krunk-a vs krunk-b, same build)

Band = the larger of |corpus median diff %| and the median per-page |diff %|; a fix must beat it.

| scenario | metric | corpus median diff | per-page median |diff| (median, max) | band |
|---|---|---|---|---|
| cold | fcp | -94 ms (-14.1 %) | 10.2 %, 30.5 % | 14.1 % |
| cold | lcp | -104 ms (-12.8 %) | 12.4 %, 33.4 % | 12.8 % |
| cold | load | +428 ms (+47.0 %) | 12.9 %, 24.5 % | 47.0 % |
| cold | ttfb | +0 ms (+0.3 %) | 1.1 %, 15.1 % | 1.1 % |
| cold | nav | +1 ms (+2.9 %) | 5.4 %, 16.3 % | 5.4 % |
| warm | fcp | -64 ms (-12.3 %) | 11.1 %, 56.7 % | 12.3 % |
| warm | lcp | +66 ms (+10.5 %) | 11.1 %, 52.7 % | 11.1 % |
| warm | load | -81 ms (-8.2 %) | 7.2 %, 51.8 % | 8.2 % |
| warm | ttfb | -32 ms (-21.8 %) | 21.8 %, 90.2 % | 21.8 % |
| warm | nav | -0 ms (-1.0 %) | 14.0 %, 98.1 % | 14.0 % |
| link | fcp | +55 ms (+9.1 %) | 4.0 %, 15.6 % | 9.1 % |
| link | lcp | -115 ms (-14.3 %) | 9.5 %, 21.0 % | 14.3 % |
| link | load | -37 ms (-3.9 %) | 4.4 %, 31.1 % | 4.4 % |
| link | ttfb | +1 ms (+0.8 %) | 2.4 %, 17.8 % | 2.4 % |
| link | nav | -1 ms (-4.2 %) | 9.2 %, 39.3 % | 9.2 % |
| back | frame | +6 ms (+15.9 %) | 10.9 %, 34.5 % | 15.9 % |
| forward | frame | +5 ms (+18.2 %) | 12.8 %, 29.1 % | 18.2 % |
| forward | lcp | +390 ms (+8.7 %) | 8.0 %, 8.0 % | 8.7 % |
