//! The password vault (`core::vault`) for Android: the same file format and the same code as
//! the Windows browser, with the Android Keystore as the key wrapper (Kotlin implements
//! [`VaultKeyWrapper`]). Nothing here encrypts anything itself.
//!
//! Two files in the vault folder: `vault.json` (the core's format, untouched) and `state.json`
//! (this module's side file: the unlock rate limiter, the wrapped key and the tags of the
//! locked-vault hint index, and the opaque blob of the biometric unlock). `state.json` holds no
//! secret in the clear.
//!
//! What crosses the boundary: the vault password and entry passwords arrive as UniFFI strings
//! and are moved into `Zeroizing` at once (the transient UniFFI buffer is freed, not wiped);
//! entry passwords go back to Kotlin only from [`KrunkVault::reveal`] and
//! [`KrunkVault::secret_for_fill`]. Lists never carry passwords.
use base64ct::{Base64, Encoding};
use lb_core::urlclean::origin_of;
use lb_core::vault::{self as core_vault, HintIndex, KdfParams, KeyWrapper, RateLimiter, Vault, VaultError};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use zeroize::Zeroizing;

/// The wrapper name written into the vault file's `wrap` field on Android.
const ANDROID_WRAP_NAME: &str = "android-keystore";
const VAULT_FILE: &str = "vault.json";
const STATE_FILE: &str = "state.json";

// ------------------------------------------------------------------------------ key wrapper

/// How a wrapper failure that says nothing about the key itself starts (see [`WrapError::Failed`]
/// and `VaultFfiError::KeyUnavailable`): the core carries a wrapper's failure as text only.
const WRAP_FAILED: &str = "device key failed: ";

/// Why the device key could not wrap or unwrap.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum WrapError {
    /// The key-store key does not exist (app data restored from elsewhere, key store reset).
    KeyMissing { detail: String },
    /// The key exists but can no longer be used.
    KeyInvalidated { detail: String },
    /// Anything else (bad blob, key-store failure).
    Failed { detail: String },
}

impl fmt::Display for WrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WrapError::KeyMissing { detail } => write!(f, "device key missing: {detail}"),
            WrapError::KeyInvalidated { detail } => write!(f, "device key invalidated: {detail}"),
            WrapError::Failed { detail } => write!(f, "{WRAP_FAILED}{detail}"),
        }
    }
}

impl std::error::Error for WrapError {}

/// An exception the Kotlin implementation did not declare (UniFFI turns it into this).
impl From<uniffi::UnexpectedUniFFICallbackError> for WrapError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        WrapError::Failed { detail: e.reason }
    }
}

/// The device's key wrapper, implemented in Kotlin over the Android Keystore. It wraps the
/// vault key, which the master password has already sealed, so a copied vault file is useless
/// on another device; and the key of the locked-vault hint index.
#[uniffi::export(with_foreign)]
pub trait VaultKeyWrapper: Send + Sync {
    fn protect(&self, data: Vec<u8>) -> Result<Vec<u8>, WrapError>;
    fn unprotect(&self, data: Vec<u8>) -> Result<Vec<u8>, WrapError>;
}

/// [`VaultKeyWrapper`] as the core's `KeyWrapper`.
struct ForeignWrapper(Arc<dyn VaultKeyWrapper>);

impl KeyWrapper for ForeignWrapper {
    fn name(&self) -> &'static str {
        ANDROID_WRAP_NAME
    }
    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        // The copy handed over is consumed by the UniFFI call (lowered and freed there).
        self.0.protect(data.to_vec()).map_err(|e| e.to_string())
    }
    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        self.0.unprotect(data.to_vec()).map(Zeroizing::new).map_err(|e| e.to_string())
    }
}

type BoxedWrapper = Box<dyn KeyWrapper + Send + Sync>;

// ------------------------------------------------------------------------------ errors

