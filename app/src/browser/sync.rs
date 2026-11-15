//! Sync (beta, 0.9.0): `krunk://sync`, Settings → Sync (beta), and the work behind them
//! (`lb_core::sync::client` over WinHTTP).
//!
//! **Opt-in is absolute.** Nothing here sends a request until the user clicks *Turn on sync*:
//! until then the views come from local files only (`account.json`, `sync.json`). Once sync is
//! on, requests go out while `krunk://sync` is open (approvals, devices, keys) and, from S7 on,
//! on the sync schedule. Turning sync off stops all of it.
//!
//! **Secrets.** The passphrase and the recovery key exist in memory during setup only and are
//! shown only in `krunk://sync`. Sync's keys live in `sync.json` (DPAPI, "Krunk sync state v1")
//! and, for passwords, inside the vault. Nothing secret is logged.

use super::Browser;
use crate::ffi::http::{self, ProxyMode};
use crate::ffi::secure::{self, Dpapi, DpapiSync};
use crate::log::{info, warning};
use lb_core::account::Failure;
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::storage::now_ms;
use lb_core::sync::api::{self as sapi, ApiError, Backoff, HttpClient, HttpResponse};
use lb_core::sync::client::{
    self, AApproval, AStep, Api, ApprovalItem, DeviceWire, NApproval, NStep, Refresh, SyncFail,
};
use lb_core::sync::coll;
use lb_core::sync::engine::{self, Report, RunOpts};
use lb_core::sync::hlc::Hlc;
use lb_core::sync::keys::{self, KeyError, Me, PakState, RestorePlan, Secret as SyncSecret, SetupPlan};
use lb_core::sync::local;
use lb_core::sync::pak::{self, Outgoing};
use lb_core::sync::poll::{self as spoll, Poll, PollState};
use lb_core::sync::state::{self, parse_account_id, SyncState};
use lb_core::sync::view::{ApprovalRow, Approving, CollectionRow, DeviceRow, OnView, RemoteTabs, SyncView};
use lb_core::tabs::TabId;
use lb_core::vault::VaultSync;
use std::cell::{Cell, RefCell};
use zeroize::Zeroizing;

const TIMEOUT_MS: i32 = 30_000;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
/// While krunk://sync is open: a tick every 3 s; what it asks is core::sync::poll's schedule.
const POLL_MS: i64 = spoll::TICK_MS;
/// The schedule (PLAN §5.3): a run a minute after start, every 5 minutes, and 30 s after a
/// local change (open tabs wait for the 5-minute run); a 30 s tick checks.
const START_DELAY_MS: i64 = 60_000;
const EVERY_MS: i64 = 5 * 60_000;
const AFTER_CHANGE_MS: i64 = 30_000;
const TICK_MS: i64 = 30_000;
/// The collections a device can switch off, with their labels.
const COLLECTIONS: [(&str, &str); 7] = [
    ("bookmarks", "Bookmarks"),
    ("passwords", "Passwords (while the vault is unlocked)"),
    ("history", "History"),
    ("settings", "Settings"),
    ("pins", "New tab pins"),
    ("allowlist", "Ad-blocker exceptions"),
    ("tabs", "Open tabs"),
];
const SOCKS_TEXT: &str = "Sync can't work through a SOCKS proxy. Change the proxy in Settings → Advanced.";
const OFFLINE_TEXT: &str = "Krunk couldn't reach krunk.209.wf. Check your connection and try again.";

fn base() -> &'static str {
    env!("KRUNK_UPDATE_BASE_URL")
}

/// The transport of `core::sync` on Windows.
struct WinHttp {
    proxy: ProxyMode,
}

impl HttpClient for WinHttp {
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, String> {
        let agent = format!("Krunk/{}", env!("CARGO_PKG_VERSION"));
        let opts = http::Options {
            proxy: &self.proxy,
            timeout_ms: TIMEOUT_MS,
            user_agent: &agent,
            allow_http: cfg!(feature = "dev-updates"),
        };
        let auth = bearer.map(|t| Zeroizing::new(format!("Bearer {t}")));
        let mut headers = vec![("Content-Type", "application/json"), ("Accept", "application/json")];
        if let Some(a) = &auth {
            headers.push(("Authorization", a.as_str()));
        }
        let url = format!("{}/api/v1/{path}", base());
        let r = http::request(method, &url, &headers, body, MAX_RESPONSE, &opts)?;
        Ok(HttpResponse { status: r.status as u16, body: r.body, retry_after: r.retry_after, date: r.date })
    }
}

/// Where this device stands with sync.
enum Stage {
    /// Sync is off here (nothing sent). `message`: why it went off, if it did by itself.
    Off(Option<String>),
    Working(String),
    Setup {
        passwords: bool,
        message: Option<String>,
    },
    Join {
        passwords: bool,
        message: Option<String>,
    },
    RestoreConfirm {
        devices: Vec<String>,
        revoked: usize,
    },
    Joining {
        enter_code: bool,
        message: Option<String>,
    },
    On(Option<String>),
    Alarm(String),
}

pub(super) struct SyncHost {
    /// `sync.json` was read (once, lazily; no network).
    loaded: Cell<bool>,
    state: RefCell<Option<SyncState>>,
    stage: RefCell<Stage>,
    /// Bumped by every new action, so answers to an abandoned one are ignored.
    generation: Cell<u32>,
    plan: RefCell<Option<SetupPlan>>,
    restore: RefCell<Option<RestorePlan>>,
    joining: RefCell<Option<NApproval>>,
    /// The approval this device answers, its code once known, and whether passwords are shared.
    approving: RefCell<Option<(AApproval, Option<String>, bool)>>,
    /// The device just approved, and whether it got only the passwords key.
    approving_done: RefCell<Option<(String, bool)>>,
    devices: RefCell<Vec<DeviceWire>>,
    approvals: RefCell<Vec<ApprovalItem>>,
    rotation_pending: Cell<bool>,
    /// What krunk://sync polls when (core::sync::poll).
    poll_state: Cell<PollState>,
    polling: Cell<bool>,
    busy: Cell<bool>,
    // The schedule (S7).
    ticking: Cell<bool>,
    running: Cell<bool>,
    next_run_ms: Cell<i64>,
    wait_until_ms: Cell<i64>,
    backoff: Cell<Backoff>,
    last_ok_ms: Cell<Option<i64>>,
    status: RefCell<Option<String>>,
    // Rotation (S9): the user confirmed the devices that stay (or removed one here), and the
    // devices waiting for that confirmation.
    confirm_rotation: Cell<bool>,
    confirm_devices: RefCell<Vec<String>>,
}

impl Default for SyncHost {
    fn default() -> Self {
        SyncHost {
            loaded: Cell::new(false),
            state: RefCell::new(None),
            stage: RefCell::new(Stage::Off(None)),
            generation: Cell::new(0),
            plan: RefCell::new(None),
            restore: RefCell::new(None),
            joining: RefCell::new(None),
            approving: RefCell::new(None),
            approving_done: RefCell::new(None),
            devices: RefCell::new(Vec::new()),
            approvals: RefCell::new(Vec::new()),
            rotation_pending: Cell::new(false),
            poll_state: Cell::new(PollState::default()),
            polling: Cell::new(false),
            busy: Cell::new(false),
            ticking: Cell::new(false),
            running: Cell::new(false),
            next_run_ms: Cell::new(0),
            wait_until_ms: Cell::new(0),
            backoff: Cell::new(Backoff::default()),
            last_ok_ms: Cell::new(None),
            status: RefCell::new(None),
            confirm_rotation: Cell::new(false),
            confirm_devices: RefCell::new(Vec::new()),
        }
    }
}

/// Whether typed text can be the sync passphrase (7 words) or a recovery key at all: checked
/// before the escrow is fetched, which is rate-limited and mailed to the user (0.21.1).
fn secret_shaped(text: &str, recovery: bool) -> bool {
    if recovery {
        lb_core::sync::codes::parse_recovery_key(text).is_ok()
    } else {
        lb_core::sync::keys::looks_like_passphrase(text)
    }
}

const NOT_A_SECRET: &str = "That isn't your sync passphrase: it is 7 words that Krunk showed once when sync was \
     first turned on (or use the recovery key, KRNK-…). Your account and vault passwords don't work here.";

