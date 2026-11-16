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
    pub(crate) fn lock(&self) -> MutexGuard<'_, Store> {
        // A panic while holding the lock cannot leave a half-written transaction behind (SQLite
        // rolls it back), so a poisoned lock is still usable. Release builds abort on a panic.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// One history row (a visit).
/// A page title as history stores it: control characters removed, trimmed, at most 512
/// characters (the tab model's limit). A page chooses its title, and an unbounded one would
/// sit in the database and slow every address-bar query.
fn clean_title(title: &str) -> String {
    let clean: String = title.chars().filter(|c| !c.is_control()).take(512).collect();
    clean.trim().to_string()
}

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
    /// The device of the visit (Android 0.8.0): `""` this device, a sync device id, or `None`
    /// unknown (synced before devices were recorded).
    pub device: Option<String>,
}

/// A device with visits in the history: the history screen's filter chips (Android 0.8.0).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryDevice {
    /// The filter key: `"this"`, `"unknown"` or a sync device id.
    pub key: String,
    /// The device's name from sync, if it has one.
    pub name: Option<String>,
    pub visits: i64,
}

fn entry_of(h: lb_core::history::HistoryEntry) -> HistoryEntry {
    HistoryEntry { id: h.id, url: h.url, title: h.title, ts: h.ts, visit_count: h.visit_count, device: h.device }
}

