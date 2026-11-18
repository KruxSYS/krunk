# Krunk changelog

What changed in each Krunk release, newest first. Krunk for Android has its own list:
`CHANGELOG-android.md`.

## 0.25.3 — 2026-11-18

- **No more "Save password?" for a login Krunk already has.** Krunk asked again after every
  start and every automatic lock, because it couldn't compare with the locked vault. It now
  remembers this session's saved logins in memory (never on disk). Right after a start it holds
  a sign-in on a site that already has saved logins and decides when you unlock: nothing if
  the login is unchanged, "Update password?" if it changed. The comparison also ignores case
  and spaces in the username and covers logins saved on another address of the same site.
- **Import payment cards from Firefox, Chrome and Edge.** Settings → Passwords & import has a
  new "payment cards" box next to passwords: card numbers, expiry dates and names go into your
  Krunk vault (security codes are never imported). Cards already in the vault are skipped, and
  the result says how many were imported, already there or could not be read, and why. Chrome
  and Edge cards that Chrome protects for itself can't be read by other programs; those are
  counted so you can add them by hand.

## 0.25.2 — 2026-11-18

- **Attach files to the AI assistant.** Pictures, PDFs and text files (code, Markdown, CSV, JSON,
  logs and similar) can go with a message: use the paperclip button, paste them into the message
  box, or drop them on the panel. Pictures need a model that can see images.
- **Scrolling works while a tooltip is showing.** When the pointer rested on a picture or link
  with a tooltip (its title or alt text), the page wouldn't scroll with the mouse wheel until
  the tooltip went away.
- **The password suggestions close when you click elsewhere.** On some sites the list under a
  login field stayed open after a click outside it, and covered the page.
- **The AI assistant can read your userscripts and macros.** It could list them and write new
  ones, but never see an existing script's code, so it couldn't explain, fix or change one. Two
  new tools, get_userscript and get_macro, give it a script's full source (in pages for long
  scripts); it then saves the changed script under the same id.

## 0.25.1 — 2026-11-18

- **New tabs no longer keep spinning.** A new tab could show the loading circle (and the stop
  button) for ever although its page was there: the new tab page made ahead had finished loading
  before it became a tab, and the tab never heard about it.
- **Stop always stops.** The stop button (and the Stop command) now ends the tab's loading at
  once, also when the engine had nothing left to load.
- **The address bar keeps the focus in a new tab.** About a second after Ctrl+T the caret left
  the address bar: the next new tab page being made in the background took the keyboard.

## 0.25.0 — 2026-11-18

- **Faster tab switching with many busy tabs.** With 40 busy tabs (heavy scripts, timers,
  animations, video), Ctrl+Tab now shows the next tab in 18 ms typically and 60 ms in the slowest
  5 % (was 130 / 162 ms), and no switch took half a second in any test run (the slowest switch
  went from 577 ms to 344 ms). The tab you point at, the tab you just left and the next tab for
  Ctrl+Tab are woken a moment ahead; a tab you haven't looked at for a while comes up as soon as
  its page has drawn, instead of after a fixed wait.
- **New tabs open at once.** Ctrl+T shows the new tab page in about 85 ms (was 280–350 ms, even
  with one tab): Krunk keeps one new tab page ready in the background (it uses no CPU while it
  waits, and follows your theme).
- **Closing a tab is quicker:** closing a new tab or a tab opened from a link returns to the
  previous tab in 20–50 ms (was 100–150 ms).
