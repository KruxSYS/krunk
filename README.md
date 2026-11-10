# Krunk

Krunk is a lightweight, privacy-minded web browser for Windows 10/11 (x64), written in
Rust. It brings its own engine: **Chromium 156.0.8078.80** through the Chromium Embedded
Framework (**CEF 156.0.15**), bundled in the download. Neither Microsoft Edge nor WebView2 is
used. It installs per user without administrator rights, or runs portable from a zip. It includes:
- tabs and private tabs, in as many windows as you like (drag tabs between them)
- history and bookmarks
- a new-tab page with your most-visited sites, built-in wallpapers and your own colours
- built-in ad and tracker blocking
- an encrypted password vault for logins and payment cards, with an optional recovery phrase
- downloads, find, zoom, print and DevTools docked next to the page
- session restore
- background-tab sleep, to save memory
- a Krunk account to sign the browser in (0.8.0; sync comes in a later version)

## Download and requirements
- **Installer:** `Krunk-Setup-0.8.0.exe` (about 138 MB), from `https://krunk.209.wf`. It installs for the current user
  only, without administrator rights:
  - to `%LOCALAPPDATA%\Programs\Krunk`, with a Start Menu shortcut and, if you tick it, one on
    the desktop;
  - with an entry in Settings → Apps → Installed apps (per user, `HKCU`).
  If Krunk is running, the installer asks you to close it. `Krunk-Setup-0.8.0.exe /S` installs
  silently; it then closes a running Krunk itself and waits up to 30 s for it.
  **Uninstalling** removes the program and the shortcuts. Your data (`%LOCALAPPDATA%\Krunk`) is
  kept unless you tick "Also delete my Krunk data"; a silent uninstall (`/S`) always keeps it.
- **Portable:** `Krunk-win-x64.zip` (about 185 MB; 426 MB unpacked). Extract it anywhere and
  run `Krunk.exe`. Keep all files of the folder together; if one is missing, Krunk says so at
  startup. The portable copy uses the same data folder as the installed one.
- **Verify:** check `SHA256SUMS` (the installer, the zip and the three main binaries):
  - PowerShell: `Get-FileHash Krunk-Setup-0.8.0.exe`
  - Linux: `sha256sum -c SHA256SUMS`
- **System:** Windows 10 or 11, 64-bit. No runtime is needed.
- **Data location:** everything is stored in `%LOCALAPPDATA%\Krunk`:
  - `browser.db` (history, bookmarks, settings, session)
  - `vault.json` (encrypted passwords)
  - `account.json` (0.8.0, only while signed in to a Krunk account: the refresh token is DPAPI-wrapped)
  - `engine.dat` (the compiled filter engine)
  - `profile\` (the engine's profile: cookies, cache and site data in `profile\default`)
  - `krunk.log` (this session; the previous one is `krunk.old.log`), and `cef.log` (engine
    warnings and errors only)
  - `update.json` (when the last update check ran and what it found)

  Delete that folder to reset the browser completely.
- **Coming from LightBrowser (Krunk's former name):** on its first start Krunk moves
  `%LOCALAPPDATA%\LightBrowser` to `%LOCALAPPDATA%\Krunk` with everything in it, including
  the engine profile (cookies and logins), the vault and `engine.dat`. It renames the folder
  when it can, and otherwise copies it and then deletes the old one. `krunk.log` records what
  happened. If LightBrowser is still running, Krunk asks you to close it first. If a Krunk
  folder already exists, the LightBrowser folder is left alone. Old `lb://` addresses open the
  matching `krunk://` page, and saved tabs and bookmarks of built-in pages keep working.
  The old name remains on purpose in only these places:
  - two vault constants that existing vaults depend on: the key-check plaintext
    `"LightBrowser vault key check v1"` and the DPAPI entropy `"LightBrowser vault key wrap v1"`
    (changing them would make saved passwords unreadable);
  - the migration itself: the `LightBrowser` folder name, the `LightBrowserWindow` class check
    and its messages;
  - the `lb://` and `https://lightbrowser.invalid/` redirect for old saved tabs and bookmarks;
  - internal identifiers no user sees: the crate name `lb_core` and message names (`lb.*`,
    `__lbHost`).
- **Coming from Krunk 0.2.x:** installing 0.3.0 over it keeps all your data. The saved
  session (one window's tabs) opens as one window; from then on Krunk saves every window with its
  tabs, position, size and maximized state. The old session fields are still written (for the
  most recently used window), so going back to 0.2.1 opens that window's tabs. The installer
  removes the `README.md` and `SECURITY.md` that 0.2.x put into the program folder: the package
  ships no Markdown files any more (this README and `docs/SECURITY.md` are in the source and
  on the website).
- **Coming from the WebView2 version:** settings, history, bookmarks, the session, the vault and
  `engine.dat` are used as they are. Cookies and site data are not migrated: the old
  `profile\EBWebView` folder (WebView2's profile) is left untouched and unused, and you can
  delete it. The same goes for the old `bin\` folder (the WebView2 loader).
- **Unsigned binaries:** the files (including the installer) are not code-signed, so
  SmartScreen may warn the first time they run. Updates are protected by their own signature
  instead (see "Updates" below).

## Usage
Type a URL or search words in the address bar. Search uses DuckDuckGo by default; this can be
changed in Settings. Built-in pages:
- `krunk://settings`
- `krunk://history`
- `krunk://bookmarks`
- `krunk://passwords`
- `krunk://downloads`
- `krunk://about` (shows the CEF and Chromium versions of the running engine)
- `krunk://storage`: cookies and site data (also in the main menu and Settings → Privacy)
- `krunk://sitedata`: one site's cookies, local and session storage (shield → Cookies and site data)
- `krunk://adblock`: what the ad blocker blocked on a page, the filter lists, and your own filters
- `krunk://gpu`: whether pages are hardware accelerated (WebGL/WebGPU adapter, the engine's
  switches, the GPU process), with a link to Chromium's `chrome://gpu`
- `view-source:<address>` (0.11.0): the page's source in Krunk's viewer (see below)
- `krunk://scripts` (0.12.0): userscripts and macros with their code editor (see below)

Old `lb://` addresses (from LightBrowser) open the matching `krunk://` page.

| Keys | Action |
|------|--------|
| Ctrl+T / Ctrl+Shift+N | New tab / new private tab |
| Ctrl+N | New window |
| Ctrl+W, Ctrl+Shift+T | Close tab (the window closes with its last tab), reopen closed tab (in the current window) |
| Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+1…8, Ctrl+9 | Next / previous tab, tab by position, last tab |
| Ctrl+L, Alt+D, F6 | Address bar |
| F5 / Ctrl+R, Ctrl+F5 / Ctrl+Shift+R | Reload, reload bypassing the cache |
| Alt+Left / Alt+Right, Alt+Home | Back / forward, home page (there is no Home button) |
| Ctrl+D | Bookmark this page |
| Ctrl+U | View page source (0.11.0) |
| Ctrl+H, Ctrl+Shift+O, Ctrl+J | History, bookmarks, downloads (switches to the page if it is already open) |
| Ctrl+Shift+B | Show or hide the bookmarks bar |
| Ctrl+F, F3 / Shift+F3 (Ctrl+G / Ctrl+Shift+G) | Find, next / previous match |
| Ctrl+= / Ctrl+-, Ctrl+0 | Zoom in / out, reset |
| Ctrl+P | Print |
| F12, Ctrl+Shift+I | DevTools, docked in the tab's window (press again to close) |
| Ctrl+Shift+C, right-click → Inspect | DevTools with the element under the mouse selected |
| Alt+Space | Window menu (move, size, minimize, maximize, close) |
| Ctrl+Shift+Space | Show or hide the AI agent panel (0.12.0; only while AI features are on, else the page gets the key) |
| Ctrl+Shift+. | Stop the running macro (0.12.0; only while one runs) |

- **Settings** (`krunk://settings`) has tabs: Account (0.8.0), General, Privacy & security,
  Passwords & import, Downloads, Appearance, AI and scripts (0.12.0), Advanced. The search box
  finds a setting on any tab (a word of one or two letters, like "AI", matches word starts);
  `krunk://settings#privacy` (and the other tab names) opens that tab. Advanced → User agent
  shows the browser's own string with Copy, and a list of presets (Chrome, Edge, Firefox,
  Safari on Windows, macOS, iPhone, Android; versions as of November 2026) or a custom string;
  the choice applies at once to new navigations.
- **Back and forward history (0.8.0):** right-click Back for the tab's earlier pages, newest
  first, or Forward for the later ones, at most 15 each, with the site's icon (a globe without
  one) and the title (or the address). A click jumps to exactly that page, the same as pressing
  Back or Forward that many times; "Show full history" opens `krunk://history`. Up/Down, Home/End,
  Enter and Esc work in the menu. A disabled button shows no menu. The list is read from the
  engine each time and is never stored (also not in private windows).
- **Dropdowns keep the page still (0.8.0):** while a `<select>` list, a datalist's suggestions or
  one of Krunk's menus (⋮, the history menus, tab and bookmark menus) is open, the mouse wheel
  over the page does nothing, so the list stays where its field is; the wheel over the list
  scrolls the list, and keys go to it. Choosing, Esc or a click elsewhere closes it and the next
  wheel scrolls the page again. Chrome 157 and Edge close the dropdown on the first wheel notch
  instead (measured on Windows 11). Precision touchpads (DirectManipulation) are handled the same
  way in code but could not be tested here.
- **Sites that ask for a password (HTTP authentication, 0.8.0):** a site protected with Basic or
  Digest authentication (for example nginx `auth_basic`) gets Krunk's sign-in dialog: the site's
  address, its realm as plain text, "Not secure" over http, and your saved logins for exactly
  that address (one fills the fields, several are listed, a locked vault offers to unlock).
  Nothing is sent before you click Sign in; the saved password stays inside Krunk. Esc or Cancel
  shows the site's own 401 page. A user name and password that work are offered for saving with
  the usual "Save password?" (not in private windows). Proxy authentication is not supported yet
  (0.8.1), and a background tab that asks is refused (reload it to sign in).
- **Appearance (0.7.0):** colour pickers (presets, any colour, a hex field, Reset) for the
  accent, the window background, the toolbar, the tab strip and the new-tab page; changes show
  at once in every window, and text on a custom colour switches between dark and light to stay
  readable (WCAG AA where a colour allows it). The new-tab page has ten built-in wallpapers
  (made for Krunk, see THIRD-PARTY-NOTICES) besides your own picture or a colour, chosen in its
  Customize panel or in Settings → Appearance; long site titles on its tiles wrap to two lines.
- **Address bar:**
  - **Selecting (0.7.0):** dragging over the address selects text (also when the bar was not
    focused), a double click selects a word, a triple click everything; the site icon drags the
    address. A click on the page leaves the bar and shows the formatted address again; new tabs
    type into the address bar.
  - **Suggestions while you type** (0.5.1): a dropdown under the bar offers the search (or the
    typed address) first, then matching bookmarks and history pages, most visited first. Down
    and Up move through them (the bar shows the row's address), Enter or a click goes there,
    Esc closes the list and keeps what you typed; the bar keeps the focus throughout.
  - The lock (or the "i" on http) opens the site-information popup: whether the connection is
    secure, the protocol, the certificate chain with validity, names, algorithms and SHA-256
    fingerprints, and what is wrong with a bad certificate (the engine reports no cipher).
  - **Inline completion (0.6.0):** a visited address that starts with what you type is completed
    after the caret and selected. Since 0.7.2 typing on along it keeps it in place: the bar never
    shows only the typed letters or empties between keystrokes. Backspace or Delete removes the
    completion, Esc goes back to what you typed. Since 0.10.2 it starts at the first letter, and
    while you type a site's name it completes the site itself (`yo` → `youtube.com`, the visited
    site with the most visits), never one of its pages; that site is the top row with its most
    visited pages under it. Type on past the host (`youtube.com/w`) to complete a page.
  - After a navigation the bar shows the new address even if it had the focus; text you typed
    is kept only until you navigate. A new tab's bar is empty with its placeholder from the
    first frame (0.7.2: also when it starts from `about:blank`, as tabs do with edited request
    headers or the Blank new-tab setting; `about:blank` shows as the empty bar).
  - Drag the site icon or the (unfocused) address onto the bookmarks bar or into a folder to
    bookmark the page there; selected text dropped on the bar is bookmarked if it is a URL.
  - The ⋮ menu, the shield and the lock toggle: a click on the button closes an open popup.
  - The address of the link under the mouse shows at the bottom left of the page (bottom right
    when the mouse is there), as in Chrome.
- **Window:** there is no title bar. Drag the window by empty space in the tab row, and
  double-click there to maximize or restore. Hovering the maximize button shows the Windows 11
  snap layouts. Right-click the tab row for the window menu.
- **New tab** (Ctrl+T, "+", the menu) puts the cursor in the address bar, so you can type at once;
  the page loading underneath does not take it away (0.7.2).
