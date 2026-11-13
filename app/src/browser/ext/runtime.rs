//! The extension runtime added in 0.17.0, host side (`lb_core::ext_runtime` has the pure
//! parts): the ends of a message or port, ports, offscreen documents, extensions' own popup
//! windows, content scripts registered at runtime, `scripting.executeScript` with results and
//! the per-extension error list.
//!
//! An extension's contexts ("ends"): its background (a worker behind the hidden background
//! page, which relays), the action popup, an offscreen document, its own popup windows, its
//! pages in tabs, and its content-script worlds in web tabs. A request says which end it comes
//! from by the view it arrives through (or the DevTools binding of a content world), never by
//! anything in the message.

use super::*;
use lb_core::ext_runtime::{self as rt, Outcome, PortAction};
use std::rc::Weak;

/// The id of the first extension window (`chrome.windows`): far from the ids of browser
/// windows, which count up from 1.
pub(super) const FIRST_EXT_WINDOW: u32 = 0x4000_0000;
/// An extension window's size (DIP) when the extension gives none, and the limits.
const EXT_WINDOW_DEFAULT: (f64, f64) = (400.0, 620.0);
const EXT_WINDOW_MIN: (f64, f64) = (200.0, 120.0);
const EXT_WINDOW_MAX: (f64, f64) = (4000.0, 4000.0);
/// Most windows and offscreen documents one extension may have open.
const MAX_EXT_WINDOWS: usize = 8;

/// `KRUNK_EXT_LOG=1` in the environment: every console level of the extensions' contexts
/// (background worker, popup, pages, offscreen documents, windows, content scripts) is written
/// to krunk.log with its context, not only their errors. For finding out why an extension does
/// not work; extension output may contain what the user does, so it is off by default.
pub(super) fn log_all() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("KRUNK_EXT_LOG").is_ok_and(|v| v == "1"))
}

/// One end of a message or a port.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::browser) enum End {
    /// The extension's background worker.
    Background(String),
    /// The extension's action popup.
    Popup(String),
    /// The extension's offscreen document.
    Offscreen(String),
    /// One of the extension's own windows.
    Window(u32),
    /// An extension page in a tab.
    Tab(TabId),
    /// The extension's content-script world in a frame of a web tab.
    Content { tab: TabId, context: i64 },
}

impl End {
    /// The name of the context for the error list and the log.
    pub(super) fn context_name(&self) -> &'static str {
        match self {
            End::Background(_) => "background",
            End::Popup(_) => "popup",
            End::Offscreen(_) => "offscreen",
            End::Window(_) => "window",
            End::Tab(_) => "page",
            End::Content { .. } => "content script",
        }
    }
}

/// An offscreen document: a hidden view like the background's.
pub(super) struct HiddenPage {
    pub window: Window,
    pub view: View,
    pub url: String,
}

/// An extension's own window (`chrome.windows.create({ type: "popup" })`).
pub(super) struct ExtWindow {
    pub window: Window,
    pub view: View,
    pub ext: String,
    pub url: String,
}

struct ExtWindowEvents {
    browser: Weak<Browser>,
    id: u32,
}

impl WindowEvents for ExtWindowEvents {
    fn resized(&self, width: i32, height: i32) {
        let Some(b) = self.browser.upgrade() else { return };
        let view = b.ext.windows.try_borrow().ok().and_then(|w| w.get(&self.id).map(|w| w.view.clone()));
        if let Some(view) = view {
            view.set_bounds(0, 0, width, height);
        }
    }

    fn activated(&self, active: bool) {
        if !active {
            return;
        }
        let id = self.id;
        if let Some(b) = self.browser.upgrade() {
            b.defer(move |b| b.ext_window_activated(id));
        }
    }

    fn close_requested(&self) -> bool {
        // The view closes first, then the window (`ext_window_close`).
        let id = self.id;
        if let Some(b) = self.browser.upgrade() {
            b.defer(move |b| b.ext_window_close(id));
        }
        false
    }
}

impl Browser {
    // ------------------------------------------------------------ ends

    /// The view of a page end.
    pub(super) fn ext_end_view(&self, end: &End) -> Option<View> {
        match end {
            End::Background(id) => self.ext.backgrounds.borrow().get(id).map(|b| b.view.clone()),
            End::Popup(id) => self.ext.popup.borrow().as_ref().filter(|p| p.ext == *id).map(|p| p.view.clone()),
            End::Offscreen(id) => self.ext.offscreen.borrow().get(id).map(|o| o.view.clone()),
            End::Window(id) => self.ext.windows.borrow().get(id).map(|w| w.view.clone()),
            End::Tab(t) => self.tab_view(*t),
            End::Content { .. } => None,
        }
    }

    /// Is this tab showing a page of the extension?
    fn ext_tab_shows(&self, tab: TabId, ext_id: &str) -> bool {
        let own = format!("{}/", ext::origin(ext_id));
        self.tab(tab).is_some_and(|t| t.url.starts_with(&own))
    }

