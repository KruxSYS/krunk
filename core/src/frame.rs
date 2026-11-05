//! The frameless main window: which part of the window a point is over.
//!
//! The native title bar is gone; the tab strip is the top of the window.
//! The shell reports where its interactive parts are (the tabs, the "+"
//! button) and where it draws the minimize/maximize/close buttons; the host
//! answers `WM_NCHITTEST` with [`hit_test`]. Everything in the tab row that
//! is not interactive drags the window. The left, right and bottom resize
//! borders stay native (outside the client area); the top one is inside the
//! client area and handled here.

use crate::protocol::PopupRect;
use serde::{Deserialize, Serialize};

/// Height of the band at the top edge that resizes the window (DIP), when
/// the window is not maximized.
pub const TOP_RESIZE_DIP: f64 = 5.0;
/// Width of the corner zones of that band, which resize diagonally (DIP).
pub const CORNER_DIP: f64 = 16.0;
/// Interactive rectangles the shell may report.
pub const MAX_RECTS: usize = 16;
/// Tab centres the shell may report (more tabs than this are not dropped between).
pub const MAX_TABS: usize = 512;

/// What the shell reports, in CSS pixels (= DIP) of its viewport, whose
/// top-left corner is the client area's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HitMap {
    /// Height of the tab row; below it everything is client area.
    pub strip_height: f64,
    /// Parts of the tab row that take clicks (tabs, the "+" button).
    pub interactive: Vec<PopupRect>,
    pub minimize: PopupRect,
    pub maximize: PopupRect,
    pub close: PopupRect,
    /// The horizontal centre of each tab, left to right (CSS px of the viewport; tabs
    /// scrolled out of view may lie outside it): where a tab dragged from another window
    /// would be dropped.
    #[serde(default)]
    pub tab_centers: Vec<f64>,
}

impl HitMap {
    /// Sane values only (a buggy or hostile shell cannot make the whole
    /// window a drag area larger than the tab row, but it is checked anyway).
    pub fn is_valid(&self) -> bool {
        let ok =
            |r: &PopupRect| [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite() && (0.0..=16384.0).contains(v));
        self.strip_height.is_finite()
            && (0.0..=200.0).contains(&self.strip_height)
            && self.interactive.len() <= MAX_RECTS
            && self.interactive.iter().all(ok)
            && [&self.minimize, &self.maximize, &self.close].into_iter().all(ok)
            && self.tab_centers.len() <= MAX_TABS
            && self.tab_centers.iter().all(|c| c.is_finite() && (-1_000_000.0..=1_000_000.0).contains(c))
    }
}

/// The insert position for a tab dropped at `x` (CSS px) on a strip whose tabs have these
/// centres: the number of tabs whose centre lies left of `x`.
pub fn drop_index(tab_centers: &[f64], x: f64) -> usize {
    tab_centers.iter().filter(|c| **c < x).count()
}

/// Where a point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Page or shell content: the browser gets the mouse.
    Client,
    /// Empty tab-row space: drag, double-click maximizes, right-click
    /// opens the system menu.
    Caption,
    Top,
    TopLeft,
    TopRight,
    Minimize,
    /// Also what makes Windows 11 show the Snap Layouts flyout.
    Maximize,
    Close,
}

