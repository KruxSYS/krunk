//! The `CefClient` of every view: the browser-process handlers, which find
//! their view through the registry key and raise its events. All of them run
//! on the UI thread except `get_resource_request_handler` (IO thread), which
//! touches only the view's `Send + Sync` network policy.

use super::net::{self, RequestPolicy};
use super::renderer::{AUDIO_TO_BROWSER, POINTER_TO_BROWSER, TO_BROWSER};
use super::view::{
    child_window_info, lookup, DownloadControl, DownloadStart, DownloadStatus, DownloadUpdate, KeyPress, LoadingState,
    MessageSource, NavigationStart, NewWindow, Role, View, ViewInner,
};
use super::{browser_closed, browser_created, post_ui};
use cef::*;
use std::os::raw::c_int;
use std::path::PathBuf;
use std::sync::Arc;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use zeroize::Zeroizing;

/// Client for a view (also used for popups the engine creates for it).
pub(super) fn for_view(inner: &ViewInner) -> Client {
    ViewClient::new(inner.key, inner.role, inner.private, inner.policy.clone())
}

/// Client for the DevTools docked next to view `owner` (a child window of its frame). It has
/// no message handler, so DevTools gets no privileged channel; keys reach Krunk through the
/// owner, except those DevTools uses itself (`lb_core::keys::devtools_keeps`).
pub(super) fn for_tool(owner: u64) -> Client {
    ToolClient::new(owner)
}

fn text(s: Option<&CefString>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}

fn is_main(frame: &Option<&mut Frame>) -> bool {
    frame.as_ref().is_some_and(|f| f.is_main() == 1)
}

wrap_client! {
    struct ViewClient {
        key: u64,
        role: Role,
        private: bool,
        policy: Option<Arc<dyn RequestPolicy>>,
    }
    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(Life::new(self.key, self.role, self.private))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(Load::new(self.key))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(Display::new(self.key))
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(Keys::new(self.key))
        }

        fn focus_handler(&self) -> Option<FocusHandler> {
            Some(Focus::new(self.key))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(Requests::new(self.key, self.policy.clone()))
        }

        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(Permissions::new(self.key))
        }

        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            // The browser's own UI has no context menu; pages get the
            // engine's menu plus "Inspect".
            Some(if self.role == Role::Tab { TabMenu::new(self.key) } else { NoMenu::new() })
        }

        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            (self.role != Role::Tab).then(NoDialogs::new)
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(Downloads::new(self.key))
        }

        fn find_handler(&self) -> Option<FindHandler> {
            Some(Finder::new(self.key))
        }

        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> c_int {
            let (Some(browser), Some(frame), Some(message)) = (browser, frame, message) else { return 0 };
            let name = CefString::from(&message.name()).to_string();
            if name == super::scriptlets::HELLO {
                let have = message.argument_list().map(|a| CefString::from(&a.string(0)).to_string()).unwrap_or_default();
                super::scriptlets::hello(frame, &have);
                return 1;
            }
            if name == POINTER_TO_BROWSER {
                let json = message.argument_list().map(|a| CefString::from(&a.string(0)).to_string()).unwrap_or_default();
                let id = CefString::from(&frame.identifier()).to_string();
                if let Some(view) = lookup(self.key) {
                    view.ev.pointer.with(|f| f(id, json));
                }
                return 1;
            }
            if name == AUDIO_TO_BROWSER {
                let playing = message.argument_list().is_some_and(|a| a.bool(0) == 1);
                if let Some(view) = lookup(self.key) {
                    view.frame_audio(Some(CefString::from(&frame.identifier()).to_string()), playing);
                }
                return 1;
            }
            if name != TO_BROWSER {
                return 0;
            }
            let Some(view) = lookup(self.key) else { return 1 };
            let Some(args) = message.argument_list() else { return 1 };
            let json = Zeroizing::new(CefString::from(&args.string(0)).to_string());
            // The sender is the frame's committed URL; it counts as the main
            // frame only while it is the browser's current main frame.
            let current_main = browser.main_frame().is_some_and(|m| {
                CefString::from(&m.identifier()).to_string() == CefString::from(&frame.identifier()).to_string()
            });
            let source = MessageSource {
                url: CefString::from(&frame.url()).to_string(),
                main_frame: frame.is_main() == 1 && frame.is_valid() == 1 && current_main,
                frame_id: CefString::from(&frame.identifier()).to_string(),
            };
            view.deliver_message(&source, json);
            1
        }
    }
}