- **Windows and tabs:**
  - **Dragging:** drag a tab sideways to reorder it: the tab follows the mouse and the other
    tabs slide aside (with "reduce motion" they jump). Drag it down out of the tab row and it
    becomes a new window that follows the mouse; over another window's tab row the window steps
    aside and the row opens a gap with the tab in it where it will land (0.11.1); let go to move
    it there, or move away and the window comes back under the mouse. Esc while dragging puts it
    back.
  - **A window's only tab** drags the whole window; dropping it on another window's tab row
    merges it into that window and closes the empty one.
  - **No reload:** a moved tab keeps its page as it is: scroll position, typed text, history,
    playing media, DevTools and private mode.
  - **Right-click a tab** for Reload, Duplicate, **Move to new window** (not for a window's only
    tab), **New window**, Close tab, **Close other tabs** and **Close tabs to the right**.
    **Ctrl+N** and the main menu also open a new window. Each closed tab can be reopened with
    Ctrl+Shift+T.
  - **Closing animation:** a closed tab shrinks away. While the mouse stays on the tab row the
    other tabs keep their width, so the next close button is under the mouse; they spread out
    when the mouse leaves. With "reduce motion" set in Windows there is no animation.
  - **Many tabs (0.11.0):** tabs shrink to 72 DIP (the active one keeps 120 with its close
    button); past that the row scrolls: the mouse wheel and the ‹ › buttons at its ends
    (click for most of a row, hold to keep going) scroll it smoothly, and its edges fade where
    more tabs are. **▾** lists every tab of the window: type to filter by title or address,
    Up/Down and Enter or a click to switch, × or a middle-click to close; the list stays open
    and follows. The arrows and ▾ show only while the tabs do not fit. The active tab comes
    into view when it changes; a title change never scrolls the row.
  - **Middle-click** closes a tab also when the row is scrolled (0.11.0: on an overflowing row
    the press used to start the engine's autoscroll, which took the click); a middle press on
    the tab row, the bookmarks bar or Krunk's menus never starts autoscroll. Web pages keep
    their middle-click autoscroll.
  - **Merging:** a tab dropped on another window's tab row lands in that window, which comes to
    the front.
  - **Closing:** closing a window's last tab closes the window, and closing the last window
    quits Krunk.
  - **Restoring:** every window is restored with its tabs, position, size and maximized state.
    A window that would be off screen (a monitor was removed) is moved onto a monitor.
- **DevTools** (F12) opens inside the tab's window, docked right by default:
  - **Resizing and docking:** drag the splitter between page and DevTools to resize it. The
    strip above DevTools (and the control in the toolbar) docks it right or at the bottom, or
    closes it. Side and sizes are remembered.
  - **One per tab:** each tab has its own DevTools. It hides when you switch tabs, comes back
    with the tab, moves with it to another window and closes with it. Page full screen hides it;
    F11 keeps it.
  - **Keys:** while DevTools has the focus, Krunk's shortcuts still work (including F12 and
    Ctrl+Shift+I to close it). The exceptions are the keys DevTools uses itself: Ctrl+F, Ctrl+G,
    Ctrl+P, Ctrl+O, Ctrl+[ / Ctrl+], Ctrl+Shift+F, Ctrl+Shift+P, Ctrl+Shift+C and Esc.
  - **Memory:** a tab with DevTools open is never put to sleep or unloaded.
- **Bookmarks bar:**
  - **Showing it:** right-click the bar to show it always, only on the new-tab page, or never
    (Ctrl+Shift+B toggles; also in Settings → Appearance).
  - **Context menu:** right-click a bookmark to open it (here, in a new tab or a new window),
    edit its name and address, or delete it. Right-click a folder to open all, rename or delete
    it, and empty space to add the page or make a folder. Its items have icons in one column,
    like Krunk's other menus (0.7.2).
  - **Folders:** a folder opens a dropdown; long titles stay on one line and end in "…", and the
    tooltip shows the whole title and the address (0.7.2).
  - **More than fits (0.7.2):** the bar shows the bookmarks that fit completely and a "»" button
    at its right end with the rest (folders open inside, like any folder dropdown). It is
    recalculated when the window is resized and when bookmarks are added, removed or moved.
  - **Back (0.11.0):** a subfolder opened inside a dropdown (or "»") has "‹ <folder>" as its
    first row to go back up; Backspace and Left do the same, Right opens a subfolder, Up/Down
    move, Esc closes the whole dropdown.
  - **Middle-click** on a bookmark (on the bar or in a dropdown) opens it in a new tab behind
    the current one; a dropdown stays open (0.11.0; the bar opened it in front before). Folders
    have no middle-click.
  - **Dropdowns open with their own content (0.11.0):** a dropdown (and every other Krunk
    popup) opens once its content is drawn, so it never shows the previously opened folder for
    a moment.
  - **Icons of bookmarks you never visited (0.11.0):** Krunk stores a site's icon when a tab
    shows it, so imported bookmarks used to keep a plain dot. Now, 30 s after the start and
    right after you add a bookmark, Krunk fetches each such site's icon once in the background:
    its `/favicon.ico`, else the icon its home page names (`<link rel="icon">`). The request goes
    through the engine's network settings (proxy and secure DNS as for pages), sends no cookies
    or credentials and no referrer, gives up after 8 s and 256 KB (512 KB of HTML), and at most
    4 run at once. A site without an icon is tried again only after 7 days, or when you bookmark
    it again; a bookmark saved in a private window is never probed. Each probe is one line in
    `krunk.log` (`favicon-probe host=… result=…`).
  - **Dragging:** drag bookmarks to reorder them, or onto a folder to put them in it; dropped on
    "»" a bookmark goes to the end of the bar. In a folder's (or "»"'s) dropdown, drag an item
    to reorder it, onto a subfolder to move it there, or out of the dropdown onto the bar.
    Right-click an item there for its menu.
- **Shield:** the shield in the toolbar shows how many ads and trackers were blocked on the
  page, and lists the latest blocked addresses. Click it to allow ads on that site, or to open
  **Cookies and site data** for the site.
- **Ad blocking page** (`krunk://adblock`, the shield's "Show all", Settings → Privacy):
  - **Blocked on this page:** every blocked request of the tab it was opened from, grouped by
    host, with its type, the filter that matched and the list it comes from. The list is kept in
    memory only (at most 500 entries per tab) and starts again when the tab navigates.
  - **Filter lists:** every list with its address, number of filters and last update. Built-in
    lists can be turned off and on, and the default choice restored. Add a list by its https
    address: it is downloaded and checked at once, and not added if that fails. Lists you added
    can be removed. "Update now" downloads them all.
  - **My filters:** your own rules in Adblock Plus / uBlock Origin syntax, with a report of lines
    it cannot read. Ctrl+S saves.
  - Every change takes effect without a restart, once the engine is rebuilt (a few seconds).
- **PDFs (0.11.3)** open in the engine's PDF viewer, also when a site sends them as a download
  (`application/pdf`, or a `.pdf` name with a generic type). Krunk fetches the file into its data
  folder (`pdf-view`, private tabs' in `pdf-view\private`, deleted with the last private tab;
  others after 3 days) and opens it in a new tab next to the page. The viewer's Download button
  saves it. Settings → Downloads → Open PDFs in Krunk turns this off.
- **Email links (0.11.3):** `mailto:` links open in the app Windows uses for them, or in Gmail,
  Outlook.com, Yahoo Mail, Proton Mail or a web address with `%s` (Settings → General → Email
  links open in). A page can open at most one a second.
- **Downloads:**
  - **History:** `krunk://downloads` shows each download's full address, the page it came from,
    and when it started and finished. Right-click a row (or press Shift+F10 or the Menu key) for Open, Show in folder,
    Copy download link, Copy page address, Retry, Pause / Resume, Cancel, Remove from list and
    Delete file.
  - **Several connections:** large files (8 MB or more) from servers that support it are
    downloaded in parts over several connections at once (Settings → Downloads → Connections per
    download, 4 by default, 1 to turn it off). Krunk first asks the server for the first byte
    only; it splits the download only if the answer names the file's size and a validator
    (ETag or Last-Modified), and every part must come from the same version of the file. The
    parts carry the same cookies and referrer as the browser's own request. Anything else
    (and private tabs) uses the engine's normal download. A file that changes on the server
    during the download is never stitched together.
  - **Disk space and Mark-of-the-Web (0.11.7):** a download in parts starts only if the size
    the server names fits on the disk with 512 MB to spare. Every finished download carries
    the Mark-of-the-Web (its `Zone.Identifier` stream: zone Internet, the file's and the page's
    address, without addresses for private downloads), so SmartScreen and Office's Protected
    View apply to downloads in parts as they do to the engine's own.
  - **Rebalancing (0.7.0):** all parts go into one file of the full size
    (`<name>.krunkpart`, renamed when complete). A connection that finishes takes over a piece
    of the part that would take longest at its connection's speed (at least 2 MB, and more
    than a second of that connection's work), so one slow connection no longer holds up the
    end; the bar on the downloads page and popup shows each piece where it lies in the file,
    coloured by connection.
  - **Resume after a restart (0.7.0):** where a download in parts stands is saved with the list
    (the missing ranges, the validator and the referrer; never cookies). After Krunk closes it
    is "Paused when Krunk closed"; Resume asks the server again and continues if it is the same
    file. If the file changed or the server stopped sending parts, the entry says so and the
    engine downloads the file again with one connection.
  - **The download-start animation (0.7.1):** each frame shows where the glyph should be at the
    display refresh it will be seen at (the compositor's timing), so its pace follows the
    easing evenly even when Krunk's UI thread is busy.
- **Cookies and site data:**
  - `krunk://storage` lists every site with cookies or stored data, grouped by site
    (registrable domain), with a search box. Expand a site to see each cookie: name, value
    (shortened, "Show" reveals it), domain, path, expiry, Secure, HttpOnly and SameSite.
  - **Large lists (0.11.0):** each row says how many subdomains the site groups (they are
    listed when you expand it). The list shows 150 sites at a time and the next ones as you
    scroll; search and sort (name, size, cookies, last used) cover all of them. Sizes are
    measured for the sites on screen first and fill in as they come. While a search is active,
    "Clear all shown (N)" clears exactly the sites it found, after asking.
  - You can delete one cookie, all of a site's cookies, or all cookies. "Clear site data"
    removes an origin's local storage, IndexedDB, Cache Storage, service workers and cookies.
  - The shield's **Cookies and site data** opens `krunk://sitedata` for the current tab's site.
    There you can view, edit, add and delete its local storage and session storage items, and
    edit and delete its cookies.
  - **Hex editor:** the edit dialog has Text and Hex tabs. Hex shows the value's bytes (offset,
    hex and text columns) in an encoding you choose: UTF-8, a binary string (one byte per
    character, common for binary data in local storage), UTF-16LE, Base64 or percent-encoding.
    Values that look binary open on Hex. Type over bytes, press Insert to insert, select with
    Shift or the mouse, copy and paste as hex, Ctrl+Z to undo. Bytes that do not form a valid
    value in the chosen encoding are not saved. A cookie cannot hold control characters or a
    semicolon; use percent-encoding or Base64 for those. In a private tab this page opens as a private tab and only ever touches
    the private profile.
  - Limits (the engine has no API to enumerate storage):
    - The list shows origins you visited (history), origins open in tabs, and every site that
      has cookies. Data of an origin with none of these is not listed (Clear browsing data
      still removes it).
    - Sizes come from the engine's storage quota (IndexedDB, Cache Storage, service workers and
      similar). Local storage is not counted there, so a site with only local storage shows
      0 B.
    - Session storage is shown only for the tab the site-data page was opened from.
    - IndexedDB is shown as a size, not per database.
- **Passwords:**
  - When you sign in to a site, Krunk asks "Save password?". The first save asks you to
    create the vault with a master password.
  - **Filling:** click into a username or password field and Krunk's dropdown offers the
    matching logins: the site's own (exact origin); only when it has none, the logins saved
    for other sites of the same domain, with their host (https pages only; an http page gets
    only its own http logins). Choosing one fills the
    form. A login form inside a cross-origin iframe gets only that origin's logins. With the
    vault locked the dropdown offers **Unlock Krunk passwords**, which opens Krunk's own
    master-password prompt (never page content) and fills afterwards; the next time in the
    session there is no prompt until the vault locks again. The key icon in the address bar
    still works. While the vault is locked, Krunk knows which sites have logins from a keyed
    index (HMAC tags with a DPAPI-protected key in `browser.db`); the origins themselves are not
    stored outside the encrypted vault.
  - **Import** (Settings → Passwords & import): Krunk lists the Firefox, Google Chrome,
    Microsoft Edge and Brave profiles of this Windows user and imports their history,
    bookmarks and passwords (Firefox: with the primary password if one is set). Nothing is
    imported twice: visits by address and time, bookmarks by folder and address, logins by site
    and user name (a different password updates the login). Chrome and Edge passwords saved by
    Chrome 127 or newer are "app-bound" (only Chrome can decrypt them): Krunk reports how many
    and offers the CSV route — export them from the browser and import the CSV file (Chrome/Edge
    or Firefox layout). The browser's files are copied to a temporary folder for reading and
    the copies are removed. Passwords need the vault set up and unlocked. Firefox profiles with
    more than one key in `key4.db` (older ones that went from 3DES to AES) import every login
    since 0.7.2, each with the key it names; when nothing is imported the result says why (no
    saved logins, a wrong primary password, or how many could not be decrypted and why), and
    `krunk.log` has the profile's numbers (logins, keys, whether a primary password is set).
    Since 0.7.4 each field is tried with the key it names and then with the others (a profile's
    3DES and AES keys can share an id), AES-128/192-CBC and AES-GCM fields are read, and fields
    that still cannot be decrypted are listed with their cipher and key sizes.
  - Manage logins on `krunk://passwords`. Showing a password asks for the master password again.
  - **Remain unlocked this session (0.11.0):** the unlock dialog (and the passwords page) has
    this checkbox, checked by default. Checked, the vault stays open until Krunk quits; the
    inactivity lock (Settings → Passwords, 15 minutes by default) does not apply. Locking
    Windows, sleep, quitting and "Lock now" still lock it. Unchecked, the vault locks after the
    set time as before. Krunk remembers your last choice (on this device only). Nothing about
    it is stored in the vault.
