//! Krunk account and sync for Android (0.6.0): the same steps as Windows'
//! `app/src/browser/{account,sync}.rs`, in Rust over `lb_core::account` and `lb_core::sync`, so the
//! keys, the directories, the merge and the conflict rules are the shared code. Kotlin supplies
//! the HTTPS transport ([`SyncTransport`]) and the Keystore wrapper, shows [`SyncStatus`] and
//! decides when to call [`KrunkSync::run_now`] (in the foreground only).
//!
//! **No traffic before sign-in.** The constructor and [`KrunkSync::status`] read local files only;
//! the first request is [`KrunkSync::link_start`].
//!
//! **What is stored where** (all in the app's private `files/krunk-sync/`, excluded from backup):
//! `account.json` (core `AccountFile`: ids, address, device name, the refresh token wrapped by the
//! Keystore) and `sync.json` (core `SyncState`, sealed, its key wrapped by the Keystore). The access
//! token, the link code and the setup secrets live in memory only. PAK and its bookkeeping live in
//! the vault (`VaultSync`), never outside it.
//!
//! **Signing out** (and a device removed elsewhere) deletes both files, the vault's sync part
//! and sync's bookkeeping in `browser.db`; bookmarks, history, settings and logins stay unless the
//! user asked to remove what came from sync (as Windows' Turn off).

use crate::vault::{BoxedWrapper, ForeignWrapper, KrunkVault, VaultKeyWrapper};
use lb_core::account::{self as acct, AccountFile, Failure, Linking};
use lb_core::secret::Secret;
use lb_core::storage::Store;
use lb_core::sync::api::{self as sapi, ApiError, Backoff, HttpClient, HttpResponse};
use lb_core::sync::client::{self, Api, NApproval, NStep, SyncFail};
use lb_core::sync::engine::{self, RunOpts};
use lb_core::sync::hlc::Hlc;
use lb_core::sync::keys::{self, EscrowWire, KeyError, Me, PakState, RestorePlan, Secret as SyncSecret, SetupPlan};
use lb_core::sync::state::{self, parse_account_id, SyncState};
use lb_core::sync::{coll, local, pak, rotate};
use lb_core::vault::VaultSync;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

const ACCOUNT_FILE: &str = "account.json";
const STATE_FILE: &str = "sync.json";
/// An access token lives 15 minutes; renew it a minute early.
const ACCESS_MS: i64 = 900_000;
const ACCESS_MARGIN_MS: i64 = 60_000;
/// The schedule (Windows parity, PROTOCOL §10.3): every 5 minutes while the app is in front.
pub const EVERY_MS: i64 = 5 * 60_000;
/// After a local change.
pub const AFTER_CHANGE_MS: i64 = 30_000;
/// A passphrase or recovery key is far shorter; anything longer is not one.
const MAX_SECRET: usize = 256;
const MAX_LABEL: usize = 64;

const OFFLINE_TEXT: &str = "Krunk couldn't reach krunk.209.wf. Check your connection and try again.";
const REMOVED_TEXT: &str =
    "This device was removed from your Krunk account (on another device or on the website). Its sync keys were deleted here; your data on this device stays. Sign in again to sync.";

/// The collections a device can switch off, in the order the screen lists them.
pub const COLLECTIONS: [&str; 7] = ["bookmarks", "passwords", "history", "settings", "pins", "allowlist", "tabs"];

// ------------------------------------------------------------------------------ transport

/// One HTTP answer, whatever its status.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HttpAnswer {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<String>,
    pub date: Option<String>,
}

/// Why no answer arrived. Never carries a token or a body.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum TransportError {
    Offline { detail: String },
    Tls,
    Timeout,
    TooLarge,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Offline { detail } => write!(f, "offline ({detail})"),
            TransportError::Tls => f.write_str("TLS failed"),
            TransportError::Timeout => f.write_str("timed out"),
            TransportError::TooLarge => f.write_str("answer too large"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for TransportError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        TransportError::Offline { detail: e.reason }
    }
}

/// The HTTPS transport, implemented in Kotlin: `method` on `path` below `<base>/api/v1/`, with
/// the bearer token and a JSON body. HTTPS only in release builds, the system's trust store, no
/// redirects, a response cap of 1 MiB; `Err` only when no answer arrived.
#[uniffi::export(with_foreign)]
pub trait SyncTransport: Send + Sync {
    fn call(
        &self,
        method: String,
        path: String,
        bearer: Option<String>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpAnswer, TransportError>;
}

/// [`SyncTransport`] as the core's transport.
struct Foreign<'a>(&'a dyn SyncTransport);

impl HttpClient for Foreign<'_> {
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, String> {
        self.0
            .call(method.into(), path.into(), bearer.map(str::to_string), body.map(<[u8]>::to_vec))
            .map(|a| HttpResponse { status: a.status, body: a.body, retry_after: a.retry_after, date: a.date })
            .map_err(|e| e.to_string())
    }
}

/// An account call as `core::account` wants its answer (its paths carry `/api/v1/`).
fn acct_call(
    http: &dyn HttpClient,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> acct::Answer {
    let path = path.strip_prefix("/api/v1/").unwrap_or(path);
    http.call(method, path, bearer, body.map(str::as_bytes)).map(|r| (u32::from(r.status), r.body))
}

// ------------------------------------------------------------------------------ what Kotlin sees

/// Why a call failed, in words the screen shows as they are.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum SyncFfiError {
    /// The passphrase or recovery key doesn't open the sync data (nothing changed; try again).
    WrongSecret {
        text: String,
    },
    /// The 6 digits didn't match, or the recovery key group typed back is wrong.
    WrongCode {
        text: String,
    },
    /// This device was removed (keys and tokens are gone here).
    Revoked {
        text: String,
    },
    /// Not signed in (or no longer).
    SignedOut {
        text: String,
    },
    Offline {
        text: String,
    },
    Server {
        text: String,
    },
    RateLimited {
        text: String,
        retry_after_s: u64,
    },
    /// The server's keys or records don't check out: sync stopped.
    Alarm {
        text: String,
    },
    /// Sync was reset from another device.
    Reset {
        text: String,
    },
    /// Storage on this device failed.
    Local {
        text: String,
    },
    /// The call doesn't fit the current step (a double tap, a stale screen).
    NotNow {
        text: String,
    },
}

impl std::fmt::Display for SyncFfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.text())
    }
}

impl std::error::Error for SyncFfiError {}

impl SyncFfiError {
    pub fn text(&self) -> &str {
        match self {
            SyncFfiError::WrongSecret { text }
            | SyncFfiError::WrongCode { text }
            | SyncFfiError::Revoked { text }
            | SyncFfiError::SignedOut { text }
            | SyncFfiError::Offline { text }
            | SyncFfiError::Server { text }
            | SyncFfiError::RateLimited { text, .. }
            | SyncFfiError::Alarm { text }
            | SyncFfiError::Reset { text }
            | SyncFfiError::Local { text }
            | SyncFfiError::NotNow { text } => text,
        }
    }
}

type R<T> = Result<T, SyncFfiError>;

fn not_now(text: &str) -> SyncFfiError {
    SyncFfiError::NotNow { text: text.into() }
}

fn local_err(e: impl std::fmt::Display) -> SyncFfiError {
    SyncFfiError::Local { text: format!("Krunk couldn't save its sync data on this device ({e}).") }
}

const WRONG_VAULT_PW: &str =
    "That vault password doesn't open your sync data. Type the vault password you use on your other device.";

const NO_VAULT_COPY: &str = "Your other device hasn't switched to the vault password yet. Update Krunk there, \
     unlock your passwords on it once, then try again here.";

/// Errors of the vault-password paths, in their words.
fn vault_err(e: &SyncFail) -> SyncFfiError {
    match e {
        SyncFail::Keys(KeyError::WrongSecret) => SyncFfiError::WrongSecret { text: WRONG_VAULT_PW.into() },
        SyncFail::Keys(KeyError::NoVaultWrap) => SyncFfiError::NotNow { text: NO_VAULT_COPY.into() },
        e => sync_err(e),
    }
}

