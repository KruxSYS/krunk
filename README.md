# LightBrowser

A lightweight Windows 10/11 (x64) browser: a Rust host around the Microsoft Edge WebView2
(Evergreen, Chromium) runtime. *Work in progress; full build and usage instructions arrive with
the packaging milestone.*

## Architecture

```
core/   platform-independent library (lb_core): storage, history, bookmarks, frecency,
        vault crypto, ad blocking, URL cleaning, settings, tab model, shortcuts,
        message protocol, origin checks, layout math. Tested on Linux.
app/    Windows host (LightBrowser.exe). All unsafe code is in app/src/ffi.
ui/     the browser's own UI (HTML/CSS/JS), embedded into the exe at build time.
        ui/dev holds test tooling only (mock host, Playwright tests) and is not embedded.
```

### Windows and WebViews
- **Main window:** one top-level window with one shared `CoreWebView2Environment`.
- **Shell:** a WebView strip at the top showing the tab strip and toolbar. Its height is fixed:
  76 DIP, scaled by DPI.
- **Tabs:** one content controller per *live* tab, laid out below the shell. Restored tabs stay
  unloaded until they are activated. Private tabs use a separate InPrivate profile.
- **Popup:** one extra WebView in an owned `WS_POPUP` tool window. It shows the main menu, the
  shield panel and the save-password and fill-login prompts.
  - It is created on first use and then hidden and reused, never more than one.
  - It uses the shared environment and the default (shell) profile, never a private one.
  - The shell sends the popup's rectangle in CSS pixels. The host converts it to screen pixels
    with the window's DPI (`GetDpiForWindow` + `ClientToScreen`) and clamps it to the monitor
    work area (`core::layout::popup_screen_rect`).
  - Every popup kind has buttons, so the popup is always activated when shown. That keeps it
    keyboard accessible and gives a reliable "click elsewhere" signal (`WM_ACTIVATE`). The window
    is created `WS_EX_NOACTIVATE`, and that flag is cleared by `Window::set_activatable` only
    for kinds that take focus.
  - **Hide rules:** the popup hides on deactivate, Escape, a click elsewhere (another
    controller gets focus), main-window move, resize or minimize, and tab switch or close. A
    prompt hidden this way counts as dismissed, never confirmed.
  - This replaces a transparent full-window overlay: Win32 does not composite sibling child
    windows with transparency.

### Internal origins
| Origin | Serves | Who can send privileged messages |
|--------|--------|----------------------------------|
| `https://shell.lightbrowser.invalid` | `ui/shell/*` (`shell.html`, `popup.html`) + `/shared/*` | the shell WebView (only `/shell.html`) and the popup WebView (only `/popup.html`) |
| `https://lightbrowser.invalid` | `ui/pages/*` + `/shared/*` | a tab showing a known built-in page; each page has its own command allow-list |

- Requests to these origins are answered from embedded bytes through `WebResourceRequested`.
  The `.invalid` TLD never resolves, so they can't leak to the network.
- Every response carries a strict CSP (`default-src 'none'`, no inline code, no network,
  `frame-ancestors 'none'`).
- Message origins are checked by `core::protocol::classify`: exact origin, exact document,
  top-level frame only. Web pages can only send the small `ContentMessage` type.

## Development
- Core tests: `cargo test -p core`
- Windows build: `cargo build --release --target x86_64-pc-windows-gnu`
- UI tests (mock host, no exe):
  `NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-ui.cjs [shots-dir]`
