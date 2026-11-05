# Manual test checklist (Windows 10/11 x64)

Run these on a real Windows 10 or 11 x64 machine with `Krunk-Setup-<version>.exe` or the release
folder `Krunk-win-x64` (from the zip). Sections 1–3 are the release smoke test; "Krunk 0.2"
covers the round-2 features (installer, updates, window, storage, YouTube, games) and
"Krunk 0.3" the windows, tab dragging and docked DevTools. The remaining
sections cover each feature in detail. Last full pass: 2026-11-04 on a Windows 11 Pro VM (build
26200), CEF 156.0.15 / Chromium 156.0.8078.80, driven with `scripts/vm.sh` (screenshots in the
pipeline workspace under `screenshots/cef/`).

## 1. Install and first run
1. Check the hash: `Get-FileHash Krunk-win-x64.zip` matches `SHA256SUMS`; extract it.
2. **Missing engine files:** rename `libcef.dll` (or delete `locales\en-US.pak`) and start
   `Krunk.exe`. Expected: a "Krunk cannot start because some of its files are
   missing or damaged" message box that names the file. No crash and no window. Rename it
   back.
3. **First start:**
   - The window opens with one new tab and the address bar focused.
   - `%LOCALAPPDATA%\Krunk\` is created with `browser.db`, `engine.dat`, `profile\`
     (with `default\`), `krunk.log` and `cef.log`. No `debug.log` appears next to the exe.
   - `krunk://about` shows Chromium 156.0.8078.80 and CEF 156.0.15.
   - The log shows "engine initialized: CEF …, Chromium …" and "filter engine loaded from
     cache" (on the second start).