    /// The tabs showing a page of the extension.
    fn ext_page_tabs(&self, ext_id: &str) -> Vec<TabId> {
        let own = format!("{}/", ext::origin(ext_id));
        self.ws.borrow().tabs().filter(|(_, t)| t.url.starts_with(&own)).map(|(_, t)| t.id).collect()
    }

    /// Every page context of the extension that exists now (not its content scripts).
    pub(super) fn ext_page_ends(&self, ext_id: &str) -> Vec<End> {
        let mut out = Vec::new();
        if self.ext.backgrounds.borrow().contains_key(ext_id) {
            out.push(End::Background(ext_id.to_string()));
        }
        if self.ext.popup.borrow().as_ref().is_some_and(|p| p.ext == ext_id) {
            out.push(End::Popup(ext_id.to_string()));
        }
        if self.ext.offscreen.borrow().contains_key(ext_id) {
            out.push(End::Offscreen(ext_id.to_string()));
        }
        let mut windows: Vec<u32> =
            self.ext.windows.borrow().iter().filter(|(_, w)| w.ext == ext_id).map(|(id, _)| *id).collect();
        windows.sort_unstable();
        out.extend(windows.into_iter().map(End::Window));
        out.extend(self.ext_page_tabs(ext_id).into_iter().map(End::Tab));
        out
    }

    /// An event to one end. A tab only gets it while it shows a page of the extension, a
    /// content world only while it exists.
    pub(super) fn ext_send_end(&self, end: &End, ext_id: &str, kind: &str, data: Value) {
        match end {
            End::Background(id) if id == ext_id => self.ext_send_background(ext_id, kind, data),
            End::Background(_) => {}
            End::Tab(t) if !self.ext_tab_shows(*t, ext_id) => {}
            End::Content { tab, context } => {
                let known = self
                    .ext
                    .contexts
                    .borrow()
                    .get(tab)
                    .is_some_and(|l| l.iter().any(|c| c.id == *context && c.ext == ext_id));
                if !known {
                    return;
                }
                let mut msg = match data {
                    Value::Object(o) => o,
                    _ => serde_json::Map::new(),
                };
                msg.insert("ext".into(), json!(ext_id));
                msg.insert("event".into(), json!(kind));
                self.ext_deliver_content(*tab, *context, &Value::Object(msg));
            }
            page => {
                if let Some(view) = self.ext_end_view(page) {
                    view.post_json(&Event::Ext { ext: ext_id.to_string(), kind: kind.to_string(), data }.to_json());
                }
            }
        }
    }

    /// `sender` as the receiving extension context sees it. Extension pages are named as
    /// Chrome names them (`chrome-extension://<id>/…`): extensions compare the sender's origin
    /// with the one they build from `runtime.id` (docs/EXTENSIONS.md).
    pub(super) fn ext_sender(&self, caller: &Caller, ext_id: &str, data: &Value) -> Value {
        match caller {
            Caller::Content { tab, context, .. } => {
                let t = self.ext_tab_json(ext_id, *tab);
                let (known_main, origin) = self
                    .ext
                    .contexts
                    .borrow()
                    .get(tab)
                    .and_then(|l| l.iter().find(|c| c.id == *context).map(|c| (c.main, c.origin.clone())))
                    .unwrap_or((false, String::new()));
                // The frame says whether it is the top one and what its address is (it is the
                // extension's own world: it can only mislead itself); the address, the top
                // frame's too, must be on the origin the engine reported for that world. The
                // tab's address is not used: it is the pending navigation's as soon as one
                // starts, while the old document's script still runs.
                let says_top = data.pointer("/src/top").and_then(|t| t.as_bool());
                let says_url = data.pointer("/src/url").and_then(|u| u.as_str()).unwrap_or("");
                let main = known_main || says_top == Some(true);
                let (url, origin) = lb_core::ext_guard::content_sender(&origin, says_url);
                json!({
                    "id": ext_id, "tab": t, "url": url, "frameId": if main { 0 } else { *context },
                    "origin": origin, "documentLifecycle": "active",
                })
            }
            Caller::Page { end, .. } => {
                let own = ext::origin(ext_id);
                let url = match end {
                    End::Background(_) => self
                        .ext
                        .exts
                        .borrow()
                        .iter()
                        .find(|e| e.installed.id == ext_id)
                        .and_then(|e| {
                            e.manifest.background_script().map(|(s, _)| s.trim_start_matches('/').to_string())
                        })
                        .map(|s| format!("{own}/{s}")),
                    _ => self.ext.request_url.borrow().clone().filter(|u| u.starts_with(&format!("{own}/"))),
                }
                .unwrap_or_else(|| format!("{own}/"));
                let mut s = json!({
                    "id": ext_id, "url": rt::chrome_url(&url), "origin": format!("chrome-extension://{ext_id}"),
                });
                if let End::Tab(t) = end {
                    s["tab"] = self.ext_tab_json(ext_id, *t);
                    s["frameId"] = json!(0);
                }
                s
            }
        }
    }