- **Less work while you switch:** the tab preview picture is no longer taken at the moment you
  switch away (it was competing with the new tab's first frame), plain switches send the tab strip
  only the change, and background tabs use a little less CPU (40 busy tabs: 5.8 → 4.9 %).
- **Known limits:** a click on a tab that has been in the background for a long time still waits
  for that page to draw again (about 100–200 ms; much longer for very large pages on a PC without
  graphics acceleration). A link that opens a new tab still shows the old page until the new page
  has drawn its first content, so it never flashes white or black; that wait is the page's own
  loading time.
- **Security:** the DNS library behind custom DNS was updated (hickory 0.26.3) for published
  security advisories.

## 0.24.2 — 2026-11-17

- **Automatic Picture-in-Picture with two videos.** With Automatic Picture-in-Picture on and
  videos playing in two tabs, the tabs no longer take the floating window from each other (which
  could switch you back to the other tab). The video already in the floating window keeps it.

## 0.24.1 — 2026-11-17

- **Alt+click opens a link in a private tab.** Hold Alt and click a link to open it in a new
  private tab (it used to download the link instead).

## 0.24.0 — 2026-11-17

- **Krunk signs you in to krunk.209.wf.** When Krunk is signed in to your Krunk account (with or
  without sync), opening krunk.209.wf signs the website in too: you land on your account page with
  no password. The website lists it as "Signed in by Krunk" in your activity and marks the browser
  in Signed-in browsers.
- **Signing out on the website sticks.** If you sign out on the website, Krunk leaves it signed
  out until you restart Krunk. Private windows never sign the website in. Sensitive actions on the
  website (such as deleting your account) still ask for your password.
- **Sign in waits for your confirmation mail.** If you create your account from the page that
  Sign in opens, Krunk now keeps waiting (up to an hour) while you confirm your email address, and
  signs in as soon as you click the link in the mail. The Account card in Settings shows
  "Waiting for the approval…" instead of a countdown stuck at 0:00.

## 0.23.0 — 2026-11-16

- **History per device.** Every visit now remembers which device it was made on.
  `krunk://history` opens on **This device**; chips above the list show each of your other synced
  devices by name, **Unknown device** (visits synced before this version) and **All devices**.
  Search works inside the chosen device, and each row under All names its device.
- **Deleting follows the chip.** Deleting a time range of This device removes those visits here
  and on your other devices; for another device it only hides them here (they stay on that
  device). All devices and Clear browsing data still clear everywhere. Address-bar suggestions
  keep using every device's history.
- **Works with older versions.** Devices still on 0.22 or Android 0.7 keep syncing; their new
  visits are named by their device. Nothing in your history is lost by the update.
- **Turning on sync with your vault password works.** Since 0.21.1, "Turn on sync" on a computer
  that was the first to sync asked for the vault password and then did nothing: the request never
  reached the part of Krunk that handles it (found in the end-to-end check on the VM).

## 0.22.3 — 2026-11-15

- **Sync works again with a big history.** Sync stopped with "Something went wrong with sync"
  once the history held more than 200 pages of one site: Krunk then failed to find a page it
  already had, tried to add it twice and gave up, on every run. It finds it now.

## 0.22.2 — 2026-11-15

- **No more long sync waits.** "Sync will try again in 60 min" and "Too many attempts. Try again
  in 58 minutes" are gone: sync retries within a minute, and a busy server never makes it wait
  longer than that.

## 0.22.1 — 2026-11-15

- **The VPN's "Use for" choice works.** In 0.22.0, choosing "Private tabs only" in the VPN popup
  was ignored and the VPN kept covering every tab. It now takes effect at once and is remembered.

## 0.22.0 — 2026-11-15

- **VPN: no more random "Connection has been reset".** The cause: Krunk's VPN network stack
  closed any connection that had been quiet for 60 seconds, without telling the server. Idle
  HTTPS connections, paused video downloads and long-polling pages died silently, and the next
  use showed "Connection has been reset". Connections through the VPN now send a keep-alive
  every 20 seconds, so they stay open, and a server that is really gone is still noticed
  within a minute. Closed connections also free their slot sooner, so a burst of requests is
  no longer refused.
- **VPN for private tabs only.** The VPN popout has a new choice: "Enable everywhere" (as
  before) or "Private tabs only", where only private tabs go through the VPN and normal tabs,
  extensions and Krunk's own updates go direct. It takes effect at once and is remembered.
- **Picture-in-picture from the right-click menu** on videos ("Exit picture-in-picture" while one
  is floating). Videos that forbid picture-in-picture don't show it.
- **New-tab shortcuts are harder to misclick:** edit and remove moved into a small "⋮" menu in
  the tile corner, and a removed shortcut can be brought back with Undo.
- **Google is the default search engine and homepage** for new profiles. Profiles still set to
  DuckDuckGo switch to Google once. Krunk can't tell a profile that chose
  DuckDuckGo from one that kept the old default, so every profile still on DuckDuckGo switches;
  pick it again in Settings → Search to keep it.
- **Private tabs never get a hover preview picture:** their hover card shows the title only.
- **Pages that stopped loading until a restart (seen with Gmail).** Our best explanation: Krunk
  held some network requests open with no time limit, mainly site sign-in prompts waiting for
  a background tab or a hidden dialog. Each held request blocked one of the engine's few
  connections to that site, and enough of them stalled the site's later loads. Unanswered
  sign-in prompts are now refused after 2 minutes, and Krunk's own pages always answer within
  10 seconds, so one stuck request can't block later loads.
- **Sync errors say what went wrong:** instead of only "Sync will try again in 60 min", the
  status line now gives the reason first.

## 0.21.1 — 2026-11-15

- **One password for sync: your vault password.** Turning sync on asks for the password that
  unlocks your saved passwords, and nothing else: no sync passphrase or recovery key to write
  down. On another computer or your phone you sign in to your Krunk account and type that same
  vault password once; bookmarks, history, tabs, settings and passwords come along.
- **Already syncing?** Unlock your passwords once after updating: Krunk switches your sync over
  to the vault password by itself. Your old sync passphrase and recovery key stop working then.
- "Finish securing" after removing a device needs no passphrase any more: unlocking your
  passwords does it.
- No more "your sync recovery data was downloaded" mails from devices that already sync; a new
  device signing in still sends one.
- Choose a strong vault password: it now also protects your synced data on the server.

## 0.21.0 — 2026-11-15

- **Continue past a certificate warning.** When a site's certificate can't be trusted
  (self-signed, expired, issued for another name, unknown authority), Krunk's own warning page
  shows the site, the reason and the certificate's SHA-256 fingerprint, with **Back to safety**
  and **Continue anyway**:
  - **Remember for this site** keeps the choice for that site and that exact certificate. If
    the site later shows a different certificate, Krunk warns again. Without it, the choice
    lasts until Krunk is closed;
  - in private tabs a choice lasts only until the last private tab closes;
  - where the engine allows no exception (HSTS sites), there is no Continue button;
  - Settings → Privacy & security → **Certificate exceptions** lists what you remembered, with
    Remove. The list stays on this computer; it is not synced.
- **Settings keep what they don't understand.** A settings value Krunk can't read (from a newer
  Krunk, or damaged) no longer resets your other settings to their defaults, and settings a
  newer version added are kept when this one saves.
- **Safer writes.** Recording a visit, starring a page and reordering or editing new tab tiles
  are each saved as one step, so a crash in between can't leave half of it behind.

## 0.20.1 — 2026-11-14

- **Lines under the bars can be turned off.** Settings → Appearance → Tab and toolbar style →
  **Lines under the bars**: the thin lines under the toolbar and under the bookmarks bar broke
  up gradient styles. With the option off, the style runs through the bars without a break.
- **Background tabs with playing media stay awake.** A tab whose video or music is playing is
  no longer put to sleep or unloaded when the media is muted or silent (before, only a tab
  that played sound was spared), and neither is a tab that uses the camera or the microphone.
  With Settings → General → Tabs and memory → **Keep tabs with playing media awake** turned
  off, muted media does not keep a tab awake; tabs that play sound always stay awake.
  Note: the engine itself pauses a video without sound when its tab goes to the background, as
  Chrome does; this release does not change that.

## 0.20.0 — 2026-11-14

- **Frost and Neon look like their names.** Settings → Appearance → Tab and toolbar style:
  - **Frost** is now icy: pale blue-white bars, pill tabs, a white outline and a cold blue glow
    around the active tab (it was the Glass style with pill tabs).
  - **Neon** is now a neon sign: near-black bars, a hot pink outline with a cyan glow around
    the active tab (it was a plain style with an outline).
- **Eight new styles:** Synthwave, Matrix, Graphite, Citrus, Mint, Sakura, Coffee and Royal.
- **A style can bring its colours.** Frost, Neon and several of the new styles set the accent
  colour (Frost, Neon and Synthwave also the outline's). Choosing another style afterwards takes those colours away again; colours you
  chose yourself are kept. If you used Frost or Neon before, your look stays as it was:
  choose the style again to get the new one.

## 0.19.0 — 2026-11-13

- **WASM memory inspector.** ⋮ → **WASM memory inspector** opens `krunk://wasm` for the tab
  you are on: a tool for looking into and changing the memory of WebAssembly programs (games,
  emulators, apps), in the manner of a memory scanner.
  - It lists the page's WebAssembly **memories** and **instances** (with their exported
    functions and globals), also those of workers and of frames of the same site, and keeps up
    when a memory grows.
  - **Scan** for a value (exact, a range, or "unknown" to start from everything), then narrow
    the list after the value changed: changed, unchanged, increased, decreased, or a new exact
    value. Types: 8 to 64 bit integers, floats, text (UTF-8, UTF-16) and raw bytes.
  - **Edit** what you found, **freeze** it so the program cannot change it, keep a **watch
    list** with labels (export and import it), follow **pointer paths**, edit exported globals,
    and read or change bytes in a **hex view**.
  - Nothing is installed in pages you do not inspect, and nothing is stored: scans, freezes
    and the watch list end with the tab. Krunk's own pages cannot be inspected.
- **For user scripts:** `GM.wasm` (with `@grant GM.wasm`) gives a script the same operations:
  `memories`, `instances`, `read`, `write`, `bytes`, `scan`, `next`, `results`, `freeze`,
  `unfreeze`, `watch`, `globals`, `setGlobal` and more. Scripts with this grant are marked
  **WASM memory** on `krunk://scripts`.
- **For the AI agent:** the tools `wasm_memories`, `wasm_read`, `wasm_write`, `wasm_scan`,
  `wasm_results`, `wasm_freeze`, `wasm_unfreeze`, `wasm_watch` and `wasm_global`, with the
  limits of its other page tools (not on Krunk's pages, not while a password field is filled).
- Limits: a scan covers the first 512 MiB of a memory and keeps about 4 million addresses; a
  frozen value is rewritten about 20 times a second (slower while its tab is in the
  background); frames of other sites, service workers and shared workers cannot be reached.
  Reference: `docs/WASM_INSPECTOR.md`.

## 0.18.0 — 2026-11-13

- **Pages that got stuck loading, and took other tabs with them.** Two things in the engine
  caused it, both measured in Krunk on Windows (`docs/NETWORK.md`):
  - *The same address in two tabs.* While one tab waited for a page (or a script, or an API
    call) that the server was slow to answer, any other tab asking for the same address waited
    behind it, for up to 20 seconds, even though the server would have answered. Now such a
    request goes around the engine's cache queue once the first has waited a second: in the
    test the second tab's request reached the server after 1 second instead of 21.
  - *Six connections per site.* The engine opened at most six connections to one site. Six
    requests that never finished (or six open live streams) took them all, and nothing else
    from that site loaded in any tab, the tab's own Reload included. The limit is now 16, and
    before you reload a tab or send it elsewhere, Krunk stops what that tab is still waiting
    for, so a stuck tab can always be reloaded or left.
  - To see it: `krunk://tasks` has a **Pending** column (how many answers a tab waits for and
    for how long, red after 30 seconds), and `krunk.log` names the site (never the address)
    of requests pending for over 30 seconds.
- **Middle-click in the Back and Forward menus** (right-click or hold the Back or Forward
  button for the menu) opens that entry in a new background tab next to the current one. The
  new tab has the whole back/forward list, so Back and Forward work in it; the tab you were
  in stays where it is.
- **Middle-click (or Ctrl+click) on Reload** opens the page again in a new tab. Middle-click on
  the Home button opens the home page in a new tab (since 0.16.0).
- The engine was rebuilt for this release with two small changes (the tab-history copy and
  the connection limit); Chromium's version is unchanged (156.0.8078.80).
- Known limits: sixteen requests that never finish still block a site until their tab is
  closed or reloaded; a link clicked inside a stuck page is not helped by the reload rule.

## 0.17.0 — 2026-11-13

- **MetaMask works.** It installed before but never started. Tested with MetaMask 13.56 from
  the Chrome Web Store, with a throw-away wallet: set-up completes, the toolbar popup unlocks
  after a restart, pages get `window.ethereum`, a site can connect (`eth_requestAccounts`) and
  ask for a signature (`personal_sign`); MetaMask's own connect and signature windows open as
  small Krunk windows.
- **Why it did not load.** Krunk ran an extension's background "service worker" as a script
  in a hidden page. MetaMask checks `self.serviceWorker.state` first and stopped right there;
  its popup waited for a connection (`chrome.runtime.connect`) that Krunk did not have; and
  the script that gives pages `window.ethereum` (a content script for the page's own world)
  was left out. All three are fixed for every extension, not for MetaMask by name: the
  background is a real worker with `importScripts`, there are message ports between
  background, popup, pages and content scripts, and `world: "MAIN"` content scripts run at
  document start. Also new: offscreen documents, popup-type windows, `storage.session`,
  `unlimitedStorage`, `scripting.registerContentScripts`. Details: `docs/EXTENSIONS.md`.
- **Extensions can reach the servers their manifest names.** As in Chrome, an extension's
  background and pages may fetch from its permitted hosts even when the server sends no
  cross-origin headers (Krunk relays these requests itself; without that MetaMask showed
  "Security check unavailable" on every signature). No cookies are sent with them and
  responses are not streamed.
- **Errors of an extension are shown.** `krunk://extensions` lists what an extension's
  background, popup, pages and content scripts reported (uncaught errors, rejected promises,
  `console.error`), so "installed but does nothing" can be looked into.
- **Dark Reader's settings open again** from its toolbar button and Options (they stayed at
  "Loading").
- **Safer extensions.** A review of the new code found seven weaknesses, all fixed before this
  release: a content script's sender is the page it really runs in (not a page the tab was
  only on its way to); extensions cannot open local files or Krunk's own pages, and
  `scripting.executeScript` needs access to the site; web pages can load only the extension
  files the extension offers them (`web_accessible_resources`); an extension's stored data
  is written safely (a crash can no longer lose a wallet's vault) and has a size limit;
  `storage.session` stays out of web pages; sandboxed extension pages cannot be reached
  unsandboxed; an extension cannot flood the log.
- **Changed, as in Chrome:** a newly installed or enabled extension starts on a page with
  the page's next load, not in pages that are already open.
- Known limits: requests an extension makes to its servers carry no cookies and no extension
  origin; hardware wallets were not tried; MetaMask was tested with one version and with the
  flows above only (no transaction was sent). An extension page opened in a tab shows Krunk's
  address for it (`https://<id>.ext.krunk.invalid/…`).

## 0.16.0 — 2026-11-13

- **Right-click in the AI panel.** The panel now has the same context menu as pages: Cut, Copy,
  Paste and Select all in the message box and on answers, the link items (open in a new tab,
  window or private tab, copy the address) on links, and the image items on the step pictures.
  Krunk has no separate AI window: the "popped-out" panel is the one docked at the side.
- **Right-click an extension's toolbar icon** for its menu: the extension's page in Krunk,
  **Options**, **Manage extensions**, **Disable** and **Uninstall…**. Uninstall asks first
  ("Remove <name>?").
- **Session usage in the menu.** ⋮ → **Session usage** opens your session logs (krunk://sessions):
  how long each site and tab was in front, and Krunk's CPU use per session. If logging is off,
  the page offers to turn it on. (This is the browsing-session screen; there is no screen for
  the AI's token use.)
- **Private tabs' titles are readable on every style.** The purple title was hard to read in the
  dark theme and on coloured tab strips (Midnight, Forest, Ember, Ocean, Glass, purple strips).
  Krunk now picks a purple that stands out from what is behind the tab, or white or black where
  no purple does, also under the mouse, on the active private tab and while the window is in
  the background (at least 4.5:1 on every built-in style). Your own **Tab title colours** now
  apply to private tabs too.
- **A Home button, if you want one.** Settings → Appearance → **Show the Home button** adds it
  after Reload. A click opens your home page (the new tab page if you have none); Ctrl+click or
  a middle-click opens it in a new tab. The home page's address is right under the switch; it is
  the same setting as General → On startup → Homepage.
- **The AI agent keeps a scratchpad.** On longer tasks it writes down a plan and notes, so they
  don't get lost when a conversation gets long. The notepad button in the panel's header shows
  them; **Clear** empties them. Kept in memory only: nothing is written to disk, and it is gone
  with a new conversation or when the window closes.
- **The AI agent can create or enter a password when you ask for it.** Say "sign up here with a
  strong password" or "log in with the password …": Krunk asks you first, naming the site. A new
  password is made by Krunk's own generator, never by the AI, which doesn't get to see it; a
  card in the panel keeps it for you (**Show**, **Copy**), and Krunk saves the login when you
  send the form. Without such a request nothing changes: the agent can't touch password fields.

## 0.15.4 — 2026-11-12

Display scaling. With Windows' Text size setting above 100 %, at display scales of 125 % and
more, and when the scale changed while Krunk ran, the layout had faults that a plain 100 %
display never showed. What was wrong and how it is checked now is in `docs/DPI.md`.

- **Fixed: Windows' "Text size" setting broke the whole layout.** With Settings →
  Accessibility → Text size above 100 %, Krunk drew its tab strip, toolbar and menus larger
  but still placed them as for 100 %: the bookmarks bar disappeared under the page, menus
  opened in the wrong place and were cut off, and the minimize, maximize and close buttons
  did not react where they were drawn. Krunk now lays everything out for the scale it really
  draws at, which is the display's scale times Text size.
- **Fixed: changing the display scale while Krunk is maximized broke the layout.** After a
  change of "Scale" in Windows' display settings, or when the maximized window ended up on a
  monitor with another scale (a laptop and its dock), the tab strip and the toolbar kept their
  old height: at a larger scale the bookmarks bar was cut off, at a smaller one a blank band
  stayed under it, until the window was resized. Krunk now lays a window out again whenever
  its scale changes, tells the pages, and closes menus that were sized for the old scale.
- **Fixed: the ⋮ menu was cut off on smaller screens.** On a 1920×1080 display at 150 % or
  175 %, and on smaller displays at 125 %, the menu was higher than the screen: it jumped to
  the top, over the tab strip, and Settings, About Krunk and Exit were below the edge and could
  not be reached. A menu that is higher than the screen now gets more compact rows, and scrolls
  if that is still not enough. This holds for every menu and prompt of Krunk.
- **Context menus open upwards** (tab, bookmark and title-bar menus) when there is no room
  below the click, instead of covering it.
- **Fixed: new windows were tiny at 200 % and more.** A window without a saved size was
  1280 × 860 physical pixels: a quarter of the intended area at 200 %, close to the minimum
  size at 250 %. It is now the same size at every scale and never larger than the screen
  (on small displays its lower edge used to sit under the taskbar).
- **The window keeps its size when the scale changes between two starts**, and comes back
  inside the screen if it no longer fits.
- **The window's size and place are remembered in every start-up mode.** With "Open a new tab"
  or "Open the homepage" Krunk started in the default size every time; only "Continue where
  you left off" kept it.
- **Fixed: in a very narrow window the ⋮ button was pushed out of the toolbar.** The address
  field now shrinks first; if there is still no room, extension icons, the zoom indicator and
  bookmark star, and then the downloads, VPN and AI buttons step aside. Back, Forward, Reload,
  the shield and ⋮ always stay.
- Sharper taskbar and Alt+Tab icons on a monitor whose scale differs from the main one.

## 0.15.3 — 2026-11-11

- **Right-click a link** for **Open link in new tab**, **Open link in new window**, **Open link in
  private tab** and **Copy link address**.

## 0.15.2 — 2026-11-11

- **No black flash when you switch to a tab in the background** (for example from one new tab
  to another): the page you were looking at stays until the other tab has drawn again.
- **The active tab's glow is no longer cut off** at the left of the first tab and the right of
  the last one.
- **Tab titles stay readable on a styled tab strip when Krunk is in the background.** With a
  gradient strip (Ocean, for example) the titles used to turn a faint grey when the window lost
  focus.
- **Choose the tab titles' colours.** Settings → Appearance → Tab and toolbar style →
  **Tab title colours**: one for the other tabs and one for the active tab (empty = automatic).
- **Style the active tab's outline.** With **Active tab: Outlined**, pick the outline's colour
  and its thickness (1–4 px).
- **Active tab: None.** Leaves only the title (and the glow, if on) to mark the active tab; the
  Glass style now uses it.
- **The bookmarks bar follows a gradient toolbar**, as one continuous gradient with the toolbar.
- **Seven new styles:** Aurora, Midnight, Forest, Ember, Candy, Frost and Neon.
- **Big extensions install.** MetaMask failed with "the package is too large (unpacked size)":
  an extension may now unpack to 512 MB (was 64 MB) with up to 20,000 files, and the download
  may be up to 256 MB.
- **Open a selected address.** Select a web address in a page's text (like `example.com/page`)
  and right-click: **Open in new tab**, **Open in new window** and **Open in private tab**.

## 0.15.1 — 2026-11-11

- **Fixed: the AI agent couldn't change some settings** when the model sent a number as text,
  for example the new tab background's blur (`"8"` instead of `8`): it got "the value must be a
  whole number". Numbers and true/false sent as text are now accepted; the allowed ranges still
  apply.

## 0.15.0 — 2026-11-11

- **No more black flash when you open or close a tab.** The page you were looking at stays on
  screen until the next tab has painted (at most half a second), and Krunk's own pages show the
  theme's colour from their very first frame.
- **Tab previews.** Rest the mouse on a tab for half a second to see its title, its site and a
  small picture of the page. The pictures stay in memory only. Turn it off in Settings → General
  → New tabs → **Preview tabs on hover**.
- **Sign-in dialogs stay with their tab.** When a site asks for a user name and password
  (HTTP authentication), switching to another tab or app no longer cancels it: the dialog
  hides, the tab shows a key badge, and the dialog comes back when you return. A background
  tab can ask too. Only Cancel, closing the tab or leaving the page cancels it.
- **Search suggestions in the address bar.** While you type, the search engine's suggestions
  appear below your history and bookmark matches (Google, Bing, DuckDuckGo, Brave, Startpage
  and Ecosia). Never in private tabs. Turn them off in Settings → General → Search.
- **Made-up domains are searched.** Text like `test.test` now searches instead of opening a
  broken address. Real domains (`example.com`, `foo.dev`), IP addresses, `localhost`, addresses
  with a port and local names (`.local`, `.lan`, `.internal`, `.home.arpa`) still open, and so
  does anything you type with `http://` or `https://`.
- **A new downloads page.** Cards grouped by Today, Yesterday and Earlier, a search box,
  filters (in progress, done, failed), "Clear completed", and an option to remove finished
  downloads after some days (Settings → Downloads). It stays fast with thousands of downloads,
  and the list is no longer limited to the last 200.
- **Session logs (off by default).** Settings → Privacy & security → **Keep session logs**
  records, on this device only, the sites you visit, how long each tab and site is in front, and
  Krunk's CPU use, per session. See them on `krunk://sessions`, with a timeline and a chart;
  delete one or all, and choose how long they're kept (30 days by default). Private tabs are
  never logged, and while it's off nothing is recorded.
- **Tab and toolbar styles.** Settings → Appearance → Tab and toolbar style: solid, gradient (two
  or three colours and an angle), ombre across the tabs, or glass (translucent), plus ready-made
  styles, tab shapes (rounded, square, pill) and roundness, how the active tab stands out, and an
  accent glow. All of it works in light and dark.
- **New tab page tiles:** the pin, edit and remove buttons now sit together in one corner and
  never cover the icon or the title; a pinned tile shows a filled pin there.
- **AI agent:**
  - **Longer answers.** The output limit is now 32,000 tokens by default (or the model's own
    maximum when that is lower), and each provider has a **Max output tokens** setting
    (0 = let the provider decide). When a model stops at the limit before answering, Krunk tries
    once more with a higher limit, and the message names the limit it used.
  - **Long scripts are no longer cut off.** A reply cut off in the middle of an action never
    runs; the agent is told and can save long userscripts and macros in parts.
  - The agent panel follows the light and dark theme at once.
  - The agent can set the new tab background from a picture's https address (up to 15 MB; PNG,
    JPEG, WebP, GIF or AVIF).
- Known limitation: after you switch between light and dark, tabs that were already open keep
  their old background colour for the moment before a page paints (Windows' engine views can't
  change it); new tabs use the new one.

## 0.14.2 — 2026-11-10

- **Use your local network while the VPN is connected.** A new option in Settings → Privacy &
  security → VPN, **Reach the local network directly**, lets your router, printer, NAS and other
  home servers skip the VPN. It covers addresses like 192.168.x.x, 10.x.x.x and 172.16–31.x.x,
  and names without a dot or ending in .local or .lan. Everything else still goes through the
  VPN. The option is off by default and takes effect at once, also while connected.

## 0.14.1 — 2026-11-10

- **Fixed: Krunk closed on the first page after connecting a WireGuard VPN.** It happened with
  profiles whose `MTU` is below 1420, e.g. `MTU = 1280`, as many VPN providers set. Part of the
  VPN kept using the larger packet size, and the first large packet stopped Krunk. The
  profile's MTU now applies everywhere.
- **Fixed: Krunk could crash while closing** after the task manager had been open. Windows then
  reported a crash, and could save a crash dump.
- The VPN no longer turns away one of Krunk's own connections when Windows lists it a moment
  late.

## 0.14.0 — 2026-11-10

- **A VPN for Krunk only.** Settings → Privacy & security → VPN imports WireGuard (`.conf`) and
  OpenVPN (`.ovpn`) files from your VPN provider.
  - Only Krunk's connections use it: normal and private windows, and Krunk's own updates, sync,
    filter lists and extensions. Other programs on your computer are not affected, and nothing
    is installed in Windows: no network adapter, no driver, no administrator rights.
  - While the VPN is on but not connected, Krunk's connections stop instead of going around it.
    Addresses outside a profile's networks are refused, not sent directly.
  - Names are looked up through the VPN, and WebRTC can't reveal your address.
  - **Show the VPN button in the toolbar** (off by default) adds a button next to the shield. Its
    popup shows whether you're connected, the profile, your address in the VPN and the traffic,
    with Connect and Disconnect.
  - **Connect when Krunk starts** connects before any page loads.
  - Profiles are stored encrypted for your Windows account and never sync. Scripts in profiles
    never run.
- WireGuard and OpenVPN run inside Krunk (boringtun, and OpenVPN 3 Core, the library behind
  OpenVPN Connect). OpenVPN works over UDP and TCP, with a user name and password or
  certificates included in the file.

## 0.13.0 — 2026-11-10

- **Sign in by scanning a QR code.** While you sign in, Settings → Account now shows a QR code
  of the sign-in link next to the code. Scan it with a phone that is signed in to your Krunk
  account to approve this device there. If the website sends no QR code, Krunk shows the code
  alone, as before.
- **Add a device.** When you're signed in, Settings → Account has a new **Add a device** button
  next to **Manage account**. It opens the "Add a device" part of your account page in a new
  tab.
- **On the website, your account page gained:**
  - **Add a device:** a QR code that a phone scans to sign in, in its browser or, later, in the
    Krunk app for Android. You approve it after checking that both screens show the same
    6-digit code.
  - **Usage:** what your account stores against its limits, collection by collection, your
    devices and when they last synced.
  - **Activity & IP addresses:** the sign-ins and other security events of the last 90 days,
    with the IP address and country they came from.
  - **Download your data:** everything the server keeps about your account as one JSON file,
    straight to your browser and never by email.
  - **Reset sync** and **Delete account.** Deleting is immediate, and you get a receipt by
    email.
- **Changelog:** every release's notes on one page, at [/changelog](/changelog).

## 0.12.4 — 2026-11-10

- **Page dialogs can no longer get stuck behind the window.**
  - A site's alert, confirm, prompt and "Leave site?" / "Reload site?" used to be separate
    windows. They could end up behind Krunk while the page waited for them, and the tab looked
    frozen.
  - They are now Krunk's own prompt, shown in the middle under the toolbar and named after the
    site ("example.com says"). It always stays above the window.
  - Every way it ends answers the page:
    - OK / Leave (or Enter), or Cancel;
    - Esc or a click elsewhere: you stay on the page;
    - switching or closing the tab.
  - A dialog from a tab you're not looking at is answered at once; leaving the page then goes
    ahead.
  - From a site's second dialog on, **Don't let this page show more dialogs** stops the rest
    until the page loads again. A page you stopped this way always lets you leave.

**Testing note:** checked on Windows 11 with the installed build:
- alert, confirm, prompt;
- "Leave site?" (stay and leave);
- a click elsewhere;
- a background tab's dialog;
- a page that opens dialogs in a loop.

Each time the prompt was in front of the window. Krunk's automated tests cover the rest. The
engine is the same as in 0.12.3.

## 0.12.3 — 2026-11-10

Includes 0.12.1 and 0.12.2, which were not released on their own.

- **Make Krunk your default browser.**
  - Krunk now registers with Windows as a browser.
  - While it isn't your default, Settings shows a banner and General → Default browser says so.
  - **Make default** opens Windows Settings at Krunk's page; click **Set default** there.
    Windows doesn't let programs do this step themselves. Settings notices the change by itself.
  - Links and web pages you open from other apps then open in Krunk: in a new tab if it is
    already running, or Krunk starts with them.
- **Save and copy images.** Right-click an image for:
  - **Open image in new tab**;
  - **Save image as…** (always asks where to save);
  - **Copy image** (paste it into other programs; transparency is kept where the program
    supports PNG);
  - **Copy image address**.
- **Private tabs stay private in Windows' media controls.** Video or music playing in a private
  tab no longer shows its title or artwork in Windows' "Now Playing" (volume flyout, lock
  screen). Nothing is shown there while it plays.
- **Krunk's logo in the media controls.** When a site gives no artwork, Windows now shows Krunk's
  logo instead of Chromium's.
- **Picture-in-Picture chooser.** With several videos, the rows no longer overlap.

**Testing note:** all of this was checked on Windows 11 with the installed build:
- the default-browser flow, and links and files opened from other apps;
- every image menu item;
- private-tab media with Windows' own list of media sessions;
- the chooser.

Krunk's automated tests cover the rest. The engine gets two small changes (the logo, and the
private-tab media), so this update replaces the engine files too.

## 0.12.2 — 2026-11-10

Not released on its own; shipped in 0.12.3.

- **Image items in the page context menu.** Right-click an image for **Open image in new tab**,
  **Save image as…** (always asks where to save), **Copy image** and **Copy image address**.
  If one of them fails, a message says why.

## 0.12.1 — 2026-11-10

Not released on its own; shipped in 0.12.3.

- **Make Krunk your default browser.** Krunk registers with Windows as a browser. While it isn't
  your default, Settings shows a banner and General → Default browser says so; **Make default**
  opens Windows Settings at Krunk's page, and Settings notices the change by itself.
- **Links from other apps** open in Krunk: in a new tab if it is already running, or Krunk starts
  with them.
- **Krunk's logo in the media controls.** When a site gives no artwork, Windows' media controls
  show Krunk's logo instead of Chromium's.
- **Picture-in-Picture chooser.** With several videos, the rows no longer overlap.

## 0.12.0 — 2026-11-10

An optional AI agent, plus userscripts and macros.

- **AI agent (optional, off by default).** Turn it on in Settings → AI and scripts and add your
  own provider: OpenRouter, OpenAI, Anthropic, or an OpenAI-compatible server such as Ollama or
  LM Studio. Then:
  - The **✦ button** next to the shield, or **Ctrl+Shift+Space**, opens a panel beside the page.
    Tell it what to do there.
  - It reads the page, clicks, types, fills forms, opens and switches tabs, saves screenshots,
    changes some settings, and writes userscripts and macros.
  - It may ask you a question with buttons. **Stop** (or Esc) ends a task at once.
  - **Your keys** are stored encrypted on your PC (Windows DPAPI) and are never synced or shown
    again.
  - **What it never gets:** your passwords, payment cards, the password vault, sync data or API
    keys. Password and card fields are hidden from it.
  - **Asking first:** your history, bookmarks, downloads and cookies are used only when you ask
    for them and click Allow.
  - **Where your data goes:** page content goes only to the provider you chose. Krunk runs no AI
    service.
- **Userscripts.** A new page, `krunk://scripts`, also in the menu, holds Tampermonkey-style
  scripts. Import a `.user.js` or write one in the built-in editor. The common GM functions work.
  `GM_xmlhttpRequest`, `@require`, `@resource` and regular-expression `@include` are not supported
  yet; the editor warns about them.
- **Macros.** Small scripts that press keys, click and type in the current page. Start one with
  its hotkey or Run; stop it with its hotkey, the toolbar button or **Ctrl+Shift+.**. A held key
  repeats like a real keyboard, and every stop lets go of it.
- **The code editor** is Krunk's own: line numbers, colours, find, and syntax errors shown at
  their line.

**Testing note:**
- The agent was tested on Windows 11 against a scripted test provider only. No live AI provider
  and no image-capable (vision) model was tried before this release.
- Userscripts, macros and the editor were tested on Windows 11. Krunk's automated tests cover
  the rest.
- The engine is the same as in 0.10.0.

## 0.11.7 — 2026-11-10

A security and reliability release from a full audit. Nothing changes in how you use Krunk.

- **Websites can no longer open Krunk's own pages** (Settings, Passwords, View source and the
  others) through a pop-up or a redirect. Only you open them: from the address bar, the menu,
  a new tab, or a link on another Krunk page.
