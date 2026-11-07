//! Ad and tracker blocking, privacy headers, tracking-parameter stripping,
//! HTTPS-only mode and the filter-list updater.
//!
//! Hot path: [`TabPolicy::check`] runs on the engine's IO thread for every
//! request of a tab. It touches only in-memory state shared through
//! [`NetShared`] (engine, flags precomputed per navigation) and never the
//! database.

use super::Browser;
use crate::ffi::cef::{NetRequest, NetVerdict, RequestPolicy, View};
use crate::ffi::http::{self, ProxyMode};
use crate::ffi::window::AppNotifier;
use crate::log::{info, warning};
use lb_core::assets;
use lb_core::blocker::{self, Allowlist, Blocker, Decision, ResourceKind, ScriptletEngine};
use lb_core::cookies;
use lb_core::filterlists::{self, Rebuilt, SourceSet, UpdateState, MAX_LIST_BYTES};
use lb_core::headers::Headers;
use lb_core::pages::Page;
use lb_core::protection::{self, Protection, Referrer, ThirdPartyCookies, TrackerSet};
use lb_core::protocol::{ContentMessage, Event, PopupContent, Request};
use lb_core::settings::{updater_proxy, UpdaterProxy};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::urlclean::{self, LoadFailure, RewriteOptions};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex, OnceLock, RwLock};

/// `WM_APP` code: the filter worker finished.
pub(super) const APP_FILTERS_READY: usize = 1;
/// `WM_APP` code: run deferred tasks.
pub(super) const APP_DEFERRED: usize = 2;
/// `WM_APP` code: a request was blocked (posted from the IO thread).
pub(super) const APP_SHIELD: usize = 4;
/// Timer: push the shield counter (at most ~4 times a second).
pub(super) const SHIELD_TIMER: usize = 2;
const SHIELD_DEBOUNCE_MS: u32 = 250;
/// Timer: filter-list update check, a minute after start and then hourly
/// (a download happens only when the last check is a day old).
pub(super) const UPDATE_TIMER: usize = 3;
const FIRST_UPDATE_DELAY_MS: u32 = 60_000;
pub(super) const UPDATE_CHECK_INTERVAL_MS: u32 = 60 * 60 * 1000;
/// Rewrites of one navigation chain before giving up (defence in depth;
/// `navigation_rewrite` already reaches a fixed point after one).
const MAX_REWRITES: u8 = 3;
const DOWNLOAD_TIMEOUT_MS: i32 = 30_000;

/// `--log-blocked` (diagnostics): log every blocked request (without its
/// query string) to find what breaks a site.
pub(crate) static LOG_BLOCKED: AtomicBool = AtomicBool::new(false);

/// What a filter rebuild downloads first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Fetching {
    Nothing,
    /// Lists the user added that have no download yet.
    Added,
    /// Every list that is on (the daily update, "Update now").
    All,
}

/// What a reply owed after a rebuild carries.
pub(super) enum Owed {
    /// The settings page's filter status.
    Status,
    /// `{}`, or an error if the list `id` (just added) could not be downloaded.
    Added(String),
    /// The check of the user's filters.
    Report(filterlists::UserFiltersReport),
}

/// A reply owed to a page's request: (its tab, its request id, what it carries).
pub(super) type OwedReply = (Option<TabId>, Option<u32>, Owed);

/// Blocking state of the UI thread (lists, allowlist, updater).
pub(super) struct FilterState {
    pub allowlist: RefCell<Allowlist>,
    pub update: RefCell<UpdateState>,
    pub(super) busy: Cell<bool>,
    rx: RefCell<Option<mpsc::Receiver<Rebuilt>>>,
    /// Replies owed once the engine is rebuilt ("Update now", changes on krunk://adblock).
    pub(super) pending_replies: RefCell<Vec<OwedReply>>,
    /// A rebuild asked for while one runs: started when it finishes.
    queued: Cell<Option<Fetching>>,
    /// "My filters" (kept in the store under `USER_FILTERS_KEY`).
    pub user: RefCell<String>,
    /// Extensions' translated declarativeNetRequest rules (0.6.0).
    pub ext: RefCell<String>,
    /// Hosts the user allowed over plain HTTP this session (HTTPS-only).
    pub http_exceptions: RefCell<HashSet<String>>,
}

impl FilterState {
    pub fn new() -> FilterState {
        FilterState {
            allowlist: RefCell::new(Allowlist::default()),
            update: RefCell::new(UpdateState::default()),
            busy: Cell::new(false),
            rx: RefCell::new(None),
            pending_replies: RefCell::new(Vec::new()),
            queued: Cell::new(None),
            user: RefCell::new(String::new()),
            ext: RefCell::new(String::new()),
            http_exceptions: RefCell::new(HashSet::new()),
        }
    }
}

