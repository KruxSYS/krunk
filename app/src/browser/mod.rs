//! Browser host: owns the window, the shell/popup WebViews and one
//! controller per live tab, and turns `TabModel` effects into WebView calls.
//! All policy (tabs, shortcuts, origins, navigation, layout) lives in core.

use crate::ffi::dialogs;
use crate::ffi::loader::Loader;
use crate::ffi::webview::{
    Controller, Download, Environment, EnvironmentConfig, Find, InternalResponse, KeyPress, NewWindow,
};
use crate::ffi::window::{ScreenRect, Window, WindowEvents};
use crate::log::{info, warning};
use crate::paths::DataDirs;
use lb_core::assets;
use lb_core::downloads::Downloads;
use lb_core::keys::{self, Command};
use lb_core::layout;
use lb_core::pages::{self, Page};
use lb_core::protocol::{
    self, Channel, ContentMessage, Envelope, Event, NavState, PopupContent, PopupKind, PopupRect, Request, Sender,
    Source, ZoomAction,
};
use lb_core::settings::{Settings, Startup, TrackingPrevention};
use lb_core::storage::{now_ms, Store};
use lb_core::tabs::{Effect, Position, Session, TabId, TabModel};
use lb_core::urlclean;
use std::borrow::Cow;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

const SHELL_URL: &str = "https://shell.lightbrowser.invalid/shell.html";
const POPUP_URL: &str = "https://shell.lightbrowser.invalid/popup.html";
const SHELL_FILTER: &str = "https://shell.lightbrowser.invalid/*";
const SESSION_KEY: &str = "session";
const DOWNLOADS_KEY: &str = "downloads";
/// Timer that saves the session shortly after the last tab change.
const SESSION_TIMER: usize = 1;
const SESSION_DEBOUNCE_MS: u32 = 2000;

mod blocking;
mod downloads;
mod find;
mod jobs;
mod library;
mod power;
mod settings_host;

pub use power::PowerOptions;
use power::{MEMREPORT_TIMER, POWER_TIMER};
mod vault;

use settings_host::EXIT_TIMER;

use jobs::{Jobs, APP_JOB};
use vault::{VaultHost, CLIPBOARD_TIMER, VAULT_TIMER};

use blocking::{Deferred, FilterState, TabNet, APP_DEFERRED, APP_FILTERS_READY, SHIELD_TIMER, UPDATE_TIMER};
/// Background behind the UI WebViews (matches --bg in common.css).
const UI_BACKGROUND: u32 = 0xFFF3_F4F6;

/// Live state of a tab's WebView.
struct TabView {
    controller: Controller,
    /// Set right before the host itself navigates, so the navigation
    /// policy can tell host navigations from page-initiated ones.
    host_navigation: Rc<Cell<bool>>,
    can_go_back: bool,
    can_go_forward: bool,
    /// Find-in-page session and its current term.
    find: Option<Find>,
    find_query: String,
    /// Shield counter, cosmetic data and HTTPS-only state of the page.
    net: Rc<TabNet>,
    /// Sleep stage and when it was entered.
    stage: Cell<lb_core::memory::Stage>,
    stage_since: Cell<i64>,
    /// The page is playing audio (never put to sleep).
    audio: Rc<Cell<bool>>,
    /// Downloads in progress started by this tab (never put to sleep).
    downloads: Rc<Cell<u32>>,
}

/// The popup WebView in its owned popup window (created on first use).
struct Popup {
    window: Window,
    controller: Option<Controller>,
    ready: bool,
    /// Content shown now, or waiting for the popup document to be ready.
    content: Option<PopupContent>,
}

pub struct Browser {
    me: Weak<Browser>,
    window: OnceCell<Window>,
    env: OnceCell<Environment>,
    store: RefCell<Store>,
    settings: RefCell<Settings>,
    runtime_version: String,
    model: RefCell<TabModel>,
    views: RefCell<HashMap<TabId, TabView>>,
    shell: RefCell<Option<Controller>>,
    popup: RefCell<Option<Popup>>,
    new_windows: RefCell<HashMap<TabId, NewWindow>>,
    downloads: RefCell<Downloads>,
    download_ops: RefCell<HashMap<u32, Download>>,
    /// Button rect of the open bookmarks-bar folder dropdown.
    folder_anchor: Cell<Option<PopupRect>>,
    dirs: DataDirs,
    filters: Rc<FilterState>,
    /// A shield counter update is scheduled.
    shield_pending: Cell<bool>,
    deferred: RefCell<Vec<Deferred>>,
    power: PowerOptions,
    vaults: VaultHost,
    jobs: Jobs,
    /// Settings the browser was started with (for "restart required").
    running_settings: Settings,
    /// The runtime's own user agent (restored when the custom one is cleared).
    default_ua: RefCell<Option<String>>,
    restart_after_exit: Cell<bool>,
    exit_clearing: Cell<bool>,
    exit_cleared: Cell<bool>,
    closing: Cell<bool>,
}

