//! The WASM memory inspector's host side (0.19.0): `krunk://wasm`, the user scripts' `GM.wasm`
//! and the AI agent's `wasm_*` tools all end in [`Browser::wasm_call`].
//!
//! How the memories are reached, without touching the page's globals: per inspected tab and
//! context the host makes one probe instance (`ui/inject/wasm-probe.js`) with
//! `Runtime.evaluate` and keeps only its DevTools handle (object group `krunk-wasm`); the
//! `WebAssembly.Memory` objects are found with `Runtime.queryObjects` on
//! `WebAssembly.Memory.prototype` (on `list` and when a probe is new: the query makes the
//! engine collect garbage, too costly for every read) and handed to the probe with
//! `Runtime.callFunctionOn`. The probe numbers them; requests name memories by number.
//!
//! Contexts: 0 is the page (the main frame's main world); frames of the same process have
//! their execution context's number (`Runtime.executionContextCreated`, default worlds only);
//! dedicated workers are auto-attached DevTools targets (flat sessions, shared with `hints`)
//! and count from `WORKER_CTX_BASE`. Not reachable: frames of other sites (another process;
//! they would need their own target sessions), service and shared workers.
//!
//! The fallback ("hook and reload" on the page): a document-start script makes the probe
//! before any page script, hooks `WebAssembly.Memory`/`instantiate`/`instantiateStreaming`/
//! `Instance` and leaves its instance under a random, non-enumerable property until the host
//! takes it (and deletes the property) with the next call. Both ways feed the one registry
//! of that instance.
//!
//! A session lives in memory only: it starts with the first call for a tab and ends when the
//! tab closes or gets a new browser, or when the last inspector page of the tab closes (scans
//! and freezes are reset, the handles released). A navigation ends the page's probe by itself.

use super::cdp::CdpUser;
use super::Browser;
use crate::ffi::cef::View;
use crate::log::info;
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::tabs::{Position, TabId};
use lb_core::wasm::{self, WasmReq, WORKER_CTX_BASE};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// The probes' handles.
const GROUP: &str = "krunk-wasm";
/// The handles of a query (the prototype, the array of memories): released after each call.
const QUERY_GROUP: &str = "krunk-wasm-found";
/// The usual DevTools time limit.
const CALL_TIMEOUT_MS: i64 = 10_000;
/// At most this many contexts are listed.
const MAX_CONTEXTS: usize = 32;

pub(super) type WasmDone = Box<dyn FnOnce(Result<Value, String>)>;

/// The default world of a frame, as `Runtime` reported it.
#[derive(Debug, Clone)]
struct FrameContext {
    id: i64,
    frame: String,
    origin: String,
}

/// A dedicated worker the tab is attached to.
#[derive(Debug, Clone)]
struct Worker {
    n: u32,
    session: String,
    url: String,
}

#[derive(Default)]
struct Session {
    /// The probe's handle per context number.
    probes: HashMap<u32, String>,
    /// The fallback is on: the property its instances wait under, and the script's identifier
    /// (once the engine answered).
    hook: Option<(String, Option<String>)>,
}

#[derive(Default)]
pub(super) struct WasmHost {
    /// `krunk://wasm` tabs and the tab each inspects.
    pages: RefCell<HashMap<TabId, TabId>>,
    sessions: RefCell<HashMap<TabId, Session>>,
    /// Kept for every tab while its Runtime domain is on (the events are not repeated for a
    /// later user of the domain).
    contexts: RefCell<HashMap<TabId, Vec<FrameContext>>>,
    workers: RefCell<HashMap<TabId, Vec<Worker>>>,
    worker_seq: Cell<u32>,
}

#[derive(Debug, Clone)]
enum Target {
    Page,
    Frame(i64),
    Worker(String),
}

/// One request on its way through the DevTools calls.
struct Job {
    tab: TabId,
    ctx: u32,
    target: Target,
    req: Value,
    /// Look for memories first (`Runtime.queryObjects`).
    query: bool,
    timeout: i64,
    retried: bool,
    done: WasmDone,
}

