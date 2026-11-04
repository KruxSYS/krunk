//! Getting uBO scriptlets into every frame before the page's own scripts.
//!
//! The browser process compiles the lists' scriptlet filters into a small
//! engine (`lb_core::blocker::ScriptletEngine`) and hands its serialized
//! bytes to the renderers. A renderer needs them synchronously, when a
//! frame's script context is created, so they travel in each browser's
//! `extra_info` (delivered with the browser to every renderer process that
//! hosts it, before any of its documents run). After a list update or an
//! allowlist change the browser sends them again to every frame, and a
//! renderer that started with an older copy says so (`lb.sv`) and gets the
//! current one.

use super::BROWSERS;
use cef::*;
use std::cell::RefCell;
use std::sync::Arc;

/// Browser to renderer: `[version, allowlist, enabled, bytes?]`.
pub(super) const TO_RENDERER: &str = "lb.scriptlets";
/// Renderer to browser: `[version it has]`.
pub(super) const HELLO: &str = "lb.sv";

const KEY_VERSION: &str = "slVersion";
const KEY_ALLOW: &str = "slAllow";
const KEY_ON: &str = "slOn";
const KEY_BYTES: &str = "slBytes";

/// What the renderers should use.
#[derive(Default, Clone)]
struct Config {
    bytes: Option<Arc<Vec<u8>>>,
    /// Identifies `bytes` (the engine's key in hex), empty without one.
    version: String,
    /// Allowlisted sites (no scriptlets there), one host per line.
    allow: String,
    enabled: bool,
}

thread_local! {
    static CONFIG: RefCell<Config> = RefCell::new(Config::default());
}

/// Sets the scriptlet engine, allowlist and on/off state, and sends them to
/// every live browser's frames (UI thread). `bytes: None` keeps the engine.
pub fn set_scriptlets(bytes: Option<(u64, Vec<u8>)>, allowlist: Vec<String>, enabled: bool) {
    let config = CONFIG.with(|c| {
        let mut c = c.borrow_mut();
        let new_engine = bytes.is_some();
        if let Some((key, b)) = bytes {
            c.version = format!("{key:016x}");
            c.bytes = Some(Arc::new(b));
        }
        c.allow = allowlist.join("\n");
        c.enabled = enabled;
        (c.clone(), new_engine)
    });
    let (config, new_engine) = config;
    let browsers: Vec<Browser> = BROWSERS.with(|b| b.borrow().values().cloned().collect());
    for b in browsers {
        let mut ids = CefStringList::new();
        b.frame_identifiers(Some(&mut ids));
        for id in ids {
            if let Some(frame) = b.frame_by_identifier(Some(&CefString::from(id.as_str()))) {
                send(&frame, &config, new_engine);
            }
        }
    }
}

fn send(frame: &Frame, config: &Config, with_bytes: bool) {
    let Some(mut message) = process_message_create(Some(&CefString::from(TO_RENDERER))) else { return };
    if let Some(args) = message.argument_list() {
        args.set_string(0, Some(&CefString::from(config.version.as_str())));
        args.set_string(1, Some(&CefString::from(config.allow.as_str())));
        args.set_bool(2, i32::from(config.enabled));
        if let (true, Some(bytes)) = (with_bytes, &config.bytes) {
            if let Some(mut bin) = binary_value_create(Some(bytes)) {
                args.set_binary(3, Some(&mut bin));
            }
        }
    }
    frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
}

/// Adds the current scriptlet engine to a new browser's `extra_info`.
pub(super) fn add_to_extra_info(d: &mut DictionaryValue) {
    CONFIG.with(|c| {
        let c = c.borrow();
        d.set_string(Some(&CefString::from(KEY_VERSION)), Some(&CefString::from(c.version.as_str())));
        d.set_string(Some(&CefString::from(KEY_ALLOW)), Some(&CefString::from(c.allow.as_str())));
        d.set_bool(Some(&CefString::from(KEY_ON)), i32::from(c.enabled));
        if let Some(bytes) = &c.bytes {
            if let Some(mut bin) = binary_value_create(Some(bytes)) {
                d.set_binary(Some(&CefString::from(KEY_BYTES)), Some(&mut bin));
            }
        }
    });
}

/// A renderer reported the version it has: send the current one if newer.
pub(super) fn hello(frame: &Frame, have: &str) {
    let config = CONFIG.with(|c| c.borrow().clone());
    if config.bytes.is_some() && have != config.version {
        send(frame, &config, true);
    }
}

/// Renderer side: what a browser's `extra_info` or a `lb.scriptlets`
/// message says.
pub(super) struct Received {
    pub version: String,
    pub allow: String,
    pub enabled: bool,
    pub bytes: Option<Vec<u8>>,
}

pub(super) fn from_extra_info(d: &DictionaryValue) -> Received {
    let s = |k: &str| CefString::from(&d.string(Some(&CefString::from(k)))).to_string();
    Received {
        version: s(KEY_VERSION),
        allow: s(KEY_ALLOW),
        enabled: d.bool(Some(&CefString::from(KEY_ON))) == 1,
        bytes: d.binary(Some(&CefString::from(KEY_BYTES))).map(|b| super::client::binary_bytes(&b)),
    }
}

pub(super) fn from_message(args: &ListValue) -> Received {
    Received {
        version: CefString::from(&args.string(0)).to_string(),
        allow: CefString::from(&args.string(1)).to_string(),
        enabled: args.bool(2) == 1,
        bytes: (args.size() > 3).then(|| args.binary(3)).flatten().map(|b| super::client::binary_bytes(&b)),
    }
}