- **Private tabs:** they share an in-memory profile that is discarded when the last private tab
  closes. Nothing from them is written to disk, history, the session or the favicon cache.
  Since 0.12.3 their media never reaches Windows' media controls ("Now Playing", the volume
  flyout, the lock screen): while a private tab's media is the active one, Krunk shows nothing
  there (Chrome would show "Playing media in incognito"), and media keys don't control it.
- **Picture-in-Picture** (0.6.0): when a page has a video, a small window icon appears in the
  address bar; it, the menu's "Picture-in-Picture" and Alt+P send the video to the engine's
  floating always-on-top window (play/pause, back to tab, close). With several videos a chooser
  lists them with their size and state (0.12.1: the two lines of a row no longer overlap the
  next one); the same control takes the video back. Settings →
  General → Media → "Automatic Picture-in-Picture" does it when you switch tabs while a video
  plays and reverses it when you return. Videos inside a frame from another site (an embedded
  player) are not listed: use the player's own control. **Volume (0.7.0, inside the player since
  0.11.0):** the engine's floating window has a mute button but no volume. Point at that mute
  button and a small volume slider opens right next to it, inside the window (it closes a
  moment after the pointer leaves both); the mute button still mutes. The mouse wheel over the
  floating window changes the volume, and changes the page makes (its own controls, the mute
  button) move the slider. (Until 0.10 a separate bar showed under the window whenever the
  pointer was over the video.)
- **Media overlay name (0.11.0):** Windows' media overlay (the volume flyout, the lock screen)
  names Krunk with its icon for media a page plays; it showed Chromium before. Krunk sets its
  own Windows app ID (`Krunk.Browser`), registers its name and icon for it, and puts it on its
  own shortcuts (Start menu, desktop, taskbar pins), so a pinned Krunk still groups its windows.
  A portable copy sets the ID but changes nothing on the system. **Artwork (0.12.1):** when a site
  gives no artwork of its own, the overlay shows Krunk's logo (it showed Chromium's): Krunk's
  engine build carries Krunk's logo as its product logo (`scripts/cef-build/README.md`).
- **Default browser (0.12.1):** an installed Krunk registers itself with Windows as a browser
  (per user: http, https and .htm/.html/.shtml/.xht/.xhtml; the installer writes it and Krunk
  repairs it at start). While it is not the default, `krunk://settings` shows a banner, and
  General → Default browser says so; "Make default" opens Windows Settings at Krunk's own page,
  whose "Set default" makes it the default (Windows lets no program do that itself). The page
  notices the change by itself. Links and pages opened from other apps open in a new tab of the
  running Krunk (or start it with them); only http(s) addresses, `file:` URLs and absolute file
  paths are opened. A portable copy can't be the default. Uninstalling removes the registration.
- **Page dialogs (0.12.4):** a page's `alert`, `confirm`, `prompt` and "Leave site?" /
  "Reload site?" are Krunk's own prompt, in the middle under the toolbar, titled with the site
  ("example.com says"). The engine's own dialog windows could end up behind Krunk's window
  while the page waited for them, and the tab looked frozen. The prompt always stays above
  the window, and every way it ends answers the page:
  - OK/Leave (Enter) or Cancel;
  - Esc or a click elsewhere: alert acknowledged, anything else refused, the page stays;
  - switching or closing the tab.

  A dialog of a tab that isn't in front is answered at once (leaving then goes ahead). From a
  page's second dialog on, "Don't let this page show more dialogs" stops the rest until the
  page loads again; that is never offered on "Leave site?", and a stopped page always lets
  you leave.
- **Image context menu (0.12.2):** right-clicking an image offers "Open image in new tab",
  "Save image as…" (always asks where; downloaded through the tab, with its cookies and, in a
  private tab, its private profile), "Copy image" (the clipboard gets a bitmap and a PNG, so
  transparency survives in programs that read PNG) and "Copy image address". `data:` images
  work too; an image that exists only inside the page (a `blob:` address) can be saved and
  opened but not copied.
- **View page source (0.11.0):** the page's context menu, Ctrl+U or typing
  `view-source:<address>` opens the source in a new tab next to the page (a private tab for a
  private page): line numbers and colours for HTML and for the page's scripts and styles; links
  open their own source. **Beautify** re-indents the HTML by its nesting (and formats inline
  scripts and styles a statement per line); **Wrap lines** wraps long lines; both are
  remembered. The source is loaded again the way the tab loaded it (its cookies), from the cache
  when it is there. A page that cannot be loaded again (the result of a form) shows the tab's
  current document, after its scripts changed it, and says so. Very large sources (60,000 lines
  and more) stay smooth; up to 20,000 lines Ctrl+F finds anything in them.
- **Search for selected text (0.11.0):** right-click selected text for "Search <your search
  engine> for “…”" (after Copy); it opens the search in a new tab next to the page (private for a
  private page). Selections in password fields offer nothing.
- **A page that stops responding (0.11.0):** pages run in their own processes, so a stuck or
  slowly loading page does not hold up tabs, the tab strip or the address bar. After about
  15 s without reacting, Krunk asks "This page isn't responding": **Wait**, or **Close page**,
  which ends it; the tab then shows that the page stopped, with **Reload** (the address stays in
  the bar). Tabs of one site can share a process: each of them asks, and Close page ends all of
  them. A page that crashes shows the same page with Reload.
- **AI agent (0.12.0, optional, off by default):** Settings → AI and scripts → "AI features".
  While it is off there is no agent button, no agent and no request to any provider.
  - **Your own provider and key:** OpenRouter, OpenAI, Anthropic, or any OpenAI-compatible server
    (Ollama, LM Studio, …) at its address (`https://`, or `http://` only on this computer or the
    local network). You can add several. Each has a key field (masked, never shown again: Krunk
    only says "Key saved"), Test, Change key and Remove. Pick the provider and the model; the
    model list comes from the provider, with search, or type a model id. Optional: extra
    instructions for the agent and the number of steps before it asks whether to go on
    (default 30).
  - **The panel:** the ✦ button right of the shield (or Ctrl+Shift+Space) opens the agent's panel
    on the right of the window. Drag its edge to resize it. Tell it what to do in plain words;
    it works on the tab you are looking at (and may open or switch tabs itself). You see its
    answer as it is written, and each step it takes (expand a step for details, screenshots
    included).
  - **Questions and Stop:** it may ask you a question with buttons. **Stop** (or Esc in the
    panel) ends the task at once: the request to the provider is cancelled, and keys or buttons
    it held are released. The model indicator in the panel switches between the provider's
    models. "+" starts a new conversation. A conversation belongs to its window, is kept in
    memory only, and is gone when the window closes.
  - **What it can do:** read the page (text, HTML, links, forms, media, screenshots), click,
    type, fill forms, choose options, press keys, scroll, wait, run JavaScript in the page, edit
    the page, change its title, open, switch and close tabs, go back, forward and reload, save a
    screenshot to Downloads, change some settings, and write userscripts and macros.
  - **What it can't do:** see or use passwords, payment cards, the password vault, sync or account
    data, or API keys. Password and card fields are redacted and refused. It can't read or act on
    Krunk's own pages, and can't open the passwords, sync, site-data and storage pages at all.
  - **Asking first:** your history, bookmarks, downloads list and a site's cookies are offered to
    it only when your message asks for them in words ("history", "bookmarks", "downloads",
    "cookies"), and you click Allow the first time in each conversation.
  - **What you send:** the page content it works on goes to the provider you chose. Krunk itself
    runs no AI service and sends nothing anywhere else.
- **Userscripts (0.12.0):** `krunk://scripts` (menu → Scripts and macros, or Settings → AI and scripts →
  Manage scripts) lists your Tampermonkey-style userscripts: on/off, where they run, when they
  last ran, Edit, Export, Delete.
  - **Adding scripts:** New writes a script in the editor, and Import .user.js reads a file. The
    AI agent can also make one, tagged "created by AI"; it runs on the current tab at once.
  - **Metadata:** `@name @namespace @version @description @match @include @exclude @run-at
    (document-start | document-end | document-idle) @grant @noframes`.
  - **Grants:** `none`, `GM_addStyle`, `GM_getValue`/`GM_setValue`/`GM_deleteValue`/
    `GM_listValues` (stored per script), `GM_setClipboard`, `GM_notification` (a short message over
    the tab row) and `GM_openInTab`. `unsafeWindow` is `window`.
  - **Not supported in 0.12.0:** `GM_xmlhttpRequest`, `@require`, `@resource` and
    regular-expression `@include` (the editor warns).
  - **Where they run:** on web pages only, never on Krunk's own pages, and in private tabs only
    when "Also in private tabs" is on (off by default). Settings → AI and scripts → "Run
    userscripts" turns them all off; they need no AI.
- **Macros (0.12.0):** small JavaScript programs that press keys, click and type in the current
  tab, on the Macros tab of `krunk://scripts`. They need no AI.
  - **The API:** `await key.down('a')` / `key.up` / `key.press('Enter', {ctrl: true})` /
    `key.hold('a', 500)`, `mouse.click(x, y)` or `mouse.click('#button')`, `mouse.move`,
    `type('text')`, `wait(ms)`, `page.eval(js)`, `page.query(selector)`, `tabs.open(url)`,
    `tabs.switch(id)`, `navigate(url)`, `repeat(n, fn)` or `repeat('forever', fn)`, `toast(text)`
    and `log(text)`. A held key repeats at Windows' own keyboard rate (`{repeat: false}` holds it
    without repeats) until `key.up`.
  - **Running and stopping:** Run in the list, or a **hotkey** recorded in the editor (it needs
    Ctrl or Alt, or is an F-key; Krunk's own shortcuts and another macro's hotkey are refused).
    The toolbar shows a running macro with its Stop button. Its hotkey again, or Ctrl+Shift+.,
    stops it too. Every stop releases all held keys and buttons.
  - **Where they act:** a macro acts on its window's current web page (from `krunk://scripts`, on
    the last web page you used). Its keys and clicks go to the page only: never to other programs,
    and they don't trigger Krunk's own shortcuts.
- **The code editor** (userscripts and macros) is Krunk's own: line numbers, JavaScript colours,
  Tab / Shift+Tab indent, Ctrl+F find, Ctrl+S save. A syntax error is shown at its line, and the
  script is not saved.
- **Extensions** (0.6.0; `krunk://extensions`, also in the menu): Krunk runs its own subset of
  Chrome's Manifest V3, since the engine has no extension system of its own. On a Chrome Web
  Store page the store's button says **Add to Krunk** (0.7.0); you can also paste a store
  address, or install a `.zip`/`.crx` file or an unpacked folder. Store packages are fetched
  from Google's download server over HTTPS and never land in Downloads. Every CRX must carry a
  valid signature by its own developer key, and a store package also the Chrome Web Store's
  signature; then Krunk shows the extension's name, source and what it can do ("Read and
  change all your data on all websites", …) and installs only after **Add extension**. An
  extension's popup closes on Esc, a click anywhere outside it, a tab switch or when another
  program comes to the front. What runs:
  content scripts (in an isolated world of web pages, with `matches`, `exclude_matches`,
  `run_at`, `all_frames`, CSS), a background page built around the service worker,
  the toolbar button with its popup and badge, options pages, and the `chrome.runtime`
  (messaging, `getURL`, `getManifest`, `openOptionsPage`), `storage` (local/sync/session,
  `onChanged`), `tabs` (query/get/create/update/remove/reload/sendMessage, `onUpdated`,
  `onActivated`, `onRemoved`), `action`, `alarms`, `i18n`, `scripting` (`executeScript`,
  `insertCSS`), `windows` and `permissions.getAll` APIs. Static `declarativeNetRequest`
  rulesets become filter rules of the ad blocker (`block` and `allow`; `redirect` and
  `upgradeScheme` rules are counted as "not translated"). Never provided: blocking
  `webRequest`, `debugger`, native messaging, dynamic or session rules and switching
  rulesets at runtime, `cookies`, `history`, `bookmarks`, `downloads`, `identity`,
  `contextMenus`, `notifications` and the rest; the page lists what each extension asked
  for and whether Krunk has it, and the log says when an extension reaches for something
  missing. **Isolation:** extensions never run on Krunk's own pages or the shell, not in
  private tabs unless "Allow in private tabs" is on, and their pages live on their own
  origin `https://<id>.ext.krunk.invalid`; the scope list on `krunk://extensions` shows
  which sites each one may read and change. Since 0.11.7:
  - each extension's content scripts run in their own isolated world with their own binding,
    so one extension can't use another's storage or messages;
  - `scripting.executeScript`/`insertCSS` need the `scripting` permission, as in Chrome, and
    never reach Krunk's own pages;
  - a package the Web Store didn't sign can't replace an installed Web Store extension with
    the same id (remove that one first);
  - `default_locale` must be a locale name. Extensions are stored under
  `%LOCALAPPDATA%\Krunk\extensions`; Remove deletes the files and the stored data.