/// The same sentences as Windows' `fail_text`.
fn sync_text(e: &SyncFail) -> String {
    match e {
        SyncFail::Api(ApiError::Offline(_)) => OFFLINE_TEXT.into(),
        SyncFail::Api(ApiError::Unauthorized) => REMOVED_TEXT.into(),
        SyncFail::Api(ApiError::RateLimited(s)) => format!("Too many attempts. Try again in {} s.", (*s).clamp(1, 60)),
        SyncFail::Api(ApiError::Conflict(_)) => "Someone changed sync's keys meanwhile. Try again.".into(),
        SyncFail::Api(ApiError::RotationPending) => {
            "A device was removed, and another device is re-securing sync. Try again in a minute.".into()
        }
        SyncFail::Keys(KeyError::WrongSecret) => "That passphrase or recovery key doesn't open your sync data.".into(),
        SyncFail::Keys(KeyError::SasMismatch) => {
            "The code didn't match, so the request was cancelled. Ask again, and type the 6 digits the other device shows."
                .into()
        }
        SyncFail::Keys(KeyError::Crypto(lb_core::sync::SyncError::WeakKdf)) => {
            "The server offered weaker protection for your passphrase than Krunk accepts, so Krunk stopped.".into()
        }
        SyncFail::Keys(k) if k.is_alarm() => {
            "Sync's keys on the server don't check out. Krunk stopped syncing to keep your data safe.".into()
        }
        SyncFail::Reset => "Sync was reset from another device. Sign out and in again on this device.".into(),
        SyncFail::Api(ApiError::Server(_)) => "Something went wrong on krunk.209.wf. Try again later.".into(),
        _ => "Something went wrong with sync. Try again later.".into(),
    }
}

fn sync_err(e: &SyncFail) -> SyncFfiError {
    let text = sync_text(e);
    match e {
        SyncFail::Api(ApiError::Offline(_)) => SyncFfiError::Offline { text },
        SyncFail::Api(ApiError::Unauthorized) => SyncFfiError::Revoked { text },
        SyncFail::Api(ApiError::RateLimited(s)) => SyncFfiError::RateLimited { text, retry_after_s: (*s).clamp(1, 60) },
        SyncFail::Keys(KeyError::WrongSecret) => SyncFfiError::WrongSecret { text },
        SyncFail::Keys(KeyError::SasMismatch) => SyncFfiError::WrongCode { text },
        SyncFail::Keys(k) if k.is_alarm() => SyncFfiError::Alarm { text },
        SyncFail::Reset => SyncFfiError::Reset { text },
        SyncFail::Local(_) => SyncFfiError::Local { text },
        _ => SyncFfiError::Server { text },
    }
}

fn account_err(f: Failure) -> SyncFfiError {
    let text = f.text().to_string();
    match f {
        Failure::Offline => SyncFfiError::Offline { text },
        Failure::RateLimited => SyncFfiError::RateLimited { text, retry_after_s: 60 },
        Failure::SignedOut => SyncFfiError::Revoked { text: REMOVED_TEXT.into() },
        Failure::NotAvailable | Failure::Closed | Failure::Server => SyncFfiError::Server { text },
    }
}

/// Where this device stands.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SyncStage {
    SignedOut,
    /// Waiting for the approval on the website.
    Linking {
        user_code: String,
        verify_url: String,
        expires_at_ms: i64,
    },
    /// Signed in, sync not on: "Turn on sync".
    Off,
    /// The account syncs elsewhere: ask a device or restore.
    Join {
        passwords: bool,
    },
    /// First device: the passphrase and recovery key were shown; the user types the group back.
    Setup,
    /// Restore found the keys: the devices that keep access, for the user to confirm.
    RestoreConfirm {
        devices: Vec<String>,
        revoked: u32,
    },
    /// Asked a device; `enter_code` = type the 6 digits it shows.
    Joining {
        enter_code: bool,
    },
    On,
    /// Sync stopped for safety (the server's keys don't check out).
    Alarm,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CollectionState {
    pub name: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncStatus {
    pub stage: SyncStage,
    pub email: Option<String>,
    pub device_name: Option<String>,
    /// The last run that finished without an error (ms).
    pub last_sync_ms: Option<i64>,
    /// How the last run ended, as the screen shows it ("Synced", or the error).
    pub last_result: Option<String>,
    /// A sentence for the screen (a step's outcome, a problem, a hint), or none.
    pub message: Option<String>,
    pub collections: Vec<CollectionState>,
    /// Local changes waiting to be sent.
    pub pending: i64,
    /// When the schedule wants the next run (ms), while sync is on.
    pub next_run_ms: Option<i64>,
    /// A removal elsewhere waits for a device to re-secure sync (Finish on Windows).
    pub rotation_pending: bool,
    /// Whether passwords sync on this device, and if not, why (0.6.1).
    pub passwords: PasswordsSync,
    /// "Ask my PC" for the passwords key (0.6.2): where the request stands.
    pub passwords_ask: PasswordsAsk,
}

/// A request to another device for the passwords key (0.6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PasswordsAsk {
    /// None open.
    None,
    /// Waiting for the other device to answer.
    Waiting,
    /// The other device shows 6 digits: type them here.
    EnterCode,
    /// The code was sent; the key is on its way.
    Confirmed,
}

