# Krunk for Android changelog

What changed in each Krunk for Android release, newest first. The Windows browser's list is
`CHANGELOG.md`.

## 0.3.0 — 2026-11-14

The password vault. It installs over 0.2.0 and keeps tabs, bookmarks, history and settings.

- **Password vault.** Krunk can keep the user names and passwords you sign in with, encrypted on
  the device behind one vault password that only you know (at least 8 characters). The
  encryption and the file format are those of the Windows browser: Argon2id turns the password
  into the key, XChaCha20-Poly1305 seals every login, and the names of the sites are encrypted
  too. On Android the vault key is wrapped once more by a key in the Android Keystore that cannot
  leave the device (StrongBox when the device has one, otherwise the device's secure hardware).
  In the app, wrong passwords slow the next attempt down, up to a minute.
  - What the device key protects: the saved logins. A copy of the vault file cannot be decrypted
    on another device, not even with the right password.
  - What it does not protect: the vault password against guessing. Someone who has a copy of the
    file can try passwords against it on their own computer; only Argon2id slows that down
    (64 MiB of memory and 3 passes for every guess). Choose a long vault password.
- **Passwords screen** (⋮ menu › Passwords, or Settings › Privacy › Passwords): create the vault,
  unlock it, search the logins, view one (the password stays hidden until you tap Show and hides
  itself again after 30 seconds), copy the user name or the password, add, edit and delete logins
  (delete asks first), Change password and Lock now. A copied password is marked as sensitive, so
  Android 13 and later keep it out of the clipboard preview, and Krunk takes it off the clipboard
  after 30 seconds. While Krunk is on screen it does so only if the clipboard still holds that
  password; once you have left Krunk it cannot look (Android lets only the app on screen read
  the clipboard), so it clears the clipboard whatever is on it. A keyboard that keeps a clipboard
  history may have its own copy.
- **Locking.** The vault locks:
  - when you choose Lock now;
  - after a time without vault use: 1, 5, 15, 30 or 60 minutes, 15 by default ("Lock after");
  - after you leave Krunk: at once, after 30 seconds, after 1 minute (the default) or after 5
    minutes ("Lock after leaving Krunk"). The time is there so that you can switch to another
    app, for example an authenticator app for a sign-in code, and come back without unlocking
    again. It also means the vault stays open for that long after you leave Krunk; choose
    "At once" to close that;
  - always when Krunk's process ends: a newly started Krunk has a locked vault.
- **Saving and filling in pages.**
  - After you sign in on a site, Krunk offers to save the login, or to update it when the
    password changed; a login that is already saved is not offered again. "Never for this site"
    stops the offers for one site, "Offer to save sign-ins" in the vault's settings switches them
    off altogether. Without a vault the offer leads to "Create vault" and the login is saved into
    the new vault; "Don't ask again" there switches the offers off.
  - Tap a sign-in field on a site with saved logins and Krunk shows them to choose from. Krunk
    never fills a form by itself, and the engine does not hold your passwords: it gets the one
    login you picked, at the moment you pick it.
  - A locked vault stays quiet while pages load. The vault password is asked when you tap the
    field of a site that has logins, or when you choose Save.
  - "Fill from vault" in the ⋮ menu brings the choice back on a page where you closed it.
- **Private tabs:** nothing is offered by itself and there is never an offer to save. "Fill from
  vault" in the ⋮ menu is the explicit way: after it, a tap on the sign-in field shows the saved
  logins.
- **Pages without encryption (http):** before a password is filled in or saved there, Krunk shows
  a warning that you have to confirm. A login saved on an https page is never offered to an http
  page.
- **Sign-in forms inside a frame of another site are not filled.** The login belongs to the site
  in the address bar; a form that another site embedded gets nothing. This is stricter than the
  Windows browser, which offers such a form the logins of the frame's own site.
- **HTTP sign-in dialog:** it lists the saved logins of that server ("Use …"), offers "Unlock
  vault for saved sign-ins" when the vault is locked, and Krunk offers to save after a sign-in
  that worked. Proxies get no offer.
- **Fingerprint unlock** (optional, off by default, Android 11 and later): "Unlock with
  fingerprint" in the vault's settings. The vault password always works and is not stored for
  this. Enrolling a new fingerprint on the device switches it off, and so does changing the
  vault password; switch it on again afterwards.
- **Screens of the vault are blank in screenshots, screen recordings and Recents:** the Passwords
  screen, the unlock dialog, the choice of logins and the save offer.
- **One new permission:** use biometrics (for fingerprint unlock; Android does not ask for it).

What the vault on Android does not have in this version:

- **No recovery phrase. A forgotten vault password cannot be recovered**; the create and unlock
  screens say so.
- No payment cards, no sync, no import or export of logins, no password generator, and Krunk is
  not a system-wide Autofill service: it fills in its own pages only.
- **The vault stays on this device.** It is not in Android's cloud backup and not in the transfer
  to a new device, and its file cannot be opened anywhere else, because the Keystore key does not
  leave the device. A vault file from the Windows browser does not open on Android, and one from
  Android does not open on Windows, in this version.
- **When the device's key is gone.** If Android's key store reports the key as missing or no
  longer valid (app data restored from elsewhere, key store reset), Krunk says "This vault cannot
  be opened", asks for no password and offers to delete the vault behind a typed confirmation; it
  never replaces a vault silently. If the key store merely does not answer, Krunk says so and
  asks you to try again; the vault is not treated as lost.

Tested: on the emulator only (Android 16, x86_64). **Fingerprint unlock was not run on a device:**
no fingerprint can be simulated on the test emulator, so switching it on, unlocking, a new
enrolment and the fall-back to the password are covered by unit tests with stand-ins only; the
one device check is that the switch is disabled with its reason when no fingerprint is enrolled.
**StrongBox was not run on a device either:** the emulator has none and used the other Keystore
path; the StrongBox-first choice is covered by unit tests with a stand-in. Copying to the
clipboard and its timed clearing are covered by unit tests only.

<!-- M7: release-pass results and known issues are added here by the lead -->

## 0.2.0 — 2026-11-12

Ad and tracker blocking, a quieter engine, a wider address bar and themes. It installs over 0.1.0
and 0.1.1 and keeps tabs, bookmarks, history and settings.

- **Fixed: 0.1.0 and 0.1.1 contacted Mozilla and Google in the background.** Without anyone
  asking, the engine fetched Mozilla's Remote Settings (certificate data, lists) and ran a
  connectivity check, and it asked Google's Safe Browsing service for list updates although Safe
  Browsing was set to "off". Krunk promises no background traffic, so this was a bug. 0.2.0
  switches all three off and checks it with a packet capture: after a fresh install Krunk sends
  nothing until you open a page.
  - What that costs: the engine no longer receives updates of certificate revocation data (CRLite)
    or of preloaded intermediate certificates, and there is no Safe Browsing warning page. A
    certificate revoked after this version of Krunk was built is not known to it.
  - Settings › Privacy › "Update automatically" (off by default) turns the daily filter-list
    updates and Mozilla's Remote Settings on together, from the next start. Safe Browsing and the
    connectivity check stay off in every case.
- **Ad and tracker blocking.** uBlock Origin 1.76.0 is built in, unmodified, as an engine
  extension, with the filter lists that come inside it (EasyList, EasyPrivacy, uBlock Origin's own
  lists, Peter Lowe's list, the malicious-URL list). It blocks requests and hides the leftovers on
  the page, in normal and private tabs.
  - The **shield** in the address bar shows how many requests were blocked on the page. Tap it to
    switch blocking off for that site; the choice is kept. In a private tab the sheet shows the
    count and no switch, so a private visit leaves no entry.
  - An address that is itself on a list shows Krunk's page with "Go back" and "Open once".
  - Filter lists are not updated behind your back: Settings › Privacy › "Update filter lists"
    fetches them when you ask (from the lists' own hosts), or "Update automatically" does it daily.
  - When Krunk starts, pages wait a few seconds until the blocker is ready, so the first page is
    filtered too. If it is not ready after 10 seconds the page loads and the shield says so.
  - Blocking can be switched off as a whole in Settings › Privacy.
- **Tracking parameters** (`utm_…`, `fbclid`, `gclid` and the rest of Krunk's list) are now also
  removed from links you tap inside a page and from redirects, not only from addresses you type.
  Forms that are sent (POST) are never changed.
- **Toolbar.** Back and Forward moved into the ⋮ menu, as its first row together with Home; a
  long press on either lists the pages to go to. The address bar takes the room: about two
  thirds of the toolbar on a small phone. Android's back gesture still goes back in the page's
  history. Reload and stop have a full-size touch target.
- **Themes.** Settings › Appearance › Style offers the twelve styles of the Windows browser, each
  with a small preview, and Accent colour seven colours or any other. A style sets the toolbar's
  fill (plain, gradient, tinted), the corners of the address bar and the tab cards, and how the
  current tab is marked in the switcher. Light, dark and "follow the system" work as before, and
  private tabs keep their own dark look in every style. Text and icons are checked for contrast in
  every style. Without a style nothing changes, except that the current tab's card now has a
  filled title instead of a border.
- **Pages can ask for more:** files (Android's file picker, one or several, by type; a field that
  asks for a photo can use the camera app), dates, times, weeks and months, colours, "leave this
  page?", "send the form again?" and sharing through Android's share sheet. Krunk's own dialogs
  name the site that asks; closing any of them answers the page, so none hangs.
- **Links:** mail and phone links open Android's mail and phone apps (after a tap at once,
  otherwise Krunk asks first). Selected text can be searched, and a selected address opened, from
  the menu over the selection. Settings › Tabs › "Tabs opened by links" decides whether such tabs
  come to the front.
- **Tabs:** a long press on a tab in the switcher offers Duplicate, Close other tabs and Close
  tab. "Request desktop site" now stays with the tab when Android or a crash made Krunk rebuild
  the page, and over a restart.
- **About** lists uBlock Origin and its filter lists with their licences.

Known limits: custom filter lists and protection levels come later; a file you give to a page is
copied inside Krunk's storage for the upload (removed when the tab closes); whole folders cannot
be uploaded; moving tabs in the switcher is not there yet. Each APK is about 7 MB larger.

Known issues:

- **Address bar empty although a page is shown** (seen once in testing and not again when the same
  steps were repeated, once by script and once by hand; to be looked at for 0.2.1). What was done: Settings › Privacy › "Block ads and
  trackers" switched off, back to the browser, and about four seconds later a link opened from
  another app. The page loaded, unfiltered as set, but the address bar showed "Search … or enter
  address" and no shield instead of the site. Nothing wrong was shown; the site's name was
  missing.
- **"Open once" lasts a little longer than the tab.** When you open a blocked address once, the
  blocker remembers that for up to about two minutes, in memory only. This also holds for a
  private tab: for that time after the tab is closed the same address opens in a normal tab too.
  Nothing is stored, Settings lists nothing, and a restart ends it at once.

## 0.1.1 — 2026-11-11

A fix release after a full review of the 0.1.0 code, with one changed default. It installs over
0.1.0 and keeps tabs, bookmarks, history and settings.

- **The toolbar is at the top by default.** A new install has the address bar at the top. An
  install that has already saved a setting keeps the position it stored; one that never changed
  any setting moves to the top with this update. Settings › Appearance › Toolbar position
  switches between Top and Bottom.
- **Address bar:** it shows the site you are on (the host, with its end always visible) instead
  of the start of a long address. On phones "not secure" is the red sign alone and the Home
  button appears from tablet width, so the site's name fits. The lock is shown only for the page
  that is really on screen, never for an address that is still loading. The toolbar comes back
  whenever a page loads, so a hidden address bar cannot outlive a navigation or get stuck on a
  page that does not scroll. Bottom-fixed parts of a page (cookie bars, site navigation) are no
  longer covered or left floating while the toolbar hides.
- **Pages can ask:** drop-down lists (`<select>`), alert, confirm and prompt dialogs and HTTP
  sign-in now work (0.1.0 showed none of them). Each dialog names the site that asks, and from
  the second dialog of a page on you can block the rest. Links that open a new window
  (`target=_blank`, `window.open`) open a new tab.
- **Security:**
  - Addresses handed over by other apps and links opened from a page's link menu are limited to
    web addresses and are loaded like any link on a web page.
  - A new window shows no address until it has really loaded something, and a web page cannot
    put a `krunk://` address into the address bar.
  - Krunk asks before a page-started download (file name and site), and file names are cleaned
    of characters that hide or reorder them.
  - Full screen says which site entered it and how to leave.
  - Site icons are fetched with size and time limits and checked before they are decoded.
- **Tabs and restore:** a restore ends on the page you left, with its back and forward list and
  scroll position, also when you left right after a navigation or the app was closed seconds
  after starting. A page whose process crashed, or was closed by Android to free memory, shows
  "This page stopped working" with Reload instead of a dead blank view. "Restore tabs?" keeps
  the session until you answer, also across a restart; after two failed starts in a row Krunk
  asks before restoring. Tabs survive Android closing Krunk in the background whatever the
  start-up setting. "Close all" no longer loads background tabs on the way.
- **Privacy:** private tabs stay out of the Recents thumbnail and screenshots, and one private
  tab is kept alive under memory pressure so private sign-ins are not lost. "Clear browsing
  history" also clears site icons and the recently closed list. Nothing of Krunk goes into
  Android's cloud backup any more (the direct device-to-device transfer still carries your
  data). The release build no longer writes page addresses into the system log. Krunk's own
  requests (site icons, search suggestions) use the browser's user agent, not the device's.
- **Settings:** a custom search engine and a custom new-tab address can be turned on; the
  user-agent setting is applied; "Strip tracking parameters" off survives a restart, and
  tracking parameters are also removed from links other apps hand over and from the link menu.
- **History:** visits are stored under their own page's title, and addresses that only
  redirect are not listed.
- **Smaller fixes:** a database that cannot be opened no longer stops Krunk from starting; find
  in page belongs to its tab and shows its match count also when typed fast; the address editor no longer shows a suggestion from the last
  time; mail and phone links can be copied from the link menu; a cancelled history page load no
  longer ends the list; settings dialogs scroll.
- Known limits, unchanged: downloads are handed to Android without the page's sign-in; file,
  date and colour pickers and the "leave page?" question are not there yet; tracking parameters
  are not removed from links clicked inside a page.
- Internals: page state, load policy, session start-up and prompts are pure, unit-tested code
  (75 JVM and 39 Rust tests for the Android side), session rows are written in one transaction,
  the tab list is no longer rebuilt on every engine callback, the release build refuses a debug
  signature, and the publish script checks version and package of every APK.

## 0.1.0 — 2026-11-11

The first Krunk for Android preview (`android/`, `ffi/`), sharing the Rust core with the Windows
browser (core 0.15.3). It is a separate app with its own version; see the README section "Krunk
for Android (preview)" and `docs/ANDROID_FEATURES.md`. Android 8 or later, `arm64-v8a` phones
and tablets; the APK is signed with Krunk's Android release key and installs from the website
("install unknown apps" must be allowed for the browser you download it with).

- **Engine:** GeckoView 160, pinned; `arm64-v8a` devices and the `x86_64` emulator; Android 8+.
- **Browsing:** address bar with Krunk's URL-or-search rules and DuckDuckGo by default, history,
  bookmark and engine suggestions, automatic HTTPS, HTTPS-only mode, tracking parameters removed,
  back/forward/reload/stop/home, progress, title and favicon, lock / "Not secure", Krunk's error
  page, pull-to-refresh, find in page, request desktop site, pinch zoom and text size, full
  screen, share, link long-press menu (new tab, private tab, copy, share, save image, open image).
- **Tabs:** switcher grid with thumbnails, swipe to close, close all, search; private tabs in
  their own section with their own theme and no history; recently closed; tabs survive restarts
  and process death, with a "Restore tabs" prompt after a crash; background tabs sleep and can be
  discarded (the Windows policy).
- **New tab page:** most visited sites, shortcuts (pin, edit, remove), recently closed, the ten
  built-in wallpapers or a colour, light and dark.
- **Bookmarks and history:** manager with folders and search, Netscape HTML import and export,
  history by day with search, delete and clear.
- **Settings:** search engine and custom engine, homepage, start-up, new-tab page, theme, toolbar
  at the bottom or the top, text size, force zoom, suggestions, HTTPS-only, tab sleep/discard,
  new-tab background, user agent, clear browsing data; About with versions and licences.
- **Shell:** bottom toolbar that hides while scrolling, light/dark/system, edge-to-edge, gesture
  and three-button navigation, predictive back, landscape and tablet layouts, 48 dp targets and
  content descriptions for TalkBack.
- Not in this preview: blocking, vault, downloads manager, permissions, media controls, sync,
  VPN, agent, extensions, updates, release signing (the matrix lists each with its round).
