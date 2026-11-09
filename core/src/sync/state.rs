//! `sync.json`: this device's sync key material (PROTOCOL §8, as clarified in 0.9.0).
//!
//! - **What is in it:** the device's general X25519 key, the AK of the current epoch (and older
//!   ones still needed while records are re-encrypted), the accepted `ak_dir` head, the last pull
//!   position and the HLC. The account identity and the rotating refresh token stay in
//!   `account.json` (0.8.0), so a token rotation never has two copies to keep in step.
//! - **How it is stored:** everything except `v` is sealed with XChaCha20-Poly1305 under a fresh
//!   random 32-byte state key at every save; the state key is wrapped by the platform
//!   (`KeyWrapper`: DPAPI with the entropy "Krunk sync state v1" on Windows).
//! - **PAK-level state** (PAK, the vault X25519 key, the PAK pins and queue) lives inside the
//!   vault instead (`vault::VaultSync`), so it never needs less than the master password.

use super::crypto::{self, Key};
use super::dir::AkHead;
use super::AccountId;
use crate::vault::KeyWrapper;
use base64ct::{Base64, Encoding};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroizing;

/// `v` of the file.
pub const STATE_VERSION: u32 = 1;
const AAD: &[u8] = b"krunk-sync-state/v1";
/// A state larger than this is refused (it holds a few keys and counters).
const MAX_FILE: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    Io(String),
    /// The platform couldn't wrap or unwrap the state key (another Windows user, another PC).
    Wrap(String),
    /// The file doesn't parse or fails authentication.
    Corrupt(&'static str),
    /// Written by a newer Krunk.
    Version(u32),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Io(e) => write!(f, "sync state: {e}"),
            StateError::Wrap(e) => write!(f, "sync state key: {e}"),
            StateError::Corrupt(what) => write!(f, "sync state is damaged ({what})"),
            StateError::Version(v) => write!(f, "sync state version {v} is newer than this Krunk"),
        }
    }
}

impl std::error::Error for StateError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    v: u32,
    wrap: String,
    /// `wrapper.protect(state key)`, base64.
    key: String,
    n: String,
    ct: String,
}

/// The accepted `ak_dir` head (a serializable [`AkHead`] without the account id).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AkPin {
    pub epoch: u32,
    #[serde(with = "b64_key")]
    pub ak: Key,
    pub seq: u64,
    #[serde(with = "b64_32")]
    pub hash: [u8; 32],
    pub min_record_epoch: u32,
}

impl AkPin {
    pub fn head(&self, account_id: AccountId) -> AkHead {
        AkHead {
            account_id,
            epoch: self.epoch,
            ak: self.ak.clone(),
            seq: self.seq,
            hash: self.hash,
            min_record_epoch: self.min_record_epoch,
        }
    }

    pub fn from_head(h: &AkHead) -> AkPin {
        AkPin { epoch: h.epoch, ak: h.ak.clone(), seq: h.seq, hash: h.hash, min_record_epoch: h.min_record_epoch }
    }
}

/// An older epoch's AK, kept while records of that epoch may still be read (PROTOCOL §7 step 5).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpochKey {
    pub epoch: u32,
    #[serde(with = "b64_key")]
    pub key: Key,
}

/// The device's sync state. New fields are optional (`serde(default)`), so a file written by a
/// newer 0.9.x still opens.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    /// The account (hex, 32 characters) and this device, as the server named them.
    pub account_id: String,
    pub device_id: String,
    pub dev_short: u16,
    /// The general X25519 private key (its public key is in `ak_dir`).
    #[serde(with = "b64_key")]
    pub device_sk: Key,
    /// The accepted `ak_dir` head; `None` until the device holds keys.
    #[serde(default)]
    pub ak: Option<AkPin>,
    #[serde(default)]
    pub ak_old: Vec<EpochKey>,
    #[serde(default)]
    pub keyset_gen: String,
    /// The last `seq` pulled from `records`.
    #[serde(default)]
    pub last_pull_seq: u64,
    /// The last HLC value issued or observed.
    #[serde(default)]
    pub hlc: u64,
    /// A fork or tamper alarm: writes stop until it is cleared (PLAN §5.6).
    #[serde(default)]
    pub alarm: Option<String>,
}

impl std::fmt::Debug for SyncState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncState")
            .field("device_id", &self.device_id)
            .field("dev_short", &self.dev_short)
            .field("epoch", &self.ak.as_ref().map(|a| a.epoch))
            .field("last_pull_seq", &self.last_pull_seq)
            .field("alarm", &self.alarm)
            .finish_non_exhaustive()
    }
}

