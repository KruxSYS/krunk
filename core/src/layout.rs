//! Window layout math (pure, so it is testable without Windows).

use crate::dpi::Scale;
use crate::protocol::{PopupKind, PopupRect};
use crate::settings::DevToolsDock;

/// Height of the shell strip (tab row 36 + toolbar 40) in DIPs.
pub const SHELL_HEIGHT_DIP: f64 = 76.0;
/// Extra height when the bookmarks bar is shown.
pub const BOOKMARKS_BAR_DIP: f64 = 28.0;

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Shell strip height in physical pixels for a DPI scale.
pub fn shell_height(scale: f64, bookmarks_bar: bool) -> i32 {
    let dip = SHELL_HEIGHT_DIP + if bookmarks_bar { BOOKMARKS_BAR_DIP } else { 0.0 };
    (dip * sane_scale(scale)).round() as i32
}

fn sane_scale(scale: f64) -> f64 {
    Scale::new(scale).factor()
}

/// A point this far above or below the bookmarks bar still counts as over it (a drag
/// from a folder dropdown lands on the bar without pixel precision).
pub const BAR_SLACK_DIP: f64 = 6.0;

/// Is `(x, y)` (CSS px of the shell viewport, `width` CSS px wide) over the bookmarks
/// bar? Only when the bar is shown in that window.
pub fn over_bookmarks_bar(bar_shown: bool, x: f64, y: f64, width: f64) -> bool {
    bar_shown
        && (0.0..=width).contains(&x)
        && (SHELL_HEIGHT_DIP - BAR_SLACK_DIP..=SHELL_HEIGHT_DIP + BOOKMARKS_BAR_DIP + BAR_SLACK_DIP).contains(&y)
}

/// Shell and content bounds for a client area of `width` x `height`.
pub fn split(width: i32, height: i32, scale: f64, bookmarks_bar: bool) -> (Rect, Rect) {
    let width = width.max(0);
    let height = height.max(0);
    let sh = shell_height(scale, bookmarks_bar).min(height);
    (Rect { x: 0, y: 0, width, height: sh }, Rect { x: 0, y: sh, width, height: height - sh })
}

/// Converts a popup rect in CSS pixels (relative to the shell viewport,
/// whose top-left is at `origin` on screen) into screen pixels, clamped to
/// the monitor work area.
pub fn popup_screen_rect(css: PopupRect, scale: f64, origin: (i32, i32), work: Rect) -> Rect {
    let s = sane_scale(scale);
    let mut w = (css.width * s).round() as i32;
    let mut h = (css.height * s).round() as i32;
    w = w.clamp(1, work.width.max(1));
    h = h.clamp(1, work.height.max(1));
    let mut x = origin.0 + (css.x * s).round() as i32;
    let mut y = origin.1 + (css.y * s).round() as i32;
    x = x.clamp(work.x, work.x + work.width - w);
    y = y.clamp(work.y, work.y + work.height - h);
    Rect { x, y, width: w, height: h }
}

/// What a popup hangs on, which decides where it goes when the room under its anchor is too
/// small for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupAnchor {
    /// Under a control, all of it to be seen (the ⋮ menu, a prompt under the toolbar, a
    /// dialog): it moves up just as far as it must.
    Drop,
    /// At a point (a context menu at the click): it opens above the point when there is no
    /// room below it, as Windows' own menus do.
    Point,
    /// A list under the control it belongs to (a bookmark folder, the address suggestions,
    /// the tab list): it stays there, as high as the room below allows, and scrolls.
    Keep,
}

/// Where a popup goes, and whether its content has to scroll there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupPlace {
    pub rect: Rect,
    /// The work area is lower than the content: the popup takes the whole height and scrolls.
    pub scrolls: bool,
}

/// Places a popup whose content is `content_height` CSS px high (measured by the popup; `css`
/// carries where the shell or the host asked for it and its width). Like
/// [`popup_screen_rect`] it never leaves the monitor's work area, but it keeps the anchor in
/// mind (0.15.4): at 150 % and more, or on a small display, a menu is often taller than the
/// room under its button.
pub fn place_popup(
    css: PopupRect,
    content_height: f64,
    anchor: PopupAnchor,
    scale: f64,
    origin: (i32, i32),
    work: Rect,
) -> PopupPlace {
    let s = Scale::new(scale);
    let work_w = work.width.max(1);
    let work_h = work.height.max(1);
    let want = s.px(content_height.clamp(1.0, PopupRect::MAX)).max(1);
    let bottom = work.y + work_h;
    if anchor == PopupAnchor::Keep {
        // Where the rectangle asked for goes (its height is the caller's upper bound, already
        // cut to the room below where that matters), and no higher than the room from there.
        let r = popup_screen_rect(css, scale, origin, work);
        let h = want.min((bottom - r.y).max(1));
        return PopupPlace { rect: Rect { height: h, ..r }, scrolls: h < want };
    }
    let w = s.px(css.width).clamp(1, work_w);
    let x = (origin.0 + s.px(css.x)).clamp(work.x, work.x + work_w - w);
    let top = (origin.1 + s.px(css.y)).clamp(work.y, bottom - 1);
    let (y, h) = if want <= bottom - top {
        (top, want)
    } else if anchor == PopupAnchor::Point && want <= top - work.y {
        (top - want, want)
    } else if want <= work_h {
        (bottom - want, want)
    } else {
        (work.y, work_h)
    };
    PopupPlace { rect: Rect { x, y, width: w, height: h }, scrolls: h < want }
}

/// The height (CSS px) a popup at `css` can have without leaving its anchor: the room below
/// it; for a context menu the larger of the room below and above the click. Content higher
/// than this is made compact by the popup before it is placed. 0 for a list that stays under
/// its control and scrolls anyway.
pub fn popup_room(css: PopupRect, anchor: PopupAnchor, scale: f64, origin: (i32, i32), work: Rect) -> f64 {
    let s = Scale::new(scale);
    let bottom = work.y + work.height.max(1);
    let top = (origin.1 + s.px(css.y)).clamp(work.y, bottom - 1);
    let px = match anchor {
        PopupAnchor::Drop => bottom - top,
        PopupAnchor::Point => (bottom - top).max(top - work.y),
        PopupAnchor::Keep => 0,
    };
    s.dip(px)
}

/// Where host-initiated prompts (save/fill password) appear: under the
/// right end of the toolbar, in CSS pixels of the shell viewport.
/// The bar between a page and its docked DevTools (DIP).
pub const DEVTOOLS_SPLITTER_DIP: f64 = 5.0;
/// Neither side gets smaller than this while there is room for both (DIP).
pub const PAGE_MIN_DIP: f64 = 240.0;
pub const DEVTOOLS_MIN_RIGHT_DIP: f64 = 280.0;
pub const DEVTOOLS_MIN_BOTTOM_DIP: f64 = 160.0;

/// The strip above docked DevTools with its dock-side and close buttons (DIP).
pub const DEVTOOLS_STRIP_DIP: f64 = 24.0;
/// The strip's buttons: square, this big, this far from the strip's right end and each other.
const STRIP_BUTTON_DIP: f64 = 20.0;
const STRIP_GAP_DIP: f64 = 4.0;

/// A tab's content area split between its page and its docked DevTools (physical pixels).
/// The strip sits at the top of the DevTools side, above the DevTools view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevToolsSplit {
    pub page: Rect,
    pub splitter: Rect,
    pub strip: Rect,
    pub devtools: Rect,
}

/// A button of the DevTools strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripButton {
    /// Dock DevTools on the other side.
    Dock,
    /// Close DevTools.
    Close,
}

