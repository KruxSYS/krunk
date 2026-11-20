//! Renderer process: the message bridge between pages and the browser
//! process, and the content scripts.
//!
//! In every main frame, before any page script runs, a native function is
//! wrapped by a tiny shim that defines `window.__lbHost` with
//! `postMessage(obj)` and `addEventListener('message', fn)` (the API the UI
//! used with WebView2's `chrome.webview`). Messages travel as JSON strings in
//! `lb.msg` (to the browser) and `lb.evt` (to the page) process messages. The
//! browser process decides what a message may do from the sending frame's
//! committed URL and main-frame flag, never from its contents.

use super::hints;
use super::scriptlets;
use super::view::Scripts;
use cef::*;
use lb_core::assets;
use lb_core::blocker::{Allowlist, ScriptletEngine};
use lb_core::headers::HintsState;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

/// Process message names.
pub(super) const TO_BROWSER: &str = "lb.msg";
pub(super) const TO_PAGE: &str = "lb.evt";
/// A frame started or stopped playing audio (`bool`).
pub(super) const AUDIO_TO_BROWSER: &str = "lb.audio";
/// Renderer → browser: this renderer's process id (once per browser, for the task manager).
pub(super) const PID_TO_BROWSER: &str = "lb.pid";
/// Renderer-side diagnostics for the host's log (a line of text).
pub(super) const DEBUG_TO_BROWSER: &str = "lb.rdebug";
/// 0.26.0 load trace: a page context's start cost (µs in uBO's scriptlets, µs in Krunk's page
/// scripts). Sent only by renderers started with [`PERF_SWITCH`] (the browser's
/// `KRUNK_PERF_TRACE` is on).
pub(super) const PERF_CTX_TO_BROWSER: &str = "lb.perfctx";
pub(super) const PERF_SWITCH: &str = "krunk-perf-trace";

/// Whether this renderer was started with [`PERF_SWITCH`] (read once).
fn perf_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        command_line_get_global().is_some_and(|cl| cl.has_switch(Some(&CefString::from(PERF_SWITCH))) != 0)
    })
}

fn report_context_cost(frame: &mut Frame, scriptlet_us: u128, script_us: u128) {
    let Some(mut message) = process_message_create(Some(&CefString::from(PERF_CTX_TO_BROWSER))) else { return };
    if let Some(args) = message.argument_list() {
        args.set_int(0, i32::try_from(scriptlet_us).unwrap_or(i32::MAX));
        args.set_int(1, i32::try_from(script_us).unwrap_or(i32::MAX));
    }
    frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
}

fn rdebug(frame: &mut Frame, text: &str) {
    let Some(mut message) = process_message_create(Some(&CefString::from(DEBUG_TO_BROWSER))) else { return };
    if let Some(args) = message.argument_list() {
        args.set_string(0, Some(&CefString::from(text)));
    }
    frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
}
/// Pointer lock, frame to browser (`LockMessage` JSON) and back (events).
pub(super) const POINTER_TO_BROWSER: &str = "lb.plock";
pub(super) const POINTER_TO_FRAME: &str = "lb.plock.evt";
const MAX_POINTER_MESSAGE: usize = lb_core::pointer_lock::MAX_MESSAGE_BYTES;
/// Bigger messages from a page are dropped (the largest legitimate ones are
/// bookmark imports, capped by the UI at a few MB).
const MAX_MESSAGE: usize = 8 * 1024 * 1024;

