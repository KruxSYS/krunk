//! [`View`]: one CEF browser hosted as a child window (the shell strip, the
//! popup, or a tab), with the operations and events the host needs.
//!
//! A view is created first (so the host can register its event callbacks)
//! and then started; CEF reports the browser asynchronously. Calls made in
//! between (bounds, visibility, navigation, close) are remembered and applied
//! when it arrives. Callbacks are looked up through a thread-local registry
//! by a key that the browser's client carries, so a view the host has dropped
//! simply stops receiving events.

use super::client;
use super::net::RequestPolicy;
use super::renderer::TO_PAGE;
use cef::*;
use lb_core::zoom;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WS_CHILD, WS_CLIPCHILDREN,
    WS_CLIPSIBLINGS, WS_TABSTOP,
};
use zeroize::Zeroizing;

/// What a view is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The browser chrome (tab strip, address bar).
    Shell,
    /// Menus, the shield panel and prompts.
    Popup,
    /// Web content and the built-in pages.
    Tab,
}

/// Content scripts for a tab's renderer (passed at creation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scripts {
    /// `navigator.globalPrivacyControl` (all frames).
    pub gpc: bool,
    /// `navigator.doNotTrack` (all frames).
    pub dnt: bool,
    /// Generic cosmetic filtering: report class names and ids (main frame).
    pub page_tokens: bool,
    /// Login form detection and fill (main frame).
    pub login: bool,
    /// Report whether the page plays audio (all frames).
    pub audio: bool,
    /// The pointer-lock shim (all frames).
    pub pointer_lock: bool,
}

impl Scripts {
    pub(super) fn to_dictionary(self) -> Option<DictionaryValue> {
        let d = dictionary_value_create()?;
        for (k, v) in [
            ("gpc", self.gpc),
            ("dnt", self.dnt),
            ("tokens", self.page_tokens),
            ("login", self.login),
            ("audio", self.audio),
            ("plock", self.pointer_lock),
        ] {
            d.set_bool(Some(&CefString::from(k)), i32::from(v));
        }
        Some(d)
    }

    pub(super) fn from_dictionary(d: &DictionaryValue) -> Scripts {
        let get = |k: &str| d.bool(Some(&CefString::from(k))) == 1;
        Scripts {
            gpc: get("gpc"),
            dnt: get("dnt"),
            page_tokens: get("tokens"),
            login: get("login"),
            audio: get("audio"),
            pointer_lock: get("plock"),
        }
    }
}

/// How to create a view.
pub struct ViewOptions {
    pub role: Role,
    /// Use the in-memory private profile.
    pub private: bool,
    pub scripts: Scripts,
    /// Colour behind the page while it loads (0xAARRGGBB; 0 = white).
    pub background: u32,
    /// Network policy (tabs).
    pub policy: Option<Arc<dyn RequestPolicy>>,
}

/// A key press seen before the page gets it.
#[derive(Debug, Clone, Copy)]
pub struct KeyPress {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub repeat: bool,
}

/// A top-level navigation about to start.
#[derive(Debug, Clone)]
pub struct NavigationStart {
    pub url: String,
    /// The server redirected an earlier navigation to `url`.
    pub redirected: bool,
}

/// Loading state of the top-level document.
#[derive(Debug, Clone, Copy)]
pub struct LoadingState {
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// The frame a message came from, as reported by the engine.
#[derive(Debug, Clone)]
pub struct MessageSource {
    /// The frame's committed URL.
    pub url: String,
    pub main_frame: bool,
    /// The engine's identifier of the sending frame (for a reply to that frame).
    pub frame_id: String,
}

/// A page asked for a new window (`window.open`, `target=_blank`).
#[derive(Debug, Clone)]
pub struct NewWindow {
    pub url: String,
    /// `false` for background tabs (middle click, Ctrl+click).
    pub foreground: bool,
}

/// A download about to start. Dropping it without [`DownloadStart::accept`]
/// cancels the download.
pub struct DownloadStart {
    pub id: u32,
    pub url: String,
    pub suggested_name: String,
    pub total: Option<i64>,
    pub(super) callback: BeforeDownloadCallback,
}

impl DownloadStart {
    /// Starts the download into `path` (full file path).
    pub fn accept(self, path: &Path) {
        self.callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
    }
}

/// State of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    InProgress,
    Paused,
    Completed,
    Cancelled,
    /// Chromium's `DownloadInterruptReason`.
    Interrupted(i32),
}

/// Progress or state change of a download.
pub struct DownloadUpdate {
    pub id: u32,
    pub received: i64,
    pub total: Option<i64>,
    pub path: PathBuf,
    pub status: DownloadStatus,
    pub control: DownloadControl,
}

/// Cancel, pause or resume a running download.
#[derive(Clone)]
pub struct DownloadControl(pub(super) DownloadItemCallback);

impl DownloadControl {
    pub fn cancel(&self) {
        self.0.cancel();
    }
    pub fn pause(&self) {
        self.0.pause();
    }
    pub fn resume(&self) {
        self.0.resume();
    }
}

