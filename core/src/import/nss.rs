//! The NSS key database of a Firefox profile (`key4.db`) and the "SDR" blobs that
//! `logins.json` stores.
//!
//! `metaData` holds the global salt (`item1`) and the password check (`item2`): a PKCS#5
//! blob that decrypts to `password-check` with the right primary password (the empty string
//! when none is set). `nssPrivate` holds the SDR keys (`a11`), encrypted the same way, each
//! with its id (`a102`); a profile that went through Firefox's change from 3DES to AES (or was
//! migrated from key3.db) has more than one, and each login names the key it was encrypted
//! with (0.7.2, R12: only the first key was read, so every login of such a profile failed). Two PBE schemes occur: PBES2 (PBKDF2-HMAC-SHA256 + AES-256-CBC; the PBKDF2
//! password is SHA-1(globalSalt + primaryPassword)) in every current profile, and the legacy
//! PBE-SHA1-3DES in old ones. Each login field is DER `{keyId, {cipher, iv}, ciphertext}`
//! under the SDR key, DES-EDE3-CBC (or AES-256-CBC) with PKCS#7 padding.

use std::path::Path;

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockModeDecrypt, KeyInit, KeyIvInit};
use base64ct::{Base64, Encoding};
use hmac::{Hmac, Mac};
use sha1::{Digest, Sha1};
use zeroize::Zeroizing;

use super::{open_readonly, ImportError};
use crate::certinfo::{children, oid_string, tlv, Tlv};

const OID_PBES2: &str = "1.2.840.113549.1.5.13";
const OID_PBKDF2: &str = "1.2.840.113549.1.5.12";
const OID_HMAC_SHA256: &str = "1.2.840.113549.2.9";
const OID_AES256_CBC: &str = "2.16.840.1.101.3.4.1.42";
const OID_PBE_SHA1_3DES: &str = "1.2.840.113549.1.12.5.1.3";
const OID_DES_EDE3_CBC: &str = "1.2.840.113549.3.7";
/// PBKDF2 rounds are bounded: the file is untrusted input.
const MAX_ITERATIONS: u32 = 5_000_000;

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type Des3CbcDec = cbc::Decryptor<des::TdesEde3>;

/// The parts of `key4.db` the import needs.
pub struct Key4 {
    global_salt: Vec<u8>,
    password_check: Vec<u8>,
    /// Every SDR key: (encrypted key, key id).
    keys: Vec<(Vec<u8>, Vec<u8>)>,
}

/// One unlocked SDR key.
pub struct SdrKey {
    key: Zeroizing<Vec<u8>>,
    id: Vec<u8>,
}

/// The unlocked SDR keys of a profile; a login field is decrypted with the key it names.
pub struct SdrKeys {
    keys: Vec<SdrKey>,
    /// Keys in `key4.db` that could not be unlocked (an unknown scheme, a damaged entry).
    pub unusable: usize,
}

/// Why a login field could not be decrypted (0.7.2: said per reason, never a silent 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldError {
    Missing,
    NotBase64,
    Structure,
    /// The field names a key that is not in `key4.db`.
    UnknownKey,
    UnsupportedCipher,
    /// Wrong padding after decryption (a damaged field, or the wrong key).
    Damaged,
    NotUtf8,
}

impl FieldError {
    pub fn reason(self) -> &'static str {
        match self {
            FieldError::Missing => "could not be decrypted: no encrypted value",
            FieldError::NotBase64 => "could not be decrypted: not base64",
            FieldError::Structure => "could not be decrypted: unexpected format",
            FieldError::UnknownKey => "could not be decrypted: encrypted with a key that is not in key4.db",
            FieldError::UnsupportedCipher => "could not be decrypted: unsupported cipher",
            FieldError::Damaged => "could not be decrypted: damaged",
            FieldError::NotUtf8 => "could not be decrypted: not text",
        }
    }
}

