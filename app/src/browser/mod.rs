//! Browser host: owns the window, the shell/popup views and one browser
//! view per live tab, and turns `TabModel` effects into engine calls.
//! All policy (tabs, shortcuts, origins, navigation, layout) lives in core.

use crate::ffi::cef::{
    self as engine, DownloadControl, InternalResponse, KeyPress, LoadingState, MessageSource, NewWindow, Role, Scripts,
    View, ViewOptions,
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
    Source, TabAction, UnlockPurpose, ZoomAction,
};
use lb_core::settings::{Settings, Startup};
use lb_core::storage::{now_ms, Store};
use lb_core::tabs::{CloseMany, Effect, Position, Tab, TabId, TabModel};
use lb_core::urlclean::{self, LoadFailure};
use lb_core::windows::{Bounds, Op, SessionV2, WindowId, Workspace};
use std::borrow::Cow;
use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

const SHELL_URL: &str = "https://shell.krunk.invalid/shell.html";
const POPUP_URL: &str = "https://shell.krunk.invalid/popup.html";
const SESSION_KEY: &str = "session";
const DOWNLOADS_KEY: &str = "downloads";
/// Timer that saves the session shortly after the last tab change.
const SESSION_TIMER: usize = 1;
/// Zoom watcher: Ctrl+wheel and per-site zoom change the engine's zoom without telling the
/// host (CEF 156 has no zoom callback); the active tab is read every 100 ms while a Krunk
/// window is in front, and the toolbar is updated on a change.
const ZOOM_TIMER: usize = 15;
const ZOOM_WATCH_MS: u32 = 100;
const SESSION_DEBOUNCE_MS: u32 = 2000;

mod account;
mod blocking;
mod bookmarks_bar;
mod bounce;
mod devtools;
mod dns_host;
mod downloads;
mod drag;
mod ext;
mod favicons;
mod filter_lists;
mod find;
mod flight;
mod frame;
mod hang;
mod hints;
mod http_auth;
mod import;
mod integrity;
mod jobs;
mod library;
mod navmenu;
mod omnibox;
#[cfg(feature = "perf-probe")]
mod perf_probe;
mod permissions;
mod pip;
mod pointer;
mod power;
mod segmented;
mod settings_host;
mod siteinfo;
mod source;
mod spare;
mod status;
mod storage;
mod suggest;
mod sync;
mod tablist;
mod tasks;
mod updater;

pub use power::PowerOptions;
use power::{MEMREPORT_TIMER, POWER_TIMER};
mod vault;
mod vault_recovery;

use settings_host::EXIT_TIMER;

use jobs::{Jobs, APP_JOB};
use vault::{VaultHost, CLIPBOARD_TIMER, VAULT_TIMER};

pub(crate) use blocking::LOG_BLOCKED;
use blocking::{
    Deferred, FilterState, NetShared, TabNet, APP_DEFERRED, APP_FILTERS_READY, APP_SHIELD, SHIELD_TIMER, UPDATE_TIMER,
};
/// Background behind the UI views (matches --bg in common.css).
const UI_BACKGROUND: u32 = 0xFFF3_F4F6;
/// The same in the dark theme: the shell view shows it while it is resized (0.7.2, R6).
const UI_BACKGROUND_DARK: u32 = 0xFF1F_2023;
/// What a tab view shows before its page has painted (0.7.0, R7): the dark theme's page colour
/// (`--bg` in ui/shared/common.css) or white. Web pages that set no background of their own
/// still get white (`Emulation.setDefaultBackgroundColorOverride`, view.rs).
const TAB_BACKGROUND_DARK: u32 = 0xFF1F_2023;
/// How long a tab switch keeps the old page while the new view has not loaded once, and the
/// pause after its load before the swap (R7).
const REVEAL_TIMEOUT_MS: i64 = 1000;
const REVEAL_SETTLE_MS: i64 = 50;
/// How long the old page stays beneath the new one after a tab switch (0.7.3).
const HIDE_OLD_VIEW_MS: i64 = 100;
/// The light theme's page colour (0.7.2, R6: no white while a window is resized; was white).
const TAB_BACKGROUND_LIGHT: u32 = 0xFFF3_F4F6;
/// What a page without a background of its own gets (`Emulation.setDefaultBackgroundColorOverride`).
const PAGE_DEFAULT_WHITE: u32 = 0xFFFF_FFFF;
/// What a popup window shows before its page has painted (0.7.2): `--bg-raised` of the theme,
/// so a menu shown for the first time is never a light box in the dark theme.
const POPUP_BACKGROUND_DARK: u32 = 0xFF2B_2D31;
const POPUP_BACKGROUND_LIGHT: u32 = 0xFFFF_FFFF;

/// Live state of a tab's browser view.
struct TabView {
    view: View,
    /// Set right before the host itself navigates, so the navigation
    /// policy can tell host navigations from page-initiated ones.
    host_navigation: Rc<Cell<bool>>,
    can_go_back: bool,
    can_go_forward: bool,
    /// The current find-in-page term.
    find_query: String,
    /// Shield counter, cosmetic data and HTTPS-only state of the page.
    net: Rc<TabNet>,
    /// Sleep stage and when it was entered.
    stage: Cell<lb_core::memory::Stage>,
    stage_since: Cell<i64>,
    /// The page is playing audio (never put to sleep).
    audio: Rc<Cell<bool>>,
    /// The page uses the camera / the microphone (the address bar's chip).
    media_use: Cell<(bool, bool)>,
    /// Downloads in progress started by this tab (never put to sleep).
    downloads: Rc<Cell<u32>>,
    /// DevTools is docked next to the page (open or opening).
    devtools: Cell<bool>,
    /// The view has finished loading once (0.7.0, R7): before that it may show a blank frame,
    /// so a tab switch keeps the old page on screen until then.
    loaded_once: Cell<bool>,
    /// Until the tab reports an address other than `about:blank`: the engine's empty first
    /// document is not shown in the bar (0.7.2, R1).
    blank_start: Cell<bool>,
    /// The page does not respond (the engine said so; 0.11.0).
    unresponsive: Cell<bool>,
    /// The view started at `about:blank` before its first page (edited headers): that entry
    /// leaves the tab's history once the page is there (0.11.1; Back offered about:blank).
    started_blank: Cell<bool>,
    /// "Wait" was chosen: no prompt until then (ms).
    hang_quiet_until: Cell<i64>,
    /// The last icon handled (its site and a hash of its bytes): an animated or re-announced
    /// icon with the same bytes is not encoded, stored and sent again (0.11.0).
    last_favicon: Cell<u64>,
}

/// The popup view in its popup window, shared by all frames: it is owned by the frame it
/// is shown for (created hidden at start).
pub(super) struct Popup {
    window: Window,
    view: Option<View>,
    ready: bool,
    /// Content shown now, or waiting for the popup document to be ready.
    content: Option<PopupContent>,
    /// The frame the popup belongs to now.
    owner: Option<WindowId>,
    /// Content posted and not painted yet: the window shows (or moves) once the popup
    /// answers `PopupShown` for it (0.11.0).
    pending: Option<PendingShow>,
}

impl Popup {
    /// Open, or about to open for content being painted.
    pub(super) fn open(&self) -> bool {
        self.pending.is_some() || self.window.is_visible()
    }
}

/// Where a popup goes once its new content is painted.
#[derive(Clone, Copy)]
pub(super) struct PendingShow {
    seq: u64,
    rect: ScreenRect,
    focus: bool,
}

/// How long a popup waits for its paint answer before it shows anyway (a stuck popup
/// renderer must not leave a click without a popup).
const POPUP_PAINT_WAIT_MS: i64 = 250;

/// How a popup opens (0.11.0; `KRUNK_POPUP_GATE`, for the VM check only): `hidden` (default:
/// the window stays hidden until its content is painted), `park` (shown off-screen until
/// then) or `off` (shown at once, as before 0.11.0).
fn popup_gate() -> &'static str {
    static GATE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    GATE.get_or_init(|| match std::env::var("KRUNK_POPUP_GATE").as_deref() {
        Ok("park") => "park".into(),
        Ok("off") => "off".into(),
        _ => "hidden".into(),
    })
}

/// One browser window (frame): its native window, its shell view (tab strip and toolbar)
/// and the shell's tab-row layout for hit testing. Its tabs are in the workspace.
pub(super) struct Frame {
    id: WindowId,
    window: Window,
    shell: RefCell<Option<View>>,
    hit_map: RefCell<Option<lb_core::frame::HitMap>>,
    active: Cell<bool>,
    /// The bookmarks bar shows in this window (laid out with it).
    bar: Cell<bool>,
    /// The zoom factor the toolbar shows (the zoom watcher pushes a change).
    last_zoom: Cell<f64>,
}

/// Which popup window: the shared one, or the menu over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PopupSlot {
    Main,
    Menu,
    /// The volume bar next to the floating video window (0.7.0, R8).
    Pip,
}

pub struct Browser {
    me: Weak<Browser>,
    /// The hidden application window (timers, worker-thread wake-ups, broadcasts).
    app: OnceCell<Window>,
    /// A frame created before the engine started (it gets the foreground), used for the
    /// first window.
    first_window: Cell<Option<Window>>,
    store: RefCell<Store>,
    settings: RefCell<Settings>,
    ws: RefCell<Workspace>,
    frames: RefCell<Vec<Rc<Frame>>>,
    views: RefCell<HashMap<TabId, TabView>>,
    popup: RefCell<Option<Popup>>,
    /// The task manager's previous byte samples (network rate).
    task_samples: tasks::TaskSamples,
    /// The download-start flight (an icon flying to the downloads button).
    flight: flight::FlightHost,
    /// A second popup window for a menu opened from inside the first one (an item of a
    /// bookmark folder dropdown): the dropdown stays open underneath, as in Chrome.
    menu_popup: RefCell<Option<Popup>>,
    /// The entries the open history menu offers, with its tab (0.8.0): a click can jump to
    /// these only.
    nav_offer: RefCell<Option<(TabId, Vec<i64>)>>,
    /// Settings → Account (0.8.0).
    account: account::AccountHost,
    sync: sync::SyncHost,
    /// A site's HTTP sign-in dialog (0.8.0).
    http_auth: http_auth::HttpAuthHost,
    /// The floating video's volume bar (created when first needed) and its state.
    pip_popup: RefCell<Option<Popup>>,
    pip_bar: RefCell<Option<pip::PipBar>>,
    /// The menu popup is being hidden by the host (its deactivation is not the user leaving).
    menu_closing: Cell<bool>,
    /// The request being dispatched came from the menu popup's view.
    request_from: Cell<PopupSlot>,
    /// Numbers the popup contents that wait for their paint answer.
    popup_seq: Cell<u64>,
    /// Until then (ms) a lost activation does not close the popup (0.11.0).
    popup_hold_until: Cell<i64>,
    /// Views the engine is creating for a page's `window.open`, waiting for
    /// their tab's CreateView effect.
    adopted: RefCell<HashMap<TabId, (View, Rc<TabNet>)>>,
    downloads: RefCell<Downloads>,
    /// Downloads fetched in parts (segmented.rs).
    parts_jobs: segmented::PartsJobs,
    /// Engine downloads Krunk gave up (cancelled at their next update).
    abandoned_downloads: RefCell<std::collections::HashSet<u32>>,
    /// Tabs auto-attached to their workers and frames while client hints are off (0.7.0).
    hints_attached: RefCell<std::collections::HashSet<TabId>>,
    /// When the list was loaded, and the addresses of the downloads the last close stopped.
    stopped_by_close: RefCell<(i64, Vec<String>)>,
    /// Running downloads by engine download id: our id and the controls
    /// (known after the first progress update).
    download_ops: RefCell<HashMap<u32, (u32, Option<DownloadControl>)>>,
    /// Button rect of the open bookmarks-bar folder dropdown.
    folder_anchor: Cell<Option<PopupRect>>,
    /// The open folder dropdown's first item in its folder (the bar's overflow, R10).
    folder_offset: Cell<u32>,
    /// The folders (with their offsets) a dropdown went down through, for "‹ Back".
    folder_stack: RefCell<Vec<(i64, u32)>>,
    /// The bookmark favicon probe's queue and log (0.11.0).
    favicon_probe: RefCell<favicons::ProbeState>,
    /// Swallows the click that closed a toolbar popup (see `lb_core::toggle`).
    toggle: RefCell<lb_core::toggle::ToggleGuard>,
    /// The link-status label (the address of the link under the mouse).
    status: status::StatusHost,
    /// Login suggestions under a focused field.
    suggest: suggest::SuggestHost,
    imports: import::ImportHost,
    permissions: permissions::PermissionHost,
    omnibox: omnibox::OmniboxHost,
    /// Some tab was given a user agent other than the one the engine started with.
    ua_overridden: Cell<bool>,
    /// Tabs started at about:blank because of edited headers: their real first URL.
    first_url: RefCell<HashMap<TabId, String>>,
    /// The engine's own client hints (read from the shell page once it loaded): the
    /// metadata of an override that changes only the user agent or the language.
    engine_hints: RefCell<Option<lb_core::headers::ClientHints>>,
    /// A tab switch waiting for the new view's first load: (window, old tab, new tab).
    /// Per window (0.11.0; one shared slot let a switch in another window cancel this one's):
    /// the tab still on screen and the tab waiting to be revealed.
    pending_reveal: RefCell<lb_core::reveal::Reveals<WindowId, TabId>>,
    /// The custom-DNS stub, once the settings asked for it.
    dns: RefCell<Option<crate::dns::Engine>>,
    /// Where the shell asked for the shield panel (it grows with its list from there).
    shield_anchor: Cell<Option<PopupRect>>,
    /// The tab krunk://adblock shows the blocked requests of (opened from its shield).
    adblock_source: Cell<Option<TabId>>,
    /// Where the bookmarks bar's menu opened (its editor opens there too).
    bookmark_menu_at: Cell<Option<PopupRect>>,
    dirs: DataDirs,
    filters: Rc<FilterState>,
    /// Blocking state shared with the engine's IO thread.
    net: Arc<NetShared>,
    /// A shield counter update is scheduled.
    shield_pending: Cell<bool>,
    deferred: RefCell<Vec<Deferred>>,
    power: PowerOptions,
    vaults: VaultHost,
    jobs: Jobs,
    /// The last toolbar state sent to each window (`send_nav_changed`).
    last_nav: RefCell<HashMap<WindowId, String>>,
    /// Page titles waiting to be written to history (`library::TITLE_TIMER`), by URL.
    pending_titles: RefCell<std::collections::BTreeMap<String, String>>,
    /// Settings the browser was started with (for "restart required").
    running_settings: Settings,
    restart_after_exit: Cell<bool>,
    exit_clearing: Cell<bool>,
    exit_cleared: Cell<bool>,
    closing: Cell<bool>,
    /// A new-tab page whose address bar must keep the keyboard focus until
    /// the user clicks into the page or navigates.
    address_focus: Cell<Option<TabId>>,
    /// The tab whose video went to the floating window automatically (setting `auto_pip`).
    auto_pip_tab: Cell<Option<TabId>>,
    /// Extensions (0.6.0).
    ext: ext::ExtHost,
    /// The extension request being dispatched came from the action popup (not the background).
    ext_request_from_popup: Cell<bool>,
    /// `krunk://sitedata` tabs and the site each was opened for.
    site_data: RefCell<HashMap<TabId, storage::SiteDataBinding>>,
    /// Pointer lock and full screen.
    pointer: pointer::PointerHost,
    /// Krunk's own update check.
    updates: updater::UpdateHost,
    /// Whether the installed files are this release's (0.7.0).
    integrity: integrity::IntegrityHost,
    /// A tab being dragged between windows.
    drag: Cell<Option<drag::TabDrag>>,
    /// During a tab drag: the other windows (topmost first) as they were at its start, and
    /// the dragged tab as their strips show it.
    drop_windows: RefCell<Vec<lb_core::frame::DropWindow>>,
    drag_ghost: RefCell<Option<lb_core::protocol::TabGhost>>,
    /// A tab drag is creating the window that follows the mouse: it shows without Windows'
    /// fade-in.
    tearing: Cell<bool>,
    /// A hidden frame with its shell loaded, for the next new window (0.11.1).
    spare: RefCell<Option<spare::SpareFrame>>,
    /// `post_shell` goes to the spare's shell only (it has just loaded).
    spare_only: Cell<bool>,
    /// A bookmark dragged out of a folder dropdown (host-driven, like a torn-off tab).
    bar_drag: Cell<Option<drag::BarDrag>>,
    /// The folder dropdown a bookmark menu was opened from (Esc goes back to it).
    menu_from_folder: Cell<Option<i64>>,
    /// The session was saved for the last time (exit): frames are going away, so later saves
    /// would lose their places.
    session_final: Cell<bool>,
}

