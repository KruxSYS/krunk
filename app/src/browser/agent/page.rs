//! The agent's page tools (0.12.0, WP-B2a): the probe in an isolated world of the target tab
//! (`ui/inject/agent-probe.js`), screenshots, engine input and the page-editing tools — every
//! tool of `Group::Page` in `lb_core::agent::tools`. Results are redacted, scrubbed and capped
//! here before they go back.
//!
//! The probe lives in the isolated world "krunk-agent" of the tab's main frame: created with
//! `Page.createIsolatedWorld` on first use per document (the context id is kept in
//! `AgentHost::probes`; a new document gives a new context, noticed by the engine's "cannot
//! find context" answer, and the world is made again once). Page scripts can't reach it.
//!
//! Every tool first asks the probe for the current values of password and card fields
//! (`secrets`): they stay in this process (as `Zeroizing`) and are cut out of every result.
//!
//! 0.16.0: `fill_password` is the one tool that writes password fields (the probe's
//! `fillPassword`), for a password Krunk generates here or one the user gave. The session
//! offers it only on the user's request and passes the site the user allowed; the value is
//! held as `Zeroizing`, is in no outcome and no log line, and the call that carries it to the
//! page is wiped afterwards.

use super::super::cdp::CdpUser;
use super::super::input::{KeyAction as InputKey, MouseButton};
use super::super::Browser;
use crate::ffi::cef::{post_ui_delayed, View};
use crate::log::{info, warning};
use base64ct::{Base64, Encoding};
use lb_core::agent::tools::args::{self, Button, EditAction, KeyAction};
use lb_core::agent::tools::{Offer, RESULT_CAP, RESULT_CAP_MAX, TOOL_TIMEOUT_MS, WAIT_MAX_MS};
use lb_core::agent::{pages, redact, CategorySet, Image, ToolOutcome};
use lb_core::downloads::{sanitize_file_name, unique_path, DownloadState};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

/// The probe's isolated world.
const WORLD: &str = "krunk-agent";
/// Screenshots for the model are at most this wide (device px).
const SHOT_MAX_WIDTH: f64 = 1280.0;
/// A full-page PNG is cut at this size (device px): larger textures fail in the engine.
const FULL_PAGE_MAX: f64 = 16_384.0;
/// One probe `waitFor` call waits at most this long (DevTools calls time out at 10 s).
const WAIT_CHUNK_MS: u64 = 8_000;
/// How often `wait_for {navigation}` looks at the tab's load state.
const NAV_POLL_MS: i64 = 100;
/// Error and exception texts handed to the model are cut to this.
const ERROR_MAX: usize = 1_000;

type ProbeDone = Box<dyn FnOnce(Result<Value, String>)>;

/// The probe function that takes a password (0.16.0).
const FILL_PASSWORD: &str = "fillPassword";

/// A probe call's arguments. Those of [`FILL_PASSWORD`] hold a password: their strings are
/// wiped when the call is over.
struct ProbeArgs {
    value: Value,
    secret: bool,
}

impl Drop for ProbeArgs {
    fn drop(&mut self) {
        if self.secret {
            wipe_strings(&mut self.value);
        }
    }
}

/// Overwrites every string in `v` (a JSON value that held a secret).
fn wipe_strings(v: &mut Value) {
    match v {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(wipe_strings),
        Value::Object(o) => o.values_mut().for_each(wipe_strings),
        _ => {}
    }
}

/// One page tool call: what each of its steps needs.
struct Call {
    win: WindowId,
    tab: TabId,
    /// The tab's committed address when the tool started (for file names and the downloads list).
    url: String,
    private: bool,
    /// The running tool's generation (None: not started by the session, never stale).
    gen: Option<u64>,
    step: u32,
    /// The page's password and card field values, cut out of every result.
    secrets: Vec<Zeroizing<String>>,
    done: Box<dyn FnOnce(ToolOutcome)>,
}

impl Call {
    fn finish(self, outcome: ToolOutcome) {
        (self.done)(outcome);
    }

    /// `raw` scrubbed of the page's secrets (and card numbers), then paged.
    fn text(&self, raw: &str, offset: usize, max: usize) -> String {
        redact::cap(&redact::scrub(raw, &self.secrets), offset, max).into_result()
    }

    fn json(&self, v: &Value) -> String {
        self.text(&v.to_string(), 0, RESULT_CAP)
    }

    fn ok(self, v: &Value, summary: impl Into<String>) {
        let text = self.json(v);
        self.finish(ToolOutcome::ok(text, summary));
    }

    /// A failure whose text may come from the page: scrubbed and cut.
    fn fail(self, message: &str) {
        let m = redact::scrub(message, &self.secrets);
        let m: String = m.chars().take(ERROR_MAX).collect();
        self.finish(ToolOutcome::error(m));
    }

    fn refuse(self, message: &str) {
        self.finish(ToolOutcome::refused(message));
    }
}

/// The probe's `{error}` / `{refused}` answers as outcomes.
fn probe_failure(v: &Value) -> Option<(bool, String)> {
    if let Some(r) = v.get("refused").and_then(Value::as_str) {
        return Some((true, r.to_string()));
    }
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        let mut m = e.to_string();
        if let Some(missing) = v.get("missing").filter(|m| m.as_array().is_some_and(|a| !a.is_empty())) {
            m.push_str(&format!(" (missing: {missing})"));
        }
        return Some((false, m));
    }
    None
}

/// The engine's answer for a context that is gone (a new document, a crash).
fn stale_context(e: &str) -> bool {
    e.contains("Cannot find context")
        || e.contains("context was destroyed")
        || e.contains("__krunkProbe is not defined")
}

/// The text of a `Runtime.evaluate` exception.
fn exception_text(details: &Value) -> String {
    let d = details
        .get("exception")
        .and_then(|e| e.get("description").and_then(Value::as_str).or_else(|| e.get("value").and_then(Value::as_str)))
        .or_else(|| details.get("text").and_then(Value::as_str))
        .unwrap_or("the script failed");
    d.chars().take(ERROR_MAX).collect()
}

/// A bad CSS selector (no point in waiting or retrying).
fn selector_error(e: &str) -> bool {
    e.contains("SyntaxError") || e.contains("not a valid selector")
}