/// Vault failures as Kotlin sees them (`VaultFfiException.WrongPassword` etc.).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum VaultFfiError {
    /// The password (or the biometric unlock key) does not open the vault.
    WrongPassword,
    /// Too many failed attempts; nothing was tried. Ask again after `wait_ms`.
    RateLimited {
        wait_ms: u64,
    },
    /// The new vault password is shorter than [`vault_min_password_len`].
    WeakPassword,
    /// The device key cannot unwrap this vault (the file is from another device, or the key
    /// store lost the key). The vault file is left as it is.
    KeyLost {
        detail: String,
    },
    /// The device's key store failed without saying that the key is gone (it did not answer,
    /// or the wrapped key did not verify). Nothing is known about the vault: try again.
    KeyUnavailable {
        detail: String,
    },
    /// The vault file failed authentication or does not parse.
    Corrupt {
        detail: String,
    },
    /// The vault is locked.
    Locked,
    /// There is no vault yet.
    NoVault,
    /// `create` found a vault file.
    AlreadyExists,
    /// No entry with this id.
    NotFound,
    /// The entry does not belong to the page it was asked for.
    NotAllowed,
    InvalidEntry {
        detail: String,
    },
    Io {
        detail: String,
    },
}

impl fmt::Display for VaultFfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultFfiError::WrongPassword => write!(f, "wrong vault password"),
            VaultFfiError::RateLimited { wait_ms } => {
                write!(f, "too many attempts; try again in {} s", wait_ms.div_ceil(1000))
            }
            VaultFfiError::WeakPassword => {
                write!(f, "the vault password must be at least {} characters", core_vault::MIN_MASTER_LEN)
            }
            VaultFfiError::KeyLost { detail } => write!(f, "this device's vault key is not usable: {detail}"),
            VaultFfiError::KeyUnavailable { detail } => write!(f, "this device's key store did not work: {detail}"),
            VaultFfiError::Corrupt { detail } => write!(f, "the vault file is damaged: {detail}"),
            VaultFfiError::Locked => write!(f, "the vault is locked"),
            VaultFfiError::NoVault => write!(f, "there is no vault"),
            VaultFfiError::AlreadyExists => write!(f, "a vault already exists"),
            VaultFfiError::NotFound => write!(f, "entry not found"),
            VaultFfiError::NotAllowed => write!(f, "this login does not belong to that page"),
            VaultFfiError::InvalidEntry { detail } => write!(f, "invalid entry: {detail}"),
            VaultFfiError::Io { detail } => write!(f, "vault I/O error: {detail}"),
        }
    }
}

impl std::error::Error for VaultFfiError {}

impl From<VaultError> for VaultFfiError {
    fn from(e: VaultError) -> Self {
        match e {
            VaultError::WrongPassword => VaultFfiError::WrongPassword,
            VaultError::RateLimited { wait_ms } => VaultFfiError::RateLimited { wait_ms },
            VaultError::Corrupt(detail) => VaultFfiError::Corrupt { detail },
            // A key store that failed without saying the key is gone may work at the next try:
            // that must never be shown as "this vault cannot be opened, delete it".
            VaultError::Wrap(detail) if detail.starts_with(WRAP_FAILED) => VaultFfiError::KeyUnavailable { detail },
            VaultError::Wrap(detail) => VaultFfiError::KeyLost { detail },
            VaultError::WeakPassword => VaultFfiError::WeakPassword,
            VaultError::InvalidEntry(detail) => VaultFfiError::InvalidEntry { detail: detail.to_string() },
            VaultError::NotFound => VaultFfiError::NotFound,
            VaultError::Io(detail) => VaultFfiError::Io { detail },
            // Not reachable from this module (no recovery phrase on Android yet); never a panic.
            other @ (VaultError::Random | VaultError::WrongPhrase | VaultError::NoRecovery) => {
                VaultFfiError::Io { detail: other.to_string() }
            }
        }
    }
}

type VaultResult<T> = Result<T, VaultFfiError>;

// ------------------------------------------------------------------------------ records

/// Is there a vault, and is it open?
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VaultState {
    /// No vault file.
    None,
    Locked,
    Unlocked,
}

