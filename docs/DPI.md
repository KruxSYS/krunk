# Display scaling (DPI) in Krunk for Windows

Krunk is per-monitor DPI aware (PerMonitorV2, set in `app/src/ffi/process.rs` and declared in
`app/res/Krunk.exe.manifest`): every window is drawn at the scale of the monitor it is on, and
Windows never stretches it. This page says what went wrong at scales above 100 % before
0.15.4, how it was found, and the rules for new code.

## Units

| Unit | Where | Notes |
|---|---|---|
| **DIP** (1/96 inch) | every layout constant in `core/src/layout.rs` and `core/src/frame.rs` (`*_DIP`), the shell's and the popups' CSS pixels | a CSS pixel of Krunk's own UI is one DIP (their zoom is always 1) |
| **Physical pixel** | window and view rectangles, mouse positions, monitor work areas, everything Win32 takes or gives | `Rect`, `ScreenRect`, `Bounds` |
| **Scale** | `Window::scale()` = `GetDpiForWindow / 96` | 1.0, 1.25, 1.5, 1.75, 2.0, 2.5 … |

`core/src/dpi.rs` (`Scale`) is the one place that converts: `px` rounds to the nearest pixel,
`px_floor` and `px_ceil` are for the cases that want another rounding on purpose, `dip` goes
back, `rescale` carries a pixel length from one scale to another.

## What was wrong (0.15.3 and before)

Found with `scripts/vm-dpi-run.sh` on the test VM (1280x800 at 100 and 125 %, 1920x1080 at 150
and 175 %, 2560x1600 at 150 %, 3840x2160 at 200 and 250 %, and scale changes while Krunk
runs). Most of the host already scaled correctly: the shell strip, the page bounds, the hit
tests of the caption buttons and the popups' sizes were right at every scale when Krunk was
started at that scale. These were not:

1. **A scale change while Krunk is maximized left the old layout.** Windows first gives a
   maximized window its new size (the taskbar grows with the scale), at a moment when the
   window still reports its old DPI, so the frame was laid out for the old scale. The DPI
   message (`WM_DPICHANGED`) follows with a suggested rectangle, which for a maximized window
   is the one it already has; Krunk only applied that rectangle, nothing changed size, and
   nothing laid the frame out again. Result: the tab strip and toolbar kept their height in
   pixels while their content was drawn at the new scale. Going from 100 % to 150 % the
   bookmarks bar was cut off; going back, a blank band stayed under it. It lasted until the
   window was resized or restored. The same happens when a maximized window ends up on
   another monitor with the same resolution and another scale (a laptop and its dock).
   *Fix:* the frame handles the DPI change itself (`Browser::frame_dpi_changed`,
   `app/src/browser/frame.rs`): it lays out again whatever the size, tells every engine view
   of the frame, closes what was placed for the old scale (popups, the extension popup, the
   hover card, the status bubble), repaints its own parts and takes icons of the new size.
   Each frame also remembers the DPI it was laid out for and checks it on every resize and
   display change (`check_frame_scales`), so a missing or early message cannot leave a stale
   layout.

2. **Popups were clamped to the work area and cut off.** A popup window was placed with an
   upper bound for its height and pushed into the monitor's work area; content that did not
   fit was clipped (`overflow: hidden`). The ⋮ menu needs 658 CSS px. A 1920x1080 display has
   672 px of work area at 150 %, 569 px at 175 %, and the VM's 1280x800 has 592 px at 125 %:
   there the menu jumped to the top of the screen, over the tab strip, and its last items
   (Settings, About Krunk, Exit) were below the screen's edge with no way to reach them.
   *Fix:* a popup is placed again from its anchor once it has reported its content's real
   height (`layout::place_popup`): under its control or at the click when it fits there; a
   context menu turns upwards when there is no room below the click; anything else moves up
   only as far as it must. The popup learns the work area's height before it renders
   (`Event::PopupRoom`): content higher than that gets compact rows (24 px instead of 30),
   and scrolls when that is still not enough (`ui/shell/popup.js` `squeeze`).

