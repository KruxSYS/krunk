# How Krunk works

Architecture and the reference for each part of Krunk for Windows. Part of the [Krunk README](../README.md); the other pages are listed there under *Documentation*.

## Architecture

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

## Engine and processes
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

### Page loading (0.26.0)
Measured for 0.26.0 (`docs/perf-0.26.0/`: `fixes.md`, `final/table.md`).
- **What Krunk does to load pages faster:**
  - **"Preload pages for faster browsing"** (Settings › General › Tabs and memory; on by default,
    like Chrome). In normal tabs the engine's network prediction is on:
    - it follows the page's own `<link rel=preconnect>` and `dns-prefetch` hints;
    - it connects when a link is pressed;
    - it connects to the hosts a site usually needs on the next visit (the loading predictor; what
      it learned is cleared with the history);
    - it allows speculation-rules prefetch.

    Krunk also connects early to where the address bar's top suggestion goes.
  - **Never in private tabs:** their profile keeps prediction off, and Krunk's own preconnect isn't
    used there. Prediction is also off while the VPN should run and isn't connected (the kill switch's
    stand-in is in place, or Krunk is offline).
  - **The same blocking and the same route:**
    - early connections never go to a host the ad blocker blocks: the engine gets the lists'
      plain host rules (engine patch `chromium-preconnect-blocked-hosts.patch`);
    - they use the VPN when it runs, and none are made while the kill switch holds connections;
    - a page fetched ahead (speculation rules) goes through the ad blocker and tracking
      protection like any other request;
    - `<link rel=prerender>` fetches nothing (`NoStatePrefetchHoldback`): the engine ran those in a
      hidden page that Krunk's filter never saw;
    - the engine's own prerendering (Prerender2) is off in CEF.
  - **Request filtering:** the ad blocker's check runs on the network thread, without a round
    trip to the UI thread. The load trace measured 0.03–17 ms in total per navigation, spread over
    the page's requests. A cheaper variant was tried and dropped: no gain beyond the noise band.
  - **Back/forward cache:** on, as in Chrome. Krunk's page scripts don't block it; pages with their
    own `unload` listener or a `no-store` document load again, as in Chrome.
  - **Spare renderer:** the engine keeps one renderer process ready, as in Chrome. A private tab in
    between uses it up, so the next normal page starts its own.
- **What to expect** (Windows test VM, 4 vCPU, no GPU; 9 recorded pages behind a 40 ms round-trip
  shim; ms from the input, median over pages, 5 runs):

  | | Krunk 0.25.5 | Krunk 0.26.0 | Chrome 157 ¹ | Edge 156 ¹ |
  |---|---|---|---|---|
  | typed URL, cold, first byte | 170 ¹ | 64 | 145 | 376 |
  | typed URL, cold, LCP | 817 ¹ ² | 472 | 713 | 899 |
  | typed URL, LCP from navigation start | 488 ² | 370 | 418 | 412 |
  | link click, first byte | 158 | 137 | 142 | 144 |
  | link click, LCP | 559 | 562 | 669 | 590 |
  | back / forward (cached), first frame | 36 / 27 | 37 / 26 | 55 / 38 | 53 / 44 |

  - ¹ From an earlier session (2026-11-20), same corpus, VM, harness and round trip. Chrome and
    Edge auto-updated afterwards (158/157) and the harness couldn't drive their input any more.
    0.25.5's own numbers drifted up to 30 % between the sessions (link LCP 801 vs 559 ms), so
    these columns are indicative.
  - ² Biased: 0.25.5 lost the typed address on most cold starts (the address bar didn't take the
    keys after Ctrl+L), so only the loads that started count.
  - Same session, 0.25.5 → 0.26.0: link first byte −13.5 %, beyond the noise band. Link LCP and
    back/forward are unchanged, within the band.
  - These numbers come from a build that still had a 39 s start-up stall (fixed before release).
    The driver only starts measuring once Krunk answers it, and the replay allows no filter-list
    update that would stall it again. One YouTube row may have been caught; it changes no corpus
    median.

  On a real network the early connections save one round trip or more per new host. A replay
  with a fixed RTT shows that only partly, and DNS not at all (the replay maps every host, so
  there is no lookup to save).
- **Not done (and why):**
  - **V8 context snapshot:** about 7.7 ms more per load than Chrome, 2 % of LCP: below the bar for
    a near-full engine rebuild.
  - **Prerender2:** off in CEF.
  - **Tried and dropped:** cheaper request filtering, a later start for the login detector,
    writing history after the load, cosmetic filters off the navigation path, and buffering keys
    typed right after Ctrl+L. None beat the noise band (`docs/perf-0.26.0/fixes.md`).

## Windows and views
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
- **Display scaling:** Krunk is per-monitor DPI aware. Layout constants are in DIP and are
  converted with the scale of the window they belong to at the moment of placing
  (`core::dpi::Scale`). That scale is the one the engine draws at: the monitor's DPI scale
  times Windows' "Text size" setting; the shell reports its `devicePixelRatio` and the host
  follows it. When a window's scale changes (the display setting, or another monitor)
  it is laid out again, its pages are told, and popups sized for the old scale close
  (`Browser::frame_dpi_changed`). A window without a saved place opens at 1280 × 860 DIP, never
  larger than the screen; saved bounds carry the DPI they were saved at. What was wrong before
  0.15.4, the rules for new UI code and how it is checked: [docs/DPI.md](DPI.md).
