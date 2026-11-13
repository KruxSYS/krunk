//! "Send client hints" off (Krunk 0.7.0, R12). The user-agent override without metadata
//! (`settings_host::apply_user_agent_to_then`) keeps the engine from sending any `Sec-CH-UA*`
//! header and from acting on `Accept-CH`/`Critical-CH`; the request hook removes any left
//! (`core::headers::Headers::hint_removals`); `ui/inject/nohints.js` removes the then empty
//! `navigator.userAgentData` in every frame, and tabs made while the setting is off delete it in
//! frames of other sites too (their process keeps the engine's metadata). Workers are separate
//! DevTools targets: while the setting is off each tab auto-attaches to its workers, paused at
//! their start, removes `userAgentData` and lets them run. (Shared workers are not the page's
//! targets: their script still sees the object; their requests carry no hints.)

use super::Browser;
use crate::log::info;
use lb_core::tabs::TabId;
use serde_json::{json, Value};

/// Run in a worker paused at its start.
const WORKER_SCRIPT: &str = "(function(){try{delete WorkerNavigator.prototype.userAgentData}catch(e){}})()";

impl Browser {
    fn hints_off(&self) -> bool {
        !self.settings.borrow().headers.send_client_hints
    }

    /// A tab's browser exists (before its first navigation), or the setting changed: attach
    /// to its workers and frames while client hints are off, stop when they are on again.
    pub(super) fn hints_attach(&self, tab: TabId) {
        let Some(view) = self.tab_view(tab) else { return };
        let off = self.hints_off();
        // 0.19.0: the WASM inspector reaches a tab's dedicated workers through the same
        // sessions (not paused at their start for it).
        let want = off || self.wasm_wants_workers(tab);
        if !want && !self.hints_attached.borrow().contains(&tab) {
            return;
        }
        if want {
            self.hints_attached.borrow_mut().insert(tab);
        } else {
            self.hints_attached.borrow_mut().remove(&tab);
            self.wasm_workers_forget(tab);
        }
        view.devtools_method("Target.setAutoAttach", &Self::hints_attach_params(want, off));
    }

    /// Workers only: an out-of-process frame paused this way did not resume in CEF; frames
    /// need no help (their requests follow the override, nohints.js runs in them).
    fn hints_attach_params(on: bool, wait: bool) -> Value {
        if on {
            json!({
                "autoAttach": true,
                "waitForDebuggerOnStart": wait,
                "flatten": true,
                "filter": [{ "type": "iframe", "exclude": true }, { "type": "page", "exclude": true }, {}],
            })
        } else {
            // (Chromium refuses a filter when auto-attach is switched off.)
            json!({ "autoAttach": false, "waitForDebuggerOnStart": false })
        }
    }

    /// 0.19.0, the WASM inspector: attaches to the tab's workers anew (off, then on), `done`
    /// when the engine has named the ones there are. The engine names a worker once, when it
    /// starts: the workers of a page the tab returned to with Back (out of the back/forward
    /// cache) are not named again without this. Every worker session of the tab is new after it.
    pub(super) fn hints_reattach(&self, tab: TabId, done: impl FnOnce() + 'static) {
        let Some(view) = self.tab_view(tab) else { return done() };
        self.hints_attached.borrow_mut().insert(tab);
        view.devtools_method("Target.setAutoAttach", &Self::hints_attach_params(false, false));
        // (Workers that run are not paused by this.)
        let params = Self::hints_attach_params(true, self.hints_off());
        view.devtools_call("Target.setAutoAttach", &params, move |_| done());
    }

    /// A worker or frame of `tab` was attached (paused): fix it up, then let it run.
    pub(super) fn hints_target_attached(&self, tab: TabId, params: &Value) {
        let Some(view) = self.tab_view(tab) else { return };
        let Some(session) = params.get("sessionId").and_then(Value::as_str) else { return };
        let kind = params.pointer("/targetInfo/type").and_then(Value::as_str).unwrap_or("");
        let waiting = params.get("waitingForDebugger").and_then(Value::as_bool).unwrap_or(false);
        info!("tab {tab}: attached {kind} target (waiting {waiting})");
        if self.hints_off() {
            match kind {
                "worker" | "service_worker" | "shared_worker" => {
                    view.devtools_session_method(
                        session,
                        "Runtime.evaluate",
                        &json!({ "expression": WORKER_SCRIPT, "silent": true }),
                    );
                    info!("tab {tab}: {kind} without client hints");
                }
                _ => {}
            }
        }
        // Whatever it is: it waits for this.
        if waiting {
            view.devtools_session_method(session, "Runtime.runIfWaitingForDebugger", &json!({}));
        }
    }
}
