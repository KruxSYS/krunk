//! The `CefClient` of every view: the browser-process handlers, which find
//! their view through the registry key and raise its events. All of them run
//! on the UI thread except `get_resource_request_handler` (IO thread), which
//! touches only the view's `Send + Sync` network policy.

use super::net::{self, RequestPolicy};
use super::renderer::{AUDIO_TO_BROWSER, DEBUG_TO_BROWSER, PID_TO_BROWSER, POINTER_TO_BROWSER, TO_BROWSER};
use super::view::{
    child_window_info, lookup, AuthChallenge, AuthResponder, DownloadControl, DownloadStart, DownloadStatus,
    DownloadUpdate, KeyPress, LoadingState, Menu, MessageSource, NavigationStart, NewWindow, Role, View, ViewInner,
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
    ViewClient::new(inner.key, inner.role, inner.menu, inner.private, inner.policy.clone())
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
        menu: Menu,
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
            // The browser's own UI has no context menu; pages get the engine's menu plus
            // Krunk's items, and the AI panel the same menu without the page's own items.
            Some(match self.menu {
                Menu::None => NoMenu::new(),
                Menu::Page | Menu::Panel => TabMenu::new(self.key, self.menu),
            })
        }

        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            // A tab's dialogs are Krunk's own prompts (0.12.4): the engine's native dialog
            // windows could end up behind the frame while the page waited for them.
            Some(if self.role == Role::Tab { TabDialogs::new(self.key) } else { NoDialogs::new() })
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
                let args = message.argument_list();
                let have = args.as_ref().map(|a| CefString::from(&a.string(0)).to_string()).unwrap_or_default();
                super::scriptlets::hello(frame, &have);
                // (0.7.1) The client-hints generation the renderer has.
                let hints = args.filter(|a| a.size() > 1).map(|a| u64::try_from(a.int(1)).unwrap_or(0)).unwrap_or(0);
                super::hints::hello(frame, hints);
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
            if name == DEBUG_TO_BROWSER {
                let text = message.argument_list().map(|a| CefString::from(&a.string(0)).to_string()).unwrap_or_default();
                crate::log::info!("renderer: {text}");
                return 1;
            }
            if name == PID_TO_BROWSER {
                let pid = message.argument_list().map(|a| a.int(0)).unwrap_or(0);
                if let Some(view) = lookup(self.key) {
                    view.renderer_pid.set(pid.max(0) as u32);
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
                    super::hints::add_to_extra_info(d);
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

        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status_code: c_int) {
            if !is_main(&frame) {
                return;
            }
            if let Some(view) = lookup(self.key) {
                view.ev.load_end.with(|f| f(http_status_code));
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

        fn on_media_access_change(&self, _browser: Option<&mut Browser>, has_video_access: c_int, has_audio_access: c_int) {
            if let Some(view) = lookup(self.key) {
                view.ev.media_use.with(|f| f(has_video_access != 0, has_audio_access != 0));
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

// Permissions (Krunk 0.5.0: local network access; 0.6.0: every prompt type and media
// access). Every prompt goes to the host, which asks the user or answers from its store and
// defaults; an unknown type is denied there rather than left hanging.
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
            let (Some(view), Some(callback)) = (lookup(self.key), callback) else { return 0 };
            let origin = text(requesting_origin);
            view.permission_prompts.borrow_mut().insert(prompt_id, (callback.clone(), origin.clone()));
            view.ev.permission.with(|f| f(prompt_id, origin.clone(), requested_permissions));
            1
        }

        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> c_int {
            let (Some(view), Some(callback)) = (lookup(self.key), callback) else { return 0 };
            let id = view.media_next.get() + 1;
            view.media_next.set(id);
            let origin = text(requesting_origin);
            view.media_prompts.borrow_mut().insert(id, (callback.clone(), requested_permissions));
            view.ev.media_access.with(|f| f(id, origin.clone(), requested_permissions));
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

        /// IO thread (0.8.0): a server asks for HTTP authentication. The challenge goes to the
        /// UI thread, where the tab's dialog answers it; a proxy's is refused as before (the
        /// engine then cancels the request).
        fn auth_credentials(
            &self,
            _browser: Option<&mut Browser>,
            origin_url: Option<&CefString>,
            is_proxy: c_int,
            _host: Option<&CefString>,
            _port: c_int,
            realm: Option<&CefString>,
            scheme: Option<&CefString>,
            callback: Option<&mut AuthCallback>,
        ) -> c_int {
            let Some(cb) = callback else { return 0 };
            if is_proxy != 0 {
                return 0;
            }
            let challenge = AuthChallenge {
                origin_url: text(origin_url),
                realm: text(realm),
                scheme: text(scheme),
            };
            let responder = AuthResponder::new(cb.clone());
            let key = self.key;
            super::post_ui_from_any(move || {
                // Without a view or a handler the responder is dropped, which cancels.
                if let Some(view) = lookup(key) {
                    view.ev.auth.with(move |f| f(challenge, responder));
                }
            });
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
            // Views without a policy (an extension's own views): only the relay of an
            // extension's requests to its granted hosts (0.17.0).
            match self.policy.as_ref() {
                Some(p) => Some(net::handler_for(p, request_initiator)),
                None => super::relay::handler_for(request_initiator),
            }
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
                view.hang.borrow_mut().take();
                view.ev.unresponsive.with(|f| f(false));
                view.ev.crashed.with(|f| f(reason));
            }
        }

        // 0.11.0: a page that stopped handling input for about 15 s. The host asks the user
        // (Wait / Close page) and answers through the kept callback; without a host answer
        // the engine just keeps waiting.
        fn on_render_process_unresponsive(
            &self,
            _browser: Option<&mut Browser>,
            callback: Option<&mut UnresponsiveProcessCallback>,
        ) -> c_int {
            let (Some(view), Some(cb)) = (lookup(self.key), callback) else { return 0 };
            *view.hang.borrow_mut() = Some(cb.clone());
            view.ev.unresponsive.with(|f| f(true));
            1
        }

        fn on_render_process_responsive(&self, _browser: Option<&mut Browser>) {
            if let Some(view) = lookup(self.key) {
                view.hang.borrow_mut().take();
                view.ev.unresponsive.with(|f| f(false));
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
/// "Search <engine> for …" with selected text (0.11.0).
const MENU_SEARCH: c_int = 26502;
/// The image items (0.12.2), in `ImageAction::ALL`'s order from this id on.
const MENU_IMAGE_FIRST: c_int = 26503;
/// A link, or selected text that is an address (0.15.2): open it in a new tab, window or
/// private tab; copy a link's address.
const MENU_SEL_TAB: c_int = 26520;
const MENU_SEL_WINDOW: c_int = 26521;
const MENU_SEL_PRIVATE: c_int = 26522;
const MENU_SEL_COPY: c_int = 26523;
/// CEF's own ids: Copy, and "View page source" (which opened the system text editor).
const MENU_ID_COPY: c_int = lb_core::page_menu::id::COPY;
const MENU_ID_VIEW_SOURCE: c_int = lb_core::page_menu::id::VIEW_SOURCE;

/// The address of the image the context menu is for, if it is for one with contents.
fn image_url(p: &ContextMenuParams) -> Option<String> {
    if p.media_type() != ContextMenuMediaType::IMAGE || p.has_image_contents() == 0 {
        return None;
    }
    let url = CefString::from(&p.source_url()).to_string();
    (!url.is_empty()).then_some(url)
}

/// The image items of a view's menu for the image at `url`, with their places in
/// `ImageAction::ALL` (their command ids count from `MENU_IMAGE_FIRST` in that order). The AI
/// panel has items only for its own pictures, and only those that can work for them.
fn image_items(menu: Menu, url: &str) -> Vec<(usize, lb_core::image_menu::ImageAction)> {
    let panel = menu == Menu::Panel;
    if panel && !lb_core::image_menu::panel_image(url) {
        return Vec::new();
    }
    let all = lb_core::image_menu::ImageAction::ALL;
    all.into_iter().enumerate().filter(|(_, a)| !panel || a.in_panel()).collect()
}

/// The AI panel's menu: of the engine's own items only the editing ones stay (Back, Forward,
/// Print and "View page source" belong to a page). Their separators are tidied at the end.
fn keep_panel_items(m: &MenuModel) {
    for i in (0..m.count()).rev() {
        if m.type_at(i) != MenuItemType::SEPARATOR && !lb_core::page_menu::panel_keeps(m.command_id_at(i)) {
            m.remove_at(i);
        }
    }
}

/// No separator first, last or after another one.
fn tidy_separators(m: &MenuModel) {
    let separators: Vec<bool> = (0..m.count()).map(|i| m.type_at(i) == MenuItemType::SEPARATOR).collect();
    for i in lb_core::page_menu::stray_separators(&separators) {
        m.remove_at(i);
    }
}

/// What the open items are for: the link under the pointer (`true`), else a selected address.
fn open_target(p: &ContextMenuParams) -> Option<(String, bool)> {
    let link = CefString::from(&p.link_url()).to_string();
    if link.starts_with("http://") || link.starts_with("https://") {
        return Some((link, true));
    }
    if !link.is_empty() {
        return None;
    }
    let selection = CefString::from(&p.selection_text()).to_string();
    lb_core::urlclean::selection_url(&selection).map(|u| (u, false))
}

/// Hands a frame's HTML (`Frame::source`) to `done` once.
pub(super) fn source_visitor(done: Box<dyn FnOnce(Option<String>)>) -> CefStringVisitor {
    SourceVisitor::new(std::rc::Rc::new(std::cell::RefCell::new(Some(done))))
}

type SourceDone = std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnOnce(Option<String>)>>>>;

wrap_string_visitor! {
    struct SourceVisitor {
        done: SourceDone,
    }
    impl CefStringVisitor {
        fn visit(&self, string: Option<&CefString>) {
            if let Some(done) = self.done.borrow_mut().take() {
                done(string.map(|s| s.to_string()));
            }
        }
    }
}

// The page menu, for a tab (`Menu::Page`) and for the AI panel (`Menu::Panel`, 0.16.0): one
// menu and one dispatch. The panel's differs in three places, all in `on_before_context_menu`:
// the engine's page items are taken out first, the image items are the panel's, and there is
// no "Inspect". An empty model shows no menu. The commands go to the view's events, which the
// host connects to a tab or to the panel's window.
wrap_context_menu_handler! {
    struct TabMenu {
        key: u64,
        menu: Menu,
    }
    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            let Some(m) = model else { return };
            let panel = self.menu == Menu::Panel;
            if panel {
                keep_panel_items(m);
            }
            let image = params.as_ref().and_then(|p| image_url(p));
            // Selected text (never a password field's: the engine gives none): "Search … for"
            // after Copy, as in Chrome (0.11.0).
            let target = params.as_ref().and_then(|p| open_target(p));
            let selection = params.map(|p| CefString::from(&p.selection_text()).to_string()).unwrap_or_default();
            // A link or a selected address: open it, first in the menu (0.15.2).
            if let Some((url, link)) = target {
                let items: &[(c_int, String)] = if link {
                    &[
                        (MENU_SEL_TAB, "Open link in new tab".to_string()),
                        (MENU_SEL_WINDOW, "Open link in new window".to_string()),
                        (MENU_SEL_PRIVATE, "Open link in private tab".to_string()),
                        (MENU_SEL_COPY, "Copy link address".to_string()),
                    ]
                } else {
                    let shown = lb_core::urlclean::selection_url_label(&url);
                    &[
                        (MENU_SEL_TAB, format!("Open “{shown}” in new tab")),
                        (MENU_SEL_WINDOW, "Open in new window".to_string()),
                        (MENU_SEL_PRIVATE, "Open in private tab".to_string()),
                    ]
                };
                for (i, (id, label)) in items.iter().enumerate() {
                    m.insert_item_at(i, *id, Some(&CefString::from(label.as_str())));
                }
                m.insert_separator_at(items.len());
            }
            // An image: Chrome's four image items first (0.12.2); in the AI panel those that
            // work for its pictures.
            let items = image.map(|url| image_items(self.menu, &url)).unwrap_or_default();
            if !items.is_empty() {
                for (at, (i, a)) in items.iter().enumerate() {
                    m.insert_item_at(at, MENU_IMAGE_FIRST + *i as c_int, Some(&CefString::from(a.label())));
                }
                if m.count() > items.len() {
                    m.insert_separator_at(items.len());
                }
            }
            let label = lookup(self.key).and_then(|v| v.ev.search_label.with(|f| f(&selection)).flatten());
            if let Some(label) = label {
                let at = m.index_of(MENU_ID_COPY);
                if at >= 0 {
                    m.insert_item_at((at + 1) as usize, MENU_SEARCH, Some(&CefString::from(label.as_str())));
                } else {
                    m.insert_item_at(0, MENU_SEARCH, Some(&CefString::from(label.as_str())));
                    m.insert_separator_at(1);
                }
            }
            if panel {
                // No "Inspect" (only tab views are inspected). Where nothing applies (an
                // empty part of the panel), nothing is left and no menu shows.
                tidy_separators(m);
                return;
            }
            if m.count() > 0 {
                m.add_separator();
            }
            m.add_item(MENU_INSPECT, Some(&CefString::from("Inspect")));
        }

        fn on_context_menu_command(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            command_id: c_int,
            _event_flags: EventFlags,
        ) -> c_int {
            if command_id == MENU_ID_VIEW_SOURCE {
                // Krunk's own viewer, not the engine's temp file in the system text editor.
                let url = _browser.and_then(|b| b.main_frame()).map(|f| CefString::from(&f.url()).to_string());
                if let (Some(url), Some(view)) = (url, lookup(self.key)) {
                    view.ev.view_source.with(|f| f(url));
                }
                return 1;
            }
            let image_items = lb_core::image_menu::ImageAction::ALL;
            if (MENU_IMAGE_FIRST..MENU_IMAGE_FIRST + image_items.len() as c_int).contains(&command_id) {
                let action = image_items[(command_id - MENU_IMAGE_FIRST) as usize];
                let url = params.as_ref().and_then(|p| image_url(p));
                if let (Some(url), Some(view)) = (url, lookup(self.key)) {
                    view.ev.image_menu.with(|f| f(action, url));
                }
                return 1;
            }
            if (MENU_SEL_TAB..=MENU_SEL_COPY).contains(&command_id) {
                let how = match command_id {
                    MENU_SEL_TAB => super::view::OpenIn::Tab,
                    MENU_SEL_WINDOW => super::view::OpenIn::Window,
                    MENU_SEL_PRIVATE => super::view::OpenIn::PrivateTab,
                    _ => super::view::OpenIn::CopyAddress,
                };
                let target = params.as_ref().and_then(|p| open_target(p));
                if let (Some((url, _)), Some(view)) = (target, lookup(self.key)) {
                    view.ev.open_selection.with(|f| f(how, url));
                }
                return 1;
            }
            if command_id == MENU_SEARCH {
                let text = params.map(|p| CefString::from(&p.selection_text()).to_string()).unwrap_or_default();
                if let Some(view) = lookup(self.key) {
                    view.ev.search_selection.with(|f| f(text));
                }
                return 1;
            }
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
    struct TabDialogs {
        key: u64,
    }
    impl JsdialogHandler {
        fn on_jsdialog(
            &self,
            _browser: Option<&mut Browser>,
            origin_url: Option<&CefString>,
            dialog_type: JsdialogType,
            message_text: Option<&CefString>,
            default_prompt_text: Option<&CefString>,
            callback: Option<&mut JsdialogCallback>,
            suppress_message: Option<&mut c_int>,
        ) -> c_int {
            use lb_core::js_dialog::JsDialogKind;
            let kind = if dialog_type == JsdialogType::CONFIRM {
                JsDialogKind::Confirm
            } else if dialog_type == JsdialogType::PROMPT {
                JsDialogKind::Prompt
            } else {
                JsDialogKind::Alert
            };
            let Some(cb) = callback else {
                if let Some(s) = suppress_message {
                    *s = 1;
                }
                return 0;
            };
            let request = super::view::JsDialogRequest {
                kind,
                origin_url: text(origin_url),
                message: text(message_text),
                default_text: text(default_prompt_text),
            };
            // Handled either way: the host shows it, or the responder (dropped unanswered)
            // gives the dismissal's answer at once. Never the engine's own dialog.
            let responder = super::view::JsDialogResponder::new(cb.clone(), kind);
            if let Some(view) = lookup(self.key) {
                view.ev.js_dialog.with(|f| f(request, responder));
            }
            1
        }

        fn on_before_unload_dialog(
            &self,
            _browser: Option<&mut Browser>,
            message_text: Option<&CefString>,
            is_reload: c_int,
            callback: Option<&mut JsdialogCallback>,
        ) -> c_int {
            use lb_core::js_dialog::JsDialogKind;
            let Some(cb) = callback else { return 0 };
            let kind = if is_reload != 0 { JsDialogKind::BeforeReload } else { JsDialogKind::BeforeUnload };
            let request = super::view::JsDialogRequest {
                kind,
                origin_url: String::new(),
                message: text(message_text),
                default_text: String::new(),
            };
            let responder = super::view::JsDialogResponder::new(cb.clone(), kind);
            if let Some(view) = lookup(self.key) {
                view.ev.js_dialog.with(|f| f(request, responder));
            }
            1
        }

        fn on_reset_dialog_state(&self, _browser: Option<&mut Browser>) {
            if let Some(view) = lookup(self.key) {
                view.ev.js_dialog_reset.with(|f| f());
            }
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
                original_url: CefString::from(&item.original_url()).to_string(),
                suggested_name: text(suggested_name),
                mime: CefString::from(&item.mime_type()).to_string(),
                total: (total > 0).then_some(total),
                // A download starts with nothing received; one the engine continues from an
                // earlier session has its bytes (and may keep its old start time).
                from_earlier_session: item.received_bytes() > 0 || item.start_time().val < super::launched_at(),
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
        /// The raw message, before CEF parses it. CEF's parser knows `{"id":N,"result":{…}}`
        /// only: the answer to a call in an attached target's session ends in `,"sessionId":"…"}`
        /// and would be dropped (0.19.0: found on Windows; the WASM inspector's calls into a
        /// worker never came back). Those are taken here (1 = handled).
        fn on_dev_tools_message(&self, _browser: Option<&mut Browser>, message: Option<&[u8]>) -> c_int {
            let Some((id, result)) = lb_core::wasm::session_answer(message.unwrap_or_default()) else { return 0 };
            if let Some(view) = lookup(self.key) {
                view.devtools_session_result(id, result);
            }
            1
        }

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

        fn on_dev_tools_event(&self, _browser: Option<&mut Browser>, method: Option<&CefString>, params: Option<&[u8]>) {
            if let Some(view) = lookup(self.key) {
                let method = method.map(|m| m.to_string()).unwrap_or_default();
                view.devtools_event(&method, params.unwrap_or_default());
            }
        }
    }
}
