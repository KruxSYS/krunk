# Known limitations

What is not done, not verified or different from Chrome, release by release. Part of the [Krunk README](../README.md); the other pages are listed there under *Documentation*.

- **Casting (0.30.0):**
  - **Never cast to a real device in testing.** No Cast or DIAL device was reachable from the
    test machines. The Windows VM showed the picker (menu, page menu, a site's request), the
    on/off switch, the page APIs and the absence of LAN discovery before the first use. Mirroring,
    video remoting, a site's Cast app, the session controls and the toolbar button during a
    session were not tried with a device; Chromecast, Google TV, Cast speakers and DIAL TVs are
    all untested.
  - **The first use triggers Windows' firewall prompt**, which covers the picker and closes it:
    allow Krunk on private networks and choose Cast… again. If the prompt is refused, no devices
    are found (Settings → Windows Security → Firewall → Allow an app). The per-user installer
    can't add Chrome's firewall rule.
  - **Once started, discovery runs until Krunk exits** (Chromium's behaviour). It starts only
    with the picker or a site's cast button.
  - **Copy-protected (DRM / Widevine) video can't be cast on its own**; "Video on this page"
    mirrors the tab instead, and the protected picture may stay black on the device. Remoting
    also needs a device and codecs that Chromium's capability check accepts; otherwise the tab is
    mirrored.
  - **Video on this page** needs a playing video with a media session: a muted autoplaying clip
    or a page with several players is mirrored as the whole tab.
  - **The `<video>` cast button** appears only while a compatible device is available, as in Chrome.
  - **Not supported:** AirPlay, Miracast, DLNA/UPnP renderers, and Windows' own "Cast to device" /
    Project (Win+K). Desktop (whole-screen) mirroring is not offered.
  - **Android (0.15.0):**
    - Cast sends the video's address: the device downloads it itself. `blob:` or MediaSource
      (streaming) videos, which most big video sites use, can't be sent; use Cast screen.
      Addresses that need the page's cookies, sign-in or headers fail on the device too.
    - DRM video can't be cast.
    - Cast needs Google Play services (not in `-Pkrunk.noGms=true` builds).
    - There is no tab mirroring of Krunk's own: Cast screen is Android's system mirroring.
    - Never tried with a real Cast device.
  - **The Casting switch needs a restart.** Chromium can't turn its router on or off while
    running; the Cast items hide at once.
- **Page loading (0.26.0):** on the test corpus, 0.26.0 opens typed addresses sooner (median first
  byte 64 ms). For link-opened pages the time to the largest paint is unchanged from 0.25.5
  (559 → 562 ms, same session). Krunk's engine still runs page JavaScript and layout slower than
  Chrome on the same machine (the 0.7.0 finding).
- **No V8 context snapshot:** the cross-built engine creates each page context from scratch. That
  costs about 7.7 ms more per page load than Chrome (19.5 vs 4.2 ms over two loads), 2 % of the
  largest paint. Below the bar for a rebuild in 0.26.0.
- **Prerendering:** Chrome's Prerender2 is off in CEF, so pages a site asks to prerender are not
  made ahead. Speculation-rules prefetch works.
- **Preload pages (0.26.0):** early connections to hosts the ad blocker blocks are skipped only
  for plain host rules (`||host^`). A host blocked by a narrower rule (path, options) can still get
  a connection, but no request is sent. The per-site allowlist doesn't apply to early connections.
  A host is also left out when any `@@` exception in the lists touches it or a parent. With the
  default lists that removes most big ad domains (of 13 well-known ad and tracker domains checked
  on 2026-11-21, only pixel.quantserve.com is in the set). Their requests are still blocked as
  before; only the early connection can happen.
- **Page-load comparison runs:** Chrome and Edge must be pinned (auto-update off) on the test
  machine. In 0.26.0 they updated between sessions (Chrome 157 → 158, Edge 156 → 157), and the
  harness could no longer drive input into them. The Chrome/Edge figures come from the earlier
  session.
- **Theme colour of open tabs (0.15.0):** after switching between light and dark, tabs that were
  already open keep the previous background colour for the moment before one of their pages
  paints (a reload, a slow page). Krunk's tabs are windowed engine views whose pre-paint colour is
  fixed when they are created; new tabs use the new colour. Krunk's own pages always paint the
  theme colour from their first frame.
- **Glass style (0.15.0)** is drawn by Krunk (translucent layers and blur inside its bars); it
  does not show the desktop behind the window.
- **The AI agent (0.12.0) was tested against a mock provider only.** No live provider (OpenRouter,
  OpenAI, Anthropic, Ollama or LM Studio) and no vision model was exercised before the release.
  The OpenAI-style and Anthropic request and stream formats are covered by unit tests. Also:
  - Models without tool calling can't run the agent (the panel says so).
  - A model without image input gets a notice instead of screenshots.
  - The sensitive-data keyword gate understands English words only.
  - Cross-origin iframes (such as a payment provider's card form) are not read. Beyond masking
    the ones that look like payment forms in screenshots, nothing is redacted there.
  - The agent doesn't act on Krunk's own pages.
  - `fill_password` (0.16.0) reaches the password fields of the page and of its same-origin
    frames, not a login form inside another site's frame.
  - A password Krunk generated is cut out of what the agent reads as text. A screenshot is a
    picture: password fields are masked in it, but a page that prints the password as plain
    text would show it there.
  - A password you type into a message for the agent goes to your AI provider with that
    message.
- **Not tested on Windows in 0.12.0:**
  - the agent tools press_keys, forward, toggle_userscript, list_userscripts, list_macros,
    get_downloads and get_cookies_for_site;
  - GM_setClipboard, GM_openInTab, @noframes and frames, @exclude, userscripts in private tabs,
    export, and "Run on current tab now";
  - the macro calls for the mouse, typing, the page, tabs and navigation;
  - two windows with their own agents;
  - HiDPI.

  All of them have unit or UI tests; the test evidence lists every flow.
- **Userscripts in 0.12.0** don't support `GM_xmlhttpRequest`, `@require`, `@resource` or
  regular-expression `@include` (the editor warns). Only one macro runs at a time: starting
  another stops the first.
- **Sync is a beta (0.9.0).** Not yet: choosing your own passphrase, changing the passphrase
  or making a new recovery key, a decrypted export of the synced data from Krunk (the website's
  data download, sync reset and account deletion came in 0.13.0), payment
  cards, a way to see the kept previous versions of passwords, and trimming synced history beyond
  the first sync's 90 days / 20,000 pages (the server's quota stops pushes with a message instead).
  Tested on Windows with the Linux test client as the other devices; there is no other client yet.
- **The engine is bundled.** Chromium security fixes reach users only with a new Krunk
  release with a newer CEF. Krunk checks for one daily and offers it, but installs only when you
  choose Update now. Releases should follow CEF security releases promptly.
- **The update server is fixed at build time** (`https://krunk.kruxsys.com`, KruxSYS's
  permanent domain for Krunk). Another server needs a build that points at it
  (`UPDATE_BASE_URL=https://<domain> scripts/build-dist.sh`). Copies older than 0.28.1 (the
  former krunk.209.wf server) and 0.2.0 copies (placeholder server) must be reinstalled by hand.
- **No certificate revocation or CT list updates by default.** With "Download engine security
  components" off, the CRLSet and Certificate Transparency list are those built into the
  release. Chromium relaxes CT enforcement about 10 weeks after its build date. Turning the
  setting on restores the updates, at the cost of contact with Google's update servers.
- **No Safe Browsing / SmartScreen.** CEF has no working Safe Browsing, and the WebView2
  build's optional SmartScreen setting is gone. Ad and tracker blocking and third-party cookie
  blocking replace Edge's tracking prevention.
- **DRM without VMP.** With "Protected content" on, Widevine (L3, software) plays streams
  whose licence server accepts an unverified player (test streams, many smaller services).
  Krunk's binaries carry no Widevine VMP signature, so Netflix, Amazon, Disney+ and Spotify's
  web player, which insist on a verified player, refuse playback. A release can be signed
  with castLabs EVS by the build host (`scripts/vmp-sign.sh`, run by `build-dist.sh` when
  `~/.config/krunk/evs` holds the account); without credentials the build logs "VMP signing
  skipped (no EVS credentials)" and ships unsigned. Hardware-secure (L1) playback is not
  available either way.
- **No page freezing in the background.** `Page.setWebLifecycleState frozen` is not used: in
  CEF 156 a windowed (Alloy-style child window) browser never repaints after it is unfrozen.
  Background tabs rely on Chromium's hidden-page throttling plus garbage collection; discard
  frees the memory.
- **Third-party cookie blocking for scripts uses a Chromium test switch**
  (`--test-third-party-cookie-phaseout`). A future Chromium could rename or drop it. There are no
  per-site exceptions; if an embedded sign-in or widget breaks, turn the setting off (and
  restart). Network cookies are filtered by the browser itself and apply at once.
- **Some settings need a restart:** hardware acceleration, third-party cookie blocking (script
  access), engine security components, proxy and extra flags. The settings page says when.
- **Zoom is per site**, as in Chrome, not per tab as in the WebView2 build.
- **WebSocket connections are not filtered** by the ad blocker: CEF does not pass them to the
  request handler. Theme changes reach pages through DevTools emulation, so a page that has
  DevTools open shows the system theme.
- **HTTP authentication** (0.8.0) works for sites, not for proxies; a background tab's request
  is refused. Site permissions are asked since 0.6.0 (see "Site permissions"), but there is no device
  picker and no window chooser for screen sharing.
- **DNSCrypt is not implemented.** A DNSCrypt `sdns://` stamp in the custom DNS list is
  parsed and kept, shown as "not supported", and skipped: the next resolver in the list
  answers. DoH (HTTP/2 and HTTP/3), DoT, DoQ and plain DNS work.
- **Media codecs.** Since 0.5.2 the engine is Krunk's own build of CEF 156.0.15 with
  proprietary codecs (`scripts/cef-build/`): H.264 (AVC) video and AAC audio play in MP4, HLS
  and DASH streams, next to VP8, VP9, AV1, Opus, Vorbis, FLAC and MP3. Still missing or
  conditional:
  - **HEVC (H.265)** is decoded by the graphics hardware only (Chromium's platform HEVC on
    Windows, built in): `canPlayType` says `probably` on a PC with a hardware HEVC decoder and
    `""` without one (software rendering, the test VM). There is no software HEVC decoder.
  - **AC-3 / E-AC-3 (Dolby Digital)** is compiled in but needs Windows' own Dolby decoder,
    which Windows 11 24H2 and later no longer ship; it reports unsupported there.
  - **Dolby Vision** profiles are mapped to HEVC and follow the same rule. **MPEG-H** and
    **DTS** audio are not built.
  - **Widevine** (DRM) is not bundled: the engine downloads Google's module when "Protected
    content" is on (see above).
  Patent licensing for the H.264/HEVC/AAC decoders (MPEG LA / Via LA pools) is the
  distributor's responsibility; see the 0.5.2 release notes.
- **Size:** about 423 MB unpacked (184 MB zipped), against about 6 MB for the WebView2 build. All
  CEF locales and SwiftShader (software rendering for machines without a usable GPU) are kept.
- **Discarding loses back/forward history.** A discarded tab reloads its last URL; its history
  and any unsaved form input are lost. Discard is off by default.
- **Cosmetic filtering of generic rules** is limited to the first 2000 class names and ids a
  page reports.
- **uBO's procedural cosmetic filters** (`:has-text()`, `:upward()`, `:remove()` and similar) are not
  executed: only plain CSS selectors hide elements. Scriptlets written for uBO's isolated world
  run in the page's world, because CEF provides no isolated world for them.
- **YouTube** keeps changing its ad delivery. Krunk carries uBO's YouTube defences (scriptlets
  and Quick Fixes, updated daily) and was verified on 2026-11-04 (no ads, no anti-adblock wall
  on three videos). A YouTube change can still get through until the lists catch up.
- **Pointer lock is provided by Krunk, not the engine.** CEF's Alloy runtime style never
  implements `WebContentsDelegate::RequestPointerLock`; Chromium's default refuses every request
  (`kUnknownError`). So a script in every frame implements the API, and the browser confines and
  hides the cursor and reads raw mouse input (`WM_INPUT`). The page keeps getting the real
  (trusted) mouse events. Their `movementX/Y` and positions come from patched `MouseEvent`
  getters, and while locked a style sheet sends them to the locked element, as Chrome does.
  - Code that inspects those getters can tell the difference.
  - A real mouse event carries the engine's own movement of the hidden cursor (0.7.2: it belongs
    to the frame the event is dispatched in, so a camera turns evenly); only where the browser
    moved the cursor back to the centre does it carry the browser's raw-input movement, which
    reaches the page about 2–4 ms later than in Chrome.
  - Movement that no real event carries (sub-pixel motion) arrives as a synthetic event
    (`isTrusted` false).
  - The long-term fix is CEF's Chrome runtime style for tabs, which has native pointer lock.