fn fail_text(e: &SyncFail) -> String {
    use lb_core::sync::api::ApiError as A;
    match e {
        SyncFail::Api(A::Offline(_)) => OFFLINE_TEXT.into(),
        SyncFail::Api(A::Unauthorized) => "This device is signed out of the Krunk account. Sign in again in Settings → Account.".into(),
        SyncFail::Api(A::RateLimited(s)) => format!("Too many attempts. Try again in {} minutes.", s.div_ceil(60)),
        SyncFail::Api(A::Conflict(_)) => "Someone changed sync's keys meanwhile. Try again.".into(),
        SyncFail::Api(A::RotationPending) => {
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
        _ => "Something went wrong with sync. Try again later.".into(),
    }
}

impl Browser {
    // ---- local state (no network) ------------------------------------------------------------

    fn sync_load(&self) {
        if self.sync.loaded.replace(true) {
            return;
        }
        match state::load(&self.dirs.sync(), &DpapiSync) {
            Ok(Some(st)) => {
                let stage = match &st.alarm {
                    Some(a) => Stage::Alarm(a.clone()),
                    None => Stage::On(None),
                };
                *self.sync.state.borrow_mut() = Some(st);
                *self.sync.stage.borrow_mut() = stage;
            }
            Ok(None) => {}
            Err(e) => warning!("sync: sync.json could not be read: {e}"),
        }
    }

    pub(super) fn sync_is_on(&self) -> bool {
        self.sync_load();
        self.sync.state.borrow().is_some()
    }

    fn sync_save(&self) {
        if let Some(st) = self.sync.state.borrow().as_ref() {
            if let Err(e) = state::save(&self.dirs.sync(), st, &DpapiSync) {
                warning!("sync: sync.json could not be written: {e}");
            }
        }
    }

    /// The vault's sync part, if the vault is unlocked and holds one.
    fn sync_pak(&self) -> Option<PakState> {
        let v = self.vaults.vault.borrow();
        let part = v.as_ref()?.sync_part()?;
        PakState::from_bytes(&part.state).ok()
    }

    fn sync_set_pak(&self, pak: Option<&PakState>) {
        let mut v = self.vaults.vault.borrow_mut();
        let Some(vault) = v.as_mut() else { return };
        let part = pak.map(|p| VaultSync {
            state: p.to_bytes(),
            dirty: vault.sync_part().map(|x| x.dirty.clone()).unwrap_or_default(),
        });
        if let Err(e) = vault.set_sync_part(part, &Dpapi) {
            warning!("sync: the vault could not be updated: {e}");
        }
    }

    fn set_sync_stage(&self, stage: Stage) {
        *self.sync.stage.borrow_mut() = stage;
        self.push_sync();
    }

    fn sync_view(&self) -> SyncView {
        self.sync_load();
        if self.account_ids().is_none() {
            return SyncView::NoAccount;
        }
        match &*self.sync.stage.borrow() {
            Stage::Off(m) => SyncView::Off { message: m.clone() },
            Stage::Working(m) => SyncView::Working { message: m.clone() },
            Stage::Setup { passwords, message } => {
                let plan = self.sync.plan.borrow();
                match plan.as_ref() {
                    Some(p) => SyncView::Setup {
                        passphrase: p.passphrase.to_string(),
                        recovery_key: p.recovery_key.to_string(),
                        passwords: *passwords,
                        message: message.clone(),
                    },
                    None => SyncView::Off { message: None },
                }
            }
            Stage::Join { passwords, message } => SyncView::Join { message: message.clone(), passwords: *passwords },
            Stage::RestoreConfirm { devices, revoked } => {
                SyncView::RestoreConfirm { devices: devices.clone(), revoked: *revoked }
            }
            Stage::Joining { enter_code, message } => {
                SyncView::Joining { enter_code: *enter_code, message: message.clone() }
            }
            Stage::Alarm(m) => SyncView::Alarm { message: m.clone() },
            Stage::On(message) => {
                let st = self.sync.state.borrow();
                let me = st.as_ref().map(|s| s.device_id.clone()).unwrap_or_default();
                let (off, names, remote) = {
                    let store = self.store.borrow();
                    let c = store.sync_conn();
                    (
                        local::off_collections(c),
                        coll::device_names(c).unwrap_or_default(),
                        coll::remote_tabs(c).unwrap_or_default(),
                    )
                };
                let mut devices: Vec<DeviceRow> = self
                    .sync
                    .devices
                    .borrow()
                    .iter()
                    .filter(|d| d.revoked_at.is_none())
                    .map(|d| DeviceRow {
                        id: d.id.clone(),
                        name: names.get(&d.id).cloned().unwrap_or_else(|| lb_core::account::display_label(&d.label)),
                        platform: d.platform.clone(),
                        keys: d.has_keys,
                        me: d.id == me,
                        last_sync: d.last_sync.clone(),
                    })
                    .collect();
                let approvals = self
                    .sync
                    .approvals
                    .borrow()
                    .iter()
                    .filter(|a| a.state == "requested")
                    .map(|a| ApprovalRow {
                        id: a.approval_id.clone(),
                        label: lb_core::account::display_label(&a.device.label),
                        platform: a.device.platform.clone(),
                        country: a.device.country.clone(),
                        passwords_only: a.wants_pak(),
                    })
                    .collect();
                let approving = match (&*self.sync.approving.borrow(), &*self.sync.approving_done.borrow()) {
                    (Some((a, code, _)), _) => Some(Approving {
                        id: a.id.clone(),
                        label: a.device.label.clone(),
                        code: code.clone(),
                        done: false,
                        passwords_only: a.asked_pak,
                    }),
                    (None, Some((label, passwords_only))) => Some(Approving {
                        id: String::new(),
                        label: label.clone(),
                        code: None,
                        done: true,
                        passwords_only: *passwords_only,
                    }),
                    _ => None,
                };
                // This device first, then by name: the order doesn't jump between refreshes.
                devices
                    .sort_by(|a, b| (!a.me, a.name.to_lowercase(), &a.id).cmp(&(!b.me, b.name.to_lowercase(), &b.id)));
                let remote_tabs = remote
                    .iter()
                    .filter(|(id, _)| **id != me && devices.iter().any(|d| d.id == **id))
                    .map(|(id, v)| RemoteTabs {
                        device: devices.iter().find(|d| d.id == *id).map(|d| d.name.clone()).unwrap_or_default(),
                        updated_ms: v["updated"].as_i64().unwrap_or(0),
                        tabs: v["windows"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .flat_map(|w| w["tabs"].as_array().cloned().unwrap_or_default())
                            .map(|t| {
                                (
                                    t["title"].as_str().unwrap_or_default().to_string(),
                                    t["url"].as_str().unwrap_or_default().to_string(),
                                )
                            })
                            .filter(|(_, u)| u.starts_with("https://") || u.starts_with("http://"))
                            .collect(),
                    })
                    .collect();
                let syncing = self.sync.running.get();
                let status = match (syncing, self.sync.status.borrow().clone(), self.sync.last_ok_ms.get()) {
                    (true, _, _) => "Syncing…".to_string(),
                    (false, Some(s), _) => s,
                    (false, None, Some(_)) => "Up to date".into(),
                    (false, None, None) => "Sync is on. The first sync starts shortly.".into(),
                };
                let confirm_devices = self
                    .sync
                    .confirm_devices
                    .borrow()
                    .iter()
                    .map(|id| devices.iter().find(|d| d.id == *id).map_or_else(|| id.clone(), |d| d.name.clone()))
                    .collect();
                let vault_open = self.vault_unlocked();
                SyncView::On(OnView {
                    status,
                    last_sync_ms: self.sync.last_ok_ms.get(),
                    passwords: vault_open && self.sync_pak().is_some(),
                    passwords_locked: self.vault_exists() && !vault_open,
                    devices,
                    approvals,
                    approving,
                    rotation_pending: self.sync.rotation_pending.get(),
                    collections: COLLECTIONS
                        .iter()
                        .map(|(n, l)| CollectionRow {
                            name: n.to_string(),
                            label: l.to_string(),
                            on: !off.iter().any(|o| o == n),
                        })
                        .collect(),
                    message: message.clone(),
                    syncing,
                    remote_tabs,
                    confirm_devices,
                    secure_needed: st.as_ref().is_some_and(|s| s.secure_needed) && !self.sync.rotation_pending.get(),
                })
            }
        }
    }

    /// Every krunk://sync and Settings page gets the view (private ones get nothing).
    pub(super) fn push_sync(&self) {
        let view = self.sync_view();
        let tabs: Vec<(TabId, bool, Page)> = self
            .ws
            .borrow()
            .tabs()
            .filter_map(|(_, t)| match Page::from_url(&t.url) {
                Some(p @ (Page::Sync | Page::Settings)) => Some((t.id, t.private, p)),
                _ => None,
            })
            .collect();
        for (id, private, page) in tabs {
            let v = if private {
                SyncView::Private
            } else if page == Page::Settings {
                view.for_settings()
            } else {
                view.clone()
            };
            if let Some(tv) = self.tab_view(id) {
                tv.post_json(&Event::Sync { view: v }.to_json());
            }
        }
    }

    /// The account was signed out (the server said so): sync stops here too.
    pub(super) fn sync_signed_out(&self) {
        if self.sync.state.borrow().is_some() {
            info!("sync: the account was signed out; sync stops on this device");
        }
        self.push_sync();
    }

    fn sync_open_tabs(&self) -> bool {
        self.ws.borrow().tabs().any(|(_, t)| !t.private && Page::from_url(&t.url) == Some(Page::Sync))
    }

    // ---- requests -------------------------------------------------------------------------------

    pub(super) fn dispatch_sync(&self, tab: Option<TabId>, request: Request, req_id: Option<u32>) {
        let Some(tab) = tab else { return };
        if self.tab(tab).is_some_and(|t| t.private) {
            if let Some(v) = self.tab_view(tab) {
                v.post_json(&Event::Sync { view: SyncView::Private }.to_json());
            }
            return;
        }
        if self.account_ids().is_none() && !matches!(request, Request::SyncState | Request::SyncOpen) {
            self.push_sync();
            return;
        }
        match request {
            Request::SyncState => {
                // A page (re)opened: the list of devices asking to join comes at once.
                let mut ps = self.sync.poll_state.get();
                ps.opened();
                self.sync.poll_state.set(ps);
                self.push_sync();
                if self.sync_is_on() && self.sync_open_tabs() {
                    self.sync_poll_start();
                }
            }
            Request::SyncOpen => self.open_page(Page::Sync),
            Request::SyncTurnOn { passwords } => self.sync_turn_on(passwords),
            Request::SyncSetupConfirm { group, understood } => self.sync_setup_confirm(&group, understood),
            Request::SyncSetupCopy => self.sync_setup_copy(tab, req_id),
            Request::SyncSetupSave => self.sync_setup_save(tab, req_id),
            Request::SyncRestore { secret, recovery } => self.sync_restore(secret, recovery),
            Request::SyncRestoreConfirm => self.sync_restore_confirm(),
            Request::SyncJoin { passwords } => self.sync_join(passwords),
            Request::SyncJoinCode { code } => self.sync_join_code(code),
            Request::SyncCancel => self.sync_cancel(),
            Request::SyncApprove { id, share_passwords } => self.sync_approve(&id, share_passwords),
            Request::SyncApproveCancel => self.sync_approve_cancel(),
            Request::SyncRemoveDevice { id } => self.sync_remove_device(id),
            Request::SyncTurnOff { delete_data } => self.sync_turn_off(delete_data),
            Request::SyncNow => {
                if self.sync_is_on() {
                    self.sync.wait_until_ms.set(0);
                    self.sync_run();
                }
            }
            Request::SyncSetCollection { name, on } => self.sync_set_collection(&name, on),
            Request::SyncConfirmRotation => {
                if self.sync_is_on() {
                    self.sync.confirm_rotation.set(true);
                    self.sync.confirm_devices.borrow_mut().clear();
                    self.sync.wait_until_ms.set(0);
                    self.sync_run();
                }
            }
            Request::SyncFinishSecuring { secret, recovery } => self.sync_finish_securing(secret, recovery),
            Request::SyncRenameDevice { id, name } => self.sync_rename_device(&id, &name),
            _ => {}
        }
    }

    /// One API job with a fresh access token; answers to an abandoned action are dropped.
    fn sync_call<R: Send + 'static>(
        &self,
        name: &'static str,
        work: impl FnOnce(&Api) -> R + Send + 'static,
        done: impl FnOnce(&Browser, R) + 'static,
    ) {
        let Some(proxy) = self.sync_proxy() else {
            self.sync.busy.set(false);
            self.sync_failed(SOCKS_TEXT.into());
            return;
        };
        let gen = self.sync.generation.get();
        self.refresh_access(false, move |b, token| match token {
            Err(f) => {
                if b.sync.generation.get() == gen {
                    b.sync.busy.set(false);
                    b.sync_failed(match f {
                        Failure::SignedOut => "This device is signed out of the Krunk account.".into(),
                        _ => OFFLINE_TEXT.into(),
                    });
                }
            }
            Ok(t) => b.spawn_job(
                name,
                move || {
                    let http = WinHttp { proxy };
                    let api = Api { http: &http, token: &t, now_ms: now_ms() };
                    work(&api)
                },
                move |b, r| {
                    if b.sync.generation.get() == gen {
                        done(b, r);
                    }
                },
            ),
        });
    }

    /// An error while setting up or joining: back to the step before, with the message.
    fn sync_failed(&self, text: String) {
        let next = match &*self.sync.stage.borrow() {
            Stage::On(_) => Stage::On(Some(text)),
            Stage::Alarm(a) => Stage::Alarm(a.clone()),
            Stage::Join { passwords, .. } => Stage::Join { passwords: *passwords, message: Some(text) },
            Stage::Joining { .. } | Stage::RestoreConfirm { .. } => {
                Stage::Join { passwords: false, message: Some(text) }
            }
            Stage::Setup { passwords, .. } => Stage::Setup { passwords: *passwords, message: Some(text) },
            _ => Stage::Off(Some(text)),
        };
        self.set_sync_stage(next);
    }

    fn sync_me(&self) -> Option<(String, lb_core::sync::AccountId, String)> {
        let (account, device, _) = self.account_ids()?;
        let bytes = parse_account_id(&account)?;
        Some((account, bytes, device))
    }

    fn sync_turn_on(&self, passwords: bool) {
        if self.sync_is_on() {
            self.push_sync();
            return;
        }
        let Some((_, acc, device)) = self.sync_me() else { return };
        let passwords = passwords && self.vault_unlocked();
        self.sync.generation.set(self.sync.generation.get().wrapping_add(1));
        info!("sync: turning on (passwords: {passwords})");
        self.set_sync_stage(Stage::Working("Checking your Krunk account…".into()));
        self.sync_call(
            "sync-turn-on",
            move |api| {
                let st = client::state(api)?;
                let short = client::my_short(api, &device)?;
                Ok::<_, SyncFail>((st.keyset_gen.is_some(), short))
            },
            move |b, r| match r {
                Ok((true, _)) => b.set_sync_stage(Stage::Join { passwords, message: None }),
                Ok((false, short)) => {
                    let Some((_, _, device)) = b.sync_me() else { return };
                    match keys::plan_setup(acc, Me { device_id: device, short }, passwords) {
                        Ok(plan) => {
                            *b.sync.plan.borrow_mut() = Some(plan);
                            b.set_sync_stage(Stage::Setup { passwords, message: None });
                        }
                        Err(e) => b.sync_failed(fail_text(&e.into())),
                    }
                }
                Err(e) => {
                    info!("sync: could not turn on ({e})");
                    b.set_sync_stage(Stage::Off(Some(fail_text(&e))));
                }
            },
        );
    }

    fn sync_setup_confirm(&self, group: &str, understood: bool) {
        let passwords = match &*self.sync.stage.borrow() {
            Stage::Setup { passwords, .. } => *passwords,
            _ => return,
        };
        let ok = self.sync.plan.borrow().as_ref().is_some_and(|p| keys::recovery_key_confirmed(&p.recovery_key, group));
        if !understood || !ok {
            let message = if !ok {
                "Type the last group of 4 characters of your recovery key (before the last dash) to show you saved it."
            } else {
                "Tick the box to confirm you understand."
            };
            self.set_sync_stage(Stage::Setup { passwords, message: Some(message.into()) });
            return;
        }
        let Some(plan) = self.sync.plan.borrow_mut().take() else { return };
        let Some((account, _, device)) = self.sync_me() else { return };
        self.set_sync_stage(Stage::Working("Setting up sync…".into()));
        info!("sync: setting up the first device");
        self.sync_call(
            "sync-setup",
            move |api| {
                let r = client::setup(api, &plan);
                (r, plan)
            },
            move |b, (r, plan)| match r {
                Ok((joined, gen)) => {
                    let me = Me { device_id: device, short: plan.me.short };
                    *b.sync.state.borrow_mut() = Some(client::new_state(&account, &me, &joined, gen));
                    b.sync_save();
                    if let Some(p) = &joined.pak {
                        b.sync_set_pak(Some(p));
                    }
                    info!("sync: set up (passwords: {})", joined.pak.is_some());
                    b.sync_started();
                    b.set_sync_stage(Stage::On(Some(
                        "Sync is set up. Keep your passphrase and recovery key safe.".into(),
                    )));
                    b.sync_poll_start();
                }
                Err(SyncFail::Api(lb_core::sync::api::ApiError::Conflict(_))) => {
                    info!("sync: another device set up sync meanwhile");
                    b.set_sync_stage(Stage::Join {
                        passwords,
                        message: Some(
                            "Another device has set up sync meanwhile. Restore or ask it to approve this one.".into(),
                        ),
                    });
                }
                Err(e) => {
                    *b.sync.plan.borrow_mut() = Some(plan);
                    b.set_sync_stage(Stage::Setup { passwords, message: Some(fail_text(&e)) });
                }
            },
        );
    }

    /// The passphrase and recovery key, as one text.
    fn setup_text(&self) -> Option<Zeroizing<String>> {
        let p = self.sync.plan.borrow();
        let p = p.as_ref()?;
        Some(Zeroizing::new(format!(
            "Krunk sync\r\n\r\nPassphrase: {}\r\nRecovery key: {}\r\n\r\nKeep these safe and private. With either one you can restore your synced data \
             on a new device. The Krunk website never asks for them.\r\n",
            *p.passphrase, *p.recovery_key
        )))
    }

    fn sync_setup_copy(&self, tab: TabId, req_id: Option<u32>) {
        let (Some(w), Some(text)) = (self.app(), self.setup_text()) else {
            self.reply(Some(tab), Event::err(req_id, "Nothing to copy"));
            return;
        };
        match secure::copy_private(w.hwnd(), &text) {
            Some(seq) => {
                self.vaults.clipboard_seq.set(Some(seq));
                w.set_timer(super::vault::CLIPBOARD_TIMER, super::vault::CLIPBOARD_CLEAR_MS);
                self.reply(Some(tab), Event::ok(req_id, ()));
            }
            None => self.reply(Some(tab), Event::err(req_id, "Could not access the clipboard")),
        }
    }

    fn sync_setup_save(&self, tab: TabId, req_id: Option<u32>) {
        let (Some(w), Some(text)) = (self.window(), self.setup_text()) else {
            self.reply(Some(tab), Event::err(req_id, "Nothing to save"));
            return;
        };
        let Some(path) = crate::ffi::dialogs::save_file(
            w.hwnd(),
            "Save sync recovery",
            "Krunk sync recovery.txt",
            ("Text files", "*.txt"),
            None,
        ) else {
            self.reply(Some(tab), Event::ok(req_id, serde_json::json!({ "saved": false })));
            return;
        };
        match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => self.reply(Some(tab), Event::ok(req_id, serde_json::json!({ "saved": true }))),
            Err(_) => self.reply(Some(tab), Event::err(req_id, "The file could not be written")),
        }
    }

    fn sync_restore(&self, secret: lb_core::secret::Secret, recovery: bool) {
        if !matches!(&*self.sync.stage.borrow(), Stage::Join { .. }) {
            return;
        }
        let Some((_, acc, _)) = self.sync_me() else { return };
        let secret = Zeroizing::new(secret.to_string());
        let recovery = recovery || secret.trim().to_ascii_uppercase().starts_with("KRNK-");
        if !secret_shaped(&secret, recovery) {
            let passwords = matches!(&*self.sync.stage.borrow(), Stage::Join { passwords: true, .. });
            self.set_sync_stage(Stage::Join { passwords, message: Some(NOT_A_SECRET.into()) });
            return;
        }
        let secret = if recovery { SyncSecret::RecoveryKey(secret) } else { SyncSecret::Passphrase(secret) };
        self.set_sync_stage(Stage::Working(
            "Checking your passphrase or recovery key… (this takes a few seconds)".into(),
        ));
        self.sync_call(
            "sync-restore",
            move |api| {
                let plan = client::restore_begin(api, &secret, acc)?;
                let devices = client::devices(api)?;
                Ok::<_, SyncFail>((plan, devices))
            },
            move |b, r| match r {
                Ok((plan, devices)) => {
                    let names: Vec<String> = plan
                        .ak_dir
                        .devices
                        .iter()
                        .map(|d| {
                            devices
                                .iter()
                                .find(|x| x.id == d.id)
                                .map(|x| x.label.clone())
                                .unwrap_or_else(|| format!("Device #{}", d.short))
                        })
                        .collect();
                    let revoked = plan.ak_dir.revoked.len();
                    *b.sync.restore.borrow_mut() = Some(plan);
                    info!("sync: restore found the keys");
                    b.set_sync_stage(Stage::RestoreConfirm { devices: names, revoked });
                }
                Err(e) => {
                    info!("sync: restore failed ({e})");
                    b.set_sync_stage(Stage::Join { passwords: false, message: Some(fail_text(&e)) });
                }
            },
        );
    }

    fn sync_restore_confirm(&self) {
        let Some(plan) = self.sync.restore.borrow_mut().take() else { return };
        let Some((account, _, device)) = self.sync_me() else { return };
        let with_vault = self.vault_unlocked();
        self.set_sync_stage(Stage::Working("Restoring sync on this device…".into()));
        self.sync_call(
            "sync-restore-finish",
            move |api| {
                let short = client::my_short(api, &device)?;
                let me = Me { device_id: device, short };
                let r = client::restore_finish(api, &plan, &me, with_vault)?;
                Ok::<_, SyncFail>((me, r))
            },
            move |b, r| match r {
                Ok((me, (joined, gen))) => {
                    *b.sync.state.borrow_mut() = Some(client::new_state(&account, &me, &joined, gen));
                    b.sync_save();
                    if let Some(p) = &joined.pak {
                        b.sync_set_pak(Some(p));
                    }
                    info!("sync: restored (passwords: {})", joined.pak.is_some());
                    b.sync_started();
                    b.set_sync_stage(Stage::On(Some("Sync is restored on this device.".into())));
                    b.sync_poll_start();
                }
                Err(e) => {
                    info!("sync: restore failed ({e})");
                    b.set_sync_stage(Stage::Join { passwords: with_vault, message: Some(fail_text(&e)) });
                }
            },
        );
    }

    fn sync_join(&self, passwords: bool) {
        if !matches!(&*self.sync.stage.borrow(), Stage::Join { .. }) {
            return;
        }
        let with_vault = passwords && self.vault_unlocked();
        self.set_sync_stage(Stage::Working("Asking your other devices…".into()));
        self.sync_call(
            "sync-join",
            move |api| NApproval::start(api, with_vault),
            move |b, r| match r {
                Ok(n) => {
                    info!("sync: asked a device for the keys");
                    *b.sync.joining.borrow_mut() = Some(n);
                    b.set_sync_stage(Stage::Joining { enter_code: false, message: None });
                    b.sync_poll_start();
                }
                Err(e) => b.set_sync_stage(Stage::Join { passwords, message: Some(fail_text(&e)) }),
            },
        );
    }

    fn sync_join_code(&self, code: String) {
        if !matches!(&*self.sync.stage.borrow(), Stage::Joining { enter_code: true, .. }) {
            return;
        }
        let Some(mut n) = self.sync.joining.borrow_mut().take() else { return };
        self.set_sync_stage(Stage::Joining { enter_code: false, message: Some("Checking the code…".into()) });
        self.sync_call(
            "sync-join-code",
            move |api| {
                let r = n.confirm(api, &code);
                (r, n)
            },
            move |b, (r, n)| match r {
                Ok(()) => {
                    *b.sync.joining.borrow_mut() = Some(n);
                    b.set_sync_stage(Stage::Joining {
                        enter_code: false,
                        message: Some("Waiting for the keys…".into()),
                    });
                }
                Err(e) => {
                    info!("sync: the code did not match; the request was cancelled");
                    b.set_sync_stage(Stage::Join { passwords: false, message: Some(fail_text(&e)) });
                }
            },
        );
    }

    fn sync_cancel(&self) {
        self.sync.generation.set(self.sync.generation.get().wrapping_add(1));
        self.sync.busy.set(false);
        *self.sync.plan.borrow_mut() = None;
        *self.sync.restore.borrow_mut() = None;
        if let Some(n) = self.sync.joining.borrow_mut().take() {
            self.sync_call("sync-join-cancel", move |api| n.cancel(api), |_, _| {});
        }
        let on = self.sync_is_on();
        self.set_sync_stage(if on { Stage::On(None) } else { Stage::Off(None) });
    }

    fn sync_approve(&self, id: &str, share: bool) {
        if !matches!(&*self.sync.stage.borrow(), Stage::On(_)) || self.sync.approving.borrow().is_some() {
            return;
        }
        let Some(item) = self.sync.approvals.borrow().iter().find(|a| a.approval_id == id).cloned() else { return };
        let has_pak = self.vault_unlocked() && self.sync_pak().is_some();
        // A device that syncs asks for the passwords key only: there is nothing to give without
        // the open vault (0.21.1).
        if item.wants_pak() && !has_pak {
            self.set_sync_stage(Stage::On(Some(
                "Unlock your passwords first (Passwords in the menu), then approve: the device asks for them.".into(),
            )));
            return;
        }
        let share = (share || item.wants_pak()) && has_pak;
        *self.sync.approving_done.borrow_mut() = None;
        self.sync_call(
            "sync-approve",
            move |api| AApproval::respond(api, &item),
            move |b, r| match r {
                Ok(a) => {
                    info!("sync: answering a device that asks {}", if a.asked_pak { "for the passwords key" } else { "to join" });
                    *b.sync.approving.borrow_mut() = Some((a, None, share));
                    b.push_sync();
                }
                Err(e) => {
                    info!("sync: answering the request failed ({e})");
                    b.sync_poll_failed(&e);
                    b.set_sync_stage(Stage::On(Some(fail_text(&e))));
                }
            },
        );
    }

    fn sync_approve_cancel(&self) {
        if let Some((a, _, _)) = self.sync.approving.borrow_mut().take() {
            self.sync_call("sync-approve-cancel", move |api| a.cancel(api), |_, _| {});
        }
        *self.sync.approving_done.borrow_mut() = None;
        self.push_sync();
    }

    fn sync_remove_device(&self, id: String) {
        if !self.sync_is_on() {
            return;
        }
        self.sync_call(
            "sync-remove-device",
            move |api| api.post_empty(&format!("devices/{id}/revoke"), &serde_json::json!({})),
            |b, r| {
                match r {
                    Ok(()) => {
                        info!("sync: a device was removed");
                        b.sync.rotation_pending.set(true);
                        // Started here: the rotation needs no second confirmation, and runs now.
                        b.sync.confirm_rotation.set(true);
                        b.sync.next_run_ms.set(now_ms());
                        b.set_sync_stage(Stage::On(Some(
                            "The device is removed. Krunk re-secures your sync with new keys.".into(),
                        )));
                        b.sync_run();
                    }
                    Err(e) => b.set_sync_stage(Stage::On(Some(fail_text(&e)))),
                }
                b.sync_refresh_soon();
            },
        );
    }

    /// Off on this device: its sync keys go (sync.json, the vault's part). The account and the
    /// other devices are unaffected; this device stays listed until removed.
    fn sync_turn_off(&self, delete_data: bool) {
        self.sync.generation.set(self.sync.generation.get().wrapping_add(1));
        // Answers to jobs still running are dropped (generation), so their flags go now.
        self.sync.busy.set(false);
        self.sync.running.set(false);
        self.sync.confirm_rotation.set(false);
        self.sync.confirm_devices.borrow_mut().clear();
        *self.sync.state.borrow_mut() = None;
        if let Err(e) = std::fs::remove_file(self.dirs.sync()) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warning!("sync: sync.json could not be deleted: {e}");
            }
        }
        if self.vault_unlocked() {
            self.sync_set_pak(None);
        }
        let forgot = self.store.borrow_mut().sync_forget(delete_data);
        match forgot {
            Ok(n) if delete_data => {
                info!("sync: removed {n} synced items from this device");
                self.send_bookmarks_bar();
                self.send_nav();
                self.push_ntp_config();
            }
            Ok(_) => {}
            Err(e) => warning!("sync: sync's records could not be cleared: {e}"),
        }
        self.sync.last_ok_ms.set(None);
        *self.sync.status.borrow_mut() = None;
        self.sync.backoff.set(Backoff::default());
        *self.sync.joining.borrow_mut() = None;
        *self.sync.approving.borrow_mut() = None;
        self.sync.devices.borrow_mut().clear();
        self.sync.approvals.borrow_mut().clear();
        info!("sync: turned off on this device (delete synced data: {delete_data})");
        self.set_sync_stage(Stage::Off(Some("Sync is off on this device.".into())));
    }

    /// The vault was unlocked: a pending removal of sync's part (turned off while locked).
    pub(super) fn sync_vault_unlocked(&self) {
        if !self.sync_is_on() && self.sync_pak().is_some() {
            self.sync_set_pak(None);
        }
        // Passwords that waited for the vault come with the next tick.
        if self.sync_is_on() && self.sync_pak().is_some() {
            self.sync.next_run_ms.set(now_ms());
        }
        self.push_sync();
    }

    // ---- while krunk://sync is open ------------------------------------------------------------

    fn sync_poll_start(&self) {
        if self.sync.polling.replace(true) {
            return;
        }
        self.sync_poll();
    }

    fn sync_poll_later(&self) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(POLL_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.sync_poll();
            }
        });
    }

    /// One round: the approval being run, the open requests, and now and then the devices and
    /// the keys. Stops when no krunk://sync is open (nothing runs in the background in S6).
    fn sync_poll(&self) {
        let joining = matches!(&*self.sync.stage.borrow(), Stage::Joining { .. });
        if !self.sync_open_tabs() || !(joining || self.sync_is_on()) {
            self.sync.polling.set(false);
            return;
        }
        if self.sync.busy.get() || self.sync.running.get() {
            self.sync_poll_later();
            return;
        }
        let approving = self.sync.approving.borrow().is_some();
        let mut ps = self.sync.poll_state.get();
        let what = spoll::next(&mut ps, now_ms(), joining, approving);
        self.sync.poll_state.set(ps);
        match what {
            Poll::Join => self.sync_poll_join(),
            Poll::Approving => self.sync_poll_approving(),
            Poll::Refresh => self.sync_poll_refresh(),
            Poll::Approvals => self.sync_poll_approvals(),
            Poll::Wait => {}
        }
        self.sync_poll_later();
    }

    /// The next tick refreshes the directories and devices.
    fn sync_refresh_soon(&self) {
        let mut ps = self.sync.poll_state.get();
        ps.last_refresh_ms = 0;
        self.sync.poll_state.set(ps);
    }

    /// A poll's error: a 429 pauses the page's polling for its Retry-After.
    fn sync_poll_failed(&self, e: &SyncFail) {
        if let SyncFail::Api(ApiError::RateLimited(s)) = e {
            let mut ps = self.sync.poll_state.get();
            ps.rate_limited(now_ms(), *s);
            self.sync.poll_state.set(ps);
            info!("sync: the server asks to wait {s} s");
        }
    }

    fn sync_poll_join(&self) {
        let Some(mut n) = self.sync.joining.borrow_mut().take() else { return };
        let Some((account, acc, device)) = self.sync_me() else { return };
        self.sync.busy.set(true);
        self.sync_call(
            "sync-join-poll",
            move |api| {
                let name = format!("Krunk on Windows ({})", crate::ffi::window::windows_build());
                let step = n.poll(api, &acc, &device, &name);
                let finished = match &step {
                    Ok(NStep::Ready) => Some(n.finish(api, acc, &device).and_then(|(j, p)| {
                        let gen = client::state(api)?.keyset_gen.unwrap_or_default();
                        Ok((j, p, gen))
                    })),
                    _ => None,
                };
                (step, finished, n)
            },
            move |b, (step, finished, n)| {
                b.sync.busy.set(false);
                match (step, finished) {
                    (_, Some(Ok((joined, payload, gen)))) => {
                        let Some((_, _, device)) = b.sync_me() else { return };
                        let me = Me { device_id: device, short: payload.dev_short };
                        *b.sync.state.borrow_mut() = Some(client::new_state(&account, &me, &joined, gen));
                        b.sync_save();
                        if let Some(p) = &joined.pak {
                            b.sync_set_pak(Some(p));
                        }
                        info!("sync: this device got the keys (passwords: {})", joined.pak.is_some());
                        b.sync_started();
                        b.set_sync_stage(Stage::On(Some("This device is approved and syncs now.".into())));
                    }
                    (_, Some(Err(e))) => {
                        info!("sync: the keys did not check out ({e})");
                        b.set_sync_stage(Stage::Join { passwords: false, message: Some(fail_text(&e)) });
                    }
                    (Ok(NStep::EnterCode), None) => {
                        *b.sync.joining.borrow_mut() = Some(n);
                        if !matches!(&*b.sync.stage.borrow(), Stage::Joining { enter_code: true, .. }) {
                            b.set_sync_stage(Stage::Joining { enter_code: true, message: None });
                        }
                    }
                    (Ok(NStep::Gone), None) => {
                        b.set_sync_stage(Stage::Join {
                            passwords: false,
                            message: Some("The request was cancelled or ran out of time. Ask again.".into()),
                        });
                    }
                    (Ok(_), None) => *b.sync.joining.borrow_mut() = Some(n),
                    (Err(e), None) => {
                        b.sync_poll_failed(&e);
                        *b.sync.joining.borrow_mut() = Some(n);
                        info!("sync: waiting for the approval ({e})");
                    }
                }
            },
        );
    }

    fn sync_poll_approving(&self) {
        let Some((mut a, code, share)) = self.sync.approving.borrow_mut().take() else { return };
        let Some((_, acc, device)) = self.sync_me() else { return };
        let Some(st) = self.sync.state.borrow().clone() else { return };
        let pak = if share { self.sync_pak().map(|p| p.pak) } else { None };
        self.sync.busy.set(true);
        self.sync_call(
            "sync-approve-poll",
            move |api| {
                let step = a.poll(api, &acc, &device);
                let admitted = match &step {
                    // A device that syncs gets the passwords key only (0.21.1); ak_dir stays.
                    Ok(AStep::Confirmed) if a.asked_pak => Some(match pak.as_ref() {
                        Some(p) => a.admit_pak(api, &st, p).map(|()| None),
                        None => Err(SyncFail::BadAnswer("the passwords are locked")),
                    }),
                    Ok(AStep::Confirmed) => Some(a.admit(api, &st, pak.as_ref()).map(Some)),
                    _ => None,
                };
                (step, admitted, a)
            },
            move |b, (step, admitted, a)| {
                b.sync.busy.set(false);
                match (step, admitted) {
                    (_, Some(Ok(None))) => {
                        info!("sync: a device that syncs got the passwords key");
                        *b.sync.approving_done.borrow_mut() = Some((a.device.label.clone(), true));
                        b.sync_refresh_soon();
                        b.push_sync();
                    }
                    (_, Some(Ok(Some(pin)))) => {
                        if let Some(st) = b.sync.state.borrow_mut().as_mut() {
                            st.ak = Some(pin);
                        }
                        b.sync_save();
                        info!("sync: a new device was approved (passwords shared: {share})");
                        *b.sync.approving_done.borrow_mut() = Some((a.device.label.clone(), false));
                        b.sync_refresh_soon();
                        b.push_sync();
                    }
                    // A 429 while polling: the approval goes on after the wait.
                    (Err(SyncFail::Api(ApiError::RateLimited(sec))), None) => {
                        b.sync_poll_failed(&SyncFail::Api(ApiError::RateLimited(sec)));
                        *b.sync.approving.borrow_mut() = Some((a, code, share));
                    }
                    (_, Some(Err(e))) | (Err(e), None) => {
                        info!("sync: the approval failed ({e})");
                        b.set_sync_stage(Stage::On(Some(fail_text(&e))));
                    }
                    (Ok(AStep::ShowCode(c)), None) => {
                        let changed = code.as_deref() != Some(c.as_str());
                        *b.sync.approving.borrow_mut() = Some((a, Some(c), share));
                        if changed {
                            b.push_sync();
                        }
                    }
                    (Ok(AStep::Gone), None) => {
                        b.set_sync_stage(Stage::On(Some(
                            "The new device cancelled, or the request ran out of time.".into(),
                        )));
                    }
                    (Ok(_), None) => *b.sync.approving.borrow_mut() = Some((a, code, share)),
                }
            },
        );
    }

    fn sync_poll_approvals(&self) {
        self.sync.busy.set(true);
        self.sync_call("sync-approvals", client::approvals, |b, r| {
            b.sync.busy.set(false);
            if let Err(e) = &r {
                b.sync_poll_failed(e);
            }
            if let Ok(list) = r {
                let changed =
                    list.iter().map(|a| &a.approval_id).ne(b.sync.approvals.borrow().iter().map(|a| &a.approval_id));
                *b.sync.approvals.borrow_mut() = list;
                if changed {
                    b.push_sync();
                }
            }
        });
    }

    /// The devices, the state and the keys (following the directories, PROTOCOL §4.4).
    fn sync_poll_refresh(&self) {
        let Some(mut st) = self.sync.state.borrow().clone() else { return };
        let mut pak = self.sync_pak();
        self.sync.busy.set(true);
        self.sync_call(
            "sync-refresh",
            move |api| {
                let r = client::refresh(api, &mut st, pak.as_mut());
                let devices = client::devices(api);
                let server = client::state(api);
                (r, st, pak, devices, server)
            },
            |b, (r, st, pak, devices, server)| {
                b.sync.busy.set(false);
                if let Ok(d) = devices {
                    *b.sync.devices.borrow_mut() = d;
                }
                if let Ok(s) = server {
                    b.sync.rotation_pending.set(s.rotation.ak || s.rotation.pak);
                }
                match r {
                    Ok(Refresh::SelfRevoked) => {
                        info!("sync: this device was removed from sync");
                        b.sync_turn_off(false);
                        b.set_sync_stage(Stage::Off(Some(
                            "This device was removed from sync on another device.".into(),
                        )));
                    }
                    Ok(Refresh::Changed) => {
                        *b.sync.state.borrow_mut() = Some(st);
                        b.sync_save();
                        if let Some(p) = &pak {
                            b.sync_set_pak(Some(p));
                        }
                        b.push_sync();
                    }
                    Ok(Refresh::Unchanged) => b.push_sync(),
                    Err(e) if matches!(&e, SyncFail::Keys(k) if k.is_alarm()) => {
                        let text = fail_text(&e);
                        warning!("sync: security alarm ({e}); writes stopped");
                        if let Some(s) = b.sync.state.borrow_mut().as_mut() {
                            s.alarm = Some(text.clone());
                        }
                        b.sync_save();
                        b.set_sync_stage(Stage::Alarm(text));
                    }
                    Err(e) => {
                        b.sync_poll_failed(&e);
                        info!("sync: refresh skipped ({e})");
                    }
                }
            },
        );
    }
}