/// Which kinds of profile data to clear.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClearKinds {
    pub cookies: bool,
    pub cache: bool,
    pub site_data: bool,
}

/// An optional event callback. Taken out while it runs, so a callback that
/// triggers its own event again (re-entrancy) is skipped, not a panic.
pub(super) struct Slot<F: ?Sized>(RefCell<Option<Box<F>>>);

impl<F: ?Sized> Default for Slot<F> {
    fn default() -> Self {
        Slot(RefCell::new(None))
    }
}

impl<F: ?Sized> Slot<F> {
    fn set(&self, f: Box<F>) {
        *self.0.borrow_mut() = Some(f);
    }

    pub(super) fn with<R>(&self, call: impl FnOnce(&mut F) -> R) -> Option<R> {
        let mut f = self.0.borrow_mut().take()?;
        let r = call(&mut f);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        Some(r)
    }
}

type NewWindowFn = dyn FnMut(&NewWindow) -> Option<(View, HWND)>;
/// Result of a DevTools protocol call: the result object, or the error.
pub type DevToolsResult = Result<serde_json::Value, String>;
type DevToolsDone = Box<dyn FnOnce(DevToolsResult)>;
/// Where "Inspect" was chosen (view coordinates, DIP), if at an element.
pub type InspectAt = Option<(i32, i32)>;
/// The docked DevTools' place (x, y, width, height) and visibility, kept until it exists.
type DevToolsPlace = (Option<(i32, i32, i32, i32)>, Option<bool>);
/// A DevTools call that gets no answer in this time fails.
const DEVTOOLS_TIMEOUT_MS: i64 = 10_000;
type MessageFn = dyn FnMut(&MessageSource, &str);

#[derive(Default)]
pub(super) struct Events {
    pub before_browse: Slot<dyn FnMut(&NavigationStart) -> bool>,
    pub loading: Slot<dyn FnMut(LoadingState)>,
    pub load_finished: Slot<dyn FnMut(Option<i32>)>,
    pub load_start: Slot<dyn FnMut()>,
    pub address: Slot<dyn FnMut(String)>,
    pub title: Slot<dyn FnMut(String)>,
    /// The page's status text (the address of the link under the mouse, or empty).
    pub status: Slot<dyn FnMut(String)>,
    pub favicon: Slot<dyn FnMut(Vec<u8>)>,
    pub new_window: Slot<NewWindowFn>,
    pub open_tab: Slot<dyn FnMut(String, bool)>,
    pub key: Slot<dyn FnMut(KeyPress) -> bool>,
    pub got_focus: Slot<dyn FnMut()>,
    pub set_focus: Slot<dyn FnMut(bool) -> bool>,
    pub message: Slot<MessageFn>,
    pub find: Slot<dyn FnMut(i32, i32)>,
    pub download_start: Slot<dyn FnMut(DownloadStart)>,
    pub download_update: Slot<dyn FnMut(DownloadUpdate)>,
    pub created: Slot<dyn FnMut()>,
    pub crashed: Slot<dyn FnMut(String)>,
    pub audio: Slot<dyn FnMut(bool)>,
    /// A frame's pointer-lock message: (frame identifier, JSON).
    pub pointer: Slot<dyn FnMut(String, String)>,
    pub fullscreen: Slot<dyn FnMut(bool)>,
    /// "Inspect" in the page's context menu, at this point of the page (DIP).
    pub inspect: Slot<dyn FnMut(InspectAt)>,
    /// The docked DevTools opened (true) or closed.
    pub devtools: Slot<dyn FnMut(bool)>,
    /// The engine asks whether a page may reach the local network (Krunk 0.5.0): the prompt
    /// id, the requesting origin and whether only this computer (loopback) is asked for.
    pub permission: Slot<dyn FnMut(u64, String, bool)>,
    /// The engine withdrew a permission prompt (the page navigated away).
    pub permission_gone: Slot<dyn FnMut(u64)>,
}

/// Remembered until the browser exists.
#[derive(Default)]
struct Pending {
    bounds: Option<(i32, i32, i32, i32)>,
    visible: Option<bool>,
    url: Option<String>,
    focus: bool,
    close: bool,
}

pub(super) struct ViewInner {
    pub key: u64,
    pub role: Role,
    pub private: bool,
    pub scripts: Scripts,
    pub background: u32,
    pub policy: Option<Arc<dyn RequestPolicy>>,
    browser: RefCell<Option<Browser>>,
    pending: RefCell<Pending>,
    started: Cell<bool>,
    closed: Cell<bool>,
    /// Net error of the current load cycle (reported when loading stops).
    pub load_error: Cell<Option<i32>>,
    /// The certificate error of the latest navigation (the engine cancelled it), until the
    /// next navigation starts: the lock popup explains it.
    pub cert_error: RefCell<Option<lb_core::certinfo::SslReport>>,
    /// Frames (by identifier) that report playing audio.
    audible_frames: RefCell<std::collections::HashSet<String>>,
    /// Open permission prompts the host still has to answer, by prompt id.
    pub(super) permission_prompts: RefCell<std::collections::HashMap<u64, PermissionPromptCallback>>,
    devtools_id: Cell<i32>,
    /// DevTools protocol calls in flight: method (for error logs) and the
    /// callback waiting for the result, if any.
    devtools_calls: RefCell<HashMap<i32, (String, Option<DevToolsDone>)>>,
    devtools_observer: RefCell<Option<Registration>>,
    /// The docked DevTools browser, once it exists, and its place until then.
    devtools: RefCell<Option<Browser>>,
    devtools_pending: Cell<DevToolsPlace>,
    pub ev: Events,
}