    /// A context went away: its ports close, the other ends hear `onDisconnect`.
    pub(super) fn ext_context_gone(&self, gone: impl Fn(&End) -> bool) {
        if self.ext.ports.borrow().is_empty() {
            return;
        }
        let actions = self.ext.ports.borrow_mut().context_gone(gone);
        for (ext_id, port, action) in actions {
            self.ext_port_deliver(&ext_id, &port, vec![action]);
        }
    }

    /// A tab finished a load: if it showed an extension page and shows something else now,
    /// that page's ports close.
    pub(in crate::browser) fn ext_tab_left_page(&self, tab: TabId) {
        if self.ext.ports.borrow().is_empty() {
            return;
        }
        let on_extension = self.tab(tab).is_some_and(|t| ext::id_of_url(&t.url).is_some());
        if !on_extension {
            self.ext_context_gone(|e| *e == End::Tab(tab));
        }
    }

    // ------------------------------------------------------------ ports

    fn ext_port_deliver(&self, ext_id: &str, port: &str, actions: Vec<PortAction<End>>) {
        for action in actions {
            match action {
                PortAction::Message { to, data } => {
                    self.ext_send_end(&to, ext_id, "portMessage", json!({ "port": port, "data": data }))
                }
                PortAction::Disconnect { to, error } => {
                    self.ext_send_end(&to, ext_id, "portDisconnect", json!({ "port": port, "error": error }))
                }
            }
        }
    }

    /// `port.open` (`runtime.connect`, `tabs.connect`), `port.accept`, `port.post`,
    /// `port.close`. Nothing is answered: a port's life is told by events.
    pub(super) fn ext_port_op(&self, caller: &Caller, ext_id: &str, op: &str, data: &Value) {
        let Some(id) = data.get("id").and_then(|i| i.as_str()) else { return };
        let from = caller.end();
        match op {
            "port.open" => {
                let name: String = data.get("name").and_then(|n| n.as_str()).unwrap_or("").chars().take(200).collect();
                let receivers: Vec<End> = match data.get("tabId").and_then(|t| t.as_u64()) {
                    // tabs.connect: the extension's content worlds in that tab (one frame, or
                    // all). Content scripts have no chrome.tabs.
                    Some(tab) if !matches!(from, End::Content { .. }) => {
                        let tab = tab as TabId;
                        let frame = data.get("frameId").and_then(|f| f.as_i64());
                        self.ext
                            .contexts
                            .borrow()
                            .get(&tab)
                            .map(|l| {
                                l.iter()
                                    .filter(|c| c.ext == ext_id)
                                    .filter(|c| match frame {
                                        None => true,
                                        Some(0) => c.main,
                                        Some(f) => c.id == f,
                                    })
                                    .map(|c| End::Content { tab, context: c.id })
                                    .collect()
                            })
                            .unwrap_or_default()
                    }
                    Some(_) => Vec::new(),
                    None => self.ext_page_ends(ext_id),
                };
                let receivers: Vec<End> = receivers.into_iter().filter(|r| *r != from).collect();
                let sender = self.ext_sender(caller, ext_id, data);
                let opened = self.ext.ports.borrow_mut().open(ext_id, id, from, receivers.clone());
                match opened {
                    Ok(()) => {
                        for r in receivers {
                            self.ext_send_end(
                                &r,
                                ext_id,
                                "portConnect",
                                json!({ "port": id, "name": name, "sender": sender }),
                            );
                        }
                    }
                    Err(refused) => self.ext_port_deliver(ext_id, id, vec![refused]),
                }
            }
            "port.accept" => self.ext.ports.borrow_mut().accept(ext_id, id, &from),
            "port.post" => {
                let actions =
                    self.ext.ports.borrow().post(ext_id, id, &from, data.get("data").cloned().unwrap_or(Value::Null));
                self.ext_port_deliver(ext_id, id, actions);
            }
            _ => {
                let actions = self.ext.ports.borrow_mut().close(ext_id, id, &from);
                self.ext_port_deliver(ext_id, id, actions);
            }
        }
    }

    // ------------------------------------------------------------ messages

