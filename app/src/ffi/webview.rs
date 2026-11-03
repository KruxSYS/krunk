//! Safe wrappers over the WebView2 COM interfaces used by the browser.
//!
//! Everything returns plain Rust types; callbacks receive owned strings.
//! Event handlers registered here live as long as their controller.

use super::loader::Loader;
use std::ffi::c_void;
use std::path::Path;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CallDevToolsProtocolMethodCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DocumentTitleChangedEventHandler, FaviconChangedEventHandler, FocusChangedEventHandler, GetFaviconCompletedHandler,
    HistoryChangedEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, SourceChangedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, ZoomFactorChangedEventHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoTaskMemFree, IStream, STREAM_SEEK_SET};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Shell::SHCreateMemStream;

/// Profile used by normal tabs, the shell and the popup.
pub const PROFILE_DEFAULT: &str = "default";
/// Profile used by private tabs (InPrivate, nothing persisted).
pub const PROFILE_PRIVATE: &str = "private";

/// Reads a CoTaskMem string returned through an out-pointer and frees it.
fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: WebView2 returns NUL-terminated CoTaskMem strings; we free it once.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

/// Calls a getter of the form `fn(&self, *mut PWSTR) -> Result<()>`.
fn get_string(f: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut p = PWSTR::null();
    match f(&mut p) {
        Ok(()) => take(p),
        Err(_) => String::new(),
    }
}

fn get_bool(f: impl FnOnce(*mut BOOL) -> windows::core::Result<()>) -> bool {
    let mut b = BOOL(0);
    f(&mut b).is_ok() && b.as_bool()
}

/// Options for the single shared environment.
pub struct EnvironmentConfig {
    /// `AdditionalBrowserArguments` (from `Settings::browser_arguments`).
    pub browser_arguments: String,
    pub tracking_prevention: bool,
}

/// The shared `CoreWebView2Environment` (one per process).
#[derive(Clone)]
pub struct Environment(ICoreWebView2Environment);

impl Environment {
    /// Starts creating the environment; `done` runs on the UI thread.
    pub fn create(
        loader: &Loader,
        user_data_dir: &Path,
        config: &EnvironmentConfig,
        done: impl FnOnce(Result<Environment, String>) + 'static,
    ) -> Result<(), String> {
        let options = CoreWebView2EnvironmentOptions::default();
        // SAFETY: the setters write to the options object before it is shared
        // with WebView2; nothing else holds a reference yet.
        unsafe {
            options.set_additional_browser_arguments(config.browser_arguments.clone());
            options.set_enable_tracking_prevention(config.tracking_prevention);
            options.set_are_browser_extensions_enabled(false);
            options.set_allow_single_sign_on_using_os_primary_account(false);
        }
        let options: ICoreWebView2EnvironmentOptions = options.into();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |result, env| {
            done(match (result, env) {
                (Ok(()), Some(env)) => Ok(Environment(env)),
                (Err(e), _) => Err(format!("WebView2 environment creation failed: {e}")),
                (Ok(()), None) => Err("WebView2 returned no environment".into()),
            });
            Ok(())
        }));
        loader.create_environment(user_data_dir, &options, &handler)
    }

    /// Starts creating a controller hosted in `parent`. Private controllers
    /// use the InPrivate profile.
    pub fn create_controller(
        &self,
        parent: HWND,
        private: bool,
        done: impl FnOnce(Result<Controller, String>) + 'static,
    ) -> Result<(), String> {
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
            done(match (result, controller) {
                (Ok(()), Some(c)) => Controller::wrap(c),
                (Err(e), _) => Err(format!("WebView2 controller creation failed: {e}")),
                (Ok(()), None) => Err("WebView2 returned no controller".into()),
            });
            Ok(())
        }));
        // SAFETY: COM calls on a live environment on the UI thread.
        unsafe {
            let env10: ICoreWebView2Environment10 =
                self.0.cast().map_err(|_| "This WebView2 runtime is too old (profiles are not supported)")?;
            let options = env10.CreateCoreWebView2ControllerOptions().map_err(|e| e.to_string())?;
            let name = if private { PROFILE_PRIVATE } else { PROFILE_DEFAULT };
            options.SetProfileName(&HSTRING::from(name)).map_err(|e| e.to_string())?;
            options.SetIsInPrivateModeEnabled(private).map_err(|e| e.to_string())?;
            env10
                .CreateCoreWebView2ControllerWithOptions(parent, &options, &handler)
                .map_err(|e| format!("CreateCoreWebView2Controller failed: {e}"))
        }
    }

    /// Builds a response from bytes and a CRLF header block.
    fn response(
        &self,
        status: i32,
        reason: &str,
        headers: &str,
        body: &[u8],
    ) -> Option<ICoreWebView2WebResourceResponse> {
        // SAFETY: creating a memory stream copy of `body` and a response object.
        unsafe {
            let stream: Option<IStream> = SHCreateMemStream(Some(body));
            self.0
                .CreateWebResourceResponse(stream.as_ref(), status, &HSTRING::from(reason), &HSTRING::from(headers))
                .ok()
        }
    }
}

