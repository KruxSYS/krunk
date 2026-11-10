//! Userscripts (0.12.0, WP-E): one loader per tab, registered as new-document scripts in the
//! main world, re-registered when scripts or values change; GM calls through a hidden binding.
//!
//! - The loader (`core::userscripts::bundle_parts`) takes the DevTools binding off `window`
//!   before any page script runs; every GM call carries the tab's nonce, which page code can't
//!   read, and is parsed strictly (`GmCall`).
//! - The scripts' GM values are a snapshot in the loader: a change is stored at once, and the
//!   tabs get a fresh loader (for their next page) a moment later.
//! - Private tabs get userscripts only with "Run userscripts in private tabs" on; a private
//!   tab's runs don't update "last run".
//! - The log names tabs, script ids and counts only, never sources, values or addresses.

use super::cdp::CdpUser;
use super::Browser;
use crate::ffi::cef::View;
use crate::log::{info, warning};
use lb_core::agent::{ScriptCard, ScriptKind};
use lb_core::protocol::Event;
use lb_core::scripts_store::{new_id, valid_id, UserscriptRow};
use lb_core::storage::now_ms;
use lb_core::tabs::{Position, TabId};
use lb_core::userscripts::{self as us, GmCall, LoaderScript};
use serde_json::{json, Value};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// The DevTools events the userscripts listen to.
const EVENTS: &[&str] = &["Runtime.bindingCalled"];
/// GM value changes reach the other tabs' loaders after this pause (many changes, one update).
const REFRESH_DELAY_MS: i64 = 300;
/// "Last run" changes reach krunk://scripts at most this often.
const NOTIFY_DELAY_MS: i64 = 1000;
/// `GM_openInTab`: at most one tab per second per tab.
const OPEN_TAB_GAP_MS: i64 = 1000;
/// `GM_notification`: at most two toasts per second per tab.
const TOAST_GAP_MS: i64 = 500;
const TOAST_MAX_CHARS: usize = 300;
const URL_MAX_BYTES: usize = 8192;

#[derive(Default)]
pub(super) struct UserscriptHost {
    /// The binding's name: random per process ("__krunkUS_<16 hex>").
    binding: OnceCell<String>,
    /// Tabs whose browser exists.
    tabs: RefCell<HashMap<TabId, TabState>>,
    /// The enabled scripts with their values, built once per change (`None`: rebuild).
    loaded: RefCell<Option<Rc<Vec<LoaderScript>>>>,
    /// Each registration's number: replies to an older one are removed again.
    generation: Cell<u64>,
    refresh_pending: Cell<bool>,
    notify_pending: Cell<bool>,
}

/// A tab's loader.
struct TabState {
    /// Every GM call from this tab carries it (random per tab browser).
    nonce: String,
    private: bool,
    /// The registered new-document scripts' identifiers.
    scripts: Vec<String>,
    generation: u64,
    /// The binding is added in this tab.
    bound: bool,
    last_open: i64,
    last_toast: i64,
}

/// At most `max` characters.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

impl Browser {
    fn userscripts_binding(&self) -> String {
        self.userscripts.binding.get_or_init(|| format!("__krunkUS_{}", &new_id()[..16])).clone()
    }

    pub(super) fn userscripts_attach_tab(&self, tab: TabId, view: &View) {
        let weak = self.me.clone();
        view.add_devtools_listener(EVENTS, move |_, params| {
            if let Some(b) = weak.upgrade() {
                b.userscripts_binding_called(tab, params);
            }
        });
    }

    /// The tab's browser exists: register its loader.
    pub(super) fn userscripts_tab_created(&self, tab: TabId) {
        let Some(view) = self.tab_view(tab) else { return };
        let state = TabState {
            nonce: new_id(),
            private: view.is_private(),
            scripts: Vec::new(),
            generation: 0,
            bound: false,
            last_open: 0,
            last_toast: 0,
        };
        self.userscripts.tabs.borrow_mut().insert(tab, state);
        self.userscripts_register(tab, &view);
    }

    pub(super) fn userscripts_detach_tab(&self, tab: TabId) {
        self.userscripts.tabs.borrow_mut().remove(&tab);
    }

