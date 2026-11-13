//! Extensions (Krunk 0.6.0, R26): installation, state and runtime of Krunk's own Manifest V3
//! subset (`lb_core::ext` has the pure parts).
//!
//! * Packages (zip, CRX3, a folder, a Web Store address) are unpacked into
//!   `extensions\<id>\`; `extensions\state.json` keeps what is installed, enabled and
//!   allowed in private tabs; `extensions\<id>.storage.json` is `chrome.storage`.
//! * Content scripts run in the isolated world `krunk-ext` of every web tab: the bundle
//!   (`core::ext::content_bundle`) is installed with `Page.addScriptToEvaluateOnNewDocument`,
//!   calls come back through the DevTools binding `__krunkExt`, replies go in with
//!   `Runtime.evaluate` in that world. Krunk's own origins never get the bundle (the bundle
//!   refuses them too) and private tabs only get extensions allowed there.
//! * Extension pages live on `https://<id>.ext.krunk.invalid`: the background page Krunk
//!   builds around the service worker (a hidden view), the action popup (a popup window)
//!   and the options page (a tab). They talk through the page bridge like Krunk's pages.
//! * Static declarativeNetRequest rules become filter lines of the blocker.

use super::{Browser, TabId, UI_BACKGROUND};
use crate::ffi::cef::{Menu, Role, Scripts, View, ViewOptions};
use crate::ffi::window::{ScreenRect, Window, WindowEvents};
use crate::ffi::{dialogs, http};
use crate::log::{info, warning};
use lb_core::ext::{self, Installed, Manifest};
use lb_core::pages::Page;
use lb_core::protocol::Channel;
use lb_core::protocol::{Event, ExtAction, ExtMenuAction, PopupContent, PopupRect};
use lb_core::storage::now_ms;
use lb_core::{assets, layout};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod runtime;
use runtime::{End, ExtWindow, HiddenPage};

/// Largest file served from a package.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Largest file of its extension a content script reads with `fetch` (it travels as text).
const MAX_SCRIPT_READ_BYTES: u64 = 8 * 1024 * 1024;
/// The Web Store download limit and timeout.
const MAX_CRX_BYTES: usize = ext::MAX_PACKAGE_BYTES;
const CRX_TIMEOUT_MS: i32 = 60_000;
/// A message to a receiver that never answers is dropped after this.
const MESSAGE_TIMEOUT_MS: i64 = 30_000;
/// A background worker that has not run its script after this long gets its queued events
/// anyway (0.17.0; a large service worker imports many files).
const BACKGROUND_START_MS: i64 = 60_000;
/// The action popup's size until the page reports its own, and its bounds.
const POPUP_DEFAULT: (f64, f64) = (360.0, 240.0);
const POPUP_MAX: (f64, f64) = (800.0, 600.0);
const POPUP_MIN: (f64, f64) = (25.0, 25.0);

/// An installed extension as loaded from disk.
pub(super) struct LoadedExt {
    pub installed: Installed,
    pub manifest: Manifest,
    pub root: PathBuf,
    pub messages_json: String,
    pub dnr_filters: String,
    /// `chrome.action` state set at runtime.
    pub action_title: String,
    pub action_popup: Option<String>,
    pub action_icon: Option<String>,
    pub badge: String,
    pub badge_color: String,
    /// APIs the extension reached for at runtime that Krunk does not provide.
    pub runtime_unsupported: Vec<String>,
    /// Installed or updated in this run and not started since: its service worker gets the
    /// `install` and `activate` events once (0.17.0).
    pub fresh: bool,
}

impl LoadedExt {
    fn origin(&self) -> String {
        ext::origin(&self.installed.id)
    }

    fn has_tabs_permission(&self) -> bool {
        self.manifest.permissions.iter().any(|p| p == "tabs" || p == "activeTab")
    }

    fn action(&self) -> Option<ExtAction> {
        let a = self.manifest.action.as_ref()?;
        let icon = self.action_icon.clone().or_else(|| self.manifest.action_icon());
        let title = if self.action_title.is_empty() {
            a.default_title.clone().unwrap_or_else(|| self.installed.name.clone())
        } else {
            self.action_title.clone()
        };
        Some(ExtAction {
            id: self.installed.id.clone(),
            title,
            icon: icon.map(|p| format!("{}/{}", self.origin(), p.trim_start_matches('/'))),
            badge: self.badge.clone(),
            badge_color: self.badge_color.clone(),
            has_popup: self.popup_path().is_some(),
        })
    }

    fn popup_path(&self) -> Option<String> {
        match &self.action_popup {
            Some(p) if p.is_empty() => None,
            Some(p) => Some(p.clone()),
            None => self.manifest.action.as_ref()?.default_popup.clone().filter(|p| !p.is_empty()),
        }
    }
}

/// A content-script world of a tab's frame: extension `ext`'s own world (0.11.7, audit C4).
#[derive(Debug, Clone)]
struct ExtContext {
    id: i64,
    unique: String,
    main: bool,
    ext: String,
    /// The frame's origin as the engine reports it.
    origin: String,
}

/// Content-script bundles: (extension id, script).
/// `main`: for the page's own world (`world: "MAIN"`), without an API.
type Bundles = Vec<(String, String, bool)>;

/// Who asked, so the answer finds its way back.
#[derive(Clone)]
enum Caller {
    /// A content script: the tab, the world's context and the script's request id.
    Content { tab: TabId, context: i64, req: u64 },
    /// An extension page in a view (background, popup, offscreen document, window) or a tab.
    Page { view: View, req: Option<u32>, end: End },
}

impl Caller {
    /// Which of the extension's contexts this is.
    fn end(&self) -> End {
        match self {
            Caller::Content { tab, context, .. } => End::Content { tab: *tab, context: *context },
            Caller::Page { end, .. } => end.clone(),
        }
    }
}

/// The action popup window.
struct ExtPopup {
    window: Window,
    view: View,
    ext: String,
    url: String,
    anchor: ScreenRect,
    scale: f64,
    /// Shown once the page has loaded (a browser created inside an already active window
    /// took the activation away again).
    shown: bool,
    opened_at: i64,
}

/// A background page: hidden view; events wait until its scripts ran (the listeners exist).
struct Background {
    window: Window,
    view: View,
    ready: bool,
    queued: Vec<String>,
}

pub(super) struct ExtHost {
    pub(super) exts: RefCell<Vec<LoadedExt>>,
    /// Per-extension bundles (id, script) for normal and private tabs (empty: nothing to
    /// install). Each goes into the extension's own world (0.11.7, audit C4).
    bundles: RefCell<(Bundles, Bundles)>,
    storage: RefCell<HashMap<String, serde_json::Map<String, Value>>>,
    /// `storage.<area>.setAccessLevel`, by "<id>/<area>": may content scripts use the area?
    /// In memory; the extension sets it again when it starts.
    storage_access: RefCell<HashMap<String, bool>>,
    backgrounds: RefCell<HashMap<String, Background>>,
    popup: RefCell<Option<ExtPopup>>,
    /// Messages waiting for `respond`, with the extension that may answer them and how many
    /// receivers have not answered yet.
    pending: RefCell<HashMap<u64, (String, Caller, lb_core::ext_runtime::Broadcast)>>,
    next_id: Cell<u64>,
    /// Alarm generations (a cleared alarm's timer finds a newer generation and stops).
    alarms: RefCell<HashMap<String, (u64, Value)>>,
    /// The extension id of the page whose request is being dispatched, that page's address,
    /// and which of the extension's own views it came through (`None`: a tab).
    pub(super) request_origin: RefCell<Option<String>>,
    pub(super) request_url: RefCell<Option<String>>,
    request_from: RefCell<Option<End>>,
    /// Open ports (0.17.0).
    ports: RefCell<lb_core::ext_runtime::PortTable<End>>,
    /// Offscreen documents, by extension.
    offscreen: RefCell<HashMap<String, HiddenPage>>,
    /// The extensions' own windows, by `chrome.windows` id.
    windows: RefCell<HashMap<u32, ExtWindow>>,
    next_window: Cell<u32>,
    /// Content scripts registered at runtime, by extension: (script id, script).
    registered: RefCell<HashMap<String, Vec<(String, ext::ContentScript)>>>,
    /// Errors of the extensions' contexts, by extension (in memory).
    errors: RefCell<HashMap<String, Vec<lb_core::ext_runtime::ExtError>>>,
    errors_dirty: Cell<bool>,
    /// Log lines the extensions caused in this minute, by extension and kind of line.
    log_budget: RefCell<HashMap<(String, &'static str), lb_core::ext_guard::LogBudget>>,
    /// Per tab: the identifiers of the installed bundle scripts, and the extensions whose
    /// bindings are added.
    scripts: RefCell<HashMap<TabId, Vec<String>>>,
    bindings: RefCell<HashMap<TabId, std::collections::HashSet<String>>>,
    contexts: RefCell<HashMap<TabId, Vec<ExtContext>>>,
    /// `activeTab`: the tab and the origin of its page when the user last clicked the
    /// extension's button. The grant ends when the tab shows another origin.
    active_tab: RefCell<HashMap<String, (TabId, String)>>,
    /// A package waiting for the install confirmation (0.7.0, R20).
    pending_install: RefCell<Option<PendingInstall>>,
    /// The extension whose popup closed last, and when (ms): the toolbar click that closed it
    /// (by taking the focus) does not open it again (0.7.0, R21).
    popup_closed: RefCell<(String, i64)>,
}

/// While an extension's popup is open: a click outside it or another program in front closes
/// it (0.7.0, R21).
pub(super) const EXT_POPUP_TIMER: usize = 19;
const EXT_POPUP_TICK_MS: u32 = 50;
/// The click that opened it is still held for a moment.
const POPUP_GRACE_MS: i64 = 300;

/// A package's files, its id and whether the Web Store signed it.
type Prepared = (Vec<(String, Vec<u8>)>, String, bool);

/// A verified package and who asked for it, until the user answers.
pub(super) struct PendingInstall {
    request_id: u32,
    files: Vec<(String, Vec<u8>)>,
    id: String,
    source: String,
    /// The page that asked (krunk://extensions) and its request, or `None` for the store's
    /// "Add to Krunk" (the store tab gets a notice instead).
    tab: Option<TabId>,
    req_id: Option<u32>,
    store_tab: Option<TabId>,
}

impl ExtHost {
    pub(super) fn new() -> ExtHost {
        ExtHost {
            exts: RefCell::new(Vec::new()),
            bundles: RefCell::new((Vec::new(), Vec::new())),
            storage: RefCell::new(HashMap::new()),
            storage_access: RefCell::new(HashMap::new()),
            backgrounds: RefCell::new(HashMap::new()),
            popup: RefCell::new(None),
            pending: RefCell::new(HashMap::new()),
            next_id: Cell::new(1),
            alarms: RefCell::new(HashMap::new()),
            request_origin: RefCell::new(None),
            request_url: RefCell::new(None),
            request_from: RefCell::new(None),
            ports: RefCell::new(Default::default()),
            offscreen: RefCell::new(HashMap::new()),
            windows: RefCell::new(HashMap::new()),
            next_window: Cell::new(runtime::FIRST_EXT_WINDOW),
            registered: RefCell::new(HashMap::new()),
            errors: RefCell::new(HashMap::new()),
            errors_dirty: Cell::new(false),
            log_budget: RefCell::new(HashMap::new()),
            pending_install: RefCell::new(None),
            popup_closed: RefCell::new((String::new(), 0)),
            scripts: RefCell::new(HashMap::new()),
            bindings: RefCell::new(HashMap::new()),
            contexts: RefCell::new(HashMap::new()),
            active_tab: RefCell::new(HashMap::new()),
        }
    }
}

struct BackgroundEvents;
impl WindowEvents for BackgroundEvents {}

struct ExtPopupEvents;
impl WindowEvents for ExtPopupEvents {
    // Activation and focus bounce between the popup, its browser view and the main window
    // while it opens, so they decide nothing (0.7.0, R21): `ext_popup_tick` watches the mouse
    // and the foreground window instead.
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Reads every file under `dir` (relative names with `/`), within the package limits.
fn read_tree(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let p = e.path();
            if p.is_dir() {
                if !name.starts_with('.') {
                    stack.push((p, rel));
                }
                continue;
            }
            if !ext::safe_relative_path(&rel) {
                continue;
            }
            let bytes = std::fs::read(&p).map_err(|e| format!("{rel}: {e}"))?;
            total += bytes.len();
            if total > ext::MAX_UNPACKED_BYTES || out.len() >= ext::MAX_PACKAGE_FILES {
                return Err("the folder is too large for an extension".into());
            }
            out.push((rel, bytes));
        }
    }
    Ok(out)
}

/// The package's files with a single top-level folder stripped (zips made from a folder).
fn normalize_entries(mut files: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    if files.iter().any(|(n, _)| n == "manifest.json") {
        return files;
    }
    let prefixes: Vec<String> =
        files.iter().filter_map(|(n, _)| n.split_once('/').map(|(p, _)| p.to_string())).collect();
    if let Some(first) = prefixes.first() {
        if prefixes.iter().all(|p| p == first) && files.iter().any(|(n, _)| n == &format!("{first}/manifest.json")) {
            for (n, _) in files.iter_mut() {
                *n = n[first.len() + 1..].to_string();
            }
        }
    }
    files
}

/// A request's path as the file system names the file it opens, relative to the package: what
/// is compared with the manifest's lists (`sandbox.pages`, `web_accessible_resources`).
/// Decoded and in lower case (`ext_guard::package_path`); for a file that exists the name comes
/// from the file system, which also opens a file by its short name (`SANDBO~1.HTM`) or with
/// trailing spaces.
fn listed_path(root: &Path, url_path: &str) -> String {
    let rel = lb_core::ext_guard::package_path(url_path);
    let real = || {
        if !ext::safe_relative_path(&rel) {
            return None;
        }
        let root = std::fs::canonicalize(root).ok()?;
        let file = std::fs::canonicalize(ext::file_path(&root, &rel)).ok()?;
        let inside = file.strip_prefix(&root).ok()?;
        let names: Vec<String> = inside.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
        Some(names.join("/"))
    };
    real().unwrap_or(rel)
}

