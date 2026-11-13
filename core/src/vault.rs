//! Password vault.
//!
//! Key hierarchy:
//!
//! ```text
//! master password --Argon2id(salt, m, t, p)--> KEK (32 bytes, never stored)
//! KEK --XChaCha20-Poly1305--> check record   (proves the password is right)
//! KEK --XChaCha20-Poly1305--> wrapped VK     (random 32-byte vault key)
//!     --KeyWrapper (DPAPI CurrentUser on Windows)--> stored "vaultKey"
//! VK  --XChaCha20-Poly1305, fresh 24-byte nonce, AAD = entry id--> each entry
//! VK  --XChaCha20-Poly1305--> manifest (entry ids + generation)
//!
//! recovery phrase (24 BIP39 words = 256 bits, never stored)
//!     --Argon2id(recovery salt, ≥ the master's cost)--> seed --HPKE DeriveKeyPair--> X25519 key pair
//! VK  --HPKE base (X25519-HKDF-SHA256, HKDF-SHA256, ChaCha20Poly1305) to the public key-->
//!     --KeyWrapper--> stored "recovery.sealed" (format 3, Krunk 0.7.0)
//! ```
//!
//! The recovery slot keeps only the public key, so every save seals the current VK to it: a
//! changed master password (new VK) keeps the phrase valid without knowing it, and a new phrase
//! (new key pair) makes the old one useless. Like the master path it needs the same Windows
//! account (DPAPI).
//!
//! The file is JSON with base64 fields. All plaintext secrets live in
//! `Zeroizing` buffers and are wiped on drop; locking the vault means
//! dropping the [`Vault`]. Writes go to a temp file that is synced and then
//! renamed over the old one, so the vault is never half-written.

use crate::urlclean::origin_of;
use argon2::{Algorithm, Argon2, Params, Version};
use base64ct::{Base64, Encoding};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

const FORMAT: u32 = 1;
/// Format 2 (Krunk 0.6.0): the same file plus sealed payment cards. Written only once a
/// card exists, so vaults without cards stay byte-identical to 0.5.2's.
const FORMAT_CARDS: u32 = 2;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const SALT_LEN: usize = 16;
// The browser's name before the rename to Krunk. Part of the vault format:
// changing it would make every existing vault fail to unlock.
const CHECK_PLAINTEXT: &[u8] = b"LightBrowser vault key check v1";
const AAD_CHECK: &[u8] = b"lb-vault:check";
const AAD_VAULT_KEY: &[u8] = b"lb-vault:vault-key";
const AAD_MANIFEST: &[u8] = b"lb-vault:manifest";
const AAD_CARD_PREFIX: &[u8] = b"lb-vault:card:";
/// Format 3 (Krunk 0.7.0): formats 1/2 plus a recovery slot. Written only while a recovery
/// phrase exists, so vaults without one stay readable by 0.6.0.
const FORMAT_RECOVERY: u32 = 3;
/// HPKE `info` of the recovery seal.
const RECOVERY_INFO: &[u8] = b"krunk-vault-recovery-v1";
/// Format 4 (Krunk 0.9.0, sync S5; PROTOCOL §8 says "format 2", which 0.6.0's cards had taken):
/// formats 1–3 plus the sealed sync part. Written only while sync keeps state in the vault, so
/// vaults without it stay readable by 0.8.0.
const FORMAT_SYNC: u32 = 4;
const AAD_SYNC: &[u8] = b"lb-vault:sync";

/// Minimum master-password length accepted by `create` and `change_password`.
pub const MIN_MASTER_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The master password is wrong (or the header was tampered with).
    WrongPassword,
    /// Too many failed attempts; try again after this many milliseconds.
    RateLimited {
        wait_ms: u64,
    },
    /// The vault file failed authentication or does not parse.
    Corrupt(String),
    /// The key wrapper (DPAPI) failed, e.g. the file came from another user or machine.
    Wrap(String),
    WeakPassword,
    InvalidEntry(&'static str),
    NotFound,
    Io(String),
    Random,
    /// The recovery phrase does not open this vault (0.7.0).
    WrongPhrase,
    /// The vault has no recovery phrase.
    NoRecovery,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::WrongPassword => write!(f, "Wrong master password"),
            VaultError::RateLimited { wait_ms } => {
                write!(f, "Too many attempts; try again in {} s", wait_ms.div_ceil(1000))
            }
            VaultError::Corrupt(m) => write!(f, "Vault file is damaged: {m}"),
            VaultError::Wrap(m) => write!(f, "Vault key cannot be unwrapped on this account/machine: {m}"),
            VaultError::WeakPassword => write!(f, "Master password must be at least {MIN_MASTER_LEN} characters"),
            VaultError::InvalidEntry(m) => write!(f, "Invalid entry: {m}"),
            VaultError::NotFound => write!(f, "Entry not found"),
            VaultError::Io(m) => write!(f, "Vault I/O error: {m}"),
            VaultError::Random => write!(f, "System random number generator failed"),
            VaultError::WrongPhrase => write!(f, "This recovery phrase does not open the vault"),
            VaultError::NoRecovery => write!(f, "This vault has no recovery phrase"),
        }
    }
}

impl std::error::Error for VaultError {}

pub type VResult<T> = Result<T, VaultError>;

/// Defence-in-depth wrapping of the (already encrypted) vault key. On
/// Windows this is DPAPI with the CurrentUser scope, which makes a copied
/// vault file useless on another account or machine.
pub trait KeyWrapper {
    /// Name stored in the file so a mismatched wrapper is detected.
    fn name(&self) -> &'static str;
    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String>;
    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String>;
}

/// No extra wrapping (tests and non-Windows builds).
pub struct NoWrap;

impl KeyWrapper for NoWrap {
    fn name(&self) -> &'static str {
        "none"
    }
    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        Ok(data.to_vec())
    }
    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        Ok(Zeroizing::new(data.to_vec()))
    }
}

/// Argon2id cost parameters, stored in the vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl Default for KdfParams {
    /// 64 MiB, 3 passes, 1 lane.
    fn default() -> Self {
        KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }
    }
}

impl KdfParams {
    /// Cheap parameters for unit tests only.
    pub fn insecure_for_tests() -> Self {
        KdfParams { m_kib: 64, t: 1, p: 1 }
    }

    /// Upper bounds stop a crafted file from making unlock hang or OOM.
    fn sane(&self) -> bool {
        (8..=4 * 1024 * 1024).contains(&self.m_kib) && (1..=64).contains(&self.t) && (1..=16).contains(&self.p)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KdfRecord {
    alg: String,
    #[serde(flatten)]
    params: KdfParams,
    salt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sealed {
    nonce: String,
    ct: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedEntry {
    id: String,
    nonce: String,
    ct: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VaultFile {
    format: u32,
    kdf: KdfRecord,
    wrap: String,
    check: Sealed,
    /// `wrapper.protect(nonce || AEAD_KEK(VK))`, base64.
    vault_key: String,
    manifest: Sealed,
    entries: Vec<SealedEntry>,
    /// Payment cards (format 2); absent in files without any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cards: Vec<SealedEntry>,
    /// The recovery slot (format 3); absent without a recovery phrase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery: Option<RecoverySlot>,
    /// The sealed sync part (format 4); absent while sync keeps nothing in the vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync: Option<Sealed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoverySlot {
    kdf: KdfRecord,
    /// The X25519 public key the vault key is sealed to.
    public: String,
    /// `wrapper.protect(enc || ct)` of the HPKE seal of the vault key, base64.
    sealed: String,
    /// When the phrase was made (unix ms).
    created: i64,
}

/// The recovery slot of an unlocked vault (no secret in it).
#[derive(Debug, Clone, Copy)]
struct RecoveryKey {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    public: [u8; 32],
    created: i64,
}

/// The recovery cost: the master's, at least the default (the phrase protects the same keys).
fn recovery_params(master: KdfParams) -> KdfParams {
    // (Unit tests with the cheap parameters stay cheap.)
    if cfg!(test) && master == KdfParams::insecure_for_tests() {
        return master;
    }
    let d = KdfParams::default();
    KdfParams { m_kib: master.m_kib.max(d.m_kib), t: master.t.max(d.t), p: master.p.max(d.p) }
}

/// The phrase's key pair (secret, public).
fn recovery_keypair(
    phrase: &crate::recovery::Phrase,
    salt: &[u8; SALT_LEN],
    params: KdfParams,
) -> VResult<(Zeroizing<[u8; 32]>, [u8; 32])> {
    let mut seed = Zeroizing::new([0u8; KEY_LEN]);
    let p = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|e| VaultError::Corrupt(format!("KDF parameters: {e}")))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(phrase.entropy(), salt, seed.as_mut())
        .map_err(|e| VaultError::Corrupt(format!("KDF: {e}")))?;
    let (sk, pk) = crate::sync::crypto::hpke_derive_keypair(seed.as_ref());
    Ok((Zeroizing::new(sk), pk))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    generation: u64,
    ids: Vec<String>,
    /// The card ids, in file order (format 2); absent without cards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    card_ids: Vec<String>,
    /// SHA-256 of the sealed sync part (format 4), so an older one can't be put back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync: Option<String>,
}

/// What sync keeps inside the vault (PROTOCOL §8), sealed under the vault key: `state` is owned
/// by `core::sync` (the vault X25519 key, PAK by epoch, the PAK pins), `dirty` lists the entries
/// changed since sync last pushed them (the PAK queue, so it never sits outside the vault).
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultSync {
    #[serde(with = "zeroizing_b64")]
    pub state: Zeroizing<Vec<u8>>,
    #[serde(default)]
    pub dirty: Vec<String>,
}

impl std::fmt::Debug for VaultSync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultSync")
            .field("state", &format_args!("<{} bytes>", self.state.len()))
            .field("dirty", &self.dirty.len())
            .finish()
    }
}

mod zeroizing_b64 {
    use base64ct::{Base64, Encoding};
    use serde::{Deserialize, Deserializer, Serializer};
    use zeroize::Zeroizing;

    pub fn serialize<S: Serializer>(v: &Zeroizing<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&Zeroizing::new(Base64::encode_string(v)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Zeroizing<Vec<u8>>, D::Error> {
        let text = Zeroizing::new(String::deserialize(d)?);
        Base64::decode_vec(&text).map(Zeroizing::new).map_err(serde::de::Error::custom)
    }
}

fn sync_hash(sealed: &Sealed) -> String {
    b64(&crate::sync::crypto::sha256(&[sealed.nonce.as_bytes(), b".", sealed.ct.as_bytes()]))
}

/// A decrypted payment card (Krunk 0.6.0). The security code is never stored.
#[derive(Clone)]
pub struct Card {
    pub id: String,
    /// Digits only (12–19).
    pub number: Zeroizing<String>,
    pub exp_month: u8,
    pub exp_year: u16,
    pub name: String,
    pub nickname: String,
    /// Billing address, free text (optional).
    pub address: Zeroizing<String>,
    pub created: i64,
    pub updated: i64,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("id", &self.id)
            .field("number", &"<redacted>")
            .field("nickname", &self.nickname)
            .finish()
    }
}

#[derive(Serialize)]
struct CardOut<'a> {
    number: &'a str,
    exp_month: u8,
    exp_year: u16,
    name: &'a str,
    nickname: &'a str,
    address: &'a str,
    created: i64,
    updated: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardIn {
    number: String,
    exp_month: u8,
    exp_year: u16,
    name: String,
    nickname: String,
    address: String,
    created: i64,
    updated: i64,
}

/// A card as listed (masked: the brand and the last four digits only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSummary {
    pub id: String,
    pub nickname: String,
    pub brand: String,
    pub last4: String,
    pub exp_month: u8,
    pub exp_year: u16,
    pub name: String,
    pub has_address: bool,
}

/// The card brand from the number's leading digits (display only).
pub fn card_brand(number: &str) -> &'static str {
    let starts = |p: &str| number.starts_with(p);
    let in_range = |lo: u32, hi: u32, n: usize| {
        number.get(..n).and_then(|s| s.parse::<u32>().ok()).is_some_and(|v| (lo..=hi).contains(&v))
    };
    if number.is_empty() {
        ""
    } else if starts("4") {
        "Visa"
    } else if in_range(51, 55, 2) || in_range(2221, 2720, 4) {
        "Mastercard"
    } else if starts("34") || starts("37") {
        "American Express"
    } else if starts("6011") || starts("65") || in_range(644, 649, 3) {
        "Discover"
    } else if starts("35") {
        "JCB"
    } else if starts("36") || starts("38") || starts("39") || in_range(300, 305, 3) {
        "Diners Club"
    } else if starts("62") {
        "UnionPay"
    } else {
        "Card"
    }
}

/// Digits only, 12–19 of them, passing the Luhn check (spaces and dashes may separate groups).
pub fn normalize_card_number(text: &str) -> Option<String> {
    if text.chars().any(|c| !(c.is_ascii_digit() || c == ' ' || c == '-')) {
        return None;
    }
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    ((12..=19).contains(&digits.len()) && luhn_ok(&digits)).then_some(digits)
}

pub fn luhn_ok(digits: &str) -> bool {
    let mut sum = 0u32;
    for (i, c) in digits.bytes().rev().enumerate() {
        if !c.is_ascii_digit() {
            return false;
        }
        let mut d = u32::from(c - b'0');
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    !digits.is_empty() && sum.is_multiple_of(10)
}

fn card_aad(id: &str) -> Vec<u8> {
    let mut v = AAD_CARD_PREFIX.to_vec();
    v.extend_from_slice(id.as_bytes());
    v
}

fn validate_card(number: &str, exp_month: u8, exp_year: u16, name: &str, nickname: &str, address: &str) -> VResult<()> {
    if normalize_card_number(number).as_deref() != Some(number) {
        return Err(VaultError::InvalidEntry("card number"));
    }
    if !(1..=12).contains(&exp_month) || !(2000..=2100).contains(&exp_year) {
        return Err(VaultError::InvalidEntry("expiry"));
    }
    if name.chars().count() > 128 || nickname.chars().count() > 64 || address.len() > 2048 {
        return Err(VaultError::InvalidEntry("card text too long"));
    }
    Ok(())
}

/// A decrypted login. Secret fields are wiped on drop.
#[derive(Clone)]
pub struct Entry {
    pub id: String,
    /// Exact origin (`scheme://host[:port]`) the login belongs to.
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub note: Zeroizing<String>,
    pub created: i64,
    pub updated: i64,
}

impl std::fmt::Debug for Entry {
    // Never print secrets, even in debug output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Serialized form of an entry, borrowed so no extra copy of the secret is made.
#[derive(Serialize)]
struct EntryOut<'a> {
    origin: &'a str,
    username: &'a str,
    password: &'a str,
    note: &'a str,
    created: i64,
    updated: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryIn {
    origin: String,
    username: String,
    password: String,
    note: String,
    created: i64,
    updated: i64,
}

/// Entry metadata for listings (no secrets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub has_note: bool,
    pub updated: i64,
}

fn random<const N: usize>() -> VResult<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| VaultError::Random)?;
    Ok(b)
}

