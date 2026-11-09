//! Key orchestration (PROTOCOL §5, §6.2–§6.4, §4.4): the pure steps of setting up sync on the
//! first device, restoring with the passphrase or the recovery key, approving a new device by
//! SAS, and following the directories. No I/O: `client` drives these over the API.
//!
//! - **Passphrase:** 7 words from the BIP-39 English list (2048 words, 77 bits), joined by `-`.
//!   PROTOCOL §5.1 names 6 words of the EFF large list (77.5 bits); 0.9.0 uses the list already
//!   shipped for the vault's recovery phrase, with one more word for the same strength.
//! - **What lives where:** AK-level state in `sync.json` (`state::SyncState`); PAK-level state
//!   (the vault key pair, PAK, the PAK pins) in the vault's sealed sync part ([`PakState`]).

use super::codes;
use super::crypto::{self, KdfParams, Key, KeyPair, SealKind, WrapKind, PASSPHRASE_KDF};
use super::dir::{self, AkDir, AkHead, Blob, DeviceEntry, DirError, Opener, Outcome, PakDir, PakHead, PpKdf, VaultKey};
use super::envelope::{b64, unb64};
use super::sas::{self, Direction};
use super::state::{AkPin, EpochKey};
use super::{AccountId, SyncError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Why a key step failed. `Dir` errors of kind `Fork`/`BadTag` are security alarms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    Dir(DirError),
    Crypto(SyncError),
    /// The passphrase or recovery key doesn't open the escrow.
    WrongSecret,
    /// The typed SAS didn't match (the approval is cancelled).
    SasMismatch,
    /// The other side broke the protocol (a commitment or tag that doesn't verify, keys that
    /// don't match the directories).
    Protocol(&'static str),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Dir(e) => write!(f, "directory: {e:?}"),
            KeyError::Crypto(e) => write!(f, "{e}"),
            KeyError::WrongSecret => f.write_str("the passphrase or recovery key is wrong"),
            KeyError::SasMismatch => f.write_str("the code doesn't match"),
            KeyError::Protocol(w) => write!(f, "protocol: {w}"),
        }
    }
}

impl std::error::Error for KeyError {}

impl From<DirError> for KeyError {
    fn from(e: DirError) -> KeyError {
        KeyError::Dir(e)
    }
}

impl From<SyncError> for KeyError {
    fn from(e: SyncError) -> KeyError {
        KeyError::Crypto(e)
    }
}

impl KeyError {
    /// A security alarm: writes stop and the user is told (PLAN §5.6).
    pub fn is_alarm(&self) -> bool {
        matches!(
            self,
            KeyError::Dir(DirError::Fork | DirError::BadTag | DirError::EscrowKeyChanged | DirError::BadMinEpoch)
                | KeyError::Protocol(_)
        )
    }
}

pub type KResult<T> = Result<T, KeyError>;

// ---- What this device is ---------------------------------------------------------------------

/// This device as the server named it.
#[derive(Debug, Clone)]
pub struct Me {
    pub device_id: String,
    pub short: u16,
}

// ---- PAK-level state (inside the vault) ------------------------------------------------------

/// The accepted `pak_dir` head and the PAK pins (PROTOCOL §4.4 rule 6).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PakPin {
    pub pak_epoch: u32,
    #[serde(with = "b64_key")]
    pub pak: Key,
    pub seq: u64,
    #[serde(with = "b64_arr")]
    pub hash: [u8; 32],
    #[serde(with = "b64_arr")]
    pub esc_pk: [u8; 32],
    pub rr_gen: u32,
}

impl PakPin {
    pub fn head(&self, account_id: AccountId) -> PakHead {
        PakHead {
            account_id,
            pak_epoch: self.pak_epoch,
            pak: self.pak.clone(),
            seq: self.seq,
            hash: self.hash,
            esc_pk: self.esc_pk,
            rr_gen: self.rr_gen,
        }
    }

    pub fn from_head(h: &PakHead) -> PakPin {
        PakPin {
            pak_epoch: h.pak_epoch,
            pak: h.pak.clone(),
            seq: h.seq,
            hash: h.hash,
            esc_pk: h.esc_pk,
            rr_gen: h.rr_gen,
        }
    }
}

/// `vault::VaultSync.state`: what only an unlocked vault holds.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakState {
    /// The vault X25519 private key (its public key is in `pak_dir.vault_keys`).
    #[serde(with = "b64_key")]
    pub vault_sk: Key,
    pub pak: PakPin,
    #[serde(default)]
    pub pak_old: Vec<EpochKey>,
    /// The PAK collections' bookkeeping (S8): kept here, under the vault key, never in browser.db.
    #[serde(default)]
    pub book: super::pak::PakBook,
}

impl std::fmt::Debug for PakState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PakState")
            .field("pak_epoch", &self.pak.pak_epoch)
            .field("seq", &self.pak.seq)
            .finish_non_exhaustive()
    }
}

impl PakState {
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        #[allow(clippy::expect_used)] // plain fields
        Zeroizing::new(serde_json::to_vec(self).expect("PakState serializes"))
    }

    pub fn from_bytes(b: &[u8]) -> KResult<PakState> {
        serde_json::from_slice(b).map_err(|_| KeyError::Protocol("vault sync state"))
    }
}

// ---- Secrets shown to the user ----------------------------------------------------------------

pub const PASSPHRASE_WORDS: usize = 7;

/// A new sync passphrase: 7 random words of the BIP-39 English list joined by `-`.
pub fn new_passphrase() -> KResult<Zeroizing<String>> {
    let list = bip39::Language::English.word_list();
    let mut words = Vec::with_capacity(PASSPHRASE_WORDS);
    while words.len() < PASSPHRASE_WORDS {
        // 2048 words: 11 bits from two random bytes, without bias.
        let b = crypto::random::<2>()?;
        let i = (usize::from(b[0]) << 3) | (usize::from(b[1]) >> 5);
        words.push(list[i]);
    }
    Ok(Zeroizing::new(words.join("-")))
}

/// A typed passphrase as the KDF sees it: trimmed, lowercase, words joined by single `-`
/// (spaces and dashes are the same).
pub fn normalize_passphrase(typed: &str) -> Zeroizing<String> {
    let lower = Zeroizing::new(typed.trim().to_lowercase());
    Zeroizing::new(
        lower.split(|c: char| c == '-' || c.is_whitespace()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-"),
    )
}

/// The last 4-symbol data group of a formatted recovery key, which the user types back.
pub fn recovery_key_check_group(formatted: &str) -> String {
    let groups: Vec<&str> = formatted.split('-').collect();
    groups.get(groups.len().saturating_sub(2)).copied().unwrap_or("").to_string()
}

/// Whether the typed group is the key's last data group (case-insensitive, `O`/`I`/`L` forgiven).
pub fn recovery_key_confirmed(formatted: &str, typed: &str) -> bool {
    let want = recovery_key_check_group(formatted);
    let norm = |s: &str| {
        s.trim()
            .to_ascii_uppercase()
            .chars()
            .map(|c| match c {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            })
            .collect::<String>()
    };
    !want.is_empty() && norm(typed) == norm(&want)
}

// ---- Escrow on the wire -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RrWrapWire {
    pub n: String,
    pub ct: String,
    pub commit: String,
}