wrap_life_span_handler! {
    struct Life {
        key: u64,
        role: Role,
        private: bool,
    }
    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: c_int,
            _popup_features: Option<&PopupFeatures>,
            window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            settings: Option<&mut BrowserSettings>,
            extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut c_int>,
        ) -> c_int {
            let url = text(target_url);
            let foreground = target_disposition != WindowOpenDisposition::NEW_BACKGROUND_TAB;
            let Some(view) = lookup(self.key) else { return 1 };
            if self.role != Role::Tab {
                // Links in the browser's own UI open as tabs.
                view.ev.open_tab.with(|f| f(url, true));
                return 1;
            }
            let request = NewWindow { url, foreground };
            let Some((adopted, parent)) = view.ev.new_window.with(|f| f(&request)).flatten() else { return 1 };
            // The engine creates the new browser (keeping `window.opener`)
            // as a hidden child of the main window, with the new view's client.
            if let Some(w) = window_info {
                *w = child_window_info(parent);
            }
            if let Some(c) = client {
                *c = Some(for_view(&adopted.0));
            }
            if let Some(s) = settings {
                *s = adopted.browser_settings();
            }
            if let Some(e) = extra_info {
                *e = adopted.0.scripts.to_dictionary();
                if let Some(d) = e.as_mut() {
                    super::scriptlets::add_to_extra_info(d);
                }
            }
            adopted.mark_started();
            0
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            browser_created(browser, self.private);
            match lookup(self.key) {
                Some(view) => View::attach(&view, browser),
                // The host dropped the view before the browser arrived.
                None => {
                    if let Some(h) = browser.host() {
                        h.close_browser(1);
                    }
                }
            }
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> c_int {
            // Returning 0 would send WM_CLOSE to the main window (the
            // browser's top-level parent). A tab closes on its own: tear down
            // just its child window, after this callback returns.
            let Some(hwnd) = browser.and_then(|b| b.host()).map(|h| h.window_handle()) else { return 1 };
            let hwnd = HWND(hwnd.0.cast());
            post_ui(move || {
                // SAFETY: a stale handle only makes DestroyWindow fail.
                let _ = unsafe { DestroyWindow(hwnd) };
            });
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            if let Some(view) = lookup(self.key) {
                View::detach(&view);
            }
            browser_closed(browser, self.private);
        }
    }
}

wrap_load_handler! {
    struct Load {
        key: u64,
    }
    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: c_int,
            can_go_back: c_int,
            can_go_forward: c_int,
        ) {
            let Some(view) = lookup(self.key) else { return };
            let loading = is_loading == 1;
            if loading {
                view.load_error.set(None);
            }
            let state = LoadingState { loading, can_go_back: can_go_back == 1, can_go_forward: can_go_forward == 1 };
            view.ev.loading.with(|f| f(state));
            // Errors of this load cycle were reported before it ended.
            if !loading {
                let error = view.load_error.take();
                view.ev.load_finished.with(|f| f(error));
            }
        }

        fn on_load_start(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _transition_type: TransitionType) {
            if !is_main(&frame) {
                return;
            }
            if let Some(view) = lookup(self.key) {
                // A new document: frames of the old one no longer play.
                view.frame_audio(None, false);
                view.ev.load_start.with(|f| f());
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            _error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            if !is_main(&frame) {
                return;
            }
            if let Some(view) = lookup(self.key) {
                view.load_error.set(Some(sys::cef_errorcode_t::from(error_code) as i32));
                // The error page (an interstitial, say) shows for the failed address: the
                // address bar must say so, not keep the previous page's address.
                let url = text(failed_url);
                if !url.is_empty() {
                    view.ev.address.with(|f| f(url));
                }
            }
        }
    }
}