impl Key4 {
    /// Reads `key4.db` (a copy; opened read-only).
    pub fn read(path: &Path) -> Result<Key4, ImportError> {
        let conn = open_readonly(path)?;
        let fmt = |e: rusqlite::Error| ImportError::Format(format!("key4.db: {e}"));
        let (global_salt, password_check): (Vec<u8>, Vec<u8>) = conn
            .query_row("SELECT item1, item2 FROM metaData WHERE id = 'password'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(fmt)?;
        let mut keys = Vec::new();
        let mut stmt =
            conn.prepare("SELECT a11, a102 FROM nssPrivate WHERE a11 IS NOT NULL ORDER BY id").map_err(fmt)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))).map_err(fmt)?;
        for row in rows {
            keys.push(row.map_err(fmt)?);
        }
        if keys.is_empty() {
            return Err(ImportError::Format("key4.db has no SDR key (no logins were ever saved)".into()));
        }
        Ok(Key4 { global_salt, password_check, keys })
    }

    /// How many SDR keys the database holds.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Is a primary password set (the empty one does not open the database)?
    pub fn has_primary_password(&self) -> bool {
        decrypt_pbe(&self.password_check, &self.global_salt, "").as_deref().map(Vec::as_slice) != Ok(b"password-check")
    }

    /// Checks the primary password and unlocks every SDR key that can be unlocked.
    pub fn unlock(&self, primary_password: &str) -> Result<SdrKeys, ImportError> {
        let check = decrypt_pbe(&self.password_check, &self.global_salt, primary_password);
        if check.as_deref().map(Vec::as_slice) != Ok(b"password-check") {
            return Err(ImportError::WrongPassword);
        }
        let mut out = SdrKeys { keys: Vec::new(), unusable: 0 };
        for (blob, id) in &self.keys {
            match decrypt_pbe(blob, &self.global_salt, primary_password) {
                Ok(key) if key.len() >= 24 => out.keys.push(SdrKey { key, id: id.clone() }),
                _ => out.unusable += 1,
            }
        }
        if out.keys.is_empty() {
            return Err(ImportError::Format("key4.db: none of its keys could be unlocked".into()));
        }
        Ok(out)
    }
}

impl SdrKeys {
    /// Decrypts one base64 SDR blob from `logins.json` to text, with the key it names.
    pub fn decrypt_field(&self, b64: &str) -> Result<Zeroizing<String>, FieldError> {
        if b64.trim().is_empty() {
            return Err(FieldError::Missing);
        }
        let der = Base64::decode_vec(b64.trim()).map_err(|_| FieldError::NotBase64)?;
        let (outer, _) = tlv(&der).map_err(|_| FieldError::Structure)?;
        let parts = children(outer.body).map_err(|_| FieldError::Structure)?;
        let [key_id, algo, ciphertext] = parts.as_slice() else { return Err(FieldError::Structure) };
        let key = self.keys.iter().find(|k| k.id.as_slice() == key_id.body).ok_or(FieldError::UnknownKey)?;
        key.decrypt(algo, ciphertext.body)
    }

    /// The unlocked keys (for tests of a single key).
    #[cfg(test)]
    fn first(&self) -> &SdrKey {
        &self.keys[0]
    }
}

impl SdrKey {
    fn decrypt(&self, algo: &Tlv<'_>, ciphertext: &[u8]) -> Result<Zeroizing<String>, FieldError> {
        let algo = children(algo.body).map_err(|_| FieldError::Structure)?;
        let [oid, iv] = algo.as_slice() else { return Err(FieldError::Structure) };
        let plain = match oid_string(oid.body).as_str() {
            OID_DES_EDE3_CBC => Des3CbcDec::new_from_slices(&self.key[..24], iv.body)
                .map_err(|_| FieldError::Structure)?
                .decrypt_padded_vec::<Pkcs7>(ciphertext)
                .map_err(|_| FieldError::Damaged)?,
            OID_AES256_CBC => {
                Aes256CbcDec::new_from_slices(self.key.get(..32).ok_or(FieldError::UnsupportedCipher)?, iv.body)
                    .map_err(|_| FieldError::Structure)?
                    .decrypt_padded_vec::<Pkcs7>(ciphertext)
                    .map_err(|_| FieldError::Damaged)?
            }
            _ => return Err(FieldError::UnsupportedCipher),
        };
        let plain = Zeroizing::new(plain);
        String::from_utf8(plain.to_vec()).map(Zeroizing::new).map_err(|_| FieldError::NotUtf8)
    }
}