- **Extensions are kept apart:**
  - Each extension's scripts now run on their own, so one extension can't read another's
    stored data or messages.
  - Extensions can never script Krunk's own pages.
  - Running code in a tab needs the "scripting" permission, as in Chrome.
  - A package that the Chrome Web Store didn't sign can't replace a Web Store extension you
    installed. Remove the old one first if you really want to.
- **Downloads:**
  - A website can no longer crash Krunk with a very long file name.
  - **SmartScreen/MotW now applies to segmented downloads:** files Krunk fetches in parts get
    the same "downloaded from the Internet" mark as its other downloads.
  - Krunk doesn't start a download in parts if the file wouldn't fit on the disk.
- **Fewer crashes from odd files:**
  - Importing a bookmarks file with non-English text works. Large bookmark files import in
    a moment (20,000 links: from about 6.5 s to 0.01 s).
  - A filter list with non-English text near its start no longer crashes Krunk.
  - A broken extension can no longer freeze Krunk while it is being installed.
- **Krunk accounts (website and sync server):**
  - Two-step sign-in can no longer be guessed through a password-reset link.
  - Rate limits now also hold for IPv6 users.
  - Sync storage limits apply in every case.
  - Housekeeping is faster.

**Testing note:** checked on Windows 11 with the audit's attack pages and two test extensions,
plus the usual smoke tests and an in-app update from 0.11.6. Krunk's automated tests cover the
rest. The engine is the same as in 0.10.0.

