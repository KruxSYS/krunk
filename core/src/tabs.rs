//! The tab model: an ordered list of tabs, the active tab, the closed-tab
//! stack and lazy loading. It knows nothing about browser views; every operation
//! returns the [`Effect`]s the host has to apply to its controllers.

use crate::pages::{self, Page};
use crate::protocol::TabInfo;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub type TabId = u32;

/// Maximum entries remembered for "reopen closed tab".
pub const CLOSED_STACK_CAP: usize = 25;

/// Whether a tab currently has a browser view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// No controller (restored from a session, or discarded); loads when activated.
    Unloaded,
    Live,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub loading: bool,
    pub private: bool,
    pub state: LoadState,
    /// The tab this one was opened from, while that relationship still
    /// matters for choosing the next active tab (cleared on manual switch).
    pub opener: Option<TabId>,
    /// Unix ms of the last time the tab was active (used by tab sleeping).
    pub last_active: i64,
    /// Live but put to sleep (low memory / suspended); shown dimmed.
    pub asleep: bool,
}

impl Tab {
    pub fn is_live(&self) -> bool {
        self.state == LoadState::Live
    }
}

/// What the host must do to its browser views after a model change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Create a controller for the tab (private tabs need an InPrivate one)
    /// and load the tab's URL.
    CreateView(TabId),
    /// Close the tab's controller.
    DestroyView(TabId),
    /// Hide `from` (if any) and show `to`.
    Activate { from: Option<TabId>, to: TabId },
}

/// Where a new tab goes in the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    End,
    /// Right after the opener and any tabs it already opened (Chrome-like).
    AfterOpener(TabId),
}

/// Which tabs "Close other tabs" / "Close tabs to the right" close, around the tab the menu
/// was opened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseMany {
    Others,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    pub index: usize,
}

/// A tab as saved in the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
}

#[derive(Debug)]
pub struct TabModel {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: TabId,
    closed: VecDeque<ClosedTab>,
}

impl Default for TabModel {
    fn default() -> Self {
        TabModel::new()
    }
}

