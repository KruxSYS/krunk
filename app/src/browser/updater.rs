//! Krunk's own update check (the rules are in `lb_core::update`).
//!
//! A minute after start, and then once a day while "Check for updates
//! automatically" is on, a worker thread fetches the signed manifest from
//! the update server built into this copy. A newer version opens a prompt;
//! "Update now" downloads the installer to %TEMP%, checks its size and
//! SHA-256 against the signed manifest, starts it with `/S /RELAUNCH` and
//! closes Krunk (the installer waits for that, then starts the new version).
//! A portable copy (no uninstaller next to Krunk.exe) gets the download page
//! instead. Every rejection is logged with its reason.

use super::Browser;
use crate::ffi::http::{self, ProxyMode};
use crate::ffi::process;
use crate::log::{info, warning};
use lb_core::protocol::{Event, PopupContent, PopupKind, UpdateChoice, UpdateProgress};
use lb_core::settings::{updater_proxy, UpdaterProxy};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::update::{self, Config, DownloadCheck, Manifest, Offer, Status, UpdateError};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Timer: the automatic check (first after a minute, then hourly; a check
/// runs when the last one is a day old).
pub(super) const APP_UPDATE_TIMER: usize = 10;
/// Timer: refreshes the download progress in the prompt.
pub(super) const UPDATE_PROGRESS_TIMER: usize = 11;
const TICK_MS: u32 = 60 * 60 * 1000;
const PROGRESS_MS: u32 = 250;
const MANIFEST_TIMEOUT_MS: i32 = 30_000;
const DOWNLOAD_TIMEOUT_MS: i32 = 60_000;

type Reply = (Option<TabId>, Option<u32>);

/// A running installer download.
struct Download {
    bytes: Arc<AtomicU64>,
    size: u64,
}

pub(super) struct UpdateHost {
    /// `None` when this build's update server or key is unusable (logged).
    config: Result<Config, UpdateError>,
    status: RefCell<Status>,
    path: PathBuf,
    checking: Cell<bool>,
    /// The checked manifest of a newer version.
    offered: RefCell<Option<Manifest>>,
    download: RefCell<Option<Download>>,
    /// The download failed (shown in the prompt until the next try).
    failed: RefCell<Option<String>>,
    waiting: RefCell<Vec<Reply>>,
    auto_checked: Cell<bool>,
}

/// The update server and release key built into this copy (`build.rs`).
fn build_config() -> Result<Config, UpdateError> {
    let key = update::public_key_from_pem(include_str!(env!("KRUNK_UPDATE_PUBKEY_PATH")))?;
    Config::new(env!("KRUNK_UPDATE_BASE_URL"), key, cfg!(feature = "dev-updates"))
}

fn current_version() -> update::Version {
    #[allow(clippy::expect_used)] // Cargo guarantees a semver package version
    update::parse_version(env!("CARGO_PKG_VERSION")).expect("package version")
}

/// Where downloaded installers go (emptied before each download and at start).
fn download_dir() -> PathBuf {
    std::env::temp_dir().join("Krunk-update")
}

impl UpdateHost {
    pub fn new(status_path: PathBuf) -> UpdateHost {
        let status = std::fs::read_to_string(&status_path).map(|j| Status::from_json(&j)).unwrap_or_default();
        UpdateHost {
            config: build_config(),
            status: RefCell::new(status),
            path: status_path,
            checking: Cell::new(false),
            offered: RefCell::new(None),
            download: RefCell::new(None),
            failed: RefCell::new(None),
            waiting: RefCell::new(Vec::new()),
            auto_checked: Cell::new(false),
        }
    }
}

impl Browser {
    pub(super) fn init_updates(&self) {
        // The installer of the last update has finished by now. When Krunk was
        // started by that installer, the installer is still exiting and holds its
        // own file for a moment: try again a little later.
        let _ = std::fs::remove_dir_all(download_dir());
        for delay in [30_000, 120_000] {
            crate::ffi::cef::post_ui_delayed(delay, || {
                if download_dir().exists() && std::fs::remove_dir_all(download_dir()).is_ok() {
                    info!("update: removed the previous installer download");
                }
            });
        }
        match &self.updates.config {
            Ok(c) => info!(
                "update check: {} ({})",
                c.manifest_url(),
                if process::is_installed() { "installed" } else { "portable" }
            ),
            Err(e) => warning!("update check disabled: {e}"),
        }
        if let Some(w) = self.app() {
            w.set_timer(APP_UPDATE_TIMER, update::FIRST_CHECK_DELAY_MS);
        }
    }

    /// `APP_UPDATE_TIMER`: the first tick checks, later ones when a day passed.
    pub(super) fn update_timer(&self) {
        if let Some(w) = self.app() {
            w.set_timer(APP_UPDATE_TIMER, TICK_MS);
        }
        if !self.settings.borrow().check_updates_automatically {
            return;
        }
        let first = !self.updates.auto_checked.replace(true);
        if first || update::check_due(self.updates.status.borrow().last_check_ms, now_ms()) {
            self.check_for_updates(None);
        }
    }

    /// Fetches and checks the manifest on a worker thread. `reply` gets the
    /// status when it is done.
    pub(super) fn check_for_updates(&self, reply: Option<Reply>) {
        if let Some(r) = reply {
            self.updates.waiting.borrow_mut().push(r);
        }
        if self.updates.checking.get() {
            return;
        }
        let config = match &self.updates.config {
            Ok(c) => c.clone(),
            Err(e) => {
                let e = e.to_string();
                self.update_checked(Err(e));
                return;
            }
        };
        let proxy = match updater_proxy(&self.settings.borrow().proxy) {
            UpdaterProxy::Skip => {
                self.update_checked(Err("skipped: a SOCKS proxy is configured (WinHTTP would bypass it)".into()));
                return;
            }
            UpdaterProxy::Named(p) => ProxyMode::Named(p),
            UpdaterProxy::System => ProxyMode::System,
        };
        self.updates.checking.set(true);
        self.spawn_job(
            "update-check",
            move || {
                let agent = format!("Krunk-Update/{} ({})", env!("CARGO_PKG_VERSION"), update::CHANNEL);
                let opts = http::Options {
                    proxy: &proxy,
                    timeout_ms: MANIFEST_TIMEOUT_MS,
                    user_agent: &agent,
                    allow_http: config.allow_http,
                };
                let manifest = http::get_with(config.manifest_url().as_str(), update::MAX_MANIFEST_BYTES, &opts)
                    .map_err(|e| format!("latest.json: {e}"))?;
                let sig = http::get_with(config.signature_url().as_str(), update::MAX_SIGNATURE_BYTES, &opts)
                    .map_err(|e| format!("latest.json.sig: {e}"))?;
                update::verify(&manifest, &sig, &config).map_err(|e| e.to_string())
            },
            |b, r: Result<Manifest, String>| {
                b.updates.checking.set(false);
                b.update_checked(r);
            },
        );
    }

    fn update_checked(&self, r: Result<Manifest, String>) {
        let current = current_version();
        let (result, ok, offer) = match r {
            Ok(m) => match update::offer(&current, &m) {
                Offer::UpToDate => {
                    info!("update check: up to date (server has {}, released {})", m.version, m.released);
                    *self.updates.offered.borrow_mut() = None;
                    ("Krunk is up to date".to_string(), true, None)
                }
                o => {
                    info!("update check: Krunk {} is available ({o:?}, {} bytes)", m.version, m.size);
                    let text = format!("Krunk {} is available", m.version);
                    *self.updates.offered.borrow_mut() = Some(m);
                    (text, true, Some(o))
                }
            },
            Err(e) => {
                warning!("update check rejected: {e}");
                (format!("The update check failed: {e}"), false, None)
            }
        };
        {
            let mut s = self.updates.status.borrow_mut();
            s.last_check_ms = now_ms();
            s.last_result = result;
            s.last_ok = ok;
            let _ = lb_core::vault::write_atomic(&self.updates.path, s.to_json().as_bytes());
        }
        let waiting = std::mem::take(&mut *self.updates.waiting.borrow_mut());
        let status = self.update_status();
        for (tab, req_id) in waiting {
            self.reply(tab, Event::ok(req_id, &status));
        }
        // Offer it, unless another prompt is open (Settings → About still shows it).
        if offer.is_some() && matches!(self.popup_kind(), None | Some(PopupKind::Update)) {
            self.show_update_prompt(UpdateProgress::Offered);
        }
    }

    /// Can this copy install the offered update itself?
    fn can_install(&self, m: &Manifest) -> bool {
        process::is_installed() && update::offer(&current_version(), m) == Offer::Install
    }

    fn show_update_prompt(&self, state: UpdateProgress) {
        let Some(content) = self.update_prompt_content(state) else { return };
        self.show_prompt(content, 0);
    }

    fn update_prompt_content(&self, state: UpdateProgress) -> Option<PopupContent> {
        let offered = self.updates.offered.borrow();
        let m = offered.as_ref()?;
        Some(PopupContent::Update {
            version: m.version.to_string(),
            current: env!("CARGO_PKG_VERSION").to_string(),
            install: self.can_install(m),
            has_notes: m.notes_url.is_some(),
            state,
        })
    }

    /// For the settings page.
    pub(super) fn update_status(&self) -> serde_json::Value {
        let s = self.updates.status.borrow();
        let offered = self.updates.offered.borrow();
        let available = offered.as_ref().map(|m| {
            serde_json::json!({
                "version": m.version.to_string(),
                "released": m.released,
                "install": self.can_install(m),
                "notes": m.notes_url.is_some(),
            })
        });
        let downloading = self.updates.download.borrow().as_ref().map(percent);
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "server": self.updates.config.as_ref().ok().map(|c| c.base.to_string()),
            "configError": self.updates.config.as_ref().err().map(|e| e.to_string()),
            "installed": process::is_installed(),
            "lastCheck": s.last_check_ms,
            "lastResult": s.last_result,
            "lastOk": s.last_ok,
            "checking": self.updates.checking.get(),
            "available": available,
            "downloading": downloading,
            "failed": *self.updates.failed.borrow(),
        })
    }

    /// The prompt's (or the settings page's) answer.
    pub(super) fn update_reply(&self, choice: UpdateChoice) {
        let Some(m) = self.updates.offered.borrow().clone() else {
            self.hide_update_prompt();
            return;
        };
        match choice {
            UpdateChoice::Later => self.hide_update_prompt(),
            UpdateChoice::Notes => {
                self.hide_update_prompt();
                if let Some(u) = &m.notes_url {
                    self.open_url(u.as_str(), true, false);
                }
            }
            UpdateChoice::Install if !self.can_install(&m) => {
                // Portable copy (or too old to update in place): the download page.
                self.hide_update_prompt();
                if let Ok(c) = &self.updates.config {
                    self.open_url(c.base.as_str(), true, false);
                }
            }
            UpdateChoice::Install => self.start_update_download(m),
        }
    }

    fn hide_update_prompt(&self) {
        if self.popup_kind() == Some(PopupKind::Update) {
            self.hide_popup();
        }
    }

    fn start_update_download(&self, m: Manifest) {
        if self.updates.download.borrow().is_some() {
            self.show_update_prompt(UpdateProgress::Downloading { percent: self.download_percent() });
            return;
        }
        let proxy = match updater_proxy(&self.settings.borrow().proxy) {
            UpdaterProxy::Skip => {
                self.update_failed("a SOCKS proxy is configured; download the update from the website".into());
                return;
            }
            UpdaterProxy::Named(p) => ProxyMode::Named(p),
            UpdaterProxy::System => ProxyMode::System,
        };
        let allow_http = self.updates.config.as_ref().is_ok_and(|c| c.allow_http);
        let bytes = Arc::new(AtomicU64::new(0));
        *self.updates.download.borrow_mut() = Some(Download { bytes: bytes.clone(), size: m.size });
        *self.updates.failed.borrow_mut() = None;
        self.show_update_prompt(UpdateProgress::Downloading { percent: 0 });
        if let Some(w) = self.app() {
            w.set_timer(UPDATE_PROGRESS_TIMER, PROGRESS_MS);
        }
        info!("update: downloading {} ({} bytes)", m.url, m.size);
        self.spawn_job(
            "update-download",
            move || download_installer(&m, &proxy, allow_http, &bytes),
            |b, r: Result<PathBuf, String>| b.update_downloaded(r),
        );
    }

    fn download_percent(&self) -> u8 {
        self.updates.download.borrow().as_ref().map_or(0, percent)
    }

    /// `UPDATE_PROGRESS_TIMER`.
    pub(super) fn update_progress_timer(&self) {
        if self.updates.download.borrow().is_none() {
            if let Some(w) = self.app() {
                w.kill_timer(UPDATE_PROGRESS_TIMER);
            }
            return;
        }
        // Only an open prompt is refreshed (a dismissed one stays closed).
        if self.popup_kind() == Some(PopupKind::Update) {
            if let Some(c) =
                self.update_prompt_content(UpdateProgress::Downloading { percent: self.download_percent() })
            {
                self.show_popup(c, None);
            }
        }
    }

    fn update_downloaded(&self, r: Result<PathBuf, String>) {
        self.updates.download.borrow_mut().take();
        if let Some(w) = self.app() {
            w.kill_timer(UPDATE_PROGRESS_TIMER);
        }
        let path = match r {
            Ok(p) => p,
            Err(e) => {
                warning!("update download rejected: {e}");
                self.update_failed(e);
                return;
            }
        };
        info!("update: installer verified, starting {} /S /RELAUNCH", path.display());
        if let Err(e) = process::start_installer(&path, &["/S", "/RELAUNCH"]) {
            warning!("update: cannot start the installer: {e}");
            self.update_failed(format!("cannot start the installer: {e}"));
            return;
        }
        self.show_update_prompt(UpdateProgress::Installing);
        // The installer waits until Krunk has exited, installs, and starts it again.
        self.begin_exit();
    }

    fn update_failed(&self, error: String) {
        *self.updates.failed.borrow_mut() = Some(error.clone());
        self.show_update_prompt(UpdateProgress::Failed { error });
    }
}

fn percent(d: &Download) -> u8 {
    let got = d.bytes.load(Ordering::Relaxed);
    (got.saturating_mul(100) / d.size.max(1)).min(100) as u8
}

/// Downloads the installer into a fresh folder in %TEMP% and checks it
/// against the signed manifest. A file that does not match is deleted.
fn download_installer(m: &Manifest, proxy: &ProxyMode, allow_http: bool, bytes: &AtomicU64) -> Result<PathBuf, String> {
    let dir = download_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("Krunk-Setup-{}.exe", m.version));
    let result = (|| {
        let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let agent = format!("Krunk-Update/{} ({})", env!("CARGO_PKG_VERSION"), update::CHANNEL);
        let opts = http::Options { proxy, timeout_ms: DOWNLOAD_TIMEOUT_MS, user_agent: &agent, allow_http };
        let mut check = DownloadCheck::default();
        http::fetch(m.url.as_str(), m.size, &opts, &mut |chunk| {
            check.update(chunk);
            bytes.store(check.bytes(), Ordering::Relaxed);
            file.write_all(chunk).map_err(|e| format!("writing the installer: {e}"))
        })?;
        file.sync_all().map_err(|e| format!("writing the installer: {e}"))?;
        drop(file);
        check.finish(m).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}