/// Blocking state read by the engine's IO thread for every request.
#[derive(Default)]
pub(super) struct NetShared {
    engine: RwLock<Option<Arc<Blocker>>>,
    enabled: AtomicBool,
    dnt: AtomicBool,
    gpc: AtomicBool,
    /// Tracking protection (0.6.0): the cookie policy (`ThirdPartyCookies` as u8), the
    /// referrer policy (`Referrer` as u8) and the known-tracker hosts of the privacy lists.
    cookie_policy: AtomicU8,
    referrer_policy: AtomicU8,
    trackers: RwLock<Arc<TrackerSet>>,
    /// Top-level redirects seen by the IO thread (from, to), drained by the bounce timer.
    hops: Mutex<Vec<(String, String)>>,
    /// Custom request headers (0.6.0, Settings → Advanced → Request headers).
    headers: RwLock<Arc<Headers>>,
    has_header_rules: AtomicBool,
    notifier: OnceLock<AppNotifier>,
    /// An APP_SHIELD message is on its way to the UI thread.
    shield_posted: AtomicBool,
}

impl NetShared {
    pub fn engine(&self) -> Option<Arc<Blocker>> {
        self.engine.read().ok()?.clone()
    }

    pub fn set_engine(&self, engine: Blocker) {
        if let Ok(mut e) = self.engine.write() {
            *e = Some(Arc::new(engine));
        }
    }

    pub fn has_engine(&self) -> bool {
        self.engine.read().is_ok_and(|e| e.is_some())
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Relaxed);
    }

    pub fn set_privacy_headers(&self, dnt: bool, gpc: bool) {
        self.dnt.store(dnt, Relaxed);
        self.gpc.store(gpc, Relaxed);
    }

    /// Applies the cookie and referrer policies of a protection level (the lists go
    /// through the engine rebuild).
    pub fn set_protection(&self, p: &Protection) {
        let e = p.effective();
        self.cookie_policy.store(cookie_policy_code(e.third_party_cookies), Relaxed);
        self.referrer_policy.store(referrer_code(e.referrer), Relaxed);
    }

    pub fn set_headers(&self, h: &Headers) {
        self.has_header_rules.store(!h.rules.is_empty() || !h.send_client_hints, Relaxed);
        if let Ok(mut w) = self.headers.write() {
            *w = Arc::new(h.clone());
        }
    }

    /// The custom header rules for a page at `top` (cheap when there are none).
    fn header_rules(&self, top: &str) -> Vec<(String, Option<String>)> {
        if !self.has_header_rules.load(Relaxed) {
            return Vec::new();
        }
        let host = urlclean::host_of(top).unwrap_or_default();
        self.headers
            .read()
            .ok()
            .map(|h| {
                let mut out = h.hint_removals();
                out.extend(h.rules_for(&host));
                out
            })
            .unwrap_or_default()
    }

    pub fn set_trackers(&self, t: TrackerSet) {
        if let Ok(mut w) = self.trackers.write() {
            *w = Arc::new(t);
        }
    }

    fn trackers(&self) -> Option<Arc<TrackerSet>> {
        self.trackers.read().ok().map(|t| t.clone())
    }

    /// The redirect hops recorded since the last call.
    pub fn take_hops(&self) -> Vec<(String, String)> {
        self.hops.lock().map(|mut h| std::mem::take(&mut *h)).unwrap_or_default()
    }

    pub fn set_notifier(&self, n: AppNotifier) {
        let _ = self.notifier.set(n);
    }

    /// IO thread: wake the UI thread to refresh the shield (once per batch).
    fn blocked_one(&self) {
        if !self.shield_posted.swap(true, Relaxed) {
            if let Some(n) = self.notifier.get() {
                n.notify(APP_SHIELD);
            }
        }
    }
}

fn cookie_policy_code(p: ThirdPartyCookies) -> u8 {
    match p {
        ThirdPartyCookies::Off => 0,
        ThirdPartyCookies::Trackers => 1,
        ThirdPartyCookies::All => 2,
    }
}

fn referrer_code(r: Referrer) -> u8 {
    match r {
        Referrer::Full => 0,
        Referrer::Origin => 1,
        Referrer::None => 2,
    }
}

fn referrer_from_code(c: u8) -> Referrer {
    match c {
        1 => Referrer::Origin,
        2 => Referrer::None,
        _ => Referrer::Full,
    }
}

/// Most redirect hops kept between two bounce-timer ticks.
const MAX_HOPS: usize = 256;

/// Blocked addresses the shield panel lists (the rest are on krunk://adblock).
const SHIELD_RECENT: usize = 6;