/// Passwords on this device (0.6.1): they need the passwords key, which lives in the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PasswordsSync {
    /// Sync is off.
    Off,
    /// They sync (while the vault is unlocked).
    On,
    /// Switched off on this device.
    SwitchedOff,
    /// The vault is locked: unlock it to sync passwords (or to see whether they can).
    VaultLocked,
    /// There is no vault here yet: create one, then "Add passwords".
    NoVault,
    /// The vault is open but holds no passwords key (the device joined without it): "Add
    /// passwords" with the passphrase or recovery key.
    NotOnThisDevice,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LinkInfo {
    pub user_code: String,
    pub verify_url: String,
    pub expires_at_ms: i64,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LinkPoll {
    /// Ask again in this many milliseconds.
    Wait {
        ms: u64,
    },
    SignedIn {
        email: String,
    },
    /// The sign-in ended without an account (expired, denied).
    Stopped {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TurnOn {
    /// The account already syncs: ask a device or restore.
    Join,
    /// This is the first device.
    Setup { secrets: SetupSecretsText },
}

/// [`SetupSecrets`] inside [`TurnOn`] (records in an enum must be plain data).
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct SetupSecretsText {
    pub passphrase: String,
    pub recovery_key: String,
}

impl std::fmt::Debug for SetupSecretsText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SetupSecretsText(…)")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct SyncChanged {
    pub bookmarks: bool,
    pub history: bool,
    pub settings: bool,
    pub pins: bool,
    pub allowlist: bool,
    pub tabs: bool,
    pub passwords: bool,
    pub never_save: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct RunReport {
    pub pulled: u32,
    pub pushed: u32,
    pub conflicts: u32,
    pub changed: SyncChanged,
    /// Writes wait until a device re-secures sync after a removal.
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RemoteTab {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceTabs {
    pub device_id: String,
    pub name: String,
    pub updated_ms: i64,
    pub tabs: Vec<RemoteTab>,
}

// ------------------------------------------------------------------------------ the state

enum Stage {
    Off,
    Join { passwords: bool },
    Setup,
    RestoreConfirm { devices: Vec<String>, revoked: u32 },
    Joining { enter_code: bool },
    On,
    Alarm,
}

/// Everything behind [`KrunkSync`], without the transport (the tests drive it over the
/// in-memory server).
pub(crate) struct Engine {
    dir: PathBuf,
    db: PathBuf,
    wrapper: BoxedWrapper,
    vault: Option<Arc<KrunkVault>>,
    /// "Krunk on Android 16" (the device's name in the account).
    label: String,
    version: String,
    /// The site the approval page must be on; `None` for test builds against a staging stack.
    verify_base: Option<String>,
    loaded: bool,
    account: Option<AccountFile>,
    access: Option<(Secret, i64)>,
    linking: Option<Linking>,
    sync: Option<SyncState>,
    stage: Stage,
    plan: Option<SetupPlan>,
    escrow: Option<EscrowWire>,
    restore: Option<RestorePlan>,
    joining: Option<NApproval>,
    /// "Ask my PC" (0.6.2): the open request for the passwords key and where it stands.
    asking: Option<(NApproval, PasswordsAsk)>,
    backoff: Backoff,
    next_run_ms: i64,
    wait_until_ms: i64,
    last_ok_ms: Option<i64>,
    last_result: Option<String>,
    message: Option<String>,
    rotation_pending: bool,
}

fn secret_of(text: String, recovery: bool) -> R<SyncSecret> {
    if text.len() > MAX_SECRET || text.trim().is_empty() {
        return Err(SyncFfiError::WrongSecret { text: sync_text(&SyncFail::Keys(KeyError::WrongSecret)) });
    }
    let z = Zeroizing::new(text);
    // A typed `KRNK-…` is a recovery key whatever the switch says (as on Windows).
    let recovery = recovery || z.trim().to_ascii_uppercase().starts_with("KRNK-");
    // Checked here, before the escrow is fetched (rate-limited, and mailed to the user): a
    // vault or account password can't be either (0.6.2).
    let shaped = if recovery {
        lb_core::sync::codes::parse_recovery_key(&z).is_ok()
    } else {
        lb_core::sync::keys::looks_like_passphrase(&z)
    };
    if !shaped {
        return Err(SyncFfiError::WrongSecret { text: NOT_A_SECRET.into() });
    }
    Ok(if recovery { SyncSecret::RecoveryKey(z) } else { SyncSecret::Passphrase(z) })
}

/// What `secret_of` says to text that is no sync passphrase or recovery key at all.
const NOT_A_SECRET: &str = "That isn't your sync passphrase. It is 7 words that Krunk showed once when sync was \
     first turned on (or use the recovery key, KRNK-…). Your vault password doesn't work here: \
     use Ask my PC instead.";

impl Engine {
    pub(crate) fn new(
        dir: PathBuf,
        db: PathBuf,
        wrapper: BoxedWrapper,
        vault: Option<Arc<KrunkVault>>,
        label: String,
        version: String,
        verify_base: Option<String>,
    ) -> Engine {
        let label: String = label.chars().filter(|c| !c.is_control()).take(MAX_LABEL).collect();
        Engine {
            dir,
            db,
            wrapper,
            vault,
            label: if label.trim().is_empty() { "Krunk on Android".into() } else { label },
            version,
            verify_base,
            loaded: false,
            account: None,
            access: None,
            linking: None,
            sync: None,
            stage: Stage::Off,
            plan: None,
            escrow: None,
            restore: None,
            joining: None,
            asking: None,
            backoff: Backoff::default(),
            next_run_ms: 0,
            wait_until_ms: 0,
            last_ok_ms: None,
            last_result: None,
            message: None,
            rotation_pending: false,
        }
    }

    fn account_path(&self) -> PathBuf {
        self.dir.join(ACCOUNT_FILE)
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    fn store(&self) -> R<Store> {
        Store::open(&self.db).map_err(local_err)
    }

    /// Reads the two files once (no network). A state this device can't open (its Keystore key
    /// is gone, or the files came from elsewhere) counts as signed out; sync bookkeeping in
    /// browser.db without a usable sync.json (a device-to-device copy of the database) is
    /// forgotten, so it never meets another device's keys.
    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        self.account = std::fs::read(self.account_path()).ok().and_then(|b| AccountFile::parse(&b));
        if self.account.is_none() {
            let _ = std::fs::remove_file(self.account_path());
        }
        match state::load(&self.state_path(), self.wrapper.as_ref()) {
            Ok(Some(st)) if self.account.as_ref().is_some_and(|a| a.device_id == st.device_id) => {
                self.stage = if st.alarm.is_some() { Stage::Alarm } else { Stage::On };
                self.message = st.alarm.clone();
                self.sync = Some(st);
                self.next_run_ms = crate::now_ms() + 60_000;
            }
            Ok(None) => {}
            // Unreadable, or from another sign-in: gone.
            _ => {
                let _ = std::fs::remove_file(self.state_path());
            }
        }
        if self.sync.is_none() {
            if let Ok(mut store) = self.store() {
                let c = store.sync_conn();
                let leftover = local::meta_get(c, "track").ok().flatten().is_some()
                    || c.query_row("SELECT count(*) FROM sync_items", [], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0;
                if leftover {
                    let _ = store.sync_forget(false);
                }
            }
        }
    }

    fn ids(&self) -> R<(String, lb_core::sync::AccountId, String)> {
        let a = self.account.as_ref().ok_or_else(|| SyncFfiError::SignedOut { text: "Sign in first.".into() })?;
        let acc = parse_account_id(&a.account_id).ok_or_else(|| SyncFfiError::Server {
            text: "Something went wrong on krunk.209.wf. Try again later.".into(),
        })?;
        Ok((a.account_id.clone(), acc, a.device_id.clone()))
    }

    fn vault_open(&self) -> bool {
        self.vault.as_ref().is_some_and(|v| v.sync_view(|_| ()).is_some())
    }

    fn pak(&self) -> Option<PakState> {
        self.vault.as_ref()?.sync_view(|v| v.sync_part().and_then(|p| PakState::from_bytes(&p.state).ok()))?
    }

    fn set_pak(&self, p: Option<&PakState>) {
        let Some(vault) = &self.vault else { return };
        let _ = vault.sync_change(|v, w| {
            let part = p.map(|p| VaultSync {
                state: p.to_bytes(),
                dirty: v.sync_part().map(|x| x.dirty.clone()).unwrap_or_default(),
            });
            v.set_sync_part(part, w)
        });
    }

    fn save_state(&self) -> R<()> {
        let Some(st) = &self.sync else { return Ok(()) };
        std::fs::create_dir_all(&self.dir).map_err(local_err)?;
        state::save(&self.state_path(), st, self.wrapper.as_ref()).map_err(|e| local_err(format!("{e:?}")))
    }

    // ---- status (no network) -------------------------------------------------------------------

    pub(crate) fn status(&mut self) -> SyncStatus {
        self.load();
        let stage = match (&self.account, &self.linking) {
            (None, Some(l)) => SyncStage::Linking {
                user_code: l.user_code.clone(),
                verify_url: l.verify_url.clone(),
                expires_at_ms: l.expires_at_ms,
            },
            (None, None) => SyncStage::SignedOut,
            (Some(_), _) => match &self.stage {
                Stage::Off => SyncStage::Off,
                Stage::Join { passwords } => SyncStage::Join { passwords: *passwords },
                Stage::Setup => SyncStage::Setup,
                Stage::RestoreConfirm { devices, revoked } => {
                    SyncStage::RestoreConfirm { devices: devices.clone(), revoked: *revoked }
                }
                Stage::Joining { enter_code } => SyncStage::Joining { enter_code: *enter_code },
                Stage::On => SyncStage::On,
                Stage::Alarm => SyncStage::Alarm,
            },
        };
        let (collections, pending) = match (&self.sync, self.store()) {
            (Some(_), Ok(store)) => {
                let c = store.sync_conn();
                let off = local::off_collections(c);
                (
                    COLLECTIONS
                        .iter()
                        .map(|n| CollectionState { name: n.to_string(), on: !off.iter().any(|o| o == n) })
                        .collect(),
                    local::queue_len(c).unwrap_or(0),
                )
            }
            _ => (Vec::new(), 0),
        };
        let passwords = self.passwords_state(&collections);
        SyncStatus {
            passwords,
            stage,
            email: self.account.as_ref().map(|a| a.email.clone()),
            device_name: self.account.as_ref().map(|a| a.label.clone()),
            last_sync_ms: self.last_ok_ms,
            last_result: self.last_result.clone(),
            message: self.message.clone(),
            collections,
            pending,
            next_run_ms: self.sync.as_ref().map(|_| self.next_run_ms.max(self.wait_until_ms)),
            rotation_pending: self.rotation_pending,
            passwords_ask: self.asking.as_ref().map_or(PasswordsAsk::None, |(_, s)| *s),
        }
    }

    // ---- account -------------------------------------------------------------------------------

    pub(crate) fn link_start(&mut self, http: &dyn HttpClient, now: i64) -> R<LinkInfo> {
        self.load();
        if self.account.is_some() {
            return Err(not_now("This device is already signed in."));
        }
        let body = serde_json::json!({ "platform": "android", "label": self.label, "client_version": self.version })
            .to_string();
        let a: acct::Authorized =
            acct::parse(&acct_call(http, "POST", acct::AUTHORIZE, None, Some(&body))).map_err(account_err)?;
        let l = Linking::start(a, self.verify_base.as_deref(), now).map_err(account_err)?;
        let info = LinkInfo {
            user_code: l.user_code.clone(),
            verify_url: l.verify_url.clone(),
            expires_at_ms: l.expires_at_ms,
            interval_ms: l.interval_s * 1000,
        };
        self.message = None;
        self.linking = Some(l);
        Ok(info)
    }

    pub(crate) fn link_poll(&mut self, http: &dyn HttpClient, now: i64) -> R<LinkPoll> {
        let Some(mut l) = self.linking.take() else { return Err(not_now("No sign-in is waiting.")) };
        let p = acct::poll_answer(&acct_call(http, "POST", acct::TOKEN, None, Some(&l.poll_body())));
        match l.next(p, now) {
            acct::Next::Poll(ms) => {
                self.linking = Some(l);
                Ok(LinkPoll::Wait { ms })
            }
            acct::Next::Stop(text) => {
                self.message = Some(text.into());
                Ok(LinkPoll::Stopped { message: text.into() })
            }
            acct::Next::SignedIn(t) => {
                let me: acct::DeviceMe =
                    acct::parse(&acct_call(http, "GET", acct::ME, Some(&t.access_token), None)).map_err(account_err)?;
                let wrapped = self.wrapper.protect(t.refresh_token.as_bytes()).map_err(local_err)?;
                let file = AccountFile::new(&me.email, &me.label, &me.account_id, &me.device_id, &wrapped, now);
                std::fs::create_dir_all(&self.dir).map_err(local_err)?;
                lb_core::vault::write_atomic(&self.account_path(), &file.to_bytes()).map_err(local_err)?;
                self.access = Some((t.access_token, now + ACCESS_MS));
                self.account = Some(file);
                self.stage = Stage::Off;
                self.message = None;
                Ok(LinkPoll::SignedIn { email: me.email })
            }
        }
    }

    pub(crate) fn link_cancel(&mut self) {
        self.linking = None;
    }

    /// A fresh access token (renewed with the stored refresh token when needed). A 401 means the
    /// device was removed or signed out elsewhere: everything of the account goes.
    fn access(&mut self, http: &dyn HttpClient, now: i64) -> R<Secret> {
        if let Some((t, exp)) = &self.access {
            if *exp - ACCESS_MARGIN_MS > now {
                return Ok(t.clone());
            }
        }
        let Some(file) = self.account.clone() else {
            return Err(SyncFfiError::SignedOut { text: "Sign in first.".into() });
        };
        let token = file
            .wrapped_refresh()
            .and_then(|w| self.wrapper.unprotect(&w).ok())
            .map(|t| Secret::new(String::from_utf8_lossy(&t).into_owned()));
        let r = match token {
            None => Err(Failure::SignedOut),
            Some(token) => {
                let body = Zeroizing::new(serde_json::json!({ "refresh_token": &*token }).to_string());
                acct::parse::<acct::Refreshed>(&acct_call(http, "POST", acct::REFRESH, None, Some(&body)))
            }
        };
        match r {
            Ok(r) => {
                let wrapped = self.wrapper.protect(r.refresh_token.as_bytes()).map_err(local_err)?;
                let mut file = file;
                file.set_wrapped_refresh(&wrapped);
                lb_core::vault::write_atomic(&self.account_path(), &file.to_bytes()).map_err(local_err)?;
                self.account = Some(file);
                self.access = Some((r.access_token.clone(), now + ACCESS_MS));
                Ok(r.access_token)
            }
            Err(Failure::SignedOut) => {
                self.forget_account(false, REMOVED_TEXT);
                Err(SyncFfiError::Revoked { text: REMOVED_TEXT.into() })
            }
            Err(f) => Err(account_err(f)),
        }
    }

    /// The website's sign-in page with a fresh single-use code (0.11.0, core [`acct::AutoSignIn`]):
    /// signs the site in to this device's account. The answer carries a secret: never log it.
    pub(crate) fn web_sign_in_url(&mut self, http: &dyn HttpClient, next: Option<&str>, now: i64) -> R<String> {
        self.load();
        let Some(site) = self.verify_base.clone() else { return Err(not_now("No website in this build.")) };
        let token = self.access(http, now)?;
        let r =
            acct::parse::<acct::WebSignInCode>(&acct_call(http, "POST", acct::WEB_SIGN_IN, Some(&token), Some("{}")));
        match r {
            Ok(c) => Ok(acct::auto_sign_in_url(&site, &c.code, next)),
            Err(f) => {
                // A stale access token: the next attempt renews it (the account stays; a removed
                // device is found by the refresh then).
                if f == Failure::SignedOut {
                    self.access = None;
                }
                Err(account_err(f))
            }
        }
    }

    /// Everything of the account and of sync off this device (D4): the tokens, the keys (also
    /// the vault's sync part), sync's bookkeeping; with `delete_data`, what came from sync too.
    fn forget_account(&mut self, delete_data: bool, message: &str) {
        let _ = std::fs::remove_file(self.account_path());
        self.account = None;
        self.access = None;
        self.linking = None;
        self.turn_off(delete_data);
        self.message = Some(message.into());
    }

    /// Sync off on this device: sync.json, the vault's sync part, the bookkeeping.
    fn turn_off(&mut self, delete_data: bool) {
        self.sync = None;
        let _ = std::fs::remove_file(self.state_path());
        self.set_pak(None);
        if let Ok(mut store) = self.store() {
            let _ = store.sync_forget(delete_data);
        }
        self.stage = Stage::Off;
        self.plan = None;
        self.escrow = None;
        self.restore = None;
        self.joining = None;
        self.backoff = Backoff::default();
        self.last_ok_ms = None;
        self.last_result = None;
        self.rotation_pending = false;
    }

    /// Signs out: local state goes at once, then the server is told (it revokes the device).
    /// `true` = the server confirmed (or already knew).
    pub(crate) fn sign_out(&mut self, http: &dyn HttpClient, delete_data: bool, now: i64) -> bool {
        self.load();
        if self.account.is_none() {
            self.linking = None;
            return true;
        }
        let token = self.access(http, now).ok();
        self.forget_account(delete_data, "Signed out on this device.");
        let reached = match token {
            Some(t) => match acct_call(http, "POST", acct::UNLINK, Some(&t), Some("{}")) {
                Ok((204 | 200, _)) => true,
                other => acct::failure(&other) == Failure::SignedOut,
            },
            None => false,
        };
        if !reached {
            self.message =
                Some("Signed out on this device. Krunk couldn't reach krunk.209.wf, so the device may still be listed on your account page.".into());
        }
        reached
    }

    // ---- turning sync on ----------------------------------------------------------------------

    fn api<'a>(http: &'a dyn HttpClient, token: &'a str, now: i64) -> Api<'a> {
        Api { http, token, now_ms: now }
    }

    /// Asks the account whether it syncs already: the first device gets its secrets (shown once).
    pub(crate) fn turn_on(&mut self, http: &dyn HttpClient, now: i64) -> R<TurnOn> {
        self.load();
        if self.sync.is_some() {
            return Err(not_now("Sync is already on."));
        }
        let (_, acc, device) = self.ids()?;
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        let st = client::state(&api).map_err(|e| sync_err(&e))?;
        let short = client::my_short(&api, &device).map_err(|e| sync_err(&e))?;
        let passwords = self.vault_open();
        if st.keyset_gen.is_some() {
            self.stage = Stage::Join { passwords };
            self.message = None;
            return Ok(TurnOn::Join);
        }
        let plan =
            keys::plan_setup(acc, Me { device_id: device, short }, passwords).map_err(|e| sync_err(&e.into()))?;
        let secrets =
            SetupSecretsText { passphrase: plan.passphrase.to_string(), recovery_key: plan.recovery_key.to_string() };
        self.plan = Some(plan);
        self.stage = Stage::Setup;
        self.message = None;
        Ok(TurnOn::Setup { secrets })
    }

    /// First device: the user typed the asked group of the recovery key back.
    pub(crate) fn setup_confirm(&mut self, http: &dyn HttpClient, group: String, now: i64) -> R<()> {
        if !matches!(self.stage, Stage::Setup) {
            return Err(not_now("Start again with Turn on sync."));
        }
        let ok = self.plan.as_ref().is_some_and(|p| keys::recovery_key_confirmed(&p.recovery_key, &group));
        if !ok {
            return Err(SyncFfiError::WrongCode {
                text: "Type the last group of 4 characters of your recovery key (before the last dash) to show you saved it."
                    .into(),
            });
        }
        let (account, _, device) = self.ids()?;
        let token = self.access(http, now)?;
        let plan = self.plan.take().ok_or_else(|| not_now("Start again with Turn on sync."))?;
        let api = Self::api(http, &token, now);
        match client::setup(&api, &plan) {
            Ok((joined, gen)) => {
                let me = Me { device_id: device, short: plan.me.short };
                self.started(client::new_state(&account, &me, &joined, gen), joined.pak.as_ref(), now)?;
                self.message = Some("Sync is set up. Keep your passphrase and recovery key safe.".into());
                Ok(())
            }
            Err(SyncFail::Api(ApiError::Conflict(_))) => {
                self.stage = Stage::Join { passwords: self.vault_open() };
                let text = "Another device has set up sync meanwhile. Restore or ask it to approve this one.";
                self.message = Some(text.into());
                Err(SyncFfiError::NotNow { text: text.into() })
            }
            Err(e) => {
                self.plan = Some(plan);
                Err(sync_err(&e))
            }
        }
    }

    fn started(&mut self, st: SyncState, pak: Option<&PakState>, now: i64) -> R<()> {
        self.sync = Some(st);
        self.save_state()?;
        if let Some(p) = pak {
            self.set_pak(Some(p));
        }
        self.store()?.sync_set_tracking(true).map_err(local_err)?;
        self.stage = Stage::On;
        self.next_run_ms = now;
        self.escrow = None;
        Ok(())
    }

    /// First device, the one-password way (0.21.1): the vault password protects the keys; no
    /// passphrase or recovery key is shown. The vault must be open (Kotlin creates or unlocks it
    /// with the same password first).
    pub(crate) fn setup_vault(&mut self, http: &dyn HttpClient, password: String, now: i64) -> R<()> {
        self.load();
        if self.sync.is_some() {
            return Ok(());
        }
        if !self.vault_open() {
            return Err(not_now("Unlock your passwords first (Passwords in the menu)."));
        }
        let (account, acc, device) = self.ids()?;
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        let short = client::my_short(&api, &device).map_err(|e| sync_err(&e))?;
        let pw = Zeroizing::new(password);
        let plan = keys::plan_setup_vault(acc, Me { device_id: device.clone(), short }, &pw)
            .map_err(|e| sync_err(&e.into()))?;
        match client::setup(&api, &plan) {
            Ok((joined, gen)) => {
                let me = Me { device_id: device, short };
                self.plan = None;
                self.started(client::new_state(&account, &me, &joined, gen), joined.pak.as_ref(), now)?;
                self.message = Some("Sync is on. On your other devices, sign in and type this vault password.".into());
                Ok(())
            }
            Err(SyncFail::Api(ApiError::Conflict(_))) => {
                self.stage = Stage::Join { passwords: true };
                let text = "Another device has set up sync meanwhile. Type your vault password to join it.";
                self.message = Some(text.into());
                Err(SyncFfiError::NotNow { text: text.into() })
            }
            Err(e) => Err(sync_err(&e)),
        }
    }

    /// The vault was created, unlocked or its password changed with `password` (0.21.1). With
    /// sync on: a phone without the passwords key gets it with this password (when the account
    /// has a vault-password copy); a phone with it makes the vault password the account's one
    /// secret when the server has no copy yet, the password `changed`, or a removal left the
    /// keys to re-secure. Errors are kept in the status (the vault screen shows nothing of it).
    pub(crate) fn vault_password(&mut self, http: &dyn HttpClient, password: String, changed: bool, now: i64) {
        self.load();
        let Some(mut st) = self.sync.clone() else { return };
        if !matches!(self.stage, Stage::On) || !self.vault_open() || password.is_empty() {
            return;
        }
        let Ok(token) = self.access(http, now) else { return };
        let api = Self::api(http, &token, now);
        let Ok(server) = client::state(&api) else { return };
        match self.pak() {
            None => {
                if !server.escrow_vault {
                    return;
                }
                if let Err(e) = self.add_passwords_with(http, SyncSecret::Vault(Zeroizing::new(password)), now) {
                    self.message = Some(e.text().to_string());
                }
            }
            Some(mut pak) => {
                if server.escrow_vault && !changed && !st.secure_needed {
                    return;
                }
                match client::set_vault_root(&api, &mut st, &mut pak, &password) {
                    Ok(()) => {
                        self.sync = Some(st);
                        let _ = self.save_state();
                        self.set_pak(Some(&pak));
                        self.rotation_pending = false;
                    }
                    Err(e) => {
                        self.last_result = Some(sync_text(&e));
                    }
                }
            }
        }
    }

    /// Restore, step 1 of 3: fetches the escrow (once per attempt: it is limited to 5 a day).
    pub(crate) fn restore_begin(&mut self, http: &dyn HttpClient, now: i64) -> R<()> {
        if !matches!(self.stage, Stage::Join { .. } | Stage::RestoreConfirm { .. }) {
            return Err(not_now("Start again with Turn on sync."));
        }
        if self.escrow.is_some() {
            return Ok(());
        }
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        self.escrow = Some(client::fetch_escrow(&api).map_err(|e| sync_err(&e))?);
        Ok(())
    }

    /// Step 2: the passphrase or recovery key, checked against the fetched escrow. A wrong one
    /// changes nothing (no request is made for it).
    pub(crate) fn restore_try(
        &mut self,
        http: &dyn HttpClient,
        secret: String,
        recovery: bool,
        now: i64,
    ) -> R<SyncStage> {
        let secret = secret_of(secret, recovery)?;
        self.restore_try_with(http, secret, now)
    }

    /// Restore with the vault password (0.21.1): the one secret users type.
    pub(crate) fn restore_vault(&mut self, http: &dyn HttpClient, password: String, now: i64) -> R<SyncStage> {
        if password.is_empty() || password.len() > MAX_SECRET {
            return Err(SyncFfiError::WrongSecret { text: WRONG_VAULT_PW.into() });
        }
        self.restore_try_with(http, SyncSecret::Vault(Zeroizing::new(password)), now)
    }

    fn restore_try_with(&mut self, http: &dyn HttpClient, secret: SyncSecret, now: i64) -> R<SyncStage> {
        let vault = matches!(secret, SyncSecret::Vault(_));
        if self.escrow.is_none() {
            self.restore_begin(http, now)?;
        }
        let (_, acc, _) = self.ids()?;
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        let escrow = self.escrow.as_ref().ok_or_else(|| not_now("Start again with Turn on sync."))?;
        let plan = match client::restore_with(&api, escrow, &secret, acc) {
            Ok(p) => p,
            Err(e) => {
                if matches!(e, SyncFail::Keys(KeyError::NoVaultWrap)) {
                    self.escrow = None;
                }
                return Err(if vault { vault_err(&e) } else { sync_err(&e) });
            }
        };
        let listed = client::devices(&api).map_err(|e| sync_err(&e))?;
        let devices: Vec<String> = plan
            .ak_dir
            .devices
            .iter()
            .map(|d| {
                listed
                    .iter()
                    .find(|x| x.id == d.id)
                    .map(|x| acct::display_label(&x.label))
                    .unwrap_or_else(|| format!("Device #{}", d.short))
            })
            .collect();
        let revoked = plan.ak_dir.revoked.len() as u32;
        self.restore = Some(plan);
        self.stage = Stage::RestoreConfirm { devices: devices.clone(), revoked };
        Ok(SyncStage::RestoreConfirm { devices, revoked })
    }

    /// Step 3: the user confirmed the devices that keep access.
    pub(crate) fn restore_confirm(&mut self, http: &dyn HttpClient, now: i64) -> R<()> {
        let Some(plan) = self.restore.take() else { return Err(not_now("Start again with Turn on sync.")) };
        let (account, _, device) = self.ids()?;
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        let with_vault = self.vault_open();
        let r = client::my_short(&api, &device).and_then(|short| {
            let me = Me { device_id: device, short };
            client::restore_finish(&api, &plan, &me, with_vault).map(|j| (me, j))
        });
        match r {
            Ok((me, (joined, gen))) => {
                self.started(client::new_state(&account, &me, &joined, gen), joined.pak.as_ref(), now)?;
                self.message = Some(if joined.pak.is_some() {
                    "Sync is restored on this device.".into()
                } else {
                    "Sync is restored on this device. Unlock your passwords and restore again to sync them too.".into()
                });
                Ok(())
            }
            Err(e) => {
                self.stage = Stage::Join { passwords: with_vault };
                Err(sync_err(&e))
            }
        }
    }

    /// Asks a device that syncs for the keys.
    pub(crate) fn join_start(&mut self, http: &dyn HttpClient, share_passwords: bool, now: i64) -> R<()> {
        if !matches!(self.stage, Stage::Join { .. }) {
            return Err(not_now("Start again with Turn on sync."));
        }
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        let with_vault = share_passwords && self.vault_open();
        self.joining = Some(NApproval::start(&api, with_vault).map_err(|e| sync_err(&e))?);
        self.stage = Stage::Joining { enter_code: false };
        // The screen says what to do on the other device.
        self.message = None;
        Ok(())
    }

    /// Polls the approval (every 3 s while the screen shows it).
    pub(crate) fn join_poll(&mut self, http: &dyn HttpClient, now: i64) -> R<SyncStage> {
        let Some(mut n) = self.joining.take() else { return Err(not_now("No request is waiting.")) };
        let (account, acc, device) = self.ids()?;
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        match n.poll(&api, &acc, &device, &self.label) {
            Ok(NStep::Ready) => {
                let r = n.finish(&api, acc, &device).and_then(|(j, p)| {
                    let gen = client::state(&api)?.keyset_gen.unwrap_or_default();
                    Ok((j, p, gen))
                });
                match r {
                    Ok((joined, payload, gen)) => {
                        let me = Me { device_id: device, short: payload.dev_short };
                        self.started(client::new_state(&account, &me, &joined, gen), joined.pak.as_ref(), now)?;
                        self.message = Some("This device is approved and syncs now.".into());
                        Ok(SyncStage::On)
                    }
                    Err(e) => {
                        self.stage = Stage::Join { passwords: false };
                        Err(sync_err(&e))
                    }
                }
            }
            Ok(NStep::EnterCode) => {
                self.joining = Some(n);
                self.stage = Stage::Joining { enter_code: true };
                self.message = Some("Type the 6 digits the other device shows.".into());
                Ok(SyncStage::Joining { enter_code: true })
            }
            Ok(NStep::Gone) => {
                self.stage = Stage::Join { passwords: false };
                self.message = Some("The request was cancelled or ran out of time. Ask again.".into());
                Ok(SyncStage::Join { passwords: false })
            }
            Ok(NStep::Waiting(_)) => {
                let enter_code = matches!(self.stage, Stage::Joining { enter_code: true });
                self.joining = Some(n);
                Ok(SyncStage::Joining { enter_code })
            }
            Err(e) => {
                self.joining = Some(n);
                Err(sync_err(&e))
            }
        }
    }

    /// The 6 digits shown on the approving device. A wrong code cancels the request (as on
    /// Windows): ask again.
    pub(crate) fn join_code(&mut self, http: &dyn HttpClient, code: String, now: i64) -> R<()> {
        if !matches!(self.stage, Stage::Joining { enter_code: true }) {
            return Err(not_now("Wait until the other device shows the code."));
        }
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(SyncFfiError::WrongCode { text: "Type the 6 digits the other device shows.".into() });
        }
        let Some(mut n) = self.joining.take() else { return Err(not_now("No request is waiting.")) };
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        match n.confirm(&api, &code) {
            Ok(()) => {
                self.joining = Some(n);
                self.stage = Stage::Joining { enter_code: false };
                self.message = Some("Waiting for the keys…".into());
                Ok(())
            }
            Err(e) => {
                self.stage = Stage::Join { passwords: false };
                let err = sync_err(&e);
                self.message = Some(err.text().into());
                Err(err)
            }
        }
    }

    /// Leaves the current step (setup, restore, a request to join).
    pub(crate) fn cancel(&mut self, http: &dyn HttpClient, now: i64) {
        self.plan = None;
        self.restore = None;
        self.escrow = None;
        if let Some(n) = self.joining.take() {
            if let Ok(token) = self.access(http, now) {
                n.cancel(&Self::api(http, &token, now));
            }
        }
        if let Some((n, _)) = self.asking.take() {
            if let Ok(token) = self.access(http, now) {
                n.cancel(&Self::api(http, &token, now));
            }
        }
        if self.sync.is_none() && self.account.is_some() {
            self.stage = Stage::Off;
        }
        self.message = None;
    }

    // ---- passwords on a device that syncs (0.6.1) ---------------------------------------------

    fn passwords_state(&self, collections: &[CollectionState]) -> PasswordsSync {
        if self.sync.is_none() {
            return PasswordsSync::Off;
        }
        if collections.iter().any(|c| c.name == "passwords" && !c.on) {
            return PasswordsSync::SwitchedOff;
        }
        match self.vault.as_ref().map(|v| v.state()) {
            None | Some(crate::vault::VaultState::None) => PasswordsSync::NoVault,
            Some(crate::vault::VaultState::Locked) => PasswordsSync::VaultLocked,
            Some(crate::vault::VaultState::Unlocked) => {
                if self.pak().is_some() {
                    PasswordsSync::On
                } else {
                    PasswordsSync::NotOnThisDevice
                }
            }
        }
    }

    /// "Add passwords": the passphrase or recovery key gives this device the passwords key, kept
    /// in the unlocked vault; the next run brings the logins. One escrow fetch serves every try
    /// (a wrong secret is retried locally, as in a restore).
    pub(crate) fn add_passwords(&mut self, http: &dyn HttpClient, secret: String, recovery: bool, now: i64) -> R<()> {
        let secret = secret_of(secret, recovery)?;
        self.add_passwords_with(http, secret, now)
    }

    pub(crate) fn add_passwords_with_pub(&mut self, http: &dyn HttpClient, secret: SyncSecret, now: i64) -> R<()> {
        self.add_passwords_with(http, secret, now)
    }

    fn add_passwords_with(&mut self, http: &dyn HttpClient, secret: SyncSecret, now: i64) -> R<()> {
        self.load();
        let Some(mut st) = self.sync.clone() else { return Err(not_now("Sync is off.")) };
        if !self.vault_open() {
            return Err(not_now("Unlock your passwords first (Passwords in the menu)."));
        }
        if self.pak().is_some() {
            return Ok(());
        }
        let vault = matches!(secret, SyncSecret::Vault(_));
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        if self.escrow.is_none() {
            self.escrow = Some(client::fetch_escrow(&api).map_err(|e| sync_err(&e))?);
        }
        let escrow = self.escrow.clone().ok_or_else(|| not_now("Try again."))?;
        let pak = client::add_passwords(&api, &mut st, &escrow, &secret).map_err(|e| {
            if matches!(e, SyncFail::Keys(KeyError::NoVaultWrap)) {
                // Fetched before the other device added its copy: fetch again next time.
                self.escrow = None;
            }
            if vault { vault_err(&e) } else { sync_err(&e) }
        })?;
        self.escrow = None;
        self.sync = Some(st);
        self.save_state()?;
        self.set_pak(Some(&pak));
        if self.pak().is_none() {
            return Err(local_err("the vault could not keep the passwords key"));
        }
        self.next_run_ms = now;
        self.message = Some("Passwords sync on this device now.".into());
        Ok(())
    }

    /// "Ask my PC" (0.6.2): asks a device that holds the passwords key for it. The other device
    /// approves on its Account & sync page and shows 6 digits, typed here
    /// ([`Self::passwords_ask_code`]).
    pub(crate) fn passwords_ask_start(&mut self, http: &dyn HttpClient, now: i64) -> R<()> {
        self.load();
        let Some(st) = self.sync.clone() else { return Err(not_now("Sync is off.")) };
        if !self.vault_open() {
            return Err(not_now("Unlock your passwords first (Passwords in the menu)."));
        }
        if self.pak().is_some() {
            return Ok(());
        }
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        if let Some((old, _)) = self.asking.take() {
            old.cancel(&api);
        }
        let n = NApproval::start_pak(&api, &st).map_err(|e| sync_err(&e))?;
        self.asking = Some((n, PasswordsAsk::Waiting));
        self.message = None;
        Ok(())
    }

    /// Polls the request (every 3 s while the screen shows it).
    pub(crate) fn passwords_ask_poll(&mut self, http: &dyn HttpClient, now: i64) -> R<PasswordsAsk> {
        let Some((mut n, state)) = self.asking.take() else { return Ok(PasswordsAsk::None) };
        let Some(st) = self.sync.clone() else { return Err(not_now("Sync is off.")) };
        let (_, acc, device) = self.ids()?;
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        match n.poll(&api, &acc, &device, &self.label) {
            Ok(NStep::Ready) => {
                if !self.vault_open() {
                    self.asking = Some((n, state));
                    return Err(not_now("Unlock your passwords to receive the key."));
                }
                let pak = n.finish_pak(&api, &st).map_err(|e| sync_err(&e))?;
                self.set_pak(Some(&pak));
                if self.pak().is_none() {
                    return Err(local_err("the vault could not keep the passwords key"));
                }
                self.next_run_ms = now;
                self.message = Some("Passwords sync on this device now.".into());
                Ok(PasswordsAsk::None)
            }
            Ok(NStep::EnterCode) => {
                let s = if state == PasswordsAsk::Confirmed { state } else { PasswordsAsk::EnterCode };
                self.asking = Some((n, s));
                Ok(s)
            }
            Ok(NStep::Gone) => {
                self.message = Some("The request was cancelled or ran out of time. Ask again.".into());
                Ok(PasswordsAsk::None)
            }
            Ok(NStep::Waiting(_)) => {
                self.asking = Some((n, state));
                Ok(state)
            }
            Err(e) => {
                self.asking = Some((n, state));
                Err(sync_err(&e))
            }
        }
    }

    /// The 6 digits the other device shows. A wrong code cancels the request: ask again.
    pub(crate) fn passwords_ask_code(&mut self, http: &dyn HttpClient, code: String, now: i64) -> R<()> {
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(SyncFfiError::WrongCode { text: "Type the 6 digits the other device shows.".into() });
        }
        let Some((mut n, state)) = self.asking.take() else { return Err(not_now("No request is waiting.")) };
        if state != PasswordsAsk::EnterCode {
            self.asking = Some((n, state));
            return Err(not_now("Wait until the other device shows the code."));
        }
        let token = self.access(http, now)?;
        let api = Self::api(http, &token, now);
        match n.confirm(&api, &code) {
            Ok(()) => {
                self.asking = Some((n, PasswordsAsk::Confirmed));
                self.message = Some("Waiting for the passwords key…".into());
                Ok(())
            }
            Err(e) => {
                self.message = None;
                Err(match sync_err(&e) {
                    SyncFfiError::WrongCode { .. } => SyncFfiError::WrongCode {
                        text: "That isn't the code the other device shows. The request was cancelled: ask again."
                            .into(),
                    },
                    other => other,
                })
            }
        }
    }

    // ---- running -------------------------------------------------------------------------------

    /// One run: the AK collections through the engine, then the passwords if the vault is open.
    pub(crate) fn run(&mut self, http: &dyn HttpClient, now: i64) -> R<RunReport> {
        self.load();
        let Some(mut st) = self.sync.clone() else { return Err(not_now("Sync is off.")) };
        if !matches!(self.stage, Stage::On) {
            return Err(not_now("Sync is stopped."));
        }
        self.next_run_ms = now + EVERY_MS;
        let token = match self.access(http, now) {
            Ok(t) => t,
            Err(e) => {
                self.last_result = Some(e.text().into());
                return Err(e);
            }
        };
        let api = Self::api(http, &token, now);
        let mut store = self.store()?;
        let off = local::off_collections(store.sync_conn());
        let opts = RunOpts { device_name: &self.label, off: &off, max_rounds: 20, confirm_rotation: false };
        let r = engine::run(&api, &mut store, &mut st, &opts);
        let mut pak_state = if off.iter().any(|c| c == "passwords") { None } else { self.pak() };
        let pak_changed = match (&r, pak_state.as_mut()) {
            (Ok(_), Some(p)) => rotate::pak_maintenance(&api, &mut st, p).unwrap_or(false),
            _ => false,
        };
        // The state is consistent after each step (a rotation's key, a cursor): kept even when a
        // later step failed.
        self.sync = Some(st);
        self.save_state()?;
        if pak_changed {
            self.set_pak(pak_state.as_ref());
        }
        match r {
            Ok(rep) => {
                self.backoff = Backoff::default();
                self.wait_until_ms = 0;
                self.last_ok_ms = Some(now);
                self.rotation_pending = rep.frozen;
                self.last_result = Some("Synced".into());
                self.message = if rep.frozen {
                    Some(
                        "Securing your account after a device was removed… Finish it on a Windows device that syncs."
                            .into(),
                    )
                } else if !rep.confirm.is_empty() {
                    Some("A removed device had added another device. Check the device list on Windows.".into())
                } else {
                    None
                };
                let mut out = RunReport {
                    pulled: rep.pulled as u32,
                    pushed: rep.pushed as u32,
                    conflicts: rep.conflicts as u32,
                    frozen: rep.frozen,
                    changed: SyncChanged {
                        bookmarks: rep.changed.bookmarks,
                        history: rep.changed.history,
                        settings: rep.changed.settings,
                        pins: rep.changed.pins,
                        allowlist: rep.changed.allowlist,
                        tabs: rep.changed.tabs,
                        ..Default::default()
                    },
                };
                if !off.iter().any(|c| c == "passwords") {
                    if let Ok((p, n)) = self.run_passwords(&api, &store, now) {
                        out.changed.passwords = p;
                        out.changed.never_save = n;
                    }
                }
                Ok(out)
            }
            Err(e) => Err(self.run_failed(&e, now)),
        }
    }

    /// The PAK collections (PROTOCOL §9.5), only while the vault is unlocked: encrypt what
    /// changed (vault open), exchange, merge back (vault open). `(passwords, never-save)` changed.
    fn run_passwords(&mut self, api: &Api, store: &Store, now: i64) -> R<(bool, bool)> {
        let (Some(vault), Some(mut pak_state), Some(st)) = (self.vault.clone(), self.pak(), self.sync.clone()) else {
            return Ok((false, false));
        };
        let acc = st.account_bytes().map_err(|_| not_now("no account"))?;
        let never = store.never_save_list().map_err(local_err)?;
        let mut hlc = Hlc::new(st.hlc);
        let out = vault
            .sync_view(|v| pak::prepare(v, &pak_state, &never, &acc, st.dev_short, &mut hlc, now))
            .ok_or_else(|| not_now("The vault is locked."))?
            .map_err(|e| sync_err(&e))?;
        let ex = pak::exchange(api, pak_state.book.pull_seq, &out.writes).map_err(|e| sync_err(&e))?;
        let primed = pak_state.book.primed;
        let rep = vault
            .sync_change(|v, w| pak::finish(v, &mut pak_state, store, &acc, out, ex, &mut hlc, now, w))
            .ok_or_else(|| not_now("The vault is locked."))?
            .map_err(|e| sync_err(&e))?;
        if let Some(s) = self.sync.as_mut() {
            s.hlc = s.hlc.max(hlc.last());
        }
        self.save_state()?;
        // The first passwords run only pulled: what is left here goes up at once.
        if !primed || rep.conflicts > 0 {
            self.next_run_ms = now;
        }
        Ok((rep.passwords, rep.never_save))
    }

    /// The schedule after a failed run, and what the screen says (Windows' `sync_run_failed`).
    fn run_failed(&mut self, e: &SyncFail, now: i64) -> SyncFfiError {
        let err = sync_err(e);
        let mut b = self.backoff;
        let next = match e {
            SyncFail::Api(api) => sapi::next_step(api, &mut b),
            SyncFail::Local(_) | SyncFail::BadAnswer(_) => sapi::Next::RetryIn(b.fail()),
            SyncFail::Keys(k) if !k.is_alarm() => sapi::Next::RetryIn(b.fail()),
            SyncFail::Keys(_) | SyncFail::Reset => sapi::Next::Stop,
        };
        self.backoff = b;
        let text = match (e, next) {
            (SyncFail::Keys(k), _) if k.is_alarm() => {
                if let Some(s) = self.sync.as_mut() {
                    s.alarm = Some(err.text().into());
                }
                let _ = self.save_state();
                self.stage = Stage::Alarm;
                err.text().to_string()
            }
            (_, sapi::Next::SignedOut) => {
                // The token was refused mid-run, or this device is no longer in the directory.
                self.forget_account(false, REMOVED_TEXT);
                return SyncFfiError::Revoked { text: REMOVED_TEXT.into() };
            }
            (SyncFail::Api(ApiError::Offline(_)), sapi::Next::RetryIn(ms)) => {
                self.wait_until_ms = now + ms;
                format!("Offline — will retry in {} min", (ms + 59_999) / 60_000)
            }
            (_, sapi::Next::RetryIn(ms)) => {
                self.wait_until_ms = now + ms;
                self.next_run_ms = now + ms;
                format!("{} Sync will try again in {} min.", err.text(), (ms + 59_999) / 60_000)
            }
            (_, sapi::Next::Frozen) => {
                self.rotation_pending = true;
                "Securing your account after a device was removed…".into()
            }
            (_, sapi::Next::Reconcile) => {
                self.next_run_ms = now;
                "Catching up…".into()
            }
            (_, sapi::Next::Stop) => err.text().to_string(),
        };
        self.last_result = Some(text);
        err
    }

    /// A local change happened: the next run comes 30 s later (or sooner if one was due).
    pub(crate) fn local_changed(&mut self, now: i64) {
        if self.sync.is_some() {
            self.next_run_ms = self.next_run_ms.min(now + AFTER_CHANGE_MS);
        }
    }

    /// The vault was unlocked: passwords that waited come with the next run; a sync part left
    /// from a sign-out while it was locked goes now.
    pub(crate) fn vault_unlocked(&mut self, now: i64) {
        self.load();
        if self.sync.is_none() {
            if self.pak().is_some() {
                self.set_pak(None);
            }
        } else if self.pak().is_some() {
            self.next_run_ms = now;
        }
    }

    pub(crate) fn set_collection(&mut self, name: String, on: bool, now: i64) -> R<()> {
        if self.sync.is_none() || !COLLECTIONS.contains(&name.as_str()) {
            return Err(not_now("Sync is off."));
        }
        let store = self.store()?;
        let c = store.sync_conn();
        let mut off = local::off_collections(c);
        off.retain(|n| *n != name);
        if !on {
            off.push(name);
        }
        local::meta_set(c, "off", &serde_json::to_string(&off).unwrap_or_default()).map_err(local_err)?;
        if on {
            local::meta_set(c, "reconcile", "1").map_err(local_err)?;
        }
        self.next_run_ms = now;
        Ok(())
    }

    /// The open tabs the other devices last reported (web pages only).
    pub(crate) fn remote_tabs(&mut self) -> Vec<DeviceTabs> {
        self.load();
        let (Some(st), Ok(store)) = (&self.sync, self.store()) else { return Vec::new() };
        let c = store.sync_conn();
        let names = coll::device_names(c).unwrap_or_default();
        let mut out = Vec::new();
        for (device, v) in coll::remote_tabs(c).unwrap_or_default() {
            if device == st.device_id {
                continue;
            }
            let tabs: Vec<RemoteTab> = v["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|w| w["tabs"].as_array().cloned().unwrap_or_default())
                .filter_map(|t| {
                    let url = t["url"].as_str()?;
                    let ok = url.len() <= 8192 && (url.starts_with("https://") || url.starts_with("http://"));
                    ok.then(|| RemoteTab {
                        url: url.to_string(),
                        title: t["title"]
                            .as_str()
                            .unwrap_or_default()
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(512)
                            .collect(),
                    })
                })
                .take(coll_tabs_max())
                .collect();
            let name =
                names.get(&device).cloned().map(|n| acct::display_label(&n)).unwrap_or_else(|| "Another device".into());
            out.push(DeviceTabs { device_id: device, name, updated_ms: v["updated"].as_i64().unwrap_or(0), tabs });
        }
        out.sort_by_key(|d| std::cmp::Reverse(d.updated_ms));
        out
    }
}

fn coll_tabs_max() -> usize {
    500
}

// ------------------------------------------------------------------------------ the object

/// Krunk account and sync. One per app; methods block (call them off the main thread) and run
/// one at a time; [`status`](Self::status) never touches the network.
#[derive(uniffi::Object)]
pub struct KrunkSync {
    transport: Arc<dyn SyncTransport>,
    engine: Mutex<Engine>,
    /// The site the website sign-in goes to (`verify_base`; none in test builds).
    site: Option<String>,
    /// Signed in to the account, as of the end of the last engine call (read on the main thread
    /// by [`auto_sign_in_check`](Self::auto_sign_in_check), which must not wait for the engine).
    signed_in: AtomicBool,
    /// The one website sign-in guard of the app process (core [`acct::AutoSignIn`]).
    auto: Mutex<acct::AutoSignIn>,
}

/// A navigation that becomes a website sign-in (0.11.0): `next`, the sign-in page's own `next`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AutoSignInTarget {
    pub next: Option<String>,
}

/// The engine while a call runs; afterwards [`KrunkSync::signed_in`] follows its account.
struct EngineGuard<'a> {
    engine: MutexGuard<'a, Engine>,
    signed_in: &'a AtomicBool,
}

impl std::ops::Deref for EngineGuard<'_> {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

impl std::ops::DerefMut for EngineGuard<'_> {
    fn deref_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

impl Drop for EngineGuard<'_> {
    fn drop(&mut self) {
        if self.engine.loaded {
            self.signed_in.store(self.engine.account.is_some(), Ordering::Relaxed);
        }
    }
}

impl std::fmt::Debug for KrunkSync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KrunkSync")
    }
}

