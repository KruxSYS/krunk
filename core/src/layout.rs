//! Window layout math (pure, so it is testable without Windows).

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
    if scale.is_finite() && (0.5..=8.0).contains(&scale) {
        scale
    } else {
        1.0
    }
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

/// The bookmark editor (CSS px): a name, and an address for bookmarks.
pub fn bookmark_edit_rect(x: f64, y: f64, is_folder: bool) -> PopupRect {
    PopupRect { x, y, width: 340.0, height: if is_folder { 150.0 } else { 214.0 } }
}

pub fn prompt_rect(kind: PopupKind, entries: usize, shell_width_css: f64) -> PopupRect {
    let (w, h) = match kind {
        PopupKind::SavePassword => (360.0, 150.0),
        PopupKind::Fill => (340.0, 110.0 + 42.0 * entries.clamp(1, 6) as f64),
        PopupKind::Menu => (280.0, 620.0),
        PopupKind::Shield => (300.0, 226.0),
        PopupKind::SiteInfo => (360.0, 260.0),
        PopupKind::Suggest | PopupKind::CardFill => (300.0, 160.0),
        PopupKind::BookmarkFolder => (280.0, folder_height(entries)),
        PopupKind::Unlock => (360.0, 200.0),
        PopupKind::Permission => (380.0, 196.0),
        PopupKind::PasswordSaved => (360.0, 92.0),
        PopupKind::SaveCard => (360.0, 150.0),
        PopupKind::AddressSuggest => (560.0, 8.0 + 7.0 * 38.0),
        PopupKind::AddressMenu => (230.0, 260.0),
        PopupKind::Update => (360.0, 150.0),
        PopupKind::TabMenu => TAB_MENU_SIZE,
        PopupKind::BookmarkMenu => (240.0, 352.0),
        PopupKind::CaptionMenu => (250.0, 352.0),
        PopupKind::Downloads => (380.0, 420.0),
        PopupKind::Pip => (340.0, 300.0),
        PopupKind::PipVolume => (PIP_BAR_MIN_WIDTH_DIP, PIP_BAR_HEIGHT_DIP),
        PopupKind::BookmarkEdit => (340.0, 214.0),
    };
    let x = (shell_width_css - w - 8.0).max(0.0);
    PopupRect { x, y: SHELL_HEIGHT_DIP - 4.0, width: w, height: h }
}

/// The volume bar next to the floating video window (0.7.0, R8), in DIP.
pub const PIP_BAR_HEIGHT_DIP: f64 = 40.0;
pub const PIP_BAR_MIN_WIDTH_DIP: f64 = 240.0;
const PIP_BAR_GAP_DIP: f64 = 4.0;

/// Where the volume bar goes (screen pixels): as wide as the floating window (at least
/// [`PIP_BAR_MIN_WIDTH_DIP`]), flush under it; above it when the work area ends there; over
/// its bottom edge when neither fits. Always inside the work area.
pub fn pip_bar_rect(pip: Rect, work: Rect, scale: f64) -> Rect {
    let h = (PIP_BAR_HEIGHT_DIP * scale).round() as i32;
    let gap = (PIP_BAR_GAP_DIP * scale).round() as i32;
    let width = pip.width.max((PIP_BAR_MIN_WIDTH_DIP * scale).round() as i32).min(work.width);
    let below = pip.y + pip.height + gap;
    let above = pip.y - gap - h;
    let y = if below + h <= work.y + work.height {
        below
    } else if above >= work.y {
        above
    } else {
        pip.y + pip.height - h
    };
    let x = pip.x.clamp(work.x, (work.x + work.width - width).max(work.x));
    Rect { x, y: y.clamp(work.y, (work.y + work.height - h).max(work.y)), width, height: h }
}

fn folder_height(items: usize) -> f64 {
    (14.0 + 32.0 * items.max(1) as f64).min(600.0)
}

/// A bookmarks-bar folder dropdown anchored under the folder button
/// (`anchor` is the button's rect in shell CSS pixels).
pub fn folder_popup_rect(anchor: PopupRect, items: usize) -> PopupRect {
    PopupRect { x: anchor.x, y: anchor.y + anchor.height + 2.0, width: 280.0, height: folder_height(items) }
}

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
    use super::*;

    #[test]
    fn the_pip_volume_bar_stays_next_to_the_floating_window() {
        let work = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        let r = |x, y, width, height| Rect { x, y, width, height };
        // Room below: flush under it, as wide as it.
        assert_eq!(pip_bar_rect(r(1400, 600, 400, 225), work, 1.0), r(1400, 829, 400, 40));
        // At the bottom of the work area: above it.
        assert_eq!(pip_bar_rect(r(1400, 800, 400, 225), work, 1.0), r(1400, 756, 400, 40));
        // A small window: the bar keeps its minimum width, inside the screen.
        assert_eq!(pip_bar_rect(r(1800, 100, 160, 90), work, 1.0), r(1680, 194, 240, 40));
        // 150 %: sizes scale.
        assert_eq!(pip_bar_rect(r(100, 100, 600, 338), work, 1.5), r(100, 444, 600, 60));
        // A window filling the height: over its bottom edge.
        assert_eq!(pip_bar_rect(r(0, 0, 1920, 1040), work, 1.0), r(0, 1000, 1920, 40));
        // A second monitor left of the primary.
        let left = Rect { x: -1280, y: 0, width: 1280, height: 984 };
        assert_eq!(pip_bar_rect(r(-1290, 50, 300, 170), left, 1.0), r(-1280, 224, 300, 40));
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
        let r = folder_popup_rect(anchor, 3);
        assert_eq!((r.x, r.y, r.width, r.height), (120.0, 104.0, 280.0, 110.0));
        assert_eq!(folder_popup_rect(anchor, 0).height, 46.0, "room for an 'empty' line");
        assert_eq!(folder_popup_rect(anchor, 500).height, 600.0, "scrolls beyond 600px");
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
