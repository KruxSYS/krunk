//! Ad and tracker blocking, privacy headers, tracking-parameter stripping,
//! HTTPS-only mode and the filter-list updater.
//!
//! Hot path: [`Browser::request_handler`] runs on the UI thread for every
//! request of a tab. It touches only in-memory state (engine, allowlist
//! flags precomputed per navigation) and never the database.

use super::Browser;
use crate::ffi::http::{self, ProxyMode};
use crate::ffi::webview::{Controller, ResourceAction, ResourceRequest};
use crate::log::{info, warning};
use lb_core::assets;
use lb_core::blocker::{self, Allowlist, Blocker, Decision, ResourceKind};
use lb_core::filterlists::{self, Rebuilt, UpdateState, MAX_LIST_BYTES};
use lb_core::pages::Page;
use lb_core::protocol::{ContentMessage, Event, PopupContent, Request};
use lb_core::settings::{updater_proxy, TrackingPrevention, UpdaterProxy, FILTER_LISTS};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::urlclean::{self, RewriteOptions};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc;

/// `WM_APP` code: the filter worker finished.
pub(super) const APP_FILTERS_READY: usize = 1;
/// `WM_APP` code: run deferred tasks.
pub(super) const APP_DEFERRED: usize = 2;
/// Timer: push the shield counter (at most ~4 times a second).
pub(super) const SHIELD_TIMER: usize = 2;
const SHIELD_DEBOUNCE_MS: u32 = 250;
/// Timer: first filter-list update check, a minute after start.
pub(super) const UPDATE_TIMER: usize = 3;
const FIRST_UPDATE_DELAY_MS: u32 = 60_000;
/// Rewrites of one navigation chain before giving up (defence in depth;
/// `navigation_rewrite` already reaches a fixed point after one).
const MAX_REWRITES: u8 = 3;
const DOWNLOAD_TIMEOUT_MS: i32 = 30_000;

/// Blocking state shared by every tab's request handler.
pub(super) struct FilterState {
    pub engine: RefCell<Option<Blocker>>,
    pub allowlist: RefCell<Allowlist>,
    pub enabled: Cell<bool>,
    pub dnt: Cell<bool>,
    pub gpc: Cell<bool>,
    pub update: RefCell<UpdateState>,
    busy: Cell<bool>,
    rx: RefCell<Option<mpsc::Receiver<Rebuilt>>>,
    /// Replies owed to "Update now" requests.
    pending_replies: RefCell<Vec<(Option<TabId>, Option<u32>)>>,
    /// Hosts the user allowed over plain HTTP this session (HTTPS-only).
    pub http_exceptions: RefCell<HashSet<String>>,
}

impl FilterState {
    pub fn new() -> FilterState {
        FilterState {
            engine: RefCell::new(None),
            allowlist: RefCell::new(Allowlist::default()),
            enabled: Cell::new(false),
            dnt: Cell::new(false),
            gpc: Cell::new(false),
            update: RefCell::new(UpdateState::default()),
            busy: Cell::new(false),
            rx: RefCell::new(None),
            pending_replies: RefCell::new(Vec::new()),
            http_exceptions: RefCell::new(HashSet::new()),
        }
    }
}

/// Per-tab network state, shared with the tab's request handler.
#[derive(Default)]
pub(super) struct TabNet {
    /// URL of the top-level navigation in progress or loaded.
    pub top_url: RefCell<String>,
    /// Ad blocking is off for this page (allowlisted site).
    pub allowed: Cell<bool>,
    pub blocked: Cell<u32>,
    /// The page has a login form (key icon in the address bar).
    pub login_form: Cell<bool>,
    /// Site-specific cosmetic CSS and generic-hide data for the page.
    pub css: RefCell<String>,
    pub generichide: Cell<bool>,
    pub exceptions: RefCell<HashSet<String>>,
    /// HTTPS-only: the http:// URL the current navigation was upgraded from.
    pub upgraded_from: RefCell<Option<String>>,
    /// The (https URL, http URL) of an upgrade we just issued.
    pending_upgrade: RefCell<Option<(String, String)>>,
    /// A rewrite navigation we issued and its chain length.
    rewriting: Cell<bool>,
    rewrites: Cell<u8>,
}

impl Browser {
    // ------------------------------------------------------------ startup

