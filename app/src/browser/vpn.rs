//! The in-browser VPN (0.14.0): only Krunk's own connections go through it, never the system's.
//!
//! The `krunk_vpn` crate runs the tunnel and a local HTTP proxy on `127.0.0.1:<random>`; while
//! it runs the engine's `proxy` preference (both profiles) and Krunk's own HTTP (updates, sync,
//! filter lists, extensions) point at that proxy. It is fail-closed: while the tunnel is down the
//! proxy answers 503, nothing goes direct, and a VPN set to connect at start is in place before
//! the first page loads and before any of Krunk's own requests (if it can't start then, a local
//! stand-in refuses every request until the user connects or disconnects; if not even that can
//! be put in place, Krunk stays offline: no tabs are restored, only Settings → VPN opens, page
//! requests are blocked and Krunk's own requests are skipped).
//!
//! Each change of route (connect, switch, disconnect) closes the engine's open connections so
//! they come back the new way, and ends Krunk's own requests decided before (`http::route_changed`;
//! downloads in parts go on through the new route). WebRTC peer connections can't move.
//!
//! Profiles are stored in `vpn_profiles` as DPAPI blobs (`DpapiVpn`) of the file and the OpenVPN
//! user name and password; they are opened in zeroizing memory only to connect, and never sent
//! to a page, logged or synced.

use super::Browser;
use crate::ffi::cef as engine;
use crate::ffi::dialogs;
use crate::ffi::http::{self, ProxyMode};
use crate::ffi::secure::DpapiVpn;
use crate::log::{info, warning};
use lb_core::pages::Page;
use lb_core::protocol::{Event, PopupContent, PopupKind, PopupRect, Request, Sender};
use lb_core::settings::UpdaterProxy;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::vpn::{self as v, ProfileSecret, VpnKind, VpnProfile, VpnProfileView, VpnState, VpnStatus, VpnView};
use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

/// `WM_APP` code: the VPN's state changed (or its counters moved).
pub(super) const APP_VPN: usize = 6;

#[derive(Default)]
pub(super) struct VpnHost {
    running: RefCell<Option<Running>>,
    /// The stand-in that refuses every request when the VPN should be on and couldn't start.
    block: RefCell<Option<Block>>,
    status: RefCell<VpnStatus>,
    /// The profiles as the UI shows them (re-read after each change).
    profiles: RefCell<Vec<VpnProfileView>>,
    /// The latest status from the VPN's runtime thread, with the run it belongs to.
    latest: Arc<Mutex<Option<(u64, krunk_vpn::Status)>>>,
    generation: Cell<u64>,
    /// The VPN should run, but neither it nor the stand-in is the engine's proxy: page requests
    /// are blocked and Krunk's own requests skipped until the user connects or disconnects.
    offline: Cell<bool>,
    /// The saved session is kept as it was (start-up opened only Settings → VPN, offline).
    hold_session: Cell<bool>,
}

/// Who gets a reply.
#[derive(Clone, Copy)]
enum To {
    Tab(Option<TabId>),
    Popup,
}

struct Running {
    vpn: krunk_vpn::Vpn,
    id: String,
    addr: SocketAddr,
}

/// A loopback listener that answers every connection with `503 VPN not connected`.
struct Block {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
}

const BLOCK_ANSWER: &[u8] = b"HTTP/1.1 503 VPN not connected\r\nContent-Type: text/plain\r\nContent-Length: 17\r\n\
Connection: close\r\n\r\nVPN not connected";

impl Block {
    fn start() -> std::io::Result<Block> {
        // Exclusive on Windows, like the VPN's own proxy: no other program can share the port.
        let listener = krunk_vpn::bind_loopback()?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::Builder::new().name("vpn-block".into()).spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(mut s) = conn {
                    let _ = s.set_write_timeout(Some(std::time::Duration::from_secs(1)));
                    let _ = s.write_all(BLOCK_ANSWER);
                }
            }
        })?;
        Ok(Block { addr, stop })
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wakes the accept loop so it sees the flag.
        let _ = TcpStream::connect_timeout(&self.addr, std::time::Duration::from_millis(200));
    }
}

/// Stops a VPN on a thread of its own: a graceful stop waits for the tunnel to close (seconds),
/// which the UI thread must not. Its proxy answers 503 until it is closed.
fn stop_later(vpn: krunk_vpn::Vpn) {
    let spawned = std::thread::Builder::new().name("vpn-stop".into()).spawn(move || vpn.stop());
    if let Err(e) = spawned {
        warning!("vpn: no thread to stop it on ({e}); stopping here");
    }
}