- **Protected content (Widevine)** (0.6.0; Settings → Privacy & security, off by default,
  restart): lets sites play DRM media with Google's Widevine module. Krunk does not bundle
  it: when the setting is on, the engine's component updater downloads it from Google
  (`update.googleapis.com`, `edgedl.me.gvt1.com`) into `profile\WidevineCdm` a minute or
  two after start, under Google's terms; the setting also lets the updater's other components
  through, as "Download engine security components" does. Settings and krunk://gpu show the
  installed version. Krunk's binaries are not VMP-signed (see Known limitations).
- **Zoom** is remembered per site, as in Chrome. "Default zoom" in Settings applies to sites you
  have not zoomed.
- **Memory:** background tabs go to sleep after 5 minutes. Optionally they can be unloaded
  completely after a longer time; turn this on in Settings → Performance.
- **Games (pointer lock and full screen):** `requestPointerLock()` works as in Chrome. The cursor is
  hidden and confined to the page, movement keeps coming at the screen edge (smoothly, also
  while mouse buttons and keys are held; since 0.7.2 the turn per frame is as even as in Chrome
  when a movement key is held), and
  `{unadjustedMovement: true}` gives raw mouse input. Esc, Alt+Tab, switching tabs or leaving
  the page release the lock; a small "Press Esc to show your cursor" pill appears for 3 s, at
  most three times per tab. A page's
  `requestFullscreen()` and **F11** fill the monitor without the tab strip and toolbar; Esc
  leaves page full screen and F11 toggles.
- **Starting Krunk again** while it runs opens a new tab in the most recently used window.

Command-line options (all optional):

| Option | Effect |
|--------|--------|
| `--update-filters` | Update the filter lists and run a blocking self-test, without opening a window |
| `--mem-report` | Log the memory of the browser's process tree to `memory-report.log` every 10 s |
| `--sleep-test-seconds=N` | Testing only: run the tab sleep/discard timers in seconds instead of minutes |
| `--log-blocked` | Diagnostics: log each blocked or replaced request (without its query string) to `krunk.log` |

Chromium switches typed on the command line are ignored: only the browser itself, through its
settings, configures the engine (so e.g. `--no-sandbox` cannot be passed this way).

## How it works

### Architecture

```
core/    platform-independent library (lb_core): storage, history, bookmarks, frecency,
         vault crypto, ad blocking, cookies, URL cleaning, settings and engine switches,
         zoom, tab model, shortcuts, message protocol, origin checks, layout math.
         Tested on Linux.
app/     Krunk.dll: the browser on top of CEF. All unsafe code is in app/src/ffi
         (ffi/cef wraps the engine; the rest wraps Win32).
ui/      the browser's own UI (HTML/CSS/JS), embedded into the DLL at build time.
         ui/dev holds test tooling only (mock host, Playwright tests) and is not embedded.
tools/   pebrand: build tool that brands the exe (version strings, manifest).
scripts/ build, CEF download/verification and Windows VM test scripts.
```

### Engine and processes
- **`Krunk.exe` is CEF's sandbox bootstrap** (`bootstrap.exe`, renamed, with
  Krunk's version strings and manifest). It sets up the Chromium sandbox, loads
  `Krunk.dll` from its own folder and calls the DLL's `RunWinMain`. The same exe + DLL
  pair runs the browser process and every engine subprocess.
- **Sandbox:** always on. Renderers run at Untrusted integrity with restricted tokens; the GPU
  process at Low. `scripts/vm-sandbox.ps1` prints the integrity level and token of every process.
- **Startup:** the DLL first checks that the runtime files exist (`libcef.dll` is delay-loaded,
  so a missing engine gives a message box instead of a silent failure), lets subprocesses run,
  opens the database and settings, and only then starts the engine.
- **Engine profile:** `profile\` is CEF's root cache path and `profile\default` the persistent
  profile. Private tabs use a separate in-memory request context. CEF's own password manager and
  autofill are switched off on both (profile preferences), because the browser has its own vault.
- **Engine version:** the exact CEF build is pinned by the `cef` crate version in `app/Cargo.toml`
  and the archive hashes in `cef.lock`. The About page reads the versions from the loaded
  `libcef.dll` at runtime.
- **Engine build:** since 0.5.2 `libcef.dll` is Krunk's own build of CEF 156.0.15 (the same
  CEF commit and Chromium 156.0.8078.80 as the stock binary distribution) with proprietary
  codecs, built as an official (ThinLTO + PGO) Windows build on a Linux host:
  `scripts/cef-build/README.md`. One consequence of the cross build: Chromium cannot generate
  the Blink V8 *context* snapshot off Windows, so the engine loads V8's plain
  `snapshot_blob.bin` instead of `v8_context_snapshot.bin` (the startup check accepts either);
  the only cost is a few milliseconds slower page-context creation. The engine is about
  2 MB bigger than the stock build; package and update sizes are unchanged.
- **Engine patch (0.10.0):** one Krunk patch on top of CEF,
  `scripts/cef-build/patches/krunk-windowed-hidden.patch` (its SHA-256 is in `cef.lock`):
  `CefBrowserHost::WasHidden` also works for windowed browsers. A background tab's content
  window is hidden from the engine, which then throttles the page like Chrome does (no animation
  frames, timers once a second, background priority); a tab playing sound, a call, a download or
  a picture-in-picture video goes on. Shown again, the tab repaints at once. Measured on the
  4-core test VM with 30 busy tabs: 90 % → 1.2 % CPU; Speedometer with 29 busy background tabs
  1.5 → 27.5 (27.9 with one tab).

### Windows and views
- **Windows:** any number of top-level Win32 frames. Every view is a CEF browser (Alloy style)
  hosted as a child window of a frame. A hidden app window takes the timers, cross-thread
  messages and system broadcasts (session lock, power), so they don't depend on a frame.
- **Model:** `core::windows::Workspace` holds one tab model per window, unique tab ids across
  windows, one closed-tab stack and the most-recently-used order of the windows. It turns every
  change into operations for the host (create/close a window, create/destroy/move a view,
  quit). "The current window" is the most recently used one: keys and menu commands act on the
  window they were typed in, `window.open` opens in the opener's window, and a second start of
  Krunk or the update prompt use the current window.
- **Shell:** a view strip at the top of each window showing its tab strip and toolbar. Its
  height is fixed: 76 DIP, scaled by that window's DPI.
- **Tabs:** one browser per *live* tab, laid out below the shell. Restored tabs stay unloaded
  until they are activated. Pages that open windows (`window.open`, `target=_blank`) get a new
  tab next to them; `window.open` keeps its opener.
- **Switching tabs:** the views of the other tabs are never hidden: they are parked outside the
  window's client area, shown and sized (0.7.4). A hidden window could lose what the GPU had
  drawn for it, and shown again its first frame was empty — an occasional one-frame white flash
  on tab switches on GPU machines. A switch moves the new tab's view into place on top, and the
  old one is parked 100 ms later (0.7.3), so the old page shows through until the new one is
  drawn. A tab that has never loaded keeps the old page on screen until it has.
- **Moving a tab between windows** re-parents its browser's child window (`SetParent`, then
  CEF's move/resize notifications). The browser, its renderer, its request context (so a
  private tab stays private) and its history are untouched, so nothing reloads.
- **Dragging tabs:** the shell reorders tabs within its strip with pointer events. When the
  pointer leaves the strip, the shell hands the drag to the host (`tabDragOut`), because a view
  can't see other windows. 0.11.1: the source frame then holds the mouse capture and the host
  moves the dragged window on every `WM_MOUSEMOVE` (`SetWindowPos` without a size; a 15 ms
  timer only backs up a lost capture and Esc). The other windows' places, client origins,
  scales and z-order are taken once at the start (`frame::DropWindow`); over the topmost one's
  tab strip the host tells its shell where the tab would land (`frame::drop_index` over the
  tab centres the shell reported before the gap opened; it reports none while a gap is open),
  with the tab's title and icon (`tabDropHint{index, ghost}`). The dragged window then steps
  aside (off-screen, keeping the capture) and the shell shows a ghost tab in a gap; the dropped
  tab takes the ghost's place. While a window follows the mouse its move handling (popups, the
  engine's move notifications, the session timer) waits for the drop, and Windows' fade-in is
  off for a torn-off window.
- **Spare frame (0.11.1):** a hidden frame whose shell has loaded, prepared 3 s after a shell
  loads or a window took the last one. The next new window (a tear-off, "Move to new window",
  Ctrl+N) takes it and is complete at once. Its shell gets what all shells share (config,
  bookmarks bar, downloads, update and extension buttons). It is created with
  `WS_EX_NOACTIVATE` (the engine shows a new browser with activation, and the hidden window took
  the keyboard) and refuses focus; a close request to it goes to the current window. It costs
  one more shell browser.
- **Popups and closing windows (0.11.1):** popup windows are owned by a frame (so they stay
  above it). Before a frame is destroyed they are re-owned to another frame (or the app window):
  Windows destroys owned windows with their owner, and a destroyed menu window made every later
  menu fail. A popup whose window or renderer is gone is recreated.
- **Session:** the kv entry `session`, version 2: every window's tabs, active tab, restored
  bounds (physical pixels, `rcNormalPosition`) and maximized state, plus which window was in
  front. It also keeps 0.2.x's `tabs`/`active` fields for the most recently used window, and a
  0.2.x session (no `windows`) restores as one window. On restore,
  `core::layout::fit_to_monitors` moves a window whose title band isn't on any monitor.
- **DevTools:** `ShowDevTools` with `set_as_child(frame)`: a child window of the tab's frame,
  placed by the host beside the page (`core::layout::devtools_split`: a 5 DIP splitter, at least
  240 DIP of page and 280/160 DIP of DevTools). The splitter is the frame's own client area
  between the two (cursor, capture and drag in the window procedure). The DevTools browser has
  a keyboard handler (shortcuts reach Krunk except `keys::devtools_keeps`) and no message
  handler, so it has no privileged channel. Only tab views can be inspected: the shell and
  popup have no context menu and DevTools commands act on the active tab.
- **Popup:** one extra view in an owned `WS_POPUP` tool window, re-owned by the window it opens
  for. It shows the main menu, the tab menu, the shield panel and the save-password and
  fill-login prompts.
  - It is created on first use and then hidden and reused, never more than one.
  - It uses the default profile, never a private one.
  - The shell sends the popup's rectangle in CSS pixels. The host converts it to screen pixels
    with the window's DPI (`GetDpiForWindow` + `ClientToScreen`) and clamps it to the monitor
    work area (`core::layout::popup_screen_rect`).
  - **Hide rules:** the popup hides on deactivate, Escape, a click elsewhere (another view gets
    focus), its window's move, resize or minimize, and tab switch or close. A prompt hidden this
    way counts as dismissed, never confirmed.
- **Keyboard shortcuts** are taken before the page sees them (`CefKeyboardHandler::OnPreKeyEvent`),
  in every view.

### Internal origins
| Origin | Serves | Who can send privileged messages |
|--------|--------|----------------------------------|
| `https://shell.krunk.invalid` | `ui/shell/*` (`shell.html`, `popup.html`, 0.12.0: `agent.html`, `runner.html`) + `/shared/*` | the shell view (only `/shell.html`), the popup view (only `/popup.html`), a window's agent panel (only `/agent.html`) and the hidden macro runner (only `/runner.html`); each has its own allow-list |
| `https://krunk.invalid` | `ui/pages/*` + `/shared/*` | a tab showing a known built-in page; each page has its own command allow-list |

- A CEF scheme-handler factory answers every request to these origins from embedded bytes. An
  unknown path gets a 404 from the handler, never a network request.
- `.invalid` never resolves. In addition, the engine's host rules map both hosts to "not found"
  and keep them away from a configured proxy, because Chromium preconnects to a navigation's
  host before the handler answers.
- Every response carries a strict CSP (`default-src 'none'`, no inline code, no network,
  `frame-ancestors 'none'`), `X-Frame-Options: DENY` and `Cross-Origin-Resource-Policy: same-origin`.
- Tabs may load internal files only from a built-in page of the same origin; web pages cannot
  embed, frame or fetch them (`core::assets::internal_request_allowed`). The shell origin is
  not reachable from tabs at all.
- A tab reaches a built-in page only when the browser itself navigates it there (address bar,
  menus, new tab), or by a link on a built-in page it already shows. Since 0.11.7 the check
  uses the address the tab really shows (`core::assets::allow_navigation`): a popup opened
  with `window.open`, a fresh `about:blank` and a server redirect never count as a built-in
  page.
