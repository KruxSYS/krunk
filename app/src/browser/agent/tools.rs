//! The agent's tools on the host side (0.12.0, WP-B2): each one runs against the window's
//! target tab (CDP, the probe in its isolated world, engine input), the browser's tabs,
//! settings, downloads, userscripts and macros, and returns a redacted, capped `ToolOutcome`.
//!
//! Page tools (`Group::Page`) are in `page.rs` (WP-B2a); this file (WP-B2b) dispatches every
//! tool and runs the others: navigation and tabs, settings, userscripts, macros and the
//! sensitive-data tools.
//!
//! Every text result is scrubbed (card-like digit runs) and capped here. Tab ids are those of
//! the agent's own window only. Nothing here logs addresses, titles or values.

use super::super::cdp::CdpUser;
use super::super::Browser;
use crate::ffi::cef::{list_cookies, post_ui_delayed, Profile, View};
use crate::log::warning;
use lb_core::agent::tools::{self as agent_tools, args, Group, RESULT_CAP, TOOL_TIMEOUT_MS};
use lb_core::agent::{pages as agent_pages, redact, settings_allow, Category, ToolOutcome};
use lb_core::pages::{self, Page};
use lb_core::storage::now_ms;
use lb_core::tabs::{Position, TabId};
use lb_core::windows::WindowId;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// The tab the agent of a window acts on: the window's active tab, at its committed address.
pub(super) struct Target {
    pub tab: TabId,
    pub url: String,
}

type Done = Box<dyn FnOnce(ToolOutcome)>;

/// A callback that runs once: the first of its callers (an answer or a timer) wins.
type Slot<T> = Rc<RefCell<Option<Box<dyn FnOnce(&Browser, T)>>>>;

fn fire<T>(slot: &Slot<T>, weak: &Weak<Browser>, value: T) {
    let f = slot.borrow_mut().take();
    if let (Some(f), Some(b)) = (f, weak.upgrade()) {
        f(&b, value);
    }
}

/// How often a navigation's progress is looked at, how long a load that hasn't shown up yet is
/// waited for, and how long a load is waited for at most (the tool then reports it as loading).
const LOAD_POLL_MS: i64 = 200;
const LOAD_START_MS: i64 = 1_000;
const LOAD_WAIT_MS: i64 = 10_000;
/// A macro still running after this is reported as running (the toolbar shows Stop).
const MACRO_EARLY_MS: u64 = 2_000;
/// How long the macro syntax check may take before the macro is saved without it.
const COMPILE_WAIT_MS: i64 = 3_000;
/// Cookie values are shortened to this many characters.
const COOKIE_VALUE_CHARS: usize = 200;
const DAY_MS: i64 = 86_400_000;

/// What the model reads: card-like digit runs scrubbed, capped at the default size.
fn model_text(text: &str) -> String {
    redact::cap(&redact::scrub(text, &[]), 0, RESULT_CAP).into_result()
}

/// A successful outcome with a JSON result and a one-line summary for the step log.
fn ok_json(v: Value, summary: impl Into<String>) -> ToolOutcome {
    let summary = redact::scrub(&summary.into(), &[]);
    ToolOutcome::ok(model_text(&v.to_string()), summary)
}

/// The address to load for `input` (a web address, `about:blank`, or one of Krunk's pages by
/// its `krunk://` name or real address), if the agent may open it.
fn agent_url(input: &str) -> Result<String, &'static str> {
    let t = input.trim();
    agent_pages::may_navigate(t)?;
    if let Some((page, section)) = Page::from_display_with_section(t) {
        return Ok(page.url_with_section(section.as_deref()));
    }
    if pages::is_blank(t) {
        return Ok("about:blank".into());
    }
    Ok(url::Url::parse(t).map(String::from).unwrap_or_else(|_| t.to_string()))
}

/// `2026-11-10 14:03 UTC` for a Unix time in ms.
fn utc_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

