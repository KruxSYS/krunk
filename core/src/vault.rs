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
//! ```
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
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const SALT_LEN: usize = 16;
// The browser's name before the rename to Krunk. Part of the vault format:
// changing it would make every existing vault fail to unlock.
const CHECK_PLAINTEXT: &[u8] = b"LightBrowser vault key check v1";
const AAD_CHECK: &[u8] = b"lb-vault:check";
const AAD_VAULT_KEY: &[u8] = b"lb-vault:vault-key";
const AAD_MANIFEST: &[u8] = b"lb-vault:manifest";

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
        }
    }
}

impl std::error::Error for VaultError {}

type VResult<T> = Result<T, VaultError>;

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
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    generation: u64,
    ids: Vec<String>,
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

fn random_id() -> VResult<String> {
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

/// May the stored login for `entry_origin` be filled into a page whose
/// top-level document is `sender_url`? Only on an exact origin match
/// (scheme, host and port), where `sender_url` must be the origin the
/// browser reported for the message, never a URL the page claims.
/// Hosts are compared after IDNA normalization, so a look-alike Unicode
/// host never matches the ASCII one.
pub fn fill_allowed(entry_origin: &str, sender_url: &str) -> bool {
    match (origin_of(entry_origin), origin_of(sender_url)) {
        (Some(a), Some(b)) => a == b && (a.starts_with("https://") || a.starts_with("http://")),
        _ => false,
    }
}

/// Is a password prompt raised for (`prompt_tab`, `prompt_origin`) still
/// valid now that the user answered? Only if that tab is still the active
/// tab and still shows the same origin.
pub fn prompt_still_valid(prompt_tab: u32, prompt_origin: &str, active_tab: Option<u32>, active_url: &str) -> bool {
    active_tab == Some(prompt_tab) && fill_allowed(prompt_origin, active_url)
}

/// Is there a vault at `path`?
pub fn exists(path: &Path) -> bool {
    path.is_file()
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
    entries: Vec<Entry>,
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
        };
        vault.save(wrapper)?;
        Ok(vault)
    }

    /// Unlocks the vault at `path`.
    pub fn open(path: &Path, password: &str, wrapper: &dyn KeyWrapper) -> VResult<Vault> {
        let text = Zeroizing::new(std::fs::read_to_string(path).map_err(|e| VaultError::Io(e.to_string()))?);
        Self::open_str(path, &text, password, wrapper)
    }

    fn open_str(path: &Path, text: &str, password: &str, wrapper: &dyn KeyWrapper) -> VResult<Vault> {
        let file: VaultFile = serde_json::from_str(text).map_err(json_err)?;
        if file.format != FORMAT || file.kdf.alg != "argon2id" {
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

        let kek = derive_kek(password, &salt, params)?;
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

        let manifest_bytes = open_sealed(&vk, &file.manifest, AAD_MANIFEST)
            .ok_or_else(|| VaultError::Corrupt("manifest failed authentication".into()))?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(json_err)?;
        let ids: Vec<&str> = file.entries.iter().map(|e| e.id.as_str()).collect();
        if manifest.ids.iter().map(String::as_str).ne(ids.iter().copied()) {
            return Err(VaultError::Corrupt("entries were added, removed or reordered".into()));
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
        Ok(Vault { path: path.to_path_buf(), params, salt, kek, vk, generation: manifest.generation, entries })
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
        self.save(wrapper)?;
        Ok(id)
    }

    pub fn delete(&mut self, id: &str, wrapper: &dyn KeyWrapper) -> VResult<()> {
        let i = self.entries.iter().position(|e| e.id == id).ok_or(VaultError::NotFound)?;
        self.entries.remove(i);
        self.save(wrapper)
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
    fn save(&mut self, wrapper: &dyn KeyWrapper) -> VResult<()> {
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
        let manifest = Manifest { generation, ids: self.entries.iter().map(|e| e.id.clone()).collect() };
        let manifest = seal(
            &self.vk,
            &serde_json::to_vec(&manifest).map_err(|_| VaultError::Corrupt("encoding failed".into()))?,
            AAD_MANIFEST,
        )?;
        let file = VaultFile {
            format: FORMAT,
            kdf: KdfRecord { alg: "argon2id".into(), params: self.params, salt: b64(&self.salt) },
            wrap: wrapper.name().into(),
            check,
            vault_key,
            manifest,
            entries,
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
        assert!(!fill_allowed(e, "https://bank.example:8443/"), "port");
        assert!(!fill_allowed(e, "http://bank.example/"), "scheme");
        assert!(!fill_allowed(e, "https://login.bank.example/"), "subdomain");
        assert!(!fill_allowed("https://login.bank.example", "https://bank.example/"), "parent domain");
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
}
