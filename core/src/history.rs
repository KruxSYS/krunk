//! Browsing history: recording visits, search and deletion.

use crate::pages;
use crate::storage::{like_pattern, DbResult, Store};
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde::Serialize;

/// One visit as listed on the history page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Visit id (used for deletion).
    pub id: i64,
    pub url: String,
    pub title: String,
    /// Unix ms of this visit.
    pub ts: i64,
    /// Total visits to this URL.
    pub visit_count: i64,
}

/// Only real web pages go into history (no built-in pages, data:, about:).
pub fn should_record(url: &str) -> bool {
    (url.starts_with("http://") || url.starts_with("https://"))
        && !pages::is_pages_origin(url)
        && !pages::is_shell_origin(url)
        && url.len() <= 8192
}

/// The URL to store for a completed top-level navigation, or `None` if it
/// must not be recorded (private tab, built-in page, non-web URL). Tracking
/// parameters are stripped first.
pub fn visit_url(url: &str, private: bool) -> Option<String> {
    if private || !should_record(url) {
        return None;
    }
    let url = crate::urlclean::strip_credentials(url);
    Some(crate::urlclean::strip_tracking_params(&url).unwrap_or(url))
}

impl Store {
    /// Records a navigation to `url` at `ts`. Returns the visit id, or `None`
    /// if the URL is not recorded at all.
    pub fn record_visit(&self, url: &str, title: &str, ts: i64) -> DbResult<Option<i64>> {
        if !should_record(url) {
            return Ok(None);
        }
        self.conn.execute(
            "INSERT INTO urls(url, title, visit_count, last_visit) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(url) DO UPDATE SET
                visit_count = visit_count + 1,
                last_visit = max(last_visit, excluded.last_visit),
                title = CASE WHEN excluded.title <> '' THEN excluded.title ELSE title END",
            params![url, title, ts],
        )?;
        let url_id: i64 = self.conn.query_row("SELECT id FROM urls WHERE url = ?1", [url], |r| r.get(0))?;
        self.conn.execute("INSERT INTO visits(url_id, ts) VALUES (?1, ?2)", params![url_id, ts])?;
        Ok(Some(self.conn.last_insert_rowid()))
    }

    /// Updates the title once the page has reported it.
    pub fn set_title(&self, url: &str, title: &str) -> DbResult<()> {
        if !title.is_empty() {
            self.conn.execute("UPDATE urls SET title = ?2 WHERE url = ?1", params![url, title])?;
        }
        Ok(())
    }

