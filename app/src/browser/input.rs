//! Engine input for the agent and macros (0.12.0, WP-F): key and mouse events sent to a tab's
//! page with CDP `Input.*` (engine-level, never OS-wide injection). Held keys repeat at the
//! system's keyboard delay and rate until released; every Stop releases all of them.
//!
//! Events go to the tab's renderer whether or not its window is in front: the tab gets focus
//! emulation (`Emulation.setFocusEmulationEnabled`) while Krunk drives it, so the focused
//! element keeps receiving keys; it ends with [`Browser::input_release_all`].

use super::Browser;
use crate::ffi::cef::{post_ui_delayed, View};
use lb_core::macros::{self, KeySpec, Mods};
use lb_core::tabs::TabId;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// What a key event does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyAction {
    Press,
    Down { repeat: bool },
    Up,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    /// "left", "middle", "right" (also the CDP names).
    pub(super) fn parse(s: &str) -> Option<MouseButton> {
        match s {
            "left" => Some(MouseButton::Left),
            "middle" => Some(MouseButton::Middle),
            "right" => Some(MouseButton::Right),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Middle => "middle",
            MouseButton::Right => "right",
        }
    }

    /// The CDP `buttons` bit: left 1, right 2, middle 4.
    fn bit(self) -> u8 {
        match self {
            MouseButton::Left => 1,
            MouseButton::Right => 2,
            MouseButton::Middle => 4,
        }
    }
}

/// The end of an input call: Ok once the engine took the events, or why not.
pub(super) type InputDone = Box<dyn FnOnce(Result<(), String>)>;

struct HeldKey {
    spec: KeySpec,
    mods: Mods,
    /// Identifies this hold: a repeat timer of an earlier hold of the same key stops.
    hold: u64,
}

#[derive(Default)]
struct TabInput {
    /// Held keys by DOM code.
    keys: HashMap<String, HeldKey>,
    /// Held mouse buttons (CDP bits).
    buttons: u8,
    /// The last mouse position (CSS px).
    x: f64,
    y: f64,
    focus_emulated: bool,
}

impl TabInput {
    /// The modifiers the held modifier keys add.
    fn held_mods(&self) -> Mods {
        self.keys.values().fold(Mods::default(), |m, k| m.union(Mods::of_key(k.spec.vk)))
    }
}

#[derive(Default)]
pub(super) struct InputHost {
    tabs: RefCell<HashMap<TabId, TabInput>>,
    holds: Cell<u64>,
    /// Bumped by every release: typing started before it stops.
    epoch: Cell<u64>,
}

/// One step of typing text.
enum TypeStep {
    Key(Mods, KeySpec),
    Text(String),
}

fn type_steps(text: &str) -> Vec<TypeStep> {
    let mut steps = Vec::new();
    let mut pending = String::new();
    for ch in text.chars() {
        match macros::key_for_char(ch).filter(|_| ch != '\r') {
            Some((mods, spec)) => {
                if !pending.is_empty() {
                    steps.push(TypeStep::Text(std::mem::take(&mut pending)));
                }
                steps.push(TypeStep::Key(mods, spec));
            }
            // "\r\n" types one Enter.
            None if ch == '\r' => {}
            None => pending.push(ch),
        }
    }
    if !pending.is_empty() {
        steps.push(TypeStep::Text(pending));
    }
    steps
}

/// `Input.dispatchKeyEvent` parameters. A key-down that types (a character key without Ctrl,
/// Alt or Meta) is a `keyDown` with its text; any other is a `rawKeyDown`.
fn key_params(spec: &KeySpec, mods: Mods, down: bool, auto_repeat: bool) -> Value {
    let typed = down && !(mods.ctrl || mods.alt || mods.meta) && spec.text.is_some();
    let kind = if !down {
        "keyUp"
    } else if typed {
        "keyDown"
    } else {
        "rawKeyDown"
    };
    let keypad = spec.code.starts_with("Numpad");
    let location = if keypad {
        3
    } else if spec.is_modifier() && spec.code.ends_with("Left") {
        1
    } else if spec.is_modifier() && spec.code.ends_with("Right") {
        2
    } else {
        0
    };
    let mut p = json!({
        "type": kind,
        "modifiers": mods.cdp_bits(),
        "windowsVirtualKeyCode": spec.vk,
        "nativeVirtualKeyCode": spec.vk,
        "code": spec.code,
        "key": spec.key,
        "autoRepeat": auto_repeat,
        "isKeypad": keypad,
        "location": location,
    });
    if typed {
        let text = spec.text.clone().unwrap_or_default();
        p["text"] = json!(text);
        p["unmodifiedText"] = json!(text);
    }
    p
}