impl Browser {
    /// Opens the window, the shell and the first tabs. Runs once the engine
    /// is ready.
    pub fn start(
        window: Window,
        dirs: &DataDirs,
        store: Store,
        settings: Settings,
        power: PowerOptions,
    ) -> Result<Rc<Browser>, String> {
        if !window.exists() {
            return Err("The window was closed during start-up".into());
        }
        // What the engine runs with (CEF's switches and Krunk's merged; 0.7.1).
        info!("engine command line: {}", crate::ffi::cef::command_line());
        let running_settings = settings.clone();
        let browser = Rc::new_cyclic(|me| Browser {
            me: me.clone(),
            app: OnceCell::new(),
            first_window: Cell::new(Some(window)),
            store: RefCell::new(store),
            settings: RefCell::new(settings),
            ws: RefCell::new(Workspace::new()),
            frames: RefCell::new(Vec::new()),
            views: RefCell::new(HashMap::new()),
            popup: RefCell::new(None),
            task_samples: tasks::TaskSamples::default(),
            flight: flight::FlightHost::default(),
            menu_popup: RefCell::new(None),
            nav_offer: RefCell::new(None),
            account: account::AccountHost::default(),
            sync: sync::SyncHost::default(),
            http_auth: http_auth::HttpAuthHost::default(),
            pip_popup: RefCell::new(None),
            pip_bar: RefCell::new(None),
            menu_closing: Cell::new(false),
            request_from: Cell::new(PopupSlot::Main),
            popup_seq: Cell::new(0),
            popup_hold_until: Cell::new(0),
            adopted: RefCell::new(HashMap::new()),
            downloads: RefCell::new(Downloads::new()),
            parts_jobs: segmented::PartsJobs::default(),
            abandoned_downloads: RefCell::new(Default::default()),
            hints_attached: RefCell::new(Default::default()),
            stopped_by_close: RefCell::new((0, Vec::new())),
            download_ops: RefCell::new(HashMap::new()),
            folder_anchor: Cell::new(None),
            folder_offset: Cell::new(0),
            folder_stack: RefCell::new(Vec::new()),
            favicon_probe: RefCell::new(favicons::ProbeState::default()),
            toggle: RefCell::new(lb_core::toggle::ToggleGuard::default()),
            status: status::StatusHost::default(),
            suggest: suggest::SuggestHost::default(),
            imports: import::ImportHost::default(),
            permissions: permissions::PermissionHost::default(),
            omnibox: omnibox::OmniboxHost::default(),
            ua_overridden: Cell::new(false),
            first_url: RefCell::new(HashMap::new()),
            engine_hints: RefCell::new(None),
            pending_reveal: RefCell::new(lb_core::reveal::Reveals::default()),
            dns: RefCell::new(None),
            bookmark_menu_at: Cell::new(None),
            adblock_source: Cell::new(None),
            shield_anchor: Cell::new(None),
            dirs: dirs.clone(),
            filters: Rc::new(FilterState::new()),
            net: Arc::new(NetShared::default()),
            shield_pending: Cell::new(false),
            deferred: RefCell::new(Vec::new()),
            power,
            vaults: VaultHost::default(),
            jobs: Jobs::new(),
            last_nav: RefCell::default(),
            pending_titles: RefCell::default(),
            running_settings,
            restart_after_exit: Cell::new(false),
            exit_clearing: Cell::new(false),
            exit_cleared: Cell::new(false),
            closing: Cell::new(false),
            address_focus: Cell::new(None),
            auto_pip_tab: Cell::new(None),
            ext: ext::ExtHost::new(),
            ext_request_from_popup: Cell::new(false),
            site_data: RefCell::new(HashMap::new()),
            pointer: pointer::PointerHost::default(),
            updates: updater::UpdateHost::new(dirs.update_status()),
            integrity: integrity::IntegrityHost::default(),
            drag: Cell::new(None),
            drop_windows: RefCell::new(Vec::new()),
            drag_ghost: RefCell::new(None),
            tearing: Cell::new(false),
            spare: RefCell::new(None),
            spare_only: Cell::new(false),
            bar_drag: Cell::new(None),
            menu_from_folder: Cell::new(None),
            session_final: Cell::new(false),
        });
        let app = Window::create_app(Rc::new(AppEvents(browser.me.clone())))?;
        let _ = browser.app.set(app);
        browser.net.set_notifier(app.notifier());
        browser.init_filters();
        browser.init_extensions();
        browser.start_bounce_timer();
        browser.apply_dns();
        browser.init_vault();
        engine::set_internal_server(browser.server());
        let weak = browser.me.clone();
        engine::on_relaunch(move |_args| {
            if let Some(b) = weak.upgrade() {
                info!("Krunk was started again; opening a new tab");
                if let Some(w) = b.window() {
                    w.bring_to_front();
                }
                b.new_tab(None, false);
            }
        });
        engine::set_color_scheme(browser.color_scheme());
        // The popup (menus, prompts, the pointer-lock hint) is created now,
        // hidden: creating its browser later would take the keyboard focus
        // from the page once (a game would see the window lose focus).
        browser.create_popup(window, PopupSlot::Main);
        browser.restore_or_open_start();
        if browser.frames.borrow().is_empty() {
            return Err("Cannot create the browser window".into());
        }
        browser.init_updates();
        browser.init_favicon_probe();
        browser.init_app_identity();
        browser.start_hang_timer();
        browser.init_integrity();
        browser.start_power_timer();
        browser.init_sync();
        if let Some(w) = browser.app() {
            w.set_timer(ZOOM_TIMER, ZOOM_WATCH_MS);
        }
        browser.mem_report_line("start");
        Ok(browser)
    }

    /// The current (most recently used) frame's window.
    fn window(&self) -> Option<Window> {
        self.cur().map(|f| f.window)
    }

    /// The hidden application window: timers and worker-thread notifications.
    fn app(&self) -> Option<Window> {
        self.app.get().copied()
    }

    fn frame(&self, win: WindowId) -> Option<Rc<Frame>> {
        self.frames.try_borrow().ok()?.iter().find(|f| f.id == win).cloned()
    }

    /// The current (most recently used) frame: where new tabs, menus and prompts go.
    fn cur(&self) -> Option<Rc<Frame>> {
        let win = self.ws.try_borrow().ok()?.current()?;
        self.frame(win)
    }

    fn cur_id(&self) -> Option<WindowId> {
        self.ws.try_borrow().ok()?.current()
    }

    fn all_frames(&self) -> Vec<Rc<Frame>> {
        self.frames.try_borrow().map(|f| f.clone()).unwrap_or_default()
    }

    /// The window a tab is in.
    fn window_of(&self, tab: TabId) -> Option<WindowId> {
        self.ws.try_borrow().ok()?.window_of(tab)
    }

    /// Input arrived from `win` (a key, a click into its shell): it is the current window.
    fn make_current(&self, win: WindowId) {
        if let Ok(mut ws) = self.ws.try_borrow_mut() {
            if ws.current() != Some(win) {
                ws.focus(win);
            }
        }
    }