- Both origins also serve `/favicon?host=<host>`: PNG favicons captured from pages, cached in
  SQLite (never from private tabs).

### Messages between pages and the browser
- In every main frame, before any page script runs, the renderer installs `window.__lbHost`
  (`postMessage`, `addEventListener('message', …)`): a small shim around a native function.
  Messages travel as JSON in CEF process messages.
- The browser decides who sent a message from the **sending frame's committed URL and
  main-frame flag** (as reported by the engine), never from the message:
  `core::protocol::classify` checks the exact origin, the exact document and the main frame.
  Web pages can only send the small `ContentMessage` type (login form found, login submitted,
  fill requested, page class names for cosmetic filtering).
- Replies and vault fills go to the main frame only; a fill is sent only if the tab still shows
  the origin the login belongs to.

### AI agent, userscripts and macros (0.12.0)

**Architecture.**
- The agent's loop runs in the **browser process**. Its logic is in `core::agent`, which is tested
  on Linux:
  - `session`: the conversation state machine; the user's message, stream events, tool results
    and answers go in, actions come out;
  - `provider` and `sse`: OpenAI-style requests for OpenRouter, OpenAI and custom servers;
    Anthropic's Messages API; the streamed answers;
  - `tools`: 41 tools with their JSON schemas and argument checks;
  - `context`: the context block and the system prompt;
  - `redact`, `gate`, `settings_allow` and `pages`: the privacy rules below.
- `app/src/browser/agent/` carries the actions out. Provider calls are engine URL requests from
  the browser process (`ffi/cef/fetch.rs`: POST, a streamed body, an idle timeout, cancel). They go
  through the profile's network context, so the proxy and secure-DNS settings apply. They send no
  cookies, and every one checks that AI is on.
- Tools run on the window's active tab:
  - **page tools** use a probe (`ui/inject/agent-probe.js`) in an isolated world
    (`krunk-agent`), so page scripts can't see or change it;
  - **`eval_js`** alone runs in the page's main world;
  - **input** uses DevTools `Input.*` events on that tab (engine-level, never Windows-wide input).
    These don't reach Krunk's own shortcuts.
- **Before each task** Krunk sends a context block: the tab's address, title, load state and
  private flag; the window's tabs; the selection; viewport and scroll; a page outline; the tools.
  The system prompt says that page content, titles and tool results are untrusted data, never
  instructions, and lists the built-in pages and settings the agent may use.
- **Results are capped** at 8,000 characters (20,000 for HTML and text on request), with an offset
  to read on. Only the last two screenshots stay in the conversation.

**The panel** is a CEF view per window on its own channel (`shell.krunk.invalid/agent.html`), docked
right of the tab area. The frame paints and drags its splitter. Its width is a setting.

**Tools.**
- **Reading:** get_page_context, get_html, get_text, query, list_links, list_forms, list_media.
- **Screenshots:** screenshot (to the model as an image when it takes images), save_screenshot.
- **Input:** click, type_text, fill_fields, select_option, press_keys, scroll, wait_for.
- **Page changes:** eval_js, edit_page, set_title.
- **Tabs:** navigate, back, forward, reload, open_tab, switch_tab, close_tab, list_tabs.
- **Settings:** get_settings, set_setting.
- **Userscripts:** list_userscripts, save_userscript, toggle_userscript.
- **Macros:** list_macros, save_macro, run_macro.
- **Talking to you:** ask_user, finish.
- **Asked for in words only:** get_history, get_bookmarks, get_downloads, get_cookies_for_site.

**Privacy and redaction.**
- **No code path to secrets.** No tool, setting key or schema reaches the vault, payment cards,
  sync, the account or API keys; a core test checks the tool table.
- **Settings.** Settings change only through an explicit allow-list (`core::agent::settings_allow`).
  It holds appearance, start-up, new tabs, search engine, tab sleep, ad blocking, DNT/GPC, PDFs,
  download connections, DevTools dock, and userscripts and macros on/off. The vault, cards,
  sync, account, AI, proxy, DNS, flags, user agent, headers, updates, the download folder and
  custom search or mail templates are not on it.
- **Krunk's own pages.** The agent never reads, scripts, clicks, types into or screenshots them,
  nor extension pages or DevTools. It cannot open the passwords, sync, site-data and storage
  pages at all.
- **Password and card fields** are `input[type=password]`, an `autocomplete` `cc-*` token,
  card-like names or labels, or a Luhn-valid 13–19 digit value.
  - Their values are blanked in every result.
  - `fill_fields`, `type_text`, `press_keys` and `edit_page` refuse them.
  - `eval_js` is refused while one holds a value.
  - Every result is scrubbed of the page's current secret values, read again after `eval_js` and
    after an agent-started macro, and of any card number.
  - Screenshots mask the fields, and cover cross-origin iframes that look like payment forms.
- **History, bookmarks, downloads and cookies** are offered only when the user's message names
  them (an English keyword gate in `core::agent::gate`), and after Allow once per conversation.
  They are never in the context block.
- **Memory and logs.** Conversations are in memory only. The log holds step and tool names,
  statuses and times only, never content, addresses or keys.