impl RrWrapWire {
    fn from(w: &crypto::RrWrap) -> RrWrapWire {
        RrWrapWire { n: b64(&w.n), ct: b64(&w.ct), commit: b64(&w.commit) }
    }

    fn to_wrap(&self) -> KResult<crypto::RrWrap> {
        Ok(crypto::RrWrap {
            n: unb64(&self.n)?.try_into().map_err(|_| KeyError::Protocol("rr_wrap"))?,
            ct: unb64(&self.ct)?,
            commit: unb64(&self.commit)?.try_into().map_err(|_| KeyError::Protocol("rr_wrap"))?,
        })
    }
}

/// `keys/escrow` and the escrow part of `keys/commit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscrowWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub rr_gen: u32,
    pub rr_wrap_pp: RrWrapWire,
    pub rr_wrap_rk: RrWrapWire,
    pub pp_kdf: PpKdf,
}

/// The wraps of a new recovery root under the passphrase and the recovery key.
fn escrow_for(
    rr: &[u8; 32],
    passphrase: &str,
    rk: &[u8; codes::RECOVERY_KEY_BYTES],
    salt: &[u8; 16],
    rr_gen: u32,
    account_id: &AccountId,
) -> KResult<EscrowWire> {
    let kek_pp = crypto::passphrase_kek(passphrase.as_bytes(), salt, &PASSPHRASE_KDF)?;
    let kek_rk = crypto::recovery_kek(rk, account_id);
    Ok(EscrowWire {
        account_id: None,
        rr_gen,
        rr_wrap_pp: RrWrapWire::from(&crypto::rr_wrap(&kek_pp, rr, WrapKind::Passphrase, account_id, rr_gen)?),
        rr_wrap_rk: RrWrapWire::from(&crypto::rr_wrap(&kek_rk, rr, WrapKind::RecoveryKey, account_id, rr_gen)?),
        pp_kdf: pp_kdf(salt),
    })
}

fn pp_kdf(salt: &[u8; 16]) -> PpKdf {
    PpKdf {
        alg: "argon2id".into(),
        m_kib: PASSPHRASE_KDF.m_kib,
        t: PASSPHRASE_KDF.t,
        p: PASSPHRASE_KDF.p,
        salt: b64(salt),
    }
}

/// What the user typed to restore.
pub enum Secret {
    Passphrase(Zeroizing<String>),
    RecoveryKey(Zeroizing<String>),
}

/// The recovery root from the escrow: the KDF parameters must meet the floor, the commitment is
/// checked before decrypting. A wrong secret is `WrongSecret`.
pub fn unwrap_rr(escrow: &EscrowWire, secret: &Secret, account_id: &AccountId) -> KResult<Key> {
    let r = match secret {
        Secret::Passphrase(p) => {
            let k = &escrow.pp_kdf;
            if k.alg != "argon2id" {
                return Err(KeyError::Dir(DirError::Malformed("pp_kdf")));
            }
            let params = KdfParams { m_kib: k.m_kib, t: k.t, p: k.p };
            let salt: [u8; 16] = unb64(&k.salt)?.try_into().map_err(|_| KeyError::Protocol("pp salt"))?;
            let kek = crypto::passphrase_kek(normalize_passphrase(p).as_bytes(), &salt, &params)?;
            crypto::rr_unwrap(&kek, &escrow.rr_wrap_pp.to_wrap()?, WrapKind::Passphrase, account_id, escrow.rr_gen)
        }
        Secret::RecoveryKey(text) => {
            let rk = codes::parse_recovery_key(text).map_err(|_| KeyError::WrongSecret)?;
            let kek = crypto::recovery_kek(&rk, account_id);
            crypto::rr_unwrap(&kek, &escrow.rr_wrap_rk.to_wrap()?, WrapKind::RecoveryKey, account_id, escrow.rr_gen)
        }
    };
    r.map_err(|e| match e {
        SyncError::Auth => KeyError::WrongSecret,
        e => KeyError::Crypto(e),
    })
}

// ---- A commit to send --------------------------------------------------------------------------

/// The body of one `keys/commit` (the client maps it to the wire).
#[derive(Debug, Clone, Default)]
pub struct Commit {
    pub ak_dir: Option<Blob>,
    pub pak_dir: Option<Blob>,
    pub if_absent: bool,
    pub if_ak_prev_hash: Option<[u8; 32]>,
    pub if_pak_prev_hash: Option<[u8; 32]>,
    pub escrow: Option<EscrowWire>,
}

/// What a successful key step leaves on the device: the AK side for `sync.json`, the PAK side
/// for the vault (when the vault takes part).
pub struct Joined {
    pub device_sk: Key,
    pub ak: AkPin,
    pub ak_old: Vec<EpochKey>,
    pub pak: Option<PakState>,
    /// The escrow public key from an authenticated source (this setup, RR, or the approver's
    /// box): a rotation seals the next AK to it, never to one the server names [R1].
    pub esc_pk: [u8; 32],
}

fn seal(
    kind: SealKind,
    key: &[u8; 32],
    acc: &AccountId,
    epoch: u32,
    to: &str,
    pk: &[u8; 32],
) -> KResult<dir::SealEntry> {
    Ok(dir::seal_key(kind, key, acc, epoch, to, pk)?)
}

// ---- First device (PROTOCOL §6.4) -------------------------------------------------------------

/// Everything the first device makes before the user confirms the passphrase and recovery key.
/// Nothing is sent until [`setup_commit`].
pub struct SetupPlan {
    pub account_id: AccountId,
    pub me: Me,
    pub passphrase: Zeroizing<String>,
    /// `KRNK-XXXX-…-C`, shown once.
    pub recovery_key: Zeroizing<String>,
    rk: Zeroizing<[u8; codes::RECOVERY_KEY_BYTES]>,
    rr: Key,
    ak: Key,
    pak: Key,
    device: KeyPair,
    vault: Option<KeyPair>,
    salt: [u8; 16],
}

/// `with_vault`: the password vault takes part (its key pair is made now and PAK goes into it).
pub fn plan_setup(account_id: AccountId, me: Me, with_vault: bool) -> KResult<SetupPlan> {
    let rk = Zeroizing::new(crypto::random::<{ codes::RECOVERY_KEY_BYTES }>()?);
    Ok(SetupPlan {
        account_id,
        me,
        passphrase: new_passphrase()?,
        recovery_key: Zeroizing::new(codes::format_recovery_key(&rk)),
        rk,
        rr: Zeroizing::new(crypto::random::<32>()?),
        ak: Zeroizing::new(crypto::random::<32>()?),
        pak: Zeroizing::new(crypto::random::<32>()?),
        device: KeyPair::generate()?,
        vault: if with_vault { Some(KeyPair::generate()?) } else { None },
        salt: crypto::random::<16>()?,
    })
}

