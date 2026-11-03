//! Find in page (ICoreWebView2Find) driven by the shell's find bar.

use super::Browser;
use lb_core::protocol::Event;
use lb_core::tabs::TabId;

impl Browser {
    /// Searches the active tab for `query`. Repeating the same query moves
    /// to the next (or previous) match.
    pub(super) fn find(&self, query: &str, backwards: bool) {
        let Some(id) = self.model.borrow().active_id() else { return };
        let Some(env) = self.env.get() else { return };
        let find = self.find_session(id);
        let Some(find) = find else {
            // Runtime without ICoreWebView2Find: report "no matches".
            self.post_shell(&Event::FindResult { active: 0, total: 0 });
            return;
        };
        let same = self.views.borrow().get(&id).is_some_and(|v| v.find_query == query);
        if same {
            if backwards {
                find.previous();
            } else {
                find.next();
            }
        } else if find.start(env, query) {
            if let Some(v) = self.views.borrow_mut().get_mut(&id) {
                v.find_query = query.to_string();
            }
        }
    }

    /// F3 / Shift+F3: continue the current search, or open the find bar.
    pub(super) fn find_step(&self, backwards: bool) {
        let has_query = self
            .model
            .borrow()
            .active_id()
            .and_then(|id| self.views.borrow().get(&id).map(|v| v.find_query.clone()))
            .filter(|q| !q.is_empty());
        match has_query {
            Some(q) => self.find(&q, backwards),
            None => self.show_find_bar(),
        }
    }

    pub(super) fn stop_find(&self, id: TabId) {
        let find = {
            let mut views = self.views.borrow_mut();
            let Some(v) = views.get_mut(&id) else { return };
            v.find_query.clear();
            v.find.clone()
        };
        if let Some(f) = find {
            f.stop();
        }
    }

    /// The tab's find object, created (with its change events) on first use.
    fn find_session(&self, id: TabId) -> Option<crate::ffi::webview::Find> {
        if let Some(f) = self.views.borrow().get(&id).and_then(|v| v.find.clone()) {
            return Some(f);
        }
        let find = self.controller(id)?.find()?;
        let weak = self.me.clone();
        let f2 = find.clone();
        find.on_change(move || {
            let Some(b) = weak.upgrade() else { return };
            if b.model.borrow().active_id() == Some(id) {
                let (active, total) = f2.result();
                b.post_shell(&Event::FindResult { active, total });
            }
        });
        if let Some(v) = self.views.borrow_mut().get_mut(&id) {
            v.find = Some(find.clone());
        }
        Some(find)
    }
}