impl Browser {
    /// Opens the window and starts creating the WebView2 environment.
    pub fn start(
        loader: &Loader,
        dirs: &DataDirs,
        store: Store,
        settings: Settings,
        runtime_version: String,
        power: PowerOptions,
    ) -> Result<Rc<Browser>, String> {
        let running_settings = settings.clone();
        let browser = Rc::new_cyclic(|me| Browser {
            me: me.clone(),
            window: OnceCell::new(),
            env: OnceCell::new(),
            store: RefCell::new(store),
            settings: RefCell::new(settings),
            runtime_version,
            model: RefCell::new(TabModel::new()),
            views: RefCell::new(HashMap::new()),
            shell: RefCell::new(None),
            popup: RefCell::new(None),
            new_windows: RefCell::new(HashMap::new()),
            downloads: RefCell::new(Downloads::new()),
            download_ops: RefCell::new(HashMap::new()),
            folder_anchor: Cell::new(None),
            dirs: dirs.clone(),
            filters: Rc::new(FilterState::new()),
            shield_pending: Cell::new(false),
            deferred: RefCell::new(Vec::new()),
            power,
            vaults: VaultHost::default(),
            jobs: Jobs::new(),
            running_settings,
            default_ua: RefCell::new(None),
            restart_after_exit: Cell::new(false),
            exit_clearing: Cell::new(false),
            exit_cleared: Cell::new(false),
            closing: Cell::new(false),
        });
        let window = Window::create_main("LightBrowser", browser.clone())?;
        let _ = browser.window.set(window);
        browser.apply_frame_theme();
        browser.init_filters();
        browser.init_vault();

        let config = {
            let s = browser.settings.borrow();
            EnvironmentConfig {
                browser_arguments: s.browser_arguments(),
                tracking_prevention: s.tracking_prevention != TrackingPrevention::None,
            }
        };
        info!("browser arguments: {}", config.browser_arguments);
        let weak = browser.me.clone();
        Environment::create(loader, &dirs.profile(), &config, move |result| {
            let Some(b) = weak.upgrade() else { return };
            match result {
                Ok(env) => b.environment_ready(env),
                Err(e) => b.fatal(&e),
            }
        })?;
        Ok(browser)
    }

    fn window(&self) -> Option<Window> {
        self.window.get().copied()
    }

    fn fatal(&self, error: &str) {
        warning!("fatal: {error}");
        dialogs::runtime_missing(&format!("Details: {error}\n"));
        if let Some(w) = self.window() {
            w.destroy();
        }
    }

    // ------------------------------------------------------------ startup

    fn environment_ready(&self, env: Environment) {
        info!("WebView2 environment created (runtime {})", self.runtime_version);
        let _ = self.env.set(env.clone());
        let Some(window) = self.window() else { return };
        let weak = self.me.clone();
        let started = env.create_controller(window.hwnd(), false, move |result| {
            let Some(b) = weak.upgrade() else { return };
            match result {
                Ok(c) => b.shell_ready(c),
                Err(e) => b.fatal(&e),
            }
        });
        if let Err(e) = started {
            self.fatal(&e);
        }
    }