impl Browser {
    /// The tab's view, with focus emulation on (once per drive).
    fn input_view(&self, tab: TabId) -> Result<View, String> {
        let view = self.tab_view(tab).ok_or("The tab is gone")?;
        let first = {
            let mut tabs = self.input.tabs.borrow_mut();
            let t = tabs.entry(tab).or_default();
            !std::mem::replace(&mut t.focus_emulated, true)
        };
        if first {
            view.devtools_method("Emulation.setFocusEmulationEnabled", &json!({ "enabled": true }));
        }
        Ok(view)
    }

    /// Sends one key event; `done` (if any) gets the engine's answer. (Engine key events
    /// don't pass Krunk's key hook, so they never trigger shortcuts or macro hotkeys: the
    /// 0.12.0 spike.)
    fn send_key(&self, view: &View, params: Value, done: Option<InputDone>) {
        match done {
            Some(d) => view.devtools_call("Input.dispatchKeyEvent", &params, move |r| d(r.map(|_| ()))),
            None => view.devtools_method("Input.dispatchKeyEvent", &params),
        }
    }

    /// A key event; `done` runs once the engine took the event (for a held key: its
    /// first key-down). Modifier keys held down add their modifier to every later event of the
    /// tab, like on a keyboard.
    pub(super) fn input_key_then(&self, tab: TabId, key: &KeySpec, mods: Mods, action: KeyAction, done: InputDone) {
        let view = match self.input_view(tab) {
            Ok(v) => v,
            Err(e) => return done(Err(e)),
        };
        let held = self.input.tabs.borrow().get(&tab).map(|t| t.held_mods()).unwrap_or_default();
        let own = Mods::of_key(key.vk);
        match action {
            KeyAction::Press => {
                let m = mods.union(held);
                self.send_key(&view, key_params(key, m.union(own), true, false), None);
                self.send_key(&view, key_params(key, m, false, false), Some(done));
            }
            KeyAction::Down { repeat } => {
                let hold = self.input.holds.get() + 1;
                let fresh = {
                    let mut tabs = self.input.tabs.borrow_mut();
                    let t = tabs.entry(tab).or_default();
                    if t.keys.contains_key(&key.code) {
                        false
                    } else {
                        t.keys.insert(key.code.clone(), HeldKey { spec: key.clone(), mods, hold });
                        true
                    }
                };
                if !fresh {
                    return done(Ok(()));
                }
                self.input.holds.set(hold);
                let m = mods.union(held).union(own);
                self.send_key(&view, key_params(key, m, true, false), Some(done));
                if repeat {
                    let (delay_setting, speed_setting) = crate::ffi::keyboard::repeat_settings();
                    let (delay, interval) = macros::repeat_timing(delay_setting, speed_setting);
                    self.schedule_repeat(tab, key.code.clone(), hold, delay, interval);
                }
            }
            KeyAction::Up => {
                let removed = self.input.tabs.borrow_mut().get_mut(&tab).and_then(|t| t.keys.remove(&key.code));
                let held = self.input.tabs.borrow().get(&tab).map(|t| t.held_mods()).unwrap_or_default();
                let m = removed.map_or(mods, |h| h.mods).union(mods).union(held);
                self.send_key(&view, key_params(key, m, false, false), Some(done));
            }
        }
    }

