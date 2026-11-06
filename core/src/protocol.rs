//! Typed JSON message protocol between the web UI and the Rust host.
//!
//! There are two inbound channels with very different trust levels:
//!
//! * [`Request`]: privileged commands from the browser chrome (shell
//!   browser view) and from built-in pages. The host has to check the sender's
//!   origin first ([`Sender`]); each sender can only use the commands that
//!   [`Sender::permits`] allows.
//! * [`ContentMessage`]: messages from scripts we inject into ordinary web
//!   pages. Any page can forge these, so the type is deliberately tiny and
//!   strictly parsed, and none of its variants can cause a privileged action
//!   without a trusted confirmation in the shell.
//!
//! Outbound messages are [`Event`]s.

use crate::downloads::DownloadItem;
use crate::keys::Command;
use crate::pages::{self, Page};
use crate::secret::Secret;
use crate::settings::{Settings, Theme};
use serde::{Deserialize, Serialize};

/// Maximum size of a privileged message (settings JSON is the largest).
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Maximum size of a message from web content.
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;
/// Class/id tokens a page may report for generic cosmetic filtering.
pub const MAX_PAGE_TOKENS: usize = 2000;
pub const MAX_TOKEN_CHARS: usize = 100;
pub const MAX_USERNAME_CHARS: usize = 512;
pub const MAX_PASSWORD_CHARS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    TooLarge,
    Malformed(String),
    Forbidden,
    Invalid(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::TooLarge => write!(f, "message too large"),
            ProtocolError::Malformed(m) => write!(f, "malformed message: {m}"),
            ProtocolError::Forbidden => write!(f, "command not allowed for this sender"),
            ProtocolError::Invalid(m) => write!(f, "invalid message: {m}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoomAction {
    In,
    Out,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyField {
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadAction {
    Open,
    ShowInFolder,
    Cancel,
    Pause,
    Resume,
    Remove,
    /// The download's address to the clipboard.
    CopyLink,
    /// The address of the page it came from to the clipboard.
    CopyPageAddress,
    /// Download the same address again (a failed, cancelled or finished one).
    Retry,
    /// Delete the downloaded file (a completed one) and remove it from the list.
    DeleteFile,
}

/// Data kinds for "Clear browsing data".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClearData {
    pub history: bool,
    pub cookies: bool,
    pub cache: bool,
    /// Local storage, IndexedDB, service workers and other site storage.
    pub site_data: bool,
    pub downloads: bool,
    /// Unix milliseconds; `None` = everything.
    pub since: Option<i64>,
}

/// A vault entry as edited on the passwords page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryInput {
    /// `None` creates a new entry.
    pub id: Option<String>,
    pub origin: String,
    pub username: String,
    /// `None` keeps the stored password when editing.
    pub password: Option<Secret>,
    /// `None` (or absent) keeps the stored note when editing; notes are not
    /// sent to the page, so it cannot pre-fill them.
    #[serde(default)]
    pub note: Option<String>,
}

/// Privileged commands. Secrets travel as plain `String` only as far as the
/// host's message handler, which moves them into `Zeroizing` immediately.
/// Popups the shell may ask for. Password prompts are opened by the host only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PopupKind {
    Menu,
    Shield,
    SavePassword,
    Fill,
    /// A folder of the bookmarks bar (opened with `ShowBookmarkFolder`).
    BookmarkFolder,
    /// Master-password prompt before saving or filling a login.
    Unlock,
    /// A short notice that does not take focus ("Press Esc to show your
    /// cursor" while the pointer is locked).
    Hint,
    /// "Krunk X.Y.Z is available" (opened by the host after an update check).
    Update,
    /// A tab's context menu (right-click on the tab).
    TabMenu,
    /// The bookmarks bar's context menu (right-click on an item, a folder or the bar).
    BookmarkMenu,
    /// Name (and address) of a bookmark or folder: edit, rename, new folder.
    BookmarkEdit,
}

impl PopupKind {
    /// Prompts about passwords must come from the host, never the shell.
    pub fn shell_may_open(self) -> bool {
        matches!(self, PopupKind::Menu | PopupKind::Shield)
    }

    /// Every popup has buttons, so every popup takes keyboard focus. This
    /// also makes dismissal reliable: clicking anywhere else deactivates the
    /// popup window (a no-activate window gets no such signal when the page
    /// it covers already had focus).
    pub fn takes_focus(self) -> bool {
        // A hint must not take the keyboard from the page (a game).
        self != PopupKind::Hint
    }
}

/// What a tab's context menu can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabAction {
    Reload,
    Duplicate,
    MoveToNewWindow,
    Close,
    /// Close every other tab of the window.
    CloseOthers,
    /// Close the tabs to the right of this one.
    CloseRight,
}

/// What the bookmarks bar's context menu can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BookmarkAction {
    OpenNewTab,
    OpenNewWindow,
    OpenPrivate,
    /// A folder: every bookmark in it, as new tabs.
    OpenAll,
    Edit,
    Delete,
    /// Bookmark the current page on the bar (or in the folder).
    AddPage,
    NewFolder,
    ShowAlways,
    ShowOnNewTab,
    ShowNever,
}

/// Caption-button actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowAction {
    Minimize,
    /// Maximize, or restore when maximized.
    Maximize,
    Close,
}

/// A caption button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

/// A rectangle in CSS pixels relative to the shell's viewport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopupRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PopupRect {
    pub const MAX: f64 = 4096.0;

    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite() && (0.0..=Self::MAX).contains(v))
            && self.width >= 1.0
            && self.height >= 1.0
    }
}

/// What the popup view shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PopupContent {
    Menu {
        zoom: f64,
        can_reopen: bool,
    },
    Shield {
        host: String,
        blocked: u32,
        allowed: bool,
        enabled: bool,
        /// The newest blocked addresses (grouped; the full list is on krunk://adblock).
        #[serde(default)]
        recent: Vec<crate::blocklog::Item>,
        /// How many different addresses were blocked.
        #[serde(default)]
        distinct: u32,
    },
    /// "Save password for `username` on `origin`?" (no password is sent to the UI).
    SavePassword {
        tab_id: u32,
        origin: String,
        username: String,
        update: bool,
    },
    /// "Fill the login for `origin`?" with the matching entries.
    Fill {
        tab_id: u32,
        origin: String,
        entries: Vec<FillChoice>,
    },
    /// "Unlock your passwords" before saving or filling on `origin`.
    Unlock {
        tab_id: u32,
        origin: String,
        purpose: UnlockPurpose,
        /// Last error (wrong password, too many attempts).
        error: Option<String>,
        /// Milliseconds before the next attempt is accepted.
        wait_ms: u64,
    },
    /// Contents of a bookmarks-bar folder.
    BookmarkFolder {
        id: i64,
        title: String,
        items: Vec<BarItem>,
    },
    Hint {
        text: String,
    },
    /// A newer Krunk: Update now (or Download) / Later / Release notes.
    Update {
        version: String,
        current: String,
        /// The installer can update this copy silently; otherwise the
        /// prompt offers the download page (portable copy, or too old).
        install: bool,
        has_notes: bool,
        state: UpdateProgress,
    },
    /// The context menu of tab `tab_id`. `can_move`: the window has other tabs, so the tab
    /// can move to a new window of its own; `can_close_right`: there are tabs to its right.
    TabMenu {
        tab_id: u32,
        can_move: bool,
        can_close_right: bool,
    },
    /// The bookmarks bar's context menu: on bookmark or folder `id`, or on the bar itself
    /// (`id` none). `children`: a folder's bookmarks (for "Open all"); `can_add`: the page
    /// can be bookmarked here ("Add this page").
    BookmarkMenu {
        id: Option<i64>,
        is_folder: bool,
        title: String,
        url: Option<String>,
        children: u32,
        mode: crate::settings::BookmarksBar,
        can_add: bool,
    },
    /// Edit bookmark or folder `id`, or name a new folder in `parent` (`id` none).
    BookmarkEdit {
        id: Option<i64>,
        parent: i64,
        is_folder: bool,
        title: String,
        url: Option<String>,
    },
}

/// Where an accepted update is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateProgress {
    Offered,
    Downloading {
        percent: u8,
    },
    /// The installer is starting; Krunk closes and restarts.
    Installing,
    Failed {
        error: String,
    },
}

/// Answers to the update prompt (and the settings page's "Update now").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChoice {
    /// Download, check and install (installed copies), or open the
    /// download page (portable copies).
    Install,
    Later,
    Notes,
}

/// Why the vault must be unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnlockPurpose {
    Save,
    Fill,
}

/// An entry of the bookmarks bar or of a folder dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarItem {
    pub id: i64,
    pub title: String,
    /// `None` for folders.
    pub url: Option<String>,
    pub is_folder: bool,
    /// Same-origin URL of the cached favicon (`/favicon?host=...`), if any.
    pub favicon: Option<String>,
}

impl PopupContent {
    pub fn kind(&self) -> PopupKind {
        match self {
            PopupContent::Menu { .. } => PopupKind::Menu,
            PopupContent::Shield { .. } => PopupKind::Shield,
            PopupContent::SavePassword { .. } => PopupKind::SavePassword,
            PopupContent::Fill { .. } => PopupKind::Fill,
            PopupContent::BookmarkFolder { .. } => PopupKind::BookmarkFolder,
            PopupContent::Unlock { .. } => PopupKind::Unlock,
            PopupContent::Hint { .. } => PopupKind::Hint,
            PopupContent::Update { .. } => PopupKind::Update,
            PopupContent::TabMenu { .. } => PopupKind::TabMenu,
            PopupContent::BookmarkMenu { .. } => PopupKind::BookmarkMenu,
            PopupContent::BookmarkEdit { .. } => PopupKind::BookmarkEdit,
        }
    }

