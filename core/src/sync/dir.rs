//! The key directories (PROTOCOL §4): `ak_dir` and `pak_dir`, their tags (`mac`, `chain`,
//! `escrow_mac`), the history wraps, and the acceptance rules of §4.4 as pure functions.
//!
//! A directory is MAC'd over its **exact bytes**; the server stores those bytes and never
//! re-serialises them. Nothing here trusts a field before the tags are verified.

use super::crypto::{self, KdfParams, Key, SealKind, Sealed};
use super::envelope::{b64, unb64};
use super::{labels, AccountId, SyncError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

// ---------------------------------------------------------------- formats

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEntry {
    pub id: String,
    pub short: u16,
    /// General X25519 public key (b64url).
    pub pk: String,
    pub added_by: String,
    pub added_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealEntry {
    /// A device id, or `"escrow"`.
    pub to: String,
    pub enc: String,
    pub ct: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AkHist {
    /// The epoch of the wrapped key (the previous one).
    pub epoch: u32,
    pub n: String,
    pub ct: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AkDir {
    pub v: u8,
    pub account_id: String,
    pub epoch: u32,
    pub seq: u64,
    /// SHA-256 of the previous `ak_dir` bytes (b64url), empty at seq 1.
    pub prev_hash: String,
    pub devices: Vec<DeviceEntry>,
    pub revoked: Vec<String>,
    pub min_record_epoch: u32,
    pub seals: Vec<SealEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hist: Option<AkHist>,
    pub created_ms: u64,
    pub created_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PpKdf {
    pub alg: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultKey {
    pub device: String,
    pub pk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PakHist {
    pub pak_epoch: u32,
    pub n: String,
    pub ct: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PakDir {
    pub v: u8,
    pub account_id: String,
    pub pak_epoch: u32,
    pub seq: u64,
    pub prev_hash: String,
    pub pp_kdf: PpKdf,
    pub esc_pk: String,
    pub rr_gen: u32,
    pub vault_keys: Vec<VaultKey>,
    pub seals: Vec<SealEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hist: Option<PakHist>,
    pub created_ms: u64,
    pub created_by: String,
}

/// A directory as stored and transferred: the exact bytes and their tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub bytes: Vec<u8>,
    pub mac: [u8; 32],
    pub chain: Option<[u8; 32]>,
    pub escrow_mac: Option<[u8; 32]>,
}

/// Why a directory was not accepted. `Fork` and `BadTag` are security alarms (stop writes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirError {
    Malformed(&'static str),
    WrongAccount,
    /// Older than the accepted head (a replay or rollback): ignored.
    Stale,
    /// Not the next `seq`: fetch the chain in between first.
    Gap,
    /// Different bytes at an accepted `seq`, or a `prev_hash` that doesn't match the head.
    Fork,
    /// More than one epoch ahead: walk the chain epoch by epoch.
    EpochSkip,
    /// A tag (`mac`, `chain`, `escrow_mac`) or a key confirmation failed.
    BadTag,
    /// No seal addressed to us.
    NotForUs,
    /// `pp_kdf` below the built-in floor [R1].
    WeakKdf,
    /// `esc_pk` changed without an `rr_gen` increase in a PAK-authenticated blob [R1].
    EscrowKeyChanged,
    /// The escrow key derived from RR doesn't match the anchor's `esc_pk`.
    EscrowKeyMismatch,
    /// `min_record_epoch` went backwards or beyond the epoch.
    BadMinEpoch,
}

impl From<SyncError> for DirError {
    fn from(e: SyncError) -> DirError {
        match e {
            SyncError::Auth => DirError::BadTag,
            SyncError::WeakKdf => DirError::WeakKdf,
            SyncError::Malformed(w) => DirError::Malformed(w),
            _ => DirError::Malformed("key material"),
        }
    }
}

// ---------------------------------------------------------------- keys and tags

pub fn ak_mac_key(ak: &[u8; 32], account_id: &AccountId, epoch: u32) -> Key {
    crypto::hkdf32(ak, account_id, &labels::ak_dir_mac(epoch))
}

pub fn pak_mac_key(pak: &[u8; 32], account_id: &AccountId, pak_epoch: u32) -> Key {
    crypto::hkdf32(pak, account_id, &labels::pak_dir_mac(pak_epoch))
}

fn escrow_mac_key(rr: &[u8; 32], account_id: &AccountId, label: &str) -> Key {
    crypto::hkdf32(rr, account_id, label)
}

pub fn hash(bytes: &[u8]) -> [u8; 32] {
    crypto::sha256(&[bytes])
}

fn decode32(s: &str, what: &'static str) -> Result<[u8; 32], DirError> {
    unb64(s)?.try_into().map_err(|_| DirError::Malformed(what))
}

fn decode_account(s: &str) -> Result<AccountId, DirError> {
    unb64(s)?.try_into().map_err(|_| DirError::Malformed("account_id"))
}

fn open_seal(seals: &[SealEntry], to: &str, sk: &[u8; 32], info: &[u8]) -> Result<Key, DirError> {
    let s = seals.iter().find(|s| s.to == to).ok_or(DirError::NotForUs)?;
    let sealed = Sealed { enc: decode32(&s.enc, "seal enc")?, ct: unb64(&s.ct)? };
    Ok(crypto::open_key(sk, info, &sealed)?)
}

/// Seals a 32-byte key to a recipient.
pub fn seal_key(
    kind: SealKind,
    key: &[u8; 32],
    account_id: &AccountId,
    epoch: u32,
    to: &str,
    pk: &[u8; 32],
) -> Result<SealEntry, SyncError> {
    let s = crypto::seal(pk, &crypto::seal_info(kind, account_id, epoch, to)?, key)?;
    Ok(SealEntry { to: to.into(), enc: b64(&s.enc), ct: b64(&s.ct) })
}

/// `hist = AEAD(K_hist,new_epoch(new_key), old_key, aad = label ‖ account_id ‖ u32be(new_epoch))`.
pub fn hist_seal(
    kind: SealKind,
    new_key: &[u8; 32],
    old_key: &[u8; 32],
    account_id: &AccountId,
    new_epoch: u32,
) -> Result<(String, String), SyncError> {
    let (k, aad) = hist_key_aad(kind, new_key, account_id, new_epoch);
    let (n, ct) = crypto::aead_seal(&k, &aad, old_key)?;
    Ok((b64(&n), b64(&ct)))
}

pub fn hist_open(
    kind: SealKind,
    new_key: &[u8; 32],
    account_id: &AccountId,
    new_epoch: u32,
    n: &str,
    ct: &str,
) -> Result<Key, DirError> {
    let (k, aad) = hist_key_aad(kind, new_key, account_id, new_epoch);
    let pt = crypto::aead_open(&k, &unb64(n)?, &aad, &unb64(ct)?)?;
    let key: [u8; 32] = pt.as_slice().try_into().map_err(|_| DirError::Malformed("hist"))?;
    Ok(Zeroizing::new(key))
}

fn hist_key_aad(kind: SealKind, new_key: &[u8; 32], account_id: &AccountId, new_epoch: u32) -> (Key, Vec<u8>) {
    let (label, prefix) = match kind {
        SealKind::Ak => (labels::ak_hist(new_epoch), labels::AK_HIST_AAD),
        SealKind::Pak => (labels::pak_hist(new_epoch), labels::PAK_HIST_AAD),
    };
    let mut aad = prefix.as_bytes().to_vec();
    aad.extend_from_slice(account_id);
    aad.extend_from_slice(&new_epoch.to_be_bytes());
    (crypto::hkdf32(new_key, account_id, &label), aad)
}

/// Serialises a directory and computes its tags. `prev_key` is the previous epoch's key on
/// an epoch change (`chain`); `rr` adds `escrow_mac` (an anchor).
pub fn sign_ak(
    dir: &AkDir,
    ak: &[u8; 32],
    prev_ak: Option<&[u8; 32]>,
    rr: Option<&[u8; 32]>,
    account_id: &AccountId,
) -> Result<Blob, SyncError> {
    let bytes = serde_json::to_vec(dir).map_err(|_| SyncError::Malformed("ak_dir"))?;
    Ok(Blob {
        mac: crypto::hmac(ak_mac_key(ak, account_id, dir.epoch).as_ref(), &bytes),
        chain: prev_ak.map(|p| crypto::hmac(ak_mac_key(p, account_id, dir.epoch.saturating_sub(1)).as_ref(), &bytes)),
        escrow_mac: rr.map(|rr| crypto::hmac(escrow_mac_key(rr, account_id, labels::ESCROW_MAC_AK).as_ref(), &bytes)),
        bytes,
    })
}

pub fn sign_pak(
    dir: &PakDir,
    pak: &[u8; 32],
    prev_pak: Option<&[u8; 32]>,
    rr: Option<&[u8; 32]>,
    account_id: &AccountId,
) -> Result<Blob, SyncError> {
    let bytes = serde_json::to_vec(dir).map_err(|_| SyncError::Malformed("pak_dir"))?;
    Ok(Blob {
        mac: crypto::hmac(pak_mac_key(pak, account_id, dir.pak_epoch).as_ref(), &bytes),
        chain: prev_pak
            .map(|p| crypto::hmac(pak_mac_key(p, account_id, dir.pak_epoch.saturating_sub(1)).as_ref(), &bytes)),
        escrow_mac: rr.map(|rr| crypto::hmac(escrow_mac_key(rr, account_id, labels::ESCROW_MAC_PAK).as_ref(), &bytes)),
        bytes,
    })
}

// ---------------------------------------------------------------- acceptance (PROTOCOL §4.4)

/// The accepted `ak_dir` head of a device (AK-level high-water marks, kept in `sync.json`).
pub struct AkHead {
    pub account_id: AccountId,
    pub epoch: u32,
    pub ak: Key,
    pub seq: u64,
    pub hash: [u8; 32],
    pub min_record_epoch: u32,
}

/// The accepted `pak_dir` head and the PAK-level pins (kept inside the vault).
pub struct PakHead {
    pub account_id: AccountId,
    pub pak_epoch: u32,
    pub pak: Key,
    pub seq: u64,
    pub hash: [u8; 32],
    pub esc_pk: [u8; 32],
    pub rr_gen: u32,
}

/// Whose seal to open on an epoch change: a device's own key, or the escrow key (restorer).
pub enum Opener<'a> {
    Device { id: &'a str, sk: &'a [u8; 32] },
    Escrow { sk: &'a [u8; 32] },
}

impl Opener<'_> {
    fn recipient(&self) -> &str {
        match self {
            Opener::Device { id, .. } => id,
            Opener::Escrow { .. } => "escrow",
        }
    }
    fn sk(&self) -> &[u8; 32] {
        match self {
            Opener::Device { sk, .. } | Opener::Escrow { sk } => sk,
        }
    }
}

pub enum Outcome<H> {
    Accepted(H),
    /// The same bytes as the head: nothing to do.
    Unchanged,
    /// This device is no longer listed, or is revoked: wipe the sync keys and state.
    SelfRevoked,
}

/// The ordering checks shared by both directories: `Unchanged`, `Stale`, `Fork` or `Gap`.
fn check_order(
    seq: u64,
    prev_hash: &str,
    bytes_hash: [u8; 32],
    head_seq: u64,
    head_hash: [u8; 32],
) -> Result<Option<()>, DirError> {
    if seq == head_seq {
        return if bytes_hash == head_hash { Ok(None) } else { Err(DirError::Fork) };
    }
    if seq < head_seq {
        return Err(DirError::Stale);
    }
    if seq != head_seq + 1 {
        return Err(DirError::Gap);
    }
    if decode32(prev_hash, "prev_hash")? != head_hash {
        return Err(DirError::Fork);
    }
    Ok(Some(()))
}

pub fn parse_ak(bytes: &[u8]) -> Result<AkDir, DirError> {
    let d: AkDir = serde_json::from_slice(bytes).map_err(|_| DirError::Malformed("ak_dir"))?;
    if d.v != labels::VERSION {
        return Err(DirError::Malformed("ak_dir version"));
    }
    Ok(d)
}

pub fn parse_pak(bytes: &[u8]) -> Result<PakDir, DirError> {
    let d: PakDir = serde_json::from_slice(bytes).map_err(|_| DirError::Malformed("pak_dir"))?;
    if d.v != labels::VERSION {
        return Err(DirError::Malformed("pak_dir version"));
    }
    Ok(d)
}

/// Rule 2–4 for `ak_dir`. The MACs are checked before any other field is acted on.
pub fn accept_ak(head: &AkHead, blob: &Blob, me: &Opener) -> Result<Outcome<AkHead>, DirError> {
    let dir = parse_ak(&blob.bytes)?;
    if decode_account(&dir.account_id)? != head.account_id {
        return Err(DirError::WrongAccount);
    }
    let h = hash(&blob.bytes);
    if check_order(dir.seq, &dir.prev_hash, h, head.seq, head.hash)?.is_none() {
        return Ok(Outcome::Unchanged);
    }
    let acc = &head.account_id;
    let ak = if dir.epoch == head.epoch {
        crypto::hmac_verify(ak_mac_key(&head.ak, acc, dir.epoch).as_ref(), &blob.bytes, &blob.mac)?;
        Zeroizing::new(*head.ak)
    } else if dir.epoch == head.epoch + 1 {
        let chain = blob.chain.ok_or(DirError::BadTag)?;
        crypto::hmac_verify(ak_mac_key(&head.ak, acc, head.epoch).as_ref(), &blob.bytes, &chain)?;
        let info = crypto::seal_info(SealKind::Ak, acc, dir.epoch, me.recipient())?;
        let new_ak = open_seal(&dir.seals, me.recipient(), me.sk(), &info)?;
        // Key confirmation: the new key must also produce this directory's mac.
        crypto::hmac_verify(ak_mac_key(&new_ak, acc, dir.epoch).as_ref(), &blob.bytes, &blob.mac)?;
        new_ak
    } else if dir.epoch < head.epoch {
        return Err(DirError::Stale);
    } else {
        return Err(DirError::EpochSkip);
    };
    if dir.min_record_epoch < head.min_record_epoch || dir.min_record_epoch > dir.epoch {
        return Err(DirError::BadMinEpoch);
    }
    if let Opener::Device { id, .. } = me {
        if !dir.devices.iter().any(|d| d.id == *id) || dir.revoked.iter().any(|r| r == id) {
            return Ok(Outcome::SelfRevoked);
        }
    }
    Ok(Outcome::Accepted(AkHead {
        account_id: head.account_id,
        epoch: dir.epoch,
        ak,
        seq: dir.seq,
        hash: h,
        min_record_epoch: dir.min_record_epoch,
    }))
}

fn check_pp_kdf(k: &PpKdf) -> Result<(), DirError> {
    if k.alg != "argon2id" || unb64(&k.salt)?.len() != 16 {
        return Err(DirError::Malformed("pp_kdf"));
    }
    crypto::check_floor(&KdfParams { m_kib: k.m_kib, t: k.t, p: k.p }).map_err(|_| DirError::WeakKdf)
}

/// Rule 5 for `pak_dir`, with the `esc_pk`/`rr_gen` pin and the KDF floor.
pub fn accept_pak(head: &PakHead, blob: &Blob, me: &Opener) -> Result<Outcome<PakHead>, DirError> {
    let dir = parse_pak(&blob.bytes)?;
    if decode_account(&dir.account_id)? != head.account_id {
        return Err(DirError::WrongAccount);
    }
    let h = hash(&blob.bytes);
    if check_order(dir.seq, &dir.prev_hash, h, head.seq, head.hash)?.is_none() {
        return Ok(Outcome::Unchanged);
    }
    let acc = &head.account_id;
    let pak = if dir.pak_epoch == head.pak_epoch {
        crypto::hmac_verify(pak_mac_key(&head.pak, acc, dir.pak_epoch).as_ref(), &blob.bytes, &blob.mac)?;
        Zeroizing::new(*head.pak)
    } else if dir.pak_epoch == head.pak_epoch + 1 {
        let chain = blob.chain.ok_or(DirError::BadTag)?;
        crypto::hmac_verify(pak_mac_key(&head.pak, acc, head.pak_epoch).as_ref(), &blob.bytes, &chain)?;
        let info = crypto::seal_info(SealKind::Pak, acc, dir.pak_epoch, me.recipient())?;
        let new_pak = open_seal(&dir.seals, me.recipient(), me.sk(), &info)?;
        crypto::hmac_verify(pak_mac_key(&new_pak, acc, dir.pak_epoch).as_ref(), &blob.bytes, &blob.mac)?;
        new_pak
    } else if dir.pak_epoch < head.pak_epoch {
        return Err(DirError::Stale);
    } else {
        return Err(DirError::EpochSkip);
    };
    check_pp_kdf(&dir.pp_kdf)?;
    let esc_pk = decode32(&dir.esc_pk, "esc_pk")?;
    if dir.rr_gen < head.rr_gen || (esc_pk != head.esc_pk && dir.rr_gen == head.rr_gen) {
        return Err(DirError::EscrowKeyChanged);
    }
    if let Opener::Device { id, .. } = me {
        if !dir.vault_keys.iter().any(|v| v.device == *id) {
            return Ok(Outcome::SelfRevoked);
        }
    }
    Ok(Outcome::Accepted(PakHead {
        account_id: head.account_id,
        pak_epoch: dir.pak_epoch,
        pak,
        seq: dir.seq,
        hash: h,
        esc_pk,
        rr_gen: dir.rr_gen,
    }))
}

/// A restorer's starting point (PROTOCOL §5.4, §6.3): an anchor pair carrying `escrow_mac`.
/// Checks both escrow tags, that `esc_pk` is the one derived from RR, the KDF floor, and
/// unseals AK and PAK with the escrow key (with key confirmation by `mac`).
pub fn open_anchor(
    ak_blob: &Blob,
    pak_blob: &Blob,
    rr: &[u8; 32],
    account_id: &AccountId,
) -> Result<(AkHead, PakHead), DirError> {
    let ak_tag = ak_blob.escrow_mac.ok_or(DirError::BadTag)?;
    let pak_tag = pak_blob.escrow_mac.ok_or(DirError::BadTag)?;
    crypto::hmac_verify(escrow_mac_key(rr, account_id, labels::ESCROW_MAC_AK).as_ref(), &ak_blob.bytes, &ak_tag)?;
    crypto::hmac_verify(escrow_mac_key(rr, account_id, labels::ESCROW_MAC_PAK).as_ref(), &pak_blob.bytes, &pak_tag)?;
    let ak_dir = parse_ak(&ak_blob.bytes)?;
    let pak_dir = parse_pak(&pak_blob.bytes)?;
    if decode_account(&ak_dir.account_id)? != *account_id || decode_account(&pak_dir.account_id)? != *account_id {
        return Err(DirError::WrongAccount);
    }
    check_pp_kdf(&pak_dir.pp_kdf)?;
    let esc = crypto::escrow_keypair(rr, account_id);
    let esc_pk = decode32(&pak_dir.esc_pk, "esc_pk")?;
    if esc_pk != esc.pk {
        return Err(DirError::EscrowKeyMismatch);
    }
    let ak = open_seal(
        &ak_dir.seals,
        "escrow",
        &esc.sk,
        &crypto::seal_info(SealKind::Ak, account_id, ak_dir.epoch, "escrow")?,
    )?;
    crypto::hmac_verify(ak_mac_key(&ak, account_id, ak_dir.epoch).as_ref(), &ak_blob.bytes, &ak_blob.mac)?;
    let pak = open_seal(
        &pak_dir.seals,
        "escrow",
        &esc.sk,
        &crypto::seal_info(SealKind::Pak, account_id, pak_dir.pak_epoch, "escrow")?,
    )?;
    crypto::hmac_verify(pak_mac_key(&pak, account_id, pak_dir.pak_epoch).as_ref(), &pak_blob.bytes, &pak_blob.mac)?;
    Ok((
        AkHead {
            account_id: *account_id,
            epoch: ak_dir.epoch,
            ak,
            seq: ak_dir.seq,
            hash: hash(&ak_blob.bytes),
            min_record_epoch: ak_dir.min_record_epoch,
        },
        PakHead {
            account_id: *account_id,
            pak_epoch: pak_dir.pak_epoch,
            pak,
            seq: pak_dir.seq,
            hash: hash(&pak_blob.bytes),
            esc_pk,
            rr_gen: pak_dir.rr_gen,
        },
    ))
}

/// Rule 7: whom to seal a new key to. Devices in `ak_dir` that aren't revoked **and** that the
/// server lists as active; a device the server doesn't know is never sealed to [R3].
pub fn seal_targets(dir: &AkDir, server_active: &[&str]) -> Result<Vec<(String, [u8; 32])>, DirError> {
    let mut out = Vec::new();
    for d in &dir.devices {
        if dir.revoked.contains(&d.id) || !server_active.contains(&d.id.as_str()) {
            continue;
        }
        let pk = decode32(&d.pk, "device pk")?;
        crypto::check_public_key(&pk).map_err(|_| DirError::Malformed("device pk"))?;
        out.push((d.id.clone(), pk));
    }
    Ok(out)
}

/// The same rule for PAK: vault keys whose device is active in the accepted `ak_dir`.
pub fn vault_seal_targets(
    pak_dir: &PakDir,
    ak_dir: &AkDir,
    server_active: &[&str],
) -> Result<Vec<(String, [u8; 32])>, DirError> {
    let active: Vec<String> = seal_targets(ak_dir, server_active)?.into_iter().map(|(id, _)| id).collect();
    let mut out = Vec::new();
    for v in &pak_dir.vault_keys {
        if active.contains(&v.device) {
            let pk = decode32(&v.pk, "vault pk")?;
            crypto::check_public_key(&pk).map_err(|_| DirError::Malformed("vault pk"))?;
            out.push((v.device.clone(), pk));
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sync::crypto::{escrow_keypair, KeyPair, PASSPHRASE_KDF};

    pub const ACC: AccountId = [0x42; 16];

    /// A small account: devices A and B, escrow from RR, epoch 1, seq 1.
    pub struct World {
        pub rr: [u8; 32],
        pub ak: [u8; 32],
        pub pak: [u8; 32],
        pub a: KeyPair,
        pub b: KeyPair,
        pub va: KeyPair,
        pub vb: KeyPair,
        pub ak_dir: AkDir,
        pub ak_blob: Blob,
        pub pak_dir: PakDir,
        pub pak_blob: Blob,
    }

    pub fn device(id: &str, short: u16, pk: &[u8; 32]) -> DeviceEntry {
        DeviceEntry { id: id.into(), short, pk: b64(pk), added_by: "A".into(), added_seq: 1 }
    }

    pub fn world() -> World {
        let (rr, ak, pak) = ([1u8; 32], [2u8; 32], [3u8; 32]);
        let (a, b, va, vb) = (
            KeyPair::generate().unwrap(),
            KeyPair::generate().unwrap(),
            KeyPair::generate().unwrap(),
            KeyPair::generate().unwrap(),
        );
        let esc = escrow_keypair(&rr, &ACC);
        let ak_dir = AkDir {
            v: 1,
            account_id: b64(&ACC),
            epoch: 1,
            seq: 1,
            prev_hash: String::new(),
            devices: vec![device("A", 1, &a.pk), device("B", 2, &b.pk)],
            revoked: vec![],
            min_record_epoch: 1,
            seals: vec![
                seal_key(SealKind::Ak, &ak, &ACC, 1, "A", &a.pk).unwrap(),
                seal_key(SealKind::Ak, &ak, &ACC, 1, "B", &b.pk).unwrap(),
                seal_key(SealKind::Ak, &ak, &ACC, 1, "escrow", &esc.pk).unwrap(),
            ],
            hist: None,
            created_ms: 1,
            created_by: "A".into(),
        };
        let ak_blob = sign_ak(&ak_dir, &ak, None, Some(&rr), &ACC).unwrap();
        let pak_dir = PakDir {
            v: 1,
            account_id: b64(&ACC),
            pak_epoch: 1,
            seq: 1,
            prev_hash: String::new(),
            pp_kdf: PpKdf { alg: "argon2id".into(), m_kib: PASSPHRASE_KDF.m_kib, t: 3, p: 1, salt: b64(&[9u8; 16]) },
            esc_pk: b64(&esc.pk),
            rr_gen: 1,
            vault_keys: vec![
                VaultKey { device: "A".into(), pk: b64(&va.pk) },
                VaultKey { device: "B".into(), pk: b64(&vb.pk) },
            ],
            seals: vec![
                seal_key(SealKind::Pak, &pak, &ACC, 1, "A", &va.pk).unwrap(),
                seal_key(SealKind::Pak, &pak, &ACC, 1, "B", &vb.pk).unwrap(),
                seal_key(SealKind::Pak, &pak, &ACC, 1, "escrow", &esc.pk).unwrap(),
            ],
            hist: None,
            created_ms: 1,
            created_by: "A".into(),
        };
        let pak_blob = sign_pak(&pak_dir, &pak, None, Some(&rr), &ACC).unwrap();
        World { rr, ak, pak, a, b, va, vb, ak_dir, ak_blob, pak_dir, pak_blob }
    }

    fn ak_head(w: &World) -> AkHead {
        AkHead {
            account_id: ACC,
            epoch: 1,
            ak: Zeroizing::new(w.ak),
            seq: 1,
            hash: hash(&w.ak_blob.bytes),
            min_record_epoch: 1,
        }
    }

    fn pak_head(w: &World) -> PakHead {
        PakHead {
            account_id: ACC,
            pak_epoch: 1,
            pak: Zeroizing::new(w.pak),
            seq: 1,
            hash: hash(&w.pak_blob.bytes),
            esc_pk: escrow_keypair(&w.rr, &ACC).pk,
            rr_gen: 1,
        }
    }

    fn next_ak(w: &World, f: impl FnOnce(&mut AkDir)) -> AkDir {
        let mut d = w.ak_dir.clone();
        d.seq = 2;
        d.prev_hash = b64(&hash(&w.ak_blob.bytes));
        f(&mut d);
        d
    }

    fn accepted<H>(o: Result<Outcome<H>, DirError>) -> H {
        match o {
            Ok(Outcome::Accepted(h)) => h,
            Ok(Outcome::Unchanged) => panic!("unchanged"),
            Ok(Outcome::SelfRevoked) => panic!("self-revoked"),
            Err(e) => panic!("rejected: {e:?}"),
        }
    }

    #[test]
    fn same_epoch_update_is_accepted() {
        let w = world();
        let c = KeyPair::generate().unwrap();
        let d = next_ak(&w, |d| d.devices.push(device("C", 3, &c.pk)));
        let blob = sign_ak(&d, &w.ak, None, None, &ACC).unwrap();
        let h = accepted(accept_ak(&ak_head(&w), &blob, &Opener::Device { id: "A", sk: &w.a.sk }));
        assert_eq!((h.seq, h.epoch), (2, 1));
        // The same bytes again: unchanged.
        let head2 = AkHead { ak: Zeroizing::new(w.ak), ..h };
        assert!(matches!(accept_ak(&head2, &blob, &Opener::Device { id: "A", sk: &w.a.sk }), Ok(Outcome::Unchanged)));
    }

    #[test]
    fn a_server_cannot_inject_or_alter() {
        let w = world();
        let head = ak_head(&w);
        let me = Opener::Device { id: "A", sk: &w.a.sk };
        // The server adds its own device and recomputes nothing it can't: the mac fails.
        let evil = KeyPair::generate().unwrap();
        let d = next_ak(&w, |d| d.devices.push(device("EVIL", 9, &evil.pk)));
        let forged = sign_ak(&d, &[0xee; 32], None, None, &ACC).unwrap();
        assert_eq!(accept_ak(&head, &forged, &me).map(|_| ()), Err(DirError::BadTag));
        // Flip a byte of a correctly signed blob.
        let good = sign_ak(&next_ak(&w, |_| {}), &w.ak, None, None, &ACC).unwrap();
        let mut bad = good.clone();
        let i = bad.bytes.len() / 2;
        bad.bytes[i] ^= 1;
        assert!(accept_ak(&head, &bad, &me).is_err());
        // A forged epoch change: chain under a key the server doesn't have.
        let d = next_ak(&w, |d| d.epoch = 2);
        let forged = sign_ak(&d, &[0xee; 32], Some(&[0xdd; 32]), None, &ACC).unwrap();
        assert_eq!(accept_ak(&head, &forged, &me).map(|_| ()), Err(DirError::BadTag));
    }

    #[test]
    fn ordering_rules() {
        let w = world();
        let head = ak_head(&w);
        let me = Opener::Device { id: "A", sk: &w.a.sk };
        // Different bytes at the accepted seq: fork.
        let mut d = w.ak_dir.clone();
        d.created_ms = 2;
        let fork = sign_ak(&d, &w.ak, None, None, &ACC).unwrap();
        assert_eq!(accept_ak(&head, &fork, &me).map(|_| ()), Err(DirError::Fork));
        // A gap.
        let d = next_ak(&w, |d| d.seq = 3);
        assert_eq!(
            accept_ak(&head, &sign_ak(&d, &w.ak, None, None, &ACC).unwrap(), &me).map(|_| ()),
            Err(DirError::Gap)
        );
        // prev_hash not the head: fork.
        let d = next_ak(&w, |d| d.prev_hash = b64(&[0u8; 32]));
        assert_eq!(
            accept_ak(&head, &sign_ak(&d, &w.ak, None, None, &ACC).unwrap(), &me).map(|_| ()),
            Err(DirError::Fork)
        );
        // Older seq: stale.
        let newer = AkHead { seq: 5, ..ak_head(&w) };
        assert_eq!(accept_ak(&newer, &w.ak_blob, &me).map(|_| ()), Err(DirError::Stale));
        // Another account.
        let other = AkHead { account_id: [1; 16], ..ak_head(&w) };
        assert_eq!(accept_ak(&other, &w.ak_blob, &me).map(|_| ()), Err(DirError::WrongAccount));
    }

    #[test]
    fn rotation_excludes_the_revoked_device() {
        let w = world();
        let new_ak = [7u8; 32];
        let esc = escrow_keypair(&w.rr, &ACC);
        let (hn, hc) = hist_seal(SealKind::Ak, &new_ak, &w.ak, &ACC, 2).unwrap();
        let d = next_ak(&w, |d| {
            d.epoch = 2;
            d.revoked = vec!["B".into()];
            d.devices.retain(|x| x.id != "B");
            d.seals = vec![
                seal_key(SealKind::Ak, &new_ak, &ACC, 2, "A", &w.a.pk).unwrap(),
                seal_key(SealKind::Ak, &new_ak, &ACC, 2, "escrow", &esc.pk).unwrap(),
            ];
            d.hist = Some(AkHist { epoch: 1, n: hn.clone(), ct: hc.clone() });
        });
        let blob = sign_ak(&d, &new_ak, Some(&w.ak), None, &ACC).unwrap();
        // A (remaining) unseals the new key and accepts.
        let h = accepted(accept_ak(&ak_head(&w), &blob, &Opener::Device { id: "A", sk: &w.a.sk }));
        assert_eq!((h.epoch, *h.ak), (2, new_ak));
        // B (revoked) has no seal: it can't learn AK_2.
        assert_eq!(
            accept_ak(&ak_head(&w), &blob, &Opener::Device { id: "B", sk: &w.b.sk }).map(|_| ()),
            Err(DirError::NotForUs)
        );
        // The history wrap gives AK_1 to holders of AK_2 only.
        assert_eq!(*hist_open(SealKind::Ak, &new_ak, &ACC, 2, &hn, &hc).unwrap(), w.ak);
        assert!(hist_open(SealKind::Ak, &w.ak, &ACC, 2, &hn, &hc).is_err());
        // Without the chain tag, or skipping an epoch: refused.
        let mut no_chain = blob.clone();
        no_chain.chain = None;
        assert_eq!(
            accept_ak(&ak_head(&w), &no_chain, &Opener::Device { id: "A", sk: &w.a.sk }).map(|_| ()),
            Err(DirError::BadTag)
        );
        let d3 = next_ak(&w, |d| d.epoch = 3);
        let b3 = sign_ak(&d3, &new_ak, Some(&w.ak), None, &ACC).unwrap();
        assert_eq!(
            accept_ak(&ak_head(&w), &b3, &Opener::Device { id: "A", sk: &w.a.sk }).map(|_| ()),
            Err(DirError::EpochSkip)
        );
    }

    #[test]
    fn a_listed_revocation_wipes_the_device() {
        let w = world();
        let d = next_ak(&w, |d| d.revoked = vec!["B".into()]);
        let blob = sign_ak(&d, &w.ak, None, None, &ACC).unwrap();
        assert!(matches!(
            accept_ak(&ak_head(&w), &blob, &Opener::Device { id: "B", sk: &w.b.sk }),
            Ok(Outcome::SelfRevoked)
        ));
    }

    #[test]
    fn min_record_epoch_only_moves_forward() {
        let w = world();
        let d = next_ak(&w, |d| d.min_record_epoch = 0);
        let blob = sign_ak(&d, &w.ak, None, None, &ACC).unwrap();
        assert_eq!(
            accept_ak(&ak_head(&w), &blob, &Opener::Device { id: "A", sk: &w.a.sk }).map(|_| ()),
            Err(DirError::BadMinEpoch)
        );
    }

    fn next_pak(w: &World, f: impl FnOnce(&mut PakDir)) -> PakDir {
        let mut d = w.pak_dir.clone();
        d.seq = 2;
        d.prev_hash = b64(&hash(&w.pak_blob.bytes));
        f(&mut d);
        d
    }

    #[test]
    fn ak_only_malware_cannot_touch_the_pak_dir() {
        let w = world();
        let me = Opener::Device { id: "A", sk: &w.va.sk };
        // Malware with AK (not PAK) adds its own vault key and swaps esc_pk: it can only sign
        // with keys it has, so the PAK mac fails.
        let evil = KeyPair::generate().unwrap();
        let d = next_pak(&w, |d| {
            d.vault_keys.push(VaultKey { device: "EVIL".into(), pk: b64(&evil.pk) });
            d.esc_pk = b64(&evil.pk);
        });
        let forged = sign_pak(&d, &w.ak, None, None, &ACC).unwrap();
        assert_eq!(accept_pak(&pak_head(&w), &forged, &me).map(|_| ()), Err(DirError::BadTag));
    }

    #[test]
    fn escrow_key_and_kdf_pins() {
        let w = world();
        let me = Opener::Device { id: "A", sk: &w.va.sk };
        // Even a PAK holder can't change esc_pk without raising rr_gen.
        let other = KeyPair::generate().unwrap();
        let d = next_pak(&w, |d| d.esc_pk = b64(&other.pk));
        assert_eq!(
            accept_pak(&pak_head(&w), &sign_pak(&d, &w.pak, None, None, &ACC).unwrap(), &me).map(|_| ()),
            Err(DirError::EscrowKeyChanged)
        );
        // With an rr_gen increase (an RR rotation) it is accepted and re-pinned.
        let d = next_pak(&w, |d| {
            d.esc_pk = b64(&other.pk);
            d.rr_gen = 2;
        });
        let h = accepted(accept_pak(&pak_head(&w), &sign_pak(&d, &w.pak, None, None, &ACC).unwrap(), &me));
        assert_eq!((h.esc_pk, h.rr_gen), (other.pk, 2));
        // A lowered KDF is refused.
        let d = next_pak(&w, |d| d.pp_kdf.m_kib = 65536);
        assert_eq!(
            accept_pak(&pak_head(&w), &sign_pak(&d, &w.pak, None, None, &ACC).unwrap(), &me).map(|_| ()),
            Err(DirError::WeakKdf)
        );
    }

    #[test]
    fn restore_from_the_escrow_anchor() {
        let w = world();
        let (ak, pak) = open_anchor(&w.ak_blob, &w.pak_blob, &w.rr, &ACC).unwrap();
        assert_eq!((*ak.ak, *pak.pak), (w.ak, w.pak));
        assert_eq!(pak.esc_pk, escrow_keypair(&w.rr, &ACC).pk);
        // The wrong RR (wrong passphrase) fails on the escrow mac.
        assert_eq!(open_anchor(&w.ak_blob, &w.pak_blob, &[9u8; 32], &ACC).map(|_| ()), Err(DirError::BadTag));
        // No anchor tag: refused.
        let mut plain = w.ak_blob.clone();
        plain.escrow_mac = None;
        assert_eq!(open_anchor(&plain, &w.pak_blob, &w.rr, &ACC).map(|_| ()), Err(DirError::BadTag));
        // The restorer walks forward with its escrow key, without being listed as a device.
        let d = next_ak(&w, |_| {});
        let blob = sign_ak(&d, &w.ak, None, None, &ACC).unwrap();
        let esc = escrow_keypair(&w.rr, &ACC);
        assert!(matches!(accept_ak(&ak, &blob, &Opener::Escrow { sk: &esc.sk }), Ok(Outcome::Accepted(_))));
    }

    #[test]
    fn each_device_opens_only_its_own_seals() {
        let w = world();
        let open = |to: &str, sk: &[u8; 32]| {
            open_seal(&w.pak_dir.seals, to, sk, &crypto::seal_info(SealKind::Pak, &ACC, 1, to).unwrap()).map(|k| *k)
        };
        assert_eq!(open("A", &w.va.sk).unwrap(), w.pak);
        assert_eq!(open("B", &w.vb.sk).unwrap(), w.pak);
        // B's vault key can't open A's seal, and a general key can't open a vault seal.
        assert_eq!(open("A", &w.vb.sk), Err(DirError::BadTag));
        assert_eq!(open("B", &w.b.sk), Err(DirError::BadTag));
    }

    #[test]
    fn seal_targets_need_the_server_too() {
        let w = world();
        let mut d = w.ak_dir.clone();
        d.revoked = vec!["B".into()];
        let ids: Vec<String> = seal_targets(&d, &["A", "B"]).unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["A"], "revoked B is never a target");
        let ids: Vec<String> = seal_targets(&w.ak_dir, &["B"]).unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["B"], "a phantom device unknown to the server is never a target");
        let vids: Vec<String> =
            vault_seal_targets(&w.pak_dir, &d, &["A", "B"]).unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(vids, ["A"]);
    }
}
