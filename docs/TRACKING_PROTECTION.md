# Tracking protection in Krunk

A short research note (Krunk 0.6.0) on what the main browsers do against tracking, what Krunk
does, and what the Standard / Strict / Custom levels in Settings → Privacy map to.

## What the others do

**Firefox – Enhanced Tracking Protection (ETP).** Three levels. *Standard* blocks social-media
trackers, cross-site tracking cookies (Total Cookie Protection: every site gets its own cookie
jar), cryptominers and known fingerprinters, and strips known tracking parameters from URLs
(Query Parameter Stripping); it also runs bounce-tracking protection (sites that are only ever
seen as a redirect hop and were never interacted with lose their storage after a while). *Strict*
adds all third-party tracking content (the Disconnect "tracking" list), suspected fingerprinters
and the stricter query stripping, at the cost of broken sites. *Custom* lets the user pick each
part. Per-site exceptions come from the shield in the address bar. Lists: Disconnect's entity and
tracking lists (CC BY-NC-SA 4.0 – not usable in a product without a licence).

**Brave – Shields.** On by default for every site: ad and tracker blocking from EasyList,
EasyPrivacy, uBlock Origin's filters and Brave's own lists, third-party cookie blocking, a
farbling-based fingerprint randomisation (canvas, audio, WebGL, fonts…), bounce-tracking
mitigation (de-bouncing known trackers' redirect URLs and clearing their storage), query
parameter stripping (a fixed list: `fbclid`, `gclid`, `mc_eid`, `utm_*` and more), and
referrer trimming to the origin for cross-site requests. Aggressive mode adds first-party ad
blocking and stricter fingerprint protection. Exceptions per site from the shield panel.

**Chrome.** No tracker blocking; third-party cookies are being phased out behind opt-in
"Tracking protection" and partitioned (CHIPS). Privacy Sandbox APIs replace cross-site
cookies for ads. Referrer policy defaults to `strict-origin-when-cross-origin`. There is no
query stripping and no fingerprinting list (only the "Fingerprinting protection" in Incognito
of recent versions, which blocks a short list of scripts). Site-level controls exist but there
is no "level" switch.

## What Krunk does (0.6.0)

Krunk's blocking engine is uBlock Origin's rules (the `adblock` crate) over EasyList,
EasyPrivacy, uBO's own lists, uBO's unbreak and quick-fixes lists and the badware list, plus
cosmetic filtering and uBO's scriptlets, with a per-site shield switch. Settings → Privacy →
Tracking protection adds a level on top (`core::protection`):

| Feature | Standard (default) | Strict | Custom |
|---|---|---|---|
| Ad and tracker lists | the lists above (the user's choice on krunk://adblock) | the same | the same |
| Cryptomining | the **NoCoin** list (MIT), added to the engine | the same | on/off |
| Fingerprinting | EasyPrivacy's rules only | plus **Krunk's fingerprinting list** (`assets/filters/krunk-fingerprinting.txt`: FingerprintJS and its CDNs, ThreatMetrix, iovation, Sardine, SEON, Castle, DataDome, Ravelin, Riskified, Forter, Sift, …) | on/off |
| Third-party cookies | blocked for **known trackers**: a third-party request to a host the privacy lists block (`\|\|host^` rules of EasyPrivacy and uBO privacy, ≈ 47 000 hosts, subdomains included) sends and stores no cookies | blocked for **all** third parties (the cookie filter at once; `document.cookie` through Chromium's `--test-third-party-cookie-phaseout` switch after a restart) | off / trackers / all |
| Tracking parameters | `utm_*`, `pk_*`, `piwik_*`, `mtm_*`, `hmb_*`, `fbclid`, `gclid`, `gclsrc`, `dclid`, `gbraid`, `wbraid`, `msclkid`, `yclid`, `twclid`, `ttclid`, `igshid`, `igsh`, `li_fat_id`, `mc_cid`, `mc_eid`, `mkt_tok`, `_hsenc`, `_hsmi`, `__hssc`, `__hstc`, `__hsfp`, `hsctatracking`, `oly_*`, `vero_*`, `_openstat`, `wickedid`, `rb_clickid`, `s_cid`, `_ga`, `_gl`, `ref_src`, `ref_url`, `srsltid` (and `si` on YouTube/Spotify links) stripped from top-level navigations (`core::urlclean`) | the same | on/off |
| Referrer | every cross-site request carries only the **origin** (`https://news.example/`), whatever the page's referrer policy says (`unsafe-url`, `no-referrer-when-downgrade`); same-site requests are untouched | **no Referer** on cross-site requests | page's choice (the engine default, `strict-origin-when-cross-origin`, already sends only the origin cross-origin) / origin / none |
| Bounce trackers | the IO thread notes top-level redirects; a site seen only as a redirect hop in the last 7 days and never visited directly (no history entry) loses its cookies and storage, checked **daily** (Settings shows the count; "Clean now") | the same, **hourly** | on/off |
| Exceptions | the shield popup's site switch turns the lists, the cookie policy and the referrer trimming off for that site (parameter stripping happens before the site is known) | the same | the same |

Everything applies live except the "all third-party cookies" script switch (restart).
Settings from 0.5 migrate: the default toggles become Standard; a changed toggle becomes a
Custom configuration with the same effect.

## Embedded content (0.28.0)

Code: `core/src/embeds.rs` (pure, unit-tested), hooked into `TabPolicy::check`
(`app/src/browser/blocking.rs`).

- **Video players get the origin on Strict.** Strict sends no `Referer` cross-site, and some
  players refuse to play without one (YouTube's "error 153"). A cross-site sub-frame
  document to a video embed host (`VIDEO_EMBED_HOSTS`: YouTube incl. youtube-nocookie,
  ytimg and googlevideo, Vimeo, Dailymotion, Twitch's player, Google Maps embeds, Spotify
  and SoundCloud players), and every request whose initiator is such a host, gets the
  origin referrer, which is what Chrome sends by default. Everything else on Strict still
  gets none; Standard and the engine default are unchanged. (Initiators are origins, so
  hosts listed with a path, like `google.com/maps/embed`, count only as frame addresses.)
- **Social media embeds are click-to-load** ("Click-to-load social media embeds", Settings
  key `protection.socialEmbeds`; on in Strict, off in Standard, the user's choice in
  Custom). A third-party sub-frame document to a social embed host (`SOCIAL_EMBED_HOSTS`:
  Facebook `/plugins/`, Twitter/X platform and syndication, Instagram `…/embed`, TikTok,
  LinkedIn `/embed`, Disqus, embed.reddit.com) is not sent: the frame shows a placeholder.
  Like Firefox's SmartBlock placeholders, a filter list blocking a third-party frame of
  either list also shows the placeholder instead of an empty frame, at any level.
  First-party frames are never replaced.
- **The placeholder** is a small self-contained page answered locally (no script, no
  external loads, light and dark): "Krunk blocked content from *host*", **Load** and
  **Always allow on this site** (not offered in private tabs). The buttons navigate the
  frame to its address plus `krunk-embed=<nonce>` (and `&krunk-embed-always=1`). The nonce
  is random, bound to that address, single-use and expires after 10 minutes; it is kept in
  the tab's IO-side state. A request with a valid marker is answered locally (the marker
  never reaches the site) by a page that sends the frame on to the address without the
  marker (`location.replace`, no referrer from the stand-in). A forged, stale or reused
  marker is an ordinary request (the placeholder again).
- **Allowed embeds.** Load allows the frame's site for the tab's page until the tab goes to
  another page; Always also stores (page site → embed site) in Settings `embedAllow`. An
  allowed frame and the requests it makes (initiator in the allowed site) are neither
  replaced nor list-blocked, and send the origin of the page they are on.
- **Settings → Privacy → Tracking protection** lists "Embedded content allowed on sites"
  (one row per site and embed, Remove takes effect at once through `embedAllowRemove`). The
  list stays on the device (it is not synced).

## Bot checks (0.28.0)

Code: `core/src/challenge.rs` (pure, unit-tested), used by `app/src/browser/blocking.rs` and
`bounce.rs`. Captchas and anti-bot services (`CHALLENGE_HOSTS`: Arkose/FunCaptcha incl.
octocaptcha.com, Cloudflare Turnstile, hCaptcha, reCAPTCHA (`google.com/recaptcha/`,
`gstatic.com/recaptcha/`, recaptcha.net), DataDome, HUMAN/PerimeterX, Radware, AWS WAF,
GeeTest) are exempt at **every** level: no filter list blocks a request to them or made by their
frames (EasyPrivacy's `arkoselabs.com/metrics/` rule included), their cookies are not blocked,
their referrer is the engine's, and bounce cleanup never clears them or the big sign-in providers
(`IDP_SITES`). Blocking them protects nothing and makes the site's check fail or loop. Arkose,
DataDome and Radware were taken off `krunk-fingerprinting.txt` for the same reason.

Partitioned (CHIPS) cookies: Krunk's cookie filter makes CEF load and save cookies itself; until
0.28.0 that path dropped partitioned cookies on renderer requests, so a check's "passed" cookie
(DataDome's, Cloudflare's in embedded challenges) never came back and the check looped. The engine
patch `krunk-chips-cookies.patch` gives those requests the frame's partition key, as Chrome's
network service does. Strict's "block all third-party cookies" still blocks partitioned
third-party cookies too (Chrome allows them); unchanged.

## Not done, and why

- **Total Cookie Protection / cookie partitioning.** Chromium can partition third-party
  cookies (CHIPS) only when the site opts in; the engine has no general partitioning switch
  an embedder can turn on. Krunk blocks instead of partitioning.
- **Fingerprint randomisation (farbling).** Brave patches Blink for this; it cannot be done
  from outside the engine without rebuilding Chromium. Krunk blocks fingerprinting *scripts*
  by list instead.
- **Disconnect's lists.** Their licence (CC BY-NC-SA) does not allow redistribution in a
  product. Krunk uses EasyPrivacy, uBO's lists, NoCoin (MIT) and its own fingerprinting list
  (MIT).
- **Query stripping inside pages.** Only top-level navigations are rewritten; links in a page
  keep their parameters until clicked (the same as Firefox), and `fetch()` URLs are left
  alone.
- **Bounce tracking.** Krunk's version is heuristic (redirect hops never visited directly);
  Firefox and Brave additionally use interaction signals Krunk does not track, and Krunk
  cannot see redirects the engine resolves inside a service worker.
- **Referrer trimming and the page's own policy.** A page's `referrerpolicy=no-referrer`
  still wins (Krunk only shortens what the engine would send); `unsafe-url` is overridden
  cross-site.
