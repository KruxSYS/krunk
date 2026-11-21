# Page-load pass: /work/projects/extra/browser/.pipeline/evidence-0.26.0/p1/ab-b

## Headline: corpus median (p90) over cold + warm typed loads

Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.

| browser | ttfb | fcp | lcp | dcl | load | nav | fcp_ns | lcp_ns | load_ns | pages |
|---|---|---|---|---|---|---|---|---|---|---|
| krunk-base | 103 (154) | 407 (510) | 480 (598) | 514 (621) | 587 (793) | 10 (14) | 294 (420) | 406 (492) | 534 (617) | 9 |
| krunk-filter | 105 (155) | 394 (486) | 475 (586) | 520 (625) | 610 (756) | 10 (13) | 304 (420) | 386 (476) | 556 (617) | 9 |
| krunk-scripts | 105 (160) | 385 (519) | 484 (594) | 532 (620) | 610 (717) | 10 (14) | 314 (412) | 378 (508) | 556 (648) | 9 |
| krunk-writes | 105 (156) | 400 (503) | 488 (603) | 521 (634) | 608 (772) | 10 (14) | 300 (432) | 394 (496) | 563 (620) | 9 |
| krunk-cosmetic | 106 (156) | 408 (511) | 494 (582) | 531 (618) | 614 (647) | 10 (13) | 336 (412) | 400 (500) | 545 (623) | 9 |
| krunk-nokeys | 105 (157) | 402 (494) | 501 (585) | 531 (641) | 615 (775) | 10 (13) | 322 (436) | 392 (484) | 541 (614) | 9 |

Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).

## cold

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-base | 12 / 16 | 153 / 165 | 498 / 516 | 572 / 636 | 600 / 662 | 758 / 846 | load 45 |
| krunk-filter | 11 / 18 | 153 / 163 | 477 / 507 | 567 / 595 | 619 / 648 | 756 / 791 | load 45 |
| krunk-scripts | 12 / 18 | 153 / 165 | 506 / 528 | 594 / 671 | 617 / 693 | 645 / 736 | load 45 |
| krunk-writes | 12 / 16 | 154 / 166 | 490 / 528 | 590 / 614 | 620 / 637 | 740 / 792 | load 45 |
| krunk-cosmetic | 11 / 21 | 152 / 165 | 503 / 535 | 573 / 630 | 603 / 669 | 629 / 697 | load 45 |
| krunk-nokeys | 12 / 18 | 153 / 162 | 487 / 504 | 579 / 605 | 604 / 654 | 668 / 838 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-base | krunk-filter | krunk-scripts | krunk-writes | krunk-cosmetic | krunk-nokeys |
|---|---|---|---|---|---|---|
| news | 911 / 941 (5) | 878 / 942 (5) | 877 / 941 (5) | 869 / 964 (5) | 890 / 923 (5) | 907 / 926 (5) |
| spa | 380 / 389 (5) | 385 / 400 (5) | 368 / 435 (5) | 375 / 386 (5) | 399 / 414 (5) | 401 / 416 (5) |
| search | 1546 / 2033 (5) | 1537 / 1679 (5) | 1543 / 1603 (5) | 1519 / 1545 (5) | 1620 / 1653 (5) | 1596 / 1619 (5) |
| shop | 887 / 991 (5) | 869 / 1015 (5) | 842 / 881 (5) | 898 / 961 (5) | 863 / 916 (5) | 881 / 909 (5) |
| youtube | 4383 / 4638 (5) | 4330 / 4511 (5) | 4334 / 4842 (5) | 4385 / 4432 (5) | 4420 / 4487 (5) | 4395 / 4728 (5) |
| wiki | 424 / 437 (5) | 416 / 449 (5) | 424 / 444 (5) | 436 / 463 (5) | 417 / 449 (5) | 439 / 454 (5) |
| ads | 572 / 636 (5) | 567 / 595 (5) | 594 / 671 (5) | 590 / 614 (5) | 573 / 620 (5) | 579 / 605 (5) |
| docs | 498 / 516 (5) | 477 / 507 (5) | 506 / 528 (5) | 490 / 571 (5) | 503 / 630 (5) | 487 / 504 (5) |
| hn | 279 / 286 (5) | 281 / 285 (5) | 292 / 332 (5) | 284 / 291 (5) | 294 / 312 (5) | 284 / 289 (5) |