/// The first commit (`if_absent`) and what the device keeps (Argon2id runs here: ~1 s).
pub fn setup_commit(plan: &SetupPlan, now_ms: u64) -> KResult<(Commit, Joined)> {
    let acc = plan.account_id;
    let me = &plan.me.device_id;
    let esc = crypto::escrow_keypair(&plan.rr, &acc);
    let ak_dir = AkDir {
        v: 1,
        account_id: b64(&acc),
        epoch: 1,
        seq: 1,
        prev_hash: String::new(),
        devices: vec![DeviceEntry {
            id: me.clone(),
            short: plan.me.short,
            pk: b64(&plan.device.pk),
            added_by: me.clone(),
            added_seq: 1,
        }],
        revoked: vec![],
        min_record_epoch: 1,
        seals: vec![
            seal(SealKind::Ak, &plan.ak, &acc, 1, me, &plan.device.pk)?,
            seal(SealKind::Ak, &plan.ak, &acc, 1, "escrow", &esc.pk)?,
        ],
        hist: None,
        created_ms: now_ms,
        created_by: me.clone(),
    };
    let mut pak_seals = vec![seal(SealKind::Pak, &plan.pak, &acc, 1, "escrow", &esc.pk)?];
    let mut vault_keys = vec![];
    if let Some(v) = &plan.vault {
        vault_keys.push(VaultKey { device: me.clone(), pk: b64(&v.pk) });
        pak_seals.insert(0, seal(SealKind::Pak, &plan.pak, &acc, 1, me, &v.pk)?);
    }
    let pak_dir = PakDir {
        v: 1,
        account_id: b64(&acc),
        pak_epoch: 1,
        seq: 1,
        prev_hash: String::new(),
        pp_kdf: pp_kdf(&plan.salt),
        esc_pk: b64(&esc.pk),
        rr_gen: 1,
        vault_keys,
        seals: pak_seals,
        hist: None,
        created_ms: now_ms,
        created_by: me.clone(),
    };
    let ak_blob = dir::sign_ak(&ak_dir, &plan.ak, None, Some(&plan.rr), &acc)?;
    let pak_blob = dir::sign_pak(&pak_dir, &plan.pak, None, Some(&plan.rr), &acc)?;
    let escrow = escrow_for(&plan.rr, &plan.passphrase, &plan.rk, &plan.salt, 1, &acc)?;
    let joined = Joined {
        device_sk: plan.device.sk.clone(),
        ak: AkPin { epoch: 1, ak: plan.ak.clone(), seq: 1, hash: dir::hash(&ak_blob.bytes), min_record_epoch: 1 },
        ak_old: vec![],
        pak: plan.vault.as_ref().map(|v| PakState {
            vault_sk: v.sk.clone(),
            pak: PakPin {
                pak_epoch: 1,
                pak: plan.pak.clone(),
                seq: 1,
                hash: dir::hash(&pak_blob.bytes),
                esc_pk: esc.pk,
                rr_gen: 1,
            },
            pak_old: vec![],
            book: Default::default(),
        }),
        esc_pk: esc.pk,
    };
    Ok((
        Commit {
            ak_dir: Some(ak_blob),
            pak_dir: Some(pak_blob),
            if_absent: true,
            escrow: Some(escrow),
            ..Default::default()
        },
        joined,
    ))
}

// ---- Walking the directories ------------------------------------------------------------------

/// Accepts the versions after the head, in order (PROTOCOL §4.4). On an epoch change the old
/// key is kept in `old` (records of that epoch may still need it until the rekey is done).
/// `Ok(None)`: this device was removed (wipe the sync keys).
pub fn advance_ak(mut head: AkHead, old: &mut Vec<EpochKey>, blobs: &[Blob], me: &Opener) -> KResult<Option<AkHead>> {
    for b in blobs {
        match dir::accept_ak(&head, b, me)? {
            Outcome::Accepted(h) => {
                if h.epoch != head.epoch {
                    old.retain(|k| k.epoch != head.epoch);
                    old.push(EpochKey { epoch: head.epoch, key: head.ak.clone() });
                }
                // Keys of epochs below the minimum are never needed again.
                old.retain(|k| k.epoch >= h.min_record_epoch);
                head = h;
            }
            Outcome::Unchanged => {}
            Outcome::SelfRevoked => return Ok(None),
        }
    }
    Ok(Some(head))
}

pub fn advance_pak(
    mut head: PakHead,
    old: &mut Vec<EpochKey>,
    blobs: &[Blob],
    me: &Opener,
) -> KResult<Option<PakHead>> {
    for b in blobs {
        match dir::accept_pak(&head, b, me)? {
            Outcome::Accepted(h) => {
                if h.pak_epoch != head.pak_epoch {
                    old.retain(|k| k.epoch != head.pak_epoch);
                    old.push(EpochKey { epoch: head.pak_epoch, key: head.pak.clone() });
                }
                head = h;
            }
            Outcome::Unchanged => {}
            Outcome::SelfRevoked => return Ok(None),
        }
    }
    Ok(Some(head))
}

// ---- Restore with the passphrase or the recovery key (PROTOCOL §6.3) ---------------------------

/// The keys found by a restore, before the user confirms the device list.
pub struct RestorePlan {
    pub account_id: AccountId,
    rr: Key,
    pub ak_head: AkHead,
    pub ak_old: Vec<EpochKey>,
    pub pak_head: PakHead,
    pub pak_old: Vec<EpochKey>,
    /// The head directories (the devices that keep access are listed here).
    pub ak_dir: AkDir,
    pub pak_dir: PakDir,
}

/// From the anchor (the first versions of each chain, with `escrow_mac`) to the heads.
pub fn restore_open(rr: Key, account_id: AccountId, ak_chain: &[Blob], pak_chain: &[Blob]) -> KResult<RestorePlan> {
    let (Some(ak0), Some(pak0)) = (ak_chain.first(), pak_chain.first()) else {
        return Err(KeyError::Protocol("no anchor"));
    };
    let (ak_head, pak_head) = dir::open_anchor(ak0, pak0, &rr, &account_id)?;
    let esc = crypto::escrow_keypair(&rr, &account_id);
    let me = Opener::Escrow { sk: &esc.sk };
    let mut ak_old = Vec::new();
    let mut pak_old = Vec::new();
    let ak_head = advance_ak(ak_head, &mut ak_old, &ak_chain[1..], &me)?.ok_or(KeyError::Protocol("escrow revoked"))?;
    let pak_head =
        advance_pak(pak_head, &mut pak_old, &pak_chain[1..], &me)?.ok_or(KeyError::Protocol("escrow revoked"))?;
    #[allow(clippy::expect_used)] // non-empty, checked above
    let (ak_last, pak_last) = (ak_chain.last().expect("anchor"), pak_chain.last().expect("anchor"));
    Ok(RestorePlan {
        account_id,
        ak_dir: dir::parse_ak(&ak_last.bytes)?,
        pak_dir: dir::parse_pak(&pak_last.bytes)?,
        rr,
        ak_head,
        ak_old,
        pak_head,
        pak_old,
    })
}