    fn schedule_repeat(&self, tab: TabId, code: String, hold: u64, after: u64, interval: u64) {
        let weak = self.me.clone();
        post_ui_delayed(after as i64, move || {
            let Some(b) = weak.upgrade() else { return };
            let params = {
                let tabs = b.input.tabs.borrow();
                let Some(t) = tabs.get(&tab) else { return };
                let Some(k) = t.keys.get(&code).filter(|k| k.hold == hold) else { return };
                key_params(&k.spec, k.mods.union(t.held_mods()), true, true)
            };
            let Some(view) = b.tab_view(tab) else { return };
            b.send_key(&view, params, None);
            b.schedule_repeat(tab, code, hold, interval, interval);
        });
    }

    fn mouse_params(&self, tab: TabId, kind: &str, x: f64, y: f64, button: Option<MouseButton>, count: u8) -> Value {
        let (buttons, mods) =
            self.input.tabs.borrow().get(&tab).map(|t| (t.buttons, t.held_mods())).unwrap_or_default();
        // A move while a button is held drags with it.
        let button = button.or_else(|| {
            [MouseButton::Left, MouseButton::Right, MouseButton::Middle].into_iter().find(|b| buttons & b.bit() != 0)
        });
        json!({
            "type": kind,
            "x": x,
            "y": y,
            "button": button.map_or("none", MouseButton::name),
            "buttons": buttons,
            "clickCount": count,
            "modifiers": mods.cdp_bits(),
        })
    }

    fn set_mouse(&self, tab: TabId, x: f64, y: f64, button: Option<(MouseButton, bool)>) {
        let mut tabs = self.input.tabs.borrow_mut();
        let t = tabs.entry(tab).or_default();
        t.x = x;
        t.y = y;
        if let Some((b, down)) = button {
            if down {
                t.buttons |= b.bit();
            } else {
                t.buttons &= !b.bit();
            }
        }
    }

    /// A click: a move there, then `count` presses and releases (1 = click,
    /// 2 = double click); `done` once the engine took the last release.
    pub(super) fn input_click_then(&self, tab: TabId, x: f64, y: f64, button: MouseButton, count: u8, done: InputDone) {
        let view = match self.input_view(tab) {
            Ok(v) => v,
            Err(e) => return done(Err(e)),
        };
        self.set_mouse(tab, x, y, None);
        view.devtools_method("Input.dispatchMouseEvent", &self.mouse_params(tab, "mouseMoved", x, y, None, 0));
        let count = count.clamp(1, 3);
        let mut done = Some(done);
        for n in 1..=count {
            self.set_mouse(tab, x, y, Some((button, true)));
            view.devtools_method(
                "Input.dispatchMouseEvent",
                &self.mouse_params(tab, "mousePressed", x, y, Some(button), n),
            );
            self.set_mouse(tab, x, y, Some((button, false)));
            let params = self.mouse_params(tab, "mouseReleased", x, y, Some(button), n);
            match done.take().filter(|_| n == count) {
                Some(d) => view.devtools_call("Input.dispatchMouseEvent", &params, move |r| d(r.map(|_| ()))),
                None => view.devtools_method("Input.dispatchMouseEvent", &params),
            }
        }
    }

    /// Moves the mouse (dragging any held button).
    pub(super) fn input_mouse_move_then(&self, tab: TabId, x: f64, y: f64, done: InputDone) {
        let view = match self.input_view(tab) {
            Ok(v) => v,
            Err(e) => return done(Err(e)),
        };
        self.set_mouse(tab, x, y, None);
        let params = self.mouse_params(tab, "mouseMoved", x, y, None, 0);
        view.devtools_call("Input.dispatchMouseEvent", &params, move |r| done(r.map(|_| ())));
    }

    /// Presses (`down`) or releases a mouse button at a point; a pressed button stays held
    /// until released, Stop, or the end of the agent's task.
    pub(super) fn input_mouse_button(
        &self,
        tab: TabId,
        x: f64,
        y: f64,
        button: MouseButton,
        down: bool,
        done: InputDone,
    ) {
        let view = match self.input_view(tab) {
            Ok(v) => v,
            Err(e) => return done(Err(e)),
        };
        let (moved, was_down) = self
            .input
            .tabs
            .borrow()
            .get(&tab)
            .map_or((true, false), |t| (t.x != x || t.y != y, t.buttons & button.bit() != 0));
        if down && was_down {
            return done(Ok(()));
        }
        if moved {
            self.set_mouse(tab, x, y, None);
            view.devtools_method("Input.dispatchMouseEvent", &self.mouse_params(tab, "mouseMoved", x, y, None, 0));
        }
        self.set_mouse(tab, x, y, Some((button, down)));
        let kind = if down { "mousePressed" } else { "mouseReleased" };
        let params = self.mouse_params(tab, kind, x, y, Some(button), 1);
        view.devtools_call("Input.dispatchMouseEvent", &params, move |r| done(r.map(|_| ())));
    }