- **Tabs:** one browser per *live* tab, laid out below the shell. Restored tabs stay unloaded
  until they are activated. Pages that open windows (`window.open`, `target=_blank`) get a new
  tab next to them; `window.open` keeps its opener.
- **Switching tabs:** background tabs are parked outside the window's client area and hidden
  from the engine (throttled like Chrome's background tabs, 0.10.0). A switch brings the new
  tab's view on top and parks the old one 100 ms later, so the old page shows until the new one
  has drawn: a tab that has never loaded waits for its first paint (at most 500 ms); a tab that
  painted before waits for its page to report `shown` (two animation frames after it became
  visible again, from `painted.js` over the page bridge; Krunk's own pages answer through
  DevTools), at most 150 ms (0.25.0, `core::reveal`). **Warm tabs (0.25.0, `warm.rs`):** the
  tab the pointer rests on for 80 ms (the shell's `tabWarm`), the tab just left and, after
  Ctrl+Tab, the next one are shown to the engine beneath the page on screen for 2.5 s (at most
  two), so a switch to them swaps at once; resting CPU is unchanged. **New tab page made ahead
  (0.25.0, `spare_tab.rs`):** one hidden new tab page under a reserved tab id; Ctrl+T opens the
  tab with that id. Made again after a theme change; never for private tabs or another new-tab
  setting. `KRUNK_PERF_TRACE=1` writes `perf.log` (per switch: path, wait, UI time, stalls;
  `scripts/perf/trace-summary.py`); the VM measurement is `scripts/vm-0250-tabs.ps1` with
  `scripts/perf/vm-0250-run.sh`. Since 0.26.0 it also writes one `load` line per top-level
  navigation (input, engine call, the engine's steps, first paint, Krunk's UI-thread work on
  the way, request and cookie filtering cost; `scripts/perf/load-trace-summary.py`).
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
  popup have no context menu, the AI panel's has no "Inspect" (0.16.0, see the panel below),
  and DevTools commands act on the active tab.
- **Popup:** one extra view in an owned `WS_POPUP` tool window, re-owned by the window it opens
  for. It shows the main menu, the tab menu, the menu of an extension's toolbar icon (0.16.0),
  the shield panel and the save-password and fill-login prompts.
  - It is created on first use and then hidden and reused, never more than one.
  - It uses the default profile, never a private one.
  - The shell sends the popup's rectangle in CSS pixels. The host converts it to screen pixels
    with the window's DPI (`GetDpiForWindow` + `ClientToScreen`) and keeps it inside the monitor
    work area (`core::layout::popup_screen_rect`).
  - Since 0.15.4 a popup is placed again once it has reported its content's height
    (`core::layout::place_popup`), by what it hangs on (`PopupKind::anchor`): a context menu
    opens above the click when there is no room below; the ⋮ menu, prompts and dialogs move up
    only as far as they must; lists (a bookmark folder, address suggestions, the tab list) stay
    under their control and scroll. Content higher than the room at its anchor first gets
    compact rows, and scrolls when even the whole work area is too low (a 1080p display at
    175 % has 569 px of it; the ⋮ menu needs 688 since 0.16.0's Session usage row, 658 before).
  - **Hide rules:** the popup hides on deactivate, Escape, a click elsewhere (another view gets
    focus), its window's move, resize or minimize, and tab switch or close. A prompt hidden this
    way counts as dismissed, never confirmed.
- **Keyboard shortcuts** are taken before the page sees them (`CefKeyboardHandler::OnPreKeyEvent`),
  in every view.

## Internal origins
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

## Messages between pages and the browser
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

## AI agent, userscripts and macros (0.12.0)

**Architecture.**
- The agent's loop runs in the **browser process**. Its logic is in `core::agent`, which is tested
  on Linux:
  - `session`: the conversation state machine; the user's message, stream events, tool results
    and answers go in, actions come out;
  - `provider` and `sse`: OpenAI-style requests for OpenRouter, OpenAI and custom servers;
    Anthropic's Messages API; the streamed answers;
  - `tools`: 49 tools with their JSON schemas and argument checks;
  - `context`: the context block and the system prompt;
  - `scratch` (0.16.0): the scratchpad's notes and plan;
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

**Attached files.** The panel's paperclip button, a paste into the message box, or a drop
on the panel attaches up to 10 files to a message: pictures (PNG, JPEG, WebP, GIF), PDFs and
text files (code, Markdown, CSV, JSON, logs and similar). The panel scales pictures down to a
longest side of 1568 px (JPEG, or PNG when that is smaller or the picture has transparency). Text
files are read as text. Everything goes in the one `agentSend` message, because the renderer's
bridge carries at most 8 M characters. The limits: PDFs up to 5 MB, pictures and PDFs up to
5.5 MB together, text files up to 2 MB, of which 100,000 characters go to the model.
`core::agent::attach` checks everything again in the browser process: the kind comes from the
bytes, never from the name, and pictures need a model with vision. A picture becomes an image
part of the user's message. A PDF becomes a document part (Anthropic `document`, OpenAI-style
`file`). Text files are quoted in the message's text with their names. The 8 newest attached
pictures and PDFs stay in the conversation, counted apart from the two screenshots. Nothing is
stored: attachments live only in memory, with the conversation.

**Output limit (0.15.0).** Each provider has **Max output tokens** in Settings → AI (default
32,000; 0 = auto: OpenAI-style providers then send no limit, Anthropic gets the model's maximum
or 32,000). A model's known maximum (from its model list, else a small built-in table) caps it.
When a model stops at the limit with no answer and no tool call, Krunk asks once more with twice
the limit (up to the maximum), then says which limit stopped it. A tool call cut off by the limit
is never run: the model is asked again with a higher limit, then told plainly that it was cut
off. A cut-off text answer is continued once. `save_userscript` and `save_macro` take a script
in parts (`more: true`, then the returned `draft`), up to the 200 KB script limit; it is checked
only when the last part arrives.

**Scratchpad (0.16.0).** The agent's working memory for a long task: named notes and a plan.
- **Tools:** `note_write` (replace or append), `note_read` (paged like `get_text`), `note_delete`,
  `note_list`; `plan_set` (the whole list) and `plan_update` (one item by index or id; states
  todo, doing, done, dropped). They are always offered and are answered inside the session
  (`core::agent::scratch`): a step in the panel and a result for the model, but the host never
  sees them.
- **Limits:** 64 notes, 32 KiB per note, 256 KiB of notes in total, names of 1–64 characters out
  of letters, digits, `-`, `_` and `.`; 100 plan items of at most 300 characters. An error names
  the limit and never repeats the content.
- **It survives the trimming.** Old tool results are shortened in the requests, so every request
  carries the scratchpad as it is now, in a marked block at the end of the system prompt
  (`<<<SCRATCHPAD … SCRATCHPAD>>>`; the one place all three provider formats share): the plan in
  full and the notes' index (name, size, first line). The plan's texts and the first lines are
  quoted on one line each, as data. A note's text comes back with `note_read`.
- **Per conversation, in memory only.** It lives in the window's session next to the
  conversation: "New conversation" and the panel's Clear empty it, and it goes with the window.
  Nothing of it is written to the profile, `krunk.log` (step lines carry the tool's name and
  status only; a core test drives every scratchpad tool with a marker and checks each log
  line) or session restore.
- **The panel's view** gets the whole scratchpad as an `agentScratch` event after every change and
  renders it as text. Clear sends `agentScratchClear`; the agent sees an empty scratchpad in its
  next request, also in the middle of a task.

**The panel** is a CEF view per window on its own channel (`shell.krunk.invalid/agent.html`), docked
right of the tab area. The frame paints and drags its splitter. Its width is a setting. It is
always docked; nothing moves it into a window of its own.

**The panel's context menu (0.16.0)** is the page menu of tabs, not a second one: a view is
created with `ViewOptions::menu` (`Menu::None` for the shell, the popup, extension popups and the
macro runner; `Menu::Page` for tabs; `Menu::Panel` for the panel), and `TabMenu` in
`app/src/ffi/cef/client.rs` builds and dispatches the menu for both. In panel form:
- Of the engine's own items only the editing ones stay (`core::page_menu::panel_keeps`: Undo,
  Redo, Cut, Copy, Paste, Paste as plain text, Delete, Select all, spelling suggestions). Back,
  Forward, Print and "View page source" are removed, "Inspect" is not added, and an item the
  engine may add later is not shown either. Leftover separators go
  (`core::page_menu::stray_separators`); an empty menu does not open.
- Link items, the open items for a selected address and "Search … for" are the tab's. Their
  commands reach the host through the same view events, connected by `hook_page_menu` with
  `MenuFrom::Tab(id)` or `MenuFrom::Panel(window)`: from the panel a tab is never private and
  goes where Settings → New tabs puts new tabs, in the panel's window. Only `http(s)` addresses
  get open items (`open_target`), and `open_selection` opens no others.
- Image items: only "Save image as…" and "Copy image" (`ImageAction::in_panel`), and only for
  the panel's own pictures (`image_menu::panel_image`: a step screenshot under `/agent-shot/…`
  or a `data:` image). The host takes the bytes from the agent's screenshot store, with no
  request; a saved screenshot is named `screenshot-<step>.jpg`. "Open image in new tab" and
  "Copy image address" are left out: the address is on the shell origin, which no tab can load.

