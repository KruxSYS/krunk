//! The frameless window (hit testing against the shell's tab-row layout,
//! caption-button and window state for the shell) and keeping the keyboard
//! focus in the address bar of a new tab.

use super::Browser;
use crate::ffi::window::Hit;
use crate::log::info;
use lb_core::frame::{self, HitMap};
use lb_core::pages::Page;
use lb_core::protocol::Event;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;

impl Browser {
    pub(super) fn set_hit_map(&self, win: WindowId, map: HitMap) {
        if !map.is_valid() {
            return;
        }
        let Some(f) = self.frame(win) else { return };
        *f.hit_map.borrow_mut() = Some(map);
        self.pass_shell_hit_tests(win);
    }

    /// The shell browser's windows let non-client points through to the
    /// frame. Chromium may add child windows later (after the first paint),
    /// so this runs whenever the shell reports its layout.
    pub(super) fn pass_shell_hit_tests(&self, win: WindowId) {
        let Some(f) = self.frame(win) else { return };
        let shell = f.shell.borrow().as_ref().and_then(|s| s.window_handle());
        if let Some(h) = shell {
            f.window.pass_hit_tests_through(h);
        }
    }

    /// What is at a client point (physical pixels) of frame `win`.
    pub(super) fn frame_hit_at(&self, win: WindowId, x: i32, y: i32) -> Hit {
        let Some(f) = self.frame(win) else { return Hit::Client };
        if self.is_fullscreen_frame(win) {
            return Hit::Client;
        }
        let w = f.window;
        let scale = w.scale();
        let (width, _) = w.client_size();
        let map = f.hit_map.borrow();
        frame::hit_test(
            map.as_ref(),
            f64::from(x) / scale,
            f64::from(y) / scale,
            f64::from(width) / scale,
            w.is_maximized(),
        )
    }

    /// Frame `win` is on another scale now (0.15.4): the display's scale was changed while
    /// Krunk runs, or the window went to a monitor with another one. Windows has resized the
    /// frame to the same size in DIP, but that can be the very rectangle it had (maximized on
    /// displays of the same resolution), so nothing else lays it out again.
    pub(super) fn frame_dpi_changed(&self, win: WindowId) {
        let Some(f) = self.frame(win) else { return };
        f.dpi.set(f.window.dpi());
        info!("window {win}: display scale {:.0} %", f.window.scale() * 100.0);
        // What was sized and placed for the old scale goes: it opens again in the right size.
        self.hide_popup();
        self.close_ext_popup_because("the display scale changed");
        self.hide_tab_preview();
        self.hide_status();
        self.hide_hint();
        self.layout_frame(&f);
        // Every view of the frame reads the scale again, the hidden ones too.
        let mut views: Vec<crate::ffi::cef::View> = f.shell.borrow().iter().cloned().collect();
        let tabs: Vec<TabId> =
            self.ws.borrow().model(win).map(|m| m.tabs().iter().map(|t| t.id).collect()).unwrap_or_default();
        views.extend(tabs.into_iter().filter_map(|id| self.tab_view(id)));
        views.extend(self.agent_panel_view(win));
        for v in views {
            v.scale_changed();
        }
        // The frame's own parts (the DevTools splitter and strip, the colour between views).
        f.window.invalidate();
        self.pass_shell_hit_tests(win);
        self.send_window_state();
        self.reapply_pointer();
        self.schedule_session_save();
    }

    /// The shell of frame `win` says at which scale the engine draws it (`devicePixelRatio`).
    /// That is the truth the layout has to follow. Usually it is what Krunk worked out itself
    /// (the monitor's DPI scale times Windows' "Text size"); when it is not (the setting was
    /// just changed, or the user forces a device scale factor with an engine flag), the factor
    /// is corrected and every frame is laid out again.
    pub(super) fn shell_scale_reported(&self, win: WindowId, dpr: f64) {
        let Some(f) = self.frame(win) else { return };
        if !dpr.is_finite() || !(0.25..=16.0).contains(&dpr) {
            return;
        }
        let display = f64::from(f.window.raw_dpi()) / 96.0;
        let factor = dpr / display;
        if (factor - crate::ffi::window::ui_factor()).abs() < 0.005 {
            return;
        }
        if crate::ffi::window::set_ui_factor(factor) {
            info!(
                "window {win}: the engine draws at {:.0} % (display scale {:.0} %): laying out for that",
                dpr * 100.0,
                display * 100.0
            );
            self.check_frame_scales();
        }
    }