wrap_display_handler! {
    struct Display {
        key: u64,
    }
    impl DisplayHandler {
        fn on_fullscreen_mode_change(&self, _browser: Option<&mut Browser>, fullscreen: c_int) {
            if let Some(view) = lookup(self.key) {
                view.ev.fullscreen.with(|f| f(fullscreen == 1));
            }
        }

        fn on_status_message(&self, _browser: Option<&mut Browser>, value: Option<&CefString>) {
            if let Some(view) = lookup(self.key) {
                view.ev.status.with(|f| f(text(value)));
            }
        }

        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if !is_main(&frame) {
                return;
            }
            if let Some(view) = lookup(self.key) {
                view.ev.address.with(|f| f(text(url)));
            }
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(view) = lookup(self.key) {
                view.ev.title.with(|f| f(text(title)));
            }
        }

        fn on_favicon_urlchange(&self, browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            // Take the engine's list (a clone would copy only its handle).
            let first = icon_urls.and_then(|l| std::mem::replace(l, CefStringList::new()).into_iter().next());
            let Some(url) = first else {
                if let Some(view) = lookup(self.key) {
                    view.ev.favicon.with(|f| f(Vec::new()));
                }
                return;
            };
            let Some(host) = browser.and_then(|b| b.host()) else { return };
            let mut done = FaviconDone::new(self.key);
            // Downscaled to 32 px (16 px at 200 % scaling).
            host.download_image(Some(&CefString::from(url.as_str())), 1, 32, 0, Some(&mut done));
        }

        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            _message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: c_int,
        ) -> c_int {
            // Page console output may contain user data: keep it out of cef.log.
            1
        }
    }
}

wrap_download_image_callback! {
    struct FaviconDone {
        key: u64,
    }
    impl DownloadImageCallback {
        fn on_download_image_finished(&self, _image_url: Option<&CefString>, _http_status_code: c_int, image: Option<&mut Image>) {
            let png = image.and_then(|img| {
                let (mut w, mut h) = (0, 0);
                img.as_png(1.0, 1, Some(&mut w), Some(&mut h))
            });
            let bytes = png.map(|bin| binary_bytes(&bin)).unwrap_or_default();
            if let Some(view) = lookup(self.key) {
                view.ev.favicon.with(|f| f(bytes));
            }
        }
    }
}

/// Plain data from the engine's SSL status, for `lb_core::certinfo`.
pub(super) fn ssl_report(
    url: String,
    secure: bool,
    status: CertStatus,
    version: SslVersion,
    content: SslContentStatus,
    cert: Option<X509Certificate>,
) -> lb_core::certinfo::SslReport {
    let mut chain = Vec::new();
    if let Some(c) = cert {
        if let Some(der) = c.derencoded() {
            chain.push(binary_bytes(&der));
        }
        let mut issuers = Vec::new();
        c.derencoded_issuer_chain(Some(&mut issuers));
        chain.extend(issuers.iter().flatten().map(binary_bytes).take(8));
    }
    lb_core::certinfo::SslReport {
        url,
        secure,
        // Flag enums: the raw bits, whatever combination the engine set.
        cert_status: *status.as_ref() as i32 as u32,
        version: *version.as_ref() as i32,
        content_status: *content.as_ref() as i32 as u32,
        chain,
    }
}

pub(super) fn binary_bytes(bin: &BinaryValue) -> Vec<u8> {
    let (ptr, len) = (bin.raw_data() as *const u8, bin.size());
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    // SAFETY: CEF guarantees `size()` readable bytes at `raw_data()` while
    // `bin` is alive; they are copied out immediately.
    unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
}

