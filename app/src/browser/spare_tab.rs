//! 0.25.0: the pre-loaded new tab page. Ctrl+T waited 280–350 ms for a new renderer and the
//! page's first paint (VM, even with one tab). One new tab page is made ahead, hidden from the
//! engine (no CPU while it waits), under a reserved tab id so its page is hooked and answered
//! like any tab's; the next plain new tab opens with that id and only has to come on top.
//!
//! It is made again after a theme change or a change of the custom background, never shown for
//! a private tab or another new-tab setting, and replaced when it is older than
//! [`MAX_AGE_MS`] (its most-visited tiles would be stale).

use super::Browser;
use crate::log::info;
use lb_core::pages::Page;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;

/// Timer (on the app window): make the pre-loaded new tab page.
pub(super) const SPARE_TAB_TIMER: usize = 37;
const PREPARE_AFTER_MS: u32 = 3000;
/// After one was used: the next soon, so tabs opened one after another all get one.
const PREPARE_AGAIN_MS: u32 = 500;
const MAX_AGE_MS: i64 = 10 * 60 * 1000;

pub(super) struct SpareTab {
    id: TabId,
    /// The frame its view lives in (it goes before that frame does).
    win: WindowId,
    dark: bool,
    background: u32,
    since: i64,
}

impl Browser {
    /// Makes the pre-loaded page a few seconds from now, if there is none.
    pub(super) fn spare_tab_schedule(&self) {
        self.spare_tab_schedule_in(PREPARE_AFTER_MS);
    }

    fn spare_tab_schedule_in(&self, ms: u32) {
        if self.spare_tab.borrow().is_some() {
            return;
        }
        if let Some(app) = self.app() {
            app.set_timer(SPARE_TAB_TIMER, ms);
        }
    }

    pub(super) fn spare_tab_timer(&self) {
        if let Some(app) = self.app() {
            app.kill_timer(SPARE_TAB_TIMER);
        }
        if self.spare_tab.borrow().is_some() {
            return;
        }
        // Only for Krunk's own new tab page (Settings → New tabs).
        if self.settings.borrow().new_tab_target().is_some() {
            return;
        }
        let Some(frame) = self.cur() else { return };
        let id = self.ws.borrow_mut().reserve_tab_id();
        let url = Page::NewTab.url();
        self.build_view(frame.id, frame.window, id, &url, false, false, true);
        if !self.views.borrow().contains_key(&id) {
            return;
        }
        info!("tab {id}: new tab page made ahead");
        *self.spare_tab.borrow_mut() = Some(SpareTab {
            id,
            win: frame.id,
            dark: self.frame_dark(),
            background: self.tab_background(),
            since: now_ms(),
        });
    }

    /// A plain new tab opening `target` (empty: Krunk's page): the pre-loaded page's id if it
    /// fits (not private, same theme and background, not too old). It is no longer the spare.
    pub(super) fn spare_tab_take(&self, target: &str, private: bool) -> Option<TabId> {
        let s = self.spare_tab.borrow_mut().take()?;
        let fits = !private
            && (target.is_empty() || Page::from_url(target) == Some(Page::NewTab))
            && s.dark == self.frame_dark()
            && s.background == self.tab_background()
            && now_ms() - s.since < MAX_AGE_MS
            && self.views.borrow().get(&s.id).is_some_and(|v| v.loaded_once.get());
        if fits {
            self.spare_tab_parent.set(Some(s.win));
            Some(s.id)
        } else {
            self.spare_tab_discard(s.id);
            self.spare_tab_schedule();
            None
        }
    }

    /// The tab opened with the pre-loaded page's id (`create_view`): its view moves into the
    /// window of the tab; the tab switch that follows brings it on top.
    pub(super) fn spare_tab_adopted(&self, win: WindowId, id: TabId, active: bool) {
        let (Some(view), Some(frame)) = (self.tab_view(id), self.frame(win)) else { return };
        view.allow_activation();
        let from = self.spare_tab_parent.take();
        if from != Some(win) && !view.reparent(frame.window.hwnd()) {
            crate::log::warning!("tab {id}: the new tab page made ahead could not move to window {win}");
        }
        if let Some(v) = self.views.borrow().get(&id) {
            v.net.set_tab(id, false);
        }
        if active {
            self.layout();
        }
        self.send_nav();
        self.spare_tab_schedule_in(PREPARE_AGAIN_MS);
    }

    /// The spare's view is closed (it never became a tab).
    pub(super) fn spare_tab_discard(&self, id: TabId) {
        self.destroy_view(id, None);
    }

    /// Theme or background changed: a new one is made with the new look.
    pub(super) fn spare_tab_refresh(&self) {
        let old = self.spare_tab.borrow_mut().take();
        if let Some(s) = old {
            self.spare_tab_discard(s.id);
        }
        self.spare_tab_schedule();
    }

    /// Frame `win` is about to close: a spare living in it goes first.
    pub(super) fn spare_tab_frame_closing(&self, win: WindowId) {
        let lives_there = self.spare_tab.borrow().as_ref().is_some_and(|s| s.win == win);
        if lives_there {
            self.spare_tab_refresh();
        }
    }
}