    /// The toolbar anchor that toggles this popup, for the reopen guard
    /// (`toggle::ToggleGuard`); `None` for popups without a toggle button.
    pub fn toggle_anchor(&self) -> Option<String> {
        match self {
            PopupContent::Menu { .. } => Some("menu".into()),
            PopupContent::Shield { .. } => Some("shield".into()),
            PopupContent::BookmarkFolder { id, .. } => Some(format!("folder:{id}")),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Request {
    /// The page has loaded and wants its initial state.
    Ready,

    // ---- tabs & navigation (shell) ----
    NewTab {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        private: bool,
    },
    CloseTab {
        id: u32,
    },
    ActivateTab {
        id: u32,
    },
    MoveTab {
        id: u32,
        index: u32,
    },
    DuplicateTab {
        id: u32,
    },
    ReopenClosedTab,
    /// Address-bar input for the active tab (URL or search terms).
    Navigate {
        input: String,
    },
    Back,
    Forward,
    Reload,
    Stop,
    /// Opens an http(s) or built-in URL (bookmarks bar, page links).
    OpenUrl {
        url: String,
        #[serde(default)]
        new_tab: bool,
    },
    OpenPage {
        page: String,
    },
    ToggleBookmark,
    /// The shell's tab-row layout for hit testing the frameless window.
    HitMap {
        map: crate::frame::HitMap,
    },
    /// Minimize, maximize/restore or close the window (keyboard use of the
    /// caption buttons; mouse clicks on them are handled natively).
    WindowControl {
        action: WindowAction,
    },
    /// A tab was dragged out of its strip (the pointer left the tab row): the host follows
    /// the mouse from here, in a new window. `offset_x`/`offset_y`: where the tab was grabbed
    /// (CSS px from the tab's left edge, and from the top of the shell).
    TabDragOut {
        id: u32,
        offset_x: f64,
        offset_y: f64,
    },
    /// Dock DevTools right or at the bottom (all tabs).
    DevToolsDock {
        dock: crate::settings::DevToolsDock,
    },
    /// Closes the active tab's DevTools.
    DevToolsClose,
    /// Right-click on a tab: its context menu at `rect` (shell CSS px).
    ShowTabMenu {
        id: u32,
        rect: PopupRect,
    },
    /// A choice in a tab's context menu (popup).
    TabMenuAction {
        id: u32,
        action: TabAction,
    },
    /// Opens the popup view at `rect` (shell: menu and shield only).
    ShowPopup {
        popup: PopupKind,
        rect: PopupRect,
    },
    HidePopup,
    /// Opens a bookmarks-bar folder in the popup. The shell passes the
    /// rect under the folder button; the popup passes `None` to show a
    /// subfolder in place.
    ShowBookmarkFolder {
        id: i64,
        #[serde(default)]
        rect: Option<PopupRect>,
    },
    /// Right-click on the bookmarks bar: its menu for bookmark or folder `id`, or for the bar
    /// itself (`id` none), at `rect` (shell CSS px).
    ShowBookmarkMenu {
        #[serde(default)]
        id: Option<i64>,
        rect: PopupRect,
    },
    /// A choice in the bookmarks bar's menu (popup).
    BookmarkMenuAction {
        #[serde(default)]
        id: Option<i64>,
        action: BookmarkAction,
    },
    /// Shield panel "Show all": krunk://adblock for what was blocked on the active tab.
    OpenBlockedPage {},
    /// krunk://adblock: what was blocked on the tab it was opened from, with the filters.
    GetBlocked {},
    /// krunk://adblock: every filter list (built-in and added) with its address, state and
    /// rule count.
    GetFilterLists {},
    /// Turns a list (a built-in id or an added list's `custom-…` id) on or off.
    SetFilterList {
        id: String,
        enabled: bool,
    },
    /// Adds a list by its https address (downloaded at once).
    AddFilterList {
        url: String,
    },
    /// Removes an added list (built-in ones can only be turned off).
    RemoveFilterList {
        id: String,
    },
    /// The built-in lists back to the default selection (added lists and My filters stay).
    RestoreDefaultLists {},
    /// "My filters": the text and its check.
    GetUserFilters {},
    SaveUserFilters {
        text: String,
    },
    /// The bookmark editor's Save (popup): rename or edit `id`, or create folder in `parent`.
    SaveBookmark {
        #[serde(default)]
        id: Option<i64>,
        parent: i64,
        title: String,
        #[serde(default)]
        url: Option<String>,
    },
    /// A browser command from a menu item or button.
    Command {
        command: Command,
    },
    Find {
        query: String,
        #[serde(default)]
        backwards: bool,
    },
    FindClose,
    Zoom {
        action: ZoomAction,
    },
    Print,
    DevTools,
    /// Allow (or block again) ads on the active tab's site.
    ShieldAllowSite {
        allow: bool,
    },
    /// Reply to the "Save password?" prompt. `tab_id` and `origin` echo the
    /// prompt; the host re-checks both before saving.
    SavePasswordReply {
        tab_id: u32,
        origin: String,
        accept: bool,
        #[serde(default)]
        never: bool,
    },
    /// Key icon in the address bar: offer to fill the active page's login.
    RequestFill,
    /// Reply to the fill prompt: the chosen entry, or `None` to dismiss.
    FillReply {
        tab_id: u32,
        origin: String,
        entry_id: Option<String>,
    },
    Quit,

    // ---- new tab page ----
    GetTopSites,
    PinSite {
        url: String,
        title: String,
    },
    UnpinSite {
        url: String,
    },
    HideSite {
        url: String,
    },

    // ---- history ----
    QueryHistory {
        #[serde(default)]
        text: String,
        /// Unix ms; only entries older than this (paging).
        #[serde(default)]
        before: Option<i64>,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    DeleteHistory {
        ids: Vec<i64>,
    },
    DeleteHistoryRange {
        from: i64,
        to: i64,
    },
    ClearBrowsingData {
        data: ClearData,
    },

    // ---- bookmarks ----
    GetBookmarks,
    AddBookmark {
        parent: i64,
        title: String,
        url: String,
    },
    AddFolder {
        parent: i64,
        title: String,
    },
    EditBookmark {
        id: i64,
        title: String,
        #[serde(default)]
        url: Option<String>,
    },
    MoveBookmark {
        id: i64,
        parent: i64,
        index: u32,
    },
    DeleteBookmark {
        id: i64,
    },
    SearchBookmarks {
        text: String,
    },
    ImportBookmarks,
    ExportBookmarks,

    // ---- settings ----
    GetSettings,
    SaveSettings {
        settings: Box<Settings>,
    },
    ResetSettings,
    /// Opens the native folder picker for the download folder.
    PickDownloadFolder,
    UpdateFilterLists,
    /// Last filter-list update time, sources and error (settings page).
    GetFilterStatus,
    GetAllowlist,
    AllowlistAdd {
        host: String,
    },
    AllowlistRemove {
        host: String,
    },
    Restart,
    /// Version, last update check and its result, an offered update.
    GetUpdateStatus,
    /// "Check for updates" (works with automatic checks off).
    CheckForUpdates,
    /// The user's answer about the offered update (the host knows which).
    UpdateReply {
        choice: UpdateChoice,
    },

    // ---- vault ----
    VaultStatus,
    VaultSetup {
        password: Secret,
    },
    VaultUnlock {
        password: Secret,
    },
    VaultLock,
    VaultList,
    /// Shows one password; requires the master password again.
    VaultReveal {
        id: String,
        password: Secret,
    },
    VaultCopy {
        id: String,
        field: CopyField,
    },
    VaultSave {
        entry: EntryInput,
    },
    VaultDelete {
        id: String,
    },
    /// Origins on the "never save" list, and removing one.
    GetNeverSave,
    NeverSaveRemove {
        origin: String,
    },
    VaultChangePassword {
        old: Secret,
        new: Secret,
    },
    GeneratePassword {
        #[serde(default = "default_pw_len")]
        length: u32,
        #[serde(default = "yes")]
        symbols: bool,
    },

    // ---- downloads ----
    GetDownloads,
    /// Removes finished, failed and cancelled downloads from the list.
    ClearDownloads,
    DownloadAction {
        id: u32,
        action: DownloadAction,
    },

    // ---- cookies and site data (krunk://storage) ----
    /// Sites with cookies or known storage, and their cookie counts.
    StorageList,
    /// Storage usage of these origins (at most 200).
    StorageUsage {
        origins: Vec<String>,
    },
    /// The cookies of one site, with values.
    StorageCookies {
        site: String,
    },
    StorageDeleteCookie {
        cookie: crate::sitedata::CookieKey,
    },
    /// Deletes every cookie of a site.
    StorageDeleteSite {
        site: String,
    },
    StorageDeleteAllCookies,
    /// Clears the site data (local storage, IndexedDB, Cache Storage,
    /// service workers, ...) of one origin.
    StorageClearOrigin {
        origin: String,
    },

    // ---- one site's data (krunk://sitedata, bound by the host to the tab
    // it was opened from; requests name no tab or origin) ----
    SiteDataGet {},
    SiteDataSet {
        area: crate::sitedata::StorageArea,
        key: String,
        value: String,
    },
    SiteDataRemove {
        area: crate::sitedata::StorageArea,
        key: String,
    },
    SiteDataClear {
        area: crate::sitedata::StorageArea,
    },
    /// A new value for one of the site's cookies (its other attributes stay).
    SiteCookieSet {
        cookie: crate::sitedata::CookieKey,
        value: String,
    },
    /// Deletes one of the site's cookies, or all of them (`None`).
    SiteCookiesDelete {
        #[serde(default)]
        cookie: Option<crate::sitedata::CookieKey>,
    },
    /// From the shield panel: open the site-data page for the active tab.
    OpenSiteData,

    // ---- about / https-only ----
    GetAbout,
    /// From the HTTPS-only interstitial: load this http:// URL once.
    HttpsOnlyProceed {
        url: String,
    },
}

fn default_limit() -> u32 {
    100
}
fn default_pw_len() -> u32 {
    20
}
fn yes() -> bool {
    true
}

/// Who sent a privileged message, derived by the host from the browser view it
/// arrived on *and* the committed URL of the sending frame (see [`classify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    Shell,
    Popup,
    Page(Page),
}

/// Which browser view a message arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Shell,
    Popup,
    Tab,
}

/// Classification of a message source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Our own UI: parse with [`parse_request`].
    Privileged(Sender),
    /// An ordinary web page on this origin: parse with [`parse_content`] only.
    Web { origin: String },
}

pub const SHELL_PATH: &str = "/shell.html";
pub const POPUP_PATH: &str = "/popup.html";

/// Decides how a message may be treated, from the channel it came in on,
/// the `Source` URI and whether it came from the top-level document.
///
/// * The shell and popup views accept only their exact document on the
///   exact shell origin; anything else on those channels is rejected.
/// * A tab accepts privileged messages only from a built-in page on the
///   exact pages origin; other http(s) pages are web content.
/// * Messages from sub-frames are always rejected.
pub fn classify(channel: Channel, source: &str, top_level: bool) -> Result<Source, ProtocolError> {
    if !top_level {
        return Err(ProtocolError::Forbidden);
    }
    let url = url::Url::parse(source).map_err(|_| ProtocolError::Forbidden)?;
    let origin = url.origin().ascii_serialization();
    match channel {
        Channel::Shell | Channel::Popup => {
            let want = if channel == Channel::Shell { SHELL_PATH } else { POPUP_PATH };
            if origin == pages::SHELL_ORIGIN && url.path() == want {
                Ok(Source::Privileged(if channel == Channel::Shell { Sender::Shell } else { Sender::Popup }))
            } else {
                Err(ProtocolError::Forbidden)
            }
        }
        Channel::Tab => {
            if let Some(page) = Page::from_url(source) {
                return Ok(Source::Privileged(Sender::Page(page)));
            }
            // Our own origins never count as web content.
            if origin == pages::SHELL_ORIGIN || origin == pages::PAGES_ORIGIN {
                return Err(ProtocolError::Forbidden);
            }
            match url.scheme() {
                "http" | "https" => Ok(Source::Web { origin }),
                _ => Err(ProtocolError::Forbidden),
            }
        }
    }
}

impl Sender {
    /// Least privilege: each built-in page can only use its own commands.
    pub fn permits(self, req: &Request) -> bool {
        use Request as R;
        // Harmless navigation-ish commands every privileged sender may use,
        // except the site-data page, which has nothing but its own commands.
        if matches!(req, R::Ready) {
            return true;
        }
        if matches!(req, R::OpenUrl { .. } | R::OpenPage { .. }) {
            return self != Sender::Page(Page::SiteData);
        }
        match self {
            Sender::Shell => matches!(
                req,
                R::NewTab { .. }
                    | R::CloseTab { .. }
                    | R::ActivateTab { .. }
                    | R::MoveTab { .. }
                    | R::DuplicateTab { .. }
                    | R::TabDragOut { .. }
                    | R::ShowTabMenu { .. }
                    | R::DevToolsDock { .. }
                    | R::DevToolsClose
                    | R::ReopenClosedTab
                    | R::Navigate { .. }
                    | R::Back
                    | R::Forward
                    | R::Reload
                    | R::Stop
                    | R::ToggleBookmark
                    | R::HitMap { .. }
                    | R::WindowControl { .. }
                    | R::ShowPopup { .. }
                    | R::HidePopup
                    | R::ShowBookmarkFolder { .. }
                    | R::ShowBookmarkMenu { .. }
                    | R::MoveBookmark { .. }
                    | R::Command { .. }
                    | R::Find { .. }
                    | R::FindClose
                    | R::Zoom { .. }
                    | R::Print
                    | R::DevTools
                    | R::Quit
                    | R::RequestFill
                    | R::GetBookmarks
                    | R::VaultStatus
                    | R::VaultUnlock { .. }
                    | R::VaultLock
                    | R::GeneratePassword { .. }
            ),
            Sender::Popup => matches!(
                req,
                R::Command { .. }
                    | R::TabMenuAction { .. }
                    | R::BookmarkMenuAction { .. }
                    | R::SaveBookmark { .. }
                    | R::HidePopup
                    | R::ShowBookmarkFolder { rect: None, .. }
                    | R::NewTab { .. }
                    | R::ReopenClosedTab
                    | R::Zoom { .. }
                    | R::ShieldAllowSite { .. }
                    | R::OpenSiteData
                    | R::SavePasswordReply { .. }
                    | R::FillReply { .. }
                    | R::VaultStatus
                    | R::VaultUnlock { .. }
                    | R::UpdateReply { .. }
                    | R::OpenBlockedPage {}
                    | R::Quit
            ),
            Sender::Page(Page::NewTab) => matches!(
                req,
                R::GetTopSites | R::PinSite { .. } | R::UnpinSite { .. } | R::HideSite { .. } | R::Navigate { .. }
            ),
            Sender::Page(Page::History) => matches!(
                req,
                R::QueryHistory { .. }
                    | R::DeleteHistory { .. }
                    | R::DeleteHistoryRange { .. }
                    | R::ClearBrowsingData { .. }
            ),
            Sender::Page(Page::Bookmarks) => matches!(
                req,
                R::GetBookmarks
                    | R::AddBookmark { .. }
                    | R::AddFolder { .. }
                    | R::EditBookmark { .. }
                    | R::MoveBookmark { .. }
                    | R::DeleteBookmark { .. }
                    | R::SearchBookmarks { .. }
                    | R::ImportBookmarks
                    | R::ExportBookmarks
            ),
            Sender::Page(Page::Passwords) => matches!(
                req,
                R::VaultStatus
                    | R::VaultSetup { .. }
                    | R::VaultUnlock { .. }
                    | R::VaultLock
                    | R::VaultList
                    | R::VaultReveal { .. }
                    | R::VaultCopy { .. }
                    | R::VaultSave { .. }
                    | R::VaultDelete { .. }
                    | R::VaultChangePassword { .. }
                    | R::GetNeverSave
                    | R::NeverSaveRemove { .. }
                    | R::GeneratePassword { .. }
            ),
            Sender::Page(Page::Settings) => matches!(
                req,
                R::GetSettings
                    | R::SaveSettings { .. }
                    | R::ResetSettings
                    | R::PickDownloadFolder
                    | R::UpdateFilterLists
                    | R::GetFilterStatus
                    | R::GetAllowlist
                    | R::AllowlistAdd { .. }
                    | R::AllowlistRemove { .. }
                    | R::Restart
                    | R::ClearBrowsingData { .. }
                    | R::VaultStatus
                    | R::VaultChangePassword { .. }
                    | R::GetAbout
                    | R::GetUpdateStatus
                    | R::CheckForUpdates
                    | R::UpdateReply { .. }
            ),
            Sender::Page(Page::Downloads) => {
                matches!(req, R::GetDownloads | R::ClearDownloads | R::DownloadAction { .. })
            }
            Sender::Page(Page::About) => matches!(req, R::GetAbout),
            Sender::Page(Page::Storage) => matches!(
                req,
                R::StorageList
                    | R::StorageUsage { .. }
                    | R::StorageCookies { .. }
                    | R::StorageDeleteCookie { .. }
                    | R::StorageDeleteSite { .. }
                    | R::StorageDeleteAllCookies
                    | R::StorageClearOrigin { .. }
            ),
            Sender::Page(Page::SiteData) => matches!(
                req,
                R::SiteDataGet {}
                    | R::SiteDataSet { .. }
                    | R::SiteCookieSet { .. }
                    | R::SiteDataRemove { .. }
                    | R::SiteDataClear { .. }
                    | R::SiteCookiesDelete { .. }
            ),
            Sender::Page(Page::HttpsOnly) => matches!(req, R::HttpsOnlyProceed { .. }),
            Sender::Page(Page::Adblock) => matches!(
                req,
                R::GetBlocked {}
                    | R::GetFilterLists {}
                    | R::SetFilterList { .. }
                    | R::AddFilterList { .. }
                    | R::RemoveFilterList { .. }
                    | R::RestoreDefaultLists {}
                    | R::GetUserFilters {}
                    | R::SaveUserFilters { .. }
                    | R::UpdateFilterLists
            ),
        }
    }
}

/// A parsed privileged message plus its optional request id (echoed back in
/// [`Event::Reply`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub req_id: Option<u32>,
    pub request: Request,
}

