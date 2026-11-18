//! A Chromium-family profile (Chrome, Edge, Brave): `Login Data` (passwords), `Web Data`
//! (payment cards, read in `super::cards` with the decryption here), `History`,
//! `Bookmarks` (JSON) and the `Local State` of the "User Data" folder that holds the
//! encryption key.
//!
//! Password blobs: `v10` = AES-256-GCM (12-byte nonce after the prefix) under the key from
//! `Local State` `os_crypt.encrypted_key` ("DPAPI" + a DPAPI blob of the key); `v20` =
//! app-bound encryption (the key is held by the browser's elevated service: counted and
//! reported, never decryptable here); no prefix = the legacy DPAPI blob of the password.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::Aes256Gcm;
use base64ct::{Base64, Encoding};
use zeroize::Zeroizing;

use super::{is_web_url, open_readonly, webkit_to_ms, ImportError, Login, Unprotect, Visit};
use crate::netscape::ImportNode;

/// What `Local State` says about the profile's encryption.
pub struct LocalStateKeys {
    /// The DPAPI blob of the AES key (`os_crypt.encrypted_key` without its "DPAPI" prefix).
    pub dpapi_key_blob: Option<Vec<u8>>,
    /// The browser also has an app-bound key (`v20` blobs are expected).
    pub app_bound: bool,
}

/// Parses `Local State`.
pub fn local_state_keys(json: &str) -> Result<LocalStateKeys, ImportError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ImportError::Format(format!("Local State: {e}")))?;
    let decode = |ptr: &str| -> Option<Vec<u8>> {
        let b64 = v.pointer(ptr)?.as_str()?;
        Base64::decode_vec(b64).ok()
    };
    let dpapi_key_blob = decode("/os_crypt/encrypted_key").and_then(|b| b.strip_prefix(b"DPAPI").map(<[u8]>::to_vec));
    let app_bound = decode("/os_crypt/app_bound_encrypted_key").is_some();
    Ok(LocalStateKeys { dpapi_key_blob, app_bound })
}

/// Logins read from `Login Data`.
pub struct ChromiumLogins {
    pub logins: Vec<Login>,
    /// Rows with app-bound (`v20`) passwords: not readable outside the browser.
    pub app_bound: usize,
    /// Rows that could not be decrypted or had no usable URL.
    pub skipped: usize,
}

/// Reads `Login Data`. `keys` comes from `Local State`; `unprotect` is DPAPI.
pub fn read_logins(
    login_data: &Path,
    keys: &LocalStateKeys,
    unprotect: &dyn Unprotect,
) -> Result<ChromiumLogins, ImportError> {
    let conn = open_readonly(login_data)?;
    let fmt = |e: rusqlite::Error| ImportError::Format(format!("Login Data: {e}"));
    let aes_key = unprotect_key(keys, unprotect)?;
    let mut stmt = conn
        .prepare(
            "SELECT origin_url, username_value, password_value, date_created FROM logins
             WHERE blacklisted_by_user = 0 ORDER BY date_created, id",
        )
        .map_err(fmt)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?, r.get::<_, i64>(3)?))
        })
        .map_err(fmt)?;
    let mut out = ChromiumLogins { logins: Vec::new(), app_bound: 0, skipped: 0 };
    for row in rows {
        let (url, username, blob, created) = row.map_err(fmt)?;
        let blob = Zeroizing::new(blob);
        let password = match decrypt_value(&blob, aes_key.as_deref().map(Vec::as_slice), unprotect) {
            Decrypted::Plain(p) => p,
            Decrypted::AppBound => {
                out.app_bound += 1;
                continue;
            }
            Decrypted::Unreadable => {
                out.skipped += 1;
                continue;
            }
        };
        let Ok(password) = String::from_utf8(password.to_vec()).map(Zeroizing::new) else {
            out.skipped += 1;
            continue;
        };
        if password.is_empty() || crate::urlclean::origin_of(&url).is_none() {
            out.skipped += 1;
            continue;
        }
        out.logins.push(Login {
            url,
            username,
            password,
            note: Zeroizing::new(String::new()),
            created_ms: Some(webkit_to_ms(created)).filter(|t| *t > 0),
        });
    }
    Ok(out)
}

