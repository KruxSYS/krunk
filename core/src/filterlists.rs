//! Filter-list sources and the daily update policy.
//!
//! Each enabled list comes either from the copy embedded in the exe or from
//! a newer download kept in the data directory. `UpdateState` (saved as
//! JSON) records which, with a content hash, and when the last update check
//! ran. The engine cache key is derived from exactly these sources, so the
//! cache is rebuilt whenever an embedded list (new exe) or a download changes.

use crate::blocker::{embedded_hash, engine_version, fnv64};
use crate::settings::{CustomList, FILTER_LISTS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Minimum time between update checks.
pub const UPDATE_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;
/// Downloads larger than this are refused.
pub const MAX_LIST_BYTES: usize = 16 * 1024 * 1024;
/// Downloads smaller than this are suspicious (truncated or an error page).
pub const MIN_LIST_BYTES: usize = 2 * 1024;
pub const MIN_LIST_RULES: usize = 50;

/// A downloaded list that replaces the embedded copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Downloaded {
    /// `fnv64` of the list text.
    pub hash: u64,
    pub fetched_ms: i64,
    pub bytes: usize,
    /// The list's own `! Title:` (lists the user added).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// The id of the user's own filters ("My filters" on krunk://adblock).
pub const USER_LIST: &str = "user";
/// The user's own filters are at most this long.
pub const MAX_USER_FILTERS_BYTES: usize = 200 * 1024;

/// Where "My filters" are kept (the store's key-value table).
pub const USER_FILTERS_KEY: &str = "user_filters";

/// [`Sources`] owned, to hand to a worker thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSet {
    pub builtin: Vec<String>,
    pub custom: Vec<CustomList>,
    pub user: String,
    /// Extensions' static declarativeNetRequest rules, translated (0.6.0).
    pub ext: String,
}

impl SourceSet {
    pub fn sources(&self) -> Sources<'_> {
        Sources { builtin: &self.builtin, custom: &self.custom, user: &self.user, ext: &self.ext }
    }

    /// Every list, on or off (to count their rules), without the user's filters.
    pub fn every_list(&self) -> SourceSet {
        SourceSet {
            builtin: FILTER_LISTS.iter().map(|s| s.to_string()).collect(),
            custom: self.custom.iter().map(|l| CustomList { enabled: true, ..l.clone() }).collect(),
            user: String::new(),
            ext: String::new(),
        }
    }
}

/// Filters in a list's text: lines that are not empty, a comment (`!`, `# `) or the header.
pub fn rule_count(text: &str) -> usize {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('!') && !l.starts_with('[') && !l.starts_with("# "))
        .count()
}

/// One list as krunk://adblock shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    /// Shipped with Krunk (can be turned off, not removed).
    pub builtin: bool,
    pub enabled: bool,
    /// "downloaded", "embedded" (the copy in the exe) or "missing" (an added list not
    /// downloaded yet).
    pub source: &'static str,
    pub fetched: Option<i64>,
    pub rules: Option<usize>,
    /// Why the last download of this list failed.
    pub error: Option<String>,
}

/// Every built-in and added list with its state; `rules` from [`rule_count`] by id.
pub fn list_infos(set: &SourceSet, state: &UpdateState, rules: &BTreeMap<String, usize>) -> Vec<ListInfo> {
    let builtin = FILTER_LISTS.iter().map(|id| {
        let dl = state.downloaded.get(*id);
        ListInfo {
            id: id.to_string(),
            title: crate::blocker::list_title(id).to_string(),
            url: crate::blocker::list_url(id).unwrap_or_default().to_string(),
            builtin: true,
            enabled: set.builtin.iter().any(|e| e == id),
            source: if dl.is_some() { "downloaded" } else { "embedded" },
            fetched: dl.map(|d| d.fetched_ms),
            rules: rules.get(*id).copied(),
            error: state.errors.get(*id).cloned(),
        }
    });
    let custom = set.custom.iter().map(|l| {
        let id = custom_id(&l.url);
        let dl = state.downloaded.get(&id);
        ListInfo {
            title: dl.and_then(|d| d.title.clone()).unwrap_or_else(|| l.url.clone()),
            url: l.url.clone(),
            builtin: false,
            enabled: l.enabled,
            source: if dl.is_some() { "downloaded" } else { "missing" },
            fetched: dl.map(|d| d.fetched_ms),
            rules: rules.get(&id).copied(),
            error: state.errors.get(&id).cloned(),
            id,
        }
    });
    builtin.chain(custom).collect()
}