/// A login as listed (no secret).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VaultEntrySummary {
    pub id: String,
    /// `scheme://host[:port]`.
    pub origin: String,
    pub username: String,
    pub has_note: bool,
    pub created: i64,
    pub updated: i64,
}

/// A login as the view screen shows it; the password comes from [`KrunkVault::reveal`].
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct VaultEntryDetail {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub note: String,
    pub created: i64,
    pub updated: i64,
}

impl fmt::Debug for VaultEntryDetail {
    // The note is the user's free text and may hold secrets: never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultEntryDetail")
            .field("id", &self.id)
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("note", &"<redacted>")
            .finish()
    }
}

/// A login offered for a page (no secret).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VaultMatch {
    pub id: String,
    pub origin: String,
    pub username: String,
    /// The page's own origin; `false` = another host of the same registrable domain (https).
    pub exact: bool,
}

/// What [`KrunkVault::secret_for_fill`] hands over for one fill.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct VaultSecret {
    pub username: String,
    pub password: String,
}

impl fmt::Debug for VaultSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultSecret").field("username", &self.username).field("password", &"<redacted>").finish()
    }
}

/// What a submitted login means for the vault (the save prompt).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SaveDecision {
    /// No login with this user name on the page's origin.
    New,
    /// One exists with another password.
    Update { id: String },
    /// Already saved with this password: nothing to ask.
    Unchanged,
}

/// The shortest vault password `create` and `change_password` accept (characters).
#[uniffi::export]
pub fn vault_min_password_len() -> u32 {
    u32::try_from(core_vault::MIN_MASTER_LEN).unwrap_or(u32::MAX)
}

// ------------------------------------------------------------------------------ state.json

/// The side file. Unknown or damaged content reads as empty: every part of it can be rebuilt
/// (the hints at the next unlock, the biometric blob by switching the setting on again).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StateFile {
    rate: RateLimiter,
    /// `wrapper.protect(hint key)`, base64.
    #[serde(skip_serializing_if = "Option::is_none")]
    hint_key: Option<String>,
    /// `HintIndex::tags_for` over every entry origin.
    hint_tags: Vec<String>,
    /// Kotlin's biometric blob (the unlock key encrypted under the biometric key), base64.
    #[serde(skip_serializing_if = "Option::is_none")]
    bio: Option<String>,
}

struct Inner {
    /// The open vault; `None` = locked (dropping it wipes the keys and the entries).
    vault: Option<Vault>,
    state: StateFile,
    /// The unwrapped hint key, once it was needed.
    hints: Option<HintIndex>,
    /// The wrapper was already asked for the hint key (asked once per lock state, not per page).
    hints_tried: bool,
    /// When the vault was last used (ms, the caller's clock).
    last_used: i64,
    /// Used since `last_used` by a call that carries no time.
    used_since: bool,
    /// How often `lock` was called (see `opened`).
    locks: u64,
}

/// The password vault. One per app; methods take `&self`, the lock is internal.
#[derive(uniffi::Object)]
pub struct KrunkVault {
    dir: PathBuf,
    wrapper: BoxedWrapper,
    /// The cost of a new vault (an existing one carries its own).
    params: KdfParams,
    inner: Mutex<Inner>,
    /// Held by the slow operations that replace the open vault (unlock, create, destroy), so
    /// two unlock attempts cannot run at once while `inner` stays free for `state()`.
    /// Lock order: `turn`, then `inner`.
    turn: Mutex<()>,
}

impl fmt::Debug for KrunkVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KrunkVault").field("dir", &self.dir).field("state", &self.state()).finish()
    }
}