**API keys** are stored DPAPI-protected (current user, Krunk's own entropy) in the `ai_keys` table.
- A key is unwrapped only for the request that needs it, then wiped.
- It is never synced, exported, sent to a view or logged.
- The settings page sends a key once (`aiKeySet`, a zeroized secret) and only ever learns whether
  one is saved.

**Messages and allow-lists.**
- **Shell:** `toggleAgent`, `stopMacros`.
- **Agent panel** (sender `Agent`): `agentSend`, `agentStop`, `agentNew`, `agentAnswer`,
  `agentConsent`, `agentContinue`, `agentSelectModel`, `agentModels`, `agentClose`,
  `agentOpenScript`, `agentToggleScript`, `openUrl` (web addresses only).
- **Macro runner** (sender `Runner`): `macroCall`, `macroDone`.
- **Settings page:** `aiProviderSave`, `aiProviderRemove`, `aiKeySet`, `aiKeyRemove`, `aiTest`,
  `aiModels`, `aiStatus`.
- **`krunk://scripts`:** `scriptsList`, `scriptGet`, `scriptSave`, `scriptToggle`, `scriptDelete`,
  `scriptExport`, `scriptRunNow`, `scriptCheck`, `macroSave`, `macroDelete`, `macroToggle`,
  `macroRun`, `macroStop`, `hotkeyCheck`, `hotkeyRecord`.
- **Events to the panel:** `agentConfig`, `agentState`, `agentDelta`, `agentMessage`, `agentStep`,
  `agentAsk`, `agentConsentAsk`, `agentContinueAsk`, `agentScript`, `agentError`, `agentClear`.
- **Events to the shell:** `agentButton`, `macroRunning`, `toast`.
- **Events to the scripts page:** `scriptsChanged`, `hotkeyRecorded`, `macroState`.
- Pages have no channel into the agent. The panel and runner documents are on the shell origin,
  which no tab reaches, and `classify` grants them only on their own view.

**Userscripts.**
- Each tab gets one loader (`core::userscripts::bundle`). It is registered as a new-document script
  in the main world, in all frames, and re-registered when scripts or their values change.
- It checks `@match`/`@include`/`@exclude` in JavaScript (a Rust twin is tested against the same
  vectors) and runs each script at its run-at with only its granted GM functions.
- GM calls go to the browser through a DevTools binding with a random name, which the loader takes
  and deletes before any page script runs. A per-tab nonce is checked too.
- `@match`/`@include` never match Krunk's own origins.
- The DevTools `Page`/`Runtime` domains of a tab are shared and reference-counted between the
  extensions, the userscripts, the agent and the scripts page (`browser/cdp.rs`).

**Macros** run in a hidden view (`shell.krunk.invalid/runner.html`).
- The browser starts the macro's code there with DevTools `Runtime.evaluate`, so the page's CSP
  needs no `eval`. Each API call is a request the browser carries out on the window's web page.
- `wait`, held keys and their repeat (at Windows' delay and rate) are browser timers.
- Stop ends the runner's script, reloads it and releases every held key and button. Before each
  key-up it sends one more key-down without a character: Chromium drops key-ups after a key the
  browser handled itself, such as the hotkey.
- Hotkeys are matched in `OnPreKeyEvent` of tab views.
- The editor's syntax check is the engine's own `Runtime.compileScript` in the scripts page's tab.

**Storage** is SQLite schema 7: `userscripts`, `userscript_values` (at most 1 MB per script),
`macros` (unique hotkeys) and `ai_keys`. All of it is local: no sync, and not in settings export or
reset.

### Ad and tracker blocking
- **Engine:** Brave's `adblock` engine with seven bundled lists: EasyList, EasyPrivacy, and the
  uBlock Origin lists (filters, privacy, unbreak, quick fixes and badware risks; see
  `assets/filters/ATTRIBUTION.md`). The lists are embedded deflated.
  - uBO's sub-lists (`!#include`) are inlined, and its `!#if` blocks are resolved for desktop
    Chromium before compiling (`core::preprocess`). Otherwise Firefox-only or mobile-only rules
    would apply too.
  - Rules from the uBO lists may use uBO's trusted scriptlets (`trusted-*`); EasyList's may not,
    as in uBO.
- **Cache:** at startup the compiled engine is loaded from `%LOCALAPPDATA%\Krunk\engine.dat`, and
  the scriptlet engine (below) from `scriptlets.dat`. The cache key covers the engine version,
  which lists are enabled, and where each comes from (embedded copy or download) with its content
  hash. If anything differs, both are rebuilt on a worker thread.
- **Interception:** every request of every tab, private tabs included (sub-resources, frames,
  workers), passes `CefResourceRequestHandler::OnBeforeResourceLoad` on CEF's IO thread.
  - Blocked requests are cancelled. For `$redirect` rules the request is not sent either, and
    a resource handler answers with uBO's stand-in: a no-op script, an empty JSON or VAST
    response, or a 1x1 GIF. Pages that expect the script to exist then keep working.
  - The shield counts both. The count is kept in atomics and pushed to the UI thread at most
    about four times a second; the log gets one line per page with the count.
  - The tab's own top-level navigation is never checked.
  - The handler uses only in-memory state shared with the UI thread (`Arc`/atomics); the
    allowlist decision is made once per navigation.
- **Scriptlets (`##+js(...)`):** uBlock Origin's scriptlets come from uBO release 1.75.4
  (`assets/filters/ubo-resources.json`, pinned by hash). They patch what a page sees before its
  own scripts run, for example by pruning ad fields from JSON and fetch/XHR responses
  (`json-prune*`, `trusted-replace-fetch-response`), fixing constants (`set-constant`) or
  stopping anti-adblock checks. This is how uBO, and now Krunk, keep YouTube ads and the "ad
  blockers" wall away.
  - The browser compiles the lists' scriptlet filters into a small separate engine and hands
    it to every renderer process. It travels with each browser's `extra_info` and is
    re-sent after list updates.
  - In `CefRenderProcessHandler::OnContextCreated`, which fires for every frame before any page
    script, the renderer runs the scripts matching the frame's URL. They run in the page's
    world, wrapped in a private scope as uBO does.
  - Nothing runs on allowlisted sites or with ad blocking off.
- **Cosmetic filtering:** site-specific hiding rules are injected when a page commits, as a
  constructed stylesheet, which page CSP doesn't block. Generic rules are applied for the class
  names and ids the page reports, capped at 2000.
- **Lists you add and your own filters** (`krunk://adblock`): an added list must be an https
  address, at most 16 MB of text with at least one filter (no header needed); it gets no trusted
  scriptlets. The addresses are in the settings, the downloads in `filters\custom-<hash>.txt`,
  and your own filters (at most 200 KB) in the database. Both are part of the cache key. Each
  change rebuilds the engine on the filter worker; a change made while a rebuild runs waits for
  it.
- **What was blocked:** each tab keeps its last 500 blocked requests in memory (address without
  the query, type, page). The filter that matched is looked up only when `krunk://adblock` asks,
  by a throwaway engine built with the filter text on a worker thread; the engine in use keeps
  none.
- **Updates:** at most once every 24 h (checked hourly), a worker thread downloads the lists and
  their sub-lists over WinHTTP (HTTPS only, 16 MB cap, 30 s timeout) and validates them: no HTML,
  a list header, at least 50 rules, at least 2 KB. A failed or suspicious download, including a
  missing sub-list, keeps the previous source.
  - The updater uses the browser's HTTP proxy setting; with a SOCKS proxy it skips the download
    rather than bypass the proxy.
  - `Krunk.exe --update-filters` runs the update and a self-test without a window: blocking
    decisions, plus the scriptlet engine giving a YouTube watch page its scripts.

### Privacy
- **Tracking protection levels** (0.6.0, Settings → Privacy → Tracking protection; the
  research and the full table are in `docs/TRACKING_PROTECTION.md`). **Standard** (default):
  the NoCoin cryptominer list, no cookies for third-party requests to hosts the privacy lists
  name, tracking parameters stripped, cross-site requests carry only the origin as `Referer`,
  bounce trackers (sites only ever seen as a redirect hop, never visited) lose their cookies
  and storage daily. **Strict** adds Krunk's fingerprinting list, blocks every third-party
  cookie, sends no `Referer` to other sites and cleans bounce trackers hourly. **Custom**
  picks each part. The shield's site switch turns all of it off for one site.
- **Third-party cookies:** a `CefCookieAccessFilter` asks `core::cookies::is_third_party` for
  every cookie a request would send or store (a cookie is third-party when the request's
  registrable domain differs from the tab's top-level site), then the level decides: known
  trackers only (`core::protection::TrackerSet`, the `||host^` rules of EasyPrivacy and uBO's
  privacy list) or all. This applies at once. "All" also blocks script access
  (`document.cookie`) through Chromium's `--test-third-party-cookie-phaseout` switch (needs
  a restart); the profile preference alone did not block it in CEF 156.
- **Tracking parameters** (`utm_*`, `fbclid`, `gclid`, ...) are stripped before navigating.
- **`DNT: 1` and `Sec-GPC: 1`** headers on every request, plus `navigator.doNotTrack` and
  `navigator.globalPrivacyControl` in every frame.
- **HTTPS-only mode** (optional) with a "Continue to HTTP site" interstitial; exceptions last only
  for the session.
- **WebRTC** uses only the default public interface, and local addresses are replaced with
  random mDNS names, as in Chrome, so pages never see your private IP. To answer for those
  names, Chromium's mDNS responder listens on UDP port 5353 (IPv6) once a page creates a WebRTC
  connection. Windows Firewall therefore asks once per program path whether Krunk may use public
  and private networks. **Cancel is safe:** it only blocks inbound mDNS, and WebRTC and the
  hiding of local addresses keep working. The installer can't pre-answer this, because adding a
  firewall rule needs administrator rights. Updates keep the same path
  (`%LOCALAPPDATA%\Programs\Krunk\Krunk.exe`), so your choice carries over to later versions.
- **No background contact with Google:** see "Engine background traffic" below.

### Engine background traffic
Chromium's own services contact Google in the background. A logging-proxy audit of an idle,
fresh profile found secure-time queries, the component updater (and the downloads it triggers),
an "AI mode" eligibility check and Safe Browsing list updates. Krunk switches all of
them off by default, verified with the proxy: a fresh profile idle for five minutes plus
page loads contacted **no `*.google.com`, `*.googleapis.com` or `*.gvt1.com` host**. The only
traffic not caused by pages is the filter-list update and Krunk's update check (below).

The settings **"Download engine security components from Google"** and **"Protected content
(Widevine)"** (both off by default, restart required) let the component updater run. It then fetches the certificate revocation list
(CRLSet) and the Certificate Transparency list from `update.googleapis.com` and
`edgedl.me.gvt1.com`. The updater installs Chrome's standard set of components, because CEF has
no per-component switch. Everything else stays off.

These services run in the browser process, not in a tab, so the ad blocker cannot see them;
they are controlled with engine switches. The WebView2 build got the same lists from Edge's
updater.

Segmentation-platform data (`profile\segmentation_platform`, which records visited URLs outside
history) is deleted at every start. Its switch crashes this engine build.

### Local network access

Chromium 156 enforces Local Network Access: before a public page may fetch from a private
address (a NAS, a Plex server on the LAN, `*.plex.direct`) or from this computer (localhost),
it asks the embedder for a permission. Without an answer the request waits forever, which is
what happened in 0.4.0. Krunk now answers: a prompt under the toolbar ("Connect to devices on
your local network?" or "Connect to this computer?") with Block and Allow, remembered for the
site when "Remember for this site" is left on. Stored answers are listed and can be removed on
the site's Cookies and site data page (shield panel → Cookies and site data → Permissions).
Private tabs ask every time and remember nothing. Dismissing the prompt blocks once.

### Site permissions

Since 0.6.0 Krunk asks about the other permissions too, in the same prompt: **camera**,
**microphone** (and both at once), **location**, **notifications**, **clipboard reading**,
**MIDI devices** (with system exclusive) and **screen sharing**, next to local network access.
Camera, microphone and screen sharing arrive through CEF's media-access callback; location,
clipboard, MIDI and notifications through its permission-prompt callback; and because the Alloy
runtime never asks the embedder for notifications, a small script in every web page makes
`Notification.requestPermission()` ask Krunk and resolves once the answer is written into the
engine's content settings. Every answer is per origin: "Remember for this site" stores it,
Settings → Permissions lists the stored answers (change or remove them) and sets the default
per kind (ask, or block without asking), the lock popup shows and changes the current site's
answers, and clearing a site's data on the Cookies and site data page forgets them. While a
page uses the camera or microphone a red chip in the address bar says so. Screen sharing shows
the camera chip (the engine reports it as a video capture).

Once allowed, `new Notification()` shows a normal Windows toast (the engine's own notification
UI). What stays missing: a device picker (the engine uses the default camera and microphone,
and screen sharing shows no window chooser: the whole screen is shared).

### Krunk account (0.8.0)
Settings → Account signs this browser in to a Krunk account (made on
[krunk.209.wf](https://krunk.209.wf/account/register)). A signed-in browser shows who it is
signed in as and appears in the account's device list on the website, where it can be renamed or
removed. Since 0.9.0 the account can also sync your data (below, opt-in). Since 0.11.0 a new
browser's name is "Krunk on Windows 11" (it carried the version at sign-in, which went stale
with the first update); Settings → Account and the website show the version it runs now next to
the name ("Krunk on Windows 11 · version 0.11.0"). Names made before 0.11.0 are shown without
their old version; a name you typed yourself is never changed.

- **Signing in** works like signing in a TV: **Sign in** asks krunk.209.wf for a code, shows it
  and opens `https://krunk.209.wf/link?code=…` in a new tab. There you sign in to the website,
  check the device and the code, enter your password again and click **Approve this device**
  (or **Deny**). Krunk asks every 5 seconds whether the code was approved; the code works for 10
  minutes and only once. **Cancel** stops it. The ⋮ menu shows "Sign in to Krunk" or "Signed in
  as …", both open the card.
- **Signed in**, the card shows the address and the device's name, **Manage account** (the
  website's account page) and **Sign out on this device**. Signing out deletes the sign-in on
  this computer at once and then tells the server, which revokes the device; if the server can't
  be reached, the card says the device may still be listed on the account page.
- **What talks to the network:** nothing until you click Sign in. After that, only signing in,
  the Account card while it is on screen (at most every 10 minutes: Krunk renews its token and
  asks who it is, which notices a device removed on the website) and signing out. There is no
  background traffic. A device unused for 90 days must sign in again.
- **What is stored where:** `%LOCALAPPDATA%\Krunk\account.json` holds the account and device
  ids, the address, the device name and the refresh token, the token wrapped with DPAPI for your
  Windows user (like the vault key, with its own entropy). The access token (15 minutes) and the
  sign-in code live only in memory. No token or code is ever written to `krunk.log`.
  `browser.db` and the vault are unchanged.
- **Private windows** show nothing of the account (the card says so; the ⋮ item is hidden).
- **The server side** (sign-in by code after RFC 8628, rotating tokens whose reuse signs the
  device out and sends a mail) is described in `docs/sync/PROTOCOL.md` §6.1 and §11.3–11.4.

### Sync (beta, 0.9.0)
Sync keeps bookmarks, history, settings, new tab pins, ad-blocker exceptions, open tabs and
(with the password vault unlocked) passwords and the never-save list the same on your devices.
It is **off until you turn it on**, and everything is **encrypted on the device** before it
leaves: the server stores ciphertext it can't read. The website never asks for your sync
passphrase or recovery key.

- **Turn it on:** sign in (Settings → Account), then Settings → Sync (beta) → **Turn on sync…**.
  On the first device Krunk makes a **passphrase** (7 words) and a **recovery key**
  (`KRNK-…`). Copy them or save them to a file, keep them safe and private, type the asked group
  of the recovery key and confirm. With the vault unlocked, passwords take part too.
- **Another device:** sign in, Turn on sync. Then either **ask a device that already syncs**: it
  shows "A device asks to join" in krunk://sync; Approve shows 6 digits that you type on the new
  device (a wrong code ends the request), optionally sharing passwords. Or **restore** with the
  passphrase or the recovery key (a `KRNK-` key is recognised as one). Krunk lists the devices
  that keep access before it joins. Passwords come only into an unlocked vault.
- **When it runs:** a minute after start, every 5 minutes, 30 s after a change, and with **Sync
  now**; offline it waits and tries again (1 → 60 minutes). Settings → Sync shows the state and
  the last sync and lets you switch each kind of data off on this device.
- **krunk://sync** lists your devices (this one first), lets you rename them, shows other
  devices' open tabs (read-only), and **Remove**s a device: its access ends at once and Krunk
  re-secures your sync with new keys, which the removed device never gets. Afterwards **Finish
  securing** asks for the passphrase or recovery key once.
- **Conflicts:** bookmarks merge per field (place and name separately) and keep one tree; history
  is the union of visits; settings and logins take the later change. A login that lost a conflict
  is kept inside the vault for 30 days. The same login saved on two devices becomes one.
- **Turn off** on a device: keep what it has, or remove what came from sync. The account and the
  other devices are unaffected.
- **What stays on this device:** proxy, DNS, user agent, downloads folder, update and window
  settings, and the tab sleep options (a test lists every setting as synced or device-local).
  Payment cards aren't synced. Private windows sync nothing.
- **Stored where:** `%LOCALAPPDATA%\Krunk\sync.json` (the sync keys, sealed and DPAPI-wrapped
  for your Windows user), sync bookkeeping in `browser.db`, and the passwords key plus its
  bookkeeping only inside the vault, so synced passwords are never weaker than the vault.
- **Losing everything:** if you lose all devices, the passphrase and the recovery key, the
  synced data can't be recovered by anyone (Krunk can't either). The website's data and backups
  hold only ciphertext.
- Design and protocol: `docs/sync/PLAN.md`, `PROTOCOL.md`, `THREAT_MODEL.md`.

### Password vault
```
master password --Argon2id (random 16-byte salt, 64 MiB, t=3, p=1)--> KEK      (never stored)
KEK --XChaCha20-Poly1305--> key-check record + wrapped vault key VK (random 32 bytes)
wrapped VK --DPAPI (CurrentUser)--> stored in vault.json
VK  --XChaCha20-Poly1305 (fresh 24-byte nonce, AAD = entry id)--> each login; manifest of ids
```
- **Storage:** passwords live in `vault.json`, separate from the SQLite database. Origins,
  usernames and notes are encrypted too. Every record is authenticated, so tampering,
  swapping or removing entries is detected.
- **Copying the file is not enough:** someone who copies `vault.json` needs the master
  password *and* the same Windows account on the same PC (DPAPI).
- **Changing the master password** re-keys everything (new salt, KEK and VK) and replaces the
  file atomically.
- **Secrets in memory** are held in `Zeroizing` buffers and wiped when the vault locks. They are
  never logged and never stored in SQLite. Messages carrying a password (login submit, fill)
  also pass through the engine's IPC strings, which Krunk cannot wipe.
- Threat model (assets, attackers, mitigations, out of scope): `docs/SECURITY.md` in the
  source (not part of the download, which ships no Markdown files).
- **Unlocking:** off the UI thread; the host enforces an exponential retry delay (1 s → 60 s).
- **Auto-lock:** after inactivity, on Windows lock and on sleep.
- **Login detection** (main frame only) offers "Save password?" in the browser's popup window.
- **Fill:** a key icon in the address bar fills a login only after the user picks it, and only
  on an exact origin match.
- **Copies:** excluded from clipboard history and cleared after 30 s.

#### Recovery phrase (0.7.0)
```
24 BIP39 words = 256 random bits (never stored)
  --Argon2id (own 16-byte salt, at least the master password's cost: 64 MiB, t=3, p=1)--> seed
  --HPKE DeriveKeyPair--> X25519 key pair; only the public key is stored
VK --HPKE base mode (X25519-HKDF-SHA256, HKDF-SHA256, ChaCha20Poly1305, info "krunk-vault-recovery-v1")
   --> sealed to that public key --DPAPI (CurrentUser)--> "recovery" in vault.json (format 3)
```
- **Making one:** `krunk://passwords` → Recovery phrase → Create recovery phrase. After the
  master password Krunk shows 24 words once (Copy keeps them out of clipboard history and
  clears them after 30 s; Save to file and Print are offered), then asks for three of them
  back. Only then is the phrase stored in the vault — as the public key above; the words, and
  the 256 bits they stand for, are never written anywhere or logged.
- **Forgot the master password:** "Forgot master password?" on the locked passwords page or in
  the unlock prompt. Type the 24 words (case, spaces, line breaks and numbering do not matter;
  the first four letters of each word are enough), choose a new master password: the vault
  opens and the new password replaces the old one. Wrong phrases count against the same retry
  delay as wrong passwords.
- **It stays valid** when you change the master password (each save seals the new vault key to
  the stored public key); **a new phrase** (Make a new phrase) replaces the old one, which then
  no longer opens anything; Remove takes it away (the file goes back to the 0.6.0 format).
- **Threat model:** anyone who has the 24 words *and* your Windows account on this PC (DPAPI)
  can open your passwords and cards, exactly like someone with the master password. Keep the
  words offline. The phrase is local: nothing about it leaves the computer (sync would leave the
  slot out). Vaults without a phrase keep the 0.6.0 format, so 0.6.0 can still read them.

### Custom DNS

Settings → Privacy → DNS (0.6.0) chooses who resolves names: Windows' resolver (the engine's
default, upgraded to encrypted DNS when the system resolver is known to offer it), or
Krunk's own ordered list. The list takes plain UDP/TCP servers, DNS over TLS (`tls://`), DNS
over HTTPS over HTTP/2 or HTTP/3 (`https://`, `h3://`), DNS over QUIC (`quic://`) and
`sdns://` stamps (DNSCrypt stamps are parsed and kept but skipped: the protocol is not
implemented), with presets for Cloudflare, Quad9, Google, AdGuard and Mullvad. The first
resolver answers; after 2 seconds or an error the next one is asked. Every resolver has a
Test button (the latency, or the error) and counters.

How it works: CEF's Alloy runtime only exposes Chromium's own DNS-over-HTTPS client, so Krunk
puts that client into *secure* mode (it then never falls back to the system resolver) and
points it at a loopback stub, `https://127.0.0.1:<port>/dns-query`, that Krunk runs in the
browser process (`app/src/dns.rs`: a small tokio runtime, `hickory-proto` for the
transports). The stub implements the ordered list and the fallback. Its certificate is
generated in memory at every start (P-256, never written to disk) and the engine trusts that
one key only, through `--ignore-certificate-errors-spki-list`. When every entry is a DoH
resolver, Chromium is given the URLs directly instead (the provider certificates are then
verified by the engine). Private tabs use the same network service, so they resolve the same
way. Resolvers given by name and without a bootstrap address (presets have one) are looked up
once through Windows' resolver, which the log says. A SOCKS proxy still resolves names at the
proxy; "secure" mode means no resolution at all when every resolver fails (by design).

### Request headers

Settings → Advanced → Request headers (0.6.0) edits what every request says about the
browser: the user agent (the 0.5 picker and custom string), `Accept-Language` (a comma list;
the engine adds the weights and `navigator.language` follows), the client hints
(`Sec-CH-UA`, `-Mobile`, `-Platform` and the high-entropy hints sites ask for:
platform version, architecture, bitness, model, full version list, WoW64) and custom headers:
set on every request or on the listed sites, or removed (an empty value). The user agent and
the hints go through DevTools' `Emulation.setUserAgentOverride` per tab, so the headers and
`navigator.userAgentData` always agree; a new tab starts blank and navigates once the engine
acknowledged the override, so even its first request carries it (popups opened by pages get
it a moment after their first request). A custom user-agent string without overridden hints
sends no client hints at all (as 0.5 did: a browser that is not Chromium has none).
`Accept-Language` travels with the same override (the only runtime route; the engine's
own client hints, read from the shell page at startup, go with it so nothing else changes),
for every request of every tab and `navigator.language`. Custom rules are applied in Krunk's request hook, which
runs before the network stack adds its own headers: `Host`, `Connection`, `Accept-Encoding`,
`Content-Length`, `Cookie`, `Sec-Fetch-*`, `Priority` and the `Sec-CH-UA*` family are
therefore read-only (the page lists them with the reason). Client hints are only sent on
https pages, like Chrome. Workers do not see the override (a limitation of the DevTools
route): their `navigator.userAgentData` is the engine's.

**Send client hints** (0.7.0; on by default, like Chrome): off, no `Sec-CH-UA*` header is sent
on any request, in normal and private tabs, and a site's `Accept-CH`/`Critical-CH` asks for
them in vain (no retry). The user-agent override then carries no metadata, Krunk's request
hook removes any hint header that would still appear, and `navigator.userAgentData` is
removed in pages and their frames and — by attaching to them through DevTools before their
first line runs — in dedicated and service workers. Since 0.7.1 the change reaches tabs that
are already open (their frames, also those of other sites, lose it at once; every engine
process started afterwards knows the setting from its first page), and a shared worker's
script gets one statement in front that removes it before the worker's own code runs (after a
`'use strict'` prologue, on the first line, so the script behaves the same). See Known
limitations for what is not reached.

Web pages see nothing of Krunk's page bridge since 0.6.0: the injected scripts (login
detection, page tokens, the notification shim) get it as a closure argument, and only Krunk's
own pages have `window.__lbHost`.