wrap_keyboard_handler! {
    struct Keys {
        key: u64,
    }
    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            os_event: Option<&mut sys::MSG>,
            _is_keyboard_shortcut: Option<&mut c_int>,
        ) -> c_int {
            let Some(e) = event else { return 0 };
            if e.type_ != KeyEventType::RAWKEYDOWN {
                return 0;
            }
            // Bit 30 of a WM_KEYDOWN lParam: the key was already down.
            let repeat = os_event.is_some_and(|m| (m.lParam & (1 << 30)) != 0);
            let press = KeyPress {
                vk: e.windows_key_code as u32,
                ctrl: e.modifiers & 4 != 0,
                shift: e.modifiers & 2 != 0,
                alt: e.modifiers & 8 != 0,
                repeat,
            };
            let handled = lookup(self.key).and_then(|v| v.ev.key.with(|f| f(press))).unwrap_or(false);
            c_int::from(handled)
        }
    }
}

wrap_focus_handler! {
    struct Focus {
        key: u64,
    }
    impl FocusHandler {
        fn on_got_focus(&self, _browser: Option<&mut Browser>) {
            if let Some(view) = lookup(self.key) {
                view.ev.got_focus.with(|f| f());
            }
        }

        fn on_set_focus(&self, _browser: Option<&mut Browser>, source: FocusSource) -> c_int {
            let navigation = source.get_raw() == FocusSource::NAVIGATION.get_raw();
            let allow = lookup(self.key).and_then(|v| v.ev.set_focus.with(|f| f(navigation))).unwrap_or(true);
            c_int::from(!allow)
        }
    }
}

// Local network access (Krunk 0.5.0): Chromium asks before a public page may reach a
// private address or this computer. Those prompts go to the host (which asks the user or
// answers from its store); every other permission type stays with the engine's default.
wrap_permission_handler! {
    struct Permissions {
        key: u64,
    }
    impl PermissionHandler {
        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            prompt_id: u64,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> c_int {
            let Some(request) = lb_core::permissions::network_request(requested_permissions) else { return 0 };
            let (Some(view), Some(callback)) = (lookup(self.key), callback) else { return 0 };
            view.permission_prompts.borrow_mut().insert(prompt_id, callback.clone());
            let origin = text(requesting_origin);
            view.ev.permission.with(|f| f(prompt_id, origin.clone(), request.loopback));
            1
        }

        fn on_dismiss_permission_prompt(&self, _browser: Option<&mut Browser>, prompt_id: u64, _result: PermissionRequestResult) {
            if let Some(view) = lookup(self.key) {
                if view.permission_prompts.borrow_mut().remove(&prompt_id).is_some() {
                    view.ev.permission_gone.with(|f| f(prompt_id));
                }
            }
        }
    }
}