thread_local! {
    static VIEWS: RefCell<HashMap<u64, Weak<ViewInner>>> = RefCell::new(HashMap::new());
    static NEXT_KEY: Cell<u64> = const { Cell::new(1) };
}

/// The live view registered under `key` (UI thread).
pub(super) fn lookup(key: u64) -> Option<Rc<ViewInner>> {
    VIEWS.with(|v| v.borrow().get(&key).and_then(Weak::upgrade))
}

impl Drop for ViewInner {
    fn drop(&mut self) {
        let _ = VIEWS.try_with(|v| v.borrow_mut().remove(&self.key));
        // Waiting DevTools calls fail (later, not inside whatever dropped us).
        for (_, (_, done)) in self.devtools_calls.borrow_mut().drain() {
            if let Some(d) = done {
                super::post_ui(move || d(Err("the page was closed".into())));
            }
        }
        // The host let go of the view: its browser goes too.
        if let Some(b) = self.browser.borrow_mut().take() {
            if let Some(h) = b.host() {
                h.close_browser(1);
            }
        }
    }
}

/// Handle to one browser view (cheap to clone; UI thread only).
#[derive(Clone)]
pub struct View(pub(super) Rc<ViewInner>);

impl View {
    /// Registers a new view. Set its event callbacks, then call
    /// [`View::start`] (or hand it to a popup request).
    pub fn new(options: ViewOptions) -> View {
        let key = NEXT_KEY.with(|k| {
            let key = k.get();
            k.set(key + 1);
            key
        });
        let inner = Rc::new(ViewInner {
            key,
            role: options.role,
            private: options.private,
            scripts: options.scripts,
            background: options.background,
            policy: options.policy,
            browser: RefCell::new(None),
            pending: RefCell::new(Pending::default()),
            started: Cell::new(false),
            closed: Cell::new(false),
            load_error: Cell::new(None),
            cert_error: RefCell::new(None),
            permission_prompts: RefCell::new(std::collections::HashMap::new()),
            audible_frames: RefCell::new(std::collections::HashSet::new()),
            devtools_id: Cell::new(0),
            devtools_calls: RefCell::new(HashMap::new()),
            devtools_observer: RefCell::new(None),
            devtools: RefCell::new(None),
            devtools_pending: Cell::new((None, None)),
            ev: Events::default(),
        });
        VIEWS.with(|v| v.borrow_mut().insert(key, Rc::downgrade(&inner)));
        View(inner)
    }

    /// Creates the browser as a hidden child of `parent`, loading `url`.
    pub fn start(&self, parent: HWND, url: &str) -> Result<(), String> {
        if self.0.started.replace(true) {
            return Ok(());
        }
        let window_info = child_window_info(parent);
        let mut client = client::for_view(&self.0);
        let settings = self.browser_settings();
        let context = if self.0.private {
            Some(super::private_context().ok_or("could not create the private profile")?)
        } else {
            None
        };
        let mut extra = self.0.scripts.to_dictionary();
        if let Some(d) = extra.as_mut() {
            super::scriptlets::add_to_extra_info(d);
        }
        let ok = browser_host_create_browser(
            Some(&window_info),
            Some(&mut client),
            Some(&CefString::from(url)),
            Some(&settings),
            extra.as_mut(),
            context.clone().as_mut(),
        );
        if ok != 1 {
            return Err("the engine could not create a browser".into());
        }
        Ok(())
    }

    pub(super) fn browser_settings(&self) -> BrowserSettings {
        BrowserSettings { background_color: self.0.background, ..Default::default() }
    }

    pub(super) fn mark_started(&self) {
        self.0.started.set(true);
    }

    pub fn is_private(&self) -> bool {
        self.0.private
    }

    fn browser(&self) -> Option<Browser> {
        self.0.browser.borrow().clone()
    }

    fn host(&self) -> Option<BrowserHost> {
        self.browser()?.host()
    }

    /// The browser's own child window, once it exists.
    pub fn window_handle(&self) -> Option<HWND> {
        self.hwnd()
    }

    fn hwnd(&self) -> Option<HWND> {
        let h = self.host()?.window_handle();
        (!h.0.is_null()).then(|| HWND(h.0.cast()))
    }

    // ------------------------------------------------------------ lifecycle