/// The host part of what the model gave as a site (`example.com`, `https://www.example.com/a`,
/// `.example.com`), lower-case, without a port.
fn site_host(site: &str) -> Option<String> {
    let t = site.trim();
    let host = match url::Url::parse(t) {
        Ok(u) if u.host_str().is_some() => u.host_str().unwrap_or_default().to_string(),
        _ => {
            let rest = t.split(['/', '?', '#']).next().unwrap_or("");
            let rest = rest.rsplit('@').next().unwrap_or(rest);
            match rest.rsplit_once(':') {
                Some((h, port)) if !h.contains(':') && port.bytes().all(|b| b.is_ascii_digit()) => h.to_string(),
                _ => rest.to_string(),
            }
        }
    };
    let host = host.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty() && !host.contains(char::is_whitespace)).then_some(host)
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    } else {
        s.to_string()
    }
}

impl Browser {
    /// Window `win`'s target tab (its active tab).
    pub(super) fn agent_target(&self, win: WindowId) -> Option<Target> {
        let tab = self.ws.borrow().model(win)?.active_id()?;
        let url = self.tab(tab)?.url.clone();
        Some(Target { tab, url })
    }

    /// Runs tool `name` with validated `args` for window `win`'s agent; `done` gets the outcome
    /// (exactly once, also on failure).
    pub(super) fn run_agent_tool(&self, win: WindowId, name: &str, args: Value, done: Box<dyn FnOnce(ToolOutcome)>) {
        let Some(tool) = agent_tools::find(name) else {
            done(ToolOutcome::error("there is no such tool"));
            return;
        };
        match tool.group {
            Group::Page => self.agent_page_tool(win, tool.name, args, done),
            // The session answers these itself.
            Group::Ask | Group::Finish => done(ToolOutcome::error(format!("{name} is not a browser tool"))),
            // AI may have gone off since the call was made.
            _ if !self.ai_enabled() => done(ToolOutcome::refused("AI features are turned off")),
            Group::Browser => self.agent_browser_tool(win, tool.name, &args, done),
            Group::Settings => done(self.agent_settings_tool(tool.name, &args)),
            Group::Userscripts => done(self.agent_userscript_tool(win, tool.name, &args)),
            Group::Macros => self.agent_macro_tool(win, tool.name, &args, done),
            Group::Sensitive(category) => self.agent_sensitive_tool(win, category, &args, done),
        }
    }

    // ------------------------------------------------------------ helpers

    /// Is `tab` one of window `win`'s tabs?
    fn agent_own_tab(&self, win: WindowId, tab: TabId) -> bool {
        self.window_of(tab) == Some(win)
    }

    /// A DevTools call on `view` that answers within `timeout_ms` (an error then).
    fn agent_cdp_call(
        &self,
        view: &View,
        method: &str,
        params: Value,
        timeout_ms: i64,
        done: impl FnOnce(&Browser, Result<Value, String>) + 'static,
    ) {
        let slot: Slot<Result<Value, String>> = Rc::new(RefCell::new(Some(Box::new(done))));
        let (s2, weak) = (slot.clone(), self.me.clone());
        post_ui_delayed(timeout_ms, move || fire(&s2, &weak, Err("the page did not answer in time".into())));
        let weak = self.me.clone();
        view.devtools_call(method, &params, move |r| fire(&slot, &weak, r));
    }

    /// Waits until tab `tab` has loaded what it started (or [`LOAD_WAIT_MS`] passed, or the
    /// tool was cancelled), then `done`.
    fn agent_after_load(&self, win: WindowId, tab: TabId, done: impl FnOnce(&Browser) + 'static) {
        let gen = self.agent_running_tool(win).map(|(g, _)| g);
        self.agent_load_poll(win, tab, gen, 0, false, Box::new(done));
    }