/// The probe's `context()` answer with the shape `lb_core::agent::context::PageFacts` reads
/// added: `outline` (headings, forms, counts) and the scroll size. The probe's own fields stay.
fn with_outline(mut v: Value) -> Value {
    let Some(o) = v.as_object_mut() else { return v };
    if o.contains_key("outline") {
        return v;
    }
    let num = |o: &serde_json::Map<String, Value>, a: &str, b: &str| {
        o.get(a).and_then(|x| x.get(b)).and_then(Value::as_f64).unwrap_or(0.0)
    };
    let (vw, vh) = (num(o, "viewport", "width"), num(o, "viewport", "height"));
    if let Some(s) = o.get_mut("scroll").and_then(Value::as_object_mut) {
        let max_y = s.get("maxY").and_then(Value::as_f64).unwrap_or(0.0);
        s.insert("width".into(), json!(vw));
        s.insert("height".into(), json!(max_y + vh));
    }
    let counts = o.get("counts").cloned().unwrap_or(Value::Null);
    let outline = json!({
        "headings": o.get("headings").cloned().unwrap_or_else(|| json!([])),
        "forms": o.get("forms").cloned().unwrap_or_else(|| json!([])),
        "links": counts.get("links"),
        "buttons": counts.get("buttons"),
        "images": counts.get("images"),
        "iframes": counts.get("iframes"),
    });
    o.insert("outline".into(), outline);
    v
}

/// The clip of a viewport screenshot no wider than [`SHOT_MAX_WIDTH`] device px, from
/// `Page.getLayoutMetrics` (None: the whole viewport as it is).
fn viewport_clip(m: &Value) -> Option<Value> {
    let css = m.get("cssVisualViewport")?;
    let w = css.get("clientWidth")?.as_f64()?;
    let h = css.get("clientHeight")?.as_f64()?;
    let dpr = device_ratio(m, w);
    if w <= 0.0 || w * dpr <= SHOT_MAX_WIDTH {
        return None;
    }
    let x = css.get("pageX").and_then(Value::as_f64).unwrap_or(0.0);
    let y = css.get("pageY").and_then(Value::as_f64).unwrap_or(0.0);
    Some(json!({ "x": x, "y": y, "width": w, "height": h, "scale": SHOT_MAX_WIDTH / (w * dpr) }))
}

/// Device pixels per CSS pixel (the deprecated `visualViewport` is in device pixels).
fn device_ratio(m: &Value, css_width: f64) -> f64 {
    let dev = m.get("visualViewport").and_then(|v| v.get("clientWidth")).and_then(Value::as_f64).unwrap_or(0.0);
    if css_width > 0.0 && dev > 0.0 {
        (dev / css_width).clamp(0.25, 8.0)
    } else {
        1.0
    }
}

/// The clip of a full-page screenshot (CSS px, cut at [`FULL_PAGE_MAX`] device px) and
/// whether it was cut.
fn full_page_clip(m: &Value) -> Option<(Value, bool)> {
    let size = m.get("cssContentSize")?;
    let w = size.get("width")?.as_f64()?;
    let h = size.get("height")?.as_f64()?;
    let css_w = m.get("cssVisualViewport").and_then(|v| v.get("clientWidth")).and_then(Value::as_f64).unwrap_or(w);
    let max = FULL_PAGE_MAX / device_ratio(m, css_w);
    let (cw, ch) = (w.clamp(1.0, max), h.clamp(1.0, max));
    Some((json!({ "x": 0, "y": 0, "width": cw.ceil(), "height": ch.ceil(), "scale": 1 }), cw < w || ch < h))
}

/// "Screenshot example.com 2026-11-10 14.03.07.png" (or the model's name, made safe, as PNG).
fn screenshot_name(asked: Option<&str>, url: &str, time: &str) -> String {
    let base = match asked.map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => n.to_string(),
        None => {
            let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string));
            format!("Screenshot {} {time}", host.as_deref().unwrap_or("page"))
        }
    };
    let mut name = sanitize_file_name(&base);
    if !name.to_ascii_lowercase().ends_with(".png") {
        name.push_str(".png");
    }
    name
}

/// Does this key type text (or paste it)?
fn types_text(spec: &lb_core::macros::KeySpec, mods: lb_core::macros::Mods) -> bool {
    let printable = spec.text.is_some() && !mods.ctrl && !mods.alt;
    let paste = (mods.ctrl && spec.code == "KeyV") || (mods.shift && spec.code == "Insert");
    printable || paste
}