    /// The browser exists (`on_after_created`): apply what was asked for so far.
    pub(super) fn attach(inner: &Rc<ViewInner>, browser: &Browser) {
        *inner.browser.borrow_mut() = Some(browser.clone());
        let view = View(inner.clone());
        let pending = std::mem::take(&mut *inner.pending.borrow_mut());
        if pending.close {
            view.close();
            return;
        }
        if let Some((x, y, w, h)) = pending.bounds {
            view.set_bounds(x, y, w, h);
        }
        if let Some(url) = pending.url {
            view.navigate(&url);
        }
        view.set_visible(pending.visible.unwrap_or(false));
        if pending.focus {
            view.focus();
        }
        inner.ev.created.with(|f| f());
    }

    pub(super) fn detach(inner: &ViewInner) {
        inner.browser.borrow_mut().take();
        inner.closed.set(true);
    }

    /// Closes the browser (its child window is destroyed).
    pub fn close(&self) {
        if self.0.closed.replace(true) {
            return;
        }
        match self.host() {
            Some(h) => h.close_browser(1),
            None => self.0.pending.borrow_mut().close = true,
        }
    }

    // ------------------------------------------------------------ window

    /// Moves the browser's child window into another frame. The browser, its page, history
    /// and request context stay as they are: nothing reloads (verified on Windows 11 with
    /// CEF 156, see .pipeline/evidence-0.3.0/spike).
    pub fn reparent(&self, parent: HWND) -> bool {
        let Some(h) = self.hwnd() else { return false };
        // SAFETY: both windows belong to this (UI) thread.
        if unsafe { windows::Win32::UI::WindowsAndMessaging::SetParent(h, Some(parent)) }.is_err() {
            return false;
        }
        // Docked DevTools goes along.
        if let Some(d) = self.devtools_hwnd() {
            // SAFETY: as above.
            let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::SetParent(d, Some(parent)) };
            if let Some(dh) = self.0.devtools.borrow().as_ref().and_then(|b| b.host()) {
                dh.notify_move_or_resize_started();
                dh.was_resized();
            }
        }
        if let Some(host) = self.host() {
            host.notify_move_or_resize_started();
            host.was_resized();
        }
        true
    }

    pub fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        let (width, height) = (width.max(0), height.max(0));
        match self.hwnd() {
            // SAFETY: the browser's own child window; no z-order change.
            Some(h) => unsafe {
                let _ = SetWindowPos(h, None, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
            },
            None => self.0.pending.borrow_mut().bounds = Some((x, y, width, height)),
        }
    }

    /// Shows or hides the view; hidden browsers render nothing and run their
    /// timers slower.
    pub fn set_visible(&self, visible: bool) {
        match (self.hwnd(), self.host()) {
            (Some(h), Some(host)) => {
                // SAFETY: the browser's own child window.
                let _ = unsafe { ShowWindow(h, if visible { SW_SHOW } else { SW_HIDE }) };
                host.was_hidden(i32::from(!visible));
            }
            _ => self.0.pending.borrow_mut().visible = Some(visible),
        }
    }

    pub fn focus(&self) {
        match self.host() {
            Some(h) => h.set_focus(1),
            None => self.0.pending.borrow_mut().focus = true,
        }
    }

    /// The main window moved: close `<select>` dropdowns and the like.
    pub fn notify_moved(&self) {
        if let Some(h) = self.host() {
            h.notify_move_or_resize_started();
        }
    }

    // ------------------------------------------------------------ navigation

    pub fn navigate(&self, url: &str) {
        match self.browser().and_then(|b| b.main_frame()) {
            Some(f) => f.load_url(Some(&CefString::from(url))),
            None => self.0.pending.borrow_mut().url = Some(url.to_string()),
        }
    }

    pub fn go_back(&self) {
        if let Some(b) = self.browser() {
            b.go_back();
        }
    }

    pub fn go_forward(&self) {
        if let Some(b) = self.browser() {
            b.go_forward();
        }
    }

    pub fn reload(&self) {
        if let Some(b) = self.browser() {
            b.reload();
        }
    }

    /// Reload bypassing the cache.
    pub fn hard_reload(&self) {
        if let Some(b) = self.browser() {
            b.reload_ignore_cache();
        }
    }

    pub fn stop(&self) {
        if let Some(b) = self.browser() {
            b.stop_load();
        }
    }

    // ------------------------------------------------------------ page

    /// Zoom factor (1.0 = 100 %).
    pub fn zoom(&self) -> f64 {
        self.host().map(|h| zoom::level_to_factor(h.zoom_level())).unwrap_or(1.0)
    }

    pub fn set_zoom(&self, factor: f64) {
        if let Some(h) = self.host() {
            h.set_zoom_level(zoom::factor_to_level(factor));
        }
    }

    /// Sends a JSON message to the page's `__lbHost` listeners (main frame).
    pub fn post_json(&self, json: &str) {
        let Some(frame) = self.browser().and_then(|b| b.main_frame()) else { return };
        let Some(mut message) = process_message_create(Some(&CefString::from(TO_PAGE))) else { return };
        if let Some(args) = message.argument_list() {
            args.set_string(0, Some(&CefString::from(json)));
        }
        frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
    }

    /// Posts a JSON message to one frame's `__lbHost` listeners (any frame with the
    /// bridge); `false` if the frame is gone. Used for a login fill into the frame that
    /// asked.
    pub fn post_json_to_frame(&self, frame_id: &str, json: &str) -> bool {
        let Some(frame) = self.browser().and_then(|b| b.frame_by_identifier(Some(&CefString::from(frame_id)))) else {
            return false;
        };
        if frame.is_valid() != 1 {
            return false;
        }
        let Some(mut message) = process_message_create(Some(&CefString::from(TO_PAGE))) else { return false };
        if let Some(args) = message.argument_list() {
            args.set_string(0, Some(&CefString::from(json)));
        }
        frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
        true
    }

    /// The committed URL of one frame, if it still exists.
    pub fn frame_url(&self, frame_id: &str) -> Option<String> {
        let frame = self.browser()?.frame_by_identifier(Some(&CefString::from(frame_id)))?;
        (frame.is_valid() == 1).then(|| CefString::from(&frame.url()).to_string())
    }

    /// Sends a pointer-lock event (JSON) to one frame; `false` if the frame
    /// is gone.
    pub fn send_to_frame(&self, frame_id: &str, json: &str) -> bool {
        let Some(frame) = self.browser().and_then(|b| b.frame_by_identifier(Some(&CefString::from(frame_id)))) else {
            return false;
        };
        if frame.is_valid() != 1 {
            return false;
        }
        let Some(mut message) = process_message_create(Some(&CefString::from(super::renderer::POINTER_TO_FRAME)))
        else {
            return false;
        };
        if let Some(args) = message.argument_list() {
            args.set_string(0, Some(&CefString::from(json)));
        }
        frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
        true
    }

    /// Is this frame still part of the page?
    pub fn frame_live(&self, frame_id: &str) -> bool {
        self.browser()
            .and_then(|b| b.frame_by_identifier(Some(&CefString::from(frame_id))))
            .is_some_and(|f| f.is_valid() == 1)
    }

    /// Leaves page-requested full screen.
    pub fn exit_fullscreen(&self) {
        if let Some(h) = self.host() {
            h.exit_fullscreen(1);
        }
    }

    /// The view's window in screen coordinates (physical pixels).
    pub fn screen_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let hwnd = self.hwnd()?;
        let mut r = windows::Win32::Foundation::RECT::default();
        // SAFETY: the browser's own window and a local out pointer.
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut r) }.ok()?;
        Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }

    /// Runs `js` in the main frame's page context.
    pub fn execute_script(&self, js: &str) {
        if let Some(f) = self.browser().and_then(|b| b.main_frame()) {
            f.execute_java_script(Some(&CefString::from(js)), None, 0);
        }
    }

    /// Find in page: a new search, or the next/previous match of the same one.
    pub fn find(&self, text: &str, forward: bool, find_next: bool) {
        if let Some(h) = self.host() {
            h.find(Some(&CefString::from(text)), i32::from(forward), 0, i32::from(find_next));
        }
    }

    pub fn stop_find(&self) {
        if let Some(h) = self.host() {
            h.stop_finding(1);
        }
    }

    pub fn print(&self) {
        if let Some(h) = self.host() {
            h.print();
        }
    }

    /// Downloads `url` through this browser (its profile and cookies), like a link the page
    /// was told to download.
    pub fn start_download(&self, url: &str) {
        if let Some(h) = self.host() {
            h.start_download(Some(&CefString::from(url)));
        }
    }

    // ------------------------------------------------------------ docked DevTools

    /// Opens DevTools as a child window of the frame `parent`, next to this view (place it
    /// with [`View::set_devtools_bounds`]); with `at` (view coordinates, DIP) the element
    /// there is selected. An open DevTools is focused and inspects that element instead.
    pub fn show_devtools_docked(&self, parent: HWND, at: Option<(i32, i32)>) {
        let Some(h) = self.host() else { return };
        let (x, y, w, hh) = self.0.devtools_pending.get().0.unwrap_or((0, 0, 0, 0));
        // CEF takes the size from the rect but not the position; the host places it.
        let window_info = WindowInfo { runtime_style: RuntimeStyle::ALLOY, ..Default::default() }
            .set_as_child(sys::HWND(parent.0.cast()), &Rect { x, y, width: w, height: hh });
        let mut client = client::for_tool(self.0.key);
        let point = at.map(|(x, y)| Point { x, y });
        h.show_dev_tools(Some(&window_info), Some(&mut client), Some(&BrowserSettings::default()), point.as_ref());
    }

    /// Is the docked DevTools open (its browser exists)?
    pub fn devtools_open(&self) -> bool {
        self.0.devtools.borrow().is_some()
    }

    fn devtools_hwnd(&self) -> Option<HWND> {
        let h = self.0.devtools.borrow().as_ref()?.host()?.window_handle();
        (!h.0.is_null()).then(|| HWND(h.0.cast()))
    }

    pub fn set_devtools_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        let (width, height) = (width.max(0), height.max(0));
        let (_, visible) = self.0.devtools_pending.get();
        self.0.devtools_pending.set((Some((x, y, width, height)), visible));
        if let Some(h) = self.devtools_hwnd() {
            // SAFETY: the DevTools browser's own child window; no z-order change.
            unsafe {
                let _ = SetWindowPos(h, None, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }

    pub fn set_devtools_visible(&self, visible: bool) {
        let (bounds, _) = self.0.devtools_pending.get();
        self.0.devtools_pending.set((bounds, Some(visible)));
        let host = self.0.devtools.borrow().as_ref().and_then(|b| b.host());
        if let (Some(h), Some(host)) = (self.devtools_hwnd(), host) {
            // SAFETY: the DevTools browser's own child window.
            let _ = unsafe { ShowWindow(h, if visible { SW_SHOW } else { SW_HIDE }) };
            host.was_hidden(i32::from(!visible));
        }
    }

    pub fn close_devtools(&self) {
        if let Some(h) = self.host() {
            h.close_dev_tools();
        }
    }

    /// The docked DevTools exists: put it where the host wants it.
    pub(super) fn devtools_attach(inner: &Rc<ViewInner>, browser: &Browser) {
        *inner.devtools.borrow_mut() = Some(browser.clone());
        let view = View(inner.clone());
        let (bounds, visible) = inner.devtools_pending.get();
        if let Some((x, y, w, h)) = bounds {
            view.set_devtools_bounds(x, y, w, h);
        }
        view.set_devtools_visible(visible.unwrap_or(true));
        inner.ev.devtools.with(|f| f(true));
    }

    pub(super) fn devtools_detach(inner: &ViewInner, browser: &Browser) {
        let mine = inner.devtools.borrow().as_ref().is_some_and(|b| b.identifier() == browser.identifier());
        if mine {
            inner.devtools.borrow_mut().take();
            inner.devtools_pending.set((None, None));
            inner.ev.devtools.with(|f| f(false));
        }
    }

    /// The mouse position in this view (DIP), if it is over the view.
    pub fn cursor_position(&self) -> Option<(i32, i32)> {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::ScreenToClient;
        use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetCursorPos};
        let hwnd = self.hwnd()?;
        let mut p = POINT::default();
        let mut r = RECT::default();
        // SAFETY: plain queries on the browser's own window.
        unsafe {
            GetCursorPos(&mut p).ok()?;
            let _ = ScreenToClient(hwnd, &mut p);
            GetClientRect(hwnd, &mut r).ok()?;
        }
        if p.x < 0 || p.y < 0 || p.x >= r.right || p.y >= r.bottom {
            return None;
        }
        // SAFETY: plain query.
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96);
        let scale = f64::from(dpi) / 96.0;
        Some(((f64::from(p.x) / scale) as i32, (f64::from(p.y) / scale) as i32))
    }

    /// What the engine reports about the page's connection: the visible navigation entry's
    /// SSL status, with the certificate chain as DER. A navigation the engine refused for a
    /// certificate error has no entry of its own: that error is reported instead.
    pub fn ssl_report(&self) -> Option<lb_core::certinfo::SslReport> {
        let from_entry = self.host().and_then(|h| h.visible_navigation_entry()).and_then(|e| {
            let s = e.sslstatus()?;
            let url = CefString::from(&e.url()).to_string();
            Some(client::ssl_report(
                url,
                s.is_secure_connection() != 0,
                s.cert_status(),
                s.sslversion(),
                s.content_status(),
                s.x509_certificate(),
            ))
        });
        match from_entry {
            Some(r) if !r.chain.is_empty() || self.0.cert_error.borrow().is_none() => Some(r),
            _ => self.0.cert_error.borrow().clone().or(from_entry),
        }
    }

    /// The latest navigation was refused for a certificate error (the engine's error page
    /// shows): the address bar must not show a lock.
    pub fn has_cert_error(&self) -> bool {
        self.0.cert_error.borrow().is_some()
    }

    /// Calls a DevTools protocol method on this browser (fire and forget).
    pub fn devtools_method(&self, method: &str, params: &serde_json::Value) {
        self.send_devtools(method, params, None);
    }

    /// Calls a DevTools protocol method; `done` gets the result object or
    /// the error (also when the browser is gone or no answer came in time).
    pub fn devtools_call(&self, method: &str, params: &serde_json::Value, done: impl FnOnce(DevToolsResult) + 'static) {
        self.send_devtools(method, params, Some(Box::new(done)));
    }

    fn send_devtools(&self, method: &str, params: &serde_json::Value, done: Option<DevToolsDone>) {
        let fail = |done: Option<DevToolsDone>, why: &str| {
            if let Some(d) = done {
                d(Err(why.to_string()));
            }
        };
        let Some(h) = self.host() else { return fail(done, "the page is not open") };
        let Some(value) = parse_json(Some(&CefString::from(params.to_string().as_str())), JsonParserOptions::RFC)
        else {
            return fail(done, "bad parameters");
        };
        let Some(mut dict) = value.dictionary() else { return fail(done, "bad parameters") };
        let id = self.0.devtools_id.get() + 1;
        self.0.devtools_id.set(id);
        if self.0.devtools_observer.borrow().is_none() {
            let mut observer = client::devtools_observer(self.0.key);
            *self.0.devtools_observer.borrow_mut() = h.add_dev_tools_message_observer(Some(&mut observer));
        }
        let waits = done.is_some();
        self.0.devtools_calls.borrow_mut().insert(id, (method.to_string(), done));
        if h.execute_dev_tools_method(id, Some(&CefString::from(method)), Some(&mut dict)) == 0 {
            let call = self.0.devtools_calls.borrow_mut().remove(&id);
            crate::log::warning!("DevTools method {method} could not be sent");
            return fail(call.and_then(|c| c.1), "the engine refused the call");
        }
        if waits {
            let key = self.0.key;
            super::post_ui_delayed(DEVTOOLS_TIMEOUT_MS, move || {
                let late = lookup(key).and_then(|v| v.devtools_calls.borrow_mut().remove(&id));
                if let Some((method, Some(done))) = late {
                    crate::log::warning!("DevTools method {method} timed out");
                    done(Err("no answer from the engine".into()));
                }
            });
        }
    }

    /// Background-tab sleep, first stage (the view is already hidden): a
    /// full garbage collection of the page's JavaScript heap. (A simulated
    /// memory-pressure signal would reach every renderer, the visible tab's
    /// included, and left it unable to paint.)
    pub fn set_memory_low(&self, low: bool) {
        if low {
            self.devtools_method("HeapProfiler.collectGarbage", &serde_json::json!({}));
        }
    }

    /// `prefers-color-scheme` for this browser's pages: `None` follows
    /// Windows. (The profile-wide colour scheme of CEF does not reach web
    /// content in Alloy-style browsers.)
    pub fn set_color_scheme(&self, dark: Option<bool>) {
        let value = match dark {
            None => "",
            Some(true) => "dark",
            Some(false) => "light",
        };
        self.devtools_method(
            "Emulation.setEmulatedMedia",
            &serde_json::json!({ "features": [{ "name": "prefers-color-scheme", "value": value }] }),
        );
    }

    /// Overrides the user agent of this browser (`None` = the default).
    pub fn set_user_agent(&self, ua: Option<&str>) {
        self.devtools_method("Emulation.setUserAgentOverride", &serde_json::json!({ "userAgent": ua.unwrap_or("") }));
    }

    // ------------------------------------------------------------ events

    /// A top-level navigation is starting; return `false` to cancel it.
    pub fn on_before_browse(&self, f: impl FnMut(&NavigationStart) -> bool + 'static) {
        self.0.ev.before_browse.set(Box::new(f));
    }

    /// Loading started or stopped, or the history changed.
    pub fn on_loading(&self, f: impl FnMut(LoadingState) + 'static) {
        self.0.ev.loading.set(Box::new(f));
    }

    /// Loading stopped: `None` on success, else Chromium's net error code.
    pub fn on_load_finished(&self, f: impl FnMut(Option<i32>) + 'static) {
        self.0.ev.load_finished.set(Box::new(f));
    }

    /// A new top-level document committed (before DOMContentLoaded).
    pub fn on_load_start(&self, f: impl FnMut() + 'static) {
        self.0.ev.load_start.set(Box::new(f));
    }

    pub fn on_address(&self, f: impl FnMut(String) + 'static) {
        self.0.ev.address.set(Box::new(f));
    }

    pub fn on_title(&self, f: impl FnMut(String) + 'static) {
        self.0.ev.title.set(Box::new(f));
    }

    /// The link under the mouse (`CefDisplayHandler::OnStatusMessage`), empty when none.
    pub fn on_status(&self, f: impl FnMut(String) + 'static) {
        self.0.ev.status.set(Box::new(f));
    }

    /// The page's favicon as PNG bytes (empty when it has none).
    pub fn on_favicon(&self, f: impl FnMut(Vec<u8>) + 'static) {
        self.0.ev.favicon.set(Box::new(f));
    }

    /// The page opens a window. Return a new (not started) view and its
    /// parent window to adopt it (the engine creates the browser, keeping
    /// `window.opener`), or `None` to refuse.
    pub fn on_new_window(&self, f: impl FnMut(&NewWindow) -> Option<(View, HWND)> + 'static) {
        self.0.ev.new_window.set(Box::new(f));
    }

    /// A link should open in a new tab (`(url, foreground)`): middle click,
    /// or any new window from the shell and popup.
    pub fn on_open_tab(&self, f: impl FnMut(String, bool) + 'static) {
        self.0.ev.open_tab.set(Box::new(f));
    }

    /// Key down before the page sees it; return `true` if handled.
    pub fn on_key(&self, f: impl FnMut(KeyPress) -> bool + 'static) {
        self.0.ev.key.set(Box::new(f));
    }

    pub fn on_got_focus(&self, f: impl FnMut() + 'static) {
        self.0.ev.got_focus.set(Box::new(f));
    }

    /// The engine asks whether the page may reach the local network: (prompt id, requesting
    /// origin, loopback only). Answer with `answer_permission`.
    pub fn on_permission(&self, f: impl FnMut(u64, String, bool) + 'static) {
        self.0.ev.permission.set(Box::new(f));
    }

    /// The engine withdrew the prompt (navigation): the host's prompt should go too.
    pub fn on_permission_gone(&self, f: impl FnMut(u64) + 'static) {
        self.0.ev.permission_gone.set(Box::new(f));
    }

    /// Answers an open permission prompt; false when the engine no longer waits for it.
    pub fn answer_permission(&self, prompt_id: u64, allow: bool) -> bool {
        let Some(cb) = self.0.permission_prompts.borrow_mut().remove(&prompt_id) else { return false };
        cb.cont(if allow { PermissionRequestResult::ACCEPT } else { PermissionRequestResult::DENY });
        true
    }

    /// The engine is about to focus this browser (`true`: because a page is
    /// loading, `false`: a click or other system focus). Return `false` to
    /// refuse.
    pub fn on_set_focus(&self, f: impl FnMut(bool) -> bool + 'static) {
        self.0.ev.set_focus.set(Box::new(f));
    }

    /// A message from the page's `__lbHost.postMessage`. The JSON may hold a
    /// password; it is wiped after the callback.
    pub fn on_message(&self, f: impl FnMut(&MessageSource, &str) + 'static) {
        self.0.ev.message.set(Box::new(f));
    }

    /// Find results: `(active match, total)`.
    pub fn on_find_result(&self, f: impl FnMut(i32, i32) + 'static) {
        self.0.ev.find.set(Box::new(f));
    }

    pub fn on_download_start(&self, f: impl FnMut(DownloadStart) + 'static) {
        self.0.ev.download_start.set(Box::new(f));
    }

    pub fn on_download_update(&self, f: impl FnMut(DownloadUpdate) + 'static) {
        self.0.ev.download_update.set(Box::new(f));
    }

    /// The browser exists (bounds, visibility and navigation were applied).
    pub fn on_created(&self, f: impl FnMut() + 'static) {
        self.0.ev.created.set(Box::new(f));
    }

    /// The page started or stopped playing audio (any frame).
    pub fn on_audio(&self, f: impl FnMut(bool) + 'static) {
        self.0.ev.audio.set(Box::new(f));
    }

    /// A frame sent a pointer-lock message: `(frame identifier, JSON)`.
    pub fn on_pointer_lock(&self, f: impl FnMut(String, String) + 'static) {
        self.0.ev.pointer.set(Box::new(f));
    }

    /// The page entered or left full screen (`requestFullscreen`).
    pub fn on_fullscreen(&self, f: impl FnMut(bool) + 'static) {
        self.0.ev.fullscreen.set(Box::new(f));
    }

    /// The renderer process of this view ended unexpectedly.
    pub fn on_inspect(&self, f: impl FnMut(Option<(i32, i32)>) + 'static) {
        self.0.ev.inspect.set(Box::new(f));
    }

    pub fn on_devtools(&self, f: impl FnMut(bool) + 'static) {
        self.0.ev.devtools.set(Box::new(f));
    }

    pub fn on_crashed(&self, f: impl FnMut(String) + 'static) {
        self.0.ev.crashed.set(Box::new(f));
    }
}

