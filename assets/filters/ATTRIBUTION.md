# Bundled filter lists

These lists are embedded (deflate-compressed) into LightBrowser and refreshed
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

EasyList licence statement: https://easylist.to/pages/licence.html

The lists are used unmodified, as data. uBlock Origin scriptlet rules (`+js(...)`)
are parsed but not executed, because LightBrowser does not ship uBO's scriptlet
resources; network and CSS (cosmetic) rules work.