impl Browser {
    /// Calls the probe's function `func` with `args` (a JSON array) in tab `tab`'s isolated
    /// world (created and the probe injected on first use per document); `done` gets its JSON
    /// result.
    pub(super) fn agent_probe(
        &self,
        tab: TabId,
        func: &str,
        args: Value,
        done: impl FnOnce(Result<Value, String>) + 'static,
    ) {
        if func.is_empty() || !func.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return done(Err("no such probe function".into()));
        }
        let value = if args.is_array() { args } else { json!([args]) };
        let args = ProbeArgs { value, secret: func == FILL_PASSWORD };
        self.probe_call(tab, func.to_string(), args, true, Box::new(done));
    }

    /// [`Browser::agent_probe`]; `retry`: a context found gone is made again once.
    fn probe_call(&self, tab: TabId, func: String, args: ProbeArgs, retry: bool, done: ProbeDone) {
        let Some(view) = self.tab_view(tab) else { return done(Err("The tab is gone".into())) };
        // Page (frame tree, isolated worlds) and Runtime: held while the tab lives.
        if !self.cdp_has(tab, CdpUser::Agent) {
            self.cdp_use(tab, &view, CdpUser::Agent);
        }
        self.agent.cdp_tabs.borrow_mut().insert(tab);
        let ctx = self.agent.probes.borrow().get(&tab).copied();
        if let Some(ctx) = ctx {
            return self.probe_eval(tab, &view, ctx, func, args, retry, done);
        }
        let weak = self.me.clone();
        self.probe_world(tab, &view, move |r| {
            let Some(b) = weak.upgrade() else { return done(Err("Krunk is closing".into())) };
            match r {
                Ok(ctx) => {
                    b.agent.probes.borrow_mut().insert(tab, ctx);
                    match b.tab_view(tab) {
                        Some(view) => b.probe_eval(tab, &view, ctx, func, args, false, done),
                        None => done(Err("The tab is gone".into())),
                    }
                }
                Err(e) => done(Err(e)),
            }
        });
    }

    /// Creates the isolated world in the tab's main frame and runs the probe there; `done`
    /// gets the world's execution context id.
    fn probe_world(&self, tab: TabId, view: &View, done: impl FnOnce(Result<i64, String>) + 'static) {
        let source = lb_core::assets::inject_script("agent-probe.js");
        if source.is_empty() {
            return done(Err("the page probe is missing".into()));
        }
        let weak = self.me.clone();
        view.devtools_call("Page.getFrameTree", &json!({}), move |r| {
            let frame = r.ok().and_then(|v| v["frameTree"]["frame"]["id"].as_str().map(str::to_string));
            let (Some(frame), Some(b)) = (frame, weak.upgrade()) else {
                return done(Err("The page did not answer (it may be loading).".into()));
            };
            let Some(view) = b.tab_view(tab) else { return done(Err("The tab is gone".into())) };
            let params = json!({ "frameId": frame, "worldName": WORLD, "grantUniveralAccess": false });
            let view2 = view.clone();
            view.devtools_call("Page.createIsolatedWorld", &params, move |r| {
                let Some(ctx) = r.ok().and_then(|v| v["executionContextId"].as_i64()) else {
                    warning!("tab {tab}: agent probe: no isolated world");
                    return done(Err("The page did not answer (it may be loading).".into()));
                };
                // `void`: the probe's object stays in the world, no handle to it comes back.
                let expression = format!("void ({source})({})", redact::probe_rules());
                let params = json!({ "contextId": ctx, "expression": expression, "returnByValue": true });
                view2.devtools_call("Runtime.evaluate", &params, move |r| match r {
                    Ok(v) if v.get("exceptionDetails").is_none() => done(Ok(ctx)),
                    _ => {
                        warning!("tab {tab}: agent probe: could not start");
                        done(Err("The page probe could not start.".into()))
                    }
                });
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn probe_eval(
        &self,
        tab: TabId,
        view: &View,
        ctx: i64,
        func: String,
        args: ProbeArgs,
        retry: bool,
        done: ProbeDone,
    ) {
        let expression = Zeroizing::new(format!("__krunkProbe.{func}(...{})", args.value));
        let mut params =
            json!({ "contextId": ctx, "expression": expression.as_str(), "returnByValue": true, "awaitPromise": true });
        let secret = args.secret;
        let weak = self.me.clone();
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            let failed = match &r {
                Err(e) => Some(e.clone()),
                Ok(v) => v.get("exceptionDetails").map(exception_text),
            };
            if let Some(e) = failed {
                if retry && stale_context(&e) {
                    if let Some(b) = weak.upgrade() {
                        // Only if no other call made a new world meanwhile.
                        let mut probes = b.agent.probes.borrow_mut();
                        if probes.get(&tab) == Some(&ctx) {
                            probes.remove(&tab);
                        }
                        drop(probes);
                        return b.probe_call(tab, func, args, false, done);
                    }
                }
                return done(Err(if stale_context(&e) {
                    "The page did not answer (it may be loading).".into()
                } else {
                    e
                }));
            }
            let value = r.ok().and_then(|mut v| v.get_mut("result").and_then(|x| x.get_mut("value")).map(Value::take));
            let value = value.unwrap_or(Value::Null);
            done(Ok(if func == "context" { with_outline(value) } else { value }));
        });
        // The call is on its way: this copy of a password in it is not needed any more.
        if secret {
            wipe_strings(&mut params);
        }
    }

    /// Runs a page tool (`Group::Page`) for window `win`'s agent on its target tab. The caller
    /// has validated `args`; the built-in-page check (`pages::may_act`) is done here.
    pub(super) fn agent_page_tool(
        &self,
        win: WindowId,
        name: &'static str,
        args: Value,
        done: Box<dyn FnOnce(ToolOutcome)>,
    ) {
        let Some(target) = self.agent_target(win) else {
            return done(ToolOutcome::error("No tab is open in this window."));
        };
        if let Err(why) = pages::may_act(&target.url) {
            let why = if why.is_empty() { "The agent can't act on Krunk's own pages." } else { why };
            return done(ToolOutcome::refused(why));
        }
        let (gen, step) = match self.agent_running_tool(win) {
            Some((g, s)) => (Some(g), s),
            None => (None, 0),
        };
        let private = self.tab(target.tab).is_some_and(|t| t.private);
        let mut c = Call { win, tab: target.tab, url: target.url, private, gen, step, secrets: Vec::new(), done };
        // Waiting reads nothing from the page.
        if name == "wait_for" {
            return self.page_tool_run(c, name, args);
        }
        let weak = self.me.clone();
        self.agent_probe(c.tab, "secrets", json!([]), move |r| {
            let Some(b) = weak.upgrade() else { return c.fail("Krunk is closing") };
            match r {
                Ok(Value::Array(list)) => {
                    c.secrets = list
                        .into_iter()
                        .filter_map(|v| match v {
                            Value::String(s) => Some(Zeroizing::new(s)),
                            _ => None,
                        })
                        .collect();
                }
                Ok(_) => {}
                Err(_) => return c.fail("The page did not answer (it may be loading)."),
            }
            if !b.page_call_live(&c) {
                return c.fail("stopped");
            }
            b.page_tool_run(c, name, args);
        });
    }

    /// Is the call still wanted (not stopped or replaced)?
    fn page_call_live(&self, c: &Call) -> bool {
        c.gen.is_none_or(|g| self.agent_tool_live(c.win, g))
    }

    /// Calls the probe for `c`; `then` gets its answer while the call is still wanted (a
    /// failure ends the call).
    fn page_probe(&self, c: Call, func: &str, args: Value, then: impl FnOnce(&Browser, Call, Value) + 'static) {
        let weak = self.me.clone();
        self.agent_probe(c.tab, func, args, move |r| {
            let Some(b) = weak.upgrade() else { return c.fail("Krunk is closing") };
            if !b.page_call_live(&c) {
                return c.fail("stopped");
            }
            match r {
                Ok(v) => then(&b, c, v),
                Err(e) => c.fail(&e),
            }
        });
    }

    /// Like [`Browser::page_probe`], with the probe's `{error}` / `{refused}` answers ending
    /// the call.
    fn page_probe_checked(&self, c: Call, func: &str, args: Value, then: impl FnOnce(&Browser, Call, Value) + 'static) {
        self.page_probe(c, func, args, move |b, c, v| match probe_failure(&v) {
            Some((true, m)) => c.refuse(&m),
            Some((false, m)) => c.fail(&m),
            None => then(b, c, v),
        });
    }

    fn page_tool_run(&self, c: Call, name: &'static str, a: Value) {
        macro_rules! parse {
            ($t:ty) => {
                match args::parse::<$t>(&a) {
                    Ok(v) => v,
                    Err(e) => return c.fail(&e),
                }
            };
        }
        match name {
            "get_page_context" => self.tool_page_context(c),
            "get_html" => {
                let p = parse!(args::GetHtml);
                let max = p.max_chars.unwrap_or(RESULT_CAP).clamp(1, RESULT_CAP_MAX);
                self.page_probe(c, "html", json!([p.selector, p.include_scripts]), move |_, c, v| {
                    let Some(html) = v.get("html").and_then(Value::as_str) else {
                        return c.fail("No element matches the selector.");
                    };
                    let total = html.chars().count();
                    let text = c.text(&redact::redact_html(html), p.offset, max);
                    c.finish(ToolOutcome::ok(text, format!("HTML, {total} characters")));
                });
            }
            "get_text" => {
                let p = parse!(args::GetText);
                let max = p.max_chars.unwrap_or(RESULT_CAP).clamp(1, RESULT_CAP_MAX);
                self.page_probe(c, "text", json!([p.selector]), move |_, c, v| {
                    let Some(t) = v.get("text").and_then(Value::as_str) else {
                        return c.fail("No element matches the selector.");
                    };
                    let total = t.chars().count();
                    let text = c.text(t, p.offset, max);
                    c.finish(ToolOutcome::ok(text, format!("Text, {total} characters")));
                });
            }
            "query" => {
                let p = parse!(args::Query);
                self.page_probe(c, "query", json!([p.selector, p.limit, p.offset]), |_, c, v| {
                    let total = v["total"].as_u64().unwrap_or(0);
                    c.ok(&v, if total == 1 { "1 match".into() } else { format!("{total} matches") });
                });
            }
            "list_links" => {
                let p = parse!(args::ListLinks);
                self.page_probe(c, "links", json!([p.filter, p.limit, p.offset]), |_, c, v| {
                    let total = v["total"].as_u64().unwrap_or(0);
                    c.ok(&v, if total == 1 { "1 link".into() } else { format!("{total} links") });
                });
            }
            "list_forms" => self.page_probe(c, "forms", json!([]), |_, c, v| {
                let n = v["forms"].as_array().map_or(0, Vec::len);
                c.ok(&v, if n == 1 { "1 form".into() } else { format!("{n} forms") });
            }),
            "list_media" => self.page_probe(c, "media", json!([]), |_, c, v| {
                let e = v["elements"].as_array().map_or(0, Vec::len);
                let r = v["requests"].as_array().map_or(0, Vec::len);
                c.ok(&v, format!("{e} media elements, {r} media requests"));
            }),
            "screenshot" => self.tool_screenshot(c),
            "save_screenshot" => {
                let p = parse!(args::SaveScreenshot);
                self.tool_save_screenshot(c, p);
            }
            "click" => {
                let p = parse!(args::Click);
                self.tool_click(c, p);
            }
            "type_text" => {
                let p = parse!(args::TypeText);
                self.tool_type(c, p);
            }
            "fill_fields" => {
                let p = parse!(args::FillFields);
                let asked = p.fields.len();
                self.page_probe_checked(c, "fill", json!([p.fields]), move |_, c, v| {
                    let n = v["filled"].as_u64().unwrap_or(0);
                    c.ok(&v, format!("Filled {n} of {asked} fields"));
                });
            }
            "select_option" => {
                let p = parse!(args::SelectOption);
                self.page_probe_checked(c, "select", json!([p.selector, p.values]), |_, c, v| {
                    let n = v["selected"].as_array().map_or(0, Vec::len);
                    c.ok(&v, if n == 1 { "Chose 1 option".into() } else { format!("Chose {n} options") });
                });
            }
            "press_keys" => {
                let p = parse!(args::PressKeys);
                self.tool_keys(c, p);
            }
            "scroll" => {
                let p = parse!(args::Scroll);
                let to = p.to.map(|t| match t {
                    args::ScrollTo::Top => "top",
                    args::ScrollTo::Bottom => "bottom",
                });
                self.page_probe_checked(c, "scroll", json!([p.dx, p.dy, to, p.selector]), |_, c, v| {
                    let (x, y) = (v["x"].as_i64().unwrap_or(0), v["y"].as_i64().unwrap_or(0));
                    c.ok(&v, format!("Scrolled to {x}, {y}"));
                });
            }
            "wait_for" => {
                let p = parse!(args::WaitFor);
                self.tool_wait(c, p);
            }
            "eval_js" => {
                let p = parse!(args::EvalJs);
                self.tool_eval(c, p);
            }
            "edit_page" => {
                let p = parse!(args::EditPage);
                let action = match p.action {
                    EditAction::SetText => "set_text",
                    EditAction::SetAttribute => "set_attribute",
                    EditAction::RemoveAttribute => "remove_attribute",
                    EditAction::SetStyle => "set_style",
                    EditAction::Remove => "remove",
                    EditAction::InsertCss => "insert_css",
                };
                let params = json!([action, p.selector, p.name, p.value, p.css, p.all]);
                self.page_probe_checked(c, "edit", params, |_, c, v| {
                    let n = v["count"].as_u64().unwrap_or(0);
                    c.ok(&v, if n == 1 { "Changed 1 element".into() } else { format!("Changed {n} elements") });
                });
            }
            "set_title" => {
                let p = parse!(args::SetTitle);
                self.page_probe_checked(c, "setTitle", json!([p.title]), |_, c, v| c.ok(&v, "Title set"));
            }
            "fill_password" => {
                let p = parse!(args::FillPassword);
                // The typed arguments own the password now (wiped when dropped).
                let mut a = a;
                wipe_strings(&mut a);
                self.tool_fill_password(c, p);
            }
            _ => c.fail("This is not a page tool."),
        }
    }

    // ------------------------------------------------------------ reading

    /// The automatic context block, read again now.
    fn tool_page_context(&self, c: Call) {
        let offer = self.agent.wins.borrow().get(&c.win).map(|w| {
            let cfg = w.session.config();
            Offer { userscripts: cfg.userscripts, macros: cfg.macros, gated: CategorySet::EMPTY }
        });
        let tools =
            offer.map(|o| lb_core::agent::tools::offered(o).iter().map(|t| t.name).collect()).unwrap_or_default();
        self.agent_context(c.win, tools, move |b, block| {
            if !b.page_call_live(&c) {
                return c.fail("stopped");
            }
            let text = c.text(&block, 0, RESULT_CAP);
            c.finish(ToolOutcome::ok(text, "Page context"));
        });
    }

    // ------------------------------------------------------------ screenshots

    /// Masks the sensitive fields, captures with `params` (the layout metrics decide the clip
    /// through `clip`), unmasks; `then` gets the image (base64) and the metrics.
    fn masked_capture(
        &self,
        c: Call,
        clip: impl FnOnce(&Value) -> (Value, Value) + 'static,
        then: impl FnOnce(&Browser, Call, String, Value) + 'static,
    ) {
        self.page_probe(c, "mask", json!([true]), move |b, c, _| {
            let Some(view) = b.tab_view(c.tab) else { return c.fail("The tab is gone.") };
            let weak = b.me.clone();
            let view2 = view.clone();
            view.devtools_call("Page.getLayoutMetrics", &json!({}), move |r| {
                let metrics = r.unwrap_or(Value::Null);
                let (params, info) = clip(&metrics);
                let weak2 = weak.clone();
                view2.devtools_call("Page.captureScreenshot", &params, move |r| {
                    let Some(b) = weak2.upgrade() else { return c.fail("Krunk is closing") };
                    // Always unmasked, whatever happened.
                    b.agent_probe(c.tab, "mask", json!([false]), |_| {});
                    if !b.page_call_live(&c) {
                        return c.fail("stopped");
                    }
                    match r.ok().and_then(|mut v| v.get_mut("data").map(Value::take)) {
                        Some(Value::String(data)) => then(&b, c, data, info),
                        _ => c.fail("The screenshot failed (the page may be hidden or still loading)."),
                    }
                });
            });
        });
    }

    fn tool_screenshot(&self, c: Call) {
        let clip = |m: &Value| {
            let mut p = json!({ "format": "jpeg", "quality": 70 });
            if let Some(clip) = viewport_clip(m) {
                p["clip"] = clip;
            }
            (p, Value::Null)
        };
        self.masked_capture(c, clip, |b, c, data, _| {
            let Ok(jpeg) = Base64::decode_vec(&data) else { return c.fail("The screenshot could not be read.") };
            let kb = jpeg.len().div_ceil(1024);
            let image_url = b.agent_store_shot(c.win, c.step, jpeg);
            let vision = b.agent_vision(c.win);
            let mut o = if vision {
                ToolOutcome::ok(
                    "Screenshot of the visible part of the page (password and card fields masked).",
                    format!("Screenshot, {kb} KB"),
                )
            } else {
                ToolOutcome::ok(
                    json!({ "error": "model has no image input", "note": "The screenshot is shown to the user only. Use get_text, query or list_forms to read the page." }).to_string(),
                    "Screenshot (the model can't see images)",
                )
            };
            o.image_url = image_url;
            if vision {
                o.image = Some(Image { mime: "image/jpeg".into(), base64: data });
            }
            c.finish(o);
        });
    }

    /// The folder downloads go to: the configured one, else the user's Downloads folder (as
    /// `downloads.rs` decides it).
    fn agent_download_folder(&self) -> Option<PathBuf> {
        let dir = self.settings.borrow().download_dir.clone();
        if !dir.is_empty() && Path::new(&dir).is_dir() {
            return Some(PathBuf::from(dir));
        }
        crate::ffi::dialogs::downloads_folder()
    }

    fn tool_save_screenshot(&self, c: Call, p: args::SaveScreenshot) {
        let full = p.full_page;
        // The local time for the file name comes from the page (the system's time zone).
        self.page_probe(c, "now", json!([]), move |b, c, now| {
            let time = now.as_str().map(str::to_string).unwrap_or_else(|| now_ms().to_string());
            let name = screenshot_name(p.file_name.as_deref(), &c.url, &time);
            let clip = move |m: &Value| {
                let mut params = json!({ "format": "png" });
                let mut cut = false;
                if full {
                    if let Some((clip, was_cut)) = full_page_clip(m) {
                        params["clip"] = clip;
                        params["captureBeyondViewport"] = json!(true);
                        cut = was_cut;
                    }
                }
                (params, json!(cut))
            };
            b.masked_capture(c, clip, move |b, c, data, cut| {
                let Ok(png) = Base64::decode_vec(&data) else { return c.fail("The screenshot could not be read.") };
                drop(data);
                let Some(folder) = b.agent_download_folder() else { return c.fail("There is no download folder.") };
                let path = unique_path(&folder, &name, |p| p.exists());
                if std::fs::write(&path, &png).is_err() {
                    warning!("window {}: agent step {}: screenshot not saved", c.win, c.step);
                    return c.fail("The file could not be written to the download folder.");
                }
                let size = png.len() as i64;
                {
                    let mut d = b.downloads.borrow_mut();
                    let id = d.start(&c.url, &path, Some(size), now_ms(), c.private);
                    d.progress(id, size, Some(size));
                    d.set_state(id, DownloadState::Completed, None, now_ms());
                }
                b.save_downloads();
                b.push_downloads();
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut v = json!({ "saved": file, "folder": "the download folder", "bytes": size, "full_page": full });
                if cut.as_bool() == Some(true) {
                    v["note"] = json!("The page is very long: the image was cut at 16384 pixels.");
                }
                c.ok(&v, format!("Saved {file}"));
            });
        });
    }

    // ------------------------------------------------------------ input

    fn tool_click(&self, c: Call, p: args::Click) {
        let button = match p.button {
            Button::Left => MouseButton::Left,
            Button::Middle => MouseButton::Middle,
            Button::Right => MouseButton::Right,
        };
        let count = p.count.unwrap_or(1).clamp(1, 2);
        let click = move |b: &Browser, c: Call, x: f64, y: f64, what: String, extra: Value| {
            let weak = b.me.clone();
            b.input_click_then(
                c.tab,
                x,
                y,
                button,
                count,
                Box::new(move |r| {
                    if let Err(e) = r {
                        return c.fail(&e);
                    }
                    if weak.upgrade().is_none() {
                        return c.fail("Krunk is closing");
                    }
                    let mut v = json!({ "clicked": true, "x": x.round(), "y": y.round() });
                    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                        o.extend(e.clone());
                    }
                    let verb = if count == 2 { "Double-clicked" } else { "Clicked" };
                    c.ok(&v, format!("{verb} {what}"));
                }),
            );
        };
        match (p.selector, p.x, p.y) {
            (Some(sel), _, _) => {
                let what: String = sel.chars().take(80).collect();
                self.page_probe(c, "box", json!([sel]), move |b, c, v| {
                    if v["found"].as_bool() != Some(true) {
                        return c.fail("No element matches the selector.");
                    }
                    let (Some(x), Some(y)) = (v["x"].as_f64(), v["y"].as_f64()) else {
                        return c.fail("The element has no position.");
                    };
                    if v["visible"].as_bool() == Some(false) {
                        return c.fail("The element is not visible (no size, hidden or transparent).");
                    }
                    let mut extra = json!({});
                    if v["covered"].as_bool() == Some(true) {
                        extra["note"] = json!("another element covers its centre: the click went to that element");
                    }
                    if v["inViewport"].as_bool() == Some(false) {
                        extra["note"] = json!("the element is outside the visible area");
                    }
                    click(b, c, x, y, what, extra);
                });
            }
            (None, Some(x), Some(y)) => click(self, c, x, y, format!("at {}, {}", x.round(), y.round()), json!({})),
            _ => c.fail("Give a selector, or both x and y."),
        }
    }

    fn tool_type(&self, c: Call, mut p: args::TypeText) {
        let typed = p.text.chars().count();
        let selector = p.selector.take();
        let after_focus = move |b: &Browser, c: Call| {
            b.page_probe(c, "focusedSensitive", json!([]), move |b, c, f| {
                if f["sensitive"].as_bool() == Some(true) {
                    return c.refuse("refused: password/card field");
                }
                let editable = f["editable"].as_bool() == Some(true);
                let weak = b.me.clone();
                b.input_type_then(
                    c.tab,
                    &p.text,
                    Box::new(move |r| {
                        if let Err(e) = r {
                            return c.fail(&e);
                        }
                        let Some(b) = weak.upgrade() else { return c.fail("Krunk is closing") };
                        let mut v = json!({ "typed": typed, "submitted": p.submit });
                        if !editable {
                            v["note"] = json!("the focused element is not a text field; the keys went to the page");
                        }
                        let summary = format!("Typed {typed} characters");
                        if !p.submit {
                            return c.ok(&v, summary);
                        }
                        let Ok((mods, enter)) = lb_core::macros::parse_keys("Enter") else {
                            return c.ok(&v, summary);
                        };
                        b.input_key_then(
                            c.tab,
                            &enter,
                            mods,
                            InputKey::Press,
                            Box::new(move |r| match r {
                                Ok(()) => c.ok(&v, format!("{summary} and pressed Enter")),
                                Err(e) => c.fail(&e),
                            }),
                        );
                    }),
                );
            });
        };
        match selector {
            Some(sel) => self.page_probe(c, "focus", json!([sel]), move |b, c, v| {
                if v["found"].as_bool() != Some(true) {
                    return c.fail("No element matches the selector.");
                }
                if v["sensitive"].as_bool() == Some(true) {
                    return c.refuse("refused: password/card field");
                }
                after_focus(b, c);
            }),
            None => after_focus(self, c),
        }
    }

    fn tool_keys(&self, c: Call, p: args::PressKeys) {
        let (mods, spec) = match lb_core::macros::parse_keys(&p.keys) {
            Ok(k) => k,
            Err(e) => return c.fail(&e),
        };
        let repeat = p.repeat.unwrap_or(true);
        let hold = p.hold_ms.filter(|&ms| ms > 0 && p.action != KeyAction::Up);
        let label: String = p.keys.chars().take(40).collect();
        let check = types_text(&spec, mods) && p.action != KeyAction::Up;
        let send = move |b: &Browser, c: Call| {
            let weak = b.me.clone();
            let tab = c.tab;
            let (action, summary) = match (p.action, hold) {
                (_, Some(ms)) => (InputKey::Down { repeat }, format!("Held {label} for {ms} ms")),
                (KeyAction::Press, None) => (InputKey::Press, format!("Pressed {label}")),
                (KeyAction::Down, None) => (InputKey::Down { repeat }, format!("Holding {label}")),
                (KeyAction::Up, None) => (InputKey::Up, format!("Released {label}")),
            };
            let spec2 = spec.clone();
            b.input_key_then(
                tab,
                &spec,
                mods,
                action,
                Box::new(move |r| {
                    if let Err(e) = r {
                        return c.fail(&e);
                    }
                    let v = json!({ "ok": true });
                    let Some(ms) = hold else { return c.ok(&v, summary) };
                    post_ui_delayed(ms as i64, move || {
                        let Some(b) = weak.upgrade() else { return c.fail("Krunk is closing") };
                        // A Stop meanwhile released the key already.
                        if !b.page_call_live(&c) {
                            return c.fail("stopped");
                        }
                        b.input_key_then(
                            tab,
                            &spec2,
                            mods,
                            InputKey::Up,
                            Box::new(move |r| match r {
                                Ok(()) => c.ok(&v, summary),
                                Err(e) => c.fail(&e),
                            }),
                        );
                    });
                }),
            );
        };
        if !check {
            return send(self, c);
        }
        self.page_probe(c, "focusedSensitive", json!([]), move |b, c, f| {
            if f["sensitive"].as_bool() == Some(true) {
                return c.refuse("refused: password/card field");
            }
            send(b, c);
        });
    }

    // ------------------------------------------------------------ passwords (0.16.0)

    /// `fill_password`: the one tool that writes password fields. The session offers it only
    /// when the user's message asked for a password, and the user allowed `p.origin` for it:
    /// nothing is entered unless the tab still shows that site. The value is never part of the
    /// outcome.
    fn tool_fill_password(&self, c: Call, p: args::FillPassword) {
        let Some(site) = pages::password_site(&c.url).filter(|s| !p.origin.is_empty() && *s == p.origin) else {
            return c.refuse(
                "Nothing was entered: the tab does not show the site the user allowed. Look at the page and call \
                 fill_password again.",
            );
        };
        match p.mode {
            args::PasswordMode::Provided => match p.password.filter(|pw| !pw.is_empty()) {
                Some(pw) => self.fill_password_fields(c, pw, p.selector, None),
                None => c.fail("password is missing"),
            },
            args::PasswordMode::Generate => {
                // A dry run first (nothing is written): which fields would it be, and is this a
                // form where a new password is chosen? The same test as for the dropdown's "Use
                // a strong password".
                let selector = p.selector;
                let params = json!([Value::Null, selector, true]);
                self.page_probe_checked(c, FILL_PASSWORD, params, move |b, c, v| {
                    if v["fields"].as_array().is_none_or(Vec::is_empty) {
                        return c.fail(
                            "No field for a new password was found. Give the selector of the new-password field or \
                             of its form.",
                        );
                    }
                    let autocomplete = v["autocomplete"].as_str().unwrap_or_default();
                    let passwords = v["passwords"].as_u64().unwrap_or(0).min(u64::from(u32::MAX)) as u32;
                    let text = v["formText"].as_str().unwrap_or_default();
                    if !lb_core::passgen::wants_new_password(autocomplete, passwords, text) {
                        return c.refuse(
                            "Nothing was entered: this does not look like a form where a new password is chosen \
                             (sign-up or change password). A generated password is only entered there.",
                        );
                    }
                    let settings = b.settings.borrow().passgen.clone();
                    match lb_core::passgen::generate(settings.length, settings.symbols) {
                        Ok(pw) => b.fill_password_fields(c, pw, selector, Some(site)),
                        Err(_) => {
                            warning!("window {}: agent step {}: no password generated", c.win, c.step);
                            c.fail("Krunk could not generate a password.")
                        }
                    }
                });
            }
        }
    }

    /// Writes `pw` into the password fields the probe picks. `generated`: the site of a
    /// password Krunk made: it is handed to the password manager like one chosen with "Use a
    /// strong password" (sending the form saves it, or asks) and kept for the panel's card.
    fn fill_password_fields(
        &self,
        mut c: Call,
        pw: Zeroizing<String>,
        selector: Option<String>,
        generated: Option<String>,
    ) {
        // The probe never echoes the value; should a page's error text hold it, it is cut out.
        c.secrets.push(pw.clone());
        let params = json!([pw.as_str(), selector, generated.is_some()]);
        self.page_probe_checked(c, FILL_PASSWORD, params, move |b, c, v| {
            let filled = v["filled"].as_u64().unwrap_or(0).min(100) as u32;
            if filled == 0 {
                return c.fail("No password field was filled.");
            }
            let fields = v.get("fields").cloned().unwrap_or_else(|| json!([]));
            let length = pw.chars().count();
            let what = if filled == 1 { "1 field".to_string() } else { format!("{filled} fields") };
            let Some(site) = generated else {
                let v = json!({ "filled": filled, "fields": fields, "length": length });
                return c.ok(&v, format!("Entered the password into {what}"));
            };
            info!("window {}: agent step {}: generated password filled ({what})", c.win, c.step);
            b.remember_generated(c.tab, &site, pw.clone());
            b.agent_keep_password(c.win, &site, pw, filled);
            let note = if c.private {
                "The user sees the password in the panel. You can't read it. Krunk does not save passwords from \
                 private tabs."
            } else {
                "The user sees the password in the panel. You can't read it. Krunk's password manager saves it, \
                 or offers to, when the form is sent."
            };
            let v = json!({ "filled": filled, "fields": fields, "generated": true, "length": length, "note": note });
            c.ok(&v, format!("Created a password and entered it into {what}"));
        });
    }

    // ------------------------------------------------------------ waiting

    fn tool_wait(&self, c: Call, p: args::WaitFor) {
        let modes = usize::from(p.selector.is_some()) + usize::from(p.navigation) + usize::from(p.ms.is_some());
        if modes != 1 {
            return c.fail("Give exactly one of selector, navigation or ms.");
        }
        let timeout = p.timeout_ms.unwrap_or(TOOL_TIMEOUT_MS).min(WAIT_MAX_MS);
        if let Some(ms) = p.ms {
            let ms = ms.min(WAIT_MAX_MS);
            let weak = self.me.clone();
            post_ui_delayed(ms as i64, move || match weak.upgrade() {
                Some(b) if b.page_call_live(&c) => c.ok(&json!({ "waited_ms": ms }), format!("Waited {ms} ms")),
                Some(_) => c.fail("stopped"),
                None => c.fail("Krunk is closing"),
            });
        } else if let Some(sel) = p.selector {
            self.wait_selector(c, sel, Instant::now() + Duration::from_millis(timeout), timeout);
        } else {
            self.wait_navigation(c, false, timeout as i64);
        }
    }

    /// Asks the probe to wait in chunks (a DevTools call may not take longer than 10 s); a
    /// navigation meanwhile is waited through.
    fn wait_selector(&self, c: Call, sel: String, deadline: Instant, timeout: u64) {
        let left = deadline.saturating_duration_since(Instant::now()).as_millis() as u64;
        let weak = self.me.clone();
        let sel2 = sel.clone();
        self.agent_probe(c.tab, "waitFor", json!([sel, left.min(WAIT_CHUNK_MS)]), move |r| {
            let Some(b) = weak.upgrade() else { return c.fail("Krunk is closing") };
            if !b.page_call_live(&c) {
                return c.fail("stopped");
            }
            let over = Instant::now() >= deadline;
            match r {
                Ok(v) if v["found"].as_bool() == Some(true) => c.ok(&json!({ "found": true }), "Element found"),
                Err(e) if selector_error(&e) => c.fail(&e),
                _ if over => c.fail(&format!("No element matched within {timeout} ms.")),
                Ok(_) => b.wait_selector(c, sel2, deadline, timeout),
                // The page is between documents: look again shortly.
                Err(_) => {
                    let weak = b.me.clone();
                    post_ui_delayed(250, move || match weak.upgrade() {
                        Some(b) => b.wait_selector(c, sel2, deadline, timeout),
                        None => c.fail("Krunk is closing"),
                    });
                }
            }
        });
    }

    /// Polls the tab's load state until a load started and finished.
    fn wait_navigation(&self, c: Call, started: bool, left_ms: i64) {
        let Some(loading) = self.tab(c.tab).map(|t| t.loading) else { return c.fail("The tab was closed.") };
        let started = started || loading;
        if started && !loading {
            return c.ok(&json!({ "loaded": true }), "Page loaded");
        }
        if left_ms <= 0 {
            return c.fail(if started {
                "The page is still loading."
            } else {
                "No navigation started in time (it may have finished before the wait began)."
            });
        }
        if !self.page_call_live(&c) {
            return c.fail("stopped");
        }
        let weak = self.me.clone();
        post_ui_delayed(NAV_POLL_MS, move || match weak.upgrade() {
            Some(b) => b.wait_navigation(c, started, left_ms - NAV_POLL_MS),
            None => c.fail("Krunk is closing"),
        });
    }

    // ------------------------------------------------------------ scripts

    /// Runs the model's code in the page's main world (not the probe's).
    fn tool_eval(&self, c: Call, p: args::EvalJs) {
        if !c.secrets.is_empty() {
            return c.refuse("eval_js is not available while a password or card field holds a value.");
        }
        let Some(view) = self.tab_view(c.tab) else { return c.fail("The tab is gone.") };
        let params = json!({
            "expression": p.code,
            "awaitPromise": p.await_promise.unwrap_or(true),
            "returnByValue": true,
            "userGesture": true,
            // The engine stops the script before the DevTools call itself times out.
            "timeout": 9_000,
        });
        let weak = self.me.clone();
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            let Some(b) = weak.upgrade() else { return c.fail("Krunk is closing") };
            if !b.page_call_live(&c) {
                return c.fail("stopped");
            }
            let v = match r {
                Ok(v) => v,
                Err(e) => return c.fail(&e),
            };
            if let Some(d) = v.get("exceptionDetails") {
                return c.fail(&format!("The code threw: {}", exception_text(d)));
            }
            let res = &v["result"];
            let text = if let Some(value) = res.get("value") {
                value.to_string()
            } else if let Some(u) = res.get("unserializableValue").and_then(Value::as_str) {
                u.to_string()
            } else if res["type"] == "undefined" {
                "undefined".into()
            } else {
                res.get("description").and_then(Value::as_str).unwrap_or("(no value)").to_string()
            };
            // The code may have filled a password or card field and read it back: the page's
            // secrets are read again now and cut out too (a failure to read them withholds the
            // result rather than risk it).
            let weak = b.me.clone();
            let raw = Zeroizing::new(text);
            b.agent_probe(c.tab, "secrets", json!([]), move |r| {
                let Some(_b) = weak.upgrade() else { return c.fail("Krunk is closing") };
                let mut c = c;
                match r {
                    Ok(Value::Array(list)) => c.secrets.extend(list.into_iter().filter_map(|v| match v {
                        Value::String(s) => Some(Zeroizing::new(s)),
                        _ => None,
                    })),
                    Ok(_) => {}
                    Err(_) => return c.fail("The result was withheld: the page did not answer the redaction check."),
                }
                let text = c.text(&raw, 0, RESULT_CAP);
                c.finish(ToolOutcome::ok(text, "Ran the script"));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_gets_an_outline() {
        let v = with_outline(json!({
            "selection": "", "viewport": {"width": 800, "height": 600},
            "scroll": {"x": 0, "y": 10, "maxY": 1400},
            "headings": [{"level": 1, "text": "Hi"}], "forms": [],
            "counts": {"links": 3, "buttons": 1, "images": 0, "iframes": 0}
        }));
        let f: lb_core::agent::context::PageFacts = serde_json::from_value(v).unwrap();
        assert_eq!(f.outline.headings.len(), 1);
        assert_eq!(f.outline.links, Some(3));
        assert_eq!(f.scroll.unwrap().height, 2000.0);
    }

    #[test]
    fn clips() {
        let m = json!({"cssVisualViewport": {"clientWidth": 1000.0, "clientHeight": 700.0, "pageX": 0.0, "pageY": 50.0},
                       "visualViewport": {"clientWidth": 1500.0}, "cssContentSize": {"width": 1000.0, "height": 40000.0}});
        let c = viewport_clip(&m).unwrap();
        assert!((c["scale"].as_f64().unwrap() * 1500.0 - 1280.0).abs() < 0.01);
        assert_eq!(c["y"], 50.0);
        let small = json!({"cssVisualViewport": {"clientWidth": 1000.0, "clientHeight": 700.0}});
        assert!(viewport_clip(&small).is_none());
        let (full, cut) = full_page_clip(&m).unwrap();
        assert!(cut);
        assert!(full["height"].as_f64().unwrap() <= 16_384.0 / 1.5 + 1.0);
    }

    #[test]
    fn names() {
        assert_eq!(
            screenshot_name(None, "https://example.com/a", "2026-11-10 14.03.07"),
            "Screenshot example.com 2026-11-10 14.03.07.png"
        );
        assert_eq!(screenshot_name(Some("a/b:c"), "", "t"), "b_c.png");
        assert_eq!(screenshot_name(Some("x.PNG"), "", "t"), "x.PNG");
    }
}