fn inside(r: &PopupRect, x: f64, y: f64) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Classifies a point (DIP, client coordinates) of a client area `width`
/// DIP wide. `map` is `None` until the shell reported one: then the whole
/// default tab row drags. In fullscreen the host does not call this.
pub fn hit_test(map: Option<&HitMap>, x: f64, y: f64, width: f64, maximized: bool) -> Hit {
    if !maximized && (0.0..TOP_RESIZE_DIP).contains(&y) {
        if x < CORNER_DIP {
            return Hit::TopLeft;
        }
        if x >= width - CORNER_DIP {
            return Hit::TopRight;
        }
        return Hit::Top;
    }
    let Some(map) = map else {
        return if y < 36.0 { Hit::Caption } else { Hit::Client };
    };
    // Maximized, the buttons reach the screen edge (Fitts' law): the
    // top-right pixel closes, as in other Windows apps.
    let (bx, by) = if maximized { (x.min(width - 1.0), y.max(map.close.y)) } else { (x, y) };
    if inside(&map.close, bx, by) {
        return Hit::Close;
    }
    if inside(&map.maximize, bx, by) {
        return Hit::Maximize;
    }
    if inside(&map.minimize, bx, by) {
        return Hit::Minimize;
    }
    if y >= map.strip_height || y < 0.0 {
        return Hit::Client;
    }
    if map.interactive.iter().any(|r| inside(r, x, y)) {
        return Hit::Client;
    }
    Hit::Caption
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, width: f64, height: f64) -> PopupRect {
        PopupRect { x, y, width, height }
    }

    /// A 1000-DIP-wide shell: tabs from 6 to 500, "+" at 502, caption
    /// buttons of 46 x 36 at the right.
    fn map() -> HitMap {
        HitMap {
            strip_height: 36.0,
            interactive: vec![r(6.0, 6.0, 494.0, 30.0), r(502.0, 6.0, 28.0, 28.0)],
            minimize: r(862.0, 0.0, 46.0, 36.0),
            maximize: r(908.0, 0.0, 46.0, 36.0),
            close: r(954.0, 0.0, 46.0, 36.0),
            tab_centers: vec![50.0, 150.0, 250.0],
        }
    }

    #[test]
    fn drop_positions() {
        let m = map();
        assert_eq!(drop_index(&m.tab_centers, 10.0), 0);
        assert_eq!(drop_index(&m.tab_centers, 50.0), 0, "on a centre: before that tab");
        assert_eq!(drop_index(&m.tab_centers, 120.0), 1);
        assert_eq!(drop_index(&m.tab_centers, 260.0), 3, "after the last tab");
        assert_eq!(drop_index(&[], 260.0), 0);
        assert!(m.is_valid());
        let mut bad = m.clone();
        bad.tab_centers = vec![f64::NAN];
        assert!(!bad.is_valid());
        bad.tab_centers = vec![0.0; MAX_TABS + 1];
        assert!(!bad.is_valid());
        // Older shells (and the 0.2.x protocol) send no tab centres.
        let json = r#"{"stripHeight":36,"interactive":[],"minimize":{"x":0,"y":0,"width":1,"height":1},"maximize":{"x":0,"y":0,"width":1,"height":1},"close":{"x":0,"y":0,"width":1,"height":1}}"#;
        let parsed: HitMap = serde_json::from_str(json).unwrap();
        assert!(parsed.tab_centers.is_empty());
        let with: HitMap =
            serde_json::from_str(&json.replace("\"interactive\"", "\"tabCenters\":[1.5,9],\"interactive\"")).unwrap();
        assert_eq!(with.tab_centers, vec![1.5, 9.0]);
    }

    #[test]
    fn regions() {
        let m = map();
        let hit = |x, y| hit_test(Some(&m), x, y, 1000.0, false);
        assert_eq!(hit(100.0, 20.0), Hit::Client, "a tab");
        assert_eq!(hit(510.0, 20.0), Hit::Client, "the + button");
        assert_eq!(hit(700.0, 20.0), Hit::Caption, "empty tab-row space");
        assert_eq!(hit(3.0, 20.0), Hit::Caption, "left of the first tab");
        assert_eq!(hit(100.0, 3.0), Hit::Top, "above a tab: top resize band");
        assert_eq!(hit(870.0, 20.0), Hit::Minimize);
        assert_eq!(hit(930.0, 20.0), Hit::Maximize);
        assert_eq!(hit(990.0, 20.0), Hit::Close);
        assert_eq!(hit(500.0, 50.0), Hit::Client, "toolbar");
        assert_eq!(hit(990.0, 50.0), Hit::Client, "toolbar under the close button");
    }

    #[test]
    fn top_band_and_corners() {
        let m = map();
        assert_eq!(hit_test(Some(&m), 2.0, 1.0, 1000.0, false), Hit::TopLeft);
        assert_eq!(hit_test(Some(&m), 995.0, 1.0, 1000.0, false), Hit::TopRight);
        assert_eq!(hit_test(Some(&m), 930.0, 2.0, 1000.0, false), Hit::Top, "resizing wins at the very edge");
        assert_eq!(hit_test(Some(&m), 930.0, 6.0, 1000.0, false), Hit::Maximize);
    }

    #[test]
    fn maximized_has_no_resize_band_and_edge_buttons() {
        let m = map();
        assert_eq!(hit_test(Some(&m), 100.0, 1.0, 1000.0, true), Hit::Caption, "no resize when maximized");
        assert_eq!(hit_test(Some(&m), 999.5, 0.0, 1000.0, true), Hit::Close, "top-right pixel closes");
        assert_eq!(hit_test(Some(&m), 930.0, 0.0, 1000.0, true), Hit::Maximize);
        assert_eq!(hit_test(Some(&m), 2.0, 0.0, 1000.0, true), Hit::Caption);
    }

    #[test]
    fn without_a_map_the_tab_row_drags() {
        assert_eq!(hit_test(None, 300.0, 20.0, 1000.0, false), Hit::Caption);
        assert_eq!(hit_test(None, 300.0, 50.0, 1000.0, false), Hit::Client);
        assert_eq!(hit_test(None, 300.0, 1.0, 1000.0, false), Hit::Top);
    }

    #[test]
    fn validation() {
        assert!(map().is_valid());
        let mut m = map();
        m.strip_height = f64::NAN;
        assert!(!m.is_valid());
        let mut m = map();
        m.interactive = vec![r(0.0, 0.0, 1.0, 1.0); MAX_RECTS + 1];
        assert!(!m.is_valid());
        let mut m = map();
        m.close.width = -1.0;
        assert!(!m.is_valid());
        let mut m = map();
        m.strip_height = 5000.0;
        assert!(!m.is_valid());
    }

    #[test]
    fn wire_format() {
        let json = r#"{"stripHeight":36,"interactive":[{"x":6,"y":6,"width":494,"height":30}],
            "minimize":{"x":862,"y":0,"width":46,"height":36},"maximize":{"x":908,"y":0,"width":46,"height":36},
            "close":{"x":954,"y":0,"width":46,"height":36}}"#;
        let m: HitMap = serde_json::from_str(json).unwrap();
        assert_eq!(m.interactive.len(), 1);
        assert!(serde_json::from_str::<HitMap>(r#"{"stripHeight":36}"#).is_err());
    }
}