- **Resizing on the new-tab page (0.7.2).** While a window is resized, the parts the engine
  has not drawn yet show the theme's colour (0.7.1 showed white: the window's own surface
  beneath the engine's views). On the test VM two single frames of white remained on the
  dark new-tab page, at the edge being dragged (a 12–16 px strip); pages in the theme's colours
  showed none. Their source is inside the engine's views and was not found.
- **The bookmarks bar's "»" takes drops only at the end.** Dropping a bookmark on "»" moves it
  to the end of the bar; to put it at a place inside the overflow, drop it in the "»" dropdown
  or drag it there within the dropdown.
- **The vault does not detect rollback.** Replacing `vault.json` with an older copy is not
  detected. See `docs/SECURITY.md` for the full threat model and what is out of scope.
- **No tab pinning in the UI.** The memory policy supports pinned tabs, but the tab strip
  cannot pin yet.
- **Platform support:** no Windows on ARM build. Extensions run in Krunk's own Manifest V3
  subset (see Usage): a Web Store extension that needs an API Krunk does not provide works
  only in part, and `krunk://extensions` says which calls those are.
- **Tab dragging is Krunk's own, not Chrome's.** Under the Alloy runtime the engine has no tab
  strip, so the drag is done by the host: the torn-off window itself is the drag image (there
  is no translucent preview), and a tab can't be dropped into another program.
