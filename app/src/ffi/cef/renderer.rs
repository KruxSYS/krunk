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

use super::scriptlets;
use super::view::Scripts;
use cef::*;
use lb_core::assets;
use lb_core::blocker::{Allowlist, ScriptletEngine};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Process message names.
pub(super) const TO_BROWSER: &str = "lb.msg";
pub(super) const TO_PAGE: &str = "lb.evt";
/// A frame started or stopped playing audio (`bool`).
pub(super) const AUDIO_TO_BROWSER: &str = "lb.audio";
/// Pointer lock, frame to browser (`LockMessage` JSON) and back (events).
pub(super) const POINTER_TO_BROWSER: &str = "lb.plock";
pub(super) const POINTER_TO_FRAME: &str = "lb.plock.evt";
const MAX_POINTER_MESSAGE: usize = lb_core::pointer_lock::MAX_MESSAGE_BYTES;
/// Bigger messages from a page are dropped (the largest legitimate ones are
/// bookmark imports, capped by the UI at a few MB).
const MAX_MESSAGE: usize = 8 * 1024 * 1024;

/// Installs `window.__lbHost` and returns the dispatch function for
/// browser-to-page messages. Built-ins are captured before page scripts run.
const SHIM: &str = r#"(function (send) {
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
  Object.defineProperty(window, '__lbHost', { value: host, writable: false, configurable: false, enumerable: false });
  return function (json) {
    var data;
    try { data = parse(json); } catch (e) { return; }
    var copy = listeners.slice(), ev = { data: data };
    for (var i = 0; i < copy.length; i++) {
      try { copy[i](ev); } catch (e) { setTimeout(function () { throw e; }); }
    }
  };
})"#;

/// Reports whether this frame plays audio: unmuted media elements that
/// are playing (in the document or not) and running Web Audio contexts.
/// The native `report` function stays in the closure.
const AUDIO_TRACKER: &str = r#"(function (report) {
  'use strict';
  var playing = new Set(), contexts = new Set(), last = false, hooked = new WeakSet();
  function update() {
    var now = playing.size > 0 || contexts.size > 0;
    if (now !== last) { last = now; report(now); }
  }
  function track(el) {
    if (hooked.has(el)) return;
    hooked.add(el);
    var on = function () { if (!el.paused && !el.muted && el.volume > 0) playing.add(el); else playing.delete(el); update(); };
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
    /// Content scripts per browser identifier (from the browser's extra info).
    static SCRIPTS: RefCell<HashMap<i32, Scripts>> = RefCell::new(HashMap::new());
    /// The dispatch function of each main frame, by frame identifier.
    static DISPATCH: RefCell<HashMap<String, V8Value>> = RefCell::new(HashMap::new());
    /// The pointer-lock shim of each frame, by frame identifier.
    static POINTER: RefCell<HashMap<String, V8Value>> = RefCell::new(HashMap::new());
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
            SCRIPTS.with(|s| s.borrow_mut().insert(browser.identifier(), scripts));
            let received = scriptlets::from_extra_info(info);
            SCRIPTLETS.with(|s| s.borrow_mut().apply(received));
        }

        fn on_browser_destroyed(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                SCRIPTS.with(|s| s.borrow_mut().remove(&b.identifier()));
            }
        }

        fn on_context_created(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let (Some(browser), Some(frame), Some(context)) = (browser, frame, context) else { return };
            // First, before anything of the page runs: uBO's scriptlets.
            run_scriptlets(browser, frame, context);
            let scripts = SCRIPTS.with(|s| s.borrow().get(&browser.identifier()).copied()).unwrap_or_default();
            context_created(scripts, frame, context);
        }

        fn on_context_released(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _context: Option<&mut V8Context>,
        ) {
            if let Some(frame) = frame {
                let id = CefString::from(&frame.identifier()).to_string();
                DISPATCH.with(|d| d.borrow_mut().remove(&id));
                POINTER.with(|d| d.borrow_mut().remove(&id));
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

fn context_created(scripts: Scripts, frame: &mut Frame, context: &mut V8Context) {
    let main = frame.is_main() == 1;
    // Navigator properties matching the privacy headers: every frame.
    if scripts.gpc {
        eval(context, assets::inject_script("gpc.js"));
    }
    if scripts.dnt {
        eval(context, assets::inject_script("dnt.js"));
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
    install_bridge(frame, context);
    if scripts.login {
        eval(context, assets::inject_script("login.js"));
    }
    if !main {
        return;
    }
    if scripts.page_tokens {
        eval(context, assets::inject_script("page-tokens.js"));
    }
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

fn install_bridge(frame: &mut Frame, context: &mut V8Context) {
    let Some(shim) = eval(context, SHIM) else { return };
    let mut handler = SendToBrowser::new();
    let Some(send) = v8_value_create_function(Some(&CefString::from("send")), Some(&mut handler)) else { return };
    let Some(dispatch) = shim.execute_function(None, Some(&[Some(send)])) else { return };
    if dispatch.is_function() == 1 {
        let id = CefString::from(&frame.identifier()).to_string();
        DISPATCH.with(|d| d.borrow_mut().insert(id, dispatch));
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
        POINTER.with(|d| d.borrow_mut().insert(id, dispatch));
    }
}

/// Browser to page: calls the frame's dispatch function with the JSON text.
fn deliver(frame: &mut Frame, json: &CefString) {
    deliver_to(&DISPATCH, frame, json);
}

fn deliver_to(
    table: &'static std::thread::LocalKey<RefCell<HashMap<String, V8Value>>>,
    frame: &mut Frame,
    json: &CefString,
) {
    let id = CefString::from(&frame.identifier()).to_string();
    let Some(dispatch) = table.with(|d| d.borrow().get(&id).cloned()) else { return };
    let Some(context) = frame.v8_context() else { return };
    if context.enter() != 1 {
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
            let playing = arguments.and_then(|a| a.first()).and_then(|a| a.as_ref()).is_some_and(|v| v.bool_value() == 1);
            let Some(frame) = v8_context_get_current_context().and_then(|c| c.frame()) else { return 1 };
            let Some(mut message) = process_message_create(Some(&CefString::from(AUDIO_TO_BROWSER))) else { return 1 };
            if let Some(args) = message.argument_list() {
                args.set_bool(0, i32::from(playing));
            }
            frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            1
        }
    }
}