impl KrunkSync {
    fn engine(&self) -> EngineGuard<'_> {
        EngineGuard { engine: self.engine.lock().unwrap_or_else(|e| e.into_inner()), signed_in: &self.signed_in }
    }
}

#[uniffi::export]
impl KrunkSync {
    /// `dir`: the app-private `files/krunk-sync`; `db`: browser.db; `wrapper`: the Keystore key
    /// `krunk-sync-state`; `vault`: the app's vault (passwords sync while it is unlocked);
    /// `label`: "Krunk on Android 16"; `verify_base`: the site (release: `https://krunk.209.wf`),
    /// `None` only in test builds.
    #[uniffi::constructor]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dir: String,
        db: String,
        transport: Arc<dyn SyncTransport>,
        wrapper: Arc<dyn VaultKeyWrapper>,
        vault: Option<Arc<KrunkVault>>,
        label: String,
        version: String,
        verify_base: Option<String>,
    ) -> Arc<Self> {
        let site = verify_base.clone();
        let engine = Engine::new(
            PathBuf::from(dir),
            PathBuf::from(db),
            Box::new(ForeignWrapper(wrapper)),
            vault,
            label,
            version,
            verify_base,
        );
        Arc::new(KrunkSync {
            transport,
            engine: Mutex::new(engine),
            site,
            signed_in: AtomicBool::new(false),
            auto: Mutex::new(acct::AutoSignIn::default()),
        })
    }

    pub fn status(&self) -> SyncStatus {
        self.engine().status()
    }

    pub fn link_start(&self) -> Result<LinkInfo, SyncFfiError> {
        self.engine().link_start(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn link_poll(&self) -> Result<LinkPoll, SyncFfiError> {
        self.engine().link_poll(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn link_cancel(&self) {
        self.engine().link_cancel()
    }

    /// `remove_synced_data`: also delete what came from sync (bookmarks, history … that another
    /// device added). `true` = the server confirmed.
    pub fn sign_out(&self, remove_synced_data: bool) -> bool {
        self.engine().sign_out(&Foreign(self.transport.as_ref()), remove_synced_data, crate::now_ms())
    }

    pub fn turn_on(&self) -> Result<TurnOn, SyncFfiError> {
        self.engine().turn_on(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn setup_confirm(&self, group: String) -> Result<(), SyncFfiError> {
        self.engine().setup_confirm(&Foreign(self.transport.as_ref()), group, crate::now_ms())
    }

    pub fn restore_begin(&self) -> Result<(), SyncFfiError> {
        self.engine().restore_begin(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn restore_try(&self, secret: String, is_recovery_key: bool) -> Result<SyncStage, SyncFfiError> {
        self.engine().restore_try(&Foreign(self.transport.as_ref()), secret, is_recovery_key, crate::now_ms())
    }

    pub fn restore_confirm(&self) -> Result<(), SyncFfiError> {
        self.engine().restore_confirm(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn join_start(&self, share_passwords: bool) -> Result<(), SyncFfiError> {
        self.engine().join_start(&Foreign(self.transport.as_ref()), share_passwords, crate::now_ms())
    }

    pub fn join_poll(&self) -> Result<SyncStage, SyncFfiError> {
        self.engine().join_poll(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn join_code(&self, code: String) -> Result<(), SyncFfiError> {
        self.engine().join_code(&Foreign(self.transport.as_ref()), code, crate::now_ms())
    }

    pub fn cancel(&self) {
        self.engine().cancel(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn run_now(&self) -> Result<RunReport, SyncFfiError> {
        self.engine().run(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn local_changed(&self) {
        self.engine().local_changed(crate::now_ms())
    }

    pub fn vault_unlocked(&self) {
        self.engine().vault_unlocked(crate::now_ms())
    }

    pub fn set_collection(&self, name: String, on: bool) -> Result<(), SyncFfiError> {
        self.engine().set_collection(name, on, crate::now_ms())
    }

    /// Join with the vault password (0.21.1): goes on to the devices to confirm, as a restore.
    pub fn restore_vault(&self, password: String) -> Result<SyncStage, SyncFfiError> {
        self.engine().restore_vault(&Foreign(self.transport.as_ref()), password, crate::now_ms())
    }

    /// First device: sync on, protected by the vault password (0.21.1).
    pub fn setup_vault(&self, password: String) -> Result<(), SyncFfiError> {
        self.engine().setup_vault(&Foreign(self.transport.as_ref()), password, crate::now_ms())
    }

    /// Passwords on a phone that syncs without them, with the vault password (0.21.1).
    pub fn add_passwords_vault(&self, password: String) -> Result<(), SyncFfiError> {
        if password.is_empty() || password.len() > MAX_SECRET {
            return Err(SyncFfiError::WrongSecret { text: WRONG_VAULT_PW.into() });
        }
        self.engine().add_passwords_with_pub(
            &Foreign(self.transport.as_ref()),
            SyncSecret::Vault(Zeroizing::new(password)),
            crate::now_ms(),
        )
    }

    /// Call after the vault was created, unlocked with its password, or the password changed.
    pub fn vault_password(&self, password: String, changed: bool) {
        self.engine().vault_password(&Foreign(self.transport.as_ref()), password, changed, crate::now_ms())
    }

    /// "Add passwords" (0.6.1): `secret` is the passphrase or the recovery key.
    pub fn add_passwords(&self, secret: String, is_recovery_key: bool) -> Result<(), SyncFfiError> {
        self.engine().add_passwords(&Foreign(self.transport.as_ref()), secret, is_recovery_key, crate::now_ms())
    }

    /// "Ask my PC" (0.6.2): asks a device that holds the passwords key for it.
    pub fn passwords_ask_start(&self) -> Result<(), SyncFfiError> {
        self.engine().passwords_ask_start(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn passwords_ask_poll(&self) -> Result<PasswordsAsk, SyncFfiError> {
        self.engine().passwords_ask_poll(&Foreign(self.transport.as_ref()), crate::now_ms())
    }

    pub fn passwords_ask_code(&self, code: String) -> Result<(), SyncFfiError> {
        self.engine().passwords_ask_code(&Foreign(self.transport.as_ref()), code, crate::now_ms())
    }

    pub fn remote_tabs(&self) -> Vec<DeviceTabs> {
        self.engine().remote_tabs()
    }

    /// Website sign-in (0.11.0), for every top-level navigation of a tab: `Some` when Krunk
    /// should cancel it and open [`web_sign_in_url`](Self::web_sign_in_url) instead. No network,
    /// no files; main thread. "Signed in" is the state after the last call ([`status`](Self::status)
    /// runs at every app start), so it never waits for a running sync.
    pub fn auto_sign_in_check(&self, url: String, private: bool) -> Option<AutoSignInTarget> {
        let site = self.site.as_deref()?;
        let signed_in = self.signed_in.load(Ordering::Relaxed);
        let mut auto = self.auto.lock().unwrap_or_else(|e| e.into_inner());
        auto.check(&url, site, crate::now_ms(), signed_in, private).map(|next| AutoSignInTarget { next })
    }

    /// The website's sign-in page with a fresh single-use code (blocking: off the main thread).
    /// The address is a secret: never log it.
    pub fn web_sign_in_url(&self, next: Option<String>) -> Result<String, SyncFfiError> {
        self.engine().web_sign_in_url(&Foreign(self.transport.as_ref()), next.as_deref(), crate::now_ms())
    }
}
