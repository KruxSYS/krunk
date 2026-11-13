//! krunk://scripts (0.12.0, WP-G): the lists, import/export, the editor's parse check
//! (`Runtime.compileScript` in the page's own tab) and hotkey recording.

use super::cdp::CdpUser;
use super::Browser;
use crate::ffi::dialogs;
use crate::log::{info, warning};
use lb_core::agent::ScriptKind;
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::scripts_store::valid_id;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::userscripts as us;
use serde_json::{json, Value};
use std::cell::Cell;
use std::rc::Rc;

impl Browser {
    /// Tells every open krunk://scripts page that the lists changed (a save, toggle, delete, a
    /// script the agent made, a last-run time).
    pub(super) fn notify_scripts_changed(&self) {
        self.post_scripts_pages(&Event::ScriptsChanged);
    }

    /// Posts an event to every tab showing krunk://scripts.
    pub(super) fn post_scripts_pages(&self, event: &Event) {
        let tabs: Vec<TabId> = match self.ws.try_borrow() {
            Ok(ws) => {
                ws.tabs().filter(|(_, t)| Page::from_url(&t.url) == Some(Page::Scripts)).map(|(_, t)| t.id).collect()
            }
            Err(_) => return,
        };
        if tabs.is_empty() {
            return;
        }
        let json = event.to_json();
        for id in tabs {
            if let Some(v) = self.tab_view(id) {
                v.post_json(&json);
            }
        }
    }

    pub(super) fn scripts_request(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let reply = |r: Result<Value, String>| match r {
            Ok(v) => self.reply(tab, Event::ok(req_id, v)),
            Err(e) => self.reply(tab, Event::err(req_id, e)),
        };
        match request {
            Request::ScriptsList => reply(Ok(self.scripts_list())),
            Request::ScriptGet { id } => reply(self.script_get(&id)),
            Request::ScriptSave { id, source } => {
                reply(self.userscript_save(id.as_deref(), &source, false).map(|card| {
                    let warnings = us::parse_meta(&source).map(|m| m.warnings).unwrap_or_default();
                    json!({ "id": card.id, "name": card.name, "warnings": warnings })
                }))
            }
            Request::ScriptToggle { id, enabled } => {
                reply(self.userscript_toggle(&id, enabled).map(|c| json!({ "id": c.id, "enabled": c.enabled })))
            }
            Request::ScriptDelete { id } => reply(self.userscript_delete(&id).map(|_| Value::Null)),
            Request::ScriptExport { id } => reply(self.script_export(&id)),
            Request::ScriptRunNow { id } => reply(self.script_run_now(tab, &id)),
            Request::ScriptCheck { source, kind } => self.script_check(tab, req_id, source, kind),
            Request::MacroSave { id, name, source, hotkey } => reply(
                self.macro_save(id.as_deref(), &name, &source, hotkey.as_deref(), false).map(|c| json!({ "id": c.id })),
            ),
            Request::MacroDelete { id } => reply(self.scripts_macro_delete(&id)),
            Request::MacroToggle { id, enabled } => reply(self.scripts_macro_toggle(&id, enabled)),
            Request::MacroRun { id } => self.scripts_macro_run(tab, req_id, &id),
            Request::MacroStop => {
                self.stop_macros("stopped on the scripts page");
                reply(Ok(Value::Null));
            }
            Request::HotkeyCheck { chord, id } => {
                let r = match self.macro_hotkey_check(&chord, id.as_deref()) {
                    Ok(canonical) => json!({ "chord": canonical, "conflict": null }),
                    Err(problem) => json!({ "chord": chord, "conflict": problem }),
                };
                reply(Ok(r));
            }
            Request::HotkeyRecord { on, id } => match tab {
                Some(t) => {
                    self.macro_record_for(t, on, id.filter(|i| lb_core::scripts_store::valid_id(i)));
                    reply(Ok(Value::Null));
                }
                None => reply(Err("No such tab.".into())),
            },
            _ => reply(Err("Not a scripts command".into())),
        }
    }