### Generated passwords

On a sign-up or change-password form (`autocomplete=new-password`, a password with its
confirmation, or a form whose text says sign up / register / new password — the page's hint
and Krunk's own heuristics in `core::passgen::wants_new_password` both have to agree), the
login dropdown under the password field offers **Use a strong password** with the generated
password shown (0.6.0; Settings → Passwords sets the length, 8–128, and whether symbols are
used; look-alike characters are never used). Choosing it fills every password field of the
form; the username is left alone. When the form is sent with that password and the vault is
open, the login is saved without asking and a toast offers **Undo** (deletes the new entry,
or puts the previous password back) and **Edit** (opens the entry on krunk://passwords);
otherwise the usual "Save password?" prompt appears. Nothing is offered in private tabs.

### Payment methods

The vault also keeps payment cards (0.6.0): the number, expiry, name on the card, a nickname
and a billing address — **never the security code**, which has no field anywhere and is not
read from pages. Cards are encrypted like logins (vault format 2, AAD `lb-vault:card:<id>`;
a vault without cards keeps format 1 byte for byte) and listed masked on krunk://passwords →
Payment methods; **Show** needs the master password again and hides the number after 20 s;
**Copy number** clears the clipboard after 30 s. On a checkout page (a field with
`autocomplete=cc-*` or a card-number name) the dropdown under the card field lists the saved
cards (or **Unlock Krunk passwords** first); choosing one fills the number, expiry
(`cc-exp`, `cc-exp-month`, `cc-exp-year`, two- or four-digit years as the field suggests) and
name of that form, and you type the code. When a form with a Luhn-valid card number is sent
from a main frame and the vault exists, Krunk offers **Save this card?** with the brand and
last four digits (Settings → Passwords → "Offer to save payment cards"; never in private
tabs; nothing is offered for a number already stored). A `payments` sync collection is
defined in `docs/sync/PROTOCOL.md` §9.5 under the passwords key; the upload waits for the
sync client.

### Chromium switches
Built by `core::settings::Settings::browser_switches` and added in
`CefApp::OnBeforeCommandLineProcessing`. Always:
- `--no-pings` disables hyperlink auditing (`<a ping>`).
- `--disable-domain-reliability` disables Chromium's domain reliability telemetry.
- `--disable-background-networking` stops Safe Browsing updates, variations and other
  background downloads.
- `--disable-chrome-login-prompt` (0.8.0) hands a site's HTTP authentication to Krunk's own
  sign-in dialog (with the vault) instead of the engine's.
- `--host-resolver-rules=MAP krunk.invalid ~NOTFOUND, MAP *.krunk.invalid ~NOTFOUND`
  (see "Internal origins").
- `--disable-features=AutofillServerCommunication,MediaRouter,OptimizationHints,InterestFeedContentSuggestions,NetworkTimeServiceQuerying,AimEnabled,AimServerEligibilityEnabled,AimServerRequestOnStartupEnabled`
  turns off autofill crowdsourcing, Cast discovery, optimization hints, feed suggestions,
  secure-time queries and the "AI mode" eligibility request.

Conditional:
- `--disable-component-update`, and `MAP update.googleapis.com ~NOTFOUND` in the host rules,
  unless "Download engine security components" is on.
- `--test-third-party-cookie-phaseout` when "Block third-party cookies" is on (the default).
  Passed once even if also typed into the extra flags.
- `--disable-gpu` when hardware acceleration is off.
- `--proxy-server=<value>` when a proxy is set, with `--proxy-bypass-list` for the internal hosts
  (and the updater host).
- The user's extra flags, validated. `--disable-features` and `--enable-features` lists are
  merged, and our disabled features stay disabled. Security-weakening switches such as
  `--no-sandbox` and `--disable-web-security` are rejected.

CEF is also started with the sandbox on, `command_line_args_disabled` (the exe's own command line
cannot configure the engine) and log level WARNING. Profile preferences on every request context:
no password manager, no autofill, no network prediction (`net.network_prediction_options = 2`),
and WebRTC on the default public interface only.

Krunk runs no telemetry, update service or background process of its own. The only
network traffic it starts itself is the daily filter-list update and the daily update check
(both WinHTTP, see above and "Updates"), and, only after you click Sign in, the account calls
(see "Krunk account").

### Updates
- **What is sent:** ten seconds after start and then once a day, Krunk fetches two small files
  from its update server: `<server>/updates/win-x64/latest.json` and `latest.json.sig`.
  The request carries the user agent `Krunk-Update/<version> (win-x64)` and nothing else: no
  ID, no settings and nothing about your browsing. The server sees your IP address, as with any
  download; the current server is served through Cloudflare, which sees it too. Turn it off with Settings → About → "Check for updates automatically"; "Check for
  updates" there still works on demand. The check uses the configured HTTP proxy and is
  skipped with a SOCKS proxy, as the filter update is.
- **What is trusted:** `latest.json` must carry a valid Ed25519 signature made with Krunk's
  release key; the public key is built into Krunk (`assets/keys/update-signing.pub`). The
  installer it names must be on the update server itself, over HTTPS. After downloading,
  its size and SHA-256 must match the signed manifest before it runs. Older or equal versions
  are never offered, so there are no downgrades. Every rejection is written to `krunk.log`
  with its reason.
- **The notice (0.7.2):** a newer version puts a dot on the ⋮ button and "Update available –
  Krunk X.Y.Z" at the top of its menu; nothing pops up by itself. The menu item (and Settings →
  About → "Check for updates") opens the prompt. A failed check only writes its reason to
  `krunk.log`.
- **Installing:** the prompt has **Update now**, **Later** and **Release notes**. Update now downloads the installer to `%TEMP%\Krunk-update`, checks it, starts it
  silently (`/S /RELAUNCH`) and closes Krunk. The installer waits for Krunk to exit, installs,
  and starts Krunk again, with your tabs restored. A portable copy gets **Download**, which
  opens the website, instead.
- **Engine files and Repair (0.7.0):** Krunk knows the SHA-256 of every engine file of its
  release (built into Krunk.dll from `engine-files.sha256`). Twenty seconds after start it
  hashes the installed files (cached by size and date in `browser.db`); `krunk://gpu` and About
  name the engine ("Krunk's engine with proprietary codecs · libcef.dll c4e38bbf…" or "the old
  engine without H.264/AAC"). A changed or missing file opens a prompt with **Repair**, which
  reinstalls the current version from the signed manifest (portable copies get the download
  page). The signed manifest also names the release's full file list (`files.json`, its
  SHA-256 in the manifest), checked after every update.
- **The installer no longer skips locked files (0.7.0):** a silent install (the updater's) used
  to skip a file another program held and still report success, which could leave the old
  engine under a new Krunk. Now it ends Krunk processes still running from the install folder,
  moves loaded files aside (`*.old-krunk`, deleted at the next start) and, if a file can still
  not be replaced, stops before changing anything (exit code 3); Krunk shows the reason at its
  next start and offers Repair.
- **The server** is set when Krunk is built (`UPDATE_BASE_URL`). Builds use
  **`https://krunk.209.wf`**, the project's website and update server for now (a temporary
  domain while Krunk is tested). Moving to the final domain takes one release from this server
  whose build points at the new one. Krunk 0.2.0 builds pointed at the placeholder
  `https://krunk.example` and can't update themselves; reinstall them from the website. The
  server itself (Docker, Caddy, behind Cloudflare) is in `dist/server/`; see its README.

### Memory
- **Lazy restore:** restored tabs show their saved title and the cached favicon. They get no
  browser (and so no renderer process) until they are first activated.
- **Background sleep:** after `tab_sleep_minutes` (default 5) without activation, a background
  tab's JavaScript heap is garbage-collected (DevTools protocol `HeapProfiler.collectGarbage`).
  The tab is already hidden, so the engine throttles it (since 0.10.0, see "Background tabs"
  below). A second stage 60 s later
  repeats the collection. Activating the tab shows it again, with no reload. Pages are not
  frozen; see "Known limitations".
- **Discard** (optional, off by default): after `tab_discard_minutes` the tab's browser is
  closed. URL, title and favicon are kept, and the tab reloads when activated. A discarded
  private tab keeps only its URL. The last live private tab is never discarded, so the private
  session (cookies, logins) survives.
- **Never touched:** the active tab of every visible window, tabs playing audio, tabs with a
  download in progress, tabs with DevTools open, and the UI shell and popup. A small tracker in each frame reports playing unmuted media and
  running Web Audio to the browser.
- The rules are pure functions in `core::memory` (`decide`, `exempt`), with unit tests. The
  host evaluates them on a timer.
- Hidden options, not shown in the UI:
  - `--sleep-test-seconds=N` scales the timers for testing: sleep after N s, the second stage
    N s later, discard after 4N s when discard is enabled.
  - `--mem-report` appends the summed working set and private bytes of the browser process
    tree to `%LOCALAPPDATA%\Krunk\memory-report.log` every 10 s. It includes tab and
    sleep counts, and renderer counts taken from each process's `--type=` switch. The tree is our
    process plus every descendant (the engine's subprocesses).

Measured on the Windows 11 VM (CEF 156.0.15 / Chromium 156.0.8078.80) as the sum over the whole
process tree. "10 sites" were Wikipedia, GitHub, Hacker News, MDN, BBC News, The Guardian,
Stack Overflow, Reddit, NYTimes and CNN, plus the new-tab page:

| Scenario | Processes | Renderers | Working set | Private bytes |
|----------|-----------|-----------|-------------|---------------|
| 1 tab (new-tab page) | 7 | 3 | 354 MB | 140 MB |
| 11-tab session restored (lazy) | 7 | 3 | 354 MB | 135 MB |
| 10 sites open, awake | 22 | 18 | 2955–3044 MB | 1731–1802 MB |
| 10 background tabs asleep | 22 | 18 | 2629–2633 MB | 1369–1372 MB |
| 10 background tabs after the second stage | 22 | 18 | 2630–2639 MB | 1368–1372 MB |
| after discard (10 unloaded, CNN active) | 10 | 6 | 722–745 MB | 422–425 MB |

The WebView2 build measured 379 MB idle, 2131–2173 MB / 1165–1291 MB with the 10 sites awake,
1084–1179 MB after its low-memory and suspend stages, and 973–988 MB / 660 MB after discard.

How to read these numbers:
- The working-set sum counts shared DLL pages once per process, so it overstates physical RAM.
- With many sites awake the CEF build uses about 40 % more: Chromium isolates more sites,
  including cross-site ad and video frames, into their own renderer processes (18 against 15).
- Sleep saves about 11 % of the working set and about 21 % of private bytes. WebView2's
  "low memory" target trimmed more (about 45 % of the working set); CEF has no equivalent.
- Discard frees most memory and ends the renderer processes.

To reproduce: enable discard in Settings, start with `--sleep-test-seconds=90 --mem-report`,
open the sites and read `memory-report.log` (steps in `docs/MANUAL_TEST.md`).

## Building from source
The release is cross-compiled on Linux for `x86_64-pc-windows-msvc` with `cargo-xwin`. CEF's
Rust bindings (`cef` crate) build CEF's C++ wrapper library and support only the MSVC target:
`x86_64-pc-windows-gnu` fails with "Unsupported target triplet". One command builds and
packages everything:

```sh
scripts/build-dist.sh            # -> dist/Krunk-win-x64/, the zip, Krunk-Setup-<version>.exe, SHA256SUMS
scripts/build-dist.sh --debug    # same layout with a debug build (testing only)
scripts/build-dist.sh --dev      # dev-updates build (update check accepts http://) -> target/dev-dist/
UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh   # another update server
```
`UPDATE_BASE_URL` must be `https://` (the build fails otherwise, except with `--dev`).
`KRUNK_UPDATE_PUBKEY_FILE` replaces the trusted public key (test keys for dev builds only).

What it does:
1. `scripts/fetch-cef.sh` downloads the CEF archive pinned in `cef.lock` (the "minimal"
   Windows x64 distribution, about 175 MB) into its own cache directory
   (`~/.cache/krunk-cef/downloads`). It checks the SHA-1 published in CEF's build index
   (`cef-builds.spotifycdn.com/index.json`) **and** the SHA-256 pinned in `cef.lock`, and only
   then extracts it with `export-cef-dir`. Nothing from CEF is committed to the repository.
2. `cargo xwin build --release --target x86_64-pc-windows-msvc -p app --lib` builds
   `Krunk.dll` with the static CRT (`.cargo/config.toml`) and `libcef.dll` delay-loaded.
3. It assembles `dist/Krunk-win-x64/`: `Krunk.exe` (the CEF bootstrap, branded by
   `tools/pebrand`), `Krunk.dll`, the CEF DLLs, `.pak` files, `icudtl.dat`,
   `v8_context_snapshot.bin`, all `locales/`, `licenses/` and `THIRD-PARTY-NOTICES.txt`. No
   Markdown: the build fails if any `*.md` file ends up in the folder (README and SECURITY are
   in the source and on the website). Then it writes the zip (fixed timestamps and order) and
   `SHA256SUMS`.
4. `scripts/fetch-nsis.sh` fetches NSIS 3.09 (Ubuntu's `nsis` and `nsis-common` packages,
   SHA-256 pinned in `nsis.lock`, unpacked with `dpkg-deb` into `~/.cache/krunk-toolchain`),
   and `makensis` builds `Krunk-Setup-<version>.exe` from `installer/krunk.nsi`. The installer
   deletes `*.md` from the program folder before copying (0.2.x shipped README.md and
   SECURITY.md); it never touches the data folder.

Requirements (versions used for the release):
- Rust stable (1.87 or newer; built with 1.99.1) with `rustup target add x86_64-pc-windows-msvc`
- `cargo install cargo-xwin --version 0.23.1`
- `cargo install export-cef-dir --version 156.3.0` (must match the `cef` crate version)
- LLVM **20.1.8** (`clang-cl`, `lld-link`, `llvm-lib`; the MSVC C++ headers need clang 19 or
  newer), unpacked from the official release tarball. The script expects it in
  `~/.cache/krunk-toolchain/llvm-20.1.8` or in `$LB_LLVM`.
- `cmake`, `ninja`, `curl`, `python3`, `zip`, `dpkg-deb` (for NSIS)

**Microsoft's licence:** cargo-xwin downloads the MSVC CRT and the Windows SDK (headers and
import libraries) from Microsoft. Building therefore means accepting Microsoft's licence terms
for them; `build-dist.sh` sets `XWIN_ACCEPT_LICENSE=1` for this (and only there). The versions
are pinned so that builds are reproducible: `XWIN_VERSION=17`, CRT `14.44.17.14` (MSVC 14.44.35220),
SDK `10.0.26100`; the download is cached in `~/.cache/krunk-xwin`.

A clean build from a fresh clone took 1 min 27 s on the build machine (CEF's C++ wrapper and
LTO), plus the one-time downloads of CEF (175 MB) and the MSVC CRT/SDK; it has no warnings. The
release profile uses LTO, `opt-level = "s"`, strip and `panic = "abort"`. Two clean builds
from the same checkout path give the same `Krunk.dll` except for a few bytes (12 to 20
measured): the PE time stamp and the debug-directory record (GUID). `Krunk.exe` and the CEF
files are always identical. The zip is byte-identical for the same `Krunk.dll` and
`SOURCE_DATE_EPOCH` (default: the HEAD commit time, which stamps every file). The installer is
not byte-reproducible: NSIS's compressed data differs between runs, while the files it
installs are identical.

**Alternative: native build on Windows** (documented, not used for the release). With Visual
Studio 2022 Build Tools ("Desktop development with C++", MSVC 14.44, Windows SDK 10.0.26100),
Rust, CMake and Ninja: set `CEF_PATH` to the folder produced by `scripts/fetch-cef.sh` (or
run `export-cef-dir` on the verified archive) and run
`cargo build --release --target x86_64-pc-windows-msvc -p app --lib`. Then assemble the folder
as `build-dist.sh` does.

### Updating the engine
Security fixes for the engine reach users only through new Krunk releases (see "Known
limitations"). To move to a newer CEF:
1. Set the new `cef` crate version (e.g. `=157.x.y`) in `app/Cargo.toml` and run
   `cargo update -p cef`. Install the matching `export-cef-dir` version.
2. Build the engine with the codecs: `scripts/cef-build/cross-build.sh` with the new CEF
   commit and Chromium tag (`scripts/cef-build/README.md`; about 2 h on 32 cores once the
   checkout is there). Copy the archive into `~/.cache/krunk-cef/downloads/` and the artifact
   store, set `CEF_VERSION`, `CHROMIUM_VERSION`, `CEF_ARCHIVE` and `CEF_ARTIFACT` in
   `cef.lock` (`CEF_SOURCE=self-built`) and run `scripts/fetch-cef.sh --pin` once to record
   the archive's SHA-256. (A stock Spotify "minimal" build, without H.264/AAC, works too:
   `CEF_SOURCE=spotify` plus `CEF_SHA1` from `https://cef-builds.spotifycdn.com/index.json`,
   but it lacks Krunk's patch: background tabs would run at full speed again.) Krunk's patches
   in `scripts/cef-build/patches/` must still apply (`cross-build.sh build` stops if one
   doesn't); re-record their SHA-256 in `cef.lock`.
3. `scripts/build-dist.sh`, then run the checks below and `docs/MANUAL_TEST.md` (including
   the media matrix: `scripts/vm-050-media.ps1`, `scripts/vm-052-engine.ps1`).

### Checks
```sh
cargo test --workspace                                                   # core: 323 unit tests
cargo clippy --workspace --all-targets -- -D warnings                    # Linux
cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo audit
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-ui.cjs [shots-dir]   # UI pages with a mock host
```
For `cargo xwin clippy`, export the same `CEF_PATH` and `XWIN_*` variables as `build-dist.sh`.
The app's own unit tests (DPAPI, message escaping) run on Windows:
`cargo xwin test --target x86_64-pc-windows-msvc -p app --no-run`, then run the test exe.

Testing on Windows:
- `scripts/vm.sh` deploys `dist/Krunk-win-x64` to a Windows test machine over SSH and
  drives it (start, stop, screenshots, keys, clicks).
- `scripts/vm-sandbox.ps1` shows the integrity level and token of every Krunk process.
- `scripts/vm-engine-check.ps1` shows which engine DLLs each process loaded and whether any
  `msedgewebview2.exe` was started by Krunk.
- `scripts/vm-mem.ps1` lists the process tree with memory figures.
- `scripts/vm-leak-scan.ps1` searches the data folder for strings that must never be stored,
  such as a test password.
- The Windows checklist is `docs/MANUAL_TEST.md`.

## Known limitations
- **The AI agent (0.12.0) was tested against a mock provider only.** No live provider (OpenRouter,
  OpenAI, Anthropic, Ollama or LM Studio) and no vision model was exercised before the release.
  The OpenAI-style and Anthropic request and stream formats are covered by unit tests. Also:
  - Models without tool calling can't run the agent (the panel says so).
  - A model without image input gets a notice instead of screenshots.
  - The sensitive-data keyword gate understands English words only.
  - Cross-origin iframes (such as a payment provider's card form) are not read. Beyond masking
    the ones that look like payment forms in screenshots, nothing is redacted there.
  - The agent doesn't act on Krunk's own pages.
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
  or making a new recovery key, deleting or exporting the synced data, a sync reset, payment
  cards, a way to see the kept previous versions of passwords, and trimming synced history beyond
  the first sync's 90 days / 20,000 pages (the server's quota stops pushes with a message instead).
  Tested on Windows with the Linux test client as the other devices; there is no other client yet.
- **The engine is bundled.** Chromium security fixes reach users only with a new Krunk
  release with a newer CEF. Krunk checks for one daily and offers it, but installs only when you
  choose Update now. Releases should follow CEF security releases promptly.
- **The update server is on a temporary domain** (`https://krunk.209.wf`). When the final
  domain exists, one release from it must point Krunk at the new domain
  (`UPDATE_BASE_URL=https://<domain> scripts/build-dist.sh`). 0.2.0 copies (placeholder server)
  must be reinstalled by hand.
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
  screen if that monitor is gone. Dragging across monitors with different scaling was not
  tested (the test machine has one monitor, at up to 125 %).
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

## Third-party components
- **Chromium Embedded Framework 156.0.15** and **Chromium 156.0.8078.80**: CEF is BSD-licensed
  (`licenses/CEF-LICENSE.txt` in the download). Chromium and the third-party code it contains are
  listed with their licences in `licenses/Chromium-CREDITS.html`. Since 0.5.2 the binaries are
  Krunk's own build of the unmodified CEF and Chromium sources (`scripts/cef-build/`), with
  FFmpeg's proprietary codec set (H.264, AAC, HEVC, AC-3/E-AC-3 parsing) compiled in;
  `Krunk.exe` (CEF's `bootstrap.exe`) carries Krunk's version strings and manifest. Decoders
  for H.264, HEVC and AAC are covered by patent pools (MPEG LA / Via LA); licensing for
  distribution is the distributor's responsibility.
- Filter lists:
  - EasyList and EasyPrivacy: GPL-3.0-or-later or CC BY-SA 3.0.
  - uBlock Origin uAssets: GPL-3.0.
  - Attribution and licences are in `assets/filters/` and `THIRD-PARTY-NOTICES.txt`. The lists
    are used unmodified, as data.
- Rust crates: see `Cargo.lock`. The main ones are `cef` / `cef-dll-sys` (Apache-2.0 OR MIT),
  `windows` (MIT/Apache-2.0), `adblock` (MPL-2.0), `psl` (MIT/Apache-2.0), `rusqlite` with
  bundled SQLite (MIT, public domain), and RustCrypto `argon2`/`chacha20poly1305`
  (MIT/Apache-2.0). The build tool `tools/pebrand` uses `editpe` (MIT).
- **No third-party editor or Markdown library (0.12.0).** The code editor of `krunk://scripts`
  (`ui/shared/editor.js`, `jstok.js`), the agent panel's Markdown renderer
  (`ui/shell/markdown.js`) and the SSE parser are Krunk's own code. Nothing new is bundled.