/// The strip's button rects (physical pixels), right-aligned: [dock, close].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripLayout {
    pub strip: Rect,
    pub dock: Rect,
    pub close: Rect,
}

/// Where the strip's buttons are, for painting and hit-testing.
pub fn devtools_strip(split: &DevToolsSplit, scale: f64) -> StripLayout {
    let s = sane_scale(scale);
    let size = (STRIP_BUTTON_DIP * s).round() as i32;
    let gap = (STRIP_GAP_DIP * s).round() as i32;
    let strip = split.strip;
    let y = strip.y + (strip.height - size) / 2;
    let close = Rect { x: strip.x + strip.width - gap - size, y, width: size, height: size };
    let dock = Rect { x: close.x - gap - size, y, width: size, height: size };
    StripLayout { strip, dock, close }
}

/// Which strip button is at the client point, if any.
pub fn strip_hit(layout: &StripLayout, x: i32, y: i32) -> Option<StripButton> {
    let inside = |r: &Rect| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
    if inside(&layout.close) {
        Some(StripButton::Close)
    } else if inside(&layout.dock) {
        Some(StripButton::Dock)
    } else {
        None
    }
}

/// Is the client point on the strip (buttons included)?
pub fn on_strip(split: &DevToolsSplit, x: i32, y: i32) -> bool {
    let r = split.strip;
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// The DevTools size (DIP) that fits `content`: at least its minimum and leaving the page
/// at least [`PAGE_MIN_DIP`]; when both minimums don't fit, they share the space in proportion.
pub fn clamp_devtools_size(content: Rect, dock: DevToolsDock, size_dip: f64, scale: f64) -> f64 {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let total = match dock {
        DevToolsDock::Right => f64::from(content.width),
        DevToolsDock::Bottom => f64::from(content.height),
    } / scale
        - DEVTOOLS_SPLITTER_DIP;
    let min_dt = match dock {
        DevToolsDock::Right => DEVTOOLS_MIN_RIGHT_DIP,
        DevToolsDock::Bottom => DEVTOOLS_MIN_BOTTOM_DIP,
    };
    let total = total.max(0.0);
    if total < min_dt + PAGE_MIN_DIP {
        return total * min_dt / (min_dt + PAGE_MIN_DIP);
    }
    let size = if size_dip.is_finite() { size_dip } else { min_dt };
    size.clamp(min_dt, total - PAGE_MIN_DIP)
}

/// Splits `content` into page, splitter and DevTools, DevTools on the `dock` side.
pub fn devtools_split(content: Rect, dock: DevToolsDock, size_dip: f64, scale: f64) -> DevToolsSplit {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let dt = (clamp_devtools_size(content, dock, size_dip, scale) * scale).round() as i32;
    let bar = (DEVTOOLS_SPLITTER_DIP * scale).round() as i32;
    let strip_h = (DEVTOOLS_STRIP_DIP * scale).round() as i32;
    match dock {
        DevToolsDock::Right => {
            let bar = bar.min(content.width);
            let dt = dt.min(content.width - bar).max(0);
            let page_w = (content.width - bar - dt).max(0);
            let strip_h = strip_h.min(content.height);
            let x = content.x + page_w + bar;
            DevToolsSplit {
                page: Rect { x: content.x, y: content.y, width: page_w, height: content.height },
                splitter: Rect { x: content.x + page_w, y: content.y, width: bar, height: content.height },
                strip: Rect { x, y: content.y, width: dt, height: strip_h },
                devtools: Rect { x, y: content.y + strip_h, width: dt, height: content.height - strip_h },
            }
        }
        DevToolsDock::Bottom => {
            let bar = bar.min(content.height);
            let dt = dt.min(content.height - bar).max(0);
            let page_h = (content.height - bar - dt).max(0);
            let strip_h = strip_h.min(dt);
            let y = content.y + page_h + bar;
            DevToolsSplit {
                page: Rect { x: content.x, y: content.y, width: content.width, height: page_h },
                splitter: Rect { x: content.x, y: content.y + page_h, width: content.width, height: bar },
                strip: Rect { x: content.x, y, width: content.width, height: strip_h },
                devtools: Rect { x: content.x, y: y + strip_h, width: content.width, height: dt - strip_h },
            }
        }
    }
}

/// The DevTools size (DIP) for a splitter dragged to `(x, y)` (physical client pixels).
pub fn devtools_size_at(content: Rect, dock: DevToolsDock, x: i32, y: i32, scale: f64) -> f64 {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let px = match dock {
        DevToolsDock::Right => content.x + content.width - x,
        DevToolsDock::Bottom => content.y + content.height - y,
    };
    let dip = f64::from(px) / scale - DEVTOOLS_SPLITTER_DIP / 2.0;
    clamp_devtools_size(content, dock, dip, scale)
}

/// The agent side panel (0.12.0): docked right of the tab area, like DevTools docked right.
pub const PANEL_SPLITTER_DIP: f64 = 5.0;
pub const PANEL_MIN_DIP: f64 = 280.0;
pub const PANEL_MAX_DIP: f64 = 900.0;
/// The page keeps at least this much next to the panel while there is room for both (DIP).
pub const PANEL_PAGE_MIN_DIP: f64 = 320.0;
pub const PANEL_DEFAULT_DIP: f64 = 380.0;

/// A window's content area split between the tab (page plus any docked DevTools), the
/// splitter and the agent panel (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSplit {
    pub page: Rect,
    pub splitter: Rect,
    pub panel: Rect,
}

/// The panel width (DIP) that fits `content`: within its range and leaving the page
/// [`PANEL_PAGE_MIN_DIP`]; when both minimums don't fit, they share the space in proportion.
pub fn clamp_panel_width(content: Rect, width_dip: f64, scale: f64) -> f64 {
    let s = sane_scale(scale);
    let total = (f64::from(content.width) / s - PANEL_SPLITTER_DIP).max(0.0);
    if total < PANEL_MIN_DIP + PANEL_PAGE_MIN_DIP {
        return total * PANEL_MIN_DIP / (PANEL_MIN_DIP + PANEL_PAGE_MIN_DIP);
    }
    let w = if width_dip.is_finite() { width_dip } else { PANEL_DEFAULT_DIP };
    w.clamp(PANEL_MIN_DIP, PANEL_MAX_DIP.min(total - PANEL_PAGE_MIN_DIP).max(PANEL_MIN_DIP))
}

/// Splits `content` into page, splitter and panel (panel on the right).
pub fn panel_split(content: Rect, width_dip: f64, scale: f64) -> PanelSplit {
    let s = sane_scale(scale);
    let bar = ((PANEL_SPLITTER_DIP * s).round() as i32).min(content.width.max(0));
    let w = ((clamp_panel_width(content, width_dip, s) * s).round() as i32).min(content.width - bar).max(0);
    let page_w = (content.width - bar - w).max(0);
    PanelSplit {
        page: Rect { x: content.x, y: content.y, width: page_w, height: content.height },
        splitter: Rect { x: content.x + page_w, y: content.y, width: bar, height: content.height },
        panel: Rect { x: content.x + page_w + bar, y: content.y, width: w, height: content.height },
    }
}

/// The panel width (DIP) for its splitter dragged to client x `x` (physical pixels).
pub fn panel_width_at(content: Rect, x: i32, scale: f64) -> f64 {
    let s = sane_scale(scale);
    let px = content.x + content.width - x;
    clamp_panel_width(content, f64::from(px) / s - PANEL_SPLITTER_DIP / 2.0, s)
}