    /// `runtime.sendMessage` (to every other page context of the extension) or
    /// `tabs.sendMessage` (to a tab's content world). The answer arrives with `respond`.
    pub(super) fn ext_route_message(
        &self,
        caller: &Caller,
        ext_id: &str,
        data: &Value,
        to_tab: Option<TabId>,
    ) -> Option<Result<Value, String>> {
        let id = self.ext.next_id.get();
        self.ext.next_id.set(id + 1);
        let message = data.get("message").cloned().unwrap_or(Value::Null);
        let sender = self.ext_sender(caller, ext_id, data);
        let from = caller.end();
        let receivers: Vec<End> = match to_tab {
            Some(tab) => {
                self.ext_context(tab, ext_id).map(|context| vec![End::Content { tab, context }]).unwrap_or_default()
            }
            None => self.ext_page_ends(ext_id).into_iter().filter(|e| *e != from).collect(),
        };
        if receivers.is_empty() {
            return Some(Err(rt::NO_RECEIVER.into()));
        }
        self.ext
            .pending
            .borrow_mut()
            .insert(id, (ext_id.to_string(), caller.clone(), rt::Broadcast::new(receivers.len())));
        for r in &receivers {
            self.ext_send_end(r, ext_id, "message", json!({ "id": id, "data": message, "sender": sender }));
        }
        // A receiver that never answers: the caller gets undefined, as in Chrome.
        let weak = self.me.clone();
        let ext_owned = ext_id.to_string();
        crate::ffi::cef::post_ui_delayed(MESSAGE_TIMEOUT_MS, move || {
            if let Some(b) = weak.upgrade() {
                let target = b.ext.pending.borrow_mut().remove(&id);
                if let Some((_, target, _)) = target {
                    b.ext_reply(&target, &ext_owned, Ok(Value::Null));
                }
            }
        });
        None
    }

    /// `respond`: a receiver's answer to a message (a real one, "no listeners here" or "my
    /// listeners did not answer"). Only the extension the message went to may answer it
    /// (0.11.7, audit C4).
    pub(super) fn ext_respond(&self, ext_id: &str, data: &Value) {
        let id = data.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
        let flag = |k: &str| data.get(k).and_then(|v| v.as_bool()) == Some(true);
        let outcome = {
            let mut pending = self.ext.pending.borrow_mut();
            match pending.get_mut(&id) {
                Some((owner, _, broadcast)) if owner == ext_id => {
                    broadcast.respond(flag("none"), flag("empty"), data.get("data").cloned().unwrap_or(Value::Null))
                }
                _ => return,
            }
        };
        let result = match outcome {
            Outcome::Wait => return,
            Outcome::Answer(v) => Ok(v),
            Outcome::Fail(e) => Err(e),
        };
        let target = self.ext.pending.borrow_mut().remove(&id).map(|(_, c, _)| c);
        if let Some(target) = target {
            self.ext_reply(&target, ext_id, result);
        }
    }

    // ------------------------------------------------------------ runtime.getContexts