wrap_request_handler! {
    struct Requests {
        key: u64,
        policy: Option<Arc<dyn RequestPolicy>>,
    }
    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: c_int,
            is_redirect: c_int,
        ) -> c_int {
            if !is_main(&frame) {
                return 0;
            }
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            if let Some(view) = lookup(self.key) {
                // A new navigation: the last certificate error no longer describes the page.
                *view.cert_error.borrow_mut() = None;
            }
            let nav = NavigationStart { url, redirected: is_redirect == 1 };
            // Fail closed: no view or no policy = no navigation.
            let allow = lookup(self.key).and_then(|v| v.ev.before_browse.with(|f| f(&nav))).unwrap_or(false);
            c_int::from(!allow)
        }

        /// The engine refuses the navigation (the default: no "proceed anyway"); what it
        /// found is kept for the lock popup.
        fn on_certificate_error(
            &self,
            _browser: Option<&mut Browser>,
            _cert_error: Errorcode,
            request_url: Option<&CefString>,
            ssl_info: Option<&mut Sslinfo>,
            _callback: Option<&mut Callback>,
        ) -> c_int {
            if let (Some(view), Some(info)) = (lookup(self.key), ssl_info) {
                let report = ssl_report(
                    text(request_url),
                    false, info.cert_status(), SslVersion::default(), SslContentStatus::default(), info.x509_certificate());
                *view.cert_error.borrow_mut() = Some(report);
            }
            0
        }

        fn on_open_urlfrom_tab(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            target_url: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: c_int,
        ) -> c_int {
            let foreground = target_disposition != WindowOpenDisposition::NEW_BACKGROUND_TAB;
            let new_tab = [
                WindowOpenDisposition::NEW_FOREGROUND_TAB,
                WindowOpenDisposition::NEW_BACKGROUND_TAB,
                WindowOpenDisposition::NEW_WINDOW,
                WindowOpenDisposition::NEW_POPUP,
            ]
            .contains(&target_disposition);
            if !new_tab {
                return 0;
            }
            if let Some(view) = lookup(self.key) {
                view.ev.open_tab.with(|f| f(text(target_url), foreground));
            }
            1
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: c_int,
            _is_download: c_int,
            request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut c_int>,
        ) -> Option<ResourceRequestHandler> {
            // IO thread: only the Send + Sync policy is used here.
            self.policy.as_ref().map(|p| net::handler_for(p, request_initiator))
        }

        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            status: TerminationStatus,
            error_code: c_int,
            _error_string: Option<&CefString>,
        ) {
            let reason = format!("status {}, code {error_code}", sys::cef_termination_status_t::from(status) as i32);
            if let Some(view) = lookup(self.key) {
                view.ev.crashed.with(|f| f(reason));
            }
        }
    }
}

wrap_context_menu_handler! {
    struct NoMenu;
    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(m) = model {
                m.clear();
            }
        }
    }
}

/// "Inspect" in the page context menu (CEF's `MENU_ID_USER_FIRST` + 1).
const MENU_INSPECT: c_int = 26501;

wrap_context_menu_handler! {
    struct TabMenu {
        key: u64,
    }
    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(m) = model {
                if m.count() > 0 {
                    m.add_separator();
                }
                m.add_item(MENU_INSPECT, Some(&CefString::from("Inspect")));
            }
        }

        fn on_context_menu_command(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            command_id: c_int,
            _event_flags: EventFlags,
        ) -> c_int {
            if command_id != MENU_INSPECT {
                return 0;
            }
            let at = params.map(|p| (p.xcoord(), p.ycoord()));
            // The host docks DevTools next to the tab.
            if let Some(view) = lookup(self.key) {
                view.ev.inspect.with(|f| f(at));
            }
            1
        }
    }
}

wrap_jsdialog_handler! {
    struct NoDialogs;
    impl JsdialogHandler {
        fn on_jsdialog(
            &self,
            _browser: Option<&mut Browser>,
            _origin_url: Option<&CefString>,
            _dialog_type: JsdialogType,
            _message_text: Option<&CefString>,
            _default_prompt_text: Option<&CefString>,
            _callback: Option<&mut JsdialogCallback>,
            suppress_message: Option<&mut c_int>,
        ) -> c_int {
            if let Some(s) = suppress_message {
                *s = 1;
            }
            0
        }
    }
}