impl SyncState {
    /// The account id as the 16 bytes bound into every MAC and AAD.
    pub fn account_bytes(&self) -> Result<AccountId, StateError> {
        parse_account_id(&self.account_id).ok_or(StateError::Corrupt("account id"))
    }
}

/// A server account or device id (a UUID, with or without dashes) as 16 bytes.
pub fn parse_account_id(text: &str) -> Option<AccountId> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The sealed file for `state`.
pub fn seal(state: &SyncState, wrapper: &dyn KeyWrapper) -> Result<Vec<u8>, StateError> {
    let key: Key = Zeroizing::new(crypto::random::<32>().map_err(|_| StateError::Wrap("random".into()))?);
    let pt = Zeroizing::new(serde_json::to_vec(state).map_err(|_| StateError::Corrupt("encoding"))?);
    let (n, ct) = crypto::aead_seal(&key, AAD, &pt).map_err(|_| StateError::Corrupt("sealing"))?;
    let wrapped = wrapper.protect(key.as_ref()).map_err(StateError::Wrap)?;
    let file = StateFile {
        v: STATE_VERSION,
        wrap: wrapper.name().into(),
        key: Base64::encode_string(&wrapped),
        n: Base64::encode_string(&n),
        ct: Base64::encode_string(&ct),
    };
    serde_json::to_vec_pretty(&file).map_err(|_| StateError::Corrupt("encoding"))
}

/// Opens a sealed file.
pub fn open(bytes: &[u8], wrapper: &dyn KeyWrapper) -> Result<SyncState, StateError> {
    if bytes.len() > MAX_FILE {
        return Err(StateError::Corrupt("size"));
    }
    let file: StateFile = serde_json::from_slice(bytes).map_err(|_| StateError::Corrupt("format"))?;
    if file.v != STATE_VERSION {
        return Err(StateError::Version(file.v));
    }
    if file.wrap != wrapper.name() {
        return Err(StateError::Wrap(format!("the file uses '{}' key wrapping", file.wrap)));
    }
    let wrapped = Base64::decode_vec(&file.key).map_err(|_| StateError::Corrupt("key"))?;
    let raw = wrapper.unprotect(&wrapped).map_err(StateError::Wrap)?;
    let key: Key = Zeroizing::new(raw.as_slice().try_into().map_err(|_| StateError::Corrupt("key length"))?);
    let n = Base64::decode_vec(&file.n).map_err(|_| StateError::Corrupt("nonce"))?;
    let ct = Base64::decode_vec(&file.ct).map_err(|_| StateError::Corrupt("ciphertext"))?;
    let pt = crypto::aead_open(&key, &n, AAD, &ct).map_err(|_| StateError::Corrupt("authentication"))?;
    let state: SyncState = serde_json::from_slice(&pt).map_err(|_| StateError::Corrupt("contents"))?;
    state.account_bytes()?;
    Ok(state)
}

/// Writes `state` to `path` atomically.
pub fn save(path: &Path, state: &SyncState, wrapper: &dyn KeyWrapper) -> Result<(), StateError> {
    let bytes = seal(state, wrapper)?;
    crate::vault::write_atomic(path, &bytes).map_err(|e| StateError::Io(e.to_string()))
}

/// The state at `path`; `None` when there is no file (sync was never turned on).
pub fn load(path: &Path, wrapper: &dyn KeyWrapper) -> Result<Option<SyncState>, StateError> {
    match std::fs::read(path) {
        Ok(bytes) => open(&bytes, wrapper).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StateError::Io(e.to_string())),
    }
}

mod b64_key {
    use super::Key;
    use base64ct::{Base64, Encoding};
    use serde::{Deserialize, Deserializer, Serializer};
    use zeroize::Zeroizing;

    pub fn serialize<S: Serializer>(k: &Key, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&Zeroizing::new(Base64::encode_string(k.as_ref())))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        let text = Zeroizing::new(String::deserialize(d)?);
        let raw = Zeroizing::new(Base64::decode_vec(&text).map_err(serde::de::Error::custom)?);
        let arr: [u8; 32] = raw.as_slice().try_into().map_err(|_| serde::de::Error::custom("key length"))?;
        Ok(Zeroizing::new(arr))
    }
}