/// A key press seen by `AcceleratorKeyPressed`.
#[derive(Debug, Clone, Copy)]
pub struct KeyPress {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub repeat: bool,
}

/// A navigation about to start.
#[derive(Debug, Clone)]
pub struct NavigationStart {
    pub uri: String,
}

/// What to answer for a request to one of our internal origins.
pub struct InternalResponse {
    pub status: i32,
    pub reason: &'static str,
    pub headers: String,
    pub body: &'static [u8],
}

/// A pending `NewWindowRequested`: the host must call [`NewWindow::attach`]
/// with the new tab's controller (or drop it to let the request fail).
pub struct NewWindow {
    args: ICoreWebView2NewWindowRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
    pub uri: String,
}

impl NewWindow {
    /// Hands the new controller to the opener so `window.open` returns it.
    pub fn attach(self, controller: &Controller) {
        // SAFETY: live args and deferral from the event; completing once.
        unsafe {
            let _ = self.args.SetNewWindow(&controller.webview);
            let _ = self.args.SetHandled(true);
            let _ = self.deferral.Complete();
        }
    }
}

impl Drop for NewWindow {
    fn drop(&mut self) {
        // Completing twice is harmless; this covers the "never attached" case.
        // SAFETY: live deferral.
        let _ = unsafe { self.deferral.Complete() };
    }
}

/// One WebView (controller + its `CoreWebView2`).
#[derive(Clone)]
pub struct Controller {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

impl Controller {
    fn wrap(controller: ICoreWebView2Controller) -> Result<Controller, String> {
        // SAFETY: a freshly created controller always has a CoreWebView2.
        let webview = unsafe { controller.CoreWebView2() }.map_err(|e| e.to_string())?;
        let c = Controller { controller, webview };
        c.apply_base_settings();
        Ok(c)
    }

    /// Settings every WebView gets: no built-in autofill or password saving
    /// (we have our own vault), no status bar, no built-in accelerators that
    /// we implement ourselves.
    fn apply_base_settings(&self) {
        // SAFETY: COM calls on a live WebView; failures only mean an older
        // runtime without the setting, which we ignore.
        unsafe {
            if let Ok(s) = self.webview.Settings() {
                let _ = s.SetIsStatusBarEnabled(false);
                let _ = s.SetAreDevToolsEnabled(true);
                let _ = s.SetIsZoomControlEnabled(true);
                if let Ok(s4) = s.cast::<ICoreWebView2Settings4>() {
                    let _ = s4.SetIsGeneralAutofillEnabled(false);
                    let _ = s4.SetIsPasswordAutosaveEnabled(false);
                }
            }
        }
    }