    /// Loads the allowlist and the engine (from cache if valid, otherwise
    /// compiled on a worker) and schedules the daily update check.
    pub(super) fn init_filters(&self) {
        let (enabled, dnt, gpc) = {
            let s = self.settings.borrow();
            (s.adblock_enabled, s.send_dnt, s.send_gpc)
        };
        let f = &self.filters;
        f.enabled.set(enabled);
        f.dnt.set(dnt);
        f.gpc.set(gpc);
        *f.allowlist.borrow_mut() = Allowlist::new(self.store.borrow().allowlist().unwrap_or_default());
        if !enabled {
            info!("ad blocking is off");
            return;
        }
        let state = std::fs::read_to_string(self.dirs.filters().join("state.json"))
            .map(|j| UpdateState::from_json(&j))
            .unwrap_or_default();
        let lists = self.settings.borrow().filter_lists.clone();
        let key = filterlists::cache_key(&lists, &state);
        let t = std::time::Instant::now();
        let cached =
            std::fs::read(self.dirs.engine_cache()).ok().and_then(|data| Blocker::deserialize(&data, key).ok());
        let due = state.update_due(now_ms());
        *f.update.borrow_mut() = state;
        match cached {
            Some(engine) => {
                info!("filter engine loaded from cache in {} ms", t.elapsed().as_millis());
                *f.engine.borrow_mut() = Some(engine);
            }
            None => {
                info!("filter engine cache missing or stale; compiling");
                self.spawn_filter_job(false);
            }
        }
        if due {
            if let Some(w) = self.window() {
                w.set_timer(UPDATE_TIMER, FIRST_UPDATE_DELAY_MS);
            }
        }
    }