impl Job {
    fn fail(self, why: impl Into<String>) {
        (self.done)(Err(why.into()))
    }
}

/// The engine's errors, as a user reads them.
fn friendly(e: String) -> String {
    if e.contains("no answer from the engine") {
        "The page did not answer in time (is it paused in the DevTools debugger?).".into()
    } else if wasm::handle_lost(&e) {
        "The page changed; list the memories again.".into()
    } else {
        e.chars().take(300).collect()
    }
}

/// The object handle of an evaluation's result.
fn object_id(r: Result<Value, String>) -> Result<String, String> {
    let v = r?;
    if let Some(d) = v.get("exceptionDetails") {
        return Err(wasm::exception_message(d));
    }
    v.pointer("/result/objectId").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "no object".to_string())
}

/// The frames of `Page.getFrameTree`: the main frame's id, and every frame's address.
fn frame_urls(tree: &Value) -> (String, HashMap<String, String>) {
    fn walk(node: &Value, out: &mut HashMap<String, String>) {
        if let Some(id) = node.pointer("/frame/id").and_then(Value::as_str) {
            let url = node.pointer("/frame/url").and_then(Value::as_str).unwrap_or("");
            out.insert(id.to_string(), url.to_string());
        }
        for child in node.get("childFrames").and_then(Value::as_array).into_iter().flatten() {
            walk(child, out);
        }
    }
    let mut out = HashMap::new();
    let root = &tree["frameTree"];
    walk(root, &mut out);
    (root.pointer("/frame/id").and_then(Value::as_str).unwrap_or("").to_string(), out)
}

impl Browser {
    // ------------------------------------------------------------ the tab's contexts

    /// DevTools events of tab `tab` (every tab's listener, `ext_devtools_event`): the frames'
    /// worlds and the workers, kept whether or not the tab is inspected.
    pub(super) fn wasm_devtools_event(&self, tab: TabId, method: &str, params: &Value) {
        match method {
            "Runtime.executionContextCreated" => {
                let Some(ctx) = params.get("context") else { return };
                if ctx.pointer("/auxData/isDefault").and_then(Value::as_bool) != Some(true) {
                    return;
                }
                let Some(id) = ctx.get("id").and_then(Value::as_i64) else { return };
                let frame = ctx.pointer("/auxData/frameId").and_then(Value::as_str).unwrap_or("").to_string();
                let origin = ctx.get("origin").and_then(Value::as_str).unwrap_or("").to_string();
                let mut all = self.wasm.contexts.borrow_mut();
                let list = all.entry(tab).or_default();
                list.retain(|c| c.id != id);
                list.push(FrameContext { id, frame, origin });
            }
            "Runtime.executionContextDestroyed" => {
                let Some(id) = params.get("executionContextId").and_then(Value::as_i64) else { return };
                if let Some(list) = self.wasm.contexts.borrow_mut().get_mut(&tab) {
                    list.retain(|c| c.id != id);
                }
                if let Some(s) = self.wasm.sessions.borrow_mut().get_mut(&tab) {
                    s.probes.remove(&(id as u32));
                }
            }
            "Runtime.executionContextsCleared" => {
                self.wasm.contexts.borrow_mut().remove(&tab);
                if let Some(s) = self.wasm.sessions.borrow_mut().get_mut(&tab) {
                    s.probes.retain(|ctx, _| *ctx >= WORKER_CTX_BASE);
                }
            }
            "Target.attachedToTarget" => {
                if params.pointer("/targetInfo/type").and_then(Value::as_str) != Some("worker") {
                    return;
                }
                let Some(session) = params.get("sessionId").and_then(Value::as_str) else { return };
                let url = params.pointer("/targetInfo/url").and_then(Value::as_str).unwrap_or("").to_string();
                let n = self.wasm.worker_seq.get() + 1;
                self.wasm.worker_seq.set(n);
                let mut all = self.wasm.workers.borrow_mut();
                let list = all.entry(tab).or_default();
                list.retain(|w| w.session != session);
                list.push(Worker { n, session: session.to_string(), url });
            }
            "Target.detachedFromTarget" => {
                let Some(session) = params.get("sessionId").and_then(Value::as_str) else { return };
                let mut gone = Vec::new();
                if let Some(list) = self.wasm.workers.borrow_mut().get_mut(&tab) {
                    list.retain(|w| {
                        if w.session == session {
                            gone.push(w.n);
                        }
                        w.session != session
                    });
                }
                if let Some(s) = self.wasm.sessions.borrow_mut().get_mut(&tab) {
                    for n in gone {
                        s.probes.remove(&(WORKER_CTX_BASE + n));
                    }
                }
            }
            _ => {}
        }
    }

