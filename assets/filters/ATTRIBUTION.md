# Bundled filter lists

These lists are embedded (deflate-compressed) into Krunk and refreshed
at runtime at most once a day. They are third-party data, not covered by the
project's MIT licence. Refresh the bundled copies with `scripts/update-filters.sh`;
the date of the last fetch is in `FETCHED`.

| File | Source | Authors | Licence |
|------|--------|---------|---------|
| `easylist.txt` | https://easylist.to/easylist/easylist.txt | The EasyList authors (https://easylist.to/) | Dual: GPL-3.0-or-later or CC BY-SA 3.0 (`LICENSE-GPL-3.0.txt`, `LICENSE-CC-BY-SA-3.0.txt`) |
| `easyprivacy.txt` | https://easylist.to/easylist/easyprivacy.txt | The EasyList authors | Dual: GPL-3.0-or-later or CC BY-SA 3.0 |
| `ublock-filters.txt` | https://github.com/uBlockOrigin/uAssets (`filters/filters.txt`) | Raymond Hill and uAssets contributors | GPL-3.0 (`LICENSE-GPL-3.0.txt`) |
| `ublock-privacy.txt` | https://github.com/uBlockOrigin/uAssets (`filters/privacy.txt`) | Raymond Hill and uAssets contributors | GPL-3.0 |
| `ublock-unbreak.txt` | https://github.com/uBlockOrigin/uAssets (`filters/unbreak.txt`) | Raymond Hill and uAssets contributors | GPL-3.0 |
| `ublock-quick-fixes.txt` | https://github.com/uBlockOrigin/uAssets (`filters/quick-fixes.txt`) | Raymond Hill and uAssets contributors | GPL-3.0 |
| `ublock-badware.txt` | https://github.com/uBlockOrigin/uAssets (`filters/badware.txt`) | Raymond Hill and uAssets contributors | GPL-3.0 |
| `nocoin.txt` | https://github.com/hoshsadiq/adblock-nocoin-list (`nocoin.txt`) | Hosh Sadiq and contributors | MIT (https://mit-license.org/) |
| `krunk-fingerprinting.txt` | Krunk's own (this repository) | The Krunk authors | MIT (the project licence) |

EasyList licence statement: https://easylist.to/pages/licence.html

The lists are used as data. `ublock-filters.txt` and `ublock-privacy.txt` include
sub-lists with `!#include` (`filters-general.txt`, `filters-2020.txt` …
`filters-2026.txt`, `ubo-link-shorteners.txt`, `filters-mobile.txt`,
`resource-abuse.txt`); their text is inlined in place, between `! >>> name` and
`! <<< name` comment lines, otherwise unchanged.

## uBlock Origin resources

`ubo-resources.json` holds uBlock Origin's scriptlets (`+js(...)` filters) and
redirect resources (`$redirect` filters), converted to the `adblock` crate's
resource format by `scripts/build-ubo-resources.mjs` (function sources and file
contents unchanged, base64-encoded).

| | |
|---|---|
| Source | https://github.com/gorhill/uBlock, release **1.75.4** (`src/js/resources/`, `src/js/redirect-resources.js`, `src/web_accessible_resources/`) |
| Archive | `https://codeload.github.com/gorhill/uBlock/tar.gz/refs/tags/1.75.4`, SHA-256 `ca3a6fd43aec99c002e3bb1cb1d97b34eea4228ef35f8c076b55644caa397870` (pinned in `ubo-resources.lock`) |
| Authors | Raymond Hill and uBlock Origin contributors |
| Licence | GPL-3.0 (`LICENSE-GPL-3.0.txt`) |

Some redirect resources carry their own notices (for example `fingerprint2.js`,
MIT); they are kept in the files as distributed by uBlock Origin.
