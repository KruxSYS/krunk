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

use crate::pages::{self, Page};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};

/// Maximum size of a privileged message (settings JSON is the largest).
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Maximum size of a message from web content.
pub const MAX_CONTENT_BYTES: usize = 8 * 1024;
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
    pub downloads: bool,
    /// Unix milliseconds; `None` = everything.
    pub since: Option<i64>,
}

/// A vault entry as edited on the passwords page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryInput {
    /// `None` creates a new entry.
    pub id: Option<String>,
    pub origin: String,
    pub username: String,
    /// `None` keeps the stored password when editing.
    pub password: Option<String>,
    #[serde(default)]
    pub note: String,
}

/// Privileged commands. Secrets travel as plain `String` only as far as the
/// host's message handler, which moves them into `Zeroizing` immediately.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
    /// The shell needs the whole window (menus, popups) or just its strip.
    SetOverlay {
        open: bool,
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
    /// Reply to the "Save password?" prompt.
    SavePasswordReply {
        accept: bool,
        #[serde(default)]
        never: bool,
    },
    /// Reply to the fill prompt: the chosen entry, or `None` to dismiss.
    FillReply {
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
    UpdateFilterLists,
    Restart,

    // ---- vault ----
    VaultStatus,
    VaultSetup {
        password: String,
    },
    VaultUnlock {
        password: String,
    },
    VaultLock,
    VaultList,
    /// Shows one password; requires the master password again.
    VaultReveal {
        id: String,
        password: String,
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
    VaultChangePassword {
        old: String,
        new: String,
    },
    GeneratePassword {
        #[serde(default = "default_pw_len")]
        length: u32,
        #[serde(default = "yes")]
        symbols: bool,
    },

    // ---- downloads ----
    GetDownloads,
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
/// arrived on *and* the `Source` URI the runtime reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    Shell,
    Page(Page),
}

impl Sender {
    /// Classifies a message from the shell WebView: only the exact shell
    /// origin is trusted.
    pub fn from_shell_source(source: &str) -> Option<Sender> {
        pages::is_shell_origin(source).then_some(Sender::Shell)
    }

    /// Classifies a message from a tab WebView: only a built-in page on the
    /// exact pages origin is privileged. Everything else is web content.
    pub fn from_tab_source(source: &str) -> Option<Sender> {
        Page::from_url(source).map(Sender::Page)
    }

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
                    | R::SetOverlay { .. }
                    | R::Find { .. }
                    | R::FindClose
                    | R::Zoom { .. }
                    | R::Print
                    | R::DevTools
                    | R::ShieldAllowSite { .. }
                    | R::SavePasswordReply { .. }
                    | R::FillReply { .. }
                    | R::Quit
                    | R::GetBookmarks
                    | R::VaultStatus
                    | R::VaultUnlock { .. }
                    | R::VaultLock
                    | R::GeneratePassword { .. }
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
                    | R::GeneratePassword { .. }
            ),
            Sender::Page(Page::Settings) => matches!(
                req,
                R::GetSettings
                    | R::SaveSettings { .. }
                    | R::ResetSettings
                    | R::UpdateFilterLists
                    | R::Restart
                    | R::ClearBrowsingData { .. }
                    | R::VaultStatus
                    | R::VaultChangePassword { .. }
                    | R::GetAbout
            ),
            Sender::Page(Page::Downloads) => matches!(req, R::GetDownloads | R::DownloadAction { .. }),
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

/// Parses and authorizes a privileged message.
pub fn parse_request(sender: Sender, json: &str) -> Result<Envelope, ProtocolError> {
    if json.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
    let obj = value.as_object_mut().ok_or(ProtocolError::Invalid("not an object"))?;
    let req_id = match obj.remove("reqId") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(ProtocolError::Invalid("reqId"))?),
    };
    let request: Request = serde_json::from_value(value).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
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
    Submit { username: String, password: String },
    /// The user clicked the key badge in a login field. The host shows a
    /// trusted confirmation in the shell before anything is filled.
    RequestFill {},
}

/// Parses a message from web content.
pub fn parse_content(json: &str) -> Result<ContentMessage, ProtocolError> {
    if json.len() > MAX_CONTENT_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let msg: ContentMessage = serde_json::from_str(json).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
}