#[derive(Deserialize)]
struct Wire {
    #[serde(rename = "reqId", default)]
    req_id: Option<u32>,
    #[serde(flatten)]
    request: Request,
}

/// A parse error without any of the input: serde messages can quote values
/// (which may be passwords), so only the error class and position are kept.
fn malformed(e: serde_json::Error) -> ProtocolError {
    ProtocolError::Malformed(format!("{:?} error at line {} column {}", e.classify(), e.line(), e.column()))
}

/// Parses and authorizes a privileged message.
pub fn parse_request(sender: Sender, json: &str) -> Result<Envelope, ProtocolError> {
    if json.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    // Parsed straight from the text (no intermediate JSON tree), so secret
    // fields are allocated once, inside `Secret`.
    let Wire { req_id, request } = serde_json::from_str(json).map_err(malformed)?;
    if !sender.permits(&request) {
        return Err(ProtocolError::Forbidden);
    }
    Ok(Envelope { req_id, request })
}

/// Messages from scripts injected into web content (untrusted).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "lb", rename_all = "camelCase", deny_unknown_fields)]
pub enum ContentMessage {
    /// The top-level document has a login form. (Empty struct variants, not
    /// unit variants, so that `deny_unknown_fields` applies to them.)
    LoginForm {},
    /// A login form was submitted; the host may offer to save it.
    Submit { username: String, password: Secret },
    /// The user clicked the key badge in a login field. The host shows a
    /// trusted confirmation in the shell before anything is filled.
    RequestFill {},
    /// Class names and ids used on the page, for generic cosmetic filters.
    /// (A page can only affect which elements get hidden on itself.)
    PageTokens { classes: Vec<String>, ids: Vec<String> },
}