pub(crate) fn random_id() -> VResult<String> {
    Ok(random::<16>()?.iter().map(|b| format!("{b:02x}")).collect())
}

fn b64(data: &[u8]) -> String {
    Base64::encode_string(data)
}

fn unb64(s: &str) -> VResult<Vec<u8>> {
    Base64::decode_vec(s).map_err(|_| VaultError::Corrupt("bad base64".into()))
}

/// serde errors can quote input; keep only the class (never decrypted data).
fn json_err(e: serde_json::Error) -> VaultError {
    VaultError::Corrupt(format!("invalid data ({:?})", e.classify()))
}

fn derive_kek(password: &str, salt: &[u8], p: KdfParams) -> VResult<Zeroizing<[u8; KEY_LEN]>> {
    let params = Params::new(p.m_kib, p.t, p.p, Some(KEY_LEN))
        .map_err(|_| VaultError::Corrupt("invalid KDF parameters".into()))?;
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, out.as_mut())
        .map_err(|_| VaultError::Corrupt("key derivation failed".into()))?;
    Ok(out)
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    // Key length is fixed at 32 bytes, so this cannot fail.
    XChaCha20Poly1305::new_from_slice(key).expect("32-byte key")
}

fn seal(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> VResult<Sealed> {
    let nonce: [u8; NONCE_LEN] = random()?;
    let ct = cipher(key)
        .encrypt(&XNonce::from(nonce), Payload { msg: plaintext, aad })
        .map_err(|_| VaultError::Corrupt("encryption failed".into()))?;
    Ok(Sealed { nonce: b64(&nonce), ct: b64(&ct) })
}

fn open_sealed(key: &[u8; KEY_LEN], s: &Sealed, aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let nonce: [u8; NONCE_LEN] = unb64(&s.nonce).ok()?.try_into().ok()?;
    let ct = unb64(&s.ct).ok()?;
    cipher(key).decrypt(&XNonce::from(nonce), Payload { msg: &ct, aad }).ok().map(Zeroizing::new)
}

fn entry_aad(id: &str) -> Vec<u8> {
    [b"lb-vault:entry:".as_slice(), id.as_bytes()].concat()
}

/// Rate limiting for unlock attempts: after `n` consecutive failures the
/// next attempt must wait `min(2^(n-1), 60)` seconds. The host persists it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimiter {
    pub failures: u32,
    pub last_failure_ms: i64,
}

impl RateLimiter {
    pub const MAX_DELAY_MS: u64 = 60_000;

    pub fn delay_after(failures: u32) -> u64 {
        if failures == 0 {
            0
        } else {
            1000u64.saturating_mul(1u64 << (failures - 1).min(16)).min(Self::MAX_DELAY_MS)
        }
    }

    /// Milliseconds the caller still has to wait before trying again.
    pub fn wait_ms(&self, now_ms: i64) -> u64 {
        let delay = Self::delay_after(self.failures);
        let elapsed = u64::try_from(now_ms.saturating_sub(self.last_failure_ms)).unwrap_or(0);
        // A clock that jumped backwards counts as "no time elapsed".
        delay.saturating_sub(elapsed)
    }

    pub fn record_failure(&mut self, now_ms: i64) {
        self.failures = self.failures.saturating_add(1);
        self.last_failure_ms = now_ms;
    }

    pub fn record_success(&mut self) {
        *self = RateLimiter::default();
    }
}

/// Checks a master password against an unlocked vault's key (see
/// [`Vault::verifier`]). Wiped on drop.
#[derive(Clone)]
pub struct Verifier {
    salt: [u8; SALT_LEN],
    params: KdfParams,
    kek: Zeroizing<[u8; KEY_LEN]>,
}

impl Verifier {
    pub fn verify(&self, password: &str) -> bool {
        derive_kek(password, &self.salt, self.params).is_ok_and(|k| {
            // Constant-time comparison of the two derived keys.
            k.iter().zip(self.kek.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
        })
    }
}

/// How a stored login relates to a page (Krunk 0.5.0; since 0.10.1 the same registrable
/// domain only when the page's origin has no logins, see [`Vault::matches_for`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    /// Same scheme, host and port.
    Exact,
    /// Same registrable domain (public-suffix list), both https: `accounts.example.com`
    /// for a login saved on `example.com`. An http page only ever gets exact matches, and
    /// an https entry is never offered to an http page.
    Domain,
}

/// The registrable domain of an origin or URL (`login.bank.example` → `bank.example`),
/// from the host the URL parser normalized (punycode, lower case).
pub fn registrable_domain(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    psl::domain_str(&host).map(str::to_string)
}

/// How the login for `entry_origin` matches a page at `sender_url` (the origin the
/// browser reported for the frame, never one the page claims), or `None`.
pub fn match_kind(entry_origin: &str, sender_url: &str) -> Option<MatchKind> {
    let (a, b) = (origin_of(entry_origin)?, origin_of(sender_url)?);
    let web = |o: &str| o.starts_with("https://") || o.starts_with("http://");
    if !web(&a) || !web(&b) {
        return None;
    }
    if a == b {
        return Some(MatchKind::Exact);
    }
    if !a.starts_with("https://") || !b.starts_with("https://") {
        return None;
    }
    match (registrable_domain(&a), registrable_domain(&b)) {
        (Some(x), Some(y)) if x == y => Some(MatchKind::Domain),
        _ => None,
    }
}

/// May the stored login for `entry_origin` be filled into a page whose document is
/// `sender_url`? Exact origin, or (https only) the same registrable domain; see
/// [`match_kind`]. Hosts are compared after IDNA normalization, so a look-alike
/// Unicode host never matches the ASCII one.
pub fn fill_allowed(entry_origin: &str, sender_url: &str) -> bool {
    match_kind(entry_origin, sender_url).is_some()
}

/// The locked-vault index (Krunk 0.5.0): which sites have logins, without the vault's
/// key. Each entry origin and its registrable domain are stored as `HMAC-SHA256(key, ·)`
/// tags; the key is random and kept DPAPI-wrapped in the database, so the tags reveal
/// nothing offline and no more than the browsing history does on the user's own session.
#[derive(Debug, Clone)]
pub struct HintIndex {
    key: Zeroizing<[u8; 32]>,
}

impl HintIndex {
    pub fn new(key: [u8; 32]) -> HintIndex {
        HintIndex { key: Zeroizing::new(key) }
    }