    /// Typing: one key press per character on a US layout ("\n" is Enter),
    /// other characters as `Input.insertText`; stops at a release (Stop) with an error.
    pub(super) fn input_type_then(&self, tab: TabId, text: &str, done: InputDone) {
        if let Err(e) = self.input_view(tab) {
            return done(Err(e));
        }
        let steps = Rc::new(type_steps(text));
        self.type_step(tab, steps, 0, self.input.epoch.get(), done);
    }

    fn type_step(&self, tab: TabId, steps: Rc<Vec<TypeStep>>, i: usize, epoch: u64, done: InputDone) {
        if self.input.epoch.get() != epoch {
            return done(Err("stopped".into()));
        }
        let Some(step) = steps.get(i) else { return done(Ok(())) };
        let Some(view) = self.tab_view(tab) else { return done(Err("The tab is gone".into())) };
        let weak = self.me.clone();
        let steps2 = steps.clone();
        let next: InputDone = Box::new(move |r| match (r, weak.upgrade()) {
            (Ok(()), Some(b)) => b.type_step(tab, steps2, i + 1, epoch, done),
            (Err(e), _) => done(Err(e)),
            (Ok(()), None) => done(Err("Krunk is closing".into())),
        });
        match step {
            TypeStep::Key(mods, spec) => {
                self.send_key(&view, key_params(spec, *mods, true, false), None);
                self.send_key(&view, key_params(spec, *mods, false, false), Some(next));
            }
            TypeStep::Text(t) => {
                view.devtools_call("Input.insertText", &json!({ "text": t }), move |r| next(r.map(|_| ())))
            }
        }
    }

    /// Releases every key and button held in any tab (key-ups and button releases go to their
    /// tabs), stops the repeats and any typing, and ends focus emulation.
    pub(super) fn input_release_all(&self) {
        self.input.epoch.set(self.input.epoch.get() + 1);
        let tabs: Vec<(TabId, TabInput)> = self.input.tabs.borrow_mut().drain().collect();
        for (tab, t) in tabs {
            let Some(view) = self.tab_view(tab) else { continue };
            if !t.keys.is_empty() || t.buttons != 0 {
                crate::log::info!("tab {tab}: input released ({} keys, buttons {:#x})", t.keys.len(), t.buttons);
            }
            let mods = t.held_mods();
            for k in t.keys.values() {
                // The engine drops key-ups after a key-down the browser handled itself (the
                // macro's hotkey, Ctrl+Shift+. pressed in this page) until the next key-down: one
                // more auto-repeat key-down of the held key (no character) comes first, so the
                // page always sees the key go up.
                let mut repeat = key_params(&k.spec, mods, true, true);
                repeat["type"] = Value::from("rawKeyDown");
                if let Some(o) = repeat.as_object_mut() {
                    o.remove("text");
                    o.remove("unmodifiedText");
                }
                view.devtools_method("Input.dispatchKeyEvent", &repeat);
                view.devtools_method("Input.dispatchKeyEvent", &key_params(&k.spec, mods, false, false));
            }
            for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                if t.buttons & b.bit() != 0 {
                    let params = json!({ "type": "mouseReleased", "x": t.x, "y": t.y, "button": b.name(),
                                         "buttons": 0, "clickCount": 1 });
                    view.devtools_method("Input.dispatchMouseEvent", &params);
                }
            }
            if t.focus_emulated {
                view.devtools_method("Emulation.setFocusEmulationEnabled", &json!({ "enabled": false }));
            }
        }
    }

    pub(super) fn input_tab_gone(&self, tab: TabId) {
        self.input.tabs.borrow_mut().remove(&tab);
    }
}
