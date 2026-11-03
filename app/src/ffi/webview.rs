//! Safe wrappers over the WebView2 COM interfaces used by the browser.
//!
//! Everything returns plain Rust types; callbacks receive owned strings.
//! Event handlers registered here live as long as their controller.

use super::loader::Loader;
use std::borrow::Cow;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    BytesReceivedChangedEventHandler, CallDevToolsProtocolMethodCompletedHandler, ClearBrowsingDataCompletedHandler,
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DOMContentLoadedEventHandler, DocumentTitleChangedEventHandler,
    DownloadStartingEventHandler, ExecuteScriptCompletedHandler, FaviconChangedEventHandler,
    FindActiveMatchIndexChangedEventHandler, FindMatchCountChangedEventHandler, FindStartCompletedHandler,
    FocusChangedEventHandler, GetFaviconCompletedHandler, HistoryChangedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, SourceChangedEventHandler,
    StateChangedEventHandler, WebMessageReceivedEventHandler, WebResourceRequestedEventHandler,
    ZoomFactorChangedEventHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoTaskMemFree, IStream, STREAM_SEEK_SET};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Shell::SHCreateMemStream;
use zeroize::Zeroizing;

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

/// Like [`take`] for strings that may hold secrets: the CoTaskMem buffer is
/// zeroed before it is freed and the copy is `Zeroizing`.
fn take_wiped(p: PWSTR) -> Zeroizing<String> {
    if p.is_null() {
        return Zeroizing::new(String::new());
    }
    // SAFETY: NUL-terminated CoTaskMem wide string from WebView2; we read it,
    // overwrite it in place (same length) and free it exactly once.
    unsafe {
        let len = p.len();
        let s = Zeroizing::new(p.to_string().unwrap_or_default());
        std::ptr::write_bytes(p.0, 0, len);
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
                (Ok(()), Some(c)) => Controller::wrap(c, private),
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
            let stream: Option<IStream> = if body.is_empty() { None } else { SHCreateMemStream(Some(body)) };
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
    pub body: Cow<'static, [u8]>,
}

/// A request seen by [`Controller::intercept_all`].
pub struct ResourceRequest<'a> {
    pub uri: &'a str,
    /// `COREWEBVIEW2_WEB_RESOURCE_CONTEXT` value.
    pub context: i32,
}

/// What to do with an intercepted request.
pub enum ResourceAction {
    /// Answer from the host (internal origins).
    Serve(InternalResponse),
    NotFound,
    /// Answer with an empty 403 (blocked by the ad/tracker filter).
    Block,
    /// Let it go to the network, optionally adding privacy headers.
    Continue {
        dnt: bool,
        gpc: bool,
    },
}

/// Which kinds of profile data to clear.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClearKinds {
    pub cookies: bool,
    pub cache: bool,
    pub site_data: bool,
}

/// A `DownloadStarting` event; dropping it lets the download proceed.
pub struct DownloadStart {
    args: ICoreWebView2DownloadStartingEventArgs,
    deferral: ICoreWebView2Deferral,
    pub operation: Download,
    pub uri: String,
    /// Where WebView2 intends to save the file.
    pub suggested_path: PathBuf,
    pub total: Option<i64>,
}

impl DownloadStart {
    pub fn set_path(&self, path: &Path) {
        // SAFETY: live event args (kept valid by the deferral).
        let _ = unsafe { self.args.SetResultFilePath(&HSTRING::from(path.as_os_str())) };
    }

    /// Cancels the download before it starts (e.g. "Save as" was cancelled).
    pub fn cancel(&self) {
        // SAFETY: live event args.
        let _ = unsafe { self.args.SetCancel(true) };
    }
}

impl Drop for DownloadStart {
    fn drop(&mut self) {
        // SAFETY: live deferral, completed exactly once here.
        let _ = unsafe { self.deferral.Complete() };
    }
}

/// State of a download operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    InProgress,
    Interrupted(i32),
    Completed,
}

/// A running download (`ICoreWebView2DownloadOperation`).
#[derive(Clone)]
pub struct Download(ICoreWebView2DownloadOperation);

impl Download {
    pub fn received(&self) -> i64 {
        let mut n = 0i64;
        // SAFETY: live operation, valid out pointer.
        let _ = unsafe { self.0.BytesReceived(&mut n) };
        n
    }

    pub fn total(&self) -> Option<i64> {
        let mut n = 0i64;
        // SAFETY: as above.
        let _ = unsafe { self.0.TotalBytesToReceive(&mut n) };
        (n > 0).then_some(n)
    }

