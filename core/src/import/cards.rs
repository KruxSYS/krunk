//! Payment cards from other browsers: Firefox's `autofill-profiles.json` and the
//! Chromium family's `Web Data`. Pure parsing and decryption of copies the host made, given
//! the keys the host read from the operating system (Windows Credential Manager for Firefox,
//! DPAPI for Chromium). A card number is never logged; [`ImportedCard`] redacts itself in
//! `Debug`. Security codes are never read, even where a browser keeps one (Chrome's
//! `local_stored_cvc` table, Firefox's `cc-csc` when synced from elsewhere).
//!
//! ## Firefox (`autofill-profiles.json`, `creditCards`)
//!
//! Format from mozilla-central (`toolkit/components/formautofill/FormAutofillStorageBase.sys.mjs`,
//! the schema comment at the top, and `default/FormAutofillStorage.sys.mjs` `_encryptNumber`):
//! each record has `guid`, `version` (3 today; 4 was briefly used by Firefox 110/111 and is
//! read the same way), `cc-name`, `cc-number` (masked), `cc-number-encrypted`, `cc-exp-month`,
//! `cc-exp-year` (4 digits), `cc-type`, `timeCreated` (ms) and, for removed cards synced
//! as such, `deleted: true`. Records of version 1–2 were encrypted with an older scheme and
//! Firefox itself drops them; they are counted as unreadable.
//!
//! `cc-number-encrypted` comes from `OSKeyStore.encrypt` (`toolkit/modules/OSKeyStore.sys.mjs`):
//! the number as UTF-8 bytes, encrypted by `security/manager/ssl/OSKeyStore.cpp`
//! (`AbstractOSKeyStore::DoCipher`): AES-GCM with a fresh 12-byte IV, 128-bit tag and no AAD,
//! output IV || ciphertext || tag, base64 encoded (`OSKeyStore::EncryptBytes`). The key is
//! `mKeyByteLength` = 16 bytes (`OSKeyStore.h`), so AES-128-GCM. On Windows the raw key is the
//! blob of a generic Credential Manager entry named `STORE_LABEL` = "Firefox Encrypted Storage"
//! (`CredentialManagerSecret.cpp`, `CredWriteA` with `CRED_TYPE_GENERIC`); the primary password
//! plays no part. Firefox's optional OS re-authentication before showing a number is its own UI.
//!
//! Firefox is moving cards to an Application Services store (`autofill.db`, its key in
//! `key4.db`; `RustAutofillStore.sys.mjs`), behind the pref
//! `extensions.formautofill.creditCards.storage.rust.enabled`, off by default as of Firefox
//! 162 nightly. That store is not read here; the host says so when a profile has it and the
//! JSON file holds no cards.
//!
//! ## Chromium (`Web Data`, table `credit_cards`)
//!
//! From chromium `components/autofill/core/browser/webdata/payments/payments_autofill_table.cc`
//! (`BindCreditCardToStatement` / `CreditCardFromStatement`): `name_on_card`,
//! `expiration_month`, `expiration_year` (4 digits, as text), `card_number_encrypted`,
//! `date_modified` (seconds), `nickname`. The number is `Encryptor::EncryptString16`
//! (`components/os_crypt/async/common/encryptor.cc`: UTF-16 to UTF-8, then the same `v10` /
//! `v20` scheme as passwords; see `chromium::decrypt_value`).

use std::fmt;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm};
use base64ct::{Base64, Encoding};
use zeroize::Zeroizing;

use super::chromium::{decrypt_value, unprotect_key, Decrypted, LocalStateKeys};
use super::{open_readonly, ImportError, Unprotect};

/// The Credential Manager target name of Firefox's OS key store key (`OSKeyStore.sys.mjs`
/// `STORE_LABEL` = `MOZ_APP_BASENAME + " Encrypted Storage"`).
pub const FIREFOX_KEY_LABEL: &str = "Firefox Encrypted Storage";

/// The file name of Firefox's card store in the profile.
pub const FIREFOX_FILE: &str = "autofill-profiles.json";

/// Firefox's newer card store (not read; see the module docs).
pub const FIREFOX_RUST_STORE: &str = "autofill.db";