    /// Starts the worker that (optionally downloads and) rebuilds the engine.
    pub(super) fn spawn_filter_job(&self, download: bool) {
        let f = &self.filters;
        if f.busy.replace(true) {
            return;
        }
        let Some(window) = self.window() else {
            f.busy.set(false);
            return;
        };
        let notifier = window.notifier();
        let dir = self.dirs.filters();
        let cache_path = self.dirs.engine_cache();
        let (lists, proxy) = {
            let s = self.settings.borrow();
            (s.filter_lists.clone(), updater_proxy(&s.proxy))
        };
        let mut state = f.update.borrow().clone();
        let current_key = f.engine.borrow().as_ref().map(Blocker::key);
        let (tx, rx) = mpsc::channel();
        *f.rx.borrow_mut() = Some(rx);
        let skip_reason = (download && proxy == UpdaterProxy::Skip)
            .then_some("Updates are skipped while a SOCKS proxy is configured (it would bypass the proxy)");
        if let Some(reason) = skip_reason {
            state.last_error = Some(reason.into());
        }
        let download = download && skip_reason.is_none();
        let spawned = std::thread::Builder::new().name("filter-lists".into()).spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            let mode = match proxy {
                UpdaterProxy::Named(p) => ProxyMode::Named(p),
                _ => ProxyMode::System,
            };
            let fetch = move |url: &str| http::get(url, MAX_LIST_BYTES, &mode, DOWNLOAD_TIMEOUT_MS);
            let fetch_ref: &dyn Fn(&str) -> Result<Vec<u8>, String> = &fetch;
            let r = filterlists::rebuild(&dir, &lists, state, download.then_some(fetch_ref), current_key, now_ms());
            if let Some(cache) = &r.cache {
                let _ = lb_core::vault::write_atomic(&cache_path, cache);
            }
            let _ = lb_core::vault::write_atomic(&dir.join("state.json"), r.state.to_json().as_bytes());
            let _ = tx.send(r);
            notifier.notify(APP_FILTERS_READY);
        });
        if spawned.is_err() {
            f.busy.set(false);
            warning!("could not start the filter-list worker");
        }
    }

    /// The worker finished: swap in the new engine (UI thread).
    pub(super) fn filters_ready(&self) {
        let f = &self.filters;
        let result = f.rx.borrow().as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(r) = result else { return };
        f.busy.set(false);
        if let Some(engine) = r.engine {
            info!("filter engine rebuilt");
            *f.engine.borrow_mut() = Some(engine);
        }
        if let Some(e) = &r.state.last_error {
            warning!("filter-list update: {e}");
        }
        *f.update.borrow_mut() = r.state;
        let replies: Vec<_> = f.pending_replies.borrow_mut().drain(..).collect();
        let status = self.filter_status();
        for (tab, req_id) in replies {
            self.reply(tab, Event::ok(req_id, &status));
        }
    }

    /// "Update now" (settings page) or the scheduled daily check.
    pub(super) fn update_filter_lists(&self, reply_to: Option<(Option<TabId>, Option<u32>)>) {
        if let Some(r) = reply_to {
            self.filters.pending_replies.borrow_mut().push(r);
        }
        if !self.filters.enabled.get() {
            let replies: Vec<_> = self.filters.pending_replies.borrow_mut().drain(..).collect();
            for (tab, req_id) in replies {
                self.reply(tab, Event::err(req_id, "Ad blocking is turned off"));
            }
            return;
        }
        self.spawn_filter_job(true);
    }

    fn filter_status(&self) -> serde_json::Value {
        let state = self.filters.update.borrow();
        let enabled = self.settings.borrow().filter_lists.clone();
        let lists: Vec<serde_json::Value> = FILTER_LISTS
            .iter()
            .map(|id| {
                let dl = state.downloaded.get(*id);
                serde_json::json!({
                    "id": id,
                    "title": blocker::list_title(id),
                    "enabled": enabled.iter().any(|e| e == id),
                    "source": if dl.is_some() { "downloaded" } else { "embedded" },
                    "fetched": dl.map(|d| d.fetched_ms),
                })
            })
            .collect();
        serde_json::json!({
            "enabled": self.filters.enabled.get(),
            "lastCheck": (state.last_check_ms > 0).then_some(state.last_check_ms),
            "lastError": state.last_error,
            "busy": self.filters.busy.get(),
            "lists": lists,
        })
    }

    // ------------------------------------------------------------ per tab

    /// Request interception, privacy scripts and cosmetic filtering for a
    /// new tab controller.
    pub(super) fn hook_blocking(&self, id: TabId, c: &Controller, net: Rc<TabNet>) {
        let Some(env) = self.env.get() else { return };
        let level = match self.settings.borrow().tracking_prevention {
            TrackingPrevention::None => 0,
            TrackingPrevention::Basic => 1,
            TrackingPrevention::Balanced => 2,
            TrackingPrevention::Strict => 3,
        };
        c.set_tracking_prevention(level);
        if self.filters.gpc.get() {
            c.add_document_script(assets::inject_script("gpc.js"));
        }
        if self.filters.dnt.get() {
            c.add_document_script(assets::inject_script("dnt.js"));
        }
        if self.filters.enabled.get() {
            c.add_document_script(assets::inject_script("page-tokens.js"));
        }
        // Login form detection and fill (top-level frame only).
        c.add_document_script(assets::inject_script("login.js"));
        c.intercept_all(env, self.request_handler(id, net.clone()));

        let weak = self.me.clone();
        let c2 = c.clone();
        c.on_dom_content_loaded(move || {
            let Some(b) = weak.upgrade() else { return };
            if !b.filters.enabled.get() || net.allowed.get() {
                return;
            }
            let css = net.css.borrow().clone();
            if !css.is_empty() {
                c2.execute_script(&assets::style_injection_script(&css));
            }
        });
    }

    /// The per-request decision function for tab `id`.
    fn request_handler(&self, id: TabId, net: Rc<TabNet>) -> impl Fn(&ResourceRequest<'_>) -> ResourceAction + 'static {
        let filters = self.filters.clone();
        let server = self.server();
        let weak = self.me.clone();
        move |req: &ResourceRequest<'_>| {
            if blocker::is_internal(req.uri) {
                return match server(req.uri) {
                    Some(r) => ResourceAction::Serve(r),
                    None => ResourceAction::NotFound,
                };
            }
            let headers = ResourceAction::Continue { dnt: filters.dnt.get(), gpc: filters.gpc.get() };
            if !filters.enabled.get() || net.allowed.get() || !blocker::is_filterable(req.uri) {
                return headers;
            }
            let top = net.top_url.borrow();
            let Some(kind) =
                ResourceKind::from_webview2_context(req.context, blocker::is_top_level_request(req.uri, &top))
            else {
                return headers;
            };
            let blocked =
                filters.engine.borrow().as_ref().is_some_and(|e| e.check(req.uri, &top, kind) == Decision::Block);
            if !blocked {
                return headers;
            }
            net.blocked.set(net.blocked.get().saturating_add(1));
            if let Some(b) = weak.upgrade() {
                b.shield_changed(id);
            }
            ResourceAction::Block
        }
    }

    /// `NavigationStarting` for a tab, after the internal-page policy
    /// allowed it. Returns `Some(url)` if the navigation must be cancelled
    /// and restarted at `url` (tracking parameters, HTTPS upgrade).
    pub(super) fn navigation_starting(&self, net: &TabNet, uri: &str, redirected: bool) -> Option<String> {
        // HTTPS-only: the site sent our upgrade back to http:// - show the
        // interstitial rather than upgrading again in an endless loop.
        let in_flight = net.upgraded_from.borrow().is_some();
        if let Some(page) = urlclean::https_only_downgrade(in_flight, redirected, uri) {
            *net.upgraded_from.borrow_mut() = None;
            net.pending_upgrade.borrow_mut().take();
            return Some(page);
        }
        // Our own rewrite arriving: keep the chain count, else start over.
        if !net.rewriting.replace(false) {
            net.rewrites.set(0);
        }
        let pending = net.pending_upgrade.borrow_mut().take();
        *net.upgraded_from.borrow_mut() = pending.filter(|(https, _)| https == uri).map(|(_, http)| http);

        if !blocker::is_internal(uri) && net.rewrites.get() < MAX_REWRITES {
            let opts = {
                let s = self.settings.borrow();
                RewriteOptions { strip_tracking: s.strip_tracking_params, https_only: s.https_only }
            };
            if let Some(rw) = urlclean::navigation_rewrite(uri, opts, &self.filters.http_exceptions.borrow()) {
                net.rewrites.set(net.rewrites.get() + 1);
                net.rewriting.set(true);
                if rw.upgraded {
                    let http = urlclean::navigation_rewrite(
                        uri,
                        RewriteOptions { strip_tracking: opts.strip_tracking, https_only: false },
                        &HashSet::new(),
                    )
                    .map(|r| r.url)
                    .unwrap_or_else(|| uri.to_string());
                    *net.pending_upgrade.borrow_mut() = Some((rw.url.clone(), http));
                }
                return Some(rw.url);
            }
        }

        // A real navigation: reset the shield and prepare cosmetic filters.
        *net.top_url.borrow_mut() = uri.to_string();
        net.blocked.set(0);
        net.login_form.set(false);
        let host = urlclean::host_of(uri).unwrap_or_default();
        let allowed = !host.is_empty() && self.filters.allowlist.borrow().contains(&host);
        net.allowed.set(allowed);
        let (css, generichide, exceptions) = match self.filters.engine.borrow().as_ref() {
            Some(e) if self.filters.enabled.get() && !allowed && blocker::is_filterable(uri) => {
                let c = e.cosmetic(uri);
                (c.css, c.generichide, c.exceptions)
            }
            _ => (String::new(), true, HashSet::new()),
        };
        *net.css.borrow_mut() = css;
        net.generichide.set(generichide);
        *net.exceptions.borrow_mut() = exceptions;
        None
    }

    /// `NavigationCompleted`: if an HTTPS upgrade failed, show the interstitial.
    pub(super) fn navigation_completed(&self, id: TabId, net: &TabNet, ok: bool, status: i32) {
        let upgraded = net.upgraded_from.borrow().clone();
        if ok {
            // The upgrade is done; later http:// links upgrade afresh.
            *net.upgraded_from.borrow_mut() = None;
        }
        if let Some(page) = urlclean::https_only_fallback(upgraded.as_deref(), ok, status) {
            *net.upgraded_from.borrow_mut() = None;
            self.defer(move |b| b.navigate_tab(id, &page));
        }
    }

    /// Generic cosmetic filtering: the page reported its classes and ids.
    pub(super) fn page_tokens(&self, id: TabId, origin: &str, msg: ContentMessage) {
        let ContentMessage::PageTokens { classes, ids } = msg else { return };
        let Some((c, net)) = self.views.borrow().get(&id).map(|v| (v.controller.clone(), v.net.clone())) else {
            return;
        };
        // Only for the page currently loaded in this tab.
        let current = urlclean::origin_of(&net.top_url.borrow());
        if current.as_deref() != Some(origin)
            || !self.filters.enabled.get()
            || net.allowed.get()
            || net.generichide.get()
        {
            return;
        }
        let selectors = match self.filters.engine.borrow().as_ref() {
            Some(e) => e.generic_selectors(&classes, &ids, &net.exceptions.borrow()),
            None => return,
        };
        if !selectors.is_empty() {
            c.execute_script(&assets::style_injection_script(&blocker::hide_css(selectors.iter())));
        }
    }

    // ------------------------------------------------------------ shield

    /// A request was blocked in tab `id`: refresh the shield soon.
    fn shield_changed(&self, id: TabId) {
        if self.model.try_borrow().ok().and_then(|m| m.active_id()) != Some(id) {
            return;
        }
        if !self.shield_pending.replace(true) {
            if let Some(w) = self.window() {
                w.set_timer(SHIELD_TIMER, SHIELD_DEBOUNCE_MS);
            }
        }
    }

    pub(super) fn shield_timer(&self) {
        if let Some(w) = self.window() {
            w.kill_timer(SHIELD_TIMER);
        }
        self.shield_pending.set(false);
        self.send_nav();
        let shield_open =
            self.popup.borrow().as_ref().is_some_and(|p| matches!(p.content, Some(PopupContent::Shield { .. })));
        if shield_open {
            self.show_popup(self.shield_content(), None);
        }
    }

    /// `(blocked, site allowed)` for the active tab.
    pub(super) fn shield_state(&self) -> (u32, bool) {
        let Some(id) = self.model.try_borrow().ok().and_then(|m| m.active_id()) else { return (0, false) };
        self.views
            .try_borrow()
            .ok()
            .and_then(|v| v.get(&id).map(|v| (v.net.blocked.get(), v.net.allowed.get())))
            .unwrap_or((0, false))
    }

    pub(super) fn shield_content(&self) -> PopupContent {
        let host = self.model.borrow().active().and_then(|t| urlclean::host_of(&t.url)).unwrap_or_default();
        let (blocked, allowed) = self.shield_state();
        PopupContent::Shield { host, blocked, allowed, enabled: self.filters.enabled.get() }
    }

    /// Shield toggle: allow (or block again) ads on the active tab's site,
    /// then reload it.
    pub(super) fn shield_allow_site(&self, allow: bool) {
        let Some(host) = self.model.borrow().active().and_then(|t| urlclean::host_of(&t.url)) else { return };
        if allow {
            match self.store.borrow().allowlist_add(&host, now_ms()) {
                Ok(h) => {
                    self.filters.allowlist.borrow_mut().insert(&h);
                }
                Err(e) => warning!("allowlist: {e}"),
            }
        } else {
            // Remove whichever entry covers this host (it may be a parent domain).
            let entry = self.filters.allowlist.borrow().matching(&host).map(str::to_string);
            if let Some(entry) = entry {
                let _ = self.store.borrow().allowlist_remove(&entry);
                self.filters.allowlist.borrow_mut().remove(&entry);
            }
        }
        self.hide_popup();
        self.run_command(lb_core::keys::Command::Reload);
    }

    /// Settings-page allowlist and filter commands.
    pub(super) fn dispatch_blocking(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::GetAllowlist => self.reply_db(tab, req_id, self.store.borrow().allowlist()),
            Request::AllowlistAdd { host } => {
                let r = self.store.borrow().allowlist_add(&host, now_ms());
                if let Ok(h) = &r {
                    self.filters.allowlist.borrow_mut().insert(h);
                }
                self.reply_db(tab, req_id, r);
            }
            Request::AllowlistRemove { host } => {
                let r = self.store.borrow().allowlist_remove(&host);
                self.filters.allowlist.borrow_mut().remove(&host);
                self.reply_db(tab, req_id, r);
            }
            Request::UpdateFilterLists => self.update_filter_lists(Some((tab, req_id))),
            Request::GetFilterStatus => self.reply(tab, Event::ok(req_id, self.filter_status())),
            Request::HttpsOnlyProceed { url } => self.https_only_proceed(tab, &url),
            _ => {}
        }
    }

    /// "Continue to HTTP site" on the interstitial: allow this host over
    /// HTTP for the rest of the session and load the page once.
    fn https_only_proceed(&self, tab: Option<TabId>, url: &str) {
        let Some(tab) = tab else { return };
        let valid = url.starts_with("http://") && urlclean::host_of(url).is_some();
        let on_interstitial =
            self.model.borrow().get(tab).is_some_and(|t| Page::from_url(&t.url) == Some(Page::HttpsOnly));
        if !valid || !on_interstitial {
            return;
        }
        if let Some(host) = urlclean::host_of(url) {
            self.filters.http_exceptions.borrow_mut().insert(host);
        }
        self.navigate_tab(tab, url);
    }

    /// Navigates a specific tab as the host.
    pub(super) fn navigate_tab(&self, id: TabId, url: &str) {
        let views = self.views.borrow();
        if let Some(v) = views.get(&id) {
            v.host_navigation.set(true);
            let _ = v.controller.navigate(url);
        }
    }
}

/// Deferred work (run after the current WebView2 event handler returns).
pub(super) type Deferred = Box<dyn FnOnce(&Browser)>;

impl Browser {
    pub(super) fn defer(&self, f: impl FnOnce(&Browser) + 'static) {
        self.deferred.borrow_mut().push(Box::new(f));
        if let Some(w) = self.window() {
            w.notifier().notify(APP_DEFERRED);
        }
    }

    pub(super) fn run_deferred(&self) {
        let tasks: Vec<Deferred> = self.deferred.borrow_mut().drain(..).collect();
        for t in tasks {
            t(self);
        }
    }
}