    /// The tab's Runtime domain went off (or its browser is new): the contexts it reported are
    /// reported again when it goes on.
    pub(super) fn wasm_contexts_forget(&self, tab: TabId) {
        self.wasm.contexts.borrow_mut().remove(&tab);
    }

    /// Auto-attach was switched off for the tab: its worker sessions are gone.
    pub(super) fn wasm_workers_forget(&self, tab: TabId) {
        self.wasm.workers.borrow_mut().remove(&tab);
    }

    /// The tab is inspected: it stays attached to its workers (`hints_attach`).
    pub(super) fn wasm_wants_workers(&self, tab: TabId) -> bool {
        self.wasm.sessions.borrow().contains_key(&tab)
    }

    /// The tab's browser is new: nothing of the session is left in it.
    pub(super) fn wasm_tab_created(&self, tab: TabId) {
        self.wasm.sessions.borrow_mut().remove(&tab);
        self.wasm.contexts.borrow_mut().remove(&tab);
        self.wasm.workers.borrow_mut().remove(&tab);
    }

    /// A tab is gone: the session of an inspected tab, or (an inspector page) the session of
    /// its tab if no other page inspects it.
    pub(super) fn wasm_tab_gone(&self, tab: TabId) {
        self.wasm.sessions.borrow_mut().remove(&tab);
        self.wasm.contexts.borrow_mut().remove(&tab);
        self.wasm.workers.borrow_mut().remove(&tab);
        let source = self.wasm.pages.borrow_mut().remove(&tab);
        if let Some(source) = source {
            if !self.wasm.pages.borrow().values().any(|s| *s == source) {
                self.wasm_session_end(source);
            }
        }
    }

    // ------------------------------------------------------------ sessions

    fn wasm_session(&self, tab: TabId, view: &View) {
        if self.wasm.sessions.borrow().contains_key(&tab) {
            return;
        }
        self.wasm.sessions.borrow_mut().insert(tab, Session::default());
        self.cdp_use(tab, view, CdpUser::Wasm);
        // Its dedicated workers, the ones running now included.
        self.hints_attach(tab);
        info!("tab {tab}: WASM inspector session started");
    }

    /// Ends the tab's session: scans and freezes stop, the handles are released, the hook is
    /// removed (it stays in the loaded document until the next navigation).
    fn wasm_session_end(&self, tab: TabId) {
        let Some(session) = self.wasm.sessions.borrow_mut().remove(&tab) else { return };
        let Some(view) = self.tab_view(tab) else { return };
        let reset = json!({ "op": "reset" });
        for (ctx, probe) in &session.probes {
            let Some(target) = self.wasm_target(tab, *ctx) else { continue };
            let params = json!({
                "objectId": probe, "functionDeclaration": wasm::CALL_FUNCTION,
                "arguments": [{ "value": null }, { "value": reset }], "silent": true,
            });
            self.wasm_cdp(&view, &target, "Runtime.callFunctionOn", params, CALL_TIMEOUT_MS, |_| {});
            if let Target::Worker(s) = &target {
                let group = json!({ "objectGroup": GROUP });
                view.devtools_session_call(s, "Runtime.releaseObjectGroup", &group, CALL_TIMEOUT_MS, |_| {});
            }
        }
        view.devtools_method("Runtime.releaseObjectGroup", &json!({ "objectGroup": GROUP }));
        if let Some((_, Some(id))) = &session.hook {
            view.devtools_method("Page.removeScriptToEvaluateOnNewDocument", &json!({ "identifier": id }));
        }
        self.cdp_release(tab, &view, CdpUser::Wasm);
        self.hints_attach(tab);
        info!("tab {tab}: WASM inspector session ended");
    }