/// Keeps a restored window reachable: if not even the top 40 px of `rect` overlap any
/// monitor's work area by 100 px (a monitor was unplugged, or the resolution got smaller), it
/// moves onto the monitor it overlaps most (else the first one, the primary), shrunk to fit.
pub fn fit_to_monitors(rect: Rect, work_areas: &[Rect]) -> Rect {
    let overlap = |a: &Rect, b: &Rect| {
        let w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
        let h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
        (w.max(0), h.max(0))
    };
    let title = Rect { x: rect.x, y: rect.y, width: rect.width, height: 40.min(rect.height) };
    let reachable = work_areas.iter().any(|a| {
        let (w, h) = overlap(&title, a);
        w >= 100.min(rect.width) && h >= 20.min(title.height)
    });
    if reachable || work_areas.is_empty() {
        return rect;
    }
    let target = work_areas
        .iter()
        .max_by_key(|a| {
            let (w, h) = overlap(&rect, a);
            i64::from(w) * i64::from(h)
        })
        .filter(|a| {
            let (w, h) = overlap(&rect, a);
            w > 0 && h > 0
        })
        .unwrap_or(&work_areas[0]);
    let width = rect.width.min(target.width).max(1);
    let height = rect.height.min(target.height).max(1);
    let x = rect.x.clamp(target.x, target.x + target.width - width);
    let y = rect.y.clamp(target.y, target.y + target.height - height);
    Rect { x, y, width, height }
}

/// The size of a browser window that has no saved place (DIP; 0.15.4: it was 1280 x 860
/// physical pixels, a quarter of that area at 200 %).
pub const DEFAULT_WINDOW_DIP: (f64, f64) = (1280.0, 860.0);

/// A new window without a saved place: Windows put it at `chosen` (its own cascade). It gets
/// the default size at its monitor's `scale`, never larger than the work area, and lies
/// inside it.
pub fn default_window(chosen: Rect, scale: f64, work: Rect) -> Rect {
    let s = Scale::new(scale);
    let width = s.px(DEFAULT_WINDOW_DIP.0).min(work.width).max(1);
    let height = s.px(DEFAULT_WINDOW_DIP.1).min(work.height).max(1);
    Rect {
        x: chosen.x.clamp(work.x, work.x + work.width - width),
        y: chosen.y.clamp(work.y, work.y + work.height - height),
        width,
        height,
    }
}

/// A monitor: its work area (screen pixels) and its DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub work: Rect,
    pub dpi: u32,
}

/// Where a saved window goes now (0.15.4). `saved_dpi` is the DPI of its monitor when it was
/// saved (`None`: saved by 0.15.3 or older, taken as unchanged). On a monitor whose scale
/// changed since, it keeps its size in DIP, as Windows does for a running window; it is never
/// larger than the work area it is on, and stays reachable ([`fit_to_monitors`]).
pub fn restore_window(saved: Rect, saved_dpi: Option<u32>, monitors: &[Monitor]) -> Rect {
    let works: Vec<Rect> = monitors.iter().map(|m| m.work).collect();
    let on = |r: &Rect| {
        monitors.iter().max_by_key(|m| {
            let w = (r.x + r.width).min(m.work.x + m.work.width) - r.x.max(m.work.x);
            let h = (r.y + r.height).min(m.work.y + m.work.height) - r.y.max(m.work.y);
            i64::from(w.max(0)) * i64::from(h.max(0))
        })
    };
    let mut r = saved;
    let mut rescaled = false;
    if let (Some(from), Some(m)) = (saved_dpi, on(&saved)) {
        if from != m.dpi && from >= 48 && m.dpi >= 48 {
            let (to, from) = (Scale::from_dpi(m.dpi), Scale::from_dpi(from));
            r.width = to.rescale(saved.width, from).max(1);
            r.height = to.rescale(saved.height, from).max(1);
            rescaled = true;
        }
    }
    let mut r = fit_to_monitors(r, &works);
    if let Some(m) = on(&r) {
        let too_big = r.width > m.work.width || r.height > m.work.height;
        r.width = r.width.min(m.work.width).max(1);
        r.height = r.height.min(m.work.height).max(1);
        if rescaled || too_big {
            r.x = r.x.clamp(m.work.x, m.work.x + m.work.width - r.width);
            r.y = r.y.clamp(m.work.y, m.work.y + m.work.height - r.height);
        }
    }
    r
}

/// The tab context menu (CSS px): seven items (30 px each) and two separators (11 px), plus
/// padding and border.
pub const TAB_MENU_SIZE: (f64, f64) = (240.0, 256.0);

/// Where the tab context menu opens: at the click (CSS px of the shell).
pub fn tab_menu_rect(x: f64, y: f64) -> PopupRect {
    PopupRect { x, y, width: TAB_MENU_SIZE.0, height: TAB_MENU_SIZE.1 }
}

/// The shield panel at `anchor` (the rect the shell asked for, under the shield button):
/// with `rows` blocked addresses listed it is wider and taller, its right edge staying put.
pub fn shield_rect(anchor: PopupRect, rows: usize) -> PopupRect {
    if rows == 0 {
        return anchor;
    }
    let width = 360.0_f64.max(anchor.width);
    let height = anchor.height + 12.0 + 24.0 * rows.min(6) as f64 + 32.0;
    PopupRect { x: (anchor.x + anchor.width - width).max(0.0), y: anchor.y, width, height }
}

/// The bookmarks bar's context menu (CSS px), at the click: on a bookmark ten items and
/// three separators, on a folder eight and three, on the bar six and two (30 and 11 px,
/// plus padding and border).
pub fn bookmark_menu_rect(x: f64, y: f64, on_item: bool, is_folder: bool) -> PopupRect {
    let (items, seps) = match (on_item, is_folder) {
        (true, false) => (10.0, 3.0),
        (true, true) => (8.0, 3.0),
        (false, _) => (6.0, 2.0),
    };
    PopupRect { x, y, width: 240.0, height: items * 30.0 + seps * 11.0 + 20.0 }
}

/// The back/forward history menu (CSS px, 0.8.0): under the button at (`x`, `y`), one 30 px
/// row per entry, a separator and "Show full history".
pub const NAV_MENU_WIDTH: f64 = 340.0;
pub fn nav_menu_rect(x: f64, y: f64, items: usize) -> PopupRect {
    PopupRect { x, y, width: NAV_MENU_WIDTH, height: nav_menu_height(items) }
}
fn nav_menu_height(items: usize) -> f64 {
    (items.min(crate::navmenu::MAX_ITEMS) + 1) as f64 * 30.0 + 11.0 + 12.0
}

/// The bookmark editor (CSS px): a name, and an address for bookmarks.
pub fn bookmark_edit_rect(x: f64, y: f64, is_folder: bool) -> PopupRect {
    PopupRect { x, y, width: 340.0, height: if is_folder { 150.0 } else { 214.0 } }
}

/// The width of a page's dialog (CSS px, 0.12.4).
pub const JS_DIALOG_WIDTH: f64 = 440.0;