/// Installs `window.__lbHost` and returns the dispatch function for
/// browser-to-page messages. Built-ins are captured before page scripts run.
const SHIM: &str = r#"(function (send, expose) {
  'use strict';
  var stringify = JSON.stringify, parse = JSON.parse, listeners = [];
  var host = Object.freeze({
    postMessage: function (m) { send(stringify(m)); },
    addEventListener: function (type, fn) {
      if (type === 'message' && typeof fn === 'function') listeners[listeners.length] = fn;
    },
    removeEventListener: function (type, fn) {
      for (var i = 0; i < listeners.length; i++) if (listeners[i] === fn) { listeners.splice(i, 1); return; }
    }
  });
  // Krunk's own pages find the bridge on the window; web pages never do (0.6.0: nothing to
  // fingerprint), their injected scripts get it as a closure argument.
  if (expose) Object.defineProperty(window, '__lbHost', { value: host, writable: false, configurable: false, enumerable: false });
  var dispatch = function (json) {
    var data;
    try { data = parse(json); } catch (e) { return; }
    var copy = listeners.slice(), ev = { data: data };
    for (var i = 0; i < copy.length; i++) {
      try { copy[i](ev); } catch (e) { setTimeout(function () { throw e; }); }
    }
  };
  dispatch.host = host;
  return dispatch;
})"#;

/// Reports what this frame plays: 2 = audio (unmuted media elements that are playing, in the
/// document or not, and running Web Audio contexts), 1 = media without sound (muted or at
/// volume 0; 0.20.1, for the sleep exemption), 0 = nothing.
/// The native `report` function stays in the closure.
const AUDIO_TRACKER: &str = r#"(function (report) {
  'use strict';
  var playing = new Set(), silent = new Set(), contexts = new Set(), last = 0, hooked = new WeakSet();
  function update() {
    var now = playing.size > 0 || contexts.size > 0 ? 2 : silent.size > 0 ? 1 : 0;
    if (now !== last) { last = now; report(now); }
  }
  function track(el) {
    if (hooked.has(el)) return;
    hooked.add(el);
    var on = function () {
      var runs = !el.paused && !el.ended, heard = runs && !el.muted && el.volume > 0;
      if (heard) playing.add(el); else playing.delete(el);
      if (runs && !heard) silent.add(el); else silent.delete(el);
      update();
    };
    ['playing', 'pause', 'ended', 'emptied', 'volumechange'].forEach(function (t) { el.addEventListener(t, on); });
    on();
  }
  document.addEventListener('play', function (e) { if (e.target instanceof HTMLMediaElement) track(e.target); }, true);
  var play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function () { track(this); return play.apply(this, arguments); };
  ['AudioContext', 'webkitAudioContext'].forEach(function (name) {
    var AC = window[name];
    if (typeof AC !== 'function') return;
    window[name] = new Proxy(AC, { construct: function (t, a, n) {
      var c = Reflect.construct(t, a, n);
      var on = function () { if (c.state === 'running') contexts.add(c); else contexts.delete(c); update(); };
      c.addEventListener('statechange', on);
      on();
      return c;
    } });
  });
})"#;

thread_local! {
    /// Browsers whose renderer already told the host its process id.
    static PID_SENT: RefCell<std::collections::HashSet<i32>> = RefCell::new(std::collections::HashSet::new());
}

/// Tells the browser process which OS process hosts this browser (the task manager shows
/// PIDs; CEF's task list has none).
fn report_pid(browser: &mut Browser, frame: &mut Frame) {
    let id = browser.identifier();
    if !PID_SENT.with(|s| s.borrow_mut().insert(id)) {
        return;
    }
    let Some(mut message) = process_message_create(Some(&CefString::from(PID_TO_BROWSER))) else { return };
    if let Some(args) = message.argument_list() {
        args.set_int(0, std::process::id() as i32);
    }
    frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
}

/// The uBO scriptlets of this renderer process (one engine for all its
/// browsers; see `scriptlets.rs`).
#[derive(Default)]
struct ProcessScriptlets {
    version: String,
    engine: Option<ScriptletEngine>,
    allow: Allowlist,
    enabled: bool,
    /// Browsers that told the browser process which version we have.
    greeted: HashSet<i32>,
}