impl ViewInner {
    /// A DevTools protocol call finished: failures are logged, a waiting
    /// callback gets the result.
    pub(super) fn devtools_result(&self, id: i32, success: bool, result: &[u8]) {
        let Some((method, done)) = self.devtools_calls.borrow_mut().remove(&id) else { return };
        if !success {
            let text = String::from_utf8_lossy(&result[..result.len().min(300)]).into_owned();
            // Callers that handle the error log it themselves if needed.
            if done.is_none() {
                crate::log::warning!("DevTools method {method} failed: {text}");
            }
            if let Some(d) = done {
                let message = serde_json::from_slice::<serde_json::Value>(result)
                    .ok()
                    .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
                    .unwrap_or(text);
                d(Err(message));
            }
            return;
        }
        if let Some(d) = done {
            d(serde_json::from_slice(result).map_err(|e| e.to_string()));
        }
    }

    /// A frame reported its audio state (`frame` = None: forget all frames,
    /// e.g. a new top-level document).
    pub(super) fn frame_audio(&self, frame: Option<String>, playing: bool) {
        let (before, after) = {
            let mut set = self.audible_frames.borrow_mut();
            let before = !set.is_empty();
            match frame {
                Some(f) if playing => {
                    set.insert(f);
                }
                Some(f) => {
                    set.remove(&f);
                }
                None => set.clear(),
            }
            (before, !set.is_empty())
        };
        if before != after {
            self.ev.audio.with(|f| f(after));
        }
    }

    /// Wipes our copy of a page message after the callback ran.
    pub(super) fn deliver_message(&self, source: &MessageSource, json: Zeroizing<String>) {
        self.ev.message.with(|f| f(source, &json));
    }
}

/// Window info for a hidden child browser of `parent`.
pub(super) fn child_window_info(parent: HWND) -> WindowInfo {
    WindowInfo {
        style: (WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | WS_TABSTOP).0,
        parent_window: sys::HWND(parent.0.cast()),
        bounds: Rect { x: 0, y: 0, width: 0, height: 0 },
        runtime_style: RuntimeStyle::ALLOY,
        ..Default::default()
    }
}