wrap_download_handler! {
    struct Downloads {
        key: u64,
    }
    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> c_int {
            let (Some(item), Some(callback)) = (download_item, callback) else { return 0 };
            let Some(view) = lookup(self.key) else { return 0 };
            let total = item.total_bytes();
            let start = DownloadStart {
                id: item.id(),
                url: CefString::from(&item.url()).to_string(),
                suggested_name: text(suggested_name),
                total: (total > 0).then_some(total),
                callback: callback.clone(),
            };
            // Handled: the host continues (or drops = cancels) the callback.
            c_int::from(view.ev.download_start.with(|f| f(start)).is_some())
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let (Some(item), Some(callback)) = (download_item, callback) else { return };
            let Some(view) = lookup(self.key) else { return };
            let status = if item.is_complete() == 1 {
                DownloadStatus::Completed
            } else if item.is_canceled() == 1 {
                DownloadStatus::Cancelled
            } else if item.is_interrupted() == 1 {
                DownloadStatus::Interrupted(sys::cef_download_interrupt_reason_t::from(item.interrupt_reason()) as i32)
            } else if item.is_paused() == 1 {
                DownloadStatus::Paused
            } else {
                DownloadStatus::InProgress
            };
            let total = item.total_bytes();
            let update = DownloadUpdate {
                id: item.id(),
                received: item.received_bytes(),
                total: (total > 0).then_some(total),
                path: PathBuf::from(CefString::from(&item.full_path()).to_string()),
                status,
                control: DownloadControl(callback.clone()),
            };
            view.ev.download_update.with(|f| f(update));
        }
    }
}

wrap_find_handler! {
    struct Finder {
        key: u64,
    }
    impl FindHandler {
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            _identifier: c_int,
            count: c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: c_int,
            _final_update: c_int,
        ) {
            if let Some(view) = lookup(self.key) {
                view.ev.find.with(|f| f(active_match_ordinal.max(0), count.max(0)));
            }
        }
    }
}

wrap_client! {
    struct ToolClient {
        owner: u64,
    }
    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(ToolLife::new(self.owner))
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(ToolKeys::new(self.owner))
        }
    }
}

wrap_keyboard_handler! {
    struct ToolKeys {
        owner: u64,
    }
    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            os_event: Option<&mut sys::MSG>,
            _is_keyboard_shortcut: Option<&mut c_int>,
        ) -> c_int {
            let Some(e) = event else { return 0 };
            if e.type_ != KeyEventType::RAWKEYDOWN {
                return 0;
            }
            let press = KeyPress {
                vk: e.windows_key_code as u32,
                ctrl: e.modifiers & 4 != 0,
                shift: e.modifiers & 2 != 0,
                alt: e.modifiers & 8 != 0,
                repeat: os_event.is_some_and(|m| (m.lParam & (1 << 30)) != 0),
            };
            if lb_core::keys::devtools_keeps(press.vk, press.ctrl, press.shift, press.alt) {
                return 0;
            }
            let handled = lookup(self.owner).and_then(|v| v.ev.key.with(|f| f(press))).unwrap_or(false);
            c_int::from(handled)
        }
    }
}

wrap_life_span_handler! {
    struct ToolLife {
        owner: u64,
    }
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(b) = browser else { return };
            browser_created(b, false);
            match lookup(self.owner) {
                Some(view) => View::devtools_attach(&view, b),
                // The tab went away meanwhile.
                None => {
                    if let Some(h) = b.host() {
                        h.close_browser(1);
                    }
                }
            }
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> c_int {
            // As for tabs: returning 0 would send WM_CLOSE to the frame (the docked DevTools'
            // top-level parent) and close the whole window. Only its own child window goes.
            let Some(hwnd) = browser.and_then(|b| b.host()).map(|h| h.window_handle()) else { return 1 };
            let hwnd = HWND(hwnd.0.cast());
            post_ui(move || {
                // SAFETY: a stale handle only makes DestroyWindow fail.
                let _ = unsafe { DestroyWindow(hwnd) };
            });
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                browser_closed(b, false);
                if let Some(view) = lookup(self.owner) {
                    View::devtools_detach(&view, b);
                }
            }
        }
    }
}

/// Observer of this view's DevTools protocol results (UI thread).
pub(super) fn devtools_observer(key: u64) -> DevToolsMessageObserver {
    ResultLogger::new(key)
}

wrap_dev_tools_message_observer! {
    struct ResultLogger {
        key: u64,
    }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: c_int,
            success: c_int,
            result: Option<&[u8]>,
        ) {
            if let Some(view) = lookup(self.key) {
                view.devtools_result(message_id, success == 1, result.unwrap_or_default());
            }
        }
    }
}