/// The next `ak_dir` with `me` added (or its key replaced: a device that turns sync on again).
fn with_device(
    d: &AkDir,
    head: &AkHead,
    author: &str,
    new_id: &str,
    short: u16,
    pk: &[u8; 32],
    now_ms: u64,
) -> KResult<AkDir> {
    let mut next = d.clone();
    let seq = head.seq + 1;
    next.seq = seq;
    next.prev_hash = b64(&head.hash);
    next.devices.retain(|e| e.id != new_id);
    next.devices.push(DeviceEntry { id: new_id.into(), short, pk: b64(pk), added_by: author.into(), added_seq: seq });
    next.seals.retain(|s| s.to != new_id);
    next.seals.push(seal(SealKind::Ak, &head.ak, &head.account_id, head.epoch, new_id, pk)?);
    next.created_ms = now_ms;
    next.created_by = author.into();
    Ok(next)
}

/// The next `pak_dir` with a vault key of `device` added (or replaced) and PAK sealed to it.
fn with_vault_key(
    d: &PakDir,
    head: &PakHead,
    author: &str,
    device: &str,
    pk: &[u8; 32],
    now_ms: u64,
) -> KResult<PakDir> {
    let mut next = d.clone();
    next.seq = head.seq + 1;
    next.prev_hash = b64(&head.hash);
    next.vault_keys.retain(|v| v.device != device);
    next.vault_keys.push(VaultKey { device: device.into(), pk: b64(pk) });
    next.seals.retain(|s| s.to != device);
    next.seals.push(seal(SealKind::Pak, &head.pak, &head.account_id, head.pak_epoch, device, pk)?);
    next.created_ms = now_ms;
    next.created_by = author.into();
    Ok(next)
}

/// After the user confirmed the device list: `me` joins `ak_dir` (re-anchored under RR) and,
/// with `with_vault`, gets a vault key in `pak_dir` (re-anchored too).
pub fn restore_commit(plan: &RestorePlan, me: &Me, with_vault: bool, now_ms: u64) -> KResult<(Commit, Joined)> {
    let acc = plan.account_id;
    let device = KeyPair::generate()?;
    let ak_dir = with_device(&plan.ak_dir, &plan.ak_head, &me.device_id, &me.device_id, me.short, &device.pk, now_ms)?;
    let ak_blob = dir::sign_ak(&ak_dir, &plan.ak_head.ak, None, Some(&plan.rr), &acc)?;
    let mut commit =
        Commit { ak_dir: Some(ak_blob.clone()), if_ak_prev_hash: Some(plan.ak_head.hash), ..Default::default() };
    let pak = if with_vault {
        let vault = KeyPair::generate()?;
        let pak_dir = with_vault_key(&plan.pak_dir, &plan.pak_head, &me.device_id, &me.device_id, &vault.pk, now_ms)?;
        let pak_blob = dir::sign_pak(&pak_dir, &plan.pak_head.pak, None, Some(&plan.rr), &acc)?;
        let mut pin = PakPin::from_head(&plan.pak_head);
        pin.seq = pak_dir.seq;
        pin.hash = dir::hash(&pak_blob.bytes);
        commit.pak_dir = Some(pak_blob);
        commit.if_pak_prev_hash = Some(plan.pak_head.hash);
        Some(PakState { vault_sk: vault.sk.clone(), pak: pin, pak_old: plan.pak_old.clone(), book: Default::default() })
    } else {
        None
    };
    let joined = Joined {
        device_sk: device.sk.clone(),
        ak: AkPin {
            epoch: plan.ak_head.epoch,
            ak: plan.ak_head.ak.clone(),
            seq: ak_dir.seq,
            hash: dir::hash(&ak_blob.bytes),
            min_record_epoch: plan.ak_head.min_record_epoch,
        },
        ak_old: plan.ak_old.clone(),
        pak,
        esc_pk: plan.pak_head.esc_pk,
    };
    Ok((commit, joined))
}

/// A restore while a removal waits for its rotation and no device is online to do it (PROTOCOL
/// §7 step 8): the restorer joins *and* rotates in one commit — the next `ak_dir` epoch with this
/// device in and the devices the server no longer lists out, and with `rotate_pak` the next PAK
/// epoch too. Both are anchored under RR. The caller rekeys the records afterwards (as any
/// rotator does). Returns the removed device ids too.
pub fn restore_rotate_commit(
    plan: &RestorePlan,
    me: &Me,
    with_vault: bool,
    active: &[String],
    rotate_pak: bool,
    now_ms: u64,
) -> KResult<(Commit, Joined, Vec<String>)> {
    let acc = plan.account_id;
    let device = KeyPair::generate()?;
    let esc = crypto::escrow_keypair(&plan.rr, &acc);
    let head = &plan.ak_head;
    let e = head.epoch + 1;
    let new_ak = Zeroizing::new(crypto::random::<32>()?);
    let mut next = plan.ak_dir.clone();
    let removed: Vec<String> =
        next.devices.iter().filter(|d| !active.contains(&d.id) && d.id != me.device_id).map(|d| d.id.clone()).collect();
    next.devices.retain(|d| active.contains(&d.id) && d.id != me.device_id);
    next.devices.push(DeviceEntry {
        id: me.device_id.clone(),
        short: me.short,
        pk: b64(&device.pk),
        added_by: me.device_id.clone(),
        added_seq: head.seq + 1,
    });
    for r in &removed {
        if !next.revoked.contains(r) {
            next.revoked.push(r.clone());
        }
    }
    next.epoch = e;
    next.seq = head.seq + 1;
    next.prev_hash = b64(&head.hash);
    next.min_record_epoch = head.epoch;
    let active_refs: Vec<&str> = active.iter().map(String::as_str).collect();
    let mut seals = Vec::new();
    for (id, pk) in dir::seal_targets(&next, &active_refs)? {
        seals.push(seal(SealKind::Ak, &new_ak, &acc, e, &id, &pk)?);
    }
    seals.push(seal(SealKind::Ak, &new_ak, &acc, e, "escrow", &esc.pk)?);
    next.seals = seals;
    let (n, ct) = dir::hist_seal(SealKind::Ak, &new_ak, &head.ak, &acc, e)?;
    next.hist = Some(dir::AkHist { epoch: head.epoch, n, ct });
    next.created_ms = now_ms;
    next.created_by = me.device_id.clone();
    let ak_blob = dir::sign_ak(&next, &new_ak, Some(&head.ak), Some(&plan.rr), &acc)?;
    let mut commit = Commit { ak_dir: Some(ak_blob.clone()), if_ak_prev_hash: Some(head.hash), ..Default::default() };
    let mut ak_old = plan.ak_old.clone();
    ak_old.push(EpochKey { epoch: head.epoch, key: head.ak.clone() });
    let vault = if with_vault { Some(KeyPair::generate()?) } else { None };
    let ph = &plan.pak_head;
    let pak = if rotate_pak {
        let p = ph.pak_epoch + 1;
        let new_pak = Zeroizing::new(crypto::random::<32>()?);
        let mut np = plan.pak_dir.clone();
        let mut targets = dir::vault_seal_targets(&plan.pak_dir, &next, &active_refs)?;
        targets.retain(|(id, _)| *id != me.device_id);
        np.vault_keys.retain(|v| targets.iter().any(|(id, _)| *id == v.device));
        if let Some(v) = &vault {
            np.vault_keys.push(VaultKey { device: me.device_id.clone(), pk: b64(&v.pk) });
            targets.push((me.device_id.clone(), v.pk));
        }
        let mut seals = Vec::new();
        for (id, pk) in &targets {
            seals.push(seal(SealKind::Pak, &new_pak, &acc, p, id, pk)?);
        }
        seals.push(seal(SealKind::Pak, &new_pak, &acc, p, "escrow", &esc.pk)?);
        np.seals = seals;
        np.pak_epoch = p;
        np.seq = ph.seq + 1;
        np.prev_hash = b64(&ph.hash);
        let (n, ct) = dir::hist_seal(SealKind::Pak, &new_pak, &ph.pak, &acc, p)?;
        np.hist = Some(dir::PakHist { pak_epoch: ph.pak_epoch, n, ct });
        np.created_ms = now_ms;
        np.created_by = me.device_id.clone();
        let pak_blob = dir::sign_pak(&np, &new_pak, Some(&ph.pak), Some(&plan.rr), &acc)?;
        commit.pak_dir = Some(pak_blob.clone());
        commit.if_pak_prev_hash = Some(ph.hash);
        vault.as_ref().map(|v| {
            let mut pak_old = plan.pak_old.clone();
            pak_old.push(EpochKey { epoch: ph.pak_epoch, key: ph.pak.clone() });
            PakState {
                vault_sk: v.sk.clone(),
                pak: PakPin {
                    pak_epoch: p,
                    pak: new_pak.clone(),
                    seq: np.seq,
                    hash: dir::hash(&pak_blob.bytes),
                    esc_pk: esc.pk,
                    rr_gen: ph.rr_gen,
                },
                pak_old,
                book: Default::default(),
            }
        })
    } else if let Some(v) = &vault {
        let pak_dir = with_vault_key(&plan.pak_dir, ph, &me.device_id, &me.device_id, &v.pk, now_ms)?;
        let pak_blob = dir::sign_pak(&pak_dir, &ph.pak, None, Some(&plan.rr), &acc)?;
        let mut pin = PakPin::from_head(ph);
        pin.seq = pak_dir.seq;
        pin.hash = dir::hash(&pak_blob.bytes);
        commit.pak_dir = Some(pak_blob);
        commit.if_pak_prev_hash = Some(ph.hash);
        Some(PakState { vault_sk: v.sk.clone(), pak: pin, pak_old: plan.pak_old.clone(), book: Default::default() })
    } else {
        None
    };
    let joined = Joined {
        device_sk: device.sk.clone(),
        ak: AkPin {
            epoch: e,
            ak: new_ak,
            seq: next.seq,
            hash: dir::hash(&ak_blob.bytes),
            min_record_epoch: head.epoch,
        },
        ak_old,
        pak,
        esc_pk: esc.pk,
    };
    Ok((commit, joined, removed))
}