/// The profile's AES key: `Local State`'s DPAPI blob unprotected (`None` without one).
pub(crate) fn unprotect_key(
    keys: &LocalStateKeys,
    unprotect: &dyn Unprotect,
) -> Result<Option<Zeroizing<Vec<u8>>>, ImportError> {
    match &keys.dpapi_key_blob {
        Some(blob) => unprotect
            .unprotect(blob)
            .map(Some)
            .map_err(|e| ImportError::Format(format!("the password key cannot be unprotected: {e}"))),
        None => Ok(None),
    }
}

/// One value `os_crypt` encrypted (passwords in `Login Data`, card numbers in `Web Data`).
pub(crate) enum Decrypted {
    Plain(Zeroizing<Vec<u8>>),
    /// A `v20` blob: app-bound, readable only through the browser's own elevated service.
    AppBound,
    /// Empty, or it would not decrypt (no key, another key, a damaged blob).
    Unreadable,
}

/// Decrypts one `os_crypt` value: `v10` + 12-byte nonce + AES-256-GCM ciphertext and tag under
/// `aes_key`; `v20` = app-bound; anything else = a legacy DPAPI blob of the value.
pub(crate) fn decrypt_value(blob: &[u8], aes_key: Option<&[u8]>, unprotect: &dyn Unprotect) -> Decrypted {
    if blob.starts_with(b"v20") {
        Decrypted::AppBound
    } else if let Some(rest) = blob.strip_prefix(b"v10") {
        match aes_key.and_then(|key| decrypt_v10(key, rest)) {
            Some(p) => Decrypted::Plain(p),
            None => Decrypted::Unreadable,
        }
    } else if blob.is_empty() {
        Decrypted::Unreadable
    } else {
        match unprotect.unprotect(blob) {
            Ok(p) => Decrypted::Plain(p),
            Err(_) => Decrypted::Unreadable,
        }
    }
}

fn decrypt_v10(key: &[u8], rest: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if rest.len() < 12 + 16 {
        return None;
    }
    let (nonce, ciphertext) = rest.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = aes_gcm::Nonce::try_from(nonce).ok()?;
    cipher.decrypt(&nonce, ciphertext).ok().map(Zeroizing::new)
}

