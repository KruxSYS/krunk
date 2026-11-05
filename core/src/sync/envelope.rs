//! Records (PROTOCOL §9.1–§9.2): opaque ids, the exact AAD, padding buckets, and the
//! encrypted envelope `{v, c, id, e, k, n, ct}` around `{ver, dev, deleted, data}`.

use super::crypto::{self, Key};
use super::{labels, AccountId, SyncError};
use base64ct::{Base64UrlUnpadded, Encoding};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const ID_LEN: usize = 16;
pub const MIN_BUCKET: usize = 256;
pub const MAX_BUCKET: usize = 65536;
/// The largest plaintext that fits the largest bucket (one byte is the 0x80 marker).
pub const MAX_PLAINTEXT: usize = MAX_BUCKET - 1;

/// Which key domain a collection belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    Ak,
    Pak,
}

impl KeyKind {
    pub fn label(self) -> &'static str {
        match self {
            KeyKind::Ak => "ak",
            KeyKind::Pak => "pak",
        }
    }
}

/// The collections of v1 and their key domain (PROTOCOL §9.5).
pub const COLLECTIONS: &[(&str, KeyKind)] = &[
    ("history", KeyKind::Ak),
    ("bookmarks", KeyKind::Ak),
    ("settings", KeyKind::Ak),
    ("tabs", KeyKind::Ak),
    ("allowlist", KeyKind::Ak),
    ("pins", KeyKind::Ak),
    ("device_meta", KeyKind::Ak),
    ("passwords", KeyKind::Pak),
    ("never_save", KeyKind::Pak),
];

pub fn key_kind(collection: &str) -> Option<KeyKind> {
    COLLECTIONS.iter().find(|(c, _)| *c == collection).map(|(_, k)| *k)
}

/// The keys of one collection in one epoch: `CK` encrypts, `IK` derives record ids.
pub struct CollectionKeys {
    pub ck: Key,
    pub ik: Key,
}

/// `CK = HKDF(root, account_id, "…/enc/<c>/<e>")`, `IK = HKDF(root, account_id, "…/id/<c>/<e>")`.
/// `root` is AK for AK collections and PAK for PAK collections; `epoch` is the matching epoch.
pub fn collection_keys(root: &[u8; 32], account_id: &AccountId, collection: &str, epoch: u32) -> CollectionKeys {
    CollectionKeys {
        ck: crypto::hkdf32(root, account_id, &labels::enc(collection, epoch)),
        ik: crypto::hkdf32(root, account_id, &labels::id(collection, epoch)),
    }
}

/// `id = HMAC(IK, natural_key)[0..16]`.
pub fn record_id(ik: &[u8; 32], natural_key: &[u8]) -> [u8; ID_LEN] {
    let mut id = [0u8; ID_LEN];
    id.copy_from_slice(&crypto::hmac(ik, natural_key)[..ID_LEN]);
    id
}

/// `lp("krunk-sync/v1/rec") ‖ u8(v) ‖ account_id ‖ lp(c) ‖ id ‖ lp(k) ‖ u32be(e)`.
pub fn aad(
    account_id: &AccountId,
    collection: &str,
    id: &[u8; ID_LEN],
    kind: KeyKind,
    epoch: u32,
) -> Result<Vec<u8>, SyncError> {
    let mut a = Vec::with_capacity(64);
    crypto::put_lp(&mut a, labels::REC.as_bytes())?;
    a.push(labels::VERSION);
    a.extend_from_slice(account_id);
    crypto::put_lp(&mut a, collection.as_bytes())?;
    a.extend_from_slice(id);
    crypto::put_lp(&mut a, kind.label().as_bytes())?;
    a.extend_from_slice(&epoch.to_be_bytes());
    Ok(a)
}

/// The padding bucket for a plaintext: the smallest power of two ≥ 256 that holds it plus
/// the 0x80 marker.
pub fn bucket_for(len: usize) -> Result<usize, SyncError> {
    if len > MAX_PLAINTEXT {
        return Err(SyncError::TooLarge);
    }
    Ok((len + 1).next_power_of_two().max(MIN_BUCKET))
}

/// ISO/IEC 7816-4 padding: `pt ‖ 0x80 ‖ 0x00…` to exactly `bucket` bytes.
pub fn pad(pt: &[u8], bucket: usize) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    if !bucket.is_power_of_two() || !(MIN_BUCKET..=MAX_BUCKET).contains(&bucket) || pt.len() + 1 > bucket {
        return Err(SyncError::TooLarge);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(bucket));
    out.extend_from_slice(pt);
    out.push(0x80);
    out.resize(bucket, 0);
    Ok(out)
}