/// The part of a tab's network state the IO thread reads.
#[derive(Default)]
struct TabShared {
    /// URL of the top-level navigation in progress or loaded.
    top_url: Mutex<String>,
    /// The last top-level requests and the `Referer` the engine sent with each (a download
    /// fetched in parts sends the same one).
    navigations: Mutex<std::collections::VecDeque<(String, String)>>,
    /// What was blocked on the page (see `lb_core::blocklog`; memory only).
    blocked_log: Mutex<std::collections::VecDeque<lb_core::blocklog::Entry>>,
    /// Ad blocking is off for this page (allowlisted site).
    allowed: AtomicBool,
    blocked: AtomicU32,
    /// Bytes received by this tab's requests (the task manager's network rate).
    received: AtomicU64,
}

/// The network policy of one tab (runs on the IO thread).
struct TabPolicy {
    net: Arc<NetShared>,
    tab: Arc<TabShared>,
}

impl RequestPolicy for TabPolicy {
    fn check(&self, req: &NetRequest<'_>) -> NetVerdict {
        if req.resource_type == 0 {
            if let Ok(mut n) = self.tab.navigations.lock() {
                if n.len() >= 8 {
                    n.pop_front();
                }
                n.push_back((req.url.to_string(), req.referrer.to_string()));
            }
        }
        if blocker::is_internal(req.url) {
            // Only built-in pages may load from the internal origins.
            let top_level = req.resource_type == 0;
            return if assets::internal_request_allowed(req.url, req.initiator, top_level) {
                NetVerdict::Allow { dnt: false, gpc: false, referrer: None, headers: Vec::new() }
            } else {
                NetVerdict::Block
            };
        }
        // Tracking protection: a cross-site request says at most the origin it comes from
        // (nothing in Strict); off for sites the shield exempts.
        let referrer = if self.tab.allowed.load(Relaxed) {
            None
        } else {
            protection::trim_referrer(req.referrer, req.url, referrer_from_code(self.net.referrer_policy.load(Relaxed)))
        };
        let top_for_rules = self.tab.top_url.lock().map(|t| t.clone()).unwrap_or_default();
        let headers = NetVerdict::Allow {
            dnt: self.net.dnt.load(Relaxed),
            gpc: self.net.gpc.load(Relaxed),
            referrer,
            headers: self.net.header_rules(&top_for_rules),
        };
        if !self.net.enabled() || self.tab.allowed.load(Relaxed) || !blocker::is_filterable(req.url) {
            return headers;
        }
        let Some(kind) = ResourceKind::from_cef_resource_type(req.resource_type, req.url) else { return headers };
        let Some(engine) = self.net.engine() else { return headers };
        let top = self.tab.top_url.lock().map(|t| t.clone()).unwrap_or_default();
        let verdict = match engine.check(req.url, &top, kind) {
            Decision::Allow => return headers,
            Decision::Block => NetVerdict::Block,
            Decision::Redirect { content_type, body } => NetVerdict::Respond { content_type, body },
        };
        if LOG_BLOCKED.load(Relaxed) {
            let url = req.url.split(['?', '#']).next().unwrap_or("");
            let how = if matches!(verdict, NetVerdict::Block) { "blocked" } else { "redirected" };
            crate::log::info!("{how} {kind:?} {url} (page {})", urlclean::host_of(&top).unwrap_or_default());
        }
        if let Ok(mut log) = self.tab.blocked_log.lock() {
            if log.len() >= lb_core::blocklog::MAX_ENTRIES {
                log.pop_front();
            }
            log.push_back(lb_core::blocklog::Entry {
                url: req.url.to_string(),
                page: top.clone(),
                kind,
                redirected: matches!(verdict, NetVerdict::Respond { .. }),
            });
        }
        self.tab.blocked.fetch_add(1, Relaxed);
        self.net.blocked_one();
        verdict
    }

    fn loaded(&self, bytes: u64) {
        self.tab.received.fetch_add(bytes, Relaxed);
    }

    fn cookie_allowed(&self, url: &str) -> bool {
        let policy = self.net.cookie_policy.load(Relaxed);
        if policy == 0 || self.tab.allowed.load(Relaxed) {
            return true;
        }
        let top = self.tab.top_url.lock().map(|t| t.clone()).unwrap_or_default();
        if !cookies::is_third_party(&top, url) {
            return true;
        }
        if policy == 2 {
            return false;
        }
        // Trackers only: a third party the privacy lists name.
        let Some(host) = urlclean::host_of(url) else { return true };
        !self.net.trackers().is_some_and(|t| t.is_tracker(&host))
    }

