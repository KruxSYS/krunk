# Krunk user guide

Installing Krunk for Windows, where it keeps your data, and how to use it. Part of the [Krunk README](../README.md); the other pages are listed there under *Documentation*.

## Download and requirements

- **Installer:** `Krunk-Setup-0.29.0.exe` (about 142 MB), from `https://krunk.kruxsys.com`. It installs for the current user
  only, without administrator rights:
  - to `%LOCALAPPDATA%\Programs\Krunk`, with a Start Menu shortcut and, if you tick it, one on
    the desktop;
  - with an entry in Settings → Apps → Installed apps (per user, `HKCU`).
  If Krunk is running, the installer asks you to close it. `Krunk-Setup-0.29.0.exe /S` installs
  silently; it then closes a running Krunk itself and waits up to 30 s for it.
  **Uninstalling** removes the program and the shortcuts. Your data (`%LOCALAPPDATA%\Krunk`) is
  kept unless you tick "Also delete my Krunk data"; a silent uninstall (`/S`) always keeps it.
- **Portable:** `Krunk-win-x64.zip` (about 189 MB; 432 MB unpacked). Extract it anywhere and
  run `Krunk.exe`. Keep all files of the folder together; if one is missing, Krunk says so at
  startup. The portable copy uses the same data folder as the installed one.
- **Verify:** check `SHA256SUMS` (the installer, the zip and the three main binaries):
  - PowerShell: `Get-FileHash Krunk-Setup-0.29.0.exe`
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
Type a URL or search words in the address bar. Search uses Google by default (since 0.22.0; profiles still on the old DuckDuckGo default
switch once); this can be changed in Settings. Built-in pages:
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
- `krunk://sessions` (0.15.0): your session logs, when Settings → Privacy & security → Keep
  session logs is on (see Privacy below); ⋮ → **Session usage** opens it (0.16.0)
- `krunk://wasm` (0.19.0): the WASM memory inspector for one tab; ⋮ → **WASM memory inspector**
  opens it for the tab you are on (see "WASM memory inspector" below)

Old `lb://` addresses (from LightBrowser) open the matching `krunk://` page.

| Keys | Action |
|------|--------|
| Ctrl+T / Ctrl+Shift+N | New tab / new private tab |
| Ctrl+N | New window |
| Ctrl+W, Ctrl+Shift+T | Close tab (the window closes with its last tab), reopen closed tab (in the current window) |
| Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+1…8, Ctrl+9 | Next / previous tab, tab by position, last tab |
| Ctrl+L, Alt+D, F6 | Address bar |
| F5 / Ctrl+R, Ctrl+F5 / Ctrl+Shift+R | Reload, reload bypassing the cache |
| Alt+Left / Alt+Right, Alt+Home | Back / forward, home page (the Home button is optional: Settings → Appearance, 0.16.0) |
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
  Middle-click an entry (0.18.0) to open it in a new background tab next to this one; the new
  tab gets this tab's whole back/forward list with that entry current, this tab stays where it
  is and the menu stays open. A private tab's copy is private. (With an engine that lacks
  Krunk's `chromium-clone-navigation.patch` the new tab opens the entry's address only.)
  **Reload** (0.18.0): a middle-click or Ctrl+click opens the page again in a new tab. Before a
  reload or a new address, a tab that still waits for six or more answers has those loads
  stopped, so a stuck tab can always be reloaded (why: [docs/NETWORK.md](NETWORK.md)).
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
  (0.8.1). **Since 0.15.0 the dialog belongs to its tab:** switching tabs, another app or a menu
  only hides it; the tab shows a key badge and the dialog comes back when you return (or click
  the tab or its badge). A background tab can ask too and waits with the badge. Only Cancel/Esc,
  closing the tab or a new navigation in it cancels the request.
- **Appearance (0.7.0):** colour pickers (presets, any colour, a hex field, Reset) for the
  accent, the window background, the toolbar, the tab strip and the new-tab page; changes show
  at once in every window, and text on a custom colour switches between dark and light to stay
  readable (WCAG AA where a colour allows it). The new-tab page has ten built-in wallpapers
  (made for Krunk, see THIRD-PARTY-NOTICES) besides your own picture or a colour, chosen in its
  Customize panel or in Settings → Appearance; long site titles on its tiles wrap to two lines.