/// Every visit of a web page in `History`.
pub fn read_history(history: &Path) -> Result<Vec<Visit>, ImportError> {
    let conn = open_readonly(history)?;
    let fmt = |e: rusqlite::Error| ImportError::Format(format!("History: {e}"));
    let mut stmt = conn
        .prepare(
            "SELECT u.url, IFNULL(u.title, ''), v.visit_time FROM visits v JOIN urls u ON u.id = v.url
             WHERE u.hidden = 0 ORDER BY v.visit_time",
        )
        .map_err(fmt)?;
    let rows = stmt
        .query_map([], |r| Ok(Visit { url: r.get(0)?, title: r.get(1)?, ts_ms: webkit_to_ms(r.get::<_, i64>(2)?) }))
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

/// The bookmark tree of the `Bookmarks` JSON file.
pub fn read_bookmarks(json: &str) -> Result<Vec<ImportNode>, ImportError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ImportError::Format(format!("Bookmarks: {e}")))?;
    let roots =
        v.get("roots").and_then(|r| r.as_object()).ok_or_else(|| ImportError::Format("Bookmarks: no roots".into()))?;
    fn node(v: &serde_json::Value, depth: usize) -> Option<ImportNode> {
        if depth > 32 {
            return None;
        }
        let title = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let add_date = v
            .get("date_added")
            .and_then(|d| d.as_str())
            .and_then(|d| d.parse::<i64>().ok())
            .map(webkit_to_ms)
            .unwrap_or(0)
            / 1000;
        match v.get("type").and_then(|t| t.as_str()) {
            Some("url") => {
                let url = v.get("url")?.as_str()?.to_string();
                is_web_url(&url).then_some(ImportNode::Link { title, url, add_date })
            }
            Some("folder") => {
                Some(ImportNode::Folder { title, toolbar: false, add_date, children: children(v, depth + 1) })
            }
            _ => None,
        }
    }
    fn children(v: &serde_json::Value, depth: usize) -> Vec<ImportNode> {
        v.get("children")
            .and_then(|c| c.as_array())
            .map(|c| c.iter().filter_map(|n| node(n, depth)).collect())
            .unwrap_or_default()
    }
    let mut out = Vec::new();
    if let Some(bar) = roots.get("bookmark_bar") {
        out.push(ImportNode::Folder {
            title: "Bookmarks bar".into(),
            toolbar: true,
            add_date: 0,
            children: children(bar, 0),
        });
    }
    if let Some(other) = roots.get("other") {
        out.extend(children(other, 0));
    }
    if let Some(synced) = roots.get("synced") {
        let kids = children(synced, 0);
        if !kids.is_empty() {
            out.push(ImportNode::Folder {
                title: "Mobile bookmarks".into(),
                toolbar: false,
                add_date: 0,
                children: kids,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testing::{fixture, FakeDpapi};

    fn keys() -> LocalStateKeys {
        local_state_keys(&std::fs::read_to_string(fixture("chrome/Local State")).unwrap()).unwrap()
    }

    #[test]
    fn local_state() {
        let k = keys();
        assert!(k.app_bound);
        assert_eq!(k.dpapi_key_blob.as_deref().map(|b| b.starts_with(b"FAKEDPAPI")), Some(true));
        let none = local_state_keys(r#"{"os_crypt":{}}"#).unwrap();
        assert!(none.dpapi_key_blob.is_none() && !none.app_bound);
        assert!(matches!(local_state_keys("nope"), Err(ImportError::Format(_))));
    }

    #[test]
    fn logins_v10_legacy_and_v20_counted() {
        let got = read_logins(&fixture("chrome/Default/Login Data"), &keys(), &FakeDpapi).unwrap();
        assert_eq!(got.app_bound, 1, "the v20 row is counted");
        assert_eq!(got.skipped, 0, "the never-save row is not selected at all");
        let names: Vec<(&str, &str, &str)> =
            got.logins.iter().map(|l| (l.url.as_str(), l.username.as_str(), l.password.as_str())).collect();
        assert_eq!(
            names,
            [
                ("https://login.example/", "cr-alice", "Cr-pass-alice-1"),
                ("https://shop.example/signin", "cr-bob", "Cr-pass-bob-2"),
                ("https://old.example/", "cr-dave", "Cr-pass-dave-4"),
            ]
        );
        assert_eq!(got.logins[0].created_ms, Some(1_700_000_001_000));
    }

    #[test]
    fn logins_without_a_key_skip_v10() {
        let keys = LocalStateKeys { dpapi_key_blob: None, app_bound: false };
        let got = read_logins(&fixture("chrome/Default/Login Data"), &keys, &FakeDpapi).unwrap();
        assert_eq!((got.logins.len(), got.skipped, got.app_bound), (1, 2, 1));
        struct Broken;
        impl Unprotect for Broken {
            fn unprotect(&self, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
                Err("no".into())
            }
        }
        assert!(matches!(
            read_logins(&fixture("chrome/Default/Login Data"), &self::keys(), &Broken),
            Err(ImportError::Format(_))
        ));
    }

    #[test]
    fn history_skips_internal_and_hidden() {
        let got = read_history(&fixture("chrome/Default/History")).unwrap();
        let urls: Vec<&str> = got.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(urls, ["https://news.example/", "https://docs.example/guide", "https://news.example/"]);
        assert_eq!(got[0].ts_ms, 1_700_000_001_000);
        assert_eq!(got[1].title, "The guide");
    }

    #[test]
    fn bookmarks_json() {
        let got = read_bookmarks(&std::fs::read_to_string(fixture("chrome/Default/Bookmarks")).unwrap()).unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        let ImportNode::Folder { toolbar: true, children, .. } = &got[0] else { panic!() };
        assert_eq!(children.len(), 2);
        assert!(
            matches!(&children[0], ImportNode::Link { url, add_date, .. } if url == "https://news.example/" && *add_date == 1_700_000_000)
        );
        assert!(
            matches!(&children[1], ImportNode::Folder { title, children, .. } if title == "Work" && children.len() == 1)
        );
        // "other" holds Login and a javascript: link (dropped); "synced" is empty.
        assert!(matches!(&got[1], ImportNode::Link { title, .. } if title == "Login"));
        assert!(matches!(read_bookmarks("{}"), Err(ImportError::Format(_))));
    }
}