## 0.11.6 — 2026-11-09

- **Text you type in the address bar stays with its tab.**
  - Switch to another tab and back (by mouse or Ctrl+Tab), or click into the page: what
    you typed is still there, as in Chrome. It used to be replaced by the page's address.
  - The text goes away when:
    - you press Enter or Esc;
    - you open a suggestion or a bookmark;
    - the tab goes to another page.

**Testing note:** checked on Windows 11 by switching tabs by mouse and with Ctrl+Tab. Krunk's
automated tests cover the rest. The engine is the same as in 0.10.0.

## 0.11.5 — 2026-11-09

- **The downloads list under the toolbar's download button shows the newest download at the
  top.** It used to show the oldest first. With many downloads, the newest could be left out
  of the list.

**Testing note:** a one-line fix in the order of the list. It was built and checked by Krunk's
automated tests, not tried on Windows. The engine is the same as in 0.10.0.

## 0.11.4 — 2026-11-09

- **Address bar suggestions open when you click them.** A click used to close the list
  without opening anything.
- **Cancel is instant** for large downloads that use several connections. The download shows
  as cancelled at once, and Krunk stops the connections and deletes the partial file in the
  background.

**Testing note:**
- On Windows 11: clicking an address bar suggestion opens it.
- With automated tests: the shell and popup.
- Not tried on Windows: cancelling a download that uses several connections. The engine is
  the same as in 0.10.0.