fn kind_of(k: krunk_vpn::Kind) -> VpnKind {
    match k {
        krunk_vpn::Kind::WireGuard => VpnKind::WireGuard,
        krunk_vpn::Kind::OpenVpn => VpnKind::OpenVpn,
    }
}

fn status_of(s: &krunk_vpn::Status) -> VpnStatus {
    use krunk_vpn::State as S;
    VpnStatus {
        state: match s.state {
            S::Connecting => VpnState::Connecting,
            S::Connected => VpnState::Connected,
            S::Reconnecting => VpnState::Reconnecting,
            S::Failed => VpnState::Failed,
            S::Stopped => VpnState::Stopped,
        },
        error: s.error.clone(),
        since_ms: s.since_ms,
        rx_bytes: s.rx_bytes,
        tx_bytes: s.tx_bytes,
        tunnel_ip: s.tunnel_ip.clone(),
        server: s.server.clone(),
    }
}

impl Browser {
    /// At start, after the engine is up and before any tab is restored or any of Krunk's own
    /// requests starts: the profile list and, with auto-connect, the VPN (its proxy is in place
    /// before the first navigation). False when tabs must not be restored: the VPN should run
    /// and not even the stand-in could be put in place (Krunk stays offline).
    pub(super) fn init_vpn(&self) -> bool {
        self.vpn_reload_profiles();
        *self.vpn.status.borrow_mut() = VpnStatus { since_ms: now_ms(), ..VpnStatus::default() };
        let (auto, id) = {
            let s = self.settings.borrow();
            (s.vpn.auto_connect, s.vpn.active_profile.clone())
        };
        if !auto {
            // The engine keeps the proxy preference on disk: after a run that ended with the VPN
            // on, it would still point at that run's (now closed) proxy port and every page would
            // fail. Without auto-connect the start is direct, so put it back first (found on the
            // VM, 0.14.0). Only when one is left: resetting an unset preference made the engine
            // log a check failure (0.14.1).
            if engine::clear_stale_vpn_proxy() {
                info!("vpn: removed the proxy an earlier run left set");
            }
            return true;
        }
        info!("vpn: connecting at start");
        let Err(e) = self.vpn_connect(&id) else { return true };
        warning!("vpn: could not connect at start: {e}");
        let blocked = self.vpn_block(&format!(
            "Could not connect at start: {e}. Krunk's connections are stopped until you connect or disconnect."
        ));
        if !blocked {
            warning!(
                "vpn: Krunk stays offline (no tabs restored) until the VPN connects or auto-connect is turned off"
            );
            self.vpn.hold_session.set(true);
            let mut status = self.vpn.status.borrow_mut();
            status.error = Some(format!(
                "The VPN couldn't start ({e}). Krunk stays offline until you connect or turn auto-connect off; \
                 your tabs are kept for the next start."
            ));
        }
        blocked
    }

    /// Fail-closed when the VPN should run and can't: every connection of the engine and of
    /// Krunk goes to a local stand-in that refuses it. False when not even that could be put in
    /// place: then Krunk is offline (page requests blocked, its own requests skipped).
    fn vpn_block(&self, error: &str) -> bool {
        let installed = match Block::start() {
            Ok(b) => {
                let ok = self.engine_vpn_proxy(&b.addr.to_string());
                if ok {
                    *self.vpn.block.borrow_mut() = Some(b);
                } else {
                    warning!("vpn: the engine did not take the blocking proxy");
                    // Not dropped: the engine may point at it in part (a profile that took it).
                    std::mem::forget(b);
                }
                ok
            }
            Err(e) => {
                warning!("vpn: the blocking stand-in could not start: {e}");
                false
            }
        };
        self.vpn.offline.set(!installed);
        self.net.set_offline(!installed, self.vpn_private_only());
        self.vpn_route_changed();
        let server = self.vpn.status.borrow().server.clone();
        *self.vpn.status.borrow_mut() = VpnStatus {
            state: VpnState::Failed,
            error: Some(error.into()),
            since_ms: now_ms(),
            server,
            ..Default::default()
        };
        installed
    }

