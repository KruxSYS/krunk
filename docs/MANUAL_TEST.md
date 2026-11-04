# Manual test checklist (Windows 10/11 x64)

Run these on a real Windows 10 or 11 x64 machine with the release `LightBrowser.exe`.
Sections 1–3 are the release smoke test. The remaining sections cover each feature in detail.
Last full pass: 2026-11-03 on a Windows 11 Pro VM (build 26200, WebView2 156.0.4314.58),
driven with `scripts/vm.sh`.

## 1. Install and first run
1. Check the hash: `Get-FileHash LightBrowser.exe` matches `SHA256SUMS`.
2. **Without the WebView2 runtime:** on a machine without it (for example Windows Sandbox
   after uninstalling it, or an old Windows 10 image), starting the exe shows the "LightBrowser – WebView2
   Runtime required" dialog with the download link, and offers to open the page. Nothing
   else happens.
3. **With the runtime:**
   - The window opens with one new tab and the address bar focused.
   - `%LOCALAPPDATA%\LightBrowser\` is created with `browser.db`, `engine.dat`, `profile\`,
     `bin\WebView2Loader.dll` and `lightbrowser.log`.
   - The log shows the runtime version and "filter engine loaded from cache" (on the second
     start).
4. Only one `LightBrowser.exe` process runs. All other processes are `msedgewebview2.exe`
   children (`scripts/vm-mem.ps1`). There are no services and no scheduled tasks.

## 2. Release screenshot set (AC4)
Take these with the final exe. The reference set from the 2026-11-03 pass is in the pipeline
workspace under `screenshots/final/`.

| Shot | How |
|------|-----|
| `final-newtab` | Ctrl+T after browsing a few sites: tiles with favicons, address bar focused |
| `final-tabs` | 10+ tabs open, a bookmarked page (filled star), bookmarks bar |
| `final-shield` | A news site (theguardian.com, about 13 blocked), then click the shield: popover with the count and "Allow ads on this site" |
| `final-save-password` | `https://the-internet.herokuapp.com/login`, sign in as tomsmith / SuperSecretPassword!: "Save password?" popup |
| `final-passwords` | After "Save" and creating the vault: the entry on lb://passwords, masked |
| `final-settings` | lb://settings |
| `final-history` | Ctrl+H: today's visits with favicons |
| `final-bookmarks` | Ctrl+Shift+O: the bookmarks bar folder with entries |

After the save-password shot, close the browser and run
`scripts/vm-leak-scan.ps1 -Needles 'SuperSecretPassword','<your master password>'`.
Expected: `hits=0`.

## 3. Memory figures
Follow "Memory (M8)" below and record:
- 1 tab
- 10 sites awake
- 10 sites after sleep/suspend
- after discard

The 2026-11-03 figures are in the README under "Memory".

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
   - `lb://history` opens History.
   - `javascript:alert(1)` typed there becomes a search; it is never run.
9. A web page can't navigate to `https://lightbrowser.invalid/settings.html` or
   `https://shell.lightbrowser.invalid/shell.html`. The navigation is blocked, and the log has
   "blocked navigation to an internal or unsupported URL".
10. Every shortcut in the README's Usage table works with focus in the page, in the address
    bar, and after clicking the title bar.

## Popup window
1. At 100 % display scaling, open the ⋮ menu. It appears directly under the button, right-aligned,
   fully on screen, with all items visible (down to "Exit").