    fn shell_ready(&self, shell: Controller) {
        let Some(env) = self.env.get() else { return };
        shell.lock_down_ui();
        shell.set_background(UI_BACKGROUND);
        shell.serve(env, SHELL_FILTER, self.server());
        self.apply_download_folder(&shell);
        shell.set_color_scheme(match self.settings.borrow().theme {
            lb_core::settings::Theme::System => None,
            lb_core::settings::Theme::Light => Some(false),
            lb_core::settings::Theme::Dark => Some(true),
        });
        shell.set_tracking_prevention(match self.settings.borrow().tracking_prevention {
            TrackingPrevention::None => 0,
            TrackingPrevention::Basic => 1,
            TrackingPrevention::Balanced => 2,
            TrackingPrevention::Strict => 3,
        });
        shell.on_navigation_starting(|nav| assets::allow_navigation(Channel::Shell, "", &nav.uri, true));
        let weak = self.me.clone();
        shell.on_new_window(move |nw| {
            // Links in the shell never open windows; open them as tabs instead.
            if let Some(b) = weak.upgrade() {
                b.open_url(&nw.uri, true, false);
            }
        });
        let weak = self.me.clone();
        shell.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                b.on_ui_message(Channel::Shell, &source, json);
            }
        });
        self.hook_common(&shell);
        let _ = shell.navigate(SHELL_URL);
        shell.set_visible(true);
        *self.shell.borrow_mut() = Some(shell);
        self.layout();
        self.restore_or_open_start();
        self.start_power_timer();
        self.mem_report_line("start");
    }

    /// Accelerators and focus handling shared by every controller.
    fn hook_common(&self, c: &Controller) {
        let weak = self.me.clone();
        c.on_accelerator(move |key| weak.upgrade().is_some_and(|b| b.on_key(key)));
        let weak = self.me.clone();
        c.on_got_focus(move || {
            if let Some(b) = weak.upgrade() {
                b.hide_popup();
            }
        });
    }

    fn restore_or_open_start(&self) {
        if let Ok(Some(json)) = self.store.borrow().kv_get(DOWNLOADS_KEY) {
            *self.downloads.borrow_mut() = Downloads::from_json(&json);
        }
        let (startup, homepage) = {
            let s = self.settings.borrow();
            (s.startup, s.homepage.clone())
        };
        let saved: Option<Session> =
            self.store.borrow().kv_get(SESSION_KEY).ok().flatten().and_then(|j| serde_json::from_str(&j).ok());
        let effects = {
            let mut m = self.model.borrow_mut();
            match (startup, saved) {
                (Startup::RestoreSession, Some(session)) if !session.tabs.is_empty() => m.restore(&session, now_ms()),
                (Startup::Homepage, _) if !homepage.is_empty() => {
                    m.open(&homepage, false, Position::End, true, true, now_ms()).1
                }
                _ => m.open("", false, Position::End, true, true, now_ms()).1,
            }
        };
        // Restored tabs show their favicons from the local cache (no network).
        {
            let store = self.store.borrow();
            let mut m = self.model.borrow_mut();
            let hosts: Vec<(TabId, String)> =
                m.tabs().iter().filter_map(|t| urlclean::host_of(&t.url).map(|h| (t.id, h))).collect();
            for (id, host) in hosts {
                if let Ok(Some(png)) = store.favicon(&host) {
                    m.set_favicon(id, assets::png_data_url(&png));
                }
            }
        }
        self.apply(effects);
    }

    // ------------------------------------------------------------ effects

    fn apply(&self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::CreateView(id) => self.create_view(id),
                Effect::DestroyView(id) => self.destroy_view(id),
                Effect::Activate { from, to } => self.activate_view(from, to),
            }
        }
        self.send_tabs();
        self.send_nav();
        self.save_session();
    }

    fn create_view(&self, id: TabId) {
        let (Some(env), Some(window)) = (self.env.get(), self.window()) else { return };
        let Some(private) = self.model.borrow().get(id).map(|t| t.private) else { return };
        let weak = self.me.clone();
        let started = env.create_controller(window.hwnd(), private, move |result| {
            let Some(b) = weak.upgrade() else { return };
            match result {
                Ok(c) => b.view_ready(id, c),
                Err(e) => warning!("tab {id}: {e}"),
            }
        });
        if let Err(e) = started {
            warning!("tab {id}: {e}");
        }
    }

    fn view_ready(&self, id: TabId, c: Controller) {
        let (url, active) = {
            let m = self.model.borrow();
            match m.get(id) {
                Some(t) if t.is_live() => (t.url.clone(), m.active_id() == Some(id)),
                // Closed or unloaded while the controller was being created.
                _ => {
                    c.close();
                    return;
                }
            }
        };
        let host_navigation = Rc::new(Cell::new(false));
        let net = Rc::new(TabNet::default());
        let audio = Rc::new(Cell::new(false));
        let audio2 = audio.clone();
        c.on_audio_changed(move |playing| audio2.set(playing));
        self.hook_tab(id, &c, host_navigation.clone(), net.clone());
        self.hook_blocking(id, &c, net.clone());
        self.apply_user_agent(&c);
        if c.is_private() {
            c.set_color_scheme(match self.settings.borrow().theme {
                lb_core::settings::Theme::System => None,
                lb_core::settings::Theme::Light => Some(false),
                lb_core::settings::Theme::Dark => Some(true),
            });
        }
        self.apply_download_folder(&c);
        c.set_zoom(self.settings.borrow().default_zoom);
        let pending = self.new_windows.borrow_mut().remove(&id);
        match pending {
            Some(nw) => nw.attach(&c),
            None => {
                host_navigation.set(true);
                if let Err(e) = c.navigate(&url) {
                    warning!("tab {id}: navigate failed: {e}");
                }
            }
        }
        c.set_visible(active);
        self.views.borrow_mut().insert(
            id,
            TabView {
                controller: c,
                host_navigation,
                can_go_back: false,
                can_go_forward: false,
                find: None,
                find_query: String::new(),
                net,
                stage: Cell::new(lb_core::memory::Stage::Awake),
                stage_since: Cell::new(now_ms()),
                audio,
                downloads: Rc::new(Cell::new(0)),
            },
        );
        if active {
            self.layout();
            self.focus_active();
        }
        self.send_nav();
    }

    fn hook_tab(&self, id: TabId, c: &Controller, host_navigation: Rc<Cell<bool>>, net: Rc<TabNet>) {
        self.hook_common(c);
        self.hook_downloads(c, Some(id));

        let weak = self.me.clone();
        let net_start = net.clone();
        c.on_navigation_starting(move |nav| {
            let Some(b) = weak.upgrade() else { return false };
            let host = host_navigation.replace(false);
            let current = b.model.borrow().get(id).map(|t| t.url.clone()).unwrap_or_default();
            let allow = assets::allow_navigation(Channel::Tab, &current, &nav.uri, host);
            if !allow {
                info!("tab {id}: blocked navigation to an internal or unsupported URL");
                return false;
            }
            if let Some(rewritten) = b.navigation_starting(&net_start, &nav.uri, nav.redirected) {
                // Cancel and restart at the cleaned/upgraded URL.
                b.defer(move |b| b.navigate_tab(id, &rewritten));
                return false;
            }
            b.model.borrow_mut().set_loading(id, true);
            b.tab_changed(id);
            true
        });
        let weak = self.me.clone();
        c.on_navigation_completed(move |ok, status| {
            if let Some(b) = weak.upgrade() {
                b.navigation_completed(id, &net, ok, status);
                b.model.borrow_mut().set_loading(id, false);
                if ok {
                    b.record_visit(id);
                }
                b.tab_changed(id);
            }
        });
        let weak = self.me.clone();
        c.on_source_changed(move |url| {
            if let Some(b) = weak.upgrade() {
                b.model.borrow_mut().set_url(id, &url);
                b.tab_changed(id);
            }
        });
        let weak = self.me.clone();
        c.on_title_changed(move |title| {
            if let Some(b) = weak.upgrade() {
                b.model.borrow_mut().set_title(id, &title);
                b.record_title(id);
                b.tab_changed(id);
            }
        });
        let weak = self.me.clone();
        c.on_history_changed(move |back, fwd| {
            if let Some(b) = weak.upgrade() {
                if let Some(v) = b.views.borrow_mut().get_mut(&id) {
                    v.can_go_back = back;
                    v.can_go_forward = fwd;
                }
                b.send_nav();
            }
        });
        let weak = self.me.clone();
        c.on_favicon_changed(move |png| {
            if let Some(b) = weak.upgrade() {
                b.model.borrow_mut().set_favicon(id, assets::png_data_url(&png));
                b.cache_favicon(id, &png);
                b.send_tabs();
            }
        });
        let weak = self.me.clone();
        c.on_new_window(move |nw| {
            if let Some(b) = weak.upgrade() {
                b.open_new_window(id, nw);
            }
        });
        let weak = self.me.clone();
        c.on_zoom_changed(move |_| {
            if let Some(b) = weak.upgrade() {
                b.send_nav();
            }
        });
        let weak = self.me.clone();
        c.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                b.on_tab_message(id, &source, json);
            }
        });
    }

    fn destroy_view(&self, id: TabId) {
        self.new_windows.borrow_mut().remove(&id);
        let view = self.views.borrow_mut().remove(&id);
        if let Some(v) = view {
            v.controller.close();
        }
    }

    fn activate_view(&self, from: Option<TabId>, to: TabId) {
        self.wake_tab(to);
        self.hide_popup();
        if let Some(f) = from {
            self.stop_find(f);
        }
        if let Some(f) = from.and_then(|f| self.controller(f)) {
            f.set_visible(false);
        }
        if let Some(t) = self.controller(to) {
            t.set_visible(true);
        }
        self.layout();
        self.focus_active();
        self.update_title();
    }

    fn controller(&self, id: TabId) -> Option<Controller> {
        self.views.try_borrow().ok()?.get(&id).map(|v| v.controller.clone())
    }

    fn active_controller(&self) -> Option<Controller> {
        let id = self.model.try_borrow().ok()?.active_id()?;
        self.controller(id)
    }

    fn shell(&self) -> Option<Controller> {
        self.shell.try_borrow().ok()?.clone()
    }

    /// New tab pages put the caret in the address bar; other pages get focus.
    fn focus_active(&self) {
        let is_new_tab = self.model.borrow().active().is_some_and(|t| Page::from_url(&t.url) == Some(Page::NewTab));
        if is_new_tab {
            self.focus_address_bar();
        } else if let Some(c) = self.active_controller() {
            c.focus();
        }
    }

    fn focus_address_bar(&self) {
        if let Some(s) = self.shell() {
            s.focus();
            s.post_json(&Event::FocusAddressBar.to_json());
        }
    }

    /// Opens the find bar with keyboard focus in it: the shell WebView must
    /// get focus first, or typing still goes to the page.
    pub(super) fn show_find_bar(&self) {
        if let Some(s) = self.shell() {
            s.focus();
            s.post_json(&Event::ShowFind.to_json());
        }
    }

    // ------------------------------------------------------------ layout & state

    fn layout(&self) {
        let Some(w) = self.window() else { return };
        let (width, height) = w.client_size();
        let bar = self.settings.borrow().show_bookmarks_bar;
        let (shell, content) = layout::split(width, height, w.scale(), bar);
        if let Some(s) = self.shell() {
            s.set_bounds(shell.x, shell.y, shell.width, shell.height);
        }
        if let Some(c) = self.active_controller() {
            c.set_bounds(content.x, content.y, content.width, content.height);
        }
    }

    fn tab_changed(&self, id: TabId) {
        self.schedule_session_save();
        self.send_tabs();
        if self.model.borrow().active_id() == Some(id) {
            self.send_nav();
            self.update_title();
        }
    }

    fn update_title(&self) {
        let title = self.model.borrow().active().map(|t| t.title.clone()).unwrap_or_default();
        if let Some(w) = self.window() {
            w.set_title(&if title.is_empty() {
                "LightBrowser".to_string()
            } else {
                format!("{title} – LightBrowser")
            });
        }
    }

    fn post_shell(&self, e: &Event) {
        if let Some(s) = self.shell() {
            s.post_json(&e.to_json());
        }
    }

    fn send_tabs(&self) {
        let (tabs, active) = {
            let m = self.model.borrow();
            (m.infos(), m.active_id())
        };
        self.post_shell(&Event::Tabs { tabs, active });
    }

    fn nav_state(&self) -> Option<NavState> {
        let m = self.model.try_borrow().ok()?;
        let tab = m.active()?;
        let views = self.views.try_borrow().ok()?;
        let view = views.get(&tab.id);
        let secure = tab.url.starts_with("https://") && !pages::is_pages_origin(&tab.url);
        let bookmarked = self.store.try_borrow().ok().is_some_and(|s| s.is_bookmarked(&tab.url).unwrap_or(false));
        Some(NavState {
            tab_id: tab.id,
            url: pages::display_url(&tab.url),
            can_go_back: view.is_some_and(|v| v.can_go_back),
            can_go_forward: view.is_some_and(|v| v.can_go_forward),
            loading: tab.loading,
            secure,
            bookmarked,
            blocked: view.map(|v| v.net.blocked.get()).unwrap_or(0),
            site_allowed: view.is_some_and(|v| v.net.allowed.get()),
            zoom: view.map(|v| v.controller.zoom()).unwrap_or(1.0),
            private: tab.private,
            login_form: view.is_some_and(|v| v.net.login_form.get()),
        })
    }

    fn send_nav(&self) {
        if let Some(state) = self.nav_state() {
            self.post_shell(&Event::Nav { state });
        }
    }

    fn send_shell_config(&self) {
        let e = {
            let s = self.settings.borrow();
            Event::ShellConfig {
                show_home_button: s.show_home_button,
                show_bookmarks_bar: s.show_bookmarks_bar,
                theme: s.theme,
            }
        };
        self.post_shell(&e);
    }

    /// Saves the session shortly after the last change (titles and URLs
    /// change often while pages load).
    fn schedule_session_save(&self) {
        if let Some(w) = self.window() {
            w.set_timer(SESSION_TIMER, SESSION_DEBOUNCE_MS);
        }
    }

    fn save_session(&self) {
        let session = self.model.borrow().session();
        if let Ok(json) = serde_json::to_string(&session) {
            if let Ok(store) = self.store.try_borrow() {
                let _ = store.kv_set(SESSION_KEY, &json);
            }
        }
    }

    // ------------------------------------------------------------ tab operations

    fn with_model<T>(&self, f: impl FnOnce(&mut TabModel) -> T) -> T {
        f(&mut self.model.borrow_mut())
    }

    fn new_tab(&self, url: Option<&str>, private: bool) {
        let effects = self.with_model(|m| m.open(url.unwrap_or(""), private, Position::End, true, true, now_ms()).1);
        self.apply(effects);
    }

    fn open_new_window(&self, opener: TabId, nw: NewWindow) {
        let private = self.model.borrow().get(opener).is_some_and(|t| t.private);
        let url = nw.uri.clone();
        let (id, effects) =
            self.with_model(|m| m.open(&url, private, Position::AfterOpener(opener), true, true, now_ms()));
        // The view for `id` is created by the CreateView effect; it picks
        // up the pending request and attaches itself as the new window.
        self.new_windows.borrow_mut().insert(id, nw);
        self.apply(effects);
    }

    /// Opens an http(s) or built-in URL, in the active tab or a new one.
    fn open_url(&self, url: &str, new_tab: bool, private: bool) {
        let ok = url.starts_with("https://") || url.starts_with("http://") || Page::from_url(url).is_some();
        if !ok {
            return;
        }
        if new_tab {
            self.new_tab(Some(url), private);
        } else {
            self.navigate_active(url);
        }
    }

    fn navigate_active(&self, url: &str) {
        let Some(id) = self.model.borrow().active_id() else { return };
        let views = self.views.borrow();
        if let Some(v) = views.get(&id) {
            v.host_navigation.set(true);
            let _ = v.controller.navigate(url);
        }
    }

    /// Built-in pages replace a new-tab page, otherwise open in a new tab.
    fn open_page(&self, page: Page) {
        let on_new_tab = self.model.borrow().active().is_some_and(|t| Page::from_url(&t.url) == Some(Page::NewTab));
        if on_new_tab {
            self.navigate_active(&page.url());
        } else {
            self.new_tab(Some(&page.url()), false);
        }
    }

    // ------------------------------------------------------------ commands

    fn on_key(&self, key: KeyPress) -> bool {
        let Some(cmd) = keys::command_for(key.vk, key.ctrl, key.shift, key.alt) else { return false };
        // Holding a key down repeats only commands where that makes sense.
        let repeatable = matches!(cmd, Command::ZoomIn | Command::ZoomOut | Command::NextTab | Command::PrevTab);
        if !key.repeat || repeatable {
            self.run_command(cmd);
        }
        true
    }

    fn run_command(&self, cmd: Command) {
        let now = now_ms();
        match cmd {
            Command::NewTab => self.new_tab(None, false),
            Command::NewPrivateTab => self.new_tab(None, true),
            Command::CloseTab => {
                // Bind first: an `if let` scrutinee's `Ref` lives for the whole
                // block, and `with_model` needs the model mutably.
                let active = self.model.borrow().active_id();
                if let Some(id) = active {
                    let effects = self.with_model(|m| m.close(id, now));
                    self.apply(effects);
                }
            }
            Command::ReopenClosedTab => {
                if let Some((_, effects)) = self.with_model(|m| m.reopen_closed(now)) {
                    self.apply(effects);
                }
            }
            Command::NextTab | Command::PrevTab => {
                let effects = self.with_model(|m| m.cycle(cmd == Command::NextTab, now));
                self.apply(effects);
            }
            Command::SelectTab(i) => {
                let effects = self.with_model(|m| m.activate_index(usize::from(i), now));
                self.apply(effects);
            }
            Command::LastTab => {
                let effects = self.with_model(|m| m.activate_last(now));
                self.apply(effects);
            }
            Command::FocusAddressBar => self.focus_address_bar(),
            Command::Reload => self.active_do(|c| c.reload()),
            Command::HardReload => self.active_do(|c| c.hard_reload()),
            Command::Stop => self.active_do(|c| c.stop()),
            Command::Back => self.active_do(|c| c.go_back()),
            Command::Forward => self.active_do(|c| c.go_forward()),
            Command::Home => {
                let home = self.settings.borrow().homepage.clone();
                let target = if home.is_empty() { Page::NewTab.url() } else { home };
                self.navigate_active(&target);
            }
            Command::ZoomIn => self.zoom(ZoomAction::In),
            Command::ZoomOut => self.zoom(ZoomAction::Out),
            Command::ZoomReset => self.zoom(ZoomAction::Reset),
            Command::Print => self.active_do(|c| c.print()),
            Command::DevTools => self.active_do(|c| c.open_devtools()),
            Command::History => self.open_page(Page::History),
            Command::Bookmarks => self.open_page(Page::Bookmarks),
            Command::Downloads => self.open_page(Page::Downloads),
            // Wired up in milestone 4 (bookmarks, find in page).
            Command::Bookmark => self.toggle_bookmark(),
            Command::Find => self.show_find_bar(),
            Command::FindNext => self.find_step(false),
            Command::FindPrev => self.find_step(true),
        }
    }

    fn active_do(&self, f: impl FnOnce(&Controller)) {
        if let Some(c) = self.active_controller() {
            f(&c);
        }
    }

    fn zoom(&self, action: ZoomAction) {
        let default = self.settings.borrow().default_zoom;
        self.active_do(|c| {
            let z = match action {
                ZoomAction::In => layout::next_zoom(c.zoom(), true),
                ZoomAction::Out => layout::next_zoom(c.zoom(), false),
                ZoomAction::Reset => default,
            };
            c.set_zoom(z);
        });
        self.send_nav();
        // Keep the zoom value in an open menu up to date.
        let menu_open =
            self.popup.borrow().as_ref().is_some_and(|p| matches!(p.content, Some(PopupContent::Menu { .. })));
        if menu_open {
            self.show_popup(self.menu_content(), None);
        }
    }

    // ------------------------------------------------------------ messages

    fn on_ui_message(&self, channel: Channel, source: &str, json: &str) {
        let sender = match protocol::classify(channel, source, true) {
            Ok(Source::Privileged(s)) => s,
            _ => {
                warning!("rejected message on {channel:?} channel from an unexpected origin");
                return;
            }
        };
        match protocol::parse_request(sender, json) {
            Ok(env) => self.dispatch(sender, None, env),
            Err(e) => warning!("rejected {sender:?} message: {e}"),
        }
    }

    fn on_tab_message(&self, id: TabId, source: &str, json: &str) {
        match protocol::classify(Channel::Tab, source, true) {
            Ok(Source::Privileged(sender)) => match protocol::parse_request(sender, json) {
                Ok(env) => self.dispatch(sender, Some(id), env),
                Err(e) => warning!("tab {id}: rejected {sender:?} message: {e}"),
            },
            Ok(Source::Web { origin }) => match protocol::parse_content(json) {
                Ok(msg @ ContentMessage::PageTokens { .. }) => self.page_tokens(id, &origin, msg),
                Ok(ContentMessage::LoginForm {}) => {
                    let net = self.views.borrow().get(&id).map(|v| v.net.clone());
                    if let Some(net) = net {
                        // Only for the page currently shown in the tab.
                        if urlclean::origin_of(&net.top_url.borrow()).as_deref() == Some(origin.as_str()) {
                            net.login_form.set(true);
                            self.send_nav();
                        }
                    }
                }
                Ok(ContentMessage::Submit { username, password }) => {
                    self.login_submitted(id, &origin, username, password)
                }
                Ok(ContentMessage::RequestFill {}) => self.request_fill(id, Some(&origin)),
                Err(_) => {}
            },
            Err(_) => {}
        }
    }

    /// Sends a reply to whoever asked (a built-in page in tab `tab`).
    fn reply(&self, tab: Option<TabId>, event: Event) {
        if let Some(c) = tab.and_then(|t| self.controller(t)) {
            c.post_json(&event.to_json());
        }
    }

    fn dispatch(&self, sender: Sender, tab: Option<TabId>, env: Envelope) {
        let now = now_ms();
        let Envelope { req_id, request } = env;
        match request {
            Request::Ready => match sender {
                Sender::Shell => {
                    self.send_shell_config();
                    self.send_tabs();
                    self.send_nav();
                    self.send_bookmarks_bar();
                    self.post_shell(&Event::DownloadsChanged { active: self.downloads.borrow().active_count() });
                    // A new-tab page activated before the shell finished
                    // loading could not focus the address bar yet.
                    self.focus_active();
                }
                Sender::Popup => {
                    if let Some(c) = self.popup.borrow().as_ref().and_then(|p| p.controller.clone()) {
                        self.send_theme_to(&c);
                    }
                    self.popup_ready();
                }
                Sender::Page(_) => {
                    if let Some(c) = tab.and_then(|t| self.controller(t)) {
                        self.send_theme_to(&c);
                    }
                }
            },
            Request::NewTab { url, private } => {
                let url = url.filter(|u| u.starts_with("https://") || u.starts_with("http://"));
                self.new_tab(url.as_deref(), private);
                if sender == Sender::Popup {
                    self.hide_popup();
                }
            }
            Request::CloseTab { id } => {
                let effects = self.with_model(|m| m.close(id, now));
                self.apply(effects);
            }
            Request::ActivateTab { id } => {
                let effects = self.with_model(|m| m.activate(id, now));
                self.apply(effects);
            }
            Request::MoveTab { id, index } => {
                if self.with_model(|m| m.move_tab(id, index as usize)) {
                    self.send_tabs();
                    self.save_session();
                }
            }
            Request::DuplicateTab { id } => {
                if let Some((_, effects)) = self.with_model(|m| m.duplicate(id, now)) {
                    self.apply(effects);
                }
            }
            Request::ReopenClosedTab => self.run_command(Command::ReopenClosedTab),
            Request::Navigate { input } => {
                let template = self.settings.borrow().search_template().to_string();
                let url = urlclean::input_to_url(&input, &template);
                self.navigate_active(&url);
                if sender == Sender::Shell {
                    if let Some(c) = self.active_controller() {
                        c.focus();
                    }
                }
            }
            Request::Back => self.run_command(Command::Back),
            Request::Forward => self.run_command(Command::Forward),
            Request::Reload => self.run_command(Command::Reload),
            Request::Stop => self.run_command(Command::Stop),
            Request::Home => self.run_command(Command::Home),
            Request::OpenUrl { url, new_tab } => self.open_url(&url, new_tab, false),
            Request::OpenPage { page } => {
                if let Some(p) = Page::from_name(&page) {
                    self.open_page(p);
                }
            }
            Request::Command { command } => self.run_command(command),
            Request::Zoom { action } => self.zoom(action),
            Request::Print => self.run_command(Command::Print),
            Request::DevTools => self.run_command(Command::DevTools),
            Request::ShowPopup { popup, rect } => {
                if popup.shell_may_open() && rect.is_valid() {
                    let content = match popup {
                        PopupKind::Menu => self.menu_content(),
                        _ => self.shield_content(),
                    };
                    self.show_popup(content, Some(rect));
                }
            }
            Request::HidePopup => self.hide_popup(),
            Request::Quit => self.begin_exit(),
            r @ (Request::GetSettings
            | Request::SaveSettings { .. }
            | Request::ResetSettings
            | Request::PickDownloadFolder
            | Request::Restart
            | Request::GetAbout) => self.dispatch_settings(tab, req_id, r),
            // Prompts and the remaining features arrive in later milestones;
            // the popup still treats a reply as "dismissed" by hiding.
            Request::SavePasswordReply { tab_id, origin, accept, never } => {
                self.save_password_reply(tab_id, &origin, accept, never)
            }
            Request::FillReply { tab_id, origin, entry_id } => self.fill_reply(tab_id, &origin, entry_id),
            Request::RequestFill => {
                let active = self.model.borrow().active_id();
                if let Some(id) = active {
                    self.request_fill(id, None);
                }
            }
            r @ (Request::VaultStatus
            | Request::VaultSetup { .. }
            | Request::VaultUnlock { .. }
            | Request::VaultLock
            | Request::VaultList
            | Request::VaultReveal { .. }
            | Request::VaultCopy { .. }
            | Request::VaultSave { .. }
            | Request::VaultDelete { .. }
            | Request::VaultChangePassword { .. }
            | Request::GeneratePassword { .. }
            | Request::GetNeverSave
            | Request::NeverSaveRemove { .. }) => self.dispatch_vault(tab, req_id, r),
            Request::ShieldAllowSite { allow } => self.shield_allow_site(allow),
            r @ (Request::GetAllowlist
            | Request::AllowlistAdd { .. }
            | Request::AllowlistRemove { .. }
            | Request::UpdateFilterLists
            | Request::GetFilterStatus
            | Request::HttpsOnlyProceed { .. }) => self.dispatch_blocking(tab, req_id, r),
            other => self.dispatch_library(sender, tab, req_id, other),
        }
    }

    /// Fallback for requests no milestone implements yet.
    fn not_available(&self, sender: Sender, tab: Option<TabId>, req_id: Option<u32>, request: &Request) {
        info!("{sender:?}: request not implemented yet: {}", request_name(request));
        self.reply(tab, Event::err(req_id, "Not available yet"));
    }

    // ------------------------------------------------------------ popup

    fn menu_content(&self) -> PopupContent {
        let zoom = self.active_controller().map(|c| c.zoom()).unwrap_or(1.0);
        PopupContent::Menu { zoom, can_reopen: self.model.borrow().closed_count() > 0 }
    }

    /// Shows `content` in the popup. `rect` is in shell CSS pixels; `None`
    /// keeps the current position (content refresh).
    fn show_popup(&self, content: PopupContent, rect: Option<PopupRect>) {
        let Some(main) = self.window() else { return };
        if self.popup.borrow().is_none() && !self.create_popup(main) {
            return;
        }
        let mut guard = self.popup.borrow_mut();
        let Some(popup) = guard.as_mut() else { return };
        let kind = content.kind();
        popup.content = Some(content.clone());
        if let Some(rect) = rect {
            let scale = main.scale();
            let origin = main.client_to_screen(0, 0);
            let work = Window::work_area_at(origin.0 + (rect.x * scale) as i32, origin.1);
            let r = layout::popup_screen_rect(
                rect,
                scale,
                origin,
                layout::Rect { x: work.x, y: work.y, width: work.width, height: work.height },
            );
            popup.window.set_activatable(kind.takes_focus());
            popup.window.show_at(ScreenRect { x: r.x, y: r.y, width: r.width, height: r.height }, kind.takes_focus());
            if let Some(c) = &popup.controller {
                c.set_bounds(0, 0, r.width, r.height);
                c.set_visible(true);
                if kind.takes_focus() {
                    c.focus();
                }
            }
        }
        if popup.ready {
            if let Some(c) = &popup.controller {
                c.post_json(&Event::Popup { content }.to_json());
            }
        }
    }

    fn create_popup(&self, main: Window) -> bool {
        let Some(env) = self.env.get().cloned() else { return false };
        let events: Rc<dyn WindowEvents> = Rc::new(PopupEvents(self.me.clone()));
        let window = match Window::create_popup(main, events) {
            Ok(w) => w,
            Err(e) => {
                warning!("popup: {e}");
                return false;
            }
        };
        *self.popup.borrow_mut() = Some(Popup { window, controller: None, ready: false, content: None });
        let weak = self.me.clone();
        // The popup always uses the default (shell) profile, never a private one.
        let started = env.create_controller(window.hwnd(), false, move |result| {
            let Some(b) = weak.upgrade() else { return };
            match result {
                Ok(c) => b.popup_controller_ready(c),
                Err(e) => warning!("popup: {e}"),
            }
        });
        if let Err(e) = started {
            warning!("popup: {e}");
            return false;
        }
        true
    }

    fn popup_controller_ready(&self, c: Controller) {
        let Some(env) = self.env.get() else { return };
        c.lock_down_ui();
        c.set_background(UI_BACKGROUND);
        c.serve(env, SHELL_FILTER, self.server());
        c.on_navigation_starting(|nav| assets::allow_navigation(Channel::Popup, "", &nav.uri, true));
        c.on_new_window(|_| {});
        let weak = self.me.clone();
        c.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                b.on_ui_message(Channel::Popup, &source, json);
            }
        });
        let weak = self.me.clone();
        c.on_accelerator(move |key| weak.upgrade().is_some_and(|b| b.on_key(key)));
        let _ = c.navigate(POPUP_URL);
        let mut guard = self.popup.borrow_mut();
        if let Some(p) = guard.as_mut() {
            let (w, h) = p.window.client_size();
            c.set_bounds(0, 0, w, h);
            c.set_visible(p.window.is_visible());
            p.controller = Some(c);
        }
    }

    fn popup_ready(&self) {
        let pending = {
            let mut guard = self.popup.borrow_mut();
            let Some(p) = guard.as_mut() else { return };
            p.ready = true;
            if p.window.is_visible() {
                p.content.clone().zip(p.controller.clone())
            } else {
                None
            }
        };
        if let Some((content, c)) = pending {
            c.post_json(&Event::Popup { content }.to_json());
            c.focus();
        }
    }

    /// Hides the popup. A hidden prompt counts as dismissed, never confirmed.
    fn hide_popup(&self) {
        let (was_visible, content) = {
            let Ok(mut guard) = self.popup.try_borrow_mut() else { return };
            let Some(p) = guard.as_mut() else { return };
            let visible = p.window.is_visible();
            if visible {
                p.window.hide();
            }
            (visible, p.content.take())
        };
        if let Some(c) = &content {
            self.prompt_dismissed(c);
        }
        if was_visible {
            self.post_shell(&Event::PopupClosed);
        }
    }

    // ------------------------------------------------------------ internal origins

    /// Request handler for our internal origins: embedded UI files, and the
    /// cached favicons from the database.
    fn server(&self) -> impl Fn(&str) -> Option<InternalResponse> + 'static {
        let weak = self.me.clone();
        move |uri: &str| {
            if let Some(host) = assets::favicon_request(uri) {
                let png = weak.upgrade()?.store.try_borrow().ok()?.favicon(&host).ok()??;
                return Some(InternalResponse {
                    status: 200,
                    reason: "OK",
                    headers: assets::favicon_headers().to_string(),
                    body: Cow::Owned(png),
                });
            }
            let asset = assets::resolve(uri)?;
            Some(InternalResponse {
                status: 200,
                reason: "OK",
                headers: assets::response_headers(&asset),
                body: Cow::Borrowed(asset.bytes),
            })
        }
    }

    // ------------------------------------------------------------ shutdown

    fn shutdown(&self) {
        if self.closing.replace(true) {
            return;
        }
        if let Some(w) = self.window() {
            w.kill_timer(SESSION_TIMER);
        }
        self.save_session();
        self.save_downloads();
        self.shutdown_vault();
        self.relaunch_if_requested();
        self.new_windows.borrow_mut().clear();
        let views: Vec<TabView> = self.views.borrow_mut().drain().map(|(_, v)| v).collect();
        for v in views {
            v.controller.close();
        }
        if let Some(p) = self.popup.borrow_mut().take() {
            if let Some(c) = p.controller {
                c.close();
            }
            p.window.destroy();
        }
        if let Some(s) = self.shell.borrow_mut().take() {
            s.close();
        }
    }
}