    /// Every frame whose DPI is not the one it was laid out for is laid out again. Windows
    /// tells a frame about a new DPI (`WM_DPICHANGED`), but a maximized one first gets its new
    /// size (the taskbar grows with the scale) while it still reports the old DPI, and the
    /// rectangle suggested with the DPI message is then the one it already has.
    pub(super) fn check_frame_scales(&self) {
        for f in self.all_frames() {
            if f.window.dpi() != f.dpi.get() {
                self.frame_dpi_changed(f.id);
            }
        }
    }

    /// The displays changed: check now, and again once Windows has settled (the messages of a
    /// scale change arrive over about a second).
    pub(super) fn check_frame_scales_soon(&self) {
        // "Text size" may be what changed: it is part of every frame's scale.
        crate::ffi::window::refresh_ui_factor();
        self.check_frame_scales();
        for ms in [250, 1200] {
            let weak = self.me.clone();
            crate::ffi::cef::post_ui_delayed(ms, move || {
                if let Some(b) = weak.upgrade() {
                    crate::ffi::window::refresh_ui_factor();
                    b.check_frame_scales();
                }
            });
        }
    }

    /// Each frame's shell shows its own maximized, active and full-screen state.
    pub(super) fn send_window_state(&self) {
        for f in self.all_frames() {
            let e = Event::WindowState {
                maximized: f.window.is_maximized(),
                active: f.active.get(),
                fullscreen: self.is_fullscreen_frame(f.id),
            };
            self.post_shell_to(f.id, &e);
        }
    }

    // ------------------------------------------------------------ new-tab focus

    /// A new tab on the new-tab page was opened by the user (Ctrl+T, "+",
    /// the menu) or at start-up: its address bar keeps the focus while the
    /// page loads.
    pub(super) fn hold_address_focus(&self, id: TabId) {
        self.address_focus.set(Some(id));
        self.focus_address_bar();
    }

    /// The engine wants to focus a tab's browser. Refused while that tab's
    /// address bar holds the focus and the request comes from loading the
    /// page (CEF focuses a browser when it navigates); a click into the page
    /// (system focus) is allowed and ends the hold.
    ///
    /// 0.25.1: a view that is not a tab yet (the new tab page made ahead, built ~0.5 s after a
    /// new tab took the last one) never takes the focus: it took the keyboard from the new
    /// tab's address bar about a second after Ctrl+T. Nor does a background tab's navigation.
    pub(super) fn tab_wants_focus(&self, id: TabId, from_navigation: bool) -> bool {
        if let Ok(ws) = self.ws.try_borrow() {
            let Some(win) = ws.window_of(id) else { return false };
            if from_navigation && ws.model(win).is_some_and(|m| m.active_id() != Some(id)) {
                return false;
            }
        }
        if self.address_focus.get() != Some(id) {
            return true;
        }
        if from_navigation {
            return false;
        }
        self.address_focus.set(None);
        true
    }

    /// The held tab finished loading: put the caret back in case something
    /// moved the focus anyway.
    pub(super) fn address_focus_loaded(&self, id: TabId) {
        if self.address_focus.get() == Some(id) && self.active_id() == Some(id) {
            self.focus_address_bar();
        }
    }

    /// Ends the hold when the tab leaves the new-tab page or another tab
    /// becomes active.
    /// `about:blank` (where a tab with edited headers starts, or the Blank new-tab setting)
    /// keeps the hold (0.7.2, R2).
    pub(super) fn release_address_focus_unless(&self, id: TabId, url: &str) {
        if self.address_focus.get() == Some(id)
            && Page::from_url(url) != Some(Page::NewTab)
            && !lb_core::pages::is_blank(url)
        {
            self.address_focus.set(None);
        }
    }
}
