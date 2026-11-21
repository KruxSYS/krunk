//! The browser engine: Chromium Embedded Framework (CEF 156, Alloy style),
//! bundled next to the exe and loaded through CEF's sandbox bootstrap.
//!
//! Everything engine-specific lives here behind a small safe API: [`View`]
//! (one browser: the shell, the popup or a tab), [`run_engine`] and
//! [`post_ui`]. CEF calls most handlers on its UI thread, which is our main
//! thread (single-threaded message loop), so they can reach the host through
//! thread-local state. Resource and cookie hooks run on CEF's IO thread and
//! only touch `Send + Sync` state ([`RequestPolicy`]).

mod app;
mod client;
mod cookies;
mod data;
pub mod entry;
pub mod fetch;
mod hints;
mod net;
pub mod relay;
mod renderer;
mod scheme;
mod scriptlets;
pub mod tasks;
mod view;

pub use cookies::{cookie_header, delete_cookies, list_cookies, set_cookie_value, Profile};
pub use data::clear_profile_data;
pub use hints::set_no_hints;
pub use net::{NetRequest, NetVerdict, RequestPolicy};
pub use scheme::InternalResponse;
pub use scriptlets::set_scriptlets;
pub use view::{
    with_batched_moves, AuthChallenge, AuthResponder, CertErrorSeen, ClearKinds, DownloadControl, DownloadStart,
    DownloadStatus, DownloadUpdate, JsDialogRequest, JsDialogResponder, KeyPress, LoadingState, Menu, MessageSource,
    NewWindow, OpenIn, Role, Scripts, View, ViewOptions, AUTH_TIMEOUT_MS,
};

use crate::log::{info, warning};
use cef::*;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// What the engine is started with (from the settings and data folder).
pub struct EngineConfig {
    /// CEF's root profile folder; `cache` must be inside it.
    pub root_cache: PathBuf,
    /// The persistent profile of normal tabs, the shell and the popup.
    pub cache: PathBuf,
    pub log_file: PathBuf,
    /// Chromium switches, `--name` or `--name=value`.
    pub switches: Vec<String>,
    /// Empty = the engine's default user agent.
    pub user_agent: String,
    /// 0.15.0: what a browser shows before its page has painted when its own settings do not
    /// say (`CefSettings.background_color`, ARGB): the theme's page colour, never black.
    pub background: u32,
}

/// Starts the browser once the engine is ready; returns what must stay alive
/// until the message loop ends (dropped before the engine shuts down), or
/// `None` to quit right away.
pub type Start = Box<dyn FnOnce() -> Option<Box<dyn Any>>>;

type InternalServer = Rc<dyn Fn(&str) -> InternalResponse>;
type RelaunchHandler = Rc<dyn Fn(Vec<String>)>;
type UiClosure = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;
type SendClosure = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

thread_local! {
    /// Browser process, UI thread: every live CEF browser by identifier.
    static BROWSERS: RefCell<HashMap<i32, Browser>> = RefCell::new(HashMap::new());
    /// Live browsers that use the private (in-memory) request context.
    static PRIVATE_LIVE: Cell<usize> = const { Cell::new(0) };
    static PRIVATE_CONTEXT: RefCell<Option<RequestContext>> = const { RefCell::new(None) };
    /// 0.14.0: the private context's preferences (the VPN's proxy among them) are in place;
    /// until then its browsers wait in `PRIVATE_WAITING` (CEF initializes a new context, and
    /// calls its handler, asynchronously: a browser created at once could navigate first).
    static PRIVATE_READY: Cell<bool> = const { Cell::new(false) };
    static PRIVATE_WAITING: RefCell<Vec<Box<dyn FnOnce()>>> = RefCell::new(Vec::new());
    static QUITTING: Cell<bool> = const { Cell::new(false) };
    static START: RefCell<Option<Start>> = RefCell::new(None);
    static KEEP: RefCell<Option<Box<dyn Any>>> = RefCell::new(None);
    /// Answers requests to the internal origins (embedded UI, cached favicons).
    static INTERNAL: RefCell<Option<InternalServer>> = RefCell::new(None);
    /// A second `Krunk.exe` was started: its command-line arguments.
    static RELAUNCH: RefCell<Option<RelaunchHandler>> = RefCell::new(None);
}

