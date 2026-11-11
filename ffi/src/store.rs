//! The browser database (`core::storage::Store`): history, bookmarks, new-tab tiles, favicons,
//! key-value entries and the address-bar suggestions that read them. One `KrunkStore` per app,
//! shared behind a mutex (a SQLite connection is `Send` but not `Sync`).
use crate::error::FfiError;
use crate::settings::{AndroidSettings, EXTRAS_KEY};
use crate::SettingsError;
use lb_core::storage::Store;
use lb_core::{bookmarks, frecency, netscape, omnibox, protocol};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// Open database. Methods take `&self`; the lock is internal.
#[derive(uniffi::Object)]
pub struct KrunkStore {
    inner: Mutex<Store>,
}

impl KrunkStore {
    fn lock(&self) -> MutexGuard<'_, Store> {
        // A panic while holding the lock cannot leave a half-written transaction behind (SQLite
        // rolls it back), so a poisoned lock is still usable. Release builds abort on a panic.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// One history row (a visit).
/// One `kv` row for [`KrunkStore::kv_set_many`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KvEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryEntry {
    /// The visit id (for `delete_visits`).
    pub id: i64,
    pub url: String,
    pub title: String,
    /// Visit time, ms.
    pub ts: i64,
    pub visit_count: i64,
}

/// A bookmark or folder. The roots are `BOOKMARKS_BAR` (1) and `OTHER_BOOKMARKS` (2).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Bookmark {
    pub id: i64,
    pub parent: Option<i64>,
    pub is_folder: bool,
    pub title: String,
    pub url: Option<String>,
    pub position: i64,
    pub added: i64,
}

impl From<bookmarks::Bookmark> for Bookmark {
    fn from(b: bookmarks::Bookmark) -> Self {
        Bookmark {
            id: b.id,
            parent: b.parent,
            is_folder: b.is_folder,
            title: b.title,
            url: b.url,
            position: b.position,
            added: b.added,
        }
    }
}

/// Netscape HTML import result.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ImportStats {
    pub bookmarks: u64,
    pub folders: u64,
    pub skipped: u64,
}

/// A new-tab tile: a pinned shortcut or a frequently visited site.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Tile {
    pub url: String,
    pub title: String,
    pub pinned: bool,
    /// Frecency score (0 for pinned-only tiles).
    pub score: f64,
}

impl From<frecency::Tile> for Tile {
    fn from(t: frecency::Tile) -> Self {
        Tile { url: t.url, title: t.title, pinned: t.pinned, score: t.score }
    }
}

/// One address-bar suggestion row.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AddressSuggestion {
    /// Where the row goes (a search URL for `Search`).
    pub url: String,
    /// The page title, or the typed text for `Search`/`Url`.
    pub title: String,
    pub kind: SuggestionKind,
    /// What the address bar shows while the row is highlighted.
    pub fill: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestionKind {
    Search,
    Url,
    History,
    Bookmark,
    SearchSuggestion,
}

impl From<protocol::AddressSuggestion> for AddressSuggestion {
    fn from(s: protocol::AddressSuggestion) -> Self {
        AddressSuggestion {
            url: s.url,
            title: s.title,
            kind: match s.kind {
                protocol::AddressSuggestionKind::Search => SuggestionKind::Search,
                protocol::AddressSuggestionKind::Url => SuggestionKind::Url,
                protocol::AddressSuggestionKind::History => SuggestionKind::History,
                protocol::AddressSuggestionKind::Bookmark => SuggestionKind::Bookmark,
                protocol::AddressSuggestionKind::SearchSuggestion => SuggestionKind::SearchSuggestion,
            },
            fill: s.fill,
        }
    }
}

impl From<AddressSuggestion> for protocol::AddressSuggestion {
    fn from(s: AddressSuggestion) -> Self {
        protocol::AddressSuggestion {
            url: s.url,
            title: s.title,
            kind: match s.kind {
                SuggestionKind::Search => protocol::AddressSuggestionKind::Search,
                SuggestionKind::Url => protocol::AddressSuggestionKind::Url,
                SuggestionKind::History => protocol::AddressSuggestionKind::History,
                SuggestionKind::Bookmark => protocol::AddressSuggestionKind::Bookmark,
                SuggestionKind::SearchSuggestion => protocol::AddressSuggestionKind::SearchSuggestion,
            },
            fill: s.fill,
        }
    }
}