impl ProcessScriptlets {
    fn apply(&mut self, r: scriptlets::Received) {
        self.allow = Allowlist::new(r.allow.lines().map(str::to_string));
        self.enabled = r.enabled;
        if r.version == self.version {
            return;
        }
        if let Some(bytes) = r.bytes {
            if let Ok(engine) = ScriptletEngine::deserialize(&bytes) {
                self.engine = Some(engine);
                self.version = r.version;
            }
        }
    }
}

thread_local! {
    static SCRIPTLETS: RefCell<ProcessScriptlets> = RefCell::new(ProcessScriptlets::default());
    /// Content scripts per browser identifier (from the browser's extra info), with the number
    /// of live browser objects for that identifier: when a frame moves to this process (a
    /// cross-origin iframe, a reload), CEF creates the new browser object before it destroys
    /// the old one with the same identifier, so a plain remove-on-destroy would drop the entry
    /// the new frames need (their login script then never ran).
    static SCRIPTS: RefCell<HashMap<i32, (Scripts, u32)>> = RefCell::new(HashMap::new());
    /// The dispatch function of each main frame, by frame identifier, with the context it
    /// belongs to: a navigation can create the new document's context before the old one is
    /// released, so a release only drops the entry of its own context.
    static DISPATCH: RefCell<HashMap<String, (V8Context, V8Value)>> = RefCell::new(HashMap::new());
    /// The pointer-lock shim of each frame, by frame identifier (same rule).
    static POINTER: RefCell<HashMap<String, (V8Context, V8Value)>> = RefCell::new(HashMap::new());
    /// "Send client hints" off, the newest this process has heard of (0.7.1; `None` until the
    /// process's own command line was read).
    static HINTS: Cell<Option<HintsState>> = const { Cell::new(None) };
}

fn hints_state() -> HintsState {
    HINTS.with(|h| {
        h.get().unwrap_or_else(|| {
            let s = hints::from_own_command_line();
            h.set(Some(s));
            s
        })
    })
}

fn merge_hints(newer: HintsState) {
    let mut s = hints_state();
    s.merge(newer);
    HINTS.with(|h| h.set(Some(s)));
}

pub(super) fn handler() -> RenderProcessHandler {
    LbRenderer::new()
}