/// Window events of the popup window.
struct PopupEvents(Weak<Browser>);

impl WindowEvents for PopupEvents {
    fn activated(&self, active: bool) {
        if !active {
            if let Some(b) = self.0.upgrade() {
                b.hide_popup();
            }
        }
    }
}

impl WindowEvents for Browser {
    fn resized(&self, _width: i32, _height: i32) {
        self.hide_popup();
        self.layout();
    }

    fn minimized(&self, minimized: bool) {
        self.hide_popup();
        // Hidden WebViews use less CPU and memory.
        if let Some(c) = self.active_controller() {
            c.set_visible(!minimized);
        }
        if let Some(s) = self.shell() {
            s.set_visible(!minimized);
        }
    }

    fn moved(&self) {
        self.hide_popup();
        if let Some(s) = self.shell() {
            s.notify_parent_moved();
        }
        if let Some(c) = self.active_controller() {
            c.notify_parent_moved();
        }
    }

    fn system_theme_changed(&self) {
        self.apply_frame_theme();
    }

    fn got_focus(&self) {
        // Keyboard shortcuts arrive through the WebViews, so the frame
        // must never keep the focus itself.
        self.focus_active();
    }

    fn close_requested(&self) -> bool {
        // With clear-on-exit, close only after the data has been cleared.
        if self.settings.borrow().clear_on_exit.any() && !self.exit_cleared.get() {
            self.begin_exit();
            return false;
        }
        true
    }