    /// The route of the engine and of Krunk's requests changed (connect, switch, stand-in,
    /// disconnect): requests decided before end (`http::route_changed`), downloads in parts go
    /// on through the new route, and the DNS settings follow (no custom DNS while the VPN runs).
    /// The engine's open connections were closed by `engine::set_vpn_proxy`.
    fn vpn_route_changed(&self) {
        http::route_changed();
        self.parts_reroute();
        self.apply_dns();
    }

    /// Start-up opened only Settings → VPN (offline): the saved session is kept as it was.
    pub(super) fn vpn_holds_session(&self) -> bool {
        self.vpn.hold_session.get()
    }

    fn vpn_bypass(&self) -> String {
        let local = self.settings.borrow().vpn.allow_local_network;
        self.running_settings.vpn_bypass_list_with(local)
    }

    /// 0.22.0: the VPN is for private tabs only (`vpn.scope`): normal tabs and Krunk's own
    /// requests go direct, and only private tabs are refused while it is down.
    pub(super) fn vpn_private_only(&self) -> bool {
        self.settings.borrow().vpn.scope.private_only()
    }

    /// Points the engine at the local proxy `addr` (the VPN's or the stand-in's), for the
    /// contexts the scope says. True when the engine took it.
    fn engine_vpn_proxy(&self, addr: &str) -> bool {
        engine::set_vpn_proxy(Some((format!("http://{addr}"), self.vpn_bypass())), self.vpn_private_only())
    }

    /// The local proxy Krunk's traffic goes to now (the VPN's or the stand-in's), `host:port`.
    pub(super) fn vpn_proxy_addr(&self) -> Option<String> {
        if let Some(r) = self.vpn.running.borrow().as_ref() {
            return Some(r.addr.to_string());
        }
        self.vpn.block.borrow().as_ref().map(|b| b.addr.to_string())
    }

    /// How Krunk's own requests reach the network: through the VPN while it runs, else as the
    /// proxy setting says (`UpdaterProxy::Skip` for SOCKS, which WinHTTP can't use, and while
    /// Krunk is offline because the VPN couldn't start).
    /// With the VPN for private tabs only they follow normal tabs: direct (0.22.0).
    pub(super) fn own_proxy(&self) -> UpdaterProxy {
        let private_only = self.vpn_private_only();
        if self.vpn.offline.get() && !private_only {
            return UpdaterProxy::Skip;
        }
        let vpn = if private_only { None } else { self.vpn_proxy_addr() };
        lb_core::settings::krunk_proxy(&self.settings.borrow().proxy, vpn.as_deref())
    }

    /// [`Browser::own_proxy`] for WinHTTP, with the route generation of now; `None`: don't.
    pub(super) fn own_route(&self) -> Option<ProxyMode> {
        ProxyMode::from_updater(self.own_proxy())
    }

    /// Why [`Browser::own_route`] said no: `socks` (a SOCKS proxy), or that Krunk is offline.
    pub(super) fn own_route_skipped(&self, socks: &str) -> String {
        if self.vpn.offline.get() && !self.vpn_private_only() {
            "skipped: the VPN couldn't start (Krunk stays offline until you connect or turn auto-connect off)".into()
        } else {
            socks.into()
        }
    }

    /// The VPN is on (its proxy, or the stand-in, is the engine's; or Krunk is offline because
    /// neither could be put in place).
    pub(super) fn vpn_engaged(&self) -> bool {
        self.vpn_proxy_addr().is_some() || self.vpn.offline.get()
    }

    fn vpn_reload_profiles(&self) {
        let rows = match self.store.borrow().vpn_profiles() {
            Ok(r) => r,
            Err(e) => {
                warning!("vpn: reading the profiles failed: {e}");
                Vec::new()
            }
        };
        // Whether a user name and password are stored is a column: no blob is opened here.
        let views = rows.iter().map(VpnProfileView::new).collect();
        *self.vpn.profiles.borrow_mut() = views;
    }

    pub(super) fn vpn_view(&self) -> VpnView {
        let active = match self.vpn.running.borrow().as_ref() {
            Some(r) => r.id.clone(),
            None => self.settings.borrow().vpn.active_profile.clone(),
        };
        VpnView {
            status: self.vpn.status.borrow().clone(),
            profiles: self.vpn.profiles.borrow().clone(),
            active,
            show_button: self.settings.borrow().vpn.show_button,
            scope: self.settings.borrow().vpn.scope,
        }
    }