/// A directory head after a commit: (seq, hash).
pub type HeadAt = (u64, [u8; 32]);

/// "Finish securing" (PROTOCOL §7 step 8, THREAT_MODEL R1): with RR from the passphrase or the
/// recovery key, the current heads once more, carrying `escrow_mac` — a new anchor a revoked
/// device can't forge (it never had RR). Returns the commit and the new (seq, hash) of both.
pub fn reanchor_commit(plan: &RestorePlan, me: &str, now_ms: u64) -> KResult<(Commit, HeadAt, HeadAt)> {
    let acc = plan.account_id;
    let mut a = plan.ak_dir.clone();
    a.seq = plan.ak_head.seq + 1;
    a.prev_hash = b64(&plan.ak_head.hash);
    a.created_ms = now_ms;
    a.created_by = me.into();
    let ak_blob = dir::sign_ak(&a, &plan.ak_head.ak, None, Some(&plan.rr), &acc)?;
    let mut p = plan.pak_dir.clone();
    p.seq = plan.pak_head.seq + 1;
    p.prev_hash = b64(&plan.pak_head.hash);
    p.created_ms = now_ms;
    p.created_by = me.into();
    let pak_blob = dir::sign_pak(&p, &plan.pak_head.pak, None, Some(&plan.rr), &acc)?;
    let out_ak = (a.seq, dir::hash(&ak_blob.bytes));
    let out_pak = (p.seq, dir::hash(&pak_blob.bytes));
    Ok((
        Commit {
            ak_dir: Some(ak_blob),
            pak_dir: Some(pak_blob),
            if_ak_prev_hash: Some(plan.ak_head.hash),
            if_pak_prev_hash: Some(plan.pak_head.hash),
            ..Default::default()
        },
        out_ak,
        out_pak,
    ))
}

// ---- Device approval (PROTOCOL §6.2) ------------------------------------------------------------

/// N's ephemeral side: made before asking, bound by the commitment.
pub struct NewSide {
    esk: Key,
    pub epk: [u8; 32],
    pub nonce: [u8; 32],
    pub commit: [u8; 32],
    /// N's long-term keys, sent inside `box_N` (so the server can't swap them).
    pub device: KeyPair,
    pub vault: Option<KeyPair>,
}

pub fn new_side(with_vault: bool) -> KResult<NewSide> {
    let e = KeyPair::generate()?;
    let nonce = crypto::random::<32>()?;
    Ok(NewSide {
        commit: sas::commit(&e.pk, &nonce),
        epk: e.pk,
        esk: e.sk,
        nonce,
        device: KeyPair::generate()?,
        vault: if with_vault { Some(KeyPair::generate()?) } else { None },
    })
}

/// The values both sides put into the transcript.
pub struct ApprovalCtx<'a> {
    pub account_id: &'a AccountId,
    pub approval_id: &'a str,
    pub dev_id_a: &'a str,
    pub dev_id_n: &'a str,
    pub n_label: &'a str,
    pub n_platform: &'a str,
}

/// The SAS and the box keys.
pub struct Agreed {
    pub sas: u32,
    k_an: Key,
    k_na: Key,
}

impl Agreed {
    pub fn sas_text(&self) -> String {
        sas::format_sas(self.sas)
    }
}

fn agree(
    ctx: &ApprovalCtx,
    dh: &[u8; 32],
    commit: &[u8; 32],
    epk_a: &[u8; 32],
    nonce_a: &[u8; 32],
    epk_n: &[u8; 32],
    nonce_n: &[u8; 32],
) -> KResult<Agreed> {
    let t = sas::Transcript {
        account_id: ctx.account_id,
        approval_id: ctx.approval_id,
        dev_id_a: ctx.dev_id_a,
        dev_id_n: ctx.dev_id_n,
        n_label: ctx.n_label,
        n_platform: ctx.n_platform,
        commit_n: commit,
        epk_a,
        nonce_a,
        epk_n,
        nonce_n,
    }
    .bytes()?;
    let d = sas::derive(dh, &t);
    Ok(Agreed { sas: d.sas, k_an: d.k_an, k_na: d.k_na })
}

/// What N sends A inside `box_N`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxN {
    pub pk_general: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pk_vault: Option<String>,
    pub name: String,
}