impl Browser {
    // ---- the schedule (S7) ------------------------------------------------------------------------

    /// At start: if sync is on here (sync.json exists), the first run is a minute away. Nothing
    /// is sent otherwise.
    pub(super) fn init_sync(&self) {
        if self.sync_is_on() {
            if let Err(e) = self.store.borrow().sync_set_tracking(true) {
                warning!("sync: tracking could not start: {e}");
            }
            self.sync.next_run_ms.set(now_ms() + START_DELAY_MS);
            self.sync_tick_start();
        }
    }

    /// This device just got the keys: local changes are tracked from now on, and the first
    /// sync runs at once.
    fn sync_started(&self) {
        if let Err(e) = self.store.borrow().sync_set_tracking(true) {
            warning!("sync: tracking could not start: {e}");
        }
        self.sync.next_run_ms.set(now_ms());
        self.sync_tick_start();
        self.sync_run();
    }

    fn sync_tick_start(&self) {
        if !self.sync.ticking.replace(true) {
            self.sync_tick_later();
        }
    }

    fn sync_tick_later(&self) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(TICK_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.sync_tick();
            }
        });
    }

    fn sync_tick(&self) {
        if !self.sync_is_on() {
            self.sync.ticking.set(false);
            return;
        }
        let now = now_ms();
        let changed: Option<i64> = self
            .store
            .borrow()
            .sync_conn()
            .query_row("SELECT min(enqueued_at) FROM sync_queue WHERE collection <> 'tabs'", [], |r| r.get(0))
            .ok()
            .flatten();
        // A login changed in the open vault counts as a change too.
        let vault_changed =
            self.vaults.vault.borrow().as_ref().and_then(|v| v.sync_part()).is_some_and(|p| !p.dirty.is_empty());
        let due =
            now >= self.sync.next_run_ms.get() || changed.is_some_and(|t| now - t >= AFTER_CHANGE_MS) || vault_changed;
        if due && now >= self.sync.wait_until_ms.get() {
            self.sync_run();
        }
        self.sync_tick_later();
    }

    fn sync_set_collection(&self, name: &str, on: bool) {
        if !self.sync_is_on() || !COLLECTIONS.iter().any(|(n, _)| *n == name) {
            return;
        }
        let r = {
            let store = self.store.borrow();
            let c = store.sync_conn();
            let mut off = local::off_collections(c);
            off.retain(|n| n != name);
            if !on {
                off.push(name.to_string());
            }
            // Switched back on: everything again (what changed meanwhile merges in).
            local::meta_set(c, "off", &serde_json::to_string(&off).unwrap_or_default()).and_then(|()| {
                if on {
                    local::meta_set(c, "reconcile", "1")
                } else {
                    Ok(())
                }
            })
        };
        if let Err(e) = r {
            warning!("sync: the collection setting could not be saved: {e}");
        }
        info!("sync: {name} {}", if on { "on" } else { "off" });
        self.sync.next_run_ms.set(now_ms());
        self.push_sync();
    }

    fn sync_rename_device(&self, id: &str, name: &str) {
        let name = name.trim();
        if !self.sync_is_on() || name.is_empty() || name.chars().any(char::is_control) {
            self.push_sync();
            return;
        }
        let known = self.sync.devices.borrow().iter().any(|d| d.id == id && d.revoked_at.is_none());
        if known {
            if let Err(e) = coll::rename_device(self.store.borrow().sync_conn(), id, name, now_ms()) {
                warning!("sync: the device could not be renamed: {e}");
            }
            self.sync.next_run_ms.set(now_ms());
        }
        self.push_sync();
    }

    /// One sync run on a worker thread, with its own connection to browser.db. Runs never
    /// overlap with each other or with the keys polling of krunk://sync.
    fn sync_run(&self) {
        if self.sync.running.get() || self.sync.busy.get() || !matches!(&*self.sync.stage.borrow(), Stage::On(_)) {
            return;
        }
        let Some(mut st) = self.sync.state.borrow().clone() else { return };
        let Some(proxy) = self.sync_proxy() else {
            *self.sync.status.borrow_mut() = Some(SOCKS_TEXT.into());
            self.push_sync();
            return;
        };
        let off = local::off_collections(self.store.borrow().sync_conn());
        let db = self.dirs.database();
        let name = format!("Krunk on Windows ({})", crate::ffi::window::windows_build());
        let confirm_rotation = self.sync.confirm_rotation.get();
        // With the vault open, a pending passwords-key rotation (or a rekey left) runs too.
        let mut pak_state =
            if off.iter().any(|c| c == "passwords") || !self.vault_unlocked() { None } else { self.sync_pak() };
        self.sync.running.set(true);
        self.sync.next_run_ms.set(now_ms() + EVERY_MS);
        self.push_sync();
        self.refresh_access(false, move |b, token| match token {
            Err(f) => {
                let e = match f {
                    Failure::SignedOut => SyncFail::Api(ApiError::Unauthorized),
                    _ => SyncFail::Api(ApiError::Offline("no token".into())),
                };
                b.sync_ran(Err(e), None, None);
            }
            Ok(t) => b.spawn_job(
                "sync-run",
                move || {
                    let http = WinHttp { proxy };
                    let api = Api { http: &http, token: &t, now_ms: now_ms() };
                    let opts = RunOpts { device_name: &name, off: &off, max_rounds: 20, confirm_rotation };
                    let r = lb_core::storage::Store::open(&db)
                        .map_err(|e| SyncFail::Local(e.to_string()))
                        .and_then(|mut store| engine::run(&api, &mut store, &mut st, &opts));
                    let pak_changed = match (&r, pak_state.as_mut()) {
                        (Ok(_), Some(p)) => match lb_core::sync::rotate::pak_maintenance(&api, &mut st, p) {
                            Ok(changed) => changed,
                            Err(e) => {
                                info!("sync: passwords key maintenance skipped ({e})");
                                false
                            }
                        },
                        _ => false,
                    };
                    (r, st, pak_state.filter(|_| pak_changed))
                },
                |b, (r, st, pak)| b.sync_ran(r, Some(st), pak),
            ),
        });
    }

    fn sync_ran(&self, r: Result<Report, SyncFail>, after: Option<SyncState>, pak: Option<PakState>) {
        self.sync.running.set(false);
        let now = now_ms();
        // Sync was turned off (or on anew) meanwhile: the answer belongs to nobody.
        let same = matches!((&*self.sync.state.borrow(), &after), (Some(a), Some(b)) if a.device_id == b.device_id && a.account_id == b.account_id);
        if after.is_some() && !same {
            self.push_sync();
            return;
        }
        // The state is consistent after each step of a run (a rotation's new key, a cursor), so
        // it is kept even when a later step failed.
        if let Some(after) = after {
            *self.sync.state.borrow_mut() = Some(after);
            self.sync_save();
        }
        if let Some(p) = &pak {
            if self.vault_unlocked() {
                self.sync_set_pak(Some(p));
            }
        }
        match r {
            Ok(report) => {
                self.sync.backoff.set(Backoff::default());
                self.sync.wait_until_ms.set(0);
                self.sync.last_ok_ms.set(Some(now));
                self.sync.rotation_pending.set(report.frozen);
                *self.sync.status.borrow_mut() =
                    report.frozen.then(|| "Securing your account after a device was removed…".to_string());
                if let Some(e) = report.rotated {
                    info!("sync: rotated the account key to epoch {e}");
                    self.sync.confirm_rotation.set(false);
                }
                *self.sync.confirm_devices.borrow_mut() = report.confirm.clone();
                if !report.confirm.is_empty() {
                    *self.sync.status.borrow_mut() =
                        Some("A removed device had added another device. Check the list below.".into());
                }
                info!(
                    "sync: run done (pulled {}, pushed {}, conflicts {}, full {}, frozen {}, left {}, passwords waiting {})",
                    report.pulled,
                    report.pushed,
                    report.conflicts,
                    report.full,
                    report.frozen,
                    report.queue_left,
                    report.pak_waiting
                );
                self.sync_apply_changes(&report);
                self.sync_pak_run();
            }
            Err(e) => {
                info!("sync: run failed ({e})");
                let text = self.sync_run_failed(&e, now);
                *self.sync.status.borrow_mut() = Some(text);
            }
        }
        self.push_sync();
    }

    // ---- passwords (S8): only while the vault is unlocked -------------------------------------------

    /// After an AK run: the PAK collections, in three steps — encrypt what changed (here, the
    /// vault is open on this thread), exchange on a worker, merge into the vault (here).
    fn sync_pak_run(&self) {
        let off = local::off_collections(self.store.borrow().sync_conn());
        if off.iter().any(|c| c == "passwords") || !self.vault_unlocked() {
            return;
        }
        let Some(pak_state) = self.sync_pak() else { return };
        let Some(st) = self.sync.state.borrow().clone() else { return };
        let Ok(acc) = st.account_bytes() else { return };
        let never = self.store.borrow().never_save_list().unwrap_or_default();
        let mut hlc = Hlc::new(st.hlc);
        let now = now_ms();
        let prepared = {
            let v = self.vaults.vault.borrow();
            let Some(vault) = v.as_ref() else { return };
            pak::prepare(vault, &pak_state, &never, &acc, st.dev_short, &mut hlc, now)
        };
        let out: Outgoing = match prepared {
            Ok(o) => o,
            Err(e) => {
                warning!("sync: passwords could not be prepared ({e})");
                return;
            }
        };
        let Some(proxy) = self.sync_proxy() else { return };
        let since = pak_state.book.pull_seq;
        let writes = out.writes.clone();
        self.sync.running.set(true);
        self.refresh_access(false, move |b, token| match token {
            Err(_) => {
                b.sync.running.set(false);
                b.push_sync();
            }
            Ok(t) => b.spawn_job(
                "sync-passwords",
                move || {
                    let http = WinHttp { proxy };
                    let api = Api { http: &http, token: &t, now_ms: now_ms() };
                    pak::exchange(&api, since, &writes)
                },
                move |b, r| b.sync_pak_ran(r, out, hlc, acc),
            ),
        });
    }

    fn sync_pak_ran(
        &self,
        r: Result<pak::Exchanged, SyncFail>,
        out: Outgoing,
        mut hlc: Hlc,
        acc: lb_core::sync::AccountId,
    ) {
        self.sync.running.set(false);
        let ex = match r {
            Ok(ex) => ex,
            Err(e) => {
                info!("sync: passwords run failed ({e})");
                if matches!(&e, SyncFail::Keys(k) if k.is_alarm()) {
                    let text = self.sync_run_failed(&e, now_ms());
                    *self.sync.status.borrow_mut() = Some(text);
                }
                self.push_sync();
                return;
            }
        };
        // Locked or turned off meanwhile: nothing is merged (the next run catches up).
        let (Some(mut pak_state), true) = (self.sync_pak(), self.sync_is_on()) else {
            self.push_sync();
            return;
        };
        let primed = pak_state.book.primed;
        let finished = {
            let store = self.store.borrow();
            let mut v = self.vaults.vault.borrow_mut();
            let Some(vault) = v.as_mut() else { return };
            pak::finish(vault, &mut pak_state, &store, &acc, out, ex, &mut hlc, now_ms(), &Dpapi)
        };
        match finished {
            Ok(rep) => {
                if let Some(st) = self.sync.state.borrow_mut().as_mut() {
                    st.hlc = st.hlc.max(hlc.last());
                }
                self.sync_save();
                info!(
                    "sync: passwords done (pulled {}, pushed {}, conflicts {}, skipped {}, frozen {})",
                    rep.pulled, rep.pushed, rep.conflicts, rep.skipped, rep.frozen
                );
                if rep.passwords {
                    self.push_vault_state();
                }
                // The first run only pulled: what is left here goes up soon.
                if !primed || rep.conflicts > 0 {
                    self.sync.next_run_ms.set(now_ms());
                }
            }
            Err(e) => {
                warning!("sync: passwords could not be merged ({e})");
                if matches!(&e, SyncFail::Keys(k) if k.is_alarm()) {
                    let text = self.sync_run_failed(&e, now_ms());
                    *self.sync.status.borrow_mut() = Some(text);
                }
            }
        }
        self.push_sync();
    }

    /// What the schedule does after a failed run, and the status line for it.
    fn sync_run_failed(&self, e: &SyncFail, now: i64) -> String {
        let mut b = self.sync.backoff.get();
        let next = match e {
            SyncFail::Api(api) => sapi::next_step(api, &mut b),
            SyncFail::Local(_) | SyncFail::BadAnswer(_) => sapi::Next::RetryIn(b.fail()),
            SyncFail::Keys(k) if !k.is_alarm() => sapi::Next::RetryIn(b.fail()),
            SyncFail::Keys(_) | SyncFail::Reset => sapi::Next::Stop,
        };
        self.sync.backoff.set(b);
        match (e, next) {
            (SyncFail::Keys(k), _) if k.is_alarm() => {
                let text = fail_text(e);
                warning!("sync: security alarm during a run ({e}); writes stopped");
                if let Some(s) = self.sync.state.borrow_mut().as_mut() {
                    s.alarm = Some(text.clone());
                }
                self.sync_save();
                *self.sync.stage.borrow_mut() = Stage::Alarm(text.clone());
                text
            }
            (SyncFail::Reset, _) => {
                "Sync was reset from another device. Turn sync off and on again on this device.".into()
            }
            (SyncFail::Api(ApiError::Offline(_)), sapi::Next::RetryIn(ms)) => {
                self.sync.wait_until_ms.set(now + ms);
                format!("Offline — will retry in {} min", (ms + 59_999) / 60_000)
            }
            (_, sapi::Next::RetryIn(ms)) => {
                self.sync.wait_until_ms.set(now + ms);
                self.sync.next_run_ms.set(now + ms);
                format!("Sync will try again in {} min", (ms + 59_999) / 60_000)
            }
            (_, sapi::Next::SignedOut) => {
                "This device is signed out of the Krunk account. Sign in again in Settings → Account.".into()
            }
            (_, sapi::Next::Frozen) => {
                self.sync.rotation_pending.set(true);
                "Securing your account after a device was removed…".into()
            }
            (_, sapi::Next::Reconcile) => {
                self.sync.next_run_ms.set(now);
                "Catching up…".into()
            }
            (_, sapi::Next::Stop) => fail_text(e),
        }
    }

    /// What a run changed here reaches the open windows and pages.
    fn sync_apply_changes(&self, r: &Report) {
        let ch = r.changed;
        if ch.settings {
            let loaded = self.store.borrow().load_settings();
            match loaded {
                Ok(mut new) if new.validate().is_ok() => {
                    new.sync_protection();
                    let old = std::mem::replace(&mut *self.settings.borrow_mut(), new.clone());
                    self.apply_settings(&old, &new);
                }
                Ok(_) => warning!("sync: synced settings did not validate; they apply after a restart"),
                Err(e) => warning!("sync: settings could not be read: {e}"),
            }
        }
        if ch.bookmarks {
            self.send_bookmarks_bar();
            self.send_nav();
        }
        if ch.pins {
            self.push_ntp_config();
        }
    }
}