/// A card read from another browser. The number is digits only, 12–19 of them, Luhn-valid.
#[derive(Clone)]
pub struct ImportedCard {
    pub number: Zeroizing<String>,
    pub exp_month: u8,
    pub exp_year: u16,
    pub name: String,
    pub nickname: String,
    /// Creation (or last change) time in ms since the epoch, when the source has one.
    pub created_ms: Option<i64>,
}

impl fmt::Debug for ImportedCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedCard")
            .field("number", &"<redacted>")
            .field("exp_month", &self.exp_month)
            .field("exp_year", &self.exp_year)
            .finish_non_exhaustive()
    }
}

/// Why a stored card was not read.
pub const NO_NUMBER: &str = "no card number";
pub const BAD_NUMBER: &str = "not a valid card number";
pub const NO_EXPIRY: &str = "no valid expiry date";
pub const NO_KEY: &str = "Firefox's key is not in Windows Credential Manager";
pub const UNDECRYPTABLE: &str = "could not be decrypted";
pub const OLD_FIREFOX: &str = "encrypted by an old Firefox version";

/// Cards read from a profile, and the ones that could not be used.
#[derive(Debug, Default)]
pub struct ReadCards {
    pub cards: Vec<ImportedCard>,
    /// Card records in the source (removed ones not counted).
    pub entries: usize,
    /// Chromium `v20` (app-bound) numbers: not readable outside the browser.
    pub app_bound: usize,
    /// (reason, count), in the order first seen.
    pub unreadable: Vec<(&'static str, usize)>,
}

impl ReadCards {
    fn skip(&mut self, reason: &'static str) {
        match self.unreadable.iter_mut().find(|(r, _)| *r == reason) {
            Some((_, n)) => *n += 1,
            None => self.unreadable.push((reason, 1)),
        }
    }

    /// Records that could not be imported, for whatever reason.
    pub fn unreadable_total(&self) -> usize {
        self.unreadable.iter().map(|(_, n)| n).sum()
    }

    fn push(
        &mut self,
        number: &str,
        month: Option<i64>,
        year: Option<i64>,
        name: &str,
        nickname: &str,
        ts: Option<i64>,
    ) {
        let Some(number) = normalize_number(number) else {
            self.skip(if number.trim().is_empty() { NO_NUMBER } else { BAD_NUMBER });
            return;
        };
        let Some((exp_month, exp_year)) = normalize_expiry(month, year) else {
            self.skip(NO_EXPIRY);
            return;
        };
        self.cards.push(ImportedCard {
            number,
            exp_month,
            exp_year,
            name: name.trim().chars().take(128).collect(),
            nickname: nickname.trim().chars().take(64).collect(),
            created_ms: ts.filter(|t| *t > 0),
        });
    }
}

/// Digits only (spaces and dashes between groups are dropped), 12–19 of them, Luhn-valid.
pub fn normalize_number(text: &str) -> Option<Zeroizing<String>> {
    if text.chars().any(|c| !(c.is_ascii_digit() || c == ' ' || c == '-')) {
        return None;
    }
    let digits = Zeroizing::new(text.chars().filter(char::is_ascii_digit).collect::<String>());
    ((12..=19).contains(&digits.len()) && crate::vault::luhn_ok(&digits)).then_some(digits)
}

/// Month 1–12; a 2-digit year means 20xx; the year must be within 2000–2100 (what the vault
/// keeps).
pub fn normalize_expiry(month: Option<i64>, year: Option<i64>) -> Option<(u8, u16)> {
    let month = u8::try_from(month?).ok().filter(|m| (1..=12).contains(m))?;
    let year = match year? {
        y @ 0..=99 => 2000 + y,
        y => y,
    };
    let year = u16::try_from(year).ok().filter(|y| (2000..=2100).contains(y))?;
    Some((month, year))
}

/// A JSON number or numeric string (Firefox writes numbers; a hand-edited or synced file may
/// hold strings).
fn json_int(v: Option<&serde_json::Value>) -> Option<i64> {
    match v? {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn json_str<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|s| s.as_str()).unwrap_or("")
}

/// Decrypts one `cc-number-encrypted` value with the OS key store key (`OSKeyStore.cpp`
/// `DoCipher`: base64 of IV(12) || ciphertext || tag(16), AES-GCM). Firefox's key is 16
/// bytes; a 32-byte key (AES-256) is accepted too, for builds that differ.
pub fn decrypt_oskeystore(key: &[u8], encrypted_b64: &str) -> Option<Zeroizing<Vec<u8>>> {
    let raw = Zeroizing::new(Base64::decode_vec(encrypted_b64.trim()).ok()?);
    if raw.len() < 12 + 16 {
        return None;
    }
    let (iv, ct) = raw.split_at(12);
    let nonce = aes_gcm::Nonce::try_from(iv).ok()?;
    let pt = match key.len() {
        16 => Aes128Gcm::new_from_slice(key).ok()?.decrypt(&nonce, ct).ok()?,
        32 => Aes256Gcm::new_from_slice(key).ok()?.decrypt(&nonce, ct).ok()?,
        _ => return None,
    };
    Some(Zeroizing::new(pt))
}

/// Reads the cards of a Firefox `autofill-profiles.json`. `key` is the OS key store key
/// (`None`: Credential Manager has no entry, so no number can be read).
pub fn read_firefox(json: &str, key: Option<&[u8]>) -> Result<ReadCards, ImportError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ImportError::Format(format!("{FIREFOX_FILE}: {e}")))?;
    let mut out = ReadCards::default();
    let Some(list) = v.get("creditCards").and_then(|c| c.as_array()) else { return Ok(out) };
    for rec in list {
        if !rec.is_object() || rec.get("deleted").and_then(|d| d.as_bool()) == Some(true) {
            continue;
        }
        out.entries += 1;
        let encrypted = json_str(rec, "cc-number-encrypted");
        if encrypted.is_empty() {
            out.skip(NO_NUMBER);
            continue;
        }
        if json_int(rec.get("version")).is_some_and(|v| v < 3) {
            out.skip(OLD_FIREFOX);
            continue;
        }
        let Some(key) = key else {
            out.skip(NO_KEY);
            continue;
        };
        let Some(pt) = decrypt_oskeystore(key, encrypted) else {
            out.skip(UNDECRYPTABLE);
            continue;
        };
        let Ok(number) = std::str::from_utf8(&pt) else {
            out.skip(BAD_NUMBER);
            continue;
        };
        out.push(
            number,
            json_int(rec.get("cc-exp-month")),
            json_int(rec.get("cc-exp-year")),
            json_str(rec, "cc-name"),
            "",
            json_int(rec.get("timeCreated")),
        );
    }
    Ok(out)
}