/// The DevTools events `ext_devtools_event` handles.
const DEVTOOLS_EVENTS: &[&str] = &[
    "Runtime.exceptionThrown",
    "Target.attachedToTarget",
    "Runtime.executionContextCreated",
    "Runtime.executionContextDestroyed",
    "Runtime.executionContextsCleared",
    "Runtime.bindingCalled",
];

impl Browser {
    // ------------------------------------------------------------ start-up and state

    fn ext_dir(&self) -> PathBuf {
        self.dirs.root().join("extensions")
    }

    fn ext_state_path(&self) -> PathBuf {
        self.ext_dir().join("state.json")
    }

    /// Loads the installed extensions at start-up.
    pub(super) fn init_extensions(&self) {
        let list: Vec<Installed> =
            read_json(&self.ext_state_path()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let mut loaded = Vec::new();
        for inst in list {
            match self.load_ext(inst.clone()) {
                Ok(l) => loaded.push(l),
                Err(e) => warning!("extension {} ({}): not loaded: {e}", inst.name, inst.id),
            }
        }
        let n = loaded.len();
        *self.ext.exts.borrow_mut() = loaded;
        if n > 0 {
            info!(
                "extensions: {n} installed ({} enabled)",
                self.ext.exts.borrow().iter().filter(|e| e.installed.enabled).count()
            );
        }
        self.ext_apply_all(false);
        let ids: Vec<String> = self.ext.exts.borrow().iter().map(|e| e.installed.id.clone()).collect();
        for id in ids {
            self.ext_send_background(&id, "startup", json!({}));
        }
    }

    fn load_ext(&self, installed: Installed) -> Result<LoadedExt, String> {
        if !ext::valid_id(&installed.id) {
            return Err("bad id".into());
        }
        let root = self.ext_dir().join(&installed.id);
        let text = std::fs::read_to_string(root.join("manifest.json")).map_err(|e| format!("manifest.json: {e}"))?;
        let manifest = Manifest::parse(&text).map_err(|e| e.to_string())?;
        let messages_json = Self::ext_messages(&root, &manifest, &self.ext_languages());
        let dnr_filters = Self::ext_dnr_text(&root, &manifest, &installed.name).0;
        Ok(LoadedExt {
            installed,
            manifest,
            root,
            messages_json,
            dnr_filters,
            action_title: String::new(),
            action_popup: None,
            action_icon: None,
            badge: String::new(),
            badge_color: String::new(),
            runtime_unsupported: Vec::new(),
            fresh: false,
        })
    }

    /// The user's languages (the Accept-Language setting, else the engine's).
    fn ext_languages(&self) -> Vec<String> {
        let s = self.settings.borrow();
        let list = if s.headers.accept_language.trim().is_empty() {
            "en-US,en".to_string()
        } else {
            s.headers.accept_language.clone()
        };
        list.split(',')
            .map(|l| l.trim().split(';').next().unwrap_or("").to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// `_locales/<default_locale>/messages.json` (the user's language first, if present).
    fn ext_messages(root: &Path, manifest: &Manifest, languages: &[String]) -> String {
        let mut candidates: Vec<String> = Vec::new();
        for lang in languages {
            candidates.push(lang.replace('-', "_"));
            if let Some((base, _)) = lang.split_once('-') {
                candidates.push(base.to_string());
            }
        }
        if let Some(d) = &manifest.default_locale {
            candidates.push(d.clone());
        }
        candidates.push("en".into());
        // Locale names only: they become a path (0.11.7, audit C5).
        for c in candidates.into_iter().filter(|c| ext::valid_locale(c)) {
            let p = root.join("_locales").join(&c).join("messages.json");
            if let Ok(t) = std::fs::read_to_string(&p) {
                if serde_json::from_str::<Value>(&t).is_ok() {
                    return t;
                }
            }
        }
        String::new()
    }

    /// The enabled rule resources translated: (filter text, translated, skipped).
    fn ext_dnr_text(root: &Path, manifest: &Manifest, name: &str) -> (String, usize, usize) {
        let mut text = String::new();
        let (mut t, mut s) = (0, 0);
        if let Some(d) = &manifest.declarative_net_request {
            for r in d.rule_resources.iter().filter(|r| r.enabled && ext::safe_relative_path(&r.path)) {
                if let Ok(json) = std::fs::read_to_string(ext::file_path(root, &r.path)) {
                    match ext::translate_dnr(&format!("{name} ({})", r.id), &json) {
                        Ok(tr) => {
                            text.push_str(&tr.filters);
                            t += tr.translated;
                            s += tr.skipped;
                        }
                        Err(e) => warning!("extension {name}: rules {}: {e}", r.id),
                    }
                }
            }
        }
        (text, t, s)
    }

    fn save_ext_state(&self) {
        let list: Vec<Installed> = self.ext.exts.borrow().iter().map(|e| e.installed.clone()).collect();
        let _ = std::fs::create_dir_all(self.ext_dir());
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            if let Err(e) = std::fs::write(self.ext_state_path(), json) {
                warning!("extensions: state not saved: {e}");
            }
        }
    }

    /// Everything that follows a change: bundles, backgrounds, actions, filters, the scheme
    /// hosts. `changed`: the list is sent to open krunk://extensions pages.
    fn ext_apply_all(&self, changed: bool) {
        for id in self.ext.exts.borrow().iter().map(|e| e.installed.id.clone()).collect::<Vec<_>>() {
            crate::ffi::cef::register_scheme_host(&format!("{id}{}", ext::HOST_SUFFIX));
        }
        self.ext_rebuild_bundles();
        self.ext_sync_backgrounds();
        self.ext_send_actions();
        self.ext_sync_filters();
        if changed {
            self.save_ext_state();
            let list = self.ext_list_json();
            for id in self.tabs_showing(Page::Extensions) {
                self.reply(Some(id), Event::ExtChanged { extensions: list.clone() });
            }
        }
    }

    pub(super) fn ext_rebuild_bundles(&self) {
        let shim = assets::inject_script("ext-content.js");
        // Who may load each extension's files from a tab: the tabs' network policy reads it.
        self.net.set_ext_access(
            self.ext
                .exts
                .borrow()
                .iter()
                .map(|e| {
                    let access = lb_core::ext_guard::WebAccess::new(&e.manifest, &self.ext_content_scripts(e));
                    (e.installed.id.clone(), access)
                })
                .collect(),
        );
        // One bundle per extension, for its own world (0.11.7, audit C4). An extension with
        // the `scripting` permission gets a world even without content scripts, for
        // `scripting.executeScript` (it used to borrow the shared world). 0.17.0: scripts
        // registered at runtime count like the manifest's, and `world: "MAIN"` scripts get a
        // second bundle for the page's own world.
        let build = |private: bool| -> Bundles {
            let exts = self.ext.exts.borrow();
            let mut out = Bundles::new();
            for e in exts.iter().filter(|e| e.installed.enabled && (!private || e.installed.allowed_in_private)) {
                let scripts = self.ext_content_scripts(e);
                let id = &e.installed.id;
                let read = |f: &String| {
                    std::fs::read_to_string(ext::file_path(&e.root, f))
                        .ok()
                        .map(|src| format!("{src}\n//# sourceURL={}/{}", ext::origin(id), f.trim_start_matches('/')))
                };
                let is_main = |cs: &ext::ContentScript| cs.world.as_deref() == Some("MAIN");
                if scripts.iter().any(|cs| !is_main(cs)) || ext::may_use_scripting(&e.manifest) {
                    let bundled = ext::BundledExtension {
                        id: id.clone(),
                        manifest_json: serde_json::to_string(&e.manifest).unwrap_or_else(|_| "{}".into()),
                        messages_json: e.messages_json.clone(),
                        log_all: runtime::log_all(),
                        scripts: scripts
                            .iter()
                            .filter(|cs| !is_main(cs))
                            .map(|cs| ext::BundledScript {
                                id: id.clone(),
                                matches: cs.matches.clone(),
                                exclude_matches: cs.exclude_matches.clone(),
                                all_frames: cs.all_frames,
                                run_at: cs.run_at.clone().unwrap_or_else(|| "document_idle".into()),
                                js: cs.js.iter().filter_map(read).collect(),
                                css: cs.css.clone(),
                            })
                            .collect(),
                    };
                    out.push((id.clone(), ext::content_bundle(shim, std::slice::from_ref(&bundled)), false));
                }
                let main: Vec<lb_core::ext_runtime::MainScript> = scripts
                    .iter()
                    .filter(|cs| is_main(cs) && !cs.js.is_empty())
                    .map(|cs| lb_core::ext_runtime::MainScript {
                        matches: cs.matches.clone(),
                        exclude_matches: cs.exclude_matches.clone(),
                        all_frames: cs.all_frames,
                        run_at: cs.run_at.clone().unwrap_or_else(|| "document_idle".into()),
                        js: cs.js.iter().filter_map(read).collect(),
                    })
                    .collect();
                if !main.is_empty() {
                    out.push((id.clone(), lb_core::ext_runtime::main_bundle(id, &main), true));
                }
            }
            out
        };
        let bundles = (build(false), build(true));
        let same = *self.ext.bundles.borrow() == bundles;
        *self.ext.bundles.borrow_mut() = bundles;
        if !same {
            let views: Vec<(TabId, View, bool)> =
                self.views.borrow().iter().map(|(id, v)| (*id, v.view.clone(), v.view.is_private())).collect();
            for (id, view, private) in views {
                self.ext_install_bundle(id, &view, private);
            }
        }
    }

    /// Installs (or replaces) the bundles in a tab: each extension's in its own isolated world,
    /// with its own binding.
    fn ext_install_bundle(&self, tab: TabId, view: &View, private: bool) {
        let bundles = {
            let b = self.ext.bundles.borrow();
            if private {
                b.1.clone()
            } else {
                b.0.clone()
            }
        };
        for old in self.ext.scripts.borrow_mut().remove(&tab).unwrap_or_default() {
            view.devtools_method("Page.removeScriptToEvaluateOnNewDocument", &json!({ "identifier": old }));
        }
        // Bindings of extensions that have no bundle here any more.
        let wanted: std::collections::HashSet<String> =
            bundles.iter().filter(|(_, _, main)| !main).map(|(id, _, _)| id.clone()).collect();
        if let Some(bound) = self.ext.bindings.borrow_mut().get_mut(&tab) {
            for id in bound.iter().filter(|id| !wanted.contains(*id)) {
                view.devtools_method("Runtime.removeBinding", &json!({ "name": ext::binding_name(id) }));
            }
            bound.retain(|id| wanted.contains(id));
        }
        if bundles.is_empty() {
            // No content scripts for this tab (any more): the domains go off again.
            if self.cdp_release(tab, view, super::cdp::CdpUser::Ext) {
                self.ext.contexts.borrow_mut().remove(&tab);
                self.ext.bindings.borrow_mut().remove(&tab);
            }
            return;
        }
        #[cfg(feature = "perf-probe")]
        if super::perf_probe::off("cdp") {
            return;
        }
        // The Page domain (the scripts added below run on new documents only while it is on)
        // and the Runtime domain (contexts, binding calls).
        self.cdp_use(tab, view, super::cdp::CdpUser::Ext);
        for (id, bundle, main) in bundles {
            let world = ext::world_name(&id);
            if !main && self.ext.bindings.borrow_mut().entry(tab).or_default().insert(id.clone()) {
                view.devtools_method(
                    "Runtime.addBinding",
                    &json!({ "name": ext::binding_name(&id), "executionContextName": world }),
                );
            }
            let weak = self.me.clone();
            // The page's own world for `world: "MAIN"` scripts: no world name, no binding.
            let params = if main {
                json!({ "source": bundle, "runImmediately": true })
            } else {
                json!({ "source": bundle, "worldName": world, "runImmediately": true })
            };
            view.devtools_call("Page.addScriptToEvaluateOnNewDocument", &params, move |r| {
                let Some(b) = weak.upgrade() else { return };
                match r {
                    Ok(v) => {
                        if let Some(script) = v.get("identifier").and_then(|i| i.as_str()) {
                            info!("tab {tab}: extension {} scripts installed ({script})", b.ext_name(&id));
                            b.ext.scripts.borrow_mut().entry(tab).or_default().push(script.to_string());
                        }
                    }
                    Err(e) => warning!("tab {tab}: extension {} scripts not installed: {e}", b.ext_name(&id)),
                }
            });
        }
    }

    /// A tab view is being set up: its DevTools events (the binding calls and the worlds).
    pub(super) fn ext_attach_tab(&self, tab: TabId, view: &View) {
        let weak = self.me.clone();
        view.on_devtools_event(DEVTOOLS_EVENTS, move |method, params| {
            if let Some(b) = weak.upgrade() {
                b.ext_devtools_event(tab, &method, &params);
            }
        });
    }

    /// The tab's browser exists now (DevTools calls before that are lost): the bundle, with the
    /// domains it needs, if any extension has content scripts.
    pub(super) fn ext_tab_created(&self, tab: TabId) {
        let Some(view) = self.tab_view(tab) else { return };
        self.ext.bindings.borrow_mut().remove(&tab);
        self.ext_install_bundle(tab, &view, view.is_private());
    }

    pub(super) fn ext_detach_tab(&self, tab: TabId) {
        self.ext_context_gone(|e| matches!(e, End::Tab(t) | End::Content { tab: t, .. } if *t == tab));
        self.ext.scripts.borrow_mut().remove(&tab);
        self.ext.bindings.borrow_mut().remove(&tab);
        self.ext.contexts.borrow_mut().remove(&tab);
        self.ext_tab_event("tabRemoved", json!({ "tabId": tab, "windowId": self.window_of(tab) }));
    }

    fn ext_devtools_event(&self, tab: TabId, method: &str, params: &Value) {
        match method {
            // A worker or frame paused for "Send client hints" off (0.7.0, `hints`).
            "Target.attachedToTarget" => self.hints_target_attached(tab, params),
            "Runtime.exceptionThrown" => self.ext_content_exception(tab, params),
            "Runtime.executionContextCreated" => {
                let Some(ctx) = params.get("context") else { return };
                let Some(ext_id) = ctx.get("name").and_then(|n| n.as_str()).and_then(ext::ext_of_world) else {
                    return;
                };
                let ext_id = ext_id.to_string();
                let id = ctx.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
                // The world exists in every document of the tab; on Krunk's own pages it is
                // never used (0.11.7, audit C1).
                let origin = ctx.get("origin").and_then(|o| o.as_str()).unwrap_or("");
                if !ext::content_context_allowed(origin) {
                    info!("tab {tab}: extension world {id} on an internal page: not used");
                    return;
                }
                let unique = ctx.get("uniqueId").and_then(|u| u.as_str()).unwrap_or("").to_string();
                info!("tab {tab}: extension {} world {id} created", self.ext_name(&ext_id));
                self.ext.contexts.borrow_mut().entry(tab).or_default().push(ExtContext {
                    id,
                    unique,
                    main: false,
                    ext: ext_id,
                    origin: origin.to_string(),
                });
                // Which frame: the main one gets tabs.sendMessage.
                if let Some(view) = self.tab_view(tab) {
                    let weak = self.me.clone();
                    let params =
                        json!({ "expression": "window.top === window", "contextId": id, "returnByValue": true });
                    view.devtools_call("Runtime.evaluate", &params, move |r| {
                        let Some(b) = weak.upgrade() else { return };
                        let main = r.ok().and_then(|v| v.pointer("/result/value")?.as_bool()) == Some(true);
                        let mut contexts = b.ext.contexts.borrow_mut();
                        if let Some(c) = contexts.get_mut(&tab).and_then(|list| list.iter_mut().find(|c| c.id == id)) {
                            c.main = main;
                        }
                    });
                }
            }
            "Runtime.executionContextDestroyed" => {
                let id = params.get("executionContextId").and_then(|i| i.as_i64());
                let unique = params.get("executionContextUniqueId").and_then(|u| u.as_str()).map(str::to_string);
                let mut gone: Vec<i64> = Vec::new();
                if let Some(list) = self.ext.contexts.borrow_mut().get_mut(&tab) {
                    list.retain(|c| {
                        let keep =
                            Some(c.id) != id && (unique.is_none() || Some(c.unique.as_str()) != unique.as_deref());
                        if !keep {
                            gone.push(c.id);
                        }
                        keep
                    });
                }
                // The frame navigated or went away: its ports close (0.17.0).
                if !gone.is_empty() {
                    self.ext_context_gone(
                        |e| matches!(e, End::Content { tab: t, context } if *t == tab && gone.contains(context)),
                    );
                }
            }
            "Runtime.executionContextsCleared" => {
                self.ext_context_gone(|e| matches!(e, End::Content { tab: t, .. } if *t == tab));
                self.ext.contexts.borrow_mut().remove(&tab);
            }
            "Runtime.bindingCalled" => {
                // The extension is the binding's: each extension's world has only its own
                // (0.11.7, audit C4). The message's own "ext" field is not trusted.
                let Some(ext_id) = params.get("name").and_then(|n| n.as_str()).and_then(ext::ext_of_binding) else {
                    return;
                };
                let ext_id = ext_id.to_string();
                let context = params.get("executionContextId").and_then(|i| i.as_i64()).unwrap_or(0);
                // Only from that extension's recorded world (not one on an internal page, C1).
                let known = self
                    .ext
                    .contexts
                    .borrow()
                    .get(&tab)
                    .is_some_and(|l| l.iter().any(|c| c.id == context && c.ext == ext_id));
                if !known {
                    warning!("tab {tab}: extension call from an unknown world {context}: ignored");
                    return;
                }
                let payload = params.get("payload").and_then(|p| p.as_str()).unwrap_or("");
                let Ok(msg) = serde_json::from_str::<Value>(payload) else { return };
                let cmd = msg.get("cmd").and_then(|c| c.as_str()).unwrap_or("").to_string();
                if self.ext_may_log(&ext_id, "call") {
                    let shown = lb_core::ext_guard::log_text(&cmd, 60);
                    info!("tab {tab}: extension {} call {shown}", self.ext_name(&ext_id));
                }
                let data = msg.get("data").cloned().unwrap_or(Value::Null);
                let req = msg.get("id").and_then(|i| i.as_u64());
                let enabled = self.ext.exts.borrow().iter().any(|e| e.installed.id == ext_id && e.installed.enabled);
                if !enabled {
                    return;
                }
                let caller = Caller::Content { tab, context, req: req.unwrap_or(0) };
                let result = self.ext_op(&caller, &ext_id, &cmd, data);
                if let (Some(result), Some(req)) = (result, req) {
                    self.ext_reply(&Caller::Content { tab, context, req }, &ext_id, result);
                }
            }
            _ => {}
        }
    }

    /// Extension `ext_id`'s content world in a tab: the main frame's, else any.
    fn ext_context(&self, tab: TabId, ext_id: &str) -> Option<i64> {
        let contexts = self.ext.contexts.borrow();
        let mine: Vec<&ExtContext> = contexts.get(&tab)?.iter().filter(|c| c.ext == ext_id).collect();
        mine.iter().find(|c| c.main).or(mine.first()).map(|c| c.id)
    }

    /// Where `scripting.executeScript` / `insertCSS` of `ext_id` run in a tab: the extension's
    /// world there, when the extension may touch the document. As for content scripts the
    /// document must be one the manifest's host patterns cover (the origin the engine reported
    /// for that world), or the user's click on the extension's button granted `activeTab` for
    /// it. `main_world`: the script goes to the top frame's own world, so that frame's world
    /// must be the one checked.
    fn ext_script_target(&self, tab: TabId, ext_id: &str, main_world: bool) -> Result<i64, String> {
        let found = {
            let contexts = self.ext.contexts.borrow();
            let mine: Vec<&ExtContext> =
                contexts.get(&tab).map(|l| l.iter().filter(|c| c.ext == ext_id).collect()).unwrap_or_default();
            mine.iter().find(|c| c.main).or(mine.first()).map(|c| (c.id, c.main, c.origin.clone()))
        };
        let Some((context, main, origin)) = found else {
            return Err("The tab has no extension world (not a web page, or not allowed there).".into());
        };
        if main_world && !main {
            return Err("The tab's top frame has no extension world (not a web page, or not allowed there).".into());
        }
        let by_manifest = self
            .ext
            .exts
            .borrow()
            .iter()
            .any(|e| e.installed.id == ext_id && lb_core::ext_guard::host_access(&e.manifest, &origin));
        let by_click = || self.ext.active_tab.borrow().get(ext_id).is_some_and(|(t, o)| *t == tab && *o == origin);
        if !by_manifest && !by_click() {
            return Err(lb_core::ext_guard::NO_HOST_ACCESS.into());
        }
        Ok(context)
    }

    /// The user clicked the extension's button: with the `activeTab` permission the extension
    /// may script the page the active tab shows now.
    fn ext_grant_active_tab(&self, ext_id: &str) {
        let wants = self
            .ext
            .exts
            .borrow()
            .iter()
            .any(|e| e.installed.id == ext_id && e.manifest.permissions.iter().any(|p| p == "activeTab"));
        let Some(tab) = self.active_id().filter(|_| wants) else { return };
        let origin = self
            .ext
            .contexts
            .borrow()
            .get(&tab)
            .and_then(|l| l.iter().find(|c| c.ext == ext_id && c.main).map(|c| c.origin.clone()))
            .or_else(|| self.tab(tab).and_then(|t| lb_core::urlclean::origin_of(&t.url)));
        if let Some(origin) = origin {
            self.ext.active_tab.borrow_mut().insert(ext_id.to_string(), (tab, origin));
        }
    }

    /// Delivers JSON to the content world of a tab (a reply or an event).
    fn ext_deliver_content(&self, tab: TabId, context: i64, json: &Value) {
        let Some(view) = self.tab_view(tab) else { return };
        let text = serde_json::to_string(&json.to_string()).unwrap_or_else(|_| "\"\"".into());
        let params = json!({ "expression": format!("window.__krunkExtDeliver && window.__krunkExtDeliver({text})"), "contextId": context, "returnByValue": true });
        view.devtools_method("Runtime.evaluate", &params);
    }

    fn ext_reply(&self, caller: &Caller, ext_id: &str, result: Result<Value, String>) {
        match caller {
            Caller::Content { tab, context, req } => {
                let msg = match result {
                    Ok(data) => json!({ "ext": ext_id, "reply": req, "data": data }),
                    Err(e) => json!({ "ext": ext_id, "reply": req, "error": e }),
                };
                self.ext_deliver_content(*tab, *context, &msg);
            }
            Caller::Page { view, req, .. } => {
                let ev = match result {
                    Ok(data) => Event::ok(*req, data),
                    Err(e) => Event::err(*req, e),
                };
                view.post_json(&ev.to_json());
            }
        }
    }

    // ------------------------------------------------------------ pages (background, popup, options)

    /// A request from an extension page (`Request::Ext`): `tab` when the page is in a tab.
    pub(super) fn ext_page_request(
        &self,
        tab: Option<TabId>,
        req_id: Option<u32>,
        ext_id: &str,
        op: &str,
        data: Value,
    ) {
        let origin = self.ext.request_origin.borrow().clone();
        if origin.as_deref() != Some(ext_id) {
            let shown = lb_core::ext_guard::log_text(ext_id, ext::ID_LEN);
            warning!("extension request for {shown} from another origin refused");
            return;
        }
        // Which of the extension's contexts: a tab, or the view the message came through.
        let end = match tab {
            Some(t) => End::Tab(t),
            None => match self.ext.request_from.borrow().clone() {
                Some(end) => end,
                None => return,
            },
        };
        // A view only ever speaks for the extension it was made for.
        let own_view = match &end {
            End::Background(id) | End::Popup(id) | End::Offscreen(id) => id == ext_id,
            End::Window(w) => self.ext.windows.borrow().get(w).is_some_and(|x| x.ext == ext_id),
            End::Tab(_) | End::Content { .. } => true,
        };
        if !own_view {
            let shown = lb_core::ext_guard::log_text(ext_id, ext::ID_LEN);
            warning!("extension request for {shown} from another extension's view refused");
            return;
        }
        // A sandboxed page (manifest `sandbox.pages`) has no extension API, as in Chrome.
        let path = self
            .ext
            .request_url
            .borrow()
            .as_deref()
            .and_then(|u| url::Url::parse(u).ok())
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        let sandboxed = self.ext.exts.borrow().iter().any(|e| {
            e.installed.id == ext_id && lb_core::ext_runtime::is_sandbox_page(&e.manifest, &listed_path(&e.root, &path))
        });
        if sandboxed {
            if self.ext_may_log(ext_id, "call") {
                let shown = lb_core::ext_guard::log_text(op, 60);
                warning!("extension {}: a sandboxed page asked for chrome.{shown}: refused", self.ext_name(ext_id));
            }
            return;
        }
        let Some(view) = self.ext_end_view(&end) else { return };
        let caller = Caller::Page { view, req: req_id, end };
        if let Some(result) = self.ext_op(&caller, ext_id, op, data) {
            self.ext_reply(&caller, ext_id, result);
        }
    }

    fn ext_send_background(&self, ext_id: &str, kind: &str, data: Value) {
        let json = Event::Ext { ext: ext_id.to_string(), kind: kind.to_string(), data }.to_json();
        if let Some(bg) = self.ext.backgrounds.borrow_mut().get_mut(ext_id) {
            if bg.ready {
                bg.view.post_json(&json);
            } else {
                bg.queued.push(json);
            }
        }
    }

    /// The background page finished loading: its listeners exist, queued events go out.
    fn ext_background_ready(&self, ext_id: &str) {
        let queued: Vec<String> = {
            let mut map = self.ext.backgrounds.borrow_mut();
            let Some(bg) = map.get_mut(ext_id) else { return };
            bg.ready = true;
            std::mem::take(&mut bg.queued)
        };
        if let Some(bg) = self.ext.backgrounds.borrow().get(ext_id) {
            for json in queued {
                bg.view.post_json(&json);
            }
        }
    }

    /// An event to every page context of the extension (the background, the popup, an
    /// offscreen document, its windows and its pages in tabs).
    fn ext_send_pages(&self, ext_id: &str, kind: &str, data: Value) {
        for end in self.ext_page_ends(ext_id) {
            self.ext_send_end(&end, ext_id, kind, data.clone());
        }
    }

    fn ext_sync_backgrounds(&self) {
        let wanted: Vec<(String, String, bool)> = self
            .ext
            .exts
            .borrow()
            .iter()
            .filter(|e| e.installed.enabled)
            .filter_map(|e| e.manifest.background_script().map(|(s, m)| (e.installed.id.clone(), s.to_string(), m)))
            .collect();
        let running: Vec<String> = self.ext.backgrounds.borrow().keys().cloned().collect();
        for id in &running {
            if !wanted.iter().any(|(w, _, _)| w == id) {
                self.ext_stop_background(id);
            }
        }
        for (id, _, _) in wanted {
            if !running.contains(&id) {
                self.ext_start_background(&id);
            }
        }
    }

    fn ext_start_background(&self, ext_id: &str) {
        let Some(app) = self.app() else { return };
        let window = match Window::create_popup(app, Rc::new(BackgroundEvents)) {
            Ok(w) => w,
            Err(e) => {
                warning!("extension {ext_id}: background window: {e}");
                return;
            }
        };
        // Queued events go out when the worker says its script ran (`bgReady`), not when this
        // relay page loaded (0.17.0).
        let view = self.ext_view(ext_id, End::Background(ext_id.to_string()));
        let url = format!("{}/_krunk/background.html", ext::origin(ext_id));
        if let Err(e) = view.start(window.hwnd(), &url) {
            warning!("extension {ext_id}: background page: {e}");
            window.destroy();
            return;
        }
        view.set_bounds(0, 0, 2, 2);
        // A worker that never says so (nothing was served, the engine refused it): the reason
        // is worth an error, and the queued events must not wait for ever.
        let weak = self.me.clone();
        let id_owned = ext_id.to_string();
        crate::ffi::cef::post_ui_delayed(BACKGROUND_START_MS, move || {
            let Some(b) = weak.upgrade() else { return };
            let waiting = b.ext.backgrounds.borrow().get(&id_owned).is_some_and(|bg| !bg.ready);
            if waiting {
                b.ext_error(
                    &id_owned,
                    "The background worker did not finish starting within a minute.",
                    "",
                    0,
                    "background",
                );
                b.ext_background_ready(&id_owned);
            }
        });
        info!("extension {}: background page started", self.ext_name(ext_id));
        self.ext
            .backgrounds
            .borrow_mut()
            .insert(ext_id.to_string(), Background { window, view, ready: false, queued: Vec::new() });
    }

    fn ext_stop_background(&self, ext_id: &str) {
        let bg = self.ext.backgrounds.borrow_mut().remove(ext_id);
        if let Some(bg) = bg {
            self.ext_context_gone(|e| *e == End::Background(ext_id.to_string()));
            bg.view.close();
            bg.window.destroy();
            info!("extension {}: background page stopped", self.ext_name(ext_id));
        }
    }

    /// A view for an extension page of `ext_id` (the background's relay page, the popup, an
    /// offscreen document, a window): own origin only. `end`: which context its requests are.
    fn ext_view(&self, ext_id: &str, end: End) -> View {
        let popup = matches!(end, End::Popup(_));
        let view = View::new(ViewOptions {
            role: Role::Popup,
            menu: Menu::None,
            private: false,
            scripts: Scripts::default(),
            background: if popup { UI_BACKGROUND } else { 0 },
            policy: None,
        });
        let own = ext::origin(ext_id);
        let weak = self.me.clone();
        view.on_before_browse(move |nav| {
            let ok = nav.url.starts_with(&format!("{own}/"));
            if !ok {
                // A link in a popup: a new tab, like target=_blank.
                if let Some(b) = weak.upgrade() {
                    if assets::allow_navigation(Channel::Tab, "", &nav.url, false, nav.redirected) {
                        b.open_url(&nav.url, true, false);
                    }
                }
            }
            ok
        });
        let weak = self.me.clone();
        view.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                *b.ext.request_from.borrow_mut() = Some(end.clone());
                b.on_ui_message(Channel::Ext, None, source, json);
                *b.ext.request_from.borrow_mut() = None;
            }
        });
        let weak = self.me.clone();
        view.on_open_tab(move |url, _foreground| {
            if let Some(b) = weak.upgrade() {
                b.open_url(&url, true, false);
            }
        });
        view
    }

    /// Tabs showing a built-in page.
    fn tabs_showing(&self, page: Page) -> Vec<TabId> {
        let ws = self.ws.borrow();
        let mut out = Vec::new();
        for win in ws.window_ids() {
            if let Some(m) = ws.model(win) {
                for t in m.infos() {
                    if m.get(t.id).is_some_and(|tab| Page::from_url(&tab.url) == Some(page)) {
                        out.push(t.id);
                    }
                }
            }
        }
        out
    }

    pub(super) fn ext_open_options_request(&self, id: &str) {
        self.ext_open_options(id);
    }

    /// How an extension download reaches the store: through the VPN while it runs, else as the
    /// proxy setting says. `None` with a SOCKS proxy, which WinHTTP can't use: going around it
    /// would reveal the user's address (0.14.0; it used to fall back to the system's settings).
    fn ext_proxy(&self) -> Option<http::ProxyMode> {
        self.own_route()
    }

    fn ext_name(&self, ext_id: &str) -> String {
        self.ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == ext_id)
            .map(|e| lb_core::ext_guard::log_text(&e.installed.name, 75))
            .unwrap_or_else(|| lb_core::ext_guard::log_text(ext_id, ext::ID_LEN))
    }

    // ------------------------------------------------------------ the action button and popup

    fn ext_actions(&self) -> Vec<ExtAction> {
        self.ext.exts.borrow().iter().filter(|e| e.installed.enabled).filter_map(LoadedExt::action).collect()
    }

    pub(super) fn ext_send_actions(&self) {
        let actions = self.ext_actions();
        self.post_shell(&Event::ExtActions { actions });
    }

    /// Right-click on an extension's toolbar button (0.16.0): its menu at the click. The shell
    /// only names the extension; what the menu says (the name, whether there is an options
    /// page, on or off) is read from the installed extension here.
    pub(super) fn show_ext_menu(&self, ext_id: &str, rect: PopupRect) {
        if !rect.is_valid() || !ext::valid_id(ext_id) {
            return;
        }
        let content = self.ext.exts.borrow().iter().find(|e| e.installed.id == ext_id).map(|e| PopupContent::ExtMenu {
            id: e.installed.id.clone(),
            name: e.installed.name.clone(),
            has_options: e.manifest.options().is_some(),
            enabled: e.installed.enabled,
        });
        match content {
            Some(c) => self.show_popup(c, Some(layout::ext_menu_rect(rect.x, rect.y))),
            None => warning!("extension menu: {ext_id} is not installed"),
        }
    }

    /// A choice in that menu. The popup is not trusted with the id: only the menu the host has
    /// open acts, for the extension it was built for (`ext::menu_may_act`), and only while that
    /// extension is still installed. Disable and Uninstall go through the handlers of
    /// krunk://extensions, so the toolbar and open extension pages follow as they do there.
    pub(super) fn ext_menu_action(&self, ext_id: &str, action: ExtMenuAction) {
        let from_its_menu = self
            .popup
            .try_borrow()
            .ok()
            .is_some_and(|p| p.as_ref().is_some_and(|p| p.open() && ext::menu_may_act(p.content.as_ref(), ext_id)));
        self.hide_popup();
        let installed = self.ext.exts.borrow().iter().any(|e| e.installed.id == ext_id);
        if !from_its_menu || !installed {
            warning!("extension menu: a choice for an extension whose menu is not open was refused ({action:?})");
            return;
        }
        match action {
            ExtMenuAction::Details => self.open_page_at(Page::Extensions, Some(ext_id)),
            ExtMenuAction::Options => self.ext_open_options(ext_id),
            ExtMenuAction::Manage => self.open_page(Page::Extensions),
            ExtMenuAction::Disable => self.ext_set(ext_id, Some(false), None),
            ExtMenuAction::Uninstall => self.ext_remove(ext_id),
        }
    }

    /// The toolbar button: the popup, or `action.onClicked` to the background.
    pub(super) fn ext_action_clicked(&self, ext_id: &str, rect: PopupRect) {
        let popup = self
            .ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == ext_id && e.installed.enabled)
            .and_then(|e| e.popup_path());
        let (closed, at) = self.ext.popup_closed.borrow().clone();
        if popup.is_some() && closed == ext_id && now_ms() - at < 400 {
            info!("extension {}: popup stays closed (its button was the click away)", self.ext_name(ext_id));
            return;
        }
        self.ext_grant_active_tab(ext_id);
        match popup {
            Some(p) => self.open_ext_popup(ext_id, &p, rect),
            None => {
                let tab = self.active_id().map(|t| self.ext_tab_json(ext_id, t)).unwrap_or(Value::Null);
                self.ext_send_background(ext_id, "actionClicked", json!({ "tab": tab }));
            }
        }
    }

    fn open_ext_popup(&self, ext_id: &str, path: &str, rect: PopupRect) {
        self.close_ext_popup();
        self.hide_popup();
        let Some(win) = self.cur_id() else { return };
        let Some(f) = self.frame(win) else { return };
        let scale = f.window.scale();
        let origin = f.window.client_to_screen(0, 0);
        let anchor = ScreenRect {
            x: origin.0 + (rect.x * scale).round() as i32,
            y: origin.1 + (rect.y * scale).round() as i32,
            width: (rect.width * scale).round() as i32,
            height: (rect.height * scale).round() as i32,
        };
        let window = match Window::create_popup(f.window, Rc::new(ExtPopupEvents)) {
            Ok(w) => w,
            Err(e) => {
                warning!("extension popup: {e}");
                return;
            }
        };
        let view = self.ext_view(ext_id, End::Popup(ext_id.to_string()));
        // Esc closes it (0.7.0, R21); the page sees the key first only if it handles it itself.
        let weak = self.me.clone();
        view.on_key(move |key| {
            if key.vk == lb_core::keys::vk::ESCAPE && !key.ctrl && !key.alt && !key.shift {
                if let Some(b) = weak.upgrade() {
                    b.defer(|b| b.close_ext_popup_because("Esc"));
                }
                return true;
            }
            false
        });
        let weak = self.me.clone();
        view.on_load_finished(move |_| {
            if let Some(b) = weak.upgrade() {
                b.show_ext_popup();
            }
        });
        let url = format!("{}/{}", ext::origin(ext_id), path.trim_start_matches('/'));
        if let Err(e) = view.start(window.hwnd(), &url) {
            warning!("extension popup: {e}");
            window.destroy();
            return;
        }
        // The largest size first (hidden): the page measures its body and reports.
        view.set_bounds(0, 0, (POPUP_MAX.0 * scale).round() as i32, (POPUP_MAX.1 * scale).round() as i32);
        let popup =
            ExtPopup { window, view, ext: ext_id.to_string(), url, anchor, scale, shown: false, opened_at: now_ms() };
        *self.ext.popup.borrow_mut() = Some(popup);
        info!("extension {}: popup loading", self.ext_name(ext_id));
    }

    /// The popup page loaded: if it has not reported a size within a moment, show it at the
    /// default size (the report shows it at its own size first).
    fn show_ext_popup(&self) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(600, move || {
            if let Some(b) = weak.upgrade() {
                // Only a popup that never reported its size; a shown one keeps its size.
                let unshown = b.ext.popup.borrow().as_ref().is_some_and(|p| !p.shown);
                if unshown {
                    b.show_ext_popup_sized(POPUP_DEFAULT.0, POPUP_DEFAULT.1);
                }
            }
        });
    }

    fn show_ext_popup_sized(&self, w: f64, h: f64) {
        let mut guard = self.ext.popup.borrow_mut();
        let Some(p) = guard.as_mut() else { return };
        if p.shown {
            self.place_ext_popup(p, w, h);
            return;
        }
        p.shown = true;
        p.opened_at = now_ms();
        self.place_ext_popup(p, w, h);
        p.view.set_visible(true);
        p.view.focus();
        if let Some(app) = self.app() {
            app.set_timer(EXT_POPUP_TIMER, EXT_POPUP_TICK_MS);
        }
        info!("extension {}: popup opened ({}x{})", self.ext_name(&p.ext), w as i32, h as i32);
    }

    /// Places the popup under its button, `w`×`h` CSS px, within the work area.
    fn place_ext_popup(&self, p: &ExtPopup, w: f64, h: f64) {
        let w = w.clamp(POPUP_MIN.0, POPUP_MAX.0);
        let h = h.clamp(POPUP_MIN.1, POPUP_MAX.1);
        let work = Window::work_area_at(p.anchor.x, p.anchor.y);
        // The popup's rect in shell CSS px (right-aligned under the button), as the other
        // popups are placed; the screen origin is the anchor's window.
        let css = PopupRect {
            x: f64::from(p.anchor.width) / p.scale - w,
            y: f64::from(p.anchor.height) / p.scale + 2.0,
            width: w,
            height: h,
        };
        let r = layout::popup_screen_rect(
            css,
            p.scale,
            (p.anchor.x, p.anchor.y),
            layout::Rect { x: work.x, y: work.y, width: work.width, height: work.height },
        );
        p.window.set_activatable(true);
        p.window.show_at(ScreenRect { x: r.x, y: r.y, width: r.width, height: r.height }, true);
        p.view.set_bounds(0, 0, r.width, r.height);
    }

    pub(super) fn ext_popup_open(&self) -> bool {
        self.ext.popup.try_borrow().ok().is_some_and(|p| p.as_ref().is_some_and(|p| p.shown))
    }

    /// `EXT_POPUP_TIMER`: a mouse button down outside the popup (the page, the toolbar, the tab
    /// strip, another window) or another program in front closes it.
    pub(super) fn ext_popup_tick(&self) {
        let state = self.ext.popup.borrow().as_ref().map(|p| (p.shown, p.opened_at, p.window.rect()));
        let Some((shown, opened_at, r)) = state else {
            if let Some(app) = self.app() {
                app.kill_timer(EXT_POPUP_TIMER);
            }
            return;
        };
        if !shown || now_ms() - opened_at < POPUP_GRACE_MS {
            return;
        }
        if !crate::ffi::window::foreground_is_ours() {
            self.close_ext_popup_because("another program came to the front");
            return;
        }
        if crate::ffi::cursor::buttons() != 0 {
            let (x, y) = crate::ffi::cursor::position();
            let inside = x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
            if !inside {
                self.close_ext_popup_because("a click outside it");
            }
        }
    }

    pub(super) fn close_ext_popup(&self) {
        self.close_ext_popup_because("closed");
    }

    pub(super) fn close_ext_popup_because(&self, why: &str) {
        let taken = self.ext.popup.try_borrow_mut().ok().and_then(|mut p| p.take());
        if let Some(p) = taken {
            info!("extension {}: popup {why}", self.ext_name(&p.ext));
            *self.ext.popup_closed.borrow_mut() = (p.ext.clone(), now_ms());
            self.ext_context_gone(|e| *e == End::Popup(p.ext.clone()));
            p.view.close();
            p.window.destroy();
        }
    }

    // ------------------------------------------------------------ the API

    /// Runs one `chrome.*` operation. `None`: the answer comes later (messaging).
    fn ext_op(&self, caller: &Caller, ext_id: &str, op: &str, data: Value) -> Option<Result<Value, String>> {
        let get_str = |k: &str| data.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let result: Result<Value, String> = match op {
            "ready" => {
                // A new document in this context (a page loaded or reloaded): the ports of
                // the one before it are gone.
                let end = caller.end();
                if !matches!(end, End::Content { .. } | End::Background(_)) {
                    self.ext_context_gone(|e| *e == end);
                }
                Ok(Value::Null)
            }
            // The background worker ran its script: its listeners exist, queued events go.
            "bgReady" => {
                if caller.end() == End::Background(ext_id.to_string()) {
                    if let Some(e) = self.ext.exts.borrow_mut().iter_mut().find(|e| e.installed.id == ext_id) {
                        e.fresh = false;
                    }
                    self.ext_background_ready(ext_id);
                }
                Ok(Value::Null)
            }
            "log" => {
                let text = get_str("text").unwrap_or_default();
                if get_str("level").as_deref() == Some("error") {
                    // krunk.log and the extension's Errors list (0.17.0).
                    let file = get_str("file").unwrap_or_default();
                    let line = data.get("line").and_then(|l| l.as_u64()).unwrap_or(0);
                    self.ext_error(ext_id, &text, &file, line, caller.end().context_name());
                } else if runtime::log_all() {
                    // KRUNK_EXT_LOG=1: every console level of the extension's contexts.
                    let text = lb_core::ext_guard::log_text(&text, 2000);
                    let level = lb_core::ext_guard::log_text(&get_str("level").unwrap_or_default(), 8);
                    info!("extension {}: [{} {level}] {text}", self.ext_name(ext_id), caller.end().context_name());
                }
                Ok(Value::Null)
            }
            "port.open" | "port.accept" | "port.post" | "port.close" => {
                self.ext_port_op(caller, ext_id, op, &data);
                return None;
            }
            "unsupported" => {
                let api = lb_core::ext_guard::log_text(&get_str("api").unwrap_or_default(), 80);
                // Not an API: bundlers and promise code probe objects for these.
                if api.starts_with("__") || api == "default" || api == "toJSON" || api.is_empty() {
                    return Some(Ok(Value::Null));
                }
                let name = self.ext_name(ext_id);
                let mut exts = self.ext.exts.borrow_mut();
                if let Some(e) = exts.iter_mut().find(|e| e.installed.id == ext_id) {
                    if !e.runtime_unsupported.contains(&api) {
                        e.runtime_unsupported.push(api.clone());
                        info!("extension {name}: uses chrome.{api}, which Krunk does not provide");
                    }
                }
                Ok(Value::Null)
            }
            "storage.get" | "storage.set" | "storage.remove" | "storage.clear" | "storage.bytes" => {
                // A content script uses the session area only when the extension opened it.
                let area = data.get("area").and_then(|a| a.as_str()).unwrap_or("local");
                if matches!(caller, Caller::Content { .. }) && !self.ext_storage_open(ext_id, area) {
                    Err(lb_core::ext_guard::STORAGE_NOT_ALLOWED.into())
                } else {
                    self.ext_storage_op(ext_id, op, &data)
                }
            }
            // From the extension's pages and background only (trusted contexts).
            "storage.setAccessLevel" => {
                let area = get_str("area").unwrap_or_default();
                let level = lb_core::ext_guard::parse_access_level(&get_str("accessLevel").unwrap_or_default());
                match (caller, level) {
                    (Caller::Content { .. }, _) => Err(lb_core::ext_guard::STORAGE_NOT_ALLOWED.into()),
                    (_, Some(open)) if matches!(area.as_str(), "local" | "sync" | "session") => {
                        self.ext.storage_access.borrow_mut().insert(Self::ext_storage_key(ext_id, &area), open);
                        Ok(Value::Null)
                    }
                    _ => Err("Invalid access level or storage area.".into()),
                }
            }
            // A content script's `fetch` of a file of its own extension.
            "resource.read" => match caller {
                Caller::Content { .. } => self.ext_read_resource(ext_id, &get_str("url").unwrap_or_default()),
                Caller::Page { .. } => Err("chrome.resource.read is not provided by Krunk".into()),
            },
            "runtime.sendMessage" => return self.ext_route_message(caller, ext_id, &data, None),
            "respond" => {
                self.ext_respond(ext_id, &data);
                return None;
            }
            "runtime.getContexts" => Ok(self.ext_contexts_json(ext_id, &data)),
            "offscreen.createDocument" => self.ext_offscreen_create(ext_id, &get_str("url").unwrap_or_default()),
            "offscreen.closeDocument" => {
                if self.ext_offscreen_close(ext_id) {
                    Ok(Value::Null)
                } else {
                    Err("No current offscreen document.".into())
                }
            }
            "offscreen.hasDocument" => Ok(json!(self.ext.offscreen.borrow().contains_key(ext_id))),
            "tabs.query" => Ok(Value::Array(self.ext_tabs_query(ext_id, &data))),
            "tabs.get" => {
                let id = data.get("tabId").and_then(|i| i.as_u64()).unwrap_or(0) as TabId;
                if self.window_of(id).is_some() {
                    Ok(self.ext_tab_json(ext_id, id))
                } else {
                    Err(format!("No tab with id: {id}."))
                }
            }
            "tabs.getCurrent" => Ok(match caller {
                Caller::Content { tab, .. } => self.ext_tab_json(ext_id, *tab),
                Caller::Page { end: End::Tab(t), .. } => self.ext_tab_json(ext_id, *t),
                Caller::Page { view, .. } => self
                    .views
                    .borrow()
                    .iter()
                    .find(|(_, v)| v.view.same(view))
                    .map(|(id, _)| self.ext_tab_json(ext_id, *id))
                    .unwrap_or(Value::Null),
            }),
            "tabs.create" => {
                // An address the extension gives is not the host's (no local file, none of
                // Krunk's pages); without one it is a new tab.
                let url = match get_str("url") {
                    Some(url) => {
                        let url = self.ext_resolve_url(ext_id, &url);
                        if !lb_core::ext_guard::may_open(ext_id, &url) {
                            return Some(Err("That address cannot be opened.".into()));
                        }
                        url
                    }
                    None => lb_core::pages::Page::NewTab.url(),
                };
                let active = data.get("active").and_then(|a| a.as_bool()).unwrap_or(true);
                self.open_url(&url, true, false);
                let id = self.active_id();
                if !active {
                    // Chrome opens it in the background; Krunk's new tab is active: leave it.
                }
                Ok(id.map(|t| self.ext_tab_json(ext_id, t)).unwrap_or(Value::Null))
            }
            "tabs.update" => {
                let id = data.get("tabId").and_then(|i| i.as_u64()).map(|i| i as TabId).or_else(|| self.active_id());
                let Some(id) = id else { return Some(Err("No active tab.".into())) };
                if let Some(url) = data.pointer("/props/url").and_then(|u| u.as_str()) {
                    let url = self.ext_resolve_url(ext_id, url);
                    if !lb_core::ext_guard::may_open(ext_id, &url) {
                        return Some(Err("That address cannot be opened.".into()));
                    }
                    if let Some(v) = self.tab_view(id) {
                        v.navigate(&url);
                    }
                }
                if data.pointer("/props/active").and_then(|a| a.as_bool()) == Some(true) {
                    self.ext_activate_tab(id);
                }
                Ok(self.ext_tab_json(ext_id, id))
            }
            "tabs.remove" => {
                for id in data.get("tabIds").and_then(|t| t.as_array()).cloned().unwrap_or_default() {
                    if let Some(id) = id.as_u64() {
                        self.close_tab(id as TabId);
                    }
                }
                Ok(Value::Null)
            }
            "tabs.reload" => {
                let id = data.get("tabId").and_then(|i| i.as_u64()).map(|i| i as TabId).or_else(|| self.active_id());
                if let Some(v) = id.and_then(|i| self.tab_view(i)) {
                    v.reload();
                }
                Ok(Value::Null)
            }
            "tabs.sendMessage" => {
                let Some(id) = data.get("tabId").and_then(|i| i.as_u64()).map(|i| i as TabId) else {
                    return Some(Err("tabs.sendMessage needs a tab id".into()));
                };
                return self.ext_route_message(caller, ext_id, &data, Some(id));
            }
            "runtime.openOptionsPage" => {
                self.ext_open_options(ext_id);
                Ok(Value::Null)
            }
            "runtime.reload" => {
                // Not from inside the request of a view that is about to close.
                let id = ext_id.to_string();
                self.defer(move |b| {
                    b.close_ext_popup();
                    b.ext_close_contexts(&id);
                    b.ext_stop_background(&id);
                    b.ext_sync_backgrounds();
                });
                return None;
            }
            "action.setTitle"
            | "action.setPopup"
            | "action.setIcon"
            | "action.setBadgeText"
            | "action.setBadgeBackgroundColor" => {
                {
                    let mut exts = self.ext.exts.borrow_mut();
                    if let Some(e) = exts.iter_mut().find(|e| e.installed.id == ext_id) {
                        match op {
                            "action.setTitle" => {
                                e.action_title = get_str("title").unwrap_or_default().chars().take(200).collect()
                            }
                            "action.setPopup" => e.action_popup = Some(get_str("popup").unwrap_or_default()),
                            "action.setIcon" => e.action_icon = get_str("path").filter(|p| ext::safe_relative_path(p)),
                            "action.setBadgeText" => {
                                e.badge = get_str("text").unwrap_or_default().chars().take(4).collect()
                            }
                            _ => e.badge_color = Self::ext_color(data.get("color")),
                        }
                    }
                }
                self.ext_send_actions();
                Ok(Value::Null)
            }
            "action.getTitle" | "action.getPopup" | "action.getBadgeText" => {
                let exts = self.ext.exts.borrow();
                let e = exts.iter().find(|e| e.installed.id == ext_id);
                Ok(match (op, e) {
                    ("action.getTitle", Some(e)) => json!(e.action().map(|a| a.title).unwrap_or_default()),
                    ("action.getPopup", Some(e)) => {
                        json!(e.popup_path().map(|p| format!("{}/{p}", e.origin())).unwrap_or_default())
                    }
                    (_, Some(e)) => json!(e.badge),
                    _ => json!(""),
                })
            }
            "alarms.create" => {
                let name = get_str("name").unwrap_or_default();
                let info = data.get("info").cloned().unwrap_or(json!({}));
                self.ext_alarm_create(ext_id, &name, &info);
                Ok(Value::Null)
            }
            "alarms.get" => {
                let name = get_str("name").unwrap_or_default();
                Ok(self
                    .ext
                    .alarms
                    .borrow()
                    .get(&format!("{ext_id}/{name}"))
                    .map(|(_, a)| a.clone())
                    .unwrap_or(Value::Null))
            }
            "alarms.getAll" => Ok(Value::Array(
                self.ext
                    .alarms
                    .borrow()
                    .iter()
                    .filter(|(k, _)| k.starts_with(&format!("{ext_id}/")))
                    .map(|(_, (_, a))| a.clone())
                    .collect(),
            )),
            "alarms.clear" => {
                let name = get_str("name").unwrap_or_default();
                Ok(json!(self.ext.alarms.borrow_mut().remove(&format!("{ext_id}/{name}")).is_some()))
            }
            "alarms.clearAll" => {
                self.ext.alarms.borrow_mut().retain(|k, _| !k.starts_with(&format!("{ext_id}/")));
                Ok(json!(true))
            }
            // A page in one of the extension's own windows is in that window.
            "windows.getCurrent" => Ok(match caller.end() {
                End::Window(id) => self.ext_own_window_json(id),
                End::Tab(t) => self.ext_window_json(self.window_of(t)),
                _ => self.ext_window_json(self.cur_id()),
            }),
            "windows.getLastFocused" => Ok(self.ext_window_json(self.cur_id())),
            "windows.getAll" => {
                let mut all: Vec<Value> = self.all_frames().iter().map(|f| self.ext_window_json(Some(f.id))).collect();
                all.extend(self.ext_own_windows(ext_id).into_iter().map(|id| self.ext_own_window_json(id)));
                Ok(Value::Array(all))
            }
            "windows.get" | "windows.update" | "windows.remove" => {
                let id = data.get("windowId").and_then(|i| i.as_u64()).unwrap_or(u64::MAX);
                match (self.ext_owns_window(ext_id, id), op) {
                    (Some(own), "windows.get") => Ok(self.ext_own_window_json(own)),
                    (Some(own), "windows.update") => {
                        Ok(self.ext_window_update(own, data.get("info").unwrap_or(&Value::Null)))
                    }
                    (Some(own), _) => {
                        self.defer(move |b| b.ext_window_close(own));
                        Ok(Value::Null)
                    }
                    (None, "windows.remove") => Err(format!("No window with id: {id}.")),
                    (None, _) => match self.all_frames().iter().find(|f| u64::from(f.id) == id) {
                        Some(f) => {
                            if op == "windows.update"
                                && data.pointer("/info/focused").and_then(|f| f.as_bool()) == Some(true)
                            {
                                f.window.bring_to_front();
                            }
                            Ok(self.ext_window_json(Some(f.id)))
                        }
                        None => Err(format!("No window with id: {id}.")),
                    },
                }
            }
            "windows.create" => self.ext_window_create(ext_id, &data),
            // `window.close()` of a page that has a view or a tab of its own.
            "closeSelf" => {
                match caller.end() {
                    End::Window(id) => self.defer(move |b| b.ext_window_close(id)),
                    End::Popup(_) => self.defer(|b| b.close_ext_popup_because("closed by its page")),
                    End::Tab(t) => self.defer(move |b| b.close_tab(t)),
                    _ => {}
                }
                return None;
            }
            "scripting.executeScript" => return self.ext_execute_script(caller, ext_id, &data),
            "scripting.insertCSS" => self.ext_scripting(ext_id, op, &data),
            "scripting.registerContentScripts"
            | "scripting.getRegisteredContentScripts"
            | "scripting.unregisterContentScripts" => self.ext_register_scripts(ext_id, op, &data),
            "permissions.getAll" => {
                let exts = self.ext.exts.borrow();
                let e = exts.iter().find(|e| e.installed.id == ext_id);
                Ok(json!({
                    "permissions": e.map(|e| e.manifest.api_permissions().iter().map(|p| p.to_string()).collect::<Vec<_>>()).unwrap_or_default(),
                    "origins": e.map(|e| e.manifest.scope()).unwrap_or_default(),
                }))
            }
            "dnr.getEnabledRulesets" => {
                let exts = self.ext.exts.borrow();
                let ids: Vec<String> = exts
                    .iter()
                    .find(|e| e.installed.id == ext_id)
                    .and_then(|e| e.manifest.declarative_net_request.as_ref())
                    .map(|d| d.rule_resources.iter().filter(|r| r.enabled).map(|r| r.id.clone()).collect())
                    .unwrap_or_default();
                Ok(json!(ids))
            }
            "popupSize" => {
                let w = data.get("width").and_then(|v| v.as_f64()).unwrap_or(POPUP_DEFAULT.0);
                let h = data.get("height").and_then(|v| v.as_f64()).unwrap_or(POPUP_DEFAULT.1);
                // Every page says its size; only the action popup's matters.
                let mine = caller.end() == End::Popup(ext_id.to_string())
                    && self.ext.popup.borrow().as_ref().is_some_and(|p| p.ext == ext_id);
                if mine {
                    self.show_ext_popup_sized(w, h);
                }
                Ok(Value::Null)
            }
            "closePopup" => {
                if caller.end() == End::Popup(ext_id.to_string())
                    && self.ext.popup.borrow().as_ref().is_some_and(|p| p.ext == ext_id)
                {
                    self.close_ext_popup_because("Escape");
                }
                Ok(Value::Null)
            }
            other => Err(format!("chrome.{other} is not provided by Krunk")),
        };
        Some(result)
    }

    fn ext_color(v: Option<&Value>) -> String {
        match v {
            Some(Value::String(s)) => s.chars().take(32).collect(),
            Some(Value::Array(a)) if a.len() >= 3 => {
                let n = |i: usize| a.get(i).and_then(|x| x.as_u64()).unwrap_or(0).min(255);
                format!("rgb({},{},{})", n(0), n(1), n(2))
            }
            _ => String::new(),
        }
    }

    /// `chrome.runtime.getURL`-style resolution of a relative address.
    fn ext_resolve_url(&self, ext_id: &str, url: &str) -> String {
        // An address the extension built itself in Chrome's form is its page here.
        let url = &lb_core::ext_runtime::krunk_url(url);
        if url.contains("://") || url.starts_with("about:") || url.starts_with("krunk://") {
            lb_core::pages::Page::from_display(url).map(|p| p.url()).unwrap_or_else(|| url.to_string())
        } else {
            // Normalized ("./page.html" is "page.html"): the address is compared later.
            let joined = format!("{}/{}", ext::origin(ext_id), url.trim_start_matches('/'));
            url::Url::parse(&joined).map(String::from).unwrap_or(joined)
        }
    }

    fn ext_open_options(&self, ext_id: &str) {
        let page = self
            .ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == ext_id)
            .and_then(|e| e.manifest.options().map(str::to_string));
        match page {
            Some(p) => self.open_url(&format!("{}/{}", ext::origin(ext_id), p.trim_start_matches('/')), true, false),
            None => info!("extension {}: no options page", self.ext_name(ext_id)),
        }
    }

    fn ext_activate_tab(&self, id: TabId) {
        let Some(win) = self.window_of(id) else { return };
        let now = now_ms();
        let ops =
            self.ws.borrow_mut().update(win, |m| ((), m.activate(id, now))).map(|(_, ops)| ops).unwrap_or_default();
        self.apply(ops);
    }

    fn ext_window_json(&self, win: Option<super::WindowId>) -> Value {
        match win {
            Some(w) => {
                let mut win = json!({ "id": w, "focused": self.cur_id() == Some(w), "incognito": false, "type": "normal", "state": "normal", "alwaysOnTop": false });
                // Where it is, in DIP (0.17.0): extensions place their own windows by it.
                if let Some(f) = self.frame(w) {
                    let (r, s) = (f.window.rect(), f.window.scale().max(0.1));
                    win["left"] = json!((f64::from(r.x) / s).round());
                    win["top"] = json!((f64::from(r.y) / s).round());
                    win["width"] = json!((f64::from(r.width) / s).round());
                    win["height"] = json!((f64::from(r.height) / s).round());
                    if f.window.is_minimized() {
                        win["state"] = json!("minimized");
                    } else if f.window.is_maximized() {
                        win["state"] = json!("maximized");
                    }
                }
                win
            }
            None => Value::Null,
        }
    }

    /// A `chrome.tabs.Tab` for the extension: the address and title only with the `tabs`
    /// permission or a host permission for the page.
    fn ext_tab_json(&self, ext_id: &str, id: TabId) -> Value {
        let Some(win) = self.window_of(id) else { return Value::Null };
        let ws = self.ws.borrow();
        let Some(m) = ws.model(win) else { return Value::Null };
        let Some(t) = m.get(id) else { return Value::Null };
        let index = m.infos().iter().position(|i| i.id == id).unwrap_or(0);
        let active = m.active_id() == Some(id);
        let exts = self.ext.exts.borrow();
        let e = exts.iter().find(|e| e.installed.id == ext_id);
        let may_see = e.is_some_and(|e| {
            e.has_tabs_permission()
                || e.manifest.scope().iter().filter_map(|p| ext::MatchPattern::parse(p)).any(|p| p.matches(&t.url))
        });
        let (audible, muted) =
            self.views.borrow().get(&id).map(|v| (v.audio.get(), v.view.audio_muted())).unwrap_or((false, false));
        let mut tab = json!({
            "id": id, "windowId": win, "index": index, "active": active, "highlighted": active, "selected": active,
            "pinned": false, "incognito": t.private, "discarded": t.asleep, "autoDiscardable": true,
            "status": if t.loading { "loading" } else { "complete" }, "audible": audible, "mutedInfo": { "muted": muted },
            "groupId": -1,
        });
        if may_see {
            tab["url"] = json!(t.url);
            tab["title"] = json!(t.title);
            if let Some(f) = &t.favicon {
                tab["favIconUrl"] = json!(f);
            }
        }
        tab
    }

    fn ext_tabs_query(&self, ext_id: &str, q: &Value) -> Vec<Value> {
        let active = q.get("active").and_then(|v| v.as_bool());
        let current =
            q.get("currentWindow").and_then(|v| v.as_bool()).or(q.get("lastFocusedWindow").and_then(|v| v.as_bool()));
        let window_id = q.get("windowId").and_then(|v| v.as_u64());
        let patterns: Vec<ext::MatchPattern> = match q.get("url") {
            Some(Value::String(s)) => ext::MatchPattern::parse(s).into_iter().collect(),
            Some(Value::Array(a)) => a.iter().filter_map(|s| s.as_str()).filter_map(ext::MatchPattern::parse).collect(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        let cur = self.cur_id();
        let wins: Vec<super::WindowId> = self.all_frames().iter().map(|f| f.id).collect();
        for win in wins {
            if current == Some(true) && Some(win) != cur {
                continue;
            }
            if window_id.is_some_and(|w| w != u64::from(win)) {
                continue;
            }
            let (ids, active_id, urls): (Vec<TabId>, Option<TabId>, Vec<String>) = {
                let ws = self.ws.borrow();
                let Some(m) = ws.model(win) else { continue };
                let infos = m.infos();
                (
                    infos.iter().map(|i| i.id).collect(),
                    m.active_id(),
                    infos.iter().map(|i| m.get(i.id).map(|t| t.url.clone()).unwrap_or_default()).collect(),
                )
            };
            for (i, id) in ids.iter().enumerate() {
                if active.is_some_and(|a| a != (Some(*id) == active_id)) {
                    continue;
                }
                if !patterns.is_empty() && !patterns.iter().any(|p| p.matches(&urls[i])) {
                    continue;
                }
                out.push(self.ext_tab_json(ext_id, *id));
            }
        }
        out
    }

    /// A tab changed, was activated or closed: the backgrounds that may see it hear about it.
    pub(super) fn ext_tab_event(&self, kind: &str, data: Value) {
        if self.ext.backgrounds.borrow().is_empty() {
            return;
        }
        let ids: Vec<String> = self.ext.backgrounds.borrow().keys().cloned().collect();
        for id in ids {
            let mut d = data.clone();
            if let Some(tab) = data.get("tabId").and_then(|t| t.as_u64()) {
                if kind == "tabUpdated" {
                    let t = self.ext_tab_json(&id, tab as TabId);
                    let mut change = json!({ "status": t.get("status").cloned().unwrap_or(Value::Null) });
                    if let Some(u) = t.get("url") {
                        change["url"] = u.clone();
                    }
                    if let Some(u) = t.get("title") {
                        change["title"] = u.clone();
                    }
                    d["changeInfo"] = change;
                    d["tab"] = t;
                }
            }
            self.ext_send_background(&id, kind, d);
        }
    }

    // ------------------------------------------------------------ messaging

    // ------------------------------------------------------------ storage

    fn ext_storage_key(ext_id: &str, area: &str) -> String {
        format!("{ext_id}/{area}")
    }

    /// May the extension's content scripts use this storage area (and hear its changes)?
    fn ext_storage_open(&self, ext_id: &str, area: &str) -> bool {
        let set = self.ext.storage_access.borrow().get(&Self::ext_storage_key(ext_id, area)).copied();
        lb_core::ext_guard::storage_open_to_content(area, set)
    }

    fn ext_storage_load(&self, ext_id: &str, area: &str) {
        let key = Self::ext_storage_key(ext_id, area);
        if self.ext.storage.borrow().contains_key(&key) {
            return;
        }
        let map = if area == "session" {
            serde_json::Map::new()
        } else {
            read_json(&self.ext_dir().join(format!("{ext_id}.storage.json")))
                .and_then(|v| v.get(area).and_then(|a| a.as_object()).cloned())
                .unwrap_or_default()
        };
        self.ext.storage.borrow_mut().insert(key, map);
    }

    fn ext_storage_save(&self, ext_id: &str) {
        let mut file = serde_json::Map::new();
        {
            let st = self.ext.storage.borrow();
            for area in ["local", "sync"] {
                if let Some(m) = st.get(&Self::ext_storage_key(ext_id, area)) {
                    file.insert(area.into(), Value::Object(m.clone()));
                }
            }
        }
        let _ = std::fs::create_dir_all(self.ext_dir());
        // A synced temporary file next to it, then a rename: a crash in the middle leaves the
        // old file, not half of the new one.
        let path = self.ext_dir().join(format!("{ext_id}.storage.json"));
        if let Err(e) = lb_core::vault::write_atomic(&path, Value::Object(file).to_string().as_bytes()) {
            warning!("extension {ext_id}: storage not saved: {e}");
        }
    }

    fn ext_storage_op(&self, ext_id: &str, op: &str, data: &Value) -> Result<Value, String> {
        let area = data.get("area").and_then(|a| a.as_str()).unwrap_or("local");
        if !matches!(area, "local" | "sync" | "session") {
            return Err("unknown storage area".into());
        }
        self.ext_storage_load(ext_id, area);
        let key = Self::ext_storage_key(ext_id, area);
        let keys: Option<Vec<String>> = match data.get("keys") {
            Some(Value::String(s)) => Some(vec![s.clone()]),
            Some(Value::Array(a)) => Some(a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect()),
            Some(Value::Object(o)) => Some(o.keys().cloned().collect()),
            _ => None,
        };
        let mut changes = serde_json::Map::new();
        let unlimited = self
            .ext
            .exts
            .borrow()
            .iter()
            .any(|e| e.installed.id == ext_id && lb_core::ext_runtime::unlimited_storage(&e.manifest));
        let result = {
            let mut st = self.ext.storage.borrow_mut();
            let map = st.entry(key).or_default();
            match op {
                "storage.get" => {
                    let mut out = serde_json::Map::new();
                    match (&keys, data.get("keys")) {
                        (None, _) => out = map.clone(),
                        (Some(ks), Some(Value::Object(defaults))) => {
                            for k in ks {
                                if let Some(v) = map.get(k) {
                                    out.insert(k.clone(), v.clone());
                                } else if let Some(d) = defaults.get(k) {
                                    out.insert(k.clone(), d.clone());
                                }
                            }
                        }
                        (Some(ks), _) => {
                            for k in ks {
                                if let Some(v) = map.get(k) {
                                    out.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                    Value::Object(out)
                }
                "storage.set" => {
                    if let Some(Value::Object(items)) = data.get("items") {
                        // Checked before anything is stored (a refused `set` changes nothing):
                        // 10 MiB as in Chrome; with `unlimitedStorage` the local area has a
                        // hard cap instead (it is held in memory and written as one file).
                        let limit = lb_core::ext_guard::storage_limit(area, unlimited);
                        if !lb_core::ext_guard::storage_set_fits(map, items, limit) {
                            return Err(lb_core::ext_guard::QUOTA_EXCEEDED.into());
                        }
                        for (k, v) in items {
                            let old = map.insert(k.clone(), v.clone());
                            let mut c = json!({ "newValue": v });
                            if let Some(o) = old {
                                c["oldValue"] = o;
                            }
                            changes.insert(k.clone(), c);
                        }
                    }
                    Value::Null
                }
                "storage.remove" => {
                    for k in keys.unwrap_or_default() {
                        if let Some(old) = map.remove(&k) {
                            changes.insert(k, json!({ "oldValue": old }));
                        }
                    }
                    Value::Null
                }
                "storage.clear" => {
                    for (k, old) in map.iter() {
                        changes.insert(k.clone(), json!({ "oldValue": old }));
                    }
                    map.clear();
                    Value::Null
                }
                _ => {
                    let bytes: usize = map
                        .iter()
                        .filter(|(k, _)| keys.as_ref().is_none_or(|ks| ks.contains(k)))
                        .map(|(k, v)| k.len() + v.to_string().len())
                        .sum();
                    json!(bytes)
                }
            }
        };
        if !changes.is_empty() {
            if area != "session" {
                self.ext_storage_save(ext_id);
            }
            let data = json!({ "changes": changes, "area": area });
            self.ext_send_pages(ext_id, "storageChanged", data.clone());
            // The extension's content worlds in every tab (only its own, C4), when content
            // scripts may use the area at all (the session area: only once it was opened).
            let open = self.ext_storage_open(ext_id, area);
            let targets: Vec<(TabId, i64)> = self
                .ext
                .contexts
                .borrow()
                .iter()
                .flat_map(|(t, l)| l.iter().filter(|c| c.ext == ext_id).map(move |c| (*t, c.id)))
                .filter(|_| open)
                .collect();
            let msg = json!({ "ext": ext_id, "event": "storageChanged", "changes": data["changes"], "area": area });
            for (tab, ctx) in targets {
                self.ext_deliver_content(tab, ctx, &msg);
            }
        }
        Ok(result)
    }

    // ------------------------------------------------------------ alarms

    fn ext_alarm_create(&self, ext_id: &str, name: &str, info: &Value) {
        let key = format!("{ext_id}/{name}");
        let generation = self.ext.next_id.get();
        self.ext.next_id.set(generation + 1);
        let minutes = |k: &str| info.get(k).and_then(|v| v.as_f64()).filter(|m| *m > 0.0);
        let period = minutes("periodInMinutes");
        let delay_ms = match (info.get("when").and_then(|w| w.as_f64()), minutes("delayInMinutes"), period) {
            (Some(when), _, _) => (when as i64 - now_ms()).max(0),
            (_, Some(d), _) => (d * 60_000.0) as i64,
            (_, _, Some(p)) => (p * 60_000.0) as i64,
            _ => 0,
        };
        let delay_ms = delay_ms.max(500);
        let alarm = json!({ "name": name, "scheduledTime": now_ms() + delay_ms, "periodInMinutes": period });
        self.ext.alarms.borrow_mut().insert(key.clone(), (generation, alarm));
        self.ext_alarm_arm(ext_id.to_string(), key, generation, delay_ms);
    }

    fn ext_alarm_arm(&self, ext_id: String, key: String, generation: u64, delay_ms: i64) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(delay_ms, move || {
            let Some(b) = weak.upgrade() else { return };
            let current = b.ext.alarms.borrow().get(&key).cloned();
            let Some((g, alarm)) = current else { return };
            if g != generation {
                return;
            }
            b.ext_send_background(&ext_id, "alarm", json!({ "alarm": alarm }));
            let period = alarm.get("periodInMinutes").and_then(|p| p.as_f64());
            match period {
                Some(p) => {
                    let next = (p * 60_000.0) as i64;
                    let mut a = alarm.clone();
                    a["scheduledTime"] = json!(now_ms() + next);
                    b.ext.alarms.borrow_mut().insert(key.clone(), (generation, a));
                    b.ext_alarm_arm(ext_id, key, generation, next.max(500));
                }
                None => {
                    b.ext.alarms.borrow_mut().remove(&key);
                }
            }
        });
    }

    // ------------------------------------------------------------ scripting

    fn ext_scripting(&self, ext_id: &str, op: &str, data: &Value) -> Result<Value, String> {
        let Some(tab) = data.get("tabId").and_then(|t| t.as_u64()).map(|t| t as TabId).or_else(|| self.active_id())
        else {
            return Err("no tab".into());
        };
        let context = self.ext_script_target(tab, ext_id, false)?;
        let found = self
            .ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == ext_id)
            .map(|e| (e.root.clone(), ext::may_use_scripting(&e.manifest)));
        let Some((root, permitted)) = found else { return Err("unknown extension".into()) };
        // As in Chrome: the `scripting` permission (0.11.7, audit C1).
        if !permitted {
            return Err("The extension doesn't have the \"scripting\" permission.".into());
        }
        let Some(view) = self.tab_view(tab) else { return Err("no such tab".into()) };
        let files: Vec<String> = data
            .get("files")
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let mut code = String::new();
        for f in files.iter().filter(|f| ext::safe_relative_path(f)) {
            match std::fs::read_to_string(ext::file_path(&root, f)) {
                Ok(src) => {
                    if op == "scripting.insertCSS" {
                        code.push_str(&format!("(function(){{var s=document.createElement('style');s.textContent={};document.documentElement.appendChild(s);}})();\n", serde_json::to_string(&src).unwrap_or_default()));
                    } else {
                        code.push_str(&src);
                        code.push('\n');
                    }
                }
                Err(e) => return Err(format!("{f}: {e}")),
            }
        }
        if let Some(inline) = data.get("code").and_then(|c| c.as_str()) {
            code.push_str(inline);
        }
        if let Some(css) = data.get("css").and_then(|c| c.as_str()) {
            code.push_str(&format!("(function(){{var s=document.createElement('style');s.textContent={};document.documentElement.appendChild(s);}})();\n", serde_json::to_string(css).unwrap_or_default()));
        }
        if code.is_empty() {
            return Err("nothing to run".into());
        }
        view.devtools_method(
            "Runtime.evaluate",
            &json!({ "expression": code, "contextId": context, "userGesture": true }),
        );
        Ok(json!([{ "frameId": 0, "result": null }]))
    }

    // ------------------------------------------------------------ filters (declarativeNetRequest)

    fn ext_sync_filters(&self) {
        let text: String = self
            .ext
            .exts
            .borrow()
            .iter()
            .filter(|e| e.installed.enabled && !e.dnr_filters.is_empty())
            .map(|e| e.dnr_filters.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.set_ext_filters(text);
    }

    // ------------------------------------------------------------ serving extension files

    /// A request on an extension origin: the file, the generated background page, the
    /// messages. `None` when the host is not an installed extension.
    pub(super) fn ext_response(&self, url: &str) -> Option<crate::ffi::cef::InternalResponse> {
        use crate::ffi::cef::InternalResponse;
        use std::borrow::Cow;
        let id = ext::id_of_url(url)?;
        let path = url::Url::parse(url).ok()?.path().to_string();
        let (root, background, messages, fresh, sandbox_csp, web_accessible) = {
            let exts = self.ext.exts.borrow();
            let e = exts.iter().find(|e| e.installed.id == id)?;
            (
                e.root.clone(),
                e.manifest.background_script().map(|(s, m)| (s.to_string(), m)),
                e.messages_json.clone(),
                e.fresh,
                lb_core::ext_runtime::is_sandbox_page(&e.manifest, &listed_path(&e.root, &path))
                    .then(|| lb_core::ext_runtime::sandbox_csp(&e.manifest)),
                lb_core::ext_guard::WebAccess::new(&e.manifest, &[]).web_accessible(&path),
            )
        };
        // Other origins may read only what `web_accessible_resources` lists (the tabs' network
        // policy decides which of them may ask at all).
        let cors = if web_accessible { "\r\nAccess-Control-Allow-Origin: *" } else { "" };
        // A sandboxed page (manifest `sandbox.pages`, 0.17.0) gets the sandbox policy: a
        // unique origin, no extension API, and the manifest's (or Chrome's default) script rules.
        let csp = sandbox_csp.unwrap_or_else(|| ext::PAGE_CSP.to_string());
        let headers = |mime: &str| {
            format!(
                "Content-Type: {mime}\r\nContent-Security-Policy: {csp}\r\nX-Content-Type-Options: nosniff\r\n\
                 Referrer-Policy: no-referrer\r\nCache-Control: no-cache{cors}"
            )
        };
        let generated = |mime: &str, body: String| {
            Some(InternalResponse {
                status: 200,
                reason: "OK",
                headers: headers(mime),
                body: Cow::Owned(body.into_bytes()),
            })
        };
        // The background (0.17.0): a relay page (the shim in it starts the worker), what the
        // relay needs to know, and the worker's bootstrap next to the service worker's script.
        if path == "/_krunk/background.html" {
            background.as_ref()?;
            return generated(
                "text/html; charset=utf-8",
                format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>{id} background</title></head><body></body></html>"),
            );
        }
        if path == "/_krunk/background.json" {
            let (script, module) = background?;
            let boot = lb_core::ext_runtime::worker_boot_path(&script);
            return generated(
                "application/json",
                json!({ "boot": boot, "module": module, "install": fresh }).to_string(),
            );
        }
        if let Some((script, module)) = &background {
            if path == lb_core::ext_runtime::worker_boot_path(script) {
                let shim = assets::inject_script("ext-page.js");
                return generated(
                    "text/javascript; charset=utf-8",
                    lb_core::ext_runtime::worker_bootstrap(
                        assets::inject_script("ext-worker.js"),
                        shim,
                        script,
                        *module,
                    ),
                );
            }
        }
        if path == "/_krunk/env.json" {
            return generated("application/json", json!({ "log": runtime::log_all() }).to_string());
        }
        if path == "/_krunk/messages.json" {
            let body = if messages.is_empty() { "{}".to_string() } else { messages };
            return Some(InternalResponse {
                status: 200,
                reason: "OK",
                headers: headers("application/json"),
                body: Cow::Owned(body.into_bytes()),
            });
        }
        let (_, rel) = ext::resolve_file(url)?;
        let file = ext::file_path(&root, &rel);
        let meta = std::fs::metadata(&file).ok()?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Some(InternalResponse::not_found());
        }
        let body = std::fs::read(&file).ok()?;
        Some(InternalResponse {
            status: 200,
            reason: "OK",
            headers: headers(ext::mime_for(&rel)),
            body: Cow::Owned(body),
        })
    }

    /// May a server's redirect lead a tab to `url`? Not to an extension's page unless
    /// `web_accessible_resources` lists it (the redirecting site is not known here; a request
    /// that names its initiator is checked by the tabs' network policy).
    pub(super) fn ext_redirect_allowed(&self, url: &str) -> bool {
        let Some(id) = ext::id_of_url(url) else { return true };
        let path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
        self.ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == id)
            .is_none_or(|e| lb_core::ext_guard::WebAccess::new(&e.manifest, &[]).web_accessible(&path))
    }

    /// A content script reads a file of its own extension (`fetch` in the isolated world goes
    /// through here): the request itself would be the page's, and a page gets only
    /// web-accessible resources. The file as `{ mime, body }` (base64).
    fn ext_read_resource(&self, ext_id: &str, url: &str) -> Result<Value, String> {
        let failed = || "Failed to fetch".to_string();
        let (id, rel) = ext::resolve_file(&lb_core::ext_runtime::krunk_url(url)).ok_or_else(failed)?;
        if id != ext_id {
            return Err(failed());
        }
        let root = self
            .ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == ext_id)
            .map(|e| e.root.clone())
            .ok_or_else(failed)?;
        let file = ext::file_path(&root, &rel);
        let meta = std::fs::metadata(&file).map_err(|_| failed())?;
        if !meta.is_file() || meta.len() > MAX_SCRIPT_READ_BYTES {
            return Err(failed());
        }
        let body = std::fs::read(&file).map_err(|_| failed())?;
        use base64ct::Encoding;
        Ok(json!({ "mime": ext::mime_for(&rel), "body": base64ct::Base64::encode_string(&body) }))
    }

    // ------------------------------------------------------------ installation

    /// Installs a package's files under a fresh folder and loads it. `expect_id`: the Web
    /// Store id the user asked for must be the package's.
    fn ext_install_files(&self, files: Vec<(String, Vec<u8>)>, id: String, source: &str) -> Result<Installed, String> {
        let files = normalize_entries(files);
        let manifest_text = files
            .iter()
            .find(|(n, _)| n == "manifest.json")
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .ok_or("the package has no manifest.json")?;
        let manifest = Manifest::parse(&manifest_text).map_err(|e| e.to_string())?;
        let dir = self.ext_dir().join(&id);
        let staging = self.ext_dir().join(format!("{id}.new"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
        for (name, bytes) in &files {
            let p = ext::file_path(&staging, name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{name}: {e}"))?;
            }
            std::fs::write(&p, bytes).map_err(|e| format!("{name}: {e}"))?;
        }
        let was = self.ext.exts.borrow().iter().find(|e| e.installed.id == id).map(|e| e.installed.clone());
        if was.is_some() {
            self.ext_stop_background(&id);
            self.close_ext_popup();
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&staging, &dir).map_err(|e| format!("could not place the extension folder: {e}"))?;
        // Localized name and description, the survey, the rules.
        let messages = Self::ext_messages(&dir, &manifest, &self.ext_languages());
        let msg_value: Value = serde_json::from_str(&messages).unwrap_or(json!({}));
        let name = ext::localize(&manifest.name, &msg_value);
        let description = ext::localize(&manifest.description, &msg_value);
        let sources: Vec<String> = files
            .iter()
            .filter(|(n, _)| n.ends_with(".js") || n.ends_with(".mjs") || n.ends_with(".html"))
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .collect();
        let apis = ext::survey_apis(&sources.iter().map(String::as_str).collect::<Vec<_>>());
        let (_, translated, skipped) = Self::ext_dnr_text(&dir, &manifest, &name);
        let now = now_ms();
        let installed = Installed {
            id: id.clone(),
            name: name.chars().take(75).collect(),
            version: manifest.version.clone(),
            description: description.chars().take(300).collect(),
            enabled: was.as_ref().map(|w| w.enabled).unwrap_or(true),
            allowed_in_private: was.as_ref().map(|w| w.allowed_in_private).unwrap_or(false),
            source: source.to_string(),
            installed_at: was.as_ref().map(|w| w.installed_at).unwrap_or(now),
            updated_at: now,
            apis,
            dnr_translated: translated,
            dnr_skipped: skipped,
        };
        let loaded = self.load_ext(installed.clone())?;
        {
            let mut exts = self.ext.exts.borrow_mut();
            exts.retain(|e| e.installed.id != id);
            exts.push(loaded);
        }
        let unsupported: Vec<String> = installed.apis.iter().filter(|a| !a.supported).map(|a| a.api.clone()).collect();
        info!(
            "extension installed: {} {} ({id}) from {source}; {} content script(s), background {}, {} rule(s) translated, {} skipped{}",
            installed.name,
            installed.version,
            manifest.content_scripts.len(),
            if manifest.background_script().is_some() { "yes" } else { "no" },
            translated,
            skipped,
            if unsupported.is_empty() { String::new() } else { format!("; not provided: {}", unsupported.join(", ")) }
        );
        // An update starts the new code: the running background (old files) and what it had
        // open go first. The new worker gets `install` and `activate` (0.17.0).
        self.ext_close_contexts(&id);
        self.ext_stop_background(&id);
        self.ext.registered.borrow_mut().remove(&id);
        self.ext.errors.borrow_mut().remove(&id);
        if let Some(e) = self.ext.exts.borrow_mut().iter_mut().find(|e| e.installed.id == id) {
            e.fresh = true;
        }
        self.ext_apply_all(true);
        let reason = if was.is_some() { "update" } else { "install" };
        self.ext_send_background(
            &id,
            "installed",
            json!({ "reason": reason, "previousVersion": was.map(|w| w.version) }),
        );
        Ok(installed)
    }

    /// A zip or a CRX3 in memory: the files and the id. A CRX3 must carry a valid signature by
    /// its own key (0.7.0); `expect_id` (a Web Store download) must be that id.
    fn ext_prepare_bytes(&self, bytes: &[u8], expect_id: Option<&str>) -> Result<Prepared, String> {
        let (zip, id, store_signed) = if bytes.starts_with(b"Cr24") {
            let v = ext::verify_crx(bytes).map_err(|e| e.to_string())?;
            (v.crx.zip, Some(v.crx.id), v.store_signed)
        } else {
            (bytes.to_vec(), None, false)
        };
        if let (Some(want), Some(got)) = (expect_id, id.as_deref()) {
            if want != got {
                return Err(format!("the package is signed for another id ({got}), not {want}"));
            }
        }
        if expect_id.is_some() && !store_signed {
            return Err("the package from the Web Store is not signed by the Web Store".into());
        }
        let entries = ext::unzip(&zip).map_err(|e| e.to_string())?;
        let files: Vec<(String, Vec<u8>)> = entries.into_iter().map(|e| (e.name, e.data)).collect();
        let files = normalize_entries(files);
        let id = match id {
            Some(i) => i,
            None => {
                // A zip without a key: the manifest's `key` if it has one, else its name.
                let manifest: Value = files
                    .iter()
                    .find(|(n, _)| n == "manifest.json")
                    .and_then(|(_, b)| serde_json::from_slice(b).ok())
                    .unwrap_or(Value::Null);
                match manifest.get("key").and_then(|k| k.as_str()).and_then(ext::base64_decode) {
                    Some(der) => ext::id_from_public_key(&der),
                    None => {
                        ext::id_from_public_key(manifest.get("name").and_then(|n| n.as_str()).unwrap_or("").as_bytes())
                    }
                }
            }
        };
        Ok((files, id, store_signed))
    }

    /// Every install path ends here (0.7.0, R20): the verified package is shown with what it
    /// may do, and installed only when the user says so.
    fn ext_confirm_install(
        &self,
        prepared: Result<Prepared, String>,
        source: String,
        source_text: &str,
        tab: Option<TabId>,
        req_id: Option<u32>,
        store_tab: Option<TabId>,
    ) {
        let (files, id, store_signed) = match prepared {
            Ok(p) => p,
            Err(e) => {
                warning!("extension install failed: {e}");
                self.ext_install_reply(tab, req_id, Err(e.clone()));
                self.ext_store_notice(store_tab, &format!("Krunk could not add this extension: {e}"));
                return;
            }
        };
        let manifest = files
            .iter()
            .find(|(n, _)| n == "manifest.json")
            .ok_or_else(|| "the package has no manifest.json".to_string())
            .and_then(|(_, b)| Manifest::parse(&String::from_utf8_lossy(b)).map_err(|e| e.to_string()));
        let manifest = match manifest {
            Ok(m) => m,
            Err(e) => {
                warning!("extension install failed: {e}");
                self.ext_install_reply(tab, req_id, Err(e.clone()));
                self.ext_store_notice(store_tab, &format!("Krunk could not add this extension: {e}"));
                return;
            }
        };
        // Its name in the user's language (the same as after the install).
        let messages: Value = files
            .iter()
            .find(|(n, _)| {
                self.ext_languages().iter().any(|l| *n == format!("_locales/{l}/messages.json"))
                    || manifest.default_locale.as_deref().is_some_and(|d| *n == format!("_locales/{d}/messages.json"))
            })
            .and_then(|(_, b)| serde_json::from_slice(b).ok())
            .unwrap_or(json!({}));
        let name = ext::localize(&manifest.name, &messages);
        let installed = self
            .ext
            .exts
            .borrow()
            .iter()
            .find(|e| e.installed.id == id)
            .map(|e| (e.installed.version.clone(), e.installed.source.clone(), e.installed.name.clone()));
        if let Some((_, installed_source, installed_name)) = &installed {
            if !ext::replacement_allowed(installed_source, store_signed) {
                let e = format!(
                    "{installed_name} from the Chrome Web Store is installed with the same id. This package is not \
                     signed by the Web Store, so it can't replace it. Remove {installed_name} first to install it."
                );
                warning!("extension install refused: {id} would replace a Web Store extension");
                self.ext_install_reply(tab, req_id, Err(e.clone()));
                self.ext_store_notice(store_tab, &format!("Krunk could not add this extension: {e}"));
                return;
            }
        }
        let previous = installed.map(|(version, _, _)| version);
        let request_id = self.ext.next_id.get() as u32;
        self.ext.next_id.set(self.ext.next_id.get() + 1);
        let src = if store_signed {
            format!("{source_text}, signed by the Chrome Web Store")
        } else {
            source_text.to_string()
        };
        let warnings = ext::install_warnings(&manifest);
        info!("extension: asking before installing {name} {} ({id}) from {source}", manifest.version);
        // A newer request replaces one still waiting (that one counts as declined).
        if let Some(old) = self.ext.pending_install.borrow_mut().take() {
            self.ext_install_reply(old.tab, old.req_id, Err("replaced by another install".into()));
        }
        *self.ext.pending_install.borrow_mut() =
            Some(PendingInstall { request_id, files, id, source, tab, req_id, store_tab });
        let entries = warnings.len();
        self.show_prompt(
            lb_core::protocol::PopupContent::ExtInstall {
                request_id,
                name,
                version: manifest.version.clone(),
                source: src,
                warnings,
                previous,
            },
            entries,
        );
    }

    /// The confirmation's answer.
    pub(super) fn ext_install_answer(&self, request_id: u32, accept: bool) {
        let pending = {
            let mut slot = self.ext.pending_install.borrow_mut();
            if slot.as_ref().is_none_or(|p| p.request_id != request_id) {
                return;
            }
            slot.take()
        };
        let Some(p) = pending else { return };
        self.hide_popup();
        if !accept {
            info!("extension install declined ({})", p.id);
            self.ext_install_reply(p.tab, p.req_id, Err("declined".into()));
            return;
        }
        let result = self.ext_install_files(p.files, p.id, &p.source);
        if let (Some(store_tab), Ok(i)) = (p.store_tab, &result) {
            self.ext_store_notice(Some(store_tab), &format!("{} was added to Krunk.", i.name));
        }
        self.ext_install_reply(p.tab, p.req_id, result);
        self.push_ext_list();
    }

    /// The confirmation closed without an answer: declined.
    pub(super) fn ext_install_dismissed(&self, request_id: u32) {
        let waiting = self.ext.pending_install.borrow().as_ref().is_some_and(|p| p.request_id == request_id);
        if waiting {
            self.ext_install_answer(request_id, false);
        }
    }

    pub(super) fn ext_install_file(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.window() else { return };
        let Some(path) =
            dialogs::open_file(w.hwnd(), "Install an extension (zip or crx)", ("Extensions", "*.zip;*.crx"))
        else {
            self.reply(tab, Event::ok(req_id, Value::Null));
            return;
        };
        let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let prepared =
            std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| self.ext_prepare_bytes(&bytes, None));
        self.ext_confirm_install(prepared, format!("file:{file}"), &format!("The file {file}"), tab, req_id, None);
    }

    pub(super) fn ext_install_folder(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.window() else { return };
        let Some(path) = dialogs::pick_folder(w.hwnd(), "Install an unpacked extension (its folder)", None) else {
            self.reply(tab, Event::ok(req_id, Value::Null));
            return;
        };
        let prepared = read_tree(&path).map(|files| (files, ext::id_from_path(&path), false));
        self.ext_confirm_install(
            prepared,
            format!("folder:{}", path.display()),
            &format!("The folder {} (unpacked, not signed)", path.display()),
            tab,
            req_id,
            None,
        );
    }

    pub(super) fn ext_install_url(&self, tab: Option<TabId>, req_id: Option<u32>, url: &str) {
        let Some(id) = ext::webstore_id(url).or_else(|| ext::webstore_crx_id(url)) else {
            self.reply(
                tab,
                Event::err(req_id, "That is not a Chrome Web Store extension address (…/detail/<name>/<id>)."),
            );
            return;
        };
        self.ext_install_from_store(&id, tab, req_id, None);
    }

    /// Downloads extension `id` from the Web Store (over HTTPS, never into Downloads), checks
    /// its signatures and asks before installing it.
    pub(super) fn ext_install_from_store(
        &self,
        id: &str,
        tab: Option<TabId>,
        req_id: Option<u32>,
        store_tab: Option<TabId>,
    ) {
        let Some(download) = ext::webstore_download_url(id) else { return };
        let Some(proxy) = self.ext_proxy() else {
            warning!("extension: {id} not downloaded: a SOCKS proxy is set");
            let e = "Extensions can't be downloaded while a SOCKS proxy is set (it would be bypassed)".to_string();
            self.ext_confirm_install(Err(e), format!("webstore:{id}"), "Chrome Web Store", tab, req_id, store_tab);
            return;
        };
        info!("extension: downloading {id} from the Web Store");
        let id2 = id.to_string();
        self.spawn_job(
            "extension download",
            move || {
                let opts = http::Options { proxy: &proxy, timeout_ms: CRX_TIMEOUT_MS, user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36", allow_http: false };
                http::get_with(&download, MAX_CRX_BYTES, &opts)
            },
            move |b, r| {
                let prepared = r.and_then(|bytes| b.ext_prepare_bytes(&bytes, Some(&id2)));
                b.ext_confirm_install(prepared, format!("webstore:{id2}"), "Chrome Web Store", tab, req_id, store_tab);
            },
        );
    }

    /// "Add to Krunk" on a store page (0.7.0, R20): the id comes from the tab's own address.
    pub(super) fn ext_store_click(&self, tab: TabId) {
        let url = self.tab(tab).map(|t| t.url.clone()).unwrap_or_default();
        match ext::webstore_id(&url) {
            Some(id) => self.ext_install_from_store(&id, None, None, Some(tab)),
            None => warning!("tab {tab}: \"Add to Krunk\" on a page that is not a store detail page"),
        }
    }

    /// A one-line notice on the store page (in the tab's page through its media-free bridge).
    fn ext_store_notice(&self, tab: Option<TabId>, text: &str) {
        let Some(view) = tab.and_then(|t| self.tab_view(t)) else { return };
        view.post_json(&json!({ "event": "krunkStore", "text": text }).to_string());
    }

    /// The extensions page, if open, shows the new list.
    fn push_ext_list(&self) {
        let event = json!({ "event": "extChanged", "extensions": self.ext_list_json() }).to_string();
        let tabs: Vec<TabId> = self
            .ws
            .borrow()
            .tabs()
            .filter(|(_, t)| lb_core::pages::Page::from_url(&t.url) == Some(lb_core::pages::Page::Extensions))
            .map(|(_, t)| t.id)
            .collect();
        for id in tabs {
            if let Some(c) = self.tab_view(id) {
                c.post_json(&event);
            }
        }
    }

    fn ext_install_reply(&self, tab: Option<TabId>, req_id: Option<u32>, result: Result<Installed, String>) {
        if tab.is_none() && req_id.is_none() {
            return;
        }
        match result {
            Err(e) if e == "declined" => self.reply(tab, Event::ok(req_id, json!({ "declined": true }))),
            Ok(i) => self.reply(
                tab,
                Event::ok(req_id, json!({ "installed": i.id, "name": i.name, "extensions": self.ext_list_json() })),
            ),
            Err(e) => {
                warning!("extension install failed: {e}");
                self.reply(tab, Event::err(req_id, format!("Install failed: {e}")));
            }
        }
    }

    pub(super) fn ext_set(&self, id: &str, enabled: Option<bool>, private: Option<bool>) {
        {
            let mut exts = self.ext.exts.borrow_mut();
            let Some(e) = exts.iter_mut().find(|e| e.installed.id == id) else { return };
            if let Some(on) = enabled {
                e.installed.enabled = on;
                info!("extension {}: {}", e.installed.name, if on { "enabled" } else { "disabled" });
            }
            if let Some(p) = private {
                e.installed.allowed_in_private = p;
                info!("extension {}: {} in private tabs", e.installed.name, if p { "allowed" } else { "not allowed" });
            }
        }
        if enabled == Some(false) {
            self.close_ext_popup();
            self.ext_close_contexts(id);
            self.ext.registered.borrow_mut().remove(id);
        }
        self.ext_apply_all(true);
    }

    pub(super) fn ext_remove(&self, id: &str) {
        let name = self.ext_name(id);
        self.ext_close_contexts(id);
        self.ext_stop_background(id);
        self.close_ext_popup();
        self.ext.registered.borrow_mut().remove(id);
        self.ext.errors.borrow_mut().remove(id);
        self.ext.exts.borrow_mut().retain(|e| e.installed.id != id);
        self.ext.alarms.borrow_mut().retain(|k, _| !k.starts_with(&format!("{id}/")));
        self.ext.storage.borrow_mut().retain(|k, _| !k.starts_with(&format!("{id}/")));
        let _ = std::fs::remove_dir_all(self.ext_dir().join(id));
        let _ = std::fs::remove_file(self.ext_dir().join(format!("{id}.storage.json")));
        info!("extension removed: {name} ({id})");
        self.ext_apply_all(true);
    }

    /// krunk://extensions' list.
    pub(super) fn ext_list_json(&self) -> Value {
        let exts = self.ext.exts.borrow();
        let errors = self.ext.errors.borrow();
        Value::Array(
            exts.iter()
                .map(|e| {
                    let i = &e.installed;
                    let scope: Vec<Value> = e
                        .manifest
                        .scope()
                        .iter()
                        .filter_map(|p| {
                            ext::MatchPattern::parse(p)
                                .map(|m| json!({ "pattern": p, "text": m.describe(), "allSites": m.is_all_sites() }))
                        })
                        .collect();
                    json!({
                        "id": i.id, "name": i.name, "version": i.version, "description": i.description,
                        "enabled": i.enabled, "allowedInPrivate": i.allowed_in_private, "source": i.source,
                        "installedAt": i.installed_at, "updatedAt": i.updated_at,
                        "scope": scope, "apis": i.apis, "runtimeUnsupported": e.runtime_unsupported,
                        "dnrTranslated": i.dnr_translated, "dnrSkipped": i.dnr_skipped,
                        "hasOptions": e.manifest.options().is_some(), "hasPopup": e.popup_path().is_some(),
                        "hasBackground": e.manifest.background_script().is_some(),
                        "contentScripts": e.manifest.content_scripts.len(),
                        "icon": e.manifest.icon().map(|p| format!("{}/{}", e.origin(), p.trim_start_matches('/'))),
                        "permissions": e.manifest.api_permissions(),
                        "errors": errors.get(&i.id).cloned().unwrap_or_default(),
                    })
                })
                .collect(),
        )
    }
}