pub fn unpad(buf: &[u8]) -> Result<&[u8], SyncError> {
    if !buf.len().is_power_of_two() || !(MIN_BUCKET..=MAX_BUCKET).contains(&buf.len()) {
        return Err(SyncError::Malformed("padding bucket"));
    }
    let end = buf.iter().rposition(|&b| b != 0).ok_or(SyncError::Malformed("padding"))?;
    if buf[end] != 0x80 {
        return Err(SyncError::Malformed("padding"));
    }
    Ok(&buf[..end])
}

/// The record plaintext: the version stays inside the ciphertext [R16].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordPlain {
    pub ver: u64,
    pub dev: u16,
    pub deleted: bool,
    pub data: serde_json::Value,
}

/// The wire envelope (JSON; byte fields are strict base64url without padding).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub v: u8,
    pub c: String,
    pub id: String,
    pub e: u32,
    pub k: KeyKind,
    pub n: String,
    pub ct: String,
}

pub fn b64(b: &[u8]) -> String {
    Base64UrlUnpadded::encode_string(b)
}

/// Strict base64url decoding (no padding, no whitespace, canonical trailing bits).
pub fn unb64(s: &str) -> Result<Vec<u8>, SyncError> {
    let v = Base64UrlUnpadded::decode_vec(s).map_err(|_| SyncError::Malformed("base64"))?;
    if b64(&v) != s {
        return Err(SyncError::Malformed("base64"));
    }
    Ok(v)
}

/// Encrypts a record. `bucket` is normally `bucket_for(len)`; a tombstone passes the bucket of
/// the record's last version (PROTOCOL §9.4).
pub fn seal_record(
    ck: &[u8; 32],
    account_id: &AccountId,
    collection: &str,
    id: &[u8; ID_LEN],
    epoch: u32,
    plain: &RecordPlain,
    bucket: Option<usize>,
) -> Result<Envelope, SyncError> {
    let nonce = crypto::random::<{ crypto::NONCE_LEN }>()?;
    seal_record_with_nonce(ck, account_id, collection, id, epoch, plain, bucket, &nonce)
}

#[allow(clippy::too_many_arguments)]
pub fn seal_record_with_nonce(
    ck: &[u8; 32],
    account_id: &AccountId,
    collection: &str,
    id: &[u8; ID_LEN],
    epoch: u32,
    plain: &RecordPlain,
    bucket: Option<usize>,
    nonce: &[u8; crypto::NONCE_LEN],
) -> Result<Envelope, SyncError> {
    let kind = key_kind(collection).ok_or(SyncError::Malformed("collection"))?;
    let json = Zeroizing::new(serde_json::to_vec(plain).map_err(|_| SyncError::Malformed("record"))?);
    let bucket = match bucket {
        Some(b) if b > json.len() => b,
        _ => bucket_for(json.len())?,
    };
    let padded = pad(&json, bucket)?;
    let ct = crypto::aead_seal_with_nonce(ck, nonce, &aad(account_id, collection, id, kind, epoch)?, &padded)?;
    Ok(Envelope {
        v: labels::VERSION,
        c: collection.into(),
        id: b64(id),
        e: epoch,
        k: kind,
        n: b64(nonce),
        ct: b64(&ct),
    })
}

