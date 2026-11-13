//! Macros (0.12.0, WP-F): the hidden runner view, its API calls, the one running macro, hotkeys
//! (caught in tab views before the page), Stop and the toolbar indicator.
//!
//! A macro's code runs in the runner (`https://shell.krunk.invalid/runner.html`, a hidden
//! `Role::Popup` view owned by the app window, created on first use and closed 5 minutes after
//! the last run). The host starts it with DevTools `Runtime.evaluate` (not subject to the
//! page's CSP); each API call comes back as a `macroCall` that this file carries out on the
//! macro's target: the active tab of the window it started in, looked up at each call. Stop
//! terminates the runner's script, reloads the runner and releases every held key and button.

use super::input::{InputDone, KeyAction, MouseButton};
use super::Browser;
use crate::ffi::cef::{post_ui_delayed, KeyPress, Menu, Role, Scripts, View, ViewOptions};
use crate::ffi::window::{Window, WindowEvents};
use crate::log::{info, warning};
use lb_core::agent::{ScriptCard, ScriptKind};
use lb_core::assets;
use lb_core::macros::{self, Chord, Mods};
use lb_core::pages::Page;
use lb_core::protocol::{Channel, Event, Request};
use lb_core::scripts_store::{self, MacroRow};
use lb_core::storage::now_ms;
use lb_core::tabs::{Position, TabId};
use lb_core::windows::WindowId;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

pub(super) const RUNNER_URL: &str = "https://shell.krunk.invalid/runner.html";
/// The runner closes this long after the last macro ended.
const RUNNER_IDLE_MS: i64 = 5 * 60 * 1000;
/// `wait` and `key.hold`: at most an hour.
const MAX_WAIT_MS: u64 = 3_600_000;
/// `page.eval` results (JSON) above this are refused.
const MAX_EVAL_RESULT: usize = 100 * 1024;
const MAX_NAME: usize = 200;
const MAX_SOURCE: usize = 200 * 1024;
/// Lines of `log(…)` kept for the run's result.
const MAX_LOG_LINES: usize = 50;
const MAX_LOG_LINE: usize = 500;
/// True when a password field holds a value (agent-started macros don't run `page.eval` then).
const PASSWORD_FILLED: &str = "!!document.querySelector('input[type=password]')&&[...document.querySelectorAll('input[type=password]')].some(e=>e.value)";

struct RunnerEvents;
impl WindowEvents for RunnerEvents {}

struct Runner {
    window: Window,
    view: View,
    /// The document sent `ready` (after a load or a Stop's reload).
    ready: bool,
}

type MacroDoneFn = Box<dyn FnOnce(Result<String, String>)>;
/// Answers one `macroCall` (dropped once its run is over).
type CallReply = Rc<dyn Fn(Result<Value, String>)>;

struct Running {
    id: String,
    name: String,
    run_id: u64,
    by_agent: bool,
    started: Instant,
    /// The window it started in: key and mouse calls go to that window's active tab.
    win: Option<WindowId>,
    /// The code, until the runner is ready to take it.
    source: Option<String>,
    done: Option<MacroDoneFn>,
    log: Vec<String>,
}

#[derive(Default)]
pub(super) struct MacroHost {
    runner: RefCell<Option<Runner>>,
    running: RefCell<Option<Running>>,
    runs: Cell<u64>,
    /// Pending `wait` / `key.hold` timers (timer id → the call's request id).
    waits: RefCell<HashMap<u64, Option<u32>>>,
    timers: Cell<u64>,
    /// Bumped at each macro end: the idle close only happens if nothing ran since.
    idle: Cell<u64>,
    /// krunk://scripts tab recording a hotkey, and the macro being edited there.
    recording: RefCell<Option<(TabId, Option<String>)>>,
}

/// A whole number argument in `lo..=hi`.
fn arg_u64(args: &Value, name: &str, lo: u64, hi: u64) -> Result<u64, String> {
    let n = args.get(name).and_then(Value::as_f64).ok_or_else(|| format!("{name} must be a number"))?;
    if !n.is_finite() || n.fract() != 0.0 || n < lo as f64 || n > hi as f64 {
        return Err(format!("{name} must be a whole number from {lo} to {hi}"));
    }
    Ok(n as u64)
}

/// A viewport coordinate.
fn arg_coord(args: &Value, name: &str) -> Result<f64, String> {
    let n = args.get(name).and_then(Value::as_f64).ok_or_else(|| format!("{name} must be a number"))?;
    if !n.is_finite() || !(-100_000.0..=100_000.0).contains(&n) {
        return Err(format!("{name} is out of range"));
    }
    Ok(n)
}