    /// Several pages' titles in one transaction (0.10.0: the browser collects title changes and
    /// writes them every few seconds on a worker).
    pub fn set_titles(&mut self, titles: &[(String, String)]) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE urls SET title = ?2 WHERE url = ?1")?;
            for (url, title) in titles.iter().filter(|(_, t)| !t.is_empty()) {
                stmt.execute(params![url, title])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Visits matching `text` in URL or title, newest first. `before` pages
    /// through results (exclusive upper bound on the timestamp).
    pub fn query_history(&self, text: &str, before: Option<i64>, limit: u32) -> DbResult<Vec<HistoryEntry>> {
        let limit = limit.clamp(1, 1000);
        let before = before.unwrap_or(i64::MAX);
        let pattern = like_pattern(text.trim());
        let mut stmt = self.conn.prepare_cached(
            "SELECT v.id, u.url, u.title, v.ts, u.visit_count
             FROM visits v JOIN urls u ON u.id = v.url_id
             WHERE v.ts < ?1 AND (u.url LIKE ?2 ESCAPE '\\' OR u.title LIKE ?2 ESCAPE '\\')
             ORDER BY v.ts DESC, v.id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![before, pattern, limit], |r| {
            Ok(HistoryEntry { id: r.get(0)?, url: r.get(1)?, title: r.get(2)?, ts: r.get(3)?, visit_count: r.get(4)? })
        })?;
        rows.collect()
    }

    /// A page of visits matching `text` in URL or title, newest first, strictly after the cursor
    /// `before` = (timestamp, visit id) of the last row of the page before (Krunk for Android
    /// 0.4.0). Unlike [`Store::query_history`], whose cursor is the timestamp alone, visits that
    /// share a millisecond are neither skipped nor repeated across pages.
    pub fn query_history_page(
        &self,
        text: &str,
        before: Option<(i64, i64)>,
        limit: u32,
    ) -> DbResult<Vec<HistoryEntry>> {
        let limit = limit.clamp(1, 1000);
        let (ts, id) = before.unwrap_or((i64::MAX, i64::MAX));
        let pattern = like_pattern(text.trim());
        let mut stmt = self.conn.prepare_cached(
            "SELECT v.id, u.url, u.title, v.ts, u.visit_count
             FROM visits v JOIN urls u ON u.id = v.url_id
             WHERE (v.ts < ?1 OR (v.ts = ?1 AND v.id < ?2))
               AND (u.url LIKE ?3 ESCAPE '\\' OR u.title LIKE ?3 ESCAPE '\\')
             ORDER BY v.ts DESC, v.id DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![ts, id, pattern, limit], |r| {
            Ok(HistoryEntry { id: r.get(0)?, url: r.get(1)?, title: r.get(2)?, ts: r.get(3)?, visit_count: r.get(4)? })
        })?;
        rows.collect()
    }

    /// Deletes individual visits.
    pub fn delete_visits(&mut self, ids: &[i64]) -> DbResult<usize> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut del = tx.prepare_cached("DELETE FROM visits WHERE id = ?1")?;
            for id in ids {
                n += del.execute([id])?;
            }
        }
        recount(&tx)?;
        tx.commit()?;
        Ok(n)
    }

    /// Deletes every visit to `url` and the URL itself.
    pub fn delete_url(&self, url: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM urls WHERE url = ?1", [url])?;
        Ok(())
    }

    /// Deletes visits with `from <= ts < to`.
    pub fn delete_history_range(&mut self, from: i64, to: i64) -> DbResult<usize> {
        let tx = self.conn.transaction()?;
        let n = tx.execute("DELETE FROM visits WHERE ts >= ?1 AND ts < ?2", params![from, to])?;
        recount(&tx)?;
        tx.commit()?;
        Ok(n)
    }

    pub fn clear_history(&mut self) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM visits; DELETE FROM urls;")?;
        tx.commit()
    }

    /// Web origins (`scheme://host[:port]`) with a visit at or after
    /// `since` (all visits if `None`), for clearing their site data.
    pub fn visited_origins(&self, since: Option<i64>) -> DbResult<Vec<String>> {
        let mut stmt =
            self.conn.prepare("SELECT DISTINCT u.url FROM urls u JOIN visits v ON v.url_id = u.id WHERE v.ts >= ?1")?;
        let rows = stmt.query_map([since.unwrap_or(i64::MIN)], |r| r.get::<_, String>(0))?;
        let mut origins: Vec<String> = rows
            .filter_map(Result::ok)
            .filter_map(|u| url::Url::parse(&u).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .map(|u| u.origin().ascii_serialization())
            .collect();
        origins.sort();
        origins.dedup();
        Ok(origins)
    }

    /// Every visited web origin with its most recent visit (0.6.0: the site-data
    /// page's "last used").
    pub fn visited_origins_last(&self) -> DbResult<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare("SELECT url, last_visit FROM urls WHERE visit_count > 0")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut last: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for (u, t) in rows.filter_map(Result::ok) {
            let Some(u) = url::Url::parse(&u).ok().filter(|u| matches!(u.scheme(), "http" | "https")) else { continue };
            let e = last.entry(u.origin().ascii_serialization()).or_insert(0);
            *e = (*e).max(t);
        }
        Ok(last.into_iter().collect())
    }

    /// The most recent visit for `url`, if any.
    pub fn last_visit(&self, url: &str) -> DbResult<Option<i64>> {
        self.conn
            .query_row("SELECT last_visit FROM urls WHERE url = ?1 AND visit_count > 0", [url], |r| r.get(0))
            .optional()
    }

    /// Number of visits in the database (for tests and the about page).
    pub fn visit_total(&self) -> DbResult<i64> {
        self.conn.query_row("SELECT count(*) FROM visits", [], |r| r.get(0))
    }

    /// Recent URLs whose URL or title starts a word with `prefix`, for
    /// address-bar suggestions.
    pub fn suggest(&self, prefix: &str, limit: u32) -> DbResult<Vec<(String, String)>> {
        if prefix.trim().is_empty() {
            return Ok(Vec::new());
        }
        let pattern = like_pattern(prefix.trim());
        let mut stmt = self.conn.prepare_cached(
            "SELECT url, title FROM urls WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
             ORDER BY visit_count DESC, last_visit DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit.min(20)], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// History pages whose address starts with `start` once its scheme and `www.` are ignored
    /// (`start`: lowercase, no scheme), most visited first: `(url, title, visit_count)`. The
    /// address bar finds the site the user is typing with it (0.10.2).
    pub fn pages_starting_with(&self, start: &str, limit: u32) -> DbResult<Vec<(String, String, i64)>> {
        if start.is_empty() {
            return Ok(Vec::new());
        }
        let mut esc = String::with_capacity(start.len() + 1);
        for c in start.chars() {
            if matches!(c, '%' | '_' | '\\') {
                esc.push('\\');
            }
            esc.push(c);
        }
        esc.push('%');
        let mut stmt = self.conn.prepare_cached(
            "SELECT url, title, visit_count FROM urls
             WHERE url LIKE 'https://' || ?1 ESCAPE '\\' OR url LIKE 'http://' || ?1 ESCAPE '\\'
                OR url LIKE 'https://www.' || ?1 ESCAPE '\\' OR url LIKE 'http://www.' || ?1 ESCAPE '\\'
             ORDER BY visit_count DESC, last_visit DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![esc, limit.min(500)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// Up to `per_url` most recent visit timestamps per URL, for frecency.
    pub(crate) fn recent_visits(&self, url_ids: &[i64], per_url: u32) -> DbResult<Vec<Vec<i64>>> {
        let mut stmt = self.conn.prepare_cached("SELECT ts FROM visits WHERE url_id = ?1 ORDER BY ts DESC LIMIT ?2")?;
        url_ids.iter().map(|id| stmt.query_map(params![id, per_url], |r| r.get(0))?.collect()).collect()
    }

    /// Removes the given URLs (all visits) in one go.
    pub fn delete_urls(&self, urls: &[String]) -> DbResult<()> {
        if urls.is_empty() {
            return Ok(());
        }
        let marks = vec!["?"; urls.len()].join(",");
        self.conn.execute(&format!("DELETE FROM urls WHERE url IN ({marks})"), params_from_iter(urls))?;
        Ok(())
    }
}

/// Recomputes visit counts after visits were deleted and drops URLs with no
/// visits left.
fn recount(tx: &rusqlite::Transaction<'_>) -> DbResult<()> {
    tx.execute_batch(
        "UPDATE urls SET
            visit_count = (SELECT count(*) FROM visits WHERE url_id = urls.id),
            last_visit = coalesce((SELECT max(ts) FROM visits WHERE url_id = urls.id), 0);
         DELETE FROM urls WHERE visit_count = 0;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn seeded() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.record_visit("https://rust-lang.org/", "Rust Programming Language", 1000).unwrap();
        s.record_visit("https://docs.rs/serde", "serde - Docs.rs", 2000).unwrap();
        s.record_visit("https://rust-lang.org/", "Rust", 3000).unwrap();
        s.record_visit("https://example.com/100%_off", "Sale", 4000).unwrap();
        s
    }

    #[test]
    fn pages_by_time_and_id() {
        let s = Store::open_in_memory().unwrap();
        for n in 0..7 {
            s.record_visit(&format!("https://example.com/{n}"), "T", 5000).unwrap();
        }
        s.record_visit("https://example.com/older", "Older", 4000).unwrap();
        let mut ids = Vec::new();
        let mut before = None;
        loop {
            let page = s.query_history_page("", before, 3).unwrap();
            let Some(last) = page.last() else { break };
            before = Some((last.ts, last.id));
            ids.extend(page.iter().map(|e| e.id));
        }
        assert_eq!(ids.len(), 8, "every visit once");
        assert!(ids.windows(2).all(|w| w[0] > w[1]) || ids[..7].windows(2).all(|w| w[0] > w[1]));
        assert_eq!(s.query_history_page("older", None, 10).unwrap().len(), 1);
        assert_eq!(s.query_history_page("100%", None, 10).unwrap().len(), 0, "LIKE wildcards are escaped");
        // The same rows, in the same order, as the query by time alone gives in one page.
        let all: Vec<i64> = s.query_history("", None, 100).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(all, ids);
    }

    #[test]
    fn titles_in_one_batch() {
        let mut s = seeded();
        let titles = vec![
            ("https://rust-lang.org/".to_string(), "(3) Rust".to_string()),
            ("https://docs.rs/serde".to_string(), String::new()),
            ("https://never.visited/".to_string(), "Not recorded".to_string()),
        ];
        s.set_titles(&titles).unwrap();
        let all = s.query_history("", None, 100).unwrap();
        assert!(all.iter().filter(|e| e.url == "https://rust-lang.org/").all(|e| e.title == "(3) Rust"));
        assert!(
            all.iter().any(|e| e.url == "https://docs.rs/serde" && e.title == "serde - Docs.rs"),
            "empty titles are skipped"
        );
        assert!(all.iter().all(|e| e.url != "https://never.visited/"), "no new rows");
    }

    #[test]
    fn records_and_counts() {
        let s = seeded();
        let all = s.query_history("", None, 100).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].ts, 4000);
        let rust: Vec<_> = all.iter().filter(|e| e.url == "https://rust-lang.org/").collect();
        assert_eq!(rust.len(), 2);
        assert!(rust.iter().all(|e| e.visit_count == 2 && e.title == "Rust"));
        assert_eq!(s.last_visit("https://rust-lang.org/").unwrap(), Some(3000));
    }

    #[test]
    fn visit_policy() {
        assert_eq!(
            visit_url("https://a.example/?utm_source=x&id=1", false).as_deref(),
            Some("https://a.example/?id=1")
        );
        assert_eq!(visit_url("https://a.example/", true), None, "private tabs are never recorded");
        assert_eq!(visit_url(&crate::pages::Page::History.url(), false), None);
        assert_eq!(visit_url("https://shell.krunk.invalid/shell.html", false), None);
        assert_eq!(visit_url("about:blank", false), None);
        assert_eq!(
            visit_url("https://u:pw@a.example/", false).as_deref(),
            Some("https://a.example/"),
            "no credentials"
        );
    }

    #[test]
    fn skips_internal_and_non_web_urls() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.record_visit(&crate::pages::Page::Settings.url(), "Settings", 1).unwrap(), None);
        assert_eq!(s.record_visit("about:blank", "", 1).unwrap(), None);
        assert_eq!(s.record_visit("data:text/html,hi", "", 1).unwrap(), None);
        assert_eq!(s.visit_total().unwrap(), 0);
    }

    #[test]
    fn search_by_title_and_url_with_wildcards_escaped() {
        let s = seeded();
        let r = s.query_history("docs", None, 100).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url, "https://docs.rs/serde");
        assert_eq!(s.query_history("RUST", None, 100).unwrap().len(), 2);
        // '%' and '_' are literal, not wildcards.
        assert_eq!(s.query_history("100%_", None, 100).unwrap().len(), 1);
        assert_eq!(s.query_history("%", None, 100).unwrap().len(), 1);
        assert_eq!(s.query_history("_", None, 100).unwrap().len(), 1);
        // Paging.
        let page = s.query_history("", Some(3000), 100).unwrap();
        assert_eq!(page.iter().map(|e| e.ts).collect::<Vec<_>>(), vec![2000, 1000]);
    }

    #[test]
    fn delete_single_visit_recounts() {
        let mut s = seeded();
        let rust = s.query_history("rust-lang", None, 10).unwrap();
        let newest = rust[0].id;
        assert_eq!(s.delete_visits(&[newest]).unwrap(), 1);
        let rust = s.query_history("rust-lang", None, 10).unwrap();
        assert_eq!(rust.len(), 1);
        assert_eq!(rust[0].visit_count, 1);
        assert_eq!(s.last_visit("https://rust-lang.org/").unwrap(), Some(1000));
        // Deleting the last visit removes the URL completely.
        s.delete_visits(&[rust[0].id]).unwrap();
        assert_eq!(s.last_visit("https://rust-lang.org/").unwrap(), None);
        assert!(s.suggest("rust", 5).unwrap().is_empty());
    }

    #[test]
    fn delete_time_range() {
        let mut s = Store::open_in_memory().unwrap();
        for d in 0..10 {
            s.record_visit(&format!("https://site{d}.example/"), "", d * DAY).unwrap();
        }
        s.record_visit("https://site1.example/", "", 9 * DAY + 5).unwrap();
        // Delete days 2..5 (half-open).
        assert_eq!(s.delete_history_range(2 * DAY, 5 * DAY).unwrap(), 3);
        let left: Vec<_> = s.query_history("", None, 100).unwrap().into_iter().map(|e| e.ts / DAY).collect();
        assert_eq!(left, vec![9, 9, 8, 7, 6, 5, 1, 0]);
        assert_eq!(s.last_visit("https://site3.example/").unwrap(), None);
        // site1 keeps its recount after an unrelated range deletion.
        s.delete_history_range(9 * DAY + 1, 10 * DAY).unwrap();
        assert_eq!(s.last_visit("https://site1.example/").unwrap(), Some(DAY));
        s.clear_history().unwrap();
        assert_eq!(s.visit_total().unwrap(), 0);
    }

    #[test]
    fn delete_url_cascades_visits() {
        let s = seeded();
        s.delete_url("https://rust-lang.org/").unwrap();
        assert_eq!(s.visit_total().unwrap(), 2);
        s.delete_urls(&["https://docs.rs/serde".to_string()]).unwrap();
        assert_eq!(s.visit_total().unwrap(), 1);
    }

    #[test]
    fn origins_for_clearing_site_data() {
        let s = Store::open_in_memory().unwrap();
        s.record_visit("https://a.example/x", "", 1_000).unwrap();
        s.record_visit("https://a.example/y?q=1", "", 2_000).unwrap();
        s.record_visit("http://b.example:8080/", "", 3_000).unwrap();
        s.record_visit("https://c.example/", "", 500).unwrap();
        assert_eq!(
            s.visited_origins(None).unwrap(),
            ["http://b.example:8080", "https://a.example", "https://c.example"]
        );
        assert_eq!(s.visited_origins(Some(1_500)).unwrap(), ["http://b.example:8080", "https://a.example"]);
        assert!(s.visited_origins(Some(9_000)).unwrap().is_empty());
    }
}