/// Inline autocomplete: `text` replaces the typed text with its tail selected, `url` is where
/// Enter goes, `index` the row it came from.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InlineCompletion {
    pub text: String,
    pub url: String,
    pub index: u32,
}

/// Address-bar suggestions for the typed text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OmniboxResult {
    pub rows: Vec<AddressSuggestion>,
    pub inline: Option<InlineCompletion>,
}

/// Adds engine suggestions (fetched by Kotlin) under the local rows: at most 5 remote rows, 10 in
/// total, duplicates of local rows dropped. `None` when nothing changes.
#[uniffi::export]
pub fn merge_search_suggestions(
    rows: Vec<AddressSuggestion>,
    typed: String,
    suggestions: Vec<String>,
    search_template: String,
) -> Option<Vec<AddressSuggestion>> {
    let entries: Vec<protocol::AddressSuggestion> = rows.into_iter().map(Into::into).collect();
    omnibox::with_search_suggestions(&entries, &typed, &suggestions, &search_template)
        .map(|v| v.into_iter().map(Into::into).collect())
}

/// Root folder ids.
#[uniffi::export]
pub fn bookmarks_bar_id() -> i64 {
    lb_core::storage::BOOKMARKS_BAR
}

#[uniffi::export]
pub fn other_bookmarks_id() -> i64 {
    lb_core::storage::OTHER_BOOKMARKS
}