impl Browser {
    /// "Finish securing" (S9): a new escrow anchor at the rotated heads, with the passphrase or
    /// the recovery key (Argon2id on a worker).
    fn sync_finish_securing(&self, secret: lb_core::secret::Secret, recovery: bool) {
        if !matches!(&*self.sync.stage.borrow(), Stage::On(_)) || self.sync.running.get() || self.sync.busy.get() {
            return;
        }
        let Some(mut st) = self.sync.state.borrow().clone() else { return };
        let mut pak = if self.vault_unlocked() { self.sync_pak() } else { None };
        let secret = Zeroizing::new(secret.to_string());
        let recovery = recovery || secret.trim().to_ascii_uppercase().starts_with("KRNK-");
        if !secret_shaped(&secret, recovery) {
            *self.sync.status.borrow_mut() = Some(NOT_A_SECRET.into());
            self.push_sync();
            return;
        }
        let secret = if recovery { SyncSecret::RecoveryKey(secret) } else { SyncSecret::Passphrase(secret) };
        // `busy` keeps runs and the keys polling away while the directories change.
        self.sync.busy.set(true);
        *self.sync.status.borrow_mut() = Some("Checking your passphrase or recovery key…".into());
        self.push_sync();
        self.sync_call(
            "sync-finish-securing",
            move |api| {
                let r = client::finish_securing(api, &mut st, pak.as_mut(), &secret);
                (r, st, pak)
            },
            |b, (r, st, pak)| {
                b.sync.busy.set(false);
                match r {
                    Ok(()) => {
                        *b.sync.state.borrow_mut() = Some(st);
                        b.sync_save();
                        if let Some(p) = &pak {
                            b.sync_set_pak(Some(p));
                        }
                        info!("sync: finished securing (new anchor)");
                        *b.sync.status.borrow_mut() = None;
                        b.set_sync_stage(Stage::On(Some("Your account is secured with the new keys.".into())));
                    }
                    Err(e) => {
                        info!("sync: finish securing failed ({e})");
                        *b.sync.status.borrow_mut() = None;
                        if matches!(&e, SyncFail::Keys(k) if k.is_alarm()) {
                            let text = b.sync_run_failed(&e, now_ms());
                            *b.sync.status.borrow_mut() = Some(text);
                            b.push_sync();
                        } else {
                            b.set_sync_stage(Stage::On(Some(fail_text(&e))));
                        }
                    }
                }
            },
        );
    }
}