**Tools.**
- **Reading:** get_page_context, get_html, get_text, query, list_links, list_forms, list_media.
- **Screenshots:** screenshot (to the model as an image when it takes images), save_screenshot.
- **Input:** click, type_text, fill_fields, select_option, press_keys, scroll, wait_for.
- **Page changes:** eval_js, edit_page, set_title.
- **WebAssembly memory (0.19.0):** wasm_memories, wasm_read, wasm_write, wasm_scan (mode first
  or next), wasm_results, wasm_freeze, wasm_unfreeze, wasm_watch, wasm_global (see "WASM memory
  inspector").
- **Tabs:** navigate, back, forward, reload, open_tab, switch_tab, close_tab, list_tabs.
- **Settings:** get_settings, set_setting, set_new_tab_background_url (0.15.0: an https picture,
  at most 15 MB, PNG/JPEG/WebP/GIF/AVIF by its first bytes, stored like one you pick; no extra
  confirmation, like the other settings tools).
- **Userscripts:** list_userscripts, get_userscript (the source, paged), save_userscript,
  toggle_userscript.
- **Macros:** list_macros, get_macro (the source, paged), save_macro, run_macro.
- **Talking to you:** ask_user, finish.
- **Scratchpad (0.16.0):** note_write, note_read, note_delete, note_list, plan_set, plan_update.
- **Asked for in words only:** get_history, get_bookmarks, get_downloads, get_cookies_for_site,
  fill_password (0.16.0).

**Privacy and redaction.**
- **No code path to secrets.** No tool, setting key or schema reaches the vault, payment cards,
  sync, the account or API keys; a core test checks the tool table. One tool, `fill_password`
  (0.16.0), writes password fields on request and reads nothing (see "Passwords on request").
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
  - `eval_js` and the `wasm_*` tools are refused while one holds a value.
  - Every result is scrubbed of the page's current secret values, read again after `eval_js` and
    after an agent-started macro, and of any card number.
  - Screenshots mask the fields, and cover cross-origin iframes that look like payment forms.
- **History, bookmarks, downloads and cookies** are offered only when the user's message names
  them (an English keyword gate in `core::agent::gate`), and after Allow once per conversation.
  They are never in the context block.
- **Passwords on request (0.16.0).** By default nothing changes: the agent can't read, type,
  fill or edit a password field. One tool, `fill_password`, writes password fields:
  - **When it exists.** Only for a task whose message asks for a password to be created or
    entered. The gate (`core::agent::gate`, English) looks for a word starting with `password`,
    `passcode` or `passphrase`; `signup`, `register`, `registers`, `registering`,
    `registration`; "sign up" ("sign me up"); "create/make … account" within three words, or
    "new account". "Log in" alone is not enough. Without it the tool is not offered and a call
    is refused.
  - **Consent per site.** The session asks the host for the target tab's origin
    (`core::agent::pages::password_site`: an http or https page, never one of Krunk's own),
    and the first use on that origin in a conversation asks the user, naming it. Allow or Deny
    holds for that site until the conversation ends. The allowed origin travels with the call,
    and the host enters nothing unless the tab still shows it.
  - **`generate`.** The browser makes the password with `core::passgen` (Settings → Passwords:
    length and symbols). It is entered only on a form where a new password is chosen, by the
    same test as the dropdown's "Use a strong password" (`passgen::wants_new_password`), into
    every visible password field of that form that can take a new one: so the confirmation
    too, but not a current-password or one-time-code field and not a read-only one. The tool's
    result has the number of fields and their selectors, never the value: the model does not
    see it. It is handed to the vault's save flow (`remember_generated`: sending the form saves
    the login with an Undo toast when the vault is open, else "Save password?"; nothing in
    private tabs).
  - **The card.** The window's agent keeps the generated password in memory until the
    conversation ends. The panel gets an `agentPassword` event without the value and shows it
    masked; **Show** asks for it (`agentPasswordShow`, answered to that panel only from a wiped
    buffer), **Copy** lets the browser put it on the clipboard like a password copied on
    `krunk://passwords` (kept out of the clipboard history, cleared after 30 s).
  - **`provided`.** The model passes on a password the user gave in the conversation, into the
    field(s) named by a selector, or the focused or only password field. That password is part
    of the conversation and so of the requests to the provider; Krunk keeps it out of the step
    log (the detail shows its length) and out of `krunk.log`.
  - **The probe** has one function for it, `fillPassword`: it writes `input[type=password]`
    fields only, with the native value setter and `input` and `change` events, and refuses
    card fields (a security code is often typed as a password) and everything else. All other
    functions refuse password fields as before.
  - **No leaks.** The value is in no step label, detail or summary, in no log line and in no
    tool result. After the fill every result is scrubbed as before (the fields are password
    fields, so `secrets()` reports them), and on top every tool result and context block of
    the conversation is scrubbed of the passwords generated in it, as they are, HTML-escaped
    and URL-encoded: a site that shows the password in its page or puts it in an address
    doesn't hand it to the model. In the browser the value is held in wiped buffers
    (`Zeroizing`), and the DevTools call that carries it to the page is wiped after sending.
- **Memory and logs.** Conversations and the scratchpad (0.16.0) are in memory only. The log holds
  step and tool names, statuses and times only, never content, addresses, keys, or a note's or
  the plan's text. Krunk itself writes no crash dumps: it sets up no crash reporter and no dump
  handler. (Windows can keep its own crash data if error reporting or local dumps are set up,
  and memory can reach the page file, as with any program.)

**API keys** are stored DPAPI-protected (current user, Krunk's own entropy) in the `ai_keys` table.
- A key is unwrapped only for the request that needs it, then wiped.
- It is never synced, exported, sent to a view or logged.
- The settings page sends a key once (`aiKeySet`, a zeroized secret) and only ever learns whether
  one is saved.

**Messages and allow-lists.**
- **Shell:** `toggleAgent`, `stopMacros`.
- **Agent panel** (sender `Agent`): `agentSend`, `agentStop`, `agentNew`, `agentAnswer`,
  `agentConsent`, `agentContinue`, `agentSelectModel`, `agentModels`, `agentClose`,
  `agentOpenScript`, `agentToggleScript`, `agentScratchClear`, `agentPasswordShow`,
  `agentPasswordCopy` (0.16.0), `openUrl` (web addresses only).
- **Macro runner** (sender `Runner`): `macroCall`, `macroDone`.
- **Settings page:** `aiProviderSave`, `aiProviderRemove`, `aiKeySet`, `aiKeyRemove`, `aiTest`,
  `aiModels`, `aiStatus`.
- **`krunk://scripts`:** `scriptsList`, `scriptGet`, `scriptSave`, `scriptToggle`, `scriptDelete`,
  `scriptExport`, `scriptRunNow`, `scriptCheck`, `macroSave`, `macroDelete`, `macroToggle`,
  `macroRun`, `macroStop`, `hotkeyCheck`, `hotkeyRecord`.
- **Events to the panel:** `agentConfig`, `agentState`, `agentDelta`, `agentMessage`, `agentStep`,
  `agentAsk`, `agentConsentAsk` (0.16.0: with the `site` for a password), `agentContinueAsk`,
  `agentScript`, `agentError`, `agentClear`, `agentScratch` and `agentPassword` (0.16.0).
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

## WASM memory inspector (0.19.0)
Games and apps compiled to WebAssembly keep their state (health, score, money, positions) in a
`WebAssembly.Memory`, not in the page. ⋮ → **WASM memory inspector** opens `krunk://wasm` for the
tab you are on; the page is bound to that tab (like the site-data page) and works Cheat-Engine
style:
- **Memories:** every memory found in the tab, with its size. **Find memories** looks again
  (a game makes its memory when it starts). A memory that grows (`memory.grow`) keeps its
  number, its scan results, its watch entries and its frozen values; addresses past the old
  size become valid, and the list and the hex view show the new size.
- **Instances:** every `WebAssembly.Instance` found, with its exports by kind (functions,
  memories, globals, tables, tags) and by name. A memory an instance exports says so in the
  memory list (`export "memory" of instance 1`); choosing an instance chooses its memory.
  Memories no instance exports (imported ones, or made by JavaScript) stay listed. The chosen
  instance's **exported globals** are a table of name, type (`i32 i64 f32 f64`; `v128`,
  `funcref`, `externref` are shown without a value) and live value; a mutable global takes a
  new value with Enter, a constant one is shown only. Globals that are not exported can't be
  reached from JavaScript, so not from here either.
- **Scan:** a first scan for an exact value, a range, bigger/smaller than, or an unknown initial
  value (a snapshot); then next scans (exact, range, changed, unchanged, increased, decreased,
  increased by, decreased by) narrow the addresses down. Types: `i8 u8 i16 u16 i32 u32 i64 u64
  f32 f64`, little-endian. Floats match within a small tolerance.
- **Watch list:** label, address or pointer path (`0x400 > 8 > 0`: read the 32-bit pointer at
  0x400, add 8, read again, add 0), type, the live value (type a new one and press Enter),
  Freeze, Remove. **Export** and **Import** move the list as JSON (copy, or a file).
- **Hex view:** a 256-byte window of the memory with go-to-address; click a byte and type hex
  digits to change it in place.

**How the memories are reached.** Nothing is added to the page's globals:
- The host asks the engine's DevTools protocol for the objects whose prototype is
  `WebAssembly.Memory.prototype`, and those of `WebAssembly.Instance.prototype`
  (`Runtime.queryObjects`), and calls its probe (`ui/inject/wasm-probe.js`) on them with
  `Runtime.callFunctionOn`. The probe is an object only
  the host holds a handle to (object group `krunk-wasm`, released when the session ends).
- The query makes the engine collect garbage, so it runs when memories are listed and when a
  document is new, not on every read. `memory.grow` replaces a memory's buffer: every operation
  takes `memory.buffer` again, and a scan that paused for the page takes it again before it goes
  on. Shared memories (threads) work the same way.
- **A global's type** is not something JavaScript can ask for: the probe links the global into
  18-byte modules that import one global of a given type (a wrong type fails to link). Nothing
  is written to the global for that. Where the page's content security policy forbids compiling
  WebAssembly, the type is `i64` or `number` and the mutability is unknown.
- **Contexts:** the page; frames of the same process (in practice: same-site frames), each with
  its own memories; dedicated workers, through the tab's auto-attached DevTools sessions. Memory
  numbers, scans and freezes belong to one context (`ctx`, 0 is the page).
- **Fallback:** "Hook WebAssembly and reload the tab" installs a document-start script that
  wraps `WebAssembly.Memory`, `instantiate`, `instantiateStreaming` and `Instance` before any
  page script runs, so memories and instances are caught as they are made. It needs the reload, and it is
  visible to the page: the wrapped functions, and a random non-enumerable property that holds
  the probe until the host takes it with its next call. Both ways feed the same list.

**Limits.** A scan covers at most the first 512 MiB of a memory and keeps at most 4,194,304
addresses (it says so when more matched); 8 scans per context; 1000 results per request (the
page shows 100 at a time, the agent's tool gives at most 200); reads, writes and the hex view
64 KiB at once; 256 frozen values and 256 watch entries; pointer paths of at most 8 offsets
(32-bit pointers: `memory64` modules are not followed); 256 instances per context, 2000 exports
listed per instance (memories, globals, tables and tags first; the rest are counted), export
names cut to 200 characters, a global's new value at most 64 characters. Scans run in slices of 30 ms so the page
keeps running; a scan may take up to two minutes before the host gives up.

**Not reachable:** frames of other sites (they run in another process), service workers and
shared workers. A page **paused in the DevTools debugger** can't answer: requests time out until
it runs again. A frozen value is written back by a 50 ms timer of the page itself: about 20
times a second, slower while the tab is in the background (the engine throttles its timers), and
the game can still see the other value between two writes.

**No anti-detection.** The inspector does not hide: a page can notice the DevTools session, the
hook, timing, or simply that its values changed. Online games check their state on the server;
use it on single-player games and your own apps.

**Sessions** are in memory only, one per inspected tab. They end when the tab closes, or when
its last inspector page closes (scans and freezes are dropped then). A navigation ends the
document's scans and freezes by itself. The inspector works in private tabs (its page opens as a
private tab) and never on Krunk's own pages.

**User scripts and the AI agent.** The same operations are there for user scripts as `GM.wasm`
(`// @grant GM.wasm`; a script with it is tagged **WASM memory** in `krunk://scripts`; without the
grant `GM.wasm` is `undefined`) and for the AI agent as the `wasm_*` tools (`wasm_memories`,
`wasm_read`, `wasm_write`, `wasm_scan`, `wasm_results`, `wasm_freeze`, `wasm_unfreeze`,
`wasm_watch`, `wasm_global`). All three go through the same checked requests and the same probe.
The agent's tools are refused while a password or card field of the page holds a value (what
they read goes to the model's provider); the page and user scripts are not (a user script runs
in the page anyway). The full reference, with every function, its arguments, results, errors, the
limits and a worked example script: [`docs/WASM_INSPECTOR.md`](WASM_INSPECTOR.md). What is
tested and what was shown on Windows: `docs/WASM_INSPECTOR_STATUS.md`.