## 0.11.3 — 2026-11-09

- **Password suggestions list every saved login for the site.** The list used to stop at 8.
  After 8 logins it scrolls.
- **PDF viewer**
  - PDFs open in a Krunk tab, also when a site sends them as a download. Use the viewer's
    toolbar to zoom, search, print or **Download** the file.
  - To save PDFs as downloads instead, turn off Settings → Downloads → **Open PDFs in Krunk**.
- **Email links**
  - Links that start an email (`mailto:`) work. Before, nothing happened.
  - Choose where they open in Settings → General → **Email links open in**:
    - your email app (Windows' default);
    - Gmail, Outlook.com, Yahoo Mail or Proton Mail;
    - another website.

**Testing note:** checked on Windows 11:
- a PDF a site sends as a download opens in the viewer and is not saved to Downloads;
- an email link opens Windows' email app, and with the Gmail setting opens Gmail.

The other changes were checked with Krunk's automated tests. The engine is the same as in
0.10.0.

## 0.11.2 — 2026-11-09

- **Dragging a window by its only tab** moves the window at once. It used to slide the tab
  along the tab row first, as if there were other tabs.
- **Save / update password:** the prompt shows the username in a field. Correct it before
  you choose Save or Update, and the login is saved under that name.
- **Logins in steps** (email or username first, then the password on the next step):
  - the username step gets your saved logins as suggestions, and picking one fills the
    username;
  - when the password is saved, Krunk keeps the username from the first step.

**Testing note:** this release was built and published without new testing on Windows, at
the operator's request. Only the build and its compile checks ran. The engine is the same as
in 0.10.0.

## 0.11.1 — 2026-11-09

- **Dragging tabs is smooth**
  - A tab you drag out of a window becomes its own window at once. The window follows the
    mouse on every move, without jumps or pauses.
  - Over another window's tab row, the dragged window steps aside and that row opens a gap
    with your tab in it, where it will land. Let go to drop it there. Move away from the row
    and the window comes back under the mouse.
  - Esc while dragging puts the tab back.
- **Menus keep working.** Right-click menus and ⋮ could stop responding after you moved tabs
  between windows or closed a window. That no longer happens.
- **Bookmarks bar:**
  - In » (and in folder dropdowns), Back returns to a list of the right height. It used to
    keep the height of the folder you came from.
  - The dropdowns are wider and can grow as tall as the screen allows.
- **Back history:** `about:blank` no longer appears in the Back button's history menu of a
  new tab.
- **Extensions page** (krunk://extensions): more space around the header, the Web Store box
  and the extension cards.

**Testing note:** tested on Windows 11 (one monitor, 100 % scaling).

Checked on Windows:
- tab dragging: tearing off, following the mouse (measured), dropping onto another window, the
  gap in its tab row, and Esc;
- the menus after merging a window back;
- the bookmarks dropdown height after Back, the history menu and the extensions page;
- the general shell and engine checks, the older window and tab-drag checks, and the update
  from 0.11.0.

Not tested: dragging across monitors with different scaling. The engine is the same as in
0.10.0.

## 0.11.0 — 2026-11-09

- **Bookmarks bar**
  - A folder opens with its own contents. It no longer shows the folder you opened before for a
    moment.
  - In a folder opened inside a dropdown (or "»"), the first row is **‹ Back** to the folder it
    came from. Backspace and Left also go back.
  - Middle-click on a bookmark opens it in a new tab behind the current one, and the dropdown
    stays open.
  - Bookmarks of sites you never visited (imported ones, for example) get their icon. Krunk
    fetches each one once in the background, without cookies.
- **Many tabs**
  - The tab row scrolls with the mouse wheel and ‹ › buttons, and its edges fade where more tabs
    are.
  - **▾** lists every tab of the window: type to find one, × to close it.
  - Middle-click closes a tab also when the row is scrolled. It used to start the scroll cursor
    instead.
- **Pages that hang:** a page that stops responding no longer leaves you stuck. After about
  15 seconds Krunk asks: **Wait** or **Close page** (then **Reload**). The rest of Krunk keeps
  working. A page that crashes shows the same Reload page.
- **View page source** (right-click, Ctrl+U or `view-source:`) opens in a Krunk tab instead of
  Notepad. It has line numbers and colours, **Beautify** to indent the HTML, and **Wrap lines**.
- **Search for selected text:** right-click selected text for "Search <your search engine> for
  …".
- **Passwords:** the unlock dialog has **Remain unlocked this session**, checked by default.
  Checked, your passwords stay unlocked until you quit Krunk, without the auto-lock timer.
  Krunk remembers your choice.
- **Picture-in-Picture volume** is now inside the floating video: point at its mute button and a
  small slider opens next to it. The separate bar under the window is gone.
- **Windows media overlay** (volume flyout, lock screen) shows Krunk's name and icon instead of
  Chromium's.
- **Cookies and site data** stays fast with thousands of sites:
  - it shows the list a page at a time;
  - it says how many subdomains each site has;
  - you can sort by cookies, and "Clear all shown" says how many sites it clears.
- **Your device's name** no longer carries the version from the day you signed in. Settings and
  the website show the version it runs now.
- **Website** (krunk.209.wf): your account page has a card for each topic: profile and email,
  password, two-step verification, devices and sync, signed-in browsers.

**Testing note:** tested on Windows 11 (one monitor, 100 % scaling). The new features were each
checked on Windows: bookmark dropdowns and Back, middle-click, the icon fetch and that it is not
repeated after a restart, 60 tabs, the source viewer, search for a selection, the PiP slider, the
unlock checkbox both ways, a hung and a slow page, and the cookie list. The general shell and
engine checks were rerun, and so was the update from 0.10.2. Not tested on Windows: the media
overlay's name with a second media app open, the account card after signing in (the website and
the device-name change were tested on the server), auto-PiP on tab switch, and older test rounds
whose code did not change. The engine is the same as in 0.10.0.

## 0.10.2 — 2026-11-09

- **Address bar:** suggestions complete from the first letter, and typing a site's name now
  completes the site itself: `yo` completes `youtube.com` (the site you visit most that starts
  with what you typed), not one of its videos. That site is the top row, with its most visited
  pages under it. Type on past the name (`youtube.com/w`) to complete a page; a completed page
  address now keeps its upper and lower case (a video link used to be completed in lowercase).
- Includes 0.10.1: after unlocking your passwords from a login form, only that site's logins.

**Testing note:** only the address-bar suggestions changed; tested on Windows 11 with real
YouTube history. The engine is 0.10.0's.

## 0.10.1 — 2026-11-09

- **Saved passwords:** after you unlock your passwords from a site's login form, the list now
  shows only that site's logins. Krunk used to also offer every login saved for other sites of
  the same domain (for example all your `*.example.com` services), without saying which site
  they were for. A site with no logins of its own still offers those of its domain, each with its
  site's name.
- **Account page:** the devices list on krunk.209.wf now shows the version each device runs
  after an update, not the one it had when it was linked (a server fix; it shows up once the
  device has synced).

**Testing note:** a small fix. The engine and everything else are 0.10.0's. The unlock flow was
tested on Windows 11 before and after the fix, and the full regression was not repeated.

## 0.10.0 — 2026-11-09

- **Fast with many tabs.** Tabs in the background are now really in the background: Krunk's
  engine treats them as hidden, the way Chrome does, so a page you're not looking at no longer
  animates or runs its timers at full speed. Up to 0.9.0 every open tab kept running as if it
  were on screen, which made Krunk slow down sharply with many busy tabs. On our 4-core test PC:
  - 30 busy tabs used 90 % of the processor, now about 1 %;
  - with 60 tabs open, the tab in front kept 64 frames a second instead of 35, typing reacted
    within 30 ms instead of up to 0.7 s, and switching tabs took 21 ms instead of 0.2 s;
  - a browser benchmark (Speedometer 3.1) in the front tab scored 27.5 with 29 busy tabs behind
    it, against 1.5 before — about the same as with no other tabs (27.9).
- **What keeps running in the background:** music and videos with sound, calls (WebRTC),
  downloads, notifications and picture-in-picture videos. Coming back to a tab shows it at once,
  with what you typed and where you scrolled.
- **Smoother tab strip:** pages that change their title or icon all the time (mail, chat) no
  longer make Krunk redraw the whole tab strip or the toolbar, and page titles are written to
  your history in the background.
- **A new look for the website** (krunk.209.wf): the home page, the download box, the privacy
  notice and the error pages.

**Engine note:** 0.10.0 changes the engine itself. Krunk's `libcef.dll` (Chromium 156.0.8078.80
through CEF 156.0.15, with codecs) now carries one Krunk patch,
`scripts/cef-build/patches/krunk-windowed-hidden.patch`. The update replaces the engine files
(the download is the same size as before); your profile is not changed.

