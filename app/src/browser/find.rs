//! Find in page, driven by the shell's find bar. Results arrive through the
//! tab view's find event (see `hook_tab`).

use super::Browser;
use lb_core::tabs::TabId;

impl Browser {
    /// Searches the active tab for `query`. Repeating the same query moves
    /// to the next (or previous) match.
    pub(super) fn find(&self, query: &str, backwards: bool) {
        let Some(id) = self.model.borrow().active_id() else { return };
        let Some(view) = self.tab_view(id) else { return };
        if query.is_empty() {
            self.stop_find(id);
            self.post_shell(&lb_core::protocol::Event::FindResult { active: 0, total: 0 });
            return;
        }
        let same = self.views.borrow().get(&id).is_some_and(|v| v.find_query == query);
        view.find(query, !backwards, same);
        if !same {
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
        let view = {
            let mut views = self.views.borrow_mut();
            let Some(v) = views.get_mut(&id) else { return };
            v.find_query.clear();
            v.view.clone()
        };
        view.stop_find();
    }
}
