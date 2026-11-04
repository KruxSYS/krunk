//! Filter-list sources and the daily update policy.
//!
//! Each enabled list comes either from the copy embedded in the exe or from
//! a newer download kept in the data directory. `UpdateState` (saved as
//! JSON) records which, with a content hash, and when the last update check
//! ran. The engine cache key is derived from exactly these sources, so the
//! cache is rebuilt whenever an embedded list (new exe) or a download changes.

use crate::blocker::{embedded_hash, engine_version, fnv64};
use crate::settings::FILTER_LISTS;
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
    let mut desc = String::from(engine_version());
    for id in FILTER_LISTS.iter().filter(|id| enabled.iter().any(|e| e == *id)) {
        match state.downloaded.get(*id) {
            Some(d) => desc.push_str(&format!("|{id}:dl:{:016x}", d.hash)),
            None => desc.push_str(&format!("|{id}:embedded:{:016x}", embedded_hash(id).unwrap_or(0))),
        }
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
    mut state: UpdateState,
    fetch: Option<Fetch<'_>>,
    current_key: Option<u64>,
    now_ms: i64,
) -> Rebuilt {
    let ids: Vec<&str> = FILTER_LISTS.iter().copied().filter(|id| enabled.iter().any(|e| e == id)).collect();
    if let Some(fetch) = fetch {
        let mut errors = Vec::new();
        for id in &ids {
            let Some(url) = crate::blocker::list_url(id) else { continue };
            match fetch_list(url, fetch) {
                Ok(text) => {
                    let hash = fnv64(text.as_bytes());
                    let path = list_path(dir, id);
                    match crate::vault::write_atomic(&path, text.as_bytes()) {
                        Ok(()) => {
                            state
                                .downloaded
                                .insert(id.to_string(), Downloaded { hash, fetched_ms: now_ms, bytes: text.len() });
                        }
                        Err(e) => errors.push(format!("{id}: {e}")),
                    }
                }
                Err(e) => errors.push(format!("{id}: {e}")),
            }
        }
        state.last_check_ms = now_ms;
        state.last_error = (!errors.is_empty()).then(|| errors.join("; "));
    }
    // Load the texts, verifying stored downloads against their hashes.
    let mut texts = Vec::with_capacity(ids.len());
    for id in &ids {
        let downloaded = state.downloaded.get(*id).and_then(|d| {
            let text = std::fs::read(list_path(dir, id)).ok()?;
            (fnv64(&text) == d.hash).then(|| validate_download(&text).ok()).flatten()
        });
        match downloaded {
            Some(t) => texts.push((*id, t)),
            None => {
                state.downloaded.remove(*id);
                if let Some(t) = crate::blocker::bundled_list(id) {
                    texts.push((*id, t));
                }
            }
        }
    }
    let key = cache_key(enabled, &state);
    if current_key == Some(key) {
        return Rebuilt { state, engine: None, cache: None, scriptlets: None, key };
    }
    let refs: Vec<(&str, &str)> = texts.iter().map(|(i, t)| (*i, t.as_str())).collect();
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
        dl.downloaded.insert("easylist".into(), Downloaded { hash: 1, fetched_ms: 5, bytes: 10 });
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
        dl2.downloaded.insert("easylist".into(), Downloaded { hash: 7, fetched_ms: 1, bytes: 1 });
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