    /// The `vpn` event to every Settings page, every shell (the button) and an open VPN popup.
    pub(super) fn vpn_push(&self) {
        let event = Event::Vpn { view: self.vpn_view() };
        let json = event.to_json();
        let tabs: Vec<TabId> = self
            .ws
            .borrow()
            .tabs()
            .filter(|(_, t)| Page::from_url(&t.url) == Some(Page::Settings))
            .map(|(_, t)| t.id)
            .collect();
        for id in tabs {
            if let Some(view) = self.tab_view(id) {
                view.post_json(&json);
            }
        }
        self.post_shell(&event);
        if self.popup_kind() == Some(PopupKind::Vpn) {
            let popup =
                self.popup.try_borrow().ok().and_then(|p| p.as_ref().filter(|p| p.ready).and_then(|p| p.view.clone()));
            if let Some(view) = popup {
                view.post_json(&json);
            }
        }
    }

    /// The button's state for a shell that has just loaded.
    pub(super) fn send_vpn_to(&self, win: lb_core::windows::WindowId) {
        self.post_shell_to(win, &Event::Vpn { view: self.vpn_view() });
    }

    /// The toolbar button: the popup at `rect` (shell CSS px).
    pub(super) fn show_vpn_popup(&self, rect: PopupRect) {
        self.show_popup(PopupContent::Vpn { view: self.vpn_view() }, Some(rect));
    }

    pub(super) fn dispatch_vpn(&self, sender: Sender, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let to = if sender == Sender::Popup { To::Popup } else { To::Tab(tab) };
        match request {
            Request::VpnState => self.vpn_reply(to, Event::ok(req_id, self.vpn_view())),
            Request::VpnImport {} => self.vpn_import(tab, req_id),
            Request::VpnRename { id, name } => {
                let r =
                    v::clean_name(&name).and_then(|name| match self.store.borrow().vpn_profile_rename(&id, &name) {
                        Ok(true) => Ok(()),
                        Ok(false) => Err("This profile is gone".to_string()),
                        Err(e) => {
                            warning!("vpn: renaming failed: {e}");
                            Err("Could not save the name".to_string())
                        }
                    });
                self.vpn_answer(to, req_id, r);
            }
            Request::VpnRemove { id } => {
                let r = self.vpn_remove(&id);
                self.vpn_answer(to, req_id, r);
            }
            Request::VpnCredentials { id, username, password } => {
                let password = password.into_zeroizing();
                let r = self.vpn_set_credentials(&id, Zeroizing::new(username), password);
                self.vpn_answer(to, req_id, r);
            }
            Request::VpnConnect { .. } | Request::VpnDisconnect {} | Request::VpnScope { .. }
                if sender == Sender::Popup && !v::popup_may_control(self.popup_kind()) =>
            {
                // The popup browser also shows menus and other panels: only the VPN popup itself
                // starts or stops the VPN.
                warning!("vpn: a connect or disconnect from a popup that is not the VPN's was refused");
                self.vpn_reply(to, Event::err(req_id, "Not from this popup"));
            }
            Request::VpnConnect { id } => {
                let r = self.vpn_connect(&id);
                if let Err(e) = &r {
                    info!("vpn: not connected: {e}");
                }
                self.vpn_answer(to, req_id, r);
            }
            Request::VpnDisconnect {} => {
                self.vpn_disconnect();
                self.vpn_answer(to, req_id, Ok(()));
            }
            Request::VpnScope { scope } => {
                // Applied by `vpn_settings_changed` (the engine, Krunk's requests, the block).
                let r = self.try_change_settings(move |s| s.vpn.scope = scope);
                self.vpn_answer(to, req_id, r);
            }
            _ => {}
        }
    }

    /// Replies with the new view (or the error) and tells everyone else.
    fn vpn_answer(&self, to: To, req_id: Option<u32>, r: Result<(), String>) {
        self.vpn_reload_profiles();
        match r {
            Ok(()) => self.vpn_reply(to, Event::ok(req_id, self.vpn_view())),
            Err(e) => self.vpn_reply(to, Event::err(req_id, e)),
        }
        self.vpn_push();
    }