    /// For the browser's own UI: no context menu, no devtools, no zoom, no
    /// swipe navigation, no default script dialogs.
    pub fn lock_down_ui(&self) {
        // SAFETY: COM calls on a live WebView.
        unsafe {
            if let Ok(s) = self.webview.Settings() {
                let _ = s.SetAreDefaultContextMenusEnabled(false);
                let _ = s.SetAreDevToolsEnabled(cfg!(debug_assertions));
                let _ = s.SetIsZoomControlEnabled(false);
                let _ = s.SetAreDefaultScriptDialogsEnabled(false);
                if let Ok(s3) = s.cast::<ICoreWebView2Settings3>() {
                    let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                }
                if let Ok(s6) = s.cast::<ICoreWebView2Settings6>() {
                    let _ = s6.SetIsSwipeNavigationEnabled(false);
                }
                if let Ok(s5) = s.cast::<ICoreWebView2Settings5>() {
                    let _ = s5.SetIsPinchZoomEnabled(false);
                }
            }
        }
    }

    pub fn set_bounds(&self, left: i32, top: i32, width: i32, height: i32) {
        let r = RECT { left, top, right: left + width.max(0), bottom: top + height.max(0) };
        // SAFETY: live controller.
        let _ = unsafe { self.controller.SetBounds(r) };
    }

    pub fn set_visible(&self, visible: bool) {
        // SAFETY: live controller.
        let _ = unsafe { self.controller.SetIsVisible(visible) };
    }

    pub fn focus(&self) {
        // SAFETY: live controller.
        let _ = unsafe { self.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
    }

    pub fn notify_parent_moved(&self) {
        // SAFETY: live controller.
        let _ = unsafe { self.controller.NotifyParentWindowPositionChanged() };
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        // SAFETY: live WebView; HSTRING outlives the call.
        unsafe { self.webview.Navigate(&HSTRING::from(url)) }.map_err(|e| e.to_string())
    }

    pub fn go_back(&self) {
        // SAFETY: live WebView.
        let _ = unsafe { self.webview.GoBack() };
    }

    pub fn go_forward(&self) {
        // SAFETY: live WebView.
        let _ = unsafe { self.webview.GoForward() };
    }

    pub fn reload(&self) {
        // SAFETY: live WebView.
        let _ = unsafe { self.webview.Reload() };
    }

    /// Reload bypassing the cache (DevTools protocol `Page.reload`).
    pub fn hard_reload(&self) {
        let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
        // SAFETY: live WebView; static method name and JSON.
        let _ = unsafe {
            self.webview.CallDevToolsProtocolMethod(
                &HSTRING::from("Page.reload"),
                &HSTRING::from(r#"{"ignoreCache":true}"#),
                &handler,
            )
        };
    }

    pub fn stop(&self) {
        // SAFETY: live WebView.
        let _ = unsafe { self.webview.Stop() };
    }

    pub fn zoom(&self) -> f64 {
        let mut z = 1.0;
        // SAFETY: live controller, valid out pointer.
        let _ = unsafe { self.controller.ZoomFactor(&mut z) };
        z
    }

    pub fn set_zoom(&self, factor: f64) {
        // SAFETY: live controller.
        let _ = unsafe { self.controller.SetZoomFactor(factor.clamp(0.25, 5.0)) };
    }

    pub fn open_devtools(&self) {
        // SAFETY: live WebView.
        let _ = unsafe { self.webview.OpenDevToolsWindow() };
    }

    pub fn print(&self) {
        // SAFETY: live WebView; ShowPrintUI needs ICoreWebView2_16.
        unsafe {
            if let Ok(w) = self.webview.cast::<ICoreWebView2_16>() {
                let _ = w.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER);
            }
        }
    }

    pub fn post_json(&self, json: &str) {
        // SAFETY: live WebView; HSTRING outlives the call.
        let _ = unsafe { self.webview.PostWebMessageAsJson(&HSTRING::from(json)) };
    }

    /// Background colour behind the page (0xAARRGGBB).
    pub fn set_background(&self, argb: u32) {
        let c =
            COREWEBVIEW2_COLOR { A: (argb >> 24) as u8, R: (argb >> 16) as u8, G: (argb >> 8) as u8, B: argb as u8 };
        // SAFETY: live controller.
        unsafe {
            if let Ok(c2) = self.controller.cast::<ICoreWebView2Controller2>() {
                let _ = c2.SetDefaultBackgroundColor(c);
            }
        }
    }

    pub fn close(&self) {
        // SAFETY: live controller; closing twice is harmless.
        let _ = unsafe { self.controller.Close() };
    }

    // ---------------- events ----------------

    /// `WebMessageReceived` from the top-level document: `(source, json)`.
    pub fn on_message(&self, mut f: impl FnMut(String, String) + 'static) {
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                // SAFETY: live event args during the callback.
                let source = get_string(|p| unsafe { args.Source(p) });
                let json = get_string(|p| unsafe { args.WebMessageAsJson(p) });
                f(source, json);
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_WebMessageReceived(&handler, &mut token) };
    }