4. **Processes and engine:** all processes are `Krunk.exe` (the browser plus the
   engine's subprocesses). There are no services and no scheduled tasks.
   - `scripts/vm-engine-check.ps1`: every process has `libcef.dll`; none has
     `WebView2Loader.dll` or `EmbeddedBrowserWebView.dll`;
     `msedgewebview2_started_by_krunk=0`.
   - `scripts/vm-sandbox.ps1`: every renderer is Untrusted (or Low) with a restricted token,
     and no process has `--no-sandbox`.
   - Task Manager → Details → Description shows "Krunk web browser".
5. **Upgrade from the WebView2 version:** with an old data folder (with `profile\EBWebView`), the
   settings, history, bookmarks, session and vault are all there. `profile\EBWebView` is left as
   it was.

## 2. Release screenshot set (AC4)
Take these with the final build. The reference set from the 2026-11-03 pass is in the pipeline
workspace under `screenshots/final/`.

| Shot | How |
|------|-----|
| `final-newtab` | Ctrl+T after browsing a few sites: tiles with favicons, address bar focused |
| `final-tabs` | 10+ tabs open, a bookmarked page (filled star), bookmarks bar |
| `final-shield` | A news site (theguardian.com, about 13 blocked), then click the shield: popover with the count and "Allow ads on this site" |
| `final-save-password` | `https://the-internet.herokuapp.com/login`, sign in as tomsmith / SuperSecretPassword!: "Save password?" popup |
| `final-passwords` | After "Save" and creating the vault: the entry on krunk://passwords, masked |
| `final-settings` | krunk://settings |
| `final-history` | Ctrl+H: today's visits with favicons |
| `final-bookmarks` | Ctrl+Shift+O: the bookmarks bar folder with entries |

After the save-password shot, close the browser and run
`scripts/vm-leak-scan.ps1 -Needles 'SuperSecretPassword','<your master password>'`.
Expected: `hits=0`.

## 3. Memory figures
Follow "Memory (M8)" below and record:
- 1 tab
- 10 sites awake
- 10 sites after sleep (and after the second stage)
- after discard

The 2026-11-04 figures are in the README under "Memory".

## Tabs and shell (M3)
1. Ctrl+T opens a tab. Ctrl+W closes it. Closing the last tab leaves a fresh new-tab page.
   Ctrl+Shift+T reopens closed tabs in order, with their URLs.
2. Click tabs to switch. Ctrl+Tab and Ctrl+Shift+Tab cycle. Ctrl+1…8 pick by position and
   Ctrl+9 picks the last tab.
3. Drag a tab to a new position: the order sticks and is kept after a restart.
4. Double-click a tab (not on its ×): a duplicate opens next to it.
5. Titles and favicons update as pages load. A spinner shows while a tab is loading.
6. A link with `target=_blank` or `window.open` opens a new tab next to the opener.
7. **Private tabs:** Ctrl+Shift+N opens a tab with the private badge. Log in to a site there:
   the normal tabs are not logged in. Close all private tabs and open a new one: you are
   logged out.
8. **Address bar:**
   - `example.com` → `https://example.com/`.
   - `rust borrow checker` → a DuckDuckGo search.
   - `krunk://history` opens History.
   - `javascript:alert(1)` typed there becomes a search; it is never run.
9. A web page can't navigate to `https://krunk.invalid/settings.html` or
   `https://shell.krunk.invalid/shell.html`. The navigation is blocked, and the log has
   "blocked navigation to an internal or unsupported URL".
10. Every shortcut in the README's Usage table works with focus in the page, in the address
    bar, and after clicking the title bar.

## Popup window
1. At 100 % display scaling, open the ⋮ menu. It appears directly under the button, right-aligned,
   fully on screen, with all items visible (down to "Exit").
2. Set Windows display scaling to 150 % (Settings → System → Display → Scale) and restart
   Krunk. Repeat step 1: the popup is still aligned under the button and is not cut off.
3. Put the window near the right and bottom edges of the screen and open the menu. The popup
   stays inside the monitor work area. Repeat on a second monitor if you have one, including a
   monitor to the left of the primary one.
4. Dismissal: with the menu open, check each of these closes it:
   - Escape
   - clicking in the page
   - clicking in the address bar
   - moving the window
   - resizing it
   - minimizing it
   - Ctrl+Tab
   - closing the tab
   - Alt+Tab to another app
5. Shield panel: it opens with keyboard focus (Tab reaches its button, Enter activates it).
   Clicking the page closes it.
6. Password prompt after a tab switch (once the vault exists):
   - Trigger "Save password?" on site A, then press Ctrl+Tab. The prompt disappears and nothing
     is saved.
   - Trigger it again, switch to another tab and back, and confirm the prompt is gone and
     nothing was saved.
   - Fill prompt on site A: navigate the tab to site B before choosing an account. Nothing is
     filled.

## History (M4)
1. Visit 3–4 sites, for example `example.com`, `en.wikipedia.org` and `www.rust-lang.org`.
   Open History (Ctrl+H). Each visit appears under "Today" with its title and favicon.
2. Visit `https://example.com/?utm_source=test&id=1`. History shows `https://example.com/?id=1`
   (the tracking parameter is removed).
3. Open a private tab (Ctrl+Shift+N) and visit `https://www.mozilla.org/`. It does **not**
   appear in History.
4. Built-in pages (krunk://settings, the new-tab page) never appear in History.
5. Type in the search box: results filter as you type. Hover a row, click ×, and the visit
   disappears.
6. Choose "Last hour" and click Delete, then confirm. The visits from the last hour are gone.
7. "Clear browsing data…": tick Cookies and Site data, choose "All time", and click Clear.
   - Then reload a site where you were logged in: you are logged out.
   - With "Cached images and files", the next load of a heavy site is slower (it is fetched
     again).

## Bookmarks (M4)
1. On a site, click ☆ or press Ctrl+D. The star fills and the site appears on the bookmarks bar.
   Press Ctrl+D again: the star clears and the bookmark is removed.
2. Switch tabs: the star always reflects the active tab's URL.
3. Open the bookmarks manager (Ctrl+Shift+O):
   - Create a folder, then drag a bookmark onto it in the tree.
   - Edit a bookmark (rename it and move it to another folder with the dialog).
   - Delete one.
   - Search for one.
4. Folders on the bookmarks bar open as a dropdown under the button. Subfolders open in place.
   Clicking an entry opens it, and middle-click opens it in a new tab. The dropdown closes when
   you click elsewhere.
5. Import: export bookmarks from Chrome or Edge (`bookmarks_*.html`), then use Import… in the
   manager. A native Open dialog appears, and the toolbar folder lands on the bookmarks bar.
6. Export: Export… opens a native Save dialog. Re-importing the exported file into a fresh
   profile gives the same tree.

## New-tab page (M4)
1. After browsing a few sites several times, a new tab (Ctrl+T) shows up to 12 tiles with their
   favicons. The sites visited most often and most recently come first.
2. Hover a tile and click the pin: it moves to the front and survives restarts. Click × on a
   tile: it disappears and does not come back.
3. Network check:
   - Open DevTools on the new-tab page (F12 on that tab → Network) and reload it.
   - Every request goes to `krunk.invalid` (HTML, CSS, JS, `/favicon?host=`). None goes
     to the internet.
4. The search box uses the configured search engine (DuckDuckGo by default).

## Basics (M4)
1. **Downloads:**
   - Download a file. The downloads button appears in the toolbar with a badge.
   - Ctrl+J shows progress. When it finishes, "Open file" opens it and "Show in folder" opens
     Explorer with the file selected.
   - Cancel a large download partway through: it is marked Cancelled.
   - After a restart the list is still there, but downloads from private tabs are not.
2. **Find in page:**
   - Ctrl+F opens the find bar in the toolbar. Typing highlights all matches and shows "n/total".
   - Enter, F3 and Ctrl+G go to the next match; Shift+Enter and Shift+F3 go back. With no
     matches the counter turns red.
   - Escape closes the bar. Switching tabs also closes it and clears the highlights.
3. **Zoom:** Ctrl+= or Ctrl++ zooms in, Ctrl+- zooms out, Ctrl+0 resets. The zoom chip (for
   example "125%") shows in the address bar while zoomed, and clicking it resets. The ⋮ menu
   zoom buttons do the same.
4. **Print and DevTools:** Ctrl+P opens the browser print dialog, and F12 opens DevTools for the
   page.
5. **Session restore:**
   - Open 5 tabs, then close the window. Start Krunk again: the 5 tabs come back with
     their titles.
   - Only the active tab loads. The others are dimmed and load when clicked.
   - Private tabs are not restored.
   - Kill the process from Task Manager about 3 seconds after changing tabs. The session is
     still restored (it is saved about 2 seconds after each change).

## Ad and tracker blocking (M5)
0. **Updater smoke test (no window):**
   - Run `Krunk.exe --update-filters` in a terminal. The exit code is 0.
   - `%LOCALAPPDATA%\Krunk\krunk.log` ends with `filter update: downloaded [...]`
     and `filter self-test: ... decisions OK`.
   - `engine.dat` and `filters\*.txt` exist.
1. **News-site counter:**
   - Open a large ad-supported news site, such as `https://www.cnn.com/`,
     `https://www.dailymail.co.uk/` or `https://www.theguardian.com/`.
   - The orange shield badge counts up while the page loads (typically 20–150) and settles. It
     updates smoothly, not once per request.
   - Navigate to another article: the count restarts from 0.
   - **Record:** the site, the final count, and a screenshot of the toolbar.
2. **Shield popover:**
   - Click the shield. It shows the site, "N blocked on this page", and "Allow ads on this site".
   - Click that button. The page reloads, the badge disappears, the shield turns grey, and ads
     are visible again.
   - Click the shield, choose "Block on this site again", and the ads are gone after the reload.
   - The choice survives a restart (it is stored in the `adblock_allowlist` table).
3. **Network check:**
   - Open DevTools (F12) → Network on the news site and filter for `doubleclick` or
     `googlesyndication`.
   - Those requests show as failed (cancelled).
   - `krunk.log` has "tab N: K requests blocked on this page".
   - The page's own document request is never blocked, even when you open an ad link directly
     (for example a `https://ad.doubleclick.net/...` link in a new tab still loads).
4. **Cosmetic filtering:**
   - Open `https://www.advfn.com/`. The ad slot areas (for example `#APS_300_X_600`) are hidden,
     with no empty boxes.
   - On a page with generic ad containers (classes like `ad-banner-300`), they collapse.
5. **Private tabs:** repeat step 1 in a private tab (Ctrl+Shift+N). Blocking and the counter work
   the same.
6. **Tracking parameters:**
   - Visit `https://example.com/?utm_source=newsletter&fbclid=abc&id=1`. The address bar ends up
     at `https://example.com/?id=1`.
   - Pressing Back does not bounce back to the long URL (no redirect loop).
7. **DNT / GPC:**
   - Open `https://globalprivacycontrol.org/`. It reports "GPC signal detected".
   - In DevTools, `navigator.globalPrivacyControl === true` and `navigator.doNotTrack === "1"`.
   - Network → any request → Request Headers include `DNT: 1` and `Sec-GPC: 1`.
8. **Third-party cookies** (on by default):
   - A page on one site embedding a frame of another site (e.g. `http://127.0.0.1:8081` with a
     `http://localhost:8082` frame that sets a cookie by header and by script): the frame sees
     `document.cookie = []`, and the next request of the frame carries no `Cookie`.
   - The same frame URL opened as the top-level page keeps its cookies.
   - The test pages are in the pipeline workspace (`screenshots/verify2/test-pages`, served by
     `serve.ps1`).
9. **HTTPS-only** (Settings → Privacy and security → HTTPS-only mode):
   - `http://neverssl.com/` is upgraded and fails over HTTPS, so the "This site doesn't support
     a secure connection" page appears.
   - `http://http.badssl.com/` (its HTTPS answers with a redirect back to HTTP) shows the same
     page within a second or two. It must not keep loading forever.
   - "Continue to HTTP site" loads it over HTTP. Revisiting it in the same session doesn't show
     the page again. After a restart it does.
   - `http://example.com/` silently ends up on `https://example.com/`.
10. **Daily update:**
    - Delete `filters\state.json` and start the browser. About a minute later the log shows
      `filter engine rebuilt`, or no rebuild if the downloads match the cache.
    - Start again within 24 h: there is no download.
    - The next start logs `filter engine loaded from cache in N ms` (expected under 20 ms).
11. **Offline fallback:** disconnect the network, delete `engine.dat` and `filters\`, and start
    the browser. Blocking still works with the embedded lists, and the log shows the download
    errors.

## Password vault (M6)
1. **Setup:**
   - Open ⋮ → Passwords. Create a vault; mismatched or short passwords are refused.
   - `%LOCALAPPDATA%\Krunk\vault.json` appears. Open it in Notepad: no usernames,
     sites or passwords are readable, only base64, and `"wrap": "dpapi"`.
2. **Save prompt:**
   - Log in on `https://github.com/login` with a test account. "Save password?" appears under
     the toolbar (outside the page) with the username and origin.
   - Save → the login appears on the passwords page.
   - Log in again with a different password: the prompt says "Update". With the same password
     there is no prompt.
3. **Never:** on another site choose "Never for this site". No more prompts there; the origin
   is listed under "Never saved" and can be removed.
4. **Prompt dismissal:** trigger the save prompt, then press Ctrl+Tab, or click into the page, or
   press Escape. The prompt disappears and nothing is saved.
5. **Fill:**
   - Open the login page again. A key icon appears in the address bar; nothing is filled
     automatically.
   - Click the key, pick the account: the username and password are filled.
   - With the vault locked, the key first asks for the master password.
6. **Exact origin:**
   - Save a login for `https://bank.example`, or any site where you can use two hosts or ports.
   - On `https://www.bank.example`, `http://bank.example` and `https://bank.example:8443` the
     fill prompt shows "No saved logins".
7. **Cross-origin iframe (must not fill):**
   - Use a page whose login form is in a cross-origin iframe. For example, save
     `<iframe src="https://github.com/login" width=600 height=500></iframe>` as a local `.html`
     file served from `http://localhost`, or use any site that embeds a third-party login widget.
   - No key icon appears for the iframe's form. Submitting inside the iframe gives no save
     prompt, and nothing is ever filled into the iframe.
   - Record the result.
8. **Reveal and copy:**
   - Passwords page → Show: the master password is required again, and a wrong one is
     refused (and counts towards the delay).
   - Copy password: paste it into Notepad. Press Win+V: it is **not** in the clipboard history.
   - Wait 30 s and paste again: the clipboard is empty.
   - Copy, then copy some other text yourself within 30 s: your text is not wiped.
9. **Rate limit:**
   - Enter a wrong master password 4 times. The messages say "Try again in 1 s / 2 s / 4 s /
     8 s", and Unlock is disabled meanwhile.
   - Restart the browser: the delay still applies.
   - The correct password resets it.
10. **Auto-lock:**
    - Unlock, then press Win+L and log back in: the passwords page shows "locked".
    - Unlock, sleep the PC, wake it: locked.
    - Unlock and leave the vault unused for longer than the auto-lock time (lower it to 1 min
      once the M7 settings exist): locked.
11. **Change master password:**
    - Change it; the old one no longer works and the new one does, with all logins intact.
    - Interrupt test: make `vault.tmp` a folder (`mkdir vault.tmp` in the data dir) and try to
      change it. An error is shown, the old password still works, and `vault.json` is
      unchanged. Delete the folder afterwards.
12. **Copied vault on another account (DPAPI):**
    - Copy `vault.json` into `%LOCALAPPDATA%\Krunk\` of a *different* Windows user
      account (or another PC) that also has Krunk.
    - Unlocking with the **correct** master password fails with "This vault can only be opened
      by the Windows account (and PC) that created it."
13. **Leak check:** search `krunk.log` and `browser.db` (DB Browser for SQLite, or
    `findstr /s /i` over the data dir) for the test password: there are no hits.

## Settings (M7)
Open ⋮ → Settings, or type `krunk://settings` in the address bar.
1. **Search engine:** choose Google: address-bar searches go to Google. Choose Custom with
   `https://search.example/?q=` (no `%s`): an inline error appears and the setting is not saved.
   Add `%s` and searches use the template.
2. **Startup:**
   - "Open the homepage" with an empty or `ftp://` homepage gives an inline error.
   - A valid homepage opens on the next start.
   - "Continue where you left off" restores the tabs; only the active one loads.
3. **Hardware acceleration:** untick it and the "Restart required" banner appears. Click
   "Restart now": the browser closes and reopens with the same tabs. `krunk://about` → "Browser
   arguments" now contains `--disable-gpu`.
4. **Sleep and discard:** sleep 0 is accepted (never). Discard enabled with a time ≤ the sleep time
   gives an inline error. Values above 1440 give an error.
5. **Ad blocking:**
   - Untick "Block ads and trackers": the shield counter stays at 0 on a news site.
   - Untick EasyList: a rebuild happens (the log says "filter engine rebuilt").
   - "Update now" shows the last check time.
   - Add `bad host` to the allowlist: inline error. Add `example.com`, then remove it.
6. **Privacy:**
   - There is no "Tracking prevention" or "SmartScreen" setting any more.
   - HTTPS-only: `http://example.com` opens as https.
   - DNT/GPC unticked: `https://httpbin.org/headers` no longer shows `Dnt` / `Sec-Gpc` (headers
     change immediately; `navigator.*` on new tabs).
   - "Download engine security components from Google" shows the restart banner. With the
     default (off), an idle browser behind a logging proxy contacts no Google host (see
     "Background traffic" below).
7. **Clear on exit:**
   - Tick "Cookies and site data" and "Browsing history".
   - Visit `https://httpbin.org/cookies/set?lbtest=42`, then close the window. It disappears
     after the data is cleared (the log says "clearing data on exit", then "data cleared,
     closing").
   - Start again: `https://httpbin.org/cookies` shows `{}` and History is empty apart from new
     visits.
8. **Passwords:** set auto-lock to 1 minute; the vault locks after a minute without use.
   "Change master password…" opens the passwords page with the change dialog.
9. **Downloads:**
   - "Change…" opens a native folder picker. Downloads then go to that folder (a name clash
     gives `name (1).ext`).
   - "Ask where to save" shows a Save dialog for each download; cancelling it cancels the
     download.
10. **Advanced:**
    - User agent `Test/1.0`: `https://httpbin.org/user-agent` shows it in a new tab and in an
      existing tab after a reload; clearing the field restores the default.
    - Proxy `not a proxy` gives an inline error; `127.0.0.1:8888` shows the restart banner.
    - Extra flags: `--no-sandbox`, `"--x"` and `--a=` are refused inline; `--force-dark-mode`
      shows the banner.
11. **Zoom and theme:**
    - Default zoom 125 %: a site you have not zoomed opens at 125 %; a site you zoomed keeps its
      own zoom (zoom is remembered per site).
    - Theme Dark: the shell, built-in pages, web pages that support `prefers-color-scheme`, and
      the window title bar all turn dark.
    - System follows Windows (Settings → Personalization → Colors).
12. **Enter in dialogs:** in every built-in dialog, pressing Enter in a field confirms. Examples:
    - Passwords → Show → type the master password → Enter reveals the password.
    - Add or edit login, change master password, Clear browsing data, edit bookmark.
    - Cancel (or Escape) closes without changes.
13. **Editing keeps the note:** add a login with a note, then edit only its username. Reopen the
    editor: the note field still says "(unchanged)", so the note was kept.
    "Show" (after the master password) displays the note under the username for 20 s, together
    with the password.
14. **Reset:** "Reset settings…" asks first, in an in-page dialog (no native
    "krunk.invalid says" box; the same applies to every delete prompt on the built-in
    pages). After "Reset", every value is back to its default, and the
    restart banner appears if restart-only values were different.
15. **Isolation:** only `krunk://settings` can change settings. In DevTools on any website,
    `__lbHost.postMessage({cmd:"saveSettings",settings:{}})` has no effect.

## Memory (M8)
Use `scripts/vm-mem.ps1` (PowerShell) to list the process tree with each process's type and
working set.
1. **Switches at runtime:** `krunk://about` → "Browser arguments" contains `--no-pings`,
   `--disable-domain-reliability`, `--disable-background-networking`,
   `--disable-component-update`, the `--host-resolver-rules` for `krunk.invalid`, and
   `--disable-features=AutofillServerCommunication,…,AimServerRequestOnStartupEnabled`.
2. **Lazy restore:**
   - Set "Continue where you left off", open 5 sites, close and start again.
   - Every tab shows its title and favicon, but the background tabs are dimmed. `vm-mem.ps1`
     shows 3 renderers (the shell, the active tab and one spare).
   - Click one restored tab: it loads, and there is one renderer more.
3. **1-tab baseline:** start with `--mem-report` and one new tab. Wait 10 s and note the last
   `tick:` line of `%LOCALAPPDATA%\Krunk\memory-report.log`
   (about 355 MB working set, 3 renderers).
4. **Sleep** (test scale):
   - In Settings, turn on "Discard background tabs".
   - Start with `--sleep-test-seconds=90 --mem-report` (sleep 90 s, second stage 90 s later,
     discard 360 s after the tab was last active). `krunk.log` has "tab sleep test mode".
   - Open 10 real sites, one per tab. Note the `tick:` line (about 3 GB working set,
     18 renderers).
   - Do not touch the browser for about 3 minutes. Background tabs dim. `krunk.log` has
     "tab N: low memory", then "tab N: asleep (second stage)" for each background tab. The
     mem-report shows `asleep=10 suspended=10` and about 11 % less working set.
   - Never slept:
     - the active tab
     - a tab playing audio (start a video, or a Web Audio tone, in a background tab; the log
       says "tab N: audio playing")
     - a tab with a download in progress
5. **Wake (must paint):** with `--sleep-test-seconds=10`, open a page that updates itself every
   second (a clock). Switch to another tab and back:
   - after about 15 s (low memory),
   - after about 30 s (second stage),
   - after more than 40 s with discard on (reloaded).
   Each time the page is shown at once and keeps updating; it is never left white. The log has
   "tab N: woken (LowMemory)" or "(Suspended)". (Pages are not frozen: see README, "Known
   limitations".)
6. **Discard:**
   - About 6 minutes after a tab was last active, the log has "tab N: discarded". The
     mem-report shows `unloaded=` rising and fewer processes and renderers.
   - Clicking a discarded tab reloads its URL. Its back history is gone, which is expected.
7. **Private discard:**
   - Open two private tabs and log in to a site in one of them. Let the other one be
     discarded.
   - The discarded private tab shows only its host as the title, with no favicon. Activating it
     reloads it in the same private session, so you are still logged in.
   - The last live private tab is never discarded.
8. The test options are not in the Settings UI. Without `--sleep-test-seconds`, the timers are
   the settings values in minutes.

## Krunk 0.2 (round 2)
Last pass: 2026-11-04 on the Windows 11 VM; screenshots in the pipeline workspace under
`screenshots/krunk/` (`m1-*` … `m7-*`). VM helpers: `scripts/vm-frame-test.ps1`, `vm-dpi.ps1`,
`vm-res.ps1`, `vm-youtube.ps1`, `vm-install.ps1`, `vm-update.ps1`.

### Rename and migration
1. With `%LOCALAPPDATA%\LightBrowser` from 0.1 (history, bookmarks, a logged-in cookie, a vault)
   and no Krunk folder, start Krunk: the log says "data migration: renamed …". History,
   bookmarks, passwords and the login are all there.
2. While `LightBrowser.exe` runs, Krunk refuses to start and asks to close it first.
3. `lb://history` opens `krunk://history`; restored old tabs show the Krunk pages.

### Window and tabs (100 % and 150 % scaling)
1. Drag the window by empty tab-row space; double-click it to maximize and restore. Resize from
   every edge and corner, including the top.
2. Hover the maximize button: the Snap Layouts flyout appears. Right-click the tab row, or press
   Alt+Space: the system menu opens.
3. Maximized, the window covers the work area exactly; the taskbar stays visible.
4. Ctrl+T, "+", menu → New tab, and the first window at start: typing goes into the address bar.
5. Tabs are 256 px wide when few, shrink evenly to 72 px, then the row scrolls with the active tab
   in view.

### Cookies, site data, Inspect
1. `krunk://storage` lists sites with their cookie counts. Search, expand, delete a cookie, a site,
   then all; the site is signed out.
2. Shield → "Cookies and site data" on a test origin: view, edit and add localStorage entries.
   The page sees the edit (reload); DevTools → Application agrees.
3. In a private tab the same page shows only the private context's data.
4. Right-click → Inspect selects that element in DevTools; Ctrl+Shift+C inspects at the cursor.

### YouTube
1. `Krunk.exe --update-filters` exits 0; the log says "YouTube script … bytes (OK)".
2. Open three watch pages, cold and warm cache. After about 40 s in the DevTools console:
   `document.querySelector('.ad-showing')` is null, there is no enforcement wall, the video
   plays, and no sponsored card shows in the sidebar.
3. Search, comments, seeking and pause work.
4. Control: Shield → "Allow ads on this site" brings the sponsored card back.

### Pointer lock and full screen
1. Serve `ui/dev/pointer-lock.html` (it is in `scripts/vm-www/8090`) and click the stage:
   - the state is LOCKED, the cursor is hidden and confined;
   - moving far past the screen edge keeps adding movement;
   - `isTrusted` is true;
   - "Press Esc to show your cursor" shows for 3 s.
2. Esc releases the lock, and the page gets no Escape keydown. Alt+Tab, a tab switch and
   navigation release it too.
3. "Lock, unadjustedMovement": raw mouse counts (no acceleration).
4. "Full screen + lock": the window fills the monitor and the lock works. The first Esc unlocks,
   the second leaves full screen. F11 hides the shell and fills the monitor; F11 again restores.
5. krunker.io: join a match. The view turns with the mouse, windowed and in F11.

### Installer
1. Run `Krunk-Setup-<version>.exe`:
   - no UAC prompt (it asks for no elevation);
   - components "Krunk" and "Desktop shortcut";
   - folder `%LOCALAPPDATA%\Programs\Krunk`;
   - Finish → Start Krunk.
2. Start Menu → Krunk starts it. Settings → Apps → Installed apps lists "Krunk" with its version
   (`HKCU\…\Uninstall\Krunk`; nothing under HKLM).
3. With Krunk running, `Krunk-Setup-<version>.exe /S` closes it, installs within about 20 s and
   keeps an existing desktop shortcut. Without `/S`, it asks you to close Krunk (Retry/Cancel).
4. Uninstall from Installed apps: "Also delete my Krunk data" is unticked by default. The program,
   shortcuts and entry are removed; `%LOCALAPPDATA%\Krunk` is kept. With the box ticked, it is
   deleted too.
5. On a standard (non-admin) account with UAC on, steps 1–4 work the same.
   *Not run on the VM: it has UAC off and an admin user.*

### Update check
Use a dev build that points at a test server:
- `UPDATE_BASE_URL=http://127.0.0.1:18080 KRUNK_UPDATE_PUBKEY_FILE=core/tests/fixtures/update/test-signing.pub scripts/build-dist.sh --dev`;
- once at 0.2.0 and once with the version bumped to 0.2.1;
- the 0.2.1 build published with `dist/server/publish-release.sh … --key core/tests/fixtures/update/test-signing.key`
  into a local folder or the staging volume;
- on the test PC: `ssh -R 127.0.0.1:18080:<server>`.

1. Install dev 0.2.0 and start it. After about 60 s the prompt "Krunk 0.2.1 is available"
   appears (Release notes / Later / Update now).
2. Update now: a progress bar, then Krunk closes, and about 20 s later reopens with the tabs
   restored. Settings → About says 0.2.1, and so does the HKCU entry. `%TEMP%\Krunk-update` is
   gone after the next start.
3. Tampered manifest (change the version in `latest.json`, keep the old `.sig`): Settings →
   About → Check for updates says "The update check failed: the manifest signature is not valid
   for Krunk's release key", and `krunk.log` has "update check rejected: …".
4. Wrong hash (re-sign a manifest whose `sha256` is wrong): Update now downloads, then the prompt
   says "the download's SHA-256 does not match the manifest", with Try again. The file is deleted
   and Krunk keeps running.
5. Turn off "Check for updates automatically" and restart: no request reaches the server
   within 2 minutes. "Check for updates" still works.
6. A release build checks `https://krunk.209.wf` (live). It reports "Krunk is up to date" in
   About, or offers the published version when it is newer. With no network it says "The update
   check failed: … the server name could not be resolved", and nothing else happens.

### Download server (`dist/server`)
See `dist/server/README.md` → "Checks". After publishing:
- `curl -I` shows the content types and cache headers;
- the downloads' SHA-256s match `SHA256SUMS`;
- `openssl pkeyutl -verify -pubin -inkey assets/keys/update-signing.pub -rawin -in latest.json -sigfile <(base64 -d latest.json.sig)`
  succeeds;
- the landing page shows the version, both downloads and their hashes.

## Krunk 0.3 (windows, tab dragging, docked DevTools)
Last pass: 2026-11-05 on the Windows 11 VM; evidence in the pipeline workspace under
`evidence-0.3.0/`. VM helpers: `scripts/vm-windows.ps1` (window lookup, tab positions, safe
focus and keys), `vm-030-smoke.ps1`, `vm-030-windows.ps1`, `vm-030-session.ps1`,
`vm-030-dpi.ps1`, `vm-030-devtools.ps1` (phases `main`, `restart`, `fullscreen`). Test page:
`scripts/vm-www/8092/move.html` (a field, a `<select>`, a looping video, history entries and a
status line with the load time; double-click the video for page full screen).

### Package (release build)
1. `unzip -l Krunk-win-x64.zip` and `7z l Krunk-Setup-<version>.exe` list no `*.md`.
2. Put any `x.md` into `dist/Krunk-win-x64/licenses/` during a build (or into `$out` in
   `build-dist.sh`): the build stops with "Markdown files in the package".
3. Installing over 0.2.1 (silently with `/S`, and with the wizard) leaves no `README.md` or
   `SECURITY.md` in `%LOCALAPPDATA%\Programs\Krunk`; the data folder is untouched.

### Upgrade from 0.2.1
1. In 0.2.1, open several tabs, add a bookmark, visit a few pages and save a password. Close it.
2. Install 0.3.0 and start it: one window with the same tabs and the same active tab. History,
   bookmarks and the vault entry are there.

### Windows and tabs
1. **Drag out:** on `move.html`, type into the field, scroll down a little, wait for the video to
   play, then drag the tab down out of the strip. A new window follows the mouse with the tab in
   it; release it anywhere. The status line shows the same load time (no reload), the same
   scroll position, the field text, the same `history` count, and the video still playing;
   the `<select>` opens in the right place. The server log has no new request.
2. **Drag in:** drag a tab from one window over another's tab strip: a marker shows the drop
   position, and the tab lands there. Esc during the drag puts the tab back.
3. **Merge:** drag a window's only tab onto another window's strip: the tab moves and its empty
   window closes. Dragging that only tab elsewhere moves the window.
4. **Menu:** right-click a tab → Move to new window (greyed out for a window's only tab), New
   window; Ctrl+N; main menu → New window.
5. **Closing:** Ctrl+W on a window's last tab closes the window; closing the last window quits
   Krunk. `scripts/vm-mem.ps1` then shows no Krunk process.
6. **Per window:** in a second window, the main menu, the shield, find (Ctrl+F), zoom, F11 and
   pointer lock act on that window only; Ctrl+Shift+T reopens into the current window.
7. **Private:** move a private tab to a new window. It stays private (the private badge,
   its private login), and `krunk://history` doesn't list it.
8. **Restore:** with three windows (one maximized, two at known places), quit and start again:
   all three come back with their tabs, places and the maximized state. A window saved off
   screen (edit its bounds to 5000,3000 in the `session` kv entry) comes back on the monitor.
9. **Scaling:** repeat 1, 2 and 8 at 125 % or 150 %; the strip, the drop marker and the popups
   line up. *VM: one monitor, at most 125 %; two monitors were not tested.*

### DevTools
1. F12: DevTools opens docked on the right inside the window. A window enumeration (`KrunkWindows`
   in `scripts/vm-windows.ps1`) shows no other top-level Krunk window. F12 again (also typed
   inside DevTools) closes it.
2. Right-click → Inspect selects the clicked element.
3. Tab 2 has no DevTools; F12 there opens its own. Switching back shows tab 1's again.
4. Drag the splitter; dock at the bottom with the toolbar control and drag that splitter. Restart:
   F12 opens it at the bottom with that size.
5. Move the tab with DevTools to a new window: DevTools is still docked there. Close the tab: its
   DevTools closes.
6. In DevTools, Ctrl+F searches DevTools; Ctrl+T opens a tab in Krunk.
7. F11 keeps DevTools; a page's full screen hides it; Esc shows it again.
8. A tab with DevTools open is never put to sleep or discarded (`--sleep-test-seconds=30`, leave
   it in the background; the log has no "tab N: low memory" line for it, while other background
   tabs get one).

## Background traffic
1. Run a logging HTTP proxy (`screenshots/verify2/proxy.ps1` in the pipeline workspace listens on
   `127.0.0.1:8888`) and set Settings → Advanced → Proxy to `127.0.0.1:8888` (restart).
2. Start with a fresh data folder and leave the new-tab page idle for 5 minutes, then load a few
   pages.
3. Expected in the proxy log: only the filter-list hosts (`easylist.to`,
   `ublockorigin.github.io`, on a fresh profile) and the pages you loaded. No `*.google.com`,
   `*.googleapis.com` or `*.gvt1.com` host, and no `krunk.invalid` host.
4. With "Download engine security components" on (restart), `update.googleapis.com` and
   `edgedl.me.gvt1.com` appear, and nothing else from Google.
5. WebRTC: open `https://browserleaks.com/webrtc`. The first time for this `Krunk.exe` path, Windows
   Firewall may ask whether Krunk may use public and private networks. That is Chromium's mDNS
   responder on UDP `[::]:5353`, which `Get-NetUDPEndpoint` shows. Click **Cancel**. The page
   must still say **No leak** and list no private (LAN) IP address under "Local IP Address".

## Verification recipes (final pass)
These are the methods used on the Windows 11 VM (`scripts/vm.sh`). The test server, test pages
and proxy are in the pipeline workspace under `screenshots/verify2/`. They are small PowerShell
scripts: `serve.ps1` is an HttpListener with one folder per port; `proxy.ps1` is a logging
HTTP/CONNECT proxy on 127.0.0.1:8888.
1. **Cross-origin iframe never filled:**
   - Serve `login.html` on `http://127.0.0.1:8081`. It has a login form plus an iframe of
     `http://localhost:8082/frame.html`, which has a login form of its own.
   - Save a login for each origin on `krunk://passwords`.
   - On the top page: nothing is filled on load. The key icon offers only the 127.0.0.1:8081
     login, and filling it leaves the iframe's fields empty.
   - Submitting a login inside the iframe gives no "Save password?".
   - Control: `http://localhost:8082/frame.html` opened directly offers its own login.
2. **Auto-lock on Windows lock and sleep:**
   - On a real PC, unlock the vault, press **Win+L**, sign in again: the log says
     "vault locked (Windows session locked)".
   - Unlock, then sleep the PC (Start → Power → Sleep) and wake it: "vault locked (system suspend)".
   - Without locking the session (VM): post `WM_WTSSESSION_CHANGE` (0x2B1, wParam 7) and
     `WM_POWERBROADCAST` (0x218, wParam 4) to the hidden `KrunkApp` window (since 0.3.0; it
     was `KrunkWindow` before) from a helper in the same session. The log lines are the same.
3. **Cosmetic filtering:**
   - On `https://edition.cnn.com/` (rules `cnn.com##.stack__ads`, `##.zone__ads`,
     `##.ad-slot-header__wrapper`), run this in the DevTools console:
     `Array.from(document.querySelectorAll(".stack__ads, .zone__ads, .ad-slot-header__wrapper")).map(e => getComputedStyle(e).display)`.
     Every value is `none`.
   - Shield → "Allow ads on this site", then run it again: the values are `block`, and the top ad
     slot is visible.
4. **Third-party cookies:**
   - Serve `cookie-top.html` on `http://127.0.0.1:8081`. It embeds `http://localhost:8082/set-cookie`,
     which answers `Set-Cookie: tp_header=1; SameSite=None; Secure`, sets `tp_js` from script, and
     reports `document.cookie`.
   - With "Block third-party cookies" on (the default): the iframe reports `[]`, and a reload sends
     no Cookie header (server log).
   - Open `http://localhost:8082/set-cookie` directly: both cookies are set (first party). Back on
     the top page, the iframe still reports `[]`.
   - With the setting off and after a restart: the iframe reports `[tp_header=1; tp_js=1]`, and the
     reload sends them.
5. **Daily list update:**
   - The log at start says `filter lists: last checked X h ago; next check in Y h`. A relaunch
     within 24 h downloads nothing: `state.json` `lastCheckMs` and the list files are unchanged
     after more than 60 s.
   - Subtract 25 h from `lastCheckMs` in `%LOCALAPPDATA%\Krunk\filters\state.json` and
     start: about 60 s later the log says `update due; updating`, then
     `filter lists checked; … next check in 24.0 h`.
6. **New-tab page makes no network requests:** on the new-tab page, open DevTools → Network,
   press Ctrl+R, and filter `-domain:krunk.invalid`. Expected "0 / N requests".
7. **WebRTC and the firewall:** open a Stack Overflow question (it creates an
   RTCPeerConnection). The first time for this `Krunk.exe` path, Windows Firewall may ask about
   public and private networks: Chromium's mDNS responder on UDP 5353, as in Chrome. Cancel it;
   WebRTC keeps working and `browserleaks.com/webrtc` still shows no private IP (see "Background
   traffic", step 5).
8. **Download folder:** Settings → Downloads → "Change…" opens a native folder picker. After
   picking a folder, the next download is saved there.
9. **Startup with the homepage:** set the homepage and choose "Open the homepage", then restart.
   A single tab opens on the homepage.
10. **Update now:** the "Last checked" time changes, and the log says "filter lists checked".
11. **Proxy:**
    - Run a local proxy, set "Proxy server" to `127.0.0.1:8888`, and click "Restart now". The
      browser arguments contain `--proxy-server=127.0.0.1:8888`.
    - The proxy log shows `CONNECT host:443` for every site visited.
    - With the proxy stopped, pages fail with a connection error.
    - No `CONNECT shell.krunk.invalid` or `krunk.invalid` ever appears in the
      proxy log.