/// Decrypts a PBES2 or PBE-SHA1-3DES blob of `key4.db` (PKCS#7 padding removed).
fn decrypt_pbe(blob: &[u8], global_salt: &[u8], password: &str) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let bad = |what: &str| ImportError::Format(format!("key4.db: {what}"));
    let (outer, _) = tlv(blob).map_err(|e| bad(&e))?;
    let parts = children(outer.body).map_err(|e| bad(&e))?;
    let [algo, ciphertext] = parts.as_slice() else { return Err(bad("unexpected PBE structure")) };
    let algo = children(algo.body).map_err(|e| bad(&e))?;
    let [oid, params] = algo.as_slice() else { return Err(bad("unexpected algorithm structure")) };
    match oid_string(oid.body).as_str() {
        OID_PBES2 => decrypt_pbes2(*params, ciphertext.body, global_salt, password),
        OID_PBE_SHA1_3DES => decrypt_pbe_3des(*params, ciphertext.body, global_salt, password),
        other => Err(bad(&format!("unsupported PBE scheme {other}"))),
    }
}

fn decrypt_pbes2(
    params: Tlv<'_>,
    ciphertext: &[u8],
    global_salt: &[u8],
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let bad = |what: &str| ImportError::Format(format!("key4.db PBES2: {what}"));
    let parts = children(params.body).map_err(|e| bad(&e))?;
    let [kdf, scheme] = parts.as_slice() else { return Err(bad("unexpected parameters")) };
    let kdf = children(kdf.body).map_err(|e| bad(&e))?;
    let [kdf_oid, kdf_params] = kdf.as_slice() else { return Err(bad("unexpected KDF")) };
    if oid_string(kdf_oid.body) != OID_PBKDF2 {
        return Err(bad("the KDF is not PBKDF2"));
    }
    let kp = children(kdf_params.body).map_err(|e| bad(&e))?;
    let salt = kp.first().filter(|t| t.tag == 0x04).ok_or_else(|| bad("no salt"))?.body;
    let iterations =
        kp.get(1).filter(|t| t.tag == 0x02).map(|t| be_u32(t.body)).ok_or_else(|| bad("no iteration count"))?;
    if iterations == 0 || iterations > MAX_ITERATIONS {
        return Err(bad("unreasonable iteration count"));
    }
    // Optional key length, then the PRF.
    let mut key_len = 32usize;
    let mut prf = None;
    for t in kp.iter().skip(2) {
        match t.tag {
            0x02 => key_len = be_u32(t.body) as usize,
            0x30 => prf = children(t.body).ok().and_then(|c| c.first().map(|o| oid_string(o.body))),
            _ => {}
        }
    }
    if prf.as_deref() != Some(OID_HMAC_SHA256) {
        return Err(bad("the PRF is not HMAC-SHA256"));
    }
    if key_len != 32 {
        return Err(bad("the key length is not 32"));
    }
    let scheme = children(scheme.body).map_err(|e| bad(&e))?;
    let [enc_oid, iv] = scheme.as_slice() else { return Err(bad("unexpected encryption scheme")) };
    if oid_string(enc_oid.body) != OID_AES256_CBC {
        return Err(bad("the cipher is not AES-256-CBC"));
    }
    // NSS stores 14 iv bytes and prepends the OCTET STRING header to make 16.
    let iv: Vec<u8> = match iv.body.len() {
        14 => [&[0x04, 0x0e][..], iv.body].concat(),
        16 => iv.body.to_vec(),
        _ => return Err(bad("bad iv length")),
    };
    let pw =
        Zeroizing::new(Sha1::new().chain_update(global_salt).chain_update(password.as_bytes()).finalize().to_vec());
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(&pw, salt, iterations, key.as_mut());
    Aes256CbcDec::new_from_slices(key.as_ref(), &iv)
        .map_err(|_| bad("bad key or iv"))?
        .decrypt_padded_vec::<Pkcs7>(ciphertext)
        .map(Zeroizing::new)
        .map_err(|_| ImportError::WrongPassword)
}

fn decrypt_pbe_3des(
    params: Tlv<'_>,
    ciphertext: &[u8],
    global_salt: &[u8],
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let bad = |what: &str| ImportError::Format(format!("key4.db PBE-SHA1-3DES: {what}"));
    let parts = children(params.body).map_err(|e| bad(&e))?;
    let [salt, _iterations] = parts.as_slice() else { return Err(bad("unexpected parameters")) };
    let entry_salt = salt.body;
    if entry_salt.is_empty() || entry_salt.len() > 20 {
        return Err(bad("bad salt length"));
    }
    let hp =
        Zeroizing::new(Sha1::new().chain_update(global_salt).chain_update(password.as_bytes()).finalize().to_vec());
    let mut pes = entry_salt.to_vec();
    pes.resize(20, 0);
    let chp = Zeroizing::new(Sha1::new().chain_update(hp.as_slice()).chain_update(entry_salt).finalize().to_vec());
    let mac = |data: &[&[u8]]| -> Zeroizing<Vec<u8>> {
        let mut m = Hmac::<Sha1>::new_from_slice(chp.as_slice()).expect("HMAC takes any key length");
        for d in data {
            m.update(d);
        }
        Zeroizing::new(m.finalize().into_bytes().to_vec())
    };
    let k1 = mac(&[&pes, entry_salt]);
    let tk = mac(&[&pes]);
    let k2 = mac(&[&tk, entry_salt]);
    let k = Zeroizing::new([k1.as_slice(), k2.as_slice()].concat());
    let (key, iv) = (&k[..24], &k[k.len() - 8..]);
    Des3CbcDec::new_from_slices(key, iv)
        .map_err(|_| bad("bad key or iv"))?
        .decrypt_padded_vec::<Pkcs7>(ciphertext)
        .map(Zeroizing::new)
        .map_err(|_| ImportError::WrongPassword)
}