/// Parses a message from web content.
pub fn parse_content(json: &str) -> Result<ContentMessage, ProtocolError> {
    if json.len() > MAX_CONTENT_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let msg: ContentMessage = serde_json::from_str(json).map_err(malformed)?;
    if let ContentMessage::PageTokens { classes, ids } = &msg {
        if classes.len() + ids.len() > MAX_PAGE_TOKENS
            || classes.iter().chain(ids).any(|t| t.is_empty() || t.chars().count() > MAX_TOKEN_CHARS)
        {
            return Err(ProtocolError::Invalid("page tokens"));
        }
    }
    if let ContentMessage::Submit { username, password } = &msg {
        if username.chars().count() > MAX_USERNAME_CHARS {
            return Err(ProtocolError::Invalid("username too long"));
        }
        if password.is_empty() || password.chars().count() > MAX_PASSWORD_CHARS {
            return Err(ProtocolError::Invalid("password length"));
        }
    }
    Ok(msg)
}

/// State of one tab, as shown in the tab strip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub id: u32,
    pub title: String,
    /// Address-bar text (built-in pages shown as `krunk://...`).
    pub url: String,
    /// `data:` URL of the favicon, if any.
    pub favicon: Option<String>,
    pub loading: bool,
    pub private: bool,
    /// Asleep or discarded (shown dimmed).
    pub sleeping: bool,
}

/// Navigation state of the active tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavState {
    pub tab_id: u32,
    pub url: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub loading: bool,
    pub secure: bool,
    pub bookmarked: bool,
    pub blocked: u32,
    pub site_allowed: bool,
    pub zoom: f64,
    pub private: bool,
    /// The page has a login form (shows the key icon).
    #[serde(default)]
    pub login_form: bool,
    /// The bookmarks bar shows in this window now (setting, and in "new tab only" mode
    /// whether the active tab is the new-tab page).
    #[serde(default)]
    pub bookmarks_bar: bool,
}

