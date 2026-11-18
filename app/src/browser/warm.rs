//! 0.25.0: tabs woken ahead of a likely switch (see `lb_core::warm`). A tab hidden from the
//! engine needs its renderer to draw again before it can come on top (85–530 ms with busy
//! tabs, measured on the VM); a warm tab is shown beneath the page on screen and has drawn by
//! the time it is clicked. The tab the pointer rests on (the shell's `tabWarm`, 80 ms), the tab
//! just left, and the next one for Ctrl+Tab; at most two, each for 2.5 s.

use super::Browser;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::warm::{Slot, WARM_MS};

/// Timer (on the app window): warm tabs whose time is up go back to sleep.
pub(super) const WARM_TIMER: usize = 36;
const WARM_TICK_MS: u32 = 500;

impl Browser {
    /// The shell: the pointer rests on tab `id` (none: it left; the tab cools by its timer).
    pub(super) fn tab_warm_request(&self, id: Option<u32>) {
        if let Some(id) = id {
            self.warm_tab(id, Slot::Hover);
        }
    }

    /// Ctrl+Tab or Ctrl+Shift+Tab just ran: the tab after the new one, the same way.
    pub(super) fn warm_cycle_target(&self, forward: bool) {
        let Some(win) = self.cur_id() else { return };
        let next = self.ws.borrow().model(win).and_then(|m| m.cycle_target(forward));
        if let Some(id) = next {
            self.warm_tab(id, Slot::Next);
        }
    }

    /// Wakes tab `id` beneath the page on screen in its window, for [`WARM_MS`].
    pub(super) fn warm_tab(&self, id: TabId, slot: Slot) {
        let Some(win) = self.window_of(id) else { return };
        if self.cur_id() != Some(win) || self.frame(win).is_none_or(|f| f.window.is_minimized()) {
            return;
        }
        let active = self.ws.borrow().model(win).and_then(|m| m.active_id());
        if active == Some(id) {
            return;
        }
        // Only a live view that has painted (a discarded or never-loaded tab has nothing to wake).
        let Some((view, loaded)) = self.views.borrow().get(&id).map(|v| (v.view.clone(), v.loaded_once.get())) else {
            return;
        };
        if !loaded {
            return;
        }
        let cooled = self.warm.borrow_mut().set(slot, id, now_ms() + WARM_MS);
        // The tab that lost its slot goes back to sleep after this switch, not during it: hiding
        // it from the engine took ~5 ms of the UI thread right on the switch path (VM, 0.25.0).
        if let Some(c) = cooled {
            let weak = self.me.clone();
            crate::ffi::cef::post_ui(move || {
                if let Some(b) = weak.upgrade() {
                    if !b.warm.borrow().contains(c) {
                        b.cool_tab(c);
                    }
                }
            });
        }
        if !view.is_visible() {
            // Beneath the page on screen: the one a waiting switch still shows, else the active.
            let above = self.pending_reveal.borrow().shown_in(win).or(active).and_then(|t| self.screen_view(t));
            let Some(above) = above else { return };
            if let Some(v) = self.views.borrow().get(&id) {
                v.frame_ready.set(false);
            }
            view.show_beneath(&above);
            self.wake_tab(id);
            // Krunk's own pages have no `painted.js`: they answer two frames through DevTools.
            if self.tab(id).is_some_and(|t| lb_core::pages::Page::from_url(&t.url).is_some()) {
                let weak = self.me.clone();
                view.eval_json(
                    "new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))",
                    move |_| {
                        if let Some(b) = weak.upgrade() {
                            b.warm_frames_drawn(id);
                        }
                    },
                );
            }
        }
        if let Some(app) = self.app() {
            app.set_timer(WARM_TIMER, WARM_TICK_MS);
        }
    }

    fn warm_frames_drawn(&self, id: TabId) {
        if let Some(v) = self.views.borrow().get(&id) {
            if v.view.is_visible() {
                v.frame_ready.set(v.loaded_once.get());
            }
        }
    }

    /// Tab `id` is no longer warm: hidden from the engine again, unless it is on screen (active,
    /// waited for, or the page a switch still shows).
    fn cool_tab(&self, id: TabId) {
        let Some(win) = self.window_of(id) else { return };
        let active = self.ws.borrow().model(win).and_then(|m| m.active_id()) == Some(id);
        let on_screen = {
            let r = self.pending_reveal.borrow();
            r.is_waiting(id) || r.shown_in(win) == Some(id)
        };
        if active || on_screen {
            return;
        }
        if let Some(v) = self.tab_view(id) {
            if v.is_visible() {
                v.set_visible(false);
                if v.devtools_open() {
                    v.set_devtools_visible(false);
                }
            }
        }
        if let Some(v) = self.views.borrow().get(&id) {
            v.frame_ready.set(false);
        }
    }

    pub(super) fn warm_tick(&self) {
        let expired = self.warm.borrow_mut().expire(now_ms());
        for id in expired {
            self.cool_tab(id);
        }
        if self.warm.borrow().is_empty() {
            if let Some(app) = self.app() {
                app.kill_timer(WARM_TIMER);
            }
        }
    }
}