    /// Both lists, the running macro and whether each kind is turned on in Settings.
    fn scripts_list(&self) -> Value {
        let (rows, macros) = {
            let store = self.store.borrow();
            (store.userscripts().unwrap_or_default(), store.macros().unwrap_or_default())
        };
        let userscripts: Vec<Value> = rows
            .into_iter()
            .map(|r| {
                // 0.19.0: a script that may use the WASM memory inspector (`@grant GM.wasm`) says so
                // in the list.
                let (matches, warnings, wasm) = match us::parse_meta(&r.source) {
                    Ok(m) => {
                        let wasm = us::grants_wasm(&m);
                        (m.matches.into_iter().chain(m.includes).collect(), m.warnings, wasm)
                    }
                    Err(e) => (Vec::new(), vec![e], false),
                };
                json!({
                    "id": r.id,
                    "name": r.name,
                    "namespace": r.namespace,
                    "version": r.version,
                    "enabled": r.enabled,
                    "createdByAi": r.created_by_ai,
                    "matches": matches,
                    "lastRun": r.last_run,
                    "updated": r.updated,
                    "warnings": warnings,
                    "wasm": wasm,
                })
            })
            .collect();
        let macros: Vec<Value> = macros
            .into_iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "name": m.name,
                    "hotkey": m.hotkey,
                    "enabled": m.enabled,
                    "createdByAi": m.created_by_ai,
                    "lastRun": m.last_run,
                })
            })
            .collect();
        let enabled = {
            let s = self.settings.borrow();
            json!({ "userscripts": s.userscripts.enabled, "macros": s.macros.enabled })
        };
        json!({
            "userscripts": userscripts,
            "macros": macros,
            "running": self.running_macro_id(),
            "enabled": enabled,
        })
    }

    /// A userscript or a macro for the editor.
    fn script_get(&self, id: &str) -> Result<Value, String> {
        const NONE: &str = "No such script";
        if !valid_id(id) {
            return Err(NONE.into());
        }
        let store = self.store.borrow();
        if let Some(r) = store.userscript(id).ok().flatten() {
            let warnings = us::parse_meta(&r.source).map(|m| m.warnings).unwrap_or_default();
            return Ok(json!({
                "kind": "userscript",
                "id": r.id,
                "name": r.name,
                "source": r.source,
                "enabled": r.enabled,
                "warnings": warnings,
            }));
        }
        if let Some(m) = store.macro_by_id(id).ok().flatten() {
            return Ok(json!({
                "kind": "macro",
                "id": m.id,
                "name": m.name,
                "source": m.source,
                "hotkey": m.hotkey,
                "enabled": m.enabled,
            }));
        }
        Err(NONE.into())
    }

    /// Saves a userscript's source to a file the user picks: `{saved}`.
    fn script_export(&self, id: &str) -> Result<Value, String> {
        let row = valid_id(id).then(|| self.store.borrow().userscript(id).ok().flatten()).flatten();
        let row = row.ok_or_else(|| "No such userscript.".to_string())?;
        let Some(w) = self.window() else { return Ok(json!({ "saved": false })) };
        let name = lb_core::downloads::sanitize_file_name(&format!("{}.user.js", row.name));
        let Some(path) = dialogs::save_file(w.hwnd(), "Export userscript", &name, ("Userscripts", "*.user.js"), None)
        else {
            return Ok(json!({ "saved": false }));
        };
        std::fs::write(&path, row.source.as_bytes()).map_err(|e| {
            warning!("userscript {id}: export failed: {e}");
            "The file could not be written.".to_string()
        })?;
        info!("userscript {id} exported");
        Ok(json!({ "saved": true }))
    }

    /// Runs a userscript once on the most recently used web tab of the page's window:
    /// `{ran, title}`.
    fn script_run_now(&self, page_tab: Option<TabId>, id: &str) -> Result<Value, String> {
        let target = {
            let ws = self.ws.borrow();
            let win = page_tab.and_then(|t| ws.window_of(t)).or_else(|| ws.current());
            win.and_then(|w| ws.model(w)).and_then(|m| {
                m.tabs()
                    .iter()
                    .filter(|t| Some(t.id) != page_tab)
                    .filter(|t| t.url.starts_with("https://") || t.url.starts_with("http://"))
                    .max_by_key(|t| t.last_active)
                    .map(|t| (t.id, t.title.clone()))
            })
        };
        let (tab, title) = target.ok_or_else(|| "Open a web page in this window first.".to_string())?;
        let ran = self.userscript_run_now(id, tab, true)?;
        Ok(json!({ "ran": ran, "title": title }))
    }

    /// The parse check: the engine compiles the source in the scripts page's own tab (nothing
    /// runs), wrapped the way it runs. Replies `{ok:false, line, column, message}` (1-based)
    /// or `{ok:true, warnings}`.
    fn script_check(&self, tab: Option<TabId>, req_id: Option<u32>, source: String, kind: ScriptKind) {
        let Some((tab, view)) = tab.and_then(|t| self.tab_view(t).map(|v| (t, v))) else {
            self.reply(tab, Event::err(req_id, "No such tab."));
            return;
        };
        if source.len() > us::MAX_SOURCE_BYTES {
            self.reply(Some(tab), Event::err(req_id, "The script is too long (at most 200 KB)."));
            return;
        }
        // Userscripts run as the body of a plain function, macros of an async one: `return`
        // and (for macros) `await` are fine at the top level. The wrapper adds one line.
        let expression = match kind {
            ScriptKind::Userscript => format!("(function () {{\n{source}\n}})"),
            ScriptKind::Macro => format!("(async () => {{\n{source}\n}})"),
        };
        let lines = source.lines().count().max(1) as u64;
        self.cdp_use(tab, &view, CdpUser::ScriptsPage);
        let params = json!({ "expression": expression, "sourceURL": "script.js", "persistScript": false });
        let weak = self.me.clone();
        let release = view.clone();
        view.devtools_call("Runtime.compileScript", &params, move |r| {
            let Some(b) = weak.upgrade() else { return };
            // Runtime stays on only as long as another user needs it.
            b.cdp_release(tab, &release, CdpUser::ScriptsPage);
            let result = match r {
                Ok(v) => match v.get("exceptionDetails") {
                    Some(d) => {
                        let at = |k: &str| d.get(k).and_then(Value::as_u64).unwrap_or(0);
                        // 0-based, on the wrapped text: its first line is the wrapper's.
                        let line = at("lineNumber").clamp(1, lines);
                        let column = at("columnNumber") + 1;
                        let message = d
                            .get("exception")
                            .and_then(|e| e.get("description"))
                            .and_then(Value::as_str)
                            .or_else(|| d.get("text").and_then(Value::as_str))
                            .and_then(|s| s.lines().next())
                            .unwrap_or("Syntax error")
                            .to_string();
                        json!({ "ok": false, "line": line, "column": column, "message": message })
                    }
                    None => match kind {
                        ScriptKind::Userscript => match us::parse_meta(&source) {
                            Ok(m) => json!({ "ok": true, "warnings": m.warnings }),
                            Err(e) => json!({ "ok": false, "line": 1, "column": 1, "message": e }),
                        },
                        ScriptKind::Macro => json!({ "ok": true, "warnings": [] }),
                    },
                },
                Err(e) => {
                    warning!("tab {tab}: script check failed: {e}");
                    b.reply(Some(tab), Event::err(req_id, "The check could not run. Try again."));
                    return;
                }
            };
            b.reply(Some(tab), Event::ok(req_id, result));
        });
    }

    fn scripts_macro_toggle(&self, id: &str, enabled: bool) -> Result<Value, String> {
        let changed = valid_id(id) && self.store.borrow().set_macro_enabled(id, enabled, now_ms()).unwrap_or(false);
        if !changed {
            return Err("No such macro".into());
        }
        info!("macro {id} {}", if enabled { "enabled" } else { "disabled" });
        if !enabled && self.running_macro_id().as_deref() == Some(id) {
            self.stop_macros("macro turned off");
        }
        self.notify_scripts_changed();
        Ok(Value::Null)
    }

    fn scripts_macro_delete(&self, id: &str) -> Result<Value, String> {
        if !valid_id(id) {
            return Ok(Value::Null);
        }
        if self.running_macro_id().as_deref() == Some(id) {
            self.stop_macros("macro deleted");
        }
        let deleted = self.store.borrow().delete_macro(id).map_err(|e| {
            warning!("macro {id} not deleted: {e}");
            "The macro could not be deleted".to_string()
        })?;
        if deleted {
            info!("macro {id} deleted");
            self.notify_scripts_changed();
        }
        Ok(Value::Null)
    }

    /// Starts a macro. The reply comes as soon as it started (or failed to start); its end
    /// reaches the page as `macroState`.
    fn scripts_macro_run(&self, tab: Option<TabId>, req_id: Option<u32>, id: &str) {
        let replied = Rc::new(Cell::new(false));
        let weak = self.me.clone();
        let flag = replied.clone();
        let win = tab.and_then(|t| self.window_of(t));
        self.macro_run(id, win, false, None, move |r| {
            if flag.replace(true) {
                return;
            }
            // Ended (or failed) before the start returned: that is the answer.
            if let Some(b) = weak.upgrade() {
                match r {
                    Ok(_) => b.reply(tab, Event::ok(req_id, Value::Null)),
                    Err(e) => b.reply(tab, Event::err(req_id, e)),
                }
            }
        });
        if !replied.replace(true) {
            self.reply(tab, Event::ok(req_id, Value::Null));
        }
    }
}