    pub(super) fn userscripts_settings_changed(&self) {
        self.userscripts_refresh_all();
    }

    /// Scripts changed (saved, toggled, deleted, imported): every tab gets a new loader and
    /// krunk://scripts its lists.
    pub(super) fn userscripts_changed(&self) {
        self.userscripts_refresh_all();
        self.notify_scripts_changed();
    }

    // ------------------------------------------------------------ registration

    /// The enabled scripts that parse, with their values.
    fn userscripts_loaded(&self) -> Rc<Vec<LoaderScript>> {
        if let Some(list) = self.userscripts.loaded.borrow().as_ref() {
            return list.clone();
        }
        let list: Vec<LoaderScript> = {
            let store = self.store.borrow();
            let rows = store.userscripts().unwrap_or_else(|e| {
                warning!("userscripts: list failed: {e}");
                Vec::new()
            });
            rows.into_iter()
                .filter(|r| r.enabled)
                .filter_map(|r| {
                    let meta = us::parse_meta(&r.source).ok()?;
                    let values = store.gm_values(&r.id).unwrap_or_default();
                    Some(LoaderScript { id: r.id, meta, source: r.source, values })
                })
                .collect()
        };
        let list = Rc::new(list);
        *self.userscripts.loaded.borrow_mut() = Some(list.clone());
        list
    }

    /// Do userscripts run in this tab at all (the settings)?
    fn userscripts_allowed(&self, private: bool) -> bool {
        let s = self.settings.borrow();
        s.userscripts.enabled && (!private || s.userscripts.private_tabs)
    }

    /// (Re)registers a tab's loader: the old scripts go, the domains and the binding are held
    /// while there is something to run.
    fn userscripts_register(&self, tab: TabId, view: &View) {
        let scripts =
            if self.userscripts_allowed(view.is_private()) { self.userscripts_loaded() } else { Rc::new(Vec::new()) };
        let generation = self.userscripts.generation.get() + 1;
        self.userscripts.generation.set(generation);
        let (old, nonce, bound) = {
            let mut tabs = self.userscripts.tabs.borrow_mut();
            let Some(st) = tabs.get_mut(&tab) else { return };
            st.generation = generation;
            (std::mem::take(&mut st.scripts), st.nonce.clone(), std::mem::replace(&mut st.bound, !scripts.is_empty()))
        };
        for id in old {
            view.devtools_method("Page.removeScriptToEvaluateOnNewDocument", &json!({ "identifier": id }));
        }
        let binding = self.userscripts_binding();
        if scripts.is_empty() {
            if bound {
                view.devtools_method("Runtime.removeBinding", &json!({ "name": binding }));
            }
            if self.cdp_release(tab, view, CdpUser::Userscripts) {
                info!("tab {tab}: no userscripts");
            }
            return;
        }
        // The Page domain (new-document scripts run only while it is on) and Runtime (the
        // binding calls).
        self.cdp_use(tab, view, CdpUser::Userscripts);
        if !bound {
            view.devtools_method("Runtime.addBinding", &json!({ "name": binding }));
        }
        let parts = us::bundle_parts(&scripts, &binding, &nonce);
        info!("tab {tab}: {} userscripts registered", scripts.len());
        for part in parts {
            let weak = self.me.clone();
            let params = json!({ "source": part, "runImmediately": false });
            view.devtools_call("Page.addScriptToEvaluateOnNewDocument", &params, move |r| {
                let Some(b) = weak.upgrade() else { return };
                match r {
                    Ok(v) => {
                        let Some(id) = v.get("identifier").and_then(Value::as_str) else { return };
                        let current = match b.userscripts.tabs.borrow_mut().get_mut(&tab) {
                            Some(st) if st.generation == generation => {
                                st.scripts.push(id.to_string());
                                true
                            }
                            _ => false,
                        };
                        if !current {
                            // A newer registration (or none) replaced this one meanwhile.
                            if let Some(view) = b.tab_view(tab) {
                                view.devtools_method(
                                    "Page.removeScriptToEvaluateOnNewDocument",
                                    &json!({ "identifier": id }),
                                );
                            }
                        }
                    }
                    Err(e) => warning!("tab {tab}: userscripts not registered: {e}"),
                }
            });
        }
    }