    /// A reply to the Settings page that asked, or to the popup (its connect and disconnect).
    fn vpn_reply(&self, to: To, event: Event) {
        match to {
            To::Tab(tab) => self.reply(tab, event),
            To::Popup => {
                let popup = self.popup.try_borrow().ok().and_then(|p| p.as_ref().and_then(|p| p.view.clone()));
                if let Some(view) = popup {
                    view.post_json(&event.to_json());
                }
            }
        }
    }

    /// Settings → VPN → Import file…: the native dialog, the file read here (never by the page).
    fn vpn_import(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.window() else { return };
        if self.store.borrow().vpn_profile_count().unwrap_or(0) >= v::MAX_PROFILES {
            return self.reply(tab, Event::err(req_id, "At most 32 profiles: remove one first"));
        }
        let Some(path) = dialogs::open_file(
            w.hwnd(),
            "Import a VPN profile",
            ("VPN profiles (WireGuard, OpenVPN)", "*.conf;*.ovpn"),
        ) else {
            return self.reply(tab, Event::ok(req_id, serde_json::Value::Null));
        };
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let r = self.vpn_import_file(&path, &file_name);
        match r {
            Ok((view, warnings)) => {
                self.vpn_reload_profiles();
                self.reply(tab, Event::ok(req_id, serde_json::json!({ "profile": view, "warnings": warnings })));
                self.vpn_push();
            }
            Err(e) => self.reply(tab, Event::err(req_id, e)),
        }
    }

    fn vpn_import_file(
        &self,
        path: &std::path::Path,
        file_name: &str,
    ) -> Result<(VpnProfileView, Vec<String>), String> {
        // Read with a bound (never more than the limit plus one byte, whatever the file says it
        // is), into a buffer that is never reallocated (no unzeroized copies).
        let file = std::fs::File::open(path).map_err(|e| format!("The file could not be read: {e}"))?;
        let mut bytes = Zeroizing::new(Vec::with_capacity(v::MAX_IMPORT_BYTES + 1));
        file.take(v::MAX_IMPORT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("The file could not be read: {e}"))?;
        if bytes.len() > v::MAX_IMPORT_BYTES {
            return Err("The file is larger than 64 KiB: this is not a VPN profile".into());
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "This is not a text file".to_string())?;
        let (profile, imported) = krunk_vpn::import(file_name, text).map_err(|e| e.to_string())?;
        drop(profile);
        let name: String = imported.name.chars().take(v::MAX_NAME_CHARS).collect();
        let row = VpnProfile {
            id: v::new_id(),
            name: v::clean_name(&name).unwrap_or_else(|_| v::name_from_file(file_name)),
            kind: kind_of(imported.kind),
            server: imported.server.chars().take(300).collect(),
            needs_credentials: imported.needs_credentials,
            created_ms: now_ms(),
            has_credentials: false,
        };
        // The file as it was imported: it is parsed (and rewritten) again at each connect.
        let secret = ProfileSecret { text: Zeroizing::new(text.to_string()), username: None, password: None };
        let blob = v::seal(&DpapiVpn, &secret).map_err(|e| {
            warning!("vpn: protecting the profile failed: {e}");
            "Could not protect the profile".to_string()
        })?;
        self.store.borrow().vpn_profile_add(&row, &blob).map_err(|e| {
            warning!("vpn: saving the profile failed: {e}");
            "Could not save the profile".to_string()
        })?;
        info!(
            "vpn: imported a {} profile ({} warning(s){})",
            row.kind.as_str(),
            imported.warnings.len(),
            if row.needs_credentials { ", needs a user name and password" } else { "" }
        );
        Ok((VpnProfileView::new(&row), imported.warnings))
    }

    fn vpn_remove(&self, id: &str) -> Result<(), String> {
        let running = self.vpn.running.borrow().as_ref().is_some_and(|r| r.id == id);
        if running {
            self.vpn_disconnect();
        }
        match self.store.borrow().vpn_profile_remove(id) {
            Ok(_) => {}
            Err(e) => {
                warning!("vpn: removing the profile failed: {e}");
                return Err("Could not remove the profile".into());
            }
        }
        if self.settings.borrow().vpn.active_profile == id {
            self.change_settings(|s| {
                s.vpn.active_profile.clear();
                s.vpn.auto_connect = false;
            });
        }
        info!("vpn: a profile was removed");
        Ok(())
    }