    pub(super) fn ext_contexts_json(&self, ext_id: &str, filter: &Value) -> Value {
        let own = ext::origin(ext_id);
        let strings = |k: &str| -> Option<Vec<String>> {
            filter.get(k).and_then(|a| a.as_array()).map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str())
                    .map(|s| rt::krunk_url(s).trim_end_matches('#').to_string())
                    .collect()
            })
        };
        let types = strings("contextTypes");
        let urls = strings("documentUrls");
        let mut out = Vec::new();
        for end in self.ext_page_ends(ext_id) {
            let (kind, url, tab, window): (&str, Option<String>, i64, i64) = match &end {
                End::Background(_) => ("BACKGROUND", None, -1, -1),
                End::Popup(_) => ("POPUP", self.ext.popup.borrow().as_ref().map(|p| p.url.clone()), -1, -1),
                End::Offscreen(_) => {
                    ("OFFSCREEN_DOCUMENT", self.ext.offscreen.borrow().get(ext_id).map(|o| o.url.clone()), -1, -1)
                }
                End::Window(id) => {
                    ("TAB", self.ext.windows.borrow().get(id).map(|w| w.url.clone()), -1, i64::from(*id))
                }
                End::Tab(t) => (
                    "TAB",
                    self.tab(*t).map(|tab| tab.url.clone()),
                    i64::from(*t),
                    self.window_of(*t).map(i64::from).unwrap_or(-1),
                ),
                End::Content { .. } => continue,
            };
            if types.as_ref().is_some_and(|t| !t.iter().any(|x| x == kind)) {
                continue;
            }
            if let Some(wanted) = &urls {
                let Some(u) = &url else { continue };
                let bare = u.split('#').next().unwrap_or(u);
                if !wanted.iter().any(|w| w == u || w.split('#').next() == Some(bare)) {
                    continue;
                }
            }
            out.push(json!({
                "contextType": kind, "contextId": format!("{end:?}"), "documentUrl": url, "documentOrigin": own,
                "documentId": Value::Null, "frameId": if kind == "BACKGROUND" { -1 } else { 0 }, "tabId": tab,
                "windowId": window, "incognito": false,
            }));
        }
        Value::Array(out)
    }

    // ------------------------------------------------------------ offscreen documents

    pub(super) fn ext_offscreen_create(&self, ext_id: &str, url: &str) -> Result<Value, String> {
        if self.ext.offscreen.borrow().contains_key(ext_id) {
            return Err("Only a single offscreen document may be created.".into());
        }
        let url = self.ext_resolve_url(ext_id, url);
        if !url.starts_with(&format!("{}/", ext::origin(ext_id))) {
            return Err("Invalid URL: an offscreen document must be a page of the extension.".into());
        }
        let Some(app) = self.app() else { return Err("no window".into()) };
        let window = Window::create_popup(app, Rc::new(BackgroundEvents))?;
        let view = self.ext_view(ext_id, End::Offscreen(ext_id.to_string()));
        if let Err(e) = view.start(window.hwnd(), &url) {
            window.destroy();
            return Err(e);
        }
        view.set_bounds(0, 0, 2, 2);
        info!("extension {}: offscreen document opened", self.ext_name(ext_id));
        self.ext.offscreen.borrow_mut().insert(ext_id.to_string(), HiddenPage { window, view, url });
        Ok(Value::Null)
    }

    pub(super) fn ext_offscreen_close(&self, ext_id: &str) -> bool {
        let Some(page) = self.ext.offscreen.borrow_mut().remove(ext_id) else { return false };
        self.ext_context_gone(|e| *e == End::Offscreen(ext_id.to_string()));
        page.view.close();
        page.window.destroy();
        info!("extension {}: offscreen document closed", self.ext_name(ext_id));
        true
    }

    // ------------------------------------------------------------ the extension's own windows

    /// `chrome.windows.create`: type `popup` (or `panel`) with a page of the extension is a
    /// small window of its own; anything else opens as a tab of the current window.
    pub(super) fn ext_window_create(&self, ext_id: &str, data: &Value) -> Result<Value, String> {
        let url = match data.get("url") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(a)) => a.first().and_then(|s| s.as_str()).unwrap_or("").to_string(),
            _ => String::new(),
        };
        let given = !url.is_empty();
        let url = if url.is_empty() { Page::NewTab.url() } else { self.ext_resolve_url(ext_id, &url) };
        let own = url.starts_with(&format!("{}/", ext::origin(ext_id)));
        let small = matches!(data.get("type").and_then(|t| t.as_str()), Some("popup" | "panel"));
        if !(small && own) {
            // An address the extension gives is not the host's: its own pages and the web.
            if given && !lb_core::ext_guard::may_open(ext_id, &url) {
                return Err("That address cannot be opened.".into());
            }
            self.open_url(&url, true, false);
            let mut win = self.ext_window_json(self.cur_id());
            if let Some(t) = self.active_id() {
                win["tabs"] = json!([self.ext_tab_json(ext_id, t)]);
            }
            return Ok(win);
        }
        if self.ext.windows.borrow().values().filter(|w| w.ext == ext_id).count() >= MAX_EXT_WINDOWS {
            return Err("Too many windows are open.".into());
        }
        let base = self.cur().map(|f| (f.window.rect(), f.window.scale()));
        let (frame, scale) = base.unwrap_or((ScreenRect { x: 100, y: 100, width: 1280, height: 860 }, 1.0));
        let num = |k: &str| data.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite());
        let w = num("width").unwrap_or(EXT_WINDOW_DEFAULT.0).clamp(EXT_WINDOW_MIN.0, EXT_WINDOW_MAX.0);
        let h = num("height").unwrap_or(EXT_WINDOW_DEFAULT.1).clamp(EXT_WINDOW_MIN.1, EXT_WINDOW_MAX.1);
        let (wp, hp) = ((w * scale).round() as i32, (h * scale).round() as i32);
        let x = num("left").map(|l| (l * scale).round() as i32).unwrap_or(frame.x + (frame.width - wp) / 2);
        let y = num("top").map(|t| (t * scale).round() as i32).unwrap_or(frame.y + (60.0 * scale) as i32);
        // Wholly on the screen it is mostly on.
        let work = Window::work_area_at(x + wp / 2, y + hp / 2);
        let (wp, hp) = (wp.min(work.width), hp.min(work.height));
        let x = x.clamp(work.x, (work.x + work.width - wp).max(work.x));
        let y = y.clamp(work.y, (work.y + work.height - hp).max(work.y));
        let rect = ScreenRect { x, y, width: wp, height: hp };
        let id = self.ext.next_window.get();
        self.ext.next_window.set(id + 1);
        let events = Rc::new(ExtWindowEvents { browser: self.me.clone(), id });
        let window = Window::create_ext_window(&self.ext_name(ext_id), rect, events)?;
        window.set_dark_frame(self.ext_dark_frames());
        let view = self.ext_view(ext_id, End::Window(id));
        view.on_title(move |title| {
            if !title.is_empty() && !title.contains(ext::HOST_SUFFIX) {
                window.set_title(&title);
            }
        });
        if let Err(e) = view.start(window.hwnd(), &url) {
            window.destroy();
            return Err(e);
        }
        let (cw, ch) = window.client_size();
        view.set_bounds(0, 0, cw, ch);
        let focused = data.get("focused").and_then(|f| f.as_bool()) != Some(false);
        window.show_at(rect, focused);
        view.set_visible(true);
        if focused {
            view.focus();
        }
        info!("extension {}: window {id} opened ({}x{})", self.ext_name(ext_id), w as i32, h as i32);
        self.ext.windows.borrow_mut().insert(id, ExtWindow { window, view, ext: ext_id.to_string(), url });
        Ok(self.ext_own_window_json(id))
    }

    /// Whether native title bars are dark now (the theme setting).
    fn ext_dark_frames(&self) -> bool {
        match self.settings.borrow().theme {
            lb_core::settings::Theme::Dark => true,
            lb_core::settings::Theme::Light => false,
            lb_core::settings::Theme::System => crate::ffi::window::system_prefers_dark(),
        }
    }

    /// Is `id` an open window of this extension?
    pub(super) fn ext_owns_window(&self, ext_id: &str, id: u64) -> Option<u32> {
        let id = u32::try_from(id).ok()?;
        self.ext.windows.borrow().get(&id).filter(|w| w.ext == ext_id).map(|_| id)
    }

    /// A `chrome.windows.Window` for one of the extension's own windows (sizes in DIP).
    pub(super) fn ext_own_window_json(&self, id: u32) -> Value {
        let windows = self.ext.windows.borrow();
        let Some(w) = windows.get(&id) else { return Value::Null };
        let (r, s) = (w.window.rect(), w.window.scale().max(0.1));
        json!({
            "id": id, "type": "popup", "focused": w.window.is_foreground(), "incognito": false, "alwaysOnTop": false,
            "state": if w.window.is_minimized() { "minimized" } else { "normal" },
            "left": (f64::from(r.x) / s).round(), "top": (f64::from(r.y) / s).round(),
            "width": (f64::from(r.width) / s).round(), "height": (f64::from(r.height) / s).round(),
            "tabs": [],
        })
    }

    /// The extension's own windows, as `windows.getAll` lists them after the browser's.
    pub(super) fn ext_own_windows(&self, ext_id: &str) -> Vec<u32> {
        let mut ids: Vec<u32> =
            self.ext.windows.borrow().iter().filter(|(_, w)| w.ext == ext_id).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids
    }

    /// `chrome.windows.update` for one of the extension's own windows.
    pub(super) fn ext_window_update(&self, id: u32, info: &Value) -> Value {
        let found = self.ext.windows.borrow().get(&id).map(|w| (w.window, w.view.clone()));
        let Some((window, view)) = found else { return Value::Null };
        let num = |k: &str| info.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite());
        if ["left", "top", "width", "height"].iter().any(|k| num(k).is_some()) {
            let (r, s) = (window.rect(), window.scale());
            let px = |k: &str, old: i32| num(k).map(|v| (v * s).round() as i32).unwrap_or(old);
            let width = px("width", r.width).clamp((EXT_WINDOW_MIN.0 * s) as i32, (EXT_WINDOW_MAX.0 * s) as i32);
            let height = px("height", r.height).clamp((EXT_WINDOW_MIN.1 * s) as i32, (EXT_WINDOW_MAX.1 * s) as i32);
            window.move_to(ScreenRect { x: px("left", r.x), y: px("top", r.y), width, height });
        }
        if info.get("focused").and_then(|f| f.as_bool()) == Some(true) {
            window.bring_to_front();
            view.focus();
        }
        self.ext_own_window_json(id)
    }

    fn ext_window_activated(&self, id: u32) {
        let found = self.ext.windows.borrow().get(&id).map(|w| (w.ext.clone(), w.view.clone()));
        let Some((ext_id, view)) = found else { return };
        view.focus();
        for end in self.ext_page_ends(&ext_id) {
            self.ext_send_end(&end, &ext_id, "windowFocus", json!({ "windowId": id }));
        }
    }

    /// Closes one of the extension's windows (its close button, `windows.remove`,
    /// `window.close()`): its ports close, the extension hears `windows.onRemoved`.
    pub(super) fn ext_window_close(&self, id: u32) {
        let Some(w) = self.ext.windows.borrow_mut().remove(&id) else { return };
        self.ext_context_gone(|e| *e == End::Window(id));
        w.view.close();
        w.window.destroy();
        info!("extension {}: window {id} closed", self.ext_name(&w.ext));
        for end in self.ext_page_ends(&w.ext) {
            self.ext_send_end(&end, &w.ext, "windowRemoved", json!({ "windowId": id }));
        }
    }

    /// Everything an extension has open besides its background and popup goes (it was
    /// disabled or removed, or its background restarts).
    pub(super) fn ext_close_contexts(&self, ext_id: &str) {
        self.ext_offscreen_close(ext_id);
        for id in self.ext_own_windows(ext_id) {
            self.ext_window_close(id);
        }
        self.ext.ports.borrow_mut().drop_ext(ext_id);
        self.ext.pending.borrow_mut().retain(|_, (owner, _, _)| owner != ext_id);
        // The code that starts next chooses its storage access levels itself.
        self.ext.storage_access.borrow_mut().retain(|key, _| !key.starts_with(&format!("{ext_id}/")));
    }

    /// Krunk is closing: the extensions' windows and offscreen documents go with it.
    pub(in crate::browser) fn ext_shutdown(&self) {
        let windows: Vec<ExtWindow> = self.ext.windows.borrow_mut().drain().map(|(_, w)| w).collect();
        for w in windows {
            w.view.close();
            w.window.destroy();
        }
        let pages: Vec<HiddenPage> = self.ext.offscreen.borrow_mut().drain().map(|(_, p)| p).collect();
        for p in pages {
            p.view.close();
            p.window.destroy();
        }
    }

    // ------------------------------------------------------------ content scripts registered at runtime

    /// The extension's content scripts: the manifest's, then those it registered
    /// (`chrome.scripting.registerContentScripts`; kept until Krunk closes).
    pub(super) fn ext_content_scripts(&self, e: &LoadedExt) -> Vec<ext::ContentScript> {
        let mut all = e.manifest.content_scripts.clone();
        if let Some(extra) = self.ext.registered.borrow().get(&e.installed.id) {
            all.extend(extra.iter().map(|(_, s)| s.clone()));
        }
        all
    }

    pub(super) fn ext_register_scripts(&self, ext_id: &str, op: &str, data: &Value) -> Result<Value, String> {
        let permitted =
            self.ext.exts.borrow().iter().any(|e| e.installed.id == ext_id && ext::may_use_scripting(&e.manifest));
        if !permitted {
            return Err("The extension doesn't have the \"scripting\" permission.".into());
        }
        let ids: Option<Vec<String>> = data
            .get("ids")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect());
        match op {
            "scripting.registerContentScripts" => {
                let parsed: Vec<(String, ext::ContentScript)> = data
                    .get("scripts")
                    .and_then(|s| s.as_array())
                    .map(|a| a.iter().map(rt::parse_registered).collect::<Result<Vec<_>, _>>())
                    .unwrap_or_else(|| Ok(Vec::new()))?;
                {
                    let mut all = self.ext.registered.borrow_mut();
                    let mine = all.entry(ext_id.to_string()).or_default();
                    for (i, (id, _)) in parsed.iter().enumerate() {
                        if mine.iter().any(|(old, _)| old == id) || parsed[..i].iter().any(|(other, _)| other == id) {
                            return Err(format!("Duplicate script ID '{id}'"));
                        }
                    }
                    if mine.len() + parsed.len() > rt::MAX_REGISTERED_SCRIPTS {
                        return Err("Too many content scripts are registered.".into());
                    }
                    mine.extend(parsed);
                }
                self.ext_rebuild_bundles();
                Ok(Value::Null)
            }
            "scripting.unregisterContentScripts" => {
                {
                    let mut all = self.ext.registered.borrow_mut();
                    let mine = all.entry(ext_id.to_string()).or_default();
                    if let Some(ids) = &ids {
                        if let Some(missing) = ids.iter().find(|id| !mine.iter().any(|(old, _)| old == *id)) {
                            return Err(format!("Nonexistent script ID '{missing}'"));
                        }
                        mine.retain(|(id, _)| !ids.contains(id));
                    } else {
                        mine.clear();
                    }
                }
                self.ext_rebuild_bundles();
                Ok(Value::Null)
            }
            _ => Ok(Value::Array(
                self.ext
                    .registered
                    .borrow()
                    .get(ext_id)
                    .map(|l| {
                        l.iter()
                            .filter(|(id, _)| ids.as_ref().is_none_or(|ids| ids.contains(id)))
                            .map(|(id, s)| rt::registered_json(id, s))
                            .collect()
                    })
                    .unwrap_or_default(),
            )),
        }
    }

    // ------------------------------------------------------------ scripting.executeScript

    /// Runs files or code in a tab and answers with the result (by value; a promise is
    /// awaited). `world: "MAIN"` runs in the page's own world, else in the extension's world.
    /// Either way the tab must have the extension's world: that is where Krunk decided the
    /// extension may run (not on Krunk's own pages or local files, private tabs only when
    /// allowed), and the extension needs host access to the document (`ext_script_target`).
    pub(super) fn ext_execute_script(
        &self,
        caller: &Caller,
        ext_id: &str,
        data: &Value,
    ) -> Option<Result<Value, String>> {
        let tab = data.get("tabId").and_then(|t| t.as_u64()).map(|t| t as TabId).or_else(|| self.active_id());
        let Some(tab) = tab else { return Some(Err("no tab".into())) };
        let main = data.get("world").and_then(|w| w.as_str()) == Some("MAIN");
        let context = match self.ext_script_target(tab, ext_id, main) {
            Ok(context) => context,
            Err(e) => return Some(Err(e)),
        };
        let found = self
            .ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == ext_id)
            .map(|e| (e.root.clone(), ext::may_use_scripting(&e.manifest)));
        let Some((root, permitted)) = found else { return Some(Err("unknown extension".into())) };
        // As in Chrome: the `scripting` permission (0.11.7, audit C1).
        if !permitted {
            return Some(Err("The extension doesn't have the \"scripting\" permission.".into()));
        }
        let Some(view) = self.tab_view(tab) else { return Some(Err("no such tab".into())) };
        let mut code = String::new();
        for f in data.get("files").and_then(|f| f.as_array()).into_iter().flatten().filter_map(|f| f.as_str()) {
            if !ext::safe_relative_path(f) {
                return Some(Err(format!("{f}: not a file of the extension")));
            }
            match std::fs::read_to_string(ext::file_path(&root, f.trim_start_matches('/'))) {
                Ok(src) => {
                    code.push_str(&src);
                    code.push_str("\n;\n");
                }
                Err(e) => return Some(Err(format!("Could not load file: '{f}'. ({e})"))),
            }
        }
        if let Some(inline) = data.get("code").and_then(|c| c.as_str()) {
            code.push_str(inline);
        }
        if code.is_empty() {
            return Some(Err("nothing to run".into()));
        }
        let mut params =
            json!({ "expression": code, "returnByValue": true, "awaitPromise": true, "userGesture": true });
        if !main {
            params["contextId"] = json!(context);
        }
        let weak = self.me.clone();
        let (caller, ext_owned) = (caller.clone(), ext_id.to_string());
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            let Some(b) = weak.upgrade() else { return };
            let result = match r {
                Ok(v) => match v.get("exceptionDetails") {
                    Some(d) => Err(d
                        .pointer("/exception/description")
                        .or(d.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("The script threw.")
                        .chars()
                        .take(500)
                        .collect()),
                    None => Ok(json!([{
                        "frameId": 0, "documentId": Value::Null,
                        "result": v.pointer("/result/value").cloned().unwrap_or(Value::Null),
                    }])),
                },
                Err(e) => Err(e),
            };
            b.ext_reply(&caller, &ext_owned, result);
        });
        None
    }

    // ------------------------------------------------------------ errors

    /// An error of one of the extension's contexts: krunk.log and the extension's Errors list
    /// on krunk://extensions (in memory, capped; open pages are told at most once a second).
    pub(super) fn ext_error(&self, ext_id: &str, message: &str, file: &str, line: u64, context: &str) {
        let name = self.ext_name(ext_id);
        // In the log: capped, on one line, and not more than so many lines a minute.
        let short = lb_core::ext_guard::log_text(message, 500);
        let changed = {
            let mut errors = self.ext.errors.borrow_mut();
            rt::push_error(errors.entry(ext_id.to_string()).or_default(), ext_id, message, file, line, context)
        };
        if !changed {
            return;
        }
        // Over the limit it is not in the log; the list on krunk://extensions has it.
        let logged = self.ext_may_log(ext_id, "error");
        if logged && file.is_empty() {
            warning!("extension {name}: {short} [{context}]");
        } else if logged {
            let file = lb_core::ext_guard::log_text(file, 200);
            warning!("extension {name}: {short} [{context}, {file}:{line}]");
        }
        if !self.ext.errors_dirty.replace(true) {
            let weak = self.me.clone();
            crate::ffi::cef::post_ui_delayed(1000, move || {
                if let Some(b) = weak.upgrade() {
                    b.ext.errors_dirty.set(false);
                    b.push_ext_list();
                }
            });
        }
    }

    /// May another line of this kind about the extension go to krunk.log now? A limit per
    /// extension, kind and minute (`ext_guard::LogBudget`): an extension (or a page that makes
    /// its content script talk) cannot fill the log. With `KRUNK_EXT_LOG=1` nothing is dropped.
    pub(super) fn ext_may_log(&self, ext_id: &str, kind: &'static str) -> bool {
        use lb_core::ext_guard::LogVerdict;
        if log_all() {
            return true;
        }
        let verdict = self.ext.log_budget.borrow_mut().entry((ext_id.to_string(), kind)).or_default().take(now_ms());
        if verdict == LogVerdict::Last {
            warning!(
                "extension {}: {} \"{kind}\" lines in a minute: the rest of this minute is not logged",
                self.ext_name(ext_id),
                lb_core::ext_guard::LOG_LINES_PER_MINUTE
            );
        }
        verdict != LogVerdict::Drop
    }

    /// An uncaught error in a content-script world (`Runtime.exceptionThrown`): only the
    /// extensions' own worlds, never the page's.
    pub(super) fn ext_content_exception(&self, tab: TabId, params: &Value) {
        let Some(details) = params.get("exceptionDetails") else { return };
        let Some(context) = details.get("executionContextId").and_then(|c| c.as_i64()) else { return };
        let ext_id = self
            .ext
            .contexts
            .borrow()
            .get(&tab)
            .and_then(|l| l.iter().find(|c| c.id == context).map(|c| c.ext.clone()));
        let Some(ext_id) = ext_id else { return };
        let text = details
            .pointer("/exception/description")
            .or(details.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("Uncaught error");
        let file = details.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let line = details.get("lineNumber").and_then(|l| l.as_u64()).map(|l| l + 1).unwrap_or(0);
        self.ext_error(&ext_id, text, file, line, "content script");
    }
}