    fn wasm_target(&self, tab: TabId, ctx: u32) -> Option<Target> {
        if ctx == 0 {
            return Some(Target::Page);
        }
        if ctx >= WORKER_CTX_BASE {
            let workers = self.wasm.workers.borrow();
            let w = workers.get(&tab)?.iter().find(|w| WORKER_CTX_BASE + w.n == ctx)?;
            return Some(Target::Worker(w.session.clone()));
        }
        let contexts = self.wasm.contexts.borrow();
        contexts.get(&tab)?.iter().find(|c| c.id == i64::from(ctx)).map(|c| Target::Frame(c.id))
    }

    /// A DevTools call in `target` (an evaluation in a frame names the frame's world).
    fn wasm_cdp(
        &self,
        view: &View,
        target: &Target,
        method: &str,
        mut params: Value,
        timeout: i64,
        done: impl FnOnce(Result<Value, String>) + 'static,
    ) {
        match target {
            Target::Worker(session) => view.devtools_session_call(session, method, &params, timeout, done),
            Target::Frame(id) => {
                if method == "Runtime.evaluate" {
                    params["contextId"] = json!(id);
                }
                view.devtools_call_for(method, &params, timeout, done)
            }
            Target::Page => view.devtools_call_for(method, &params, timeout, done),
        }
    }

    // ------------------------------------------------------------ calls

    /// A request to the probe of context `ctx` of tab `tab`, from the inspector page, a user
    /// script of that tab or the agent. `list` and `reset` cover every context of the tab.
    pub(super) fn wasm_call(&self, tab: TabId, ctx: u32, req: WasmReq, done: WasmDone) {
        if let Err(e) = req.check() {
            return done(Err(e));
        }
        let Some(url) = self.tab(tab).map(|t| t.url.clone()) else { return done(Err("The tab is gone.".into())) };
        if !wasm::inspectable(&url) {
            return done(Err("Only web pages can be inspected (not Krunk's own pages).".into()));
        }
        let Some(view) = self.tab_view(tab) else { return done(Err("The tab is not loaded.".into())) };
        self.wasm_session(tab, &view);
        match req {
            WasmReq::List {} => self.wasm_list(tab, &view, url, done),
            WasmReq::Reset {} => {
                let ctxs: Vec<u32> = self
                    .wasm
                    .sessions
                    .borrow()
                    .get(&tab)
                    .map(|s| s.probes.keys().copied().collect())
                    .unwrap_or_default();
                for ctx in ctxs {
                    if let Some(target) = self.wasm_target(tab, ctx) {
                        let req = json!({ "op": "reset" });
                        let job = Job {
                            tab,
                            ctx,
                            target,
                            req,
                            query: false,
                            timeout: CALL_TIMEOUT_MS,
                            retried: true,
                            done: Box::new(|_| {}),
                        };
                        self.wasm_run(job);
                    }
                }
                done(Ok(json!({ "reset": true })))
            }
            req => {
                let Some(target) = self.wasm_target(tab, ctx) else {
                    return done(Err("That frame or worker is gone; list the memories again.".into()));
                };
                let timeout = if req.is_scan() { wasm::SCAN_TIMEOUT_MS } else { CALL_TIMEOUT_MS };
                self.wasm_run(Job {
                    tab,
                    ctx,
                    target,
                    req: req.to_json(),
                    query: false,
                    timeout,
                    retried: false,
                    done,
                });
            }
        }
    }