/// N, once A answered: the SAS to compare and `box_N` to reveal.
pub fn n_agree(
    side: &NewSide,
    ctx: &ApprovalCtx,
    epk_a: &[u8; 32],
    nonce_a: &[u8; 32],
    name: &str,
) -> KResult<(Agreed, Vec<u8>)> {
    let dh = sas::dh(&side.esk, epk_a)?;
    let ag = agree(ctx, &dh, &side.commit, epk_a, nonce_a, &side.epk, &side.nonce)?;
    let body =
        BoxN { pk_general: b64(&side.device.pk), pk_vault: side.vault.as_ref().map(|v| b64(&v.pk)), name: name.into() };
    #[allow(clippy::expect_used)] // plain fields
    let pt = Zeroizing::new(serde_json::to_vec(&body).expect("BoxN serializes"));
    let bx = sas::box_seal(&ag.k_na, Direction::NToA, &pt)?;
    Ok((ag, bx))
}

/// N after the user typed the code shown on A: the confirmation tag, or `SasMismatch` (then
/// the approval is cancelled; one attempt only).
pub fn n_confirm(ag: &Agreed, typed: &str) -> KResult<[u8; 32]> {
    let digits: String = typed.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 6 || digits.parse::<u32>().ok() != Some(ag.sas) {
        return Err(KeyError::SasMismatch);
    }
    Ok(sas::confirm_tag(&ag.k_na))
}

/// A's ephemeral side, made only after `commit_N` was received.
pub struct ApproverSide {
    esk: Key,
    pub epk: [u8; 32],
    pub nonce: [u8; 32],
}

pub fn approver_side() -> KResult<ApproverSide> {
    let e = KeyPair::generate()?;
    Ok(ApproverSide { epk: e.pk, esk: e.sk, nonce: crypto::random::<32>()? })
}

/// A, once N revealed: checks the commitment, derives the SAS and opens `box_N`.
pub fn a_agree(
    side: &ApproverSide,
    ctx: &ApprovalCtx,
    commit_n: &[u8; 32],
    epk_n: &[u8; 32],
    nonce_n: &[u8; 32],
    box_n: &[u8],
) -> KResult<(Agreed, BoxN)> {
    if sas::commit(epk_n, nonce_n) != *commit_n {
        return Err(KeyError::Protocol("the commitment doesn't match"));
    }
    let dh = sas::dh(&side.esk, epk_n)?;
    let ag = agree(ctx, &dh, commit_n, &side.epk, &side.nonce, epk_n, nonce_n)?;
    let pt = sas::box_open(&ag.k_na, Direction::NToA, box_n).map_err(|_| KeyError::Protocol("box_N"))?;
    let b: BoxN = serde_json::from_slice(&pt).map_err(|_| KeyError::Protocol("box_N"))?;
    let pk: [u8; 32] = unb64(&b.pk_general)?.try_into().map_err(|_| KeyError::Protocol("pk"))?;
    crypto::check_public_key(&pk)?;
    if let Some(v) = &b.pk_vault {
        let vpk: [u8; 32] = unb64(v)?.try_into().map_err(|_| KeyError::Protocol("pk"))?;
        crypto::check_public_key(&vpk)?;
    }
    Ok((ag, b))
}

/// A checks N's confirmation tag (sent only if the typed SAS matched on N).
pub fn a_check_tag(ag: &Agreed, tag: &[u8]) -> KResult<()> {
    sas::confirm_verify(&ag.k_na, tag).map_err(|_| KeyError::Protocol("tag_N"))
}

/// What A gives N inside `box_A`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub ak: String,
    pub epoch: u32,
    pub ak_dir_head: String,
    pub pak_dir_head: String,
    pub esc_pk: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pak: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pak_epoch: Option<u32>,
    pub dev_short: u16,
}

/// A adds N to `ak_dir` (seq + 1, sealed to N's general key) and builds `box_A`. `pak`: A's PAK
/// side when the user chose to share passwords (and entered the master password for it).
#[allow(clippy::too_many_arguments)]
pub fn a_admit(
    ag: &Agreed,
    head: &AkHead,
    head_dir: &AkDir,
    pak_dir_hash: [u8; 32],
    esc_pk: [u8; 32],
    me: &str,
    n_id: &str,
    n_short: u16,
    n_box: &BoxN,
    pak: Option<&PakPin>,
    now_ms: u64,
) -> KResult<(Commit, Vec<u8>, [u8; 32])> {
    let pk: [u8; 32] = unb64(&n_box.pk_general)?.try_into().map_err(|_| KeyError::Protocol("pk"))?;
    let next = with_device(head_dir, head, me, n_id, n_short, &pk, now_ms)?;
    let blob = dir::sign_ak(&next, &head.ak, None, None, &head.account_id)?;
    let new_hash = dir::hash(&blob.bytes);
    let payload = Payload {
        ak: b64(head.ak.as_ref()),
        epoch: head.epoch,
        ak_dir_head: b64(&new_hash),
        pak_dir_head: b64(&pak_dir_hash),
        esc_pk: b64(&esc_pk),
        pak: pak.map(|p| b64(p.pak.as_ref())),
        pak_epoch: pak.map(|p| p.pak_epoch),
        dev_short: n_short,
    };
    #[allow(clippy::expect_used)] // plain fields
    let pt = Zeroizing::new(serde_json::to_vec(&payload).expect("Payload serializes"));
    let bx = sas::box_seal(&ag.k_an, Direction::AToN, &pt)?;
    Ok((Commit { ak_dir: Some(blob), if_ak_prev_hash: Some(head.hash), ..Default::default() }, bx, new_hash))
}