- **Toolbar items:** Settings → Appearance shows or hides the downloads button, the zoom
  indicator, the extensions button and (0.16.0, off by default) the **Home button**
  (`appearance.showHomeButton`). The Home button sits after Reload; a click does what Alt+Home
  does: it opens the home page, or the new tab page when the home page is empty. Ctrl+click or
  a middle-click opens it in a new tab. The **Home page** field under the switch is the same
  setting as General → On startup → Homepage (`homepage`; `https://www.google.com/` by
  default): editing either one changes both. The change shows at once in every window. In a
  window too narrow for everything the Home button goes in the last step, with downloads, VPN
  and AI; Back, Forward and Reload stay.
- **Tab and toolbar style (0.15.0):** Settings → Appearance → Tab and toolbar style. The tab strip
  and the toolbar can each be solid, a gradient (two or three colours and an angle; empty
  colours follow the accent) or glass (translucent with a blur inside Krunk's bars; the desktop
  does not show through, because the engine's windows can't be transparent); the tab strip can
  also be an ombre that runs across all tabs. Ready-made styles (Classic, Sunset, Ocean, Glass,
  Mono) fill these in. Tab shape (rounded with a roundness, square, pill), the active tab
  (filled, underlined, outlined) and an accent glow are separate. Text colour on a gradient is
  chosen for contrast across all its colours, in light and dark. The agent can change these.
