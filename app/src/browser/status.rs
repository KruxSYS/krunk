//! The link-status label (Krunk 0.5.0): the address of the link under the mouse, shown in
//! a small label at the bottom-left of the page as Chrome and Firefox do. It is a native
//! overlay (`ffi::overlay`), so it never takes the focus and lets clicks through; it moves to
//! the bottom-right corner while the mouse is near it, is elided in the middle when long,
//! and hides when the text is empty, on a tab switch, in full screen and in pointer lock.

use super::Browser;
use crate::ffi::cursor;
use crate::ffi::overlay::{Overlay, OverlaySpec};
use lb_core::layout::Rect;
use lb_core::overlay::{self, Run};
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use std::cell::{Cell, RefCell};

/// Timer: while the label shows, see whether the mouse came near it.
pub(super) const STATUS_TIMER: usize = 14;
const STATUS_TICK_MS: u32 = 100;
/// The label flips to the other corner when the cursor comes this close (DIP).
const NEAR_DIP: i32 = 24;
/// The label never takes more than this part of the page width, nor more than this many DIP.
const MAX_FRACTION: f64 = 0.6;
const MAX_WIDTH_DIP: f64 = 900.0;

#[derive(Default)]
pub(super) struct StatusHost {
    overlay: RefCell<Option<(WindowId, Overlay)>>,
    text: RefCell<String>,
    pub(super) tab: Cell<Option<TabId>>,
    on_right: Cell<bool>,
}

impl Browser {
    /// A tab's page reports its status text (the link under the mouse, or nothing).
    pub(super) fn status_message(&self, tab: TabId, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            if self.status.tab.get() == Some(tab) {
                self.hide_status();
            }
            return;
        }
        *self.status.text.borrow_mut() = text;
        self.status.tab.set(Some(tab));
        self.place_status(true);
    }

    pub(super) fn hide_status(&self) {
        self.status.tab.set(None);
        self.status.text.borrow_mut().clear();
        if let Some((_, o)) = self.status.overlay.borrow().as_ref() {
            o.hide();
        }
        if let Some(app) = self.app() {
            app.kill_timer(STATUS_TIMER);
        }
    }

    /// `STATUS_TIMER`: the mouse may have come near the label.
    pub(super) fn status_tick(&self) {
        if self.status.tab.get().is_none() {
            return self.hide_status();
        }
        self.place_status(false);
    }

    /// Draws the label for the current text at its corner of the page. `fresh`: the text
    /// changed (always redraws); otherwise only a corner change redraws.
    fn place_status(&self, fresh: bool) {
        let Some(tab) = self.status.tab.get() else { return };
        let Some(win) = self.window_of(tab) else { return self.hide_status() };
        let Some(f) = self.frame(win) else { return self.hide_status() };
        let active = self.ws.try_borrow().ok().and_then(|ws| ws.model(win).and_then(|m| m.active_id())) == Some(tab);
        if !active
            || f.window.is_minimized()
            || self.is_fullscreen_frame(win)
            || self.pointer_locked()
            || !f.window.is_active()
        {
            return self.hide_status();
        }
        // The page's own area (DevTools docked: the page part), in screen pixels.
        let (_, content) = self.frame_areas(&f);
        let page = self.devtools_split_of(&f).map(|s| s.page).unwrap_or(content);
        let (sx, sy) = f.window.client_to_screen(page.x, page.y);
        let page = Rect { x: sx, y: sy, width: page.width, height: page.height };
        if page.width < 40 || page.height < 40 {
            return self.hide_status();
        }
        let scale = f.window.scale();
        // One overlay, owned by the frame it shows in.
        let mut slot = self.status.overlay.borrow_mut();
        if slot.as_ref().is_none_or(|(w, _)| *w != win) {
            match Overlay::create(f.window) {
                Ok(o) => *slot = Some((win, o)),
                Err(e) => {
                    crate::log::warning!("status label: {e}");
                    return;
                }
            }
        }
        let Some((_, o)) = slot.as_ref() else { return };
        let font_px = (12.0 * scale).round() as i32;
        let pad_x = (8.0 * scale).round() as i32;
        let pad_y = (5.0 * scale).round() as i32;
        let max_w = (f64::from(page.width) * MAX_FRACTION).min(MAX_WIDTH_DIP * scale) - f64::from(2 * pad_x);
        let text = self.status.text.borrow().clone();
        let shown = overlay::elide_middle(&text, max_w.max(20.0), |s| f64::from(o.measure(s, font_px)));
        let spec = OverlaySpec {
            palette: overlay::status_palette(self.frame_dark()),
            runs: vec![Run::Text(shown)],
            font_px,
            pad_x,
            pad_y,
            radius: 4.0 * scale,
            border: 1,
        };
        let (w, h) = o.size_of(&spec);
        let cursor = Some(cursor::position());
        let (rect, right) = overlay::status_rect(page, w, h, cursor, (f64::from(NEAR_DIP) * scale) as i32);
        if fresh || right != self.status.on_right.get() || !o.is_visible() {
            self.status.on_right.set(right);
            o.show(&spec, rect.x, rect.y, 255);
        }
        if let Some(app) = self.app() {
            app.set_timer(STATUS_TIMER, STATUS_TICK_MS);
        }
    }

    /// Is the frame chrome dark (the theme, or the system's choice)?
    pub(super) fn frame_dark(&self) -> bool {
        match self.settings.borrow().theme {
            lb_core::settings::Theme::Dark => true,
            lb_core::settings::Theme::Light => false,
            lb_core::settings::Theme::System => crate::ffi::window::system_prefers_dark(),
        }
    }
}
