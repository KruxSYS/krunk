//! The download-start animation (Krunk 0.6.0): a small download glyph flies from the middle
//! of the page to the downloads button over ~450 ms, drawn with the native layered overlay
//! (the page is its own window, so the shell cannot draw over it); then the shell pulses the
//! button. With Windows' "Animation effects" off, only the pulse/badge happens.

use super::Browser;
use crate::ffi::overlay::{Overlay, OverlaySpec};
use lb_core::overlay::Run;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use std::cell::{Cell, RefCell};

pub(super) const FLIGHT_TIMER: usize = 16;
const FLIGHT_MS: f64 = 450.0;
const TICK_MS: u32 = 16;

#[derive(Default)]
pub(super) struct FlightHost {
    overlay: RefCell<Option<(WindowId, Overlay)>>,
    started: Cell<i64>,
    from: Cell<(i32, i32)>,
    to: Cell<(i32, i32)>,
    win: Cell<Option<WindowId>>,
}

/// Ease in, then out (a smooth arc of speed).
fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

impl Browser {
    /// A download just started in `tab`: fly to that window's downloads button.
    pub(super) fn download_flight(&self, tab: Option<TabId>) {
        let Some(win) = tab.and_then(|t| self.window_of(t)).or_else(|| self.cur_id()) else { return };
        if !crate::ffi::window::client_area_animation() {
            self.post_shell_to(win, &lb_core::protocol::Event::DownloadStarted);
            return;
        }
        let Some(f) = self.frame(win) else { return };
        if f.window.is_minimized() || !f.window.is_active() {
            self.post_shell_to(win, &lb_core::protocol::Event::DownloadStarted);
            return;
        }
        let (shell, content) = self.frame_areas(&f);
        let page = self.devtools_split_of(&f).map(|s| s.page).unwrap_or(content);
        let scale = f.window.scale();
        let (sx, sy) = f.window.client_to_screen(page.x + page.width / 2, page.y + page.height / 2);
        // The button from the shell's hit map, else the toolbar's right end.
        let target = f.hit_map.borrow().as_ref().map(|m| m.downloads).filter(|r| r.width > 0.0);
        let (tx, ty) = match (target, shell) {
            (Some(r), _) => f
                .window
                .client_to_screen(((r.x + r.width / 2.0) * scale) as i32, ((r.y + r.height / 2.0) * scale) as i32),
            (None, Some(s)) => {
                f.window.client_to_screen(s.x + s.width - (60.0 * scale) as i32, s.y + (56.0 * scale) as i32)
            }
            (None, None) => return self.post_shell_to(win, &lb_core::protocol::Event::DownloadStarted),
        };
        let mut slot = self.flight.overlay.borrow_mut();
        if slot.as_ref().is_none_or(|(w, _)| *w != win) {
            match Overlay::create(f.window) {
                Ok(o) => *slot = Some((win, o)),
                Err(e) => {
                    crate::log::warning!("download flight: {e}");
                    return self.post_shell_to(win, &lb_core::protocol::Event::DownloadStarted);
                }
            }
        }
        drop(slot);
        crate::log::info!("download flight: from ({sx},{sy}) to ({tx},{ty})");
        self.flight.from.set((sx, sy));
        self.flight.to.set((tx, ty));
        self.flight.win.set(Some(win));
        self.flight.started.set(lb_core::storage::now_ms());
        self.flight_tick();
        if let Some(app) = self.app() {
            app.set_timer(FLIGHT_TIMER, TICK_MS);
        }
    }

    pub(super) fn flight_tick(&self) {
        let Some(win) = self.flight.win.get() else { return self.end_flight() };
        let Some(f) = self.frame(win) else { return self.end_flight() };
        let elapsed = (lb_core::storage::now_ms() - self.flight.started.get()) as f64;
        let t = ease(elapsed / FLIGHT_MS);
        let scale = f.window.scale();
        let slot = self.flight.overlay.borrow();
        let Some((_, o)) = slot.as_ref() else { return self.end_flight() };
        let spec = OverlaySpec {
            palette: lb_core::overlay::hint_palette(),
            runs: vec![Run::Text("↓".into())],
            font_px: (22.0 * scale).round() as i32,
            pad_x: (8.0 * scale).round() as i32,
            pad_y: (4.0 * scale).round() as i32,
            radius: 14.0 * scale,
            border: 0,
        };
        let (w, h) = o.size_of(&spec);
        let (fx, fy) = self.flight.from.get();
        let (tx, ty) = self.flight.to.get();
        // A slight arc: the glyph rises first, then drops into the button.
        let x = fx as f64 + (tx - fx) as f64 * t;
        let y = fy as f64 + (ty - fy) as f64 * t - (1.0 - (2.0 * t - 1.0).powi(2)) * 40.0 * scale;
        // Shrinks a little and fades at the end.
        let alpha = if t > 0.8 { (255.0 * (1.0 - (t - 0.8) / 0.2)).max(0.0) as u8 } else { 255 };
        o.show(&spec, x as i32 - w / 2, y as i32 - h / 2, alpha);
        drop(slot);
        if elapsed >= FLIGHT_MS {
            self.end_flight();
        }
    }

    fn end_flight(&self) {
        if let Some(app) = self.app() {
            app.kill_timer(FLIGHT_TIMER);
        }
        if let Some((_, o)) = self.flight.overlay.borrow().as_ref() {
            o.hide();
        }
        if let Some(win) = self.flight.win.take() {
            self.post_shell_to(win, &lb_core::protocol::Event::DownloadStarted);
        }
    }
}