mod b64_32 {
    use base64ct::{Base64, Encoding};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&Base64::encode_string(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let raw = Base64::decode_vec(&String::deserialize(d)?).map_err(serde::de::Error::custom)?;
        raw.as_slice().try_into().map_err(|_| serde::de::Error::custom("length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::NoWrap;

    /// A wrapper standing in for DPAPI: XOR with a per-"user" byte, so another user fails.
    struct UserWrap(u8);
    impl KeyWrapper for UserWrap {
        fn name(&self) -> &'static str {
            "dpapi-sync"
        }
        fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
            let mut v = vec![self.0];
            v.extend(data.iter().map(|b| b ^ self.0));
            Ok(v)
        }
        fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            if data.first() != Some(&self.0) {
                return Err("another user's data".into());
            }
            Ok(Zeroizing::new(data[1..].iter().map(|b| b ^ self.0).collect()))
        }
    }

    fn sample() -> SyncState {
        SyncState {
            account_id: "0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0".into(),
            device_id: "7d4012d9-f415-4a68-9b92-e029ffd1db29".into(),
            dev_short: 3,
            device_sk: Zeroizing::new([9; 32]),
            ak: Some(AkPin { epoch: 2, ak: Zeroizing::new([5; 32]), seq: 7, hash: [6; 32], min_record_epoch: 1 }),
            ak_old: vec![EpochKey { epoch: 1, key: Zeroizing::new([4; 32]) }],
            keyset_gen: "g1".into(),
            last_pull_seq: 42,
            hlc: 1 << 20,
            alarm: None,
        }
    }

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lb-syncstate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_through_the_wrapper_and_no_key_in_clear() {
        let d = dir("rt");
        let path = d.join("sync.json");
        assert_eq!(load(&path, &UserWrap(7)).unwrap(), None, "no file: sync never turned on");
        let st = sample();
        save(&path, &st, &UserWrap(7)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in [[9u8; 32], [5; 32], [4; 32]] {
            assert!(!text.contains(&Base64::encode_string(&secret)), "a key in clear");
        }
        assert!(!text.contains("7d4012d9") && !text.contains("0b1c2d3e"), "ids are sealed too");
        let back = load(&path, &UserWrap(7)).unwrap().unwrap();
        assert!(back == st);
        assert_eq!(back.account_bytes().unwrap()[..2], [0x0b, 0x1c]);
        // A fresh state key at every save.
        save(&path, &st, &UserWrap(7)).unwrap();
        assert_ne!(std::fs::read_to_string(&path).unwrap(), text);
        // Another Windows user (or PC) can't open it; nor can another wrapping kind.
        assert!(matches!(load(&path, &UserWrap(8)), Err(StateError::Wrap(_))));
        assert!(matches!(load(&path, &NoWrap), Err(StateError::Wrap(_))));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn damaged_or_newer_files_are_refused() {
        let st = sample();
        let good = seal(&st, &NoWrap).unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&good).unwrap();
        let mut ct = Base64::decode_vec(v["ct"].as_str().unwrap()).unwrap();
        ct[3] ^= 1;
        v["ct"] = Base64::encode_string(&ct).into();
        assert_eq!(open(&serde_json::to_vec(&v).unwrap(), &NoWrap), Err(StateError::Corrupt("authentication")));
        let mut v: serde_json::Value = serde_json::from_slice(&good).unwrap();
        v["v"] = 2.into();
        assert_eq!(open(&serde_json::to_vec(&v).unwrap(), &NoWrap), Err(StateError::Version(2)));
        let mut v: serde_json::Value = serde_json::from_slice(&good).unwrap();
        v["extra"] = 1.into();
        assert_eq!(open(&serde_json::to_vec(&v).unwrap(), &NoWrap), Err(StateError::Corrupt("format")));
        assert_eq!(open(b"not json", &NoWrap), Err(StateError::Corrupt("format")));
        // A state with a bad account id isn't accepted either.
        let bad = SyncState { account_id: "nope".into(), ..sample() };
        assert_eq!(open(&seal(&bad, &NoWrap).unwrap(), &NoWrap), Err(StateError::Corrupt("account id")));
        // The debug form names no key.
        let dbg = format!("{:?}", sample());
        assert!(!dbg.contains("[9, 9") && dbg.contains("dev_short: 3"), "{dbg}");
    }

    #[test]
    fn the_ak_pin_converts_to_and_from_the_acceptance_head() {
        let st = sample();
        let acc = st.account_bytes().unwrap();
        let head = st.ak.as_ref().unwrap().head(acc);
        assert_eq!((head.epoch, head.seq, head.min_record_epoch), (2, 7, 1));
        assert!(AkPin::from_head(&head) == *st.ak.as_ref().unwrap());
        assert_eq!(parse_account_id("0B1C2D3E4F506172 8394A5B6C7D8E9F0".replace(' ', "").as_str()).unwrap()[15], 0xf0);
        assert_eq!(parse_account_id("0b1c"), None);
    }
}
