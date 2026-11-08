//! The frameless window (hit testing against the shell's tab-row layout,
//! caption-button and window state for the shell) and keeping the keyboard
//! focus in the address bar of a new tab.

use super::Browser;
use crate::ffi::window::Hit;
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
    pub(super) fn tab_wants_focus(&self, id: TabId, from_navigation: bool) -> bool {
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