wrap_render_process_handler! {
    struct LbRenderer;
    impl RenderProcessHandler {
        fn on_browser_created(&self, browser: Option<&mut Browser>, extra_info: Option<&mut DictionaryValue>) {
            let (Some(browser), Some(info)) = (browser, extra_info) else { return };
            let scripts = Scripts::from_dictionary(info);
            merge_hints(hints::from_extra_info(info));
            SCRIPTS.with(|s| {
                let mut s = s.borrow_mut();
                let e = s.entry(browser.identifier()).or_insert((scripts, 0));
                e.0 = scripts;
                e.1 += 1;
            });
            let received = scriptlets::from_extra_info(info);
            SCRIPTLETS.with(|s| s.borrow_mut().apply(received));
        }

        fn on_browser_destroyed(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                SCRIPTS.with(|s| {
                    let mut s = s.borrow_mut();
                    if let Some(e) = s.get_mut(&b.identifier()) {
                        e.1 = e.1.saturating_sub(1);
                        if e.1 == 0 {
                            s.remove(&b.identifier());
                        }
                    }
                });
            }
        }

        fn on_context_created(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let (Some(browser), Some(frame), Some(context)) = (browser, frame, context) else { return };
            // Only the main world: the extensions' isolated world (0.6.0) is a context of the
            // same frame too, and must get neither the bridge (it would take over the frame's
            // dispatch entry) nor the scriptlets and page scripts.
            if frame.v8_context().is_none_or(|mut main| context.is_same(Some(&mut main)) != 1) {
                return;
            }
            if frame.is_main() == 1 {
                report_pid(browser, frame);
            }
            let timed = perf_trace().then(std::time::Instant::now);
            // First, before anything of the page runs: uBO's scriptlets.
            run_scriptlets(browser, frame, context);
            let after_scriptlets = timed.map(|_| std::time::Instant::now());
            let scripts = SCRIPTS.with(|s| s.borrow().get(&browser.identifier()).map(|e| e.0)).unwrap_or_default();
            context_created(scripts, frame, context);
            if let (Some(t0), Some(t1)) = (timed, after_scriptlets) {
                report_context_cost(frame, (t1 - t0).as_micros(), t1.elapsed().as_micros());
            }
        }

        fn on_context_released(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let (Some(frame), Some(context)) = (frame, context) else { return };
            let id = CefString::from(&frame.identifier()).to_string();
            for table in [&DISPATCH, &POINTER] {
                table.with(|d| {
                    let mut d = d.borrow_mut();
                    if d.get(&id).is_some_and(|(c, _)| c.is_same(Some(context)) == 1) {
                        d.remove(&id);
                    }
                });
            }
        }

        fn on_process_message_received(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> ::std::os::raw::c_int {
            let (Some(frame), Some(message)) = (frame, message) else { return 0 };
            let name = CefString::from(&message.name()).to_string();
            if name == scriptlets::TO_RENDERER {
                if let Some(args) = message.argument_list() {
                    let received = scriptlets::from_message(&args);
                    SCRIPTLETS.with(|s| s.borrow_mut().apply(received));
                }
                return 1;
            }
            if name == hints::TO_RENDERER {
                if let Some(args) = message.argument_list() {
                    merge_hints(hints::from_message(&args));
                }
                // Switched off while this document is open: it goes now (scripts that read it
                // already keep what they read; a reload starts clean).
                if hints_state().off && !is_own_page(&document_url(frame)) {
                    if let Some(mut context) = frame.v8_context() {
                        eval(&mut context, NO_USER_AGENT_DATA);
                    }
                }
                return 1;
            }
            if name == POINTER_TO_FRAME {
                if let Some(args) = message.argument_list() {
                    deliver_to(&POINTER, frame, &CefString::from(&args.string(0)));
                }
                return 1;
            }
            if name != TO_PAGE {
                return 0;
            }
            let Some(args) = message.argument_list() else { return 1 };
            let json = CefString::from(&args.string(0));
            deliver(frame, &json);
            1
        }
    }
}

/// "Send client hints" off: `navigator.userAgentData` goes in every frame.
const NO_USER_AGENT_DATA: &str = "(function(){try{delete Navigator.prototype.userAgentData}catch(e){}})()";

/// `login.js`'s switch for watching the DOM from the document's start (`Scripts::login_watch_early`).
const LOGIN_EARLY_MARK: &str = "/*KRUNK_LOGIN_EARLY*/false";