2. Set Windows display scaling to 150 % (Settings → System → Display → Scale) and restart
   LightBrowser. Repeat step 1: the popup is still aligned under the button and is not cut off.
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
4. Built-in pages (lb://settings, the new-tab page) never appear in History.
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
   - Every request goes to `lightbrowser.invalid` (HTML, CSS, JS, `/favicon?host=`). None goes
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
   - Open 5 tabs, then close the window. Start LightBrowser again: the 5 tabs come back with
     their titles.
   - Only the active tab loads. The others are dimmed and load when clicked.
   - Private tabs are not restored.
   - Kill the process from Task Manager about 3 seconds after changing tabs. The session is
     still restored (it is saved about 2 seconds after each change).

## Ad and tracker blocking (M5)
0. **Updater smoke test (no window):**
   - Run `LightBrowser.exe --update-filters` in a terminal. The exit code is 0.
   - `%LOCALAPPDATA%\LightBrowser\lightbrowser.log` ends with `filter update: downloaded [...]`
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
   - Those requests show status 403 with an empty response.
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
8. **Tracking prevention:**
   - `edge://settings` is not available in WebView2. Instead, open a site that embeds
     third-party trackers (such as a news site) and check DevTools → Application → Cookies:
     third-party tracker cookies are absent.
   - Strict tracking prevention is the default.
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
   - `%LOCALAPPDATA%\LightBrowser\vault.json` appears. Open it in Notepad: no usernames,
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
    - Copy `vault.json` into `%LOCALAPPDATA%\LightBrowser\` of a *different* Windows user
      account (or another PC), with LightBrowser.exe alongside.
    - Unlocking with the **correct** master password fails with "This vault can only be opened
      by the Windows account (and PC) that created it."
13. **Leak check:** search `lightbrowser.log` and `browser.db` (DB Browser for SQLite, or
    `findstr /s /i` over the data dir) for the test password: there are no hits.

## Settings (M7)
Open ⋮ → Settings, or type `lb://settings` in the address bar.
1. **Search engine:** choose Google: address-bar searches go to Google. Choose Custom with
   `https://search.example/?q=` (no `%s`): an inline error appears and the setting is not saved.
   Add `%s` and searches use the template.
2. **Startup:**
   - "Open the homepage" with an empty or `ftp://` homepage gives an inline error.
   - A valid homepage opens on the next start.
   - "Continue where you left off" restores the tabs; only the active one loads.
3. **Hardware acceleration:** untick it and the "Restart required" banner appears. Click
   "Restart now": the browser closes and reopens with the same tabs. `lb://about` → "Browser
   arguments" now contains `--disable-gpu`.
4. **Sleep and discard:** sleep 0 is accepted (never). Discard enabled with a time ≤ the sleep time
   gives an inline error. Values above 1440 give an error.
5. **Ad blocking:**
   - Untick "Block ads and trackers": the shield counter stays at 0 on a news site.
   - Untick EasyList: a rebuild happens (the log says "filter engine rebuilt").
   - "Update now" shows the last check time.
   - Add `bad host` to the allowlist: inline error. Add `example.com`, then remove it.
6. **Privacy:**
   - Tracking prevention levels apply live.
   - HTTPS-only: `http://example.com` opens as https.
   - DNT/GPC unticked: `https://httpbin.org/headers` no longer shows `Dnt` / `Sec-Gpc` (headers
     change immediately; `navigator.*` on new tabs).
   - SmartScreen shows the restart banner.
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
    - User agent `Test/1.0`: `https://httpbin.org/headers` in an existing tab shows it after a
      reload; clearing the field restores the default.
    - Proxy `not a proxy` gives an inline error; `127.0.0.1:8888` shows the restart banner.
    - Extra flags: `--no-sandbox`, `"--x"` and `--a=` are refused inline; `--force-dark-mode`
      shows the banner.
11. **Zoom and theme:**
    - Default zoom 125 %: new tabs open at 125 %.
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
    "lightbrowser.invalid says" box; the same applies to every delete prompt on the built-in
    pages). After "Reset", every value is back to its default, and the
    restart banner appears if restart-only values were different.
15. **Isolation:** only `lb://settings` can change settings. In DevTools on any website,
    `chrome.webview.postMessage({cmd:"saveSettings",settings:{}})` has no effect.

## Memory (M8)
Use `scripts/vm-mem.ps1` (PowerShell) to list the process tree with each process's type and
working set. With `-CommandLine` it also prints the WebView2 browser process command line.
1. **Flags at runtime:** start normally and run `vm-mem.ps1 -CommandLine`. The browser command
   line contains:
   - `--no-pings`
   - `--disable-domain-reliability`
   - `--disable-features=AutofillServerCommunication,…,msSmartScreenProtection`

   After turning SmartScreen on and restarting, `msSmartScreenProtection` is gone.
2. **Lazy restore:**
   - Set "Continue where you left off", open 5 sites, close and start again.
   - Every tab shows its title and favicon, but the background tabs are dimmed. `vm-mem.ps1`
     shows 2 renderers: the shell and the active tab.
   - Click one restored tab: it loads, and there are 3 renderers.
3. **1-tab baseline:** start with `--mem-report` and one new tab. Wait 10 s and note the last
   `tick:` line of `%LOCALAPPDATA%\LightBrowser\memory-report.log`
   (about 380 MB working set, 2 renderers).
4. **Sleep and suspend** (test scale):
   - In Settings, turn on "Discard background tabs".
   - Start with `--sleep-test-seconds=90 --mem-report` (sleep 90 s, suspend 90 s later,
     discard 360 s after the tab was last active). `lightbrowser.log` has "tab sleep test mode".
   - Open 10 real sites, one per tab. Note the `tick:` line (about 2.1 GB working set,
     15 renderers).
   - Do not touch the browser for about 3 minutes. Background tabs dim. `lightbrowser.log` has
     "tab N: low memory", then "tab N: suspended" for each background tab. The mem-report shows
     `suspended=10` and roughly half the working set.
   - Never slept:
     - the active tab
     - a tab playing audio (start a YouTube video in a background tab)
     - a tab with a download in progress
5. **Wake:** click a dimmed tab. It shows at once with no reload, and the log has
   "tab N: woken (Suspended)".
6. **Discard:**
   - About 6 minutes after a tab was last active, the log has "tab N: discarded". The
     mem-report shows `unloaded=` rising and fewer processes and renderers.
   - Clicking a discarded tab reloads its URL. Its back history is gone, which is expected.
7. **Private discard:**
   - Open two private tabs and log in to a site in one of them. Let the other one be
     discarded.
   - The discarded private tab shows only its host as the title, with no favicon. Activating it
     reloads it in the same InPrivate session, so you are still logged in.
   - The last live private tab is never discarded.
8. The test options are not in the Settings UI. Without `--sleep-test-seconds`, the timers are
   the settings values in minutes.