/// Messages to the shell or a built-in page.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// Ask the user whether to save a login (no password is sent to the UI).
    SavePasswordPrompt {
        origin: String,
        username: String,
        update: bool,
    },
    /// Offer to fill one of these entries on `origin`.
    FillPrompt {
        origin: String,
        entries: Vec<FillChoice>,
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
    Theme {
        dark: bool,
    },
    FocusAddressBar,
    ShowFind,
    Toast {
        message: String,
    },
    /// Settings that need a restart changed.
    RestartRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FillChoice {
    pub id: String,
    pub username: String,
}

impl Event {
    pub fn ok(req_id: Option<u32>, data: impl Serialize) -> Event {
        Event::Reply { req_id, ok: true, data: serde_json::to_value(data).ok(), error: None }
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

    #[test]
    fn parses_shell_commands() {
        let s = Sender::from_shell_source(SHELL).unwrap();
        let e = parse_request(s, r#"{"cmd":"newTab","private":true,"reqId":7}"#).unwrap();
        assert_eq!(e.req_id, Some(7));
        assert_eq!(e.request, Request::NewTab { url: None, private: true });
        let e = parse_request(s, r#"{"cmd":"moveTab","id":3,"index":0}"#).unwrap();
        assert_eq!(e.request, Request::MoveTab { id: 3, index: 0 });
        let e = parse_request(s, r#"{"cmd":"fillReply","entryId":"abc"}"#).unwrap();
        assert_eq!(e.request, Request::FillReply { entry_id: Some("abc".into()) });
    }

    #[test]
    fn rejects_malformed_and_unknown() {
        let s = Sender::Shell;
        assert!(matches!(parse_request(s, "nope"), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, "[1]"), Err(ProtocolError::Invalid(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"rmRf"}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"closeTab","id":1,"extra":1}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"closeTab","id":-1}"#), Err(ProtocolError::Malformed(_))));
        assert!(matches!(parse_request(s, r#"{"cmd":"back","reqId":"x"}"#), Err(ProtocolError::Invalid(_))));
        let big = format!(r#"{{"cmd":"navigate","input":"{}"}}"#, "a".repeat(MAX_REQUEST_BYTES));
        assert_eq!(parse_request(s, &big), Err(ProtocolError::TooLarge));
    }

    #[test]
    fn origin_classification() {
        assert_eq!(Sender::from_shell_source(SHELL), Some(Sender::Shell));
        assert_eq!(Sender::from_shell_source("https://evil.example/"), None);
        assert_eq!(Sender::from_shell_source("https://lightbrowser.invalid/settings.html"), None);
        assert_eq!(
            Sender::from_tab_source("https://lightbrowser.invalid/settings.html"),
            Some(Sender::Page(Page::Settings))
        );
        // A tab can never claim to be the shell.
        assert_eq!(Sender::from_tab_source(SHELL), None);
        assert_eq!(Sender::from_tab_source("https://evil.example/settings.html"), None);
    }

    #[test]
    fn least_privilege_per_page() {
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
        // The shell cannot dump the vault either.
        assert_eq!(parse_request(Sender::Shell, r#"{"cmd":"vaultList"}"#), Err(ProtocolError::Forbidden));
        assert!(parse_request(Sender::Page(Page::Passwords), r#"{"cmd":"vaultList"}"#).is_ok());
        assert!(parse_request(Sender::Page(Page::Settings), r#"{"cmd":"saveSettings","settings":{}}"#).is_ok());
    }

    #[test]
    fn content_messages_are_strict() {
        assert_eq!(parse_content(r#"{"lb":"loginForm"}"#), Ok(ContentMessage::LoginForm {}));
        assert_eq!(
            parse_content(r#"{"lb":"submit","username":"a","password":"b"}"#),
            Ok(ContentMessage::Submit { username: "a".into(), password: "b".into() })
        );
        // Privileged commands cannot be expressed on this channel.
        assert!(parse_content(r#"{"cmd":"vaultList"}"#).is_err());
        assert!(parse_content(r#"{"lb":"vaultList"}"#).is_err());
        assert!(parse_content(r#"{"lb":"loginForm","origin":"https://bank.example"}"#).is_err());
        assert!(parse_content(r#"{"lb":"submit","username":"a","password":""}"#).is_err());
        let long = format!(r#"{{"lb":"submit","username":"{}","password":"x"}}"#, "u".repeat(600));
        assert_eq!(parse_content(&long), Err(ProtocolError::Invalid("username too long")));
        let huge = format!(r#"{{"lb":"submit","username":"a","password":"{}"}}"#, "p".repeat(MAX_CONTENT_BYTES));
        assert_eq!(parse_content(&huge), Err(ProtocolError::TooLarge));
    }

    #[test]
    fn events_serialize_camel_case() {
        let j = Event::ok(Some(3), vec![1, 2]).to_json();
        assert_eq!(j, r#"{"event":"reply","reqId":3,"ok":true,"data":[1,2]}"#);
        let j = Event::FindResult { active: 1, total: 4 }.to_json();
        assert_eq!(j, r#"{"event":"findResult","active":1,"total":4}"#);
    }
}