    fn vpn_set_credentials(
        &self,
        id: &str,
        username: Zeroizing<String>,
        password: Zeroizing<String>,
    ) -> Result<(), String> {
        v::check_credentials(&username, &password)?;
        let blob = self.store.borrow().vpn_profile_blob(id).ok().flatten().ok_or("This profile is gone")?;
        let old = v::open(&DpapiVpn, &blob).map_err(|e| {
            warning!("vpn: opening the profile failed: {e}");
            "The stored profile could not be opened. Import it again.".to_string()
        })?;
        let secret = ProfileSecret {
            text: old.text,
            username: Some(Zeroizing::new(username.trim().to_string())),
            password: Some(password),
        };
        let blob = v::seal(&DpapiVpn, &secret).map_err(|_| "Could not protect the password".to_string())?;
        match self.store.borrow().vpn_profile_set_blob(id, &blob, secret.has_credentials()) {
            Ok(true) => Ok(()),
            Ok(false) => Err("This profile is gone".into()),
            Err(e) => {
                warning!("vpn: saving the credentials failed: {e}");
                Err("Could not save the password".into())
            }
        }
    }

    /// Starts the VPN with profile `id` (replacing one that runs). The proxy is the engine's
    /// before this returns; the tunnel connects in the background.
    pub(super) fn vpn_connect(&self, id: &str) -> Result<(), String> {
        if !v::valid_id(id) {
            return Err("Choose a profile".into());
        }
        if !self.settings.borrow().proxy.trim().is_empty() || !self.running_settings.proxy.trim().is_empty() {
            return Err(
                "A manual proxy is set in Settings → Advanced. Remove it (and restart Krunk) to use the VPN.".into()
            );
        }
        // An engine switch that changes how proxies are used would take traffic around the VPN.
        let flag = lb_core::settings::vpn_conflicting_flag(&self.settings.borrow().extra_flags)
            .or_else(|| lb_core::settings::vpn_conflicting_flag(&self.running_settings.extra_flags));
        if let Some(flag) = flag {
            return Err(format!(
                "The engine flag {flag} (Settings → Advanced → extra flags) changes how proxies are used. \
                 Remove it and restart Krunk to use the VPN."
            ));
        }
        let same = self.vpn.running.borrow().as_ref().is_some_and(|r| r.id == id);
        if same && self.vpn.status.borrow().state != VpnState::Failed {
            return Ok(());
        }
        let row = self.store.borrow().vpn_profile(id).ok().flatten().ok_or("This profile is gone")?;
        let blob = self.store.borrow().vpn_profile_blob(id).ok().flatten().ok_or("This profile is gone")?;
        let secret = v::open(&DpapiVpn, &blob).map_err(|e| {
            warning!("vpn: opening the profile failed: {e}");
            "The stored profile could not be opened. Import it again.".to_string()
        })?;
        let (profile, _) = krunk_vpn::import("profile.conf", &secret.text).map_err(|e| e.to_string())?;
        let creds = match (&secret.username, &secret.password) {
            (Some(u), p) if !u.is_empty() => Some(krunk_vpn::Credentials {
                username: Zeroizing::new(u.to_string()),
                password: Zeroizing::new(p.as_ref().map(|p| p.to_string()).unwrap_or_default()),
            }),
            _ => None,
        };
        if row.needs_credentials && creds.is_none() {
            return Err("Enter the user name and password for this profile first (Settings → VPN)".into());
        }
        drop(secret);
        let generation = self.vpn.generation.get() + 1;
        let Some(app) = self.app() else { return Err("Krunk is closing".into()) };
        let notifier = app.notifier();
        let latest = self.vpn.latest.clone();
        let on_status = move |s: &krunk_vpn::Status| {
            if let Ok(mut l) = latest.lock() {
                *l = Some((generation, s.clone()));
            }
            notifier.notify(APP_VPN);
        };
        let vpn = krunk_vpn::Vpn::start(profile, creds, Box::new(crate::ffi::peer::peer_allowed), Box::new(on_status))
            .map_err(|e| e.to_string())?;
        let addr = vpn.proxy_addr();
        if !self.engine_vpn_proxy(&addr.to_string()) {
            // Back to what was there (another VPN, the stand-in, or nothing).
            let before = self.vpn_proxy_addr().map(|a| (format!("http://{a}"), self.vpn_bypass()));
            engine::set_vpn_proxy(before, self.vpn_private_only());
            stop_later(vpn);
            return Err("The engine did not take the VPN's proxy; nothing was changed".into());
        }
        self.vpn.generation.set(generation);
        *self.vpn.status.borrow_mut() = status_of(&vpn.status());
        let old = self.vpn.running.borrow_mut().replace(Running { vpn, id: id.to_string(), addr });
        self.vpn.block.borrow_mut().take();
        if let Some(old) = old {
            stop_later(old.vpn);
        }
        self.vpn.offline.set(false);
        self.net.set_offline(false, false);
        self.vpn_route_changed();
        info!("vpn: started a {} profile; Krunk's traffic goes through the local proxy {addr}", row.kind.as_str());
        if self.settings.borrow().vpn.active_profile != id {
            let id = id.to_string();
            self.change_settings(move |s| s.vpn.active_profile = id);
        }
        Ok(())
    }