    fn redirected(&self, resource_type: i32, from_url: &str, new_url: &str) {
        if resource_type != 0 {
            return;
        }
        if let Ok(mut h) = self.net.hops.lock() {
            if h.len() < MAX_HOPS {
                h.push((from_url.to_string(), new_url.to_string()));
            }
        }
    }
}

/// Per-tab network state (UI thread), with the part shared with the IO thread.
pub(super) struct TabNet {
    io: Arc<TabShared>,
    net: Arc<NetShared>,
    /// The page has a login form (key icon in the address bar).
    pub login_form: Cell<bool>,
    /// The page's videos as the media script reports them (Picture-in-Picture, 0.6.0).
    pub videos: RefCell<Vec<lb_core::protocol::VideoInfo>>,
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
    /// A top-level navigation started and has not been recorded in history
    /// (a page can stop and start loading several times).
    pub visit_pending: Cell<bool>,
    /// The blocked count last written to the log.
    logged_blocked: Cell<u32>,
}

impl TabNet {
    pub fn new(net: Arc<NetShared>) -> TabNet {
        TabNet {
            io: Arc::new(TabShared::default()),
            net,
            login_form: Cell::new(false),
            videos: RefCell::new(Vec::new()),
            css: RefCell::new(String::new()),
            generichide: Cell::new(false),
            exceptions: RefCell::new(HashSet::new()),
            upgraded_from: RefCell::new(None),
            pending_upgrade: RefCell::new(None),
            rewriting: Cell::new(false),
            rewrites: Cell::new(0),
            visit_pending: Cell::new(false),
            logged_blocked: Cell::new(0),
        }
    }

    /// The request policy for this tab's view.
    pub fn policy(&self) -> Arc<dyn RequestPolicy> {
        Arc::new(TabPolicy { net: self.net.clone(), tab: self.io.clone() })
    }

    pub fn blocked(&self) -> u32 {
        self.io.blocked.load(Relaxed)
    }

    /// Bytes this tab's requests received so far.
    pub fn received(&self) -> u64 {
        self.io.received.load(Relaxed)
    }

    /// What was blocked on the page so far (oldest first).
    pub fn blocked_entries(&self) -> Vec<lb_core::blocklog::Entry> {
        self.io.blocked_log.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }

    /// The `Referer` the engine sent with its last top-level request for `url`: `Some("")`
    /// for none, `None` if the request was not seen.
    pub fn engine_referrer(&self, url: &str) -> Option<String> {
        let n = self.io.navigations.lock().ok()?;
        n.iter().rev().find(|(u, _)| u == url).map(|(_, r)| r.clone())
    }

    pub fn allowed(&self) -> bool {
        self.io.allowed.load(Relaxed)
    }