**Testing note:** tested on Windows 11 with one monitor; a second monitor was not tested. The
Windows test run covered the hidden-tab checks (sound, calls, downloads, notifications, DevTools,
printing, minimise/restore, covered and moved windows, no blank or white frames), 42 of the
regression flows and the in-app update from 0.9.0. Sync code is unchanged; its end-to-end runs
and one local-network flow were not repeated for this release. At one to ten tabs, the time
from a key press to the next frame in the front tab measures 27 ms (18–22 ms before), the same
as Chrome on that PC.

## 0.9.0 — 2026-11-09

- **Sync (beta):** keep bookmarks, history, settings, new tab pins, ad-blocker exceptions, open
  tabs and — with your password vault unlocked — passwords on all your devices. It is off until
  you turn it on in Settings → Sync (beta), and everything is encrypted on your device before it
  leaves: the server only ever stores data it can't read, and the website never asks for your
  sync passphrase or recovery key.
  - The first device makes a 7-word passphrase and a recovery key: keep them safe. Another device
    joins by approval from a device that already syncs (you compare a 6-digit code) or by
    restoring with the passphrase or the recovery key.
  - krunk://sync shows your devices, other devices' open tabs, and lets you rename or remove a
    device. Removing one ends its access at once: Krunk re-secures your data with new keys that
    the removed device never gets.
  - Choose per device what syncs; Sync now; turn sync off (keeping or removing what it brought).
  - Your passwords are protected by the vault as before: the key that encrypts synced passwords is
    kept only inside the vault.
- **A redesigned account website** (krunk.209.wf/account): a clearer dashboard with your devices
  and their sync state, "Sign out everywhere else", rename and remove, two-step sign-in with a
  copyable key, backup codes you can copy or download, a dark theme, and better keyboard and
  screen-reader support. Signing in a device shows the code large, with Approve and Deny apart.
- **Settings → Account** has the same look, with a Sync (beta) card below it.
- Your profile is upgraded on first start (the database to version 6; the password vault stays
  as it is until you sync passwords). Everything you had stays.

**Beta limits:** you can't yet choose your own passphrase, change it, make a new recovery key,
delete or export your synced data, or sync payment cards. See the README's "Sync (beta)" and
"Known limitations".

**Testing note:** the Windows test run covered the upgrade from 0.8.0, sync on the real build
(set up, approval, restore, passwords, removal) and 39 of the 43 regression flows. Four
media-only flows that 0.9.0 doesn't touch didn't run within the test time: 070-flash, 050-lna,
060-pip and 070-pip.

## 0.8.0 — 2026-11-09

- **Krunk account:** Settings → Account signs the browser in to your Krunk account. Sign in shows
  a code and opens krunk.209.wf, where you approve this device; the card then shows who you are
  signed in as, with Manage account and Sign out on this device. There is no sync yet. Krunk
  sends nothing to the account server until you click Sign in, and keeps the sign-in protected
  for your Windows user in `account.json`. Private windows show nothing of the account.
- **Back and forward history:** right-click Back or Forward for the tab's earlier or later pages
  (up to 15) and jump straight to one; "Show full history" opens the history page. Works with the
  keyboard too.
- **Dropdowns keep the page still:** while a dropdown list, a datalist or one of Krunk's menus is
  open, the mouse wheel no longer scrolls the page underneath (Chrome and Edge close the dropdown
  instead); the wheel over the list scrolls the list.
- **Sites that ask for a password (HTTP authentication):** sites behind nginx `auth_basic` and
  similar now get Krunk's sign-in dialog, filled from your saved passwords for that exact site,
  and a working password is offered for saving. Proxy authentication follows in 0.8.1.

## 0.7.4 — 2026-11-08