## warm

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk-base | 8 / 11 | 59 / 62 | 248 / 264 | 368 / 380 | 385 / 415 | 460 / 472 | load 45 |
| krunk-filter | 9 / 11 | 59 / 61 | 244 / 270 | 355 / 419 | 378 / 417 | 448 / 485 | load 45 |
| krunk-scripts | 8 / 10 | 57 / 61 | 249 / 274 | 381 / 405 | 378 / 410 | 455 / 524 | load 45 |
| krunk-writes | 8 / 10 | 60 / 61 | 258 / 268 | 363 / 394 | 384 / 402 | 449 / 484 | load 45 |
| krunk-cosmetic | 9 / 10 | 58 / 63 | 249 / 274 | 361 / 385 | 393 / 412 | 460 / 499 | load 45 |
| krunk-nokeys | 9 / 11 | 59 / 61 | 243 / 267 | 366 / 383 | 370 / 410 | 446 / 508 | load 45 |

Per page, lcp (median / p90, n):

| page | krunk-base | krunk-filter | krunk-scripts | krunk-writes | krunk-cosmetic | krunk-nokeys |
|---|---|---|---|---|---|---|
| news | 692 / 705 (5) | 668 / 694 (5) | 669 / 729 (5) | 660 / 721 (5) | 657 / 709 (5) | 664 / 717 (5) |
| spa | 236 / 246 (5) | 244 / 268 (5) | 242 / 274 (5) | 242 / 252 (5) | 237 / 253 (5) | 238 / 250 (5) |
| search | 368 / 380 (5) | 355 / 423 (5) | 381 / 405 (5) | 363 / 394 (5) | 361 / 385 (5) | 366 / 383 (5) |
| shop | 669 / 682 (5) | 652 / 685 (5) | 652 / 676 (5) | 642 / 706 (5) | 682 / 706 (5) | 667 / 686 (5) |
| youtube | 3906 / 4336 (5) | 3948 / 4458 (5) | 4055 / 4220 (5) | 3952 / 4131 (5) | 4041 / 4128 (5) | 4016 / 4375 (5) |
| wiki | 277 / 307 (5) | 279 / 310 (5) | 310 / 338 (5) | 277 / 288 (5) | 271 / 301 (5) | 275 / 290 (5) |
| ads | 399 / 425 (5) | 397 / 419 (5) | 394 / 446 (5) | 414 / 441 (5) | 393 / 449 (5) | 408 / 439 (5) |
| docs | 322 / 366 (5) | 328 / 347 (5) | 316 / 324 (5) | 315 / 350 (5) | 317 / 338 (5) | 326 / 336 (5) |
| hn | 170 / 176 (5) | 168 / 169 (5) | 157 / 174 (5) | 164 / 170 (5) | 157 / 170 (5) | 165 / 170 (5) |

## A/B verdicts (corpus median (p90), ms from the input)