fn load_state(path: &Path) -> StateFile {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

impl KrunkVault {
    /// With any core key wrapper and cost (Rust tests; the app goes through [`KrunkVault::new`]).
    pub(crate) fn with_wrapper(dir: PathBuf, wrapper: BoxedWrapper, params: KdfParams) -> Arc<Self> {
        let state = load_state(&dir.join(STATE_FILE));
        Arc::new(KrunkVault {
            dir,
            wrapper,
            params,
            inner: Mutex::new(Inner {
                vault: None,
                state,
                hints: None,
                hints_tried: false,
                last_used: 0,
                used_since: false,
                locks: 0,
            }),
            turn: Mutex::new(()),
        })
    }

    fn vault_path(&self) -> PathBuf {
        self.dir.join(VAULT_FILE)
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        // The core rolls its own state back when a save fails and writes files atomically, so
        // a poisoned lock is still usable. Release builds abort on a panic.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn turn(&self) -> MutexGuard<'_, ()> {
        self.turn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wrapper(&self) -> &dyn KeyWrapper {
        self.wrapper.as_ref()
    }

    fn persist(&self, inner: &Inner) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_vec_pretty(&inner.state).map_err(std::io::Error::other)?;
        core_vault::write_atomic(&self.state_path(), &json)
    }

    /// The hint key from `state.json`, unwrapped (asked of the wrapper once).
    fn load_hints(&self, inner: &mut Inner) {
        if inner.hints.is_some() || inner.hints_tried {
            return;
        }
        inner.hints_tried = true;
        let Some(wrapped) = inner.state.hint_key.as_deref().and_then(|t| Base64::decode_vec(t).ok()) else {
            return;
        };
        let Ok(raw) = self.wrapper.unprotect(&wrapped) else { return };
        let mut key = Zeroizing::new([0u8; 32]);
        if raw.len() == key.len() {
            key.copy_from_slice(&raw);
            inner.hints = Some(HintIndex::new(*key));
        }
    }

    /// Brings the hint tags in line with the open vault's entries (a new hint key if there is
    /// none this device can unwrap). `true` = `state.json` has to be written. A wrapper failure
    /// leaves the vault without hints, which is never an error.
    fn refresh_hints(&self, inner: &mut Inner) -> bool {
        let before = (inner.state.hint_key.clone(), inner.state.hint_tags.clone());
        self.load_hints(inner);
        if inner.hints.is_none() {
            let index = HintIndex::random();
            match self.wrapper.protect(index.key()) {
                Ok(wrapped) => {
                    inner.state.hint_key = Some(Base64::encode_string(&wrapped));
                    inner.hints = Some(index);
                }
                Err(_) => inner.state.hint_key = None,
            }
        }
        inner.state.hint_tags = match (&inner.hints, &inner.vault) {
            (Some(index), Some(vault)) => index.tags_for(vault.entries().iter().map(|e| e.origin.as_str())),
            _ => Vec::new(),
        };
        before != (inner.state.hint_key.clone(), inner.state.hint_tags.clone())
    }

    /// After a change to the entries: new hint tags, written if they differ. Best effort (the
    /// change itself is already in the vault file).
    fn entries_changed(&self, inner: &mut Inner) {
        if self.refresh_hints(inner) {
            let _ = self.persist(inner);
        }
    }

    /// Takes over a freshly opened vault, unless `lock` was called since the caller read `locks`
    /// (a "Lock now" during the slow key derivation wins: the vault is dropped again).
    fn opened(&self, inner: &mut Inner, vault: Vault, now: Option<i64>, locks: u64) -> VaultResult<()> {
        inner.vault = Some(vault);
        match now {
            Some(now) => {
                inner.last_used = now;
                inner.used_since = false;
            }
            // No time with the call: used "now", by the clock of the next `autolock_due`.
            None => inner.used_since = true,
        }
        self.refresh_hints(inner);
        // Best effort: the vault is open either way, and the limiter is right in memory.
        let _ = self.persist(inner);
        if inner.locks != locks {
            inner.vault = None;
            inner.used_since = false;
            return Err(VaultFfiError::Locked);
        }
        Ok(())
    }

    fn missing(&self) -> VaultFfiError {
        if core_vault::exists(&self.vault_path()) {
            VaultFfiError::Locked
        } else {
            VaultFfiError::NoVault
        }
    }

    /// Runs `f` on the open vault; counts as use.
    /// Like [`read`](Self::read) for what the engine asks by itself (a page with a sign-in form
    /// was loaded, a form was sent): it is not the user using the vault and must not keep the
    /// inactivity lock from falling.
    fn peek<T>(&self, f: impl FnOnce(&Vault) -> VaultResult<T>) -> VaultResult<T> {
        let inner = self.inner();
        let Some(vault) = inner.vault.as_ref() else { return Err(self.missing()) };
        f(vault)
    }

    fn read<T>(&self, f: impl FnOnce(&Vault) -> VaultResult<T>) -> VaultResult<T> {
        let mut inner = self.inner();
        let Some(vault) = inner.vault.as_ref() else { return Err(self.missing()) };
        let out = f(vault);
        inner.used_since = true;
        out
    }

    /// Runs the change `f` on the open vault, then updates the hints; counts as use at `now`
    /// (or at the next `touch`/`autolock_due` without one).
    fn write<T>(
        &self,
        now: Option<i64>,
        f: impl FnOnce(&mut Vault, &dyn KeyWrapper) -> Result<T, VaultError>,
    ) -> VaultResult<T> {
        let mut inner = self.inner();
        let Some(vault) = inner.vault.as_mut() else { return Err(self.missing()) };
        let out = f(vault, self.wrapper());
        match now {
            Some(now) => {
                inner.last_used = now;
                inner.used_since = false;
            }
            None => inner.used_since = true,
        }
        if out.is_ok() {
            self.entries_changed(&mut inner);
        }
        out.map_err(Into::into)
    }
}

fn summary(e: &core_vault::Entry) -> VaultEntrySummary {
    VaultEntrySummary {
        id: e.id.clone(),
        origin: e.origin.clone(),
        username: e.username.clone(),
        has_note: !e.note.is_empty(),
        created: e.created,
        updated: e.updated,
    }
}

/// Every entry matching `query`, sorted by origin, then user name (as the core's listing).
fn summaries_matching(vault: &Vault, query: &str) -> Vec<VaultEntrySummary> {
    let needle = query.trim().to_lowercase();
    let mut out: Vec<VaultEntrySummary> = vault
        .entries()
        .iter()
        .filter(|e| {
            needle.is_empty()
                || e.origin.to_lowercase().contains(&needle)
                || e.username.to_lowercase().contains(&needle)
        })
        .map(summary)
        .collect();
    out.sort_by(|a, b| a.origin.cmp(&b.origin).then_with(|| a.username.cmp(&b.username)));
    out
}

#[uniffi::export]
impl KrunkVault {
    /// The vault in `dir` (`vault.json`, `state.json`), wrapped by this device's key. Nothing
    /// is created or read from the key store here.
    #[uniffi::constructor]
    pub fn new(dir: String, wrapper: Arc<dyn VaultKeyWrapper>) -> Arc<Self> {
        Self::with_wrapper(PathBuf::from(dir), Box::new(ForeignWrapper(wrapper)), KdfParams::default())
    }

    pub fn state(&self) -> VaultState {
        if self.inner().vault.is_some() {
            VaultState::Unlocked
        } else if core_vault::exists(&self.vault_path()) {
            VaultState::Locked
        } else {
            VaultState::None
        }
    }

    /// Makes a new, empty vault and leaves it unlocked. Never replaces an existing file.
    pub fn create(&self, password: String, now: i64) -> VaultResult<()> {
        let password = Zeroizing::new(password);
        let _turn = self.turn();
        let path = self.vault_path();
        if path.exists() {
            return Err(VaultFfiError::AlreadyExists);
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| VaultFfiError::Io { detail: e.to_string() })?;
        let locks = self.inner().locks;
        // The key derivation runs without `inner`, so `state()` answers meanwhile.
        let vault = Vault::create(&path, &password, self.params, self.wrapper())?;
        let mut inner = self.inner();
        // Nothing of an earlier vault applies to this one.
        inner.state = StateFile::default();
        inner.hints = None;
        inner.hints_tried = false;
        // Locked meanwhile: the vault was made all the same.
        let _ = self.opened(&mut inner, vault, Some(now), locks);
        Ok(())
    }

    /// Unlocks with the vault password. A wrong password counts against the rate limit
    /// (1 s, 2 s, 4 s … 60 s, kept across restarts); while it applies nothing is tried.
    pub fn unlock(&self, password: String, now: i64) -> VaultResult<()> {
        let password = Zeroizing::new(password);
        let _turn = self.turn();
        let path = self.vault_path();
        let locks = {
            let mut inner = self.inner();
            if !core_vault::exists(&path) {
                return Err(VaultFfiError::NoVault);
            }
            // Open already (two unlocks in a row): nothing to do, and the open vault is not
            // replaced by a copy read from the file a moment ago.
            if inner.vault.is_some() {
                return Ok(());
            }
            // A failure recorded while the clock was ahead must not hold the vault shut until
            // real time catches up: at most one delay is served from now.
            if inner.state.rate.last_failure_ms > now {
                inner.state.rate.last_failure_ms = now;
            }
            let wait_ms = inner.state.rate.wait_ms(now);
            if wait_ms > 0 {
                return Err(VaultFfiError::RateLimited { wait_ms });
            }
            inner.locks
        };
        // The key derivation (about a second) runs without `inner`.
        let opened = Vault::open(&path, &password, self.wrapper());
        let mut inner = self.inner();
        match opened {
            Ok(vault) => {
                inner.state.rate.record_success();
                self.opened(&mut inner, vault, Some(now), locks)
            }
            Err(VaultError::WrongPassword) => {
                inner.state.rate.record_failure(now);
                // Best effort on disk; the count in memory applies regardless.
                let _ = self.persist(&inner);
                Err(VaultFfiError::WrongPassword)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// How long the next `unlock` still has to wait (0 = now).
    pub fn retry_wait_ms(&self, now: i64) -> u64 {
        self.inner().state.rate.wait_ms(now)
    }

    /// Unlocks with the key `unlock_key` gave (the biometric path: Kotlin decrypts it with a
    /// key-store key that needs the user's authentication). Not rate limited: the key store
    /// gates every attempt. A key from before a password change is `WrongPassword`.
    pub fn unlock_with_key(&self, key: Vec<u8>) -> VaultResult<()> {
        let key = Zeroizing::new(key);
        let _turn = self.turn();
        let path = self.vault_path();
        if !core_vault::exists(&path) {
            return Err(VaultFfiError::NoVault);
        }
        let mut fixed = Zeroizing::new([0u8; 32]);
        if key.len() != fixed.len() {
            return Err(VaultFfiError::WrongPassword);
        }
        fixed.copy_from_slice(&key);
        let locks = self.inner().locks;
        let vault = Vault::open_with_key(&path, &fixed, self.wrapper())?;
        let mut inner = self.inner();
        self.opened(&mut inner, vault, None, locks)
    }

    /// The key that unlocks this vault without the password, until the password changes. For
    /// the biometric switch only: Kotlin encrypts it at once and keeps the result with
    /// `set_bio_blob`.
    pub fn unlock_key(&self) -> VaultResult<Vec<u8>> {
        self.read(|v| Ok(v.unlock_key().to_vec()))
    }

    /// Locks: drops the open vault, which wipes its keys and entries. An unlock that is still
    /// deriving its key ends locked too (it reports `Locked`).
    pub fn lock(&self) {
        let mut inner = self.inner();
        inner.locks = inner.locks.wrapping_add(1);
        inner.vault = None;
        inner.used_since = false;
    }

    /// Changes the vault password (new salt, new keys, every entry sealed again). The biometric
    /// blob is dropped: it held the old key.
    pub fn change_password(&self, old: String, new: String) -> VaultResult<()> {
        let (old, new) = (Zeroizing::new(old), Zeroizing::new(new));
        let mut inner = self.inner();
        let Some(vault) = inner.vault.as_mut() else { return Err(self.missing()) };
        vault.change_password(&old, &new, self.wrapper())?;
        inner.used_since = true;
        inner.state.bio = None;
        // The password is changed once the vault file is written; the side file is best effort
        // (a blob that stayed behind holds the old key and opens nothing).
        let _ = self.persist(&inner);
        Ok(())
    }

    pub fn summaries(&self) -> VaultResult<Vec<VaultEntrySummary>> {
        self.read(|v| Ok(summaries_matching(v, "")))
    }

    /// Entries whose origin or user name contains `query` (case-insensitive); all for an empty one.
    pub fn search(&self, query: String) -> VaultResult<Vec<VaultEntrySummary>> {
        self.read(|v| Ok(summaries_matching(v, &query)))
    }

    /// One entry without its password.
    pub fn entry(&self, id: String) -> VaultResult<VaultEntryDetail> {
        self.read(|v| {
            let e = v.entry(&id).ok_or(VaultFfiError::NotFound)?;
            Ok(VaultEntryDetail {
                id: e.id.clone(),
                origin: e.origin.clone(),
                username: e.username.clone(),
                note: e.note.to_string(),
                created: e.created,
                updated: e.updated,
            })
        })
    }

    /// The password of one entry (the view screen's "Show" and "Copy").
    pub fn reveal(&self, id: String) -> VaultResult<String> {
        self.read(|v| v.entry(&id).map(|e| e.password.to_string()).ok_or(VaultFfiError::NotFound))
    }

    /// Saves a submitted login: new, or the new password of the entry with the same origin and
    /// user name. Returns the entry id.
    pub fn save_login(&self, url: String, username: String, password: String, now: i64) -> VaultResult<String> {
        let password = Zeroizing::new(password);
        self.write(Some(now), |v, w| v.save_login(&url, &username, password, now, w))
    }

    /// Adds (`id` = null; needs a password) or edits an entry from the vault screen; a null
    /// `password` or `note` keeps the stored one. Returns the entry id.
    pub fn upsert(
        &self,
        id: Option<String>,
        url: String,
        username: String,
        password: Option<String>,
        note: Option<String>,
        now: i64,
    ) -> VaultResult<String> {
        let (password, note) = (password.map(Zeroizing::new), note.map(Zeroizing::new));
        self.write(Some(now), |v, w| v.upsert(id.as_deref(), &url, &username, password, note, now, w))
    }

    pub fn delete(&self, id: String) -> VaultResult<()> {
        self.write(None, |v, w| v.delete(&id, w))
    }

    /// The logins to offer on the page at `url`: its own origin's, or (only when it has none,
    /// https only) those of the same registrable domain. Sorted by user name.
    pub fn matches_for(&self, url: String) -> VaultResult<Vec<VaultMatch>> {
        self.peek(|v| {
            Ok(v.matches_for(&url)
                .into_iter()
                .map(|(e, kind)| VaultMatch {
                    id: e.id.clone(),
                    origin: e.origin.clone(),
                    username: e.username.clone(),
                    exact: kind == core_vault::MatchKind::Exact,
                })
                .collect())
        })
    }

    /// The logins for an HTTP sign-in dialog of the server at `url`: its exact origin only.
    pub fn for_http_auth(&self, url: String) -> VaultResult<Vec<VaultMatch>> {
        self.peek(|v| {
            let mut out: Vec<VaultMatch> = v
                .for_origin(&url)
                .into_iter()
                .map(|e| VaultMatch {
                    id: e.id.clone(),
                    origin: e.origin.clone(),
                    username: e.username.clone(),
                    exact: true,
                })
                .collect();
            out.sort_by(|a, b| a.username.cmp(&b.username));
            Ok(out)
        })
    }

    /// The user name and password of entry `id` for the page at `page_url` (the address the
    /// engine reports, never one the page claims). `NotAllowed` unless the entry belongs to
    /// that page: the same origin, or for https the same registrable domain.
    pub fn secret_for_fill(&self, id: String, page_url: String) -> VaultResult<VaultSecret> {
        self.read(|v| {
            let e = v.entry(&id).ok_or(VaultFfiError::NotFound)?;
            if core_vault::match_kind(&e.origin, &page_url).is_none() {
                return Err(VaultFfiError::NotAllowed);
            }
            Ok(VaultSecret { username: e.username.clone(), password: e.password.to_string() })
        })
    }

    /// What saving this submitted login would do (as the Windows browser decides it): compared
    /// with the entry of the same origin and user name.
    pub fn save_decision(&self, url: String, username: String, password: String) -> VaultResult<SaveDecision> {
        let password = Zeroizing::new(password);
        self.peek(|v| {
            Ok(match v.for_origin(&url).into_iter().find(|e| e.username == username) {
                Some(e) if *e.password == *password => SaveDecision::Unchanged,
                Some(e) => SaveDecision::Update { id: e.id.clone() },
                None => SaveDecision::New,
            })
        })
    }

    /// Does the vault have logins for the page at `url`? Works while locked (keyed tags in
    /// `state.json`, written at every unlock and change). `false` when the device key fails or
    /// there is no hint data; never an error.
    pub fn has_hint(&self, url: String) -> bool {
        let mut inner = self.inner();
        self.load_hints(&mut inner);
        match &inner.hints {
            Some(index) => index.has_logins(&url, &inner.state.hint_tags),
            None => false,
        }
    }

    /// Kotlin's blob for the biometric unlock, if the setting is on.
    pub fn bio_blob(&self) -> Option<Vec<u8>> {
        self.inner().state.bio.as_deref().and_then(|t| Base64::decode_vec(t).ok())
    }

    /// Stores (or with null removes) the biometric blob: opaque bytes, already encrypted by
    /// Kotlin under the biometric key.
    pub fn set_bio_blob(&self, blob: Option<Vec<u8>>) -> VaultResult<()> {
        let mut inner = self.inner();
        inner.state.bio = blob.map(|b| Base64::encode_string(&b));
        self.persist(&inner).map_err(|e| VaultFfiError::Io { detail: e.to_string() })
    }

    /// The user did something with the vault at `now` (restarts the lock timer).
    pub fn touch(&self, now: i64) {
        let mut inner = self.inner();
        inner.last_used = now;
        inner.used_since = false;
    }

    /// Should the vault lock for inactivity (`minutes`; 0 counts as 1)? Unlocking, creating and
    /// the calls that carry a time count as use at that time. A call without one (listing,
    /// revealing, filling, deleting) counts as use at the time of the next `touch` or of this
    /// check, whichever comes first.
    pub fn autolock_due(&self, now: i64, minutes: u32) -> bool {
        let mut inner = self.inner();
        if inner.used_since {
            inner.used_since = false;
            inner.last_used = now;
        }
        core_vault::autolock_due(inner.vault.is_some(), false, now.saturating_sub(inner.last_used), minutes)
    }

    /// Deletes the vault and its side file for good (the "this device's key is gone: delete this
    /// vault and start anew" flow; the caller asks for a typed confirmation first).
    pub fn erase(&self) -> VaultResult<()> {
        let _turn = self.turn();
        let mut inner = self.inner();
        inner.vault = None;
        inner.state = StateFile::default();
        inner.hints = None;
        inner.hints_tried = false;
        inner.used_since = false;
        let mut result = Ok(());
        for path in [self.vault_path(), self.state_path()] {
            // A temp file an interrupted write left behind goes too.
            let _ = std::fs::remove_file(path.with_extension("tmp"));
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => result = Err(VaultFfiError::Io { detail: e.to_string() }),
            }
        }
        result
    }
}

/// The origin (`scheme://host[:port]`) of a URL, as vault entries and the never-save list store
/// it; null for anything that has none.
#[uniffi::export]
pub fn vault_origin_of(url: String) -> Option<String> {
    origin_of(&url)
}

#[cfg(test)]
mod tests;