impl TabModel {
    pub fn new() -> TabModel {
        TabModel { tabs: Vec::new(), active: None, next_id: 1, closed: VecDeque::new() }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn active(&self) -> Option<&Tab> {
        self.active.and_then(|id| self.get(id))
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    pub fn closed_count(&self) -> usize {
        self.closed.len()
    }

    fn alloc_id(&mut self) -> TabId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    fn insert_index(&self, position: Position) -> usize {
        match position {
            Position::End => self.tabs.len(),
            Position::AfterOpener(opener) => match self.index_of(opener) {
                Some(i) => {
                    // Skip past tabs already opened from the same opener.
                    let mut j = i + 1;
                    while j < self.tabs.len() && self.tabs[j].opener == Some(opener) {
                        j += 1;
                    }
                    j
                }
                None => self.tabs.len(),
            },
        }
    }

    /// Opens a tab. Background tabs that are not `live` stay unloaded until
    /// activated; active tabs are always live.
    pub fn open(
        &mut self,
        url: &str,
        private: bool,
        position: Position,
        activate: bool,
        live: bool,
        now: i64,
    ) -> (TabId, Vec<Effect>) {
        let id = self.alloc_id();
        let index = self.insert_index(position);
        let opener = match position {
            Position::AfterOpener(o) if self.get(o).is_some() => Some(o),
            _ => None,
        };
        let url = if url.is_empty() { Page::NewTab.url() } else { crate::pages::migrate_legacy_url(url) };
        let state = if live || activate { LoadState::Live } else { LoadState::Unloaded };
        self.tabs.insert(
            index,
            Tab {
                id,
                title: initial_title(&url),
                url,
                favicon: None,
                loading: state == LoadState::Live,
                private,
                state,
                opener,
                last_active: now,
                asleep: false,
            },
        );
        let mut effects = Vec::new();
        if state == LoadState::Live {
            effects.push(Effect::CreateView(id));
        }
        if activate {
            effects.extend(self.set_active(id, now, false));
        }
        (id, effects)
    }

    /// Makes `id` active (loading it first if needed). A manual switch
    /// clears opener relationships.
    pub fn activate(&mut self, id: TabId, now: i64) -> Vec<Effect> {
        self.set_active(id, now, true)
    }

    fn set_active(&mut self, id: TabId, now: i64, manual: bool) -> Vec<Effect> {
        let Some(idx) = self.index_of(id) else { return Vec::new() };
        let from = self.active;
        if from == Some(id) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if let Some(prev) = from.and_then(|f| self.get_mut(f)) {
            prev.last_active = now;
        }
        if manual {
            for t in &mut self.tabs {
                t.opener = None;
            }
        }
        let tab = &mut self.tabs[idx];
        tab.last_active = now;
        tab.asleep = false;
        if tab.state == LoadState::Unloaded {
            tab.state = LoadState::Live;
            tab.loading = true;
            effects.push(Effect::CreateView(id));
        }
        self.active = Some(id);
        effects.push(Effect::Activate { from, to: id });
        effects
    }

    /// Closes a tab. If it was active, the opener (if still known), else the
    /// right neighbour, else the left neighbour becomes active. Closing the
    /// last tab opens a fresh new-tab page.
    pub fn close(&mut self, id: TabId, now: i64) -> Vec<Effect> {
        let Some(idx) = self.index_of(id) else { return Vec::new() };
        let tab = self.tabs.remove(idx);
        let mut effects = Vec::new();
        if tab.is_live() {
            effects.push(Effect::DestroyView(id));
        }
        if !tab.private && Page::from_url(&tab.url) != Some(Page::NewTab) {
            self.closed.push_back(ClosedTab { url: tab.url.clone(), title: tab.title.clone(), index: idx });
            while self.closed.len() > CLOSED_STACK_CAP {
                self.closed.pop_front();
            }
        }
        for t in &mut self.tabs {
            if t.opener == Some(id) {
                t.opener = None;
            }
        }
        if self.active == Some(id) {
            self.active = None;
            if self.tabs.is_empty() {
                let (_, e) = self.open("", false, Position::End, true, true, now);
                effects.extend(e);
            } else {
                let next = tab
                    .opener
                    .filter(|o| self.get(*o).is_some())
                    .unwrap_or_else(|| self.tabs[idx.min(self.tabs.len() - 1)].id);
                effects.extend(self.set_active(next, now, false));
            }
        }
        effects
    }

    /// The tabs [`TabModel::close_many`] would close.
    pub fn close_many_ids(&self, id: TabId, which: CloseMany) -> Vec<TabId> {
        let Some(idx) = self.index_of(id) else { return Vec::new() };
        self.tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| match which {
                CloseMany::Others => *i != idx,
                CloseMany::Right => *i > idx,
            })
            .map(|(_, t)| t.id)
            .collect()
    }

    /// "Close other tabs" / "Close tabs to the right" of tab `id`. If the active tab is among
    /// them, `id` becomes active first (so no tab about to close gets loaded for a moment).
    /// They close right to left: each closed entry keeps its real index, and Ctrl+Shift+T
    /// brings them back in their old order.
    pub fn close_many(&mut self, id: TabId, which: CloseMany, now: i64) -> Vec<Effect> {
        let doomed = self.close_many_ids(id, which);
        let mut effects = Vec::new();
        if self.active.is_some_and(|a| doomed.contains(&a)) {
            effects.extend(self.set_active(id, now, true));
        }
        for t in doomed.into_iter().rev() {
            effects.extend(self.close(t, now));
        }
        effects
    }

    /// Ctrl+Shift+T: reopens the most recently closed tab at its old index.
    pub fn reopen_closed(&mut self, now: i64) -> Option<(TabId, Vec<Effect>)> {
        let c = self.closed.pop_back()?;
        let id = self.alloc_id();
        let index = c.index.min(self.tabs.len());
        self.tabs.insert(
            index,
            Tab {
                id,
                url: c.url,
                title: c.title,
                favicon: None,
                loading: true,
                private: false,
                state: LoadState::Live,
                opener: None,
                last_active: now,
                asleep: false,
            },
        );
        let mut effects = vec![Effect::CreateView(id)];
        effects.extend(self.set_active(id, now, true));
        Some((id, effects))
    }

    /// Duplicates a tab right after it (same URL, same private flag) and
    /// activates the copy.
    pub fn duplicate(&mut self, id: TabId, now: i64) -> Option<(TabId, Vec<Effect>)> {
        let src = self.get(id)?.clone();
        let index = self.index_of(id)? + 1;
        let new_id = self.alloc_id();
        self.tabs.insert(
            index,
            Tab {
                id: new_id,
                url: src.url,
                title: src.title,
                favicon: src.favicon,
                loading: true,
                private: src.private,
                state: LoadState::Live,
                opener: None,
                last_active: now,
                asleep: false,
            },
        );
        let mut effects = vec![Effect::CreateView(new_id)];
        effects.extend(self.set_active(new_id, now, true));
        Some((new_id, effects))
    }

    /// Removes a tab without closing it (it moves to another window): no
    /// closed-stack entry, its view stays. If it was active, the next tab is
    /// chosen as for [`TabModel::close`]; an emptied model stays empty.
    pub fn take(&mut self, id: TabId, now: i64) -> Option<(Tab, Vec<Effect>)> {
        let idx = self.index_of(id)?;
        let tab = self.tabs.remove(idx);
        for t in &mut self.tabs {
            if t.opener == Some(id) {
                t.opener = None;
            }
        }
        let mut effects = Vec::new();
        if self.active == Some(id) {
            self.active = None;
            if !self.tabs.is_empty() {
                let next = tab
                    .opener
                    .filter(|o| self.get(*o).is_some())
                    .unwrap_or_else(|| self.tabs[idx.min(self.tabs.len() - 1)].id);
                effects.extend(self.set_active(next, now, false));
            }
        }
        Some((tab, effects))
    }

    /// Inserts a tab taken from another model at `index` (clamped). Its id
    /// stays (ids are unique across a workspace).
    pub fn insert(&mut self, mut tab: Tab, index: usize, activate: bool, now: i64) -> Vec<Effect> {
        let id = tab.id;
        tab.opener = None;
        let to = index.min(self.tabs.len());
        self.tabs.insert(to, tab);
        if activate || self.active.is_none() {
            self.set_active(id, now, true)
        } else {
            Vec::new()
        }
    }

    /// The next id this model would hand out (the workspace shares one counter).
    pub(crate) fn next_id(&self) -> TabId {
        self.next_id
    }

    pub(crate) fn set_next_id(&mut self, next: TabId) {
        self.next_id = next.max(1);
    }

    /// Takes the entries closing tabs added to this model's closed stack.
    pub(crate) fn drain_closed(&mut self) -> Vec<ClosedTab> {
        self.closed.drain(..).collect()
    }

    pub(crate) fn push_closed(&mut self, c: ClosedTab) {
        self.closed.push_back(c);
    }

    /// Moves a tab to `index` (clamped). The active tab stays active.
    pub fn move_tab(&mut self, id: TabId, index: usize) -> bool {
        let Some(from) = self.index_of(id) else { return false };
        let tab = self.tabs.remove(from);
        let to = index.min(self.tabs.len());
        self.tabs.insert(to, tab);
        from != to
    }

    /// Ctrl+1..8 select by position (0-based here); Ctrl+9 selects the last tab.
    pub fn activate_index(&mut self, index: usize, now: i64) -> Vec<Effect> {
        match self.tabs.get(index) {
            Some(t) => {
                let id = t.id;
                self.activate(id, now)
            }
            None => Vec::new(),
        }
    }

    pub fn activate_last(&mut self, now: i64) -> Vec<Effect> {
        match self.tabs.last() {
            Some(t) => {
                let id = t.id;
                self.activate(id, now)
            }
            None => Vec::new(),
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab, wrapping around.
    pub fn cycle(&mut self, forward: bool, now: i64) -> Vec<Effect> {
        let n = self.tabs.len();
        if n < 2 {
            return Vec::new();
        }
        let cur = self.active.and_then(|a| self.index_of(a)).unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.activate_index(next, now)
    }

    /// Unloads a background tab's browser view, keeping its URL (tab discard).
    pub fn unload(&mut self, id: TabId) -> Vec<Effect> {
        if self.active == Some(id) {
            return Vec::new();
        }
        match self.get_mut(id) {
            Some(t) if t.is_live() => {
                t.state = LoadState::Unloaded;
                t.loading = false;
                t.asleep = false;
                // A discarded private tab keeps only its URL.
                if t.private {
                    t.favicon = None;
                    t.title = initial_title(&t.url);
                }
                vec![Effect::DestroyView(id)]
            }
            _ => Vec::new(),
        }
    }

    // ---- updates from browser view events ----

    pub fn set_url(&mut self, id: TabId, url: &str) {
        if let Some(t) = self.get_mut(id) {
            if t.url != url {
                t.url = url.to_string();
                if let Some(p) = Page::from_url(url) {
                    t.title = page_title(p).to_string();
                }
            }
        }
    }

    pub fn set_title(&mut self, id: TabId, title: &str) {
        if let Some(t) = self.get_mut(id) {
            let clean: String = title.chars().filter(|c| !c.is_control()).take(512).collect();
            t.title = if clean.trim().is_empty() { initial_title(&t.url) } else { clean };
        }
    }

    pub fn set_favicon(&mut self, id: TabId, favicon: Option<String>) {
        if let Some(t) = self.get_mut(id) {
            t.favicon = favicon.filter(|f| f.starts_with("data:image/") && f.len() <= 64 * 1024);
        }
    }

    /// Marks a live background tab as asleep (or awake again).
    pub fn set_asleep(&mut self, id: TabId, asleep: bool) {
        if let Some(t) = self.get_mut(id) {
            t.asleep = asleep && t.is_live();
        }
    }

    pub fn set_loading(&mut self, id: TabId, loading: bool) {
        if let Some(t) = self.get_mut(id) {
            t.loading = loading;
        }
    }

    // ---- UI and session ----

    /// Tab strip state for the shell.
    pub fn infos(&self) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .map(|t| TabInfo {
                id: t.id,
                title: t.title.clone(),
                url: pages::display_url(&t.url),
                favicon: t.favicon.clone(),
                loading: t.loading,
                private: t.private,
                sleeping: !t.is_live() || t.asleep,
                audible: false,
                muted: false,
            })
            .collect()
    }

    /// Non-private tabs for session restore.
    pub fn session(&self) -> Session {
        let saved: Vec<&Tab> = self.tabs.iter().filter(|t| !t.private).collect();
        let active = saved.iter().position(|t| Some(t.id) == self.active).unwrap_or(0);
        Session {
            tabs: saved
                .iter()
                .map(|t| SessionTab { url: crate::urlclean::strip_credentials(&t.url), title: t.title.clone() })
                .collect(),
            active,
        }
    }

    /// Restores a session: all tabs unloaded except the active one.
    pub fn restore(&mut self, session: &Session, now: i64) -> Vec<Effect> {
        let mut ids = Vec::new();
        for t in session.tabs.iter() {
            // Sessions saved before the rename name the old internal origin.
            let url = crate::pages::migrate_legacy_url(&t.url);
            if !restorable(&url) {
                continue;
            }
            let (id, _) = self.open(&url, false, Position::End, false, false, now);
            if let Some(tab) = self.get_mut(id) {
                if !t.title.is_empty() {
                    tab.title = t.title.chars().take(512).collect();
                }
            }
            ids.push(id);
        }
        match ids.get(session.active).or(ids.last()) {
            Some(&id) => self.set_active(id, now, false),
            None => self.open("", false, Position::End, true, true, now).1,
        }
    }
}

fn restorable(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || Page::from_url(url).is_some()
}

fn page_title(p: Page) -> &'static str {
    match p {
        Page::NewTab => "New tab",
        Page::History => "History",
        Page::Bookmarks => "Bookmarks",
        Page::Passwords => "Passwords",
        Page::Settings => "Settings",
        Page::Downloads => "Downloads",
        Page::About => "About Krunk",
        Page::Storage => "Cookies and site data",
        Page::SiteData => "Site data",
        Page::Adblock => "Ad blocking",
        Page::HttpsOnly => "Not secure",
        Page::Gpu => "Graphics",
        Page::Tasks => "Task manager",
    }
}

fn initial_title(url: &str) -> String {
    match Page::from_url(url) {
        Some(p) => page_title(p).to_string(),
        None => crate::urlclean::host_of(url).unwrap_or_else(|| url.chars().take(100).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(m: &TabModel) -> Vec<TabId> {
        m.tabs().iter().map(|t| t.id).collect()
    }

    /// Model with tabs a, b, c (ids 1, 2, 3); `active` selects the active one.
    fn three(active: usize) -> TabModel {
        let mut m = TabModel::new();
        for u in ["https://a.example/", "https://b.example/", "https://c.example/"] {
            m.open(u, false, Position::End, false, true, 0);
        }
        m.activate_index(active, 1);
        m
    }

    #[test]
    fn open_at_end_and_after_opener() {
        let mut m = three(0);
        let (d, e) = m.open("https://d.example/", false, Position::AfterOpener(1), false, true, 2);
        assert_eq!(e, vec![Effect::CreateView(d)]);
        let (e2, _) = m.open("https://e.example/", false, Position::AfterOpener(1), false, true, 2);
        // Children of the same opener keep their order after it.
        assert_eq!(ids(&m), vec![1, d, e2, 2, 3]);
        let (f, eff) = m.open("", false, Position::End, true, true, 3);
        assert_eq!(ids(&m).last(), Some(&f));
        assert_eq!(m.get(f).unwrap().url, Page::NewTab.url());
        assert_eq!(eff, vec![Effect::CreateView(f), Effect::Activate { from: Some(1), to: f }]);
    }

    #[test]
    fn close_active_picks_right_then_left() {
        let mut m = three(1);
        let e = m.close(2, 5);
        assert_eq!(e, vec![Effect::DestroyView(2), Effect::Activate { from: None, to: 3 }]);
        let e = m.close(3, 6);
        assert_eq!(e, vec![Effect::DestroyView(3), Effect::Activate { from: None, to: 1 }]);
    }

    /// a, b, c, d, e (ids 1–5); `active` selects the active one.
    fn five(active: usize) -> TabModel {
        let mut m = TabModel::new();
        for u in [
            "https://a.example/",
            "https://b.example/",
            "https://c.example/",
            "https://d.example/",
            "https://e.example/",
        ] {
            m.open(u, false, Position::End, false, true, 0);
        }
        m.activate_index(active, 1);
        m
    }

    #[test]
    fn close_others_keeps_one_and_reopens_in_order() {
        let mut m = five(4);
        assert_eq!(m.close_many_ids(3, CloseMany::Others), vec![1, 2, 4, 5]);
        let e = m.close_many(3, CloseMany::Others, 5);
        // The kept tab becomes active first, then the others go from the right.
        assert_eq!(
            e,
            vec![
                Effect::Activate { from: Some(5), to: 3 },
                Effect::DestroyView(5),
                Effect::DestroyView(4),
                Effect::DestroyView(2),
                Effect::DestroyView(1),
            ]
        );
        assert_eq!(ids(&m), vec![3]);
        assert_eq!(m.active_id(), Some(3));
        // Ctrl+Shift+T four times: a, b, d, e come back where they were.
        for _ in 0..4 {
            m.reopen_closed(6).unwrap();
        }
        let urls: Vec<_> = m.tabs().iter().map(|t| t.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://a.example/",
                "https://b.example/",
                "https://c.example/",
                "https://d.example/",
                "https://e.example/"
            ]
        );
    }

    #[test]
    fn close_right_keeps_an_active_tab_on_the_left() {
        let mut m = five(0);
        assert_eq!(m.close_many_ids(3, CloseMany::Right), vec![4, 5]);
        assert_eq!(m.close_many(3, CloseMany::Right, 5), vec![Effect::DestroyView(5), Effect::DestroyView(4)]);
        assert_eq!(ids(&m), vec![1, 2, 3]);
        assert_eq!(m.active_id(), Some(1));
        // An active tab to the right goes too: the menu's tab takes over.
        let mut m = five(4);
        let e = m.close_many(2, CloseMany::Right, 5);
        assert_eq!(e[0], Effect::Activate { from: Some(5), to: 2 });
        assert_eq!(ids(&m), vec![1, 2]);
        // Nothing to the right of the last tab; nothing else beside a single tab.
        assert!(m.close_many_ids(2, CloseMany::Right).is_empty());
        assert!(m.close_many(2, CloseMany::Right, 6).is_empty());
        let mut one = TabModel::new();
        let (only, _) = one.open("https://a.example/", false, Position::End, true, true, 0);
        assert!(one.close_many_ids(only, CloseMany::Others).is_empty());
    }

    #[test]
    fn close_many_skips_private_tabs_in_the_closed_stack() {
        let mut m = TabModel::new();
        m.open("https://a.example/", false, Position::End, true, true, 0);
        let (p, _) = m.open("https://secret.example/", true, Position::End, false, true, 0);
        let (keep, _) = m.open("https://c.example/", false, Position::End, false, true, 0);
        m.close_many(keep, CloseMany::Others, 1);
        assert!(m.get(p).is_none());
        m.reopen_closed(2).unwrap();
        assert!(m.reopen_closed(3).is_none(), "the private tab is not reopenable");
        assert!(m.tabs().iter().all(|t| t.url != "https://secret.example/"));
    }

    #[test]
    fn close_background_keeps_active() {
        let mut m = three(0);
        assert_eq!(m.close(3, 5), vec![Effect::DestroyView(3)]);
        assert_eq!(m.active_id(), Some(1));
    }

    #[test]
    fn close_returns_to_opener() {
        let mut m = three(0);
        let (child, _) = m.open("https://child.example/", false, Position::AfterOpener(1), true, true, 2);
        assert_eq!(ids(&m), vec![1, child, 2, 3]);
        let e = m.close(child, 3);
        assert_eq!(e.last(), Some(&Effect::Activate { from: None, to: 1 }));
        // After a manual switch the opener link is forgotten.
        let (child2, _) = m.open("https://c2.example/", false, Position::AfterOpener(1), true, true, 4);
        m.activate(3, 5);
        m.activate(child2, 6);
        let e = m.close(child2, 7);
        assert_eq!(e.last(), Some(&Effect::Activate { from: None, to: 2 }), "right neighbour");
    }

    #[test]
    fn closing_last_tab_opens_new_tab_page() {
        let mut m = TabModel::new();
        let (a, _) = m.open("https://a.example/", false, Position::End, true, true, 0);
        let e = m.close(a, 1);
        assert_eq!(m.len(), 1);
        let nt = m.active().unwrap();
        assert_eq!(nt.url, Page::NewTab.url());
        assert_eq!(
            e,
            vec![Effect::DestroyView(a), Effect::CreateView(nt.id), Effect::Activate { from: None, to: nt.id }]
        );
    }

    #[test]
    fn closed_stack_reopens_at_original_index() {
        let mut m = three(0);
        m.close(2, 1);
        let (id, e) = m.reopen_closed(2).unwrap();
        assert_eq!(m.index_of(id), Some(1));
        assert_eq!(m.get(id).unwrap().url, "https://b.example/");
        assert_eq!(e, vec![Effect::CreateView(id), Effect::Activate { from: Some(1), to: id }]);
        assert!(m.reopen_closed(3).is_none());
        // Index is clamped when the strip got shorter.
        let mut m = three(2);
        m.close(3, 1);
        m.close(2, 1);
        m.reopen_closed(2).unwrap();
        let (id, _) = m.reopen_closed(3).unwrap();
        assert_eq!(m.get(id).unwrap().url, "https://c.example/");
        assert_eq!(m.index_of(id), Some(2));
    }

    #[test]
    fn closed_stack_excludes_private_and_new_tab_and_is_capped() {
        let mut m = TabModel::new();
        m.open("https://keep.example/", false, Position::End, true, true, 0);
        let (p, _) = m.open("https://secret.example/", true, Position::End, false, true, 0);
        let (n, _) = m.open("", false, Position::End, false, true, 0);
        m.close(p, 1);
        m.close(n, 1);
        assert_eq!(m.closed_count(), 0);
        for i in 0..40 {
            let (id, _) = m.open(&format!("https://t{i}.example/"), false, Position::End, false, true, 0);
            m.close(id, 1);
        }
        assert_eq!(m.closed_count(), CLOSED_STACK_CAP);
        let (id, _) = m.reopen_closed(2).unwrap();
        assert_eq!(m.get(id).unwrap().url, "https://t39.example/");
    }

    #[test]
    fn move_across_active_keeps_active() {
        let mut m = three(1);
        assert!(m.move_tab(1, 2));
        assert_eq!(ids(&m), vec![2, 3, 1]);
        assert_eq!(m.active_id(), Some(2));
        assert!(m.move_tab(2, 99));
        assert_eq!(ids(&m), vec![3, 1, 2]);
        assert!(!m.move_tab(2, 2));
        assert!(!m.move_tab(42, 0));
        assert_eq!(m.active_id(), Some(2));
    }

    #[test]
    fn duplicate_places_copy_after_and_keeps_private() {
        let mut m = three(0);
        let (p, _) = m.open("https://p.example/", true, Position::End, false, true, 0);
        let (d, e) = m.duplicate(p, 1).unwrap();
        assert_eq!(ids(&m), vec![1, 2, 3, p, d]);
        assert!(m.get(d).unwrap().private);
        assert_eq!(e, vec![Effect::CreateView(d), Effect::Activate { from: Some(1), to: d }]);
        assert!(m.duplicate(999, 1).is_none());
    }

    #[test]
    fn numeric_and_cycle_selection() {
        let mut m = three(0);
        m.activate_index(2, 1);
        assert_eq!(m.active_id(), Some(3));
        assert!(m.activate_index(7, 1).is_empty(), "Ctrl+8 with 3 tabs does nothing");
        m.activate_last(1);
        assert_eq!(m.active_id(), Some(3));
        m.cycle(true, 1);
        assert_eq!(m.active_id(), Some(1), "wraps forward");
        m.cycle(false, 1);
        assert_eq!(m.active_id(), Some(3), "wraps backward");
        m.cycle(false, 1);
        assert_eq!(m.active_id(), Some(2));
        let mut one = TabModel::new();
        one.open("", false, Position::End, true, true, 0);
        assert!(one.cycle(true, 1).is_empty());
    }

    #[test]
    fn lazy_tabs_load_on_activation() {
        let mut m = TabModel::new();
        let session = Session {
            tabs: vec![
                SessionTab { url: "https://a.example/".into(), title: "A".into() },
                SessionTab { url: "javascript:alert(1)".into(), title: "x".into() },
                SessionTab { url: "https://b.example/".into(), title: "B".into() },
            ],
            active: 1,
        };
        let e = m.restore(&session, 0);
        assert_eq!(m.len(), 2, "non-restorable URL dropped");
        let b = m.tabs()[1].id;
        assert_eq!(e, vec![Effect::CreateView(b), Effect::Activate { from: None, to: b }]);
        let a = m.tabs()[0].id;
        assert_eq!(m.get(a).unwrap().state, LoadState::Unloaded);
        assert!(m.infos()[0].sleeping);
        assert_eq!(m.get(a).unwrap().title, "A");
        let e = m.activate(a, 1);
        assert_eq!(e, vec![Effect::CreateView(a), Effect::Activate { from: Some(b), to: a }]);
        // Unloading the active tab is refused; a background tab is unloaded.
        assert!(m.unload(a).is_empty());
        assert_eq!(m.unload(b), vec![Effect::DestroyView(b)]);
        assert!(m.unload(b).is_empty());
        // Closing an unloaded tab has no view to destroy.
        let e = m.close(b, 2);
        assert!(e.is_empty());
    }

    #[test]
    fn sessions_from_before_the_rename_restore() {
        let mut m = TabModel::new();
        let session = Session {
            tabs: vec![
                SessionTab { url: "https://lightbrowser.invalid/history.html".into(), title: "History".into() },
                SessionTab { url: "https://a.example/".into(), title: "A".into() },
            ],
            active: 0,
        };
        m.restore(&session, 0);
        assert_eq!(m.len(), 2);
        assert_eq!(m.tabs()[0].url, Page::History.url());
        assert_eq!(m.infos()[0].url, "krunk://history");
    }

    #[test]
    fn session_never_stores_credentials() {
        let mut m = TabModel::new();
        m.open("https://alice:secret@intranet.example/", false, Position::End, true, true, 0);
        let s = m.session();
        assert_eq!(s.tabs[0].url, "https://intranet.example/");
        assert!(!serde_json::to_string(&s).unwrap().contains("secret"));
    }

    #[test]
    fn sleeping_and_discarding() {
        let mut m = three(0);
        m.set_asleep(2, true);
        assert!(m.infos()[1].sleeping);
        m.activate(2, 5);
        assert!(!m.get(2).unwrap().asleep, "activation wakes");
        let (p, _) = m.open("https://secret.example/", true, Position::End, false, true, 6);
        m.set_title(p, "Secret page");
        m.set_favicon(p, Some("data:image/png;base64,AA".into()));
        m.unload(p);
        let t = m.get(p).unwrap();
        assert_eq!(t.url, "https://secret.example/");
        assert_eq!(t.title, "secret.example", "private discard keeps only the URL");
        assert_eq!(t.favicon, None);
        m.set_title(3, "Gamma");
        m.set_favicon(3, Some("data:image/png;base64,BB".into()));
        m.unload(3);
        assert_eq!(m.get(3).unwrap().title, "Gamma", "normal tabs keep title and favicon");
        assert!(m.get(3).unwrap().favicon.is_some());
        m.set_asleep(3, true);
        assert!(!m.get(3).unwrap().asleep, "only live tabs can be asleep");
    }

    #[test]
    fn session_skips_private_tabs() {
        let mut m = three(2);
        m.open("https://secret.example/", true, Position::End, true, true, 1);
        let s = m.session();
        assert_eq!(s.tabs.len(), 3);
        assert_eq!(s.active, 0, "active private tab is not saved; falls back to first");
        let mut m = three(2);
        assert_eq!(m.session().active, 2);
        let restored = {
            let mut r = TabModel::new();
            r.restore(&m.session(), 0);
            r
        };
        assert_eq!(restored.active().unwrap().url, "https://c.example/");
        m.close(1, 0);
        let mut empty = TabModel::new();
        empty.restore(&Session { tabs: vec![], active: 0 }, 0);
        assert_eq!(empty.active().unwrap().url, Page::NewTab.url());
    }

    #[test]
    fn updates_and_infos() {
        let mut m = TabModel::new();
        let (id, _) = m.open("https://a.example/", false, Position::End, true, true, 0);
        assert_eq!(m.get(id).unwrap().title, "a.example");
        m.set_title(id, "Hello\u{7}World");
        m.set_favicon(id, Some("data:image/png;base64,AA".into()));
        m.set_loading(id, false);
        m.set_url(id, &Page::Settings.url());
        let info = &m.infos()[0];
        assert_eq!(info.url, "krunk://settings");
        assert_eq!(info.title, "Settings");
        assert_eq!(info.favicon.as_deref(), Some("data:image/png;base64,AA"));
        assert!(!info.loading);
        m.set_title(id, "   ");
        assert_eq!(m.get(id).unwrap().title, "Settings");
        m.set_favicon(id, Some("https://evil.example/x.png".into()));
        assert_eq!(m.get(id).unwrap().favicon, None, "only data: images");
    }
}