## Ad and tracker blocking
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

## Privacy
- **Tracking protection levels** (0.6.0, Settings → Privacy → Tracking protection; the
  research and the full table are in `docs/TRACKING_PROTECTION.md`). **Standard** (default):
  the NoCoin cryptominer list, no cookies for third-party requests to hosts the privacy lists
  name, tracking parameters stripped, cross-site requests carry only the origin as `Referer`,
  bounce trackers (sites only ever seen as a redirect hop, never visited) lose their cookies
  and storage daily. **Strict** adds Krunk's fingerprinting list, blocks every third-party
  cookie, sends no `Referer` to other sites (except the page's origin to embedded video players,
  0.28.0: YouTube refuses to play without it), makes social media embeds click-to-load (a
  "Krunk blocked content from …" placeholder with Load / Always allow on this site; the allowed
  sites are listed and removable in Settings → Privacy) and cleans bounce trackers hourly.
  **Custom** picks each part. The shield's site switch turns all of it off for one site.
- **Bot checks are left alone** (0.28.0, `core::challenge`): captcha and anti-bot services
  (Turnstile, reCAPTCHA, hCaptcha, Arkose, DataDome, HUMAN/PerimeterX, …) are never blocked by a
  list, keep their cookies and referrer at every level, and bounce cleanup skips them and the big
  sign-in providers.