    pub fn random() -> HintIndex {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).expect("system randomness");
        HintIndex::new(key)
    }

    pub fn key(&self) -> &[u8; 32] {
        &self.key
    }

    fn tag(&self, kind: &str, value: &str) -> String {
        use chacha20poly1305::aead::KeyInit;
        use hmac::{Hmac, Mac};
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(&*self.key).expect("any key length");
        mac.update(kind.as_bytes());
        mac.update(b":");
        mac.update(value.as_bytes());
        mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The tags to store for a set of entry origins: one per origin, one per registrable
    /// domain of an https origin.
    pub fn tags_for<'a>(&self, origins: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        for o in origins {
            if let Some(origin) = origin_of(o) {
                out.insert(self.tag("origin", &origin));
                if origin.starts_with("https://") {
                    if let Some(d) = registrable_domain(&origin) {
                        out.insert(self.tag("domain", &d));
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// Does a page at `url` have logins, by the stored `tags`?
    pub fn has_logins(&self, url: &str, tags: &[String]) -> bool {
        let Some(origin) = origin_of(url) else { return false };
        if tags.contains(&self.tag("origin", &origin)) {
            return true;
        }
        origin.starts_with("https://")
            && registrable_domain(&origin).is_some_and(|d| tags.contains(&self.tag("domain", &d)))
    }
}

/// Is a password prompt raised for (`prompt_tab`, `prompt_origin`) still
/// valid now that the user answered? Only if that tab is still the active
/// tab and still shows the same origin.
pub fn prompt_still_valid(prompt_tab: u32, prompt_origin: &str, active_tab: Option<u32>, active_url: &str) -> bool {
    active_tab == Some(prompt_tab) && fill_allowed(prompt_origin, active_url)
}

/// Is there a vault at `path`?
/// Does the inactivity lock apply now (0.11.0)? Not while the vault was unlocked with "Remain
/// unlocked this session": then only quitting, locking Windows, sleep and "Lock now" lock it.
pub fn autolock_due(unlocked: bool, remain_this_session: bool, idle_ms: i64, minutes: u32) -> bool {
    unlocked && !remain_this_session && idle_ms >= i64::from(minutes.max(1)) * 60_000
}

pub fn exists(path: &Path) -> bool {
    path.is_file()
}

/// When the vault's recovery phrase was made, read without opening the vault (the locked
/// passwords page offers "Forgot master password?" only with one). Krunk 0.7.0.
pub fn recovery_in_file(path: &Path) -> Option<i64> {
    #[derive(Deserialize)]
    struct Peek {
        #[serde(default)]
        recovery: Option<PeekSlot>,
    }
    #[derive(Deserialize)]
    struct PeekSlot {
        created: i64,
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Peek>(&text).ok()?.recovery.map(|r| r.created)
}

/// An unlocked vault. Dropping it locks the vault (all key material and
/// plaintext are wiped).
pub struct Vault {
    path: PathBuf,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    kek: Zeroizing<[u8; KEY_LEN]>,
    vk: Zeroizing<[u8; KEY_LEN]>,
    generation: u64,
    pub(crate) entries: Vec<Entry>,
    cards: Vec<Card>,
    recovery: Option<RecoveryKey>,
    sync: Option<VaultSync>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault").field("path", &self.path).field("entries", &self.entries.len()).finish()
    }
}

impl Vault {
    /// Creates a new, empty vault at `path` (overwrites nothing: fails if it exists).
    pub fn create(path: &Path, password: &str, params: KdfParams, wrapper: &dyn KeyWrapper) -> VResult<Vault> {
        if password.chars().count() < MIN_MASTER_LEN {
            return Err(VaultError::WeakPassword);
        }
        if path.exists() {
            return Err(VaultError::Io("vault already exists".into()));
        }
        let salt = random::<SALT_LEN>()?;
        let mut vault = Vault {
            path: path.to_path_buf(),
            params,
            salt,
            kek: derive_kek(password, &salt, params)?,
            vk: Zeroizing::new(random::<KEY_LEN>()?),
            generation: 0,
            entries: Vec::new(),
            cards: Vec::new(),
            recovery: None,
            sync: None,
        };
        vault.save(wrapper)?;
        Ok(vault)
    }

    /// Unlocks the vault at `path`.
    pub fn open(path: &Path, password: &str, wrapper: &dyn KeyWrapper) -> VResult<Vault> {
        let text = Zeroizing::new(std::fs::read_to_string(path).map_err(|e| VaultError::Io(e.to_string()))?);
        Self::open_str(path, &text, password, wrapper)
    }

    /// The file, checked for what this version reads, and its KDF parameters and salt.
    fn parse_file(text: &str, wrapper: &dyn KeyWrapper) -> VResult<(VaultFile, KdfParams, [u8; SALT_LEN])> {
        let file: VaultFile = serde_json::from_str(text).map_err(json_err)?;
        if ![FORMAT, FORMAT_CARDS, FORMAT_RECOVERY, FORMAT_SYNC].contains(&file.format) || file.kdf.alg != "argon2id" {
            return Err(VaultError::Corrupt("unsupported format".into()));
        }
        if file.wrap != wrapper.name() {
            return Err(VaultError::Wrap(format!("file uses '{}' key wrapping", file.wrap)));
        }
        let params = file.kdf.params;
        if !params.sane() {
            return Err(VaultError::Corrupt("unreasonable KDF parameters".into()));
        }
        let salt: [u8; SALT_LEN] =
            unb64(&file.kdf.salt)?.try_into().map_err(|_| VaultError::Corrupt("salt length".into()))?;
        Ok((file, params, salt))
    }

    fn open_str(path: &Path, text: &str, password: &str, wrapper: &dyn KeyWrapper) -> VResult<Vault> {
        let (file, params, salt) = Self::parse_file(text, wrapper)?;
        let kek = derive_kek(password, &salt, params)?;
        Self::open_parsed(path, file, params, salt, kek, wrapper)
    }

    /// Unlocks the vault at `path` with the key [`Vault::unlock_key`] gave earlier instead of the
    /// master password (no Argon2 run). A key that is not this file's (another vault, or the
    /// password changed since) is `WrongPassword`. Krunk for Android 0.3.0, biometric unlock.
    pub fn open_with_key(path: &Path, key: &[u8; KEY_LEN], wrapper: &dyn KeyWrapper) -> VResult<Vault> {
        let text = Zeroizing::new(std::fs::read_to_string(path).map_err(|e| VaultError::Io(e.to_string()))?);
        let (file, params, salt) = Self::parse_file(&text, wrapper)?;
        Self::open_parsed(path, file, params, salt, Zeroizing::new(*key), wrapper)
    }

    /// A copy of the key the master password derives (the KEK), for [`Vault::open_with_key`].
    /// It opens this vault until the master password changes, so the caller must keep it as
    /// carefully as the password: Android stores it only encrypted under a key-store key that
    /// needs the user's biometric authentication for every use. Wiped on drop.
    pub fn unlock_key(&self) -> Zeroizing<[u8; KEY_LEN]> {
        self.kek.clone()
    }

    /// The parsed file opened with the key the password derives.
    fn open_parsed(
        path: &Path,
        file: VaultFile,
        params: KdfParams,
        salt: [u8; SALT_LEN],
        kek: Zeroizing<[u8; KEY_LEN]>,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<Vault> {
        // Key check: wrong password (or tampered salt/params) fails here.
        let check = open_sealed(&kek, &file.check, AAD_CHECK).ok_or(VaultError::WrongPassword)?;
        if check.as_slice() != CHECK_PLAINTEXT {
            return Err(VaultError::WrongPassword);
        }

        let wrapped = unb64(&file.vault_key)?;
        let inner = wrapper.unprotect(&wrapped).map_err(VaultError::Wrap)?;
        if inner.len() < NONCE_LEN {
            return Err(VaultError::Corrupt("vault key".into()));
        }
        let (nonce, ct) = inner.split_at(NONCE_LEN);
        let sealed_vk = Sealed { nonce: b64(nonce), ct: b64(ct) };
        let vk_bytes = open_sealed(&kek, &sealed_vk, AAD_VAULT_KEY)
            .ok_or_else(|| VaultError::Corrupt("vault key failed authentication".into()))?;
        let mut vk = Zeroizing::new([0u8; KEY_LEN]);
        if vk_bytes.len() != KEY_LEN {
            return Err(VaultError::Corrupt("vault key length".into()));
        }
        vk.copy_from_slice(&vk_bytes);
        Self::with_keys(path, file, params, salt, kek, vk)
    }

    /// The vault from its file once the keys are known: every entry and card authenticated.
    fn with_keys(
        path: &Path,
        file: VaultFile,
        params: KdfParams,
        salt: [u8; SALT_LEN],
        kek: Zeroizing<[u8; KEY_LEN]>,
        vk: Zeroizing<[u8; KEY_LEN]>,
    ) -> VResult<Vault> {
        let manifest_bytes = open_sealed(&vk, &file.manifest, AAD_MANIFEST)
            .ok_or_else(|| VaultError::Corrupt("manifest failed authentication".into()))?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(json_err)?;
        let ids: Vec<&str> = file.entries.iter().map(|e| e.id.as_str()).collect();
        if manifest.ids.iter().map(String::as_str).ne(ids.iter().copied()) {
            return Err(VaultError::Corrupt("entries were added, removed or reordered".into()));
        }
        let card_ids: Vec<&str> = file.cards.iter().map(|c| c.id.as_str()).collect();
        if manifest.card_ids.iter().map(String::as_str).ne(card_ids.iter().copied()) {
            return Err(VaultError::Corrupt("cards were added, removed or reordered".into()));
        }
        let mut cards = Vec::with_capacity(file.cards.len());
        for sc in &file.cards {
            let pt = open_sealed(&vk, &Sealed { nonce: sc.nonce.clone(), ct: sc.ct.clone() }, &card_aad(&sc.id))
                .ok_or_else(|| VaultError::Corrupt("card failed authentication".into()))?;
            let c: CardIn = serde_json::from_slice(&pt).map_err(json_err)?;
            cards.push(Card {
                id: sc.id.clone(),
                number: Zeroizing::new(c.number),
                exp_month: c.exp_month,
                exp_year: c.exp_year,
                name: c.name,
                nickname: c.nickname,
                address: Zeroizing::new(c.address),
                created: c.created,
                updated: c.updated,
            });
        }

        let mut entries = Vec::with_capacity(file.entries.len());
        for se in &file.entries {
            let pt = open_sealed(&vk, &Sealed { nonce: se.nonce.clone(), ct: se.ct.clone() }, &entry_aad(&se.id))
                .ok_or_else(|| VaultError::Corrupt("entry failed authentication".into()))?;
            let e: EntryIn = serde_json::from_slice(&pt).map_err(json_err)?;
            entries.push(Entry {
                id: se.id.clone(),
                origin: e.origin,
                username: e.username,
                password: Zeroizing::new(e.password),
                note: Zeroizing::new(e.note),
                created: e.created,
                updated: e.updated,
            });
        }
        let recovery = match &file.recovery {
            Some(r) => Some(Self::recovery_key(r)?),
            None => None,
        };
        let sync = match (&file.sync, &manifest.sync) {
            (None, None) => None,
            (Some(sealed), Some(hash)) if sync_hash(sealed) == *hash => {
                let pt = open_sealed(&vk, sealed, AAD_SYNC)
                    .ok_or_else(|| VaultError::Corrupt("sync part failed authentication".into()))?;
                Some(serde_json::from_slice::<VaultSync>(&pt).map_err(json_err)?)
            }
            _ => return Err(VaultError::Corrupt("the sync part doesn't match the manifest".into())),
        };
        Ok(Vault {
            path: path.to_path_buf(),
            params,
            salt,
            kek,
            vk,
            generation: manifest.generation,
            entries,
            cards,
            recovery,
            sync,
        })
    }

    fn recovery_key(r: &RecoverySlot) -> VResult<RecoveryKey> {
        if r.kdf.alg != "argon2id" || !r.kdf.params.sane() {
            return Err(VaultError::Corrupt("recovery parameters".into()));
        }
        let salt: [u8; SALT_LEN] =
            unb64(&r.kdf.salt)?.try_into().map_err(|_| VaultError::Corrupt("recovery salt".into()))?;
        let public: [u8; 32] = unb64(&r.public)?.try_into().map_err(|_| VaultError::Corrupt("recovery key".into()))?;
        Ok(RecoveryKey { params: r.kdf.params, salt, public, created: r.created })
    }

    /// Opens the vault at `path` with its recovery phrase and gives it the new master password
    /// `new` (new salt, new vault key; the phrase stays valid). For a forgotten master password.
    pub fn recover(
        path: &Path,
        phrase: &crate::recovery::Phrase,
        new: &str,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<Vault> {
        if new.chars().count() < MIN_MASTER_LEN {
            return Err(VaultError::WeakPassword);
        }
        let text = Zeroizing::new(std::fs::read_to_string(path).map_err(|e| VaultError::Io(e.to_string()))?);
        let (file, params, salt) = Self::parse_file(&text, wrapper)?;
        let slot = file.recovery.clone().ok_or(VaultError::NoRecovery)?;
        let key = Self::recovery_key(&slot)?;
        let (sk, pk) = recovery_keypair(phrase, &key.salt, key.params)?;
        if pk != key.public {
            return Err(VaultError::WrongPhrase);
        }
        let inner = wrapper.unprotect(&unb64(&slot.sealed)?).map_err(VaultError::Wrap)?;
        if inner.len() <= 32 {
            return Err(VaultError::Corrupt("recovery seal".into()));
        }
        let (enc, ct) = inner.split_at(32);
        let sealed = crate::sync::crypto::Sealed {
            enc: enc.try_into().map_err(|_| VaultError::Corrupt("recovery seal".into()))?,
            ct: ct.to_vec(),
        };
        let vk = crate::sync::crypto::open_key(&sk, RECOVERY_INFO, &sealed)
            .map_err(|_| VaultError::Corrupt("recovery seal failed authentication".into()))?;
        // The old KEK is not known: a placeholder until the new password is set below.
        let mut vault = Self::with_keys(path, file, params, salt, Zeroizing::new([0u8; KEY_LEN]), vk)?;
        vault.salt = random::<SALT_LEN>()?;
        vault.kek = derive_kek(new, &vault.salt, vault.params)?;
        vault.vk = Zeroizing::new(random::<KEY_LEN>()?);
        vault.save(wrapper)?;
        Ok(vault)
    }

    /// Makes `phrase` the vault's recovery phrase (a new key pair: an older phrase stops
    /// working). `now` is unix ms.
    pub fn set_recovery(
        &mut self,
        phrase: &crate::recovery::Phrase,
        now: i64,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<()> {
        let params = recovery_params(self.params);
        let salt = random::<SALT_LEN>()?;
        let (_, public) = recovery_keypair(phrase, &salt, params)?;
        let before = self.recovery.replace(RecoveryKey { params, salt, public, created: now });
        if let Err(e) = self.save(wrapper) {
            self.recovery = before;
            return Err(e);
        }
        Ok(())
    }

    /// Removes the recovery phrase (the file goes back to format 1 or 2).
    pub fn remove_recovery(&mut self, wrapper: &dyn KeyWrapper) -> VResult<()> {
        let before = self.recovery.take();
        if let Err(e) = self.save(wrapper) {
            self.recovery = before;
            return Err(e);
        }
        Ok(())
    }

    /// When the recovery phrase was made, if there is one.
    pub fn recovery_created(&self) -> Option<i64> {
        self.recovery.map(|r| r.created)
    }

    /// Re-checks the master password (for revealing a password).
    pub fn verify_password(&self, password: &str) -> bool {
        self.verifier().verify(password)
    }

    /// A `Send` copy of what is needed to check the master password, so the
    /// slow Argon2 run can happen off the UI thread.
    pub fn verifier(&self) -> Verifier {
        Verifier { salt: self.salt, params: self.params, kek: self.kek.clone() }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn summaries(&self) -> Vec<EntrySummary> {
        let mut v: Vec<EntrySummary> = self
            .entries
            .iter()
            .map(|e| EntrySummary {
                id: e.id.clone(),
                origin: e.origin.clone(),
                username: e.username.clone(),
                has_note: !e.note.is_empty(),
                updated: e.updated,
            })
            .collect();
        v.sort_by(|a, b| a.origin.cmp(&b.origin).then_with(|| a.username.cmp(&b.username)));
        v
    }

    /// Logins for a page, matched on the exact origin of `url`.
    pub fn for_origin(&self, url: &str) -> Vec<&Entry> {
        match origin_of(url) {
            Some(o) => self.entries.iter().filter(|e| e.origin == o).collect(),
            None => Vec::new(),
        }
    }

    /// Logins for a page, sorted by username: the page's own origin's, or (only when it has
    /// none) the same registrable domain's (https only). A vault with logins for many
    /// subdomains of one domain must not offer all of them on each of those sites (0.10.1).
    pub fn matches_for(&self, url: &str) -> Vec<(&Entry, MatchKind)> {
        let mut v: Vec<(&Entry, MatchKind)> =
            self.entries.iter().filter_map(|e| match_kind(&e.origin, url).map(|k| (e, k))).collect();
        if v.iter().any(|(_, k)| *k == MatchKind::Exact) {
            v.retain(|(_, k)| *k == MatchKind::Exact);
        }
        v.sort_by(|a, b| a.0.username.cmp(&b.0.username).then_with(|| a.0.origin.cmp(&b.0.origin)));
        v
    }

    /// Adds a login, or updates the password of an existing one with the
    /// same origin and username. Returns the entry id.
    pub fn save_login(
        &mut self,
        url: &str,
        username: &str,
        password: Zeroizing<String>,
        now: i64,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<String> {
        let origin = origin_of(url).ok_or(VaultError::InvalidEntry("origin"))?;
        validate(username, Some(&password), "")?;
        let existing = self.entries.iter().position(|e| e.origin == origin && e.username == username);
        let id = match existing {
            Some(i) => {
                self.entries[i].password = password;
                self.entries[i].updated = now;
                self.entries[i].id.clone()
            }
            None => {
                let id = random_id()?;
                self.entries.push(Entry {
                    id: id.clone(),
                    origin,
                    username: username.to_string(),
                    password,
                    note: Zeroizing::new(String::new()),
                    created: now,
                    updated: now,
                });
                id
            }
        };
        self.touched(&id);
        self.save(wrapper)?;
        Ok(id)
    }

    /// Creates (`id == None`) or edits an entry from the passwords page.
    /// `password == None` keeps the stored password.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert(
        &mut self,
        id: Option<&str>,
        url: &str,
        username: &str,
        password: Option<Zeroizing<String>>,
        note: Option<Zeroizing<String>>,
        now: i64,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<String> {
        let origin = origin_of(url).ok_or(VaultError::InvalidEntry("origin"))?;
        validate(username, password.as_deref().map(String::as_str), note.as_deref().map_or("", String::as_str))?;
        let id = match id {
            Some(id) => {
                let e = self.entries.iter_mut().find(|e| e.id == id).ok_or(VaultError::NotFound)?;
                e.origin = origin;
                e.username = username.to_string();
                if let Some(p) = password {
                    e.password = p;
                }
                if let Some(n) = note {
                    e.note = n;
                }
                e.updated = now;
                id.to_string()
            }
            None => {
                let password = password.ok_or(VaultError::InvalidEntry("password"))?;
                let id = random_id()?;
                self.entries.push(Entry {
                    id: id.clone(),
                    origin,
                    username: username.to_string(),
                    password,
                    note: note.unwrap_or_default(),
                    created: now,
                    updated: now,
                });
                id
            }
        };
        self.touched(&id);
        self.save(wrapper)?;
        Ok(id)
    }

    // ------------------------------------------------------------ cards (0.6.0)

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    pub fn card(&self, id: &str) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == id)
    }

    pub fn card_summaries(&self) -> Vec<CardSummary> {
        self.cards
            .iter()
            .map(|c| CardSummary {
                id: c.id.clone(),
                nickname: c.nickname.clone(),
                brand: card_brand(&c.number).to_string(),
                last4: c.number.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect(),
                exp_month: c.exp_month,
                exp_year: c.exp_year,
                name: c.name.clone(),
                has_address: !c.address.is_empty(),
            })
            .collect()
    }

    /// A card with this number is already stored.
    pub fn has_card_number(&self, number: &str) -> bool {
        self.cards.iter().any(|c| *c.number == *number)
    }

    /// Adds (`id` = `None`) or edits a card; `number`/`address` = `None` keep the stored ones.
    #[allow(clippy::too_many_arguments)]
    pub fn save_card(
        &mut self,
        id: Option<&str>,
        number: Option<Zeroizing<String>>,
        exp_month: u8,
        exp_year: u16,
        name: &str,
        nickname: &str,
        address: Option<Zeroizing<String>>,
        now: i64,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<String> {
        let number = number.map(|n| Zeroizing::new(n.chars().filter(|c| c.is_ascii_digit()).collect::<String>()));
        let id = match id {
            Some(id) => {
                let c = self.cards.iter_mut().find(|c| c.id == id).ok_or(VaultError::NotFound)?;
                let number_ref = number.as_ref().unwrap_or(&c.number);
                let address_ref = address.as_ref().unwrap_or(&c.address);
                validate_card(number_ref, exp_month, exp_year, name, nickname, address_ref)?;
                if let Some(n) = number {
                    c.number = n;
                }
                if let Some(a) = address {
                    c.address = a;
                }
                c.exp_month = exp_month;
                c.exp_year = exp_year;
                c.name = name.to_string();
                c.nickname = nickname.to_string();
                c.updated = now;
                c.id.clone()
            }
            None => {
                let number = number.ok_or(VaultError::InvalidEntry("card number"))?;
                let address = address.unwrap_or_default();
                validate_card(&number, exp_month, exp_year, name, nickname, &address)?;
                let id = random_id()?;
                self.cards.push(Card {
                    id: id.clone(),
                    number,
                    exp_month,
                    exp_year,
                    name: name.to_string(),
                    nickname: nickname.to_string(),
                    address,
                    created: now,
                    updated: now,
                });
                id
            }
        };
        self.save(wrapper)?;
        Ok(id)
    }

    pub fn delete_card(&mut self, id: &str, wrapper: &dyn KeyWrapper) -> VResult<()> {
        let i = self.cards.iter().position(|c| c.id == id).ok_or(VaultError::NotFound)?;
        self.cards.remove(i);
        self.save(wrapper)
    }

    pub fn delete(&mut self, id: &str, wrapper: &dyn KeyWrapper) -> VResult<()> {
        let i = self.entries.iter().position(|e| e.id == id).ok_or(VaultError::NotFound)?;
        self.entries.remove(i);
        self.touched(id);
        self.save(wrapper)
    }

    // ---- Sync (0.9.0) ----------------------------------------------------------------------

    /// While sync keeps state here, an entry change is remembered for the next push.
    fn touched(&mut self, id: &str) {
        if let Some(s) = self.sync.as_mut() {
            if !s.dirty.iter().any(|d| d == id) {
                s.dirty.push(id.to_string());
            }
        }
    }

    /// Sync (0.9.0): what other devices changed, in one write: entries put by id (created or
    /// replaced, keeping their own times), entries removed, and sync's new part. These changes
    /// are not remembered for a push. An entry that fails the usual checks is skipped; the ids
    /// of the skipped ones are returned.
    pub fn sync_apply(
        &mut self,
        puts: Vec<Entry>,
        removes: &[String],
        part: VaultSync,
        wrapper: &dyn KeyWrapper,
    ) -> VResult<Vec<String>> {
        let entries_before = self.entries.clone();
        let sync_before = self.sync.clone();
        let mut skipped = Vec::new();
        for e in puts {
            let id_ok = e.id.len() == 32 && e.id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            let ok = id_ok
                && origin_of(&e.origin).as_deref() == Some(e.origin.as_str())
                && validate(&e.username, Some(&e.password), &e.note).is_ok();
            if !ok {
                skipped.push(e.id.clone());
                continue;
            }
            match self.entries.iter_mut().find(|x| x.id == e.id) {
                Some(x) => *x = e,
                None => self.entries.push(e),
            }
        }
        self.entries.retain(|x| !removes.contains(&x.id));
        self.sync = Some(part);
        if let Err(err) = self.save(wrapper) {
            self.entries = entries_before;
            self.sync = sync_before;
            return Err(err);
        }
        Ok(skipped)
    }

    /// Sync's part of the vault, if sync keeps state here.
    pub fn sync_part(&self) -> Option<&VaultSync> {
        self.sync.as_ref()
    }

    /// Replaces sync's part (or removes it with `None`: the file goes back to format 1–3) and
    /// writes the vault.
    pub fn set_sync_part(&mut self, part: Option<VaultSync>, wrapper: &dyn KeyWrapper) -> VResult<()> {
        let before = std::mem::replace(&mut self.sync, part);
        if let Err(e) = self.save(wrapper) {
            self.sync = before;
            return Err(e);
        }
        Ok(())
    }

    /// Changes the master password: new salt, new KEK *and* a new vault key,
    /// with every entry re-encrypted, written atomically.
    pub fn change_password(&mut self, old: &str, new: &str, wrapper: &dyn KeyWrapper) -> VResult<()> {
        if !self.verify_password(old) {
            return Err(VaultError::WrongPassword);
        }
        if new.chars().count() < MIN_MASTER_LEN {
            return Err(VaultError::WeakPassword);
        }
        let salt = random::<SALT_LEN>()?;
        let params = self.params;
        let kek = derive_kek(new, &salt, params)?;
        let vk = Zeroizing::new(random::<KEY_LEN>()?);
        let old_state = (self.salt, self.params, self.kek.clone(), self.vk.clone());
        self.salt = salt;
        self.params = params;
        self.kek = kek;
        self.vk = vk;
        if let Err(e) = self.save(wrapper) {
            // Keep the in-memory state consistent with the file on disk.
            (self.salt, self.params, self.kek, self.vk) = old_state;
            return Err(e);
        }
        Ok(())
    }

    /// Encrypts everything with fresh nonces and atomically replaces the file.
    pub(crate) fn save(&mut self, wrapper: &dyn KeyWrapper) -> VResult<()> {
        let generation = self.generation + 1;
        let check = seal(&self.kek, CHECK_PLAINTEXT, AAD_CHECK)?;
        let sealed_vk = seal(&self.kek, self.vk.as_ref(), AAD_VAULT_KEY)?;
        let mut inner = Zeroizing::new(unb64(&sealed_vk.nonce)?);
        inner.extend_from_slice(&unb64(&sealed_vk.ct)?);
        let vault_key = b64(&wrapper.protect(&inner).map_err(VaultError::Wrap)?);

        let mut entries = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            let mut pt = Zeroizing::new(Vec::new());
            serde_json::to_writer(
                &mut *pt,
                &EntryOut {
                    origin: &e.origin,
                    username: &e.username,
                    password: &e.password,
                    note: &e.note,
                    created: e.created,
                    updated: e.updated,
                },
            )
            .map_err(|_| VaultError::Corrupt("encoding failed".into()))?;
            let s = seal(&self.vk, &pt, &entry_aad(&e.id))?;
            entries.push(SealedEntry { id: e.id.clone(), nonce: s.nonce, ct: s.ct });
        }
        let mut cards = Vec::with_capacity(self.cards.len());
        for c in &self.cards {
            let mut pt = Zeroizing::new(Vec::new());
            serde_json::to_writer(
                &mut *pt,
                &CardOut {
                    number: &c.number,
                    exp_month: c.exp_month,
                    exp_year: c.exp_year,
                    name: &c.name,
                    nickname: &c.nickname,
                    address: &c.address,
                    created: c.created,
                    updated: c.updated,
                },
            )
            .map_err(|_| VaultError::Corrupt("encoding failed".into()))?;
            let s = seal(&self.vk, &pt, &card_aad(&c.id))?;
            cards.push(SealedEntry { id: c.id.clone(), nonce: s.nonce, ct: s.ct });
        }
        let sync = match &self.sync {
            Some(v) => {
                let pt =
                    Zeroizing::new(serde_json::to_vec(v).map_err(|_| VaultError::Corrupt("encoding failed".into()))?);
                Some(seal(&self.vk, &pt, AAD_SYNC)?)
            }
            None => None,
        };
        let manifest = Manifest {
            generation,
            ids: self.entries.iter().map(|e| e.id.clone()).collect(),
            card_ids: self.cards.iter().map(|c| c.id.clone()).collect(),
            sync: sync.as_ref().map(sync_hash),
        };
        let manifest = seal(
            &self.vk,
            &serde_json::to_vec(&manifest).map_err(|_| VaultError::Corrupt("encoding failed".into()))?,
            AAD_MANIFEST,
        )?;
        let recovery = match &self.recovery {
            Some(r) => {
                let sealed = crate::sync::crypto::seal(&r.public, RECOVERY_INFO, self.vk.as_ref())
                    .map_err(|_| VaultError::Corrupt("recovery key".into()))?;
                let mut inner = Zeroizing::new(sealed.enc.to_vec());
                inner.extend_from_slice(&sealed.ct);
                Some(RecoverySlot {
                    kdf: KdfRecord { alg: "argon2id".into(), params: r.params, salt: b64(&r.salt) },
                    public: b64(&r.public),
                    sealed: b64(&wrapper.protect(&inner).map_err(VaultError::Wrap)?),
                    created: r.created,
                })
            }
            None => None,
        };
        let file = VaultFile {
            format: if sync.is_some() {
                FORMAT_SYNC
            } else if recovery.is_some() {
                FORMAT_RECOVERY
            } else if cards.is_empty() {
                FORMAT
            } else {
                FORMAT_CARDS
            },
            kdf: KdfRecord { alg: "argon2id".into(), params: self.params, salt: b64(&self.salt) },
            wrap: wrapper.name().into(),
            check,
            vault_key,
            manifest,
            entries,
            cards,
            recovery,
            sync,
        };
        let json = serde_json::to_vec_pretty(&file).map_err(|_| VaultError::Corrupt("encoding failed".into()))?;
        write_atomic(&self.path, &json).map_err(|e| VaultError::Io(e.to_string()))?;
        self.generation = generation;
        Ok(())
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        // Entries' secret fields are Zeroizing; also wipe the usernames.
        for e in &mut self.entries {
            e.username.zeroize();
            e.origin.zeroize();
        }
    }
}

fn validate(username: &str, password: Option<&str>, note: &str) -> VResult<()> {
    if username.chars().count() > crate::protocol::MAX_USERNAME_CHARS {
        return Err(VaultError::InvalidEntry("username too long"));
    }
    if let Some(p) = password {
        if p.is_empty() || p.chars().count() > crate::protocol::MAX_PASSWORD_CHARS {
            return Err(VaultError::InvalidEntry("password length"));
        }
    }
    if note.len() > 16 * 1024 {
        return Err(VaultError::InvalidEntry("note too long"));
    }
    Ok(())
}

/// Writes `data` to `path` via a synced temp file and a rename.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remain_unlocked_this_session_skips_the_inactivity_lock() {
        let min = 60_000;
        // Unchecked: today's auto-lock after the set minutes.
        assert!(!autolock_due(true, false, 14 * min, 15));
        assert!(autolock_due(true, false, 15 * min, 15));
        // Checked: never by inactivity, however long.
        assert!(!autolock_due(true, true, 24 * 60 * min, 15));
        // Locked already: nothing to do; a zero setting counts as one minute.
        assert!(!autolock_due(false, false, 99 * min, 15));
        assert!(autolock_due(true, false, min, 0));
    }

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(name: &str) -> TestDir {
            let p = std::env::temp_dir().join(format!("lb-vault-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TestDir(p)
        }
        fn file(&self) -> PathBuf {
            self.0.join("vault.json")
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const PW: &str = "correct horse battery";
    const SECRET: &str = "hunter2-SuperSecret!";

    fn z(s: &str) -> Zeroizing<String> {
        Zeroizing::new(s.to_string())
    }

    fn fast_vault(dir: &TestDir) -> Vault {
        let mut v = Vault::create(&dir.file(), PW, KdfParams::insecure_for_tests(), &NoWrap).unwrap();
        v.save_login("https://bank.example/login", "alice", z(SECRET), 1, &NoWrap).unwrap();
        v.save_login("https://mail.example:8443/", "bob", z("p2"), 2, &NoWrap).unwrap();
        v
    }

    // ------------------------------------------------------------ recovery phrase (0.7.0)

    fn phrase(byte: u8) -> crate::recovery::Phrase {
        crate::recovery::Phrase::from_entropy([byte; 32])
    }

    #[test]
    fn the_recovery_cost_is_at_least_the_masters() {
        let d = KdfParams::default();
        assert_eq!(recovery_params(KdfParams { m_kib: 1024, t: 1, p: 1 }), d);
        let strong = KdfParams { m_kib: 256 * 1024, t: 4, p: 2 };
        assert_eq!(recovery_params(strong), strong);
        assert_eq!(
            recovery_params(KdfParams { m_kib: 128 * 1024, t: 2, p: 1 }),
            KdfParams { m_kib: 128 * 1024, t: 3, p: 1 }
        );
    }

    #[test]
    fn a_recovery_phrase_opens_the_vault_and_sets_a_new_password() {
        let dir = TestDir::new("recovery");
        let mut v = fast_vault(&dir);
        v.save_card(None, Some(z("4111111111111111")), 12, 2030, "A", "", None, 3, &NoWrap).unwrap();
        assert_eq!(v.recovery_created(), None);
        assert_eq!(recovery_in_file(&dir.file()), None);
        v.set_recovery(&phrase(0x42), 77, &NoWrap).unwrap();
        assert_eq!(v.recovery_created(), Some(77));
        assert_eq!(recovery_in_file(&dir.file()), Some(77));
        let text = std::fs::read_to_string(dir.file()).unwrap();
        assert!(
            text.contains("\"format\": 3") && text.contains("\"recovery\"") && text.contains("\"cards\""),
            "format 3, cards kept"
        );
        // Neither the phrase nor its bits are in the file.
        let p = phrase(0x42);
        assert!(!text.contains(&p.words()[0]) && !text.contains(&b64(p.entropy())));
        drop(v);
        // The master password still opens it and sees the slot.
        assert_eq!(Vault::open(&dir.file(), PW, &NoWrap).unwrap().recovery_created(), Some(77));
        // Forgotten password: the phrase opens it with a new one.
        let v = Vault::recover(&dir.file(), &phrase(0x42), "a brand new master", &NoWrap).unwrap();
        assert_eq!(v.entries().len(), 2);
        assert_eq!(v.entries()[0].password.as_str(), SECRET);
        assert_eq!(v.cards().len(), 1);
        assert_eq!(v.recovery_created(), Some(77), "the phrase stays");
        drop(v);
        assert_eq!(
            Vault::open(&dir.file(), PW, &NoWrap).unwrap_err(),
            VaultError::WrongPassword,
            "the old password is gone"
        );
        let v = Vault::open(&dir.file(), "a brand new master", &NoWrap).unwrap();
        assert_eq!(v.entries().len(), 2);
        // The phrase still works after that (a second recovery).
        drop(v);
        assert!(Vault::recover(&dir.file(), &phrase(0x42), "third password!", &NoWrap).is_ok());
        // Short new passwords are refused before anything happens.
        assert_eq!(Vault::recover(&dir.file(), &phrase(0x42), "short", &NoWrap).unwrap_err(), VaultError::WeakPassword);
    }

    #[test]
    fn a_wrong_or_replaced_phrase_fails_and_a_password_change_keeps_it_valid() {
        let dir = TestDir::new("recovery2");
        let mut v = fast_vault(&dir);
        assert_eq!(
            Vault::recover(&dir.file(), &phrase(1), "new password 1", &NoWrap).unwrap_err(),
            VaultError::NoRecovery
        );
        v.set_recovery(&phrase(1), 1, &NoWrap).unwrap();
        assert_eq!(
            Vault::recover(&dir.file(), &phrase(2), "new password 1", &NoWrap).unwrap_err(),
            VaultError::WrongPhrase
        );
        // A master password change rotates the vault key; the phrase still opens it.
        v.change_password(PW, "changed password", &NoWrap).unwrap();
        drop(v);
        let mut v = Vault::recover(&dir.file(), &phrase(1), "after change 1", &NoWrap).unwrap();
        assert_eq!(v.entries().len(), 2);
        // A new phrase: the old one stops working.
        v.set_recovery(&phrase(3), 2, &NoWrap).unwrap();
        drop(v);
        assert_eq!(
            Vault::recover(&dir.file(), &phrase(1), "after change 2", &NoWrap).unwrap_err(),
            VaultError::WrongPhrase
        );
        let mut v = Vault::recover(&dir.file(), &phrase(3), "after change 3", &NoWrap).unwrap();
        // Removed: back to the 0.6.0 layout, readable by older versions.
        v.remove_recovery(&NoWrap).unwrap();
        let text = std::fs::read_to_string(dir.file()).unwrap();
        assert!(text.contains("\"format\": 1") && !text.contains("recovery"));
        drop(v);
        assert_eq!(
            Vault::recover(&dir.file(), &phrase(3), "after change 4", &NoWrap).unwrap_err(),
            VaultError::NoRecovery
        );
        assert!(Vault::open(&dir.file(), "after change 3", &NoWrap).is_ok());
    }

    #[test]
    fn a_tampered_recovery_slot_is_refused() {
        let dir = TestDir::new("recovery3");
        let mut v = fast_vault(&dir);
        v.set_recovery(&phrase(9), 1, &NoWrap).unwrap();
        drop(v);
        let good = std::fs::read_to_string(dir.file()).unwrap();
        let mut file: serde_json::Value = serde_json::from_str(&good).unwrap();
        // Another public key: the phrase no longer matches it.
        let mut other = file.clone();
        other["recovery"]["public"] = serde_json::json!(b64(&[7u8; 32]));
        std::fs::write(dir.file(), serde_json::to_string(&other).unwrap()).unwrap();
        assert_eq!(
            Vault::recover(&dir.file(), &phrase(9), "new password 9", &NoWrap).unwrap_err(),
            VaultError::WrongPhrase
        );
        // A changed seal: fails authentication, nothing is written.
        let sealed = file["recovery"]["sealed"].as_str().unwrap().to_string();
        let mut bytes = unb64(&sealed).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        file["recovery"]["sealed"] = serde_json::json!(b64(&bytes));
        let tampered = serde_json::to_string(&file).unwrap();
        std::fs::write(dir.file(), &tampered).unwrap();
        assert!(matches!(
            Vault::recover(&dir.file(), &phrase(9), "new password 9", &NoWrap),
            Err(VaultError::Corrupt(_))
        ));
        assert_eq!(std::fs::read_to_string(dir.file()).unwrap(), tampered, "untouched");
        // Unknown fields in the slot are refused like everywhere else.
        let mut extra: serde_json::Value = serde_json::from_str(&good).unwrap();
        extra["recovery"]["phrase"] = serde_json::json!("abandon");
        std::fs::write(dir.file(), serde_json::to_string(&extra).unwrap()).unwrap();
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
    }

    #[test]
    fn older_vaults_get_a_recovery_phrase() {
        // A format 1 (0.5.x) and a format 2 (0.6.0) vault: both open, take a phrase (format 3)
        // and recover.
        for cards in [false, true] {
            let dir = TestDir::new(if cards { "recovery-f2" } else { "recovery-f1" });
            let mut v = fast_vault(&dir);
            if cards {
                v.save_card(None, Some(z("5500000000000004")), 1, 2031, "B", "", None, 3, &NoWrap).unwrap();
            }
            drop(v);
            let before = std::fs::read_to_string(dir.file()).unwrap();
            assert!(before.contains(if cards { "\"format\": 2" } else { "\"format\": 1" }));
            let mut v = Vault::open(&dir.file(), PW, &NoWrap).unwrap();
            v.set_recovery(&phrase(5), 1, &NoWrap).unwrap();
            drop(v);
            let v = Vault::recover(&dir.file(), &phrase(5), "migrated password", &NoWrap).unwrap();
            assert_eq!((v.entries().len(), v.cards().len()), (2, usize::from(cards)));
        }
    }

    #[test]
    fn cards_round_trip_and_the_format_stays_put_without_them() {
        let dir = TestDir::new("cards");
        let mut v = fast_vault(&dir);
        let before = std::fs::read_to_string(dir.file()).unwrap();
        assert!(
            before.contains("\"format\": 1") && !before.contains("cards") && !before.contains("cardIds"),
            "no card: the 0.5.2 layout"
        );
        assert_eq!(card_brand("4111111111111111"), "Visa");
        assert_eq!(card_brand("5500000000000004"), "Mastercard");
        assert_eq!(card_brand("378282246310005"), "American Express");
        assert_eq!(card_brand("6011111111111117"), "Discover");
        assert_eq!(normalize_card_number("4111 1111 1111 1111").as_deref(), Some("4111111111111111"));
        assert!(normalize_card_number("4111 1111 1111 1112").is_none(), "Luhn");
        assert!(normalize_card_number("4111x111").is_none() && normalize_card_number("41111111111").is_none());
        let id = v
            .save_card(
                None,
                Some(z("4111 1111 1111 1111")),
                12,
                2030,
                "Alice Example",
                "Visa work",
                Some(z("1 Main St")),
                5,
                &NoWrap,
            )
            .unwrap();
        assert!(v.save_card(None, Some(z("1234")), 12, 2030, "", "", None, 5, &NoWrap).is_err(), "bad number");
        assert!(
            v.save_card(None, Some(z("4111111111111111")), 13, 2030, "", "", None, 5, &NoWrap).is_err(),
            "bad month"
        );
        let after = std::fs::read_to_string(dir.file()).unwrap();
        assert!(after.contains("\"format\": 2") && after.contains("\"cards\""), "format 2 once a card exists");
        assert!(!after.contains("4111") && !after.contains("Alice"), "nothing in clear");
        let s = &v.card_summaries()[0];
        assert_eq!(
            (s.brand.as_str(), s.last4.as_str(), s.exp_month, s.exp_year, s.nickname.as_str(), s.has_address),
            ("Visa", "1111", 12, 2030, "Visa work", true)
        );
        assert!(v.has_card_number("4111111111111111") && !v.has_card_number("5500000000000004"));
        drop(v);
        let mut v2 = Vault::open(&dir.file(), PW, &NoWrap).unwrap();
        assert_eq!(v2.cards().len(), 1);
        assert_eq!(*v2.card(&id).unwrap().number, "4111111111111111");
        assert_eq!(*v2.card(&id).unwrap().address, "1 Main St");
        // Edit keeps the number and address when they are not given.
        v2.save_card(Some(&id), None, 1, 2031, "Alice E.", "Visa", None, 6, &NoWrap).unwrap();
        assert_eq!((*v2.card(&id).unwrap().number).as_str(), "4111111111111111");
        assert_eq!(v2.card(&id).unwrap().exp_year, 2031);
        assert_eq!(v2.entries().len(), 2, "logins untouched");
        v2.delete_card(&id, &NoWrap).unwrap();
        assert!(v2.cards().is_empty());
        let gone = std::fs::read_to_string(dir.file()).unwrap();
        assert!(gone.contains("\"format\": 1"), "back to format 1 without cards");
        // A card list tampered with (a card removed from the file) is caught by the manifest.
        v2.save_card(None, Some(z("5500000000000004")), 3, 2029, "", "", None, 7, &NoWrap).unwrap();
        let text = std::fs::read_to_string(dir.file()).unwrap();
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["cards"] = serde_json::json!([]);
        assert!(matches!(Vault::open_str(&dir.file(), &json.to_string(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
    }

    #[test]
    fn round_trip_with_default_params() {
        let dir = TestDir::new("rt");
        let params = KdfParams::default();
        assert!(params.m_kib >= 64 * 1024 && params.t >= 3 && params.p == 1);
        let mut v = Vault::create(&dir.file(), PW, params, &NoWrap).unwrap();
        let id = v.save_login("https://bank.example/login?x=1", "alice", z(SECRET), 10, &NoWrap).unwrap();
        drop(v);
        let v = Vault::open(&dir.file(), PW, &NoWrap).unwrap();
        let e = v.entry(&id).unwrap();
        assert_eq!(e.origin, "https://bank.example");
        assert_eq!(e.username, "alice");
        assert_eq!(e.password.as_str(), SECRET);
        let text = std::fs::read_to_string(dir.file()).unwrap();
        assert!(text.contains("\"m_kib\": 65536"), "parameters stored with the vault");
    }

    #[test]
    fn wrong_password_rejected() {
        let dir = TestDir::new("wrong");
        drop(fast_vault(&dir));
        assert_eq!(Vault::open(&dir.file(), "wrong password", &NoWrap).unwrap_err(), VaultError::WrongPassword);
        assert_eq!(Vault::open(&dir.file(), "", &NoWrap).unwrap_err(), VaultError::WrongPassword);
        assert!(Vault::open(&dir.file(), PW, &NoWrap).is_ok());
    }

    #[test]
    fn no_plaintext_in_file() {
        let dir = TestDir::new("plain");
        drop(fast_vault(&dir));
        let bytes = std::fs::read(dir.file()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for needle in [SECRET, PW, "alice", "bank.example", "hunter2"] {
            assert!(!text.contains(needle), "{needle} leaked into the vault file");
        }
        assert!(!dir.0.join("vault.tmp").exists(), "temp file cleaned up by rename");
    }

    fn tamper(dir: &TestDir, f: impl FnOnce(&mut serde_json::Value)) {
        let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.file()).unwrap()).unwrap();
        f(&mut v);
        std::fs::write(dir.file(), serde_json::to_vec(&v).unwrap()).unwrap();
    }

    fn flip_b64(s: &str) -> String {
        let mut bytes = Base64::decode_vec(s).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        Base64::encode_string(&bytes)
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let dir = TestDir::new("tamper-ct");
        drop(fast_vault(&dir));
        tamper(&dir, |v| {
            let ct = v["entries"][0]["ct"].as_str().unwrap().to_string();
            v["entries"][0]["ct"] = flip_b64(&ct).into();
        });
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
    }

    #[test]
    fn swapped_or_removed_entries_rejected() {
        let dir = TestDir::new("tamper-swap");
        drop(fast_vault(&dir));
        let orig = std::fs::read(dir.file()).unwrap();
        // Swap the ciphertexts of two entries (AAD binds each to its id).
        tamper(&dir, |v| {
            let a = v["entries"][0].clone();
            let b = v["entries"][1].clone();
            v["entries"][0]["ct"] = b["ct"].clone();
            v["entries"][0]["nonce"] = b["nonce"].clone();
            v["entries"][1]["ct"] = a["ct"].clone();
            v["entries"][1]["nonce"] = a["nonce"].clone();
        });
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
        // Drop an entry (the manifest lists all ids).
        std::fs::write(dir.file(), &orig).unwrap();
        tamper(&dir, |v| {
            v["entries"].as_array_mut().unwrap().pop();
        });
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
    }

    #[test]
    fn tampered_header_rejected() {
        let dir = TestDir::new("tamper-hdr");
        drop(fast_vault(&dir));
        let orig = std::fs::read(dir.file()).unwrap();
        // Salt changed -> different KEK -> key check fails.
        tamper(&dir, |v| {
            let s = v["kdf"]["salt"].as_str().unwrap().to_string();
            v["kdf"]["salt"] = flip_b64(&s).into();
        });
        assert_eq!(Vault::open(&dir.file(), PW, &NoWrap).unwrap_err(), VaultError::WrongPassword);
        // KDF parameters changed.
        std::fs::write(dir.file(), &orig).unwrap();
        tamper(&dir, |v| v["kdf"]["t"] = 2.into());
        assert_eq!(Vault::open(&dir.file(), PW, &NoWrap).unwrap_err(), VaultError::WrongPassword);
        // Absurd parameters are refused before any work is done.
        std::fs::write(dir.file(), &orig).unwrap();
        tamper(&dir, |v| v["kdf"]["m_kib"] = u32::MAX.into());
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
        // Wrapped vault key changed.
        std::fs::write(dir.file(), &orig).unwrap();
        tamper(&dir, |v| {
            let s = v["vaultKey"].as_str().unwrap().to_string();
            v["vaultKey"] = flip_b64(&s).into();
        });
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Corrupt(_))));
        // Check record changed.
        std::fs::write(dir.file(), &orig).unwrap();
        tamper(&dir, |v| {
            let s = v["check"]["ct"].as_str().unwrap().to_string();
            v["check"]["ct"] = flip_b64(&s).into();
        });
        assert_eq!(Vault::open(&dir.file(), PW, &NoWrap).unwrap_err(), VaultError::WrongPassword);
        // Unknown fields / wrapper mismatch.
        std::fs::write(dir.file(), &orig).unwrap();
        tamper(&dir, |v| v["wrap"] = "dpapi".into());
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Wrap(_))));
    }

    #[test]
    fn nonces_are_unique() {
        let dir = TestDir::new("nonces");
        let mut v = fast_vault(&dir);
        // Same plaintext twice must still give different nonces/ciphertexts.
        v.save_login("https://c.example/", "same", z("same"), 3, &NoWrap).unwrap();
        v.save_login("https://d.example/", "same", z("same"), 3, &NoWrap).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..5 {
            v.save(&NoWrap).unwrap();
            let f: VaultFile = serde_json::from_slice(&std::fs::read(dir.file()).unwrap()).unwrap();
            let all =
                f.entries.iter().map(|e| e.nonce.clone()).chain([f.check.nonce, f.manifest.nonce]).collect::<Vec<_>>();
            for n in all {
                assert_eq!(Base64::decode_vec(&n).unwrap().len(), NONCE_LEN);
                assert!(seen.insert(n), "nonce reused");
            }
        }
        assert_eq!(seen.len(), 5 * 6);
    }

    #[test]
    fn master_password_change() {
        let dir = TestDir::new("change");
        let mut v = fast_vault(&dir);
        let before = std::fs::read_to_string(dir.file()).unwrap();
        assert_eq!(v.change_password("nope nope", "new password 123", &NoWrap), Err(VaultError::WrongPassword));
        assert_eq!(v.change_password(PW, "short", &NoWrap), Err(VaultError::WeakPassword));
        assert_eq!(std::fs::read_to_string(dir.file()).unwrap(), before, "failed change leaves file intact");

        v.change_password(PW, "new password 123", &NoWrap).unwrap();
        drop(v);
        let after = std::fs::read_to_string(dir.file()).unwrap();
        let (b, a): (VaultFile, VaultFile) =
            (serde_json::from_str(&before).unwrap(), serde_json::from_str(&after).unwrap());
        assert_ne!(b.kdf.salt, a.kdf.salt, "new salt");
        assert_ne!(b.entries[0].ct, a.entries[0].ct, "entries re-encrypted");
        assert_eq!(Vault::open(&dir.file(), PW, &NoWrap).unwrap_err(), VaultError::WrongPassword);
        let v = Vault::open(&dir.file(), "new password 123", &NoWrap).unwrap();
        assert_eq!(v.entries().len(), 2);
        assert_eq!(v.for_origin("https://bank.example/other")[0].password.as_str(), SECRET);
        assert!(v.verify_password("new password 123"));
        assert!(!v.verify_password(PW));
    }

    #[test]
    fn exact_origin_matching() {
        let dir = TestDir::new("origin");
        let v = fast_vault(&dir);
        assert_eq!(v.for_origin("https://bank.example/anything").len(), 1);
        assert!(v.for_origin("http://bank.example/").is_empty(), "scheme");
        assert!(v.for_origin("https://evil.bank.example/").is_empty(), "subdomain");
        assert!(v.for_origin("https://bank.example.evil.com/").is_empty(), "suffix");
        assert!(v.for_origin("https://mail.example/").is_empty(), "port");
        assert_eq!(v.for_origin("https://mail.example:8443/inbox").len(), 1);
        assert!(v.for_origin("data:text/html,x").is_empty());
    }

    #[test]
    fn edit_update_delete() {
        let dir = TestDir::new("edit");
        let mut v = fast_vault(&dir);
        // Saving the same origin+username updates instead of duplicating.
        let id = v.save_login("https://bank.example/", "alice", z("rotated"), 5, &NoWrap).unwrap();
        assert_eq!(v.entries().len(), 2);
        assert_eq!(v.entry(&id).unwrap().password.as_str(), "rotated");
        v.upsert(Some(&id), "https://bank.example/", "alice2", None, Some(z("note")), 6, &NoWrap).unwrap();
        assert_eq!(v.entry(&id).unwrap().password.as_str(), "rotated", "None keeps password");
        assert_eq!(v.entry(&id).unwrap().note.as_str(), "note");
        // Editing without a note (the page sends null when the field is left empty) keeps it.
        v.upsert(Some(&id), "https://bank.example/", "alice3", None, None, 7, &NoWrap).unwrap();
        assert_eq!(v.entry(&id).unwrap().note.as_str(), "note", "None keeps note");
        assert_eq!(v.entry(&id).unwrap().username, "alice3");
        assert!(v.upsert(None, "not a url", "x", Some(z("p")), None, 7, &NoWrap).is_err());
        assert!(v.upsert(None, "https://x.example/", "x", Some(z("")), None, 7, &NoWrap).is_err());
        v.delete(&id, &NoWrap).unwrap();
        assert_eq!(v.delete(&id, &NoWrap), Err(VaultError::NotFound));
        drop(v);
        let v = Vault::open(&dir.file(), PW, &NoWrap).unwrap();
        assert_eq!(v.summaries().len(), 1);
        assert!(!format!("{:?}", v.entries()).contains("p2"), "Debug redacts passwords");
    }

    #[test]
    fn create_rules() {
        let dir = TestDir::new("create");
        assert_eq!(
            Vault::create(&dir.file(), "short", KdfParams::insecure_for_tests(), &NoWrap).unwrap_err(),
            VaultError::WeakPassword
        );
        drop(fast_vault(&dir));
        assert!(Vault::create(&dir.file(), PW, KdfParams::insecure_for_tests(), &NoWrap).is_err());
    }

    struct XorWrap;
    impl KeyWrapper for XorWrap {
        fn name(&self) -> &'static str {
            "xor-test"
        }
        fn protect(&self, d: &[u8]) -> Result<Vec<u8>, String> {
            Ok(d.iter().map(|b| b ^ 0x5a).collect())
        }
        fn unprotect(&self, d: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            Ok(Zeroizing::new(d.iter().map(|b| b ^ 0x5a).collect()))
        }
    }

    struct FailingWrap;
    impl KeyWrapper for FailingWrap {
        fn name(&self) -> &'static str {
            "xor-test"
        }
        fn protect(&self, _: &[u8]) -> Result<Vec<u8>, String> {
            Err("no".into())
        }
        fn unprotect(&self, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            Err("other machine".into())
        }
    }

    #[test]
    fn key_wrapper_is_required() {
        let dir = TestDir::new("wrap");
        let mut v = Vault::create(&dir.file(), PW, KdfParams::insecure_for_tests(), &XorWrap).unwrap();
        v.save_login("https://a.example/", "u", z("p"), 1, &XorWrap).unwrap();
        drop(v);
        assert!(Vault::open(&dir.file(), PW, &XorWrap).is_ok());
        // Simulates DPAPI failing on another machine: password alone is not enough.
        assert!(matches!(Vault::open(&dir.file(), PW, &FailingWrap), Err(VaultError::Wrap(_))));
        assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Wrap(_))));
    }

    #[test]
    fn fill_requires_exact_origin() {
        let e = "https://bank.example";
        assert!(fill_allowed(e, "https://bank.example/login?next=/"));
        assert!(fill_allowed(e, "https://BANK.example:443/"));
        assert_eq!(
            match_kind(e, "https://bank.example:8443/"),
            Some(MatchKind::Domain),
            "another port: the same site, a domain match"
        );
        assert!(!fill_allowed(e, "http://bank.example/"), "scheme");
        // 0.5: the same registrable domain matches too (https only).
        assert_eq!(match_kind(e, "https://login.bank.example/"), Some(MatchKind::Domain), "subdomain");
        assert_eq!(
            match_kind("https://login.bank.example", "https://bank.example/"),
            Some(MatchKind::Domain),
            "parent domain"
        );
        assert_eq!(match_kind(e, "https://bank.example/x"), Some(MatchKind::Exact));
        assert_eq!(match_kind("http://bank.example", "http://login.bank.example/"), None, "http: exact only");
        assert_eq!(match_kind("http://bank.example", "https://bank.example/"), None, "an http entry never fills https");
        assert_eq!(match_kind("https://a.github.io", "https://b.github.io/"), None, "github.io is a public suffix");
        assert_eq!(match_kind("https://192.168.1.5", "https://192.168.1.6/"), None, "addresses have no domain");
        assert!(!fill_allowed(e, "https://bank.example.evil.com/"), "suffix");
        assert!(!fill_allowed(e, "https://evilbank.example/"), "prefix");
        // Cyrillic 'а' look-alike: normalizes to punycode, never equals the ASCII host.
        assert!(!fill_allowed("https://apple.com", "https://аpple.com/"));
        assert!(!fill_allowed("https://apple.com", "https://xn--pple-43d.com/"));
        assert!(fill_allowed("https://xn--pple-43d.com", "https://аpple.com/"), "same IDN host matches itself");
        // Opaque or non-web origins never match.
        assert!(!fill_allowed("null", "data:text/html,x"));
        assert!(!fill_allowed("file:///C:/x", "file:///C:/x"));
        assert!(!fill_allowed(e, "not a url"));
    }

    #[test]
    fn matches_are_exact_else_domain() {
        let dir = TestDir::new("matches");
        let mut v = Vault::create(&dir.file(), PW, KdfParams::insecure_for_tests(), &NoWrap).unwrap();
        v.save_login("https://accounts.example.com/", "sub-user", z("p"), 1, &NoWrap).unwrap();
        v.save_login("https://example.com/login", "bob", z("p"), 2, &NoWrap).unwrap();
        v.save_login("https://example.com/login", "alice", z("p"), 3, &NoWrap).unwrap();
        v.save_login("https://other.example/", "x", z("p"), 4, &NoWrap).unwrap();
        v.save_login("http://example.com/", "plain", z("p"), 5, &NoWrap).unwrap();
        let m: Vec<(String, MatchKind)> =
            v.matches_for("https://example.com/").into_iter().map(|(e, k)| (e.username.clone(), k)).collect();
        assert_eq!(
            m,
            vec![("alice".into(), MatchKind::Exact), ("bob".into(), MatchKind::Exact)],
            "the page's own logins hide the sister subdomains'"
        );
        // A sister subdomain without logins of its own gets the domain's, never other domains'.
        let sister: Vec<(String, MatchKind)> =
            v.matches_for("https://mail.example.com/").into_iter().map(|(e, k)| (e.username.clone(), k)).collect();
        assert_eq!(
            sister,
            vec![
                ("alice".into(), MatchKind::Domain),
                ("bob".into(), MatchKind::Domain),
                ("sub-user".into(), MatchKind::Domain)
            ]
        );
        let sub: Vec<String> =
            v.matches_for("https://accounts.example.com/").into_iter().map(|(e, _)| e.username.clone()).collect();
        assert_eq!(sub, vec!["sub-user"]);
        let http: Vec<String> =
            v.matches_for("http://example.com/").into_iter().map(|(e, _)| e.username.clone()).collect();
        assert_eq!(http, vec!["plain"], "an http page gets only its exact http entry");
        assert!(v.matches_for("https://evil.example/").is_empty());
    }

    #[test]
    fn hint_index_tags_sites_without_the_vault_key() {
        let idx = HintIndex::new([7u8; 32]);
        let tags = idx.tags_for(["https://accounts.example.com", "http://plain.example"]);
        assert_eq!(tags.len(), 3, "origin + domain for https, origin only for http");
        assert!(tags.iter().all(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit())));
        assert!(idx.has_logins("https://accounts.example.com/login", &tags), "exact origin");
        assert!(idx.has_logins("https://www.example.com/", &tags), "same registrable domain");
        assert!(!idx.has_logins("http://www.example.com/", &tags), "http page: no domain match");
        assert!(idx.has_logins("http://plain.example/", &tags));
        assert!(!idx.has_logins("https://plain.example/", &tags), "the http entry does not count for https");
        assert!(!idx.has_logins("https://other.example/", &tags));
        // Another key gives other tags: a stolen tag list says nothing without the key.
        let other = HintIndex::new([8u8; 32]);
        assert!(!other.has_logins("https://accounts.example.com/", &tags));
        assert_ne!(other.tags_for(["https://accounts.example.com"]), tags);
        assert!(!tags.iter().any(|t| t.contains("example")), "tags are opaque");
        assert_ne!(HintIndex::random().key(), HintIndex::random().key());
    }

    #[test]
    fn prompts_are_revalidated() {
        assert!(prompt_still_valid(3, "https://a.example", Some(3), "https://a.example/next"));
        assert!(!prompt_still_valid(3, "https://a.example", Some(4), "https://a.example/"), "tab switched");
        assert!(!prompt_still_valid(3, "https://a.example", Some(3), "https://b.example/"), "navigated away");
        assert!(!prompt_still_valid(3, "https://a.example", None, "https://a.example/"));
    }

    #[test]
    fn verifier_off_thread() {
        let dir = TestDir::new("verifier");
        let v = fast_vault(&dir);
        let ver = v.verifier();
        let ok = std::thread::spawn(move || (ver.verify(PW), ver.verify("nope"))).join().unwrap();
        assert_eq!(ok, (true, false));
    }

    #[test]
    fn failed_password_change_keeps_old_vault() {
        let dir = TestDir::new("change-fail");
        let mut v = Vault::create(&dir.file(), PW, KdfParams::insecure_for_tests(), &XorWrap).unwrap();
        v.save_login("https://a.example/", "alice", z(SECRET), 1, &XorWrap).unwrap();
        let before = std::fs::read(dir.file()).unwrap();

        // 1. Failure before anything is written (key wrapping fails).
        assert!(matches!(v.change_password(PW, "new password 1", &FailingWrap), Err(VaultError::Wrap(_))));
        assert_eq!(std::fs::read(dir.file()).unwrap(), before, "file untouched");
        assert!(v.verify_password(PW), "in-memory key reverted");

        // 2. Failure while writing: the temp file cannot be created.
        std::fs::create_dir(dir.0.join("vault.tmp")).unwrap();
        assert!(matches!(v.change_password(PW, "new password 2", &XorWrap), Err(VaultError::Io(_))));
        assert_eq!(std::fs::read(dir.file()).unwrap(), before, "file untouched");
        assert!(v.verify_password(PW));
        std::fs::remove_dir(dir.0.join("vault.tmp")).unwrap();

        // The old vault still opens with the old password and its data.
        drop(v);
        let v = Vault::open(&dir.file(), PW, &XorWrap).unwrap();
        assert_eq!(v.for_origin("https://a.example/")[0].password.as_str(), SECRET);
        assert!(Vault::open(&dir.file(), "new password 1", &XorWrap).is_err());
    }

    #[test]
    fn file_never_contains_plaintext_after_edits() {
        let dir = TestDir::new("leak");
        let mut v = fast_vault(&dir);
        let id = v.save_login("https://shop.example/", "carol@example.org", z("Correct-Horse-42"), 3, &NoWrap).unwrap();
        v.upsert(
            Some(&id),
            "https://shop.example/",
            "carol@example.org",
            None,
            Some(z("recovery code 9911")),
            4,
            &NoWrap,
        )
        .unwrap();
        v.change_password(PW, "another master pw", &NoWrap).unwrap();
        let bytes = std::fs::read(dir.file()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for needle in
            ["carol", "Correct-Horse-42", "recovery code", "shop.example", "another master pw", PW, "alice", SECRET]
        {
            assert!(!text.contains(needle), "{needle} found in vault file");
        }
    }

    #[test]
    fn rate_limiter_backoff() {
        let mut r = RateLimiter::default();
        assert_eq!(r.wait_ms(0), 0);
        r.record_failure(1_000);
        assert_eq!(r.wait_ms(1_000), 1_000);
        assert_eq!(r.wait_ms(1_600), 400);
        assert_eq!(r.wait_ms(2_000), 0);
        r.record_failure(2_000);
        assert_eq!(r.wait_ms(2_000), 2_000);
        for _ in 0..3 {
            r.record_failure(10_000);
        }
        assert_eq!(r.wait_ms(10_000), 16_000);
        for _ in 0..40 {
            r.record_failure(20_000);
        }
        assert_eq!(r.wait_ms(20_000), RateLimiter::MAX_DELAY_MS);
        // Clock going backwards does not bypass the delay.
        assert_eq!(r.wait_ms(0), RateLimiter::MAX_DELAY_MS);
        r.record_success();
        assert_eq!(r.wait_ms(20_000), 0);
        // Strictly increasing up to the cap.
        let delays: Vec<u64> = (1..=8).map(RateLimiter::delay_after).collect();
        assert!(delays.windows(2).all(|w| w[0] < w[1] || w[1] == RateLimiter::MAX_DELAY_MS));
    }

    // ------------------------------------------------- unlock key (Android 0.3.0, biometric)

    #[test]
    fn the_unlock_key_reopens_the_vault_until_the_password_changes() {
        let dir = TestDir::new("unlock-key");
        let mut v = fast_vault(&dir);
        let key = v.unlock_key();
        let again = Vault::open_with_key(&dir.file(), &key, &NoWrap).unwrap();
        assert_eq!(again.entries().len(), 2);
        assert_eq!(again.for_origin("https://bank.example/")[0].password.as_str(), SECRET);
        assert!(again.verify_password(PW), "the reopened vault has the same key");
        assert_eq!(*again.unlock_key(), *key);
        drop(again);

        // A wrong key is a wrong password, and leaves the file alone.
        let before = std::fs::read(dir.file()).unwrap();
        let mut wrong = *key;
        wrong[0] ^= 1;
        assert_eq!(Vault::open_with_key(&dir.file(), &wrong, &NoWrap).unwrap_err(), VaultError::WrongPassword);
        assert_eq!(Vault::open_with_key(&dir.file(), &[0u8; 32], &NoWrap).unwrap_err(), VaultError::WrongPassword);
        assert_eq!(std::fs::read(dir.file()).unwrap(), before);

        // A vault opened by key can be changed and saved like any other.
        let mut by_key = Vault::open_with_key(&dir.file(), &key, &NoWrap).unwrap();
        by_key.save_login("https://new.example/", "carol", z("x-pass-1"), 5, &NoWrap).unwrap();
        drop(by_key);
        assert_eq!(Vault::open(&dir.file(), PW, &NoWrap).unwrap().entries().len(), 3);

        // After a password change the old key no longer opens the vault; the new one does.
        v.change_password(PW, "new password 123", &NoWrap).unwrap();
        assert_eq!(Vault::open_with_key(&dir.file(), &key, &NoWrap).unwrap_err(), VaultError::WrongPassword);
        let new_key = v.unlock_key();
        assert_ne!(*new_key, *key);
        assert_eq!(Vault::open_with_key(&dir.file(), &new_key, &NoWrap).unwrap().entries().len(), 2);
    }

    #[test]
    fn the_unlock_key_path_still_needs_the_key_wrapper() {
        struct Failing;
        impl KeyWrapper for Failing {
            fn name(&self) -> &'static str {
                "none"
            }
            fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
                Ok(data.to_vec())
            }
            fn unprotect(&self, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
                Err("key gone".into())
            }
        }
        let dir = TestDir::new("unlock-key-wrap");
        let key = fast_vault(&dir).unlock_key();
        assert_eq!(Vault::open_with_key(&dir.file(), &key, &Failing).unwrap_err(), VaultError::Wrap("key gone".into()));
    }

    // ---------------------------------------------------------------- format 4 (0.9.0, S5)

    fn fixture(f: u32, dir: &TestDir) -> PathBuf {
        let src =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/vault/vault-format{f}.json"));
        std::fs::copy(src, dir.file()).unwrap();
        dir.file()
    }
    const FIXTURE_PW: &str = "fixture password";

    fn format_of(path: &Path) -> u64 {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        v["format"].as_u64().unwrap()
    }

    fn part(bytes: &[u8]) -> VaultSync {
        VaultSync { state: Zeroizing::new(bytes.to_vec()), dirty: Vec::new() }
    }

    #[test]
    fn frozen_0_8_0_vaults_open_and_keep_their_format() {
        for f in [1u32, 2, 3] {
            let dir = TestDir::new(&format!("fixture-f{f}"));
            let path = fixture(f, &dir);
            let mut v = Vault::open(&path, FIXTURE_PW, &NoWrap).unwrap();
            let users: Vec<&str> = v.entries().iter().map(|e| e.username.as_str()).collect();
            assert_eq!(users, ["alice", "bob"], "format {f}");
            assert_eq!(&*v.entries()[0].password, "s3cret-one");
            assert_eq!(v.cards().len(), usize::from(f >= 2));
            assert_eq!(v.recovery_created().is_some(), f >= 3);
            assert!(v.sync_part().is_none());
            // A change without sync keeps the format (0.8.0 still reads the file).
            v.save_login("https://new.example/", "carol", z("x-pass-1"), 5, &NoWrap).unwrap();
            assert_eq!(format_of(&path), u64::from(f));
            assert!(!std::fs::read_to_string(&path).unwrap().contains("\"sync\""));
        }
    }

    #[test]
    fn the_sync_part_round_trips_tracks_changes_and_goes_away() {
        let dir = TestDir::new("sync-part");
        let path = fixture(3, &dir);
        let mut v = Vault::open(&path, FIXTURE_PW, &NoWrap).unwrap();
        v.set_sync_part(Some(part(b"pak state")), &NoWrap).unwrap();
        assert_eq!(format_of(&path), 4);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("pak state") && !text.contains(&Base64::encode_string(b"pak state")), "sealed");
        // Changes while sync keeps state are remembered (the PAK queue lives in the vault).
        let alice = v.entries()[0].id.clone();
        v.save_login("https://bank.example/", "alice", z("changed"), 9, &NoWrap).unwrap();
        let new = v.upsert(None, "https://shop.example/", "dave", Some(z("dave-pass")), None, 10, &NoWrap).unwrap();
        let bob = v.entries()[1].id.clone();
        v.delete(&bob, &NoWrap).unwrap();
        v.save_login("https://bank.example/", "alice", z("changed again"), 11, &NoWrap).unwrap();
        drop(v);
        let mut v = Vault::open(&path, FIXTURE_PW, &NoWrap).unwrap();
        let p = v.sync_part().unwrap().clone();
        assert_eq!(&*p.state, b"pak state");
        assert_eq!(p.dirty, vec![alice.clone(), new.clone(), bob.clone()], "each id once, in order");
        assert_eq!(v.recovery_created(), Some(1_793_000_300_000), "the recovery slot stays");
        // Turning sync off removes the part: back to format 3.
        v.set_sync_part(None, &NoWrap).unwrap();
        assert_eq!(format_of(&path), 3);
        drop(v);
        let mut v = Vault::open(&path, FIXTURE_PW, &NoWrap).unwrap();
        assert!(v.sync_part().is_none());
        v.save_login("https://x.example/", "x", z("x-pass-1"), 12, &NoWrap).unwrap();
        assert!(v.sync_part().is_none(), "no tracking without sync");
        assert_eq!(format_of(&path), 3);
    }

    #[test]
    fn a_swapped_or_missing_sync_part_is_refused() {
        let dir = TestDir::new("sync-swap");
        let path = fixture(1, &dir);
        let mut v = Vault::open(&path, FIXTURE_PW, &NoWrap).unwrap();
        v.set_sync_part(Some(part(b"old")), &NoWrap).unwrap();
        let old: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v.set_sync_part(Some(part(b"new")), &NoWrap).unwrap();
        drop(v);
        let good = std::fs::read(&path).unwrap();
        // An older sync part put back into the newer file.
        tamper(&dir, |f| f["sync"] = old["sync"].clone());
        assert!(matches!(Vault::open(&path, FIXTURE_PW, &NoWrap), Err(VaultError::Corrupt(_))));
        // The part removed (the manifest still names it).
        std::fs::write(&path, &good).unwrap();
        tamper(&dir, |f| {
            f.as_object_mut().unwrap().remove("sync");
            f["format"] = 1.into();
        });
        assert!(matches!(Vault::open(&path, FIXTURE_PW, &NoWrap), Err(VaultError::Corrupt(_))));
        // Its ciphertext changed.
        std::fs::write(&path, &good).unwrap();
        tamper(&dir, |f| {
            let ct = f["sync"]["ct"].as_str().unwrap().to_string();
            f["sync"]["ct"] = flip_b64(&ct).into();
        });
        assert!(matches!(Vault::open(&path, FIXTURE_PW, &NoWrap), Err(VaultError::Corrupt(_))));
        // A part added to a file whose manifest has none.
        let dir2 = TestDir::new("sync-add");
        let p2 = fixture(1, &dir2);
        tamper(&dir2, |f| f["sync"] = old["sync"].clone());
        assert!(matches!(Vault::open(&p2, FIXTURE_PW, &NoWrap), Err(VaultError::Corrupt(_))));
        std::fs::write(&path, &good).unwrap();
        assert_eq!(&*Vault::open(&path, FIXTURE_PW, &NoWrap).unwrap().sync_part().unwrap().state, b"new");
    }
}
