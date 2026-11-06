//! A Firefox profile: `logins.json` + `key4.db` (passwords), `places.sqlite` (history and
//! bookmarks). All files are copies the host made; nothing is written.

use std::path::Path;

use serde::Deserialize;
use zeroize::Zeroizing;

use super::nss::Key4;
use super::{is_web_url, open_readonly, us_to_ms, ImportError, Login, Visit};
use crate::netscape::ImportNode;

/// Logins and how many entries could not be read.
pub struct FirefoxLogins {
    pub logins: Vec<Login>,
    pub skipped: usize,
}

#[derive(Deserialize)]
struct LoginsFile {
    #[serde(default)]
    logins: Vec<RawLogin>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLogin {
    hostname: String,
    #[serde(default)]
    encrypted_username: String,
    #[serde(default)]
    encrypted_password: String,
    #[serde(default)]
    time_created: Option<i64>,
}

/// Reads the saved logins of a profile directory (`key4.db` and `logins.json` inside).
/// `primary_password` is the Firefox primary password, "" when none is set.
pub fn read_logins(profile: &Path, primary_password: &str) -> Result<FirefoxLogins, ImportError> {
    let logins_path = profile.join("logins.json");
    if !logins_path.is_file() {
        return Err(ImportError::NotFound("logins.json".into()));
    }
    let text = std::fs::read_to_string(&logins_path).map_err(|e| ImportError::Read(format!("logins.json: {e}")))?;
    let file: LoginsFile = serde_json::from_str(&text).map_err(|e| ImportError::Format(format!("logins.json: {e}")))?;
    if file.logins.is_empty() {
        return Ok(FirefoxLogins { logins: Vec::new(), skipped: 0 });
    }
    let key = Key4::read(&profile.join("key4.db"))?.unlock(primary_password)?;
    let mut out = FirefoxLogins { logins: Vec::new(), skipped: 0 };
    for raw in file.logins {
        let (Ok(username), Ok(password)) =
            (key.decrypt_field(&raw.encrypted_username), key.decrypt_field(&raw.encrypted_password))
        else {
            out.skipped += 1;
            continue;
        };
        if password.is_empty() || crate::urlclean::origin_of(&raw.hostname).is_none() {
            out.skipped += 1;
            continue;
        }
        out.logins.push(Login {
            url: raw.hostname,
            username: username.to_string(),
            password,
            note: Zeroizing::new(String::new()),
            created_ms: raw.time_created.filter(|t| *t > 0),
        });
    }
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