/// A string argument of 1..=max characters.
fn arg_str<'a>(args: &'a Value, name: &str, max: usize) -> Result<&'a str, String> {
    let s = args.get(name).and_then(Value::as_str).ok_or_else(|| format!("{name} must be a string"))?;
    if s.is_empty() {
        return Err(format!("{name} is empty"));
    }
    if s.chars().count() > max {
        return Err(format!("{name} is longer than {max} characters"));
    }
    Ok(s)
}

fn arg_button(args: &Value) -> Result<MouseButton, String> {
    match args.get("button") {
        None | Some(Value::Null) => Ok(MouseButton::Left),
        Some(v) => v.as_str().and_then(MouseButton::parse).ok_or_else(|| "button must be left, middle or right".into()),
    }
}

/// An optional boolean (absent = `default`).
fn arg_bool(args: &Value, name: &str, default: bool) -> Result<bool, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("{name} must be true or false")),
    }
}

/// `key` (a combination like "Control+a") plus optional `mods: {ctrl, shift, alt, meta}`.
fn arg_keys(args: &Value) -> Result<(Mods, lb_core::macros::KeySpec), String> {
    let (mut mods, spec) = macros::parse_keys(arg_str(args, "key", 100)?)?;
    if let Some(m) = args.get("mods").filter(|m| !m.is_null()) {
        let obj = m.as_object().ok_or("mods must be an object like {ctrl: true}")?;
        for (k, v) in obj {
            let on = v.as_bool().ok_or("mods values must be true or false")?;
            match k.as_str() {
                "ctrl" => mods.ctrl |= on,
                "shift" => mods.shift |= on,
                "alt" => mods.alt |= on,
                "meta" => {}
                _ => return Err(format!("unknown modifier {k}")),
            }
        }
    }
    Ok((mods, spec))
}

/// A web address (http or https).
fn arg_url(args: &Value) -> Result<String, String> {
    let url = arg_str(args, "url", 8192)?.trim();
    let parsed = url::Url::parse(url).map_err(|_| "url is not a valid address".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Macros open only http and https addresses".into());
    }
    Ok(parsed.to_string())
}

/// `s` cut to `max` characters.
fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// The text of a DevTools exception ("ReferenceError: x is not defined").
fn exception_text(details: &Value) -> String {
    let desc = details["exception"]["description"].as_str().or_else(|| details["text"].as_str()).unwrap_or("error");
    cap_chars(desc.lines().next().unwrap_or(desc), 300)
}

impl Browser {
    fn macros_enabled(&self) -> bool {
        self.settings.borrow().macros.enabled
    }

    /// The runner document loaded.
    pub(super) fn macro_runner_ready(&self) {
        let ok = match self.macros.runner.borrow_mut().as_mut() {
            Some(r) => {
                r.ready = true;
                true
            }
            None => false,
        };
        if ok {
            self.macro_start_in_runner();
        }
    }

