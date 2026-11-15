//! The tab model (`core::tabs::TabModel`): order, active tab, opener relations, lazy restore,
//! recently closed. Kotlin owns the `GeckoSession`s and applies the returned effects
//! (create/destroy a session, switch the visible one).
use crate::error::FfiError;
use lb_core::tabs::{self, CloseMany, Effect, Position, Session};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};

/// What Kotlin must do to its sessions after a model change.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TabEffect {
    /// Create a session for the tab (private tabs in private mode) and load its URL.
    CreateView { id: u32 },
    /// Close the tab's session.
    DestroyView { id: u32 },
    /// Hide `from` (if any) and show `to`.
    Activate { from: Option<u32>, to: u32 },
}

fn effects(v: Vec<Effect>) -> Vec<TabEffect> {
    v.into_iter()
        .map(|e| match e {
            Effect::CreateView(id) => TabEffect::CreateView { id },
            Effect::DestroyView(id) => TabEffect::DestroyView { id },
            Effect::Activate { from, to } => TabEffect::Activate { from, to },
        })
        .collect()
}

/// A tab as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TabInfo {
    pub id: u32,
    /// The stored URL (internal pages as `https://krunk.invalid/...`; use `display_url`).
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub loading: bool,
    pub private: bool,
    /// Has a session (false = restored lazily or discarded; loads when activated).
    pub live: bool,
    pub asleep: bool,
    pub opener: Option<u32>,
    pub last_active: i64,
}

fn info(t: &tabs::Tab) -> TabInfo {
    TabInfo {
        id: t.id,
        url: t.url.clone(),
        title: t.title.clone(),
        favicon: t.favicon.clone(),
        loading: t.loading,
        private: t.private,
        live: t.is_live(),
        asleep: t.asleep,
        opener: t.opener,
        last_active: t.last_active,
    }
}

/// A new or reopened tab and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenResult {
    pub id: u32,
    pub effects: Vec<TabEffect>,
}

/// A recently closed tab (oldest first in `closed_tabs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    /// Where it was in the strip.
    pub index: u32,
}

/// The tab model. One per app.
#[derive(uniffi::Object)]
pub struct TabModel {
    inner: Mutex<tabs::TabModel>,
}