    /// `list`: the memories of every context that can be reached.
    fn wasm_list(&self, tab: TabId, view: &View, url: String, done: WasmDone) {
        let weak = self.me.clone();
        view.devtools_call("Page.getFrameTree", &json!({}), move |r| {
            let Some(b) = weak.upgrade() else { return done(Err("Krunk is closing".into())) };
            let (main, urls) = r.as_ref().map(frame_urls).unwrap_or_default();
            let mut targets: Vec<(u32, &'static str, String, Target)> = vec![(0, "page", url, Target::Page)];
            for c in b.wasm.contexts.borrow().get(&tab).into_iter().flatten() {
                // Without the frame tree the main frame can't be told from the others: the
                // page only, then.
                if main.is_empty() || c.frame == main || c.id <= 0 || c.id >= i64::from(WORKER_CTX_BASE) {
                    continue;
                }
                let url = urls.get(&c.frame).cloned().unwrap_or_else(|| c.origin.clone());
                targets.push((c.id as u32, "frame", url, Target::Frame(c.id)));
            }
            for w in b.wasm.workers.borrow().get(&tab).into_iter().flatten() {
                targets.push((WORKER_CTX_BASE + w.n, "worker", w.url.clone(), Target::Worker(w.session.clone())));
            }
            targets.truncate(MAX_CONTEXTS);
            targets.reverse();
            b.wasm_list_next(tab, targets, Vec::new(), done);
        });
    }

    /// One context after the other (`left` is in reverse order).
    fn wasm_list_next(
        &self,
        tab: TabId,
        mut left: Vec<(u32, &'static str, String, Target)>,
        mut out: Vec<Value>,
        done: WasmDone,
    ) {
        let Some((ctx, kind, url, target)) = left.pop() else {
            return done(Ok(json!({ "contexts": out, "unreachable": wasm::UNREACHABLE })));
        };
        let weak = self.me.clone();
        let each: WasmDone = Box::new(move |r| {
            let Some(b) = weak.upgrade() else { return done(Err("Krunk is closing".into())) };
            let mut row = json!({ "ctx": ctx, "kind": kind, "url": url });
            match r {
                Ok(v) => row["memories"] = v,
                Err(e) => {
                    row["memories"] = json!([]);
                    row["error"] = json!(e);
                }
            }
            out.push(row);
            b.wasm_list_next(tab, left, out, done);
        });
        let req = json!({ "op": "list" });
        self.wasm_run(Job { tab, ctx, target, req, query: true, timeout: CALL_TIMEOUT_MS, retried: false, done: each });
    }

    fn wasm_run(&self, job: Job) {
        let Some(view) = self.tab_view(job.tab) else { return job.fail("The tab is gone.") };
        let probe = self.wasm.sessions.borrow().get(&job.tab).and_then(|s| s.probes.get(&job.ctx).cloned());
        match probe {
            None => self.wasm_make_probe(&view, job),
            Some(probe) if job.query => self.wasm_query(&view, probe, job),
            Some(probe) => self.wasm_invoke(&view, probe, None, job),
        }
    }

    /// The context has no probe yet (or lost it with a navigation): make one, then look for
    /// the memories.
    fn wasm_make_probe(&self, view: &View, mut job: Job) {
        let key = match job.target {
            // New-document scripts don't run in workers.
            Target::Worker(_) => None,
            _ => self.wasm.sessions.borrow().get(&job.tab).and_then(|s| s.hook.as_ref().map(|h| h.0.clone())),
        };
        let params = json!({
            "expression": wasm::make_expression(key.as_deref()),
            "objectGroup": GROUP, "returnByValue": false, "silent": true,
        });
        let weak = self.me.clone();
        let target = job.target.clone();
        self.wasm_cdp(view, &target, "Runtime.evaluate", params, CALL_TIMEOUT_MS, move |r| {
            let Some(b) = weak.upgrade() else { return job.fail("Krunk is closing") };
            let id = match object_id(r) {
                Ok(id) => id,
                Err(e) => return job.fail(friendly(e)),
            };
            match b.wasm.sessions.borrow_mut().get_mut(&job.tab) {
                Some(s) => s.probes.insert(job.ctx, id),
                None => return job.fail("The inspector was closed."),
            };
            job.query = true;
            b.wasm_run(job);
        });
    }

    /// Finds the context's `WebAssembly.Memory` objects and hands them to the probe with the
    /// request.
    fn wasm_query(&self, view: &View, probe: String, job: Job) {
        let params = json!({
            "expression": "WebAssembly.Memory.prototype",
            "objectGroup": QUERY_GROUP, "returnByValue": false, "silent": true,
        });
        let weak = self.me.clone();
        let target = job.target.clone();
        self.wasm_cdp(view, &target, "Runtime.evaluate", params, CALL_TIMEOUT_MS, move |r| {
            let Some(b) = weak.upgrade() else { return job.fail("Krunk is closing") };
            let Some(view) = b.tab_view(job.tab) else { return job.fail("The tab is gone.") };
            let proto = match object_id(r) {
                Ok(id) => id,
                // A context without WebAssembly (or one that is gone: the call says so).
                Err(_) => return b.wasm_invoke(&view, probe, None, job),
            };
            let params = json!({ "prototypeObjectId": proto, "objectGroup": QUERY_GROUP });
            let weak = b.me.clone();
            let target = job.target.clone();
            b.wasm_cdp(&view, &target, "Runtime.queryObjects", params, CALL_TIMEOUT_MS, move |r| {
                let Some(b) = weak.upgrade() else { return job.fail("Krunk is closing") };
                let Some(view) = b.tab_view(job.tab) else { return job.fail("The tab is gone.") };
                let found =
                    r.ok().and_then(|v| v.pointer("/objects/objectId").and_then(Value::as_str).map(str::to_string));
                b.wasm_invoke(&view, probe, found, job);
            });
        });
    }

    fn wasm_invoke(&self, view: &View, probe: String, found: Option<String>, job: Job) {
        let first = match &found {
            Some(id) => json!({ "objectId": id }),
            None => json!({ "value": null }),
        };
        let params = json!({
            "objectId": probe, "functionDeclaration": wasm::CALL_FUNCTION,
            "arguments": [first, { "value": job.req }],
            "returnByValue": true, "awaitPromise": true, "silent": true,
        });
        let weak = self.me.clone();
        let target = job.target.clone();
        let queried = job.query;
        self.wasm_cdp(view, &target, "Runtime.callFunctionOn", params, job.timeout, move |r| {
            let Some(b) = weak.upgrade() else { return job.fail("Krunk is closing") };
            if queried {
                if let Some(view) = b.tab_view(job.tab) {
                    let group = json!({ "objectGroup": QUERY_GROUP });
                    b.wasm_cdp(&view, &job.target, "Runtime.releaseObjectGroup", group, CALL_TIMEOUT_MS, |_| {});
                }
            }
            match r {
                Ok(v) => match v.get("exceptionDetails") {
                    Some(d) => job.fail(wasm::exception_message(d)),
                    None => (job.done)(Ok(v.pointer("/result/value").cloned().unwrap_or(Value::Null))),
                },
                // The document is another one: its probe went with the old one. Once more,
                // with a new probe (which knows no scans: those requests then say so).
                Err(e) if wasm::handle_lost(&e) && !job.retried => {
                    if let Some(s) = b.wasm.sessions.borrow_mut().get_mut(&job.tab) {
                        s.probes.remove(&job.ctx);
                    }
                    let mut job = job;
                    job.retried = true;
                    b.wasm_run(job);
                }
                Err(e) => job.fail(friendly(e)),
            }
        });
    }

    // ------------------------------------------------------------ the fallback

    /// Hooks `WebAssembly` at document start (or stops doing so) and reloads the tab.
    fn wasm_hook(&self, tab: TabId, on: bool, done: WasmDone) {
        let Some(view) = self.tab_view(tab) else { return done(Err("The tab is not loaded.".into())) };
        if !self.tab(tab).is_some_and(|t| wasm::inspectable(&t.url)) {
            return done(Err("Only web pages can be inspected (not Krunk's own pages).".into()));
        }
        self.wasm_session(tab, &view);
        let old = self.wasm.sessions.borrow_mut().get_mut(&tab).and_then(|s| s.hook.take());
        if let Some((_, Some(id))) = old {
            view.devtools_method("Page.removeScriptToEvaluateOnNewDocument", &json!({ "identifier": id }));
        }
        if !on {
            view.reload();
            return done(Ok(json!({ "hook": false })));
        }
        let key = format!("__krunkWasm{}", lb_core::scripts_store::new_id());
        if let Some(s) = self.wasm.sessions.borrow_mut().get_mut(&tab) {
            s.hook = Some((key.clone(), None));
        }
        let weak = self.me.clone();
        let params = json!({ "source": wasm::hook_script(&key) });
        view.devtools_call("Page.addScriptToEvaluateOnNewDocument", &params, move |r| {
            let Some(b) = weak.upgrade() else { return done(Err("Krunk is closing".into())) };
            let id = match r {
                Ok(v) => v.get("identifier").and_then(Value::as_str).map(str::to_string),
                Err(e) => {
                    if let Some(s) = b.wasm.sessions.borrow_mut().get_mut(&tab) {
                        s.hook = None;
                    }
                    return done(Err(friendly(e)));
                }
            };
            if let Some(s) = b.wasm.sessions.borrow_mut().get_mut(&tab) {
                if let Some(h) = s.hook.as_mut().filter(|h| h.0 == key) {
                    h.1 = id;
                }
            }
            if let Some(view) = b.tab_view(tab) {
                view.reload();
            }
            info!("tab {tab}: WASM inspector hook installed, reloading");
            done(Ok(json!({ "hook": true })));
        });
    }

    // ------------------------------------------------------------ krunk://wasm

    /// ⋮ menu → "WASM memory inspector": opens the page next to the active tab, in the same
    /// profile, bound to that tab.
    pub(super) fn open_wasm(&self) {
        self.hide_popup();
        let Some((source, private)) = self.active_tab().map(|t| (t.id, t.private)) else { return };
        // From an inspector page: another one for the same tab.
        let source = self.wasm.pages.borrow().get(&source).copied().unwrap_or(source);
        let Some(win) = self.window_of(source) else { return };
        let opened = self.ws.borrow_mut().open(
            win,
            &Page::Wasm.url(),
            private,
            Position::AfterOpener(source),
            true,
            lb_core::storage::now_ms(),
        );
        let Some((id, effects)) = opened else { return };
        self.wasm.pages.borrow_mut().insert(id, source);
        self.apply(effects);
    }

    pub(super) fn dispatch_wasm(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let weak = self.me.clone();
        let reply = move |event: Event| {
            if let Some(b) = weak.upgrade() {
                b.reply(tab, event);
            }
        };
        let source = tab.and_then(|t| self.wasm.pages.borrow().get(&t).copied());
        let Some(source) = source else {
            return reply(Event::err(req_id, "Open this page from the ⋮ menu of the tab to inspect"));
        };
        let answer: WasmDone = Box::new(move |r| {
            reply(match r {
                Ok(v) => Event::ok(req_id, v),
                Err(e) => Event::err(req_id, e),
            })
        });
        match request {
            Request::WasmInfo {} => {
                let info = match self.tab(source) {
                    Some(t) => json!({
                        "open": true, "title": t.title, "url": t.url, "private": t.private,
                        "inspectable": wasm::inspectable(&t.url),
                    }),
                    None => json!({ "open": false }),
                };
                let hook = self.wasm.sessions.borrow().get(&source).is_some_and(|s| s.hook.is_some());
                let mut info = info;
                info["hook"] = json!(hook);
                info["unreachable"] = json!(wasm::UNREACHABLE);
                answer(Ok(info));
            }
            Request::WasmCall { ctx, req } => self.wasm_call(source, ctx, req, answer),
            Request::WasmHook { on } => self.wasm_hook(source, on, answer),
            _ => {}
        }
    }
}
