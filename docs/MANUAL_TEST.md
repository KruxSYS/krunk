# Manual test checklist (Windows 10/11 x64)

*Draft; the complete checklist (all screenshots, RAM measurements) comes with packaging.*

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