    /// `NavigationStarting`; return `false` to cancel.
    pub fn on_navigation_starting(&self, mut f: impl FnMut(NavigationStart) -> bool + 'static) {
        let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                // SAFETY: live event args during the callback.
                let uri = get_string(|p| unsafe { args.Uri(p) });
                if !f(NavigationStart { uri }) {
                    // SAFETY: as above.
                    let _ = unsafe { args.SetCancel(true) };
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_NavigationStarting(&handler, &mut token) };
    }

    /// `NavigationCompleted(success)`.
    pub fn on_navigation_completed(&self, mut f: impl FnMut(bool) + 'static) {
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            // SAFETY: live event args during the callback.
            let ok = args.map(|a| get_bool(|b| unsafe { a.IsSuccess(b) })).unwrap_or(false);
            f(ok);
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_NavigationCompleted(&handler, &mut token) };
    }

    /// `SourceChanged(new URL)`.
    pub fn on_source_changed(&self, mut f: impl FnMut(String) + 'static) {
        let wv = self.webview.clone();
        let handler = SourceChangedEventHandler::create(Box::new(move |_, _| {
            // SAFETY: live WebView.
            f(get_string(|p| unsafe { wv.Source(p) }));
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_SourceChanged(&handler, &mut token) };
    }

    /// `DocumentTitleChanged(title)`.
    pub fn on_title_changed(&self, mut f: impl FnMut(String) + 'static) {
        let wv = self.webview.clone();
        let handler = DocumentTitleChangedEventHandler::create(Box::new(move |_, _| {
            // SAFETY: live WebView.
            f(get_string(|p| unsafe { wv.DocumentTitle(p) }));
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_DocumentTitleChanged(&handler, &mut token) };
    }

    /// `HistoryChanged(can_go_back, can_go_forward)`.
    pub fn on_history_changed(&self, mut f: impl FnMut(bool, bool) + 'static) {
        let wv = self.webview.clone();
        let handler = HistoryChangedEventHandler::create(Box::new(move |_, _| {
            // SAFETY: live WebView.
            let back = get_bool(|b| unsafe { wv.CanGoBack(b) });
            let fwd = get_bool(|b| unsafe { wv.CanGoForward(b) });
            f(back, fwd);
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_HistoryChanged(&handler, &mut token) };
    }

    /// `FaviconChanged`: delivers the favicon as PNG bytes (empty if none).
    pub fn on_favicon_changed(&self, f: impl FnMut(Vec<u8>) + 'static) {
        // FaviconChanged needs ICoreWebView2_15 (runtime 1.0.1185+).
        let Ok(wv15) = self.webview.cast::<ICoreWebView2_15>() else { return };
        let f = std::rc::Rc::new(std::cell::RefCell::new(f));
        let wv = wv15.clone();
        let handler = FaviconChangedEventHandler::create(Box::new(move |_, _| {
            let f = f.clone();
            let done = GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
                let bytes = match (result, stream) {
                    (Ok(()), Some(s)) => read_stream(&s, 256 * 1024),
                    _ => Vec::new(),
                };
                if let Ok(mut f) = f.try_borrow_mut() {
                    f(bytes);
                }
                Ok(())
            }));
            // SAFETY: live WebView; handler kept alive by WebView2.
            let _ = unsafe { wv.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &done) };
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { wv15.add_FaviconChanged(&handler, &mut token) };
    }

    /// `NewWindowRequested`: the callback gets a [`NewWindow`] to attach a
    /// new tab's controller to. The event is deferred until then.
    pub fn on_new_window(&self, mut f: impl FnMut(NewWindow) + 'static) {
        let handler = NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                // SAFETY: live event args; the deferral keeps them valid
                // until `NewWindow` completes it.
                unsafe {
                    let uri = get_string(|p| args.Uri(p));
                    if let Ok(deferral) = args.GetDeferral() {
                        let _ = args.SetHandled(true);
                        f(NewWindow { args, deferral, uri });
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { self.webview.add_NewWindowRequested(&handler, &mut token) };
    }

    /// `AcceleratorKeyPressed` (key-down only); return `true` if handled.
    pub fn on_accelerator(&self, mut f: impl FnMut(KeyPress) -> bool + 'static) {
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            // SAFETY: live event args during the callback; GetKeyState reads
            // this thread's keyboard state.
            unsafe {
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                let _ = args.KeyEventKind(&mut kind);
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
                    return Ok(());
                }
                let mut vk = 0u32;
                let _ = args.VirtualKey(&mut vk);
                let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                let _ = args.PhysicalKeyStatus(&mut status);
                let down = |k: i32| GetKeyState(k) < 0;
                let press = KeyPress {
                    vk,
                    ctrl: down(VK_CONTROL.0 as i32),
                    shift: down(VK_SHIFT.0 as i32),
                    alt: down(VK_MENU.0 as i32),
                    repeat: status.WasKeyDown.as_bool(),
                };
                if f(press) {
                    let _ = args.SetHandled(true);
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live controller.
        let _ = unsafe { self.controller.add_AcceleratorKeyPressed(&handler, &mut token) };
    }

    /// `GotFocus` on the controller.
    pub fn on_got_focus(&self, mut f: impl FnMut() + 'static) {
        let handler = FocusChangedEventHandler::create(Box::new(move |_, _| {
            f();
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live controller.
        let _ = unsafe { self.controller.add_GotFocus(&handler, &mut token) };
    }

    /// `ZoomFactorChanged(new factor)`.
    pub fn on_zoom_changed(&self, mut f: impl FnMut(f64) + 'static) {
        let c = self.controller.clone();
        let handler = ZoomFactorChangedEventHandler::create(Box::new(move |_, _| {
            let mut z = 1.0;
            // SAFETY: live controller, valid out pointer.
            let _ = unsafe { c.ZoomFactor(&mut z) };
            f(z);
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live controller.
        let _ = unsafe { self.controller.add_ZoomFactorChanged(&handler, &mut token) };
    }

    /// Serves requests matching `filter` (e.g. `https://lightbrowser.invalid/*`)
    /// from `resolve(uri)`; `None` answers 404.
    pub fn serve(&self, env: &Environment, filter: &str, resolve: impl Fn(&str) -> Option<InternalResponse> + 'static) {
        let env = env.clone();
        let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            // SAFETY: live event args during the callback.
            unsafe {
                let Ok(req) = args.Request() else { return Ok(()) };
                let uri = get_string(|p| req.Uri(p));
                let resp = match resolve(&uri) {
                    Some(r) => env.response(r.status, r.reason, &r.headers, r.body),
                    None => env.response(404, "Not Found", "Content-Type: text/plain", b"Not found"),
                };
                if let Some(resp) = resp {
                    let _ = args.SetResponse(&resp);
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a filter and handler on a live WebView.
        unsafe {
            let _ = self
                .webview
                .AddWebResourceRequestedFilter(&HSTRING::from(filter), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
            let _ = self.webview.add_WebResourceRequested(&handler, &mut token);
        }
    }
}

/// Reads an IStream from the start, up to `max` bytes.
fn read_stream(stream: &IStream, max: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    // SAFETY: reading into a local buffer with its exact length.
    unsafe {
        let _ = stream.Seek(0, STREAM_SEEK_SET, None);
        loop {
            let mut read = 0u32;
            let hr = stream.Read(buf.as_mut_ptr() as *mut c_void, buf.len() as u32, Some(&mut read));
            if hr.is_err() || read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() > max {
                return Vec::new();
            }
        }
    }
    out
}