/// The id under which a list the user added by `url` is stored.
pub fn custom_id(url: &str) -> String {
    format!("custom-{:016x}", fnv64(url.as_bytes()))
}

/// Everything the engine is built from: the built-in lists that are on, the lists the user
/// added that are on, and the user's own filters.
#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    pub builtin: &'a [String],
    pub custom: &'a [CustomList],
    pub user: &'a str,
    /// Extensions' rules (0.6.0).
    pub ext: &'a str,
}

/// The id of the extensions' rules in the engine.
pub const EXT_LIST: &str = "extensions";

impl<'a> Sources<'a> {
    /// Only built-in lists (the old way, and the tests).
    pub fn builtin(enabled: &'a [String]) -> Sources<'a> {
        Sources { builtin: enabled, custom: &[], user: "", ext: "" }
    }

    /// The bundled lists that are on: the catalogue's plus the ones a tracking-protection
    /// level adds (`protection::EXTRA_LISTS`, not on the lists page).
    fn builtin_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        FILTER_LISTS
            .iter()
            .chain(crate::protection::EXTRA_LISTS.iter())
            .copied()
            .filter(|id| self.builtin.iter().any(|e| e == id))
    }

    /// The added lists that are on: (id, url).
    pub fn custom_ids(&self) -> impl Iterator<Item = (String, &'a str)> + '_ {
        self.custom.iter().filter(|l| l.enabled).map(|l| (custom_id(&l.url), l.url.as_str()))
    }
}

/// Checks a list the user added: any size up to the limit, text, at least one filter (no
/// header needed: it may be a short list of their own).
pub fn validate_custom(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_LIST_BYTES {
        return Err(format!("too large ({} bytes)", bytes.len()));
    }
    if bytes.contains(&0) {
        return Err("not a text file".into());
    }
    let text = String::from_utf8(bytes.to_vec()).map_err(|_| "not a UTF-8 text file".to_string())?;
    let head = text.trim_start();
    if head.starts_with('<') {
        return Err("a web page, not a filter list".into());
    }
    let rules =
        text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('!') && !l.starts_with('[')).count();
    if rules == 0 {
        return Err("no filters in it".into());
    }
    Ok(text)
}

/// The `! Title:` a list gives itself (in its first lines).
pub fn list_title_of(text: &str) -> Option<String> {
    text.lines().take(50).find_map(|l| {
        let t = l.trim().strip_prefix('!')?.trim();
        let title = t.strip_prefix("Title:")?.trim();
        (!title.is_empty()).then(|| title.chars().take(100).collect())
    })
}

/// A line of the user's own filters that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterError {
    /// 1-based.
    pub line: usize,
    pub text: String,
    pub message: String,
}

/// What the user's own filters contain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFiltersReport {
    pub network: usize,
    pub cosmetic: usize,
    pub errors: Vec<FilterError>,
}

/// Reads the user's own filters line by line (comments `!` and empty lines skipped); at most
/// 100 errors are listed.
pub fn check_user_filters(text: &str) -> UserFiltersReport {
    use adblock::lists::{parse_filter, ParseOptions, ParsedLine};
    let mut r = UserFiltersReport::default();
    for (i, line) in text.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('!') || l.starts_with('[') {
            continue;
        }
        match parse_filter(l, false, ParseOptions::default()) {
            Ok(ParsedLine::Network(_)) => r.network += 1,
            Ok(ParsedLine::Cosmetic(_)) => r.cosmetic += 1,
            Err(e) if r.errors.len() < 100 => {
                r.errors.push(FilterError { line: i + 1, text: l.chars().take(200).collect(), message: e.to_string() })
            }
            Err(_) => {}
        }
    }
    r
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateState {
    /// When the last update check finished (successful or not).
    pub last_check_ms: i64,
    /// Lists for which a valid download exists on disk.
    pub downloaded: BTreeMap<String, Downloaded>,
    /// Short description of the last failure, for the settings page.
    pub last_error: Option<String>,
    /// Why the last download of a list failed, by id (cleared by a good download).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub errors: BTreeMap<String, String>,
}