/// Reads the `credit_cards` table of a copied Chromium `Web Data`. `keys` comes from
/// `Local State`; `unprotect` is DPAPI. A database without the table has no cards.
pub fn read_chromium(
    web_data: &Path,
    keys: &LocalStateKeys,
    unprotect: &dyn Unprotect,
) -> Result<ReadCards, ImportError> {
    let conn = open_readonly(web_data)?;
    let fmt = |e: rusqlite::Error| ImportError::Format(format!("Web Data: {e}"));
    let mut columns = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('credit_cards')").map_err(fmt)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(fmt)?;
        for c in rows {
            columns.push(c.map_err(fmt)?);
        }
    }
    let mut out = ReadCards::default();
    if columns.is_empty() {
        return Ok(out);
    }
    let has = |c: &str| columns.iter().any(|x| x == c);
    let col = |c: &str, fallback: &'static str| if has(c) { c.to_string() } else { fallback.to_string() };
    let aes_key = unprotect_key(keys, unprotect)?;
    let sql = format!(
        "SELECT {}, {}, {}, card_number_encrypted, {}, {} FROM credit_cards ORDER BY {}",
        col("name_on_card", "''"),
        col("expiration_month", "0"),
        col("expiration_year", "0"),
        col("nickname", "''"),
        col("date_modified", "0"),
        if has("date_modified") { "date_modified, rowid" } else { "rowid" },
    );
    let mut stmt = conn.prepare(&sql).map_err(fmt)?;
    let rows = stmt
        .query_map([], |r| {
            let text = |i: usize| -> rusqlite::Result<String> {
                Ok(match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                    rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                    _ => String::new(),
                })
            };
            Ok((
                text(0)?,
                text(1)?,
                text(2)?,
                r.get::<_, Option<Vec<u8>>>(3)?.unwrap_or_default(),
                text(4)?,
                r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            ))
        })
        .map_err(fmt)?;
    for row in rows {
        let (name, month, year, blob, nickname, modified) = row.map_err(fmt)?;
        let blob = Zeroizing::new(blob);
        out.entries += 1;
        if blob.is_empty() {
            out.skip(NO_NUMBER);
            continue;
        }
        let pt = match decrypt_value(&blob, aes_key.as_deref().map(Vec::as_slice), unprotect) {
            Decrypted::Plain(p) => p,
            Decrypted::AppBound => {
                out.app_bound += 1;
                continue;
            }
            Decrypted::Unreadable => {
                out.skip(UNDECRYPTABLE);
                continue;
            }
        };
        let Ok(number) = std::str::from_utf8(&pt) else {
            out.skip(BAD_NUMBER);
            continue;
        };
        out.push(
            number,
            month.trim().parse().ok(),
            year.trim().parse().ok(),
            &name,
            &nickname,
            Some(modified.saturating_mul(1000)),
        );
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::import::testing::FakeDpapi;
    use std::path::PathBuf;

    const FF_KEY: [u8; 16] = *b"0123456789abcdef";

    /// Encrypts like `OSKeyStore::EncryptBytes` (AES-128-GCM, base64 of IV || ct || tag).
    fn ff_encrypt(key: &[u8; 16], iv: [u8; 12], plain: &str) -> String {
        let ct = Aes128Gcm::new_from_slice(key).unwrap().encrypt(&aes_gcm::Nonce::from(iv), plain.as_bytes()).unwrap();
        let mut raw = iv.to_vec();
        raw.extend_from_slice(&ct);
        Base64::encode_string(&raw)
    }

    pub(crate) fn card(number: &str, month: u8, year: u16) -> ImportedCard {
        ImportedCard {
            number: Zeroizing::new(number.into()),
            exp_month: month,
            exp_year: year,
            name: "A".into(),
            nickname: String::new(),
            created_ms: None,
        }
    }

    #[test]
    fn numbers_and_expiry() {
        assert_eq!(normalize_number("4111 1111-1111 1111").as_deref().map(String::as_str), Some("4111111111111111"));
        assert!(normalize_number("4111111111111112").is_none(), "Luhn");
        assert!(normalize_number("41111111111").is_none(), "too short");
        assert!(normalize_number("4111111111111111111111").is_none(), "too long");
        assert!(normalize_number("************1111").is_none(), "masked");
        assert_eq!(normalize_expiry(Some(3), Some(29)), Some((3, 2029)));
        assert_eq!(normalize_expiry(Some(12), Some(2031)), Some((12, 2031)));
        assert_eq!(normalize_expiry(Some(13), Some(2031)), None);
        assert_eq!(normalize_expiry(Some(0), Some(2031)), None);
        assert_eq!(normalize_expiry(Some(1), Some(1999)), None);
        assert_eq!(normalize_expiry(Some(1), Some(2101)), None);
        assert_eq!(normalize_expiry(None, Some(2030)), None);
        assert_eq!(normalize_expiry(Some(1), None), None);
    }

    #[test]
    fn debug_hides_the_number() {
        let d = format!("{:?}", card("4111111111111111", 1, 2030));
        assert!(!d.contains("4111") && d.contains("redacted"), "{d}");
    }

    #[test]
    fn oskeystore_round_trip_and_wrong_key() {
        let enc = ff_encrypt(&FF_KEY, [7; 12], "4111111111111111");
        assert_eq!(decrypt_oskeystore(&FF_KEY, &enc).as_deref().map(Vec::as_slice), Some(&b"4111111111111111"[..]));
        assert!(decrypt_oskeystore(b"fedcba9876543210", &enc).is_none(), "another key");
        assert!(decrypt_oskeystore(&FF_KEY[..15], &enc).is_none(), "bad key size");
        assert!(decrypt_oskeystore(&FF_KEY, "not base64!").is_none());
        assert!(decrypt_oskeystore(&FF_KEY, &Base64::encode_string(&[0u8; 20])).is_none(), "too short");
        // A 32-byte key works the same way (AES-256-GCM).
        let k32 = [9u8; 32];
        let ct = Aes256Gcm::new_from_slice(&k32)
            .unwrap()
            .encrypt(&aes_gcm::Nonce::from([1; 12]), &b"5500000000000004"[..])
            .unwrap();
        let mut raw = vec![1u8; 12];
        raw.extend_from_slice(&ct);
        assert!(decrypt_oskeystore(&k32, &Base64::encode_string(&raw)).is_some());
    }

    fn firefox_json() -> String {
        serde_json::json!({
            "version": 1,
            "addresses": [],
            "creditCards": [
                { "guid": "a1", "version": 3, "cc-name": "Alice Example", "cc-number": "************1111",
                  "cc-number-encrypted": ff_encrypt(&FF_KEY, [1; 12], "4111111111111111"),
                  "cc-exp-month": 4, "cc-exp-year": 2030, "cc-type": "visa", "timeCreated": 1_700_000_000_000_i64 },
                { "guid": "a2", "version": 4, "cc-name": "Bob", "cc-number-encrypted": ff_encrypt(&FF_KEY, [2; 12], "5500 0000 0000 0004"),
                  "cc-exp-month": "11", "cc-exp-year": "29" },
                { "guid": "a3", "version": 3, "cc-number-encrypted": ff_encrypt(&FF_KEY, [3; 12], "4111111111111111"),
                  "cc-exp-month": 13, "cc-exp-year": 2030 },
                { "guid": "a4", "version": 3, "cc-number-encrypted": ff_encrypt(b"fedcba9876543210", [4; 12], "4111111111111111"),
                  "cc-exp-month": 1, "cc-exp-year": 2030 },
                { "guid": "a5", "version": 2, "cc-number-encrypted": "AAAA", "cc-exp-month": 1, "cc-exp-year": 2030 },
                { "guid": "a6", "version": 3, "cc-number-encrypted": "", "cc-exp-month": 1, "cc-exp-year": 2030 },
                { "guid": "a7", "version": 3, "cc-number-encrypted": ff_encrypt(&FF_KEY, [5; 12], "1234567890123"),
                  "cc-exp-month": 1, "cc-exp-year": 2030 },
                { "guid": "gone", "timeLastModified": 5, "deleted": true },
            ]
        })
        .to_string()
    }

    #[test]
    fn firefox_cards() {
        let got = read_firefox(&firefox_json(), Some(&FF_KEY)).unwrap();
        assert_eq!(got.entries, 7, "the tombstone is not a card");
        let cards: Vec<(&str, u8, u16, &str)> =
            got.cards.iter().map(|c| (c.number.as_str(), c.exp_month, c.exp_year, c.name.as_str())).collect();
        assert_eq!(cards, [("4111111111111111", 4, 2030, "Alice Example"), ("5500000000000004", 11, 2029, "Bob")]);
        assert_eq!(got.cards[0].created_ms, Some(1_700_000_000_000));
        assert_eq!(
            got.unreadable,
            [(NO_EXPIRY, 1), (UNDECRYPTABLE, 1), (OLD_FIREFOX, 1), (NO_NUMBER, 1), (BAD_NUMBER, 1)]
        );
        assert_eq!(got.unreadable_total(), 5);
    }

    #[test]
    fn firefox_without_key_or_cards() {
        let got = read_firefox(&firefox_json(), None).unwrap();
        assert!(got.cards.is_empty());
        assert_eq!(got.unreadable.iter().find(|(r, _)| *r == NO_KEY).map(|(_, n)| *n), Some(5));
        let none = read_firefox(r#"{"version":1,"addresses":[]}"#, Some(&FF_KEY)).unwrap();
        assert_eq!((none.entries, none.cards.len()), (0, 0));
        assert!(matches!(read_firefox("nope", None), Err(ImportError::Format(_))));
    }

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const CR_KEY: [u8; 32] = [0x42; 32];

    fn v10(plain: &str, iv: u8) -> Vec<u8> {
        let ct = Aes256Gcm::new_from_slice(&CR_KEY)
            .unwrap()
            .encrypt(&aes_gcm::Nonce::from([iv; 12]), plain.as_bytes())
            .unwrap();
        let mut out = b"v10".to_vec();
        out.extend_from_slice(&[iv; 12]);
        out.extend_from_slice(&ct);
        out
    }

    /// guid, name, month, year, encrypted number, date_modified, nickname.
    type CardRow = (&'static str, &'static str, &'static str, &'static str, Vec<u8>, i64, &'static str);

    /// A `Web Data` with the columns Chromium's `payments_autofill_table.cc` binds.
    fn web_data(name: &str) -> (TempDir, PathBuf) {
        let dir = TempDir(std::env::temp_dir().join(format!("lb-import-cards-{name}-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir.0);
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("Web Data");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE credit_cards (guid VARCHAR PRIMARY KEY, name_on_card VARCHAR, expiration_month INTEGER,
               expiration_year INTEGER, card_number_encrypted BLOB, date_modified INTEGER NOT NULL DEFAULT 0,
               origin VARCHAR DEFAULT '', use_count INTEGER NOT NULL DEFAULT 0, use_date INTEGER NOT NULL DEFAULT 0,
               billing_address_id VARCHAR, nickname VARCHAR);
             CREATE TABLE local_stored_cvc (guid VARCHAR PRIMARY KEY, value_encrypted VARCHAR, last_updated_timestamp INTEGER);",
        )
        .unwrap();
        let rows: Vec<CardRow> = vec![
            ("g1", "Carol", "7", "2031", v10("4111111111111111", 1), 1_700_000_100, "Work card"),
            ("g2", "Dave", "02", "2028", b"v20appboundblob".to_vec(), 1_700_000_200, ""),
            ("g3", "Erin", "9", "2030", b"FAKEDPAPI378282246310005".to_vec(), 1_700_000_300, ""),
            ("g4", "Frank", "0", "0", v10("5500000000000004", 2), 1_700_000_400, ""),
            ("g5", "Gina", "1", "2030", b"v10garbagegarbagegarbagegarbage".to_vec(), 1_700_000_500, ""),
            ("g6", "Hank", "1", "2030", Vec::new(), 1_700_000_600, ""),
        ];
        for (g, n, m, y, blob, t, nick) in rows {
            conn.execute(
                "INSERT INTO credit_cards (guid, name_on_card, expiration_month, expiration_year, card_number_encrypted, date_modified, nickname)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![g, n, m, y, blob, t, nick],
            )
            .unwrap();
        }
        conn.execute("INSERT INTO local_stored_cvc VALUES ('g1', ?1, 0)", [v10("123", 3)]).unwrap();
        (dir, path)
    }

    fn keys() -> LocalStateKeys {
        let mut blob = b"FAKEDPAPI".to_vec();
        blob.extend_from_slice(&CR_KEY);
        LocalStateKeys { dpapi_key_blob: Some(blob), app_bound: true }
    }

    #[test]
    fn chromium_cards() {
        let (_dir, path) = web_data("cr");
        let got = read_chromium(&path, &keys(), &FakeDpapi).unwrap();
        assert_eq!((got.entries, got.app_bound), (6, 1));
        let cards: Vec<(&str, u8, u16, &str, &str)> = got
            .cards
            .iter()
            .map(|c| (c.number.as_str(), c.exp_month, c.exp_year, c.name.as_str(), c.nickname.as_str()))
            .collect();
        assert_eq!(
            cards,
            [("4111111111111111", 7, 2031, "Carol", "Work card"), ("378282246310005", 9, 2030, "Erin", "")]
        );
        assert_eq!(got.cards[0].created_ms, Some(1_700_000_100_000));
        assert_eq!(got.unreadable, [(NO_EXPIRY, 1), (UNDECRYPTABLE, 1), (NO_NUMBER, 1)]);
    }

    #[test]
    fn chromium_without_key_or_table() {
        let (dir, path) = web_data("cr-nokey");
        let got = read_chromium(&path, &LocalStateKeys { dpapi_key_blob: None, app_bound: false }, &FakeDpapi).unwrap();
        assert_eq!(got.cards.len(), 1, "only the legacy DPAPI row");
        assert_eq!(got.unreadable.iter().find(|(r, _)| *r == UNDECRYPTABLE).map(|(_, n)| *n), Some(3));
        let empty = dir.0.join("Empty");
        rusqlite::Connection::open(&empty).unwrap().execute_batch("CREATE TABLE meta (key TEXT);").unwrap();
        let none = read_chromium(&empty, &keys(), &FakeDpapi).unwrap();
        assert_eq!((none.entries, none.cards.len()), (0, 0));
        assert!(matches!(read_chromium(&dir.0.join("missing"), &keys(), &FakeDpapi), Err(ImportError::NotFound(_))));
    }
}