    fn create_runner(&self) -> Result<(), String> {
        let app = self.app().ok_or("the app window is missing")?;
        let window = Window::create_popup(app, Rc::new(RunnerEvents))?;
        let view = View::new(ViewOptions {
            role: Role::Popup,
            menu: Menu::None,
            private: false,
            scripts: Scripts::default(),
            background: 0,
            policy: None,
        });
        view.on_before_browse(|nav| assets::allow_navigation(Channel::Runner, "", &nav.url, true, nav.redirected));
        let weak = self.me.clone();
        view.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                b.on_ui_message(Channel::Runner, None, source, json);
            }
        });
        // No popups and no focus: it is never shown.
        view.on_open_tab(|_, _| {});
        view.on_set_focus(|_| false);
        let weak = self.me.clone();
        view.on_crashed(move |reason| {
            let Some(b) = weak.upgrade() else { return };
            warning!("the macro runner's renderer ended ({reason})");
            b.close_runner();
            let run = b.macros.running.borrow().as_ref().map(|r| r.run_id);
            if let Some(run) = run {
                b.macro_finish(run, Err("The macro runner stopped unexpectedly".into()));
            }
        });
        if let Err(e) = view.start_inactive(window.hwnd(), RUNNER_URL) {
            window.destroy();
            return Err(e);
        }
        view.set_bounds(0, 0, 2, 2);
        info!("macro runner started");
        *self.macros.runner.borrow_mut() = Some(Runner { window, view, ready: false });
        Ok(())
    }

    fn close_runner(&self) {
        if let Some(r) = self.macros.runner.borrow_mut().take() {
            r.view.close();
            r.window.destroy();
            info!("macro runner closed");
        }
    }

    fn runner_view(&self) -> Option<View> {
        self.macros.runner.borrow().as_ref().map(|r| r.view.clone())
    }

    /// Starts the running macro's code once the runner is ready.
    fn macro_start_in_runner(&self) {
        let ready = self.macros.runner.borrow().as_ref().is_some_and(|r| r.ready);
        if !ready {
            return;
        }
        let Some((run_id, source)) =
            self.macros.running.borrow_mut().as_mut().and_then(|r| r.source.take().map(|s| (r.run_id, s)))
        else {
            return;
        };
        let Some(view) = self.runner_view() else { return };
        // The code becomes the body of an async function with the API names in scope; one
        // line goes on top, which the runner and the syntax-error line below take off again.
        let expression = format!(
            "window.__krunkRunMacro(async function (api) {{ const {{key, mouse, type, wait, page, tabs, navigate, repeat, toast, log}} = api;\n{source}\n}}, {run_id})\n//# sourceURL=macro.js"
        );
        let weak = self.me.clone();
        view.devtools_call(
            "Runtime.evaluate",
            &json!({ "expression": expression, "returnByValue": true, "awaitPromise": false }),
            move |r| {
                let Some(b) = weak.upgrade() else { return };
                match r {
                    Ok(v) => {
                        if let Some(d) = v.get("exceptionDetails") {
                            let line = d["lineNumber"].as_u64().unwrap_or(0);
                            let mut msg = exception_text(d);
                            if line >= 1 {
                                msg = format!("{msg} (line {line})");
                            }
                            b.macro_finish(run_id, Err(msg));
                        }
                    }
                    Err(e) => b.macro_finish(run_id, Err(format!("The macro could not start: {e}"))),
                }
            },
        );
    }

    /// The running macro's run id.
    fn current_run(&self) -> Option<u64> {
        self.macros.running.borrow().as_ref().map(|r| r.run_id)
    }

    /// Answers a `macroCall` of run `run_id` (dropped when that run is over).
    fn runner_reply(&self, run_id: u64, req_id: Option<u32>, result: Result<Value, String>) {
        if self.current_run() != Some(run_id) {
            return;
        }
        let Some(view) = self.runner_view() else { return };
        let event = match result {
            Ok(v) => Event::ok(req_id, v),
            Err(e) => Event::err(req_id, e),
        };
        view.post_json(&event.to_json());
    }

    /// `macroCall` / `macroDone` from the runner.
    pub(super) fn macro_runner_request(&self, req_id: Option<u32>, request: Request) {
        let ready = self.macros.runner.borrow().as_ref().is_some_and(|r| r.ready);
        let run = self.current_run();
        match request {
            Request::MacroDone { ok, error } => {
                let Some(run) = run.filter(|_| ready) else { return };
                let result =
                    if ok { Ok(()) } else { Err(cap_chars(error.as_deref().unwrap_or("The macro failed"), 300)) };
                self.macro_finish(run, result.map(|()| String::new()));
            }
            Request::MacroCall { op, args } => {
                let Some(run) = run.filter(|_| ready) else {
                    // A call from a macro that already ended (or before Stop's reload).
                    if let Some(v) = self.runner_view() {
                        v.post_json(&Event::err(req_id, "stopped").to_json());
                    }
                    return;
                };
                let weak = self.me.clone();
                let reply = move |r: Result<Value, String>| {
                    if let Some(b) = weak.upgrade() {
                        b.runner_reply(run, req_id, r);
                    }
                };
                if let Err(e) = self.macro_call(run, req_id, &op, &args, Rc::new(reply.clone())) {
                    reply(Err(e));
                }
            }
            _ => {}
        }
    }

    /// The tab key and mouse calls go to: the active tab of the macro's window now.
    fn macro_target(&self) -> Result<TabId, String> {
        let win = self.macros.running.borrow().as_ref().and_then(|r| r.win);
        let ws = self.ws.borrow();
        win.and_then(|w| ws.model(w))
            .or_else(|| ws.current().and_then(|w| ws.model(w)))
            .and_then(macros::target_tab)
            .ok_or_else(|| "There is no web page to act on in this window".to_string())
    }

    /// The target tab, refused while it shows a page macros don't act on (Krunk's own pages,
    /// extension pages, DevTools: anything but http, https and file).
    fn macro_page_target(&self) -> Result<TabId, String> {
        let tab = self.macro_target()?;
        let url = self.tab(tab).map(|t| t.url.clone()).unwrap_or_default();
        lb_core::agent::pages::may_act(&url).map_err(str::to_string)?;
        let web = url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file:");
        if !web || Page::from_url(&url).is_some() {
            return Err("Macros don't act on Krunk's own pages".into());
        }
        Ok(tab)
    }

    /// Evaluates `expression` in tab `tab`'s main world (a promise is awaited).
    fn macro_eval(&self, tab: TabId, expression: &str, done: impl FnOnce(Result<Value, String>) + 'static) {
        let Some(view) = self.tab_view(tab) else { return done(Err("The tab is gone".into())) };
        let params =
            json!({ "expression": expression, "awaitPromise": true, "returnByValue": true, "userGesture": true });
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            done(match r {
                Ok(v) => match v.get("exceptionDetails") {
                    Some(d) => Err(exception_text(d)),
                    None => Ok(v["result"].get("value").cloned().unwrap_or(Value::Null)),
                },
                Err(e) => Err(e),
            })
        });
    }

    /// Starts a host timer for run `run` that calls `then` after `ms` unless Stop came first
    /// (then the call gets "stopped").
    fn macro_timer(&self, run: u64, req_id: Option<u32>, ms: u64, then: impl FnOnce(&Browser) + 'static) {
        let timer = self.macros.timers.get() + 1;
        self.macros.timers.set(timer);
        self.macros.waits.borrow_mut().insert(timer, req_id);
        let weak = self.me.clone();
        post_ui_delayed(ms as i64, move || {
            let Some(b) = weak.upgrade() else { return };
            if b.macros.waits.borrow_mut().remove(&timer).is_some() && b.current_run() == Some(run) {
                then(&b);
            }
        });
    }

    /// Carries out one API call; `reply` gets the result. An `Err` return is a bad argument.
    fn macro_call(
        &self,
        run: u64,
        req_id: Option<u32>,
        op: &str,
        args: &Value,
        reply: CallReply,
    ) -> Result<(), String> {
        let input_reply = |reply: &CallReply| -> InputDone {
            let reply = reply.clone();
            Box::new(move |r: Result<(), String>| reply(r.map(|()| Value::Null)))
        };
        match op {
            "key.down" | "key.up" | "key.press" => {
                let (mods, spec) = arg_keys(args)?;
                let action = match op {
                    "key.down" => KeyAction::Down { repeat: arg_bool(args, "repeat", true)? },
                    "key.up" => KeyAction::Up,
                    _ => KeyAction::Press,
                };
                let tab = self.macro_page_target()?;
                self.input_key_then(tab, &spec, mods, action, input_reply(&reply));
            }
            "key.hold" => {
                let (mods, spec) = arg_keys(args)?;
                let ms = arg_u64(args, "ms", 0, MAX_WAIT_MS)?;
                let tab = self.macro_page_target()?;
                let r = reply.clone();
                let spec2 = spec.clone();
                self.input_key_then(
                    tab,
                    &spec,
                    mods,
                    KeyAction::Down { repeat: true },
                    Box::new(move |res| {
                        if let Err(e) = res {
                            r(Err(e));
                        }
                    }),
                );
                let r = reply.clone();
                self.macro_timer(run, req_id, ms, move |b| {
                    b.input_key_then(
                        tab,
                        &spec2,
                        mods,
                        KeyAction::Up,
                        Box::new(move |res| r(res.map(|()| Value::Null))),
                    );
                });
            }
            "mouse.click" => {
                let button = arg_button(args)?;
                let count = match args.get("count") {
                    None | Some(Value::Null) => 1,
                    Some(_) => arg_u64(args, "count", 1, 3)? as u8,
                };
                let tab = self.macro_page_target()?;
                if args.get("selector").is_some_and(|s| !s.is_null()) {
                    let selector = arg_str(args, "selector", 2000)?;
                    let sel = serde_json::to_string(selector).unwrap_or_default();
                    let expr = format!(
                        "(() => {{ const e = document.querySelector({sel}); if (!e) return null; \
                         e.scrollIntoView({{block: 'center', inline: 'center', behavior: 'instant'}}); const r = e.getBoundingClientRect(); \
                         return {{x: r.x + r.width / 2, y: r.y + r.height / 2}}; }})()"
                    );
                    let weak = self.me.clone();
                    let r = reply.clone();
                    self.macro_eval(tab, &expr, move |res| {
                        let Some(b) = weak.upgrade() else { return };
                        let point = match res {
                            Ok(v) => match (v["x"].as_f64(), v["y"].as_f64()) {
                                (Some(x), Some(y)) => (x, y),
                                _ => return r(Err("No element matches the selector".into())),
                            },
                            Err(e) => return r(Err(e)),
                        };
                        let r2 = r.clone();
                        b.input_click_then(
                            tab,
                            point.0,
                            point.1,
                            button,
                            count,
                            Box::new(move |res| r2(res.map(|()| json!({"x": point.0, "y": point.1})))),
                        );
                    });
                } else {
                    let (x, y) = (arg_coord(args, "x")?, arg_coord(args, "y")?);
                    self.input_click_then(tab, x, y, button, count, input_reply(&reply));
                }
            }
            "mouse.move" => {
                let (x, y) = (arg_coord(args, "x")?, arg_coord(args, "y")?);
                let tab = self.macro_page_target()?;
                self.input_mouse_move_then(tab, x, y, input_reply(&reply));
            }
            "mouse.down" | "mouse.up" => {
                let (x, y) = (arg_coord(args, "x")?, arg_coord(args, "y")?);
                let button = arg_button(args)?;
                let tab = self.macro_page_target()?;
                self.input_mouse_button(tab, x, y, button, op == "mouse.down", input_reply(&reply));
            }
            "type" => {
                let text = args.get("text").and_then(Value::as_str).ok_or("text must be a string")?;
                if text.chars().count() > 100_000 {
                    return Err("text is longer than 100000 characters".into());
                }
                let tab = self.macro_page_target()?;
                self.input_type_then(tab, text, input_reply(&reply));
            }
            "wait" => {
                let ms = arg_u64(args, "ms", 0, MAX_WAIT_MS)?;
                let r = reply.clone();
                self.macro_timer(run, req_id, ms, move |_| r(Ok(Value::Null)));
            }
            "page.eval" => {
                let code = arg_str(args, "code", 1_000_000)?.to_string();
                let tab = self.macro_page_target()?;
                let by_agent = self.macros.running.borrow().as_ref().is_some_and(|r| r.by_agent);
                let weak = self.me.clone();
                let r = reply.clone();
                let run_code = move |b: &Browser| {
                    b.macro_eval(tab, &code, move |res| {
                        r(res.and_then(|v| {
                            let size = v.to_string().len();
                            if size > MAX_EVAL_RESULT {
                                Err(format!("The result is too large ({} KB; at most 100 KB)", size / 1024))
                            } else {
                                Ok(v)
                            }
                        }))
                    })
                };
                if by_agent {
                    // The agent's macros don't run page code while a password is typed in.
                    let r = reply.clone();
                    self.macro_eval(tab, PASSWORD_FILLED, move |res| {
                        let Some(b) = weak.upgrade() else { return };
                        match res {
                            Ok(Value::Bool(false)) => run_code(&b),
                            Ok(_) => r(Err("refused: a password field on the page holds a value".into())),
                            Err(e) => r(Err(e)),
                        }
                    });
                } else {
                    run_code(self);
                }
            }
            "page.query" => {
                let selector = arg_str(args, "selector", 2000)?;
                let limit = match args.get("limit") {
                    None | Some(Value::Null) => 20,
                    Some(_) => arg_u64(args, "limit", 1, 100)?,
                };
                let tab = self.macro_page_target()?;
                let sel = serde_json::to_string(selector).unwrap_or_default();
                let expr = format!(
                    "(() => {{ const out = []; for (const e of document.querySelectorAll({sel})) {{ \
                     if (out.length >= {limit}) break; const r = e.getBoundingClientRect(); \
                     out.push({{tag: e.tagName.toLowerCase(), text: (e.innerText || e.textContent || '').trim().replace(/\\s+/g, ' ').slice(0, 200), \
                     box: {{x: r.x, y: r.y, w: r.width, h: r.height}}}}); }} return out; }})()"
                );
                let r = reply.clone();
                self.macro_eval(tab, &expr, move |res| r(res));
            }
            "tabs.open" => {
                let url = arg_url(args)?;
                let background = arg_bool(args, "background", false)?;
                let win = self.macros.running.borrow().as_ref().and_then(|r| r.win);
                let win =
                    win.filter(|w| self.frame(*w).is_some()).or_else(|| self.cur_id()).ok_or("No window is open")?;
                let opened = self.ws.borrow_mut().open(win, &url, false, Position::End, !background, now_ms());
                let (id, ops) = opened.ok_or("The tab could not be opened")?;
                self.apply(ops);
                reply(Ok(json!(id)));
            }
            "tabs.switch" => {
                let tab = arg_u64(args, "id", 0, u64::from(u32::MAX))? as TabId;
                let win = self.window_of(tab).ok_or("No such tab")?;
                let ops = self.ws.borrow_mut().update(win, |m| ((), m.activate(tab, now_ms()))).map(|(_, ops)| ops);
                self.apply(ops.unwrap_or_default());
                if let Some(r) = self.macros.running.borrow_mut().as_mut() {
                    r.win = Some(win);
                }
                reply(Ok(Value::Null));
            }
            "navigate" => {
                let url = arg_url(args)?;
                let tab = self.macro_target()?;
                self.navigate_tab(tab, &url);
                reply(Ok(Value::Null));
            }
            "toast" => {
                let message = cap_chars(arg_str(args, "message", 10_000)?, 300);
                let win = self.macros.running.borrow().as_ref().and_then(|r| r.win);
                if let Some(win) = win.or_else(|| self.cur_id()) {
                    self.post_shell_to(win, &Event::Toast { message });
                }
                reply(Ok(Value::Null));
            }
            "log" => {
                let text = args.get("text").and_then(Value::as_str).ok_or("text must be a string")?;
                if let Some(r) = self.macros.running.borrow_mut().as_mut() {
                    if r.log.len() >= MAX_LOG_LINES {
                        r.log.remove(0);
                    }
                    r.log.push(cap_chars(text, MAX_LOG_LINE));
                }
                reply(Ok(Value::Null));
            }
            _ => return Err(format!("Unknown macro call {op}")),
        }
        Ok(())
    }

    /// Run `run_id` ended (its code finished, failed or was stopped).
    fn macro_finish(&self, run_id: u64, result: Result<String, String>) {
        let r = {
            let mut running = self.macros.running.borrow_mut();
            if running.as_ref().map(|r| r.run_id) != Some(run_id) {
                return;
            }
            running.take()
        };
        let Some(mut r) = r else { return };
        self.macros.waits.borrow_mut().clear();
        self.input_release_all();
        self.send_macro_state();
        let ms = r.started.elapsed().as_millis();
        let status = match &result {
            Ok(_) => "ok",
            Err(e) if e == "stopped" => "stopped",
            Err(_) => "error",
        };
        info!("macro run {run_id}: {status} after {ms} ms");
        // A macro the user started (hotkey, Run) that failed says why; the agent gets the
        // error as its tool result instead.
        if let (Err(e), false, Some(win)) = (&result, r.by_agent, r.win) {
            if e != "stopped" {
                let first = e.lines().next().unwrap_or("").chars().take(160).collect::<String>();
                self.post_shell_to(win, &Event::Toast { message: format!("Macro \"{}\": {first}", r.name) });
            }
        }
        if let Some(done) = r.done.take() {
            let log = if r.log.is_empty() { String::new() } else { format!("\nLog:\n{}", r.log.join("\n")) };
            done(match result {
                Ok(_) => Ok(format!("The macro finished in {:.1} s.{log}", ms as f64 / 1000.0)),
                Err(e) => Err(format!("{e}{log}")),
            });
        }
        // The runner goes after a while without macros.
        let idle = self.macros.idle.get() + 1;
        self.macros.idle.set(idle);
        let weak = self.me.clone();
        post_ui_delayed(RUNNER_IDLE_MS, move || {
            if let Some(b) = weak.upgrade() {
                if b.macros.idle.get() == idle && b.current_run().is_none() {
                    b.close_runner();
                }
            }
        });
    }

    /// Every shell's Stop indicator and every krunk://scripts page.
    fn send_macro_state(&self) {
        let (id, name) = self.macros.running.borrow().as_ref().map(|r| (r.id.clone(), r.name.clone())).unzip();
        self.post_shell(&Event::MacroRunning { id: id.clone(), name });
        self.post_scripts_pages(&Event::MacroState { running: id });
    }

    pub(super) fn macro_running(&self) -> bool {
        self.macros.running.borrow().is_some()
    }

    /// The running macro's id.
    pub(super) fn running_macro_id(&self) -> Option<String> {
        self.macros.running.borrow().as_ref().map(|r| r.id.clone())
    }

    /// Stops the running macro (if any) and releases every held key and button.
    pub(super) fn stop_macros(&self, why: &str) {
        let Some(run) = self.current_run() else {
            self.input_release_all();
            return;
        };
        info!("macros: stopping ({why})");
        // Pending waits end with "stopped", then the runner's script is ended and the runner
        // reloaded (a macro stuck in a loop never answers on its own).
        let waits: Vec<Option<u32>> = self.macros.waits.borrow_mut().drain().map(|(_, r)| r).collect();
        for req_id in waits {
            self.runner_reply(run, req_id, Err("stopped".into()));
        }
        let view = {
            let mut runner = self.macros.runner.borrow_mut();
            runner.as_mut().map(|r| {
                r.ready = false;
                r.view.clone()
            })
        };
        if let Some(view) = view {
            let reloaded = Rc::new(Cell::new(false));
            let reload = {
                let (view, reloaded) = (view.clone(), reloaded.clone());
                move || {
                    if !reloaded.replace(true) {
                        view.reload();
                    }
                }
            };
            let again = reload.clone();
            view.devtools_call("Runtime.terminateExecution", &json!({}), move |_| reload());
            post_ui_delayed(500, again);
        }
        self.macro_finish(run, Err("stopped".into()));
    }

    /// The agent stopped (or its window closed): a macro it started stops too, and every held
    /// key and button is released.
    pub(super) fn stop_agent_macros(&self) {
        let by_agent = self.macros.running.borrow().as_ref().is_some_and(|r| r.by_agent);
        if by_agent {
            self.stop_macros("the agent stopped");
        }
        self.input_release_all();
    }

    /// krunk://scripts (tab `tab`) records a hotkey (`on`) or stops recording, while macro
    /// `editing` (if any) is edited: its own hotkey is no conflict.
    pub(super) fn macro_record_for(&self, tab: TabId, on: bool, editing: Option<String>) {
        let mut rec = self.macros.recording.borrow_mut();
        if on {
            *rec = Some((tab, editing));
        } else if rec.as_ref().is_some_and(|(t, _)| *t == tab) {
            *rec = None;
        }
    }

    /// Every macro's hotkey: (id, name, chord).
    fn macro_hotkeys(&self) -> Vec<(String, String, Chord)> {
        let rows = self.store.borrow().macros().unwrap_or_default();
        rows.into_iter()
            .filter_map(|m| {
                let c = Chord::parse(m.hotkey.as_deref()?).ok()?;
                Some((m.id, m.name, c))
            })
            .collect()
    }

    /// Checks a hotkey text for macro `except` (None: a new macro): Ok(its canonical text) or
    /// why it can't be used (for the scripts page's `hotkeyCheck`).
    pub(super) fn macro_hotkey_check(&self, text: &str, except: Option<&str>) -> Result<String, String> {
        let chord = Chord::parse(text)?;
        match macros::hotkey_problem(chord, &self.macro_hotkeys(), except) {
            Some(p) => Err(p),
            None => Ok(chord.format()),
        }
    }

    /// A key in tab `tab` before the page sees it: hotkey recording on krunk://scripts, a
    /// macro's hotkey (start, or stop when it runs). True = handled.
    pub(super) fn macro_key(&self, tab: TabId, key: KeyPress) -> bool {
        const VK_F1: u32 = 0x70;
        const VK_F24: u32 = 0x87;
        if macros::is_modifier_vk(key.vk) {
            return false;
        }
        let fkey = (VK_F1..=VK_F24).contains(&key.vk);
        let chord = Chord { ctrl: key.ctrl, shift: key.shift, alt: key.alt, vk: key.vk };
        // (1) Recording a hotkey on krunk://scripts.
        let recording = self.macros.recording.borrow().clone();
        if let Some((rec_tab, editing)) = recording.filter(|(t, _)| *t == tab) {
            let on_page = self.tab(rec_tab).is_some_and(|t| Page::from_url(&t.url) == Some(Page::Scripts));
            if !on_page {
                *self.macros.recording.borrow_mut() = None;
            } else if key.vk == lb_core::keys::vk::ESCAPE && !key.ctrl && !key.alt && !key.shift {
                // Esc cancels (the page sees it too).
                *self.macros.recording.borrow_mut() = None;
                return false;
            } else if key.ctrl || key.alt || key.shift || fkey {
                if !key.repeat {
                    let conflict = macros::hotkey_problem(chord, &self.macro_hotkeys(), editing.as_deref());
                    self.reply(Some(tab), Event::HotkeyRecorded { chord: chord.format(), conflict });
                }
                return true;
            } else {
                return false;
            }
        }
        // (2) A macro's hotkey (they all need Ctrl or Alt, or are F-keys).
        if !(key.ctrl || key.alt || fkey) || !self.macros_enabled() {
            return false;
        }
        let rows = self.store.borrow().macros().unwrap_or_default();
        let Some(m) = rows
            .into_iter()
            .find(|m| m.enabled && m.hotkey.as_deref().and_then(|h| Chord::parse(h).ok()) == Some(chord))
        else {
            return false;
        };
        if key.repeat {
            return true;
        }
        // Not inside the engine's key callback: starting creates a view.
        let id = m.id;
        self.defer(move |b| {
            if b.running_macro_id().as_deref() == Some(id.as_str()) {
                b.stop_macros("hotkey");
            } else {
                let win = b.window_of(tab);
                b.macro_run(&id, win, false, None, |_| {});
            }
        });
        true
    }

    pub(super) fn send_macro_running_to(&self, win: WindowId) {
        let (id, name) = self.macros.running.borrow().as_ref().map(|r| (r.id.clone(), r.name.clone())).unzip();
        self.post_shell_to(win, &Event::MacroRunning { id, name });
    }

    pub(super) fn macro_tab_gone(&self, tab: TabId) {
        let mut rec = self.macros.recording.borrow_mut();
        if rec.as_ref().is_some_and(|(t, _)| *t == tab) {
            *rec = None;
        }
    }

    /// Settings → macros changed: turned off, the running macro stops.
    pub(super) fn macros_settings_changed(&self) {
        if !self.macros_enabled() {
            self.stop_macros("macros turned off");
        }
    }

    /// Creates or updates a macro (`id` None = new). Checks the name, the size and the hotkey
    /// (the syntax check is the scripts page's, before it saves).
    pub(super) fn macro_save(
        &self,
        id: Option<&str>,
        name: &str,
        source: &str,
        hotkey: Option<&str>,
        by_ai: bool,
    ) -> Result<ScriptCard, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the macro a name".into());
        }
        if name.chars().count() > MAX_NAME {
            return Err(format!("The name is longer than {MAX_NAME} characters"));
        }
        if source.len() > MAX_SOURCE {
            return Err("The macro is larger than 200 KB".into());
        }
        let existing = match id {
            Some(id) => {
                if !scripts_store::valid_id(id) {
                    return Err("No such macro".into());
                }
                let row = self.store.borrow().macro_by_id(id).map_err(|_| "The macros could not be read")?;
                Some(row.ok_or("No such macro")?)
            }
            None => None,
        };
        let id = existing.as_ref().map_or_else(scripts_store::new_id, |m| m.id.clone());
        let hotkey = match hotkey.map(str::trim).filter(|h| !h.is_empty()) {
            Some(h) => Some(self.macro_hotkey_check(h, Some(&id))?),
            None => None,
        };
        let now = now_ms();
        let row = MacroRow {
            id: id.clone(),
            name: name.to_string(),
            source: source.to_string(),
            hotkey,
            enabled: existing.as_ref().is_none_or(|m| m.enabled),
            created_by_ai: existing.as_ref().map_or(by_ai, |m| m.created_by_ai || by_ai),
            created: existing.as_ref().map_or(now, |m| m.created),
            updated: now,
            last_run: existing.as_ref().and_then(|m| m.last_run),
            position: existing.as_ref().map_or(0, |m| m.position),
        };
        if let Err(e) = self.store.borrow().save_macro(&row) {
            warning!("saving a macro: {e}");
            return Err("The macro could not be saved".into());
        }
        info!("macro saved{}", if by_ai { " (by the agent)" } else { "" });
        self.notify_scripts_changed();
        Ok(ScriptCard { id, kind: ScriptKind::Macro, name: row.name, enabled: row.enabled })
    }

    /// Runs macro `id`; `done` gets its end (Ok with its result text, Err with its error) or,
    /// when `early_ms` passes first, Ok("running"). One macro runs at a time: starting
    /// another stops the running one.
    /// `win`: the window it acts in (its active web tab); `None` = the current window.
    pub(super) fn macro_run(
        &self,
        id: &str,
        win: Option<WindowId>,
        by_agent: bool,
        early_ms: Option<u64>,
        done: impl FnOnce(Result<String, String>) + 'static,
    ) {
        if !self.macros_enabled() {
            return done(Err("Macros are turned off in Settings".into()));
        }
        if !scripts_store::valid_id(id) {
            return done(Err("No such macro".into()));
        }
        let row = self.store.borrow().macro_by_id(id);
        let row = match row {
            Ok(Some(r)) => r,
            Ok(None) => return done(Err("No such macro".into())),
            Err(_) => return done(Err("The macros could not be read".into())),
        };
        if self.macro_running() {
            self.stop_macros("another macro started");
        }
        if self.macros.runner.borrow().is_none() {
            if let Err(e) = self.create_runner() {
                warning!("the macro runner could not start: {e}");
                return done(Err("The macro runner could not start".into()));
            }
        }
        let run_id = self.macros.runs.get() + 1;
        self.macros.runs.set(run_id);
        *self.macros.running.borrow_mut() = Some(Running {
            id: row.id.clone(),
            name: row.name.clone(),
            run_id,
            by_agent,
            started: Instant::now(),
            win: win.or_else(|| self.cur_id()),
            source: Some(row.source),
            done: Some(Box::new(done)),
            log: Vec::new(),
        });
        info!("macro run {run_id}: started{}", if by_agent { " by the agent" } else { "" });
        let _ = self.store.borrow().touch_macro_run(&row.id, now_ms());
        self.send_macro_state();
        self.notify_scripts_changed();
        self.macro_start_in_runner();
        if let Some(ms) = early_ms {
            let weak = self.me.clone();
            post_ui_delayed(ms as i64, move || {
                let Some(b) = weak.upgrade() else { return };
                let done =
                    b.macros.running.borrow_mut().as_mut().filter(|r| r.run_id == run_id).and_then(|r| r.done.take());
                if let Some(done) = done {
                    done(Ok("running".into()));
                }
            });
        }
    }

    /// The macros for the agent's `list_macros`.
    pub(super) fn macros_list_json(&self) -> serde_json::Value {
        let running = self.running_macro_id();
        let rows = self.store.borrow().macros().unwrap_or_default();
        Value::Array(
            rows.into_iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "name": m.name,
                        "hotkey": m.hotkey,
                        "enabled": m.enabled,
                        "createdByAi": m.created_by_ai,
                        "lastRun": m.last_run,
                        "running": running.as_deref() == Some(m.id.as_str()),
                    })
                })
                .collect(),
        )
    }
}
