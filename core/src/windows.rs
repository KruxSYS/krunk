//! Several browser windows, each with its own tab strip (Krunk 0.3.0).
//!
//! A [`Workspace`] owns one [`TabModel`] per window and the state they share: one counter for
//! tab ids (a tab keeps its id when it moves between windows), one closed-tab stack, and the
//! order in which windows were last used. Like the tab model it only returns [`Op`]s; the host
//! creates frames and views, moves a tab's live view to another frame, and closes frames.
//!
//! Rules:
//! - A window always has at least one tab. Closing its last tab closes the window; closing the
//!   last window ends the browser ([`Op::Exit`]).
//! - Moving a tab to another window never reloads it: a live tab's view moves along
//!   ([`Op::MoveView`]). A window emptied by a move closes.
//! - "Reopen closed tab" reopens into the current (most recently used) window.

use crate::tabs::{ClosedTab, Effect, Position, Session, SessionTab, Tab, TabId, TabModel, CLOSED_STACK_CAP};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub type WindowId = u32;

/// A window's restored (not maximized) rectangle in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// One window in the session: its tabs (as in the 0.2.x single-window session), where it
/// was and whether it was maximized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSession {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    #[serde(default)]
    pub maximized: bool,
    /// The DPI of the window's monitor when `bounds` were saved (0.15.4; absent in older
    /// sessions): on a monitor with another scale the window keeps its size in DIP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
}

/// Where a window is, for the saved session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Placement {
    pub bounds: Option<Bounds>,
    pub maximized: bool,
    pub dpi: Option<u32>,
}

/// The saved session (kv `"session"` in browser.db), version 2: every window.
///
/// `tabs` and `active` at the top level are the 0.2.x format; 0.3.0 still writes them, as a
/// copy of the window used last, so going back to 0.2.x restores that window. A session
/// without `windows` (written by 0.2.x) becomes one window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionV2 {
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub windows: Vec<WindowSession>,
    /// Index into `windows` of the window used last.
    #[serde(default)]
    pub focused: usize,
}

pub const SESSION_VERSION: u32 = 2;

impl SessionV2 {
    /// The windows to restore: v2 windows, or the 0.2.x single window.
    pub fn windows(&self) -> Vec<WindowSession> {
        if !self.windows.is_empty() {
            return self.windows.clone();
        }
        if self.tabs.is_empty() {
            return Vec::new();
        }
        vec![WindowSession { tabs: self.tabs.clone(), active: self.active, bounds: None, maximized: false, dpi: None }]
    }

    /// Which of [`SessionV2::windows`] was used last.
    pub fn focused_index(&self) -> usize {
        if self.windows.is_empty() {
            0
        } else {
            self.focused.min(self.windows.len() - 1)
        }
    }
}

/// What the host must do after a workspace change, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Create a frame (with its shell and popup) before anything is put into it. `bounds`
    /// `None` = the default placement.
    CreateWindow {
        win: WindowId,
        bounds: Option<Bounds>,
        maximized: bool,
    },
    /// Create a browser view for the tab inside the window's frame.
    CreateView {
        win: WindowId,
        tab: TabId,
    },
    DestroyView {
        win: WindowId,
        tab: TabId,
    },
    /// Hide `from` (if any) and show `to`, both in `win`.
    Activate {
        win: WindowId,
        from: Option<TabId>,
        to: TabId,
    },
    /// Move the tab's live view (and anything docked to it) from one frame to another.
    MoveView {
        tab: TabId,
        from: WindowId,
        to: WindowId,
    },
    /// Destroy an (already emptied) frame.
    CloseWindow(WindowId),
    /// The last window closed: end the browser.
    Exit,
}

#[derive(Debug)]
struct Entry {
    id: WindowId,
    model: TabModel,
}

#[derive(Debug)]
pub struct Workspace {
    windows: Vec<Entry>,
    /// Most recently used first.
    mru: Vec<WindowId>,
    next_tab: TabId,
    next_window: WindowId,
    closed: VecDeque<ClosedTab>,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace::new()
    }
}

fn ops(win: WindowId, effects: Vec<Effect>) -> impl Iterator<Item = Op> {
    effects.into_iter().map(move |e| match e {
        Effect::CreateView(tab) => Op::CreateView { win, tab },
        Effect::DestroyView(tab) => Op::DestroyView { win, tab },
        Effect::Activate { from, to } => Op::Activate { win, from, to },
    })
}