    pub fn path(&self) -> PathBuf {
        // SAFETY: as above.
        PathBuf::from(get_string(|p| unsafe { self.0.ResultFilePath(p) }))
    }

    pub fn status(&self) -> DownloadStatus {
        let mut st = COREWEBVIEW2_DOWNLOAD_STATE::default();
        let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
        // SAFETY: as above.
        unsafe {
            let _ = self.0.State(&mut st);
            let _ = self.0.InterruptReason(&mut reason);
        }
        match st {
            COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => DownloadStatus::Completed,
            COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED => DownloadStatus::Interrupted(reason.0),
            _ => DownloadStatus::InProgress,
        }
    }

    pub fn cancel(&self) {
        // SAFETY: live operation.
        let _ = unsafe { self.0.Cancel() };
    }

    pub fn pause(&self) {
        // SAFETY: live operation.
        let _ = unsafe { self.0.Pause() };
    }

    pub fn resume(&self) {
        // SAFETY: live operation.
        let _ = unsafe { self.0.Resume() };
    }

    pub fn on_progress(&self, mut f: impl FnMut() + 'static) {
        let handler = BytesReceivedChangedEventHandler::create(Box::new(move |_, _| {
            f();
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live operation.
        let _ = unsafe { self.0.add_BytesReceivedChanged(&handler, &mut token) };
    }

    pub fn on_state(&self, mut f: impl FnMut() + 'static) {
        let handler = StateChangedEventHandler::create(Box::new(move |_, _| {
            f();
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live operation.
        let _ = unsafe { self.0.add_StateChanged(&handler, &mut token) };
    }
}

/// Find-in-page session of one WebView (`ICoreWebView2Find`).
#[derive(Clone)]
pub struct Find(ICoreWebView2Find);

impl Find {
    /// Starts a new search for `term` (stops any previous one).
    pub fn start(&self, env: &Environment, term: &str) -> bool {
        // SAFETY: COM calls on live objects; needs runtime 1.0.2849+.
        unsafe {
            let Ok(env15) = env.0.cast::<ICoreWebView2Environment15>() else { return false };
            let Ok(opts) = env15.CreateFindOptions() else { return false };
            let _ = opts.SetFindTerm(&HSTRING::from(term));
            let _ = opts.SetShouldHighlightAllMatches(true);
            let _ = opts.SetSuppressDefaultFindDialog(true);
            let done = FindStartCompletedHandler::create(Box::new(|_| Ok(())));
            self.0.Start(&opts, &done).is_ok()
        }
    }

    pub fn next(&self) {
        // SAFETY: live find object.
        let _ = unsafe { self.0.FindNext() };
    }

    pub fn previous(&self) {
        // SAFETY: live find object.
        let _ = unsafe { self.0.FindPrevious() };
    }

    pub fn stop(&self) {
        // SAFETY: live find object.
        let _ = unsafe { self.0.Stop() };
    }

    /// `(active match, match count)`; the active index is 1-based for display.
    pub fn result(&self) -> (i32, i32) {
        let (mut active, mut total) = (0i32, 0i32);
        // SAFETY: live find object, valid out pointers.
        unsafe {
            let _ = self.0.ActiveMatchIndex(&mut active);
            let _ = self.0.MatchCount(&mut total);
        }
        (if total > 0 { active + 1 } else { 0 }, total.max(0))
    }

    /// Calls `f` whenever the active match or the match count changes.
    pub fn on_change(&self, f: impl FnMut() + 'static) {
        let f = std::rc::Rc::new(std::cell::RefCell::new(f));
        let f2 = f.clone();
        let a = FindActiveMatchIndexChangedEventHandler::create(Box::new(move |_, _| {
            if let Ok(mut f) = f.try_borrow_mut() {
                f();
            }
            Ok(())
        }));
        let m = FindMatchCountChangedEventHandler::create(Box::new(move |_, _| {
            if let Ok(mut f) = f2.try_borrow_mut() {
                f();
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering handlers on a live find object.
        unsafe {
            let _ = self.0.add_ActiveMatchIndexChanged(&a, &mut token);
            let _ = self.0.add_MatchCountChanged(&m, &mut token);
        }
    }
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
    private: bool,
}

impl Controller {
    fn wrap(controller: ICoreWebView2Controller, private: bool) -> Result<Controller, String> {
        // SAFETY: a freshly created controller always has a CoreWebView2.
        let webview = unsafe { controller.CoreWebView2() }.map_err(|e| e.to_string())?;
        let c = Controller { controller, webview, private };
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

    /// Whether this WebView uses the InPrivate profile.
    pub fn is_private(&self) -> bool {
        self.private
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

    /// The find-in-page object (`None` on runtimes older than 1.0.2849).
    pub fn find(&self) -> Option<Find> {
        let wv: ICoreWebView2_28 = self.webview.cast().ok()?;
        // SAFETY: live WebView.
        unsafe { wv.Find() }.ok().map(Find)
    }

    fn profile(&self) -> Option<ICoreWebView2Profile> {
        let wv: ICoreWebView2_13 = self.webview.cast().ok()?;
        // SAFETY: live WebView.
        unsafe { wv.Profile() }.ok()
    }

    /// Sets the profile's default download folder.
    pub fn set_download_folder(&self, dir: &Path) {
        if let Some(p) = self.profile() {
            // SAFETY: live profile; HSTRING outlives the call.
            let _ = unsafe { p.SetDefaultDownloadFolderPath(&HSTRING::from(dir.as_os_str())) };
        }
    }

    /// Clears cookies/cache/site data of this WebView's profile, everything
    /// or since `since_ms` (Unix ms). `done(ok)` runs on the UI thread.
    pub fn clear_browsing_data(&self, kinds: ClearKinds, since_ms: Option<i64>, done: impl FnOnce(bool) + 'static) {
        let mut k = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
        if kinds.cookies {
            k.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES.0;
        }
        if kinds.cache {
            k.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0 | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE.0;
        }
        if kinds.site_data {
            k.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE.0;
        }
        let profile = self.profile().and_then(|p| p.cast::<ICoreWebView2Profile2>().ok());
        let (Some(profile), true) = (profile, k.0 != 0) else {
            done(k.0 == 0);
            return;
        };
        let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
        let d2 = done.clone();
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
            if let Some(f) = d2.borrow_mut().take() {
                f(result.is_ok());
            }
            Ok(())
        }));
        // SAFETY: live profile; the handler is kept alive by WebView2.
        let started = unsafe {
            match since_ms {
                Some(ms) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    profile.ClearBrowsingDataInTimeRange(k, ms as f64 / 1000.0, now + 60.0, &handler)
                }
                None => profile.ClearBrowsingData(k, &handler),
            }
        };
        if started.is_err() {
            if let Some(f) = done.borrow_mut().take() {
                f(false);
            }
        }
    }

    /// `DownloadStarting`. The default download UI is suppressed; the
    /// callback decides the path (or cancels).
    pub fn on_download_starting(&self, mut f: impl FnMut(DownloadStart) + 'static) {
        let Ok(wv4) = self.webview.cast::<ICoreWebView2_4>() else { return };
        let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            // SAFETY: live event args; the deferral keeps them valid until
            // `DownloadStart` is dropped.
            unsafe {
                let (Ok(op), Ok(deferral)) = (args.DownloadOperation(), args.GetDeferral()) else { return Ok(()) };
                let _ = args.SetHandled(true);
                let uri = get_string(|p| op.Uri(p));
                let suggested_path = PathBuf::from(get_string(|p| args.ResultFilePath(p)));
                let operation = Download(op);
                let total = operation.total();
                f(DownloadStart { args, deferral, operation, uri, suggested_path, total });
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { wv4.add_DownloadStarting(&handler, &mut token) };
    }

    // ---------------- events ----------------

    /// `WebMessageReceived` from the top-level document: `(source, json)`.
    ///
    /// The message text may contain a password (vault and login messages),
    /// so both WebView2's buffer and our copy are wiped after the callback.
    pub fn on_message(&self, mut f: impl FnMut(String, &str) + 'static) {
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                // SAFETY: live event args during the callback.
                let source = get_string(|p| unsafe { args.Source(p) });
                let mut p = PWSTR::null();
                // SAFETY: as above; `take_wiped` frees the returned buffer.
                let json = match unsafe { args.WebMessageAsJson(&mut p) } {
                    Ok(()) => take_wiped(p),
                    Err(_) => Zeroizing::new(String::new()),
                };
                f(source, &json);
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
    /// `NavigationCompleted(success, COREWEBVIEW2_WEB_ERROR_STATUS)`.
    pub fn on_navigation_completed(&self, mut f: impl FnMut(bool, i32) + 'static) {
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            let (ok, status) = match args {
                Some(a) => {
                    let mut st = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                    // SAFETY: live event args during the callback.
                    let _ = unsafe { a.WebErrorStatus(&mut st) };
                    (get_bool(|b| unsafe { a.IsSuccess(b) }), st.0)
                }
                None => (false, 0),
            };
            f(ok, status);
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

    /// Intercepts every request of this WebView (all resource contexts, and
    /// documents, shared workers and service workers where the runtime
    /// supports source kinds). Runs on the UI thread for each request, so
    /// `decide` must be cheap.
    pub fn intercept_all(&self, env: &Environment, decide: impl Fn(&ResourceRequest<'_>) -> ResourceAction + 'static) {
        let env = env.clone();
        let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            // SAFETY: live event args during the callback.
            unsafe {
                let Ok(req) = args.Request() else { return Ok(()) };
                let uri = get_string(|p| req.Uri(p));
                let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                let _ = args.ResourceContext(&mut ctx);
                let action = decide(&ResourceRequest { uri: &uri, context: ctx.0 });
                let resp = match action {
                    ResourceAction::Serve(r) => env.response(r.status, r.reason, &r.headers, &r.body),
                    ResourceAction::NotFound => {
                        env.response(404, "Not Found", "Content-Type: text/plain", b"Not found")
                    }
                    ResourceAction::Block => env.response(403, "Blocked", "Content-Length: 0", b""),
                    ResourceAction::Continue { dnt, gpc } => {
                        if dnt || gpc {
                            if let Ok(h) = req.Headers() {
                                if dnt {
                                    let _ = h.SetHeader(&HSTRING::from("DNT"), &HSTRING::from("1"));
                                }
                                if gpc {
                                    let _ = h.SetHeader(&HSTRING::from("Sec-GPC"), &HSTRING::from("1"));
                                }
                            }
                        }
                        None
                    }
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
            let all = HSTRING::from("*");
            let with_kinds = self.webview.cast::<ICoreWebView2_22>().ok().is_some_and(|w22| {
                w22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &all,
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )
                .is_ok()
            });
            if !with_kinds {
                let _ = self.webview.AddWebResourceRequestedFilter(&all, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
            }
            let _ = self.webview.add_WebResourceRequested(&handler, &mut token);
        }
    }

    /// Runs `js` in every new document (all frames) before page scripts.
    pub fn add_document_script(&self, js: &str) {
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(())));
        // SAFETY: live WebView; HSTRING outlives the call.
        let _ = unsafe { self.webview.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(js), &done) };
    }

    /// Runs `js` once in the current top-level document.
    pub fn execute_script(&self, js: &str) {
        let done = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
        // SAFETY: live WebView; HSTRING outlives the call.
        let _ = unsafe { self.webview.ExecuteScript(&HSTRING::from(js), &done) };
    }

    /// `DOMContentLoaded` of the top-level document.
    pub fn on_dom_content_loaded(&self, mut f: impl FnMut() + 'static) {
        let Ok(wv2) = self.webview.cast::<ICoreWebView2_2>() else { return };
        let handler = DOMContentLoadedEventHandler::create(Box::new(move |_, _| {
            f();
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: registering a handler on a live WebView.
        let _ = unsafe { wv2.add_DOMContentLoaded(&handler, &mut token) };
    }

    /// The WebView's current user agent string.
    pub fn user_agent(&self) -> String {
        let Ok(settings) = (unsafe { self.webview.Settings() }) else { return String::new() };
        let Ok(s2) = settings.cast::<ICoreWebView2Settings2>() else { return String::new() };
        // SAFETY: live settings object; out-pointer handled by `get_string`.
        get_string(|p| unsafe { s2.UserAgent(p) })
    }

    /// Overrides the user agent (applies to the next request).
    pub fn set_user_agent(&self, ua: &str) {
        // SAFETY: live WebView settings; HSTRING outlives the call.
        unsafe {
            if let Ok(s2) = self.webview.Settings().and_then(|s| s.cast::<ICoreWebView2Settings2>()) {
                let _ = s2.SetUserAgent(&HSTRING::from(ua));
            }
        }
    }

    /// Light/dark preference for this profile's pages (`prefers-color-scheme`):
    /// `None` = follow Windows.
    pub fn set_color_scheme(&self, dark: Option<bool>) {
        let scheme = match dark {
            None => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
            Some(true) => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            Some(false) => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
        };
        if let Some(p) = self.profile() {
            // SAFETY: live profile.
            let _ = unsafe { p.SetPreferredColorScheme(scheme) };
        }
    }

    /// Sets the profile's tracking prevention: 0 none, 1 basic, 2 balanced, 3 strict.
    pub fn set_tracking_prevention(&self, level: u8) {
        let lvl = match level {
            0 => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
            1 => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC,
            2 => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
            _ => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
        };
        if let Some(p) = self.profile().and_then(|p| p.cast::<ICoreWebView2Profile3>().ok()) {
            // SAFETY: live profile.
            let _ = unsafe { p.SetPreferredTrackingPreventionLevel(lvl) };
        }
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
                    Some(r) => env.response(r.status, r.reason, &r.headers, &r.body),
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