    pub fn top_url(&self) -> String {
        self.io.top_url.lock().map(|t| t.clone()).unwrap_or_default()
    }
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
        self.net.set_enabled(enabled);
        self.net.set_privacy_headers(dnt, gpc);
        self.net.set_protection(&self.settings.borrow().protection);
        self.net.set_headers(&self.settings.borrow().headers);
        self.load_trackers();
        *f.allowlist.borrow_mut() = Allowlist::new(self.store.borrow().allowlist().unwrap_or_default());
        *f.user.borrow_mut() =
            self.store.borrow().kv_get(filterlists::USER_FILTERS_KEY).ok().flatten().unwrap_or_default();
        if !enabled {
            info!("ad blocking is off");
            return;
        }
        let state = std::fs::read_to_string(self.dirs.filters().join("state.json"))
            .map(|j| UpdateState::from_json(&j))
            .unwrap_or_default();
        let key = filterlists::cache_key_for(&self.filter_sources().sources(), &state);
        let t = std::time::Instant::now();
        let cached =
            std::fs::read(self.dirs.engine_cache()).ok().and_then(|data| Blocker::deserialize(&data, key).ok());
        info!("filter lists: {}", state.schedule_note(now_ms()));
        *f.update.borrow_mut() = state;
        let scriptlets = std::fs::read(self.dirs.scriptlet_cache())
            .ok()
            .filter(|b| ScriptletEngine::stored_key(b) == Some(blocker::scriptlet_key(key)));
        match (cached, scriptlets) {
            (Some(engine), Some(scriptlets)) => {
                info!(
                    "filter engine loaded from cache in {} ms; scriptlet engine {} KB",
                    t.elapsed().as_millis(),
                    scriptlets.len() / 1024
                );
                self.net.set_engine(engine);
                self.set_scriptlet_engine(Some(scriptlets));
            }
            (cached, _) => {
                info!("filter engine cache missing or stale; compiling");
                if let Some(engine) = cached {
                    // Block with it meanwhile; the scriptlets come with the rebuild.
                    self.net.set_engine(engine);
                }
                self.spawn_filter_job_forced(Fetching::Added, true);
            }
        }
        if let Some(w) = self.app() {
            w.set_timer(UPDATE_TIMER, FIRST_UPDATE_DELAY_MS);
        }
    }

    /// UPDATE_TIMER: download the lists if the last check is a day old.
    /// Runs hourly, so a browser left open for days still updates.
    pub(super) fn scheduled_filter_check(&self) {
        let now = now_ms();
        let (due, note) = {
            let u = self.filters.update.borrow();
            (u.update_due(now), u.schedule_note(now))
        };
        if due && self.net.enabled() && !self.filters.busy.get() {
            info!("filter lists: {note}; updating");
            self.update_filter_lists(None);
        }
    }

    /// Everything the engine is built from, as the settings and "My filters" say now.
    /// The known-tracker hosts for the cookie policy, from the privacy lists' current
    /// texts (a download or the bundled copy).
    pub(super) fn load_trackers(&self) {
        let ids: Vec<String> = protection::TRACKER_LISTS.iter().map(|s| s.to_string()).collect();
        let texts = filterlists::current_texts(
            &self.dirs.filters(),
            &filterlists::Sources::builtin(&ids),
            &self.filters.update.borrow(),
        );
        let t = TrackerSet::from_lists(texts.iter().map(|(_, t)| t.as_str()));
        info!("tracking protection: {} known tracker hosts", t.len());
        self.net.set_trackers(t);
    }

    pub(super) fn filter_sources(&self) -> SourceSet {
        let s = self.settings.borrow();
        SourceSet {
            builtin: s.effective_filter_lists(),
            custom: s.custom_lists.clone(),
            user: self.filters.user.borrow().clone(),
            ext: self.filters.ext.borrow().clone(),
        }
    }

    /// The extensions' rules changed: rebuild the engine when they differ (0.6.0).
    pub(super) fn set_ext_filters(&self, text: String) {
        if *self.filters.ext.borrow() == text {
            return;
        }
        let rules = filterlists::rule_count(&text);
        *self.filters.ext.borrow_mut() = text;
        if self.settings.borrow().adblock_enabled {
            info!("extensions: {rules} network rule(s) in the blocker");
            self.spawn_filter_job(Fetching::Nothing);
        }
    }

    /// Starts the worker that (optionally downloads and) rebuilds the engine.
    pub(super) fn spawn_filter_job(&self, fetching: Fetching) {
        self.spawn_filter_job_forced(fetching, false);
    }

    /// Like [`Browser::spawn_filter_job`]; `force` compiles even if the
    /// lists did not change (the scriptlet engine cache is missing). While a rebuild runs,
    /// another one is queued (it reads the sources when it starts).
    fn spawn_filter_job_forced(&self, fetching: Fetching, force: bool) {
        let f = &self.filters;
        if f.busy.replace(true) {
            f.queued.set(Some(f.queued.get().map_or(fetching, |q| q.max(fetching))));
            return;
        }
        let Some(window) = self.app() else {
            f.busy.set(false);
            return;
        };
        let notifier = window.notifier();
        let dir = self.dirs.filters();
        let cache_path = self.dirs.engine_cache();
        let scriptlet_path = self.dirs.scriptlet_cache();
        let sources = self.filter_sources();
        let proxy = updater_proxy(&self.settings.borrow().proxy);
        let mut state = f.update.borrow().clone();
        let current_key = if force { None } else { self.net.engine().map(|e| e.key()) };
        let (tx, rx) = mpsc::channel();
        *f.rx.borrow_mut() = Some(rx);
        let skip_reason = (fetching != Fetching::Nothing && proxy == UpdaterProxy::Skip)
            .then_some("Updates are skipped while a SOCKS proxy is configured (it would bypass the proxy)");
        if let Some(reason) = skip_reason {
            state.last_error = Some(reason.into());
        }
        let download = fetching != Fetching::Nothing && skip_reason.is_none();
        let only_added = fetching == Fetching::Added;
        let spawned = std::thread::Builder::new().name("filter-lists".into()).spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            let mode = match proxy {
                UpdaterProxy::Named(p) => ProxyMode::Named(p),
                _ => ProxyMode::System,
            };
            let fetch = move |url: &str| http::get(url, MAX_LIST_BYTES, &mode, DOWNLOAD_TIMEOUT_MS);
            let fetch_ref: &dyn Fn(&str) -> Result<Vec<u8>, String> = &fetch;
            let r = filterlists::rebuild_sources(
                &dir,
                &sources.sources(),
                state,
                download.then_some(fetch_ref),
                only_added,
                current_key,
                now_ms(),
            );
            if let Some(cache) = &r.cache {
                let _ = lb_core::vault::write_atomic(&cache_path, cache);
            }
            if let Some(scriptlets) = &r.scriptlets {
                let _ = lb_core::vault::write_atomic(&scriptlet_path, scriptlets);
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
            info!("filter engine rebuilt; scriptlet engine {} KB", r.scriptlets.as_ref().map_or(0, |s| s.len() / 1024));
            self.net.set_engine(engine);
            self.set_scriptlet_engine(r.scriptlets);
        }
        let checked = r.state.last_check_ms != f.update.borrow().last_check_ms;
        if let Some(e) = &r.state.last_error {
            warning!("filter-list update: {e}");
        } else if checked {
            info!("filter lists checked; {}", r.state.schedule_note(now_ms()));
        }
        *f.update.borrow_mut() = r.state;
        if checked {
            self.load_trackers();
        }
        if let Some(next) = f.queued.take() {
            // The sources changed meanwhile: the replies wait for the engine that has them.
            self.spawn_filter_job(next);
            return;
        }
        self.answer_filter_replies();
    }

    /// Answers the replies owed after a rebuild (also when no rebuild could start).
    pub(super) fn answer_filter_replies(&self) {
        let replies: Vec<_> = self.filters.pending_replies.borrow_mut().drain(..).collect();
        let mut failed = Vec::new();
        for (tab, req_id, owed) in replies {
            let event = match owed {
                Owed::Status => Event::ok(req_id, self.filter_status()),
                Owed::Report(r) => Event::ok(req_id, r),
                Owed::Added(id) => {
                    let state = self.filters.update.borrow();
                    if state.downloaded.contains_key(&id) {
                        Event::ok(req_id, serde_json::json!({}))
                    } else {
                        let why = state.errors.get(&id).cloned().or_else(|| state.last_error.clone());
                        failed.push(id);
                        Event::err(
                            req_id,
                            format!("Could not add the list: {}", why.as_deref().unwrap_or("no download")),
                        )
                    }
                }
            };
            self.reply(tab, event);
        }
        // A list that could not be downloaded is not kept.
        if !failed.is_empty() {
            self.change_settings(|s| s.custom_lists.retain(|l| !failed.contains(&filterlists::custom_id(&l.url))));
        }
    }

    /// Hands the scriptlet engine (or, with `None`, just the allowlist and
    /// the on/off state) to the renderers.
    pub(super) fn set_scriptlet_engine(&self, bytes: Option<Vec<u8>>) {
        let bytes = bytes.and_then(|b| ScriptletEngine::stored_key(&b).map(|k| (k, b)));
        let allow = self.filters.allowlist.borrow().hosts();
        crate::ffi::cef::set_scriptlets(bytes, allow, self.net.enabled());
    }

    /// "Update now" (settings page) or the scheduled daily check.
    pub(super) fn update_filter_lists(&self, reply_to: Option<(Option<TabId>, Option<u32>)>) {
        if !self.net.enabled() {
            if let Some((tab, req_id)) = reply_to {
                self.reply(tab, Event::err(req_id, "Ad blocking is turned off"));
            }
            return;
        }
        if let Some((tab, req_id)) = reply_to {
            self.filters.pending_replies.borrow_mut().push((tab, req_id, Owed::Status));
        }
        self.spawn_filter_job(Fetching::All);
    }

    fn filter_status(&self) -> serde_json::Value {
        self.filter_status_with(&Default::default())
    }

    /// The lists (with their rule counts from `rules`, by id), the last update and "My
    /// filters" (settings page and krunk://adblock).
    pub(super) fn filter_status_with(&self, rules: &std::collections::BTreeMap<String, usize>) -> serde_json::Value {
        let state = self.filters.update.borrow();
        let set = self.filter_sources();
        serde_json::json!({
            "enabled": self.net.enabled(),
            "lastCheck": (state.last_check_ms > 0).then_some(state.last_check_ms),
            "lastError": state.last_error,
            "busy": self.filters.busy.get(),
            "lists": filterlists::list_infos(&set, &state, rules),
            "userRules": filterlists::rule_count(&set.user),
        })
    }

    // ------------------------------------------------------------ per tab

    /// Cosmetic filtering for a new tab view (request blocking and the
    /// privacy scripts are part of the view's options).
    pub(super) fn hook_blocking(&self, c: &View, net: Rc<TabNet>) {
        let weak = self.me.clone();
        let c2 = c.clone();
        c.on_load_start(move || {
            let Some(b) = weak.upgrade() else { return };
            b.apply_default_zoom(&c2);
            if !b.net.enabled() || net.allowed() {
                return;
            }
            let css = net.css.borrow().clone();
            if !css.is_empty() {
                c2.execute_script(&assets::style_injection_script(&css));
            }
        });
    }

    /// A top-level navigation is starting in a tab, after the internal-page policy
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
        if let Ok(mut top) = net.io.top_url.lock() {
            *top = uri.to_string();
        }
        net.io.blocked.store(0, Relaxed);
        if let Ok(mut log) = net.io.blocked_log.lock() {
            log.clear();
        }
        net.logged_blocked.set(0);
        net.visit_pending.set(true);
        net.login_form.set(false);
        net.videos.borrow_mut().clear();
        let host = urlclean::host_of(uri).unwrap_or_default();
        let allowed = !host.is_empty() && self.filters.allowlist.borrow().contains(&host);
        net.io.allowed.store(allowed, Relaxed);
        let (css, generichide, exceptions) = match self.net.engine() {
            Some(e) if self.net.enabled() && !allowed && blocker::is_filterable(uri) => {
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

    /// Loading stopped (`failure` is `None` on success): if an HTTPS upgrade
    /// failed, show the interstitial.
    pub(super) fn navigation_completed(&self, id: TabId, net: &TabNet, failure: Option<LoadFailure>) {
        let blocked = net.blocked();
        if blocked > net.logged_blocked.replace(blocked) {
            info!("tab {id}: {blocked} requests blocked on this page");
        }
        let upgraded = net.upgraded_from.borrow().clone();
        if failure.is_none() {
            // The upgrade is done; later http:// links upgrade afresh.
            *net.upgraded_from.borrow_mut() = None;
        }
        if let Some(page) = urlclean::https_only_fallback(upgraded.as_deref(), failure) {
            *net.upgraded_from.borrow_mut() = None;
            self.defer(move |b| b.navigate_tab(id, &page));
        }
    }

    /// Generic cosmetic filtering: the page reported its classes and ids.
    pub(super) fn page_tokens(&self, id: TabId, origin: &str, msg: ContentMessage) {
        let ContentMessage::PageTokens { classes, ids } = msg else { return };
        let Some((c, net)) = self.views.borrow().get(&id).map(|v| (v.view.clone(), v.net.clone())) else {
            return;
        };
        // Only for the page currently loaded in this tab.
        let current = urlclean::origin_of(&net.top_url());
        if current.as_deref() != Some(origin) || !self.net.enabled() || net.allowed() || net.generichide.get() {
            return;
        }
        let selectors = match self.net.engine() {
            Some(e) => e.generic_selectors(&classes, &ids, &net.exceptions.borrow()),
            None => return,
        };
        if !selectors.is_empty() {
            c.execute_script(&assets::style_injection_script(&blocker::hide_css(selectors.iter())));
        }
    }

    // ------------------------------------------------------------ shield

    /// Requests were blocked (APP_SHIELD from the IO thread): refresh the
    /// shield soon.
    pub(super) fn shield_changed(&self) {
        self.net.shield_posted.store(false, Relaxed);
        if !self.shield_pending.replace(true) {
            if let Some(w) = self.app() {
                w.set_timer(SHIELD_TIMER, SHIELD_DEBOUNCE_MS);
            }
        }
    }

    pub(super) fn shield_timer(&self) {
        if let Some(w) = self.app() {
            w.kill_timer(SHIELD_TIMER);
        }
        self.shield_pending.set(false);
        self.send_nav();
        let shield_open =
            self.popup.borrow().as_ref().is_some_and(|p| matches!(p.content, Some(PopupContent::Shield { .. })));
        if shield_open {
            self.show_shield();
        }
    }

    /// `(blocked, site allowed)` for the active tab.
    pub(super) fn shield_state(&self) -> (u32, bool) {
        let Some(id) = self.active_id() else { return (0, false) };
        self.views
            .try_borrow()
            .ok()
            .and_then(|v| v.get(&id).map(|v| (v.net.blocked(), v.net.allowed())))
            .unwrap_or((0, false))
    }

    pub(super) fn shield_content(&self) -> PopupContent {
        let host = self.active_tab().and_then(|t| urlclean::host_of(&t.url)).unwrap_or_default();
        let (blocked, allowed) = self.shield_state();
        let entries = self
            .active_id()
            .and_then(|id| self.views.borrow().get(&id).map(|v| v.net.blocked_entries()))
            .unwrap_or_default();
        let all = lb_core::blocklog::summarize(&entries);
        let recent = all.iter().take(SHIELD_RECENT).map(|(i, _)| i.clone()).collect();
        PopupContent::Shield { host, blocked, allowed, enabled: self.net.enabled(), recent, distinct: all.len() as u32 }
    }

    /// The shield panel, sized for its list of blocked addresses.
    pub(super) fn show_shield(&self) {
        let content = self.shield_content();
        let rows = match &content {
            PopupContent::Shield { recent, .. } => recent.len(),
            _ => 0,
        };
        let rect = self.shield_anchor.get().map(|a| lb_core::layout::shield_rect(a, rows));
        self.show_popup(content, rect);
    }

    /// Shield panel "Show all": krunk://adblock for the active tab.
    pub(super) fn open_blocked_page(&self) {
        self.hide_popup();
        let Some(source) = self.active_id() else { return };
        self.adblock_source.set(Some(source));
        // Open already (another tab's list, or the lists): it shows this tab's now.
        let open = self.ws.borrow().find_page(Page::Adblock);
        if let Some((_, tab)) = open {
            if let Some(v) = self.tab_view(tab) {
                v.reload();
            }
        }
        self.open_page(Page::Adblock);
    }

    /// krunk://adblock asks what was blocked on its source tab: grouped, with the filter that
    /// matched each address (looked up on a worker thread, see `Blocker::explain`).
    pub(super) fn get_blocked(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let source = self.adblock_source.get().filter(|s| self.tab(*s).is_some());
        let (host, entries) = match source {
            Some(s) => (
                self.tab(s).and_then(|t| urlclean::host_of(&t.url)).unwrap_or_default(),
                self.views.borrow().get(&s).map(|v| v.net.blocked_entries()).unwrap_or_default(),
            ),
            None => (String::new(), Vec::new()),
        };
        let summary = lb_core::blocklog::summarize(&entries);
        let requests: Vec<(String, String, ResourceKind)> = summary
            .iter()
            .map(|(_, i)| (entries[*i].url.clone(), entries[*i].page.clone(), entries[*i].kind))
            .collect();
        let items: Vec<lb_core::blocklog::Item> = summary.into_iter().map(|(i, _)| i).collect();
        let lists = filterlists::current_texts(
            &self.dirs.filters(),
            &self.filter_sources().sources(),
            &self.filters.update.borrow(),
        );
        let enabled = self.net.enabled();
        let alive = source.is_some();
        self.spawn_job(
            "explain-blocked",
            move || {
                let lists: Vec<(&str, &str)> = lists.iter().map(|(i, t)| (i.as_str(), t.as_str())).collect();
                let reqs: Vec<(&str, &str, ResourceKind)> =
                    requests.iter().map(|(u, p, k)| (u.as_str(), p.as_str(), *k)).collect();
                Blocker::explain(&lists, &reqs)
            },
            move |b, explained| {
                let items: Vec<_> = items
                    .into_iter()
                    .zip(explained)
                    .map(|(mut i, e)| {
                        if let Some(e) = e {
                            i.list = (!e.list.is_empty()).then(|| b.list_name(&e.list));
                            i.rule = Some(e.rule);
                        }
                        i
                    })
                    .collect();
                b.reply(
                    tab,
                    Event::ok(
                        req_id,
                        serde_json::json!({ "host": host, "source": alive, "enabled": enabled, "items": items }),
                    ),
                );
            },
        );
    }

    /// Shield toggle: allow (or block again) ads on the active tab's site,
    /// then reload it.
    pub(super) fn shield_allow_site(&self, allow: bool) {
        let Some(host) = self.active_tab().and_then(|t| urlclean::host_of(&t.url)) else { return };
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
        self.set_scriptlet_engine(None);
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
                self.set_scriptlet_engine(None);
                self.reply_db(tab, req_id, r);
            }
            Request::AllowlistRemove { host } => {
                let r = self.store.borrow().allowlist_remove(&host);
                self.filters.allowlist.borrow_mut().remove(&host);
                self.set_scriptlet_engine(None);
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
        let on_interstitial = self.tab(tab).is_some_and(|t| Page::from_url(&t.url) == Some(Page::HttpsOnly));
        if !valid || !on_interstitial {
            return;
        }
        if let Some(host) = urlclean::host_of(url) {
            self.filters.http_exceptions.borrow_mut().insert(host);
        }
        self.navigate_tab(tab, url);
    }

    /// Navigates a specific tab as the host. No borrow is held while the
    /// engine runs: it may call back into the browser synchronously.
    pub(super) fn navigate_tab(&self, id: TabId, url: &str) {
        let view = self.views.borrow().get(&id).map(|v| {
            v.host_navigation.set(true);
            v.view.clone()
        });
        if let Some(view) = view {
            view.navigate(url);
        }
    }
}

/// Deferred work (run after the current engine callback returns).
pub(super) type Deferred = Box<dyn FnOnce(&Browser)>;

impl Browser {
    pub(super) fn defer(&self, f: impl FnOnce(&Browser) + 'static) {
        self.deferred.borrow_mut().push(Box::new(f));
        if let Some(w) = self.app() {
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