/// A history row with the local day it belongs to (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryRow {
    pub entry: HistoryEntry,
    pub day: String,
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
    /// Frecency score; pinned tiles carry infinity, so they sort before every visited site.
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

    /// Validates and saves. Desktop-only fields already stored keep their values, and so do
    /// values this version can't read (F-69): a field keeps its stored raw value unless this
    /// save changes it.
    pub fn save_settings(&self, settings: AndroidSettings) -> Result<(), SettingsError> {
        self.save_settings_edited(settings, Vec::new())
    }

    /// [`Self::save_settings`], with the fields (dotted camelCase paths of the shared settings,
    /// e.g. `searchEngine`) the user set in this change, even to the value already shown: a raw
    /// value another platform stored there is then replaced, not kept.
    pub fn save_settings_edited(&self, settings: AndroidSettings, edited: Vec<String>) -> Result<(), SettingsError> {
        let mut store = self.lock();
        let stored = store.kv_get("settings")?;
        let mut core = store.load_settings()?;
        settings.apply_to_core(&mut core);
        for e in edited.iter().filter(|e| crate::settings::android_owned(e)) {
            core.mark_edited(e);
        }
        if let Err(errors) = core.validate() {
            // Only the fields Android edits can be wrong here: a desktop-only field is saved as
            // it was stored, so its problem is not the user's to fix on the phone.
            let errors: Vec<_> = errors.into_iter().filter(|e| crate::settings::android_owned(e.field)).collect();
            if !errors.is_empty() {
                return Err(SettingsError::Validation {
                    errors: errors
                        .into_iter()
                        .map(|e| crate::SettingsFieldError { field: e.field.to_string(), message: e.message })
                        .collect(),
                });
            }
        }
        // Both rows or neither: a failure between them must not leave half of a change stored
        // while the caller is told that nothing was saved.
        let shared = core
            .to_json_preserving(stored.as_deref())
            .map_err(|e| SettingsError::Database { detail: e.to_string() })?;
        let extras = settings.extras_json();
        store.kv_set_many(&[("settings", shared.as_str()), (EXTRAS_KEY, extras.as_str())])?;
        Ok(())
    }

    // ----- history -----

    /// Records a visit (the URL as returned by `visit_url`); `None` when the URL is not recorded.
    /// The title is cleaned and bounded ([`clean_title`]); an empty one keeps the stored title.
    pub fn record_visit(&self, url: String, title: String, ts: i64) -> Result<Option<i64>, FfiError> {
        Ok(self.lock().record_visit(&url, &clean_title(&title), ts)?)
    }

    pub fn set_title(&self, url: String, title: String) -> Result<(), FfiError> {
        Ok(self.lock().set_title(&url, &clean_title(&title))?)
    }

    /// Visits matching `text` (empty = all), newest first, older than `before` (ms, exclusive)
    /// when given, at most `limit` (1-1000).
    pub fn query_history(&self, text: String, before: Option<i64>, limit: u32) -> Result<Vec<HistoryEntry>, FfiError> {
        Ok(self.lock().query_history(&text, before, limit)?.into_iter().map(entry_of).collect())
    }

    /// A page of visits matching `text`, newest first, after the last row of the page before
    /// (`before_ts` and `before_id` of that row, both or neither): visits of one millisecond
    /// are neither skipped nor repeated. Each row carries its local day (`YYYY-MM-DD`, for
    /// `utc_offset_ms` of now), so the list is grouped without a call per row.
    pub fn query_history_page(
        &self,
        text: String,
        before_ts: Option<i64>,
        before_id: Option<i64>,
        limit: u32,
        utc_offset_ms: i64,
    ) -> Result<Vec<HistoryRow>, FfiError> {
        self.query_history_page_in(text, before_ts, before_id, limit, utc_offset_ms, "all".into())
    }

    /// [`KrunkStore::query_history_page`] inside a device filter: `"all"`, `"this"`, `"unknown"`
    /// or a sync device id (a [`HistoryDevice::key`]).
    pub fn query_history_page_in(
        &self,
        text: String,
        before_ts: Option<i64>,
        before_id: Option<i64>,
        limit: u32,
        utc_offset_ms: i64,
        filter: String,
    ) -> Result<Vec<HistoryRow>, FfiError> {
        let before = match (before_ts, before_id) {
            (Some(ts), Some(id)) => Some((ts, id)),
            (None, None) => None,
            _ => return Err(FfiError::Invalid("a cursor needs the time and the id".into())),
        };
        let filter = lb_core::history::HistoryFilter::parse(&filter);
        Ok(self
            .lock()
            .query_history_page_in(&text, before, limit, &filter)?
            .into_iter()
            .map(|h| HistoryRow {
                day: lb_core::storage::date_ymd(h.ts.saturating_add(utc_offset_ms)),
                entry: entry_of(h),
            })
            .collect())
    }

    /// The devices with visits here: this device first, then by name, unknown last.
    pub fn history_devices(&self) -> Result<Vec<HistoryDevice>, FfiError> {
        Ok(self
            .lock()
            .history_devices()?
            .into_iter()
            .map(|d| HistoryDevice { key: d.key, name: d.name, visits: d.visits })
            .collect())
    }

    /// Clears one device filter's history: `"all"` everywhere (synced), `"this"` this device's
    /// visits here and on the other devices, another device's (or `"unknown"`) only here.
    pub fn clear_history_in(&self, filter: String) -> Result<u64, FfiError> {
        Ok(self.lock().clear_history_in(&lb_core::history::HistoryFilter::parse(&filter))? as u64)
    }

    /// [`KrunkStore::delete_history_range`] inside a device filter.
    pub fn delete_history_range_in(&self, from: i64, to: i64, filter: String) -> Result<u64, FfiError> {
        Ok(self.lock().delete_history_range_in(from, to, &lb_core::history::HistoryFilter::parse(&filter))? as u64)
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

    /// Sites where ad and tracker blocking is off (the shared `adblock_allowlist` table, the
    /// same rows Windows uses), sorted. An entry covers the host and its subdomains.
    pub fn allowlist(&self) -> Result<Vec<String>, FfiError> {
        Ok(self.lock().allowlist()?)
    }

    /// Adds a site to the allow list and returns the stored form (lower case, no `*.`, no
    /// trailing dot). Something that is not a host name is `FfiError::Invalid`.
    pub fn allowlist_add(&self, host: String) -> Result<String, FfiError> {
        Ok(self.lock().allowlist_add(&host, lb_core::storage::now_ms())?)
    }

    /// Removes a site from the allow list; `false` when it was not on it.
    pub fn allowlist_remove(&self, host: String) -> Result<bool, FfiError> {
        Ok(self.lock().allowlist_remove(&host)?)
    }

    /// Is this site on the "never save passwords here" list (the shared `vault_never` table, the
    /// same rows Windows uses)? `origin` may be a page address: its origin is what counts.
    pub fn never_save(&self, origin: String) -> Result<bool, FfiError> {
        let origin = lb_core::urlclean::origin_of(&origin).unwrap_or(origin);
        Ok(self.lock().never_save(&origin)?)
    }

    /// Puts a site on the never-save list. Something without an origin is `FfiError::Invalid`.
    pub fn never_save_add(&self, origin: String, now: i64) -> Result<(), FfiError> {
        Ok(self.lock().never_save_add(&origin, now)?)
    }

    pub fn never_save_remove(&self, origin: String) -> Result<(), FfiError> {
        let origin = lb_core::urlclean::origin_of(&origin).unwrap_or(origin);
        Ok(self.lock().never_save_remove(&origin)?)
    }

    /// The never-save list (origins), sorted.
    pub fn never_save_list(&self) -> Result<Vec<String>, FfiError> {
        Ok(self.lock().never_save_list()?)
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
    fn never_save_sites() {
        let s = store();
        assert!(!s.never_save("https://bank.example/login".into()).unwrap());
        s.never_save_add("https://bank.example/login?x=1".into(), 5).unwrap();
        s.never_save_add("http://plain.example:8080/".into(), 6).unwrap();
        // Asked with a page address or with the origin; another port or scheme is another site.
        assert!(s.never_save("https://bank.example/other".into()).unwrap());
        assert!(s.never_save("https://bank.example".into()).unwrap());
        assert!(!s.never_save("https://bank.example:8443/".into()).unwrap());
        assert!(!s.never_save("http://bank.example/".into()).unwrap());
        assert!(!s.never_save("not a url".into()).unwrap());
        assert!(matches!(s.never_save_add("data:text/html,x".into(), 7), Err(FfiError::Invalid(_))));
        assert_eq!(s.never_save_list().unwrap(), ["http://plain.example:8080", "https://bank.example"]);
        s.never_save_remove("https://bank.example/whatever".into()).unwrap();
        assert_eq!(s.never_save_list().unwrap(), ["http://plain.example:8080"]);
    }

    #[test]
    fn history_pages_by_time_and_id() {
        let s = store();
        // Five visits in one millisecond, two around them.
        s.record_visit("https://example.com/old".into(), "Old".into(), 1_000).unwrap();
        for n in 0..5 {
            s.record_visit(format!("https://example.com/same{n}"), "Same".into(), 2_000).unwrap();
        }
        s.record_visit("https://example.com/new".into(), "New".into(), 3_000).unwrap();
        let mut seen = Vec::new();
        let mut cursor: Option<(i64, i64)> = None;
        loop {
            let page = s.query_history_page(String::new(), cursor.map(|c| c.0), cursor.map(|c| c.1), 2, 0).unwrap();
            if page.is_empty() {
                break;
            }
            assert!(page.len() <= 2);
            cursor = page.last().map(|r| (r.entry.ts, r.entry.id));
            seen.extend(page.into_iter().map(|r| r.entry.id));
        }
        assert_eq!(seen.len(), 7, "nothing skipped");
        let mut sorted = seen.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();
        assert_eq!(sorted, seen, "newest first, nothing twice");
        // The old cursor (time alone) loses the rest of the millisecond.
        let first = s.query_history(String::new(), None, 2).unwrap();
        let rest = s.query_history(String::new(), Some(first[1].ts), 100).unwrap();
        assert!(first.len() + rest.len() < 7);
        // Search and the day of each row; half a cursor is refused.
        let hit = s.query_history_page("same3".into(), None, None, 10, 0).unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].day, "1970-01-01");
        let late = s.query_history_page("same3".into(), None, None, 10, -3_600_000).unwrap();
        assert_eq!(late[0].day, "1969-12-31");
        assert!(matches!(s.query_history_page(String::new(), Some(1), None, 10, 0), Err(FfiError::Invalid(_))));
    }

    #[test]
    fn history_by_device() {
        let s = store();
        s.record_visit("https://a.example/".into(), "A".into(), 1_000).unwrap();
        let devs = s.history_devices().unwrap();
        assert_eq!(devs, vec![HistoryDevice { key: "this".into(), name: None, visits: 1 }]);
        let mine = s.query_history_page_in(String::new(), None, None, 10, 0, "this".into()).unwrap();
        assert_eq!(mine[0].entry.device.as_deref(), Some(""));
        assert!(s.query_history_page_in(String::new(), None, None, 10, 0, "dev-x".into()).unwrap().is_empty());
        assert!(s.query_history_page_in(String::new(), None, None, 10, 0, "unknown".into()).unwrap().is_empty());
        assert_eq!(s.clear_history_in("dev-x".into()).unwrap(), 0);
        assert_eq!(s.clear_history_in("this".into()).unwrap(), 1);
        assert_eq!(s.visit_total().unwrap(), 0);
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
    fn rejected_input_is_invalid_not_a_database_failure() {
        let s = store();
        let bad_url = s.add_bookmark(bookmarks_bar_id(), "Script".into(), "javascript:alert(1)".into());
        assert!(matches!(bad_url, Err(FfiError::Invalid(_))), "{bad_url:?}");
        let unknown = s.delete_bookmark(987_654);
        assert!(matches!(unknown, Err(FfiError::Invalid(_))), "{unknown:?}");
    }

    #[test]
    fn history_titles_are_cleaned_and_bounded() {
        let s = store();
        let huge = format!("  Big\u{0007}\ttitle {}", "x".repeat(2_000_000));
        s.record_visit("https://a.example/".into(), huge.clone(), 1_000).unwrap();
        let row = &s.query_history(String::new(), None, 10).unwrap()[0];
        assert_eq!(row.title.chars().count(), 510);
        assert!(row.title.starts_with("Bigtitle x"));
        // The later title event is bounded the same way ...
        s.set_title("https://a.example/".into(), huge).unwrap();
        assert_eq!(s.query_history(String::new(), None, 10).unwrap()[0].title.chars().count(), 510);
        // ... and a visit recorded without a title keeps the one already stored.
        s.set_title("https://a.example/".into(), "Real title".into()).unwrap();
        s.record_visit("https://a.example/".into(), String::new(), 2_000).unwrap();
        s.record_visit("https://a.example/".into(), " \u{0000} ".into(), 3_000).unwrap();
        let rows = s.query_history(String::new(), None, 10).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.title == "Real title"));
        assert_eq!(clean_title("ok"), "ok");
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
    fn allow_list_rows_are_normalised_kept_and_removed() {
        let dir = std::env::temp_dir().join(format!("krunk-ffi-allow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db").to_string_lossy().to_string();
        {
            let s = KrunkStore::open(path.clone()).unwrap();
            assert!(s.allowlist().unwrap().is_empty());
            assert_eq!(s.allowlist_add("WWW.Example.com".into()).unwrap(), "www.example.com");
            assert_eq!(s.allowlist_add("*.news.org".into()).unwrap(), "news.org");
            // Adding twice keeps one row.
            s.allowlist_add("news.org".into()).unwrap();
            assert_eq!(s.allowlist().unwrap(), vec!["news.org", "www.example.com"]);
            assert!(matches!(s.allowlist_add("https://example.com/x".into()), Err(FfiError::Invalid(_))));
        }
        // The rows are in the database file: they survive closing and reopening it (app restart).
        let s = KrunkStore::open(path).unwrap();
        assert_eq!(s.allowlist().unwrap(), vec!["news.org", "www.example.com"]);
        assert!(s.allowlist_remove("NEWS.org".into()).unwrap());
        assert!(!s.allowlist_remove("news.org".into()).unwrap());
        assert!(!s.allowlist_remove("not a host".into()).unwrap());
        assert_eq!(s.allowlist().unwrap(), vec!["www.example.com"]);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
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
