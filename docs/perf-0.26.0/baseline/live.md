# Page-load pass: /work/projects/extra/browser/.pipeline/evidence-0.26.0/runs/baseline-live

## Headline: corpus median (p90) over cold + warm typed loads

Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.

| browser | ttfb | fcp | lcp | dcl | load | nav | fcp_ns | lcp_ns | load_ns | pages |
|---|---|---|---|---|---|---|---|---|---|---|
| krunk | 201 (205) | 771 (807) | 1172 (1448) | 1170 (1423) | 1859 (2153) | 10 (15) | 492 (504) | 492 (504) | 1047 (1126) | 3 |
| chrome | – | – | – | – | – | – | – | – | – | 0 |
| edge | – | – | – | – | – | – | – | – | – | 0 |

Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).

## cold

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk | – | – | – | – | – | – | timeout 9 |
| chrome | – | – | – | – | – | – | timeout 9 |
| edge | – | – | – | – | – | – | timeout 9 |

Per page, lcp (median / p90, n):

| page | krunk | chrome | edge |
|---|---|---|---|
| live-wiki | – | – | – |
| live-news | – | – | – |
| live-youtube | – | – | – |

## warm

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk | 10 / 15 | 201 / 205 | 771 / 807 | 1172 / 1448 | 1170 / 1423 | 1859 / 2153 | load 9 |
| chrome | – | – | – | – | – | – | timeout 9 |
| edge | – | – | – | – | – | – | timeout 9 |

Per page, lcp (median / p90, n):

| page | krunk | chrome | edge |
|---|---|---|---|
| live-wiki | 507 / 514 (3) | – | – |
| live-news | 1172 / 1448 (3) | – | – |
| live-youtube | 3827 / 3844 (3) | – | – |

## link

| browser | nav | ttfb | fcp | lcp | dcl | load | results |
|---|---|---|---|---|---|---|---|
| krunk | 9 / 10 | 186 / 230 | 866 / 873 | 1212 / 1258 | 1208 / 1236 | 1879 / 1880 | load 9 |
| chrome | – | – | – | – | – | – | timeout 9 |
| edge | – | – | – | – | – | – | timeout 9 |

Per page, lcp (median / p90, n):

| page | krunk | chrome | edge |
|---|---|---|---|
| live-wiki | 482 / 486 (3) | – | – |
| live-news | 1212 / 1258 (3) | – | – |
| live-youtube | 3942 / 4053 (3) | – | – |

## back

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk | 16 / 19 | 34 / 36 | – | – | – | bfcache 9 |
| chrome | – | – | – | – | – | timeout 9 |
| edge | – | – | – | – | – | timeout 9 |

Per page, frame (median / p90, n):

| page | krunk | chrome | edge |
|---|---|---|---|
| live-wiki | 28 / 33 (3) | – | – |
| live-news | 34 / 36 (3) | – | – |
| live-youtube | 64 / 69 (3) | – | – |

## forward

| browser | show | frame | fcp | lcp | load | results |
|---|---|---|---|---|---|---|
| krunk | 15 / 16 (1p) | 29 / 32 (1p) | 833 / 921 (1p) | 3409 / 3673 (1p) | 2300 / 2461 (1p) | bfcache 3, load 3, timeout 3 |
| chrome | – | – | – | – | – | timeout 9 |
| edge | – | – | – | – | – | timeout 9 |

Per page, frame (median / p90, n):

| page | krunk | chrome | edge |
|---|---|---|---|
| live-wiki | 29 / 32 (3) | – | – |
| live-news | – | – | – |
| live-youtube | 3409 / 3673 (3) | – | – |

Back/forward cache not used:

- krunk: Circumstantial:ServiceWorkerUnregistration ×3, PageSupportNeeded:CacheControlNoStoreHTTPOnlyCookieModified ×3