    fn destroyed(&self) {
        self.shutdown();
    }

    fn session_locked(&self) {
        self.lock_vault("Windows session locked");
    }

    fn suspending(&self) {
        self.lock_vault("system suspend");
    }

    fn timer(&self, id: usize) {
        match id {
            SESSION_TIMER => {
                if let Some(w) = self.window() {
                    w.kill_timer(SESSION_TIMER);
                }
                self.save_session();
            }
            SHIELD_TIMER => self.shield_timer(),
            POWER_TIMER => self.power_tick(),
            MEMREPORT_TIMER => self.mem_report_line("tick"),
            VAULT_TIMER => self.vault_timer(),
            EXIT_TIMER => self.finish_exit(),
            CLIPBOARD_TIMER => self.clipboard_timer(),
            UPDATE_TIMER => {
                if let Some(w) = self.window() {
                    w.set_timer(UPDATE_TIMER, blocking::UPDATE_CHECK_INTERVAL_MS);
                }
                self.scheduled_filter_check();
            }
            _ => {}
        }
    }

    fn app_message(&self, code: usize) {
        match code {
            APP_FILTERS_READY => self.filters_ready(),
            APP_DEFERRED => self.run_deferred(),
            APP_JOB => self.jobs_ready(),
            _ => {}
        }
    }
}

/// Short name of a request for logs (never its contents).
fn request_name(r: &Request) -> String {
    serde_json::to_value(r)
        .ok()
        .and_then(|v| v.get("cmd").and_then(|c| c.as_str()).map(String::from))
        .unwrap_or_else(|| "?".into())
}