/// Decrypts and checks an envelope. The AAD is rebuilt from the envelope's own fields, so a
/// record moved to another collection, id, epoch, key kind or account fails authentication.
pub fn open_record(ck: &[u8; 32], account_id: &AccountId, env: &Envelope) -> Result<RecordPlain, SyncError> {
    if env.v != labels::VERSION {
        return Err(SyncError::Malformed("version"));
    }
    if key_kind(&env.c) != Some(env.k) {
        return Err(SyncError::Malformed("collection or key kind"));
    }
    let id: [u8; ID_LEN] = unb64(&env.id)?.try_into().map_err(|_| SyncError::Malformed("id"))?;
    let nonce = unb64(&env.n)?;
    let ct = unb64(&env.ct)?;
    if ct.len() > MAX_BUCKET + crypto::TAG_LEN {
        return Err(SyncError::TooLarge);
    }
    let padded = crypto::aead_open(ck, &nonce, &aad(account_id, &env.c, &id, env.k, env.e)?, &ct)?;
    let json = unpad(&padded)?;
    serde_json::from_slice(json).map_err(|_| SyncError::Malformed("record"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACC: AccountId = [1u8; 16];

    fn plain() -> RecordPlain {
        RecordPlain {
            ver: 42,
            dev: 3,
            deleted: false,
            data: serde_json::json!({"url": "https://example.com/", "title": "Example"}),
        }
    }

    #[test]
    fn buckets_and_padding() {
        assert_eq!(bucket_for(0).unwrap(), 256);
        assert_eq!(bucket_for(255).unwrap(), 256);
        assert_eq!(bucket_for(256).unwrap(), 512);
        assert_eq!(bucket_for(MAX_PLAINTEXT).unwrap(), MAX_BUCKET);
        assert_eq!(bucket_for(MAX_PLAINTEXT + 1), Err(SyncError::TooLarge));
        for len in [0usize, 1, 254, 255, 256, 257, 1000] {
            let pt = vec![0xabu8; len];
            let p = pad(&pt, bucket_for(len).unwrap()).unwrap();
            assert_eq!(p.len(), bucket_for(len).unwrap());
            assert_eq!(unpad(&p).unwrap(), &pt[..]);
        }
        assert!(unpad(&[0u8; 256]).is_err(), "no marker");
        assert!(unpad(&[0x80u8; 300]).is_err(), "not a bucket size");
        let mut tail = vec![0u8; 256];
        tail[255] = 1;
        assert!(unpad(&tail).is_err(), "last non-zero byte must be 0x80");
    }

    #[test]
    fn round_trip_and_binding() {
        let keys = collection_keys(&[9u8; 32], &ACC, "history", 2);
        let id = record_id(&keys.ik, b"https://example.com/");
        let env = seal_record(&keys.ck, &ACC, "history", &id, 2, &plain(), None).unwrap();
        assert_eq!(env.k, KeyKind::Ak);
        assert_eq!(unb64(&env.ct).unwrap().len(), 256 + 16, "padded to a bucket");
        assert_eq!(open_record(&keys.ck, &ACC, &env).unwrap(), plain());
        // Swaps: collection, id, epoch, account, key kind, version.
        let mut e = env.clone();
        e.c = "pins".into();
        assert_eq!(open_record(&keys.ck, &ACC, &e), Err(SyncError::Auth));
        let mut e = env.clone();
        e.id = b64(&[0u8; 16]);
        assert_eq!(open_record(&keys.ck, &ACC, &e), Err(SyncError::Auth));
        let mut e = env.clone();
        e.e = 3;
        assert_eq!(open_record(&keys.ck, &ACC, &e), Err(SyncError::Auth));
        assert_eq!(open_record(&keys.ck, &[2u8; 16], &env), Err(SyncError::Auth));
        let mut e = env.clone();
        e.k = KeyKind::Pak;
        assert_eq!(open_record(&keys.ck, &ACC, &e), Err(SyncError::Malformed("collection or key kind")));
        let mut e = env.clone();
        e.v = 2;
        assert_eq!(open_record(&keys.ck, &ACC, &e), Err(SyncError::Malformed("version")));
        // Another collection's key.
        let other = collection_keys(&[9u8; 32], &ACC, "bookmarks", 2);
        assert_eq!(open_record(&other.ck, &ACC, &env), Err(SyncError::Auth));
    }

    #[test]
    fn tombstones_keep_the_bucket() {
        let keys = collection_keys(&[9u8; 32], &ACC, "bookmarks", 1);
        let id = record_id(&keys.ik, &[7u8; 16]);
        let big = RecordPlain { data: serde_json::json!({"title": "x".repeat(700)}), ..plain() };
        let full = seal_record(&keys.ck, &ACC, "bookmarks", &id, 1, &big, None).unwrap();
        let tomb = RecordPlain { ver: 43, dev: 3, deleted: true, data: serde_json::Value::Null };
        let t = seal_record(&keys.ck, &ACC, "bookmarks", &id, 1, &tomb, Some(1024)).unwrap();
        assert_eq!(unb64(&full.ct).unwrap().len(), unb64(&t.ct).unwrap().len());
    }

    #[test]
    fn strict_base64() {
        assert!(unb64("AAAA").is_ok());
        assert!(unb64("AAAA=").is_err(), "padding");
        assert!(unb64("AA AA").is_err(), "whitespace");
        assert!(unb64("AB").is_err(), "non-canonical trailing bits");
    }

    #[test]
    fn unknown_collections_are_refused() {
        let keys = collection_keys(&[9u8; 32], &ACC, "x", 1);
        assert_eq!(
            seal_record(&keys.ck, &ACC, "x", &[0; 16], 1, &plain(), None).map(|_| ()),
            Err(SyncError::Malformed("collection"))
        );
    }
}
