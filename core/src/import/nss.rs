//! The NSS key database of a Firefox profile (`key4.db`) and the "SDR" blobs that
//! `logins.json` stores.
//!
//! `metaData` holds the global salt (`item1`) and the password check (`item2`): a PKCS#5
//! blob that decrypts to `password-check` with the right primary password (the empty string
//! when none is set). `nssPrivate` holds the SDR key (`a11`), encrypted the same way, with its
//! id (`a102`). Two PBE schemes occur: PBES2 (PBKDF2-HMAC-SHA256 + AES-256-CBC; the PBKDF2
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
    key_blob: Vec<u8>,
    key_id: Vec<u8>,
}

/// The unlocked SDR key.
pub struct SdrKey {
    key: Zeroizing<Vec<u8>>,
    id: Vec<u8>,
}

impl Key4 {
    /// Reads `key4.db` (a copy; opened read-only).
    pub fn read(path: &Path) -> Result<Key4, ImportError> {
        let conn = open_readonly(path)?;
        let fmt = |e: rusqlite::Error| ImportError::Format(format!("key4.db: {e}"));
        let (global_salt, password_check): (Vec<u8>, Vec<u8>) = conn
            .query_row("SELECT item1, item2 FROM metaData WHERE id = 'password'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(fmt)?;
        let (key_blob, key_id): (Vec<u8>, Vec<u8>) = conn
            .query_row("SELECT a11, a102 FROM nssPrivate WHERE a11 IS NOT NULL ORDER BY id LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|_| ImportError::Format("key4.db has no SDR key (no logins were ever saved)".into()))?;
        Ok(Key4 { global_salt, password_check, key_blob, key_id })
    }

    /// Checks the primary password and unlocks the SDR key.
    pub fn unlock(&self, primary_password: &str) -> Result<SdrKey, ImportError> {
        let check = decrypt_pbe(&self.password_check, &self.global_salt, primary_password);
        if check.as_deref().map(Vec::as_slice) != Ok(b"password-check") {
            return Err(ImportError::WrongPassword);
        }
        let key = decrypt_pbe(&self.key_blob, &self.global_salt, primary_password)?;
        if key.len() < 24 {
            return Err(ImportError::Format("the SDR key is too short".into()));
        }
        Ok(SdrKey { key, id: self.key_id.clone() })
    }
}

impl SdrKey {
    /// Decrypts one base64 SDR blob from `logins.json` to text.
    pub fn decrypt_field(&self, b64: &str) -> Result<Zeroizing<String>, ImportError> {
        let der =
            Base64::decode_vec(b64.trim()).map_err(|_| ImportError::Format("login field is not base64".into()))?;
        let bad = |what: &str| ImportError::Format(format!("login field: {what}"));
        let (outer, _) = tlv(&der).map_err(|e| bad(&e))?;
        let parts = children(outer.body).map_err(|e| bad(&e))?;
        let [key_id, algo, ciphertext] = parts.as_slice() else { return Err(bad("unexpected structure")) };
        if key_id.body != self.id.as_slice() {
            return Err(bad("encrypted with another key"));
        }
        let algo = children(algo.body).map_err(|e| bad(&e))?;
        let [oid, iv] = algo.as_slice() else { return Err(bad("unexpected cipher structure")) };
        let plain = match oid_string(oid.body).as_str() {
            OID_DES_EDE3_CBC => Des3CbcDec::new_from_slices(&self.key[..24], iv.body)
                .map_err(|_| bad("bad 3DES key or iv"))?
                .decrypt_padded_vec::<Pkcs7>(ciphertext.body)
                .map_err(|_| bad("3DES decryption failed"))?,
            OID_AES256_CBC => Aes256CbcDec::new_from_slices(
                self.key.get(..32).ok_or_else(|| bad("the key is not an AES key"))?,
                iv.body,
            )
            .map_err(|_| bad("bad AES key or iv"))?
            .decrypt_padded_vec::<Pkcs7>(ciphertext.body)
            .map_err(|_| bad("AES decryption failed"))?,
            other => return Err(bad(&format!("unsupported cipher {other}"))),
        };
        let plain = Zeroizing::new(plain);
        String::from_utf8(plain.to_vec()).map(Zeroizing::new).map_err(|_| bad("not UTF-8"))
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
        assert_eq!(sdr.key.len(), 32);
        assert!(matches!(k.unlock("anything"), Err(ImportError::WrongPassword)));
    }

    #[test]
    fn pbes2_with_primary_password_and_wrong_one() {
        let k = Key4::read(&fixture("firefox-master/key4.db")).unwrap();
        assert!(matches!(k.unlock("not it"), Err(ImportError::WrongPassword)));
        assert!(matches!(k.unlock(""), Err(ImportError::WrongPassword)));
        let sdr = k.unlock("fx-master-pw").unwrap();
        assert_eq!(sdr.key.len(), 24);
    }

    #[test]
    fn legacy_3des_key_database() {
        let k = Key4::read(&fixture("firefox-legacy/key4.db")).unwrap();
        assert!(matches!(k.unlock("wrong"), Err(ImportError::WrongPassword)));
        let sdr = k.unlock("fx-master-pw").unwrap();
        assert_eq!(sdr.key.len(), 24);
    }

    #[test]
    fn garbage_is_a_format_error_not_a_panic() {
        let k = Key4 {
            global_salt: vec![1; 20],
            password_check: vec![0x30, 0x03, 0x02, 0x01, 0x01],
            key_blob: vec![],
            key_id: vec![],
        };
        assert!(matches!(k.unlock(""), Err(ImportError::WrongPassword | ImportError::Format(_))));
        let sdr = SdrKey { key: Zeroizing::new(vec![0; 24]), id: vec![1] };
        assert!(matches!(sdr.decrypt_field("not base64!"), Err(ImportError::Format(_))));
        assert!(matches!(sdr.decrypt_field("MAA="), Err(ImportError::Format(_))));
        assert!(matches!(Key4::read(&fixture("firefox-nomaster/logins.json")), Err(ImportError::Format(_))));
    }
}
