//! DevTools docked inside its tab's window (Krunk 0.3.0): a child window of the frame beside
//! the page (right or bottom), with a splitter between them. Each tab has its own DevTools:
//! switching tabs hides it and shows it again, closing the tab closes it, moving the tab to
//! another window takes it along (`View::reparent`), and page full screen hides it (F11
//! keeps it, as in Chrome). The dock side and the sizes are in the settings.

use super::{Browser, Frame};
use crate::log::{info, warning};
use lb_core::layout::{self, DevToolsSplit};
use lb_core::protocol::Event;
use lb_core::settings::DevToolsDock;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;

/// The splitter answers the mouse this far beyond its edges (physical px).
const SPLITTER_SLACK: i32 = 2;

impl Browser {
    /// F12 / Ctrl+Shift+I / the menu: DevTools for the active tab, or closes it.
    pub(super) fn toggle_devtools(&self) {
        let Some(tab) = self.active_id() else { return };
        if self.devtools_shown(tab) {
            self.close_devtools(tab);
        } else {
            self.open_devtools(tab, None);
        }
    }

    /// Does tab `tab` have DevTools (open or opening)?
    pub(super) fn devtools_shown(&self, tab: TabId) -> bool {
        self.views.try_borrow().ok().and_then(|v| v.get(&tab).map(|v| v.devtools.get())).unwrap_or(false)
    }

    /// Opens (or focuses) DevTools docked next to tab `tab`; `at` (page DIP) selects the element
    /// there (Ctrl+Shift+C, "Inspect").
    pub(super) fn open_devtools(&self, tab: TabId, at: Option<(i32, i32)>) {
        let Some(win) = self.window_of(tab) else { return };
        let (Some(frame), Some(view)) = (self.frame(win), self.tab_view(tab)) else { return };
        if let Some(v) = self.views.borrow().get(&tab) {
            v.devtools.set(true);
        }
        // Place it before it exists (CEF uses only the size from the creation rect).
        if let Some(split) = self.devtools_split_of(&frame) {
            let d = split.devtools;
            view.set_devtools_bounds(d.x, d.y, d.width, d.height);
        }
        let active = self.ws.borrow().model(win).is_some_and(|m| m.active_id() == Some(tab));
        view.set_devtools_visible(active && !self.is_page_fullscreen_frame(win));
        view.show_devtools_docked(frame.window.hwnd(), at);
        info!(
            "tab {tab}: DevTools docked {:?}{}",
            self.settings.borrow().devtools_dock,
            if at.is_some() { " (inspect element)" } else { "" }
        );
        self.layout_frame(&frame);
        self.send_devtools_state(win);
    }

    pub(super) fn close_devtools(&self, tab: TabId) {
        if let Some(view) = self.tab_view(tab) {
            view.close_devtools();
        }
    }

    /// The engine reports the docked DevTools of `tab` open or closed.
    pub(super) fn devtools_changed(&self, tab: TabId, open: bool) {
        if let Some(v) = self.views.borrow().get(&tab) {
            v.devtools.set(open);
        }
        info!("tab {tab}: DevTools {}", if open { "open" } else { "closed" });
        let Some(win) = self.window_of(tab) else { return };
        if let Some(f) = self.frame(win) {
            self.layout_frame(&f);
        }
        self.send_devtools_state(win);
        if !open
            && self.ws.borrow().model(win).is_some_and(|m| m.active_id() == Some(tab))
            && self.cur_id() == Some(win)
        {
            self.focus_active();
        }
    }

    /// The content area of a frame split for its active tab's DevTools, if that is shown
    /// (not while the page is full screen; F11 keeps it).
    pub(super) fn devtools_split_of(&self, f: &Frame) -> Option<DevToolsSplit> {
        if self.is_page_fullscreen_frame(f.id) {
            return None;
        }
        let tab = self.ws.try_borrow().ok()?.model(f.id)?.active_id()?;
        if !self.devtools_shown(tab) {
            return None;
        }
        let w = f.window;
        let (_, content) = self.frame_areas(f);
        let s = self.settings.borrow();
        let size = match s.devtools_dock {
            DevToolsDock::Right => s.devtools_size_right,
            DevToolsDock::Bottom => s.devtools_size_bottom,
        };
        Some(layout::devtools_split(content, s.devtools_dock, size, w.scale()))
    }

    /// Tells a frame's shell whether its active tab has DevTools, for the dock/close control.
    pub(super) fn send_devtools_state(&self, win: WindowId) {
        let open = self.ws.borrow().model(win).and_then(|m| m.active_id()).is_some_and(|t| self.devtools_shown(t));
        let dock = self.settings.borrow().devtools_dock;
        self.post_shell_to(win, &Event::DevTools { open, dock });
    }

    /// The dock control: right or bottom, for every DevTools.
    pub(super) fn set_devtools_dock(&self, dock: DevToolsDock) {
        self.settings.borrow_mut().devtools_dock = dock;
        self.save_devtools_settings();
        self.layout();
        for f in self.all_frames() {
            self.send_devtools_state(f.id);
        }
    }

    pub(super) fn close_active_devtools(&self) {
        if let Some(tab) = self.active_id() {
            self.close_devtools(tab);
        }
    }

    fn save_devtools_settings(&self) {
        let s = self.settings.borrow().clone();
        if let Err(e) = self.store.borrow().save_settings(&s) {
            warning!("saving the DevTools layout: {e}");
        }
    }

    // ------------------------------------------------------------ splitter

    /// Is the client point on frame `win`'s DevTools splitter? `Some(true)` = vertical.
    pub(super) fn splitter_at(&self, win: WindowId, x: i32, y: i32) -> Option<bool> {
        let f = self.frame(win)?;
        let s = self.devtools_split_of(&f)?.splitter;
        let inside = x >= s.x - SPLITTER_SLACK
            && x < s.x + s.width + SPLITTER_SLACK
            && y >= s.y - SPLITTER_SLACK
            && y < s.y + s.height + SPLITTER_SLACK;
        inside.then(|| self.settings.borrow().devtools_dock == DevToolsDock::Right)
    }

    /// Dragging the splitter: the new size applies at once (every window, as it is one
    /// setting); it is saved when the button is released.
    pub(super) fn splitter_drag(&self, win: WindowId, x: i32, y: i32, done: bool) {
        let Some(f) = self.frame(win) else { return };
        let dock = self.settings.borrow().devtools_dock;
        let (_, content) = self.frame_areas(&f);
        let size = layout::devtools_size_at(content, dock, x, y, f.window.scale());
        {
            let mut s = self.settings.borrow_mut();
            match dock {
                DevToolsDock::Right => s.devtools_size_right = size,
                DevToolsDock::Bottom => s.devtools_size_bottom = size,
            }
        }
        self.layout();
        if done {
            info!("DevTools {dock:?} size {size:.0} DIP");
            self.save_devtools_settings();
        }
    }
}