fn context_created(scripts: Scripts, frame: &mut Frame, context: &mut V8Context) {
    let main = frame.is_main() == 1;
    // Navigator properties matching the privacy headers: every frame.
    if scripts.gpc {
        eval(context, assets::inject_script("gpc.js"));
    }
    if scripts.dnt {
        eval(context, assets::inject_script("dnt.js"));
    }
    // No user-agent metadata ("Send client hints" off): no navigator.userAgentData (0.7.0; 0.7.1:
    // the setting as of now, not as of when the tab was made). Not in Krunk's own pages: the
    // shell reads the engine's hints there at start (extension pages are like web pages).
    if hints_state().off && !is_own_page(&document_url(frame)) {
        eval(context, NO_USER_AGENT_DATA);
    } else if document_url(frame).starts_with("http") {
        eval(context, assets::inject_script("nohints.js"));
    }
    if scripts.audio {
        install_audio_tracker(context);
    }
    if scripts.pointer_lock {
        install_pointer_lock(frame, context);
    }
    // The bridge and the login script run in every http(s) frame: a login form in an iframe
    // gets suggestions for its own origin (the host accepts only the field messages from
    // sub-frames). Page tokens are a main-frame matter.
    let host = install_bridge(frame, context);
    // An extension's own page (0.6.0): the chrome.* shim before its scripts, main frame only.
    if main
        && url::Url::parse(&document_url(frame))
            .ok()
            .and_then(|u| u.host_str().map(|h| h.ends_with(".ext.krunk.invalid")))
            .unwrap_or(false)
    {
        eval_with_bridge(context, assets::inject_script("ext-page.js"), host.as_ref());
        return;
    }
    if scripts.login {
        let login = assets::inject_script("login.js");
        if scripts.login_watch_early {
            // The A side of the F-scripts A/B (measurement builds): the 0.25.5 timing.
            eval_with_bridge(context, &login.replace(LOGIN_EARLY_MARK, "true"), host.as_ref());
        } else {
            eval_with_bridge(context, login, host.as_ref());
        }
    }
    if !main {
        return;
    }
    // Krunk's own website (the update server's origin, 0.6.0 R27): a frozen `window.krunk`
    // with the version, so the site can say "you're using Krunk". Main frames of that origin
    // only; no other site sees it, and it does not depend on the User-Agent.
    if is_site_origin(&document_url(frame)) {
        eval(context, SITE_MARKER);
    }
    // The Chrome Web Store (0.7.0, R20): "Add to Krunk" instead of "Add to Chrome".
    if url::Url::parse(&document_url(frame))
        .ok()
        .is_some_and(|u| u.scheme() == "https" && u.host_str() == Some("chromewebstore.google.com"))
    {
        eval_with_bridge(context, assets::inject_script("webstore.js"), host.as_ref());
    }
    if scripts.page_tokens {
        eval_with_bridge(context, assets::inject_script("page-tokens.js"), host.as_ref());
    }
    if scripts.notify {
        eval_with_bridge(context, assets::inject_script("notify.js"), host.as_ref());
    }
    if scripts.media {
        // The page's videos for Picture-in-Picture; same-origin frames are walked by it.
        eval_with_bridge(context, assets::inject_script("media.js"), host.as_ref());
    }
    // First paint, for the tab switch (0.7.0).
    eval_with_bridge(context, assets::inject_script("painted.js"), host.as_ref());
}

/// Runs the scriptlets matching the frame's document. About:blank and
/// srcdoc frames inherit their parent's URL (they run with its origin).
/// Nothing runs on allowlisted sites or with blocking off.
fn run_scriptlets(browser: &mut Browser, frame: &mut Frame, context: &mut V8Context) {
    let url = document_url(frame);
    let top = browser.main_frame().map(|f| CefString::from(&f.url()).to_string()).filter(|u| u.starts_with("http"));
    let site_url = top.unwrap_or_else(|| url.clone());
    let script = SCRIPTLETS.with(|s| {
        let mut s = s.borrow_mut();
        // Tell the browser process once which engine this process has.
        if s.greeted.insert(browser.identifier()) {
            hello(frame, &s.version);
        }
        if !s.enabled {
            return None;
        }
        let host = url::Url::parse(&site_url).ok().and_then(|u| u.host_str().map(str::to_string));
        if host.as_deref().is_some_and(|h| s.allow.contains(h)) {
            return None;
        }
        s.engine.as_ref()?.script_for(&url)
    });
    if let Some(js) = script {
        eval(context, &js);
    }
}

/// One of Krunk's own pages (`krunk.invalid` and its hosts like `shell.krunk.invalid`; not an
/// extension's origin, `<id>.ext.krunk.invalid`).
fn is_own_page(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .is_some_and(|h| (h == "krunk.invalid" || h.ends_with(".krunk.invalid")) && !h.ends_with(".ext.krunk.invalid"))
}

fn document_url(frame: &Frame) -> String {
    let url = CefString::from(&frame.url()).to_string();
    if url.is_empty() || url.starts_with("about:") {
        if let Some(parent) = frame.parent() {
            return document_url(&parent);
        }
    }
    url
}

