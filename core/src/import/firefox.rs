//! A Firefox profile: `logins.json` + `key4.db` (passwords), `places.sqlite` (history and
//! bookmarks). All files are copies the host made; nothing is written.

use std::path::Path;

use serde::Deserialize;
use zeroize::Zeroizing;

use super::nss::{FieldError, Key4};
use super::{is_web_url, open_readonly, us_to_ms, ImportError, Login, Visit};
use crate::netscape::ImportNode;

/// Logins and the entries that could not be used, by reason.
pub struct FirefoxLogins {
    pub logins: Vec<Login>,
    pub skipped: usize,
    /// Why entries were skipped (reason, count), in a fixed order (0.7.0, R25; 0.7.2: each
    /// reason a field could not be decrypted on its own).
    pub reasons: Vec<(&'static str, usize)>,
    /// What the import found, for the log (no secrets).
    pub found: Found,
}

/// The profile's login store, as found (0.7.2, R12).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Found {
    /// Entries in `logins.json`.
    pub entries: usize,
    /// SDR keys in `key4.db`, and how many could not be unlocked.
    pub keys: usize,
    pub unusable_keys: usize,
    pub primary_password: bool,
}

#[derive(Deserialize)]
struct LoginsFile {
    /// Each entry read on its own: one odd entry never stops the others (0.7.0).
    #[serde(default)]
    logins: Vec<serde_json::Value>,
}

/// One entry, every field optional (Firefox versions and tools that write `logins.json`
/// differ: some leave out `hostname` and keep the origin in `formSubmitURL`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLogin {
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default, rename = "formSubmitURL")]
    form_submit_url: Option<String>,
    #[serde(default)]
    form_action_origin: Option<String>,
    #[serde(default)]
    http_realm: Option<String>,
    #[serde(default)]
    encrypted_username: Option<String>,
    #[serde(default)]
    encrypted_password: Option<String>,
    #[serde(default)]
    time_created: Option<i64>,
}

pub const SKIP_NO_ADDRESS: &str = "no website address";
pub const SKIP_NO_PASSWORD: &str = "no password";
pub const SKIP_UNREADABLE: &str = "unreadable entry";

impl RawLogin {
    /// The site: `hostname`, else `origin`, else the origin of the form's action, else an
    /// HTTP-auth realm that is an address.
    fn site(&self) -> Option<String> {
        [&self.hostname, &self.origin, &self.form_submit_url, &self.form_action_origin, &self.http_realm]
            .into_iter()
            .flatten()
            .find_map(|u| crate::urlclean::origin_of(u.trim()))
    }
}

/// Reads the saved logins of a profile directory (`key4.db` and `logins.json` inside).
/// `primary_password` is the Firefox primary password, "" when none is set.
pub fn read_logins(profile: &Path, primary_password: &str) -> Result<FirefoxLogins, ImportError> {
    let logins_path = profile.join("logins.json");
    if !logins_path.is_file() {
        return Err(ImportError::NotFound("logins.json".into()));
    }
    let text = std::fs::read_to_string(&logins_path).map_err(|e| ImportError::Read(format!("logins.json: {e}")))?;
    // (A file saved by another tool may start with a byte-order mark.)
    let file: LoginsFile = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| ImportError::Format(format!("logins.json: {e}")))?;
    let mut out = FirefoxLogins { logins: Vec::new(), skipped: 0, reasons: Vec::new(), found: Found::default() };
    out.found.entries = file.logins.len();
    if file.logins.is_empty() {
        return Ok(out);
    }
    let key4 = Key4::read(&profile.join("key4.db"))?;
    out.found.keys = key4.key_count();
    out.found.primary_password = key4.has_primary_password();
    let key = key4.unlock(primary_password)?;
    out.found.unusable_keys = key.unusable;
    let undecryptable = [
        FieldError::Missing,
        FieldError::NotBase64,
        FieldError::Structure,
        FieldError::UnknownKey,
        FieldError::UnsupportedCipher,
        FieldError::Damaged,
        FieldError::NotUtf8,
    ]
    .map(FieldError::reason);
    let mut counts: Vec<(&'static str, usize)> = [SKIP_NO_ADDRESS]
        .into_iter()
        .chain(undecryptable)
        .chain([SKIP_NO_PASSWORD, SKIP_UNREADABLE])
        .map(|r| (r, 0))
        .collect();
    let mut skip = |reason: &'static str| {
        if let Some(c) = counts.iter_mut().find(|(r, _)| *r == reason) {
            c.1 += 1;
        }
    };
    for value in file.logins {
        let Ok(raw) = serde_json::from_value::<RawLogin>(value) else {
            skip(SKIP_UNREADABLE);
            continue;
        };
        let Some(site) = raw.site() else {
            skip(SKIP_NO_ADDRESS);
            continue;
        };
        let decrypt = |f: &Option<String>| key.decrypt_field(f.as_deref().unwrap_or(""));
        // Each entry on its own: one that does not decrypt never stops the others.
        let (username, password) = match (decrypt(&raw.encrypted_username), decrypt(&raw.encrypted_password)) {
            (Ok(u), Ok(p)) => (u, p),
            (Err(e), _) | (_, Err(e)) => {
                skip(e.reason());
                continue;
            }
        };
        if password.is_empty() {
            skip(SKIP_NO_PASSWORD);
            continue;
        }
        out.logins.push(Login {
            url: site,
            username: username.to_string(),
            password,
            note: Zeroizing::new(String::new()),
            created_ms: raw.time_created.filter(|t| *t > 0),
        });
    }
    out.reasons = counts.into_iter().filter(|(_, n)| *n > 0).collect();
    out.skipped = out.reasons.iter().map(|(_, n)| n).sum();
    Ok(out)
}