3. **The default window size was in physical pixels.** A window without a saved place was
   1280x860 pixels: 640x430 DIP at 200 %, 512x344 DIP at 250 % (just above the minimum size,
   with a cramped toolbar), and higher than the work area on small displays (the VM's 800 px
   high display at 100 %, a 1080p display at 175 %), so its bottom edge was under the taskbar.
   *Fix:* `layout::default_window`: 1280x860 DIP at the monitor's scale, never larger than
   its work area, moved inside it.

4. **Saved window bounds carried no DPI.** A session saved at one scale and restored at
   another gave a window of the same pixel size: too small or too large by the ratio of the
   scales. *Fix:* the session stores the DPI with each window's bounds (`WindowSession.dpi`,
   absent in older sessions), and `layout::restore_window` keeps the size in DIP, limits it to
   the work area and keeps it reachable. The window's place is also remembered in every
   start-up mode now, not only with "Continue where you left off" (a new-tab or home-page
   start always got the default size).

5. **Popup windows took Windows' suggested rectangle on a DPI change.** Popups and the hidden
   application window shared the frames' `WM_DPICHANGED` handler and were resized by a
   rectangle that knows nothing of their content. *Fix:* only frames take the suggested
   rectangle; a popup is sized by its owner each time it is shown.

6. **Smaller things:** the minimum window size and a few positions were truncated instead of
   rounded (one pixel off at 125 % and 175 %); the window icons were loaded once for the
   system DPI (blurred in the taskbar on a monitor with another scale); in a window at its
   minimum width the ⋮ button was pushed partly out of the toolbar (the address field could
   not shrink below its content): the address field now gives way first, then the optional
   buttons go (`shell.js` `fitToolbar`).

## Rules for new UI code

- Sizes and positions are constants in DIP; convert at the moment of placing, with the scale
  of the window the thing belongs to (`Window::scale()`), through `core::dpi::Scale`. Never
  cache a scale, never use the system DPI (`GetSystemMetrics`, `GetDeviceCaps`): use the
  `…ForDpi` functions with the window's DPI.
- Edges that must meet are positions rounded once and subtracted (`layout::split`,
  `panel_split`), not rounded sizes added up.
- Anything that stays on screen across a scale change must be re-placed or closed in
  `Browser::frame_dpi_changed`.
- A popup's size comes from its content: let it report its height (`FITS` in `popup.js`) and
  let `layout::place_popup` decide where it goes. Do not assume the work area is higher than
  the content: 448 CSS px (a 1280x720 display at 150 %) is a real work area.
- Saved geometry needs the DPI it was saved at.
- Test at 125 % and 175 % (fractional scales show rounding mistakes) and at a scale change
  with the window maximized.

## Checking

- Unit tests: `core::dpi`, `core::layout` (`place_popup`, `default_window`, `restore_window`,
  the splits at several scales), `core::frame` hit tests.
- UI tests without Windows: `ONLY=DpiPopup,DpiShell node ui/dev/test-ui.cjs` (popups on low
  work areas at device scales 1 to 2.5, the shell from 1280 down to 480 CSS px).
- On the VM: `scripts/vm-dpi-run.sh OUTDIR 1280x800:100 1920x1080:150 … 1920x1080:150:rt`
  switches the display mode and scale (`scripts/vm-display.ps1`, always restored to
  1280x800 at 100 %), runs `scripts/vm-dpi-shots.ps1` and collects, per mode: screenshots of
  about forty surfaces and, per window state, the window, client and child rectangles, the
  hit-test codes at fixed DIP points, each popup's rectangle against the work area, and what
  a page sees (`devicePixelRatio`, viewport), read from the title of `dpi.html`. `:rt` is the
  pass that changes the scale while Krunk runs (normal and maximized) and restarts at another
  scale.

## Not verified

- A window dragged between two monitors with different scales: the VM has one monitor. It is
  the same code path as a scale change (`WM_DPICHANGED`, then the same handler), which is
  tested, and the per-monitor placement logic has unit tests with two monitors.
- Scales above 250 %.