    /// The current window's tab model.
    fn model(&self) -> Option<Ref<'_, TabModel>> {
        let ws = self.ws.try_borrow().ok()?;
        let win = ws.current()?;
        Ref::filter_map(ws, |ws| ws.model(win)).ok()
    }

    /// The active tab id of the current window.
    fn active_id(&self) -> Option<TabId> {
        self.model()?.active_id()
    }

    /// The active tab of the current window.
    fn active_tab(&self) -> Option<Ref<'_, Tab>> {
        Ref::filter_map(self.model()?, |m| m.active()).ok()
    }

    /// A tab in any window.
    fn tab(&self, id: TabId) -> Option<Ref<'_, Tab>> {
        Ref::filter_map(self.ws.try_borrow().ok()?, |ws| ws.tab(id)).ok()
    }

    /// Updates a tab's fields wherever it is.
    fn with_tab(&self, id: TabId, f: impl FnOnce(&mut TabModel, TabId)) {
        if let Ok(mut ws) = self.ws.try_borrow_mut() {
            ws.with_tab(id, f);
        }
    }

    // ------------------------------------------------------------ startup

    /// A frame for window `win` (op `CreateWindow`), with its shell.
    fn create_frame(&self, win: WindowId, bounds: Option<Bounds>, maximized: bool) {
        let window = match self.first_window.take() {
            // The frame shown at start-up: put it where the session had it.
            Some(w) => {
                if let Some(b) = bounds {
                    w.move_to(ScreenRect { x: b.x, y: b.y, width: b.width, height: b.height });
                }
                if maximized {
                    w.show(true);
                }
                w
            }
            None if self.adopt_spare(win, bounds, maximized) => return,
            None => {
                let rect = bounds.map(|b| ScreenRect { x: b.x, y: b.y, width: b.width, height: b.height });
                match Window::create_frame("Krunk", rect, false) {
                    Ok(w) => {
                        if self.tearing.get() {
                            w.set_transitions(false);
                        }
                        w.show(maximized);
                        w
                    }
                    Err(e) => {
                        warning!("window {win}: {e}");
                        return;
                    }
                }
            }
        };
        let frame = Rc::new(Frame {
            id: win,
            window,
            shell: RefCell::new(None),
            hit_map: RefCell::new(None),
            active: Cell::new(true),
            bar: Cell::new(false),
            last_zoom: Cell::new(1.0),
        });
        window.set_events(Rc::new(FrameEvents { b: self.me.clone(), win }));
        self.frames.borrow_mut().push(frame.clone());
        self.apply_frame_theme();
        if let Err(e) = self.create_shell(&frame) {
            warning!("window {win}: {e}");
        }
    }

    fn create_shell(&self, frame: &Frame) -> Result<(), String> {
        let shell = self.new_shell(frame.window, Rc::new(Cell::new(Some(frame.id))))?;
        shell.set_visible(true);
        *frame.shell.borrow_mut() = Some(shell);
        self.layout();
        Ok(())
    }

    /// A shell view in `window`, for the frame whose id `id` holds (none yet: the spare frame).
    fn new_shell(&self, window: Window, id: Rc<Cell<Option<WindowId>>>) -> Result<View, String> {
        let shell = View::new(ViewOptions {
            role: Role::Shell,
            private: false,
            scripts: Scripts::default(),
            background: if self.frame_dark() { UI_BACKGROUND_DARK } else { UI_BACKGROUND },
            policy: None,
        });
        shell.on_before_browse(|nav| assets::allow_navigation(Channel::Shell, "", &nav.url, true));
        let weak = self.me.clone();
        shell.on_load_finished(move |_| {
            if let Some(b) = weak.upgrade() {
                b.probe_engine_hints();
            }
        });
        let weak = self.me.clone();
        let win = id.clone();
        shell.on_open_tab(move |url, _| {
            // Links in the shell never open windows; open them as tabs instead.
            if let (Some(b), Some(win)) = (weak.upgrade(), win.get()) {
                b.make_current(win);
                b.open_url(&url, true, false);
            }
        });
        let weak = self.me.clone();
        let win = id.clone();
        shell.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                match win.get() {
                    Some(win) => b.on_ui_message(Channel::Shell, Some(win), source, json),
                    None => b.spare_message(json),
                }
            }
        });
        let weak = self.me.clone();
        let win = id.clone();
        shell.on_crashed(move |reason| {
            let Some(b) = weak.upgrade() else { return };
            match win.get() {
                Some(win) => {
                    warning!("window {win}: the shell's renderer ended ({reason}); reloading it");
                    if let Some(s) = b.frame(win).and_then(|f| f.shell.borrow().clone()) {
                        s.navigate(SHELL_URL);
                    }
                }
                None => b.spare_crashed(),
            }
        });
        let weak = self.me.clone();
        let win = id.clone();
        shell.on_key(move |key| {
            weak.upgrade().zip(win.get()).is_some_and(|(b, win)| {
                b.make_current(win);
                b.on_key(key)
            })
        });
        // The spare frame's shell never takes the focus: its window is hidden, and the engine
        // focuses a browser that has loaded (0.11.1: the keyboard went to the hidden window).
        let spare = id.get().is_none();
        shell.on_set_focus(move |_| id.get().is_some());
        self.hook_focus(&shell, None);
        let started =
            if spare { shell.start_inactive(window.hwnd(), SHELL_URL) } else { shell.start(window.hwnd(), SHELL_URL) };
        started.map_err(|e| format!("Cannot create the browser window: {e}"))?;
        Ok(shell)
    }

    /// A view getting the focus closes the popup. The exception is the suggestion dropdown
    /// of tab `tab` itself: the engine reports the focus again when it moves into a frame of
    /// another process (a cross-origin iframe), right when that frame's field opened it.
    fn hook_focus(&self, c: &View, tab: Option<TabId>) {
        let weak = self.me.clone();
        c.on_got_focus(move || {
            if let Some(b) = weak.upgrade() {
                if tab.is_some()
                    && matches!(b.popup_kind(), Some(PopupKind::Suggest | PopupKind::CardFill))
                    && b.suggest_open_tab() == tab
                {
                    return;
                }
                b.hide_popup_by_focus_loss();
                // A click into the page (0.7.0, R5): the address bar of its window gives up its
                // selection and shows the page's address again.
                if let Some(id) = tab {
                    if let Some(win) = b.window_of(id) {
                        b.post_shell_to(win, &Event::BlurAddressBar);
                    }
                }
            }
        });
    }

    /// Keys typed into tab `id` (in whichever window it is now).
    fn hook_tab_keys(&self, id: TabId, c: &View) {
        let weak = self.me.clone();
        c.on_key(move |key| {
            weak.upgrade().is_some_and(|b| {
                if let Some(win) = b.window_of(id) {
                    b.make_current(win);
                }
                b.on_key(key)
            })
        });
    }

    /// Light/dark preference for web content: `None` follows Windows.
    fn color_scheme(&self) -> Option<bool> {
        match self.settings.borrow().theme {
            lb_core::settings::Theme::System => None,
            lb_core::settings::Theme::Light => Some(false),
            lb_core::settings::Theme::Dark => Some(true),
        }
    }

    fn restore_or_open_start(&self) {
        if let Ok(Some(json)) = self.store.borrow().kv_get(DOWNLOADS_KEY) {
            *self.downloads.borrow_mut() = Downloads::from_json(&json);
            *self.stopped_by_close.borrow_mut() = (now_ms(), self.downloads.borrow().stopped_by_close());
            self.remove_leftover_parts();
        }
        let (startup, homepage) = {
            let s = self.settings.borrow();
            (s.startup, s.homepage.clone())
        };
        // Version 2 (every window); a 0.2.x session reads as one window.
        let saved: Option<SessionV2> =
            self.store.borrow().kv_get(SESSION_KEY).ok().flatten().and_then(|j| serde_json::from_str(&j).ok());
        let now = now_ms();
        let areas: Vec<layout::Rect> = crate::ffi::window::monitor_work_areas()
            .into_iter()
            .map(|a| layout::Rect { x: a.x, y: a.y, width: a.width, height: a.height })
            .collect();
        let fit = |b: Bounds| {
            let r = layout::fit_to_monitors(layout::Rect { x: b.x, y: b.y, width: b.width, height: b.height }, &areas);
            Bounds { x: r.x, y: r.y, width: r.width, height: r.height }
        };
        let ops = {
            let mut ws = self.ws.borrow_mut();
            let restored = match (startup, &saved) {
                (Startup::RestoreSession, Some(session)) => ws.restore(session, now, fit),
                _ => Vec::new(),
            };
            if !restored.is_empty() {
                info!("session restored: {} window(s)", ws.len());
                restored
            } else if startup == Startup::Homepage && !homepage.is_empty() {
                ws.open_window(&homepage, false, None, false, now).2
            } else {
                ws.open_window("", false, None, false, now).2
            }
        };
        // Restored tabs show their favicons from the local cache (no network).
        {
            let store = self.store.borrow();
            let mut ws = self.ws.borrow_mut();
            let hosts: Vec<(TabId, String)> =
                ws.tabs().filter_map(|(_, t)| urlclean::host_of(&t.url).map(|h| (t.id, h))).collect();
            for (id, host) in hosts {
                if let Ok(Some(png)) = store.favicon(&host) {
                    ws.with_tab(id, |m, t| m.set_favicon(t, assets::png_data_url(&png)));
                }
            }
        }
        self.apply(ops);
        // The window used last was restored last; make sure it is in front.
        if let Some(f) = self.cur() {
            f.window.bring_to_front();
        }
    }

    // ------------------------------------------------------------ effects

    fn apply(&self, ops: Vec<Op>) {
        let mut exit = false;
        for op in ops {
            match op {
                Op::CreateWindow { win, bounds, maximized } => self.create_frame(win, bounds, maximized),
                Op::CreateView { win, tab } => self.create_view(win, tab),
                Op::DestroyView { tab, .. } => self.destroy_view(tab),
                Op::Activate { win, from, to } => self.activate_view(win, from, to),
                Op::MoveView { tab, from, to } => self.move_view(tab, from, to),
                Op::CloseWindow(win) => self.close_frame(win),
                Op::Exit => exit = true,
            }
        }
        self.send_tabs();
        self.send_nav();
        // Saved a moment later (0.11.1): a tear-off or a drop doesn't wait for the database.
        // Exit saves right away.
        if exit {
            self.save_session();
            self.begin_exit();
        } else {
            self.schedule_session_save();
        }
    }

    /// Runs a change of the current window's tab model and applies its effects.
    fn update_current(&self, f: impl FnOnce(&mut TabModel) -> Vec<Effect>) {
        let Some(win) = self.cur_id() else { return };
        let ops = self.ws.borrow_mut().update(win, |m| ((), f(m))).map(|(_, ops)| ops).unwrap_or_default();
        self.apply(ops);
    }

    /// Moves a live tab's view (and anything docked to it) into another frame.
    fn move_view(&self, tab: TabId, from: WindowId, to: WindowId) {
        let (Some(view), Some(target)) = (self.tab_view(tab), self.frame(to)) else { return };
        if view.reparent(target.window.hwnd()) {
            info!("tab {tab}: moved from window {from} to window {to}");
        } else {
            warning!("tab {tab}: could not move its view to window {to}");
        }
    }

    /// Closes a frame whose tabs are gone (closed or moved away).
    fn close_frame(&self, win: WindowId) {
        let frame = {
            let mut frames = self.frames.borrow_mut();
            let i = frames.iter().position(|f| f.id == win);
            i.map(|i| frames.remove(i))
        };
        let Some(frame) = frame else { return };
        // Krunk's popups (menus, dropdowns, prompts) are owned by the window that last showed
        // them, and Windows destroys owned windows with their owner. Before 0.11.1 a window
        // closed by a tab merge took the popup with it, and from then on no menu opened in any
        // window (the operator saw ⋮ and every context menu go dead while moving tabs). They
        // go to a window that stays first.
        self.rehome_popups(frame.window);
        if let Some(s) = frame.shell.borrow_mut().take() {
            s.close();
        }
        frame.window.destroy();
        info!("window {win} closed");
    }

    /// The popups owned by `closing` (a frame about to be destroyed) are hidden and owned by
    /// another frame, or by the hidden application window when none is left.
    fn rehome_popups(&self, _closing: Window) {
        self.hide_menu_popup(false);
        self.hide_popup();
        let heir = self.all_frames().into_iter().map(|f| (Some(f.id), f.window)).next();
        let Some((owner, window)) = heir.or_else(|| self.app().map(|a| (None, a))) else { return };
        for slot in [PopupSlot::Main, PopupSlot::Menu] {
            if let Ok(mut guard) = self.popup_slot(slot).try_borrow_mut() {
                if let Some(p) = guard.as_mut().filter(|p| p.window.exists()) {
                    p.window.set_owner(window);
                    p.owner = owner;
                }
            }
        }
    }

    /// A popup window was destroyed: unless Krunk is closing, its slot empties (the window and
    /// its view are gone) and the next popup creates a new one.
    fn popup_window_gone(&self, slot: PopupSlot) {
        let gone =
            self.popup_slot(slot).try_borrow().ok().is_some_and(|p| p.as_ref().is_some_and(|p| !p.window.exists()));
        if gone && !self.session_final.get() {
            warning!("popup: its window was destroyed; a new one is made for the next popup");
            if let Some(p) = self.popup_slot(slot).borrow_mut().take() {
                if let Some(c) = p.view {
                    c.close();
                }
            }
        }
    }

    /// The first page of a tab that started blank is there: the blank entry before it goes from
    /// the tab's history (Back is not offered for it, as in Chrome).
    fn forget_blank_start(&self, id: TabId) {
        let view = self.views.borrow().get(&id).filter(|v| v.started_blank.replace(false)).map(|v| v.view.clone());
        if let Some(v) = view {
            v.devtools_call("Page.resetNavigationHistory", &serde_json::json!({}), move |r| {
                if r.is_err() {
                    warning!("tab {id}: the blank first entry could not be removed from its history");
                }
            });
        }
    }

    /// A new (not started) tab view, with its network state.
    fn new_tab_view(&self, private: bool) -> (View, Rc<TabNet>) {
        let net = Rc::new(TabNet::new(self.net.clone()));
        let scripts = {
            let s = self.settings.borrow();
            #[cfg_attr(not(feature = "perf-probe"), allow(unused_mut))]
            let mut scripts = Scripts {
                gpc: s.send_gpc,
                dnt: s.send_dnt,
                page_tokens: s.adblock_enabled,
                login: true,
                audio: true,
                pointer_lock: true,
                notify: true,
                media: true,
            };
            #[cfg(feature = "perf-probe")]
            perf_probe::apply(&mut scripts);
            scripts
        };
        let view = View::new(ViewOptions {
            role: Role::Tab,
            private,
            scripts,
            background: self.tab_background(),
            policy: Some(net.policy()),
        });
        (view, net)
    }

    fn create_view(&self, win: WindowId, id: TabId) {
        let Some(window) = self.frame(win).map(|f| f.window) else { return };
        let (url, private, active) = {
            let ws = self.ws.borrow();
            let Some(m) = ws.model(win) else { return };
            match m.get(id) {
                Some(t) if t.is_live() => (t.url.clone(), t.private, m.active_id() == Some(id)),
                _ => return,
            }
        };
        let adopted = self.adopted.borrow_mut().remove(&id);
        let popup = adopted.is_some();
        let (view, net) = adopted.unwrap_or_else(|| self.new_tab_view(private));
        let host_navigation = Rc::new(Cell::new(false));
        let audio = Rc::new(Cell::new(false));
        let audio2 = audio.clone();
        let weak_audio = self.me.clone();
        view.on_audio(move |playing| {
            info!("tab {id}: audio {}", if playing { "playing" } else { "stopped" });
            audio2.set(playing);
            if let Some(b) = weak_audio.upgrade() {
                b.send_tab_update(id);
            }
        });
        self.hook_tab(id, &view, host_navigation.clone(), net.clone());
        self.hook_blocking(&view, net.clone());
        self.ext_attach_tab(id, &view);
        self.hook_downloads(&view, Some(id));
        // A popup's browser is being created by the engine already.
        if !popup {
            host_navigation.set(true);
            // With edited headers the tab starts blank and navigates once the engine has
            // acknowledged the override, so even the first request carries it.
            let first = if self.headers_overridden() {
                self.first_url.borrow_mut().insert(id, url.clone());
                "about:blank".to_string()
            } else {
                url.clone()
            };
            if let Err(e) = view.start(window.hwnd(), &first) {
                warning!("tab {id}: {e}");
            }
        }
        // The active tab's new view is shown at once only if nothing else shows in the window;
        // otherwise the tab switch that follows keeps the old page until this one has loaded
        // (R7: a new view's first frame is blank).
        let other_shown =
            self.views.borrow().iter().any(|(t, v)| *t != id && self.window_of(*t) == Some(win) && v.view.is_visible());
        view.set_visible(active && !window.is_minimized() && !other_shown);
        self.views.borrow_mut().insert(
            id,
            TabView {
                view,
                host_navigation,
                can_go_back: false,
                can_go_forward: false,
                find_query: String::new(),
                net,
                stage: Cell::new(lb_core::memory::Stage::Awake),
                stage_since: Cell::new(now_ms()),
                audio,
                media_use: Cell::new((false, false)),
                downloads: Rc::new(Cell::new(0)),
                devtools: Cell::new(false),
                loaded_once: Cell::new(false),
                blank_start: Cell::new(!popup && !lb_core::pages::is_blank(&url)),
                unresponsive: Cell::new(false),
                started_blank: Cell::new(self.first_url.borrow().contains_key(&id)),
                hang_quiet_until: Cell::new(0),
                last_favicon: Cell::new(0),
            },
        );
        if active {
            self.layout();
            if self.cur_id() == Some(win) {
                self.focus_active();
            }
        }
        self.send_nav();
    }

    fn hook_tab(&self, id: TabId, c: &View, host_navigation: Rc<Cell<bool>>, net: Rc<TabNet>) {
        self.hook_focus(c, Some(id));
        self.hook_tab_keys(id, c);
        self.hook_permissions(id, c);

        let weak = self.me.clone();
        let net_start = net.clone();
        c.on_before_browse(move |nav| {
            let Some(b) = weak.upgrade() else { return false };
            let host = host_navigation.replace(false);
            let current = b.tab(id).map(|t| t.url.clone()).unwrap_or_default();
            let allow = assets::allow_navigation(Channel::Tab, &current, &nav.url, host);
            if !allow {
                info!("tab {id}: blocked navigation to an internal or unsupported URL");
                return false;
            }
            if let Some(rewritten) = b.navigation_starting(&net_start, &nav.url, nav.redirected) {
                // Cancel and restart at the cleaned/upgraded URL.
                b.defer(move |b| b.navigate_tab(id, &rewritten));
                return false;
            }
            b.with_tab(id, |m, t| m.set_loading(t, true));
            b.tab_changed(id);
            // A new page: the old one's pointer lock ends.
            b.release_pointer_of(id, lb_core::pointer_lock::Release::Navigated);
            true
        });
        let weak = self.me.clone();
        c.on_loading(move |state: LoadingState| {
            if let Some(b) = weak.upgrade() {
                if let Some(v) = b.views.try_borrow_mut().ok().as_mut().and_then(|v| v.get_mut(&id)) {
                    v.can_go_back = state.can_go_back;
                    v.can_go_forward = state.can_go_forward;
                }
                b.with_tab(id, |m, t| m.set_loading(t, state.loading));
                b.tab_changed(id);
                if !state.loading {
                    b.address_focus_loaded(id);
                    b.first_load_done(id);
                }
            }
        });
        let weak = self.me.clone();
        c.on_set_focus(move |from_navigation| weak.upgrade().is_none_or(|b| b.tab_wants_focus(id, from_navigation)));
        let weak = self.me.clone();
        let net_done = net.clone();
        c.on_load_finished(move |error| {
            if let Some(b) = weak.upgrade() {
                b.ext_tab_event("tabUpdated", serde_json::json!({ "tabId": id }));
                b.navigation_completed(id, &net_done, error.map(LoadFailure::from_net_error));
                // Once per top-level navigation, when it loaded.
                if net_done.visit_pending.replace(false) && error.is_none() {
                    b.record_visit(id);
                }
                b.tab_changed(id);
            }
        });
        let weak = self.me.clone();
        c.on_address(move |url| {
            if let Some(b) = weak.upgrade() {
                // The engine's empty first document of a tab on its way to another page (edited
                // headers start blank) is not the tab's address: the bar never shows it (R1).
                if b.views.borrow().get(&id).is_some_and(|v| v.blank_start.get()) {
                    if lb_core::pages::is_blank(&url) {
                        return;
                    }
                    if let Some(v) = b.views.borrow().get(&id) {
                        v.blank_start.set(false);
                    }
                    b.forget_blank_start(id);
                }
                b.release_address_focus_unless(id, &url);
                b.hang_navigated(id);
                b.with_tab(id, |m, t| m.set_url(t, &url));
                b.apply_site_mute(id, &url);
                b.apply_notification_setting(id, &url);
                b.tab_changed(id);
            }
        });
        let weak = self.me.clone();
        c.on_title(move |title| {
            if let Some(b) = weak.upgrade() {
                b.with_tab(id, |m, t| m.set_title(t, &title));
                b.record_title(id);
                b.tab_changed(id);
            }
        });
        let weak = self.me.clone();
        c.on_status(move |text| {
            if let Some(b) = weak.upgrade() {
                b.status_message(id, text);
            }
        });
        let weak = self.me.clone();
        c.on_favicon(move |png| {
            if let Some(b) = weak.upgrade() {
                let key = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    b.tab(id).and_then(|t| urlclean::host_of(&t.url)).hash(&mut h);
                    png.hash(&mut h);
                    h.finish()
                };
                if b.views.borrow().get(&id).is_some_and(|v| v.last_favicon.replace(key) == key) {
                    return;
                }
                b.with_tab(id, |m, t| m.set_favicon(t, assets::png_data_url(&png)));
                b.cache_favicon(id, &png);
                b.send_tab_update(id);
            }
        });
        let weak = self.me.clone();
        c.on_new_window(move |nw| weak.upgrade().and_then(|b| b.open_new_window(id, nw)));
        let weak = self.me.clone();
        c.on_open_tab(move |url, foreground| {
            if let Some(b) = weak.upgrade() {
                b.open_link_in_tab(id, &url, foreground);
            }
        });
        let weak = self.me.clone();
        c.on_find_result(move |active, total| {
            if let Some(b) = weak.upgrade() {
                let win = b.window_of(id);
                if let Some(win) = win.filter(|w| b.ws.borrow().model(*w).is_some_and(|m| m.active_id() == Some(id))) {
                    b.post_shell_to(win, &Event::FindResult { active, total });
                }
            }
        });
        let weak = self.me.clone();
        c.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                b.on_tab_message(id, source, json);
            }
        });
        let weak = self.me.clone();
        c.on_created(move || {
            if let Some(b) = weak.upgrade() {
                b.ext_tab_created(id);
                b.hints_attach(id);
                let first = b.first_url.borrow_mut().remove(&id);
                b.apply_user_agent_to_then(id, move |b| {
                    if let Some(url) = first {
                        b.navigate_tab(id, &url);
                    }
                });
                if let (Some(dark), Some(v)) = (b.color_scheme(), b.tab_view(id)) {
                    v.set_color_scheme(Some(dark));
                }
                // The view was made with the theme's colour (what shows before the page has
                // painted, R7, and while it is resized, R6); a page without a background of its
                // own still gets white.
                if let Some(v) = b.tab_view(id).filter(|v| v.background() != PAGE_DEFAULT_WHITE) {
                    v.devtools_method(
                        "Emulation.setDefaultBackgroundColorOverride",
                        &serde_json::json!({ "color": { "r": 255, "g": 255, "b": 255, "a": 1 } }),
                    );
                }
            }
        });
        let weak = self.me.clone();
        c.on_auth(move |challenge, responder| {
            if let Some(b) = weak.upgrade() {
                b.http_auth_challenge(id, challenge, responder);
            }
        });
        let weak = self.me.clone();
        c.on_load_end(move |status| {
            if let Some(b) = weak.upgrade() {
                b.http_auth_load_end(id, status);
            }
        });
        let weak = self.me.clone();
        c.on_crashed(move |reason| {
            warning!("tab {id}: renderer ended ({reason})");
            if let Some(b) = weak.upgrade() {
                b.release_pointer_of(id, lb_core::pointer_lock::Release::RendererGone);
                b.fullscreen_tab_gone(id);
                // Not inside the engine's callback: the crash page is a new navigation.
                b.defer(move |b| b.show_crashed_page(id));
            }
        });
        let weak = self.me.clone();
        c.on_unresponsive(move |hung| {
            if let Some(b) = weak.upgrade() {
                b.defer(move |b| b.tab_unresponsive(id, hung));
            }
        });
        let weak = self.me.clone();
        c.on_pointer_lock(move |frame, json| {
            if let Some(b) = weak.upgrade() {
                b.pointer_message(id, frame, &json);
            }
        });
        let weak = self.me.clone();
        c.on_view_source(move |url| {
            if let Some(b) = weak.upgrade() {
                b.view_source(id, Some(url));
            }
        });
        let weak = self.me.clone();
        c.on_search_label(move |sel| weak.upgrade().and_then(|b| b.search_selection_label(sel)));
        let weak = self.me.clone();
        c.on_search_selection(move |sel| {
            if let Some(b) = weak.upgrade() {
                b.search_selection(id, &sel);
            }
        });
        let weak = self.me.clone();
        c.on_inspect(move |at| {
            if let Some(b) = weak.upgrade() {
                b.open_devtools(id, at);
            }
        });
        let weak = self.me.clone();
        c.on_devtools(move |open| {
            if let Some(b) = weak.upgrade() {
                b.devtools_changed(id, open);
            }
        });
        let weak = self.me.clone();
        c.on_fullscreen(move |on| {
            if let Some(b) = weak.upgrade() {
                b.page_fullscreen(id, on);
            }
        });
    }

    fn destroy_view(&self, id: TabId) {
        self.ext_detach_tab(id);
        self.hints_attached.borrow_mut().remove(&id);
        self.pointer.hints.borrow_mut().forget(id);
        if self.status.tab.get() == Some(id) {
            self.hide_status();
        }
        self.release_pointer_of(id, lb_core::pointer_lock::Release::TabClosed);
        self.fullscreen_tab_gone(id);
        if let Some((v, _)) = self.adopted.borrow_mut().remove(&id) {
            v.close();
        }
        let view = self.views.borrow_mut().remove(&id);
        if let Some(v) = view {
            v.view.close();
        }
    }

    fn popup_background(&self) -> u32 {
        if self.frame_dark() {
            POPUP_BACKGROUND_DARK
        } else {
            POPUP_BACKGROUND_LIGHT
        }
    }

    fn tab_background(&self) -> u32 {
        if self.frame_dark() {
            TAB_BACKGROUND_DARK
        } else {
            TAB_BACKGROUND_LIGHT
        }
    }

    /// Shows `to` on top, then hides `from` (and its DevTools).
    fn swap_views(&self, win: WindowId, from: Option<TabId>, to: TabId) {
        if let Some(t) = self.tab_view(to) {
            t.set_visible(!self.frame(win).is_some_and(|f| f.window.is_minimized()));
        }
        let Some(from) = from.filter(|f| *f != to) else { return };
        // The old page is hidden a moment later (0.7.3): the engine presents the new view's
        // first frame after it is shown (on a GPU that can be a frame late), and until then
        // what shows through it must be the old page beneath, not the window's background.
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(HIDE_OLD_VIEW_MS, move || {
            let Some(b) = weak.upgrade() else { return };
            // Switched back to it in the meantime: it stays.
            let active_again = b
                .window_of(from)
                .is_some_and(|w| b.ws.borrow().model(w).and_then(|m| m.active().map(|t| t.id)) == Some(from));
            if active_again {
                return;
            }
            if let Some(f) = b.tab_view(from) {
                f.set_visible(false);
                // Its DevTools waits, hidden, until the tab is shown again.
                if f.devtools_open() {
                    f.set_devtools_visible(false);
                }
            }
        });
    }

    /// The tab a switch waited for has loaded (or waited long enough): swap the views now.
    fn reveal(&self, to: TabId) {
        let found = self.pending_reveal.borrow_mut().take(to);
        let Some((win, from)) = found else { return };
        // Still the window's active tab (else a later switch has taken over).
        if self.ws.borrow().model(win).and_then(|m| m.active().map(|t| t.id)) == Some(to) {
            let from = Some(from).filter(|f| self.window_of(*f) == Some(win));
            self.swap_views(win, from, to);
        }
    }

    /// A tab's view painted its first content, or finished a load: ready to be shown.
    fn first_load_done(&self, id: TabId) {
        let first = self.views.borrow().get(&id).is_some_and(|v| !v.loaded_once.replace(true));
        if first && self.pending_reveal.borrow().is_waiting(id) {
            // A frame for the compositor to present the page's first paint.
            let weak = self.me.clone();
            crate::ffi::cef::post_ui_delayed(REVEAL_SETTLE_MS, move || {
                if let Some(b) = weak.upgrade() {
                    b.reveal(id);
                }
            });
        }
    }

    fn activate_view(&self, win: WindowId, from: Option<TabId>, to: TabId) {
        self.close_ext_popup();
        self.auto_pip_switch(from, to);
        self.ext_tab_event("tabActivated", serde_json::json!({ "tabId": to, "windowId": win }));
        self.pointer_tab_activated(to);
        self.hide_status();
        if let Some(f) = from.filter(|f| *f != to) {
            self.fullscreen_tab_gone(f);
        }
        if self.address_focus.get().is_some_and(|held| held != to) {
            self.address_focus.set(None);
        }
        self.hide_popup();
        if let Some(f) = from {
            self.stop_find(f);
        }
        // No gap between the two pages (0.7.0, R7): the new view gets its place while still
        // hidden, is shown on top of the old one, and only then is the old one hidden. (Before,
        // hiding first showed the frame's background for a moment, and the new view was sized
        // after it appeared.) A view that has not loaded once yet (a discarded tab coming back,
        // a new one) would show a blank frame first: the old page stays until it has.
        self.layout();
        let old = from.filter(|f| *f != to && self.window_of(*f) == Some(win));
        let minimized = self.frame(win).is_some_and(|f| f.window.is_minimized());
        let ready = self.views.borrow().get(&to).is_some_and(|v| v.loaded_once.get());
        // A reveal this window was still waiting for: the page on screen is its old one.
        let shown = self.pending_reveal.borrow_mut().begin(win, old, to);
        let old = shown.filter(|f| *f != to && self.window_of(*f) == Some(win)).or(old);
        match old {
            Some(f) if !ready && !minimized && self.tab_view(f).is_some() => {
                self.pending_reveal.borrow_mut().wait(win, f, to);
                let weak = self.me.clone();
                crate::ffi::cef::post_ui_delayed(REVEAL_TIMEOUT_MS, move || {
                    if let Some(b) = weak.upgrade() {
                        b.reveal(to);
                    }
                });
            }
            _ => self.swap_views(win, old, to),
        }
        // Wake after showing: a frozen page resumes as a visible one.
        self.wake_tab(to);
        // A page that does not respond says so when its tab comes to the front (0.11.0).
        self.defer(move |b| b.show_hang_prompt(to));
        self.layout();
        if self.cur_id() == Some(win) {
            self.focus_active();
        }
        self.update_title();
        self.send_devtools_state(win);
    }

    fn tab_view(&self, id: TabId) -> Option<View> {
        self.views.try_borrow().ok()?.get(&id).map(|v| v.view.clone())
    }

    /// The current window's active tab view.
    fn active_view(&self) -> Option<View> {
        let id = self.active_id()?;
        self.tab_view(id)
    }

    /// The active tab view of window `win`.
    fn active_view_of(&self, win: WindowId) -> Option<View> {
        let id = self.ws.try_borrow().ok()?.model(win)?.active_id()?;
        self.tab_view(id)
    }

    /// The current window's shell.
    fn shell(&self) -> Option<View> {
        self.cur()?.shell.try_borrow().ok()?.clone()
    }

    /// New tab pages put the caret in the address bar; other pages get focus.
    fn focus_active(&self) {
        let is_new_tab = self.active_tab().is_some_and(|t| Page::from_url(&t.url) == Some(Page::NewTab));
        if is_new_tab {
            self.focus_address_bar();
        } else if let Some(c) = self.active_view() {
            c.focus();
        }
    }

    fn focus_address_bar(&self) {
        if let Some(s) = self.shell() {
            s.focus();
            s.post_json(&Event::FocusAddressBar.to_json());
        }
    }

    /// Opens the find bar with keyboard focus in it: the shell view must
    /// get focus first, or typing still goes to the page.
    pub(super) fn show_find_bar(&self) {
        if let Some(s) = self.shell() {
            s.focus();
            s.post_json(&Event::ShowFind.to_json());
        }
    }

    // ------------------------------------------------------------ layout & state

    /// Lays out every frame: its shell on top and its active tab below (the whole client
    /// area in full screen).
    fn layout(&self) {
        for f in self.all_frames() {
            self.layout_frame(&f);
            // The DevTools strip and splitter are painted by the frame itself.
            f.window.invalidate();
        }
    }

    /// A frame's shell and content areas; in full screen there is no shell and the content
    /// is the whole client area.
    fn frame_areas(&self, f: &Frame) -> (Option<layout::Rect>, layout::Rect) {
        let w = f.window;
        let (width, height) = w.client_size();
        if self.is_fullscreen_frame(f.id) {
            return (None, layout::Rect { x: 0, y: 0, width, height });
        }
        let bar = self.bar_shown(f.id);
        f.bar.set(bar);
        let (shell, content) = layout::split(width, height, w.scale(), bar);
        (Some(shell), content)
    }

    fn layout_frame(&self, f: &Frame) {
        engine::with_batched_moves(|| self.layout_frame_now(f));
    }

    fn layout_frame_now(&self, f: &Frame) {
        let (shell, content) = self.frame_areas(f);
        if let (Some(shell), Some(s)) = (shell, f.shell.borrow().as_ref()) {
            s.set_bounds(shell.x, shell.y, shell.width, shell.height);
        }
        let Some(c) = self.active_view_of(f.id) else { return };
        match self.devtools_split_of(f) {
            Some(split) => {
                let (p, d) = (split.page, split.devtools);
                c.set_bounds(p.x, p.y, p.width, p.height);
                c.set_devtools_bounds(d.x, d.y, d.width, d.height);
                c.set_devtools_visible(!f.window.is_minimized());
            }
            None => {
                c.set_bounds(content.x, content.y, content.width, content.height);
                // Page full screen: the page only.
                if c.devtools_open() {
                    c.set_devtools_visible(false);
                }
            }
        }
    }

    fn tab_changed(&self, id: TabId) {
        self.schedule_session_save();
        let Some(win) = self.window_of(id) else { return };
        self.send_tab_update(id);
        if self.ws.borrow().model(win).is_some_and(|m| m.active_id() == Some(id)) {
            self.send_nav_to(win);
            self.update_title();
        }
    }

    /// Each frame's title is its active tab's.
    fn update_title(&self) {
        for f in self.all_frames() {
            let title =
                self.ws.borrow().model(f.id).and_then(|m| m.active().map(|t| t.title.clone())).unwrap_or_default();
            f.window.set_title(&if title.is_empty() { "Krunk".to_string() } else { format!("{title} – Krunk") });
        }
    }

    /// Sends an event to every frame's shell (settings, bookmarks bar, downloads …).
    fn post_shell(&self, e: &Event) {
        let json = e.to_json();
        if !self.spare_only.get() {
            for f in self.all_frames() {
                if let Some(s) = f.shell.borrow().as_ref() {
                    s.post_json(&json);
                }
            }
        }
        // The spare frame's shell keeps up, so that it is complete when a window takes it.
        self.post_spare(&json);
    }

    /// Sends an event to one frame's shell.
    fn post_shell_to(&self, win: WindowId, e: &Event) {
        if let Some(s) = self.frame(win).and_then(|f| f.shell.borrow().clone()) {
            s.post_json(&e.to_json());
        }
    }

    /// Every frame's tab strip.
    fn send_tabs(&self) {
        for f in self.all_frames() {
            self.send_tabs_to(f.id);
        }
    }

    /// One tab's entry in its window's strip (title, address, icon, loading, sound).
    fn send_tab_update(&self, id: TabId) {
        let Some(win) = self.window_of(id) else { return };
        let info = self.ws.try_borrow().ok().and_then(|ws| ws.model(win).and_then(|m| m.info(id)));
        if let Some(mut tab) = info {
            if let Some(v) = self.views.try_borrow().ok().as_ref().and_then(|v| v.get(&id)) {
                tab.audible = v.audio.get();
                tab.muted = v.view.audio_muted();
            }
            self.post_shell_to(win, &Event::TabUpdate { tab });
            self.refresh_tab_list(win);
        }
    }

    fn send_tabs_to(&self, win: WindowId) {
        let state = self.ws.try_borrow().ok().and_then(|ws| ws.model(win).map(|m| (m.infos(), m.active_id())));
        if let Some((mut tabs, active)) = state {
            // Sound and mute state live with the views, not the model.
            if let Ok(views) = self.views.try_borrow() {
                for t in tabs.iter_mut() {
                    if let Some(v) = views.get(&t.id) {
                        t.audible = v.audio.get();
                        t.muted = v.view.audio_muted();
                    }
                }
            }
            self.post_shell_to(win, &Event::Tabs { tabs, active });
            self.refresh_tab_list(win);
        }
    }

    /// The installed copy keeps its app identity registered (0.11.0, the media overlay's name
    /// and icon) and its shortcuts on the same ID; a portable copy changes nothing.
    fn init_app_identity(&self) {
        if !crate::ffi::process::is_installed() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        self.spawn_job(
            "app-identity",
            move || crate::ffi::appid::register_installed(&exe),
            |_, done: Vec<String>| {
                if !done.is_empty() {
                    info!("app identity: {} set on {}", crate::ffi::appid::APP_ID, done.join(", "));
                }
            },
        );
    }

    /// Is `origin` muted ("Mute site": kind `sound` in the site permissions)?
    fn site_muted(&self, origin: &str) -> bool {
        self.store.try_borrow().ok().and_then(|s| s.permission(origin, "sound").ok().flatten()) == Some(false)
    }

    /// "Mute site" / "Unmute site": remembered for the site, applied to all its tabs now and
    /// to every later navigation to it.
    fn mute_site(&self, id: TabId, mute: bool) {
        let Some(origin) = self.tab(id).and_then(|t| urlclean::origin_of(&t.url)) else { return };
        let r = if mute {
            self.store.borrow().permission_set(&origin, "sound", false, now_ms())
        } else {
            self.store.borrow().permission_remove(&origin, "sound").map(|_| ())
        };
        if let Err(e) = r {
            warning!("mute site: {e}");
        }
        info!("site {origin}: {}", if mute { "muted" } else { "unmuted" });
        let ids: Vec<TabId> = self.views.borrow().keys().copied().collect();
        for tid in ids {
            if self.tab(tid).and_then(|t| urlclean::origin_of(&t.url)).as_deref() == Some(origin.as_str()) {
                if let Some(v) = self.tab_view(tid) {
                    v.set_audio_muted(mute);
                }
            }
        }
        for f in self.all_frames() {
            self.send_tabs_to(f.id);
        }
    }

    /// A tab navigated: a muted site stays muted, another site's tab is unmuted.
    fn apply_site_mute(&self, id: TabId, url: &str) {
        let Some(origin) = urlclean::origin_of(url) else { return };
        let muted = self.site_muted(&origin);
        if let Some(v) = self.tab_view(id) {
            if v.audio_muted() != muted {
                v.set_audio_muted(muted);
            }
        }
    }

    /// Right-click on the caption or the empty strip: Krunk's window menu at the cursor.
    fn show_caption_menu(&self, win: WindowId, sx: i32, sy: i32) {
        let Some(f) = self.frame(win) else { return };
        self.make_current(win);
        let scale = f.window.scale();
        let (ox, oy) = f.window.client_to_screen(0, 0);
        let rect = PopupRect {
            x: (f64::from(sx - ox) / scale).max(0.0),
            y: (f64::from(sy - oy) / scale).max(0.0),
            width: 250.0,
            height: 352.0,
        };
        let can_bookmark_all = self
            .ws
            .borrow()
            .model(win)
            .is_some_and(|m| m.tabs().iter().any(|t| !t.private && lb_core::bookmarks::bookmarkable(&t.url)));
        let content = PopupContent::CaptionMenu {
            maximized: f.window.is_maximized(),
            can_reopen: self.ws.borrow().closed_count() > 0,
            can_bookmark_all,
        };
        self.show_popup(content, Some(rect));
    }

    fn nav_state(&self, win: WindowId) -> Option<NavState> {
        let ws = self.ws.try_borrow().ok()?;
        let m = ws.model(win)?;
        let tab = m.active()?;
        let views = self.views.try_borrow().ok()?;
        let view = views.get(&tab.id);
        // A page the engine refused for a certificate error shows an error page: no lock.
        let cert_error = self.tab_view(tab.id).is_some_and(|v| v.has_cert_error());
        let secure = tab.url.starts_with("https://") && !pages::is_pages_origin(&tab.url) && !cert_error;
        let bookmarked = self.store.try_borrow().ok().is_some_and(|s| s.is_bookmarked(&tab.url).unwrap_or(false));
        Some(NavState {
            tab_id: tab.id,
            url: pages::display_url(&tab.url),
            can_go_back: view.is_some_and(|v| v.can_go_back),
            can_go_forward: view.is_some_and(|v| v.can_go_forward),
            loading: tab.loading,
            secure,
            bookmarked,
            blocked: view.map(|v| v.net.blocked()).unwrap_or(0),
            site_allowed: view.is_some_and(|v| v.net.allowed()),
            zoom: {
                let z = view.map(|v| v.view.zoom()).unwrap_or(1.0);
                if let Some(f) = self.frame(win) {
                    f.last_zoom.set(z);
                }
                z
            },
            private: tab.private,
            login_form: view.is_some_and(|v| v.net.login_form.get()),
            bookmarks_bar: self.bar_shown(win),
            camera: view.is_some_and(|v| v.media_use.get().0),
            microphone: view.is_some_and(|v| v.media_use.get().1),
            videos: view.map(|v| v.net.videos.borrow().len() as u32).unwrap_or(0),
            pip: view.is_some_and(|v| v.net.videos.borrow().iter().any(|x| x.pip)),
        })
    }

    /// The zoom watcher's tick: pushes the toolbar state when the front window's active
    /// tab has a zoom the chip does not show yet.
    fn zoom_tick(&self) {
        let Some(win) = self.cur_id() else { return };
        let Some(f) = self.frame(win) else { return };
        if !f.window.is_foreground() {
            return;
        }
        let Some(c) = self.active_view_of(win) else { return };
        if (c.zoom() - f.last_zoom.get()).abs() > 1e-6 {
            self.send_nav_to(win);
        }
    }

    /// What the user typed (or pasted) into the address bar: an address or a search.
    pub(super) fn navigate_input(&self, input: &str) {
        self.address_suggest_close();
        let template = self.settings.borrow().search_template().to_string();
        let url = urlclean::input_to_url(input, &template);
        self.navigate_active(&url);
    }

    /// Every frame's toolbar (address, buttons, shield) for its active tab.
    fn send_nav(&self) {
        // A permission prompt waiting for its tab to be active.
        self.show_next_permission_prompt();
        for f in self.all_frames() {
            self.send_nav_to(f.id);
        }
    }

    fn send_nav_to(&self, win: WindowId) {
        if let Some(state) = self.nav_state(win) {
            // "Only on the new-tab page": the bar comes or goes with the active tab.
            let relayout = self.frame(win).filter(|f| f.bar.get() != state.bookmarks_bar);
            let json = Event::Nav { state }.to_json();
            if let Some(s) = self.frame(win).and_then(|f| f.shell.borrow().clone()) {
                s.post_json(&json);
            }
            self.last_nav.borrow_mut().insert(win, json);
            if let Some(f) = relayout {
                self.layout_frame(&f);
            }
        }
        self.send_devtools_state(win);
    }

    /// The toolbars whose state changed (0.10.0): blocked-request counts arrive for every tab,
    /// background ones too, and refreshed every window's toolbar several times a second.
    fn send_nav_changed(&self) {
        self.show_next_permission_prompt();
        for f in self.all_frames() {
            let Some(state) = self.nav_state(f.id) else { continue };
            let json = Event::Nav { state }.to_json();
            if self.last_nav.borrow().get(&f.id) != Some(&json) {
                self.send_nav_to(f.id);
            }
        }
    }

    /// The toolbar of `tab`'s window, if `tab` is the tab it shows.
    fn send_nav_if_active(&self, tab: TabId) {
        let Some(win) = self.window_of(tab) else { return };
        if self.ws.borrow().model(win).is_some_and(|m| m.active_id() == Some(tab)) {
            self.send_nav_to(win);
        }
    }

    fn send_shell_config(&self) {
        let e = {
            let s = self.settings.borrow();
            Event::ShellConfig {
                theme: s.theme,
                search_name: s.search_name(),
                appearance: s.appearance.clone(),
                colors: lb_core::color::ThemeColors::from_appearance(&s.appearance),
            }
        };
        self.post_shell(&e);
    }

    /// Window `win` moved: popups and hints close, its views learn their new place, and the
    /// session saves later.
    fn frame_moved(&self, win: WindowId) {
        self.hide_popup();
        self.hide_status();
        self.hide_hint();
        self.reapply_pointer();
        if let Some(s) = self.frame(win).and_then(|f| f.shell.borrow().clone()) {
            s.notify_moved();
        }
        if let Some(c) = self.active_view_of(win) {
            c.notify_moved();
        }
        self.schedule_session_save();
    }

    /// Saves the session shortly after the last change (titles and URLs
    /// change often while pages load).
    fn schedule_session_save(&self) {
        if let Some(w) = self.app() {
            w.set_timer(SESSION_TIMER, SESSION_DEBOUNCE_MS);
        }
    }

    /// Saves every window's tabs and place (session v2).
    fn save_session(&self) {
        if self.session_final.get() {
            return;
        }
        let frames = self.all_frames();
        let Ok(ws) = self.ws.try_borrow() else { return };
        if ws.is_empty() {
            return;
        }
        let session = ws.session(|win| match frames.iter().find(|f| f.id == win) {
            Some(f) => {
                let (r, maximized) = f.window.placement();
                (Some(Bounds { x: r.x, y: r.y, width: r.width, height: r.height }), maximized)
            }
            None => (None, false),
        });
        drop(ws);
        if let Ok(json) = serde_json::to_string(&session) {
            if let Ok(store) = self.store.try_borrow() {
                let _ = store.kv_set(SESSION_KEY, &json);
            }
        }
    }

    // ------------------------------------------------------------ tab operations

    fn new_tab(&self, url: Option<&str>, private: bool) {
        let Some(win) = self.cur_id() else { return };
        // Settings → New tabs: what a plain new tab opens, and where it goes.
        let (target, position) = {
            let s = self.settings.borrow();
            let target = match url {
                Some(u) => u.to_string(),
                None => s.new_tab_target().unwrap_or_default(),
            };
            let position = match s.new_tab_position {
                lb_core::settings::NewTabPosition::End => Position::End,
                lb_core::settings::NewTabPosition::AfterCurrent => self
                    .ws
                    .borrow()
                    .model(win)
                    .and_then(|m| m.active_id())
                    .map(Position::AfterOpener)
                    .unwrap_or(Position::End),
            };
            (target, position)
        };
        let Some((id, effects)) = self.ws.borrow_mut().open(win, &target, private, position, true, now_ms()) else {
            return;
        };
        // A new-tab page: typing goes to the address bar at once, and the
        // page loading must not take the focus away.
        if url.is_none() {
            self.address_focus.set(Some(id));
        }
        self.apply(effects);
        if url.is_none() {
            self.hold_address_focus(id);
        }
    }

    /// Closes a tab (the close button, Ctrl+W, middle click). A window's last tab closes the
    /// window; the last window's ends Krunk.
    fn close_tab(&self, id: TabId) {
        let ops = self.ws.borrow_mut().close_tab(id, now_ms());
        self.apply(ops);
    }

    /// "Close other tabs" / "Close tabs to the right" of tab `id`, in one change of its window.
    fn close_many(&self, id: TabId, which: CloseMany) {
        let Some(win) = self.window_of(id) else { return };
        let ops = self.ws.borrow_mut().update(win, |m| ((), m.close_many(id, which, now_ms()))).map(|(_, o)| o);
        if let Some(ops) = ops {
            info!(
                "tab {id}: closing {which:?} ({} views)",
                ops.iter().filter(|o| matches!(o, Op::DestroyView { .. })).count()
            );
            self.apply(ops);
        }
    }

    /// A page in tab `opener` opened a window: it becomes a tab next to the
    /// opener, and the engine creates its browser for the returned view.
    fn open_new_window(&self, opener: TabId, nw: &NewWindow) -> Option<(View, windows::Win32::Foundation::HWND)> {
        // The new tab goes into the opener's window.
        let win = self.window_of(opener)?;
        let window = self.frame(win)?.window;
        let private = self.tab(opener).is_some_and(|t| t.private);
        let (id, effects) =
            self.ws.borrow_mut().open(win, &nw.url, private, Position::AfterOpener(opener), nw.foreground, now_ms())?;
        let (view, net) = self.new_tab_view(private);
        // The CreateView effect picks this view up and hooks it.
        self.adopted.borrow_mut().insert(id, (view.clone(), net));
        self.apply(effects);
        Some((view, window.hwnd()))
    }

    /// A link opened "in a new tab" (middle click, Ctrl+click) from tab `opener`.
    fn open_link_in_tab(&self, opener: TabId, url: &str, foreground: bool) {
        let ok = url.starts_with("https://") || url.starts_with("http://");
        if !ok {
            return;
        }
        let Some(win) = self.window_of(opener) else { return };
        let private = self.tab(opener).is_some_and(|t| t.private);
        let foreground = match self.settings.borrow().link_tabs {
            lb_core::settings::LinkTabs::Auto => foreground,
            lb_core::settings::LinkTabs::Switch => true,
            lb_core::settings::LinkTabs::Background => false,
        };
        let Some((_, effects)) =
            self.ws.borrow_mut().open(win, url, private, Position::AfterOpener(opener), foreground, now_ms())
        else {
            return;
        };
        self.apply(effects);
    }

    /// Opens an http(s) or built-in URL, in the active tab or a new one.
    fn open_url(&self, url: &str, new_tab: bool, private: bool) {
        // Bookmarks saved before the rename may name the old internal pages.
        let url = &lb_core::pages::migrate_legacy_url(url);
        let ok = url.starts_with("https://") || url.starts_with("http://") || Page::from_url(url).is_some();
        if !ok {
            return;
        }
        if new_tab {
            self.new_tab(Some(url), private);
        } else {
            self.navigate_active(url);
            self.focus_page_after_navigation();
        }
    }

    /// A bookmark, a history entry or a new-tab tile was opened: the page
    /// gets the focus, and the address bar stops editing and shows the new
    /// address (a new-tab page's address-bar hold ends).
    fn focus_page_after_navigation(&self) {
        let Some(id) = self.active_id() else { return };
        self.address_focus.set(None);
        if let Some(c) = self.active_view() {
            c.focus();
        }
        if let Some(s) = self.shell() {
            s.post_json(&Event::BlurAddressBar.to_json());
        }
        let _ = id;
    }

    fn navigate_active(&self, url: &str) {
        let Some(id) = self.active_id() else { return };
        self.navigate_tab(id, url);
    }

    /// Built-in pages replace a new-tab page, otherwise open in a new tab. A page needed only
    /// once (downloads, history, settings …) that is already open is switched to instead, in
    /// whichever window it is.
    fn open_page(&self, page: Page) {
        self.open_page_at(page, None);
    }

    /// Opens a page at a section (`settings#privacy`); an open single-instance page is
    /// switched to and sent to that section (a same-document navigation).
    fn open_page_at(&self, page: Page, section: Option<&str>) {
        let url = page.url_with_section(section);
        if page.single_instance() {
            let found = self.ws.borrow().find_page(page);
            if let Some((win, tab)) = found {
                let ops = self
                    .ws
                    .borrow_mut()
                    .update(win, |m| ((), m.activate(tab, now_ms())))
                    .map(|(_, o)| o)
                    .unwrap_or_default();
                self.apply(ops);
                self.focus_window(win);
                if section.is_some() {
                    self.navigate_tab(tab, &url);
                }
                return;
            }
        }
        let on_new_tab = self.active_tab().is_some_and(|t| Page::from_url(&t.url) == Some(Page::NewTab));
        if on_new_tab {
            self.navigate_active(&url);
        } else {
            self.new_tab(Some(&url), false);
        }
    }

    // ------------------------------------------------------------ commands

    fn on_key(&self, key: KeyPress) -> bool {
        // Esc ends the pointer lock (the page does not get it), else page
        // full screen.
        if key.vk == keys::vk::ESCAPE && !key.ctrl && !key.alt && !key.shift {
            // An extension's popup closes first (0.7.0, R21; it takes no activation, so its
            // keys can arrive here).
            if self.ext_popup_open() {
                self.close_ext_popup_because("Esc");
                return true;
            }
            if self.pointer_locked() {
                self.release_pointer(lb_core::pointer_lock::Release::Escape);
                return true;
            }
            if !key.repeat && self.escape_page_fullscreen() {
                return true;
            }
        }
        // Alt+Space: the window's system menu (there is no title bar to
        // receive it natively).
        if key.alt && !key.ctrl && !key.shift && key.vk == 0x20 {
            if let Some(w) = self.window() {
                w.show_system_menu();
            }
            return true;
        }
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
            Command::NewWindow => self.new_window(),
            Command::NewPrivateTab => self.new_tab(None, true),
            Command::NewPrivateWindow => self.new_private_window(),
            Command::CloseTab => {
                // Bind first: an `if let` scrutinee's `Ref` lives for the whole block.
                let active = self.active_id();
                if let Some(id) = active {
                    self.close_tab(id);
                }
            }
            Command::ReopenClosedTab => {
                let reopened = self.ws.borrow_mut().reopen_closed(now);
                if let Some((_, ops)) = reopened {
                    self.apply(ops);
                }
            }
            Command::NextTab | Command::PrevTab => self.update_current(|m| m.cycle(cmd == Command::NextTab, now)),
            Command::SelectTab(i) => self.update_current(|m| m.activate_index(usize::from(i), now)),
            Command::LastTab => self.update_current(|m| m.activate_last(now)),
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
            Command::DevTools => self.toggle_devtools(),
            Command::Fullscreen => self.toggle_fullscreen(),
            Command::TaskManager => self.open_page(Page::Tasks),
            Command::PictureInPicture => self.pip_command(None),
            // The element under the mouse, or DevTools as such when the
            // mouse is not over the page.
            Command::InspectElement => {
                let target = self.active_id().zip(self.active_view());
                if let Some((tab, c)) = target {
                    let at = c.cursor_position();
                    info!("inspect element at {at:?}");
                    self.open_devtools(tab, at);
                }
            }
            Command::History => self.open_page(Page::History),
            Command::ViewSource => {
                if let Some(id) = self.active_id() {
                    self.view_source(id, None);
                }
            }
            Command::Bookmarks => self.open_page(Page::Bookmarks),
            Command::ToggleBookmarksBar => self.toggle_bookmarks_bar(),
            Command::Downloads => self.open_page(Page::Downloads),
            // Wired up in milestone 4 (bookmarks, find in page).
            Command::Bookmark => self.toggle_bookmark(),
            Command::Find => self.show_find_bar(),
            Command::FindNext => self.find_step(false),
            Command::FindPrev => self.find_step(true),
        }
    }

    fn active_do(&self, f: impl FnOnce(&View)) {
        if let Some(c) = self.active_view() {
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

    /// A message from a shell (of window `win`) or the popup (`win` = its owner).
    fn on_ui_message(&self, channel: Channel, win: Option<WindowId>, source: &MessageSource, json: &str) {
        if let Some(win) = win {
            self.make_current(win);
        }
        let sender = match protocol::classify(channel, &source.url, source.main_frame) {
            Ok(Source::Privileged(s)) => s,
            _ => {
                warning!("rejected message on {channel:?} channel from an unexpected origin");
                return;
            }
        };
        match protocol::parse_request(sender, json) {
            Ok(env) => {
                *self.ext.request_origin.borrow_mut() = lb_core::ext::id_of_url(&source.url);
                self.dispatch(sender, None, env);
                *self.ext.request_origin.borrow_mut() = None;
            }
            Err(e) => warning!("rejected {sender:?} message: {e}"),
        }
    }

    fn on_tab_message(&self, id: TabId, source: &MessageSource, json: &str) {
        match protocol::classify(Channel::Tab, &source.url, source.main_frame) {
            Ok(Source::Privileged(sender)) => match protocol::parse_request(sender, json) {
                Ok(env) => {
                    *self.ext.request_origin.borrow_mut() = lb_core::ext::id_of_url(&source.url);
                    self.dispatch(sender, Some(id), env);
                    *self.ext.request_origin.borrow_mut() = None;
                }
                Err(e) => warning!("tab {id}: rejected {sender:?} message: {e}"),
            },
            Ok(Source::Web { origin }) => match protocol::parse_content(json) {
                Ok(msg @ ContentMessage::PageTokens { .. }) => self.page_tokens(id, &origin, msg),
                Ok(ContentMessage::LoginForm {}) => {
                    let net = self.views.borrow().get(&id).map(|v| v.net.clone());
                    if let Some(net) = net {
                        // Only for the page currently shown in the tab.
                        if urlclean::origin_of(&net.top_url()).as_deref() == Some(origin.as_str()) {
                            net.login_form.set(true);
                            self.send_nav_if_active(id);
                        }
                    }
                }
                Ok(ContentMessage::Submit { username, password }) => {
                    self.login_submitted(id, &origin, username, password)
                }
                Ok(ContentMessage::Identifier { username }) => self.login_identifier(id, &origin, username),
                Ok(ContentMessage::RequestFill {}) => self.request_fill(id, Some(&origin)),
                Ok(ContentMessage::NotificationPermission {}) => self.notification_permission_requested(id, &origin),
                Ok(ContentMessage::Media { videos }) => self.media_changed(id, &origin, videos),
                Ok(ContentMessage::PipBackToTab {}) => self.pip_back_to_tab(id),
                Ok(ContentMessage::Painted {}) => self.first_load_done(id),
                // Only the store's own pages (and their tab's address decides what is installed).
                Ok(ContentMessage::ExtStoreClick {}) => {
                    if lb_core::ext::is_store_origin(&origin) {
                        self.ext_store_click(id);
                    }
                }
                Ok(ContentMessage::CardSubmit { number, exp_month, exp_year, name }) => {
                    self.card_submitted(id, &origin, &number, exp_month, exp_year, name)
                }
                Ok(ContentMessage::FieldFocus { field, rect, autocomplete, passwords, form_text }) => {
                    let field = self.resolve_field_kind(field, &autocomplete, passwords, &form_text);
                    self.field_focus(id, &source.frame_id, &origin, true, field, rect)
                }
                Ok(ContentMessage::FieldBlur {}) => self.field_blur(id, &source.frame_id),
                Err(_) => {}
            },
            // An http(s) iframe: only its login fields, for its own origin.
            Ok(Source::WebFrame { origin }) => match protocol::parse_content_frame(json) {
                Ok(ContentMessage::FieldFocus { field, rect, autocomplete, passwords, form_text }) => {
                    let field = self.resolve_field_kind(field, &autocomplete, passwords, &form_text);
                    self.field_focus(id, &source.frame_id, &origin, false, field, rect)
                }
                Ok(ContentMessage::FieldBlur {}) => self.field_blur(id, &source.frame_id),
                _ => {}
            },
            Err(_) => {}
        }
    }

    /// Sends a reply to whoever asked (a built-in page in tab `tab`).
    fn reply(&self, tab: Option<TabId>, event: Event) {
        if let Some(c) = tab.and_then(|t| self.tab_view(t)) {
            c.post_json(&event.to_json());
        }
    }

    /// Everything a shell shows, to the shell of window `win` that has loaded (and what all
    /// shells share, to all of them).
    fn shell_loaded(&self, win: WindowId) {
        self.send_shell_config();
        self.send_tabs_to(win);
        self.send_nav_to(win);
        self.send_shell_shared();
        self.send_window_state();
        self.pass_shell_hit_tests(win);
    }

    /// What every shell shows alike, besides the config: the bookmarks bar, downloads,
    /// update and extension buttons.
    fn send_shell_shared(&self) {
        self.send_bookmarks_bar();
        self.post_shell(&Event::DownloadsChanged { active: self.downloads.borrow().active_count() });
        self.post_shell(&Event::UpdateAvailable { version: self.update_notice() });
        self.ext_send_actions();
    }

    /// The current window is new: a new-tab page activated before its shell finished loading
    /// could not focus the address bar yet.
    fn focus_new_window(&self) {
        let start_tab = self.active_tab().filter(|t| Page::from_url(&t.url) == Some(Page::NewTab)).map(|t| t.id);
        match start_tab {
            Some(id) => self.hold_address_focus(id),
            None => self.focus_active(),
        }
    }

    fn dispatch(&self, sender: Sender, tab: Option<TabId>, env: Envelope) {
        let now = now_ms();
        let Envelope { req_id, request } = env;
        match request {
            Request::Ready => match sender {
                Sender::Ext => {}
                Sender::Shell => {
                    // The shell that just loaded is the current window's (its message made it so).
                    let Some(win) = self.cur_id() else { return };
                    self.shell_loaded(win);
                    self.focus_new_window();
                    self.schedule_spare();
                }
                Sender::Popup => {
                    let slot = self.request_slot();
                    let popup = self.popup_slot(slot).borrow().as_ref().and_then(|p| p.view.clone());
                    if let Some(c) = popup {
                        self.send_theme_to(&c);
                    }
                    self.popup_ready(slot);
                }
                Sender::Page(_) => {
                    if let Some(c) = tab.and_then(|t| self.tab_view(t)) {
                        self.send_theme_to(&c);
                    }
                    // Krunk's own page has its theme: the tab switch may show it (R7).
                    if let Some(t) = tab {
                        self.first_load_done(t);
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
            Request::CloseTab { id } => self.close_tab(id),
            Request::ActivateTab { id } => self.update_current(|m| m.activate(id, now)),
            Request::MoveTab { id, index } => {
                let Some(win) = self.cur_id() else { return };
                let ops = self.ws.borrow_mut().move_tab(id, win, index as usize, now);
                self.apply(ops);
            }
            Request::DuplicateTab { id } => {
                self.update_current(|m| m.duplicate(id, now).map(|(_, e)| e).unwrap_or_default())
            }
            Request::ReopenClosedTab => self.run_command(Command::ReopenClosedTab),
            Request::Navigate { input } => {
                if sender == Sender::Page(Page::NewTab) {
                    // The new tab page's box may send words to another search engine.
                    let template = self.settings.borrow().ntp_search_template().to_string();
                    self.address_suggest_close();
                    let url = urlclean::input_to_url(&input, &template);
                    self.navigate_active(&url);
                } else {
                    self.navigate_input(&input);
                }
                if sender == Sender::Shell || sender == Sender::Page(Page::NewTab) {
                    self.focus_page_after_navigation();
                }
            }
            Request::ShowAddressMenu { x, y, selection, has_text, dirty } => {
                self.show_address_menu(x, y, selection, has_text, dirty)
            }
            Request::AddressMenuAction { action } => self.address_menu_action(action),
            Request::TabMute { id, muted } => {
                if let Some(v) = self.tab_view(id) {
                    v.set_audio_muted(muted);
                    info!("tab {id}: {}", if muted { "muted" } else { "unmuted" });
                }
                self.send_tab_update(id);
            }
            Request::BookmarkAllTabs => {
                self.hide_popup();
                self.bookmark_all_tabs();
            }
            Request::CaptionWindow { action } => {
                self.hide_popup();
                if let Some(w) = self.window() {
                    w.control(action);
                }
            }
            Request::PermissionExceptions => self.reply(tab, Event::ok(req_id, self.permission_exceptions())),
            r @ (Request::DnsStatus | Request::DnsTest { .. } | Request::DnsParse { .. }) => {
                self.dispatch_dns(tab, req_id, r)
            }
            Request::BounceStatus => self.reply(tab, Event::ok(req_id, self.bounce_status())),
            Request::BounceCleanNow => self.clean_bounces(Some(Box::new(move |b, n| {
                b.reply(tab, Event::ok(req_id, serde_json::json!({ "cleaned": n })));
            }))),
            Request::PermissionSet { origin, kind, state } => match self.permission_set(&origin, kind, state) {
                Ok(()) => {
                    if sender == Sender::Popup && self.popup_kind() == Some(PopupKind::SiteInfo) {
                        self.show_popup(self.site_info_content(), None);
                    }
                    self.reply(tab, Event::ok(req_id, ()));
                }
                Err(e) => self.reply(tab, Event::err(req_id, e)),
            },
            Request::GetTasks => self.reply(tab, Event::ok(req_id, self.task_rows())),
            Request::KillTask { id } => self.kill_task(tab, req_id, id),
            Request::Back => self.run_command(Command::Back),
            Request::Forward => self.run_command(Command::Forward),
            Request::Reload => self.run_command(Command::Reload),
            Request::Stop => self.run_command(Command::Stop),
            Request::OpenUrl { url, new_tab } => self.open_url(&url, new_tab, false),
            Request::OpenPage { page, section } => {
                if let Some(p) = Page::from_name(&page) {
                    self.open_page_at(p, section.as_deref());
                }
            }
            Request::Command { command } => self.run_command(command),
            Request::Zoom { action } => self.zoom(action),
            Request::Print => self.run_command(Command::Print),
            Request::DevTools => self.run_command(Command::DevTools),
            Request::DevToolsDock { dock } => self.set_devtools_dock(dock),
            Request::DevToolsClose => self.close_active_devtools(),
            Request::ShowPopup { popup, rect } => {
                let anchor = match popup {
                    PopupKind::Menu => "menu",
                    PopupKind::SiteInfo => "siteInfo",
                    PopupKind::Downloads => "downloads",
                    PopupKind::Pip => "pip",
                    PopupKind::TabList => "tabList",
                    _ => "shield",
                };
                if self.toggle_suppressed(anchor) {
                    // The click that closed it: stay closed.
                } else if popup.shell_may_open() && rect.is_valid() {
                    match popup {
                        PopupKind::Menu => self.show_popup(self.menu_content(), Some(rect)),
                        PopupKind::Downloads => self.show_popup(self.downloads_content(), Some(rect)),
                        PopupKind::Pip => self.pip_command(Some(rect)),
                        PopupKind::SiteInfo => self.show_site_info(rect),
                        PopupKind::TabList => self.show_tab_list(rect),
                        _ => {
                            self.shield_anchor.set(Some(rect));
                            self.show_shield();
                        }
                    }
                }
            }
            // The volume bar is placed and hidden by the host only.
            Request::HidePopup | Request::PopupResize { .. } if self.request_slot() == PopupSlot::Pip => {}
            Request::PipVolume { tab_id, volume, muted } => self.pip_set_volume(tab_id, volume, muted),
            Request::ExtInstallReply { request_id, accept } => self.ext_install_answer(request_id, accept),
            Request::HidePopup if self.request_slot() == PopupSlot::Menu => {
                // Esc or a click outside the item menu: back to the open folder dropdown.
                self.hide_menu_popup(true);
            }
            Request::HidePopup => self.hide_popup(),
            Request::PopupResize { height } => self.popup_resize(height),
            Request::PopupShown { seq, height } => self.popup_shown(self.request_slot(), seq, height, false),
            Request::ShowFolderItemMenu { id, x, y } => self.show_folder_item_menu(id, x, y),
            Request::MoveBookmarkInFolder { id, parent, index } => self.move_bookmark_in_folder(id, parent, index),
            Request::FolderItemDragOut { id } => {
                if self.folder_dropdown_has(id, true) {
                    self.start_bar_drag(id);
                }
            }
            Request::BookmarkActivePage { parent, index } => self.bookmark_active_page(parent, index),
            Request::AddBookmarkAt { url, parent, index } => self.add_bookmark_at(&url, parent, index),
            Request::TabDragOut { id, offset_x, offset_y, first_x } => {
                if let Some(win) = self.cur_id() {
                    self.start_tab_drag(win, id, offset_x, offset_y, first_x);
                }
            }
            Request::OpenBlockedPage {} => self.open_blocked_page(),
            Request::GetBlocked {} => self.get_blocked(tab, req_id),
            r @ (Request::GetFilterLists {}
            | Request::SetFilterList { .. }
            | Request::AddFilterList { .. }
            | Request::RemoveFilterList { .. }
            | Request::RestoreDefaultLists {}
            | Request::GetUserFilters {}
            | Request::SaveUserFilters { .. }) => self.dispatch_filter_lists(tab, req_id, r),
            Request::ShowBookmarkMenu { id, rect } => self.show_bookmark_menu(id, rect),
            Request::BookmarkMenuAction { id, action } => self.bookmark_menu_action(id, action),
            Request::SaveBookmark { id, parent, title, url } => self.save_bookmark(id, parent, &title, url.as_deref()),
            Request::MoveBookmark { id, parent, index } if sender == Sender::Shell => {
                self.move_bookmark_from_bar(id, parent, index)
            }
            Request::ShowTabMenu { id, rect } => {
                let state = self.window_of(id).and_then(|w| {
                    let ws = self.ws.borrow();
                    let m = ws.model(w)?;
                    Some((m.len() > 1, !m.close_many_ids(id, CloseMany::Right).is_empty()))
                });
                if let (Some((can_move, can_close_right)), true) = (state, rect.is_valid()) {
                    let rect = layout::tab_menu_rect(rect.x, rect.y);
                    let origin = self.tab(id).and_then(|t| urlclean::origin_of(&t.url));
                    let muted_site = origin.as_deref().is_some_and(|o| self.site_muted(o));
                    self.show_popup(
                        PopupContent::TabMenu {
                            tab_id: id,
                            can_move,
                            can_close_right,
                            muted_site,
                            has_site: origin.is_some(),
                        },
                        Some(rect),
                    );
                }
            }
            Request::ShowNavMenu { forward, rect } => self.show_nav_menu(forward, rect),
            Request::HttpAuthReply { tab_id, username, password, entry_id } => {
                self.http_auth_reply(tab_id, username, password, entry_id)
            }
            Request::HttpAuthCancel { tab_id } => self.http_auth_cancel(tab_id),
            Request::HttpAuthUnlock { tab_id } => self.http_auth_unlock(tab_id),
            Request::NavToEntry { tab_id, entry_id } => self.nav_to_entry(tab_id, entry_id),
            Request::TabMenuAction { id, action } => {
                self.hide_popup();
                match action {
                    TabAction::Reload => {
                        if let Some(v) = self.tab_view(id) {
                            v.reload();
                        }
                    }
                    TabAction::Duplicate => {
                        if let Some(win) = self.window_of(id) {
                            let ops = self
                                .ws
                                .borrow_mut()
                                .update(win, |m| ((), m.duplicate(id, now).map(|(_, e)| e).unwrap_or_default()))
                                .map(|(_, o)| o)
                                .unwrap_or_default();
                            self.apply(ops);
                        }
                    }
                    TabAction::MoveToNewWindow => self.move_tab_to_new_window(id),
                    TabAction::Close => self.close_tab(id),
                    TabAction::CloseOthers => self.close_many(id, CloseMany::Others),
                    TabAction::CloseRight => self.close_many(id, CloseMany::Right),
                    TabAction::MuteSite | TabAction::UnmuteSite => self.mute_site(id, action == TabAction::MuteSite),
                }
            }
            Request::HitMap { map } => {
                if let Some(win) = self.cur_id() {
                    self.set_hit_map(win, map);
                }
            }
            Request::WindowControl { action } => {
                if let Some(w) = self.window() {
                    w.control(action);
                }
            }
            Request::Quit => self.begin_exit(),
            r @ (Request::GetSettings
            | Request::SaveSettings { .. }
            | Request::ResetSettings
            | Request::PickDownloadFolder
            | Request::Restart
            | Request::GetAbout
            | Request::GetGpuInfo
            | Request::OpenGpuInternals
            | Request::RepairEngine
            | Request::ImportDetect
            | Request::ImportRun { .. }
            | Request::ImportCsv) => self.dispatch_settings(tab, req_id, r),
            r @ (Request::AccountState { .. }
            | Request::AccountSignIn
            | Request::AccountCancel
            | Request::AccountSignOut
            | Request::AccountManage
            | Request::AccountReopen) => self.dispatch_account(tab, r),
            r @ (Request::SyncState
            | Request::SyncOpen
            | Request::SyncTurnOn { .. }
            | Request::SyncSetupConfirm { .. }
            | Request::SyncSetupCopy
            | Request::SyncSetupSave
            | Request::SyncRestore { .. }
            | Request::SyncRestoreConfirm
            | Request::SyncConfirmRotation
            | Request::SyncFinishSecuring { .. }
            | Request::SyncJoin { .. }
            | Request::SyncJoinCode { .. }
            | Request::SyncCancel
            | Request::SyncApprove { .. }
            | Request::SyncApproveCancel
            | Request::SyncRemoveDevice { .. }
            | Request::SyncTurnOff { .. }
            | Request::SyncNow
            | Request::SyncSetCollection { .. }
            | Request::SyncRenameDevice { .. }) => self.dispatch_sync(tab, r, req_id),
            Request::GetUpdateStatus => self.reply(tab, Event::ok(req_id, self.update_status())),
            Request::CheckForUpdates => self.check_for_updates(Some((tab, req_id))),
            Request::UpdateReply { choice } => self.update_reply(choice),
            // Prompts and the remaining features arrive in later milestones;
            // the popup still treats a reply as "dismissed" by hiding.
            Request::SavePasswordReply { tab_id, origin, accept, never, username } => {
                self.save_password_reply(tab_id, &origin, accept, never, username)
            }
            Request::FillReply { tab_id, origin, entry_id } => self.fill_reply(tab_id, &origin, entry_id),
            Request::SuggestPick { tab_id, frame_id, entry_id } => self.suggest_pick(tab_id, &frame_id, &entry_id),
            Request::PipPick { tab_id, index } => self.pip_pick(tab_id, index),
            Request::Ext { ext, op, data } => self.ext_page_request(tab, req_id, &ext, &op, data),
            Request::ExtAction { id, rect } => self.ext_action_clicked(&id, rect),
            Request::ExtList => self.reply(tab, Event::ok(req_id, self.ext_list_json())),
            Request::ExtInstallFile => self.ext_install_file(tab, req_id),
            Request::ExtInstallFolder => self.ext_install_folder(tab, req_id),
            Request::ExtInstallUrl { url } => self.ext_install_url(tab, req_id, &url),
            Request::ExtSet { id, enabled, private } => {
                self.ext_set(&id, enabled, private);
                self.reply(tab, Event::ok(req_id, self.ext_list_json()));
            }
            Request::ExtRemove { id } => {
                self.ext_remove(&id);
                self.reply(tab, Event::ok(req_id, self.ext_list_json()));
            }
            Request::ExtOpenOptions { id } => self.ext_open_options_request(&id),
            Request::SuggestUnlock { tab_id, frame_id } => self.suggest_unlock(tab_id, &frame_id),
            Request::SuggestGenerate { tab_id, frame_id } => self.suggest_generate(tab_id, &frame_id),
            Request::CardFillPick { tab_id, frame_id, card_id } => self.card_fill_pick(tab_id, &frame_id, &card_id),
            Request::SaveCardReply { tab_id, origin, accept } => self.save_card_reply(tab_id, &origin, accept),
            Request::PasswordSavedUndo { entry_id } => self.password_saved_undo(&entry_id),
            Request::PasswordSavedEdit { entry_id } => {
                self.open_page_at(Page::Passwords, Some(&format!("entry={entry_id}")));
            }
            Request::RequestFill => {
                let active = self.active_id();
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
            | Request::VaultRecoveryCreate { .. }
            | Request::VaultRecoveryConfirm { .. }
            | Request::VaultRecoveryCancel
            | Request::VaultRecoveryCopy
            | Request::VaultRecoverySave
            | Request::VaultRecoveryRemove { .. }
            | Request::VaultRecoveryCheck { .. }
            | Request::VaultRecover { .. }
            | Request::GeneratePassword { .. }
            | Request::GetNeverSave
            | Request::NeverSaveRemove { .. }
            | Request::CardList
            | Request::CardSave { .. }
            | Request::CardDelete { .. }
            | Request::CardReveal { .. }
            | Request::CardCopy { .. }) => self.dispatch_vault(tab, req_id, r),
            Request::ShieldAllowSite { allow } => self.shield_allow_site(allow),
            r @ (Request::GetAllowlist
            | Request::AllowlistAdd { .. }
            | Request::AllowlistRemove { .. }
            | Request::UpdateFilterLists
            | Request::GetFilterStatus
            | Request::HttpsOnlyProceed { .. }) => self.dispatch_blocking(tab, req_id, r),
            r @ (Request::StorageList
            | Request::StorageUsage { .. }
            | Request::StorageCookies { .. }
            | Request::StorageDeleteCookie { .. }
            | Request::StorageDeleteSite { .. }
            | Request::StorageDeleteAllCookies
            | Request::StorageClearOrigin { .. }) => self.dispatch_storage(tab, req_id, r),
            Request::PermissionReply { prompt_id, allow, remember } => {
                self.permission_reply(prompt_id, allow, remember)
            }
            Request::AddressSuggest { text, rect } => self.address_suggest(text, rect),
            Request::AddressSuggestSelect { selected } => self.address_suggest_select(selected),
            Request::AddressSuggestPick { url } => self.address_suggest_pick(&url),
            r @ (Request::SiteDataGet {}
            | Request::SitePermissionRemove { .. }
            | Request::SiteDataSet { .. }
            | Request::SiteDataRemove { .. }
            | Request::SiteDataClear { .. }
            | Request::SiteCookieSet { .. }
            | Request::SiteCookiesDelete { .. }) => self.dispatch_site_data(tab, req_id, r),
            Request::OpenSiteData => self.open_site_data(),
            Request::HangReply { tab_id, wait } => self.hang_reply(tab_id, wait),
            Request::SourceGet {} => self.source_get(tab, req_id),
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
        let zoom = self.active_view().map(|c| c.zoom()).unwrap_or(1.0);
        let has_video = self
            .active_id()
            .and_then(|id| {
                self.views.try_borrow().ok().map(|v| v.get(&id).is_some_and(|t| !t.net.videos.borrow().is_empty()))
            })
            .unwrap_or(false);
        PopupContent::Menu {
            zoom,
            can_reopen: self.ws.borrow().closed_count() > 0,
            has_video,
            update: self.update_notice(),
            account: self.account_menu_label(),
        }
    }

    /// Shows `content` in the popup. `rect` is in shell CSS pixels; `None`
    /// keeps the current position (content refresh).
    /// The kind of content the popup shows, if any.
    fn popup_kind(&self) -> Option<PopupKind> {
        self.popup.try_borrow().ok()?.as_ref().and_then(|p| p.content.as_ref().map(PopupContent::kind))
    }

    fn show_popup(&self, content: PopupContent, rect: Option<PopupRect>) {
        self.show_popup_in(PopupSlot::Main, content, rect);
    }

    pub(super) fn popup_slot(&self, slot: PopupSlot) -> &RefCell<Option<Popup>> {
        match slot {
            PopupSlot::Main => &self.popup,
            PopupSlot::Menu => &self.menu_popup,
            PopupSlot::Pip => &self.pip_popup,
        }
    }

    /// The slot a popup-view request refers to (set while its message is dispatched).
    pub(super) fn request_slot(&self) -> PopupSlot {
        self.request_from.get()
    }

    pub(super) fn show_popup_in(&self, slot: PopupSlot, content: PopupContent, rect: Option<PopupRect>) {
        let Some(main) = self.window() else { return };
        // Another popup of Krunk's replaces an extension's (0.7.0, R21).
        if rect.is_some() && slot != PopupSlot::Pip {
            self.close_ext_popup_because("another popup opened");
        }
        // A popup window that is gone (destroyed with a window, 0.11.1) is made anew.
        let dead = self.popup_slot(slot).borrow().as_ref().is_some_and(|p| !p.window.exists());
        if dead {
            warning!("popup: its window was gone; making a new one");
            if let Some(p) = self.popup_slot(slot).borrow_mut().take() {
                if let Some(c) = p.view {
                    c.close();
                }
            }
        }
        if self.popup_slot(slot).borrow().is_none() && !self.create_popup(main, slot) {
            return;
        }
        let mut guard = self.popup_slot(slot).borrow_mut();
        let Some(popup) = guard.as_mut() else { return };
        let kind = content.kind();
        let replaced = popup.content.replace(content.clone());
        // An open HTTP sign-in dialog replaced by anything but its own unlock prompt: its request
        // is cancelled (later, outside this borrow; the engine may call back synchronously).
        let auth_gone = matches!(replaced, Some(PopupContent::HttpAuth { .. }))
            && !matches!(
                &content,
                PopupContent::HttpAuth { .. } | PopupContent::Unlock { purpose: UnlockPurpose::HttpAuth, .. }
            );
        if auth_gone {
            let weak = self.me.clone();
            crate::ffi::cef::post_ui(move || {
                if let Some(b) = weak.upgrade() {
                    b.http_auth_dismissed();
                }
            });
        }
        let mut seq = None;
        if let Some(rect) = rect {
            // Shown for the current window: it belongs to that frame now.
            let owner = self.cur_id();
            if popup.owner != owner {
                popup.window.set_owner(main);
                popup.owner = owner;
            }
            let scale = main.scale();
            let origin = main.client_to_screen(0, 0);
            let work = Window::work_area_at(origin.0 + (rect.x * scale) as i32, origin.1);
            let r = layout::popup_screen_rect(
                rect,
                scale,
                origin,
                layout::Rect { x: work.x, y: work.y, width: work.width, height: work.height },
            );
            let target = ScreenRect { x: r.x, y: r.y, width: r.width, height: r.height };
            let focus = kind.takes_focus();
            popup.window.set_activatable(focus);
            // The window shows the new content from its first frame (0.11.0): it opens (or
            // moves, when it is open already) once the popup has painted it. A folder dropdown
            // used to show the previously opened folder for a frame or two.
            let gate = slot != PopupSlot::Pip && popup_gate() != "off";
            if gate {
                let n = self.popup_seq.get() + 1;
                self.popup_seq.set(n);
                popup.pending = Some(PendingShow { seq: n, rect: target, focus });
                seq = Some(n);
                if popup.window.is_visible() {
                    // Shown already (a subfolder or Back in a dropdown): it takes the new place
                    // and size first, so the new content can measure its full height (0.11.1:
                    // going back to a longer list kept the shorter one's height).
                    popup.window.place_hidden(target);
                    if let Some(c) = &popup.view {
                        c.set_bounds(0, 0, r.width, r.height);
                    }
                } else {
                    if popup_gate() == "park" {
                        popup.window.show_at(
                            ScreenRect { x: -32000, y: -32000, width: target.width, height: target.height },
                            focus,
                        );
                    } else {
                        popup.window.place_hidden(target);
                    }
                    if let Some(c) = &popup.view {
                        c.set_bounds(0, 0, r.width, r.height);
                        c.set_visible(true);
                    }
                }
                let weak = self.me.clone();
                crate::ffi::cef::post_ui_delayed(POPUP_PAINT_WAIT_MS, move || {
                    if let Some(b) = weak.upgrade() {
                        b.popup_shown(slot, n, None, true);
                    }
                });
            } else {
                popup.window.show_at(target, focus);
            }
            // The page stays still under Krunk's menus (0.8.0).
            if let Some(i) = [PopupSlot::Main, PopupSlot::Menu].iter().position(|s| *s == slot) {
                let menu = matches!(
                    kind,
                    PopupKind::Menu
                        | PopupKind::NavMenu
                        | PopupKind::TabMenu
                        | PopupKind::BookmarkFolder
                        | PopupKind::BookmarkMenu
                        | PopupKind::CaptionMenu
                );
                crate::ffi::scroll_lock::set_menu(i, menu.then(|| popup.window.hwnd()));
            }
            if !gate {
                if let Some(c) = &popup.view {
                    c.set_bounds(0, 0, r.width, r.height);
                    c.set_visible(true);
                    if focus {
                        c.focus();
                    }
                }
            }
        }
        if popup.ready {
            if let Some(c) = &popup.view {
                c.post_json(&Event::Popup { content, seq }.to_json());
            }
        }
    }

    /// The popup painted the content numbered `seq` (or it did not answer in time,
    /// `late`): its window goes in place, `height_css` fitted when the content asks for it.
    pub(super) fn popup_shown(&self, slot: PopupSlot, seq: u64, height_css: Option<f64>, late: bool) {
        let Some(main) = self.window() else { return };
        let Ok(mut guard) = self.popup_slot(slot).try_borrow_mut() else { return };
        let Some(p) = guard.as_mut() else { return };
        let Some(pending) = p.pending.filter(|x| x.seq == seq) else { return };
        p.pending = None;
        if late {
            warning!("popup: shown without its paint answer ({} ms)", POPUP_PAINT_WAIT_MS);
        }
        let mut r = pending.rect;
        if let Some(h) = height_css.filter(|h| h.is_finite()) {
            let work = Window::work_area_at(r.x, r.y);
            let max = (work.y + work.height - r.y).max(40);
            r.height = ((h.clamp(40.0, 4096.0) * main.scale()).round() as i32).min(max);
        }
        p.window.show_at(r, pending.focus);
        if let Some(c) = &p.view {
            c.set_bounds(0, 0, r.width, r.height);
            c.set_visible(true);
            if pending.focus {
                c.focus();
            }
        }
    }

    pub(super) fn create_popup(&self, owner: Window, slot: PopupSlot) -> bool {
        let events: Rc<dyn WindowEvents> = match slot {
            PopupSlot::Main => Rc::new(PopupEvents(self.me.clone())),
            PopupSlot::Menu => Rc::new(MenuPopupEvents(self.me.clone())),
            PopupSlot::Pip => Rc::new(pip::PipBarEvents),
        };
        let window = match Window::create_popup(owner, events) {
            Ok(w) => w,
            Err(e) => {
                warning!("popup: {e}");
                return false;
            }
        };
        // The popup always uses the default profile, never a private one.
        let c = View::new(ViewOptions {
            role: Role::Popup,
            private: false,
            scripts: Scripts::default(),
            background: self.popup_background(),
            policy: None,
        });
        c.on_before_browse(|nav| assets::allow_navigation(Channel::Popup, "", &nav.url, true));
        let weak = self.me.clone();
        c.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                let owner = b.popup_slot(slot).try_borrow().ok().and_then(|p| p.as_ref().and_then(|p| p.owner));
                b.request_from.set(slot);
                b.on_ui_message(Channel::Popup, owner, source, json);
                b.request_from.set(PopupSlot::Main);
            }
        });
        let weak = self.me.clone();
        c.on_key(move |key| {
            weak.upgrade().is_some_and(|b| {
                let owner = b.popup_slot(slot).try_borrow().ok().and_then(|p| p.as_ref().and_then(|p| p.owner));
                if let Some(win) = owner {
                    b.make_current(win);
                }
                b.on_key(key)
            })
        });
        // A popup whose renderer ended loads again (as the shell does), so menus keep working.
        let weak = self.me.clone();
        c.on_crashed(move |reason| {
            warning!("popup: its renderer ended ({reason}); reloading it");
            let weak = weak.clone();
            crate::ffi::cef::post_ui(move || {
                let Some(b) = weak.upgrade() else { return };
                let view = b.popup_slot(slot).try_borrow_mut().ok().and_then(|mut guard| {
                    let p = guard.as_mut()?;
                    p.ready = false;
                    p.pending = None;
                    p.view.clone()
                });
                if let Some(c) = view {
                    c.navigate(POPUP_URL);
                }
            });
        });
        if let Err(e) = c.start(window.hwnd(), POPUP_URL) {
            warning!("popup: {e}");
            window.destroy();
            return false;
        }
        let (w, h) = window.client_size();
        c.set_bounds(0, 0, w, h);
        c.set_visible(window.is_visible());
        *self.popup_slot(slot).borrow_mut() =
            Some(Popup { window, view: Some(c), ready: false, content: None, owner: None, pending: None });
        true
    }

    fn popup_ready(&self, slot: PopupSlot) {
        let pending = {
            let mut guard = self.popup_slot(slot).borrow_mut();
            let Some(p) = guard.as_mut() else { return };
            p.ready = true;
            if p.open() {
                p.content.clone().zip(p.view.clone()).map(|(content, c)| (content, c, p.pending.map(|x| x.seq)))
            } else {
                None
            }
        };
        if let Some((content, c, seq)) = pending {
            let focus = content.kind().takes_focus();
            c.post_json(&Event::Popup { content, seq }.to_json());
            if focus && seq.is_none() {
                c.focus();
            }
        }
    }

    /// The popup lost activation, or another view took the focus: the
    /// popup closes, and the toolbar click that may have caused this does
    /// not reopen it (`toggle`).
    fn hide_popup_by_focus_loss(&self) {
        // A middle-clicked bookmark's new background tab took activation (0.11.0): the
        // dropdown it came from stays open (it gets activation back).
        if now_ms() < self.popup_hold_until.get() {
            return;
        }
        let anchor = self
            .popup
            .try_borrow()
            .ok()
            .and_then(|p| p.as_ref().filter(|p| p.open()).and_then(|p| p.content.as_ref()?.toggle_anchor()));
        if let Some(a) = anchor {
            self.toggle.borrow_mut().closed_by_focus_loss(&a, now_ms() as u64);
        }
        self.hide_popup();
    }

    /// Is this request to open a toolbar popup the click that just closed
    /// it? Then the shell is told it stays closed.
    pub(super) fn toggle_suppressed(&self, anchor: &str) -> bool {
        let suppressed = self.toggle.borrow_mut().suppress(anchor, now_ms() as u64);
        if suppressed {
            self.post_shell(&Event::PopupClosed);
        }
        suppressed
    }

    /// Hides the menu popup (the item menu over a folder dropdown). With `reactivate`, the
    /// dropdown underneath gets the keyboard back; otherwise the caller closes it too.
    pub(super) fn hide_menu_popup(&self, reactivate: bool) {
        self.menu_from_folder.set(None);
        let hidden = {
            let Ok(mut guard) = self.menu_popup.try_borrow_mut() else { return };
            let Some(p) = guard.as_mut() else { return };
            let visible = p.open();
            if visible {
                self.menu_closing.set(true);
                p.window.hide();
                self.menu_closing.set(false);
                if let Some(c) = &p.view {
                    c.post_json(&Event::PopupClear.to_json());
                }
            }
            p.content = None;
            p.pending = None;
            visible
        };
        if hidden && reactivate {
            let main = self
                .popup
                .try_borrow()
                .ok()
                .and_then(|p| p.as_ref().filter(|p| p.window.is_visible()).map(|p| (p.window, p.view.clone())));
            if let Some((w, view)) = main {
                let r = w.rect();
                w.show_at(ScreenRect { x: r.x, y: r.y, width: r.width, height: r.height }, true);
                if let Some(c) = view {
                    c.focus();
                }
            }
        }
    }

    /// Hides the popup (and the menu over it). A hidden prompt counts as dismissed, never
    /// confirmed.
    fn hide_popup(&self) {
        self.hide_menu_popup(false);
        self.menu_from_folder.set(None);
        let (was_visible, content, owner) = {
            let Ok(mut guard) = self.popup.try_borrow_mut() else { return };
            let Some(p) = guard.as_mut() else { return };
            let visible = p.open();
            if visible {
                p.window.hide();
                if let Some(c) = &p.view {
                    c.post_json(&Event::PopupClear.to_json());
                }
            }
            p.pending = None;
            (visible, p.content.take(), p.owner)
        };
        if let Some(c) = &content {
            self.prompt_dismissed(c);
        }
        if was_visible {
            match owner {
                Some(win) => self.post_shell_to(win, &Event::PopupClosed),
                None => self.post_shell(&Event::PopupClosed),
            }
        }
    }

    // ------------------------------------------------------------ internal origins

    /// Request handler for our internal origins: embedded UI files, and the
    /// cached favicons from the database.
    fn server(&self) -> impl Fn(&str) -> InternalResponse + 'static {
        let weak = self.me.clone();
        move |uri: &str| {
            // The new tab page's background image (copied into the profile when picked).
            if let Some(name) = lb_core::assets::ntp_image_request(uri) {
                let bytes = weak.upgrade().and_then(|b| {
                    let want = b.settings.try_borrow().ok()?.ntp.image.clone();
                    (!want.is_empty() && want == name).then(|| std::fs::read(b.dirs.root().join(&want)).ok())?
                });
                return match bytes {
                    Some(body) => InternalResponse {
                        status: 200,
                        reason: "OK",
                        headers: lb_core::assets::ntp_image_headers(&name).to_string(),
                        body: Cow::Owned(body),
                    },
                    None => InternalResponse::not_found(),
                };
            }
            if lb_core::ext::id_of_url(uri).is_some() {
                return weak.upgrade().and_then(|b| b.ext_response(uri)).unwrap_or_else(InternalResponse::not_found);
            }
            // A tab's favicon for the strip (the model keeps it as a data: URL).
            if let Some(id) = assets::tab_icon_request(uri) {
                let png = weak
                    .upgrade()
                    .and_then(|b| assets::png_of_data_url(b.ws.try_borrow().ok()?.tab(id)?.favicon.as_deref()?));
                return match png {
                    Some(png) => InternalResponse {
                        status: 200,
                        reason: "OK",
                        headers: assets::favicon_headers().to_string(),
                        body: Cow::Owned(png),
                    },
                    None => InternalResponse::not_found(),
                };
            }
            if let Some(host) = assets::favicon_request(uri) {
                let png = weak.upgrade().and_then(|b| b.store.try_borrow().ok()?.favicon(&host).ok()?);
                return match png {
                    Some(png) => InternalResponse {
                        status: 200,
                        reason: "OK",
                        headers: assets::favicon_headers().to_string(),
                        body: Cow::Owned(png),
                    },
                    None => InternalResponse::not_found(),
                };
            }
            match assets::resolve(uri) {
                Some(asset) => InternalResponse {
                    status: 200,
                    reason: "OK",
                    headers: assets::response_headers(&asset),
                    body: Cow::Borrowed(asset.bytes),
                },
                None => InternalResponse::not_found(),
            }
        }
    }

    // ------------------------------------------------------------ shutdown

    fn shutdown(&self) {
        if self.closing.replace(true) {
            return;
        }
        info!("shutting down");
        if let Some(w) = self.app() {
            w.kill_timer(SESSION_TIMER);
        }
        self.save_session();
        self.flush_titles_now();
        self.save_downloads();
        self.shutdown_vault();
        self.relaunch_if_requested();
        let adopted: Vec<View> = self.adopted.borrow_mut().drain().map(|(_, (v, _))| v).collect();
        for v in adopted {
            v.close();
        }
        let views: Vec<TabView> = self.views.borrow_mut().drain().map(|(_, v)| v).collect();
        for v in views {
            v.view.close();
        }
        if let Some(p) = self.popup.borrow_mut().take() {
            if let Some(c) = p.view {
                c.close();
            }
            p.window.destroy();
        }
        for f in self.all_frames() {
            if let Some(s) = f.shell.borrow_mut().take() {
                s.close();
            }
        }
        self.drop_spare();
        // The message loop ends when the engine has closed every browser.
        engine::quit_when_closed();
    }

    /// Ends the browser (exit, or the last window closed): every frame goes, and the last
    /// one's destruction shuts the browser down.
    fn destroy_frames(&self) {
        self.save_session();
        self.session_final.set(true);
        for f in self.all_frames() {
            f.window.destroy();
        }
    }
}

/// Window events of the popup window.
struct PopupEvents(Weak<Browser>);

impl WindowEvents for PopupEvents {
    fn activated(&self, active: bool) {
        if !active {
            if let Some(b) = self.0.upgrade() {
                b.hide_popup_by_focus_loss();
            }
        }
    }
    // Activation moving to the menu popup (an item menu over this dropdown) is not the user
    // leaving: the default no-op keeps the dropdown open.

    /// The popup window was destroyed anyway (0.11.1): its slot empties, so the next popup is
    /// made anew instead of being shown in a window that is gone.
    fn destroyed(&self) {
        if let Some(b) = self.0.upgrade() {
            b.defer(|b| b.popup_window_gone(PopupSlot::Main));
        }
    }
}

/// The menu popup: losing activation to the dropdown underneath just closes the menu;
/// losing it to anything else closes both, like a click outside.
struct MenuPopupEvents(Weak<Browser>);

impl WindowEvents for MenuPopupEvents {
    fn activated(&self, active: bool) {
        if active {
            return;
        }
        let Some(b) = self.0.upgrade() else { return };
        if b.menu_closing.get() {
            return;
        }
        b.hide_menu_popup(false);
        b.hide_popup_by_focus_loss();
    }

    fn activation_to_popup(&self) {
        if let Some(b) = self.0.upgrade() {
            if !b.menu_closing.get() {
                b.hide_menu_popup(false);
            }
        }
    }

    fn destroyed(&self) {
        if let Some(b) = self.0.upgrade() {
            b.defer(|b| b.popup_window_gone(PopupSlot::Menu));
        }
    }
}

/// Events of the hidden application window: timers, worker-thread wake-ups and
/// system broadcasts.
struct AppEvents(Weak<Browser>);

impl WindowEvents for AppEvents {
    fn system_theme_changed(&self) {
        if let Some(b) = self.0.upgrade() {
            b.apply_frame_theme();
        }
    }

    fn session_locked(&self) {
        if let Some(b) = self.0.upgrade() {
            b.lock_vault("Windows session locked");
        }
    }

    fn suspending(&self) {
        if let Some(b) = self.0.upgrade() {
            b.lock_vault("system suspend");
        }
    }

    fn timer(&self, id: usize) {
        let Some(b) = self.0.upgrade() else { return };
        match id {
            SESSION_TIMER => {
                if let Some(w) = b.app() {
                    w.kill_timer(SESSION_TIMER);
                }
                b.save_session();
            }
            SHIELD_TIMER => b.shield_timer(),
            library::TITLE_TIMER => b.flush_titles(),
            POWER_TIMER => b.power_tick(),
            MEMREPORT_TIMER => b.mem_report_line("tick"),
            pointer::HINT_TIMER => b.hint_tick(),
            status::STATUS_TIMER => b.status_tick(),
            updater::APP_UPDATE_TIMER => b.update_timer(),
            updater::UPDATE_PROGRESS_TIMER => b.update_progress_timer(),
            VAULT_TIMER => b.vault_timer(),
            EXIT_TIMER => b.finish_exit(),
            CLIPBOARD_TIMER => b.clipboard_timer(),
            drag::DRAG_TIMER => b.drag_tick(),
            drag::BAR_DRAG_TIMER => b.bar_drag_timer(),
            spare::SPARE_TIMER => b.spare_timer(),
            segmented::PARTS_TIMER => b.parts_tick(),
            ZOOM_TIMER => b.zoom_tick(),
            hang::HANG_TIMER => b.hang_tick(),
            bounce::BOUNCE_TIMER => b.bounce_tick(),
            pip::PIP_TIMER => b.pip_tick(),
            ext::EXT_POPUP_TIMER => b.ext_popup_tick(),
            UPDATE_TIMER => {
                if let Some(w) = b.app() {
                    w.set_timer(UPDATE_TIMER, blocking::UPDATE_CHECK_INTERVAL_MS);
                }
                b.scheduled_filter_check();
            }
            _ => {}
        }
    }

    fn app_message(&self, code: usize) {
        let Some(b) = self.0.upgrade() else { return };
        match code {
            APP_FILTERS_READY => b.filters_ready(),
            APP_DEFERRED => b.run_deferred(),
            APP_JOB => b.jobs_ready(),
            APP_SHIELD => b.shield_changed(),
            flight::APP_VSYNC => b.flight_tick(),
            _ => {}
        }
    }
}

/// Events of one browser window (frame).
struct FrameEvents {
    b: Weak<Browser>,
    win: WindowId,
}

impl WindowEvents for FrameEvents {
    fn devtools_chrome(&self) -> Option<crate::ffi::window::DevToolsChrome> {
        self.b.upgrade().and_then(|b| b.devtools_chrome(self.win))
    }

    fn background_dark(&self) -> bool {
        self.b.upgrade().is_some_and(|b| b.frame_dark())
    }

    fn strip_clicked(&self, button: lb_core::layout::StripButton) {
        if let Some(b) = self.b.upgrade() {
            b.strip_clicked(self.win, button);
        }
    }

    fn frame_hit(&self, x: i32, y: i32) -> lb_core::frame::Hit {
        self.b.upgrade().map(|b| b.frame_hit_at(self.win, x, y)).unwrap_or(lb_core::frame::Hit::Client)
    }

    fn caption_state(&self, hover: Option<lb_core::protocol::CaptionButton>, pressed: bool) {
        if let Some(b) = self.b.upgrade() {
            b.post_shell_to(self.win, &Event::CaptionState { hover, pressed });
        }
    }

    fn maximized_changed(&self, _maximized: bool) {
        if let Some(b) = self.b.upgrade() {
            b.send_window_state();
            b.schedule_session_save();
        }
    }

    fn activated(&self, active: bool) {
        let Some(b) = self.b.upgrade() else { return };
        if let Some(f) = b.frame(self.win) {
            f.active.set(active);
        }
        // While a tab is dragged, Windows passes activation back and forth between the source
        // and the dragged window: keep that out of the tab strips (the drop settles it).
        if b.drag.get().is_some() {
            return;
        }
        if active {
            b.make_current(self.win);
            b.send_nav_to(self.win);
        } else {
            // Alt+Tab or a click into another window ends the lock.
            b.release_pointer(lb_core::pointer_lock::Release::WindowDeactivated);
        }
        b.send_window_state();
    }

    fn raw_mouse(&self, dx: i32, dy: i32) {
        if let Some(b) = self.b.upgrade() {
            b.raw_mouse(dx, dy);
        }
    }

    fn splitter_at(&self, x: i32, y: i32) -> Option<bool> {
        self.b.upgrade()?.splitter_at(self.win, x, y)
    }

    fn splitter_drag(&self, x: i32, y: i32, done: bool) {
        if let Some(b) = self.b.upgrade() {
            b.splitter_drag(self.win, x, y, done);
        }
    }

    fn activation_to_popup(&self) {
        let Some(b) = self.b.upgrade() else { return };
        // The popup took activation without showing something to answer
        // (it is being created, or shows the pointer-lock hint): give the
        // window and the page back the focus.
        // While the popup is being shown (borrowed by show_popup, which
        // activates it on purpose) this is not a stray activation.
        let Ok(popup) = b.popup.try_borrow() else { return };
        let showing = popup.as_ref().is_some_and(|p| p.open());
        drop(popup);
        let passive = !showing;
        if passive {
            let win = self.win;
            b.defer(move |b| {
                if let Some(f) = b.frame(win) {
                    f.window.bring_to_front();
                }
                if b.pointer_locked() {
                    if let Some(v) = b.active_view_of(win) {
                        v.focus();
                    }
                } else {
                    b.focus_active();
                }
            });
        }
    }

    fn caption_menu(&self, x: i32, y: i32) -> bool {
        let Some(b) = self.b.upgrade() else { return false };
        b.show_caption_menu(self.win, x, y);
        true
    }

    fn size_loop(&self, active: bool) {
        let Some(b) = self.b.upgrade() else { return };
        if active {
            b.hide_popup();
            b.hide_status();
            b.hide_hint();
        } else {
            b.schedule_session_save();
        }
    }

    fn resized(&self, _width: i32, _height: i32) {
        let Some(b) = self.b.upgrade() else { return };
        b.hide_popup();
        b.hide_status();
        b.hide_hint();
        if let Some(f) = b.frame(self.win) {
            b.layout_frame(&f);
        }
        b.reapply_pointer();
        b.schedule_session_save();
    }

    fn minimized(&self, minimized: bool) {
        let Some(b) = self.b.upgrade() else { return };
        b.hide_status();
        if minimized {
            b.release_pointer(lb_core::pointer_lock::Release::WindowDeactivated);
        }
        b.hide_popup();
        // Hidden views use less CPU and memory.
        if let Some(c) = b.active_view_of(self.win) {
            c.set_visible(!minimized);
            if c.devtools_open() {
                c.set_devtools_visible(!minimized);
            }
        }
        if let Some(s) = b.frame(self.win).and_then(|f| f.shell.borrow().clone()) {
            s.set_visible(!minimized);
        }
    }

    fn moved(&self) {
        let Some(b) = self.b.upgrade() else { return };
        // A window following a tab drag moves on every mouse move: the drop does this once.
        if b.drag.get().is_some_and(|d| d.dragged() == self.win) {
            return;
        }
        b.frame_moved(self.win);
    }

    fn follow_mouse(&self, released: bool) {
        if let Some(b) = self.b.upgrade() {
            b.drag_mouse(released);
        }
    }

    fn got_focus(&self) {
        // Keyboard shortcuts arrive through the views, so the frame must never keep the focus
        // itself. CEF moves the focus later, though, and focusing a view also activates its
        // window: a request for a window that has lost activation meanwhile would take it back
        // (two windows then took turns, endlessly). So: next turn, and only if this window is
        // still the active one; not during a tab drag (the drop focuses the right window).
        let Some(b) = self.b.upgrade() else { return };
        if b.drag.get().is_some() {
            return;
        }
        let win = self.win;
        b.defer(move |b| {
            if b.frame(win).is_some_and(|f| f.window.is_foreground()) {
                b.make_current(win);
                b.focus_active();
            }
        });
    }

    fn close_requested(&self) -> bool {
        let Some(b) = self.b.upgrade() else { return true };
        info!("window {}: close requested ({} open)", self.win, b.frames.borrow().len());
        if b.frames.borrow().len() > 1 {
            // Another window stays open: this one goes with its tabs.
            let ops = b.ws.borrow_mut().close_window(self.win);
            b.apply(ops);
            return false;
        }
        // The last window: Krunk exits. With clear-on-exit, close only after the data has
        // been cleared.
        if b.settings.borrow().clear_on_exit.any() && !b.exit_cleared.get() {
            b.begin_exit();
            return false;
        }
        true
    }

    fn destroyed(&self) {
        let Some(b) = self.b.upgrade() else { return };
        info!("window {} destroyed", self.win);
        // The last window: save the session while it still has its place.
        let only_this = {
            let frames = b.frames.borrow();
            frames.len() == 1 && frames[0].id == self.win
        };
        if only_this {
            b.save_session();
            b.session_final.set(true);
        }
        let last = {
            let mut frames = b.frames.borrow_mut();
            frames.retain(|f| f.id != self.win);
            frames.is_empty()
        };
        if last {
            b.shutdown();
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