- **Private tabs on every style (0.16.0):** the title and the mask icon of a private tab are
  drawn in the first of Krunk's private purple `#6d28d9`, lighter purples (`#a78bfa`, `#c4b5fd`,
  `#ddd6fe`, `#ede9fe`), darker ones (`#5b21b6`, `#4c1d95`, `#2e1065`), white, the dark text
  colour and black that reaches a contrast of 4.5:1 (WCAG AA) on everything drawn behind that
  state of the tab, in the light and in the dark theme:
  - an inactive private tab lies flat on the strip (it is no slice of an ombre): the strip's or
    the window's colour, every point of a gradient strip, or the glass strip over its accent
    backdrop;
  - under the mouse it is filled with the theme's hover colour; on a gradient or glass strip
    with a 15 % veil of black or white instead, whichever keeps the title readable (the title
    keeps its colour where that still reads);
  - the active private tab keeps its purple fill;
  - a window in the background uses the same colours (private tabs are not dimmed there).

  Where the purple already read (the light theme's Classic, for one) nothing changed. A gradient
  of your own that runs through mid-tones of very different lightness has no colour that reaches
  4.5:1 everywhere; the title then gets the best of the list. **Tab title colours** (Settings →
  Appearance → Tab and toolbar style, 0.15.2) apply to private tabs too. The colours are computed
  in `core/src/color.rs` (`PrivateTabColors`) and sent to the shell with `shellConfig`
  (`colors.private.light` / `.dark`); the shell only sets CSS variables. Every style, theme and
  state with its measured ratio is in `docs/private-tab-contrast.md`; that table and
  `ui/dev/private-tab-colors.json` are generated
  (`LB_UPDATE_FIXTURES=1 cargo test -p core --lib color::`), and `ui/dev/test-0160-private.cjs`
  draws the shell with those colours and measures the contrast from the pixels.
- **Tab previews (0.15.0):** resting the mouse on a tab for half a second shows a card with its
  title, its site and a small picture of the page (the active tab: no picture). Pictures are
  taken only from the tab you are looking at (once it has been in front for 1.5 s, and shortly
  after it loads; 0.25.0: no longer at the moment you switch away, which slowed every switch),
  kept in memory only (at most 48) and never for background tabs. Settings → General → New tabs
  → Preview tabs on hover turns cards and pictures off.
- **New tab page tiles (0.15.0):** pin, edit and remove sit in one column in the tile's corner;
  a pinned tile keeps a filled pin there.
- **Address bar:**
  - **Selecting (0.7.0):** dragging over the address selects text (also when the bar was not
    focused), a double click selects a word, a triple click everything; the site icon drags the
    address. A click on the page leaves the bar and shows the formatted address again; new tabs
    type into the address bar.
  - **Suggestions while you type** (0.5.1): a dropdown under the bar offers the search (or the
    typed address) first, then matching bookmarks and history pages, most visited first. Down
    and Up move through them (the bar shows the row's address), Enter or a click goes there,
    Esc closes the list and keeps what you typed; the bar keeps the focus throughout.
  - **Search suggestions (0.15.0):** below those rows, up to five suggestions from the search
    engine (Google, Bing, DuckDuckGo, Brave, Startpage, Ecosia; none for a custom engine), asked
    150 ms after you stop typing; an older answer never replaces a newer one, and recent answers
    are reused. The request carries only the typed text, without cookies, over the same
    connection settings as pages (proxy, VPN). Never in private tabs, never for addresses.
    Settings → General → Search → Show search suggestions (on by default).
  - **What opens and what searches (0.15.0):** typed text opens as an address only with a scheme
    (`http://test.test`), as an IP address, `localhost` or `name:port`, when its last part is a
    real top-level domain (the Public Suffix List Krunk already ships) or a local suffix
    (`.local`, `.lan`, `.internal`, `.home.arpa`). Everything else is searched: `test.test`,
    `node.js`, `me@example.com`.
  - The lock (or the "i" on http) opens the site-information popup: whether the connection is
    secure, the protocol, the certificate chain with validity, names, algorithms and SHA-256
    fingerprints, and what is wrong with a bad certificate (the engine reports no cipher).
  - **Certificate warnings (0.21.0; Android 0.6.1):** a certificate Krunk can't trust opens
    Krunk's own warning page (`krunk://cert-error`): the site, the reason, the SHA-256
    fingerprint, **Back to safety** and, unless the site demands a valid certificate (HSTS),
    **Continue anyway**. **Remember for this site** keeps the choice for the site (host and port)
    and that exact certificate in `kv["cert_exceptions"]` (never synced); a different
    certificate gets the warning again. Without it, and always in private tabs, the choice lasts
    until Krunk closes (private: the last private tab). The page's request is only accepted when
    it matches the error the tab really hit. Settings → Privacy & security → Certificate
    exceptions lists and removes them.
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
    The ⋮ menu lists the pages (History, Bookmarks, Downloads, Passwords & payments, Cookies
    and site data, **Session usage** (0.16.0: `krunk://sessions`, which offers to turn the
    logging on while it is off), Extensions, Scripts and macros).
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
  - **The page (0.15.0):** `krunk://downloads` lists downloads as cards grouped by Today,
    Yesterday and Earlier, with a search box, filters (all, in progress, done, failed) and
    "Clear completed" (files stay on disk). The list is kept in the profile database without the
    old 200-entry limit, and the page loads only what is on screen, so thousands of entries stay
    smooth (2,000 entries on the test VM: first rows in 32 ms, 64 fps scrolling).
    Settings → Downloads → Remove finished downloads from the list after 1–90 days (off by
    default).
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
  - **Payment cards import** (same place, the "payment cards" box): card number, expiry, name
    on the card and (Chrome/Edge) nickname go into the vault; security codes are never read.
    Firefox keeps cards in `autofill-profiles.json` with the number encrypted (AES-GCM) under a
    key it stores in Windows Credential Manager ("Firefox Encrypted Storage"); Krunk reads that
    key as the same Windows user, so no Firefox primary password is needed. Chrome, Edge and
    Brave keep them in `Web Data`, encrypted like passwords: cards encrypted app-bound (`v20`)
    are counted and have to be added by hand (Chrome has no card export). A card whose number
    is already in the vault with the same expiry is skipped; a later expiry updates it. Cards
    without a valid number (Luhn, 12–19 digits) or expiry are counted with the reason. Firefox's
    newer card store (`autofill.db`, not yet used by default) is not read; the result says so.
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
- **Casting (0.30.0):** send a tab or its video to a Chromecast, a TV with Google Cast built in
  (Chromecast with Google TV, Android TV), a Cast speaker or a DIAL device on your network. ⋮ →
  **Cast…** or right-click the page → **Cast…** opens the device picker for the current tab:
  - **This tab** mirrors the tab, picture and sound. Casting goes on while you use other tabs and
    stops when you press Stop or close the tab.
  - **Video on this page** sends the playing video itself (the engine streams it to the device,
    which then shows it full screen). A copy-protected (DRM) video, a page that forbids it
    (`disableRemotePlayback`), a format the device can't play or a device that only plays sound
    falls back to mirroring the tab, and the picker says so.
  - Sites with their own Cast button (YouTube and others using Google's Cast SDK or the
    Presentation API) open the same picker; the site's own app is offered first ("youtube.com").
    A `<video>` element's own controls show a cast button once a device is available.
  - Each device shows its state: available, connecting, casting (this tab), or busy with
    something else (with what it plays). Stop ends a session; while this tab casts the picker
    also has play/pause, a position slider and, for devices that allow it, volume and mute.
  - While a tab casts, a cast icon appears in the toolbar; it opens the picker.
  - Private tabs can be cast; the picker says it is a private tab.
  - Krunk looks for devices only from the moment you open the picker (or a site's cast button),
    then until Krunk closes. **The first time, Windows asks whether Krunk may use the network**
    (it covers the picker): answer **Allow** for private networks, then choose Cast… again.
    Without that permission no device is found.
  - Settings → Permissions → **Casting** turns it all off (needs a restart): no Cast items, no
    device search, and sites see no cast support.
  - **Krunk for Android (0.15.0):** the page menu has **Cast** and **Cast screen**.
    - **Cast** sends the page's video to a Google Cast device with Google's Cast SDK. You pick the
      device in Android's Cast dialog; the device then plays the video from its address, resuming
      where you were. The media notification and the full-screen controller give play/pause,
      seek and stop.
    - **Cast screen** opens Android's own screen casting (Settings → Cast) to mirror the whole
      screen.
    - Cast needs Google Play services: builds without them (`-Pkrunk.noGms=true`) and phones
      without Play services show only Cast screen. Krunk on Android TV shows neither.
    - A video Cast can't send (no address, a `blob:` or streaming-source video) gets "This video
      can't be sent to a Cast device. Use Cast screen to mirror the whole screen."; a page without
      a video says so.
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
  - **Right-click in the panel (0.16.0):** Cut, Copy, Paste and Select all in the message box;
    Copy on selected text of an answer, with "Search … for" and, when the selection is an
    address, the open items; on a link "Open link in new tab", "Open link in new window", "Open
    link in private tab" and "Copy link address"; on a step's screenshot (small or enlarged)
    "Save image as…" and "Copy image". New tabs open in the panel's window. Where none of this
    applies, no menu opens.
  - **Questions and Stop:** it may ask you a question with buttons. **Stop** (or Esc in the
    panel) ends the task at once: the request to the provider is cancelled, and keys or buttons
    it held are released. The model indicator in the panel switches between the provider's
    models. "+" starts a new conversation. A conversation belongs to its window, is kept in
    memory only, and is gone when the window closes.
  - **Scratchpad (0.16.0):** on longer tasks the agent keeps a plan (each item to do, doing, done
    or dropped) and named notes with what it found on the way, so it doesn't lose them when the
    conversation gets long. The notepad button in the panel's header shows them, updated while
    it works; click a note to read it. **Clear** empties the scratchpad of this conversation at
    once. It is kept in memory only: nothing of it is written to disk, and it is gone with a new
    conversation or when the window closes.
  - **What it can do:** read the page (text, HTML, links, forms, media, screenshots), click,
    type, fill forms, choose options, press keys, scroll, wait, run JavaScript in the page, edit
    the page, change its title, open, switch and close tabs, go back, forward and reload, save a
    screenshot to Downloads, change some settings, and write userscripts and macros.
  - **What it can't do:** see passwords, payment cards, the password vault, sync or account
    data, or API keys. Password and card fields are redacted, and its normal tools refuse them.
    It can't read or act on Krunk's own pages, and can't open the passwords, sync, site-data and
    storage pages at all.
  - **Passwords, only when you ask (0.16.0):** when your message asks for a password to be
    created or entered ("sign up here with a strong password", "create an account", "log in
    with the password …"), the agent may enter one, and Krunk asks you first, naming the site
    (once per site in a conversation).
    - **A new password** is made by Krunk's own generator, not by the AI, and goes into the
      form's password field and its confirmation. The AI never sees it. A card in the panel
      keeps it for you, hidden until you press **Show**, with **Copy** (cleared from the
      clipboard after 30 seconds); when you send the form, Krunk saves the login like one made
      with "Use a strong password", or asks.
    - **A password you give** in your message is entered as it is. It is part of your message,
      so it goes to your AI provider: prefer a generated one.
    - Without such a request nothing changes: the agent can't touch password fields. It never
      reads a password and never writes card fields.
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
    the tab row) and `GM_openInTab`. `unsafeWindow` is `window`. 0.19.0: `GM.wasm` (also
    `GM_wasm`), the WASM memory inspector's API (below).
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
  `run_at`, `all_frames`, CSS; since 0.17.0 also `world: "MAIN"`), the background service
  worker (since 0.17.0 a real worker), the toolbar button with its popup and badge, options
  pages, and these `chrome.*` APIs:

  | API | What Krunk provides |
  |---|---|
  | `runtime` | `sendMessage`/`onMessage` between all of the extension's contexts, **ports** (`connect`, `onConnect`, 0.17.0), `getURL`, `getManifest`, `id`, `onInstalled`, `onStartup`, `openOptionsPage`, `reload`, `getPlatformInfo`, `getContexts`, `setUninstallURL` (accepted) |
  | `storage` | `local`, `sync` (local only), `session`, `onChanged`; no 10 MiB limit with `unlimitedStorage` |
  | `tabs` | `query`, `get`, `getCurrent`, `create`, `update`, `remove`, `reload`, `sendMessage`, `connect`, `onUpdated`, `onActivated`, `onRemoved` |
  | `windows` | `getCurrent`, `getLastFocused`, `getAll`, `get`, `update`, `remove`, `create` (type `popup`: a small window with a page of the extension; else a tab), `onRemoved`, `onFocusChanged` |
  | `scripting` | `executeScript` (files or `func` + `args`, real results, `world: "MAIN"`), `insertCSS`, `registerContentScripts`, `getRegisteredContentScripts`, `unregisterContentScripts` |
  | `action`, `alarms`, `i18n` | as before |
  | `offscreen` | `createDocument`, `closeDocument`, `hasDocument` (one hidden page) |
  | `permissions`, `management` | `getAll`/`contains`/`request`; `getSelf` |
  | `declarativeNetRequest` | static rulesets become filter rules of the ad blocker (`block` and `allow`; `redirect` and `upgradeScheme` rules are counted as "not translated") |
  | `notifications`, `cookies`, `webRequest` | stand-ins: calls resolve (nothing is shown, no cookies are found), listeners are never called |

  Never provided: `debugger`, native messaging, dynamic or session rules and switching
  rulesets at runtime, `history`, `bookmarks`, `downloads`, `identity`, `sidePanel`,
  `contextMenus`, messaging between extensions and the rest; the page lists what each
  extension asked for and whether Krunk has it (**Not in Krunk**), and since 0.17.0 the
  errors of its contexts (**Errors**: uncaught errors, rejected promises and `console.error`
  of the background, popup, pages and content scripts, also in `krunk.log`). How the runtime
  works, why MetaMask did not load before 0.17.0 and what is still missing:
  [docs/EXTENSIONS.md](EXTENSIONS.md). Since 0.17.0 an extension's background and pages
  reach the hosts its manifest names past CORS, as in Chrome (Krunk relays those requests:
  no cookies, no streaming). Tested on Windows with MetaMask 13.56 (set-up, unlock after a
  restart, `window.ethereum`, connect, `personal_sign`), Dark Reader 4.9 and uBlock Origin
  Lite; not tested: sending a transaction, hardware wallets, other wallets. **Isolation:** extensions never run on Krunk's own pages or the shell, not in
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
  - **Right-click an extension's toolbar icon** (0.16.0) for its menu: the extension's name
    (opens `krunk://extensions#<id>`: its card marked, scrolled into view, Details open),
    **Options** (only with an options page), **Manage extensions**, **Disable** and
    **Uninstall…**, which first asks "Remove <name>?" in the same popup (Remove / Cancel; Esc
    or a click elsewhere cancels). It is built like the tab menu: the shell sends only the id
    and the click (`showExtMenu`), the host reads the name, the options page and the state
    from the installed extension (`PopupContent::ExtMenu`), and the popup answers with
    `extMenuAction`. The host acts only while that extension's menu is the open popup
    (`core::ext::menu_may_act`) and the extension is installed, through the same code as the
    switch and Remove on `krunk://extensions`; the page's own requests (`extSet`,
    `extRemove`, `extOpenOptions`) are still accepted from that page only.
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