/// Initializes CEF, runs `start` when it is ready, then runs the message loop
/// until the browser quits (see [`quit_when_closed`]). Returns the exit code.
pub fn run_engine(config: EngineConfig, start: Start) -> Result<i32, String> {
    let Some((main_args, sandbox_info)) = entry::process_args() else {
        return Err("The engine entry point was not used".into());
    };
    let settings = Settings {
        no_sandbox: 0,
        multi_threaded_message_loop: 0,
        // Only our own switches (App::on_before_command_line_processing)
        // configure the engine; nothing typed on Krunk.exe's command
        // line can (e.g. --no-sandbox or --remote-debugging-port).
        command_line_args_disabled: 1,
        root_cache_path: CefString::from(config.root_cache.to_string_lossy().as_ref()),
        cache_path: CefString::from(config.cache.to_string_lossy().as_ref()),
        log_file: CefString::from(config.log_file.to_string_lossy().as_ref()),
        log_severity: LogSeverity::WARNING,
        persist_session_cookies: 0,
        user_agent: CefString::from(config.user_agent.as_str()),
        background_color: config.background,
        ..Default::default()
    };
    START.with(|s| *s.borrow_mut() = Some(start));
    let mut app = app::browser_app(config.switches);
    if initialize(Some(&main_args), Some(&settings), Some(&mut app), sandbox_info) != 1 {
        let code = get_exit_code();
        // 24 = CEF_RESULT_CODE_NORMAL_EXIT_PROCESS_NOTIFIED: another
        // Krunk is running with this profile and was told to open a
        // window; this process just exits.
        if code == 24 {
            info!("handed over to the running instance");
            return Ok(0);
        }
        return Err(format!("The browser engine failed to start (code {code})"));
    }
    info!("engine initialized: CEF {}, Chromium {}", versions().0, versions().1);
    LAUNCHED.store(basetime_now().val, std::sync::atomic::Ordering::Relaxed);
    run_message_loop();
    // Release everything that holds CEF objects before shutting down.
    KEEP.with(|k| k.borrow_mut().take());
    INTERNAL.with(|i| i.borrow_mut().take());
    RELAUNCH.with(|r| r.borrow_mut().take());
    PRIVATE_CONTEXT.with(|c| c.borrow_mut().take());
    BROWSERS.with(|b| b.borrow_mut().clear());
    // The task manager too (0.14.1): left in its thread-local it was released at process exit,
    // after the engine had shut down, and the engine's check crashed Krunk while closing.
    tasks::release();
    shutdown();
    Ok(0)
}

/// `BrowserProcessHandler::on_context_initialized`: the engine is ready.
fn context_initialized() {
    if let Some(mut ctx) = request_context_get_global_context() {
        app::apply_profile_prefs(&mut ctx);
    }
    scheme::register(request_context_get_global_context(), true);
    let start = START.with(|s| s.borrow_mut().take());
    let keep = start.and_then(|f| f());
    match keep {
        Some(k) => KEEP.with(|slot| *slot.borrow_mut() = Some(k)),
        None => {
            warning!("browser start failed; quitting");
            quit_message_loop();
        }
    }
}

/// Serves the internal origins (UI thread). Must be set before the first
/// view is created.
pub fn set_internal_server(f: impl Fn(&str) -> InternalResponse + 'static) {
    INTERNAL.with(|i| *i.borrow_mut() = Some(Rc::new(f)));
}

/// Called with the arguments of a second `Krunk.exe` started while
/// this one runs (it exits after handing over).
pub fn on_relaunch(f: impl Fn(Vec<String>) + 'static) {
    RELAUNCH.with(|r| *r.borrow_mut() = Some(Rc::new(f)));
}

fn relaunched(args: Vec<String>) {
    if let Some(f) = RELAUNCH.with(|r| r.borrow().clone()) {
        f(args);
    }
}

