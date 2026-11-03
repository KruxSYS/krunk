//! Typed JSON message protocol between the web UI and the Rust host.
//!
//! There are two inbound channels with very different trust levels:
//!
//! * [`Request`]: privileged commands from the browser chrome (shell
//!   WebView) and from built-in pages. The host has to check the sender's
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
    #[serde(default)]
    pub note: String,
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
        true
    }
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

/// What the popup WebView shows.
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
    Home,
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
    /// Opens the popup WebView at `rect` (shell: menu and shield only).
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

/// Who sent a privileged message, derived by the host from the WebView it
/// arrived on *and* the `Source` URI the runtime reports (see [`classify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    Shell,
    Popup,
    Page(Page),
}

/// Which WebView a message arrived on.
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
/// * The shell and popup WebViews accept only their exact document on the
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
        // Harmless navigation-ish commands every privileged sender may use.
        if matches!(req, R::Ready | R::OpenUrl { .. } | R::OpenPage { .. }) {
            return true;
        }
        match self {
            Sender::Shell => matches!(
                req,
                R::NewTab { .. }
                    | R::CloseTab { .. }
                    | R::ActivateTab { .. }
                    | R::MoveTab { .. }
                    | R::DuplicateTab { .. }
                    | R::ReopenClosedTab
                    | R::Navigate { .. }
                    | R::Back
                    | R::Forward
                    | R::Reload
                    | R::Stop
                    | R::Home
                    | R::ToggleBookmark
                    | R::ShowPopup { .. }
                    | R::HidePopup
                    | R::ShowBookmarkFolder { .. }
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
                    | R::HidePopup
                    | R::ShowBookmarkFolder { rect: None, .. }
                    | R::NewTab { .. }
                    | R::ReopenClosedTab
                    | R::Zoom { .. }
                    | R::ShieldAllowSite { .. }
                    | R::SavePasswordReply { .. }
                    | R::FillReply { .. }
                    | R::VaultStatus
                    | R::VaultUnlock { .. }
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
            ),
            Sender::Page(Page::Downloads) => {
                matches!(req, R::GetDownloads | R::ClearDownloads | R::DownloadAction { .. })
            }
            Sender::Page(Page::About) => matches!(req, R::GetAbout),
            Sender::Page(Page::HttpsOnly) => matches!(req, R::HttpsOnlyProceed { .. }),
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
    /// Address-bar text (built-in pages shown as `lb://...`).
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
        show_home_button: bool,
        show_bookmarks_bar: bool,
        theme: Theme,
    },
    /// To the popup WebView: render this content.
    Popup {
        content: PopupContent,
    },
    /// To the shell: the popup was hidden (un-highlight its button).
    PopupClosed,
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

    const SHELL: &str = "https://shell.lightbrowser.invalid/shell.html";
    const POPUP: &str = "https://shell.lightbrowser.invalid/popup.html";

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
            "https://shell.lightbrowser.invalid:8443/shell.html",     // port
            "http://shell.lightbrowser.invalid/shell.html",           // scheme
            "https://x.shell.lightbrowser.invalid/shell.html",        // subdomain
            "https://shell.lightbrowser.invalid.evil.com/shell.html", // look-alike suffix
            "https://shell-lightbrowser.invalid/shell.html",          // look-alike host
            "https://lightbrowser.invalid/shell.html",                // pages origin
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
        assert_eq!(
            privileged(Channel::Tab, "https://lightbrowser.invalid/settings.html"),
            Some(Sender::Page(Page::Settings))
        );
        assert_eq!(
            privileged(Channel::Tab, "https://lightbrowser.invalid/history.html?q=x#y"),
            Some(Sender::Page(Page::History))
        );
        for bad in [
            "https://lightbrowser.invalid:444/settings.html",
            "http://lightbrowser.invalid/settings.html",
            "https://www.lightbrowser.invalid/settings.html",
            "https://lightbrowser.invalid.evil.com/settings.html",
            "https://lightbrowser.invalid/unknown.html",
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
            classify(Channel::Tab, "https://lightbrowser.invalid.evil.com/settings.html", true),
            Ok(Source::Web { .. })
        ));
        // Our origins on unknown paths, opaque origins and sub-frames are rejected.
        assert_eq!(classify(Channel::Tab, "https://lightbrowser.invalid/x.html", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Tab, "data:text/html,x", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Tab, "file:///C:/x.html", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Tab, "https://bank.example/", false), Err(ProtocolError::Forbidden));
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
            Request::Home,
            Request::OpenUrl { url: "https://b.example/".into(), new_tab: true },
            Request::OpenPage { page: "settings".into() },
            Request::ToggleBookmark,
            Request::ShowPopup { popup: PopupKind::Shield, rect },
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
                    note: String::new(),
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
                    url: "lb://settings".into(),
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
                },
            },
            Event::ShellConfig { show_home_button: true, show_bookmarks_bar: false, theme: Theme::Dark },
            Event::Popup { content: PopupContent::Menu { zoom: 1.0, can_reopen: true } },
            Event::Popup {
                content: PopupContent::Shield { host: "a.example".into(), blocked: 3, allowed: false, enabled: true },
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
                }],
            },
            Event::VaultState { exists: true, locked: false },
            Event::DownloadsChanged { active: 1 },
            Event::Theme { theme: Theme::Light },
            Event::FocusAddressBar,
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