#[uniffi::export]
impl KrunkStore {
    /// Opens (creating and migrating as needed) the database at `path`, e.g.
    /// `<filesDir>/krunk/browser.db`. The parent directory must exist.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, FfiError> {
        let store = Store::open(Path::new(&path))?;
        Ok(Arc::new(KrunkStore { inner: Mutex::new(store) }))
    }

    /// An in-memory database (tests).
    #[uniffi::constructor]
    pub fn in_memory() -> Result<Arc<Self>, FfiError> {
        Ok(Arc::new(KrunkStore { inner: Mutex::new(Store::open_in_memory()?) }))
    }

    // ----- key-value -----

    pub fn kv_get(&self, key: String) -> Result<Option<String>, FfiError> {
        Ok(self.lock().kv_get(&key)?)
    }

    pub fn kv_set(&self, key: String, value: String) -> Result<(), FfiError> {
        Ok(self.lock().kv_set(&key, &value)?)
    }

    /// Writes several rows in one transaction (all or none): rows that belong together, like
    /// the session and its per-tab states, can never be stored out of step.
    pub fn kv_set_many(&self, entries: Vec<KvEntry>) -> Result<(), FfiError> {
        let rows: Vec<(&str, &str)> = entries.iter().map(|e| (e.key.as_str(), e.value.as_str())).collect();
        Ok(self.lock().kv_set_many(&rows)?)
    }

    pub fn kv_delete(&self, key: String) -> Result<(), FfiError> {
        Ok(self.lock().kv_delete(&key)?)
    }

    // ----- settings -----

    /// The settings as Android uses them (shared fields from `kv["settings"]`, Android-only ones
    /// from `kv["android.settings"]`); defaults when nothing is stored.
    pub fn load_settings(&self) -> Result<AndroidSettings, FfiError> {
        let store = self.lock();
        let core = store.load_settings()?;
        let extras = store.kv_get(EXTRAS_KEY)?;
        Ok(AndroidSettings::from_core(&core, extras.as_deref()))
    }

    /// Validates and saves. Desktop-only fields already stored keep their values.
    pub fn save_settings(&self, settings: AndroidSettings) -> Result<(), SettingsError> {
        let mut store = self.lock();
        let mut core = store.load_settings()?;
        settings.apply_to_core(&mut core);
        if let Err(errors) = core.validate() {
            return Err(SettingsError::Validation {
                errors: errors
                    .into_iter()
                    .map(|e| crate::SettingsFieldError { field: e.field.to_string(), message: e.message })
                    .collect(),
            });
        }
        // Both rows or neither: a failure between them must not leave half of a change stored
        // while the caller is told that nothing was saved.
        let shared = serde_json::to_string(&core).map_err(|e| SettingsError::Database { detail: e.to_string() })?;
        let extras = settings.extras_json();
        store.kv_set_many(&[("settings", shared.as_str()), (EXTRAS_KEY, extras.as_str())])?;
        Ok(())
    }

    // ----- history -----

    /// Records a visit (the URL as returned by `visit_url`); `None` when the URL is not recorded.
    pub fn record_visit(&self, url: String, title: String, ts: i64) -> Result<Option<i64>, FfiError> {
        Ok(self.lock().record_visit(&url, &title, ts)?)
    }

    pub fn set_title(&self, url: String, title: String) -> Result<(), FfiError> {
        Ok(self.lock().set_title(&url, &title)?)
    }

    /// Visits matching `text` (empty = all), newest first, older than `before` (ms, exclusive)
    /// when given, at most `limit` (1-1000).
    pub fn query_history(&self, text: String, before: Option<i64>, limit: u32) -> Result<Vec<HistoryEntry>, FfiError> {
        Ok(self
            .lock()
            .query_history(&text, before, limit)?
            .into_iter()
            .map(|h| HistoryEntry { id: h.id, url: h.url, title: h.title, ts: h.ts, visit_count: h.visit_count })
            .collect())
    }

    pub fn delete_visits(&self, ids: Vec<i64>) -> Result<u64, FfiError> {
        Ok(self.lock().delete_visits(&ids)? as u64)
    }

    pub fn delete_url(&self, url: String) -> Result<(), FfiError> {
        Ok(self.lock().delete_url(&url)?)
    }

    /// Deletes visits with `from <= ts < to`.
    pub fn delete_history_range(&self, from: i64, to: i64) -> Result<u64, FfiError> {
        Ok(self.lock().delete_history_range(from, to)? as u64)
    }

    pub fn clear_history(&self) -> Result<(), FfiError> {
        Ok(self.lock().clear_history()?)
    }

    pub fn visit_total(&self) -> Result<i64, FfiError> {
        Ok(self.lock().visit_total()?)
    }

    // ----- address bar -----

    /// Local suggestions for the typed text (search/URL row, the typed site's pages, bookmarks,
    /// history; at most 7) and the inline completion.
    pub fn omnibox_query(&self, text: String, search_template: String) -> OmniboxResult {
        let store = self.lock();
        let (rows, inline) = omnibox::query(&store, &text, &search_template);
        OmniboxResult {
            rows: rows.into_iter().map(Into::into).collect(),
            inline: inline.map(|i| InlineCompletion { text: i.text, url: i.url, index: i.index as u32 }),
        }
    }

    // ----- bookmarks -----

    pub fn bookmark(&self, id: i64) -> Result<Option<Bookmark>, FfiError> {
        Ok(self.lock().bookmark(id)?.map(Into::into))
    }

    /// Direct children of a folder, in order.
    pub fn bookmark_children(&self, parent: i64) -> Result<Vec<Bookmark>, FfiError> {
        Ok(self.lock().children(parent)?.into_iter().map(Into::into).collect())
    }

    /// Every bookmark and folder, depth first from the two roots (Kotlin rebuilds the tree
    /// from `parent`).
    pub fn all_bookmarks(&self) -> Result<Vec<Bookmark>, FfiError> {
        fn walk(nodes: Vec<bookmarks::BookmarkNode>, out: &mut Vec<Bookmark>) {
            for n in nodes {
                out.push(n.item.into());
                walk(n.children, out);
            }
        }
        let mut out = Vec::new();
        walk(self.lock().bookmark_tree()?, &mut out);
        Ok(out)
    }

    pub fn add_bookmark(&self, parent: i64, title: String, url: String) -> Result<i64, FfiError> {
        Ok(self.lock().add_bookmark(parent, &title, &url)?)
    }

    pub fn add_folder(&self, parent: i64, title: String) -> Result<i64, FfiError> {
        Ok(self.lock().add_folder(parent, &title)?)
    }

    /// Edits a title and, for a bookmark, its URL (`None` keeps the URL / for folders).
    pub fn edit_bookmark(&self, id: i64, title: String, url: Option<String>) -> Result<(), FfiError> {
        Ok(self.lock().edit_bookmark(id, &title, url.as_deref())?)
    }

    pub fn move_bookmark(&self, id: i64, parent: i64, index: u32) -> Result<(), FfiError> {
        Ok(self.lock().move_bookmark(id, parent, index)?)
    }

    /// Deletes a bookmark, or a folder with everything in it.
    pub fn delete_bookmark(&self, id: i64) -> Result<(), FfiError> {
        Ok(self.lock().delete_bookmark(id)?)
    }

    pub fn search_bookmarks(&self, text: String) -> Result<Vec<Bookmark>, FfiError> {
        Ok(self.lock().search_bookmarks(&text)?.into_iter().map(Into::into).collect())
    }

    pub fn is_bookmarked(&self, url: String) -> Result<bool, FfiError> {
        Ok(self.lock().is_bookmarked(&url)?)
    }

    pub fn bookmarks_for_url(&self, url: String) -> Result<Vec<i64>, FfiError> {
        Ok(self.lock().bookmarks_for_url(&url)?)
    }

    /// Adds the page to the "Bookmarks bar" folder (as the Windows star does) or removes its
    /// bookmarks; returns whether it is bookmarked afterwards.
    pub fn toggle_bookmark(&self, url: String, title: String) -> Result<bool, FfiError> {
        Ok(self.lock().toggle_bookmark(&url, &title)?)
    }

    /// Imports a Netscape bookmarks HTML file (as exported by any browser).
    pub fn import_netscape(&self, html: String) -> Result<ImportStats, FfiError> {
        let stats = self.lock().import_bookmarks(&html).map_err(FfiError::Invalid)?;
        Ok(ImportStats {
            bookmarks: stats.bookmarks as u64,
            folders: stats.folders as u64,
            skipped: stats.skipped as u64,
        })
    }

    /// All bookmarks as Netscape HTML.
    pub fn export_netscape(&self) -> Result<String, FfiError> {
        Ok(netscape::export(&self.lock())?)
    }

    // ----- new-tab tiles -----

    /// Pinned tiles first, then the most visited sites by frecency; at most `n`.
    pub fn top_sites(&self, n: u32, now: i64) -> Result<Vec<Tile>, FfiError> {
        Ok(self.lock().top_sites(n as usize, now)?.into_iter().map(Into::into).collect())
    }

    /// Only the user's pinned shortcuts.
    pub fn pinned_tiles(&self, n: u32) -> Result<Vec<Tile>, FfiError> {
        Ok(self.lock().pinned_tiles(n as usize)?.into_iter().map(Into::into).collect())
    }

    pub fn pin_site(&self, url: String, title: String) -> Result<(), FfiError> {
        Ok(self.lock().pin_site(&url, &title)?)
    }

    pub fn unpin_site(&self, url: String) -> Result<(), FfiError> {
        Ok(self.lock().unpin_site(&url)?)
    }

    /// Changes a pinned tile's URL and title; false when `url` is not pinned.
    pub fn edit_tile(&self, url: String, new_url: String, title: String) -> Result<bool, FfiError> {
        Ok(self.lock().edit_tile(&url, &new_url, &title)?)
    }

    /// Hides a most-visited site from the new-tab page.
    pub fn hide_site(&self, url: String) -> Result<(), FfiError> {
        Ok(self.lock().hide_site(&url)?)
    }

    pub fn reorder_tiles(&self, urls: Vec<String>) -> Result<(), FfiError> {
        Ok(self.lock().reorder_tiles(&urls)?)
    }

    // ----- favicons -----

    /// Stores a PNG favicon (at most 48 KiB) for a host.
    pub fn set_favicon(&self, host: String, png: Vec<u8>, now: i64) -> Result<(), FfiError> {
        Ok(self.lock().set_favicon(&host, &png, now)?)
    }

    pub fn favicon(&self, host: String) -> Result<Option<Vec<u8>>, FfiError> {
        Ok(self.lock().favicon(&host)?)
    }

    pub fn has_favicon(&self, host: String) -> Result<bool, FfiError> {
        Ok(self.lock().has_favicon(&host)?)
    }

    /// Deletes every stored favicon (part of "clear browsing history": the table names each
    /// visited host). Returns how many were deleted.
    pub fn clear_favicons(&self) -> Result<u64, FfiError> {
        Ok(self.lock().clear_favicons()? as u64)
    }

    /// Compacts the database file.
    pub fn vacuum(&self) -> Result<(), FfiError> {
        Ok(self.lock().vacuum()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = "https://duckduckgo.com/?q={searchTerms}";

    fn store() -> Arc<KrunkStore> {
        KrunkStore::in_memory().unwrap()
    }

    #[test]
    fn opens_a_file_and_migrates() {
        let dir = std::env::temp_dir().join(format!("krunk-ffi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db");
        let s = KrunkStore::open(path.to_string_lossy().into_owned()).unwrap();
        s.kv_set("k".into(), "v".into()).unwrap();
        drop(s);
        let s = KrunkStore::open(path.to_string_lossy().into_owned()).unwrap();
        assert_eq!(s.kv_get("k".into()).unwrap(), Some("v".into()));
        s.kv_delete("k".into()).unwrap();
        assert_eq!(s.kv_get("k".into()).unwrap(), None);
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bad_path_is_a_database_error() {
        let e = KrunkStore::open("/nonexistent-dir/x/browser.db".into()).err().unwrap();
        assert!(matches!(e, FfiError::Database(_)));
    }

    #[test]
    fn history_round_trip() {
        let s = store();
        let url = crate::visit_url("https://example.com/a?utm_source=x".into(), false).unwrap();
        let id = s.record_visit(url.clone(), "Example".into(), 1_000).unwrap().unwrap();
        s.record_visit(url.clone(), "Example".into(), 2_000).unwrap();
        assert_eq!(s.visit_total().unwrap(), 2);
        let rows = s.query_history("exam".into(), None, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ts, 2_000);
        assert_eq!(rows[0].url, "https://example.com/a");
        assert_eq!(s.query_history(String::new(), Some(2_000), 10).unwrap().len(), 1);
        assert_eq!(s.delete_visits(vec![id]).unwrap(), 1);
        s.set_title(url.clone(), "New title".into()).unwrap();
        assert_eq!(s.query_history(String::new(), None, 10).unwrap()[0].title, "New title");
        assert_eq!(s.delete_history_range(0, 5_000).unwrap(), 1);
        s.record_visit(url, "Example".into(), 3_000).unwrap();
        s.clear_history().unwrap();
        assert_eq!(s.visit_total().unwrap(), 0);
    }

    #[test]
    fn private_visits_have_no_url_to_record() {
        assert_eq!(crate::visit_url("https://example.com/".into(), true), None);
    }

    #[test]
    fn omnibox_uses_history_and_bookmarks() {
        let s = store();
        s.record_visit("https://krunk.209.wf/".into(), "Krunk".into(), 1_000).unwrap();
        s.add_bookmark(other_bookmarks_id(), "Krunk docs".into(), "https://krunk.209.wf/docs".into()).unwrap();
        let r = s.omnibox_query("krunk".into(), DDG.into());
        assert!(r.rows.iter().any(|r| r.kind == SuggestionKind::Search));
        assert!(r.rows.iter().any(|r| r.kind == SuggestionKind::Bookmark && r.url.ends_with("/docs")));
        assert!(r.rows.iter().any(|r| r.kind == SuggestionKind::History));
        let merged =
            merge_search_suggestions(r.rows.clone(), "krunk".into(), vec!["krunk browser".into()], DDG.into()).unwrap();
        assert!(merged.iter().any(|m| m.kind == SuggestionKind::SearchSuggestion && m.title == "krunk browser"));
        assert!(merged.len() <= 10);
        let r2 = s.omnibox_query("krunk.2".into(), DDG.into());
        assert!(r2.inline.is_some());
    }

    #[test]
    fn bookmarks_folders_and_netscape() {
        let s = store();
        let folder = s.add_folder(bookmarks_bar_id(), "Work".into()).unwrap();
        let b = s.add_bookmark(folder, "Example".into(), "https://example.com/".into()).unwrap();
        assert!(s.is_bookmarked("https://example.com/".into()).unwrap());
        assert_eq!(s.bookmarks_for_url("https://example.com/".into()).unwrap(), vec![b]);
        s.edit_bookmark(b, "Example site".into(), Some("https://example.org/".into())).unwrap();
        assert_eq!(s.bookmark(b).unwrap().unwrap().url.as_deref(), Some("https://example.org/"));
        s.move_bookmark(b, other_bookmarks_id(), 0).unwrap();
        assert_eq!(s.bookmark_children(other_bookmarks_id()).unwrap()[0].id, b);
        let all = s.all_bookmarks().unwrap();
        assert!(all.iter().any(|x| x.id == folder && x.is_folder));
        assert_eq!(s.search_bookmarks("site".into()).unwrap().len(), 1);
        assert!(!s.toggle_bookmark("https://example.org/".into(), "x".into()).unwrap());
        assert!(s.toggle_bookmark("https://example.org/".into(), "x".into()).unwrap());
        let html = s.export_netscape().unwrap();
        assert!(html.contains("example.org"));
        let t = store();
        let stats = t.import_netscape(html).unwrap();
        assert_eq!(stats.bookmarks, 1);
        assert_eq!(stats.folders, 1);
        let none = t.import_netscape("not html".into()).unwrap();
        assert_eq!((none.bookmarks, none.folders), (0, 0));
        s.delete_bookmark(folder).unwrap();
        assert!(s.bookmark(folder).unwrap().is_none());
    }

    #[test]
    fn tiles_and_favicons() {
        let s = store();
        for i in 0..3 {
            s.record_visit("https://a.example/".into(), "A".into(), 1_000 + i).unwrap();
        }
        s.record_visit("https://b.example/".into(), "B".into(), 1_000).unwrap();
        s.pin_site("https://pinned.example/".into(), "Pinned".into()).unwrap();
        let tiles = s.top_sites(8, 2_000).unwrap();
        assert!(tiles[0].pinned);
        assert_eq!(tiles[1].url, "https://a.example/");
        assert!(s.edit_tile("https://pinned.example/".into(), "https://p2.example/".into(), "P2".into()).unwrap());
        assert_eq!(s.pinned_tiles(8).unwrap()[0].url, "https://p2.example/");
        s.hide_site("https://b.example/".into()).unwrap();
        assert!(!s.top_sites(8, 2_000).unwrap().iter().any(|t| t.url == "https://b.example/"));
        s.unpin_site("https://p2.example/".into()).unwrap();
        assert!(s.pinned_tiles(8).unwrap().is_empty());
        s.reorder_tiles(vec![]).unwrap();

        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        s.set_favicon("a.example".into(), png.to_vec(), 5).unwrap();
        assert!(s.has_favicon("a.example".into()).unwrap());
        assert_eq!(s.favicon("a.example".into()).unwrap().unwrap(), png.to_vec());
        assert_eq!(s.favicon("none.example".into()).unwrap(), None);
        s.vacuum().unwrap();
    }

    #[test]
    fn kv_rows_written_together_land_together() {
        let s = store();
        s.kv_set("session".into(), "old".into()).unwrap();
        let rows = vec![
            KvEntry { key: "session".into(), value: "new".into() },
            KvEntry { key: "android.gecko_states".into(), value: "[]".into() },
            KvEntry { key: "android.closed_tabs".into(), value: "[]".into() },
        ];
        s.kv_set_many(rows).unwrap();
        assert_eq!(s.kv_get("session".into()).unwrap().as_deref(), Some("new"));
        assert_eq!(s.kv_get("android.gecko_states".into()).unwrap().as_deref(), Some("[]"));
        assert_eq!(s.kv_get("android.closed_tabs".into()).unwrap().as_deref(), Some("[]"));
        // The same key twice: the last value wins, as with single writes.
        let twice =
            vec![KvEntry { key: "k".into(), value: "1".into() }, KvEntry { key: "k".into(), value: "2".into() }];
        s.kv_set_many(twice).unwrap();
        assert_eq!(s.kv_get("k".into()).unwrap().as_deref(), Some("2"));
        s.kv_set_many(vec![]).unwrap();
    }

    #[test]
    fn clearing_history_and_favicons_leaves_no_visited_hosts() {
        let s = store();
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        for host in ["a.example", "b.example"] {
            s.record_visit(format!("https://{host}/"), host.into(), 1_000).unwrap();
            s.set_favicon(host.into(), png.to_vec(), 5).unwrap();
        }
        let kept = s.add_bookmark(bookmarks_bar_id(), "A".into(), "https://a.example/".into()).unwrap();
        s.clear_history().unwrap();
        // Clearing the history alone leaves one favicon row per visited host behind ...
        assert!(s.has_favicon("b.example".into()).unwrap());
        // ... which the Android "clear browsing history" removes as well.
        assert_eq!(s.clear_favicons().unwrap(), 2);
        assert!(!s.has_favicon("a.example".into()).unwrap());
        assert!(!s.has_favicon("b.example".into()).unwrap());
        assert_eq!(s.clear_favicons().unwrap(), 0);
        assert_eq!(s.visit_total().unwrap(), 0);
        // Bookmarks are not history.
        assert!(s.bookmark(kept).unwrap().is_some());
        s.vacuum().unwrap();
    }
}