pub fn prompt_rect(kind: PopupKind, entries: usize, shell_width_css: f64) -> PopupRect {
    let (w, h) = match kind {
        PopupKind::SavePassword => (360.0, 196.0),
        PopupKind::Fill => (340.0, 110.0 + 42.0 * entries.clamp(1, 6) as f64),
        PopupKind::Menu => (280.0, 670.0),
        PopupKind::Shield => (300.0, 226.0),
        PopupKind::SiteInfo => (360.0, 260.0),
        PopupKind::Suggest | PopupKind::CardFill => (300.0, 160.0),
        PopupKind::BookmarkFolder => (FOLDER_POPUP_WIDTH, folder_height(entries)),
        PopupKind::Unlock => (360.0, 262.0),
        PopupKind::Permission => (380.0, 196.0),
        PopupKind::PasswordSaved => (360.0, 92.0),
        PopupKind::SaveCard => (360.0, 150.0),
        PopupKind::AddressSuggest => (560.0, 8.0 + 7.0 * 38.0),
        PopupKind::AddressMenu => (230.0, 260.0),
        PopupKind::Update => (360.0, 150.0),
        PopupKind::TabMenu => TAB_MENU_SIZE,
        PopupKind::NavMenu => (NAV_MENU_WIDTH, nav_menu_height(entries)),
        PopupKind::HttpAuth => (380.0, 286.0 + 38.0 * entries.min(5) as f64),
        PopupKind::TabPreview => (TAB_PREVIEW_WIDTH, tab_preview_height(entries > 0)),
        PopupKind::BookmarkMenu => (240.0, 352.0),
        PopupKind::CaptionMenu => (250.0, 352.0),
        PopupKind::Downloads => (380.0, 420.0),
        PopupKind::Pip => (340.0, 300.0),
        PopupKind::PipVolume => (PIP_SLIDER_WIDTH_DIP, PIP_SLIDER_HEIGHT_DIP),
        PopupKind::ExtInstall => (400.0, 120.0 + 24.0 * entries.clamp(1, 8) as f64),
        PopupKind::BookmarkEdit => (340.0, 214.0),
        PopupKind::TabList => (TAB_LIST_WIDTH, tab_list_height(entries)),
        PopupKind::Unresponsive => (380.0, 150.0),
        // An upper bound; the popup fits its height to the message (0.12.4).
        PopupKind::JsDialog => (JS_DIALOG_WIDTH, 460.0),
        // 0.14.0: the shell's own size (shell.js POPUP_SIZES); the popup fits its height.
        PopupKind::Vpn => (320.0, 330.0),
    };
    // A page's dialog sits in the middle, under the toolbar, as in Chrome.
    let x = if kind == PopupKind::JsDialog {
        ((shell_width_css - w) / 2.0).max(0.0)
    } else {
        (shell_width_css - w - 8.0).max(0.0)
    };
    PopupRect { x, y: SHELL_HEIGHT_DIP - 4.0, width: w, height: h }
}

/// The volume slider of the floating video window (0.11.0), in DIP. It replaced the separate
/// bar under the window (0.7.0, R8): it opens inside the window, next to the engine's own mute
/// button, while the pointer is on that button or on the slider.
pub const PIP_SLIDER_WIDTH_DIP: f64 = 116.0;
pub const PIP_SLIDER_HEIGHT_DIP: f64 = 28.0;
const PIP_SLIDER_GAP_DIP: f64 = 4.0;

/// The engine's mute button in the floating video window (screen pixels): 28×28 DIP, bottom
/// right, left of the live-caption button (Chromium `video_overlay_window_views.cc`: right edge
/// − 12 − 28 − 4 − 28, bottom − 4 − 28).
pub fn pip_mute_rect(pip: Rect, scale: f64) -> Rect {
    let d = |v: f64| (v * scale).round() as i32;
    Rect { x: pip.x + pip.width - d(72.0), y: pip.y + pip.height - d(32.0), width: d(28.0), height: d(28.0) }
}

/// Where the slider goes (screen pixels): left of the mute button, on its row, inside the
/// floating window.
pub fn pip_slider_rect(pip: Rect, scale: f64) -> Rect {
    let mute = pip_mute_rect(pip, scale);
    let d = |v: f64| (v * scale).round() as i32;
    let width = d(PIP_SLIDER_WIDTH_DIP).min((pip.width - d(8.0) - (pip.x + pip.width - mute.x)).max(d(48.0)));
    let x = (mute.x - d(PIP_SLIDER_GAP_DIP) - width).max(pip.x + d(4.0));
    Rect { x, y: mute.y, width, height: d(PIP_SLIDER_HEIGHT_DIP) }
}

/// One row of a folder dropdown (`popup.css` `.item`) and the dropdown's padding and border.
pub const FOLDER_ROW: f64 = 30.0;
const FOLDER_CHROME: f64 = 14.0;

fn folder_height(items: usize) -> f64 {
    FOLDER_CHROME + FOLDER_ROW * items.max(1) as f64
}

/// A bookmarks-bar folder dropdown anchored under the folder button (`anchor` is the button's
/// rect in shell CSS pixels): as tall as its rows, up to `room` (the screen below the anchor,
/// CSS px; 0.11.1: it stopped at 600 px before), the rest scrolls.
pub fn folder_popup_rect(anchor: PopupRect, items: usize, room: f64) -> PopupRect {
    let y = anchor.y + anchor.height + 2.0;
    PopupRect {
        x: anchor.x,
        y,
        width: FOLDER_POPUP_WIDTH,
        height: folder_height(items).min(room.max(FOLDER_CHROME + FOLDER_ROW * 4.0)),
    }
}

/// The tab list (0.11.0): under its "▾" button (`anchor`, shell CSS px), its right edge at the
/// button's, as tall as `tabs` rows need (with the search field) up to a limit; the rest scrolls.
/// 0.15.0: the tab hover card (CSS px): the picture (`TAB_PREVIEW_THUMB`, when there is one)
/// above the title and the site, with padding (`popup.css` `.tab-preview` uses the same).
pub const TAB_PREVIEW_WIDTH: f64 = 296.0;
pub const TAB_PREVIEW_THUMB: (f64, f64) = (280.0, 158.0);

fn tab_preview_height(thumbnail: bool) -> f64 {
    if thumbnail {
        66.0 + TAB_PREVIEW_THUMB.1 + 8.0
    } else {
        66.0
    }
}

/// Where the hover card of the tab at `anchor` (its rect in the shell) goes: just under the
/// tab, its left edge with the tab's, inside the shell's width (`shell_width_css`).
pub fn tab_preview_rect(anchor: PopupRect, thumbnail: bool, shell_width_css: f64) -> PopupRect {
    let max_x = (shell_width_css - TAB_PREVIEW_WIDTH - 4.0).max(0.0);
    PopupRect {
        x: anchor.x.clamp(0.0, max_x),
        y: anchor.y + anchor.height + 4.0,
        width: TAB_PREVIEW_WIDTH,
        height: tab_preview_height(thumbnail),
    }
}

pub fn tab_list_rect(anchor: PopupRect, tabs: usize) -> PopupRect {
    PopupRect {
        x: (anchor.x + anchor.width - TAB_LIST_WIDTH).max(0.0),
        y: anchor.y + anchor.height + 2.0,
        width: TAB_LIST_WIDTH,
        height: tab_list_height(tabs),
    }
}

pub const TAB_LIST_WIDTH: f64 = 380.0;
/// One row of the tab list (CSS px; `popup.css` uses the same).
pub const TAB_LIST_ROW: f64 = 34.0;

fn tab_list_height(tabs: usize) -> f64 {
    // The search field (48) and padding, then the rows, at most 14 at once.
    56.0 + TAB_LIST_ROW * tabs.clamp(1, 14) as f64
}

/// Folder dropdowns are this wide (CSS px); longer titles are shortened with "…" (0.7.2, R9).
pub const FOLDER_POPUP_WIDTH: f64 = 360.0;

/// Zoom levels offered by Ctrl +/- (like Chromium).
pub const ZOOM_STEPS: [f64; 17] =
    [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];