- **Downloads in parts go around the engine's network stack.** They use WinHTTP with the
  engine's cookies, user agent and referrer, the HTTP proxy setting, and WinHTTP's certificate
  checks. Sites that need HTTP authentication or a client certificate, SOCKS proxies and private
  tabs always get the engine's own single-connection download. A part download paused and
  resumed continues where each part stopped, also after Krunk was closed (0.7.0).
- **Filters with a port** (`||example.com:8080/…`) never match: Brave's `adblock` engine ignores
  the port. Write `||example.com^*/path` instead.
- **What was blocked** lists requests, not elements hidden by cosmetic filters. When the
  filter text can't be recovered (a list changed since the request), only the list is named.
- **Several monitors:** windows remember their position on any monitor and are moved back on
  screen if that monitor is gone. Dragging a window between monitors with different scaling
  was not tested: the test machine has one monitor. It is the same code as a scale change
  while Krunk runs, which is tested since 0.15.4 at 100 to 250 % (`docs/DPI.md`).
- **Not tested for 0.3.0 and 0.4.0** (the Windows 11 test VM can't do them):
  - **two monitors:** the VM has one;
  - **display scaling above 125 %:** the VM allows at most 125 %;
  - **a standard (non-admin) user with UAC on:** the VM has UAC off and an admin user.

  The in-app update from 0.2.1 to 0.3.0 was tested against the live server after publishing
  (2026-11-05): 0.2.1 offered 0.3.0, Update now installed it, and Krunk restarted as 0.3.0 with
  the session in one window.
  The in-app update from 0.3.0 to 0.4.0 was tested the same way (2026-11-06): 0.3.0 offered 0.4.0
  a minute after starting, and Update now brought Krunk back as 0.4.0 in 48 s with the session
  restored. If the installer is still exiting when Krunk restarts, its download stays in
  `%TEMP%\Krunk-update` until the next start, which removes it.
- **DevTools can't be undocked** into its own window; it docks right or at the bottom of its
  tab's window. Theme changes don't reach a page while its DevTools is open (see above).
- **Background tabs** (fixed in 0.10.0). Up to 0.9.0 CEF's windowed (Alloy) browsers were never
  told they were hidden (`WasHidden` did nothing for them), so every background tab kept
  rendering at about 60 frames a second at foreground priority: 30 busy tabs used 90 % of a
  4-core machine and Speedometer fell to 5 % of its score. Since 0.10.0 Krunk's engine carries
  one patch (`scripts/cef-build/patches/krunk-windowed-hidden.patch`, recorded in `cef.lock`):
  a background tab's content window is hidden from the engine, which throttles it like Chrome
  does (no animation frames, timers once a second; pages playing sound, calls and downloads go
  on). Not tested on a second monitor.
- **Speedometer is about 14 % below Chrome 157** on the same machine (Krunk 0.7.0: 27.2 vs
  31.5 on the 4-core test VM; Edge 156 32.9). Krunk's own page scripts were measured and
  trimmed (0.7.0); the remaining gap is engine-wide (plain JavaScript and layout run 10–30 %
  slower in CEF's Alloy runtime than in Chrome or Edge, with Krunk's scripts off and with the
  stock CEF build alike). Closing it needs an engine change.
- **Client hints off: what still sees `navigator.userAgentData`.** With "Send client hints"
  off (0.7.1), the modules a *module* shared worker imports statically run before the worker's
  own code and still see it (CEF fails a worker's own requests once Krunk handles them, so
  they cannot be changed); `blob:` and `data:` shared workers are not loaded over the network
  and keep it too. Their requests carry no hints. A service worker that was already running
  when the setting is switched back on keeps it removed until the browser restarts it.
- **The store's "Switch to Chrome" notices stay.** Krunk relabels the store's install button
  and leaves the rest of Google's page as it is.
- **Unsigned:** the binaries are not code-signed (SmartScreen may warn on first start).