impl TabModel {
    fn lock(&self) -> MutexGuard<'_, tabs::TabModel> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl TabModel {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(TabModel { inner: Mutex::new(tabs::TabModel::new()) })
    }

    /// Opens a tab. `url` empty = the new-tab page. `after_opener` places it after that tab
    /// (and marks the opener, so closing it returns there). Tabs that are neither `activate`d nor
    /// `live` stay unloaded until activated.
    pub fn open(
        &self,
        url: String,
        private: bool,
        after_opener: Option<u32>,
        activate: bool,
        live: bool,
        now: i64,
    ) -> OpenResult {
        let position = match after_opener {
            Some(o) => Position::AfterOpener(o),
            None => Position::End,
        };
        let (id, e) = self.lock().open(&url, private, position, activate, live, now);
        OpenResult { id, effects: effects(e) }
    }

    pub fn activate(&self, id: u32, now: i64) -> Vec<TabEffect> {
        effects(self.lock().activate(id, now))
    }

    /// Closes a tab; closing the last one opens a new-tab page.
    pub fn close(&self, id: u32, now: i64) -> Vec<TabEffect> {
        effects(self.lock().close(id, now))
    }

    /// Closes the other tabs (`others` = true) or the tabs to the right of `id`.
    pub fn close_many(&self, id: u32, others: bool, now: i64) -> Vec<TabEffect> {
        let which = if others { CloseMany::Others } else { CloseMany::Right };
        effects(self.lock().close_many(id, which, now))
    }

    /// Ids `close_many` would close.
    pub fn close_many_ids(&self, id: u32, others: bool) -> Vec<u32> {
        let which = if others { CloseMany::Others } else { CloseMany::Right };
        self.lock().close_many_ids(id, which)
    }

    /// Reopens the most recently closed tab.
    pub fn reopen_closed(&self, now: i64) -> Option<OpenResult> {
        self.lock().reopen_closed(now).map(|(id, e)| OpenResult { id, effects: effects(e) })
    }

    /// Reopens the closed tab at `index` of `closed_tabs`.
    pub fn reopen_closed_at(&self, index: u32, now: i64) -> Option<OpenResult> {
        self.lock().reopen_closed_at(index as usize, now).map(|(id, e)| OpenResult { id, effects: effects(e) })
    }

    pub fn closed_tabs(&self) -> Vec<ClosedTab> {
        self.lock()
            .closed_tabs()
            .into_iter()
            .map(|c| ClosedTab { url: c.url, title: c.title, index: c.index as u32 })
            .collect()
    }

    /// Replaces the recently closed list (restored from `closed_tabs_json`).
    pub fn set_closed_tabs(&self, closed: Vec<ClosedTab>) {
        self.lock().set_closed_tabs(
            closed
                .into_iter()
                .map(|c| tabs::ClosedTab { url: c.url, title: c.title, index: c.index as usize })
                .collect(),
        );
    }

    pub fn clear_closed_tabs(&self) {
        self.lock().set_closed_tabs(Vec::new());
    }

    /// The recently closed list as JSON (for `kv["android.closed_tabs"]`). Written to disk, so
    /// without any `user:password@` part, like the session's addresses.
    pub fn closed_tabs_json(&self) -> String {
        let closed: Vec<ClosedTab> = self
            .closed_tabs()
            .into_iter()
            .map(|c| ClosedTab { url: lb_core::urlclean::strip_credentials(&c.url), ..c })
            .collect();
        serde_json::to_string(&closed).unwrap_or_else(|_| "[]".to_string())
    }

    /// Restores the stored list. Only addresses a session would restore come back (web pages
    /// and Krunk's own pages): a reopened tab is loaded without further checks.
    pub fn restore_closed_tabs_json(&self, json: String) -> Result<(), FfiError> {
        let closed: Vec<ClosedTab> = serde_json::from_str(&json)?;
        self.set_closed_tabs(closed.into_iter().filter(|c| restorable(&c.url)).collect());
        Ok(())
    }

    /// Duplicates a tab right after it and activates the copy.
    pub fn duplicate(&self, id: u32, now: i64) -> Option<OpenResult> {
        self.lock().duplicate(id, now).map(|(id, e)| OpenResult { id, effects: effects(e) })
    }

    /// Moves a tab to `index`; false if the id is unknown.
    pub fn move_tab(&self, id: u32, index: u32) -> bool {
        self.lock().move_tab(id, index as usize)
    }

    /// Activates the next (`forward`) or previous tab, wrapping around.
    pub fn cycle(&self, forward: bool, now: i64) -> Vec<TabEffect> {
        effects(self.lock().cycle(forward, now))
    }

    /// Discards a background tab's session (it reloads when activated).
    pub fn unload(&self, id: u32) -> Vec<TabEffect> {
        effects(self.lock().unload(id))
    }

    pub fn set_url(&self, id: u32, url: String) {
        self.lock().set_url(id, &url);
    }

    pub fn set_title(&self, id: u32, title: String) {
        self.lock().set_title(id, &title);
    }

    pub fn set_favicon(&self, id: u32, favicon: Option<String>) {
        self.lock().set_favicon(id, favicon);
    }

    pub fn set_asleep(&self, id: u32, asleep: bool) {
        self.lock().set_asleep(id, asleep);
    }

    pub fn set_loading(&self, id: u32, loading: bool) {
        self.lock().set_loading(id, loading);
    }

    pub fn tabs(&self) -> Vec<TabInfo> {
        self.lock().tabs().iter().map(info).collect()
    }

    pub fn tab(&self, id: u32) -> Option<TabInfo> {
        self.lock().get(id).map(info)
    }

    pub fn active_id(&self) -> Option<u32> {
        self.lock().active_id()
    }

    pub fn count(&self) -> u32 {
        self.lock().len() as u32
    }

    pub fn index_of(&self, id: u32) -> Option<u32> {
        self.lock().index_of(id).map(|i| i as u32)
    }

    /// The non-private tabs and the active index as JSON (`kv["session"]`, the same shape
    /// Windows stores: `{"tabs":[{"url","title"}],"active":n}`).
    ///
    /// Since 0.6.0 it also carries Windows' multi-window form (`version` 2 and one window with the
    /// web pages only): sync's `tabs` collection reads that part, so the other devices list this
    /// phone's open pages, never Krunk's own pages. Restoring reads the flat fields as before.
    pub fn session_json(&self) -> String {
        let s = self.lock().session();
        let web: Vec<&lb_core::tabs::SessionTab> = s.tabs.iter().filter(|t| is_web_page(&t.url)).collect();
        let active_url = s.tabs.get(s.active).map(|t| t.url.as_str());
        let web_active = web.iter().position(|t| Some(t.url.as_str()) == active_url).unwrap_or(0);
        serde_json::to_string(&serde_json::json!({
            "tabs": s.tabs,
            "active": s.active,
            "version": 2,
            "windows": [{ "tabs": web, "active": web_active }],
        }))
        .unwrap_or_else(|_| "{\"tabs\":[],\"active\":0}".to_string())
    }

    /// Restores a session: every tab unloaded except the active one. Also accepts Windows'
    /// multi-window form (its flat `tabs`/`active` fields).
    pub fn restore_session_json(&self, json: String, now: i64) -> Result<Vec<TabEffect>, FfiError> {
        let mut session: Session = serde_json::from_str(&json)?;
        // The core skips tabs it does not restore but takes `active` as an index into the
        // stored list: with a skipped tab before it, the wrong tab would become active. The
        // index is moved to where the active tab lands (if it is skipped itself: the next one).
        let skipped_before = session.tabs.iter().take(session.active).filter(|t| !restorable(&t.url)).count();
        session.active -= skipped_before;
        Ok(effects(self.lock().restore(&session, now)))
    }
}