thread_local! {
    /// Extension origins registered so far (the private context gets them when created).
    static EXT_HOSTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// An extension's origin answers from the internal handler on both profiles (its pages are
/// never private, but its icons and files are shown in private windows too).
pub fn register_scheme_host(host: &str) {
    let new = EXT_HOSTS.with(|h| {
        let mut h = h.borrow_mut();
        if h.iter().any(|x| x == host) {
            false
        } else {
            h.push(host.to_string());
            true
        }
    });
    if !new {
        return;
    }
    scheme::register_host(request_context_get_global_context(), host);
    if let Some(c) = PRIVATE_CONTEXT.with(|c| c.borrow().clone()) {
        scheme::register_host(Some(c), host);
    }
}

fn internal_response(url: &str) -> InternalResponse {
    match INTERNAL.with(|i| i.borrow().clone()) {
        Some(f) => f(url),
        None => InternalResponse::not_found(),
    }
}

/// Ends the message loop once every browser has closed (the host closes its
/// views first). A browser that never reports back cannot keep the process
/// alive: the loop ends after a few seconds regardless.
pub fn quit_when_closed() {
    if QUITTING.replace(true) {
        return;
    }
    // DevTools windows and anything the host no longer tracks close too.
    let left: Vec<Browser> = BROWSERS.with(|b| b.borrow().values().cloned().collect());
    for b in left {
        if let Some(h) = b.host() {
            h.close_browser(1);
        }
    }
    if BROWSERS.with(|b| b.borrow().is_empty()) {
        post_ui(quit_message_loop);
    } else {
        post_ui_delayed(4000, || {
            let left = BROWSERS.with(|b| b.borrow().len());
            if left > 0 {
                warning!("{left} browser(s) did not close in time; quitting anyway");
                quit_message_loop();
            }
        });
    }
}

fn browser_created(browser: &Browser, private: bool) {
    BROWSERS.with(|b| b.borrow_mut().insert(browser.identifier(), browser.clone()));
    if private {
        PRIVATE_LIVE.with(|n| n.set(n.get() + 1));
    }
}

fn browser_closed(browser: &Browser, private: bool) {
    let left = BROWSERS.with(|b| {
        let mut b = b.borrow_mut();
        b.remove(&browser.identifier());
        b.len()
    });
    if private {
        let live = PRIVATE_LIVE.with(|n| {
            n.set(n.get().saturating_sub(1));
            n.get()
        });
        // The last private tab closed: forget the in-memory profile, so the
        // next private tab starts empty.
        if live == 0 {
            PRIVATE_CONTEXT.with(|c| c.borrow_mut().take());
            PRIVATE_READY.set(false);
            info!("private session ended");
        }
    }
    if left == 0 && QUITTING.get() {
        quit_message_loop();
    }
}

/// The in-memory request context shared by private tabs (created on demand).
/// 0.26.0 "Preload pages for faster browsing": the default profile's network prediction, at once
/// once the engine runs (before that, the value its context starts with). Private tabs never.
pub fn set_preload(on: bool) -> bool {
    let mut global = request_context_get_global_context();
    let ok = app::set_preload(on, global.as_mut());
    info!("preload pages: {}{}", if on { "on" } else { "off" }, if ok { "" } else { " (not applied)" });
    ok
}

/// 0.26.0: hosts the engine must never preconnect to or pre-resolve (the ad blocker's plain host
/// rules, `lb_core::preconnect_block`); empty when blocking is off. Process-wide.
pub fn set_blocked_hosts(hosts: &[String]) -> bool {
    let (Some(mut global), Some(value), Some(mut list)) =
        (request_context_get_global_context(), value_create(), list_value_create())
    else {
        return false;
    };
    // Appended in order, never set_size() first: CEF's SetString on an existing slot erases and
    // re-inserts it (CefListValueImpl::SetInternal), O(n) per host. With set_size() the 85 000
    // hosts took 39 s of the UI thread at start and again after each list update (0.26.0 smoke:
    // "Not Responding"). Appending is O(1) per host.
    let t0 = std::time::Instant::now();
    for (i, h) in hosts.iter().enumerate() {
        list.set_string(i, Some(&CefString::from(h.as_str())));
    }
    value.set_list(Some(&mut list));
    let ok = app::set_krunk_pref(&mut global, "krunk.blocked_hosts", value);
    if ok {
        info!("preconnect: {} blocked host(s) handed to the engine in {} ms", hosts.len(), t0.elapsed().as_millis());
    }
    ok
}

/// 0.26.0: what Chrome's omnibox does for its top match: the engine connects early to the site
/// (and to the hosts it usually needs) in the default profile. A no-op when "Preload pages" is off
/// (the engine checks its prediction preference) and for blocked hosts. Never call it for
/// private tabs.
pub fn preconnect(url: &str) -> bool {
    let (Some(mut global), Some(value)) = (request_context_get_global_context(), value_create()) else {
        return false;
    };
    value.set_string(Some(&CefString::from(url)));
    app::set_krunk_pref(&mut global, "krunk.preconnect", value)
}

/// 0.26.0: forgets what the engine's loading predictor learned (the hosts each site needs),
/// with the history.
pub fn clear_predictor() -> bool {
    let (Some(mut global), Some(value)) = (request_context_get_global_context(), value_create()) else {
        return false;
    };
    value.set_bool(1);
    app::set_krunk_pref(&mut global, "krunk.predictor_clear", value)
}

/// 0.26.0: Krunk is offline for normal tabs (the VPN's stand-in could not be put in place): no
/// network prediction meanwhile.
pub fn set_preload_offline(offline: bool) {
    let mut global = request_context_get_global_context();
    app::set_preload_offline(offline, global.as_mut());
}

/// The setting the default profile starts with (before the engine runs: no CEF call).
pub fn init_preload(on: bool) {
    app::set_preload(on, None);
}

/// See `app::set_doh`.
pub fn set_doh(mode: &str, templates: &str) -> bool {
    app::set_doh(mode, templates)
}

/// The VPN's local proxy for all of the engine's traffic (0.14.0): `Some((server, bypass))`
/// sets the `proxy` preference to `fixed_servers` with `server` (`http://127.0.0.1:<port>`) on
/// the default and the private profile (also one created later) and keeps WebRTC from sending
/// UDP outside it; `None` goes back to the system's settings. True when every profile took it.
/// With `private_only` (0.22.0) only the private profile gets it and the default one goes back
/// to the system's settings.
///
/// When every profile took it, the engine's open connections (HTTP/2 and QUIC sessions, idle
/// and pending sockets, WebSocket pools) are closed in both profiles so they come back the new
/// way. WebRTC peer connections can't be moved: they keep their path until they end.
pub fn set_vpn_proxy(proxy: Option<(String, String)>, private_only: bool) -> bool {
    let private = PRIVATE_CONTEXT.with(|c| c.borrow().clone());
    // A private context that is still initializing gets the proxy in its handler (before any
    // of its browsers exists); setting preferences on it now would be refused.
    let private = private.filter(|_| PRIVATE_READY.get());
    let mut contexts: Vec<RequestContext> = request_context_get_global_context().into_iter().collect();
    contexts.extend(private);
    let ok =
        app::set_vpn_proxy(proxy.map(|(server, bypass)| app::VpnProxy { server, bypass, private_only }), &mut contexts);
    if ok {
        for ctx in &contexts {
            let mut done = ConnectionsClosed::new(if ctx.is_global() != 0 { "default" } else { "private" });
            ctx.close_all_connections(Some(&mut done));
        }
        info!(
            "vpn: the engine's open connections are being closed so they reconnect the new way; \
             WebRTC peer connections can't move and keep their path until they end"
        );
    }
    ok
}

/// Forgets the certificate decisions the engine keeps for the session in both profiles
/// (after "Continue anyway" it lets the same certificate through without asking again).
/// Krunk's own exceptions are asked afresh at the next certificate error, so a removed one
/// takes effect at once.
pub fn clear_certificate_exceptions() {
    let private = PRIVATE_CONTEXT.with(|c| c.borrow().clone()).filter(|_| PRIVATE_READY.get());
    for ctx in request_context_get_global_context().into_iter().chain(private) {
        ctx.clear_certificate_exceptions(None);
    }
}

/// At a start without the VPN: removes a VPN proxy the engine kept on disk from an earlier
/// run, and only then (0.14.1: resetting the preference when nothing was set made the engine
/// log an observer-list check at every start). True when one was removed.
pub fn clear_stale_vpn_proxy() -> bool {
    let Some(ctx) = request_context_get_global_context() else { return false };
    if !app::vpn_proxy_left(&ctx) {
        return false;
    }
    set_vpn_proxy(None, false)
}

wrap_completion_callback! {
    struct ConnectionsClosed {
        scope: &'static str,
    }
    impl CompletionCallback {
        fn on_complete(&self) {
            info!("vpn: the {} profile's open connections were closed", self.scope);
        }
    }
}

/// The private context's preferences are in place (its handler ran): browsers waiting for it
/// are created now. `ctx` must be the current private context (an older one finishing late
/// changes nothing).
pub(super) fn private_context_ready(ctx: &mut RequestContext) {
    let current = PRIVATE_CONTEXT.with(|c| c.borrow().clone());
    let Some(current) = current else { return };
    if current.is_same(Some(ctx)) == 0 {
        return;
    }
    PRIVATE_READY.set(true);
    let waiting = PRIVATE_WAITING.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for f in waiting {
        f();
    }
}

/// The private context did not take the VPN's proxy: it is dropped with the browsers waiting
/// for it (their tabs stay empty; the next private tab makes a new context and tries again).
pub(super) fn private_context_failed(ctx: &mut RequestContext) {
    let current = PRIVATE_CONTEXT.with(|c| c.borrow().clone());
    if !current.is_some_and(|c| c.is_same(Some(ctx)) != 0) {
        return;
    }
    PRIVATE_CONTEXT.with(|c| c.borrow_mut().take());
    PRIVATE_READY.set(false);
    let waiting = PRIVATE_WAITING.with(|w| std::mem::take(&mut *w.borrow_mut()));
    warning!("vpn: the private profile did not take the VPN's proxy; {} private tab(s) not loaded", waiting.len());
}

/// Runs `f` once the private context's preferences are in place (at once when they are).
pub(super) fn when_private_ready(f: impl FnOnce() + 'static) {
    if PRIVATE_READY.get() {
        f();
    } else {
        PRIVATE_WAITING.with(|w| w.borrow_mut().push(Box::new(f)));
    }
}

fn private_context() -> Option<RequestContext> {
    if let Some(c) = PRIVATE_CONTEXT.with(|c| c.borrow().clone()) {
        return Some(c);
    }
    // An empty cache path makes an incognito, memory-only context.
    let settings = RequestContextSettings { persist_session_cookies: 0, ..Default::default() };
    let mut handler = app::context_handler();
    PRIVATE_READY.set(false);
    let ctx = request_context_create_context(Some(&settings), Some(&mut handler))?;
    scheme::register(Some(ctx.clone()), false);
    for host in EXT_HOSTS.with(|h| h.borrow().clone()) {
        scheme::register_host(Some(ctx.clone()), &host);
    }
    PRIVATE_CONTEXT.with(|c| *c.borrow_mut() = Some(ctx.clone()));
    info!("private session started");
    Some(ctx)
}

/// Light/dark preference for web content (`prefers-color-scheme`) in both
/// profiles: `None` follows Windows.
pub fn set_color_scheme(dark: Option<bool>) {
    let variant = match dark {
        None => ColorVariant::SYSTEM,
        Some(false) => ColorVariant::LIGHT,
        Some(true) => ColorVariant::DARK,
    };
    if let Some(c) = request_context_get_global_context() {
        c.set_chrome_color_scheme(variant, 0);
    }
    if let Some(c) = PRIVATE_CONTEXT.with(|c| c.borrow().clone()) {
        c.set_chrome_color_scheme(variant, 0);
    }
}

wrap_task! {
    struct UiTask {
        f: UiClosure,
    }
    impl Task {
        fn execute(&self) {
            let f = self.f.borrow_mut().take();
            if let Some(f) = f {
                let _stall = crate::perf::Stall::new("task");
                f();
            }
        }
    }
}

/// One entry of the engine's cross-origin allow-list (UI thread): requests from
/// `source_origin` to `protocol://domain` (with `subdomains`: its subdomains too; an empty
/// domain with `subdomains`: every domain) are not subject to CORS. `add` false removes the
/// entry. Used for an extension's own origin and its host permissions only.
pub fn cross_origin_allow(source_origin: &str, protocol: &str, domain: &str, subdomains: bool, add: bool) -> bool {
    let (o, p, d) = (CefString::from(source_origin), CefString::from(protocol), CefString::from(domain));
    let n = if add {
        add_cross_origin_whitelist_entry(Some(&o), Some(&p), Some(&d), subdomains as _)
    } else {
        remove_cross_origin_whitelist_entry(Some(&o), Some(&p), Some(&d), subdomains as _)
    };
    n != 0
}

/// Runs `f` on the UI thread after the current callback returns. Only call
/// this on the UI thread (use [`post_ui_from_any`] elsewhere).
pub fn post_ui(f: impl FnOnce() + 'static) {
    let mut task = UiTask::new(Rc::new(RefCell::new(Some(Box::new(f)))));
    post_task(ThreadId::UI, Some(&mut task));
}

/// Like [`post_ui`], after `delay_ms` milliseconds.
pub fn post_ui_delayed(delay_ms: i64, f: impl FnOnce() + 'static) {
    let mut task = UiTask::new(Rc::new(RefCell::new(Some(Box::new(f)))));
    post_delayed_task(ThreadId::UI, Some(&mut task), delay_ms);
}

wrap_task! {
    struct SendTask {
        f: SendClosure,
    }
    impl Task {
        fn execute(&self) {
            let f = self.f.lock().ok().and_then(|mut f| f.take());
            if let Some(f) = f {
                let _stall = crate::perf::Stall::new("task_any");
                f();
            }
        }
    }
}

/// Runs `f` on the engine's IO thread after `delay_ms` milliseconds; callable from any thread.
/// The watchdogs of held network callbacks (0.22.0) use it: it does not need the UI thread.
pub fn post_io_delayed(delay_ms: i64, f: impl FnOnce() + Send + 'static) {
    let mut task = SendTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    post_delayed_task(ThreadId::IO, Some(&mut task), delay_ms);
}

/// Runs `f` on the UI thread; callable from any engine thread.
pub fn post_ui_from_any(f: impl FnOnce() + Send + 'static) {
    let mut task = SendTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    post_task(ThreadId::UI, Some(&mut task));
}

#[link(name = "libcef")]
unsafe extern "C" {
    /// `include/cef_version_info.h`: 0..3 = CEF major, minor, patch, commit;
    /// 4..7 = Chromium major, minor, build, patch.
    fn cef_version_info(entry: std::ffi::c_int) -> std::ffi::c_int;
}

/// `(CEF, Chromium)` versions of the loaded `libcef.dll`.
pub fn versions() -> (String, String) {
    // SAFETY: plain getter exported by libcef.dll; entries 0..=7 are defined.
    let v: Vec<i32> = (0..8).map(|i| unsafe { cef_version_info(i) }).collect();
    (format!("{}.{}.{}", v[0], v[1], v[2]), format!("{}.{}.{}.{}", v[4], v[5], v[6], v[7]))
}

/// When the engine started (CEF base time, microseconds): downloads that started earlier are
/// ones the engine resumed from an earlier session (0.7.0).
static LAUNCHED: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(i64::MAX);

pub(super) fn launched_at() -> i64 {
    LAUNCHED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The browser process's command line as the engine runs it (CEF's own switches and
/// Krunk's, merged; 0.7.0).
pub fn command_line() -> String {
    command_line_get_global().map(|c| CefString::from(&c.command_line_string()).to_string()).unwrap_or_default()
}

/// The user agent the engine presents for the setting "Default" (reduced UA format; the
/// switches of `lb_core::useragents::engine_switches`).
pub fn default_user_agent() -> String {
    // What the engine presents for user agent "Default" (0.27.0): the latest Chrome.
    lb_core::useragents::CHROME_WINDOWS.to_string()
}
