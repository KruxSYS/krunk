//! SQLite database: schema, migrations and small key/value helpers.
//!
//! One database (`browser.db`) holds history, bookmarks, new-tab tiles,
//! settings and the saved session. Passwords are *never* stored here; they
//! live encrypted in the separate vault file (see `vault`).

use crate::settings::Settings;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub use rusqlite::Error as DbError;
pub type DbResult<T> = Result<T, DbError>;

/// Id of the "Bookmarks bar" root folder.
pub const BOOKMARKS_BAR: i64 = 1;
/// Id of the "Other bookmarks" root folder.
pub const OTHER_BOOKMARKS: i64 = 2;

const SCHEMA_VERSION: i64 = 6;

/// Fixed sync guids of the two root folders (PROTOCOL §9.5): "bar" and "other".
pub const ROOT_GUID_BAR: [u8; 16] = [0x01; 16];
pub const ROOT_GUID_OTHER: [u8; 16] = [0x02; 16];

const SCHEMA_V1: &str = "
CREATE TABLE urls (
    id          INTEGER PRIMARY KEY,
    url         TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL DEFAULT '',
    visit_count INTEGER NOT NULL DEFAULT 0,
    last_visit  INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE visits (
    id     INTEGER PRIMARY KEY,
    url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    ts     INTEGER NOT NULL
);
CREATE INDEX visits_url_ts ON visits(url_id, ts);
CREATE INDEX visits_ts ON visits(ts);

CREATE TABLE bookmarks (
    id       INTEGER PRIMARY KEY,
    parent   INTEGER REFERENCES bookmarks(id) ON DELETE CASCADE,
    is_folder INTEGER NOT NULL,
    title    TEXT NOT NULL,
    url      TEXT,
    position INTEGER NOT NULL,
    added    INTEGER NOT NULL
);
CREATE INDEX bookmarks_parent ON bookmarks(parent, position);
CREATE INDEX bookmarks_url ON bookmarks(url);
INSERT INTO bookmarks(id, parent, is_folder, title, url, position, added)
    VALUES (1, NULL, 1, 'Bookmarks bar', NULL, 0, 0),
           (2, NULL, 1, 'Other bookmarks', NULL, 1, 0);

-- New-tab tiles the user pinned (state 1, ordered by position) or removed (state 2).
CREATE TABLE tiles (
    url      TEXT PRIMARY KEY,
    title    TEXT NOT NULL DEFAULT '',
    state    INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0
);

-- Favicons as data: URLs, keyed by host.
CREATE TABLE favicons (
    host TEXT PRIMARY KEY,
    data TEXT NOT NULL
);

CREATE TABLE kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// v2: favicons are stored as PNG bytes (served from the internal origins)
/// instead of data: URLs.
const SCHEMA_V2: &str = "
DROP TABLE favicons;
CREATE TABLE favicons (
    host    TEXT PRIMARY KEY,
    png     BLOB NOT NULL,
    updated INTEGER NOT NULL
);
";

/// v3: per-site ad-blocking allowlist (it used to live in the settings JSON).
const SCHEMA_V3: &str = "
CREATE TABLE adblock_allowlist (
    host  TEXT PRIMARY KEY,
    added INTEGER NOT NULL
);
";

/// v4: origins where the user chose "Never save passwords" (no secrets).
const SCHEMA_V4: &str = "
CREATE TABLE vault_never (
    origin TEXT PRIMARY KEY,
    added  INTEGER NOT NULL
);
";

// Krunk 0.5.0: the user's answers to site permission prompts (local network access).
const SCHEMA_V5: &str = "
CREATE TABLE site_permissions (
    origin TEXT NOT NULL,
    kind   TEXT NOT NULL,
    allow  INTEGER NOT NULL,
    added  INTEGER NOT NULL,
    PRIMARY KEY (origin, kind)
);
";

// Krunk 0.9.0 (sync S5; PROTOCOL §8 calls it "v5", but v5 was taken by 0.5.0): bookmark guids
// and modification times, and the sync bookkeeping (the single definition, PLAN §5.2):
// - `sync_items`: per synced item, its record id, server seq, version, padding bucket, flags and
//   collection-specific state (`extra`, JSON);
// - `sync_queue`: local changes waiting to be pushed, filled by the triggers below;
// - `sync_meta`: small state. The triggers only queue while `track` = '1' (sync on) and not while
//   the engine applies remote changes (`applying` present, set inside its own transaction). With
//   sync off they write nothing at all.
const SCHEMA_V6: &str = "
ALTER TABLE bookmarks ADD COLUMN guid BLOB;
ALTER TABLE bookmarks ADD COLUMN modified INTEGER NOT NULL DEFAULT 0;
UPDATE bookmarks SET guid = CASE id WHEN 1 THEN x'01010101010101010101010101010101'
                                    WHEN 2 THEN x'02020202020202020202020202020202'
                                    ELSE randomblob(16) END,
                     modified = added;
CREATE UNIQUE INDEX bookmarks_guid ON bookmarks(guid);
CREATE TRIGGER bookmarks_need_guid BEFORE INSERT ON bookmarks WHEN NEW.guid IS NULL OR length(NEW.guid) <> 16
BEGIN SELECT RAISE(ABORT, 'a bookmark needs a 16-byte guid'); END;

CREATE TABLE sync_items (
    collection  TEXT NOT NULL,
    natural_key TEXT NOT NULL,
    record_id   BLOB,
    server_seq  INTEGER NOT NULL DEFAULT 0,
    ver         INTEGER NOT NULL DEFAULT 0,
    bucket      INTEGER NOT NULL DEFAULT 0,
    dirty       INTEGER NOT NULL DEFAULT 0,
    deleted     INTEGER NOT NULL DEFAULT 0,
    origin_sync INTEGER NOT NULL DEFAULT 0,
    extra       TEXT,
    PRIMARY KEY (collection, natural_key)
);
CREATE INDEX sync_items_record ON sync_items(collection, record_id);
CREATE TABLE sync_queue (
    collection  TEXT NOT NULL,
    natural_key TEXT NOT NULL,
    enqueued_at INTEGER NOT NULL,
    PRIMARY KEY (collection, natural_key)
);
CREATE TABLE sync_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// The change triggers of v6 (`@TRACK` is replaced by [`TRACKING`]). Each queues the natural key
/// the engine needs: a bookmark's guid (hex), a history URL (as stored; the engine normalizes it),
/// a pinned or hidden tile's URL, an allowlisted host, a never-save origin, and the settings and
/// session records as a whole.
const SYNC_TRIGGERS: &str = "
CREATE TRIGGER sync_q_bookmarks_ins AFTER INSERT ON bookmarks WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('bookmarks', lower(hex(NEW.guid)), @NOW); END;
CREATE TRIGGER sync_q_bookmarks_upd AFTER UPDATE OF parent, position, title, url ON bookmarks WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('bookmarks', lower(hex(NEW.guid)), @NOW); END;
CREATE TRIGGER sync_q_bookmarks_del AFTER DELETE ON bookmarks WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('bookmarks', lower(hex(OLD.guid)), @NOW); END;
CREATE TRIGGER sync_q_visits_ins AFTER INSERT ON visits WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue SELECT 'history', url, @NOW FROM urls WHERE id = NEW.url_id; END;
CREATE TRIGGER sync_q_urls_upd AFTER UPDATE OF title ON urls WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('history', NEW.url, @NOW); END;
CREATE TRIGGER sync_q_urls_del AFTER DELETE ON urls WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('history', OLD.url, @NOW); END;
CREATE TRIGGER sync_q_tiles_ins AFTER INSERT ON tiles WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('pins', NEW.url, @NOW); END;
CREATE TRIGGER sync_q_tiles_upd AFTER UPDATE ON tiles WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('pins', NEW.url, @NOW);
      INSERT OR REPLACE INTO sync_queue SELECT 'pins', OLD.url, @NOW WHERE OLD.url <> NEW.url; END;
CREATE TRIGGER sync_q_tiles_del AFTER DELETE ON tiles WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('pins', OLD.url, @NOW); END;
CREATE TRIGGER sync_q_allow_ins AFTER INSERT ON adblock_allowlist WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('allowlist', NEW.host, @NOW); END;
CREATE TRIGGER sync_q_allow_del AFTER DELETE ON adblock_allowlist WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('allowlist', OLD.host, @NOW); END;
CREATE TRIGGER sync_q_never_ins AFTER INSERT ON vault_never WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('never_save', NEW.origin, @NOW); END;
CREATE TRIGGER sync_q_never_del AFTER DELETE ON vault_never WHEN @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES ('never_save', OLD.origin, @NOW); END;
CREATE TRIGGER sync_q_kv_ins AFTER INSERT ON kv WHEN NEW.key IN ('settings', 'session') AND @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES (CASE NEW.key WHEN 'settings' THEN 'settings' ELSE 'tabs' END, '*', @NOW); END;
CREATE TRIGGER sync_q_kv_upd AFTER UPDATE ON kv WHEN NEW.key IN ('settings', 'session') AND NEW.value <> OLD.value AND @TRACK
BEGIN INSERT OR REPLACE INTO sync_queue VALUES (CASE NEW.key WHEN 'settings' THEN 'settings' ELSE 'tabs' END, '*', @NOW); END;
";

/// The triggers' condition: sync is on and the engine isn't applying remote changes.
const TRACKING: &str = "(EXISTS (SELECT 1 FROM sync_meta WHERE key = 'track' AND value = '1') \
     AND NOT EXISTS (SELECT 1 FROM sync_meta WHERE key = 'applying'))";

fn sync_triggers() -> String {
    SYNC_TRIGGERS.replace("@TRACK", TRACKING).replace("@NOW", "(CAST(strftime('%s', 'now') AS INTEGER) * 1000)")
}

/// `YYYY-MM-DD` of a Unix-millisecond time (UTC; folder names, not display).
pub fn date_ymd(ms: i64) -> String {
    // Howard Hinnant's civil-from-days.
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Largest favicon we keep.
pub const MAX_FAVICON_BYTES: usize = 48 * 1024;

/// Handle to the browser database.
pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> DbResult<Store> {
        Store::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> DbResult<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> DbResult<Store> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Deleted history should really be gone from the file.
        conn.pragma_update(None, "secure_delete", "ON")?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> DbResult<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V2)?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V3)?;
            // Move an allowlist from an older settings record into the table.
            let old: Option<String> =
                tx.query_row("SELECT value FROM kv WHERE key = 'settings'", [], |r| r.get(0)).optional()?;
            if let Some(json) = old {
                let hosts = serde_json::from_str::<serde_json::Value>(&json)
                    .ok()
                    .and_then(|v| v.get("adblockAllowlist").cloned())
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
                    .unwrap_or_default();
                for h in hosts.iter().filter_map(|h| crate::blocker::normalize_host(h)) {
                    tx.execute("INSERT OR IGNORE INTO adblock_allowlist(host, added) VALUES (?1, 0)", [h])?;
                }
            }
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V4)?;
            tx.pragma_update(None, "user_version", 4)?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", 5)?;
            tx.commit()?;
        }
        if version < 6 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V6)?;
            tx.execute_batch(&sync_triggers())?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> DbResult<Option<String>> {
        self.conn.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0)).optional()
    }

    pub fn kv_set(&self, key: &str, value: &str) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO kv(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn kv_delete(&self, key: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Loads settings; a missing or corrupt record yields valid defaults.
    pub fn load_settings(&self) -> DbResult<Settings> {
        Ok(self.kv_get("settings")?.map(|j| Settings::from_json_lenient(&j)).unwrap_or_default())
    }

    /// Saves settings. Callers must have validated them.
    pub fn save_settings(&self, s: &Settings) -> DbResult<()> {
        let json = serde_json::to_string(s).map_err(|e| DbError::ToSqlConversionFailure(Box::new(e)))?;
        self.kv_set("settings", &json)
    }

    /// Stores a host's favicon (PNG bytes, as captured from the page).
    pub fn set_favicon(&self, host: &str, png: &[u8], now: i64) -> DbResult<()> {
        if png.is_empty() || png.len() > MAX_FAVICON_BYTES || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO favicons(host, png, updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET png = excluded.png, updated = excluded.updated",
            params![host, png, now],
        )?;
        Ok(())
    }

    pub fn favicon(&self, host: &str) -> DbResult<Option<Vec<u8>>> {
        self.conn.query_row("SELECT png FROM favicons WHERE host = ?1", [host], |r| r.get(0)).optional()
    }

    pub fn has_favicon(&self, host: &str) -> DbResult<bool> {
        Ok(self.conn.query_row("SELECT 1 FROM favicons WHERE host = ?1", [host], |_| Ok(())).optional()?.is_some())
    }

    /// Same-origin URL of a host's cached favicon, if there is one.
    pub fn favicon_url(&self, host: &str) -> DbResult<Option<String>> {
        Ok(self.has_favicon(host)?.then(|| crate::assets::favicon_url(host)))
    }

    /// All allowlisted hosts, sorted.
    pub fn allowlist(&self) -> DbResult<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT host FROM adblock_allowlist ORDER BY host")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    /// Adds a host (normalized; invalid names are rejected). Returns the
    /// stored form.
    pub fn allowlist_add(&self, host: &str, now: i64) -> DbResult<String> {
        let h = crate::blocker::normalize_host(host)
            .ok_or_else(|| DbError::InvalidParameterName("Not a valid host name".into()))?;
        self.conn.execute("INSERT OR IGNORE INTO adblock_allowlist(host, added) VALUES (?1, ?2)", params![h, now])?;
        Ok(h)
    }

    pub fn allowlist_remove(&self, host: &str) -> DbResult<bool> {
        let Some(h) = crate::blocker::normalize_host(host) else { return Ok(false) };
        Ok(self.conn.execute("DELETE FROM adblock_allowlist WHERE host = ?1", [h])? > 0)
    }

    /// "Never save passwords" list: exact origins (`scheme://host[:port]`).
    pub fn never_save(&self, origin: &str) -> DbResult<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM vault_never WHERE origin = ?1", [origin], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn never_save_add(&self, origin: &str, now: i64) -> DbResult<()> {
        let o =
            crate::urlclean::origin_of(origin).ok_or_else(|| DbError::InvalidParameterName("Not an origin".into()))?;
        self.conn.execute("INSERT OR IGNORE INTO vault_never(origin, added) VALUES (?1, ?2)", params![o, now])?;
        Ok(())
    }

    pub fn never_save_remove(&self, origin: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM vault_never WHERE origin = ?1", [origin])?;
        Ok(())
    }

    pub fn never_save_list(&self) -> DbResult<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT origin FROM vault_never ORDER BY origin")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    /// Persisted unlock rate limiter (failure count and time; no secrets).
    pub fn load_rate_limiter(&self) -> DbResult<crate::vault::RateLimiter> {
        Ok(self.kv_get("vault_rate")?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
    }

    pub fn save_rate_limiter(&self, r: &crate::vault::RateLimiter) -> DbResult<()> {
        let json = serde_json::to_string(r).map_err(|e| DbError::ToSqlConversionFailure(Box::new(e)))?;
        self.kv_set("vault_rate", &json)
    }

    /// Reclaims space after large deletions.
    pub fn vacuum(&self) -> DbResult<()> {
        self.conn.execute_batch("VACUUM")
    }
}

#[cfg(test)]
impl Store {
    pub(crate) fn kv_set_meta(&self, key: &str, value: &str) {
        self.conn.execute("INSERT OR REPLACE INTO sync_meta(key, value) VALUES (?1, ?2)", params![key, value]).unwrap();
    }
}

/// Escapes `%`, `_` and `\` for a `LIKE ... ESCAPE '\'` pattern.
pub(crate) fn like_pattern(text: &str) -> String {
    let mut p = String::with_capacity(text.len() + 2);
    p.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// Current time in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_ymd_is_civil_utc() {
        assert_eq!(date_ymd(0), "1970-01-01");
        assert_eq!(date_ymd(1_794_000_000_000), "2026-11-06");
        assert_eq!(date_ymd(951_782_400_000), "2000-02-29");
    }

    #[test]
    fn opens_and_migrates_idempotently() {
        let dir = std::env::temp_dir().join(format!("lb-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db");
        {
            let s = Store::open(&path).unwrap();
            s.kv_set("a", "1").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.kv_get("a").unwrap().as_deref(), Some("1"));
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_persist() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.load_settings().unwrap(), Settings::default());
        let custom = Settings { https_only: true, tab_sleep_minutes: 12, ..Settings::default() };
        s.save_settings(&custom).unwrap();
        assert_eq!(s.load_settings().unwrap(), custom);
        s.kv_set("settings", "{broken").unwrap();
        assert_eq!(s.load_settings().unwrap(), Settings::default());
    }

    #[test]
    fn allowlist_crud() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.allowlist_add("WWW.Example.com", 1).unwrap(), "www.example.com");
        assert_eq!(s.allowlist_add("*.news.org", 1).unwrap(), "news.org");
        s.allowlist_add("news.org", 2).unwrap();
        assert!(s.allowlist_add("bad host", 1).is_err());
        assert_eq!(s.allowlist().unwrap(), ["news.org", "www.example.com"]);
        assert!(s.allowlist_remove("NEWS.org").unwrap());
        assert!(!s.allowlist_remove("news.org").unwrap());
        assert_eq!(s.allowlist().unwrap(), ["www.example.com"]);
    }

    #[test]
    fn v3_migrates_settings_allowlist() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA_V1).unwrap();
        c.execute_batch(SCHEMA_V2).unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
        c.execute(
            "INSERT INTO kv(key, value) VALUES ('settings', ?1)",
            [r#"{"adblockAllowlist":["Shop.example","not valid"],"httpsOnly":true}"#],
        )
        .unwrap();
        let s = Store::init(c).unwrap();
        assert_eq!(s.allowlist().unwrap(), ["shop.example"]);
        assert!(s.load_settings().unwrap().https_only, "other settings kept");
    }

    #[test]
    fn never_save_list_and_rate_limiter() {
        let s = Store::open_in_memory().unwrap();
        s.never_save_add("https://bank.example/login?x=1", 1).unwrap();
        assert!(s.never_save("https://bank.example").unwrap());
        assert!(!s.never_save("https://bank.example:8443").unwrap());
        assert!(s.never_save_add("data:text/html,x", 1).is_err());
        assert_eq!(s.never_save_list().unwrap(), ["https://bank.example"]);
        s.never_save_remove("https://bank.example").unwrap();
        assert!(s.never_save_list().unwrap().is_empty());

        let mut r = s.load_rate_limiter().unwrap();
        assert_eq!(r.failures, 0);
        r.record_failure(5000);
        r.record_failure(6000);
        s.save_rate_limiter(&r).unwrap();
        let back = s.load_rate_limiter().unwrap();
        assert_eq!(back, r);
        assert_eq!(back.wait_ms(6000), 2000, "delay survives a restart");
    }

    #[test]
    fn like_escaping() {
        assert_eq!(like_pattern("50%_a\\b"), "%50\\%\\_a\\\\b%");
    }

    #[test]
    fn favicons_only_accept_png() {
        let s = Store::open_in_memory().unwrap();
        s.set_favicon("a.com", b"<svg onload=alert(1)>", 1).unwrap();
        assert_eq!(s.favicon("a.com").unwrap(), None);
        assert_eq!(s.favicon_url("a.com").unwrap(), None);
        let png = b"\x89PNG\r\n\x1a\nxyz";
        s.set_favicon("a.com", png, 1).unwrap();
        assert_eq!(s.favicon("a.com").unwrap().as_deref(), Some(&png[..]));
        assert_eq!(s.favicon_url("a.com").unwrap().as_deref(), Some("/favicon?host=a.com"));
        s.set_favicon("big.com", &[png.as_slice(), &[0u8; MAX_FAVICON_BYTES]].concat(), 1).unwrap();
        assert_eq!(s.favicon("big.com").unwrap(), None);
    }

    #[test]
    fn migrates_v1_database() {
        let dir = std::env::temp_dir().join(format!("lb-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(SCHEMA_V1).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
            c.execute(
                "INSERT INTO urls(url, title, visit_count, last_visit) VALUES ('https://a.example/', 'A', 1, 5)",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.last_visit("https://a.example/").unwrap(), Some(5), "data kept");
        s.set_favicon("a.example", b"\x89PNG\r\n\x1a\n", 1).unwrap();
        let v: i64 = s.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---------------------------------------------------------------- v6 (0.9.0, sync S5)

    /// A copy of a frozen fixture in a fresh temporary folder.
    fn fixture_copy(name: &str, tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("lb-fixture-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage").join(name);
        let path = dir.join("browser.db");
        std::fs::copy(src, &path).unwrap();
        (dir, path)
    }

    fn count(s: &Store, sql: &str) -> i64 {
        s.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_0_8_0_database_migrates_to_v6_without_losing_anything() {
        let (dir, path) = fixture_copy("browser-v5-0.8.0.db", "v5");
        {
            let c = Connection::open(&path).unwrap();
            let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(v, 5, "the fixture is a 0.8.0 (v5) database");
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(count(&s, "PRAGMA user_version"), 6);
        // Every row is still there.
        assert_eq!(count(&s, "SELECT count(*) FROM bookmarks"), 11);
        assert_eq!(count(&s, "SELECT count(*) FROM urls"), 3);
        assert_eq!(count(&s, "SELECT count(*) FROM visits"), 6);
        assert_eq!(count(&s, "SELECT count(*) FROM tiles"), 3);
        assert_eq!(s.allowlist().unwrap(), vec!["example.org".to_string()]);
        assert_eq!(s.never_save_list().unwrap(), vec!["https://never.example".to_string()]);
        assert_eq!(s.permission("http://localhost:8094", "lna").unwrap(), Some(true));
        assert!(s.favicon("example.com").unwrap().is_some());
        assert_eq!(s.load_settings().unwrap().homepage, "https://start.example/");
        assert!(s.kv_get("session").unwrap().unwrap().contains("https://example.com/"));
        let tree = s.bookmark_tree().unwrap();
        assert_eq!(tree.len(), 2);
        // Guids: the roots fixed, every other one 16 random bytes, all unique; modified = added.
        let root = |id: i64| -> Vec<u8> {
            s.conn.query_row("SELECT guid FROM bookmarks WHERE id = ?1", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(root(BOOKMARKS_BAR), ROOT_GUID_BAR.to_vec());
        assert_eq!(root(OTHER_BOOKMARKS), ROOT_GUID_OTHER.to_vec());
        assert_eq!(count(&s, "SELECT count(DISTINCT guid) FROM bookmarks WHERE length(guid) = 16"), 11);
        assert_eq!(count(&s, "SELECT count(*) FROM bookmarks WHERE modified <> added"), 0);
        // The sync tables exist and are empty; nothing was queued (sync is off).
        for t in ["sync_items", "sync_queue", "sync_meta"] {
            assert_eq!(count(&s, &format!("SELECT count(*) FROM {t}")), 0, "{t}");
        }
        // Reopening is a no-op.
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(count(&s, "SELECT count(DISTINCT guid) FROM bookmarks"), 11);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn new_bookmarks_get_guids_and_raw_inserts_without_one_fail() {
        let mut s = Store::open_in_memory().unwrap();
        let a = s.add_bookmark(BOOKMARKS_BAR, "A", "https://a.example/").unwrap();
        let f = s.add_folder(OTHER_BOOKMARKS, "F").unwrap();
        let g = |id: i64| -> Vec<u8> {
            s.conn.query_row("SELECT guid FROM bookmarks WHERE id = ?1", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(g(a).len(), 16);
        assert_ne!(g(a), g(f));
        assert_eq!(g(BOOKMARKS_BAR), ROOT_GUID_BAR.to_vec(), "a new database gets the fixed root guids too");
        let raw = s.conn.execute(
            "INSERT INTO bookmarks(parent, is_folder, title, url, position, added) VALUES (1, 0, 'x', 'https://x.example/', 9, 0)",
            [],
        );
        assert!(raw.is_err(), "an insert without a guid is refused");
        // An edit and a move update `modified`.
        s.conn.execute("UPDATE bookmarks SET modified = 0", []).unwrap();
        s.edit_bookmark(a, "A2", None).unwrap();
        s.move_bookmark(f, BOOKMARKS_BAR, 0).unwrap();
        assert_eq!(count(&s, "SELECT count(*) FROM bookmarks WHERE modified > 0"), 2);
    }

    fn queue(s: &Store) -> Vec<(String, String)> {
        let mut st =
            s.conn.prepare("SELECT collection, natural_key FROM sync_queue ORDER BY collection, natural_key").unwrap();
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<DbResult<_>>().unwrap()
    }

    fn guid_hex(s: &Store, id: i64) -> String {
        s.conn.query_row("SELECT lower(hex(guid)) FROM bookmarks WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn changes_are_queued_only_while_sync_tracks_them() {
        let mut s = Store::open_in_memory().unwrap();
        // Sync off: nothing is queued, whatever changes.
        let a = s.add_bookmark(BOOKMARKS_BAR, "A", "https://a.example/").unwrap();
        s.record_visit("https://h.example/", "H", 5).unwrap();
        s.pin_site("https://p.example/", "P").unwrap();
        s.allowlist_add("allow.example", 1).unwrap();
        s.never_save_add("https://n.example/", 1).unwrap();
        s.save_settings(&Settings::default()).unwrap();
        s.kv_set("session", "{}").unwrap();
        assert!(queue(&s).is_empty(), "sync off: no queue");

        s.kv_set_meta("track", "1");
        let folder = s.add_folder(BOOKMARKS_BAR, "F").unwrap();
        let child = s.add_bookmark(folder, "C", "https://c.example/").unwrap();
        s.edit_bookmark(a, "A renamed", None).unwrap();
        s.record_visit("https://h.example/", "H", 6).unwrap();
        s.pin_site("https://p2.example/", "P2").unwrap();
        s.allowlist_add("allow2.example", 2).unwrap();
        s.never_save_add("https://n2.example/", 2).unwrap();
        let st = Settings { homepage: "https://home.example/".into(), ..Default::default() };
        s.save_settings(&st).unwrap();
        s.kv_set("session", r#"{"tabs":[]}"#).unwrap();
        let (fa, fc, ff) = (guid_hex(&s, a), guid_hex(&s, child), guid_hex(&s, folder));
        let mut want = vec![
            ("allowlist".to_string(), "allow2.example".to_string()),
            ("bookmarks".into(), fa.clone()),
            ("bookmarks".into(), fc.clone()),
            ("bookmarks".into(), ff.clone()),
            ("history".into(), "https://h.example/".into()),
            ("never_save".into(), "https://n2.example".into()),
            ("pins".into(), "https://p2.example/".into()),
            ("settings".into(), "*".into()),
            ("tabs".into(), "*".into()),
        ];
        want.sort();
        assert_eq!(queue(&s), want);

        // Deleting a folder queues it and its children (the foreign-key cascade fires the trigger).
        s.conn.execute("DELETE FROM sync_queue", []).unwrap();
        s.delete_bookmark(folder).unwrap();
        let mut got: Vec<String> = queue(&s).into_iter().map(|q| q.1).collect();
        got.sort();
        let mut both = vec![fc.clone(), ff.clone()];
        both.sort();
        assert_eq!(got, both);

        // A move queues the moved item and only the siblings whose place changed.
        s.conn.execute("DELETE FROM sync_queue", []).unwrap();
        let b = s.add_bookmark(BOOKMARKS_BAR, "B", "https://b.example/").unwrap();
        let c = s.add_bookmark(BOOKMARKS_BAR, "C", "https://c2.example/").unwrap();
        s.conn.execute("DELETE FROM sync_queue", []).unwrap();
        s.move_bookmark(c, BOOKMARKS_BAR, 1).unwrap();
        let mut got: Vec<String> = queue(&s).into_iter().map(|q| q.1).collect();
        got.sort();
        let mut moved = vec![guid_hex(&s, b), guid_hex(&s, c)];
        moved.sort();
        assert_eq!(got, moved, "a ({}) kept its place and isn't queued", fa);

        // History deletion queues the URL; clearing the queue, then applying remote changes
        // (`applying`) queues nothing.
        s.conn.execute("DELETE FROM sync_queue", []).unwrap();
        s.delete_url("https://h.example/").unwrap();
        assert_eq!(queue(&s), vec![("history".to_string(), "https://h.example/".to_string())]);
        s.conn.execute("DELETE FROM sync_queue", []).unwrap();
        s.kv_set_meta("applying", "1");
        s.add_bookmark(OTHER_BOOKMARKS, "Remote", "https://r.example/").unwrap();
        s.allowlist_remove("allow2.example").unwrap();
        assert!(queue(&s).is_empty(), "remote changes are not queued again");
        s.conn.execute("DELETE FROM sync_meta WHERE key = 'applying'", []).unwrap();
        s.allowlist_remove("allow.example").unwrap();
        assert_eq!(queue(&s), vec![("allowlist".to_string(), "allow.example".to_string())]);
    }
}