| A → B | scenario | metric | A | B | change | band | verdict |
|---|---|---|---|---|---|---|---|
| krunk-base → krunk-filter | cold | fcp | 498 (516) | 477 (507) | -4.2 % | 19.8 % | within the band |
| krunk-base → krunk-filter | cold | lcp | 572 (636) | 567 (595) | -1.0 % | 35.1 % | within the band |
| krunk-base → krunk-filter | cold | load | 758 (846) | 756 (791) | -0.3 % | 16.7 % | within the band |
| krunk-base → krunk-filter | cold | ttfb | 153 (165) | 153 (163) | +0.1 % | 1.2 % | within the band |
| krunk-base → krunk-filter | cold | nav | 12 (16) | 11 (18) | -6.7 % | 5.9 % | **beats the band** |
| krunk-base → krunk-filter | warm | fcp | 248 (264) | 244 (270) | -1.5 % | 7.4 % | within the band |
| krunk-base → krunk-filter | warm | lcp | 368 (380) | 355 (419) | -3.5 % | 18.4 % | within the band |
| krunk-base → krunk-filter | warm | load | 460 (472) | 448 (485) | -2.5 % | 26.9 % | within the band |
| krunk-base → krunk-filter | warm | ttfb | 59 (62) | 59 (61) | +0.5 % | 7.6 % | within the band |
| krunk-base → krunk-filter | warm | nav | 8 (11) | 9 (11) | +3.5 % | 20.2 % | within the band |
| krunk-base → krunk-scripts | cold | fcp | 498 (516) | 506 (528) | +1.7 % | 19.8 % | within the band |
| krunk-base → krunk-scripts | cold | lcp | 572 (636) | 594 (671) | +3.7 % | 35.1 % | within the band |
| krunk-base → krunk-scripts | cold | load | 758 (846) | 645 (736) | -15.0 % | 16.7 % | within the band |
| krunk-base → krunk-scripts | cold | ttfb | 153 (165) | 153 (165) | -0.1 % | 1.2 % | within the band |
| krunk-base → krunk-scripts | cold | nav | 12 (16) | 12 (18) | -2.5 % | 5.9 % | within the band |
| krunk-base → krunk-scripts | warm | fcp | 248 (264) | 249 (274) | +0.4 % | 7.4 % | within the band |
| krunk-base → krunk-scripts | warm | lcp | 368 (380) | 381 (405) | +3.5 % | 18.4 % | within the band |
| krunk-base → krunk-scripts | warm | load | 460 (472) | 455 (524) | -1.0 % | 26.9 % | within the band |
| krunk-base → krunk-scripts | warm | ttfb | 59 (62) | 57 (61) | -3.7 % | 7.6 % | within the band |
| krunk-base → krunk-scripts | warm | nav | 8 (11) | 8 (10) | -7.1 % | 20.2 % | within the band |
| krunk-base → krunk-writes | cold | fcp | 498 (516) | 490 (528) | -1.5 % | 19.8 % | within the band |
| krunk-base → krunk-writes | cold | lcp | 572 (636) | 590 (614) | +3.1 % | 35.1 % | within the band |
| krunk-base → krunk-writes | cold | load | 758 (846) | 740 (792) | -2.4 % | 16.7 % | within the band |
| krunk-base → krunk-writes | cold | ttfb | 153 (165) | 154 (166) | +0.3 % | 1.2 % | within the band |
| krunk-base → krunk-writes | cold | nav | 12 (16) | 12 (16) | +4.2 % | 5.9 % | within the band |
| krunk-base → krunk-writes | warm | fcp | 248 (264) | 258 (268) | +3.9 % | 7.4 % | within the band |
| krunk-base → krunk-writes | warm | lcp | 368 (380) | 363 (394) | -1.4 % | 18.4 % | within the band |
| krunk-base → krunk-writes | warm | load | 460 (472) | 449 (484) | -2.3 % | 26.9 % | within the band |
| krunk-base → krunk-writes | warm | ttfb | 59 (62) | 60 (61) | +1.5 % | 7.6 % | within the band |
| krunk-base → krunk-writes | warm | nav | 8 (11) | 8 (10) | -3.5 % | 20.2 % | within the band |
| krunk-base → krunk-cosmetic | cold | fcp | 498 (516) | 503 (535) | +1.1 % | 19.8 % | within the band |
| krunk-base → krunk-cosmetic | cold | lcp | 572 (636) | 573 (630) | +0.0 % | 35.1 % | within the band |
| krunk-base → krunk-cosmetic | cold | load | 758 (846) | 629 (697) | -17.0 % | 16.7 % | **beats the band** |
| krunk-base → krunk-cosmetic | cold | ttfb | 153 (165) | 152 (165) | -0.7 % | 1.2 % | within the band |
| krunk-base → krunk-cosmetic | cold | nav | 12 (16) | 11 (21) | -7.5 % | 5.9 % | **beats the band** |
| krunk-base → krunk-cosmetic | warm | fcp | 248 (264) | 249 (274) | +0.5 % | 7.4 % | within the band |
| krunk-base → krunk-cosmetic | warm | lcp | 368 (380) | 361 (385) | -1.9 % | 18.4 % | within the band |
| krunk-base → krunk-cosmetic | warm | load | 460 (472) | 460 (499) | +0.1 % | 26.9 % | within the band |
| krunk-base → krunk-cosmetic | warm | ttfb | 59 (62) | 58 (63) | -1.7 % | 7.6 % | within the band |
| krunk-base → krunk-cosmetic | warm | nav | 8 (11) | 9 (10) | +2.4 % | 20.2 % | within the band |
| krunk-nokeys → krunk-base | cold | fcp | 487 (504) | 498 (516) | +2.2 % | 19.8 % | within the band |
| krunk-nokeys → krunk-base | cold | lcp | 579 (605) | 572 (636) | -1.1 % | 35.1 % | within the band |
| krunk-nokeys → krunk-base | cold | load | 668 (838) | 758 (846) | +13.5 % | 16.7 % | within the band |
| krunk-nokeys → krunk-base | cold | ttfb | 153 (162) | 153 (165) | +0.3 % | 1.2 % | within the band |
| krunk-nokeys → krunk-base | cold | nav | 12 (18) | 12 (16) | +1.7 % | 5.9 % | within the band |
| krunk-nokeys → krunk-base | warm | fcp | 243 (267) | 248 (264) | +1.8 % | 7.4 % | within the band |
| krunk-nokeys → krunk-base | warm | lcp | 366 (383) | 368 (380) | +0.6 % | 18.4 % | within the band |
| krunk-nokeys → krunk-base | warm | load | 446 (508) | 460 (472) | +2.9 % | 26.9 % | within the band |
| krunk-nokeys → krunk-base | warm | ttfb | 59 (61) | 59 (62) | -0.3 % | 7.6 % | within the band |
| krunk-nokeys → krunk-base | warm | nav | 9 (11) | 8 (11) | -5.6 % | 20.2 % | within the band |
