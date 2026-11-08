//! "Send client hints" off in every renderer process (Krunk 0.7.1, R12).
//!
//! A renderer removes `navigator.userAgentData` from each document as it is created, so it must
//! know the setting synchronously. Three ways bring it there, each with a generation
//! (`lb_core::headers::HintsState`; the renderer keeps the newest):
//! - the renderer's command line (`--krunk-hints`), added when the engine launches it: a
//!   process started after a change is right from its first document, also when it hosts a
//!   frame of a tab that was opened before the change (a cross-site iframe);
//! - each browser's `extra_info` (tabs made after a change);
//! - a `lb.hints` message to every frame after a change, and to a renderer whose first
//!   document reports an older generation (`lb.sv`'s second argument): a process that started
//!   before the change and is used after it (Chromium keeps a spare one).

use super::BROWSERS;
use cef::*;
use lb_core::headers::HintsState;
use std::sync::atomic::{AtomicU64, Ordering};

/// Browser to renderer: `[generation (int), off (bool)]`.
pub(super) const TO_RENDERER: &str = "lb.hints";
/// The renderer switch (`<generation>:<0|1>`).
pub(super) const SWITCH: &str = "krunk-hints";
const KEY_OFF: &str = "nohints";
const KEY_GEN: &str = "nohintsGen";

/// The browser process's current state (`HintsState::pack`); read on the engine's launcher
/// thread too.
static STATE: AtomicU64 = AtomicU64::new(0);

pub(super) fn current() -> HintsState {
    HintsState::unpack(STATE.load(Ordering::Acquire))
}

/// Sets "Send client hints" off or on (UI thread; at start before any page, then on every
/// change of the setting) and tells every frame of every browser.
pub fn set_no_hints(off: bool) {
    let before = current();
    let now = before.set(off);
    if now == before {
        return;
    }
    STATE.store(now.pack(), Ordering::Release);
    let browsers: Vec<Browser> = BROWSERS.with(|b| b.borrow().values().cloned().collect());
    for b in browsers {
        let mut ids = CefStringList::new();
        b.frame_identifiers(Some(&mut ids));
        for id in ids {
            if let Some(frame) = b.frame_by_identifier(Some(&CefString::from(id.as_str()))) {
                send(&frame, now);
            }
        }
    }
}

fn send(frame: &Frame, state: HintsState) {
    let Some(mut message) = process_message_create(Some(&CefString::from(TO_RENDERER))) else { return };
    if let Some(args) = message.argument_list() {
        args.set_int(0, i32::try_from(state.generation).unwrap_or(i32::MAX));
        args.set_bool(1, i32::from(state.off));
    }
    frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
}

/// A renderer's first document of a browser reported the generation it has: send the current
/// state when it is newer.
pub(super) fn hello(frame: &Frame, have: u64) {
    let now = current();
    if now.generation > have {
        send(frame, now);
    }
}

/// Adds the current state to a new browser's `extra_info`.
pub(super) fn add_to_extra_info(d: &mut DictionaryValue) {
    let now = current();
    d.set_bool(Some(&CefString::from(KEY_OFF)), i32::from(now.off));
    d.set_int(Some(&CefString::from(KEY_GEN)), i32::try_from(now.generation).unwrap_or(i32::MAX));
}

/// Adds the current state to a renderer's command line as the engine launches it.
pub(super) fn add_to_child(command_line: &mut CommandLine) {
    let kind = CefString::from(&command_line.switch_value(Some(&CefString::from("type")))).to_string();
    if kind == "renderer" {
        let value = current().to_switch();
        command_line.append_switch_with_value(Some(&CefString::from(SWITCH)), Some(&CefString::from(value.as_str())));
    }
}

/// Renderer side: the state in a browser's `extra_info`.
pub(super) fn from_extra_info(d: &DictionaryValue) -> HintsState {
    HintsState {
        generation: u64::try_from(d.int(Some(&CefString::from(KEY_GEN)))).unwrap_or(0),
        off: d.bool(Some(&CefString::from(KEY_OFF))) == 1,
    }
}

/// Renderer side: the state in a `lb.hints` message.
pub(super) fn from_message(args: &ListValue) -> HintsState {
    HintsState { generation: u64::try_from(args.int(0)).unwrap_or(0), off: args.bool(1) == 1 }
}

/// Renderer side: the state this process was launched with.
pub(super) fn from_own_command_line() -> HintsState {
    command_line_get_global()
        .map(|c| CefString::from(&c.switch_value(Some(&CefString::from(SWITCH)))).to_string())
        .and_then(|v| HintsState::from_switch(&v))
        .unwrap_or_default()
}