/// N opens `box_A` and checks it against the directories it fetched (PROTOCOL §6.2 "what N does
/// next"): the head hashes, the MACs under the received keys, itself listed with its own key,
/// and the escrow key pin. Nothing is uploaded before this succeeds.
pub fn n_accept(
    ag: &Agreed,
    side: &NewSide,
    box_a: &[u8],
    account_id: AccountId,
    my_id: &str,
    ak_blob: &Blob,
    pak_blob: &Blob,
) -> KResult<(Joined, Payload)> {
    let pt = sas::box_open(&ag.k_an, Direction::AToN, box_a).map_err(|_| KeyError::Protocol("box_A"))?;
    let p: Payload = serde_json::from_slice(&pt).map_err(|_| KeyError::Protocol("box_A"))?;
    let ak: [u8; 32] = unb64(&p.ak)?.try_into().map_err(|_| KeyError::Protocol("ak"))?;
    let ak = Zeroizing::new(ak);
    if b64(&dir::hash(&ak_blob.bytes)) != p.ak_dir_head || b64(&dir::hash(&pak_blob.bytes)) != p.pak_dir_head {
        return Err(KeyError::Dir(DirError::Fork));
    }
    crypto::hmac_verify(dir::ak_mac_key(&ak, &account_id, p.epoch).as_ref(), &ak_blob.bytes, &ak_blob.mac)?;
    let d = dir::parse_ak(&ak_blob.bytes)?;
    let mine = d.devices.iter().find(|e| e.id == my_id).ok_or(KeyError::Protocol("not listed"))?;
    if d.epoch != p.epoch || mine.pk != b64(&side.device.pk) || d.revoked.iter().any(|r| r == my_id) {
        return Err(KeyError::Protocol("the directory doesn't list this device's key"));
    }
    let pd = dir::parse_pak(&pak_blob.bytes)?;
    if pd.esc_pk != p.esc_pk {
        return Err(KeyError::Dir(DirError::EscrowKeyChanged));
    }
    let pak = match (&p.pak, p.pak_epoch, &side.vault) {
        (Some(k), Some(e), Some(v)) => {
            let k: [u8; 32] = unb64(k)?.try_into().map_err(|_| KeyError::Protocol("pak"))?;
            let k = Zeroizing::new(k);
            crypto::hmac_verify(dir::pak_mac_key(&k, &account_id, e).as_ref(), &pak_blob.bytes, &pak_blob.mac)?;
            if pd.pak_epoch != e {
                return Err(KeyError::Protocol("pak epoch"));
            }
            let esc_pk: [u8; 32] = unb64(&pd.esc_pk)?.try_into().map_err(|_| KeyError::Protocol("esc_pk"))?;
            Some(PakState {
                vault_sk: v.sk.clone(),
                pak: PakPin {
                    pak_epoch: e,
                    pak: k,
                    seq: pd.seq,
                    hash: dir::hash(&pak_blob.bytes),
                    esc_pk,
                    rr_gen: pd.rr_gen,
                },
                pak_old: vec![],
                book: Default::default(),
            })
        }
        _ => None,
    };
    let joined = Joined {
        device_sk: side.device.sk.clone(),
        ak: AkPin {
            epoch: d.epoch,
            ak,
            seq: d.seq,
            hash: dir::hash(&ak_blob.bytes),
            min_record_epoch: d.min_record_epoch,
        },
        ak_old: vec![],
        pak,
        esc_pk: unb64(&p.esc_pk)?.try_into().map_err(|_| KeyError::Protocol("esc_pk"))?,
    };
    Ok((joined, p))
}

/// After receiving PAK, N adds its own vault key to `pak_dir` (PROTOCOL §6.2 step 5).
pub fn add_vault_key(
    pak: &PakState,
    pak_dir: &PakDir,
    account_id: AccountId,
    me: &str,
    vault_pk: &[u8; 32],
    now_ms: u64,
) -> KResult<(Commit, PakPin)> {
    let head = pak.pak.head(account_id);
    let next = with_vault_key(pak_dir, &head, me, me, vault_pk, now_ms)?;
    let blob = dir::sign_pak(&next, &head.pak, None, None, &account_id)?;
    let mut pin = pak.pak.clone();
    pin.seq = next.seq;
    pin.hash = dir::hash(&blob.bytes);
    Ok((Commit { pak_dir: Some(blob), if_pak_prev_hash: Some(head.hash), ..Default::default() }, pin))
}

mod b64_key {
    use super::Key;
    use crate::sync::envelope::{b64, unb64};
    use serde::{Deserialize, Deserializer, Serializer};
    use zeroize::Zeroizing;

    pub fn serialize<S: Serializer>(k: &Key, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&Zeroizing::new(b64(k.as_ref())))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        let t = Zeroizing::new(String::deserialize(d)?);
        let v = Zeroizing::new(unb64(&t).map_err(|_| serde::de::Error::custom("key"))?);
        let a: [u8; 32] = v.as_slice().try_into().map_err(|_| serde::de::Error::custom("key length"))?;
        Ok(Zeroizing::new(a))
    }
}