- **Switching tabs:** the occasional one-frame white flash is fixed at its source. Krunk hid the
  windows of background tabs, and on some graphics cards a hidden window loses what was drawn
  for it, so showing it again started with one empty (white) frame. Background tabs are now
  kept drawn outside the visible area and moved into place when you switch.
- **Firefox passwords:** profiles whose old 3DES key and newer AES key share one key id now
  import (each field is tried with the key it names, then with the others; 0.7.2/0.7.3 said
  "could not be decrypted: unsupported cipher" for every login of such a profile). AES-128/192
  and AES-GCM fields are read too. If fields still cannot be decrypted, the result names the
  cipher and the key sizes (no secrets) so the cause can be found.
- **New tab page:** space below the keyboard shortcuts.

## 0.7.3 — 2026-11-08

- **Switching tabs:** the white flash between two pages is gone again. The old page now stays
  beneath the new one for a moment, until the engine has drawn the new page (on some graphics
  cards it is a frame late, and the window's background showed in that frame).

## 0.7.2 — 2026-11-08

Thirteen fixes and improvements reported after 0.7.1.

- **Address bar:** a new tab's bar is empty with its placeholder from the first frame (it showed
  `about:blank` for a moment when request headers were edited), keeps the cursor while the new
  tab page loads, and the inline completion no longer disappears and comes back between
  keystrokes.
- **Passwords & payments:** "Add login" and "Add card" are redesigned: labelled fields, focus
  rings, the expiry as one group, an error under the field, show/hide and Generate next to the
  password; light and dark.
- **Resizing:** the window no longer shows white strips while it is resized (the window's own
  surface beneath the engine's views is now painted in the theme's colour).
- **Games:** turning the camera in pointer lock is as even as in Chrome, also while a movement
  key is held. The key was not the cause: Krunk's pointer-lock movement missed a frame now and
  then; real mouse events now carry the engine's own movement for their frame.
- **Bookmarks bar:** bookmarks that do not fit go into a "»" dropdown at the bar's end
  (recalculated on resize and changes; drops on "»" go to the end of the bar); long titles in
  folder dropdowns stay on one line with "…" and a tooltip; the bookmark menu has icons like
  Krunk's other menus and follows the theme also over an open dropdown.
- **Bookmarks page:** long titles and addresses stay on one line with "…"; the page never
  scrolls sideways.
- **Firefox import:** profiles whose `key4.db` holds more than one key (an old 3DES key and a
  newer AES one) import all their logins; when nothing is imported the result says why. If
  Firefox import still shows 0 passwords, please send the `import: passwords` line from
  `krunk.log`.
- **Updates:** a newer version shows a dot on ⋮ and "Update available – Krunk X.Y.Z" at the top
  of the menu, which opens the update prompt; nothing pops up by itself. The first check runs
  ten seconds after start.

## 0.7.1 — 2026-11-08

Follow-ups from the 0.7.0 verification: client hints off reaches everything it can, a clearer
unlock form, the engine's real command line on About, and an evenly paced download animation.

- **Send client hints off, now also in tabs that were already open:** switching the setting off
  removes `navigator.userAgentData` from open pages and their frames at once (also frames of
  other sites), and every engine process started afterwards knows it from its first page.
- **Shared workers:** with client hints off a shared worker's script no longer sees
  `navigator.userAgentData` (Krunk adds one statement in front of it; strict mode and line
  numbers stay). Not reached: modules a module worker imports, and `blob:`/`data:` workers
  (see Known limitations).
- **Passwords & payments:** a wrong master password now stays said on the unlock form, also
  after the short delay before the next try.
- **About:** shows the command line the engine really runs with (CEF's and Krunk's switches
  merged).
- **Downloads:** the download-start animation is paced by the display's refresh.
- **Background tabs:** two ways to have the engine treat them as hidden were tried and do not
  work in CEF's windowed mode; tab sleep and discard stay the remedy (Known limitations).

## 0.7.0 — 2026-11-08

Fixes and improvements from testing 0.6.0: the engine files are checked and repaired, the
installer no longer leaves an old engine behind, the address bar selects like it should, tab
switches never flash white, a recovery phrase for the vault, extensions from the Chrome Web
Store with "Add to Krunk", downloads that rebalance and resume, a volume bar for
Picture-in-Picture, wallpapers and colour pickers.

- **Engine check and Repair:** Krunk knows the hash of every engine file of its release and
  checks the installed ones after start; krunk://gpu and About name the engine. A changed file
  offers **Repair**. The media-codecs text on krunk://gpu now comes from the real probe.
- **Installer:** a silent update no longer skips a file that another program holds (that could
  leave the old engine, without H.264/AAC, under a new Krunk): it moves such files aside or
  stops before changing anything and Krunk says why.