    fn agent_load_poll(
        &self,
        win: WindowId,
        tab: TabId,
        gen: Option<u64>,
        waited: i64,
        seen: bool,
        done: Box<dyn FnOnce(&Browser)>,
    ) {
        let loading = self.tab(tab).map(|t| t.loading);
        let seen = seen || loading == Some(true);
        let live = gen.is_none_or(|g| self.agent_tool_live(win, g));
        let settled = loading == Some(false) && (seen || waited >= LOAD_START_MS);
        if loading.is_none() || !live || settled || waited >= LOAD_WAIT_MS {
            done(self);
            return;
        }
        let weak = self.me.clone();
        post_ui_delayed(LOAD_POLL_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.agent_load_poll(win, tab, gen, waited + LOAD_POLL_MS, seen, done);
            }
        });
    }

    /// A tab as the tools report it.
    fn agent_tab_json(&self, tab: TabId) -> Value {
        match self.tab(tab) {
            Some(t) => json!({
                "tab_id": tab,
                "url": pages::display_url(&t.url),
                "title": t.title,
                "loading": t.loading,
                "private": t.private,
            }),
            None => json!({ "tab_id": tab, "closed": true }),
        }
    }

    /// The outcome of a navigation in `tab` once it has loaded.
    fn agent_report_load(&self, win: WindowId, tab: TabId, summary: String, done: Done) {
        self.agent_after_load(win, tab, move |b| {
            let v = b.agent_tab_json(tab);
            let summary = if v["loading"] == true { format!("{summary} (still loading)") } else { summary };
            done(ok_json(v, summary));
        });
    }

    // ------------------------------------------------------------ navigation and tabs

    fn agent_browser_tool(&self, win: WindowId, name: &'static str, a: &Value, done: Done) {
        if name == "list_tabs" {
            let (_, tabs) = self.agent_tab_facts(win);
            let n = tabs.len();
            return done(ok_json(json!({ "tabs": tabs }), format!("{n} tabs")));
        }
        if name == "open_tab" {
            return match args::parse::<args::OpenTab>(a) {
                Ok(a) => self.agent_open_tab(win, a, done),
                Err(e) => done(ToolOutcome::error(e)),
            };
        }
        if name == "switch_tab" {
            return done(match args::parse::<args::SwitchTab>(a) {
                Ok(a) => self.agent_switch_tab(win, a.tab_id),
                Err(e) => ToolOutcome::error(e),
            });
        }
        if name == "close_tab" {
            return done(match args::parse::<args::CloseTab>(a) {
                Ok(a) => self.agent_close_tab(win, a.tab_id),
                Err(e) => ToolOutcome::error(e),
            });
        }
        // The rest act on the target tab.
        let Some(target) = self.agent_target(win) else {
            return done(ToolOutcome::error("no tab is open in this window"));
        };
        let Some(view) = self.tab_view(target.tab) else {
            return done(ToolOutcome::error("the tab is not ready yet"));
        };
        let tab = target.tab;
        match name {
            "navigate" => {
                let a = match args::parse::<args::Navigate>(a) {
                    Ok(a) => a,
                    Err(e) => return done(ToolOutcome::error(e)),
                };
                let url = match agent_url(&a.url) {
                    Ok(u) => u,
                    Err(why) => return done(ToolOutcome::refused(why)),
                };
                let shown = pages::display_url(&url);
                self.navigate_tab(tab, &url);
                self.agent_report_load(win, tab, format!("Opened {}", shorten(&shown, 80)), done);
            }
            "back" | "forward" => self.agent_history_step(win, tab, &view, name == "forward", done),
            "reload" => {
                view.reload();
                self.agent_report_load(win, tab, "Reloaded the page".into(), done);
            }
            _ => done(ToolOutcome::error(format!("{name} is not available"))),
        }
    }

    /// Back / forward in tab `tab`, to an entry the agent may open (never to one of the
    /// sensitive built-in pages).
    fn agent_history_step(&self, win: WindowId, tab: TabId, view: &View, forward: bool, done: Done) {
        let timeout = TOOL_TIMEOUT_MS as i64;
        self.agent_cdp_call(view, "Page.getNavigationHistory", json!({}), timeout, move |b, r| {
            let Some((current, entries)) = r.ok().as_ref().and_then(lb_core::navmenu::parse_history) else {
                return done(ToolOutcome::error("the tab's history could not be read"));
            };
            let index = if forward { current.checked_add(1) } else { current.checked_sub(1) };
            // The engine's blank first entry doesn't count as a page (as the Back button).
            let entry = index.and_then(|i| entries.get(i)).filter(|e| !(pages::is_blank(&e.url) && !forward));
            let Some(entry) = entry else {
                let which = if forward { "forward to" } else { "back to" };
                return done(ToolOutcome::error(format!("there is no page to go {which}")));
            };
            if let Err(why) = agent_pages::may_navigate(&entry.url) {
                return done(ToolOutcome::refused(why));
            }
            let Some(view) = b.tab_view(tab) else { return done(ToolOutcome::error("the tab is gone")) };
            let params = json!({ "entryId": entry.id });
            b.agent_cdp_call(&view, "Page.navigateToHistoryEntry", params, timeout, move |b, r| {
                if r.is_err() {
                    return done(ToolOutcome::error("the tab could not go there"));
                }
                let summary = if forward { "Went forward" } else { "Went back" };
                b.agent_report_load(win, tab, summary.into(), done);
            });
        });
    }

    fn agent_open_tab(&self, win: WindowId, a: args::OpenTab, done: Done) {
        let url = match (a.url.as_deref().map(str::trim), a.query.as_deref().map(str::trim)) {
            (Some(u), None) if !u.is_empty() => agent_url(u),
            (None, Some(q)) if !q.is_empty() => {
                let template = self.settings.borrow().search_template().to_string();
                let u = lb_core::urlclean::search_url(q, &template);
                agent_pages::may_navigate(&u).map(|_| u)
            }
            _ => return done(ToolOutcome::error("give either url or query")),
        };
        let url = match url {
            Ok(u) => u,
            Err(why) => return done(ToolOutcome::refused(why)),
        };
        // Next to the target, private like it.
        let target = self.agent_target(win);
        let private = target.as_ref().and_then(|t| self.tab(t.tab).map(|t| t.private)).unwrap_or(false);
        let position = target.map_or(Position::End, |t| Position::AfterOpener(t.tab));
        let foreground = !a.background;
        let opened = self.ws.borrow_mut().open(win, &url, private, position, foreground, now_ms());
        let Some((tab, ops)) = opened else { return done(ToolOutcome::error("the tab could not be opened")) };
        self.apply(ops);
        let shown = shorten(&pages::display_url(&url), 80);
        if foreground {
            self.agent_report_load(win, tab, format!("Opened a tab: {shown}"), done);
        } else {
            let mut v = self.agent_tab_json(tab);
            v["background"] = json!(true);
            done(ok_json(v, format!("Opened a background tab: {shown}")));
        }
    }

    fn agent_switch_tab(&self, win: WindowId, tab: TabId) -> ToolOutcome {
        if !self.agent_own_tab(win, tab) {
            return ToolOutcome::error("there is no tab with this id in this window");
        }
        let ops = self.ws.borrow_mut().update(win, |m| ((), m.activate(tab, now_ms()))).map(|(_, o)| o);
        self.apply(ops.unwrap_or_default());
        ok_json(self.agent_tab_json(tab), "Switched tabs")
    }

    fn agent_close_tab(&self, win: WindowId, tab: Option<TabId>) -> ToolOutcome {
        let Some(tab) = tab.or_else(|| self.agent_target(win).map(|t| t.tab)) else {
            return ToolOutcome::error("no tab is open in this window");
        };
        if !self.agent_own_tab(win, tab) {
            return ToolOutcome::error("there is no tab with this id in this window");
        }
        // The window (and the agent with it) would close.
        let count = self.ws.borrow().model(win).map_or(0, |m| m.tabs().len());
        if count <= 1 {
            return ToolOutcome::error("this is the window's last tab: closing it would close the window");
        }
        self.close_tab(tab);
        let active = self.agent_target(win).map(|t| t.tab);
        ok_json(json!({ "closed": tab, "active_tab": active }), "Closed a tab")
    }

    // ------------------------------------------------------------ settings

    fn agent_settings_tool(&self, name: &'static str, a: &Value) -> ToolOutcome {
        match name {
            "get_settings" => {
                let all = settings_allow::get_all(&self.settings.borrow());
                ok_json(json!({ "settings": all }), "Read the settings")
            }
            "set_setting" => {
                let a = match args::parse::<args::SetSetting>(a) {
                    Ok(a) => a,
                    Err(e) => return ToolOutcome::error(e),
                };
                let mut new = self.settings.borrow().clone();
                let changed = match settings_allow::set(&mut new, &a.key, a.value) {
                    Ok(c) => c,
                    Err(e) => return ToolOutcome::error(e),
                };
                if let Err(e) = self.try_change_settings(|s| *s = new) {
                    return ToolOutcome::error(format!("the setting was not changed: {e}"));
                }
                let key = shorten(&a.key, 60);
                let summary =
                    if changed.restart { format!("Changed {key} (needs a restart)") } else { format!("Changed {key}") };
                ok_json(json!({ "ok": true, "key": a.key, "restart_needed": changed.restart }), summary)
            }
            _ => ToolOutcome::error(format!("{name} is not available")),
        }
    }

    // ------------------------------------------------------------ userscripts

    fn agent_userscript_tool(&self, win: WindowId, name: &'static str, a: &Value) -> ToolOutcome {
        if !self.settings.borrow().userscripts.enabled {
            return ToolOutcome::refused("userscripts are turned off in Settings");
        }
        match name {
            "list_userscripts" => {
                let list = self.userscripts_list_json();
                let n = list.as_array().map_or(0, Vec::len);
                ok_json(json!({ "userscripts": list }), format!("{n} userscripts"))
            }
            "save_userscript" => match args::parse::<args::SaveUserscript>(a) {
                Ok(a) => self.agent_save_userscript(win, a),
                Err(e) => ToolOutcome::error(e),
            },
            "toggle_userscript" => {
                let a = match args::parse::<args::Toggle>(a) {
                    Ok(a) => a,
                    Err(e) => return ToolOutcome::error(e),
                };
                match self.userscript_toggle(&a.id, a.enabled) {
                    Ok(card) => {
                        let verb = if card.enabled { "Turned on" } else { "Turned off" };
                        let v = json!({ "id": card.id, "name": card.name, "enabled": card.enabled });
                        let summary = format!("{verb} userscript {}", shorten(&card.name, 60));
                        ToolOutcome { script: Some(card), ..ok_json(v, summary) }
                    }
                    Err(e) => ToolOutcome::error(e),
                }
            }
            _ => ToolOutcome::error(format!("{name} is not available")),
        }
    }

    /// Saves a userscript (a new one is on) and runs it on the target tab if it matches there.
    fn agent_save_userscript(&self, win: WindowId, a: args::SaveUserscript) -> ToolOutcome {
        let created = a.id.is_none();
        let card = match self.userscript_save(a.id.as_deref(), &a.source, true) {
            Ok(c) => c,
            Err(e) => return ToolOutcome::error(e),
        };
        let meta = lb_core::userscripts::parse_meta(&a.source).unwrap_or_default();
        let mut ran = false;
        let mut note = None;
        if let Some(target) = self.agent_target(win) {
            if card.enabled && lb_core::userscripts::matches_url(&meta, &target.url) {
                match self.userscript_run_now(&card.id, target.tab, false) {
                    Ok(r) => ran = r,
                    Err(e) => note = Some(e),
                }
            }
        }
        let mut v = json!({
            "id": card.id,
            "name": card.name,
            "created": created,
            "enabled": card.enabled,
            "ran_on_page": ran,
        });
        if !meta.warnings.is_empty() {
            v["warnings"] = json!(meta.warnings);
        }
        if let Some(n) = note {
            v["not_run"] = json!(n);
        }
        let verb = if created { "Created" } else { "Updated" };
        let ran_text = if ran { ", ran it on the page" } else { "" };
        let summary = format!("{verb} userscript {}{ran_text}", shorten(&card.name, 60));
        ToolOutcome { script: Some(card), ..ok_json(v, summary) }
    }

    // ------------------------------------------------------------ macros

    fn agent_macro_tool(&self, win: WindowId, name: &'static str, a: &Value, done: Done) {
        if !self.settings.borrow().macros.enabled {
            return done(ToolOutcome::refused("macros are turned off in Settings"));
        }
        match name {
            "list_macros" => {
                let list = self.macros_list_json();
                let n = list.as_array().map_or(0, Vec::len);
                done(ok_json(json!({ "macros": list }), format!("{n} macros")));
            }
            "save_macro" => match args::parse::<args::SaveMacro>(a) {
                Ok(a) => self.agent_save_macro(win, a, done),
                Err(e) => done(ToolOutcome::error(e)),
            },
            "run_macro" => {
                let a = match args::parse::<args::Id>(a) {
                    Ok(a) => a,
                    Err(e) => return done(ToolOutcome::error(e)),
                };
                self.macro_run(&a.id, true, Some(MACRO_EARLY_MS), move |r| {
                    done(match r {
                        Ok(text) if text == "running" => {
                            ok_json(json!({ "status": "running; the toolbar shows Stop" }), "The macro is running")
                        }
                        Ok(text) => ok_json(json!({ "status": format!("finished: {text}") }), "The macro finished"),
                        Err(e) => ToolOutcome::error(format!("the macro failed: {e}")),
                    })
                });
            }
            _ => done(ToolOutcome::error(format!("{name} is not available"))),
        }
    }

    /// Checks the macro's syntax (V8's own, in the target tab when the agent may act there),
    /// then saves it. A check that can't run doesn't stop the save.
    fn agent_save_macro(&self, win: WindowId, a: args::SaveMacro, done: Done) {
        let tab = self.agent_target(win).filter(|t| agent_pages::may_act(&t.url).is_ok()).map(|t| t.tab);
        let Some((tab, view)) = tab.and_then(|t| self.tab_view(t).map(|v| (t, v))) else {
            return done(self.agent_macro_saved(&a, false));
        };
        // Compiled only (not run), wrapped as the runner runs it: line N of the source is line
        // N of the wrapper (0-based there).
        if self.agent.cdp_tabs.borrow_mut().insert(tab) {
            self.cdp_use(tab, &view, CdpUser::Agent);
        }
        let params = json!({
            "expression": format!("(async()=>{{\n{}\n}})", a.source),
            "sourceURL": "",
            "persistScript": false,
        });
        self.agent_cdp_call(&view, "Runtime.compileScript", params, COMPILE_WAIT_MS, move |b, r| {
            done(match r {
                Ok(v) => match v.get("exceptionDetails") {
                    Some(e) => {
                        let line = e.get("lineNumber").and_then(Value::as_i64).unwrap_or(0).max(1);
                        let what = e
                            .pointer("/exception/description")
                            .and_then(Value::as_str)
                            .or_else(|| e.get("text").and_then(Value::as_str))
                            .unwrap_or("syntax error");
                        let what = shorten(what.lines().next().unwrap_or(what), 200);
                        ToolOutcome::error(format!("the macro was not saved: line {line}: {what}"))
                    }
                    None => b.agent_macro_saved(&a, true),
                },
                Err(_) => b.agent_macro_saved(&a, false),
            })
        });
    }

    /// Saves the macro of `save_macro` (`checked`: its syntax was checked).
    fn agent_macro_saved(&self, a: &args::SaveMacro, checked: bool) -> ToolOutcome {
        let created = a.id.is_none();
        match self.macro_save(a.id.as_deref(), &a.name, &a.source, a.hotkey.as_deref(), true) {
            Ok(card) => {
                let verb = if created { "Created" } else { "Updated" };
                let v = json!({
                    "id": card.id,
                    "name": card.name,
                    "created": created,
                    "enabled": card.enabled,
                    "syntax_checked": checked,
                });
                let summary = format!("{verb} macro {}", shorten(&card.name, 60));
                ToolOutcome { script: Some(card), ..ok_json(v, summary) }
            }
            Err(e) => ToolOutcome::error(e),
        }
    }

    // ------------------------------------------------------------ sensitive data

    /// History, bookmarks, downloads and cookies. The session offers these only when the
    /// user's message asked for them, and asks the user once per conversation.
    fn agent_sensitive_tool(&self, win: WindowId, category: Category, a: &Value, done: Done) {
        match category {
            Category::History => done(match args::parse::<args::GetHistory>(a) {
                Ok(a) => self.agent_history(a),
                Err(e) => ToolOutcome::error(e),
            }),
            Category::Bookmarks => done(match args::parse::<args::GetBookmarks>(a) {
                Ok(a) => self.agent_bookmarks(a),
                Err(e) => ToolOutcome::error(e),
            }),
            Category::Downloads => done(match args::parse::<args::GetDownloads>(a) {
                Ok(a) => self.agent_downloads(a),
                Err(e) => ToolOutcome::error(e),
            }),
            Category::Cookies => match args::parse::<args::GetCookies>(a) {
                Ok(a) => self.agent_cookies(win, a, done),
                Err(e) => done(ToolOutcome::error(e)),
            },
        }
    }

    fn agent_history(&self, a: args::GetHistory) -> ToolOutcome {
        let days = i64::from(a.days.unwrap_or(30).clamp(1, 365));
        let limit = a.limit.unwrap_or(50).clamp(1, 100);
        let since = now_ms() - days * DAY_MS;
        let rows = self.store.borrow().query_history(a.query.as_deref().unwrap_or(""), None, limit);
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                warning!("agent: history not read: {e}");
                return ToolOutcome::error("the history could not be read");
            }
        };
        let visits: Vec<Value> = rows
            .into_iter()
            .filter(|r| r.ts >= since)
            .map(|r| json!({ "url": r.url, "title": r.title, "time": utc_time(r.ts), "visits": r.visit_count }))
            .collect();
        let n = visits.len();
        ok_json(json!({ "visits": visits, "days": days }), format!("{n} history entries"))
    }

    fn agent_bookmarks(&self, a: args::GetBookmarks) -> ToolOutcome {
        let limit = a.limit.unwrap_or(100).clamp(1, 200) as usize;
        let query = a.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
        let (tree, found) = {
            let store = self.store.borrow();
            let tree = store.bookmark_tree();
            let found = query.map(|q| store.search_bookmarks(q));
            (tree, found)
        };
        let tree = match tree {
            Ok(t) => t,
            Err(e) => {
                warning!("agent: bookmarks not read: {e}");
                return ToolOutcome::error("the bookmarks could not be read");
            }
        };
        // Every bookmark with its folder path ("Bookmarks bar / News"), in tree order.
        let mut all = Vec::new();
        let mut paths = std::collections::HashMap::new();
        fn walk(
            nodes: &[lb_core::bookmarks::BookmarkNode],
            path: &str,
            all: &mut Vec<(i64, String, String, String)>,
            paths: &mut std::collections::HashMap<i64, String>,
        ) {
            for n in nodes {
                if n.item.is_folder {
                    let p = if path.is_empty() { n.item.title.clone() } else { format!("{path} / {}", n.item.title) };
                    walk(&n.children, &p, all, paths);
                } else if let Some(url) = &n.item.url {
                    paths.insert(n.item.id, path.to_string());
                    all.push((n.item.id, n.item.title.clone(), url.clone(), path.to_string()));
                }
            }
        }
        walk(&tree, "", &mut all, &mut paths);
        let list: Vec<(String, String, String)> = match found {
            Some(Ok(rows)) => rows
                .into_iter()
                .filter_map(|b| {
                    let folder = paths.get(&b.id).cloned().unwrap_or_default();
                    b.url.map(|u| (b.title, u, folder))
                })
                .collect(),
            Some(Err(e)) => {
                warning!("agent: bookmarks not searched: {e}");
                return ToolOutcome::error("the bookmarks could not be searched");
            }
            None => all.into_iter().map(|(_, t, u, f)| (t, u, f)).collect(),
        };
        let total = list.len();
        let items: Vec<Value> = list
            .into_iter()
            .take(limit)
            .map(|(title, url, folder)| json!({ "title": title, "url": url, "folder": folder }))
            .collect();
        let n = items.len();
        ok_json(json!({ "bookmarks": items, "total": total }), format!("{n} bookmarks"))
    }

    fn agent_downloads(&self, a: args::GetDownloads) -> ToolOutcome {
        let limit = a.limit.unwrap_or(50).clamp(1, 100) as usize;
        let mut items = self.downloads.borrow().items();
        items.sort_by_key(|x| std::cmp::Reverse(x.started));
        let list: Vec<Value> = items
            .into_iter()
            .take(limit)
            .map(|d| {
                let state = serde_json::to_value(d.state).unwrap_or(Value::Null);
                json!({
                    "name": d.file_name,
                    "url": d.url,
                    "state": state,
                    "size": d.total,
                    "received": d.received,
                    "date": utc_time(d.started),
                    "private": d.private,
                })
            })
            .collect();
        let n = list.len();
        ok_json(json!({ "downloads": list }), format!("{n} downloads"))
    }

    /// The cookies of a site, from the profile of the target tab (private tabs have their own).
    fn agent_cookies(&self, win: WindowId, a: args::GetCookies, done: Done) {
        let Some(host) = site_host(&a.site) else {
            return done(ToolOutcome::error("give a site like example.com"));
        };
        let site = lb_core::cookies::site_of(&host);
        let private = self.agent_target(win).and_then(|t| self.tab(t.tab).map(|t| t.private)).unwrap_or(false);
        let profile = if private { Profile::Private } else { Profile::Default };
        list_cookies(profile, move |all| {
            let cookies: Vec<Value> = all
                .into_iter()
                .filter(|c| c.site() == site)
                .map(|c| {
                    json!({
                        "name": c.name,
                        "domain": c.domain,
                        "path": c.path,
                        "expires": c.expires.map(utc_time),
                        "secure": c.secure,
                        "httpOnly": c.http_only,
                        "sameSite": c.same_site,
                        "value": c.value.chars().take(COOKIE_VALUE_CHARS).collect::<String>(),
                    })
                })
                .collect();
            let n = cookies.len();
            done(ok_json(json!({ "site": site, "cookies": cookies }), format!("{n} cookies")));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_sites() {
        assert_eq!(utc_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_time(1_762_783_380_000), "2025-11-10 14:03 UTC");
        assert_eq!(site_host("https://www.Example.com:8443/a?b").as_deref(), Some("www.example.com"));
        assert_eq!(site_host(".example.com").as_deref(), Some("example.com"));
        assert_eq!(site_host("example.com:8080/x").as_deref(), Some("example.com"));
        assert_eq!(site_host("  "), None);
        assert!(agent_url("javascript:alert(1)").is_err());
        assert!(agent_url("krunk://passwords").is_err());
        assert_eq!(agent_url("krunk://settings#privacy").unwrap(), Page::Settings.url_with_section(Some("privacy")));
        assert_eq!(agent_url("https://example.com").unwrap(), "https://example.com/");
    }
}