/// Messages to the shell or a built-in page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    /// Answer to a request carrying `reqId`.
    Reply {
        req_id: Option<u32>,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Tabs {
        tabs: Vec<TabInfo>,
        active: Option<u32>,
    },
    Nav {
        state: NavState,
    },
    /// Shell layout options.
    ShellConfig {
        theme: Theme,
    },
    /// To the popup view: render this content.
    Popup {
        content: PopupContent,
    },
    /// To the shell: the popup was hidden (un-highlight its button).
    PopupClosed,
    /// The active tab's docked DevTools (the shell shows its dock and close control).
    DevTools {
        open: bool,
        dock: crate::settings::DevToolsDock,
    },
    /// A tab dragged from another window is over this strip: show where it would land
    /// (`None` = no longer over it).
    TabDropHint {
        index: Option<u32>,
    },
    /// To the shell: the bookmarks bar contents.
    BookmarksBar {
        items: Vec<BarItem>,
    },
    /// To the downloads page: the full list.
    Downloads {
        items: Vec<DownloadItem>,
    },
    VaultState {
        exists: bool,
        locked: bool,
    },
    FindResult {
        active: i32,
        total: i32,
    },
    DownloadsChanged {
        active: u32,
    },
    /// The theme for the shell, popup and built-in pages (`System` follows
    /// the OS through `prefers-color-scheme`).
    Theme {
        theme: Theme,
    },
    FocusAddressBar,
    /// The host moved the focus to the page (a bookmark or link was opened):
    /// the address bar stops editing and shows the page's address.
    BlurAddressBar,
    /// To the shell: the mouse is over (or pressing) a caption button. The
    /// buttons are hit-tested natively, so the shell never sees the mouse
    /// over them.
    CaptionState {
        hover: Option<CaptionButton>,
        pressed: bool,
    },
    /// To the shell: window state for the caption buttons and the tab row.
    WindowState {
        maximized: bool,
        active: bool,
        fullscreen: bool,
    },
    ShowFind,
    Toast {
        message: String,
    },
    /// Settings that need a restart changed.
    RestartRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillChoice {
    pub id: String,
    pub username: String,
}

impl Event {
    pub fn ok(req_id: Option<u32>, data: impl Serialize) -> Event {
        Event::Reply { req_id, ok: true, data: serde_json::to_value(data).ok(), error: None }
    }

    /// Settings rejected by validation: the field errors go back to the page.
    pub fn invalid(req_id: Option<u32>, errors: &[crate::settings::FieldError]) -> Event {
        Event::Reply {
            req_id,
            ok: false,
            data: serde_json::to_value(errors).ok(),
            error: Some("Some settings are not valid".into()),
        }
    }

    pub fn err(req_id: Option<u32>, error: impl Into<String>) -> Event {
        Event::Reply { req_id, ok: false, data: None, error: Some(error.into()) }
    }

    pub fn to_json(&self) -> String {
        // Serializing these plain data types cannot fail.
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: &str = "https://shell.krunk.invalid/shell.html";
    const POPUP: &str = "https://shell.krunk.invalid/popup.html";

    #[test]
    fn parses_shell_commands() {
        let e = parse_request(Sender::Shell, r#"{"cmd":"newTab","private":true,"reqId":7}"#).unwrap();
        assert_eq!(e.req_id, Some(7));
        assert_eq!(e.request, Request::NewTab { url: None, private: true });
        let e = parse_request(Sender::Shell, r#"{"cmd":"moveTab","id":3,"index":0}"#).unwrap();
        assert_eq!(e.request, Request::MoveTab { id: 3, index: 0 });
        let e = parse_request(Sender::Shell, r#"{"cmd":"command","command":{"command":"selectTab","arg":2}}"#).unwrap();
        assert_eq!(e.request, Request::Command { command: Command::SelectTab(2) });
        let e = parse_request(
            Sender::Shell,
            r#"{"cmd":"showPopup","popup":"menu","rect":{"x":900,"y":40,"width":260,"height":400}}"#,
        )
        .unwrap();
        assert!(matches!(e.request, Request::ShowPopup { popup: PopupKind::Menu, rect } if rect.is_valid()));
    }

    #[test]
    fn rejects_malformed_and_unknown() {
        let s = Sender::Shell;
        assert!(matches!(parse_request(s, "nope"), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, "[1]"), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"rmRf"}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"closeTab","id":1,"extra":1}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"closeTab","id":-1}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"back","reqId":"x"}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"back","reqId":-1}"#), Err(ProtocolError::Malformed(_))));
        let big = format!(r#"{{"cmd":"navigate","input":"{}"}}"#, "a".repeat(MAX_REQUEST_BYTES));
        assert_eq!(parse_request(s, &big), Err(ProtocolError::TooLarge));
    }

    #[test]
    fn popup_rects_and_kinds() {
        let ok = PopupRect { x: 10.0, y: 40.0, width: 200.0, height: 300.0 };
        assert!(ok.is_valid());
        for bad in [
            PopupRect { x: f64::NAN, ..ok },
            PopupRect { width: 0.0, ..ok },
            PopupRect { height: 1e9, ..ok },
            PopupRect { x: -5.0, ..ok },
            PopupRect { y: f64::INFINITY, ..ok },
        ] {
            assert!(!bad.is_valid(), "{bad:?}");
        }
        assert!(PopupKind::Menu.shell_may_open() && PopupKind::Shield.shell_may_open());
        assert!(!PopupKind::SavePassword.shell_may_open() && !PopupKind::Fill.shell_may_open());
    }

    fn privileged(ch: Channel, src: &str) -> Option<Sender> {
        match classify(ch, src, true) {
            Ok(Source::Privileged(s)) => Some(s),
            _ => None,
        }
    }

    #[test]
    fn origin_checks_shell_and_popup() {
        assert_eq!(privileged(Channel::Shell, SHELL), Some(Sender::Shell));
        assert_eq!(privileged(Channel::Popup, POPUP), Some(Sender::Popup));
        // Each channel only accepts its own document.
        assert_eq!(privileged(Channel::Shell, POPUP), None);
        assert_eq!(privileged(Channel::Popup, SHELL), None);
        for bad in [
            "https://shell.krunk.invalid:8443/shell.html",     // port
            "http://shell.krunk.invalid/shell.html",           // scheme
            "https://x.shell.krunk.invalid/shell.html",        // subdomain
            "https://shell.krunk.invalid.evil.com/shell.html", // look-alike suffix
            "https://shell-krunk.invalid/shell.html",          // look-alike host
            "https://krunk.invalid/shell.html",                // pages origin
            "https://evil.example/shell.html",
            "data:text/html,<script>",
            "about:blank",
            "not a url",
        ] {
            assert_eq!(classify(Channel::Shell, bad, true), Err(ProtocolError::Forbidden), "{bad}");
            assert_eq!(
                classify(Channel::Popup, &bad.replace("shell.html", "popup.html"), true),
                Err(ProtocolError::Forbidden),
                "{bad}"
            );
        }
        // Sub-frames are never trusted, even on the right origin.
        assert_eq!(classify(Channel::Shell, SHELL, false), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Popup, POPUP, false), Err(ProtocolError::Forbidden));
    }

    #[test]
    fn origin_checks_tabs() {
        assert_eq!(privileged(Channel::Tab, "https://krunk.invalid/settings.html"), Some(Sender::Page(Page::Settings)));
        assert_eq!(
            privileged(Channel::Tab, "https://krunk.invalid/history.html?q=x#y"),
            Some(Sender::Page(Page::History))
        );
        for bad in [
            "https://krunk.invalid:444/settings.html",
            "http://krunk.invalid/settings.html",
            "https://www.krunk.invalid/settings.html",
            "https://krunk.invalid.evil.com/settings.html",
            "https://krunk.invalid/unknown.html",
            SHELL, // a tab can never act as the shell or popup
            POPUP,
        ] {
            assert_eq!(privileged(Channel::Tab, bad), None, "{bad}");
        }
        // Ordinary pages are web content with their exact origin.
        assert_eq!(
            classify(Channel::Tab, "https://bank.example:8443/login?x=1", true),
            Ok(Source::Web { origin: "https://bank.example:8443".into() })
        );
        assert!(matches!(
            classify(Channel::Tab, "https://krunk.invalid.evil.com/settings.html", true),
            Ok(Source::Web { .. })
        ));
        // Our origins on unknown paths, opaque origins and sub-frames are rejected.
        assert_eq!(classify(Channel::Tab, "https://krunk.invalid/x.html", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Tab, "data:text/html,x", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Tab, "file:///C:/x.html", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Tab, "https://bank.example/", false), Err(ProtocolError::Forbidden));
    }

    /// The engine reports the sender as the frame's committed URL plus its
    /// main-frame flag; these are the URLs a CEF frame can have.
    #[test]
    fn cef_frame_urls() {
        let forbidden =
            |ch, url: &str, main| assert_eq!(classify(ch, url, main), Err(ProtocolError::Forbidden), "{url}");
        for ch in [Channel::Shell, Channel::Popup, Channel::Tab] {
            // Not yet navigated, srcdoc, error and DevTools frames.
            forbidden(ch, "", true);
            forbidden(ch, "about:blank", true);
            forbidden(ch, "about:srcdoc", true);
            forbidden(ch, "chrome-error://chromewebdata/", true);
            forbidden(ch, "devtools://devtools/bundled/devtools_app.html", true);
            forbidden(ch, "data:text/html,<script>x</script>", true);
            // blob: URLs carry our origin but are not our documents.
            forbidden(ch, "blob:https://shell.krunk.invalid/0b1c-uuid", true);
            forbidden(ch, "blob:https://krunk.invalid/0b1c-uuid", true);
        }
        // The shell document in a sub-frame (is_main false) is never privileged.
        forbidden(Channel::Shell, SHELL, false);
        forbidden(Channel::Tab, &Page::Settings.url(), false);
        // A page iframed into a tab cannot speak for the tab's origin.
        forbidden(Channel::Tab, "https://bank.example/login", false);
        // Main frames: exactly as before.
        assert_eq!(classify(Channel::Shell, SHELL, true), Ok(Source::Privileged(Sender::Shell)));
        assert_eq!(
            classify(Channel::Tab, &Page::Settings.url(), true),
            Ok(Source::Privileged(Sender::Page(Page::Settings)))
        );
        assert_eq!(
            classify(Channel::Tab, "http://plain.example/x", true),
            Ok(Source::Web { origin: "http://plain.example".into() })
        );
    }

    #[test]
    fn least_privilege_per_sender() {
        let newtab = Sender::Page(Page::NewTab);
        assert!(parse_request(newtab, r#"{"cmd":"getTopSites"}"#).is_ok());
        assert_eq!(
            parse_request(newtab, r#"{"cmd":"vaultReveal","id":"1","password":"x"}"#),
            Err(ProtocolError::Forbidden)
        );
        assert_eq!(parse_request(newtab, r#"{"cmd":"getSettings"}"#), Err(ProtocolError::Forbidden));
        let history = Sender::Page(Page::History);
        assert_eq!(parse_request(history, r#"{"cmd":"vaultList"}"#), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(history, r#"{"cmd":"closeTab","id":1}"#), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Shell, r#"{"cmd":"vaultList"}"#), Err(ProtocolError::Forbidden));
        assert!(parse_request(Sender::Page(Page::Passwords), r#"{"cmd":"vaultList"}"#).is_ok());
        assert!(parse_request(Sender::Page(Page::Settings), r#"{"cmd":"saveSettings","settings":{}}"#).is_ok());
        // Password prompt replies come from the popup only.
        let reply = r#"{"cmd":"savePasswordReply","tabId":1,"origin":"https://a.example","accept":true}"#;
        assert!(parse_request(Sender::Popup, reply).is_ok());
        assert_eq!(parse_request(Sender::Shell, reply), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(newtab, reply), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Popup, r#"{"cmd":"vaultList"}"#), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Popup, r#"{"cmd":"closeTab","id":1}"#), Err(ProtocolError::Forbidden));
    }

    /// The storage page's commands belong to it alone; the site-data page has
    /// only its own (not even OpenUrl/OpenPage); web content has none.
    #[test]
    fn storage_and_site_data_commands_are_page_only() {
        let storage = [
            r#"{"cmd":"storageList"}"#,
            r#"{"cmd":"storageUsage","origins":["https://a.example"]}"#,
            r#"{"cmd":"storageCookies","site":"a.example"}"#,
            r#"{"cmd":"storageDeleteCookie","cookie":{"name":"n","domain":".a.example","path":"/"}}"#,
            r#"{"cmd":"storageDeleteSite","site":"a.example"}"#,
            r#"{"cmd":"storageDeleteAllCookies"}"#,
            r#"{"cmd":"storageClearOrigin","origin":"https://a.example"}"#,
        ];
        let site_data = [
            r#"{"cmd":"siteDataGet"}"#,
            r#"{"cmd":"siteDataSet","area":"local","key":"k","value":"v"}"#,
            r#"{"cmd":"siteDataRemove","area":"session","key":"k"}"#,
            r#"{"cmd":"siteDataClear","area":"local"}"#,
            r#"{"cmd":"siteCookiesDelete"}"#,
            r#"{"cmd":"siteCookiesDelete","cookie":{"name":"n","domain":"a.example","path":"/"}}"#,
            r#"{"cmd":"siteCookieSet","cookie":{"name":"n","domain":"a.example","path":"/"},"value":"x"}"#,
        ];
        let senders = |except: Sender| {
            let mut v = vec![Sender::Shell, Sender::Popup];
            v.extend(Page::ALL.into_iter().map(Sender::Page));
            v.into_iter().filter(move |s| *s != except)
        };
        for msg in storage {
            assert!(parse_request(Sender::Page(Page::Storage), msg).is_ok(), "{msg}");
            for s in senders(Sender::Page(Page::Storage)) {
                assert_eq!(parse_request(s, msg), Err(ProtocolError::Forbidden), "{s:?} {msg}");
            }
            assert!(parse_content(msg).is_err());
        }
        for msg in site_data {
            assert!(parse_request(Sender::Page(Page::SiteData), msg).is_ok(), "{msg}");
            for s in senders(Sender::Page(Page::SiteData)) {
                assert_eq!(parse_request(s, msg), Err(ProtocolError::Forbidden), "{s:?} {msg}");
            }
            assert!(parse_content(msg).is_err());
        }
        let sd = Sender::Page(Page::SiteData);
        assert!(parse_request(sd, r#"{"cmd":"ready"}"#).is_ok());
        assert_eq!(
            parse_request(sd, r#"{"cmd":"openUrl","url":"https://evil.example/"}"#),
            Err(ProtocolError::Forbidden)
        );
        assert_eq!(parse_request(sd, r#"{"cmd":"openPage","page":"settings"}"#), Err(ProtocolError::Forbidden));
        // The site-data page names no tab or origin: extra fields are refused.
        assert!(parse_request(sd, r#"{"cmd":"siteDataGet","origin":"https://bank.example"}"#).is_err());
        assert!(parse_request(sd, r#"{"cmd":"siteDataSet","area":"local","key":"k","value":"v","tabId":3}"#).is_err());
        assert!(parse_request(sd, r#"{"cmd":"siteDataSet","area":"indexedDb","key":"k","value":"v"}"#).is_err());
        // Opening it is the popup's (shield panel) business only.
        assert!(parse_request(Sender::Popup, r#"{"cmd":"openSiteData"}"#).is_ok());
        assert_eq!(parse_request(Sender::Shell, r#"{"cmd":"openSiteData"}"#), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(sd, r#"{"cmd":"openSiteData"}"#), Err(ProtocolError::Forbidden));
    }

    /// Only the shell lays out the tab row and drives the window buttons.
    #[test]
    fn window_frame_commands_are_shell_only() {
        let map = r#"{"cmd":"hitMap","map":{"stripHeight":36,"interactive":[],
            "minimize":{"x":0,"y":0,"width":46,"height":36},"maximize":{"x":46,"y":0,"width":46,"height":36},
            "close":{"x":92,"y":0,"width":46,"height":36}}}"#;
        let control = r#"{"cmd":"windowControl","action":"close"}"#;
        for msg in [map, control] {
            assert!(parse_request(Sender::Shell, msg).is_ok());
            assert_eq!(parse_request(Sender::Popup, msg), Err(ProtocolError::Forbidden));
            for p in Page::ALL {
                assert_eq!(parse_request(Sender::Page(p), msg), Err(ProtocolError::Forbidden));
            }
            assert!(parse_content(msg).is_err());
        }
        assert!(parse_request(Sender::Shell, r#"{"cmd":"windowControl","action":"explode"}"#).is_err());
    }

    /// Web pages can never reach history or bookmark commands: their only
    /// channel is `parse_content`, and no built-in page outside History and
    /// Bookmarks (and Settings for clearing data) is allowed them either.
    #[test]
    fn web_content_cannot_touch_history_or_bookmarks() {
        let privileged = [
            r#"{"cmd":"deleteHistory","ids":[1,2]}"#,
            r#"{"cmd":"deleteHistoryRange","from":0,"to":9999999999999}"#,
            r#"{"cmd":"clearBrowsingData","data":{"history":true}}"#,
            r#"{"cmd":"queryHistory","text":""}"#,
            r#"{"cmd":"getBookmarks"}"#,
            r#"{"cmd":"addBookmark","parent":1,"title":"x","url":"https://evil.example/"}"#,
            r#"{"cmd":"editBookmark","id":3,"title":"x"}"#,
            r#"{"cmd":"moveBookmark","id":3,"parent":2,"index":0}"#,
            r#"{"cmd":"deleteBookmark","id":3}"#,
            r#"{"cmd":"searchBookmarks","text":""}"#,
            r#"{"cmd":"importBookmarks"}"#,
            r#"{"cmd":"exportBookmarks"}"#,
            r#"{"cmd":"getDownloads"}"#,
            r#"{"cmd":"downloadAction","id":1,"action":"open"}"#,
        ];
        // A web origin is classified as web content, never privileged...
        assert!(matches!(classify(Channel::Tab, "https://evil.example/history.html", true), Ok(Source::Web { .. })));
        for json in privileged {
            // ...and the content parser rejects every privileged command.
            assert!(parse_content(json).is_err(), "web content accepted {json}");
            // Built-in pages without the permission are refused too.
            for page in [Page::NewTab, Page::About, Page::Passwords, Page::HttpsOnly] {
                assert_eq!(parse_request(Sender::Page(page), json), Err(ProtocolError::Forbidden), "{page:?} {json}");
            }
        }
        assert!(parse_request(Sender::Page(Page::History), privileged[0]).is_ok());
        assert!(parse_request(Sender::Page(Page::Bookmarks), privileged[5]).is_ok());
        assert!(parse_request(Sender::Page(Page::Downloads), privileged[13]).is_ok());
        assert_eq!(parse_request(Sender::Page(Page::History), privileged[8]), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Page(Page::Bookmarks), privileged[0]), Err(ProtocolError::Forbidden));
    }

    #[test]
    fn bookmark_folder_popup_permissions() {
        let shell = r#"{"cmd":"showBookmarkFolder","id":5,"rect":{"x":100,"y":78,"width":90,"height":24}}"#;
        let inplace = r#"{"cmd":"showBookmarkFolder","id":6}"#;
        assert!(parse_request(Sender::Shell, shell).is_ok());
        assert!(parse_request(Sender::Popup, inplace).is_ok());
        // The popup cannot move itself around the screen.
        assert_eq!(parse_request(Sender::Popup, shell), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Page(Page::NewTab), inplace), Err(ProtocolError::Forbidden));
    }

    #[test]
    fn page_tokens_are_capped() {
        let ok = r#"{"lb":"pageTokens","classes":["ad-banner","content"],"ids":["sidebar"]}"#;
        assert!(matches!(parse_content(ok), Ok(ContentMessage::PageTokens { .. })));
        let many: Vec<String> = (0..MAX_PAGE_TOKENS + 1).map(|i| format!("c{i}")).collect();
        let j = serde_json::json!({"lb": "pageTokens", "classes": many, "ids": []}).to_string();
        assert_eq!(parse_content(&j), Err(ProtocolError::Invalid("page tokens")));
        let long = serde_json::json!({"lb": "pageTokens", "classes": ["x".repeat(101)], "ids": []}).to_string();
        assert_eq!(parse_content(&long), Err(ProtocolError::Invalid("page tokens")));
        assert!(parse_content(r#"{"lb":"pageTokens","classes":[""],"ids":[]}"#).is_err());
    }

    /// Settings commands are accepted only from the settings page.
    #[test]
    fn settings_commands_are_settings_page_only() {
        let cmds = [
            r#"{"cmd":"getSettings"}"#,
            r#"{"cmd":"saveSettings","settings":{"proxy":"127.0.0.1:8080"}}"#,
            r#"{"cmd":"resetSettings"}"#,
            r#"{"cmd":"pickDownloadFolder"}"#,
            r#"{"cmd":"restart"}"#,
            r#"{"cmd":"updateFilterLists"}"#,
            r#"{"cmd":"allowlistAdd","host":"evil.example"}"#,
        ];
        for json in cmds {
            assert!(parse_request(Sender::Page(Page::Settings), json).is_ok(), "{json}");
            assert!(parse_content(json).is_err(), "web content: {json}");
            for other in [
                Sender::Shell,
                Sender::Popup,
                Sender::Page(Page::NewTab),
                Sender::Page(Page::Passwords),
                Sender::Page(Page::History),
            ] {
                assert_eq!(parse_request(other, json), Err(ProtocolError::Forbidden), "{other:?} {json}");
            }
        }
        // A web page that loads the settings URL in an iframe or claims it is
        // still classified by its real origin.
        assert!(matches!(classify(Channel::Tab, "https://evil.example/settings.html", true), Ok(Source::Web { .. })));
        assert_eq!(classify(Channel::Tab, &Page::Settings.url(), false), Err(ProtocolError::Forbidden), "sub-frame");
    }

    #[test]
    fn update_commands() {
        for json in [r#"{"cmd":"getUpdateStatus"}"#, r#"{"cmd":"checkForUpdates"}"#] {
            assert!(parse_request(Sender::Page(Page::Settings), json).is_ok(), "{json}");
            assert!(parse_content(json).is_err(), "web content: {json}");
            for other in [Sender::Shell, Sender::Popup, Sender::Page(Page::NewTab), Sender::Page(Page::SiteData)] {
                assert_eq!(parse_request(other, json), Err(ProtocolError::Forbidden), "{other:?} {json}");
            }
        }
        // The prompt answers; it names no version or URL (the host has those).
        let reply = r#"{"cmd":"updateReply","choice":"install"}"#;
        assert_eq!(
            parse_request(Sender::Popup, reply).map(|e| e.request),
            Ok(Request::UpdateReply { choice: UpdateChoice::Install })
        );
        assert!(parse_request(Sender::Page(Page::Settings), reply).is_ok());
        assert!(parse_content(reply).is_err());
        assert_eq!(parse_request(Sender::Shell, reply), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Page(Page::About), reply), Err(ProtocolError::Forbidden));
        assert!(parse_request(
            Sender::Popup,
            r#"{"cmd":"updateReply","choice":"install","url":"https://evil.example/x.exe"}"#
        )
        .is_err());
        assert!(parse_request(Sender::Popup, r#"{"cmd":"updateReply","choice":"run"}"#).is_err());
        // The shell cannot open the update prompt itself.
        assert!(!PopupKind::Update.shell_may_open());
        assert!(PopupKind::Update.takes_focus());
        let content = PopupContent::Update {
            version: "0.2.1".into(),
            current: "0.2.0".into(),
            install: true,
            has_notes: true,
            state: UpdateProgress::Downloading { percent: 40 },
        };
        let v: serde_json::Value = serde_json::to_value(&content).unwrap();
        assert_eq!(v["kind"], "update");
        assert_eq!(v["hasNotes"], true);
        assert_eq!(v["state"]["step"], "downloading");
        assert_eq!(v["state"]["percent"], 40);
    }

    #[test]
    fn invalid_settings_reply_carries_field_errors() {
        let s = crate::settings::Settings { proxy: "nope".into(), ..Default::default() };
        let errs = s.validate().unwrap_err();
        let j = Event::invalid(Some(9), &errs).to_json();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["reqId"], 9);
        assert_eq!(v["data"][0]["field"], "proxy");
    }

    #[test]
    fn allowlist_commands_are_settings_only() {
        let add = r#"{"cmd":"allowlistAdd","host":"example.com"}"#;
        assert!(parse_request(Sender::Page(Page::Settings), add).is_ok());
        assert_eq!(parse_request(Sender::Page(Page::NewTab), add), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(Sender::Shell, add), Err(ProtocolError::Forbidden), "shell uses shieldAllowSite");
        assert!(parse_content(add).is_err(), "web pages cannot allowlist themselves");
    }

    /// Error messages never echo the input, which may contain a password.
    #[test]
    fn vault_save_note_is_optional() {
        let parse = |entry: &str| match parse_request(
            Sender::Page(Page::Passwords),
            &format!(r#"{{"cmd":"vaultSave","entry":{entry}}}"#),
        ) {
            Ok(Envelope { request: Request::VaultSave { entry }, .. }) => entry.note,
            other => panic!("unexpected {other:?}"),
        };
        let base = r#""id":"a1","origin":"https://a.example","username":"u","password":null"#;
        assert_eq!(parse(&format!("{{{base},\"note\":null}}")), None, "null keeps the stored note");
        assert_eq!(parse(&format!("{{{base}}}")), None, "absent keeps the stored note");
        assert_eq!(parse(&format!("{{{base},\"note\":\"\"}}")), Some(String::new()), "empty string clears it");
    }

    #[test]
    fn errors_do_not_leak_values() {
        let secret = "Tr0ub4dor&3-super-secret";
        let cases = [
            format!(r#"{{"cmd":"vaultUnlock","password":"{secret}","extra":"{secret}"}}"#),
            format!(r#"{{"cmd":"vaultUnlock","password":["{secret}"]}}"#),
            format!(r#"{{"cmd":"{secret}"}}"#),
            format!(r#"{{"cmd":"vaultUnlock","password":"{secret}","#),
        ];
        for json in &cases {
            let e = parse_request(Sender::Page(Page::Passwords), json).unwrap_err();
            assert!(!e.to_string().contains("secret"), "{e}");
        }
        let e =
            parse_content(&format!(r#"{{"lb":"submit","username":"u","password":{{"x":"{secret}"}}}}"#)).unwrap_err();
        assert!(!e.to_string().contains("secret"), "{e}");
        // Debug output of parsed requests redacts secrets too.
        let ok =
            parse_request(Sender::Page(Page::Passwords), &format!(r#"{{"cmd":"vaultUnlock","password":"{secret}"}}"#))
                .unwrap();
        assert!(!format!("{ok:?}").contains("secret"));
    }

    #[test]
    fn content_messages_are_strict() {
        assert_eq!(parse_content(r#"{"lb":"loginForm"}"#), Ok(ContentMessage::LoginForm {}));
        assert_eq!(
            parse_content(r#"{"lb":"submit","username":"a","password":"b"}"#),
            Ok(ContentMessage::Submit { username: "a".into(), password: Secret::from("b") })
        );
        assert!(parse_content(r#"{"cmd":"vaultList"}"#).is_err());
        assert!(parse_content(r#"{"lb":"vaultList"}"#).is_err());
        assert!(parse_content(r#"{"cmd":"savePasswordReply","tabId":1,"origin":"x","accept":true}"#).is_err());
        assert!(parse_content(r#"{"lb":"loginForm","origin":"https://bank.example"}"#).is_err());
        assert!(parse_content(r#"{"lb":"submit","username":"a","password":""}"#).is_err());
        let long = format!(r#"{{"lb":"submit","username":"{}","password":"x"}}"#, "u".repeat(600));
        assert_eq!(parse_content(&long), Err(ProtocolError::Invalid("username too long")));
        let huge = format!(r#"{{"lb":"submit","username":"a","password":"{}"}}"#, "p".repeat(MAX_CONTENT_BYTES));
        assert_eq!(parse_content(&huge), Err(ProtocolError::TooLarge));
    }

    /// Every request variant survives JSON in both directions.
    #[test]
    fn requests_round_trip() {
        let rect = PopupRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 };
        let samples = vec![
            Request::Ready,
            Request::NewTab { url: Some("https://a.example/".into()), private: true },
            Request::CloseTab { id: 1 },
            Request::ActivateTab { id: 2 },
            Request::MoveTab { id: 3, index: 4 },
            Request::DuplicateTab { id: 5 },
            Request::ReopenClosedTab,
            Request::Navigate { input: "rust lang".into() },
            Request::Back,
            Request::Forward,
            Request::Reload,
            Request::Stop,
            Request::OpenUrl { url: "https://b.example/".into(), new_tab: true },
            Request::OpenPage { page: "settings".into() },
            Request::ToggleBookmark,
            Request::HitMap {
                map: crate::frame::HitMap {
                    strip_height: 36.0,
                    interactive: vec![rect],
                    minimize: rect,
                    maximize: rect,
                    close: rect,
                    tab_centers: vec![10.0, 90.0],
                },
            },
            Request::WindowControl { action: WindowAction::Maximize },
            Request::StorageList,
            Request::StorageUsage { origins: vec!["https://a.example".into()] },
            Request::StorageCookies { site: "a.example".into() },
            Request::StorageDeleteCookie {
                cookie: crate::sitedata::CookieKey { name: "n".into(), domain: ".a.example".into(), path: "/".into() },
            },
            Request::StorageDeleteSite { site: "a.example".into() },
            Request::StorageDeleteAllCookies,
            Request::StorageClearOrigin { origin: "https://a.example".into() },
            Request::SiteDataGet {},
            Request::SiteDataSet { area: crate::sitedata::StorageArea::Local, key: "k".into(), value: "v".into() },
            Request::SiteDataRemove { area: crate::sitedata::StorageArea::Session, key: "k".into() },
            Request::SiteDataClear { area: crate::sitedata::StorageArea::Local },
            Request::SiteCookiesDelete { cookie: None },
            Request::SiteCookieSet {
                cookie: crate::sitedata::CookieKey { name: "n".into(), domain: ".a.example".into(), path: "/".into() },
                value: "%00%FF".into(),
            },
            Request::OpenSiteData,
            Request::ShowPopup { popup: PopupKind::Shield, rect },
            Request::TabDragOut { id: 3, offset_x: 40.5, offset_y: 18.0 },
            Request::ShowTabMenu { id: 3, rect },
            Request::OpenBlockedPage {},
            Request::GetBlocked {},
            Request::GetFilterLists {},
            Request::SetFilterList { id: "custom-00".into(), enabled: false },
            Request::AddFilterList { url: "https://l.example/a.txt".into() },
            Request::RemoveFilterList { id: "custom-00".into() },
            Request::RestoreDefaultLists {},
            Request::GetUserFilters {},
            Request::SaveUserFilters { text: "||a.example^".into() },
            Request::DownloadAction { id: 4, action: DownloadAction::CopyLink },
            Request::DownloadAction { id: 4, action: DownloadAction::CopyPageAddress },
            Request::DownloadAction { id: 4, action: DownloadAction::Retry },
            Request::DownloadAction { id: 4, action: DownloadAction::DeleteFile },
            Request::ShowBookmarkMenu { id: Some(3), rect },
            Request::ShowBookmarkMenu { id: None, rect },
            Request::BookmarkMenuAction { id: None, action: BookmarkAction::NewFolder },
            Request::SaveBookmark { id: Some(2), parent: 1, title: "B".into(), url: Some("https://b.example/".into()) },
            Request::SaveBookmark { id: None, parent: 1, title: "F".into(), url: None },
            Request::TabMenuAction { id: 3, action: TabAction::MoveToNewWindow },
            Request::DevToolsDock { dock: crate::settings::DevToolsDock::Bottom },
            Request::DevToolsClose,
            Request::Command { command: Command::NewWindow },
            Request::HidePopup,
            Request::Command { command: Command::ZoomIn },
            Request::Find { query: "x".into(), backwards: true },
            Request::FindClose,
            Request::Zoom { action: ZoomAction::Reset },
            Request::Print,
            Request::DevTools,
            Request::ShieldAllowSite { allow: true },
            Request::SavePasswordReply { tab_id: 1, origin: "https://a.example".into(), accept: true, never: false },
            Request::FillReply { tab_id: 1, origin: "https://a.example".into(), entry_id: None },
            Request::Quit,
            Request::GetTopSites,
            Request::PinSite { url: "https://a.example/".into(), title: "A".into() },
            Request::QueryHistory { text: "q".into(), before: Some(5), limit: 10 },
            Request::DeleteHistoryRange { from: 1, to: 2 },
            Request::ClearBrowsingData { data: ClearData { history: true, ..ClearData::default() } },
            Request::MoveBookmark { id: 1, parent: 2, index: 0 },
            Request::SaveSettings { settings: Box::new(Settings::default()) },
            Request::VaultSave {
                entry: EntryInput {
                    id: None,
                    origin: "https://a.example".into(),
                    username: "u".into(),
                    password: Some(Secret::from("p")),
                    note: None,
                },
            },
            Request::GeneratePassword { length: 24, symbols: false },
            Request::DownloadAction { id: 1, action: DownloadAction::ShowInFolder },
            Request::ClearDownloads,
            Request::ShowBookmarkFolder { id: 4, rect: Some(rect) },
            Request::ShowBookmarkFolder { id: 4, rect: None },
            Request::ClearBrowsingData {
                data: ClearData { cookies: true, cache: true, site_data: true, since: Some(1), ..ClearData::default() },
            },
            Request::HttpsOnlyProceed { url: "http://a.example/".into() },
            Request::GetFilterStatus,
            Request::RequestFill,
            Request::GetNeverSave,
            Request::NeverSaveRemove { origin: "https://a.example".into() },
            Request::VaultUnlock { password: Secret::from("pw") },
            Request::VaultChangePassword { old: Secret::from("a"), new: Secret::from("b") },
            Request::GetAllowlist,
            Request::AllowlistAdd { host: "a.example".into() },
            Request::AllowlistRemove { host: "a.example".into() },
        ];
        for r in samples {
            let json = serde_json::to_string(&r).unwrap();
            let back: Request = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(back, r, "{json}");
        }
    }

    #[test]
    fn blocked_list_requests() {
        let open = Request::OpenBlockedPage {};
        let get = Request::GetBlocked {};
        assert!(Sender::Popup.permits(&open) && !Sender::Shell.permits(&open));
        assert!(Sender::Page(Page::Adblock).permits(&get));
        for s in [Sender::Shell, Sender::Popup, Sender::Page(Page::NewTab), Sender::Page(Page::Settings)] {
            assert!(!s.permits(&get), "{s:?}");
        }
        assert!(!Sender::Page(Page::Adblock).permits(&Request::SiteDataGet {}));
        let edits = [
            Request::GetFilterLists {},
            Request::SetFilterList { id: "easylist".into(), enabled: false },
            Request::AddFilterList { url: "https://l.example/a.txt".into() },
            Request::RemoveFilterList { id: "custom-00".into() },
            Request::RestoreDefaultLists {},
            Request::GetUserFilters {},
            Request::SaveUserFilters { text: String::new() },
        ];
        for r in &edits {
            assert!(Sender::Page(Page::Adblock).permits(r), "{r:?}");
            for s in [Sender::Shell, Sender::Popup, Sender::Page(Page::Settings), Sender::Page(Page::NewTab)] {
                assert!(!s.permits(r), "{s:?} {r:?}");
            }
        }
        assert!(Sender::Page(Page::Adblock).permits(&Request::UpdateFilterLists));
        assert!(!Sender::Page(Page::Adblock).permits(&Request::SaveSettings { settings: Box::default() }));
        assert_eq!(Page::from_name("adblock"), Some(Page::Adblock));
        assert!(Page::Adblock.single_instance());
    }

    #[test]
    fn bookmarks_bar_requests_are_shell_and_popup_only() {
        let rect = PopupRect { x: 10.0, y: 80.0, width: 240.0, height: 300.0 };
        let menu = Request::ShowBookmarkMenu { id: Some(10), rect };
        let mv = Request::MoveBookmark { id: 10, parent: 11, index: 0 };
        let action = Request::BookmarkMenuAction { id: Some(10), action: BookmarkAction::Delete };
        let save = Request::SaveBookmark { id: None, parent: 1, title: "Tools".into(), url: None };
        assert!(Sender::Shell.permits(&menu) && Sender::Shell.permits(&mv));
        assert!(!Sender::Shell.permits(&action) && !Sender::Shell.permits(&save), "choices come from the popup");
        assert!(Sender::Popup.permits(&action) && Sender::Popup.permits(&save));
        assert!(!Sender::Popup.permits(&mv) && !Sender::Popup.permits(&menu));
        for page in [Page::NewTab, Page::Settings, Page::History, Page::SiteData] {
            let p = Sender::Page(page);
            assert!(!p.permits(&menu) && !p.permits(&action) && !p.permits(&save) && !p.permits(&mv), "{page:?}");
        }
        assert!(Sender::Page(Page::Bookmarks).permits(&mv), "the bookmarks page still moves bookmarks");
        let parsed = parse_request(
            Sender::Shell,
            r#"{"cmd":"showBookmarkMenu","id":null,"rect":{"x":1,"y":2,"width":240,"height":300}}"#,
        )
        .unwrap();
        assert_eq!(
            parsed.request,
            Request::ShowBookmarkMenu { id: None, rect: PopupRect { x: 1.0, y: 2.0, width: 240.0, height: 300.0 } }
        );
        let parsed =
            parse_request(Sender::Popup, r#"{"cmd":"bookmarkMenuAction","id":7,"action":"showOnNewTab"}"#).unwrap();
        assert_eq!(parsed.request, Request::BookmarkMenuAction { id: Some(7), action: BookmarkAction::ShowOnNewTab });
        let parsed = parse_request(
            Sender::Popup,
            r#"{"cmd":"saveBookmark","id":3,"parent":1,"title":"A","url":"https://a.example/"}"#,
        )
        .unwrap();
        assert_eq!(
            parsed.request,
            Request::SaveBookmark { id: Some(3), parent: 1, title: "A".into(), url: Some("https://a.example/".into()) }
        );
        let content = PopupContent::BookmarkMenu {
            id: None,
            is_folder: false,
            title: String::new(),
            url: None,
            children: 0,
            mode: crate::settings::BookmarksBar::NewTab,
            can_add: true,
        };
        assert_eq!(content.kind(), PopupKind::BookmarkMenu);
        assert!(serde_json::to_string(&content).unwrap().contains(r#""mode":"newTab""#));
        assert!(!PopupKind::BookmarkMenu.shell_may_open() && !PopupKind::BookmarkEdit.shell_may_open());
        let m = crate::layout::bookmark_menu_rect(5.0, 6.0, true, false);
        assert_eq!((m.x, m.y, m.width, m.height), (5.0, 6.0, 240.0, 353.0));
        assert_eq!(crate::layout::bookmark_menu_rect(0.0, 0.0, true, true).height, 293.0);
        assert_eq!(crate::layout::bookmark_menu_rect(0.0, 0.0, false, false).height, 222.0);
    }

    #[test]
    fn tab_moves_are_shell_and_popup_only() {
        let drag = Request::TabDragOut { id: 1, offset_x: 1.0, offset_y: 1.0 };
        let menu = Request::ShowTabMenu { id: 1, rect: PopupRect { x: 1.0, y: 1.0, width: 240.0, height: 196.0 } };
        let action = Request::TabMenuAction { id: 1, action: TabAction::Close };
        assert!(Sender::Shell.permits(&drag) && Sender::Shell.permits(&menu));
        assert!(!Sender::Shell.permits(&action), "menu choices come from the popup");
        assert!(Sender::Popup.permits(&action));
        assert!(!Sender::Popup.permits(&drag));
        for page in [Page::NewTab, Page::Settings, Page::History] {
            assert!(!Sender::Page(page).permits(&drag) && !Sender::Page(page).permits(&action), "{page:?}");
        }
        let parsed = parse_request(Sender::Shell, r#"{"cmd":"tabDragOut","id":4,"offsetX":12,"offsetY":20}"#).unwrap();
        assert_eq!(parsed.request, Request::TabDragOut { id: 4, offset_x: 12.0, offset_y: 20.0 });
        let parsed =
            parse_request(Sender::Popup, r#"{"cmd":"tabMenuAction","id":4,"action":"moveToNewWindow"}"#).unwrap();
        assert_eq!(parsed.request, Request::TabMenuAction { id: 4, action: TabAction::MoveToNewWindow });
        let parsed = parse_request(Sender::Popup, r#"{"cmd":"command","command":{"command":"newWindow"}}"#).unwrap();
        assert_eq!(parsed.request, Request::Command { command: Command::NewWindow });
        assert_eq!(Event::TabDropHint { index: Some(1) }.to_json(), r#"{"event":"tabDropHint","index":1}"#);
        let menu = PopupContent::TabMenu { tab_id: 2, can_move: false, can_close_right: true };
        assert_eq!(menu.kind(), PopupKind::TabMenu);
        let json = serde_json::to_string(&menu).unwrap();
        assert!(json.contains(r#""canCloseRight":true"#), "{json}");
        for (name, action) in [("closeOthers", TabAction::CloseOthers), ("closeRight", TabAction::CloseRight)] {
            let parsed =
                parse_request(Sender::Popup, &format!(r#"{{"cmd":"tabMenuAction","id":4,"action":"{name}"}}"#))
                    .unwrap();
            assert_eq!(parsed.request, Request::TabMenuAction { id: 4, action });
            assert!(parse_request(Sender::Shell, &format!(r#"{{"cmd":"tabMenuAction","id":4,"action":"{name}"}}"#))
                .is_err());
        }
        assert!(!PopupKind::TabMenu.shell_may_open(), "the shell asks with showTabMenu, not showPopup");
    }

    #[test]
    fn events_round_trip_and_shape() {
        let j = Event::ok(Some(3), vec![1, 2]).to_json();
        assert_eq!(j, r#"{"event":"reply","reqId":3,"ok":true,"data":[1,2]}"#);
        assert_eq!(
            Event::FindResult { active: 1, total: 4 }.to_json(),
            r#"{"event":"findResult","active":1,"total":4}"#
        );
        let samples = vec![
            Event::err(None, "nope"),
            Event::Tabs {
                tabs: vec![TabInfo {
                    id: 1,
                    title: "T".into(),
                    url: "krunk://settings".into(),
                    favicon: Some("data:image/png;base64,AA".into()),
                    loading: true,
                    private: true,
                    sleeping: false,
                }],
                active: Some(1),
            },
            Event::Nav {
                state: NavState {
                    tab_id: 1,
                    url: "https://a.example/".into(),
                    can_go_back: true,
                    can_go_forward: false,
                    loading: false,
                    secure: true,
                    bookmarked: false,
                    blocked: 7,
                    site_allowed: false,
                    zoom: 1.25,
                    private: false,
                    login_form: true,
                    bookmarks_bar: true,
                },
            },
            Event::ShellConfig { theme: Theme::Dark },
            Event::Popup { content: PopupContent::Menu { zoom: 1.0, can_reopen: true } },
            Event::Popup {
                content: PopupContent::Shield {
                    host: "a.example".into(),
                    blocked: 3,
                    allowed: false,
                    enabled: true,
                    recent: vec![crate::blocklog::Item {
                        url: "https://ads.example/a.js".into(),
                        host: "ads.example".into(),
                        kind: "script".into(),
                        count: 2,
                        redirected: false,
                        rule: None,
                        list: None,
                    }],
                    distinct: 1,
                },
            },
            Event::Popup {
                content: PopupContent::SavePassword {
                    tab_id: 2,
                    origin: "https://a.example".into(),
                    username: "u".into(),
                    update: false,
                },
            },
            Event::Popup {
                content: PopupContent::Fill {
                    tab_id: 2,
                    origin: "https://a.example".into(),
                    entries: vec![FillChoice { id: "e1".into(), username: "u".into() }],
                },
            },
            Event::PopupClosed,
            Event::TabDropHint { index: Some(2) },
            Event::TabDropHint { index: None },
            Event::DevTools { open: true, dock: crate::settings::DevToolsDock::Right },
            Event::Popup { content: PopupContent::Hint { text: "Press Esc to show your cursor".into() } },
            Event::CaptionState { hover: Some(CaptionButton::Maximize), pressed: false },
            Event::CaptionState { hover: None, pressed: false },
            Event::WindowState { maximized: true, active: false, fullscreen: false },
            Event::Popup {
                content: PopupContent::Unlock {
                    tab_id: 1,
                    origin: "https://a.example".into(),
                    purpose: UnlockPurpose::Fill,
                    error: Some("Wrong master password".into()),
                    wait_ms: 2000,
                },
            },
            Event::Popup {
                content: PopupContent::BookmarkFolder {
                    id: 3,
                    title: "News".into(),
                    items: vec![BarItem {
                        id: 4,
                        title: "BBC".into(),
                        url: Some("https://www.bbc.co.uk/".into()),
                        is_folder: false,
                        favicon: Some("/favicon?host=www.bbc.co.uk".into()),
                    }],
                },
            },
            Event::BookmarksBar {
                items: vec![BarItem { id: 5, title: "Dev".into(), url: None, is_folder: true, favicon: None }],
            },
            Event::Downloads {
                items: vec![DownloadItem {
                    id: 1,
                    url: "https://e.com/a.zip".into(),
                    file_name: "a.zip".into(),
                    path: "C:\\dl\\a.zip".into(),
                    state: crate::downloads::DownloadState::InProgress,
                    received: 10,
                    total: Some(100),
                    started: 5,
                    reason: None,
                    private: false,
                    finished: Some(9),
                    page_url: Some("https://e.com/".into()),
                    connections: Some(4),
                    parts: Vec::new(),
                }],
            },
            Event::VaultState { exists: true, locked: false },
            Event::DownloadsChanged { active: 1 },
            Event::Theme { theme: Theme::Light },
            Event::FocusAddressBar,
            Event::BlurAddressBar,
            Event::ShowFind,
            Event::Toast { message: "hi".into() },
            Event::RestartRequired,
        ];
        for e in samples {
            let json = e.to_json();
            let back: Event = serde_json::from_str(&json).unwrap_or_else(|err| panic!("{json}: {err}"));
            assert_eq!(back, e, "{json}");
        }
        // The save prompt never carries a password.
        let j = Event::Popup {
            content: PopupContent::SavePassword { tab_id: 1, origin: "o".into(), username: "u".into(), update: true },
        }
        .to_json();
        assert!(!j.contains("password\":"), "{j}");
    }
}