- **Engine switches:** CEF's own disabled features and Krunk's are merged into one
  `--disable-features` (before, only Krunk's took effect).
- **Address bar:** drag to select (focused or not), double/triple click, the site icon drags
  the address; a click on the page leaves the bar; new tabs type into the address bar.
- **No white flash** when switching tabs, also to tabs that are still loading or were discarded.
- **Faster pages:** Krunk's own page scripts and DevTools domains do much less on every page
  (Speedometer: the remaining gap to Chrome is the engine's; see Known limitations).
- **Recovery phrase:** 24 words that open your passwords and cards if you forget the master
  password (krunk://passwords). Shown once, never stored; a password change keeps it valid.
- **Send client hints** (Settings → Advanced → Request headers, on by default): off sends no
  `Sec-CH-UA*` headers and hides `navigator.userAgentData`.
- **Passwords & payments:** a clearer change-password dialog with strength and show/hide;
  "Manage passwords & payments".
- **Extensions:** the Chrome Web Store's button says **Add to Krunk**; every install checks the
  package's signatures and asks first, with what the extension can do. Extension popups close
  when you click elsewhere. krunk://extensions has cards.
- **Downloads:** a slow connection no longer holds up a download in parts (finished
  connections take over its work); downloads in parts continue after Krunk was closed; the
  downloads popup and page were redone (file icons, smoother bars, empty state).
- **Picture-in-Picture:** a volume bar under the floating window, and the mouse wheel over it.
- **New tab page and appearance:** tiles keep their width with long titles; ten built-in
  wallpapers; colour pickers for accent, background, toolbar, tab strip and new tab page.
- **Firefox import:** reads logins without a `hostname` and says why entries were skipped.
- **Polish:** links in Krunk's pages, permissions page spacing, the login suggestion popup,
  the task manager's selected row in dark mode.

## 0.6.0 — 2026-11-07

Extensions, Picture-in-Picture, protected content, payment cards, generated passwords,
custom DNS, request headers, tracking protection levels, site permissions, and the website
knows Krunk.

- **Extensions** (krunk://extensions): Krunk's own subset of Chrome's Manifest V3 — content
  scripts in an isolated world, a background page, the toolbar button with its popup and
  badge, options pages, `runtime`/`storage`/`tabs`/`action`/`alarms`/`i18n`/`scripting`, static
  `declarativeNetRequest` rules in the ad blocker. Install from a zip/crx, a folder or a Chrome
  Web Store address. The page lists what each extension may reach and which APIs Krunk does
  not provide (blocking `webRequest`, `debugger`, native messaging, dynamic rules, and more).
  Extensions never run on Krunk's own pages, and not in private tabs unless allowed.
- **Picture-in-Picture:** a button in the address bar, the menu item and Alt+P; a chooser for
  pages with several videos; "Automatic Picture-in-Picture" on tab switches (Settings →
  General → Media).
- **Protected content (Widevine):** Settings → Privacy & security, off by default; the engine
  downloads Google's module into the profile. Services that insist on a verified player
  (Netflix, Amazon, Disney+, Spotify's web player) still refuse: Krunk is not VMP-signed.
- **Payment methods:** the vault keeps cards (never the security code), fills checkout forms
  after an unlock, offers to save a card typed at a checkout.
- **Generated passwords** for sign-up forms, saved on submit with Undo/Edit.
- **Custom DNS** (Settings → Privacy → DNS): DoH, DoT, DoQ, plain and `sdns://` stamps in
  order, through Krunk's loopback stub, never the system resolver; per-resolver Test.
- **Request headers** (Settings → Advanced): Accept-Language, User-Agent client hints,
  custom header rules; read-only headers explained; Reset.
- **Tracking protection** levels Standard, Strict, Custom (`docs/TRACKING_PROTECTION.md`):
  tracker lists, third-party cookies, parameter stripping, referrer trimming, bounce-tracking
  cleanup, exceptions.
- **Site permissions:** camera, microphone, notifications, location, clipboard, local network
  and pointer lock each ask once; Settings → Permissions; the lock popup; in-use chips.
- **Also:** live window resizing; inline address completion and an address-bar context menu;
  a title-bar menu; krunk://tasks with PIDs; a speaker icon and Mute site; new-tab settings;
  a download animation, speeds and segments; new-tab shortcuts, backgrounds and sections;
  appearance options (accent, colours, density, font, tab shape, toolbar items); cookie and
  site-data management by size; no page-visible page bridge any more.
- **Website:** krunk.209.wf tells Krunk users whether they are up to date.
- **Package:** the installer removes a stale `v8_context_snapshot.bin` from older installs.
  Krunk.dll grew from 8.3 to about 11.6 MB (the DNS and extension code).

Updating keeps your data. The 0.5.x notes apply otherwise.

## 0.5.2 — 2026-11-06

H.264 and AAC play.

- **Engine:** Krunk now ships its own build of the engine, CEF 156.0.15 (Chromium
  156.0.8078.80, the same versions as before) with the proprietary codecs compiled in. MP4
  H.264 video and AAC audio, HLS and DASH streams built on them (Plex, most video sites' MP4
  fallbacks) play. VP8, VP9, AV1, Opus, Vorbis, FLAC and MP3 play as before.
- **Still conditional:** HEVC (H.265) is decoded by the graphics hardware only, so it works on
  a PC whose GPU decodes HEVC and not elsewhere; AC-3/E-AC-3 (Dolby Digital) needs Windows'
  own Dolby decoder, which Windows 11 24H2 and later do not ship. Widevine (DRM) is not
  included.
- **Package:** the same size as 0.5.1 (installer about 137 MB); the update downloads the whole
  package as before.

Everything else is as in 0.5.1 and 0.5.0 (see their notes). Updating keeps your data.

## 0.5.1 — 2026-11-06

Suggestions under the address bar.

- **Address bar:** as you type, a dropdown offers the search (or the typed address) first, then
  the bookmarks and history pages whose title or address matches, most visited first. Down and
  Up move through the rows and show their address in the bar, Enter or a click opens the row,
  Esc closes the list and keeps what you typed. The bar keeps the focus the whole time.

Everything else is as in 0.5.0 (see its notes). Updating keeps your data.

## 0.5.0 — 2026-11-06

Passwords offered where you type them, import from your old browser, a Settings page with tabs
and search, and the fixes for Plex-style home servers.

- **Passwords:**
  - Click into a login field and Krunk offers the matching logins right there: the site's own
    first, then logins of the same domain (https only). Choosing one fills the form. With the
    vault locked the list offers **Unlock Krunk passwords**, which asks for the master password
    once per session.
  - **Import** history, bookmarks and passwords from Firefox, Google Chrome, Microsoft Edge and
    Brave (Settings → Passwords & import), or passwords from a CSV export. Nothing is imported
    twice. Chrome and Edge passwords saved by Chrome 127 or newer are protected so that only
    Chrome can read them; Krunk says how many and offers the CSV route.
- **Settings** has tabs (General, Privacy & security, Passwords & import, Downloads, Appearance,
  Advanced), a search box and links that open a tab directly (`krunk://settings#privacy`).
  Advanced shows the browser's own user-agent string (with Copy) and offers Chrome, Edge,
  Firefox and Safari presets or a custom string; it applies at once.
- **Local network access:** sites that want to reach a device on your network or this computer
  (a Plex or Jellyfin server, a NAS, a printer) now get a prompt with Allow and Block, remembered
  for the site; before, such requests hung. Stored answers can be removed on the site's Cookies
  and site data page. Private tabs ask every time.
- **Address bar and bookmarks bar:**
  - The lock (or the "i") opens a site-information popup: connection, certificate chain with
    fingerprints, and what is wrong with a bad certificate.
  - Drag the site icon or the address onto the bookmarks bar, or into one of its folders.
  - Right-click an item in a folder's dropdown for its menu; drag items within a folder, into a
    subfolder, or out onto the bar.
  - The ⋮ menu, the shield and the lock close again when their button is clicked a second time.
- **Tabs:** dragging a tab to reorder it is smooth: the tab follows the mouse and the others
  slide aside (no animation with "reduce motion" on).
- **Overlays:** the address of a link under the mouse shows at the bottom of the page; the "Press
  Esc" hint of pointer lock is a small pill shown at most three times per tab; docked DevTools
  has a strip with Close and dock buttons.
- **`krunk://gpu`** shows whether pages are hardware accelerated, the WebGL/WebGPU adapter, the
  engine's switches and the GPU process, and links to Chromium's own `chrome://gpu`.
- **Removed:** the Home button (Alt+Home still goes to the home page). The address bar no longer
  shows stale text after a navigation.
- **Media codecs:** the bundled engine has no H.264/HEVC/AAC/AC-3 decoders (VP8/VP9/AV1/Opus/
  Vorbis/FLAC/MP3 play). Plex and other H.264 streams need a codec-enabled engine build; see the
  README.

Updating from 0.2.0 or later keeps your data; 0.4.0 settings files load as they are.

## 0.4.0 — 2026-11-06

Faster downloads, an ad blocker you can see into and edit, a bookmarks bar with a menu, and a
hex editor for site data.

- **Ad blocking page** (`krunk://adblock`, or the shield's "Show all"):
  - **Blocked on this page** lists every request the ad blocker stopped, with the filter that
    matched and the list it came from. The shield panel shows the latest ones.
  - **Filter lists** shows each list's address, filter count and last update. Turn lists on and
    off, add your own by https address, remove them, or restore the defaults.
  - **My filters** takes your own rules in Adblock Plus / uBlock Origin syntax.
  - Changes apply within seconds, without a restart.
- **Downloads:**
  - Large files download over several connections when the server allows it (Settings →
    Downloads, 4 by default, 1 turns it off).
  - The downloads page shows the full address, the page the file came from, and the date and
    time it started and finished. Right-click a download to open it, show it in its folder,
    copy the link or page address, retry, pause, cancel, remove it from the list or delete the
    file.
  - The download icon and Ctrl+J switch to an open downloads page instead of opening another.
    History, bookmarks and settings do the same.
- **Bookmarks bar:**
  - Right-click it to open, edit or delete bookmarks, make folders, and choose when it shows:
    always, only on the new-tab page, or never (Ctrl+Shift+B toggles).
  - Drag bookmarks to reorder them or into folders.
- **Tabs:**
  - Right-click a tab for **Close other tabs** and **Close tabs to the right**.
  - Closing a tab is animated, and the next tab's close button stays under the mouse.
  - A tab dragged onto another window now lands in front, not behind it.
- **Cookies and site data:**
  - Values can be edited in **hex**, as UTF-8, a binary string, UTF-16, Base64 or
    percent-encoding. Binary values open in hex.
  - Cookies can now be edited too.
- **Games:** turning with the mouse while holding mouse buttons and keys (walking and strafing)
  is smooth again.

## 0.3.0 — 2026-11-05

Tabs in as many windows as you like, and DevTools inside the tab's window.

- **Windows:** drag a tab out of the tab row and it becomes its own window; drag it onto another
  window's tab row and it lands where the marker shows. The page doesn't reload: scroll
  position, typed text, history, playing video and private mode all come along. Dragging a
  window's only tab onto another window merges the two.
- **New window:** Ctrl+N, the main menu, or right-click a tab → New window / Move to new window.
- **Closing:** closing a window's last tab now closes that window, and closing the last window
  quits Krunk. Ctrl+Shift+T reopens a closed tab in the current window.
- **Session:** every window comes back with its tabs, position, size and maximized state. A
  window from a monitor that is gone is moved back on screen. Your 0.2.x session opens as one
  window.
- **DevTools** (F12, Ctrl+Shift+I, Inspect) is docked inside the tab's window, on the right or at
  the bottom, with a splitter to resize it; F12 again closes it. Each tab has its own DevTools,
  which hides when you switch tabs and moves with its tab. Krunk's shortcuts keep working in
  DevTools, apart from the keys DevTools needs itself (such as Ctrl+F).
- Tapping Alt alone no longer swallows the next shortcut.
- The download and the installed folder no longer contain Markdown files: the installer removes
  the `README.md` and `SECURITY.md` that 0.2.x left behind.

## 0.2.1 — 2026-11-05

The first release published on Krunk's website, https://krunk.209.wf.

- The update check now asks https://krunk.209.wf, so this and later versions update themselves
  (Settings → About; Update now installs silently and restarts Krunk). Krunk 0.2.0 pointed at a
  placeholder server: install 0.2.1 over it once by hand.
- Update-check errors are readable ("the server name could not be resolved" instead of a code).
- Everything from 0.2.0: the new name and icon, the frameless window, the cookie and site-data
  manager, Inspect, YouTube ad blocking, pointer lock and full screen for games, and the
  per-user installer.

## 0.2.0 — 2026-11-04

The first release under the name Krunk (formerly LightBrowser).

- New name, icon and data folder (`%LOCALAPPDATA%\Krunk`); LightBrowser profiles are moved over.
- Frameless window with Windows 11 snap layouts; wider tabs; new tabs focus the address bar.
- Cookie and site-data manager (`krunk://storage`), per-site storage editor, Inspect element.
- uBlock Origin scriptlets and redirect resources: ad blocking on YouTube.
- Pointer lock and full screen for browser games (F11).
- Per-user installer and a signed, daily update check.