fn hello(frame: &mut Frame, version: &str) {
    let Some(mut message) = process_message_create(Some(&CefString::from(scriptlets::HELLO))) else { return };
    if let Some(args) = message.argument_list() {
        args.set_string(0, Some(&CefString::from(version)));
        // (0.7.1) The client-hints generation: an older one gets the current state back.
        args.set_int(1, i32::try_from(hints_state().generation).unwrap_or(i32::MAX));
    }
    frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
}

fn eval(context: &mut V8Context, code: &str) -> Option<V8Value> {
    if code.is_empty() {
        return None;
    }
    let mut retval: Option<V8Value> = None;
    let mut exception: Option<V8Exception> = None;
    let ok = context.eval(Some(&CefString::from(code)), None, 0, Some(&mut retval), Some(&mut exception));
    if ok == 1 {
        retval
    } else {
        None
    }
}

/// Installs the page bridge; returns the host object the injected scripts get. Only
/// Krunk's own origins see it as `window.__lbHost`.
fn install_bridge(frame: &mut Frame, context: &mut V8Context) -> Option<V8Value> {
    let shim = eval(context, SHIM)?;
    let mut handler = SendToBrowser::new();
    let send = v8_value_create_function(Some(&CefString::from("send")), Some(&mut handler))?;
    let own = url::Url::parse(&CefString::from(&frame.url()).to_string())
        .ok()
        .and_then(|u| u.host_str().map(|h| h == "krunk.invalid" || h.ends_with(".krunk.invalid")))
        .unwrap_or(false);
    let expose = v8_value_create_bool(i32::from(own))?;
    let dispatch = shim.execute_function(None, Some(&[Some(send), Some(expose)]))?;
    if dispatch.is_function() != 1 {
        return None;
    }
    let host = dispatch.value_bykey(Some(&CefString::from("host")));
    let id = CefString::from(&frame.identifier()).to_string();
    DISPATCH.with(|d| d.borrow_mut().insert(id, (context.clone(), dispatch)));
    host
}

/// The update server's origin (built in; `app/build.rs`).
fn is_site_origin(document_url: &str) -> bool {
    let base = url::Url::parse(env!("KRUNK_UPDATE_BASE_URL")).ok().map(|u| u.origin());
    let here = url::Url::parse(document_url).ok().map(|u| u.origin());
    base.is_some() && base == here
}

/// `window.krunk = Object.freeze({ version })`, not writable, not configurable.
const SITE_MARKER: &str = concat!(
    "Object.defineProperty(window, 'krunk', { value: Object.freeze({ version: '",
    env!("CARGO_PKG_VERSION"),
    "' }), writable: false, configurable: false, enumerable: false });"
);

/// Runs an injected script as `(function (__lbHost) { … })(host)`.
fn eval_with_bridge(context: &mut V8Context, code: &str, host: Option<&V8Value>) {
    if code.is_empty() {
        return;
    }
    let Some(host) = host else { return };
    let Some(f) = eval(context, &format!("(function (__lbHost) {{\n{code}\n}})")) else { return };
    if f.is_function() == 1 {
        let _ = f.execute_function(None, Some(&[Some(host.clone())]));
    }
}

fn install_audio_tracker(context: &mut V8Context) {
    let Some(tracker) = eval(context, AUDIO_TRACKER) else { return };
    let mut handler = ReportAudio::new();
    let Some(report) = v8_value_create_function(Some(&CefString::from("report")), Some(&mut handler)) else {
        return;
    };
    let _ = tracker.execute_function(None, Some(&[Some(report)]));
}

/// Pointer lock in every frame (see `ui/inject/pointer-lock.js`).
fn install_pointer_lock(frame: &mut Frame, context: &mut V8Context) {
    let Some(shim) = eval(context, assets::inject_script("pointer-lock.js")) else { return };
    let mut handler = SendPointer::new();
    let Some(send) = v8_value_create_function(Some(&CefString::from("send")), Some(&mut handler)) else { return };
    let Some(dispatch) = shim.execute_function(None, Some(&[Some(send)])) else { return };
    if dispatch.is_function() == 1 {
        let id = CefString::from(&frame.identifier()).to_string();
        POINTER.with(|d| d.borrow_mut().insert(id, (context.clone(), dispatch)));
    }
}

