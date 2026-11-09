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

**Not run for 0.3.0** (the VM can't):
- two monitors (the VM has one) and display scaling above 125 % (the VM's maximum);
- installing as a standard user with UAC on (the VM has UAC off and an admin user; see
  "Installer" step 5).

The in-app update from 0.2.1 to 0.3.0 ("Update check" above) was run after publishing, against
the live server: the live 0.2.1 offered 0.3.0 within a minute, Update now restarted Krunk as 0.3.0
in about 20 s, with the session restored as one window and no `*.md` left.

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

## Krunk 0.4 (round after 0.3.0)
Last pass: 2026-11-05 on the Windows 11 VM; evidence in the pipeline workspace under
`evidence-0.4.0/`. VM scripts (each runs on a fresh profile): `vm-040-merge.ps1`,
`vm-040-tabs.ps1`, `vm-040-tabanim.ps1`, `vm-040-plock.ps1`, `vm-040-bookmarks.ps1`,
`vm-040-downloads.ps1`, `vm-040-parts.ps1` (with `scripts/vm-www/rangeserve.py`),
`vm-040-blocked.ps1`, `vm-040-filters.ps1`, `vm-040-hex.ps1`. Test pages are in
`scripts/vm-www/` (`8090/plock-measure.html`, `8092/dl.html`, `ads.html`, `own.html`,
`bin.html`).


The in-app update from 0.3.0 to 0.4.0 was run after publishing (2026-11-06), against the live
server: the live 0.3.0 offered 0.4.0 after 56 s; Update now brought Krunk back as 0.4.0 in 48 s
with the session restored, the vault unchanged and no `*.md`. The downloaded installer was still
in `%TEMP%\Krunk-update` right after the restart (the installer was still exiting) and was gone
after the next start.
### Tabs and windows
1. **Merge:** drag a window's only tab onto another window's tab row: the tab lands there and
   that window is in front, not behind the other one.
2. **Tab menu:** right-click a tab → Close other tabs / Close tabs to the right (greyed out on
   the last tab). Ctrl+Shift+T brings the closed tabs back one by one.
3. **Downloads tab:** with `krunk://downloads` open (in any window), the download icon or Ctrl+J
   switches to it instead of opening another; the same for history, bookmarks, settings and
   `krunk://adblock`.
4. **Closing animation:** close several tabs with the mouse: each shrinks away smoothly; the
   next close button stays under the mouse until it leaves the tab row. With Windows' "Animation
   effects" off there is no animation.

### Pointer lock while buttons and keys are held
On a game (or `8090/plock-measure.html`): lock the pointer, hold both mouse buttons and A, and
turn with the mouse. The view turns smoothly; `plock-measure.html` reports no synthetic events
and an even movement per frame. Esc still releases the lock.

### Bookmarks bar
1. Right-click empty space on the bar: Add page, New folder, and Show bookmarks bar Always /
   Only on new tab / Never. "Only on new tab" shows it on the new-tab page only; Ctrl+Shift+B
   toggles. The choice survives a restart; Settings → Appearance shows the same choice.
2. Right-click a bookmark: Open, Open in new tab, Open in new window, Edit (name and address),
   Delete. Right-click a folder: Open all, Rename, Delete.
3. Drag a bookmark along the bar (a marker shows where it goes) and onto a folder (the folder
   is highlighted): it moves into the folder.

### Downloads
1. `krunk://downloads` shows each download's full address, the page it came from, start and
   finish date and time. Right-click a row (or Shift+F10 or the Menu key): Open, Show in
   folder, Copy download link, Copy page address, Retry, Remove from list, Delete file (asks
   first); a running download has Pause/Resume and Cancel.
2. **Parts:** Settings → Downloads → Connections per download (default 4). A 64 MB file from a
   server that allows ranges (`rangeserve.py`, 2 MB/s per connection) finishes about 3–4 times
   faster with 4 than with 1; the server log shows 4 range requests plus the probe, each with
   the same cookie and referrer as the engine's request. The SHA-256 matches.
3. A file that changes on the server during the download fails ("the file changed on the
   server") and leaves no part files; pause/resume and cancel work from the row menu; quitting
   during a download leaves part files that the next start removes.
4. A server without ranges, without an ETag/Last-Modified, a small file, a private tab and a
   setting of 1 use the engine's single download.

### Ad blocking
1. On `8092/ads.html` the shield lists the latest blocked addresses; "Show all" opens
   `krunk://adblock` with every blocked request grouped by host, its filter and its list. The
   same requests appear in `krunk.log` with `--log-blocked`.
2. **Filter lists** tab: each list with address, filter count and last update. Turn EasyList
   off: its rules stop applying without a restart. Add
   `https://easylist.to/easylist/fanboy-social.txt`: it is downloaded, named by its title, and
   blocks (`own.html`: `buttons.github.io` is blocked). An address that answers 404 is refused
   and not added. Remove the added list; Restore default lists; Update now.
3. **My filters** tab: add `||localhost^*/probe.png` and press Ctrl+S: `own.html` no longer loads
   it, without a restart; a line it can't read is reported with its number.
4. Restart: the lists, the added list and My filters are as they were.

### Site data hex editor
1. `8092/bin.html` stores a binary string in local storage and a percent-encoded binary cookie.
   Open the shield → Cookies and site data.
2. Edit `bin`: the dialog opens on Hex, "Binary string". Type over a byte, press Insert and type
   a byte, Delete one, Save. Reload `bin.html`: it shows exactly those bytes.
3. Edit the cookie `blob`: it opens on Hex, "Percent-encoded". Change a byte and save: the page
   reads the new value, percent-encoded; its other attributes are unchanged.
4. In the Text tab, a cookie value with `;` is refused; in Hex, bytes that are not valid UTF-8
   are not saved as UTF-8.

## Krunk 0.5 (round after 0.4.0)
Last pass: 2026-11-06 on the Windows 11 VM (release build); evidence in the pipeline workspace
under `evidence-0.5.0/`. VM scripts (each on a fresh profile): `vm-050-shell.ps1`,
`vm-050-bar.ps1`, `vm-050-reorder.ps1`, `vm-050-overlays.ps1`, `vm-050-siteinfo.ps1`,
`vm-050-settings.ps1`, `vm-050-autofill.ps1` (with `vm-050-https-setup.ps1` once),
`vm-050-import.ps1` (real Firefox/Chrome/Edge profiles), `vm-050-lna.ps1`, `vm-050-media.ps1`.
Test pages are in `scripts/vm-www/` (`8094/login.html`, `links.html`, `media.html`, `ua.html`,
`8095/frame-login.html`, `8443/origin-login.html`).

### Self-built engine and codecs (0.5.2)
The engine is Krunk's own CEF build with proprietary codecs (`scripts/cef-build/README.md`);
the package has `snapshot_blob.bin` instead of `v8_context_snapshot.bin`. On a fresh profile
(`vm-052-engine.ps1`): Krunk starts without the "runtime damaged" dialog; the probe page
`http://localhost:8094/media.html` lists H.264 baseline/high, H.264 + AAC and AAC-LC as
`true` / `probably` and its H.264/AAC MP4 clip, the public DASH and HLS streams play
(`play-…=playing` in the test server's log); example.com, DevTools (F12), krunk://gpu,
krunk://settings and krunk://history work; `krunk.log` has no WARN/ERROR. HEVC shows
`probably` only on a PC with a hardware HEVC decoder (never on the VM); AC-3/E-AC-3 only where
Windows still has its Dolby decoder (not on 24H2 or later). On the operator's PC: krunk://gpu
still says "Hardware accelerated", and a HEVC clip plays if the GPU decodes HEVC. Then the
whole 0.5 list below again on this engine (`evidence-0.5.2/vm/`), and the in-app update
0.5.1 → 0.5.2 (`vm-update.ps1`).

### Address-bar suggestions (0.5.1)
Type `exa` into the address bar after visiting and bookmarking example.com: a dropdown shows
"Search exa", the bookmark and the history pages. Down selects a row and the bar shows its
address while typing still goes to the bar; Enter opens the selected row; Esc closes the list
and keeps the typed text, a second Esc leaves the bar. A click on a row opens it. With nothing
selected, Enter searches or opens the typed address as before (`vm-051-omnibox.ps1`).

### Toolbar and bookmarks bar
1. No Home button; Alt+Home still opens the home page. Click ⋮, the shield and the lock three
   times each: open, closed, open.
2. Navigate while the address bar has the focus (click into it, then click a bookmark): the bar
   shows the new address. Type something, then navigate from a bookmark: the typed text is
   replaced.
3. Drag the site icon onto the bookmarks bar and into a folder: the page is bookmarked there.
   Right-click an item in a folder dropdown: its menu. Drag an item within the dropdown, onto a
   subfolder, and out onto the bar. Restart: the order is kept.
4. Drag a tab slowly along the row: it follows the mouse and the others slide. With Windows'
   "Animation effects" off, no slide.

### Overlays and DevTools
1. Hover a link: its address shows bottom left of the page, and moves to the bottom right when
   the mouse goes there; nothing in full screen.
2. Lock the pointer (`8094/plock…` or a game) four times in one tab: the Esc pill shows three
   times, not the fourth; a new tab counts again. No fade with "reduce motion".
3. F12: the strip above DevTools has Close and dock buttons; both docks work.

### Lock popup and shield
1. On example.com (https), neverssl.com (http), expired.badssl.com and self-signed.badssl.com,
   click the lock: connection state, certificate chain with fingerprints, and the warning text.
   Light and dark themes.
2. The shield popup: host with the on/off switch, the count, the latest blocked requests with
   type badges, "Show all".

### Settings, user agent, krunk://gpu
1. Settings has six tabs; the search box finds settings on other tabs and shows an empty state
   for nonsense; `krunk://settings#privacy` opens the Privacy tab.
2. Advanced → User agent: the default is shown read-only with Copy and equals
   `navigator.userAgent`. Pick "Firefox 159 on Windows": `8094/ua.html` (or any echo site) shows
   it on the next load. "Custom…" accepts a string; Default restores the engine's.
3. `krunk://gpu`: on real hardware the verdict is **Hardware accelerated** and WebGL names the
   real GPU; on the VM it says software rendering (SwiftShader). "Open chrome://gpu" works.

### Passwords
1. With logins saved for a site, click into its password field: the dropdown with the usernames;
   the username field shows it too. Only the site's own logins (exact origin); a site with none
   gets same-domain https logins (with their host); an http page is never offered https logins
   and vice versa.
2. Choose one: both fields fill, the page gets input and change events; before the choice the
   page's own script sees empty fields.
3. Restart (vault locked): the dropdown says "Unlock Krunk passwords"; the click opens Krunk's
   prompt (top right, outside the page); a wrong master password says so; the right one fills,
   and the next field in the session asks nothing until autolock.
4. `8094/login.html` has a cross-origin iframe (`127.0.0.1:8095`): its fields get only the
   iframe origin's login. A private tab gets suggestions and fills, but a new login submitted
   there is never offered for saving.

### Import
1. Settings → Passwords & import lists Firefox, Chrome, Edge and Brave profiles with their paths.
   Without a vault, importing passwords says to set one up first; with the vault locked, Krunk's
   unlock prompt appears and the import runs after it.
2. Import from Firefox (primary password if set): the counts (history, bookmarks, passwords);
   krunk://history, the bookmarks bar and krunk://passwords show them. A second import reports
   everything as "already there".
3. Import from Chrome/Edge: history and bookmarks; passwords report "N protected by Chrome".
   Export a CSV from Chrome and import it: the logins arrive.
4. `%TEMP%` has no `Krunk-import-*` folder left afterwards.

### Local network access
1. From a public https page, fetch `http://localhost:…` (DevTools console) or open a page that
   embeds a LAN device: Krunk's prompt "Connect to this computer?" / "…devices on your local
   network?". Allow: the request completes; a reload asks nothing. The site's Cookies and site
   data page lists "Local network access: Allowed" with Remove; after Remove the prompt is back.
   Block: the request fails. In a private tab the prompt comes every time and remembers nothing.
2. Plex: with the LAN server reachable, app.plex.tv can now list the server; playback of
   H.264/AAC streams still fails (codecs, see the README).

### Real hardware (not possible on the VM)
- `krunk://gpu` says **Hardware accelerated** and WebGL shows the real GPU; the toggle in
  Settings → Advanced turns it off after a restart (verdict: software rendering).
- Two monitors with different scaling; scaling at 150 % and 200 % (overlays, prompt and
  dropdown positions).

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


## Krunk 0.6.0 (round 6)
Scripted on the VM: `scripts/vm-060-*.ps1` with the pages in `scripts/vm-www/8094/` (evidence in
`.pipeline/evidence-0.6.0/m*/`). By hand, each in a fresh profile unless said otherwise:

### Shell and tabs (M1–M4)
1. **Menus fit their content** at 100 % and 125 %, light and dark; a right-click on a bookmarks-bar
   folder opens its item menu while the folder stays open; Ctrl+wheel updates the zoom chip at
   once; a second window shows the same search engine name in its address bar.
2. **Live resize:** drag a window edge: the page follows the frame without a gap or a frozen area.
3. **Address bar:** typing `wiki` completes inline to a visited address (Enter goes there, Delete
   removes the completion); right-click → Paste and go; "Copy" from the bar gives the full URL.
4. **Tabs:** right-click the title bar → the caption menu; Shift+Esc → krunk://tasks with PIDs,
   End process on a renderer shows the crashed-tab page; a page with sound shows the speaker,
   clicking it mutes the tab, the tab menu has Mute site; Settings → General → New tabs.
### Downloads, new tab, appearance (M5–M6)
5. A download flies to the toolbar button (pulses; reduced motion: badge only); the downloads
   popup and krunk://downloads show speed, ETA and per-connection segments for a large file.
6. New-tab page: shortcuts section, background image, tile editing, sections; Settings →
   Appearance changes accent, colours, density, font, tab shape and toolbar items live and after
   a restart.
### Permissions and site data (M7)
7. `scripts/vm-www/8094/perm.html`: camera, microphone, notifications, location, clipboard,
   local network, pointer lock each ask once; Settings → Permissions lists the answers; the lock
   popup shows them per site; the address bar chip shows camera/microphone in use.
8. krunk://storage: sortable columns, usage per site and totals, select and clear.
### Privacy (M8–M10)
9. Tracking protection Standard/Strict/Custom on `tracking.html`: the Strict-only probe
   (`cdn.ravelin.com`) is blocked only in Strict; referrer trimming and bounce cleanup per
   `docs/TRACKING_PROTECTION.md`.
10. Request headers: Accept-Language and the client hints edited in Settings → Advanced change
    `navigator.language` / `userAgentData` and the wire headers of a new tab alike (`ua.html`);
    Reset restores the engine's values.
11. Custom DNS: add a DoT/DoH/stamp resolver, Custom on; `pktmon` elevated on port 53 while
    browsing counts 0 packets; the Test buttons show latencies.
### Passwords and payments (M11–M12)
12. `signup.html`: the new-password field offers "Use a strong password", both fields fill, the
    login is saved on submit with the Undo/Edit toast; nothing in a private tab.
13. `checkout.html`: a typed card offers "Save this card?" (brand and last four only); after a
    restart the card field's dropdown unlocks and fills number, expiry and name, the CVC stays
    empty; the profile contains no plaintext number.
### Media and extensions (M13–M15)
14. `pip.html`: the PiP button; Alt+P opens the chooser for two videos; the floating window's
    Back to tab activates the tab; the setting "Automatic Picture-in-Picture" moves a playing
    video out on Ctrl+T and back on return.
15. Settings → Privacy → Protected content on, restart: `drm.html` plays the Shaka Widevine test
    stream; krunk://gpu shows the CDM version.
16. krunk://extensions: install `scripts/vm-ext/krunk-fixture.zip`; `ext.html` gets the banner
    and the blocked image; the toolbar popup pings the content script; Options opens a tab;
    krunk://newtab and a private tab get nothing; Disable removes the button; uBlock Origin Lite
    and Dark Reader install from their Web Store addresses.
### Website (M16)
17. On the site (`https://krunk.209.wf`) Krunk shows "You're using Krunk 0.6.0 — up to date";
    other browsers get the download box; `view-source:` of another site never has `window.krunk`.

## Krunk 0.7.0 (round 7)
Scripted on the VM: `scripts/vm-070-*.ps1` with the pages in `scripts/vm-www/` (evidence in
`.pipeline/evidence-0.7.0/m*/`). By hand, each in a fresh profile unless said otherwise:

### Engine and updates (M1)
1. krunk://gpu: one `--disable-features` on the engine command line; the media-codecs text says
   the engine decodes H.264/AAC and the rows agree; "Engine files: 232 checked, libcef.dll
   c4e38bbf…". Change one byte of `locales\fr.pak`, restart: the Repair prompt appears and Repair
   reinstalls the release.
2. Hold `libcef.dll` open (another program) and run the installer `/S`: it stops with exit code
   3 and changes nothing; Krunk's next start says why.
### Address bar and tabs (M2)
3. Drag over the address (focused and not focused) selects text; double click a word, triple
   click all; drag the site icon onto the bookmarks bar; click the page: the bar shows the
   formatted address; Ctrl+T, the + button and the menu: typing goes to the address bar.
4. Dark theme, a dark page and the new tab page: Ctrl+Tab 20 times — never a white frame; the
   same with a tab still loading and a discarded tab (`scripts/vm-070-flash.ps1` records it).
### New tab page and appearance (M4)
5. The new tab page with very long site titles at 800–2560 px wide and 100/125/150 %: no
   horizontal scrollbar, two lines and an ellipsis, the full title as tooltip.
6. Customize → a wallpaper (dark and light ones keep text readable); Settings → Appearance:
   colour pickers for accent, background, toolbar, tab strip, new tab page — live in a second
   window, kept after a restart, readable text on every colour.
### Media and downloads (M5–M6)
7. `pipvol.html`: Alt+P, the volume bar under the floating window; slider, wheel over the
   window, mute; the page's own controls move the bar; drag the window: the bar follows.
8. `dl7.html` (rangeserve.py): one slow connection — the download finishes about as fast as the
   others allow and the segment view shows the pieces moving; close Krunk during a download,
   start again: Paused; Resume finishes it, the SHA-256 matches; a file that changed on the
   server is downloaded again with one connection, saying why.
### Privacy and passwords (M7)
9. Settings → Advanced → Send client hints off: `hints.html` (hints-serve.py) logs no
   `Sec-CH-UA*` header and no Critical-CH retry; page, iframe and workers report
   `navigator.userAgentData` undefined (a shared worker still sees it); the same in a private tab.
10. krunk://passwords → Create recovery phrase (a test vault only): the words once, three asked
    back; Lock → Forgot master password? → the words → a new master password opens the vault;
    after a restart the old password fails; Make a new phrase: the old one is refused. The words
    appear in no file of the profile.
11. Change master password: labelled fields, show/hide, strength, an error under each field;
    light and dark. Menus and settings say "Manage passwords & payments".
### Polish (M8)
12. Links on krunk:// pages are never browser-blue and readable with a custom accent; the
    permissions page's descriptions line up with their headings; the login suggestion popup has
    room below its footer; the task manager's selected row is readable in dark mode.
### Extensions (M9)
13. On a Chrome Web Store page (after Google's consent page) the button says "Add to Krunk";
    the confirmation names the extension and what it can do, signed by the Chrome Web Store;
    Add installs it; nothing appears in Downloads. Dark Reader and uBlock Origin Lite this way.
14. An extension popup closes on Esc, a click on the page, the toolbar, a tab switch, its own
    button and a click on another program; a click inside keeps it open. krunk://extensions shows
    cards (light and dark) and an empty state.
### Import (M10)
15. A Firefox profile whose logins.json has entries without `hostname`: Import says "N imported,
    M skipped (no website address: …, unreadable entry: …)".

## Krunk 0.7.1 (round 8)
Scripted on the VM: `scripts/vm-071-*.ps1` (evidence in `.pipeline/evidence-0.7.1/m*/`).
1. `hints.html?poll` (hints-serve.py) open in a tab, Send client hints on; switch it off in
   another tab: without a reload the page and its cross-site iframe report
   `navigator.userAgentData` undefined; reload 5 times: still undefined. Switch on again and
   reload: the brands are back.
2. Send client hints off: the shared workers on `hints.html` (classic, `'use strict'`, module,
   `importScripts`) report undefined — the strict one still in strict mode; only the module's
   statically imported module reports the brands; normal and private tabs alike.
3. About: "Engine command line" shows one `--disable-features` with CEF's and Krunk's features.
4. krunk://passwords, locked: a wrong master password says "Wrong master password." during the
   short delay and after it, until you type again.
5. Start a download: the glyph flies to the downloads button evenly (`vm-071-flight.ps1`
   records it; `analyze.py track --ease 450` measures each frame against the curve).

## Krunk 0.7.2 (round 9)
1. **New tab:** with Settings → Headers → Accept-Language set, press Ctrl+T, click "+", and use
   the menu's New tab: the address bar never shows `about:blank` or any text, only its
   placeholder, and typing goes into it at once.
2. **Typing:** visit `example.com`, then type `exa` slowly in a new tab: the completion stays
   selected after each letter; Backspace removes it, Esc goes back to the typed text.
3. **Dialogs:** krunk://passwords → Add and Add card, in the light and the dark theme: labelled
   fields, a focus ring, an error under the field (empty password, month 13), Esc closes.
4. **Resizing:** in the dark theme drag a window edge out and back quickly: no white strip.
5. **Pointer lock:** in a game (or `jitter.html` of the VM scripts) hold W and turn: the turn is
   as even as without W.
6. **Bookmarks:** with more bookmarks than fit, "»" at the bar's end lists the rest; a narrower
   window moves more into it. A folder with a long title shows it on one line with "…" and the
   whole title in the tooltip; its right-click menu has icons. krunk://bookmarks with a very
   long address does not scroll sideways.
7. **Firefox import:** a Firefox profile with saved logins imports all of them; the log line
   `import: passwords …` names the profile, its logins and keys.
8. **Update notice:** with a newer version on the server, the ⋮ button shows a dot and the menu
   starts with "Update available – Krunk X.Y.Z", which opens the update prompt.

## Krunk 0.7.3 (round 10)
1. Dark theme, a GPU machine: switch quickly between a white page, a dark page and the new tab
   page (Ctrl+Tab, clicks on tabs): no white frame between them. Switch back to a tab within a
   moment: it stays shown.

## Krunk 0.7.4 (round 11)
1. GPU machine, dark theme: switch between several tabs (white pages, dark pages, the new tab
   page) for a minute with Ctrl+Tab and clicks: never a white frame. Resize the window, then
   switch: the page has the new size at once. Drag a tab into a new window and back.
2. Settings → Passwords & import → Firefox (a long-used profile): the passwords import; if any
   cannot be decrypted, the result shows the cipher and key sizes in brackets.
3. The new tab page: space below the keyboard shortcuts box.

## Krunk 0.8.0 (round 12)
1. **Dropdowns:** on a long page open a `<select>`, then turn the mouse wheel over the page: the
   list stays open where it is and the page doesn't move; the wheel over the list scrolls the list;
   Esc, a choice or a click elsewhere closes it and the next wheel scrolls the page. The same under
   a datalist's suggestions, the ⋮ menu and the history menus. With a precision touchpad too.
2. **History menus:** visit five pages, right-click Back: the four earlier pages and the first
   one, newest first; click the third: the third page back opens, Back and Forward are both
   enabled and right-click Forward lists the later pages in order. Arrows and Enter work; on a new
   tab a right-click on the disabled Back shows nothing. In a private window the same, and
   krunk://history shows none of those pages. Dark theme at 125 %: the menu fits.
3. **Account:** Settings → Account → Sign in: a code and a new tab with krunk.209.wf/link; sign in
   there, approve (password again): within seconds the card says "Signed in as …" and the ⋮ menu
   too; the device is in the website's device list. Sign out on this device: the card offers Sign
   in again and the website lists the device as removed. Without a network: one clear sentence.
   A private window's Settings shows nothing of the account.
4. **HTTP authentication:** a site behind nginx `auth_basic`: Krunk's sign-in dialog (realm, "Not
   secure" on http), Esc shows the site's 401 page, a wrong password says "not accepted", the
   right one opens the site and offers to save it; next time (after a restart) the dialog fills
   the saved login (after unlocking) and one click signs in.

## Krunk 0.9.0 (round 13)
The full regression above (rounds 1–12) because the profile formats change (`browser.db` v6,
vault format 4). The VM automation covers 39 of the 43 flows; then:
1. **Upgrade:** a profile used with 0.8.0 (bookmarks, history, a vault with logins and a card,
   signed in) opened by 0.9.0: everything is there, the vault opens with the same password and is
   rewritten only after sync is turned on with passwords.
2. **Sync stays off:** signed in, Settings → Sync (beta) says "Sync is off"; with nothing clicked,
   Krunk sends no sync request (Settings → Account's check is the only traffic).
3. **Set up:** Turn on sync… → the passphrase and the recovery key, Copy both / Save to a file,
   type the asked group, tick, Set up sync. Settings shows "Sync is on", the last sync, the
   kinds of data with switches, Sync now.
4. **Second device:** sign in, Turn on sync. (a) Ask my other devices → on the first device
   krunk://sync shows the request at the top; Approve (optionally sharing passwords) shows 6
   digits; type them → "approved", the bookmarks/history/settings arrive. A wrong code ends the
   request on both sides. (b) Restore with the recovery key (the field has the focus; Enter) →
   the list of devices that keep access (Enter confirms) → on; passwords arrive only into an
   unlocked vault, and only after it is unlocked.
5. **Changes both ways:** add, rename, move and delete bookmarks on either device; visit pages;
   change the theme; pin a site; add an ad-blocker exception: the other device shows them after
   Sync now. The same login changed on both: the later wins.
6. **Remove a device** in krunk://sync: confirm; "Securing…", then the removed device is signed
   out and can't read anything new; the remaining devices keep syncing; Finish securing with the
   passphrase.
7. **Turn off** on a device (keep data / remove synced data); private windows show nothing of
   sync.

## Krunk 0.10.0 (round 14)
The regression above (rounds 1–13) because the engine changes (`libcef.dll` with
`krunk-windowed-hidden.patch`): the hidden-tab flow (`scripts/vm-0100-spike.ps1`, checked by
`scripts/perf/spike-cases.py`), 060-pip, 070-pip, 070-flash and the 39 other flows on the release
build, and the 0.9.0 → 0.10.0 in-app update through staging. Not run in 0.10.0 because the
operator asked for time efficiency: 090-upgrade, 090-sync (sync and vault code is unchanged; a
core test covers the batched history titles with sync tracking) and 050-lna. 072-ui's update
check drives the 0.8.0 dev build and had no staging tunnel; the staging update path covers 0.10.0.
Then:
1. **Many tabs:** open 30 busy tabs (`scripts/vm-0100-tabs.ps1`, or real sites): the PC stays
   responsive, Task Manager shows Krunk at a few percent CPU when idle, typing and scrolling in
   the active tab are smooth, switching tabs is instant.
2. **Background tabs are hidden:** a page reporting `document.visibilityState` says `hidden`
   behind another tab and `visible` in front; its animation stops and its timers tick once a
   second; minimising hides the active tab too, restoring shows it at full speed again.
3. **What goes on in a background tab:** music keeps playing; a video call keeps sending and
   receiving; a download finishes; a site's notification appears; a picture-in-picture video
   keeps playing.
4. **Coming back:** every tab shows its page at once, with no white or blank frame (dark theme
   too), the text typed into a form and the scroll position kept; a discarded or sleeping tab
   reloads or wakes as before; DevTools of a tab is still there after switching away and back.
5. **Tab strip with busy titles:** pages that change their title or icon every second (mail,
   chat) don't make the tab strip or the toolbar flicker or lag.
6. **Update from 0.9.0:** the in-app update downloads the larger package (new engine) and
   installs it; krunk://gpu shows the engine files as this release's after the restart.


## Krunk 0.10.1 (round 15)
The operator reported that the first unlock from a login form listed every saved password. The
dropdown offered every login of the registrable domain (a vault with many subdomains of one
domain got all of them on each site), and the list after the unlock had no host labels and no
limit. Now a site with logins of its own gets only those, and the list after the unlock is the
same as on a later focus. Only the login suggestions changed: core tests cover the matching;
on the VM `scripts/vm-0101-unlock.ps1` (one unlock from the dropdown with logins on several
subdomains). Nothing else was rerun (the engine and the rest of the host are 0.10.0's).
1. **Unlock from a login form:** save logins on two subdomains of one domain and one other site,
   restart, click into the first subdomain's password field, unlock: it fills its own login (or
   lists only its own logins), never the other subdomain's or the other site's.
2. **Sister subdomain:** on a third subdomain without logins, the dropdown lists the domain's
   logins, each with its host.
3. **Account page (server):** after an update, krunk.209.wf → Account → Devices shows the new
   version for the device once it has synced (the server takes it from the `Krunk/<version>`
   user agent at most once a minute; it used to keep the version from the time of linking).

## Krunk 0.10.2 (round 16)
The operator asked for address-bar suggestions from the first letter, with the site first:
"yo" offered a YouTube video, not youtube.com. Only the suggestions changed (core tests cover the
rows and the completion; the shell is unchanged). On the VM: `scripts/vm-0102-omnibox.ps1`.
1. **Site first:** with a few YouTube videos in history, type `y`: the bar completes `youtube.com`
   (selected after the caret), the top row is YouTube's home page, the search for "y" next, then
   the most visited videos. Enter opens https://www.youtube.com/.
2. **Pages:** type `youtube.com/w`: the bar completes the most visited video's address with its
   own upper and lower case; Enter opens that video.
3. **Words:** `yo yo` is a search: nothing is completed.

## Krunk 0.11.0 (round 17)
The operator's batch: bookmarks bar, favicons, media/PiP, unlock, tab overflow, settings, the account
page, plus pages that hang, the source viewer and search for a selection. The engine is 0.10.0's
(libcef d9f4549b). On the VM: `scripts/vm-0110-a.ps1` (main, restart), `vm-0110-b.ps1`,
`vm-0110-c.ps1`, `vm-060-shell.ps1`, `vm-070-engine.ps1`, `vm-0110-update2.ps1` (staging) and
`vm-0110-update-live.ps1`. Not rerun: the other flows (their code did not change; see
`.pipeline/evidence-0.11.0/vm-minutes.md`).
1. **Folder dropdowns:** click two bar folders in turn, quickly: each dropdown shows its own folder
   from the first frame. In "»", open a folder: its first row is "‹ <parent>"; Backspace and Left go
   back. Middle-click a bookmark in a dropdown: a tab opens behind, the dropdown stays open.
2. **Icons of never-visited bookmarks:** bookmark (or import) sites you never opened: about 30 s
   after the start they get their icons; `krunk.log` has one `favicon-probe` line per site, and
   after a restart none (a site without an icon is tried again only after 7 days).
3. **60 tabs:** ‹ › and ▾ appear; the wheel scrolls the row; ▾ lists and filters every tab;
   middle-click closes a tab in the scrolled row (no scroll cursor).
4. **Pages that hang:** open a page that busy-loops, click into it, wait ~16 s: "This page isn't
   responding" (Wait / Close page); Close page shows "This page stopped" with Reload. Other tabs and
   the address bar stay smooth meanwhile, also while a page takes 30 s to load.
5. **Source and search:** Ctrl+U opens Krunk's viewer (not Notepad); Beautify and Wrap lines are
   remembered. Select a word, right-click: "Search <engine> for “…”".
6. **Unlock:** "Remain unlocked this session" is checked by default; checked, the vault stays open
   past the auto-lock time; unchecked, it locks; the choice is remembered.
7. **PiP volume:** in the floating video, point at its mute button: a small slider opens next to it
   inside the window; nothing shows over the rest of the video.
8. **Installed copy:** after the update, Start menu → Krunk.lnk carries the app ID `Krunk.Browser`
   (and the registry has `HKCU\Software\Classes\AppUserModelId\Krunk.Browser`); the media overlay
   names Krunk where the machine shows media controls (not visible on the VM, which has no audio
   device). Krunk starts normally after the update (0.11.0 before release crashed here).
9. **Cookies and site data, account:** large lists page in as you scroll, search and sort cover
   all; Settings → Account shows "This device: Krunk on Windows 11 · version 0.11.0".