fn be_u32(body: &[u8]) -> u32 {
    body.iter().fold(0u32, |a, &b| a.wrapping_shl(8) | u32::from(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testing::fixture;

    #[test]
    fn pbes2_without_primary_password() {
        // The AES-256-CBC SDR variant of current Firefox: a 32-byte key.
        let k = Key4::read(&fixture("firefox-nomaster/key4.db")).unwrap();
        let sdr = k.unlock("").unwrap();
        assert_eq!(sdr.first().key.len(), 32);
        assert!(!k.has_primary_password() && k.key_count() == 1);
        assert!(matches!(k.unlock("anything"), Err(ImportError::WrongPassword)));
    }

    #[test]
    fn pbes2_with_primary_password_and_wrong_one() {
        let k = Key4::read(&fixture("firefox-master/key4.db")).unwrap();
        assert!(matches!(k.unlock("not it"), Err(ImportError::WrongPassword)));
        assert!(matches!(k.unlock(""), Err(ImportError::WrongPassword)));
        let sdr = k.unlock("fx-master-pw").unwrap();
        assert_eq!(sdr.first().key.len(), 24);
        assert!(k.has_primary_password());
    }

    #[test]
    fn legacy_3des_key_database() {
        let k = Key4::read(&fixture("firefox-legacy/key4.db")).unwrap();
        assert!(matches!(k.unlock("wrong"), Err(ImportError::WrongPassword)));
        let sdr = k.unlock("fx-master-pw").unwrap();
        assert_eq!(sdr.first().key.len(), 24);
    }

    #[test]
    fn two_keys_each_login_with_its_own() {
        // 0.7.2 (R12): an old 3DES key first, then the AES key; logins under either.
        let k = Key4::read(&fixture("firefox-twokeys/key4.db")).unwrap();
        assert_eq!(k.key_count(), 2);
        let keys = k.unlock("").unwrap();
        assert_eq!((keys.keys.len(), keys.unusable), (2, 0));
    }

    #[test]
    fn a_key_only_in_the_write_ahead_log_is_read() {
        // Firefox running: its newest key4.db rows may sit in key4.db-wal. The import copies
        // the log along; the copy (here a temporary one) is opened with it.
        let src = fixture("firefox-wal");
        let dir = std::env::temp_dir().join(format!("krunk-wal-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["key4.db", "key4.db-wal"] {
            std::fs::copy(src.join(f), dir.join(f)).unwrap();
        }
        let k = Key4::read(&dir.join("key4.db")).unwrap();
        assert_eq!(k.key_count(), 2, "the second key is only in the log");
        assert_eq!(k.unlock("").unwrap().keys.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_is_a_format_error_not_a_panic() {
        let k = Key4 { global_salt: vec![1; 20], password_check: vec![0x30, 0x03, 0x02, 0x01, 0x01], keys: vec![] };
        assert!(matches!(k.unlock(""), Err(ImportError::WrongPassword | ImportError::Format(_))));
        let sdr = SdrKeys { keys: vec![SdrKey { key: Zeroizing::new(vec![0; 24]), id: vec![1] }], unusable: 0 };
        assert_eq!(sdr.decrypt_field("not base64!").err(), Some(FieldError::NotBase64));
        assert_eq!(sdr.decrypt_field("MAA=").err(), Some(FieldError::Structure));
        assert_eq!(sdr.decrypt_field(" ").err(), Some(FieldError::Missing));
        assert!(matches!(Key4::read(&fixture("firefox-nomaster/logins.json")), Err(ImportError::Format(_))));
    }
}
