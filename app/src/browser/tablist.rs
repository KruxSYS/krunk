//! The tab list (0.11.0): the "▾" button the strip shows while its tabs do not fit opens every
//! tab of the window in the popup, searchable, with close buttons. It is updated in place while
//! it is open (a tab closed from it, a title that changes).

use super::Browser;
use lb_core::layout;
use lb_core::protocol::{PopupContent, PopupRect};
use lb_core::windows::WindowId;

impl Browser {
    pub(super) fn show_tab_list(&self, anchor: PopupRect) {
        let Some(win) = self.cur_id() else { return };
        let Some(content) = self.tab_list_content(win) else { return };
        let n = match &content {
            PopupContent::TabList { tabs, .. } => tabs.len(),
            _ => 0,
        };
        self.show_popup(content, Some(layout::tab_list_rect(anchor, n)));
    }

    fn tab_list_content(&self, win: WindowId) -> Option<PopupContent> {
        let (mut tabs, active) = self.ws.try_borrow().ok()?.model(win).map(|m| (m.infos(), m.active_id()))?;
        if let Ok(views) = self.views.try_borrow() {
            for t in tabs.iter_mut() {
                if let Some(v) = views.get(&t.id) {
                    t.audible = v.audio.get();
                    t.muted = v.view.audio_muted();
                }
            }
        }
        Some(PopupContent::TabList { tabs, active })
    }

    /// The window's tabs changed: an open tab list of that window follows (in place; it closes
    /// when the last tab went).
    pub(super) fn refresh_tab_list(&self, win: WindowId) {
        let showing = self.popup.try_borrow().ok().is_some_and(|p| {
            p.as_ref().is_some_and(|p| {
                p.open() && p.owner == Some(win) && matches!(p.content, Some(PopupContent::TabList { .. }))
            })
        });
        if !showing {
            return;
        }
        match self.tab_list_content(win) {
            Some(c) => self.show_popup(c, None),
            None => self.hide_popup(),
        }
    }
}