/// The next zoom step in or out from `current`.
pub fn next_zoom(current: f64, zoom_in: bool) -> f64 {
    let eps = 1e-3;
    if zoom_in {
        ZOOM_STEPS.iter().copied().find(|z| *z > current + eps).unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|z| *z < current - eps).unwrap_or(ZOOM_STEPS[0])
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn tab_preview_card_sits_under_its_tab_inside_the_window() {
        use super::*;
        let tab = PopupRect { x: 300.0, y: 6.0, width: 220.0, height: 34.0 };
        let r = tab_preview_rect(tab, true, 1280.0);
        assert_eq!((r.x, r.y, r.width), (300.0, 44.0, TAB_PREVIEW_WIDTH));
        assert!(r.height > TAB_PREVIEW_THUMB.1);
        let small = tab_preview_rect(tab, false, 1280.0);
        assert!(small.height < r.height && small.height >= 60.0);
        // The last tab of a narrow window: the card moves left to stay inside.
        let r = tab_preview_rect(PopupRect { x: 560.0, ..tab }, true, 640.0);
        assert!(r.x + r.width <= 640.0 && r.x < 560.0);
        assert!(r.is_valid());
        assert_eq!(prompt_rect(PopupKind::TabPreview, 1, 800.0).height, r.height);
    }

    #[test]
    fn panel_split_keeps_both_sides() {
        let content = Rect { x: 0, y: 76, width: 1280, height: 700 };
        let p = panel_split(content, 380.0, 1.0);
        assert_eq!(p.panel.width, 380);
        assert_eq!(p.splitter.width, 5);
        assert_eq!(p.page.width + p.splitter.width + p.panel.width, 1280);
        assert_eq!(p.panel.x + p.panel.width, 1280);
        assert_eq!(p.page.height, 700);
        // Too wide: the page keeps its minimum; too narrow: the panel keeps its minimum.
        assert_eq!(panel_split(content, 5000.0, 1.0).panel.width, 900);
        let narrow = Rect { x: 0, y: 0, width: 800, height: 600 };
        assert_eq!(panel_split(narrow, 700.0, 1.0).page.width, 320);
        assert_eq!(panel_split(content, 10.0, 1.0).panel.width, 280);
        // 150 %: DIP scale.
        let hi = panel_split(Rect { x: 0, y: 0, width: 1920, height: 900 }, 380.0, 1.5);
        assert_eq!(hi.panel.width, 570);
        // A tiny window shares the space and never goes negative.
        let tiny = panel_split(Rect { x: 0, y: 0, width: 200, height: 100 }, 380.0, 1.0);
        assert!(tiny.page.width >= 0 && tiny.panel.width >= 0);
        assert_eq!(tiny.page.width + tiny.splitter.width + tiny.panel.width, 200);
        assert_eq!(panel_width_at(content, 1280 - 402, 1.0), 399.5);
    }

    use super::*;

    #[test]
    fn the_tab_list_hangs_under_its_button() {
        let anchor = PopupRect { x: 900.0, y: 4.0, width: 28.0, height: 28.0 };
        let r = tab_list_rect(anchor, 3);
        assert_eq!((r.x, r.y, r.width), (548.0, 34.0, TAB_LIST_WIDTH));
        assert_eq!(r.height, 56.0 + 3.0 * TAB_LIST_ROW);
        assert_eq!(tab_list_rect(anchor, 200).height, 56.0 + 14.0 * TAB_LIST_ROW, "the rest scrolls");
        assert_eq!(tab_list_rect(PopupRect { x: 10.0, ..anchor }, 1).x, 0.0, "never left of the window");
    }

    #[test]
    fn the_pip_volume_slider_sits_next_to_the_mute_button_inside_the_window() {
        let r = |x, y, width, height| Rect { x, y, width, height };
        // A 400×225 window at 100 %: the mute button at the bottom right, the slider left of it.
        assert_eq!(pip_mute_rect(r(1400, 600, 400, 225), 1.0), r(1728, 793, 28, 28));
        assert_eq!(pip_slider_rect(r(1400, 600, 400, 225), 1.0), r(1608, 793, 116, 28));
        // 150 %: sizes scale.
        assert_eq!(pip_mute_rect(r(100, 100, 600, 338), 1.5), r(592, 390, 42, 42));
        assert_eq!(pip_slider_rect(r(100, 100, 600, 338), 1.5), r(412, 390, 174, 42));
        // A very small window: the slider shrinks and stays inside it.
        let small = r(0, 0, 160, 90);
        let s = pip_slider_rect(small, 1.0);
        assert!(s.x >= 4 && s.x + s.width <= pip_mute_rect(small, 1.0).x, "{s:?}");
    }

    #[test]
    fn bar_band_follows_the_toolbar() {
        assert!(over_bookmarks_bar(true, 100.0, 80.0, 1200.0));
        assert!(over_bookmarks_bar(true, 0.0, SHELL_HEIGHT_DIP - BAR_SLACK_DIP, 1200.0));
        assert!(over_bookmarks_bar(true, 1200.0, SHELL_HEIGHT_DIP + BOOKMARKS_BAR_DIP + BAR_SLACK_DIP, 1200.0));
        assert!(!over_bookmarks_bar(true, 100.0, 60.0, 1200.0), "the toolbar is not the bar");
        assert!(!over_bookmarks_bar(true, 100.0, 120.0, 1200.0), "the page is not the bar");
        assert!(!over_bookmarks_bar(true, -1.0, 80.0, 1200.0) && !over_bookmarks_bar(true, 1201.0, 80.0, 1200.0));
        assert!(!over_bookmarks_bar(false, 100.0, 80.0, 1200.0), "no bar, no band");
    }

    #[test]
    fn shell_scales_with_dpi() {
        assert_eq!(shell_height(1.0, false), 76);
        assert_eq!(nav_menu_rect(4.0, 72.0, 3).height, 4.0 * 30.0 + 23.0);
        assert_eq!(nav_menu_rect(4.0, 72.0, 40).height, nav_menu_rect(4.0, 72.0, 15).height, "15 entries at most");
        assert_eq!(shell_height(1.5, false), 114);
        assert_eq!(shell_height(2.0, true), 208);
        assert_eq!(shell_height(f64::NAN, false), 76);
        let (s, c) = split(1000, 700, 1.25, false);
        assert_eq!(s, Rect { x: 0, y: 0, width: 1000, height: 95 });
        assert_eq!(c, Rect { x: 0, y: 95, width: 1000, height: 605 });
        let (s, c) = split(300, 50, 1.0, false);
        assert_eq!((s.height, c.height), (50, 0), "tiny window");
    }

    #[test]
    fn popup_placement_at_150_percent() {
        let work = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        let css = PopupRect { x: 900.0, y: 40.0, width: 280.0, height: 440.0 };
        let r = popup_screen_rect(css, 1.5, (100, 200), work);
        assert_eq!(r, Rect { x: 100 + 1350, y: 200 + 60, width: 420, height: 660 });
        // Pushed back inside the work area on the right and bottom.
        let r = popup_screen_rect(css, 1.5, (1500, 600), work);
        assert_eq!(r.x + r.width, 1920);
        assert_eq!(r.y + r.height, 1040);
        // Second monitor with a negative origin.
        let left = Rect { x: -1280, y: 0, width: 1280, height: 1000 };
        let r = popup_screen_rect(PopupRect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 }, 1.0, (-2000, -50), left);
        assert_eq!((r.x, r.y), (-1280, 0));
        // Larger than the screen: shrunk to fit.
        let r = popup_screen_rect(PopupRect { x: 0.0, y: 0.0, width: 4000.0, height: 4000.0 }, 1.0, (0, 0), work);
        assert_eq!((r.width, r.height), (1920, 1040));
    }

    /// The work areas (physical pixels) the DPI passes run on: a taskbar of 48 DIP.
    fn work_of(width: i32, height: i32, scale: f64) -> Rect {
        Rect { x: 0, y: 0, width, height: height - (48.0 * scale).round() as i32 }
    }

    #[test]
    fn a_popup_that_fits_stays_at_its_anchor_at_every_scale() {
        // The tab menu (240 x 287 CSS px) at a click 48,18 into a window at 8,8 DIP.
        for (w, h, scale) in [
            (1280, 800, 1.0),
            (1280, 800, 1.25),
            (1920, 1080, 1.5),
            (1920, 1080, 1.75),
            (3840, 2160, 2.0),
            (3840, 2160, 2.5),
        ] {
            let s = Scale::new(scale);
            let origin = (s.px(8.0), s.px(8.0));
            let css = PopupRect { x: 48.0, y: 18.0, width: 240.0, height: 256.0 };
            for anchor in [PopupAnchor::Drop, PopupAnchor::Point, PopupAnchor::Keep] {
                let p = place_popup(css, 287.0, anchor, scale, origin, work_of(w, h, scale));
                assert_eq!(
                    p.rect,
                    Rect {
                        x: origin.0 + s.px(48.0),
                        y: origin.1 + s.px(18.0),
                        width: s.px(240.0),
                        height: s.px(287.0)
                    },
                    "{scale}"
                );
                assert!(!p.scrolls);
            }
        }
    }

    #[test]
    fn at_100_percent_placement_is_what_it_was() {
        // Same answer as the clamp of 0.15.3 whenever the content fits the work area.
        let work = Rect { x: 0, y: 0, width: 1280, height: 752 };
        for (css, h) in [
            (PopupRect { x: 992.0, y: 72.0, width: 280.0, height: 670.0 }, 658.0),
            (PopupRect { x: 992.0, y: 300.0, width: 280.0, height: 670.0 }, 658.0),
            (PopupRect { x: 40.0, y: 600.0, width: 240.0, height: 352.0 }, 287.0),
        ] {
            let old = popup_screen_rect(PopupRect { height: h, ..css }, 1.0, (0, 0), work);
            assert_eq!(place_popup(css, h, PopupAnchor::Drop, 1.0, (0, 0), work).rect, old);
        }
    }

    #[test]
    fn a_context_menu_opens_above_the_click_when_there_is_no_room_below() {
        let work = work_of(1920, 1080, 1.5); // 1920 x 1008
        let css = PopupRect { x: 300.0, y: 560.0, width: 240.0, height: 352.0 };
        // 560 CSS px down a window at the top: 840 px; 287 CSS px = 431 px do not fit below.
        let p = place_popup(css, 287.0, PopupAnchor::Point, 1.5, (0, 0), work);
        assert_eq!(p.rect, Rect { x: 450, y: 840 - 431, width: 360, height: 431 });
        assert!(!p.scrolls);
        // A dropdown under a control moves up only as far as it must.
        let p = place_popup(css, 287.0, PopupAnchor::Drop, 1.5, (0, 0), work);
        assert_eq!(p.rect, Rect { x: 450, y: 1008 - 431, width: 360, height: 431 });
        // No room above either: moved up like a dropdown, the whole menu on screen.
        let low = Rect { x: 0, y: 0, width: 1920, height: 500 };
        let p = place_popup(PopupRect { y: 150.0, ..css }, 287.0, PopupAnchor::Point, 1.5, (0, 0), low);
        assert_eq!((p.rect.y, p.rect.height, p.scrolls), (500 - 431, 431, false));
    }

    #[test]
    fn a_popup_taller_than_the_work_area_fills_it_and_scrolls() {
        // The ⋮ menu of 0.15.3 (658 CSS px) on a 1920x1080 display at 175 %: 1152 px of
        // content, 996 px of work area. It used to be cut off (Settings, About and Exit
        // unreachable).
        let work = work_of(1920, 1080, 1.75);
        assert_eq!(work.height, 996);
        let css = PopupRect { x: 660.0, y: 72.0, width: 280.0, height: 670.0 };
        let p = place_popup(css, 658.0, PopupAnchor::Drop, 1.75, (26, 14), work);
        assert_eq!(p.rect, Rect { x: 26 + 1155, y: 0, width: 490, height: 996 });
        assert!(p.scrolls);
        // The same content made compact by the popup (545 CSS px) fits without scrolling.
        let p = place_popup(css, 545.0, PopupAnchor::Drop, 1.75, (26, 14), work);
        assert_eq!((p.rect.y + p.rect.height, p.rect.height, p.scrolls), (996, 954, false));
    }

    #[test]
    fn a_list_stays_under_its_control_and_scrolls() {
        // A bookmark folder with 40 entries (1214 CSS px) under the bar of a maximized
        // window, 1920x1080 at 100 %: the host asked for the room below (928 px).
        let work = work_of(1920, 1080, 1.0);
        let css = PopupRect { x: 8.0, y: 104.0, width: 360.0, height: 920.0 };
        let p = place_popup(css, 1214.0, PopupAnchor::Keep, 1.0, (0, 0), work);
        assert_eq!(p.rect, Rect { x: 8, y: 104, width: 360, height: 928 }, "never over the bar it hangs from");
        assert!(p.scrolls);
        // Exactly what 0.15.3 did: the asked rectangle clamped, then the content's height up
        // to the work area's bottom.
        for (css, h, scale) in [
            (PopupRect { x: 147.0, y: 70.0, width: 644.0, height: 274.0 }, 198.0, 1.0),
            (PopupRect { x: 8.0, y: 104.0, width: 360.0, height: 134.0 }, 134.0, 1.75),
            (PopupRect { x: 500.0, y: 600.0, width: 380.0, height: 500.0 }, 700.0, 1.25),
        ] {
            let work = work_of(1920, 1080, scale);
            let old = popup_screen_rect(css, scale, (10, 10), work);
            let old_h = ((h * scale).round() as i32).min(work.y + work.height - old.y);
            let p = place_popup(css, h, PopupAnchor::Keep, scale, (10, 10), work);
            assert_eq!(p.rect, Rect { height: old_h, ..old }, "{css:?} at {scale}");
        }
    }

    #[test]
    fn the_room_a_popup_has_at_its_anchor() {
        // The ⋮ menu of a maximized window, 1920x1080: under the toolbar at 72 CSS px.
        let css = PopupRect { x: 990.0, y: 72.0, width: 280.0, height: 670.0 };
        let room = |scale: f64, anchor| popup_room(css, anchor, scale, (0, 0), work_of(1920, 1080, scale));
        assert_eq!(room(1.0, PopupAnchor::Drop), 960.0);
        assert_eq!(room(1.5, PopupAnchor::Drop), 600.0, "less than the menu's 658: it becomes compact and stays put");
        assert!((room(1.75, PopupAnchor::Drop) - 497.14).abs() < 0.01);
        assert_eq!(room(1.5, PopupAnchor::Keep), 0.0);
        // A context menu low on the screen has the room above the click.
        let low = PopupRect { x: 300.0, y: 560.0, width: 240.0, height: 352.0 };
        assert_eq!(popup_room(low, PopupAnchor::Point, 1.5, (0, 0), work_of(1920, 1080, 1.5)), 560.0);
        assert_eq!(popup_room(low, PopupAnchor::Drop, 1.5, (0, 0), work_of(1920, 1080, 1.5)), 112.0);
    }

    #[test]
    fn placement_never_leaves_the_work_area() {
        let works = [
            Rect { x: 0, y: 0, width: 1280, height: 752 },
            Rect { x: -2560, y: -300, width: 2560, height: 1392 },
            Rect { x: 1920, y: 0, width: 3840, height: 2040 },
            Rect { x: 0, y: 0, width: 300, height: 200 },
        ];
        for work in works {
            for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
                for anchor in [PopupAnchor::Drop, PopupAnchor::Point, PopupAnchor::Keep] {
                    for (x, y) in [(-500.0, -500.0), (0.0, 0.0), (600.0, 90.0), (4000.0, 4000.0)] {
                        for h in [1.0, 40.0, 287.0, 658.0, 4096.0, f64::NAN] {
                            let css = PopupRect { x, y, width: 280.0, height: 100.0 };
                            let p = place_popup(css, h, anchor, scale, (work.x + 50, work.y + 20), work);
                            let r = p.rect;
                            assert!(r.width >= 1 && r.height >= 1, "{r:?}");
                            assert!(r.x >= work.x && r.x + r.width <= work.x + work.width, "{r:?} in {work:?}");
                            assert!(r.y >= work.y && r.y + r.height <= work.y + work.height, "{r:?} in {work:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_new_window_has_the_same_dip_size_at_every_scale() {
        // Windows' cascade put it at 100,100 (and at 44,44 times the scale on the VM).
        let chosen = Rect { x: 100, y: 100, width: 1280, height: 860 };
        // 100 % on a display with room: exactly what 0.15.3 made.
        assert_eq!(default_window(chosen, 1.0, work_of(1920, 1080, 1.0)), chosen);
        // 3840 x 2160 at 200 % and 250 %: 0.15.3 opened 1280 x 860 px there (640 x 430 and
        // 512 x 344 DIP).
        assert_eq!(
            default_window(chosen, 2.0, work_of(3840, 2160, 2.0)),
            Rect { x: 100, y: 100, width: 2560, height: 1720 }
        );
        let r = default_window(chosen, 2.5, work_of(3840, 2160, 2.5));
        assert_eq!((r.width, r.height), (3200, 2040), "860 DIP is 2150 px: limited to the work area");
        assert_eq!((r.x, r.y), (100, 0));
        // 1920 x 1080 at 175 %: 2240 x 1505 px wanted, the work area is 1920 x 996.
        let r = default_window(Rect { x: 220, y: 220, ..chosen }, 1.75, work_of(1920, 1080, 1.75));
        assert_eq!(r, Rect { x: 0, y: 0, width: 1920, height: 996 });
        // The VM's 1280 x 800 at 100 %: no longer hanging under the taskbar.
        let r = default_window(Rect { x: 0, y: 0, ..chosen }, 1.0, work_of(1280, 800, 1.0));
        assert_eq!(r, Rect { x: 0, y: 0, width: 1280, height: 752 });
    }

    #[test]
    fn a_saved_window_keeps_its_dip_size_when_the_scale_changed() {
        let m = |w: i32, h: i32, dpi: u32| Monitor { work: work_of(w, h, f64::from(dpi) / 96.0), dpi };
        let saved = Rect { x: 8, y: 8, width: 960, height: 560 };
        // Same scale, or a session from before 0.15.4: untouched.
        assert_eq!(restore_window(saved, Some(96), &[m(1920, 1080, 96)]), saved);
        assert_eq!(restore_window(saved, None, &[m(1920, 1080, 144)]), saved);
        // Saved at 100 %, the display is at 150 % now: 1440 x 840, same place.
        assert_eq!(
            restore_window(saved, Some(96), &[m(1920, 1080, 144)]),
            Rect { x: 8, y: 8, width: 1440, height: 840 }
        );
        // And back.
        assert_eq!(
            restore_window(Rect { x: 8, y: 8, width: 1440, height: 840 }, Some(144), &[m(1920, 1080, 96)]),
            saved
        );
        // Grown past the work area: limited to it and moved inside.
        let big = Rect { x: 300, y: 200, width: 1500, height: 800 };
        assert_eq!(restore_window(big, Some(96), &[m(1920, 1080, 168)]), Rect { x: 0, y: 0, width: 1920, height: 996 });
        // Two monitors: the scale of the one the window is on counts.
        let two = [m(1920, 1080, 96), Monitor { work: Rect { x: 1920, y: 0, width: 3840, height: 2040 }, dpi: 240 }];
        let on_right = Rect { x: 2000, y: 100, width: 1000, height: 700 };
        assert_eq!(restore_window(on_right, Some(192), &two), Rect { x: 2000, y: 100, width: 1250, height: 875 });
        assert_eq!(restore_window(saved, Some(96), &two), saved);
        // A monitor that is gone: onto the first one, as before.
        let gone = Rect { x: 6000, y: 100, width: 1000, height: 700 };
        let r = restore_window(gone, Some(96), &[m(1920, 1080, 96)]);
        assert!(r.x + r.width <= 1920 && r.x >= 0, "{r:?}");
        // No monitors reported: nothing to go by.
        assert_eq!(restore_window(saved, Some(96), &[]), saved);
        // A larger-than-screen window saved at the same scale only shrinks.
        let huge = Rect { x: -8, y: -8, width: 4000, height: 3000 };
        assert_eq!(
            restore_window(huge, Some(96), &[m(1920, 1080, 96)]),
            Rect { x: 0, y: 0, width: 1920, height: 1032 }
        );
    }

    #[test]
    fn prompts_sit_under_the_toolbar_right() {
        let r = prompt_rect(PopupKind::SavePassword, 0, 1200.0);
        assert_eq!(r.x + r.width, 1192.0);
        assert!(r.is_valid());
        assert!(prompt_rect(PopupKind::Fill, 3, 1200.0).height > prompt_rect(PopupKind::Fill, 1, 1200.0).height);
        assert!(prompt_rect(PopupKind::Fill, 99, 200.0).is_valid());
    }

    #[test]
    fn folder_dropdown() {
        let anchor = PopupRect { x: 120.0, y: 78.0, width: 90.0, height: 24.0 };
        let r = folder_popup_rect(anchor, 3, 900.0);
        assert_eq!((r.x, r.y, r.width, r.height), (120.0, 104.0, 360.0, 104.0));
        assert_eq!(folder_popup_rect(anchor, 0, 900.0).height, 44.0, "room for an 'empty' line");
        // 0.11.1: as tall as the screen below the bar allows (it stopped at 600 px), then scrolls.
        assert_eq!(folder_popup_rect(anchor, 30, 1200.0).height, 914.0);
        assert_eq!(folder_popup_rect(anchor, 500, 950.0).height, 950.0, "the rest scrolls");
        assert_eq!(folder_popup_rect(anchor, 10, 50.0).height, 134.0, "at least four rows");
    }

    #[test]
    fn zoom_steps() {
        assert_eq!(next_zoom(1.0, true), 1.1);
        assert_eq!(next_zoom(1.0, false), 0.9);
        assert_eq!(next_zoom(1.05, true), 1.1);
        assert_eq!(next_zoom(1.05, false), 1.0);
        assert_eq!(next_zoom(5.0, true), 5.0);
        assert_eq!(next_zoom(0.25, false), 0.25);
    }

    #[test]
    fn shield_grows_with_its_list() {
        let a = PopupRect { x: 900.0, y: 72.0, width: 300.0, height: 226.0 };
        assert_eq!(shield_rect(a, 0), a);
        let r = shield_rect(a, 3);
        assert_eq!((r.x + r.width, r.y, r.width), (1200.0, 72.0, 360.0), "right edge kept");
        assert_eq!(r.height, 226.0 + 12.0 + 72.0 + 32.0);
        assert_eq!(shield_rect(a, 20).height, shield_rect(a, 6).height, "six rows at most");
        assert_eq!(shield_rect(PopupRect { x: 10.0, ..a }, 2).x, 0.0);
    }

    #[test]
    fn restored_windows_stay_reachable() {
        let primary = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        let right = Rect { x: 1920, y: 0, width: 2560, height: 1400 };
        let w = Rect { x: 100, y: 80, width: 1200, height: 800 };
        assert_eq!(fit_to_monitors(w, &[primary]), w, "on screen: unchanged");
        // Partly off screen but the title band is reachable: unchanged.
        let partly = Rect { x: 1600, y: 80, width: 1200, height: 800 };
        assert_eq!(fit_to_monitors(partly, &[primary]), partly);
        // On a monitor that is gone: moved onto the primary, same size.
        let gone = Rect { x: 2200, y: 100, width: 1200, height: 800 };
        assert_eq!(fit_to_monitors(gone, &[primary, right]), gone, "that monitor is still there");
        assert_eq!(fit_to_monitors(gone, &[primary]), Rect { x: 720, y: 100, width: 1200, height: 800 });
        // Larger than the monitor now (smaller resolution): shrunk.
        let big = Rect { x: -3000, y: -50, width: 2560, height: 1400 };
        assert_eq!(fit_to_monitors(big, &[primary]), Rect { x: 0, y: 0, width: 1920, height: 1040 });
        // Title bar above the screen: moved down.
        let above = Rect { x: 200, y: -600, width: 800, height: 600 };
        assert_eq!(fit_to_monitors(above, &[primary]).y, 0);
        // A monitor left of the primary (negative coordinates) counts too.
        let left = Rect { x: -1280, y: 0, width: 1280, height: 1000 };
        let on_left = Rect { x: -1100, y: 50, width: 800, height: 600 };
        assert_eq!(fit_to_monitors(on_left, &[primary, left]), on_left);
        assert_eq!(fit_to_monitors(w, &[]), w);
    }

    #[test]
    fn devtools_strip_buttons_at_every_scale() {
        let content = Rect { x: 0, y: 76, width: 1200, height: 700 };
        for (scale, size, gap, strip_h) in [(1.0, 20, 4, 24), (1.25, 25, 5, 30), (1.5, 30, 6, 36)] {
            let s = devtools_split(content, DevToolsDock::Right, 480.0, scale);
            let l = devtools_strip(&s, scale);
            assert_eq!(l.strip.height, strip_h, "{scale}");
            assert_eq!(l.close.width, size);
            assert_eq!(l.close.x + l.close.width + gap, l.strip.x + l.strip.width, "close at the right end ({scale})");
            assert_eq!(l.dock.x + l.dock.width + gap, l.close.x, "dock left of close ({scale})");
            assert_eq!(l.close.y - l.strip.y, (strip_h - size) / 2, "centred ({scale})");
            // Hit testing: the buttons, the strip in between, nothing above.
            assert_eq!(strip_hit(&l, l.close.x + 1, l.close.y + 1), Some(StripButton::Close));
            assert_eq!(strip_hit(&l, l.dock.x + size - 1, l.dock.y + size - 1), Some(StripButton::Dock));
            assert_eq!(strip_hit(&l, l.strip.x + 5, l.strip.y + 5), None);
            assert!(on_strip(&s, l.strip.x + 5, l.strip.y + 5));
            assert!(!on_strip(&s, l.strip.x + 5, l.strip.y + l.strip.height));
            assert!(!on_strip(&s, l.strip.x - 1, l.strip.y + 5), "the splitter is not the strip");
        }
        // Bottom dock: the strip spans the window width under the splitter.
        let b = devtools_split(content, DevToolsDock::Bottom, 320.0, 1.25);
        let l = devtools_strip(&b, 1.25);
        assert_eq!((l.strip.x, l.strip.width, l.strip.y), (0, 1200, b.splitter.y + b.splitter.height));
        assert_eq!(l.close.x + l.close.width + 5, 1200);
    }

    #[test]
    fn devtools_split_and_clamping() {
        let content = Rect { x: 0, y: 76, width: 1200, height: 700 };
        let s = devtools_split(content, DevToolsDock::Right, 480.0, 1.0);
        assert_eq!(s.strip, Rect { x: 720, y: 76, width: 480, height: 24 });
        assert_eq!(s.devtools, Rect { x: 720, y: 100, width: 480, height: 676 });
        assert_eq!(s.splitter, Rect { x: 715, y: 76, width: 5, height: 700 });
        assert_eq!(s.page, Rect { x: 0, y: 76, width: 715, height: 700 });
        // The three parts tile the content exactly.
        assert_eq!(s.page.width + s.splitter.width + s.devtools.width, content.width);
        // Too small or too large a size is clamped: DevTools >= 280, page >= 240.
        assert_eq!(devtools_split(content, DevToolsDock::Right, 50.0, 1.0).devtools.width, 280);
        assert_eq!(devtools_split(content, DevToolsDock::Right, 5000.0, 1.0).page.width, 240);
        // Bottom.
        let b = devtools_split(content, DevToolsDock::Bottom, 320.0, 1.0);
        assert_eq!(b.strip, Rect { x: 0, y: 456, width: 1200, height: 24 });
        assert_eq!(b.devtools, Rect { x: 0, y: 480, width: 1200, height: 296 });
        assert_eq!(b.page.height + b.splitter.height + b.strip.height + b.devtools.height, 700);
        let min_b = devtools_split(content, DevToolsDock::Bottom, 10.0, 1.0);
        assert_eq!(min_b.strip.height + min_b.devtools.height, 160, "the minimum includes the strip");
        assert_eq!(devtools_split(content, DevToolsDock::Bottom, 900.0, 1.0).page.height, 240);
        // 150 %: sizes are DIP.
        let hi = devtools_split(Rect { x: 0, y: 114, width: 1800, height: 1050 }, DevToolsDock::Right, 480.0, 1.5);
        assert_eq!((hi.devtools.width, hi.splitter.width, hi.strip.height), (720, 8, 36));
        // A window too small for both minimums: proportional, nothing negative.
        let tiny = devtools_split(Rect { x: 0, y: 0, width: 400, height: 300 }, DevToolsDock::Right, 480.0, 1.0);
        assert!(tiny.page.width > 0 && tiny.devtools.width > 0);
        assert_eq!(tiny.page.width + tiny.splitter.width + tiny.devtools.width, 400);
        assert!(tiny.devtools.width > tiny.page.width, "DevTools' minimum is the larger one");
        let zero = devtools_split(Rect { x: 0, y: 0, width: 0, height: 0 }, DevToolsDock::Bottom, 480.0, 1.0);
        assert_eq!((zero.page.height, zero.devtools.height), (0, 0));
        // Dragging the splitter.
        assert_eq!(devtools_size_at(content, DevToolsDock::Right, 700, 300, 1.0), 497.5);
        assert_eq!(devtools_size_at(content, DevToolsDock::Right, 1190, 300, 1.0), 280.0, "clamped");
        assert_eq!(devtools_size_at(content, DevToolsDock::Bottom, 10, 500, 1.0), 273.5);
        assert_eq!(clamp_devtools_size(content, DevToolsDock::Right, f64::NAN, 1.0), 280.0);
    }
}