/// A web page (not one of Krunk's own pages, which live at `https://krunk.invalid/`).
fn is_web_page(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && !url.starts_with("https://krunk.invalid/")
}

/// What `core::tabs` restores from a session: web pages and Krunk's own pages.
fn restorable(url: &str) -> bool {
    let url = lb_core::pages::migrate_legacy_url(url);
    url.starts_with("https://") || url.starts_with("http://") || lb_core::pages::Page::from_url(&url).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_activate() {
        let m = TabModel::new();
        let a = m.open(String::new(), false, None, true, true, 1);
        assert!(a.effects.contains(&TabEffect::CreateView { id: a.id }));
        assert_eq!(m.active_id(), Some(a.id));
        assert_eq!(crate::internal_page(m.tab(a.id).unwrap().url), Some("newtab".into()));

        let b = m.open("https://example.com/".into(), false, Some(a.id), false, false, 2);
        assert!(!m.tab(b.id).unwrap().live);
        assert_eq!(m.tab(b.id).unwrap().opener, Some(a.id));
        assert_eq!(m.count(), 2);
        let e = m.activate(b.id, 3);
        assert!(e.contains(&TabEffect::CreateView { id: b.id }));
        assert!(e.contains(&TabEffect::Activate { from: Some(a.id), to: b.id }));
        assert!(m.tab(b.id).unwrap().live);

        let e = m.close(b.id, 4);
        assert!(e.contains(&TabEffect::DestroyView { id: b.id }));
        assert_eq!(m.active_id(), Some(a.id));
        assert_eq!(m.closed_tabs().len(), 1);
        let r = m.reopen_closed(5).unwrap();
        assert_eq!(m.tab(r.id).unwrap().url, "https://example.com/");
        assert_eq!(m.closed_tabs().len(), 0);

        // Closing the last tab opens a new-tab page.
        m.close(r.id, 6);
        m.close(a.id, 7);
        assert_eq!(m.count(), 1);
        assert_eq!(crate::internal_page(m.tabs()[0].url.clone()), Some("newtab".into()));
    }

    #[test]
    fn private_tabs_stay_out_of_the_session() {
        let m = TabModel::new();
        m.open("https://a.example/".into(), false, None, true, true, 1);
        let p = m.open("https://private.example/".into(), true, None, true, true, 2);
        m.set_title(p.id, "Private".into());
        let json = m.session_json();
        assert!(json.contains("a.example"));
        assert!(!json.contains("private.example"));
        m.close(p.id, 3);
        assert!(m.closed_tabs().is_empty(), "private tabs are not remembered");
    }

    /// 0.6.0: sync's `tabs` collection reads the multi-window part (web pages only).
    #[test]
    fn the_session_carries_the_web_pages_for_sync() {
        let m = TabModel::new();
        m.open("https://krunk.invalid/newtab.html".into(), false, None, true, true, 1);
        let b = m.open("https://b.example/".into(), false, None, true, true, 2);
        m.set_title(b.id, "B".into());
        let s: lb_core::windows::SessionV2 = serde_json::from_str(&m.session_json()).unwrap();
        assert_eq!(s.version, 2);
        assert_eq!(s.tabs.len(), 2, "the flat list keeps every tab for the restore");
        assert_eq!(s.windows.len(), 1);
        let w = &s.windows[0];
        assert_eq!(w.tabs.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec!["https://b.example/"]);
        assert_eq!(w.active, 0);
    }

    #[test]
    fn session_round_trip_restores_lazily() {
        let m = TabModel::new();
        let a = m.open("https://a.example/".into(), false, None, true, true, 1);
        m.set_title(a.id, "A".into());
        let b = m.open("https://b.example/".into(), false, None, true, true, 2);
        m.set_title(b.id, "B".into());
        let json = m.session_json();

        let n = TabModel::new();
        let e = n.restore_session_json(json, 10).unwrap();
        let tabs = n.tabs();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1].title, "B");
        assert!(tabs[1].live, "the active tab loads");
        assert!(!tabs[0].live, "background tabs stay unloaded");
        assert!(e.iter().any(|x| matches!(x, TabEffect::Activate { to, .. } if *to == tabs[1].id)));

        // Windows' multi-window session has the same flat fields plus more.
        let win =
            r#"{"tabs":[{"url":"https://w.example/","title":"W"}],"active":0,"version":2,"windows":[],"focused":0}"#;
        let w = TabModel::new();
        w.restore_session_json(win.into(), 11).unwrap();
        assert_eq!(w.tabs()[0].url, "https://w.example/");
        assert!(TabModel::new().restore_session_json("nope".into(), 1).is_err());
    }

    #[test]
    fn closed_tabs_persist_as_json() {
        let m = TabModel::new();
        let a = m.open("https://a.example/".into(), false, None, true, true, 1);
        let b = m.open("https://b.example/".into(), false, None, true, true, 2);
        m.close(a.id, 3);
        m.close(b.id, 4);
        let json = m.closed_tabs_json();
        let n = TabModel::new();
        n.open(String::new(), false, None, true, true, 5);
        n.restore_closed_tabs_json(json).unwrap();
        assert_eq!(n.closed_tabs().len(), 2);
        assert_eq!(n.closed_tabs()[0].url, "https://a.example/");
        let r = n.reopen_closed_at(0, 6).unwrap();
        assert_eq!(n.tab(r.id).unwrap().url, "https://a.example/");
        assert_eq!(n.closed_tabs().len(), 1);
        n.clear_closed_tabs();
        assert!(n.closed_tabs().is_empty());
        assert!(n.restore_closed_tabs_json("x".into()).is_err());
    }

    #[test]
    fn misc_operations() {
        let m = TabModel::new();
        let a = m.open("https://a.example/".into(), false, None, true, true, 1);
        let b = m.open("https://b.example/".into(), false, None, false, false, 2);
        let c = m.open("https://c.example/".into(), false, None, false, false, 3);
        assert_eq!(m.close_many_ids(a.id, true), vec![b.id, c.id]);
        assert!(m.move_tab(c.id, 0));
        assert_eq!(m.index_of(c.id), Some(0));
        m.cycle(true, 4);
        assert_ne!(m.active_id(), Some(a.id));
        let d = m.duplicate(a.id, 5).unwrap();
        assert_eq!(m.tab(d.id).unwrap().url, "https://a.example/");
        m.set_loading(a.id, false);
        m.set_favicon(a.id, Some("data:image/png;base64,AA==".into()));
        m.set_asleep(a.id, true);
        m.set_url(a.id, "https://a.example/2".into());
        let t = m.tab(a.id).unwrap();
        assert!(!t.loading && t.asleep && t.favicon.is_some() && t.url.ends_with("/2"));
        m.activate(a.id, 6);
        let e = m.unload(d.id);
        assert!(e.contains(&TabEffect::DestroyView { id: d.id }));
        m.close_many(a.id, false, 7);
        assert!(m.count() >= 1);
    }

    #[test]
    fn restore_keeps_the_active_tab_when_an_earlier_one_is_skipped() {
        // A blank tab (not restorable) before the active one: B was active, B must be active.
        let json = r#"{"tabs":[{"url":"about:blank","title":""},{"url":"https://a.example/","title":"A"},
            {"url":"https://b.example/","title":"B"},{"url":"https://c.example/","title":"C"}],"active":2}"#;
        let m = TabModel::new();
        m.restore_session_json(json.into(), 1).unwrap();
        let tabs = m.tabs();
        assert_eq!(tabs.len(), 3);
        let active = tabs.iter().find(|t| Some(t.id) == m.active_id()).unwrap();
        assert_eq!(active.url, "https://b.example/");
        // The active tab itself skipped: the next one takes over; nothing skipped: unchanged.
        let json = r#"{"tabs":[{"url":"https://a.example/","title":"A"},{"url":"file:///x","title":""},
            {"url":"https://c.example/","title":"C"}],"active":1}"#;
        let m = TabModel::new();
        m.restore_session_json(json.into(), 1).unwrap();
        let active = m.tabs().into_iter().find(|t| Some(t.id) == m.active_id()).unwrap();
        assert_eq!(active.url, "https://c.example/");
        let json = r#"{"tabs":[{"url":"https://a.example/","title":"A"},{"url":"https://b.example/","title":"B"}],"active":1}"#;
        let m = TabModel::new();
        m.restore_session_json(json.into(), 1).unwrap();
        let active = m.tabs().into_iter().find(|t| Some(t.id) == m.active_id()).unwrap();
        assert_eq!(active.url, "https://b.example/");
    }

    #[test]
    fn the_stored_closed_list_has_no_credentials_and_only_restorable_addresses() {
        let m = TabModel::new();
        let a = m.open("https://user:secret@a.example/path".into(), false, None, true, true, 1);
        m.open("https://b.example/".into(), false, None, true, true, 2);
        m.close(a.id, 3);
        let json = m.closed_tabs_json();
        assert!(!json.contains("secret"), "{json}");
        assert!(json.contains("https://a.example/path"), "{json}");
        // A stored list is not trusted: only web pages and Krunk's own pages come back.
        let stored = r#"[{"url":"file:///data/data/org.krunk.browser/files/krunk/browser.db","title":"x","index":0},
            {"url":"javascript:alert(1)","title":"y","index":0},{"url":"https://ok.example/","title":"ok","index":0}]"#;
        let n = TabModel::new();
        n.restore_closed_tabs_json(stored.into()).unwrap();
        let urls: Vec<String> = n.closed_tabs().into_iter().map(|c| c.url).collect();
        assert_eq!(urls, vec!["https://ok.example/".to_string()]);
    }
}