- **Looks like Chrome to bot checks** (0.28.0): the default identity is Google Chrome at the
  engine's own version (156.0.8078.80, `core::useragents`, tied to `cef.lock` by a test), Krunk
  changes no native function in the page (CreepJS: 0 lies, 0 % headless / stealth, as Chrome on
  the same machine), the request headers come in Chrome's order, and partitioned (CHIPS) cookies
  are sent back as in Chrome (engine patch `krunk-chips-cookies.patch`). Known remaining
  differences: Krunk's pointer-lock functions and `Notification.requestPermission` are its own
  JavaScript (source visible to a targeted check), and cross-site frames send
  `Sec-Fetch-Storage-Access`. Not verified: an Arkose puzzle. GitHub's DataDome check blocked every
  exit available for testing (the test machine's IP and 10 Tor exits) before the sign-up form,
  for Chrome 158 as well; on the test machine's IP Krunk's slide is now accepted like Chrome's
  (0.27.1 looped). Also not verified for 0.28.0: the in-app update from 0.27.1 (the update
  server's public name did not resolve at release time).
- **Third-party cookies:** a `CefCookieAccessFilter` asks `core::cookies::is_third_party` for
  every cookie a request would send or store (a cookie is third-party when the request's
  registrable domain differs from the tab's top-level site), then the level decides: known
  trackers only (`core::protection::TrackerSet`, the `||host^` rules of EasyPrivacy and uBO's
  privacy list) or all. This applies at once. "All" also blocks script access
  (`document.cookie`) through Chromium's `--test-third-party-cookie-phaseout` switch (needs
  a restart); the profile preference alone did not block it in CEF 156.
- **Tracking parameters** (`utm_*`, `fbclid`, `gclid`, ...) are stripped before navigating.
- **Do Not Track** (on by default) through Chromium's own setting: the engine sends `DNT: 1`
  and `navigator.doNotTrack` is its native getter. **Global Privacy Control** is off by default
  since 0.28.0 (no Chrome sends it, so bot defences use it to single Krunk out); when you turn it
  on, the engine's own implementation sends `Sec-GPC: 1` and exposes
  `navigator.globalPrivacyControl` (takes effect after a restart). Krunk patches neither into the
  page any more.
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
- **Session logs (0.15.0, off by default):** Settings → Privacy & security → Keep session logs.
  While on, Krunk records per session (from start to exit) the main-frame pages visited, how long
  each tab and site was in front (the active tab of the focused window while Krunk is the
  foreground app), and its CPU use every 5 s from the engine's task manager (tabs, browser, GPU
  process, utilities; "GPU" is the GPU process's CPU, Windows gives no GPU-load figure). Events
  are buffered in memory and written every 30 s and at exit. Private tabs are never logged; the
  agent can't read or change the setting. `krunk://sessions` lists sessions (start, end, time,
  top sites) and shows each with a timeline, time per site and per tab and a CPU chart; delete
  one, clear all, keep 1 day to 1 year (30 days by default). ⋮ → **Session usage** opens the
  page (0.16.0); while logging is off it says so and offers to turn it on. While off, no timer
  runs and nothing is buffered or written (idle CPU on the test VM was the same with it off and
  on).

## Engine background traffic
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

## Local network access

Chromium 156 enforces Local Network Access: before a public page may fetch from a private
address (a NAS, a Plex server on the LAN, `*.plex.direct`) or from this computer (localhost),
it asks the embedder for a permission. Without an answer the request waits forever, which is
what happened in 0.4.0. Krunk now answers: a prompt under the toolbar ("Connect to devices on
your local network?" or "Connect to this computer?") with Block and Allow, remembered for the
site when "Remember for this site" is left on. Stored answers are listed and can be removed on
the site's Cookies and site data page (shield panel → Cookies and site data → Permissions).
Private tabs ask every time and remember nothing. Dismissing the prompt blocks once.

## Site permissions

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

## Krunk account (0.8.0)
Settings → Account signs this browser in to a Krunk account (made on
[krunk.kruxsys.com](https://krunk.kruxsys.com/account/register)). A signed-in browser shows who it is
signed in as and appears in the account's device list on the website, where it can be renamed or
removed. Since 0.9.0 the account can also sync your data (below, opt-in). Since 0.11.0 a new
browser's name is "Krunk on Windows 11" (it carried the version at sign-in, which went stale
with the first update); Settings → Account and the website show the version it runs now next to
the name ("Krunk on Windows 11 · version 0.11.0"). Names made before 0.11.0 are shown without
their old version; a name you typed yourself is never changed.

- **Signing in** works like signing in a TV: **Sign in** asks krunk.kruxsys.com for a code, shows it
  and opens `https://krunk.kruxsys.com/link?code=…` in a new tab. There you sign in to the website,
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
- **Sign-in QR and Add a device (0.13.0):** while signing in, the card shows the sign-in link as
  a QR code next to the code, so a phone that is signed in to the account can approve this
  device by scanning it (without one from the server, the code alone, as before). Signed in,
  **Add a device** next to Manage account opens the account page's "Add a device" part in a new
  tab.
- **The account page** (`https://krunk.kruxsys.com/account/`, 0.13.0) also has:
  - **Add a device:** a QR code a phone scans (in its browser, later in the Android app). It
    works once for 5 minutes; you approve after checking that both screens show the same
    6-digit code (`docs/sync/PROTOCOL.md` §6.5).
  - **Usage** against the quotas, and **Activity & IP addresses** (the security events of the
    last 90 days with full IP, country and the device or browser).
  - **Download your data:** one JSON file (`krunk-account-export/1`,
    `docs/sync/export-schema-1.json`) with no keys, only to the browser, never by mail.
  - **Reset sync** and **Delete account** (`/account/delete`: password, two-step code, the
    address typed out; immediate, with a receipt by mail).
- **The server side** (sign-in by code after RFC 8628, rotating tokens whose reuse signs the
  device out and sends a mail) is described in `docs/sync/PROTOCOL.md` §6.1 and §11.3–11.4.

## Sync (beta, 0.9.0)
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
  is the union of visits, each kept with the device it was made on; settings and logins take the
  later change. A login that lost a conflict
  is kept inside the vault for 30 days. The same login saved on two devices becomes one.
- **History per device (0.23.0, Android 0.8.0):** every visit records the device it was made on
  (its sync device id; the name is the one in Sync settings). `krunk://history` and the Android
  History screen open on **This device** and have a chip per other device by name, **Unknown
  device** and **All**; search works inside the chosen one. Deleting follows the chip: clearing
  This device removes its visits here and on the other devices; another device's visits are only
  hidden here (they stay on that device); **All** (and Clear browsing data) clears every device.
  Address-bar suggestions still use every device's history. The device rides inside the
  encrypted history record (no server change); 0.22.x and Android 0.7.x devices keep syncing, and
  their new visits are named by their device. Visits synced before the upgrade show as
  **Unknown device** (on a profile that never synced history they are This device's); nothing is
  lost.
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

## Password vault
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

### Recovery phrase (0.7.0)
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

## Custom DNS

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

## VPN (0.14.0)

Settings → Privacy & security → VPN imports WireGuard (`.conf`) and OpenVPN (`.ovpn`) files from a
VPN provider. **Only Krunk's connections use it**: normal and private windows, and Krunk's own
requests (updates, sync, the account, filter lists, extensions, downloads). Nothing is installed
in Windows: no network adapter, no routes, no driver, no administrator rights, and other programs
are not affected. "Show the VPN button in the toolbar" (off by default) adds a button before the
shield whose popup shows the state, the profile, the tunnel address and the traffic, with
Connect and Disconnect. "Connect when Krunk starts" connects before any page loads.
The popup's scope choice (0.22.0) is **Enable everywhere** (the default: every tab, private ones
too, and Krunk's own requests) or **Private tabs only** (only the private profile uses the VPN;
normal tabs, extensions, updates and sync go direct). It applies at once and is remembered.

How it works (`vpn/`, `app/src/browser/vpn.rs`):
- The tunnel runs inside Krunk's process, in user space: WireGuard through boringtun's protocol
  code over a UDP socket, OpenVPN through OpenVPN 3 Core (the library behind OpenVPN Connect,
  vendored in `vendor/` with its external-TUN interface). The IP packets go to a TCP/IP stack in
  Krunk (smoltcp), not to Windows.
- In front of it is an HTTP proxy on `127.0.0.1` (a random port). While the VPN runs, the engine's
  request contexts (normal and private) use it as their proxy, set at runtime, and Krunk's own
  WinHTTP requests go through it too. Names are resolved inside the tunnel by the VPN's DNS
  servers (1.1.1.1 through the tunnel when the profile names none); Krunk's custom-DNS stub and
  the engine's secure DNS are paused meanwhile. WebRTC may only use UDP through the proxy, which
  means no UDP: calls fall back to TCP relays where the site offers them. At Connect, Disconnect
  and a switch the engine closes its idle and HTTP/2/QUIC connections and Krunk restarts its own
  requests, so they continue on the new route; a WebSocket or long download already open over
  HTTP/1.1 keeps its old route until it ends.
- Only Krunk may use the proxy: a connection is accepted when the program on the other end is
  Krunk itself or one of its engine processes (Windows' TCP table and process list), and the
  port can't be shared.
- **Fail-closed:** while the VPN is on but not connected (connecting, reconnecting, the server
  down), every connection gets "VPN not connected" and nothing goes direct. Addresses outside the
  profile's routes (a split profile, e.g. only a company network) are refused, not sent direct.
  Disconnecting returns to direct connections.
- Profiles are stored in `browser.db` (`vpn_profiles`) encrypted with Windows DPAPI for the
  user; they never sync and never reach a page. Scripts in profiles (`PostUp`, `up`, `down` …)
  never run and are reported at import. OpenVPN's `remote-cert-tls server` is enforced as the
  equivalent `remote-cert-eku` check (OpenVPN 3 Core 3.11.7 doesn't apply the former). A refused
  username/password or server certificate stops the VPN with that message instead of retrying.
- **Reach the local network directly** (0.14.2, off by default): private addresses (10/8,
  172.16/12, 192.168/16, link-local, IPv6 unique- and link-local) and names without a dot or
  ending in `.local`, `.lan`, `.home.arpa` or `.internal` skip the VPN, so routers, printers and
  home servers stay reachable. They are added to the engine's proxy bypass list, so they go out
  directly as without the VPN; a name is matched by its suffix, not by the address it resolves to.
- Not supported: a manual proxy at the same time (Settings refuses it), UDP through the VPN (QUIC
  falls back to TCP), OpenVPN profiles with a client certificate in a separate file (inline
  `<cert>`/`<key>` work), and the proxy-related Chromium switches in Advanced.

## Stuck page loads (0.18.0)

Two things in the engine's network stack let one hung request hold up others, in any tab:
a request for an address that another request is still fetching waits for that one's answer,
up to 20 s (the HTTP cache), and a host gets at most six connections, so six requests that
never finish (or six open streams) block everything else for that host. Both were measured;
the table, the mechanisms and what was not measured (Krunk itself on Windows) are in
[docs/NETWORK.md](NETWORK.md).

- **Around the cache:** a request for an address that another tab has been waiting on for a
  second is sent with `Cache-Control: no-cache` (as a hard reload), so it does not queue
  behind the other one.
- **Sixteen connections per host** instead of six, with the next engine build
  (`chromium-sockets-per-host.patch`). It gives room; it does not cure a site that never
  answers.
- **krunk://tasks** shows each renderer's **Pending** requests and the age of the oldest
  (`6 (45 s)`); the tab with many old ones is the one to close.
- **krunk.log** gets a line for requests pending over 30 s: the tab, the host (never the
  address or its query; nothing for a private tab), the kind and the number. Rate-limited.

## Request headers

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

## Generated passwords

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

The AI agent's `fill_password` (0.16.0, only when you ask it for a password) uses the same
generator and the same save flow; see "AI agent, userscripts and macros".

## Payment methods

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

## Chromium switches
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
- `--enable-features=NoStatePrefetchHoldback` (0.26.0): a page's `<link rel=prerender>` fetches
  nothing. The engine ran those prefetches in a hidden page with no Krunk tab, so their requests
  bypassed the ad blocker and tracking protection.

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

## Updates
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
  **`https://krunk.kruxsys.com`**, Krunk's website and update server on KruxSYS's permanent
  domain (since 0.28.1; earlier builds used krunk.209.wf, which no longer answers, so they must
  be updated by hand from the website). A build always checks the server it was built with; a
  build for another server sets `UPDATE_BASE_URL`. Krunk 0.2.0 builds pointed at the placeholder
  `https://krunk.example` and can't update themselves; reinstall them from the website. The
  server itself (Docker, Caddy, behind Cloudflare) is in `dist/server/`; see its README.
- **Release notes:** `CHANGELOG.md` is their one source. The website's
  [/changelog](https://krunk.kruxsys.com/changelog) page is generated from it when the site is built,
  and `publish-release.sh` takes a release's `NOTES.md` (what **Release notes** shows) and date
  from its section.

## Memory
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