    fn userscripts_refresh_all(&self) {
        *self.userscripts.loaded.borrow_mut() = None;
        let tabs: Vec<TabId> = self.userscripts.tabs.borrow().keys().copied().collect();
        for tab in tabs {
            if let Some(view) = self.tab_view(tab) {
                self.userscripts_register(tab, &view);
            }
        }
    }

    /// GM values changed: the loaders follow after a short pause.
    fn userscripts_refresh_soon(&self) {
        *self.userscripts.loaded.borrow_mut() = None;
        if self.userscripts.refresh_pending.replace(true) {
            return;
        }
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(REFRESH_DELAY_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.userscripts.refresh_pending.set(false);
                b.userscripts_refresh_all();
            }
        });
    }

    /// A "last run" changed: krunk://scripts hears about it, at most once a second.
    fn userscripts_notify_soon(&self) {
        if self.userscripts.notify_pending.replace(true) {
            return;
        }
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(NOTIFY_DELAY_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.userscripts.notify_pending.set(false);
                b.notify_scripts_changed();
            }
        });
    }

    // ------------------------------------------------------------ GM calls

    fn userscripts_binding_called(&self, tab: TabId, params: &Value) {
        if params.get("name").and_then(Value::as_str) != Some(self.userscripts_binding().as_str()) {
            return;
        }
        let payload = params.get("payload").and_then(Value::as_str).unwrap_or("");
        if payload.len() > us::MAX_CALL_BYTES {
            warning!("tab {tab}: userscript call too large: ignored");
            return;
        }
        let Some(call) = GmCall::parse(payload) else {
            warning!("tab {tab}: malformed userscript call: ignored");
            return;
        };
        let private = match self.userscripts.tabs.borrow().get(&tab) {
            Some(st) if st.nonce == call.nonce() => st.private,
            _ => {
                warning!("tab {tab}: userscript call with a wrong nonce: ignored");
                return;
            }
        };
        if !self.userscripts_allowed(private) {
            return;
        }
        let script = call.script().to_string();
        let name = if valid_id(&script) {
            self.store.borrow().userscript(&script).ok().flatten().map(|r| r.name)
        } else {
            None
        };
        let Some(name) = name else {
            warning!("tab {tab}: userscript call for an unknown script: ignored");
            return;
        };
        let op = call.op();
        match call {
            GmCall::Set { key, value, .. } => {
                if serde_json::from_str::<Value>(&value).is_err() {
                    warning!("tab {tab}: userscript {script} set: not JSON, ignored");
                    return;
                }
                match self.store.borrow().gm_set(&script, &key, &value, us::MAX_VALUES_BYTES) {
                    Ok(true) => {}
                    Ok(false) => {
                        warning!("tab {tab}: userscript {script} storage full (1 MB): value not saved");
                        return;
                    }
                    Err(e) => {
                        warning!("tab {tab}: userscript {script} set failed: {e}");
                        return;
                    }
                }
                self.userscripts_refresh_soon();
            }
            GmCall::Delete { key, .. } => {
                if let Err(e) = self.store.borrow().gm_delete(&script, &key) {
                    warning!("tab {tab}: userscript {script} delete failed: {e}");
                    return;
                }
                self.userscripts_refresh_soon();
            }
            GmCall::Clipboard { text, .. } => {
                if text.len() > us::MAX_CLIPBOARD_BYTES {
                    warning!("tab {tab}: userscript {script} clipboard text too long: ignored");
                    return;
                }
                let window = self.window_of(tab).and_then(|w| self.frame(w)).map(|f| f.window);
                let copied = window.is_some_and(|w| crate::ffi::secure::copy_text(w.hwnd(), &text));
                if !copied {
                    warning!("tab {tab}: userscript {script} clipboard: copy failed");
                }
            }
            GmCall::Notify { text, title, .. } => {
                if !self.userscripts_gap(tab, |st| &mut st.last_toast, TOAST_GAP_MS) {
                    return;
                }
                let head = if title.trim().is_empty() { name } else { title };
                let message = clip(&format!("{}: {}", head.trim(), text.trim()), TOAST_MAX_CHARS);
                if let Some(win) = self.window_of(tab) {
                    self.post_shell_to(win, &Event::Toast { message });
                }
            }
            GmCall::OpenTab { url, background, .. } => {
                let lower = url.to_ascii_lowercase();
                if url.len() > URL_MAX_BYTES || !(lower.starts_with("https://") || lower.starts_with("http://")) {
                    warning!("tab {tab}: userscript {script} openTab: not an http(s) address, refused");
                    return;
                }
                if !self.userscripts_gap(tab, |st| &mut st.last_open, OPEN_TAB_GAP_MS) {
                    warning!("tab {tab}: userscript {script} openTab: too often, refused");
                    return;
                }
                let Some(win) = self.window_of(tab) else { return };
                let private = self.tab(tab).is_some_and(|t| t.private);
                let opened =
                    self.ws.borrow_mut().open(win, &url, private, Position::AfterOpener(tab), !background, now_ms());
                if let Some((_, ops)) = opened {
                    self.apply(ops);
                }
            }
            GmCall::Ran { .. } => {
                if !private {
                    if let Err(e) = self.store.borrow().touch_userscript_run(&script, now_ms()) {
                        warning!("tab {tab}: userscript {script} last run not saved: {e}");
                    }
                    self.userscripts_notify_soon();
                }
            }
        }
        if op != "ran" {
            info!("tab {tab}: userscript {script} {op}");
        }
    }

    /// Rate limit: true (and the time noted) when the last time in `field` is `gap` ms ago.
    fn userscripts_gap(&self, tab: TabId, field: impl FnOnce(&mut TabState) -> &mut i64, gap: i64) -> bool {
        let now = now_ms();
        let mut tabs = self.userscripts.tabs.borrow_mut();
        let Some(st) = tabs.get_mut(&tab) else { return false };
        let last = field(st);
        if now - *last < gap {
            return false;
        }
        *last = now;
        true
    }

    // ------------------------------------------------------------ the API

    /// Creates or updates a userscript from its source (`id` None = new; a new one is enabled).
    pub(super) fn userscript_save(&self, id: Option<&str>, source: &str, by_ai: bool) -> Result<ScriptCard, String> {
        if source.len() > us::MAX_SOURCE_BYTES {
            return Err("The script is too long (at most 200 KB).".into());
        }
        let meta = us::parse_meta(source)?;
        let now = now_ms();
        let row = match id {
            Some(id) => {
                let old = valid_id(id).then(|| self.store.borrow().userscript(id).ok().flatten()).flatten();
                let old = old.ok_or_else(|| "No such userscript.".to_string())?;
                UserscriptRow {
                    name: meta.name.clone(),
                    namespace: meta.namespace.clone(),
                    version: meta.version.clone(),
                    source: source.to_string(),
                    updated: now,
                    ..old
                }
            }
            None => UserscriptRow {
                id: new_id(),
                name: meta.name.clone(),
                namespace: meta.namespace.clone(),
                version: meta.version.clone(),
                source: source.to_string(),
                enabled: true,
                created_by_ai: by_ai,
                created: now,
                updated: now,
                last_run: None,
                position: 0,
            },
        };
        if let Err(e) = self.store.borrow().save_userscript(&row) {
            warning!("userscript {} not saved: {e}", row.id);
            return Err("The script could not be saved.".into());
        }
        info!(
            "userscript {} saved ({}, {} warnings{})",
            row.id,
            if id.is_some() { "updated" } else { "new" },
            meta.warnings.len(),
            if by_ai { ", by the agent" } else { "" }
        );
        self.userscripts_changed();
        Ok(ScriptCard { id: row.id, kind: ScriptKind::Userscript, name: row.name, enabled: row.enabled })
    }

    pub(super) fn userscript_toggle(&self, id: &str, enabled: bool) -> Result<ScriptCard, String> {
        let changed =
            valid_id(id) && self.store.borrow().set_userscript_enabled(id, enabled, now_ms()).unwrap_or(false);
        if !changed {
            return Err("No such userscript.".into());
        }
        let name = self.store.borrow().userscript(id).ok().flatten().map(|r| r.name).unwrap_or_default();
        info!("userscript {id} {}", if enabled { "enabled" } else { "disabled" });
        self.userscripts_changed();
        Ok(ScriptCard { id: id.to_string(), kind: ScriptKind::Userscript, name, enabled })
    }

    /// Deletes a userscript and its values. Ok(false): there was no such script.
    pub(super) fn userscript_delete(&self, id: &str) -> Result<bool, String> {
        if !valid_id(id) {
            return Ok(false);
        }
        let deleted = self.store.borrow().delete_userscript(id).map_err(|e| {
            warning!("userscript {id} not deleted: {e}");
            "The script could not be deleted.".to_string()
        })?;
        if deleted {
            info!("userscript {id} deleted");
            self.userscripts_changed();
        }
        Ok(deleted)
    }

    /// Runs userscript `id` once in tab `tab` now (if it matches the tab's address unless
    /// `force`). Ok(true) = it ran.
    pub(super) fn userscript_run_now(&self, id: &str, tab: TabId, force: bool) -> Result<bool, String> {
        let Some(view) = self.tab_view(tab) else { return Err("No such tab.".into()) };
        if !self.settings.borrow().userscripts.enabled {
            return Err("Userscripts are turned off in Settings.".into());
        }
        if !self.userscripts_allowed(view.is_private()) {
            return Err("Userscripts don't run in private tabs (see Settings).".into());
        }
        let row = valid_id(id).then(|| self.store.borrow().userscript(id).ok().flatten()).flatten();
        let row = row.ok_or_else(|| "No such userscript.".to_string())?;
        let meta = us::parse_meta(&row.source)?;
        let url = self.tab(tab).map(|t| t.url.clone()).unwrap_or_default();
        if !us::injectable_url(&url) {
            return Err("Userscripts don't run on this page.".into());
        }
        if !force && !us::matches_url(&meta, &url) {
            return Ok(false);
        }
        let (nonce, bound) = {
            let mut tabs = self.userscripts.tabs.borrow_mut();
            let Some(st) = tabs.get_mut(&tab) else { return Err("The tab is not ready yet.".into()) };
            (st.nonce.clone(), std::mem::replace(&mut st.bound, true))
        };
        let binding = self.userscripts_binding();
        self.cdp_use(tab, &view, CdpUser::Userscripts);
        // The page's loader took the binding off `window` already: adding it again puts it
        // back (into the page's current documents) for this run's loader to take.
        if bound {
            view.devtools_method("Runtime.removeBinding", &json!({ "name": binding }));
        }
        view.devtools_method("Runtime.addBinding", &json!({ "name": binding }));
        let values = self.store.borrow().gm_values(&row.id).unwrap_or_default();
        let script = LoaderScript { id: row.id.clone(), meta, source: row.source, values };
        let params = json!({
            "expression": us::bundle_one(&script, &binding, &nonce, force),
            "userGesture": false,
            "awaitPromise": false,
            "returnByValue": true,
        });
        let id = row.id;
        info!("tab {tab}: userscript {id} run now{}", if force { " (forced)" } else { "" });
        view.devtools_call("Runtime.evaluate", &params, move |r| match r {
            Ok(v) if v.get("exceptionDetails").is_some() => {
                warning!("tab {tab}: userscript {id} run now: script error")
            }
            Ok(_) => {}
            Err(e) => warning!("tab {tab}: userscript {id} run now failed: {e}"),
        });
        Ok(true)
    }

    /// The userscripts for the agent's `list_userscripts`.
    pub(super) fn userscripts_list_json(&self) -> serde_json::Value {
        let rows = self.store.borrow().userscripts().unwrap_or_default();
        Value::Array(
            rows.into_iter()
                .map(|r| {
                    let meta = us::parse_meta(&r.source).unwrap_or_default();
                    json!({
                        "id": r.id,
                        "name": r.name,
                        "enabled": r.enabled,
                        "matches": meta.matches,
                        "includes": meta.includes,
                        "createdByAi": r.created_by_ai,
                        "lastRun": r.last_run,
                    })
                })
                .collect(),
        )
    }
}