mod b64_arr {
    use crate::sync::envelope::{b64, unb64};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&b64(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let v = unb64(&String::deserialize(d)?).map_err(|_| serde::de::Error::custom("base64"))?;
        v.as_slice().try_into().map_err(|_| serde::de::Error::custom("length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACC: AccountId = [0x42; 16];

    fn me(id: &str, short: u16) -> Me {
        Me { device_id: id.into(), short }
    }

    #[test]
    fn passphrases_and_recovery_key_checks() {
        let p = new_passphrase().unwrap();
        let words: Vec<&str> = p.split('-').collect();
        assert_eq!(words.len(), 7);
        assert!(words.iter().all(|w| bip39::Language::English.find_word(w).is_some()));
        assert_ne!(*p, *new_passphrase().unwrap());
        assert_eq!(*normalize_passphrase("  Apple  BANANA-cherry \n"), "apple-banana-cherry");
        let rk = codes::format_recovery_key(&[7; 20]);
        let group = recovery_key_check_group(&rk);
        assert_eq!(group.len(), 4);
        assert!(recovery_key_confirmed(&rk, &group.to_lowercase()));
        assert!(!recovery_key_confirmed(&rk, "ZZZZ"));
        assert!(!recovery_key_confirmed(&rk, ""));
    }

    #[test]
    fn setup_then_restore_by_passphrase_and_by_recovery_key() {
        let plan = plan_setup(ACC, me("dev-a", 0), true).unwrap();
        let (commit, joined) = setup_commit(&plan, 1).unwrap();
        assert!(commit.if_absent && commit.escrow.is_some());
        let (ak_blob, pak_blob) = (commit.ak_dir.clone().unwrap(), commit.pak_dir.clone().unwrap());
        assert!(ak_blob.escrow_mac.is_some() && pak_blob.escrow_mac.is_some(), "an anchor");
        assert_eq!(joined.ak.hash, dir::hash(&ak_blob.bytes));
        assert!(joined.pak.is_some());
        let escrow = commit.escrow.clone().unwrap();
        for secret in [
            Secret::Passphrase(Zeroizing::new(plan.passphrase.replace('-', " ").to_uppercase())),
            Secret::RecoveryKey(Zeroizing::new(plan.recovery_key.to_lowercase())),
        ] {
            let rr = unwrap_rr(&escrow, &secret, &ACC).unwrap();
            let r = restore_open(rr, ACC, std::slice::from_ref(&ak_blob), std::slice::from_ref(&pak_blob)).unwrap();
            assert_eq!(*r.ak_head.ak, *joined.ak.ak);
            assert_eq!(*r.pak_head.pak, *joined.pak.as_ref().unwrap().pak.pak);
            assert_eq!(r.ak_dir.devices.len(), 1);
            // The restorer joins; the first device accepts that version as a same-epoch change.
            let (c2, j2) = restore_commit(&r, &me("dev-c", 2), true, 2).unwrap();
            let next = c2.ak_dir.unwrap();
            assert_eq!(c2.if_ak_prev_hash, Some(joined.ak.hash));
            let head = joined.ak.head(ACC);
            let sk = joined.device_sk.clone();
            let mut old = vec![];
            let h = advance_ak(head, &mut old, std::slice::from_ref(&next), &Opener::Device { id: "dev-a", sk: &sk })
                .unwrap()
                .unwrap();
            assert_eq!((h.seq, h.epoch), (2, 1));
            assert_eq!(j2.ak.hash, h.hash);
            // And the restorer itself, as a device now, sees its own entry.
            let d = dir::parse_ak(&next.bytes).unwrap();
            assert!(d.devices.iter().any(|x| x.id == "dev-c"));
            assert!(c2.pak_dir.is_some() && j2.pak.is_some());
        }
        // Wrong secrets.
        assert_eq!(
            unwrap_rr(&escrow, &Secret::Passphrase(Zeroizing::new("wrong words here".into())), &ACC).err(),
            Some(KeyError::WrongSecret)
        );
        assert_eq!(
            unwrap_rr(&escrow, &Secret::RecoveryKey(Zeroizing::new(codes::format_recovery_key(&[1; 20]))), &ACC).err(),
            Some(KeyError::WrongSecret)
        );
        assert_eq!(
            unwrap_rr(&escrow, &Secret::RecoveryKey(Zeroizing::new("KRNK-nope".into())), &ACC).err(),
            Some(KeyError::WrongSecret)
        );
    }

    #[test]
    fn a_weak_kdf_or_a_swapped_escrow_key_is_refused() {
        let plan = plan_setup(ACC, me("dev-a", 0), false).unwrap();
        let (commit, _) = setup_commit(&plan, 1).unwrap();
        let mut escrow = commit.escrow.clone().unwrap();
        escrow.pp_kdf.m_kib = 1024;
        let r = unwrap_rr(&escrow, &Secret::Passphrase(plan.passphrase.clone()), &ACC);
        assert_eq!(r.err(), Some(KeyError::Crypto(SyncError::WeakKdf)), "a server-lowered KDF");
        // An anchor whose pak_dir names another escrow key (a server swapping esc_pk).
        let rr =
            unwrap_rr(&commit.escrow.clone().unwrap(), &Secret::Passphrase(plan.passphrase.clone()), &ACC).unwrap();
        let pak = commit.pak_dir.clone().unwrap();
        let mut d = dir::parse_pak(&pak.bytes).unwrap();
        d.esc_pk = b64(&KeyPair::generate().unwrap().pk);
        let forged = dir::sign_pak(&d, &[9; 32], None, Some(&rr), &ACC).unwrap();
        let r = restore_open(rr, ACC, &[commit.ak_dir.clone().unwrap()], &[forged]);
        assert!(matches!(r.err(), Some(KeyError::Dir(DirError::EscrowKeyMismatch))), "esc_pk swap");
    }

    #[test]
    fn a_tampered_directory_raises_an_alarm() {
        let plan = plan_setup(ACC, me("dev-a", 0), false).unwrap();
        let (commit, joined) = setup_commit(&plan, 1).unwrap();
        let mut d = dir::parse_ak(&commit.ak_dir.clone().unwrap().bytes).unwrap();
        // The server adds its own device to the next version, with a made-up key.
        d.seq = 2;
        d.prev_hash = b64(&joined.ak.hash);
        d.devices.push(DeviceEntry {
            id: "evil".into(),
            short: 9,
            pk: b64(&[5; 32]),
            added_by: "dev-a".into(),
            added_seq: 2,
        });
        let forged = dir::sign_ak(&d, &[0x66; 32], None, None, &ACC).unwrap();
        let sk = joined.device_sk.clone();
        let r = advance_ak(joined.ak.head(ACC), &mut vec![], &[forged], &Opener::Device { id: "dev-a", sk: &sk });
        let e = r.err().unwrap();
        assert!(e.is_alarm(), "{e:?}");
    }

    #[test]
    fn approval_with_the_right_code_and_with_a_wrong_one() {
        let plan = plan_setup(ACC, me("dev-a", 0), true).unwrap();
        let (commit, joined_a) = setup_commit(&plan, 1).unwrap();
        let ak_blob = commit.ak_dir.clone().unwrap();
        let pak_blob = commit.pak_dir.clone().unwrap();
        let head = joined_a.ak.head(ACC);
        let head_dir = dir::parse_ak(&ak_blob.bytes).unwrap();
        let ctx = ApprovalCtx {
            account_id: &ACC,
            approval_id: "appr-1",
            dev_id_a: "dev-a",
            dev_id_n: "dev-n",
            n_label: "Krunk on Windows",
            n_platform: "windows",
        };
        // N asks; A answers; N reveals; both see the same 6 digits.
        let n = new_side(true).unwrap();
        let a = approver_side().unwrap();
        let (ag_n, box_n) = n_agree(&n, &ctx, &a.epk, &a.nonce, "Laptop").unwrap();
        let (ag_a, nb) = a_agree(&a, &ctx, &n.commit, &n.epk, &n.nonce, &box_n).unwrap();
        assert_eq!(ag_a.sas, ag_n.sas);
        assert_eq!(nb.name, "Laptop");
        // A wrong code is refused on N (and the approval is cancelled by the caller).
        let wrong = format!("{:06}", (ag_n.sas + 1) % 1_000_000);
        assert_eq!(n_confirm(&ag_n, &wrong).err(), Some(KeyError::SasMismatch));
        let tag = n_confirm(&ag_n, &ag_a.sas_text()).unwrap();
        a_check_tag(&ag_a, &tag).unwrap();
        assert!(a_check_tag(&ag_a, &[0; 32]).is_err());
        // A admits N and shares PAK; N checks everything against the directories.
        let pak_pin = &joined_a.pak.as_ref().unwrap().pak;
        let (c, box_a, new_hash) =
            a_admit(&ag_a, &head, &head_dir, pak_pin.hash, pak_pin.esc_pk, "dev-a", "dev-n", 1, &nb, Some(pak_pin), 2)
                .unwrap();
        let new_ak = c.ak_dir.unwrap();
        assert_eq!(dir::hash(&new_ak.bytes), new_hash);
        let (jn, payload) = n_accept(&ag_n, &n, &box_a, ACC, "dev-n", &new_ak, &pak_blob).unwrap();
        assert_eq!(*jn.ak.ak, *joined_a.ak.ak);
        assert_eq!(payload.dev_short, 1);
        assert!(jn.pak.is_some());
        // A server that serves a different directory than A committed is caught.
        assert!(matches!(
            n_accept(&ag_n, &n, &box_a, ACC, "dev-n", &ak_blob, &pak_blob).err(),
            Some(KeyError::Dir(DirError::Fork))
        ));
        // A box from another exchange doesn't open.
        let other = new_side(false).unwrap();
        let (ag_x, _) = n_agree(&other, &ctx, &a.epk, &a.nonce, "x").unwrap();
        assert!(n_accept(&ag_x, &other, &box_a, ACC, "dev-n", &new_ak, &pak_blob).is_err());
        // A commitment that doesn't match N's reveal is refused by A.
        assert!(matches!(a_agree(&a, &ctx, &[0; 32], &n.epk, &n.nonce, &box_n).err(), Some(KeyError::Protocol(_))));
        // N adds its own vault key to pak_dir afterwards.
        let pd = dir::parse_pak(&pak_blob.bytes).unwrap();
        let vpk = n.vault.as_ref().unwrap().pk;
        let (c2, pin) = add_vault_key(jn.pak.as_ref().unwrap(), &pd, ACC, "dev-n", &vpk, 3).unwrap();
        let nd = dir::parse_pak(&c2.pak_dir.unwrap().bytes).unwrap();
        assert!(nd.vault_keys.iter().any(|v| v.device == "dev-n") && nd.seals.iter().any(|s| s.to == "dev-n"));
        assert_eq!(pin.seq, 2);
    }
}