impl Workspace {
    pub fn new() -> Workspace {
        Workspace { windows: Vec::new(), mru: Vec::new(), next_tab: 1, next_window: 1, closed: VecDeque::new() }
    }

    // ---- queries ----

    /// Windows in creation order.
    pub fn window_ids(&self) -> Vec<WindowId> {
        self.windows.iter().map(|e| e.id).collect()
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn model(&self, win: WindowId) -> Option<&TabModel> {
        self.windows.iter().find(|e| e.id == win).map(|e| &e.model)
    }

    /// The window a tab is in.
    pub fn window_of(&self, tab: TabId) -> Option<WindowId> {
        self.windows.iter().find(|e| e.model.get(tab).is_some()).map(|e| e.id)
    }

    pub fn tab(&self, tab: TabId) -> Option<&Tab> {
        self.windows.iter().find_map(|e| e.model.get(tab))
    }

    /// Every tab with its window.
    pub fn tabs(&self) -> impl Iterator<Item = (WindowId, &Tab)> {
        self.windows.iter().flat_map(|e| e.model.tabs().iter().map(move |t| (e.id, t)))
    }

    /// The most recently used window (where new things open).
    pub fn current(&self) -> Option<WindowId> {
        self.mru.first().copied()
    }

    /// Windows, most recently used first.
    pub fn mru(&self) -> &[WindowId] {
        &self.mru
    }

    /// An open (not private) tab showing built-in page `page`: in the current window first,
    /// then in the other windows from the most recently used.
    pub fn find_page(&self, page: crate::pages::Page) -> Option<(WindowId, TabId)> {
        self.mru.iter().find_map(|&win| {
            let m = self.model(win)?;
            m.tabs()
                .iter()
                .find(|t| !t.private && crate::pages::Page::from_url(&t.url) == Some(page))
                .map(|t| (win, t.id))
        })
    }

    pub fn closed_count(&self) -> usize {
        self.closed.len()
    }

    /// The active tab of every window `visible` says is shown (not minimized): sleep and
    /// discard leave them alone.
    pub fn visible_active_tabs(&self, visible: impl Fn(WindowId) -> bool) -> Vec<TabId> {
        self.windows.iter().filter(|e| visible(e.id)).filter_map(|e| e.model.active_id()).collect()
    }

    // ---- changes ----

    /// Marks `win` as used most recently (it got the focus).
    pub fn focus(&mut self, win: WindowId) {
        if self.model(win).is_some() {
            self.mru.retain(|w| *w != win);
            self.mru.insert(0, win);
        }
    }

    /// Runs `f` on a window's model with the shared tab-id counter, and collects the closed
    /// tabs it produced into the shared stack.
    fn with<T>(&mut self, win: WindowId, f: impl FnOnce(&mut TabModel) -> T) -> Option<T> {
        let next = self.next_tab;
        let entry = self.windows.iter_mut().find(|e| e.id == win)?;
        entry.model.set_next_id(next);
        let r = f(&mut entry.model);
        self.next_tab = entry.model.next_id();
        for c in entry.model.drain_closed() {
            self.closed.push_back(c);
        }
        while self.closed.len() > CLOSED_STACK_CAP {
            self.closed.pop_front();
        }
        Some(r)
    }

    fn add_window(&mut self) -> WindowId {
        let id = self.next_window;
        self.next_window = self.next_window.wrapping_add(1).max(1);
        self.windows.push(Entry { id, model: TabModel::new() });
        self.mru.insert(0, id);
        id
    }

    fn remove_window(&mut self, win: WindowId) {
        self.windows.retain(|e| e.id != win);
        self.mru.retain(|w| *w != win);
    }

    /// A new window with one tab (`url` empty = a new-tab page), made current.
    pub fn open_window(
        &mut self,
        url: &str,
        private: bool,
        bounds: Option<Bounds>,
        maximized: bool,
        now: i64,
    ) -> (WindowId, TabId, Vec<Op>) {
        let win = self.add_window();
        let mut out = vec![Op::CreateWindow { win, bounds, maximized }];
        let (tab, effects) =
            self.with(win, |m| m.open(url, private, Position::End, true, true, now)).expect("the window exists");
        out.extend(ops(win, effects));
        (win, tab, out)
    }

    /// Opens a tab in `win` (see [`TabModel::open`]).
    /// 0.25.0: a tab id for a view made ahead of its tab (the pre-loaded new tab page); the tab
    /// it becomes is opened with [`Workspace::open_as`].
    pub fn reserve_tab_id(&mut self) -> TabId {
        let id = self.next_tab;
        self.next_tab = self.next_tab.wrapping_add(1).max(1);
        id
    }

    /// [`Workspace::open`] with a reserved id ([`Workspace::reserve_tab_id`]).
    #[allow(clippy::too_many_arguments)]
    pub fn open_as(
        &mut self,
        win: WindowId,
        id: TabId,
        url: &str,
        private: bool,
        position: Position,
        activate: bool,
        now: i64,
    ) -> Option<(TabId, Vec<Op>)> {
        let keep = self.next_tab;
        self.next_tab = id;
        let r = self.open(win, url, private, position, activate, now);
        self.next_tab = keep;
        r
    }

    pub fn open(
        &mut self,
        win: WindowId,
        url: &str,
        private: bool,
        position: Position,
        activate: bool,
        now: i64,
    ) -> Option<(TabId, Vec<Op>)> {
        let (tab, effects) = self.with(win, |m| m.open(url, private, position, activate, true, now))?;
        Some((tab, ops(win, effects).collect()))
    }

    /// Any change to one window's model that only yields tab effects (activate, cycle,
    /// duplicate, unload …).
    pub fn update<T>(
        &mut self,
        win: WindowId,
        f: impl FnOnce(&mut TabModel) -> (T, Vec<Effect>),
    ) -> Option<(T, Vec<Op>)> {
        let (t, effects) = self.with(win, f)?;
        Some((t, ops(win, effects).collect()))
    }

    /// Updates a tab's fields wherever it is (title, URL, favicon, loading, asleep).
    pub fn with_tab(&mut self, tab: TabId, f: impl FnOnce(&mut TabModel, TabId)) {
        if let Some(e) = self.windows.iter_mut().find(|e| e.model.get(tab).is_some()) {
            f(&mut e.model, tab);
        }
    }

    /// Closes a tab. Its window's last tab closes the window; the last window's last tab
    /// ends the browser.
    pub fn close_tab(&mut self, tab: TabId, now: i64) -> Vec<Op> {
        let Some(win) = self.window_of(tab) else { return Vec::new() };
        let last = self.model(win).is_some_and(|m| m.len() == 1);
        if !last {
            let effects = self.with(win, |m| m.close(tab, now)).unwrap_or_default();
            return ops(win, effects).collect();
        }
        // The window goes with its last tab (which still counts as closed).
        let mut out = Vec::new();
        if let Some(t) = self.with(win, |m| m.take(tab, now)).flatten().map(|(t, _)| t) {
            if t.is_live() {
                out.push(Op::DestroyView { win, tab });
            }
            if !t.private && crate::pages::Page::from_url(&t.url) != Some(crate::pages::Page::NewTab) {
                self.closed.push_back(ClosedTab { url: t.url, title: t.title, index: 0 });
                while self.closed.len() > CLOSED_STACK_CAP {
                    self.closed.pop_front();
                }
            }
        }
        out.extend(self.finish_window(win));
        out
    }

    /// Closes a whole window (its close button) and its tabs.
    pub fn close_window(&mut self, win: WindowId) -> Vec<Op> {
        let Some(m) = self.model(win) else { return Vec::new() };
        let mut out: Vec<Op> =
            m.tabs().iter().filter(|t| t.is_live()).map(|t| Op::DestroyView { win, tab: t.id }).collect();
        out.extend(self.finish_window(win));
        out
    }

    fn finish_window(&mut self, win: WindowId) -> Vec<Op> {
        self.remove_window(win);
        if self.windows.is_empty() {
            vec![Op::CloseWindow(win), Op::Exit]
        } else {
            vec![Op::CloseWindow(win)]
        }
    }

    /// Moves a tab to position `index` of window `to` (the index counts the target's tabs
    /// without the moved one) and makes it active there. Within one window this reorders.
    /// A window emptied by the move closes.
    pub fn move_tab(&mut self, tab: TabId, to: WindowId, index: usize, now: i64) -> Vec<Op> {
        let Some(from) = self.window_of(tab) else { return Vec::new() };
        if self.model(to).is_none() {
            return Vec::new();
        }
        if from == to {
            self.with(from, |m| m.move_tab(tab, index));
            return Vec::new();
        }
        let Some((t, effects)) = self.with(from, |m| m.take(tab, now)).flatten() else { return Vec::new() };
        let live = t.is_live();
        let mut out: Vec<Op> = ops(from, effects).collect();
        if live {
            out.push(Op::MoveView { tab, from, to });
        }
        let effects = self.with(to, |m| m.insert(t, index, true, now)).unwrap_or_default();
        out.extend(ops(to, effects));
        if self.model(from).is_some_and(|m| m.is_empty()) {
            self.remove_window(from);
            out.push(Op::CloseWindow(from));
        }
        self.focus(to);
        out
    }

    /// Moves a tab into a new window of its own ("Move to new window", or dragged out of the
    /// strip). Refused for a window's only tab: that is moving the window itself.
    pub fn detach(
        &mut self,
        tab: TabId,
        bounds: Option<Bounds>,
        maximized: bool,
        now: i64,
    ) -> Option<(WindowId, Vec<Op>)> {
        let from = self.window_of(tab)?;
        if self.model(from).is_some_and(|m| m.len() < 2) {
            return None;
        }
        let (t, effects) = self.with(from, |m| m.take(tab, now)).flatten()?;
        let live = t.is_live();
        let win = self.add_window();
        let mut out = vec![Op::CreateWindow { win, bounds, maximized }];
        out.extend(ops(from, effects));
        if live {
            out.push(Op::MoveView { tab, from, to: win });
        }
        let effects = self.with(win, |m| m.insert(t, 0, true, now)).unwrap_or_default();
        out.extend(ops(win, effects));
        Some((win, out))
    }

    /// Ctrl+Shift+T: reopens the most recently closed tab in the current window.
    pub fn reopen_closed(&mut self, now: i64) -> Option<(TabId, Vec<Op>)> {
        let win = self.current()?;
        let c = self.closed.pop_back()?;
        let r = self.with(win, |m| {
            m.push_closed(c);
            m.reopen_closed(now)
        });
        let (tab, effects) = r.flatten()?;
        Some((tab, ops(win, effects).collect()))
    }

    /// The session: every window with saved tabs (windows with only private tabs are left
    /// out), its place from `placement`, and the window used last.
    pub fn session(&self, placement: impl Fn(WindowId) -> Placement) -> SessionV2 {
        let mut windows = Vec::new();
        let mut focused = None;
        let current = self.current();
        for e in &self.windows {
            let s = e.model.session();
            if s.tabs.is_empty() {
                continue;
            }
            if Some(e.id) == current {
                focused = Some(windows.len());
            }
            let Placement { bounds, maximized, dpi } = placement(e.id);
            windows.push(WindowSession { tabs: s.tabs, active: s.active, bounds, maximized, dpi });
        }
        let focused = focused.unwrap_or(windows.len().saturating_sub(1));
        let (tabs, active) = windows.get(focused).map(|w| (w.tabs.clone(), w.active)).unwrap_or_default();
        SessionV2 { tabs, active, version: SESSION_VERSION, windows, focused }
    }

    /// Restores every window of a session (`fit` keeps each one on a visible monitor). The
    /// window used last is restored last, so it is in front and current. Nothing to restore:
    /// no ops.
    pub fn restore(&mut self, session: &SessionV2, now: i64, fit: impl Fn(Bounds, Option<u32>) -> Bounds) -> Vec<Op> {
        let windows = session.windows();
        let focused = session.focused_index();
        let mut order: Vec<usize> = (0..windows.len()).filter(|i| *i != focused).collect();
        if focused < windows.len() {
            order.push(focused);
        }
        let mut out = Vec::new();
        for i in order {
            let w = &windows[i];
            let s = Session { tabs: w.tabs.clone(), active: w.active };
            out.extend(self.restore_window(&s, w.bounds.map(|b| fit(b, w.dpi)), w.maximized, now).1);
        }
        out
    }

    /// A single-window session (the 0.2.x format) restored into a new window.
    pub fn restore_window(
        &mut self,
        session: &Session,
        bounds: Option<Bounds>,
        maximized: bool,
        now: i64,
    ) -> (WindowId, Vec<Op>) {
        let win = self.add_window();
        let mut out = vec![Op::CreateWindow { win, bounds, maximized }];
        let effects = self.with(win, |m| m.restore(session, now)).unwrap_or_default();
        out.extend(ops(win, effects));
        (win, out)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_reserved_id_is_used_once_by_open_as() {
        let mut ws = Workspace::default();
        let (win, _, _) = ws.open_window("https://a.example/", false, None, false, 0);
        let r = ws.reserve_tab_id();
        let (other, _) = ws.open(win, "https://b.example/", false, Position::End, false, 1).unwrap();
        assert_ne!(other, r, "a reserved id is not handed out again");
        let (id, ops) = ws.open_as(win, r, "krunk://newtab", false, Position::End, true, 2).unwrap();
        assert_eq!(id, r);
        assert!(ops.iter().any(|o| matches!(o, Op::CreateView { tab, .. } if *tab == r)));
        let (next, _) = ws.open(win, "https://c.example/", false, Position::End, false, 3).unwrap();
        assert!(next != r && next != other);
    }

    use super::*;

    fn urls(ws: &Workspace, win: WindowId) -> Vec<String> {
        ws.model(win).unwrap().tabs().iter().map(|t| t.url.clone()).collect()
    }

    /// One window with tabs a, b, c; c active.
    fn ws3() -> (Workspace, WindowId, Vec<TabId>) {
        let mut ws = Workspace::new();
        let (w, a, _) = ws.open_window("https://a.example/", false, None, false, 0);
        let (b, _) = ws.open(w, "https://b.example/", false, Position::End, true, 0).unwrap();
        let (c, _) = ws.open(w, "https://c.example/", false, Position::End, true, 0).unwrap();
        (ws, w, vec![a, b, c])
    }

    #[test]
    fn find_page_prefers_the_current_window_and_skips_private_tabs() {
        use crate::pages::Page;
        let (mut ws, w1, _) = ws3();
        assert_eq!(ws.find_page(Page::Downloads), None);
        let (d1, _) = ws.open(w1, &Page::Downloads.url(), false, Position::End, false, 0).unwrap();
        let (w2, _, _) = ws.open_window("https://x.example/", false, None, false, 0);
        // A private tab on the page does not count.
        ws.open(w2, &Page::Downloads.url(), true, Position::End, false, 0).unwrap();
        assert_eq!(ws.current(), Some(w2));
        assert_eq!(ws.find_page(Page::Downloads), Some((w1, d1)), "the other window's tab");
        let (d2, _) = ws.open(w2, &Page::Downloads.url(), false, Position::End, false, 0).unwrap();
        assert_eq!(ws.find_page(Page::Downloads), Some((w2, d2)), "the current window first");
        ws.focus(w1);
        assert_eq!(ws.find_page(Page::Downloads), Some((w1, d1)));
        assert_eq!(ws.find_page(Page::History), None);
        assert!(Page::Downloads.single_instance() && Page::Settings.single_instance());
        assert!(!Page::NewTab.single_instance() && !Page::SiteData.single_instance());
    }

    #[test]
    fn ids_are_unique_across_windows() {
        let (mut ws, _, ids) = ws3();
        let (w2, d, _) = ws.open_window("https://d.example/", false, None, false, 0);
        let (e, _) = ws.open(w2, "https://e.example/", false, Position::End, true, 0).unwrap();
        let mut all: Vec<TabId> = ids.clone();
        all.extend([d, e]);
        let mut dedup = all.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(dedup.len(), all.len());
        assert_eq!(ws.window_of(e), Some(w2));
        assert_eq!(ws.window_of(ids[0]), Some(1));
        assert_eq!(ws.tabs().count(), 5);
    }

    #[test]
    fn detach_moves_a_live_tab_without_closing_it() {
        let (mut ws, w, ids) = ws3();
        let b = ids[1];
        // b is a background tab: c stays active in the source.
        let (w2, out) = ws.detach(b, Some(Bounds { x: 10, y: 20, width: 800, height: 600 }), false, 5).unwrap();
        assert_eq!(
            out,
            vec![
                Op::CreateWindow {
                    win: w2,
                    bounds: Some(Bounds { x: 10, y: 20, width: 800, height: 600 }),
                    maximized: false
                },
                Op::MoveView { tab: b, from: w, to: w2 },
                Op::Activate { win: w2, from: None, to: b },
            ]
        );
        assert_eq!(urls(&ws, w), vec!["https://a.example/", "https://c.example/"]);
        assert_eq!(urls(&ws, w2), vec!["https://b.example/"]);
        assert_eq!(ws.closed_count(), 0, "moving is not closing");
        assert_eq!(ws.current(), Some(w2));
        // Detaching the active tab activates its neighbour in the source first.
        let c = ids[2];
        let (w3, out) = ws.detach(c, None, false, 6).unwrap();
        assert!(out.contains(&Op::Activate { win: w, from: None, to: ids[0] }));
        assert!(out.contains(&Op::MoveView { tab: c, from: w, to: w3 }));
    }

    #[test]
    fn a_windows_only_tab_cannot_be_detached() {
        let mut ws = Workspace::new();
        let (_, t, _) = ws.open_window("https://a.example/", false, None, false, 0);
        assert!(ws.detach(t, None, false, 1).is_none());
        assert_eq!(ws.len(), 1);
    }

    #[test]
    fn move_between_windows_lands_at_the_index_and_activates() {
        let (mut ws, w, ids) = ws3();
        let (w2, d, _) = ws.open_window("https://d.example/", false, None, false, 0);
        ws.open(w2, "https://e.example/", false, Position::End, true, 0).unwrap();
        let out = ws.move_tab(ids[0], w2, 1, 9);
        assert!(out.contains(&Op::MoveView { tab: ids[0], from: w, to: w2 }));
        assert!(out.iter().any(|o| matches!(o, Op::Activate { win, to, .. } if *win == w2 && *to == ids[0])));
        assert_eq!(urls(&ws, w2), vec!["https://d.example/", "https://a.example/", "https://e.example/"]);
        assert_eq!(ws.model(w2).unwrap().active_id(), Some(ids[0]));
        assert_eq!(ws.current(), Some(w2));
        assert_eq!(ws.window_of(d), Some(w2));
        // Reordering inside one window.
        assert!(ws.move_tab(ids[1], w, 0, 9).is_empty());
        assert_eq!(urls(&ws, w), vec!["https://b.example/", "https://c.example/"]);
    }

    #[test]
    fn merging_a_windows_only_tab_closes_that_window() {
        let (mut ws, w, _) = ws3();
        let (w2, d, _) = ws.open_window("https://d.example/", false, None, false, 0);
        let out = ws.move_tab(d, w, 3, 1);
        assert_eq!(out.last(), Some(&Op::CloseWindow(w2)));
        assert!(out.contains(&Op::MoveView { tab: d, from: w2, to: w }));
        assert!(!out.contains(&Op::Exit));
        assert_eq!(ws.len(), 1);
        assert_eq!(urls(&ws, w).last().map(String::as_str), Some("https://d.example/"));
        assert_eq!(ws.mru(), &[w]);
    }

    #[test]
    fn unloaded_tabs_move_without_a_view() {
        let (mut ws, w, ids) = ws3();
        ws.update(w, |m| ((), m.unload(ids[0])));
        let (w2, out) = ws.detach(ids[0], None, false, 3).unwrap();
        assert!(!out.iter().any(|o| matches!(o, Op::MoveView { .. })));
        // Activating it in its new window loads it there.
        assert!(out.contains(&Op::CreateView { win: w2, tab: ids[0] }));
    }

    #[test]
    fn closing_the_last_tab_closes_the_window_and_the_last_window_exits() {
        let (mut ws, w, ids) = ws3();
        let (w2, d, _) = ws.open_window("https://d.example/", false, None, false, 0);
        assert_eq!(ws.close_tab(d, 1), vec![Op::DestroyView { win: w2, tab: d }, Op::CloseWindow(w2)]);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws.closed_count(), 1, "the tab counts as closed");
        ws.close_tab(ids[0], 2);
        ws.close_tab(ids[1], 2);
        let out = ws.close_tab(ids[2], 3);
        assert_eq!(out, vec![Op::DestroyView { win: w, tab: ids[2] }, Op::CloseWindow(w), Op::Exit]);
        assert!(ws.is_empty());
    }

    #[test]
    fn closing_a_window_destroys_its_views() {
        let (mut ws, w, ids) = ws3();
        let (w2, _, _) = ws.open_window("https://d.example/", false, None, false, 0);
        let out = ws.close_window(w);
        assert_eq!(out.len(), 4);
        assert!(ids.iter().all(|t| out.contains(&Op::DestroyView { win: w, tab: *t })));
        assert_eq!(out.last(), Some(&Op::CloseWindow(w)));
        assert_eq!(ws.current(), Some(w2));
        assert_eq!(ws.close_window(w2).last(), Some(&Op::Exit));
    }

    #[test]
    fn mru_and_reopen_into_the_current_window() {
        let (mut ws, w, ids) = ws3();
        let (w2, _, _) = ws.open_window("https://d.example/", false, None, false, 0);
        assert_eq!(ws.mru(), &[w2, w]);
        ws.focus(w);
        assert_eq!(ws.current(), Some(w));
        // Close a tab in w, focus w2, reopen: it comes back in w2.
        ws.close_tab(ids[1], 1);
        ws.focus(w2);
        let (t, out) = ws.reopen_closed(2).unwrap();
        assert_eq!(ws.window_of(t), Some(w2));
        assert!(out.contains(&Op::CreateView { win: w2, tab: t }));
        assert_eq!(ws.tab(t).unwrap().url, "https://b.example/");
        assert!(ws.reopen_closed(3).is_none());
        ws.focus(99);
        assert_eq!(ws.current(), Some(w2), "unknown windows are ignored");
    }

    #[test]
    fn private_tabs_stay_private_when_moved() {
        let (mut ws, w, _) = ws3();
        let (p, _) = ws.open(w, "https://secret.example/", true, Position::End, true, 0).unwrap();
        let (w2, _) = ws.detach(p, None, false, 1).unwrap();
        assert!(ws.model(w2).unwrap().get(p).unwrap().private);
        // Closing it there doesn't put it on the closed stack (and closes the window).
        let before = ws.closed_count();
        ws.close_tab(p, 2);
        assert_eq!(ws.closed_count(), before);
        assert_eq!(ws.len(), 1);
    }

    #[test]
    fn visible_active_tabs_and_updates() {
        let (mut ws, w, ids) = ws3();
        let (w2, d, _) = ws.open_window("https://d.example/", false, None, false, 0);
        assert_eq!(ws.visible_active_tabs(|_| true), vec![ids[2], d]);
        assert_eq!(ws.visible_active_tabs(|x| x != w2), vec![ids[2]]);
        ws.with_tab(d, |m, t| m.set_title(t, "Dee"));
        assert_eq!(ws.tab(d).unwrap().title, "Dee");
        let (_, out) = ws.update(w, |m| ((), m.activate(ids[0], 1))).unwrap();
        assert_eq!(out, vec![Op::Activate { win: w, from: Some(ids[2]), to: ids[0] }]);
    }

    fn tab(url: &str) -> SessionTab {
        SessionTab { url: url.into(), title: url.trim_start_matches("https://").trim_end_matches('/').into() }
    }

    #[test]
    fn a_0_2_1_session_becomes_one_window() {
        // Exactly what Krunk 0.2.1 wrote (built-in pages by their internal URL).
        let json = r#"{"tabs":[{"url":"https://a.example/","title":"A"},{"url":"https://krunk.invalid/settings.html","title":"Settings"},{"url":"https://b.example/","title":"B"}],"active":2}"#;
        let s: SessionV2 = serde_json::from_str(json).unwrap();
        let windows = s.windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].tabs.len(), 3);
        assert_eq!((windows[0].active, windows[0].bounds, windows[0].maximized), (2, None, false));
        let mut ws = Workspace::new();
        let out = ws.restore(&s, 0, |b, _| b);
        assert_eq!(ws.len(), 1);
        let w = ws.current().unwrap();
        assert_eq!(out[0], Op::CreateWindow { win: w, bounds: None, maximized: false });
        assert_eq!(
            urls(&ws, w),
            vec!["https://a.example/", "https://krunk.invalid/settings.html", "https://b.example/"]
        );
        assert_eq!(ws.model(w).unwrap().active().unwrap().url, "https://b.example/");
        // Nothing at all (or garbage): nothing to restore.
        assert!(SessionV2::default().windows().is_empty());
        assert!(Workspace::new().restore(&SessionV2::default(), 0, |b, _| b).is_empty());
    }

    #[test]
    fn multi_window_sessions_round_trip() {
        let (mut ws, w1, _) = ws3();
        let (w2, _, _) = ws.open_window("https://d.example/", false, None, false, 0);
        // A window with only a private tab is not saved.
        let (w3, _, _) = ws.open_window("https://p.example/", true, None, false, 0);
        ws.focus(w1);
        let place = |w: WindowId| match w {
            1 => Placement {
                bounds: Some(Bounds { x: 10, y: 20, width: 900, height: 700 }),
                maximized: false,
                dpi: Some(144),
            },
            _ => Placement {
                bounds: Some(Bounds { x: 1930, y: 40, width: 800, height: 600 }),
                maximized: true,
                dpi: None,
            },
        };
        let s = ws.session(place);
        assert_eq!(s.version, SESSION_VERSION);
        assert_eq!(s.windows.len(), 2, "the private-only window {w3} is left out");
        assert_eq!(s.focused, 0, "window 1 was used last");
        assert_eq!(s.tabs, s.windows[0].tabs, "the 0.2.x copy is the window used last");
        assert_eq!(s.active, s.windows[0].active);
        assert!(s.windows[1].maximized);
        assert_eq!((s.windows[0].dpi, s.windows[1].dpi), (Some(144), None));
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json.matches("\"dpi\"").count(), 1, "only where it is known: {json}");
        // A session written by 0.15.3 (no dpi) still reads.
        let old_format: SessionV2 = serde_json::from_str(&json.replace(",\"dpi\":144", "")).unwrap();
        assert_eq!(old_format.windows[0].dpi, None);
        assert_eq!(old_format.windows[0].bounds, s.windows[0].bounds);
        // 0.2.x reads only tabs/active: that still parses as its Session.
        let old: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(old.tabs.len(), 3);

        let back: SessionV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        let mut fresh = Workspace::new();
        // The second monitor is gone: fit moves that window.
        let seen = std::cell::RefCell::new(Vec::new());
        let out = fresh.restore(&back, 0, |b, dpi| {
            seen.borrow_mut().push(dpi);
            if b.x > 1900 {
                Bounds { x: 0, ..b }
            } else {
                b
            }
        });
        assert_eq!(*seen.borrow(), vec![None, Some(144)], "each window's bounds come with the DPI they were saved at");
        let created: Vec<&Op> = out.iter().filter(|o| matches!(o, Op::CreateWindow { .. })).collect();
        assert_eq!(created.len(), 2);
        // The window used last comes last (in front) and is current.
        assert!(matches!(
            created[0],
            Op::CreateWindow { bounds: Some(Bounds { x: 0, y: 40, .. }), maximized: true, .. }
        ));
        assert!(matches!(
            created[1],
            Op::CreateWindow { bounds: Some(Bounds { x: 10, y: 20, .. }), maximized: false, .. }
        ));
        let cur = fresh.current().unwrap();
        assert_eq!(fresh.model(cur).unwrap().len(), 3);
        let other = *fresh.mru().last().unwrap();
        assert_eq!(urls(&fresh, other), vec!["https://d.example/"]);
        let _ = (w2, tab("https://x/"));
    }

    #[test]
    fn single_window_sessions_restore() {
        let mut ws = Workspace::new();
        let s = Session {
            tabs: vec![
                crate::tabs::SessionTab { url: "https://a.example/".into(), title: "A".into() },
                crate::tabs::SessionTab { url: "https://b.example/".into(), title: "B".into() },
            ],
            active: 1,
        };
        let (w, out) = ws.restore_window(&s, None, false, 0);
        assert_eq!(out[0], Op::CreateWindow { win: w, bounds: None, maximized: false });
        assert_eq!(urls(&ws, w), vec!["https://a.example/", "https://b.example/"]);
        // Only the active tab loads.
        assert_eq!(out.iter().filter(|o| matches!(o, Op::CreateView { .. })).count(), 1);
    }
}