/// Every visit of a web page in `places.sqlite`.
pub fn read_history(places: &Path) -> Result<Vec<Visit>, ImportError> {
    let conn = open_readonly(places)?;
    let fmt = |e: rusqlite::Error| ImportError::Format(format!("places.sqlite: {e}"));
    let mut stmt = conn
        .prepare(
            "SELECT p.url, IFNULL(p.title, ''), v.visit_date FROM moz_historyvisits v
             JOIN moz_places p ON p.id = v.place_id WHERE p.hidden = 0 ORDER BY v.visit_date",
        )
        .map_err(fmt)?;
    let rows = stmt
        .query_map([], |r| Ok(Visit { url: r.get(0)?, title: r.get(1)?, ts_ms: us_to_ms(r.get::<_, i64>(2)?) }))
        .map_err(fmt)?;
    let mut out = Vec::new();
    for v in rows {
        let v = v.map_err(fmt)?;
        if is_web_url(&v.url) && v.ts_ms > 0 {
            out.push(v);
        }
    }
    Ok(out)
}

/// The bookmark tree of `places.sqlite`: the toolbar as the toolbar folder, the menu and
/// unfiled bookmarks at the top level (they end up under "Other bookmarks"), mobile
/// bookmarks as a folder when there are any.
pub fn read_bookmarks(places: &Path) -> Result<Vec<ImportNode>, ImportError> {
    let conn = open_readonly(places)?;
    let fmt = |e: rusqlite::Error| ImportError::Format(format!("places.sqlite: {e}"));
    struct Row {
        id: i64,
        kind: i64,
        parent: i64,
        title: String,
        url: Option<String>,
        added_s: i64,
        guid: String,
    }
    let mut stmt = conn
        .prepare(
            "SELECT b.id, b.type, IFNULL(b.parent, 0), IFNULL(b.title, ''), p.url, IFNULL(b.dateAdded, 0), IFNULL(b.guid, '')
             FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk ORDER BY b.parent, b.position, b.id",
        )
        .map_err(fmt)?;
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                kind: r.get(1)?,
                parent: r.get(2)?,
                title: r.get(3)?,
                url: r.get(4)?,
                added_s: us_to_ms(r.get::<_, i64>(5)?) / 1000,
                guid: r.get(6)?,
            })
        })
        .map_err(fmt)?
        .collect::<Result<_, _>>()
        .map_err(fmt)?;
    fn build(rows: &[Row], parent: i64, depth: usize) -> Vec<ImportNode> {
        if depth > 32 {
            return Vec::new();
        }
        rows.iter()
            .filter(|r| r.parent == parent && r.id != parent)
            .filter_map(|r| match r.kind {
                1 => r.url.clone().filter(|u| is_web_url(u)).map(|url| ImportNode::Link {
                    title: r.title.clone(),
                    url,
                    add_date: r.added_s,
                }),
                2 => Some(ImportNode::Folder {
                    title: r.title.clone(),
                    toolbar: false,
                    add_date: r.added_s,
                    children: build(rows, r.id, depth + 1),
                }),
                _ => None,
            })
            .collect()
    }
    let root_id = |guid: &str| rows.iter().find(|r| r.guid == guid).map(|r| r.id);
    let mut out = Vec::new();
    if let Some(id) = root_id("toolbar_____") {
        out.push(ImportNode::Folder {
            title: "Bookmarks bar".into(),
            toolbar: true,
            add_date: 0,
            children: build(&rows, id, 0),
        });
    }
    for guid in ["menu________", "unfiled_____"] {
        if let Some(id) = root_id(guid) {
            out.extend(build(&rows, id, 0));
        }
    }
    if let Some(id) = root_id("mobile______") {
        let children = build(&rows, id, 0);
        if !children.is_empty() {
            out.push(ImportNode::Folder { title: "Mobile bookmarks".into(), toolbar: false, add_date: 0, children });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testing::fixture;

    /// A real profile (LB_FIREFOX_PROFILE=<dir>, LB_FIREFOX_PASSWORD optional): run with
    /// `cargo test -p core --lib real_firefox_profile -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_firefox_profile_from_env() {
        let dir = std::env::var("LB_FIREFOX_PROFILE").expect("LB_FIREFOX_PROFILE");
        let pw = std::env::var("LB_FIREFOX_PASSWORD").unwrap_or_default();
        let got = read_logins(Path::new(&dir), &pw).unwrap();
        println!("logins: {} read, {} skipped", got.logins.len(), got.skipped);
        for l in &got.logins {
            println!("  {} {} (password {} chars)", l.url, l.username, l.password.chars().count());
        }
        let history = read_history(&Path::new(&dir).join("places.sqlite")).unwrap();
        println!("history visits: {} (first: {:?})", history.len(), history.first());
        let bookmarks = read_bookmarks(&Path::new(&dir).join("places.sqlite")).unwrap();
        println!("bookmark nodes: {bookmarks:?}");
        assert!(!got.logins.is_empty());
    }

    #[test]
    fn entries_without_hostname_and_odd_ones() {
        // 0.7.0 (R25): the operator's Firefox wrote entries without `hostname`.
        let got = read_logins(&fixture("firefox-nohostname"), "").unwrap();
        let names: Vec<(&str, &str)> = got.logins.iter().map(|l| (l.url.as_str(), l.username.as_str())).collect();
        assert_eq!(
            names,
            [
                ("https://login.example", "fx-alice"),
                ("https://accounts.forms.example", "no-host-user"),
                ("https://realm.example", "realm-user")
            ]
        );
        assert_eq!(got.logins[1].password.as_str(), "No-host-pass-2");
        assert_eq!(
            got.reasons,
            [(SKIP_NO_ADDRESS, 1), (FieldError::Structure.reason(), 1), (SKIP_NO_PASSWORD, 1), (SKIP_UNREADABLE, 2)]
        );
        assert_eq!(got.skipped, 5);
    }

    #[test]
    fn two_keys_mixed_ciphers_all_logins() {
        // 0.7.2 (R12): logins under an old 3DES key and under the newer AES key of the same
        // profile. 0.7.1 read the first key only and imported 1 of these 3.
        let got = read_logins(&fixture("firefox-twokeys"), "").unwrap();
        let names: Vec<(&str, &str, &str)> =
            got.logins.iter().map(|l| (l.url.as_str(), l.username.as_str(), l.password.as_str())).collect();
        assert_eq!(
            names,
            [
                ("https://new-aes.example", "fx-new-1", "Fx-new-pass-1"),
                ("https://old-3des.example", "fx-old-2", "Fx-old-pass-2"),
                ("https://new-aes.example", "fx-new-3", "Fx-new-pass-3"),
            ]
        );
        assert_eq!(got.skipped, 0);
        assert_eq!(got.found, Found { entries: 3, keys: 2, unusable_keys: 0, primary_password: false });
    }

    #[test]
    fn a_login_under_a_key_that_is_gone_says_so() {
        // The same profile with its AES key row removed: those logins name a key key4.db no
        // longer has; the reason says so (never a silent 0).
        let src = fixture("firefox-twokeys");
        let dir = std::env::temp_dir().join(format!("krunk-gone-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["key4.db", "logins.json"] {
            std::fs::copy(src.join(f), dir.join(f)).unwrap();
        }
        let c = rusqlite::Connection::open(dir.join("key4.db")).unwrap();
        c.execute("DELETE FROM nssPrivate WHERE id = 2", []).unwrap();
        drop(c);
        let got = read_logins(&dir, "").unwrap();
        assert_eq!(got.logins.len(), 1);
        assert_eq!(got.reasons, [(FieldError::UnknownKey.reason(), 2)]);
        assert_eq!(got.found.keys, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_store_is_found_empty() {
        let dir = std::env::temp_dir().join(format!("krunk-empty-logins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("logins.json"), r#"{"nextId":1,"logins":[],"version":3}"#).unwrap();
        let got = read_logins(&dir, "").unwrap();
        assert!(got.logins.is_empty() && got.reasons.is_empty());
        assert_eq!(got.found.entries, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn logins_without_primary_password() {
        let got = read_logins(&fixture("firefox-nomaster"), "").unwrap();
        assert_eq!(got.skipped, 0);
        let names: Vec<(&str, &str)> = got.logins.iter().map(|l| (l.url.as_str(), l.username.as_str())).collect();
        assert_eq!(
            names,
            [
                ("https://login.example", "fx-alice"),
                ("https://shop.example", "fx-bob"),
                ("http://intranet.example", "fx-carol")
            ]
        );
        assert_eq!(got.logins[0].password.as_str(), "Fx-pass-alice-1");
        assert_eq!(got.logins[2].password.as_str(), "Fx-pass-carol-3");
        assert_eq!(got.logins[0].created_ms, Some(1_700_000_001_000));
    }

    #[test]
    fn logins_with_primary_password_legacy_and_wrong() {
        let got = read_logins(&fixture("firefox-master"), "fx-master-pw").unwrap();
        assert_eq!(got.logins.len(), 3);
        assert_eq!(got.logins[1].password.as_str(), "Fx-pass-bob-2");
        assert!(matches!(read_logins(&fixture("firefox-master"), "nope"), Err(ImportError::WrongPassword)));
        let legacy = read_logins(&fixture("firefox-legacy"), "fx-master-pw").unwrap();
        assert_eq!(legacy.logins.len(), 3);
        assert_eq!(legacy.logins[0].password.as_str(), "Fx-pass-alice-1");
        assert!(matches!(read_logins(&fixture("firefox-legacy"), ""), Err(ImportError::WrongPassword)));
        assert!(matches!(read_logins(&fixture("nowhere"), ""), Err(ImportError::NotFound(_))));
    }

    #[test]
    fn history_from_places() {
        let got = read_history(&fixture("firefox-nomaster/places.sqlite")).unwrap();
        assert_eq!(got.len(), 5, "{got:?}");
        assert_eq!(got[0].url, "https://news.example/");
        assert_eq!(got[0].ts_ms, 1_700_000_000_500);
        assert_eq!(got[4].title, "Login");
        assert!(got.iter().all(|v| !v.url.starts_with("place:")));
    }

    #[test]
    fn bookmarks_from_places() {
        let got = read_bookmarks(&fixture("firefox-nomaster/places.sqlite")).unwrap();
        let ImportNode::Folder { toolbar: true, children, .. } = &got[0] else { panic!("{got:?}") };
        assert_eq!(children.len(), 2);
        assert!(
            matches!(&children[0], ImportNode::Link { title, url, add_date } if title == "News" && url == "https://news.example/" && *add_date == 1_700_000_000)
        );
        let ImportNode::Folder { title, children: work, .. } = &children[1] else { panic!() };
        assert_eq!(title, "Work");
        assert!(matches!(&work[0], ImportNode::Link { url, .. } if url == "https://docs.example/guide"));
        // Menu holds only a place: query (skipped); unfiled holds Login; no mobile folder.
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(matches!(&got[1], ImportNode::Link { title, .. } if title == "Login"));
    }
}