impl UpdateState {
    pub fn from_json(json: &str) -> UpdateState {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Is an update check due? (Clock jumps backwards count as due.)
    pub fn update_due(&self, now_ms: i64) -> bool {
        self.last_check_ms == 0 || now_ms < self.last_check_ms || now_ms - self.last_check_ms >= UPDATE_INTERVAL_MS
    }

    /// The schedule in words, for the log ("last checked 3.5 h ago; next check in 20.5 h").
    pub fn schedule_note(&self, now_ms: i64) -> String {
        const HOUR: f64 = 3_600_000.0;
        if self.last_check_ms == 0 {
            return "never checked; update due".into();
        }
        let ago = (now_ms - self.last_check_ms) as f64 / HOUR;
        if self.update_due(now_ms) {
            format!("last checked {ago:.1} h ago; update due")
        } else {
            let next = (UPDATE_INTERVAL_MS - (now_ms - self.last_check_ms)) as f64 / HOUR;
            format!("last checked {ago:.1} h ago; next check in {next:.1} h")
        }
    }
}

/// Key identifying the compiled engine: engine version plus, for every
/// enabled list in canonical order, where it comes from and its hash.
pub fn cache_key(enabled: &[String], state: &UpdateState) -> u64 {
    cache_key_for(&Sources::builtin(enabled), state)
}

/// [`cache_key`] for all [`Sources`]. Without added lists or own filters it is the same key
/// as before them (so an existing engine cache stays valid).
pub fn cache_key_for(sources: &Sources<'_>, state: &UpdateState) -> u64 {
    let mut desc = String::from(engine_version());
    for id in sources.builtin_ids() {
        match state.downloaded.get(id) {
            Some(d) => desc.push_str(&format!("|{id}:dl:{:016x}", d.hash)),
            None => desc.push_str(&format!("|{id}:embedded:{:016x}", embedded_hash(id).unwrap_or(0))),
        }
    }
    // An added list without a download adds nothing to the engine.
    for (id, _) in sources.custom_ids() {
        if let Some(d) = state.downloaded.get(&id) {
            desc.push_str(&format!("|{id}:dl:{:016x}", d.hash));
        }
    }
    if !sources.user.trim().is_empty() {
        desc.push_str(&format!("|{USER_LIST}:{:016x}", fnv64(sources.user.as_bytes())));
    }
    if !sources.ext.trim().is_empty() {
        desc.push_str(&format!("|{EXT_LIST}:{:016x}", fnv64(sources.ext.as_bytes())));
    }
    fnv64(desc.as_bytes())
}

/// Validates a downloaded list. Rejects error pages, truncated or empty
/// files, binary data and oversized responses.
pub fn validate_download(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_LIST_BYTES {
        return Err(format!("too large ({} bytes)", bytes.len()));
    }
    if bytes.len() < MIN_LIST_BYTES {
        return Err(format!("too small ({} bytes)", bytes.len()));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if first.starts_with('<') || text[..text.len().min(512)].to_ascii_lowercase().contains("<html") {
        return Err("looks like an HTML page".into());
    }
    if !(first.starts_with('[') || first.starts_with('!')) {
        return Err("missing filter list header".into());
    }
    if text.contains('\0') {
        return Err("binary data".into());
    }
    let rules = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('!')).count();
    if rules < MIN_LIST_RULES {
        return Err(format!("only {rules} rules"));
    }
    Ok(text.to_string())
}

/// Downloads a URL (WinHTTP in the app, a stub in tests).
pub type Fetch<'a> = &'a dyn Fn(&str) -> Result<Vec<u8>, String>;

/// Result of [`rebuild`].
pub struct Rebuilt {
    pub state: UpdateState,
    /// The new engine, or `None` if the sources did not change and the
    /// current engine (with key `current_key`) stays valid.
    pub engine: Option<crate::blocker::Blocker>,
    /// Serialized engine to store as the cache (with `engine`).
    pub cache: Option<Vec<u8>>,
    /// The serialized scriptlet engine of the same lists (with `engine`):
    /// stored next to the cache and handed to the renderers.
    pub scriptlets: Option<Vec<u8>>,
    pub key: u64,
}

/// Downloads a list and the sub-lists it includes, inlined in place.
fn fetch_list(url: &str, fetch: Fetch<'_>) -> Result<String, String> {
    let text = fetch(url).and_then(|bytes| validate_download(&bytes))?;
    if crate::preprocess::includes(&text).is_empty() {
        return Ok(text);
    }
    crate::preprocess::inline_includes(&text, |name| {
        let sub = crate::preprocess::sibling_url(url, name).ok_or("not next to the list")?;
        fetch(&sub).and_then(|bytes| crate::preprocess::validate_include(&bytes))
    })
}

fn list_path(dir: &std::path::Path, id: &str) -> std::path::PathBuf {
    dir.join(format!("{id}.txt"))
}

/// The texts of the enabled lists as the engine in use was built from them: a stored
/// download that still matches its hash, else the bundled copy; then the added lists that
/// were downloaded, and the user's own filters.
pub fn current_texts(dir: &std::path::Path, sources: &Sources<'_>, state: &UpdateState) -> Vec<(String, String)> {
    let stored = |id: &str, check: fn(&[u8]) -> Result<String, String>| {
        let d = state.downloaded.get(id)?;
        let text = std::fs::read(list_path(dir, id)).ok()?;
        (fnv64(&text) == d.hash).then(|| check(&text).ok()).flatten()
    };
    let mut out: Vec<(String, String)> = sources
        .builtin_ids()
        .filter_map(|id| {
            stored(id, validate_download).or_else(|| crate::blocker::bundled_list(id)).map(|t| (id.to_string(), t))
        })
        .collect();
    for (id, _) in sources.custom_ids() {
        if let Some(t) = stored(&id, validate_custom) {
            out.push((id, t));
        }
    }
    if !sources.user.trim().is_empty() {
        out.push((USER_LIST.to_string(), sources.user.to_string()));
    }
    if !sources.ext.trim().is_empty() {
        out.push((EXT_LIST.to_string(), sources.ext.to_string()));
    }
    out
}

/// Refreshes the filter lists and builds the engine. Runs on a worker
/// thread (it does file and network I/O and takes ~0.1–1 s).
///
/// * With `fetch`, each enabled list is downloaded and validated; a good
///   download replaces the stored copy, a bad or failed one keeps the old
///   source (a previous download, else the embedded list).
/// * Lists are read back from `dir` (downloads) or the exe (embedded); a
///   stored download that no longer validates is dropped.
/// * If the resulting cache key equals `current_key` nothing is compiled.
pub fn rebuild(
    dir: &std::path::Path,
    enabled: &[String],
    state: UpdateState,
    fetch: Option<Fetch<'_>>,
    current_key: Option<u64>,
    now_ms: i64,
) -> Rebuilt {
    rebuild_sources(dir, &Sources::builtin(enabled), state, fetch, false, current_key, now_ms)
}

/// [`rebuild`] for all [`Sources`]. With `only_missing`, `fetch` is used for lists that have
/// no download yet (a list the user just added), and the daily check is left alone.
pub fn rebuild_sources(
    dir: &std::path::Path,
    sources: &Sources<'_>,
    mut state: UpdateState,
    fetch: Option<Fetch<'_>>,
    only_missing: bool,
    current_key: Option<u64>,
    now_ms: i64,
) -> Rebuilt {
    if let Some(fetch) = fetch {
        let mut errors = Vec::new();
        let builtin =
            sources.builtin_ids().filter_map(|id| crate::blocker::list_url(id).map(|u| (id.to_string(), u, false)));
        let custom = sources.custom_ids().map(|(id, u)| (id, u, true));
        for (id, url, added) in builtin.chain(custom).collect::<Vec<_>>() {
            // `only_missing`: a list was just added; the built-in ones have their bundled copy.
            if only_missing && (!added || state.downloaded.contains_key(&id)) {
                continue;
            }
            // Built-in lists may `!#include` parts next to them (inlined); added ones are read
            // as they are.
            let got = if added { fetch(url).and_then(|b| validate_custom(&b)) } else { fetch_list(url, fetch) };
            match got {
                Ok(text) => {
                    let hash = fnv64(text.as_bytes());
                    let title = added.then(|| list_title_of(&text)).flatten();
                    match crate::vault::write_atomic(&list_path(dir, &id), text.as_bytes()) {
                        Ok(()) => {
                            state.errors.remove(&id);
                            state
                                .downloaded
                                .insert(id, Downloaded { hash, fetched_ms: now_ms, bytes: text.len(), title });
                        }
                        Err(e) => {
                            errors.push(format!("{id}: {e}"));
                            state.errors.insert(id, e.to_string());
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("{}: {e}", if added { url } else { id.as_str() }));
                    state.errors.insert(id, e);
                }
            }
        }
        if !only_missing {
            state.last_check_ms = now_ms;
        }
        state.last_error = (!errors.is_empty()).then(|| errors.join("; "));
    }
    // Forget lists the user removed, with their files.
    let removed: Vec<String> = state
        .downloaded
        .keys()
        .chain(state.errors.keys())
        .filter(|id| id.starts_with("custom-") && !sources.custom.iter().any(|l| custom_id(&l.url) == **id))
        .cloned()
        .collect();
    for id in removed {
        state.downloaded.remove(&id);
        state.errors.remove(&id);
        let _ = std::fs::remove_file(list_path(dir, &id));
    }
    // Drop records of built-in downloads that no longer validate (the bundled copy is used).
    let invalid: Vec<String> = sources
        .builtin_ids()
        .filter(|id| {
            state.downloaded.get(*id).is_some_and(|d| {
                !std::fs::read(list_path(dir, id))
                    .ok()
                    .is_some_and(|t| fnv64(&t) == d.hash && validate_download(&t).is_ok())
            })
        })
        .map(str::to_string)
        .collect();
    for id in invalid {
        state.downloaded.remove(&id);
    }
    let texts = current_texts(dir, sources, &state);
    let key = cache_key_for(sources, &state);
    if current_key == Some(key) {
        return Rebuilt { state, engine: None, cache: None, scriptlets: None, key };
    }
    let refs: Vec<(&str, &str)> = texts.iter().map(|(i, t)| (i.as_str(), t.as_str())).collect();
    let engine = crate::blocker::Blocker::from_lists(&refs, key);
    let cache = engine.serialize();
    let scriptlets = crate::blocker::ScriptletEngine::from_lists(&refs, key).serialize();
    Rebuilt { state, engine: Some(engine), cache: Some(cache), scriptlets: Some(scriptlets), key }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<String> {
        FILTER_LISTS.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn update_schedule() {
        let mut s = UpdateState::default();
        assert!(s.update_due(1_000), "never checked");
        s.last_check_ms = 10 * UPDATE_INTERVAL_MS;
        assert!(!s.update_due(10 * UPDATE_INTERVAL_MS + 1));
        assert!(!s.update_due(11 * UPDATE_INTERVAL_MS - 1));
        assert!(s.update_due(11 * UPDATE_INTERVAL_MS));
        assert!(s.update_due(5), "clock went backwards");
        assert_eq!(
            s.schedule_note(10 * UPDATE_INTERVAL_MS + 3_600_000),
            "last checked 1.0 h ago; next check in 23.0 h"
        );
        assert_eq!(s.schedule_note(11 * UPDATE_INTERVAL_MS + 3_600_000), "last checked 25.0 h ago; update due");
        assert_eq!(UpdateState::default().schedule_note(5), "never checked; update due");
        let back = UpdateState::from_json(&s.to_json());
        assert_eq!(back, s);
        assert_eq!(UpdateState::from_json("garbage"), UpdateState::default());
    }

    #[test]
    fn protection_lists_are_sources_too() {
        let enabled = vec!["easylist".to_string(), "nocoin".into(), "krunk-fingerprinting".into(), "nope".into()];
        let ids: Vec<&str> = Sources::builtin(&enabled).builtin_ids().collect();
        assert_eq!(ids, ["easylist", "nocoin", "krunk-fingerprinting"]);
        let dir = std::env::temp_dir().join(format!("krunk-lists-{}", std::process::id()));
        let texts = current_texts(&dir, &Sources::builtin(&enabled), &UpdateState::default());
        assert!(texts.iter().any(|(id, t)| id == "nocoin" && t.contains("coinhive")), "the bundled NoCoin text");
        assert!(texts.iter().any(|(id, t)| id == "krunk-fingerprinting" && t.contains("fpjscdn.net")));
        assert_ne!(
            cache_key(&enabled, &UpdateState::default()),
            cache_key(&enabled[..1], &UpdateState::default()),
            "the extra lists change the key"
        );
    }

    #[test]
    fn cache_key_tracks_sources() {
        let base = UpdateState::default();
        let k = cache_key(&all(), &base);
        assert_eq!(k, cache_key(&all(), &base), "deterministic");
        // Enabled set changes the key; order of the setting does not.
        let mut fewer = all();
        fewer.retain(|l| l != "ublock-unbreak");
        assert_ne!(cache_key(&fewer, &base), k);
        let mut reversed = all();
        reversed.reverse();
        assert_eq!(cache_key(&reversed, &base), k);
        // A downloaded list (or a different download) changes the key.
        let mut dl = base.clone();
        dl.downloaded.insert("easylist".into(), Downloaded { hash: 1, fetched_ms: 5, bytes: 10, title: None });
        let k1 = cache_key(&all(), &dl);
        assert_ne!(k1, k);
        dl.downloaded.get_mut("easylist").unwrap().hash = 2;
        assert_ne!(cache_key(&all(), &dl), k1);
        // Fetch time alone does not invalidate the cache.
        dl.downloaded.get_mut("easylist").unwrap().fetched_ms = 99;
        let k2 = cache_key(&all(), &dl);
        dl.downloaded.get_mut("easylist").unwrap().fetched_ms = 100;
        assert_eq!(cache_key(&all(), &dl), k2);
        // A download for a disabled list is irrelevant.
        let mut off = fewer.clone();
        off.retain(|l| l != "easylist");
        let mut dl2 = base.clone();
        assert_eq!(cache_key(&off, &dl2), cache_key(&off, &base));
        dl2.downloaded.insert("easylist".into(), Downloaded { hash: 7, fetched_ms: 1, bytes: 1, title: None });
        assert_eq!(cache_key(&off, &dl2), cache_key(&off, &base));
    }

    #[test]
    fn embedded_list_change_changes_key() {
        // The embedded part of the key is the hash of the embedded bytes.
        let h = embedded_hash("easylist").unwrap();
        assert_ne!(h, embedded_hash("easyprivacy").unwrap());
        assert_eq!(embedded_hash("nope"), None);
    }

    struct Tmp(std::path::PathBuf);
    impl Tmp {
        fn new(n: &str) -> Tmp {
            let p = std::env::temp_dir().join(format!("lb-lists-{n}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn list_with(rule: &str) -> Vec<u8> {
        format!("[Adblock Plus 2.0]\n! Title: test\n{}{rule}\n", "||filler-domain.example^\n".repeat(100)).into_bytes()
    }

    #[test]
    fn rebuild_with_downloads_and_fallbacks() {
        use crate::blocker::{Decision, ResourceKind};
        let tmp = Tmp::new("rebuild");
        let enabled = vec!["easylist".to_string(), "easyprivacy".to_string()];
        // easylist downloads fine (with a new rule); easyprivacy fails.
        let fetch = |url: &str| -> Result<Vec<u8>, String> {
            if url.contains("easylist.txt") {
                Ok(list_with("||brand-new-ads.example^"))
            } else {
                Err("timeout".into())
            }
        };
        let r = rebuild(&tmp.0, &enabled, UpdateState::default(), Some(&fetch), None, 1000);
        assert_eq!(r.state.last_check_ms, 1000);
        assert!(r.state.downloaded.contains_key("easylist"));
        assert!(!r.state.downloaded.contains_key("easyprivacy"), "fell back to embedded");
        assert!(r.state.last_error.as_deref().unwrap().contains("easyprivacy: timeout"));
        let engine = r.engine.unwrap();
        let src = "https://news.example/";
        assert_eq!(engine.check("https://brand-new-ads.example/a.js", src, ResourceKind::Script), Decision::Block);
        assert_eq!(
            engine.check("https://www.google-analytics.com/analytics.js", src, ResourceKind::Script),
            Decision::Block,
            "embedded easyprivacy still active"
        );
        assert_eq!(r.key, cache_key(&enabled, &r.state));
        let cache = r.cache.unwrap();
        assert_eq!(crate::blocker::Blocker::cached_key(&cache), Some(r.key));

        // Same sources again: nothing to compile.
        let again = rebuild(&tmp.0, &enabled, r.state.clone(), None, Some(r.key), 2000);
        assert!(again.engine.is_none());
        assert_eq!(again.key, r.key);

        // Suspicious download: the previous good copy is kept.
        let bad = |_: &str| -> Result<Vec<u8>, String> { Ok(b"<html>Rate limited</html>".to_vec()) };
        let r2 = rebuild(&tmp.0, &enabled, r.state.clone(), Some(&bad), Some(r.key), 3000);
        assert!(r2.engine.is_none(), "unchanged sources");
        assert_eq!(r2.state.downloaded["easylist"], r.state.downloaded["easylist"]);
        assert!(r2.state.last_error.as_deref().unwrap().contains("too small"));

        // A tampered stored download is dropped in favour of the embedded list.
        std::fs::write(tmp.0.join("easylist.txt"), list_with("||evil^")).unwrap();
        let r3 = rebuild(&tmp.0, &enabled, r2.state.clone(), None, Some(r.key), 4000);
        assert!(!r3.state.downloaded.contains_key("easylist"));
        assert_ne!(r3.key, r.key);
        let e3 = r3.engine.unwrap();
        assert_eq!(e3.check("https://brand-new-ads.example/a.js", src, ResourceKind::Script), Decision::Allow);
    }

    #[test]
    fn added_lists_and_own_filters() {
        use crate::blocker::{Decision, ResourceKind};
        let tmp = Tmp::new("custom");
        let builtin = vec!["easylist".to_string()];
        let none: Vec<CustomList> = Vec::new();
        // Without added lists or own filters: the same key as before them.
        let plain = Sources { builtin: &builtin, custom: &none, user: "  \n", ext: "" };
        assert_eq!(cache_key_for(&plain, &UpdateState::default()), cache_key(&builtin, &UpdateState::default()));
        let custom = vec![
            CustomList { url: "https://lists.example/mine.txt".into(), enabled: true },
            CustomList { url: "https://lists.example/off.txt".into(), enabled: false },
        ];
        let user = "! mine\n||own-filter.example^\nexample.com##.promo\n";
        let sources = Sources { builtin: &builtin, custom: &custom, user, ext: "" };
        let fetched = std::cell::RefCell::new(Vec::new());
        let fetch = |url: &str| -> Result<Vec<u8>, String> {
            fetched.borrow_mut().push(url.to_string());
            if url.contains("mine.txt") {
                Ok(b"! Title: My list\n||added-list.example^\n".to_vec())
            } else {
                Err("offline".into())
            }
        };
        // A list was just added: only it is fetched, and the daily check stays as it was.
        let state = UpdateState { last_check_ms: 77, ..UpdateState::default() };
        let r = rebuild_sources(&tmp.0, &sources, state, Some(&fetch), true, None, 1000);
        assert_eq!(*fetched.borrow(), vec!["https://lists.example/mine.txt".to_string()], "only the missing one");
        assert_eq!(r.state.last_check_ms, 77);
        let id = custom_id("https://lists.example/mine.txt");
        assert_eq!(r.state.downloaded[&id].title.as_deref(), Some("My list"));
        let engine = r.engine.unwrap();
        let page = "https://news.example/";
        assert_eq!(engine.check("https://added-list.example/a.js", page, ResourceKind::Script), Decision::Block);
        assert_eq!(engine.check("https://own-filter.example/a.js", page, ResourceKind::Script), Decision::Block);
        assert_eq!(engine.check("https://fine.example/a.js", page, ResourceKind::Script), Decision::Allow);
        // The key follows the own filters and the added list's download.
        let k = cache_key_for(&sources, &r.state);
        assert_eq!(r.key, k);
        assert_ne!(cache_key_for(&Sources { user: "||other.example^", ..sources }, &r.state), k);
        assert_ne!(cache_key_for(&Sources { custom: &none, ..sources }, &r.state), k);
        // Nothing changed: nothing compiled.
        assert!(rebuild_sources(&tmp.0, &sources, r.state.clone(), None, false, Some(k), 2000).engine.is_none());
        assert_eq!(r.state.errors.get(&custom_id("https://lists.example/off.txt")), None, "off: not fetched");
        // What krunk://adblock shows.
        let set = SourceSet { builtin: builtin.clone(), custom: custom.clone(), user: user.into(), ext: String::new() };
        assert_eq!(set.sources().custom.len(), 2);
        let counts: BTreeMap<String, usize> = current_texts(&tmp.0, &set.every_list().sources(), &r.state)
            .into_iter()
            .map(|(i, t)| (i, rule_count(&t)))
            .collect();
        assert_eq!(counts[&id], 1);
        assert!(counts["easyprivacy"] > 1000, "every built-in list, on or off");
        let infos = list_infos(&set, &r.state, &counts);
        assert_eq!(infos.len(), FILTER_LISTS.len() + 2);
        let mine = infos.iter().find(|i| i.id == id).unwrap();
        assert_eq!(
            (mine.title.as_str(), mine.source, mine.enabled, mine.rules),
            ("My list", "downloaded", true, Some(1))
        );
        let off = infos.last().unwrap();
        assert_eq!((off.title.as_str(), off.source, off.enabled), ("https://lists.example/off.txt", "missing", false));
        let el = &infos[0];
        assert_eq!((el.id.as_str(), el.builtin, el.enabled, el.source), ("easylist", true, true, "embedded"));
        assert_eq!(el.url, "https://easylist.to/easylist/easylist.txt");
        assert!(!infos[1].enabled);
        // A failed download is remembered per list, and cleared by a good one.
        let failing = vec![CustomList { url: "https://lists.example/gone.txt".into(), enabled: true }];
        let r2 = rebuild_sources(
            &tmp.0,
            &Sources { custom: &failing, ..sources },
            r.state.clone(),
            Some(&fetch),
            true,
            None,
            3000,
        );
        assert_eq!(
            r2.state.errors.get(&custom_id("https://lists.example/gone.txt")).map(String::as_str),
            Some("offline")
        );
        assert_eq!(
            r2.key,
            cache_key_for(&Sources { custom: &none, ..sources }, &r2.state),
            "a list that could not be downloaded changes nothing"
        );
        // A removed list is forgotten, with its file.
        let gone = rebuild_sources(&tmp.0, &Sources { custom: &none, ..sources }, r2.state, None, false, None, 4000);
        assert!(!gone.state.downloaded.keys().chain(gone.state.errors.keys()).any(|k| k.starts_with("custom-")));
        assert!(!list_path(&tmp.0, &id).exists());
        let r = rebuild_sources(&tmp.0, &sources, gone.state, Some(&fetch), true, None, 5000);
        // The texts the lookup uses include both.
        let ids: Vec<_> = current_texts(&tmp.0, &sources, &r.state).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ids, vec!["easylist".to_string(), id, USER_LIST.to_string()]);
        // Added lists and own filters never get the trusted permissions of the uBO lists.
        assert!(!custom_id("https://ublock-x.example/").starts_with("ublock-") && !USER_LIST.starts_with("ublock-"));
    }

    #[test]
    fn counts_rules() {
        assert_eq!(rule_count("[Adblock Plus 2.0]\n! Title: x\n\n||a^\n  ##.ad \n# comment\n#@#.x\n"), 3);
    }

    #[test]
    fn custom_validation_and_titles() {
        assert!(validate_custom(b"||a.example^\n").is_ok(), "a short list of one's own");
        assert!(validate_custom(b"! only a comment\n\n").is_err());
        assert!(validate_custom(b"<!doctype html><title>404</title>").is_err());
        assert!(validate_custom(b"||a.example^\0").is_err());
        assert!(validate_custom(&vec![b'a'; MAX_LIST_BYTES + 1]).is_err());
        assert_eq!(list_title_of("[Adblock Plus 2.0]\n! Title: Nice list \n||x^"), Some("Nice list".into()));
        assert_eq!(list_title_of("||x^"), None);
        assert_ne!(custom_id("https://a.example/l.txt"), custom_id("https://b.example/l.txt"));
    }

    #[test]
    fn own_filters_are_checked_line_by_line() {
        let r =
            check_user_filters("! comment\n\n||ads.example^\nexample.com##.banner\n||bad.example^$unknownoption\n##\n");
        assert_eq!((r.network, r.cosmetic), (1, 1));
        assert_eq!(r.errors.iter().map(|e| e.line).collect::<Vec<_>>(), vec![5, 6]);
        assert!(!r.errors[0].message.is_empty());
        let many: String = (0..150).map(|i| format!("||x{i}.example^$nope\n")).collect();
        assert_eq!(check_user_filters(&many).errors.len(), 100);
    }

    #[test]
    fn engine_is_send() {
        fn send<T: Send>() {}
        send::<crate::blocker::Blocker>();
        send::<Rebuilt>();
    }

    #[test]
    fn download_validation() {
        let good = format!("[Adblock Plus 2.0]\n! Title: Test\n{}", "||ads.example^\n".repeat(200));
        assert!(validate_download(good.as_bytes()).is_ok());
        let bom = format!("\u{feff}{good}");
        assert!(validate_download(bom.as_bytes()).is_ok());
        assert!(validate_download(b"").unwrap_err().contains("too small"));
        let html = format!("<!DOCTYPE html><html><body>{}</body></html>", "x".repeat(5000));
        assert!(validate_download(html.as_bytes()).is_err());
        let few = format!("! Title: x\n{}\n||a^\n", "! comment\n".repeat(400));
        assert!(validate_download(few.as_bytes()).unwrap_err().contains("rules"));
        let noheader = "||ads.example^\n".repeat(300);
        assert!(validate_download(noheader.as_bytes()).is_err());
        let huge = vec![b'!'; MAX_LIST_BYTES + 1];
        assert!(validate_download(&huge).unwrap_err().contains("too large"));
        let mut binary = good.clone().into_bytes();
        binary[100] = 0;
        assert!(validate_download(&binary).is_err());
        assert!(validate_download(&[0xff, 0xfe, 0x00, 0x41].repeat(1000)).is_err());
        // The bundled lists themselves pass.
        for id in FILTER_LISTS {
            let text = crate::blocker::bundled_list(id).unwrap();
            assert!(validate_download(text.as_bytes()).is_ok(), "{id}");
        }
    }
}