    /// Stops the VPN (or the stand-in): the engine and Krunk's requests go direct again.
    pub(super) fn vpn_disconnect(&self) {
        let engaged = self.vpn_engaged();
        if engaged && !engine::set_vpn_proxy(None, false) {
            warning!("vpn: the engine kept the VPN's proxy setting; its connections fail until Krunk restarts");
        }
        self.vpn.block.borrow_mut().take();
        self.vpn.generation.set(self.vpn.generation.get() + 1);
        let running = self.vpn.running.borrow_mut().take();
        if let Some(r) = running {
            stop_later(r.vpn);
        }
        self.vpn.offline.set(false);
        self.net.set_offline(false, false);
        *self.vpn.status.borrow_mut() = VpnStatus { since_ms: now_ms(), ..VpnStatus::default() };
        if engaged {
            self.vpn_route_changed();
            info!("vpn: disconnected; Krunk's traffic goes direct again");
        }
    }

    /// `WM_APP(APP_VPN)`: the VPN's runtime reported a state or new counters.
    pub(super) fn vpn_status_ready(&self) {
        let latest = self.vpn.latest.lock().ok().and_then(|mut l| l.take());
        let Some((generation, s)) = latest else { return };
        if generation != self.vpn.generation.get() || self.vpn.running.borrow().is_none() {
            return;
        }
        let new = status_of(&s);
        let before = self.vpn.status.borrow().state;
        if before != new.state {
            match &new.error {
                Some(e) => info!("vpn: {:?} ({e})", new.state),
                None => info!("vpn: {:?}", new.state),
            }
        }
        *self.vpn.status.borrow_mut() = new;
        self.vpn_push();
    }

    /// A VPN setting changed (the button, auto-connect, the profile used last, the local
    /// network's bypass, the scope).
    pub(super) fn vpn_settings_changed(&self, bypass_changed: bool, scope_changed: bool) {
        // The local network's bypass or the scope changed while the engine uses the VPN's proxy
        // (or the stand-in): the engine takes it at once (0.14.2, 0.22.0), and its open
        // connections are closed so they come back the new way.
        if bypass_changed || scope_changed {
            if let Some(addr) = self.vpn_proxy_addr() {
                if !self.engine_vpn_proxy(&addr) {
                    warning!("vpn: the engine did not take the new proxy settings");
                } else if bypass_changed {
                    let local = self.settings.borrow().vpn.allow_local_network;
                    info!("vpn: the local network {} the VPN", if local { "now skips" } else { "goes through" });
                }
            }
        }
        if scope_changed {
            let private_only = self.vpn_private_only();
            info!("vpn: {}", if private_only { "for private tabs only" } else { "for every tab" });
            if self.vpn.offline.get() {
                self.net.set_offline(true, private_only);
            }
            if self.vpn_engaged() {
                // Krunk's own requests and the DNS settings follow normal tabs.
                self.vpn_route_changed();
            }
        }
        self.vpn_push();
    }

    /// Exit: the tunnel stops; the engine keeps the proxy setting and the proxy (or the
    /// stand-in) keeps its port, answering 503, until the process ends, so nothing goes direct
    /// and no other program can take the port while the engine's last requests end.
    pub(super) fn vpn_shutdown(&self) {
        self.vpn.generation.set(self.vpn.generation.get() + 1);
        let running = self.vpn.running.borrow_mut().take();
        if let Some(r) = running {
            info!("vpn: stopping for exit");
            r.vpn.stop_for_exit();
        }
        if let Some(b) = self.vpn.block.borrow_mut().take() {
            // Its thread keeps answering until the process ends.
            std::mem::forget(b);
        }
    }
}