/// Browser to page: calls the frame's dispatch function with the JSON text.
fn deliver(frame: &mut Frame, json: &CefString) {
    deliver_to(&DISPATCH, frame, json);
}

fn deliver_to(
    table: &'static std::thread::LocalKey<RefCell<HashMap<String, (V8Context, V8Value)>>>,
    frame: &mut Frame,
    json: &CefString,
) {
    let id = CefString::from(&frame.identifier()).to_string();
    let Some(dispatch) = table.with(|d| d.borrow().get(&id).map(|(_, f)| f.clone())) else {
        rdebug(frame, &format!("no dispatch for frame {id} ({})", CefString::from(&frame.url())));
        return;
    };
    let Some(context) = frame.v8_context() else {
        rdebug(frame, &format!("no context for frame {id}"));
        return;
    };
    if context.enter() != 1 {
        rdebug(frame, &format!("cannot enter the context of frame {id}"));
        return;
    }
    if let Some(arg) = v8_value_create_string(Some(json)) {
        let _ = dispatch.execute_function(None, Some(&[Some(arg)]));
    }
    context.exit();
}

wrap_v8_handler! {
    struct SendToBrowser;
    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            _retval: Option<&mut Option<V8Value>>,
            exception: Option<&mut CefString>,
        ) -> ::std::os::raw::c_int {
            let Some(arg) = arguments.and_then(|a| a.first()).and_then(|a| a.as_ref()) else { return 1 };
            if arg.is_string() != 1 {
                return 1;
            }
            let text = CefString::from(&arg.string_value());
            if text.as_slice().map_or(0, |s| s.len()) > MAX_MESSAGE {
                if let Some(e) = exception {
                    *e = CefString::from("message too large");
                }
                return 1;
            }
            let Some(frame) = v8_context_get_current_context().and_then(|c| c.frame()) else { return 1 };
            let Some(mut message) = process_message_create(Some(&CefString::from(TO_BROWSER))) else { return 1 };
            if let Some(args) = message.argument_list() {
                args.set_string(0, Some(&text));
            }
            frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            1
        }
    }
}

wrap_v8_handler! {
    struct SendPointer;
    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            _retval: Option<&mut Option<V8Value>>,
            _exception: Option<&mut CefString>,
        ) -> ::std::os::raw::c_int {
            let Some(arg) = arguments.and_then(|a| a.first()).and_then(|a| a.as_ref()) else { return 1 };
            if arg.is_string() != 1 {
                return 1;
            }
            let text = CefString::from(&arg.string_value());
            if text.to_string().len() > MAX_POINTER_MESSAGE {
                return 1;
            }
            let Some(frame) = v8_context_get_current_context().and_then(|c| c.frame()) else { return 1 };
            let Some(mut message) = process_message_create(Some(&CefString::from(POINTER_TO_BROWSER))) else {
                return 1;
            };
            if let Some(args) = message.argument_list() {
                args.set_string(0, Some(&text));
            }
            frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            1
        }
    }
}

wrap_v8_handler! {
    struct ReportAudio;
    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            _retval: Option<&mut Option<V8Value>>,
            _exception: Option<&mut CefString>,
        ) -> ::std::os::raw::c_int {
            let level = arguments.and_then(|a| a.first()).and_then(|a| a.as_ref()).map_or(0, |v| v.int_value());
            let Some(frame) = v8_context_get_current_context().and_then(|c| c.frame()) else { return 1 };
            let Some(mut message) = process_message_create(Some(&CefString::from(AUDIO_TO_BROWSER))) else { return 1 };
            if let Some(args) = message.argument_list() {
                args.set_int(0, level);
            }
            frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            1
        }
    }
}
