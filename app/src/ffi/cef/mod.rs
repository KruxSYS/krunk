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
mod hints;
mod net;
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
    with_batched_moves, AuthChallenge, AuthResponder, ClearKinds, DownloadControl, DownloadStart, DownloadStatus,
    DownloadUpdate, KeyPress, LoadingState, MessageSource, NewWindow, Role, Scripts, View, ViewOptions,
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
            info!("private session ended");
        }
    }
    if left == 0 && QUITTING.get() {
        quit_message_loop();
    }
}

/// The in-memory request context shared by private tabs (created on demand).
/// See `app::set_doh`.
pub fn set_doh(mode: &str, templates: &str) -> bool {
    app::set_doh(mode, templates)
}

fn private_context() -> Option<RequestContext> {
    if let Some(c) = PRIVATE_CONTEXT.with(|c| c.borrow().clone()) {
        return Some(c);
    }
    // An empty cache path makes an incognito, memory-only context.
    let settings = RequestContextSettings { persist_session_cookies: 0, ..Default::default() };
    let mut handler = app::context_handler();
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
                f();
            }
        }
    }
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
                f();
            }
        }
    }
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

/// The engine's default user agent (what Chromium sends without an
/// override; reduced UA format).
pub fn default_user_agent() -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 \
         Safari/537.36",
        versions().1.split('.').next().unwrap_or("0")
    )
}
