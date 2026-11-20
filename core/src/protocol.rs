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

use crate::agent::scratch::ScratchView;
use crate::agent::{AskSpec, Category, ModelChoice, RunState, ScriptCard, ScriptKind, StepView};
use crate::downloads::DownloadItem;
use crate::keys::Command;
use crate::pages::{self, Page};
use crate::secret::Secret;
use crate::settings::{AiProvider, Settings, Theme};
use serde::{Deserialize, Serialize};

/// Maximum size of a privileged message (settings JSON is the largest).
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Requests from extension pages (0.6.0) may be larger: `chrome.storage` values.
pub const MAX_EXT_REQUEST_BYTES: usize = 64 * 1024 * 1024;
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
    /// The lock/site icon's popup: the connection and the certificate.
    SiteInfo,
    /// The login suggestions under a focused password or username field: it never takes
    /// the focus from the field (the host shows it without activation).
    Suggest,
    SavePassword,
    /// "Password saved · Undo · Edit" after a generated password was used (0.6.0).
    PasswordSaved,
    /// Saved cards under a card field (0.6.0).
    CardFill,
    /// "Save this card?" (0.6.0).
    SaveCard,
    Fill,
    /// A folder of the bookmarks bar (opened with `ShowBookmarkFolder`).
    BookmarkFolder,
    /// Master-password prompt before saving or filling a login.
    Unlock,
    /// "Connect to devices on your local network?" (Krunk 0.5.0; opened by the host when the
    /// engine asks for local network access).
    Permission,
    /// Suggestions under the address bar while the user types (Krunk 0.5.1): history,
    /// bookmarks, a search or the typed address. The address bar keeps the focus.
    AddressSuggest,
    /// The address bar's context menu (Krunk 0.6.0): Undo, Cut, Copy, Paste, Paste and go,
    /// Delete, Select all. The address bar keeps the focus (and its selection).
    AddressMenu,
    /// "Krunk X.Y.Z is available" (opened by the host after an update check).
    Update,
    /// The title bar's menu (right-click on the caption or the empty strip, Krunk 0.6.0).
    CaptionMenu,
    /// The downloads button's popup (Krunk 0.6.0): the list with speed and progress.
    Downloads,
    /// Which video goes to Picture-in-Picture when the page has several (0.6.0).
    Pip,
    /// "Add <extension>?" before any extension is installed (0.7.0, R20): its name, the
    /// verified source and what it may do. Opened by the host only.
    ExtInstall,
    /// The volume bar next to the floating video window (0.7.0, R8). Opened by the host only;
    /// it never takes the keyboard (the floating window keeps it).
    PipVolume,
    /// A tab's context menu (right-click on the tab).
    TabMenu,
    /// The back or forward button's history menu (right-click, 0.8.0).
    NavMenu,
    /// A site's HTTP sign-in (Basic/Digest, 0.8.0): user name and password, the vault's logins
    /// for that origin. Opened by the host only.
    HttpAuth,
    /// 0.15.0: the card shown while the pointer rests on a tab (title, site, a small picture
    /// of the page). Its own popup window; it never takes the focus. Opened by the host only.
    TabPreview,
    /// The bookmarks bar's context menu (right-click on an item, a folder or the bar).
    BookmarkMenu,
    /// Name (and address) of a bookmark or folder: edit, rename, new folder.
    BookmarkEdit,
    /// Every tab of the window, searchable, with close buttons (0.11.0): the "▾" button the
    /// tab strip shows while its tabs do not fit.
    TabList,
    /// "This page isn't responding" (0.11.0): Wait / Close page. Opened by the host only.
    Unresponsive,
    /// A page's alert / confirm / prompt or "Leave site?" (0.12.4). Opened by the host only.
    JsDialog,
    /// The VPN button's popup (0.14.0): state, profile picker, Connect / Disconnect.
    Vpn,
    /// The context menu of an extension's toolbar icon (right-click, 0.16.0).
    ExtMenu,
}

impl PopupKind {
    /// Prompts about passwords must come from the host, never the shell.
    pub fn shell_may_open(self) -> bool {
        matches!(
            self,
            PopupKind::Menu
                | PopupKind::Shield
                | PopupKind::SiteInfo
                | PopupKind::Downloads
                | PopupKind::Pip
                | PopupKind::TabList
                | PopupKind::Vpn
        )
    }

    /// Every popup has buttons, so every popup takes keyboard focus. This
    /// also makes dismissal reliable: clicking anywhere else deactivates the
    /// popup window (a no-activate window gets no such signal when the page
    /// it covers already had focus). (The "Press Esc" hint is not a popup:
    /// it is a native overlay, see `overlay`.)
    /// What the popup hangs on (0.15.4): the context menus open at the click and may go above
    /// it; lists stay under their control and scroll; everything else (the ⋮ menu, prompts,
    /// dialogs) is shown whole, moved up if it must be.
    pub fn anchor(self) -> crate::layout::PopupAnchor {
        use crate::layout::PopupAnchor;
        match self {
            PopupKind::TabMenu
            | PopupKind::BookmarkMenu
            | PopupKind::CaptionMenu
            | PopupKind::AddressMenu
            | PopupKind::ExtMenu => PopupAnchor::Point,
            PopupKind::BookmarkFolder
            | PopupKind::TabList
            | PopupKind::AddressSuggest
            | PopupKind::Suggest
            | PopupKind::CardFill
            | PopupKind::TabPreview
            | PopupKind::PipVolume => PopupAnchor::Keep,
            _ => PopupAnchor::Drop,
        }
    }

    pub fn takes_focus(self) -> bool {
        !matches!(
            self,
            PopupKind::Suggest
                | PopupKind::CardFill
                | PopupKind::AddressSuggest
                | PopupKind::AddressMenu
                | PopupKind::PipVolume
                | PopupKind::TabPreview
        )
    }
}

/// A saved login offered in the HTTP sign-in dialog: its id and user name, never the password.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthLogin {
    pub entry_id: String,
    pub username: String,
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
    /// Mute or unmute every tab of this tab's site, and remember it for the site.
    MuteSite,
    UnmuteSite,
}

/// What the context menu of an extension's toolbar icon can do (0.16.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtMenuAction {
    /// krunk://extensions with this extension's details shown.
    Details,
    /// Its options page (offered only when it has one).
    Options,
    /// krunk://extensions.
    Manage,
    Disable,
    /// Removes it with its files and stored data. The menu asks "Remove <name>?" first.
    Uninstall,
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
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
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
        /// The active tab has a video (the Picture-in-Picture item; 0.6.0).
        #[serde(default)]
        has_video: bool,
        /// A newer Krunk is offered (0.7.2): "Update available – Krunk X.Y.Z" at the top.
        #[serde(default)]
        update: Option<String>,
        /// The account item (0.8.0): `None` hides it (private windows), `""` is "Sign in to
        /// Krunk", an address is "Signed in as …". Both open Settings → Account.
        #[serde(default)]
        account: Option<String>,
    },
    /// The Picture-in-Picture chooser (0.6.0): the active tab's videos.
    Pip { tab_id: u32, videos: Vec<VideoInfo> },
    /// The floating video's volume (0.7.0, R8): percent and mute of tab `tab_id`'s video.
    PipVolume { tab_id: u32, volume: u32, muted: bool },
    /// The install confirmation (0.7.0, R20). `request_id` is echoed by the answer.
    ExtInstall {
        request_id: u32,
        name: String,
        version: String,
        /// Where it comes from ("Chrome Web Store, signed by the store", "File krunk.zip", …).
        source: String,
        warnings: Vec<String>,
        /// It replaces an installed version (`previous`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<String>,
    },
    /// The VPN popup (0.14.0): the same view the `vpn` event carries.
    Vpn {
        #[serde(flatten)]
        view: crate::vpn::VpnView,
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
    /// The lock popup (see `certinfo`).
    SiteInfo { info: crate::certinfo::SiteInfo },
    /// Login suggestions for the focused field of frame `frame_id` in tab `tab_id`.
    /// `locked`: the vault has logins for the site but is locked (one "Unlock" entry).
    Suggest {
        tab_id: u32,
        frame_id: String,
        origin: String,
        locked: bool,
        entries: Vec<SuggestChoice>,
        /// A generated password to offer for a new-password field (0.6.0); the choice sends
        /// `SuggestGenerate` and the host fills what it generated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        generated: Option<String>,
    },
    /// Saved cards for a card field of frame `frame_id` (0.6.0; masked labels only).
    CardFill { tab_id: u32, frame_id: String, origin: String, locked: bool, cards: Vec<CardChoice> },
    /// "Save this card?" after a checkout (0.6.0; the brand and last four digits only).
    SaveCard { tab_id: u32, origin: String, brand: String, last4: String },
    /// "Save password for `username` on `origin`?" (no password is sent to the UI).
    SavePassword { tab_id: u32, origin: String, username: String, update: bool },
    /// The login was saved without asking (a generated password): Undo and Edit (0.6.0).
    PasswordSaved { tab_id: u32, entry_id: String, origin: String, username: String, update: bool },
    /// The address-bar suggestions for what the user typed; `selected` is the row the
    /// keyboard picked (the shell owns the selection).
    AddressSuggest { entries: Vec<AddressSuggestion>, selected: Option<usize> },
    /// The address bar's context menu: which items are enabled, and whether the clipboard
    /// holds an address ("Paste and go") or words ("Paste and search").
    AddressMenu {
        can_undo: bool,
        can_cut: bool,
        can_copy: bool,
        can_paste: bool,
        paste_is_url: bool,
        can_delete: bool,
        can_select_all: bool,
    },
    /// A page on `origin` asks for permissions (`kinds`; Krunk 0.6.0: camera, microphone,
    /// location, notifications, clipboard, MIDI, screen sharing, local network — `loopback`
    /// means "this computer" for the latter). In a private tab the answer is never remembered.
    Permission {
        #[serde(default)]
        kinds: Vec<crate::permissions::Kind>,
        tab_id: u32,
        prompt_id: u64,
        origin: String,
        loopback: bool,
        private: bool,
    },
    /// "Fill the login for `origin`?" with the matching entries.
    Fill { tab_id: u32, origin: String, entries: Vec<FillChoice> },
    /// "Unlock your passwords" before saving or filling on `origin`.
    Unlock {
        tab_id: u32,
        origin: String,
        purpose: UnlockPurpose,
        /// Last error (wrong password, too many attempts).
        error: Option<String>,
        /// Milliseconds before the next attempt is accepted.
        wait_ms: u64,
        /// "Remain unlocked this session" starts checked (the user's last choice, 0.11.0).
        #[serde(default = "yes")]
        remain: bool,
    },
    /// Contents of a bookmarks-bar folder. The bar's overflow ("»", 0.7.2) is the bar's own
    /// folder from item `offset` on (the items that did not fit).
    BookmarkFolder {
        id: i64,
        title: String,
        items: Vec<BarItem>,
        #[serde(default)]
        offset: u32,
        /// A subfolder opened in place (0.11.0): the title of the folder "‹ Back" returns to.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        back: Option<String>,
    },
    /// The window's tabs (0.11.0): a click switches, × closes (the list stays open and is
    /// updated in place).
    TabList {
        tabs: Vec<TabInfo>,
        #[serde(default)]
        active: Option<u32>,
    },
    /// The active tab's page stopped responding (0.11.0).
    Unresponsive {
        tab_id: u32,
        /// The page's host, for the text.
        host: String,
    },
    /// A page's JavaScript dialog (0.12.4, `core::js_dialog`): `title` is "<host> says" (or
    /// "Leave site?"), `message` the page's text (cleaned), `default_text` a prompt's input;
    /// `offer_block`: "Don't let this page show more dialogs".
    JsDialog {
        tab_id: u32,
        dialog_id: u32,
        dialog: crate::js_dialog::JsDialogKind,
        title: String,
        message: String,
        default_text: String,
        offer_block: bool,
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
        /// Not an update: the installed files are not this release's (0.7.0), why in words;
        /// "Update now" then installs this very version again.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repair: Option<String>,
    },
    /// The context menu of tab `tab_id`. `can_move`: the window has other tabs, so the tab
    /// can move to a new window of its own; `can_close_right`: there are tabs to its right.
    TabMenu {
        tab_id: u32,
        can_move: bool,
        can_close_right: bool,
        /// The tab's site is muted (the menu offers "Unmute site").
        #[serde(default)]
        muted_site: bool,
        /// The tab is on a site (the mute items apply).
        #[serde(default)]
        has_site: bool,
    },
    /// The context menu of extension `id`'s toolbar icon (0.16.0). The host fills it in from
    /// the installed extension: `has_options`: it has an options page; `enabled`: it is on
    /// (only then "Disable" applies).
    ExtMenu { id: String, name: String, has_options: bool, enabled: bool },
    /// The downloads button's popup: the most recent downloads first.
    Downloads { items: Vec<crate::downloads::DownloadItem> },
    /// Back's (`forward: false`) or Forward's history menu of tab `tab_id` (0.8.0).
    NavMenu { tab_id: u32, forward: bool, items: Vec<crate::navmenu::Item> },
    /// A site asks tab `tab_id` for HTTP authentication (0.8.0). `realm` is the server's text
    /// (shortened, no control characters); `insecure`: the credentials go over plain http.
    /// `logins`: the vault's logins for exactly this origin (user names only); `locked`: the
    /// vault is locked (it may have some); `failed`: the last answer was not accepted.
    HttpAuth {
        tab_id: u32,
        origin: String,
        realm: String,
        insecure: bool,
        logins: Vec<AuthLogin>,
        locked: bool,
        failed: bool,
    },
    /// 0.15.0: the hover card of tab `tab_id`: its title, its site (`host`, empty for Krunk's
    /// own pages: `url` then), and a JPEG `data:` URL of the page as it was last seen (none for
    /// the active tab, or before one was taken).
    TabPreview {
        tab_id: u32,
        title: String,
        host: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thumbnail: Option<String>,
        active: bool,
        private: bool,
    },
    /// Right-click on the window's title bar or the empty tab strip (Krunk 0.6.0).
    CaptionMenu { maximized: bool, can_reopen: bool, can_bookmark_all: bool },
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
    BookmarkEdit { id: Option<i64>, parent: i64, is_folder: bool, title: String, url: Option<String> },
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
    /// The ⋮ menu's "Update available" item (0.7.2): show the prompt.
    Show,
}

/// Inline autocomplete for the address bar (Krunk 0.6.0): `text` replaces the typed text
/// with its tail selected, `url` is where Enter goes, `index` the row it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineCompletion {
    pub text: String,
    pub url: String,
    pub index: usize,
}

/// What the address bar's context menu can do (Krunk 0.6.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddressMenuAction {
    Undo,
    Cut,
    Copy,
    Paste,
    /// "Paste and go" (an address) or "Paste and search" (words).
    PasteAndGo,
    Delete,
    SelectAll,
}

/// An edit the host asks the shell's address bar to make (the context menu's choice).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddressEdit {
    Undo,
    /// Remove the selection (Cut after the host copied it, or Delete).
    DeleteSelection,
    /// Insert the clipboard's text at the selection.
    Insert {
        text: String,
    },
    SelectAll,
    /// 0.26.0 (F-keys): Backspace typed after Ctrl+L before the shell had the keyboard.
    Backspace,
    /// 0.26.0 (F-keys): Enter typed after Ctrl+L before the shell had the keyboard: open what
    /// the address bar holds, as its own Enter does.
    Submit,
}

/// One row of the address-bar suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressSuggestion {
    /// Where the row goes (a search URL for `Search`).
    pub url: String,
    /// The page title, or the typed text for `Search`/`Url`.
    pub title: String,
    pub kind: AddressSuggestionKind,
    /// What the address bar shows while the row is selected with the keyboard.
    pub fill: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddressSuggestionKind {
    /// Search the typed words.
    Search,
    /// Go to the typed address.
    Url,
    History,
    Bookmark,
    /// A completion from the search engine's suggest endpoint (0.15.0): searches `title`.
    SearchSuggestion,
}

/// Why the vault must be unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnlockPurpose {
    Save,
    Fill,
    /// A card offered to save at checkout (0.6.0).
    SaveCard,
    /// Unlock chosen from the suggestion dropdown under a login field (Krunk 0.5.0).
    Suggest,
    /// Unlock chosen from the card dropdown under a payment field (0.6.0).
    CardFill,
    /// Settings → import passwords from another browser or a CSV file (Krunk 0.5.0).
    Import,
    /// A site's HTTP sign-in dialog (0.8.0).
    HttpAuth,
}

/// Which login field has the focus (the suggestion dropdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Password,
    Username,
    /// A payment-card field (`autocomplete=cc-*`): saved cards are offered (0.6.0).
    Card,
    /// A password field where a new password is chosen (`autocomplete=new-password`, a
    /// confirm field, a sign-up form): the generator is offered.
    NewPassword,
}

/// A field's rectangle in CSS px of its own frame's viewport (untrusted; clamped by the host).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl FieldRect {
    pub fn is_sane(&self) -> bool {
        [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite() && (-16384.0..=16384.0).contains(v))
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

/// A login in the suggestion dropdown (no password).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestChoice {
    pub id: String,
    pub username: String,
    /// The entry's host when it is a registrable-domain match (another host of the site).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
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
            PopupContent::SiteInfo { .. } => PopupKind::SiteInfo,
            PopupContent::Suggest { .. } => PopupKind::Suggest,
            PopupContent::SavePassword { .. } => PopupKind::SavePassword,
            PopupContent::CardFill { .. } => PopupKind::CardFill,
            PopupContent::SaveCard { .. } => PopupKind::SaveCard,
            PopupContent::PasswordSaved { .. } => PopupKind::PasswordSaved,
            PopupContent::Fill { .. } => PopupKind::Fill,
            PopupContent::Permission { .. } => PopupKind::Permission,
            PopupContent::AddressSuggest { .. } => PopupKind::AddressSuggest,
            PopupContent::AddressMenu { .. } => PopupKind::AddressMenu,
            PopupContent::BookmarkFolder { .. } => PopupKind::BookmarkFolder,
            PopupContent::Unlock { .. } => PopupKind::Unlock,
            PopupContent::Update { .. } => PopupKind::Update,
            PopupContent::TabMenu { .. } => PopupKind::TabMenu,
            PopupContent::ExtMenu { .. } => PopupKind::ExtMenu,
            PopupContent::NavMenu { .. } => PopupKind::NavMenu,
            PopupContent::HttpAuth { .. } => PopupKind::HttpAuth,
            PopupContent::TabPreview { .. } => PopupKind::TabPreview,
            PopupContent::CaptionMenu { .. } => PopupKind::CaptionMenu,
            PopupContent::Downloads { .. } => PopupKind::Downloads,
            PopupContent::Pip { .. } => PopupKind::Pip,
            PopupContent::PipVolume { .. } => PopupKind::PipVolume,
            PopupContent::ExtInstall { .. } => PopupKind::ExtInstall,
            PopupContent::BookmarkMenu { .. } => PopupKind::BookmarkMenu,
            PopupContent::BookmarkEdit { .. } => PopupKind::BookmarkEdit,
            PopupContent::TabList { .. } => PopupKind::TabList,
            PopupContent::Unresponsive { .. } => PopupKind::Unresponsive,
            PopupContent::JsDialog { .. } => PopupKind::JsDialog,
            PopupContent::Vpn { .. } => PopupKind::Vpn,
        }
    }

    /// The toolbar anchor that toggles this popup, for the reopen guard
    /// (`toggle::ToggleGuard`); `None` for popups without a toggle button.
    pub fn toggle_anchor(&self) -> Option<String> {
        match self {
            PopupContent::Menu { .. } => Some("menu".into()),
            PopupContent::Shield { .. } => Some("shield".into()),
            PopupContent::SiteInfo { .. } => Some("siteInfo".into()),
            PopupContent::Downloads { .. } => Some("downloads".into()),
            PopupContent::Pip { .. } => Some("pip".into()),
            PopupContent::TabList { .. } => Some("tabList".into()),
            PopupContent::Vpn { .. } => Some("vpn".into()),
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
    /// The Home button (0.16.0): the homepage, or the new tab page when none is set, in the
    /// active tab (what Alt+Home does) or, with `new_tab` (middle-click, Ctrl+click), in a new
    /// tab in front.
    Home {
        #[serde(default)]
        new_tab: bool,
    },
    /// Opens an http(s) or built-in URL (bookmarks bar, page links).
    OpenUrl {
        url: String,
        #[serde(default)]
        new_tab: bool,
    },
    OpenPage {
        page: String,
        /// A section of the page (`settings#privacy`); only short lower-case words count.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    // ---- the Krunk account (Settings → Account, 0.8.0) ----
    /// The card's state (answered with an `account` event). `check`: the card is on screen, so
    /// a signed-in device may ask the server who it is (the only request besides sign-in and
    /// sign-out).
    AccountState {
        #[serde(default)]
        check: bool,
    },
    AccountSignIn,
    AccountCancel,
    AccountSignOut,
    /// Opens the account page on the website in a new tab.
    AccountManage,
    /// Opens the website's "Add a device" section (a sign-in QR for a phone) in a new tab (0.13.0).
    AccountAddDevice,
    /// Opens the sign-in page with the code again.
    AccountReopen,
    // ---- sync (krunk://sync and Settings → Sync (beta), 0.9.0) ----
    /// The view (answered with a `sync` event). Never a network request by itself.
    SyncState,
    /// Settings: opens krunk://sync.
    SyncOpen,
    /// The user turns sync on (the first network request of sync): the account is asked
    /// whether it already syncs. `passwords`: include the password vault.
    SyncTurnOn {
        #[serde(default)]
        passwords: bool,
    },
    /// The first device: the user typed the recovery key's last group and ticked "I understand".
    SyncSetupConfirm {
        group: String,
        #[serde(default)]
        understood: bool,
    },
    /// The recovery key (and passphrase) to the private clipboard, or into a file.
    SyncSetupCopy,
    SyncSetupSave,
    /// Restore with the passphrase (`recovery`: false) or the recovery key; `vault`: the secret
    /// is the password-vault master password (0.21.1, the one secret the page asks for).
    SyncRestore {
        secret: Secret,
        #[serde(default)]
        recovery: bool,
        #[serde(default)]
        vault: bool,
    },
    /// The first device turns sync on with the vault password (0.21.1).
    SyncSetupVault {
        password: Secret,
    },
    /// The user confirmed the device list a restore found.
    SyncRestoreConfirm,
    /// The devices that keep access after a removal are confirmed (one was added by the removed
    /// device), so the keys can be rotated (0.9.0, S9).
    SyncConfirmRotation,
    /// "Finish securing": a new escrow anchor with the passphrase (`recovery`: false) or the
    /// recovery key.
    SyncFinishSecuring {
        secret: Secret,
        #[serde(default)]
        recovery: bool,
    },
    /// Ask a device that already syncs for the keys.
    SyncJoin {
        #[serde(default)]
        passwords: bool,
    },
    /// The 6 digits shown on the other device (one attempt).
    SyncJoinCode {
        code: String,
    },
    /// Back out of setup, restore or joining.
    SyncCancel,
    /// Answer a device that asks to join; `share_passwords` hands it the passwords key too.
    SyncApprove {
        id: String,
        #[serde(default)]
        share_passwords: bool,
    },
    SyncApproveCancel,
    /// Remove another device (it is signed out; the keys are rotated).
    SyncRemoveDevice {
        id: String,
    },
    /// Turn sync off on this device; `delete_data` also removes what sync brought here.
    SyncTurnOff {
        #[serde(default)]
        delete_data: bool,
    },
    SyncNow,
    SyncSetCollection {
        name: String,
        on: bool,
    },
    /// Rename a device (an encrypted `device_meta` record).
    SyncRenameDevice {
        id: String,
        name: String,
    },
    /// Settings → Passwords & import: the browser profiles found on this computer.
    ImportDetect,
    /// Import `types` ("history", "bookmarks", "passwords", "cards") from the profile `profile`
    /// (an id from `ImportDetect`), with the Firefox primary password if one is needed.
    ImportRun {
        profile: String,
        types: Vec<String>,
        #[serde(default)]
        primary_password: Option<Secret>,
    },
    /// Import passwords from a CSV file chosen in a native dialog.
    ImportCsv,
    /// krunk://gpu: the switches, the GPU process, the acceleration setting.
    GetGpuInfo,
    /// krunk://gpu: open Chromium's own chrome://gpu page in this tab (a host navigation).
    OpenGpuInternals,
    /// krunk://gpu and About (0.7.0): reinstall this version because the installed files are
    /// not this release's (the host decides; the page only asks).
    RepairEngine,
    ToggleBookmark,
    /// The shell's tab-row layout for hit testing the frameless window.
    HitMap {
        map: crate::frame::HitMap,
    },
    /// The shell's `devicePixelRatio` (0.15.4): the scale the engine really draws Krunk's UI
    /// at. It is the window's DPI scale times Windows' "Text size", or whatever else changed
    /// the engine's scale; the host lays out with it. Sent when the shell loads, when the
    /// value changes and when the host announces a new window state.
    ShellScale {
        dpr: f64,
    },
    /// Minimize, maximize/restore or close the window (keyboard use of the
    /// caption buttons; mouse clicks on them are handled natively).
    WindowControl {
        action: WindowAction,
    },
    /// A tab was dragged out of its strip (the pointer left the tab row): the host follows
    /// the mouse from here, in a new window. `offset_x`/`offset_y`: where the tab was grabbed
    /// (CSS px from the tab's left edge, and from the top of the shell). `first_x`: where a
    /// window's first tab starts when its row does not scroll (0.11.1: the grab point in the
    /// new window; the host guesses without it).
    TabDragOut {
        id: u32,
        offset_x: f64,
        offset_y: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_x: Option<f64>,
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
    /// 0.15.0: the pointer rests on tab `id` (the shell waited its delay): its hover card under
    /// `rect` (the tab, shell CSS px). `id` none: the card goes (pointer left, click, drag,
    /// scroll).
    TabHover {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        rect: Option<PopupRect>,
    },
    /// 0.25.0: the pointer has rested on tab `id` for a moment (80 ms): its page may be woken
    /// beneath the one on screen, so a click shows it at once. `id` none: the pointer left.
    TabWarm {
        #[serde(default)]
        id: Option<u32>,
    },
    /// A choice in a tab's context menu (popup).
    TabMenuAction {
        id: u32,
        action: TabAction,
    },
    /// Right-click on Back (`forward: false`) or Forward: its history menu at `rect` (shell CSS
    /// px, under the button; 0.8.0).
    ShowNavMenu {
        forward: bool,
        rect: PopupRect,
    },
    /// A choice in a history menu (popup): jump tab `tab_id` to navigation entry `entry_id`.
    NavToEntry {
        tab_id: u32,
        entry_id: i64,
    },
    /// Middle-click in a history menu (popup, 0.18.0): open that entry in a new background tab
    /// next to tab `tab_id`, with a copy of the tab's back/forward list; `tab_id` stays where it is.
    NavToEntryNewTab {
        tab_id: u32,
        entry_id: i64,
    },
    /// The HTTP sign-in dialog's answer (popup, 0.8.0): typed credentials, or a saved login
    /// (`entry_id`; the host then takes its user name and password from the vault).
    HttpAuthReply {
        tab_id: u32,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: Secret,
        #[serde(default)]
        entry_id: Option<String>,
    },
    HttpAuthCancel {
        tab_id: u32,
    },
    /// "Unlock to use saved passwords" in the HTTP sign-in dialog.
    HttpAuthUnlock {
        tab_id: u32,
    },
    /// Opens the popup view at `rect` (shell: menu and shield only).
    ShowPopup {
        popup: PopupKind,
        rect: PopupRect,
    },
    HidePopup,
    /// The popup's content needs another height (CSS px): the site-info popup grows when
    /// "Certificate details" opens. The host clamps it to the work area.
    PopupResize {
        height: f64,
    },
    /// The popup has rendered the content numbered `seq` (and painted it twice): the host
    /// puts its window in place now, `height` (CSS px) fitted for content that asks for it.
    /// Until then a popup window opens parked off-screen, so it never shows the previous
    /// content (0.11.0, a folder dropdown showed the last folder for a frame).
    PopupShown {
        seq: u64,
        #[serde(default)]
        height: Option<f64>,
    },
    /// "‹ Back" in a folder dropdown: the folder that was open before this subfolder.
    BookmarkFolderBack,
    /// The answer to "This page isn't responding" (0.11.0): wait, or end the page.
    HangReply {
        tab_id: u32,
        wait: bool,
    },
    /// The answer to a page's dialog (0.12.4): OK / Leave (`accept`), a prompt's `text`, and
    /// "Don't let this page show more dialogs" (`block`).
    JsDialogReply {
        dialog_id: u32,
        accept: bool,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        block: bool,
    },
    /// Middle-click on a bookmark of the bar or of a folder dropdown: it opens in a new tab
    /// behind the current one, and a dropdown stays open (0.11.0).
    OpenBookmarkBackground {
        id: i64,
    },
    /// Opens a bookmarks-bar folder in the popup. The shell passes the
    /// rect under the folder button; the popup passes `None` to show a
    /// subfolder in place.
    ShowBookmarkFolder {
        id: i64,
        #[serde(default)]
        rect: Option<PopupRect>,
        /// The bar's overflow (0.7.2, R10): the shell opens the bar's folder from the first
        /// item that did not fit. Only the shell sends it.
        #[serde(default)]
        from: Option<u32>,
    },
    /// Right-click on the bookmarks bar: its menu for bookmark or folder `id`, or for the bar
    /// itself (`id` none), at `rect` (shell CSS px).
    ShowBookmarkMenu {
        #[serde(default)]
        id: Option<i64>,
        rect: PopupRect,
    },
    /// Right-click on an item of an open folder dropdown (popup): the same menu as on
    /// the bar, at `(x, y)` in CSS px of the popup's viewport.
    ShowFolderItemMenu {
        id: i64,
        x: f64,
        y: f64,
    },
    /// Dragging inside an open folder dropdown (popup): item `id` of that folder goes to
    /// `index` of `parent` (the folder itself or one of its subfolders).
    MoveBookmarkInFolder {
        id: i64,
        parent: i64,
        index: u32,
    },
    /// An item was dragged out of the open folder dropdown (popup): the host follows the
    /// mouse and lets the bar take the drop.
    FolderItemDragOut {
        id: i64,
    },
    /// The active page, dragged from the address bar onto the bar (shell): bookmarked
    /// at `index` of `parent` with the tab's own title and address.
    BookmarkActivePage {
        parent: i64,
        index: u32,
    },
    /// An address dropped onto the bar (shell), e.g. selected text of the address bar.
    AddBookmarkAt {
        url: String,
        parent: i64,
        index: u32,
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
    /// The shell: suggestions for `text` typed into the address bar, shown at `rect` (the
    /// popup's place and width in shell CSS px; the host sets the height).
    AddressSuggest {
        text: String,
        rect: PopupRect,
    },
    /// The shell: right-click in the address bar at (`x`, `y`) (shell CSS px); `selection`
    /// is the selected text, `has_text` whether the bar is non-empty, `dirty` whether it
    /// was edited since it was focused (Undo).
    ShowAddressMenu {
        x: f64,
        y: f64,
        selection: String,
        has_text: bool,
        dirty: bool,
    },
    /// The popup: a choice in the address bar's context menu.
    AddressMenuAction {
        action: AddressMenuAction,
    },
    /// The shell: the speaker icon on a tab was clicked (mute or unmute that tab).
    TabMute {
        id: u32,
        muted: bool,
    },
    /// The popup (caption menu): bookmark every tab of the window into a new folder.
    BookmarkAllTabs,
    /// The popup (caption menu): minimize, maximize/restore or close the window.
    CaptionWindow {
        action: WindowAction,
    },
    /// The settings page: every stored per-site permission.
    PermissionExceptions,
    /// Settings page (0.6.0 tracking protection): the bounce-tracker bookkeeping.
    BounceStatus,
    /// Clear the due bounce trackers now (replies `{cleaned}`).
    BounceCleanNow,
    /// Settings page (0.6.0 custom DNS): the stub's state and per-resolver counters.
    DnsStatus,
    /// Resolve a name through resolver `index` alone (replies `{ms}` or an error).
    DnsTest {
        index: u32,
    },
    /// Turn what the user typed (an address or an `sdns://` stamp) into a resolver.
    DnsParse {
        text: String,
    },
    /// The settings page: set (or the lock popup: change) a site's permission; `Ask` removes
    /// the stored decision.
    PermissionSet {
        origin: String,
        kind: crate::permissions::Kind,
        state: crate::permissions::PermissionState,
    },
    /// The task manager page: the current rows.
    GetTasks,
    /// The task manager page: end the process behind task `id` (never the browser).
    KillTask {
        id: i64,
    },
    /// krunk://sessions (0.15.0): the sessions with the logging settings.
    SessionList,
    /// krunk://sessions: one session in full.
    SessionDetail {
        id: i64,
    },
    /// krunk://sessions: delete one past session (not the running one).
    SessionDelete {
        id: i64,
    },
    /// krunk://sessions: delete every past session and the running one's events.
    SessionClear,
    /// krunk://sessions: turn logging on or off, or change how long sessions are kept.
    SessionLogSet {
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        retention_days: Option<u32>,
    },
    /// The shell moved the keyboard selection in the suggestions.
    AddressSuggestSelect {
        selected: Option<usize>,
    },
    /// A suggestion was chosen (a click in the popup, Enter in the shell): go there.
    AddressSuggestPick {
        url: String,
    },
    /// The answer to a local-network permission prompt (`remember`: keep it for the site).
    PermissionReply {
        prompt_id: u64,
        allow: bool,
        remember: bool,
    },
    /// krunk://sitedata: forget a stored permission of the page's site.
    SitePermissionRemove {
        kind: String,
    },
    SavePasswordReply {
        tab_id: u32,
        origin: String,
        accept: bool,
        #[serde(default)]
        never: bool,
        /// The username as the prompt shows it, possibly edited (0.11.2); none from older popups.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
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
    /// The new-tab page's options (search engine name, shortcuts, the page's settings).
    GetNewTabConfig,
    /// The new-tab page's customize panel saved its options.
    SetNtpOptions {
        ntp: crate::settings::Ntp,
    },
    /// The new-tab page: pick a background image (the host opens the file dialog).
    NtpPickImage,
    /// The new-tab page: the tiles' new order (pinned URLs, left to right).
    ReorderTiles {
        urls: Vec<String>,
    },
    /// The new-tab page: a tile added by hand (pinned, no address check beyond parsing).
    AddTile {
        url: String,
        title: String,
    },
    /// The new-tab page: a tile's title and address changed.
    EditTile {
        url: String,
        new_url: String,
        title: String,
    },
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
        /// 0.23.0: the device filter (`"this"`, `"unknown"`, a sync device id; empty or
        /// `"all"` = every device).
        #[serde(default)]
        device: String,
    },
    DeleteHistory {
        ids: Vec<i64>,
    },
    DeleteHistoryRange {
        from: i64,
        to: i64,
        /// 0.23.0: only this device filter's visits (see `QueryHistory`); empty = all, synced.
        #[serde(default)]
        device: String,
    },
    /// 0.23.0: the devices with visits here (the history page's filter chips).
    HistoryDevices,
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
    /// Settings page (0.12.1): is Krunk the default browser? Replies `{installed, isDefault}`
    /// (`isDefault` null when Windows can't tell).
    DefaultBrowserStatus,
    /// Settings page: registers Krunk as a browser and opens Windows Settings at its page of
    /// Default apps (replies `{opened}` or an error).
    MakeDefaultBrowser,
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
        /// "Remain unlocked this session" (0.11.0); absent: the remembered choice.
        #[serde(default)]
        remain: Option<bool>,
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
    // ---- payment cards (0.6.0) ----
    CardList,
    CardSave {
        card: CardInput,
    },
    CardDelete {
        id: String,
    },
    /// The full number and address; requires the master password again.
    CardReveal {
        id: String,
        password: Secret,
    },
    /// The number to the clipboard (cleared after 30 s).
    CardCopy {
        id: String,
    },
    /// A card chosen in the fill dropdown of frame `frame_id`.
    CardFillPick {
        tab_id: u32,
        frame_id: String,
        card_id: String,
    },
    /// The answer to "Save this card?".
    SaveCardReply {
        tab_id: u32,
        origin: String,
        accept: bool,
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
    /// The recovery phrase (Krunk 0.7.0, R13; the passwords page only). Create (or replace)
    /// one after the master password: the host makes the words and keeps them until confirmed
    /// (the reply has them and three word positions to type back).
    VaultRecoveryCreate {
        password: Secret,
    },
    /// The words typed back for the positions asked: the phrase becomes the vault's.
    VaultRecoveryConfirm {
        answers: Vec<RecoveryAnswer>,
    },
    /// The new phrase is not wanted (dialog closed): it is forgotten.
    VaultRecoveryCancel,
    /// The new phrase to the clipboard (cleared after a minute) or into a text file the user
    /// picks.
    VaultRecoveryCopy,
    VaultRecoverySave,
    /// Removes the recovery phrase (after the master password).
    VaultRecoveryRemove {
        password: Secret,
    },
    /// Is this a recovery phrase (word list and checksum only; nothing is opened)?
    VaultRecoveryCheck {
        phrase: Secret,
    },
    /// Forgotten master password: open the vault with the phrase and set `new`.
    VaultRecover {
        phrase: Secret,
        new: Secret,
    },
    GeneratePassword {
        #[serde(default = "default_pw_len")]
        length: u32,
        #[serde(default = "yes")]
        symbols: bool,
    },

    // ---- downloads ----
    /// The newest downloads (at most 200; the page uses `QueryDownloads`).
    GetDownloads,
    /// 0.15.0: a page of the list, newest first (`crate::downloads::DownloadPage`): by search
    /// text (name and address) and filter; `counts` also counts (per filter, and how many
    /// started at or after each time in `since`: the page's local midnights).
    QueryDownloads {
        #[serde(default)]
        text: String,
        #[serde(default)]
        filter: crate::downloads::DownloadFilter,
        #[serde(default)]
        offset: u32,
        #[serde(default = "default_limit")]
        limit: u32,
        #[serde(default)]
        counts: bool,
        #[serde(default)]
        since: Vec<i64>,
    },
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

    // ---- a page's source (krunk://source, 0.11.0): the address in the tab's own URL ----
    SourceGet {},

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

    // ---- the WASM memory inspector (krunk://wasm, 0.19.0; bound by the host to the tab it
    // was opened from; requests name no tab) ----
    /// The inspected tab (title, address, whether it is still open) and the hook's state.
    WasmInfo {},
    /// A request to the probe in one of the tab's contexts (`ctx` 0: the page itself; the
    /// others as `list` names them: same-process frames and dedicated workers).
    WasmCall {
        #[serde(default)]
        ctx: u32,
        req: crate::wasm::WasmReq,
    },
    /// The fallback: hook `WebAssembly` at document start (or stop doing so) and reload the tab.
    WasmHook {
        on: bool,
    },
    /// From the ⋮ menu: open the inspector for the active tab.
    OpenWasm,
    /// The suggestion dropdown (popup): fill entry `entry_id` into frame `frame_id` of tab
    /// `tab_id`. The host re-checks the tab, the frame and the entry's origin.
    SuggestPick {
        tab_id: u32,
        frame_id: String,
        entry_id: String,
    },
    /// An extension page's `chrome.*` call (0.6.0): the extension, the operation and its
    /// arguments; the host answers with a reply or later with `respond`.
    Ext {
        ext: String,
        op: String,
        #[serde(default)]
        data: serde_json::Value,
    },
    /// The shell: an extension's toolbar button was clicked (its rect in shell CSS px).
    ExtAction {
        id: String,
        rect: PopupRect,
    },
    /// The shell: right-click on an extension's toolbar button: its context menu at `rect`
    /// (the click, shell CSS px; 0.16.0).
    ShowExtMenu {
        id: String,
        rect: PopupRect,
    },
    /// A choice in that menu (popup). Apart from krunk://extensions' own requests below this is
    /// the only way to disable or remove an extension; the host acts only for the extension
    /// whose menu it has open.
    ExtMenuAction {
        id: String,
        action: ExtMenuAction,
    },
    /// krunk://extensions (0.6.0).
    ExtList,
    ExtInstallFile,
    ExtInstallFolder,
    ExtInstallUrl {
        url: String,
    },
    ExtSet {
        id: String,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        private: Option<bool>,
    },
    ExtRemove {
        id: String,
    },
    ExtOpenOptions {
        id: String,
    },
    /// The Picture-in-Picture chooser (popup): video `index` of tab `tab_id` (0.6.0).
    PipPick {
        tab_id: u32,
        index: u32,
    },
    /// The install confirmation's answer (popup, 0.7.0).
    ExtInstallReply {
        request_id: u32,
        accept: bool,
    },
    /// The floating video's volume bar (popup, 0.7.0): `volume` percent (0-100) and mute for
    /// the video of tab `tab_id` that is in Picture-in-Picture.
    PipVolume {
        tab_id: u32,
        volume: u32,
        muted: bool,
    },
    /// The suggestion dropdown (popup): "Unlock Krunk passwords" for the focused field.
    SuggestUnlock {
        tab_id: u32,
        frame_id: String,
    },
    /// "Use a strong password" in the dropdown (0.6.0): fill the generated password into the
    /// new-password fields of that frame.
    SuggestGenerate {
        tab_id: u32,
        frame_id: String,
    },
    /// The toast after an automatic save (0.6.0, a popup): undo it (delete the new entry or
    /// restore the old password) or open the entry on the passwords page.
    PasswordSavedUndo {
        entry_id: String,
    },
    PasswordSavedEdit {
        entry_id: String,
    },

    // ---- about / https-only ----
    GetAbout,
    /// From the HTTPS-only interstitial: load this http:// URL once.
    HttpsOnlyProceed {
        url: String,
    },
    /// From the certificate interstitial (`krunk://cert-error`): "Continue anyway" to `url`
    /// past the certificate `sha256`, remembered for the site when `remember`. The host
    /// checks both against the error the tab really hit.
    CertProceed {
        url: String,
        sha256: String,
        remember: bool,
    },
    /// "Back to safety" on the interstitial: the page before the refused address.
    CertBack {},
    /// Settings: the remembered certificate exceptions, newest first.
    CertExceptions,
    /// Settings: forget one (it takes effect at once).
    CertExceptionRemove {
        host: String,
        port: u16,
        sha256: String,
    },

    // ---- 0.12.0: the AI agent, userscripts and macros ----
    /// The shell's agent button (also Ctrl+Shift+Space): show or hide the window's panel.
    ToggleAgent,
    /// The shell's macro indicator: stop the running macro.
    StopMacros,
    /// The agent panel: a new task (or a follow-up in the same conversation).
    AgentSend {
        text: String,
        /// 0.25.2: attached files (checked in `agent::attach`).
        #[serde(default)]
        files: Vec<AgentFile>,
    },
    AgentStop,
    /// "New conversation".
    AgentNew,
    /// 0.16.0: Show on the card of a password Krunk generated for the agent (`agentPassword`):
    /// the reply carries it, for this window's panel only.
    AgentPasswordShow {
        id: u32,
    },
    /// 0.16.0: Copy on that card: the password goes to the clipboard like one copied on the
    /// passwords page (kept out of the clipboard history, cleared after 30 s).
    AgentPasswordCopy {
        id: u32,
    },
    /// 0.16.0: Clear in the panel's scratchpad view: the agent's notes and plan of this
    /// conversation are dropped.
    AgentScratchClear,
    AgentAnswer {
        ask: u32,
        #[serde(default)]
        choices: Vec<u8>,
        #[serde(default)]
        other: Option<String>,
    },
    /// Allow or deny a sensitive category (history, bookmarks, downloads, cookies; 0.16.0:
    /// entering a password on the site the question named).
    AgentConsent {
        ask: u32,
        allow: bool,
    },
    /// The step limit was reached: continue for another round, or end.
    AgentContinue {
        ask: u32,
        go: bool,
    },
    /// The panel's quick switch.
    AgentSelectModel {
        provider: String,
        model: String,
    },
    /// The panel asks for its configuration (model indicator, quick-switch list).
    AgentModels,
    /// The panel's close button.
    AgentClose,
    AgentOpenScript {
        id: String,
        kind: ScriptKind,
    },
    AgentToggleScript {
        id: String,
        kind: ScriptKind,
        enabled: bool,
    },
    /// The macro runner: one API call of the running macro (`op` e.g. "key.down").
    MacroCall {
        op: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// The macro runner: the macro ended.
    MacroDone {
        ok: bool,
        #[serde(default)]
        error: Option<String>,
    },
    /// Settings → AI: add or change a provider (never its key).
    AiProviderSave {
        provider: AiProvider,
    },
    AiProviderRemove {
        id: String,
    },
    /// Settings → AI: store a provider's key (DPAPI). Never echoed or sent back.
    AiKeySet {
        id: String,
        key: Secret,
    },
    AiKeyRemove {
        id: String,
    },
    /// Settings → AI: check the key and the address with the provider's model list.
    AiTest {
        id: String,
    },
    AiModels {
        id: String,
        #[serde(default)]
        refresh: bool,
    },
    /// Which providers have a key stored.
    AiStatus,
    // ---- the VPN (0.14.0; Settings, and the popup's connect and disconnect) ----
    /// The card's state (answered with the view; the `vpn` event follows each change).
    VpnState,
    /// The host opens the file dialog (`*.conf;*.ovpn`), reads the file itself and answers
    /// with the new profile and its warnings, or an error. Nothing of the file comes from the page.
    VpnImport {},
    VpnRename {
        id: String,
        name: String,
    },
    VpnRemove {
        id: String,
    },
    /// An OpenVPN profile's user name and password (DPAPI-protected with the profile; never
    /// sent back).
    VpnCredentials {
        id: String,
        username: String,
        password: Secret,
    },
    VpnConnect {
        id: String,
    },
    VpnDisconnect {},
    /// 0.22.0: which tabs use the VPN (`vpn.scope`), from the popup's selector; applied at once.
    VpnScope {
        scope: crate::settings::VpnScope,
    },
    /// krunk://scripts (0.12.0).
    ScriptsList,
    ScriptGet {
        id: String,
    },
    /// Create (no id) or update a userscript from its source.
    ScriptSave {
        #[serde(default)]
        id: Option<String>,
        source: String,
    },
    ScriptToggle {
        id: String,
        enabled: bool,
    },
    ScriptDelete {
        id: String,
    },
    ScriptExport {
        id: String,
    },
    /// Run a userscript once on the window's most recent web tab.
    ScriptRunNow {
        id: String,
    },
    /// Parse check of a userscript or macro (the engine compiles it; nothing runs).
    ScriptCheck {
        source: String,
        kind: ScriptKind,
    },
    MacroSave {
        #[serde(default)]
        id: Option<String>,
        name: String,
        source: String,
        #[serde(default)]
        hotkey: Option<String>,
    },
    MacroDelete {
        id: String,
    },
    MacroToggle {
        id: String,
        enabled: bool,
    },
    MacroRun {
        id: String,
    },
    MacroStop,
    /// Is this chord free as a macro hotkey?
    HotkeyCheck {
        chord: String,
        #[serde(default)]
        id: Option<String>,
    },
    /// While on, the page's key presses with modifiers come back as `hotkeyRecorded` instead
    /// of running Krunk's shortcuts.
    HotkeyRecord {
        on: bool,
        /// The macro being edited: its own hotkey is no conflict.
        #[serde(default)]
        id: Option<String>,
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
    /// An extension's own page on `https://<id>.ext.krunk.invalid` (0.6.0): the background
    /// page, the action popup or the options page. Only `Request::Ext` is permitted; the host
    /// checks the id against the page's origin.
    Ext,
    /// A window's AI agent panel (0.12.0, `shell.krunk.invalid/agent.html`).
    Agent,
    /// The hidden view that runs macros (0.12.0, `shell.krunk.invalid/runner.html`).
    Runner,
}

/// Which browser view a message arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Shell,
    Popup,
    Tab,
    /// An extension's background or popup view (0.6.0).
    Ext,
    /// The agent panel view (0.12.0).
    Agent,
    /// The macro runner view (0.12.0).
    Runner,
}

/// Classification of a message source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// An http(s) sub-frame: only the login-field messages are accepted from it.
    WebFrame { origin: String },
    /// Our own UI: parse with [`parse_request`].
    Privileged(Sender),
    /// An ordinary web page on this origin: parse with [`parse_content`] only.
    Web { origin: String },
}

pub const SHELL_PATH: &str = "/shell.html";
pub const POPUP_PATH: &str = "/popup.html";
pub const AGENT_PATH: &str = "/agent.html";
pub const RUNNER_PATH: &str = "/runner.html";

/// Decides how a message may be treated, from the channel it came in on,
/// the `Source` URI and whether it came from the top-level document.
///
/// * The shell and popup views accept only their exact document on the
///   exact shell origin; anything else on those channels is rejected.
/// * A tab accepts privileged messages only from a built-in page on the
///   exact pages origin; other http(s) pages are web content.
/// * Messages from sub-frames are always rejected.
pub fn classify(channel: Channel, source: &str, top_level: bool) -> Result<Source, ProtocolError> {
    let url = url::Url::parse(source).map_err(|_| ProtocolError::Forbidden)?;
    let origin = url.origin().ascii_serialization();
    if !top_level {
        // A sub-frame is never privileged; an http(s) one may report its login fields.
        let own = origin == pages::SHELL_ORIGIN || origin == pages::PAGES_ORIGIN;
        return match (channel, url.scheme()) {
            (Channel::Tab, "http" | "https") if !own => Ok(Source::WebFrame { origin }),
            _ => Err(ProtocolError::Forbidden),
        };
    }
    match channel {
        Channel::Ext => {
            if crate::ext::id_of_url(source).is_some() {
                Ok(Source::Privileged(Sender::Ext))
            } else {
                Err(ProtocolError::Forbidden)
            }
        }
        Channel::Shell | Channel::Popup | Channel::Agent | Channel::Runner => {
            let (want, sender) = match channel {
                Channel::Shell => (SHELL_PATH, Sender::Shell),
                Channel::Popup => (POPUP_PATH, Sender::Popup),
                Channel::Agent => (AGENT_PATH, Sender::Agent),
                _ => (RUNNER_PATH, Sender::Runner),
            };
            if origin == pages::SHELL_ORIGIN && url.path() == want {
                Ok(Source::Privileged(sender))
            } else {
                Err(ProtocolError::Forbidden)
            }
        }
        Channel::Tab => {
            if let Some(page) = Page::from_url(source) {
                return Ok(Source::Privileged(Sender::Page(page)));
            }
            if crate::ext::id_of_url(source).is_some() {
                return Ok(Source::Privileged(Sender::Ext));
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
            // The panel opens links of the agent's answers (http(s), checked by the host); the
            // runner opens tabs only through its own API.
            return match self {
                Sender::Page(Page::SiteData | Page::Wasm) | Sender::Runner => false,
                Sender::Agent => matches!(req, R::OpenUrl { .. }),
                _ => true,
            };
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
                    | R::TabHover { .. }
                    | R::TabWarm { .. }
                    | R::ShowNavMenu { .. }
                    | R::DevToolsDock { .. }
                    | R::DevToolsClose
                    | R::ReopenClosedTab
                    | R::Navigate { .. }
                    | R::Back
                    | R::Forward
                    | R::Reload
                    | R::Stop
                    | R::Home { .. }
                    | R::ToggleBookmark
                    | R::HitMap { .. }
                    | R::ShellScale { .. }
                    | R::WindowControl { .. }
                    | R::ShowPopup { .. }
                    | R::ExtAction { .. }
                    | R::ShowExtMenu { .. }
                    | R::AddressSuggest { .. }
                    | R::AddressSuggestSelect { .. }
                    | R::AddressSuggestPick { .. }
                    | R::ShowAddressMenu { .. }
                    | R::TabMute { .. }
                    | R::HidePopup
                    | R::ShowBookmarkFolder { .. }
                    | R::OpenBookmarkBackground { .. }
                    | R::ShowBookmarkMenu { .. }
                    | R::MoveBookmark { .. }
                    | R::BookmarkActivePage { .. }
                    | R::AddBookmarkAt { .. }
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
                    | R::ToggleAgent
                    | R::StopMacros
            ),
            Sender::Popup => matches!(
                req,
                R::Command { .. }
                    | R::ActivateTab { .. }
                    | R::CloseTab { .. }
                    | R::TabMenuAction { .. }
                    | R::ExtMenuAction { .. }
                    | R::NavToEntry { .. }
                    | R::NavToEntryNewTab { .. }
                    | R::HttpAuthReply { .. }
                    | R::HttpAuthCancel { .. }
                    | R::HttpAuthUnlock { .. }
                    | R::BookmarkMenuAction { .. }
                    | R::SaveBookmark { .. }
                    | R::HidePopup
                    | R::ShowBookmarkFolder { rect: None, from: None, .. }
                    | R::BookmarkFolderBack
                    | R::HangReply { .. }
                    | R::JsDialogReply { .. }
                    | R::OpenBookmarkBackground { .. }
                    | R::PopupResize { .. }
                    | R::PopupShown { .. }
                    | R::ShowFolderItemMenu { .. }
                    | R::MoveBookmarkInFolder { .. }
                    | R::FolderItemDragOut { .. }
                    | R::NewTab { .. }
                    | R::ReopenClosedTab
                    | R::Zoom { .. }
                    | R::ShieldAllowSite { .. }
                    | R::OpenSiteData
                    | R::OpenWasm
                    | R::SavePasswordReply { .. }
                    | R::PasswordSavedUndo { .. }
                    | R::PasswordSavedEdit { .. }
                    | R::PermissionReply { .. }
                    | R::AddressSuggestPick { .. }
                    | R::AddressMenuAction { .. }
                    | R::PermissionSet { .. }
                    | R::BookmarkAllTabs
                    | R::CaptionWindow { .. }
                    | R::DownloadAction { .. }
                    | R::FillReply { .. }
                    | R::SuggestPick { .. }
                    | R::PipPick { .. }
                    | R::PipVolume { .. }
                    | R::ExtInstallReply { .. }
                    | R::SuggestUnlock { .. }
                    | R::SuggestGenerate { .. }
                    | R::CardFillPick { .. }
                    | R::SaveCardReply { .. }
                    | R::VaultStatus
                    | R::VaultUnlock { .. }
                    | R::UpdateReply { .. }
                    | R::OpenBlockedPage {}
                    | R::Quit
                    | R::VpnConnect { .. }
                    | R::VpnDisconnect {}
                    | R::VpnScope { .. }
            ),
            Sender::Page(Page::NewTab) => matches!(
                req,
                R::GetTopSites
                    | R::GetNewTabConfig
                    | R::SetNtpOptions { .. }
                    | R::NtpPickImage
                    | R::ReorderTiles { .. }
                    | R::AddTile { .. }
                    | R::EditTile { .. }
                    | R::PinSite { .. }
                    | R::UnpinSite { .. }
                    | R::HideSite { .. }
                    | R::Navigate { .. }
            ),
            Sender::Page(Page::History) => matches!(
                req,
                R::QueryHistory { .. }
                    | R::DeleteHistory { .. }
                    | R::DeleteHistoryRange { .. }
                    | R::HistoryDevices
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
                    | R::CardList
                    | R::CardSave { .. }
                    | R::CardDelete { .. }
                    | R::CardReveal { .. }
                    | R::CardCopy { .. }
                    | R::VaultChangePassword { .. }
                    | R::VaultRecoveryCreate { .. }
                    | R::VaultRecoveryConfirm { .. }
                    | R::VaultRecoveryCancel
                    | R::VaultRecoveryCopy
                    | R::VaultRecoverySave
                    | R::VaultRecoveryRemove { .. }
                    | R::VaultRecoveryCheck { .. }
                    | R::VaultRecover { .. }
                    | R::GetNeverSave
                    | R::NeverSaveRemove { .. }
                    | R::GeneratePassword { .. }
            ),
            Sender::Page(Page::Settings) => matches!(
                req,
                // 0.7.0 (R2): Appearance has the new tab page's background too.
                R::NtpPickImage
                    | R::PermissionExceptions
                    | R::PermissionSet { .. }
                    | R::BounceStatus
                    | R::BounceCleanNow
                    | R::DnsStatus
                    | R::DnsTest { .. }
                    | R::DnsParse { .. }
                    | R::GetSettings
                    | R::SaveSettings { .. }
                    | R::ResetSettings
                    | R::PickDownloadFolder
                    | R::DefaultBrowserStatus
                    | R::MakeDefaultBrowser
                    | R::UpdateFilterLists
                    | R::GetFilterStatus
                    | R::GetAllowlist
                    | R::AllowlistAdd { .. }
                    | R::AllowlistRemove { .. }
                    | R::CertExceptions
                    | R::CertExceptionRemove { .. }
                    | R::Restart
                    | R::ClearBrowsingData { .. }
                    | R::VaultStatus
                    | R::VaultChangePassword { .. }
                    | R::GetAbout
                    | R::GetUpdateStatus
                    | R::CheckForUpdates
                    | R::UpdateReply { .. }
                    | R::ImportDetect
                    | R::ImportRun { .. }
                    | R::ImportCsv
                    | R::AccountState { .. }
                    | R::AccountSignIn
                    | R::AccountCancel
                    | R::AccountSignOut
                    | R::AccountManage
                    | R::AccountAddDevice
                    | R::AccountReopen
                    | R::SyncState
                    | R::SyncOpen
                    | R::SyncNow
                    | R::SyncSetCollection { .. }
                    | R::AiProviderSave { .. }
                    | R::AiProviderRemove { .. }
                    | R::AiKeySet { .. }
                    | R::AiKeyRemove { .. }
                    | R::AiTest { .. }
                    | R::AiModels { .. }
                    | R::AiStatus
                    | R::VpnState
                    | R::VpnImport {}
                    | R::VpnRename { .. }
                    | R::VpnRemove { .. }
                    | R::VpnCredentials { .. }
                    | R::VpnConnect { .. }
                    | R::VpnDisconnect {}
                    | R::VpnScope { .. }
            ),
            Sender::Page(Page::Sync) => matches!(
                req,
                R::SyncState
                    | R::SyncTurnOn { .. }
                    | R::SyncSetupConfirm { .. }
                    | R::SyncSetupCopy
                    | R::SyncSetupSave
                    | R::SyncRestore { .. }
                    | R::SyncSetupVault { .. }
                    | R::SyncRestoreConfirm
                    | R::SyncConfirmRotation
                    | R::SyncFinishSecuring { .. }
                    | R::SyncJoin { .. }
                    | R::SyncJoinCode { .. }
                    | R::SyncCancel
                    | R::SyncApprove { .. }
                    | R::SyncApproveCancel
                    | R::SyncRemoveDevice { .. }
                    | R::SyncTurnOff { .. }
                    | R::SyncNow
                    | R::SyncSetCollection { .. }
                    | R::SyncRenameDevice { .. }
                    | R::VaultStatus
            ),
            Sender::Page(Page::Downloads) => {
                matches!(req, R::GetDownloads | R::QueryDownloads { .. } | R::ClearDownloads | R::DownloadAction { .. })
            }
            Sender::Page(Page::About) => matches!(req, R::GetAbout | R::RepairEngine),
            Sender::Page(Page::Gpu) => matches!(req, R::GetGpuInfo | R::OpenGpuInternals | R::RepairEngine),
            Sender::Page(Page::Tasks) => matches!(req, R::GetTasks | R::KillTask { .. }),
            Sender::Page(Page::Sessions) => matches!(
                req,
                R::SessionList
                    | R::SessionDetail { .. }
                    | R::SessionDelete { .. }
                    | R::SessionClear
                    | R::SessionLogSet { .. }
            ),
            Sender::Page(Page::Extensions) => matches!(
                req,
                R::ExtList
                    | R::ExtInstallFile
                    | R::ExtInstallFolder
                    | R::ExtInstallUrl { .. }
                    | R::ExtSet { .. }
                    | R::ExtRemove { .. }
                    | R::ExtOpenOptions { .. }
            ),
            Sender::Ext => matches!(req, R::Ext { .. }),
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
            // 0.11.0: the source viewer asks for the source of the address it shows (its own URL
            // names it; the host checks that).
            Sender::Page(Page::Source) => matches!(req, R::SourceGet {}),
            // The crash page only reloads its address (an ordinary navigation).
            Sender::Page(Page::Crashed) => false,
            Sender::Agent => matches!(
                req,
                R::AgentSend { .. }
                    | R::AgentStop
                    | R::AgentNew
                    | R::AgentScratchClear
                    | R::AgentPasswordShow { .. }
                    | R::AgentPasswordCopy { .. }
                    | R::AgentAnswer { .. }
                    | R::AgentConsent { .. }
                    | R::AgentContinue { .. }
                    | R::AgentSelectModel { .. }
                    | R::AgentModels
                    | R::AgentClose
                    | R::AgentOpenScript { .. }
                    | R::AgentToggleScript { .. }
            ),
            Sender::Runner => matches!(req, R::MacroCall { .. } | R::MacroDone { .. }),
            Sender::Page(Page::Scripts) => matches!(
                req,
                R::ScriptsList
                    | R::ScriptGet { .. }
                    | R::ScriptSave { .. }
                    | R::ScriptToggle { .. }
                    | R::ScriptDelete { .. }
                    | R::ScriptExport { .. }
                    | R::ScriptRunNow { .. }
                    | R::ScriptCheck { .. }
                    | R::MacroSave { .. }
                    | R::MacroDelete { .. }
                    | R::MacroToggle { .. }
                    | R::MacroRun { .. }
                    | R::MacroStop
                    | R::HotkeyCheck { .. }
                    | R::HotkeyRecord { .. }
            ),
            Sender::Page(Page::SiteData) => matches!(
                req,
                R::SiteDataGet {}
                    | R::SitePermissionRemove { .. }
                    | R::SiteDataSet { .. }
                    | R::SiteCookieSet { .. }
                    | R::SiteDataRemove { .. }
                    | R::SiteDataClear { .. }
                    | R::SiteCookiesDelete { .. }
            ),
            Sender::Page(Page::Wasm) => matches!(req, R::WasmInfo {} | R::WasmCall { .. } | R::WasmHook { .. }),
            Sender::Page(Page::HttpsOnly) => matches!(req, R::HttpsOnlyProceed { .. }),
            Sender::Page(Page::CertError) => matches!(req, R::CertProceed { .. } | R::CertBack {}),
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

/// 0.25.2: a file attached in the agent panel: a picture or PDF (`data`, base64) or a text
/// file (`text`). `mime` is the panel's guess; the bytes decide.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFile {
    pub name: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

/// Never the content (a log line must not carry a whole file, or what is in it).
impl std::fmt::Debug for AgentFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentFile")
            .field("name", &self.name)
            .field("mime", &self.mime)
            .field("data_len", &self.data.as_ref().map(String::len))
            .field("text_len", &self.text.as_ref().map(String::len))
            .finish()
    }
}

/// The agent panel's requests may carry attached files (0.25.2; pictures are scaled down by
/// the panel, see `agent::attach` for the limits). The renderer's bridge allows 8 M UTF-16
/// characters; text files may be 3 bytes a character in UTF-8, so this is never the tighter one.
pub const MAX_AGENT_REQUEST_BYTES: usize = 24 * 1024 * 1024;

/// Parses and authorizes a privileged message.
pub fn parse_request(sender: Sender, json: &str) -> Result<Envelope, ProtocolError> {
    // Extension pages store and send larger things (a content blocker's settings).
    let limit = match sender {
        Sender::Ext => MAX_EXT_REQUEST_BYTES,
        Sender::Agent => MAX_AGENT_REQUEST_BYTES,
        _ => MAX_REQUEST_BYTES,
    };
    if json.len() > limit {
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

/// A word of the recovery phrase typed back: `position` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAnswer {
    pub position: u32,
    pub word: Secret,
}

/// A video on the page as the media script reports it (0.6.0). `index` is the script's own
/// numbering (document order, same-origin frames included); the host hands it back for a pick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoInfo {
    pub index: u32,
    /// Title, aria-label or the file name; the chooser's row.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub playing: bool,
    /// This video is in Picture-in-Picture now.
    #[serde(default)]
    pub pip: bool,
    /// Volume in percent, 0 to 100 (0.7.0, R8: the floating window's volume bar).
    #[serde(default = "full_volume")]
    pub volume: u32,
    #[serde(default)]
    pub muted: bool,
}

fn full_volume() -> u32 {
    100
}

/// The media script reports at most this many videos, each label at most this long.
pub const MAX_VIDEOS: usize = 32;
pub const MAX_VIDEO_LABEL_CHARS: usize = 80;

/// Messages from scripts injected into web content (untrusted).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "lb", rename_all = "camelCase", deny_unknown_fields)]
pub enum ContentMessage {
    /// The top-level document has a login form. (Empty struct variants, not
    /// unit variants, so that `deny_unknown_fields` applies to them.)
    LoginForm {},
    /// A login form was submitted; the host may offer to save it.
    Submit { username: String, password: Secret },
    /// The first step of a login in steps (0.11.2: the username or email alone, the password
    /// on the next step or page) was sent: the host remembers it for that password's save.
    Identifier { username: String },
    /// The user clicked the key badge in a login field. The host shows a
    /// trusted confirmation in the shell before anything is filled.
    RequestFill {},
    /// A login field got the focus (any frame): the host may show suggestions.
    FieldFocus {
        field: FieldKind,
        rect: FieldRect,
        /// 0.6.0: what the page knows about a password field (`autocomplete`, the number of
        /// visible password fields in its form, the form's text) for the new-password check.
        #[serde(default)]
        autocomplete: String,
        #[serde(default)]
        passwords: u32,
        #[serde(default, rename = "formText")]
        form_text: String,
    },
    /// The field lost the focus, or the user typed or pressed Esc.
    FieldBlur {},
    /// Class names and ids used on the page, for generic cosmetic filters.
    /// (A page can only affect which elements get hidden on itself.)
    PageTokens { classes: Vec<String>, ids: Vec<String> },
    /// The page's videos changed (main frame only; 0.6.0): the Picture-in-Picture button and
    /// chooser. Same-origin frames are included by the main frame's script.
    Media { videos: Vec<VideoInfo> },
    /// The floating video window's "Back to tab" was used (main frame only; 0.6.0): the tab
    /// and its window come to the front.
    PipBackToTab {},
    /// The document painted its first content (main frame only; 0.7.0, R7): a tab switch that
    /// kept the old page on screen while this view was blank can show it now.
    Painted {},
    /// 0.25.0: the document became visible again (un-hidden from the engine) and has drawn two
    /// animation frames since (main frame only): a switch waiting for it can show it now.
    Shown {},
    /// "Add to Krunk" on a Chrome Web Store detail page (0.7.0, R20; top frame of the store
    /// only). The host takes the extension id from the tab's address, never from the page.
    ExtStoreClick {},
    /// A checkout form with a card number was sent (main frame only; 0.6.0): the browser may
    /// offer to save the card. No security code is ever sent.
    CardSubmit {
        number: String,
        #[serde(default, rename = "expMonth")]
        exp_month: u8,
        #[serde(default, rename = "expYear")]
        exp_year: u16,
        #[serde(default)]
        name: String,
    },
    /// The page called `Notification.requestPermission()` (Krunk's shim; the engine never
    /// asks the embedder for notifications in Alloy style).
    NotificationPermission {},
}

/// Parses a message from web content.
/// Parses a message from an http(s) sub-frame: only the login-field messages are accepted.
pub fn parse_content_frame(json: &str) -> Result<ContentMessage, ProtocolError> {
    match parse_content(json)? {
        m @ (ContentMessage::FieldFocus { .. } | ContentMessage::FieldBlur {}) => Ok(m),
        _ => Err(ProtocolError::Forbidden),
    }
}

pub fn parse_content(json: &str) -> Result<ContentMessage, ProtocolError> {
    if json.len() > MAX_CONTENT_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let msg: ContentMessage = serde_json::from_str(json).map_err(malformed)?;
    if let ContentMessage::Media { videos } = &msg {
        if videos.len() > MAX_VIDEOS
            || videos.iter().any(|v| {
                v.label.chars().count() > MAX_VIDEO_LABEL_CHARS || v.index as usize >= MAX_VIDEOS || v.volume > 100
            })
        {
            return Err(ProtocolError::Invalid("videos"));
        }
    }
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
    /// The page plays sound (speaker icon), and whether the tab is muted (Krunk 0.6.0).
    #[serde(default)]
    pub audible: bool,
    #[serde(default)]
    pub muted: bool,
    /// 0.15.0: the tab waits for an HTTP sign-in (a key badge; its dialog shows with the tab).
    #[serde(default)]
    pub auth_pending: bool,
}

/// The tab a drag from another window would drop: the target strip shows it in its gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabGhost {
    pub id: u32,
    pub title: String,
    pub favicon: Option<String>,
    pub private: bool,
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
    /// 0.25.3: a changed login (sent while the vault was locked) waits on this site: the key
    /// icon shows it and offers "Update password?".
    #[serde(default)]
    pub login_offer: bool,
    /// The bookmarks bar shows in this window now (setting, and in "new tab only" mode
    /// whether the active tab is the new-tab page).
    #[serde(default)]
    pub bookmarks_bar: bool,
    /// The page uses the camera / the microphone now (the address bar's chip, Krunk 0.6.0).
    #[serde(default)]
    pub camera: bool,
    #[serde(default)]
    pub microphone: bool,
    /// Videos on the page (the Picture-in-Picture button; 0.6.0) and whether one of them is
    /// in the floating window now.
    #[serde(default)]
    pub videos: u32,
    #[serde(default)]
    pub pip: bool,
}

fn is_zero_u8(n: &u8) -> bool {
    *n == 0
}

/// An extension's toolbar button (0.6.0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtAction {
    pub id: String,
    pub title: String,
    /// The icon's address on the extension's origin, if it has one.
    pub icon: Option<String>,
    #[serde(default)]
    pub badge: String,
    #[serde(default)]
    pub badge_color: String,
    pub has_popup: bool,
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
    /// One tab's title, address, icon, loading or sound changed (0.10.0): the shell patches
    /// that tab; `tabs` stays the full list for opening, closing, moving and switching.
    TabUpdate {
        tab: TabInfo,
    },
    /// 0.25.0: a plain tab switch: only the active tab changed (the list stays as it is).
    TabActive {
        active: u32,
    },
    Nav {
        state: NavState,
    },
    /// Shell layout options: the theme and the default search engine's name (the
    /// address bar's placeholder).
    ShellConfig {
        theme: Theme,
        search_name: String,
        /// 0.15.0: hover cards on tabs (Settings → Tabs); off: the tab's tooltip instead.
        #[serde(default)]
        tab_hover_preview: bool,
        /// Appearance options (CSS variables and toolbar items); absent in old messages.
        #[serde(default)]
        appearance: Box<crate::settings::Appearance>,
        /// The custom colours with their readable text colours (0.7.0, R3).
        #[serde(default)]
        colors: crate::color::ThemeColors,
    },
    /// To an extension's pages (0.6.0): a message, a storage change, an action click, a
    /// tab event, an alarm, install/startup.
    Ext {
        ext: String,
        kind: String,
        data: serde_json::Value,
    },
    /// To the shells (0.6.0): the enabled extensions' toolbar buttons.
    ExtActions {
        actions: Vec<ExtAction>,
    },
    /// To krunk://extensions: the list changed.
    ExtChanged {
        extensions: serde_json::Value,
    },
    /// To the popup view: render this content. With `seq`, the popup answers `PopupShown`
    /// once it is painted (its window opened or moved for it).
    Popup {
        content: PopupContent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    /// To the popup view, before the content it is about (0.15.4): the height (CSS px) the
    /// content can have at its anchor (`layout::popup_room`; 0: no limit to care about).
    /// Content that is higher is made compact, and scrolls if the window it then gets is
    /// still lower than it.
    PopupRoom {
        max: f64,
    },
    /// To the popup view: its window was hidden; drop the content so that it can never show
    /// again with the next one.
    PopupClear,
    /// To the shell: the popup was hidden (un-highlight its button).
    PopupClosed,
    /// The active tab's docked DevTools (the shell shows its dock and close control).
    DevTools {
        open: bool,
        dock: crate::settings::DevToolsDock,
    },
    /// A tab dragged from another window is over this strip: show where it would land
    /// (`None` = no longer over it).
    /// To a shell while an item dragged out of a folder dropdown is over its bookmarks
    /// bar: the cursor in CSS px of the shell viewport (the shell shows the drop mark).
    BarDragAt {
        x: f64,
        y: f64,
    },
    /// The item was released over the bar at `(x, y)`: the shell works out the target and
    /// sends `moveBookmark`.
    BarDragDrop {
        id: i64,
        x: f64,
        y: f64,
    },
    /// The drag left the bar or ended elsewhere: the shell clears its marks.
    BarDragEnd,
    /// A tab dragged from another window is over this strip at `index` (`None`: it left, or
    /// the drag ended). `ghost`: what the strip shows in the gap it opens there (0.11.1).
    TabDropHint {
        index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ghost: Option<TabGhost>,
    },
    /// To the shell: the bookmarks bar contents.
    BookmarksBar {
        items: Vec<BarItem>,
    },
    /// To downloads pages: the running downloads whose progress moved (0.15.0; the list itself
    /// comes in pages, `QueryDownloads`, fetched again on `DownloadsChanged`).
    Downloads {
        items: Vec<DownloadItem>,
    },
    VaultState {
        exists: bool,
        locked: bool,
    },
    /// The Account card's state (0.8.0), to Settings pages.
    Account {
        card: crate::account::CardState,
    },
    /// Sync's state (0.9.0), to krunk://sync and Settings pages.
    Sync {
        view: crate::sync::view::SyncView,
    },
    FindResult {
        active: i32,
        total: i32,
    },
    /// To the shell: a download just started (the button pulses; the flight was native).
    DownloadStarted,
    /// To a new tab page: its options changed (the same object `getNewTabConfig` returns).
    NewTabConfig {
        config: serde_json::Value,
    },
    /// To the shell: how many downloads run. 0.15.0: also to downloads pages when the list
    /// changed beyond progress (they fetch the rows they show again).
    DownloadsChanged {
        active: u32,
    },
    /// To the shell (0.7.2): a newer Krunk is offered (a dot on ⋮), or none (`None`).
    UpdateAvailable {
        version: Option<String>,
    },
    /// The theme for the shell, popup and built-in pages (`System` follows
    /// the OS through `prefers-color-scheme`).
    Theme {
        theme: Theme,
        /// Accent colour override for pages and the popup (empty = the theme's own).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        accent: String,
        /// The UI font size for Krunk's pages and the popup (0 = default).
        #[serde(default, skip_serializing_if = "is_zero_u8")]
        ui_font_px: u8,
        /// The custom colours with their readable text colours (0.7.0, R3).
        #[serde(default, skip_serializing_if = "crate::color::ThemeColors::is_empty")]
        colors: crate::color::ThemeColors,
    },
    /// To the shell: the suggestions for `text` (the popup shows them; the shell keeps them
    /// for the keyboard).
    AddressSuggestions {
        text: String,
        entries: Vec<AddressSuggestion>,
        #[serde(skip_serializing_if = "Option::is_none")]
        inline: Option<InlineCompletion>,
    },
    /// To the shell (0.15.0): the search engine's suggestions for `text` arrived; `entries`
    /// are the rows shown now (the same rows, then the suggestions). The selection and the
    /// inline completion stay.
    AddressSearchSuggestions {
        text: String,
        entries: Vec<AddressSuggestion>,
    },
    /// To the shell: apply this edit to the address bar (its context menu's choice).
    AddressEdit {
        edit: AddressEdit,
    },
    FocusAddressBar,
    /// The host moved the focus to the page (a bookmark or link was opened):
    /// the address bar stops editing and shows the page's address.
    BlurAddressBar,
    /// The host opened what the address bar offered (a suggestion, Paste and go, a bookmark or
    /// tile): the active tab's unfinished address-bar text is dropped (0.11.6).
    AddressNavigated,
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

    // ---- 0.12.0 ----
    /// To the shell: the agent button (shown only while AI is on) and whether the panel is open.
    AgentButton {
        visible: bool,
        open: bool,
    },
    /// To the shell: a macro is running (its name) or none (`None`): the toolbar's Stop.
    MacroRunning {
        id: Option<String>,
        name: Option<String>,
    },
    /// To the panel: the model indicator and the quick-switch list. `problem` explains why the
    /// agent can't run (no provider, no model, no key).
    AgentConfig {
        provider: String,
        provider_name: String,
        model: String,
        models: Vec<ModelChoice>,
        vision: bool,
        problem: Option<String>,
    },
    AgentState {
        state: RunState,
        steps: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        ms: Option<u64>,
    },
    /// Streamed text of the assistant's current answer.
    AgentDelta {
        text: String,
    },
    /// A whole message: "user", "assistant" (the final answer) or "notice".
    AgentMessage {
        role: String,
        text: String,
    },
    /// A step was added or changed.
    AgentStep {
        step: StepView,
    },
    AgentAsk {
        ask: AskSpec,
    },
    AgentConsentAsk {
        ask: u32,
        category: Category,
        label: String,
        /// 0.16.0, `passwords`: the site (origin) the password would be entered on; the
        /// answer holds for it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        site: Option<String>,
    },
    AgentContinueAsk {
        ask: u32,
        steps: u32,
    },
    /// A userscript or macro the agent made, with Disable / Open in editor.
    AgentScript {
        card: ScriptCard,
    },
    AgentError {
        message: String,
    },
    /// "New conversation": empty the panel.
    AgentClear,
    /// 0.16.0: Krunk generated a password for the agent's `fill_password` and filled it in on
    /// `site`. The card shows it masked: the value itself is not in this event, it stays in the
    /// browser process until the user presses Show (`agentPasswordShow`) or Copy.
    AgentPassword {
        id: u32,
        site: String,
        /// Characters of the password, and the fields it went into.
        length: u32,
        fields: u32,
    },
    /// 0.16.0: the agent's scratchpad as it is now (the plan and the notes with their text),
    /// sent whenever it changes. In memory only, like the conversation.
    AgentScratch {
        scratch: ScratchView,
    },
    /// To krunk://scripts: the lists changed (reload them).
    ScriptsChanged,
    /// To krunk://scripts while recording a hotkey: the chord pressed and why it can't be used.
    HotkeyRecorded {
        chord: String,
        conflict: Option<String>,
    },
    /// To krunk://scripts: which macro runs now.
    MacroState {
        running: Option<String>,
    },
    /// The VPN's state and profiles (0.14.0), to Settings pages, the shells (the button) and
    /// the VPN popup; again every ~2 s while connected (the byte counters).
    Vpn {
        #[serde(flatten)]
        view: crate::vpn::VpnView,
    },
}

/// A saved card as the fill dropdown shows it (0.6.0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardChoice {
    pub id: String,
    /// "Visa •••• 1111" or the nickname.
    pub label: String,
    pub name: String,
}

/// A card from the passwords page (0.6.0). The security code is never part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardInput {
    /// `None` adds a card.
    pub id: Option<String>,
    /// `None` keeps the stored number when editing.
    pub number: Option<Secret>,
    pub exp_month: u8,
    pub exp_year: u16,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub nickname: String,
    /// `None` keeps the stored address when editing.
    #[serde(default)]
    pub address: Option<String>,
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
        // 0.5: an http(s) sub-frame is a WebFrame (login-field messages only).
        assert_eq!(
            classify(Channel::Tab, "https://bank.example/", false),
            Ok(Source::WebFrame { origin: "https://bank.example".into() })
        );
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
        // A page iframed into a tab cannot speak for the tab's origin: it is a WebFrame of its
        // own origin (login-field messages only, see `parse_content_frame`).
        assert_eq!(
            classify(Channel::Tab, "https://bank.example/login", false),
            Ok(Source::WebFrame { origin: "https://bank.example".into() })
        );
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

    fn every_sender() -> Vec<Sender> {
        let mut v = vec![Sender::Shell, Sender::Popup, Sender::Ext, Sender::Agent, Sender::Runner];
        v.extend(Page::ALL.into_iter().map(Sender::Page));
        v
    }

    /// 0.12.0: each new command belongs to exactly its sender; web content gets none of them.
    #[test]
    fn agent_scripts_and_macro_commands_are_least_privilege() {
        let cases: &[(&str, &[Sender])] = &[
            (r#"{"cmd":"toggleAgent"}"#, &[Sender::Shell]),
            (r#"{"cmd":"stopMacros"}"#, &[Sender::Shell]),
            (r#"{"cmd":"agentSend","text":"List the links"}"#, &[Sender::Agent]),
            (
                r#"{"cmd":"agentSend","text":"","files":[{"name":"a.txt","mime":"text/plain","text":"hi"}]}"#,
                &[Sender::Agent],
            ),
            (r#"{"cmd":"agentStop"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentNew"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentScratchClear"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentPasswordShow","id":1}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentPasswordCopy","id":1}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentAnswer","ask":1,"choices":[0,2],"other":"x"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentConsent","ask":2,"allow":false}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentContinue","ask":3,"go":true}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentSelectModel","provider":"0a1b2c3d","model":"m"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentModels"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentClose"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentOpenScript","id":"a","kind":"userscript"}"#, &[Sender::Agent]),
            (r#"{"cmd":"agentToggleScript","id":"a","kind":"macro","enabled":false}"#, &[Sender::Agent]),
            (r#"{"cmd":"macroCall","op":"key.down","args":{"key":"a"}}"#, &[Sender::Runner]),
            (r#"{"cmd":"macroDone","ok":true}"#, &[Sender::Runner]),
            (
                r#"{"cmd":"aiProviderSave","provider":{"id":"0a1b2c3d","kind":"custom","name":"Local","baseUrl":"http://127.0.0.1:11434/v1"}}"#,
                &[Sender::Page(Page::Settings)],
            ),
            (r#"{"cmd":"aiProviderRemove","id":"0a1b2c3d"}"#, &[Sender::Page(Page::Settings)]),
            (r#"{"cmd":"aiKeySet","id":"0a1b2c3d","key":"sk-test"}"#, &[Sender::Page(Page::Settings)]),
            (r#"{"cmd":"aiKeyRemove","id":"0a1b2c3d"}"#, &[Sender::Page(Page::Settings)]),
            (r#"{"cmd":"aiTest","id":"0a1b2c3d"}"#, &[Sender::Page(Page::Settings)]),
            (r#"{"cmd":"aiModels","id":"0a1b2c3d","refresh":true}"#, &[Sender::Page(Page::Settings)]),
            (r#"{"cmd":"aiStatus"}"#, &[Sender::Page(Page::Settings)]),
            (r#"{"cmd":"scriptsList"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptGet","id":"a"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptSave","source":"x"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptToggle","id":"a","enabled":true}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptDelete","id":"a"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptExport","id":"a"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptRunNow","id":"a"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"scriptCheck","source":"x","kind":"macro"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"macroSave","name":"n","source":"s","hotkey":"Ctrl+Alt+K"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"macroDelete","id":"a"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"macroToggle","id":"a","enabled":false}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"macroRun","id":"a"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"macroStop"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"hotkeyCheck","chord":"Ctrl+T"}"#, &[Sender::Page(Page::Scripts)]),
            (r#"{"cmd":"hotkeyRecord","on":true}"#, &[Sender::Page(Page::Scripts)]),
        ];
        for (msg, allowed) in cases {
            for s in every_sender() {
                let r = parse_request(s, msg);
                if allowed.contains(&s) {
                    assert!(r.is_ok(), "{s:?} {msg}: {r:?}");
                } else {
                    assert_eq!(r, Err(ProtocolError::Forbidden), "{s:?} {msg}");
                }
            }
            assert!(parse_content(msg).is_err(), "{msg}");
        }
        // The panel and the runner get nothing else: no tab, vault, settings or page commands.
        for msg in [
            r#"{"cmd":"vaultList"}"#,
            r#"{"cmd":"getSettings"}"#,
            r#"{"cmd":"closeTab","id":1}"#,
            r#"{"cmd":"newTab"}"#,
            r#"{"cmd":"openPage","page":"settings"}"#,
            r#"{"cmd":"command","command":{"command":"newTab"}}"#,
        ] {
            assert_eq!(parse_request(Sender::Agent, msg), Err(ProtocolError::Forbidden), "{msg}");
            assert_eq!(parse_request(Sender::Runner, msg), Err(ProtocolError::Forbidden), "{msg}");
        }
        assert!(parse_request(Sender::Agent, r#"{"cmd":"openUrl","url":"https://a.example/"}"#).is_ok());
        assert_eq!(
            parse_request(Sender::Runner, r#"{"cmd":"openUrl","url":"https://a.example/"}"#),
            Err(ProtocolError::Forbidden)
        );
        assert!(parse_request(Sender::Agent, r#"{"cmd":"ready"}"#).is_ok());
        assert!(parse_request(Sender::Runner, r#"{"cmd":"ready"}"#).is_ok());
        // Unknown fields of struct commands are refused.
        assert!(parse_request(Sender::Agent, r#"{"cmd":"agentSend","text":"x","tab":3}"#).is_err());
    }

    /// The panel and runner channels accept only their own document on the shell origin; a tab
    /// can never become them.
    #[test]
    fn agent_and_runner_channels() {
        let agent = "https://shell.krunk.invalid/agent.html";
        let runner = "https://shell.krunk.invalid/runner.html";
        assert_eq!(classify(Channel::Agent, agent, true), Ok(Source::Privileged(Sender::Agent)));
        assert_eq!(classify(Channel::Runner, runner, true), Ok(Source::Privileged(Sender::Runner)));
        assert_eq!(classify(Channel::Agent, runner, true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Runner, agent, true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Agent, agent, false), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Agent, "https://evil.example/agent.html", true), Err(ProtocolError::Forbidden));
        assert_eq!(classify(Channel::Agent, "https://krunk.invalid/agent.html", true), Err(ProtocolError::Forbidden));
        for url in [agent, runner] {
            assert_eq!(classify(Channel::Tab, url, true), Err(ProtocolError::Forbidden));
            assert_eq!(classify(Channel::Shell, url, true), Err(ProtocolError::Forbidden));
        }
        assert_eq!(
            classify(Channel::Tab, "https://krunk.invalid/scripts.html", true),
            Ok(Source::Privileged(Sender::Page(Page::Scripts)))
        );
    }

    /// 0.14.0: the VPN's commands belong to Settings (and connect / disconnect to the popup
    /// too); the shell only opens the popup; web content and the rest get none of them.
    #[test]
    fn vpn_commands_are_least_privilege() {
        let settings = Sender::Page(Page::Settings);
        let cases: &[(&str, &[Sender])] = &[
            (r#"{"cmd":"vpnState"}"#, &[settings]),
            (r#"{"cmd":"vpnImport"}"#, &[settings]),
            (r#"{"cmd":"vpnRename","id":"ab","name":"Office"}"#, &[settings]),
            (r#"{"cmd":"vpnRemove","id":"ab"}"#, &[settings]),
            (r#"{"cmd":"vpnCredentials","id":"ab","username":"alice","password":"pw-SECRET"}"#, &[settings]),
            (r#"{"cmd":"vpnConnect","id":"ab"}"#, &[settings, Sender::Popup]),
            (r#"{"cmd":"vpnDisconnect"}"#, &[settings, Sender::Popup]),
            (r#"{"cmd":"vpnScope","scope":"private"}"#, &[settings, Sender::Popup]),
        ];
        for (msg, allowed) in cases {
            for s in every_sender() {
                let r = parse_request(s, msg);
                if allowed.contains(&s) {
                    assert!(r.is_ok(), "{s:?} {msg}: {r:?}");
                } else {
                    assert_eq!(r, Err(ProtocolError::Forbidden), "{s:?} {msg}");
                }
            }
            assert!(parse_content(msg).is_err(), "{msg}");
        }
        // The import takes nothing from the page (the host reads the file).
        assert!(matches!(
            parse_request(settings, r#"{"cmd":"vpnImport","text":"[Interface]"}"#),
            Err(ProtocolError::Malformed(_))
        ));
        let env =
            parse_request(settings, r#"{"cmd":"vpnCredentials","id":"ab","username":"alice","password":"pw-SECRET"}"#)
                .unwrap();
        assert!(!format!("{env:?}").contains("SECRET"));
        // The shell opens the popup; the popup goes to Settings → VPN with openPage.
        let rect = r#"{"x":900,"y":40,"width":320,"height":300}"#;
        let open = format!(r#"{{"cmd":"showPopup","popup":"vpn","rect":{rect}}}"#);
        assert!(parse_request(Sender::Shell, &open).is_ok());
        assert!(PopupKind::Vpn.shell_may_open() && PopupKind::Vpn.takes_focus());
        assert!(parse_request(Sender::Popup, r#"{"cmd":"openPage","page":"settings","section":"vpn"}"#).is_ok());
        // The event and the popup carry the same view.
        let view = crate::vpn::VpnView { show_button: true, active: "ab".into(), ..Default::default() };
        let j = Event::Vpn { view: view.clone() }.to_json();
        assert!(j.starts_with(r#"{"event":"vpn","status":{"state":"stopped""#), "{j}");
        assert!(j.contains(r#""active":"ab","showButton":true"#), "{j}");
        let c = PopupContent::Vpn { view };
        assert_eq!(c.kind(), PopupKind::Vpn);
        assert_eq!(c.toggle_anchor().as_deref(), Some("vpn"));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kind"], "vpn");
        assert_eq!(v["profiles"], serde_json::json!([]));
        assert_eq!(serde_json::from_value::<PopupContent>(v).unwrap(), c);
    }

    /// An API key is parsed into a `Secret` and never shown by `Debug`.
    #[test]
    fn ai_key_is_secret() {
        let env =
            parse_request(Sender::Page(Page::Settings), r#"{"cmd":"aiKeySet","id":"0a1b2c3d","key":"sk-or-SECRET-1"}"#)
                .unwrap();
        assert!(!format!("{env:?}").contains("SECRET"));
        let bad = parse_request(
            Sender::Page(Page::Settings),
            r#"{"cmd":"aiKeySet","id":"0a1b2c3d","key":"sk-or-SECRET-1",}"#,
        );
        assert!(matches!(&bad, Err(ProtocolError::Malformed(m)) if !m.contains("SECRET")));
    }

    #[test]
    fn agent_events_shape() {
        let step = StepView {
            n: 2,
            tool: "click".into(),
            label: "Click #go".into(),
            status: crate::agent::StepStatus::Ok,
            summary: "clicked".into(),
            detail: String::new(),
            image: None,
            ms: 12,
        };
        let j = Event::AgentStep { step }.to_json();
        assert!(j.starts_with(r#"{"event":"agentStep","step":{"n":2,"tool":"click""#), "{j}");
        let j = Event::AgentState { state: RunState::Stopped, steps: 3, ms: Some(40) }.to_json();
        assert_eq!(j, r#"{"event":"agentState","state":"stopped","steps":3,"ms":40}"#);
        let j = Event::AgentConsentAsk {
            ask: 1,
            category: Category::History,
            label: "your browsing history".into(),
            site: None,
        }
        .to_json();
        assert!(j.contains(r#""category":"history""#) && !j.contains("site"), "{j}");
        // 0.16.0: the password question names the site; the card's event has no value.
        let j = Event::AgentConsentAsk {
            ask: 2,
            category: Category::Passwords,
            label: Category::Passwords.label().into(),
            site: Some("https://shop.example".into()),
        }
        .to_json();
        assert!(j.contains(r#""category":"passwords""#) && j.ends_with(r#""site":"https://shop.example"}"#), "{j}");
        let j = Event::AgentPassword { id: 3, site: "https://shop.example".into(), length: 20, fields: 2 }.to_json();
        assert_eq!(j, r#"{"event":"agentPassword","id":3,"site":"https://shop.example","length":20,"fields":2}"#);
        assert_eq!(Event::ScriptsChanged.to_json(), r#"{"event":"scriptsChanged"}"#);
        let j = Event::AgentButton { visible: true, open: false }.to_json();
        assert_eq!(j, r#"{"event":"agentButton","visible":true,"open":false}"#);
        // 0.16.0: the scratchpad view.
        let mut scratch = crate::agent::scratch::Scratch::default();
        scratch.note_write("found", "a<b>", false).unwrap();
        scratch.plan_set(&[]).unwrap();
        let j = Event::AgentScratch { scratch: scratch.view() }.to_json();
        assert_eq!(
            j,
            r#"{"event":"agentScratch","scratch":{"plan":[],"notes":[{"name":"found","bytes":4,"content":"a<b>"}],"bytes":4}}"#
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
        // 0.11.0: the tab list (popup) switches and closes tabs.
        assert!(parse_request(Sender::Popup, r#"{"cmd":"closeTab","id":1}"#).is_ok());
        assert!(parse_request(Sender::Popup, r#"{"cmd":"activateTab","id":1}"#).is_ok());
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
        // 0.19.0: the WASM inspector's commands belong to its page; the popup's menu opens it.
        let wasm = Sender::Page(Page::Wasm);
        for msg in [
            r#"{"cmd":"wasmInfo"}"#,
            r#"{"cmd":"wasmCall","req":{"op":"list"}}"#,
            r#"{"cmd":"wasmCall","ctx":3,"req":{"op":"read","mem":0,"where":{"base":8,"offsets":[4]},"type":"f32"}}"#,
            r#"{"cmd":"wasmHook","on":true}"#,
        ] {
            assert!(parse_request(wasm, msg).is_ok(), "{msg}");
            for s in senders(wasm) {
                assert_eq!(parse_request(s, msg), Err(ProtocolError::Forbidden), "{s:?} {msg}");
            }
        }
        assert!(parse_request(wasm, r#"{"cmd":"wasmCall","req":{"op":"eval","code":"1"}}"#).is_err());
        assert_eq!(
            parse_request(wasm, r#"{"cmd":"openUrl","url":"https://a.example/"}"#),
            Err(ProtocolError::Forbidden)
        );
        assert!(parse_request(Sender::Popup, r#"{"cmd":"openWasm"}"#).is_ok());
        assert_eq!(parse_request(Sender::Shell, r#"{"cmd":"openWasm"}"#), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(wasm, r#"{"cmd":"openWasm"}"#), Err(ProtocolError::Forbidden));
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
        let scale = r#"{"cmd":"shellScale","dpr":1.5}"#;
        assert_eq!(parse_request(Sender::Shell, scale).unwrap().request, Request::ShellScale { dpr: 1.5 });
        for msg in [map, control, scale] {
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
            r#"{"cmd":"queryDownloads","text":"a","filter":"failed","offset":100,"limit":50,"counts":true,"since":[1,2]}"#,
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
        assert!(parse_request(Sender::Page(Page::Downloads), privileged[14]).is_ok());
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
            repair: None,
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
            parse_content(r#"{"lb":"identifier","username":"a@b.example"}"#),
            Ok(ContentMessage::Identifier { username: "a@b.example".into() })
        );
        assert!(parse_content_frame(r#"{"lb":"identifier","username":"a"}"#).is_err(), "top frame only");
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
    /// 0.15.0: the hover card never takes the focus and only the host opens it; the shell's
    /// hover request parses; old tab lists (no `authPending`) still do.
    #[test]
    fn tab_preview_and_auth_badge() {
        assert!(!PopupKind::TabPreview.takes_focus() && !PopupKind::TabPreview.shell_may_open());
        let c = PopupContent::TabPreview {
            tab_id: 2,
            title: "T".into(),
            host: "a.example".into(),
            url: "https://a.example/".into(),
            thumbnail: None,
            active: false,
            private: false,
        };
        assert_eq!(c.kind(), PopupKind::TabPreview);
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["kind"], "tabPreview");
        assert!(j.get("thumbnail").is_none());
        let r: Request =
            serde_json::from_str(r#"{"cmd":"tabHover","id":4,"rect":{"x":10,"y":4,"width":200,"height":34}}"#).unwrap();
        assert!(matches!(r, Request::TabHover { id: Some(4), rect: Some(_) }));
        assert!(Sender::Shell.permits(&r) && !Sender::Popup.permits(&r));
        let r: Request = serde_json::from_str(r#"{"cmd":"tabWarm","id":4}"#).unwrap();
        assert!(matches!(r, Request::TabWarm { id: Some(4) }));
        assert!(Sender::Shell.permits(&r) && !Sender::Popup.permits(&r) && !Sender::Page(Page::NewTab).permits(&r));
        let r: Request = serde_json::from_str(r#"{"cmd":"tabHover"}"#).unwrap();
        assert_eq!(r, Request::TabHover { id: None, rect: None });
        let t: TabInfo = serde_json::from_str(
            r#"{"id":1,"title":"t","url":"u","favicon":null,"loading":false,"private":false,"sleeping":false}"#,
        )
        .unwrap();
        assert!(!t.auth_pending);
        let t = TabInfo { auth_pending: true, ..t };
        assert_eq!(serde_json::to_value(&t).unwrap()["authPending"], true);
    }

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
            Request::Home { new_tab: false },
            Request::Home { new_tab: true },
            Request::ShowExtMenu { id: "abcdefghijklmnopabcdefghijklmnop".into(), rect },
            Request::ExtMenuAction { id: "abcdefghijklmnopabcdefghijklmnop".into(), action: ExtMenuAction::Uninstall },
            Request::OpenUrl { url: "https://b.example/".into(), new_tab: true },
            Request::OpenPage { page: "settings".into(), section: None },
            Request::OpenPage { page: "settings".into(), section: Some("privacy".into()) },
            Request::GetGpuInfo,
            Request::OpenGpuInternals,
            Request::RepairEngine,
            Request::ImportDetect,
            Request::ImportRun {
                profile: "firefox:default".into(),
                types: vec!["history".into()],
                primary_password: Some(Secret::from("pw")),
            },
            Request::ImportCsv,
            Request::ToggleBookmark,
            Request::HitMap {
                map: crate::frame::HitMap {
                    strip_height: 36.0,
                    interactive: vec![rect],
                    minimize: rect,
                    maximize: rect,
                    close: rect,
                    tab_centers: vec![10.0, 90.0],
                    downloads: PopupRect::default(),
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
            Request::SourceGet {},
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
            Request::TabDragOut { id: 3, offset_x: 40.5, offset_y: 18.0, first_x: Some(6.0) },
            Request::ShowTabMenu { id: 3, rect },
            Request::TabHover { id: Some(3), rect: Some(rect) },
            Request::TabHover { id: None, rect: None },
            Request::ShowNavMenu { forward: true, rect },
            Request::NavToEntry { tab_id: 3, entry_id: 17 },
            Request::HttpAuthReply { tab_id: 3, username: "u".into(), password: "p".into(), entry_id: None },
            Request::HttpAuthReply {
                tab_id: 3,
                username: String::new(),
                password: Secret::default(),
                entry_id: Some("e1".into()),
            },
            Request::HttpAuthCancel { tab_id: 3 },
            Request::HttpAuthUnlock { tab_id: 3 },
            Request::AccountState { check: true },
            Request::AccountSignIn,
            Request::AccountCancel,
            Request::AccountSignOut,
            Request::AccountManage,
            Request::AccountAddDevice,
            Request::AccountReopen,
            Request::SyncState,
            Request::SyncOpen,
            Request::SyncTurnOn { passwords: true },
            Request::SyncSetupConfirm { group: "ABCD".into(), understood: true },
            Request::SyncSetupCopy,
            Request::SyncSetupSave,
            Request::SyncRestore { secret: Secret::from("words"), recovery: false, vault: false },
            Request::SyncSetupVault { password: Secret::from("vault pw") },
            Request::SyncRestoreConfirm,
            Request::SyncConfirmRotation,
            Request::SyncFinishSecuring { secret: Secret::from("words"), recovery: false },
            Request::SyncJoin { passwords: false },
            Request::SyncJoinCode { code: "123456".into() },
            Request::SyncCancel,
            Request::SyncApprove { id: "a1".into(), share_passwords: false },
            Request::SyncApproveCancel,
            Request::SyncRemoveDevice { id: "d1".into() },
            Request::SyncTurnOff { delete_data: false },
            Request::SyncNow,
            Request::SyncSetCollection { name: "history".into(), on: false },
            Request::SyncRenameDevice { id: "d1".into(), name: "Laptop".into() },
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
            Request::SavePasswordReply {
                tab_id: 1,
                origin: "https://a.example".into(),
                accept: true,
                never: false,
                username: None,
            },
            Request::SavePasswordReply {
                tab_id: 1,
                origin: "https://a.example".into(),
                accept: true,
                never: false,
                username: Some("bob@a.example".into()),
            },
            Request::PermissionReply { prompt_id: 7, allow: true, remember: true },
            Request::AddressSuggest { text: "ru".into(), rect },
            Request::ShowAddressMenu { x: 10.0, y: 40.0, selection: "ab".into(), has_text: true, dirty: false },
            Request::AddressMenuAction { action: AddressMenuAction::PasteAndGo },
            Request::TabMute { id: 3, muted: true },
            Request::BookmarkAllTabs,
            Request::CaptionWindow { action: WindowAction::Maximize },
            Request::GetTasks,
            Request::KillTask { id: 42 },
            Request::SessionList,
            Request::SessionDetail { id: 3 },
            Request::SessionDelete { id: 3 },
            Request::SessionClear,
            Request::SessionLogSet { enabled: Some(true), retention_days: None },
            Request::SessionLogSet { enabled: None, retention_days: Some(90) },
            Request::PermissionExceptions,
            Request::BounceStatus,
            Request::BounceCleanNow,
            Request::DefaultBrowserStatus,
            Request::MakeDefaultBrowser,
            Request::DnsStatus,
            Request::DnsTest { index: 1 },
            Request::DnsParse { text: "1.1.1.1".into() },
            Request::PermissionSet {
                origin: "https://a.example".into(),
                kind: crate::permissions::Kind::Camera,
                state: crate::permissions::PermissionState::Block,
            },
            Request::AddressSuggestSelect { selected: Some(1) },
            Request::AddressSuggestPick { url: "https://rust-lang.org/".into() },
            Request::SitePermissionRemove { kind: "localNetwork".into() },
            Request::FillReply { tab_id: 1, origin: "https://a.example".into(), entry_id: None },
            Request::Quit,
            Request::GetTopSites,
            Request::GetNewTabConfig,
            Request::SetNtpOptions { ntp: crate::settings::Ntp::default() },
            Request::NtpPickImage,
            Request::ReorderTiles { urls: vec!["https://a.example/".into()] },
            Request::AddTile { url: "https://b.example/".into(), title: "B".into() },
            Request::EditTile {
                url: "https://b.example/".into(),
                new_url: "https://c.example/".into(),
                title: "C".into(),
            },
            Request::PinSite { url: "https://a.example/".into(), title: "A".into() },
            Request::QueryHistory { text: "q".into(), before: Some(5), limit: 10, device: "this".into() },
            Request::DeleteHistoryRange { from: 1, to: 2, device: String::new() },
            Request::HistoryDevices,
            Request::ClearBrowsingData { data: ClearData { history: true, ..ClearData::default() } },
            Request::MoveBookmark { id: 1, parent: 2, index: 0 },
            Request::SuggestPick { tab_id: 3, frame_id: "f1".into(), entry_id: "e1".into() },
            Request::SuggestUnlock { tab_id: 3, frame_id: "f1".into() },
            Request::ShowFolderItemMenu { id: 4, x: 10.0, y: 20.0 },
            Request::MoveBookmarkInFolder { id: 4, parent: 2, index: 1 },
            Request::FolderItemDragOut { id: 4 },
            Request::PopupResize { height: 420.0 },
            Request::BookmarkActivePage { parent: 1, index: 0 },
            Request::AddBookmarkAt { url: "https://a.example/".into(), parent: 1, index: 2 },
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
            Request::QueryDownloads {
                text: "zip".into(),
                filter: crate::downloads::DownloadFilter::Done,
                offset: 200,
                limit: 100,
                counts: true,
                since: vec![5, 4],
            },
            Request::ShowBookmarkFolder { id: 4, rect: Some(rect), from: None },
            Request::ShowBookmarkFolder { id: 4, rect: None, from: None },
            Request::ShowBookmarkFolder { id: 1, rect: Some(rect), from: Some(3) },
            Request::BookmarkFolderBack,
            Request::HangReply { tab_id: 3, wait: false },
            Request::JsDialogReply { dialog_id: 2, accept: true, text: Some("x".into()), block: false },
            Request::JsDialogReply { dialog_id: 3, accept: false, text: None, block: true },
            Request::OpenBookmarkBackground { id: 9 },
            Request::PopupShown { seq: 3, height: Some(212.0) },
            Request::PopupShown { seq: 4, height: None },
            Request::ClearBrowsingData {
                data: ClearData { cookies: true, cache: true, site_data: true, since: Some(1), ..ClearData::default() },
            },
            Request::HttpsOnlyProceed { url: "http://a.example/".into() },
            Request::CertProceed { url: "https://a.example/".into(), sha256: "ab".repeat(32), remember: true },
            Request::CertBack {},
            Request::CertExceptions,
            Request::CertExceptionRemove { host: "a.example".into(), port: 8443, sha256: "ab".repeat(32) },
            Request::GetFilterStatus,
            Request::RequestFill,
            Request::GetNeverSave,
            Request::NeverSaveRemove { origin: "https://a.example".into() },
            Request::VaultUnlock { password: Secret::from("pw"), remain: None },
            Request::VaultUnlock { password: Secret::from("pw"), remain: Some(false) },
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
        // 0.5: the folder dropdown's own menu and drags (popup), the address-bar drag (shell).
        let folder_menu = Request::ShowFolderItemMenu { id: 12, x: 20.0, y: 40.0 };
        let in_folder = Request::MoveBookmarkInFolder { id: 12, parent: 11, index: 0 };
        let out = Request::FolderItemDragOut { id: 12 };
        let page_drop = Request::BookmarkActivePage { parent: 1, index: 0 };
        let url_drop = Request::AddBookmarkAt { url: "https://a.example/".into(), parent: 1, index: 0 };
        assert!(
            Sender::Popup.permits(&folder_menu) && Sender::Popup.permits(&in_folder) && Sender::Popup.permits(&out)
        );
        assert!(
            !Sender::Shell.permits(&folder_menu) && !Sender::Shell.permits(&in_folder) && !Sender::Shell.permits(&out)
        );
        assert!(Sender::Shell.permits(&page_drop) && Sender::Shell.permits(&url_drop));
        assert!(!Sender::Popup.permits(&page_drop) && !Sender::Popup.permits(&url_drop));
        for page in Page::ALL {
            let p = Sender::Page(page);
            for r in [&folder_menu, &in_folder, &out, &page_drop, &url_drop] {
                assert!(!p.permits(r), "{page:?} {r:?}");
            }
        }
        assert_eq!(
            parse_request(Sender::Popup, r#"{"cmd":"showFolderItemMenu","id":12,"x":20,"y":40}"#).unwrap().request,
            folder_menu
        );
        // The popup may ask for another height; nothing else may.
        let resize = Request::PopupResize { height: 300.0 };
        assert!(Sender::Popup.permits(&resize) && !Sender::Shell.permits(&resize));
        assert!(Page::ALL.iter().all(|p| !Sender::Page(*p).permits(&resize)));
        // The lock popup is a shell popup like the menu and the shield.
        assert!(PopupKind::SiteInfo.shell_may_open() && PopupKind::SiteInfo.takes_focus());
        let info = PopupContent::SiteInfo { info: crate::certinfo::site_info("http://neverssl.com/", None) };
        assert_eq!(info.kind(), PopupKind::SiteInfo);
        assert_eq!(info.toggle_anchor().as_deref(), Some("siteInfo"));
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains(r#""kind":"siteInfo""#) && json.contains(r#""kind":"http""#), "{json}");
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
    fn login_field_messages_from_sub_frames() {
        assert_eq!(
            classify(Channel::Tab, "https://login.example/frame.html", false),
            Ok(Source::WebFrame { origin: "https://login.example".into() })
        );
        assert_eq!(classify(Channel::Tab, "https://krunk.invalid/newtab.html", false), Err(ProtocolError::Forbidden));
        assert_eq!(
            classify(Channel::Tab, "https://shell.krunk.invalid/shell.html", false),
            Err(ProtocolError::Forbidden)
        );
        assert_eq!(
            classify(Channel::Shell, "https://shell.krunk.invalid/shell.html", false),
            Err(ProtocolError::Forbidden)
        );
        assert_eq!(classify(Channel::Tab, "data:text/html,x", false), Err(ProtocolError::Forbidden));
        assert_eq!(
            classify(Channel::Tab, "https://a.example/", true),
            Ok(Source::Web { origin: "https://a.example".into() })
        );
        // Only the field messages count from a sub-frame.
        let focus = r#"{"lb":"fieldFocus","field":"password","rect":{"x":10,"y":20,"width":200,"height":30}}"#;
        let blur = r#"{"lb":"fieldBlur"}"#;
        assert!(matches!(
            parse_content_frame(focus),
            Ok(ContentMessage::FieldFocus { field: FieldKind::Password, .. })
        ));
        assert!(matches!(parse_content_frame(blur), Ok(ContentMessage::FieldBlur {})));
        assert!(
            parse_content_frame(r#"{"lb":"submit","username":"u","password":"p"}"#).is_err(),
            "no submit from a frame"
        );
        assert!(parse_content_frame(r#"{"lb":"loginForm"}"#).is_err());
        assert!(parse_content_frame(r#"{"lb":"requestFill"}"#).is_err());
        // The main frame still gets everything, the field messages included.
        assert!(matches!(parse_content(focus), Ok(ContentMessage::FieldFocus { .. })));
        assert!(
            parse_content(r#"{"lb":"fieldFocus","field":"password","rect":{"x":1,"y":2,"width":3,"height":4,"z":5}}"#)
                .is_err(),
            "no unknown rect fields"
        );
        assert!(!FieldRect { x: f64::NAN, y: 0.0, width: 1.0, height: 1.0 }.is_sane());
        assert!(FieldRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }.is_sane());
        assert!(!FieldRect { x: 1.0, y: 2.0, width: -3.0, height: 4.0 }.is_sane());
        // The dropdown's requests come from the popup only; the dropdown never takes focus.
        let pick = Request::SuggestPick { tab_id: 1, frame_id: "f".into(), entry_id: "e".into() };
        let unlock = Request::SuggestUnlock { tab_id: 1, frame_id: "f".into() };
        assert!(Sender::Popup.permits(&pick) && Sender::Popup.permits(&unlock));
        assert!(!Sender::Shell.permits(&pick) && !Sender::Shell.permits(&unlock));
        assert!(Page::ALL.iter().all(|p| !Sender::Page(*p).permits(&pick) && !Sender::Page(*p).permits(&unlock)));
        assert!(!PopupKind::Suggest.takes_focus() && !PopupKind::Suggest.shell_may_open());
        let content = PopupContent::Suggest {
            tab_id: 1,
            frame_id: "f".into(),
            origin: "https://a.example".into(),
            locked: true,
            entries: vec![],
            generated: None,
        };
        assert_eq!(content.kind(), PopupKind::Suggest);
        assert!(serde_json::to_string(&content).unwrap().contains(r#""locked":true"#));
    }

    #[test]
    fn new_tab_config_is_new_tab_page_only() {
        let reqs = [
            Request::GetNewTabConfig,
            Request::SetNtpOptions { ntp: crate::settings::Ntp::default() },
            // (NtpPickImage: also the settings page since 0.7.0, its own test.)
            Request::ReorderTiles { urls: vec![] },
            Request::AddTile { url: "https://a.example/".into(), title: "A".into() },
            Request::EditTile {
                url: "https://a.example/".into(),
                new_url: "https://b.example/".into(),
                title: "B".into(),
            },
        ];
        for req in &reqs {
            assert!(Sender::Page(Page::NewTab).permits(req), "{req:?}");
            assert!(!Sender::Shell.permits(req) && !Sender::Popup.permits(req), "{req:?}");
            for p in Page::ALL {
                if p != Page::NewTab {
                    assert!(!Sender::Page(p).permits(req), "{p:?} {req:?}");
                }
            }
        }
        assert_eq!(serde_json::to_value(&reqs[0]).unwrap(), serde_json::json!({"cmd": "getNewTabConfig"}));
        let theme =
            Event::Theme { theme: Theme::Light, accent: "#ff0000".into(), ui_font_px: 0, colors: Default::default() }
                .to_json();
        assert!(theme.contains("\"accent\":\"#ff0000\""));
        assert!(!Event::Theme {
            theme: Theme::Light,
            accent: String::new(),
            ui_font_px: 0,
            colors: Default::default()
        }
        .to_json()
        .contains("accent"));
    }

    #[test]
    fn payment_card_messages() {
        for r in [
            Request::CardList,
            Request::CardDelete { id: "c".into() },
            Request::CardCopy { id: "c".into() },
            Request::CardReveal { id: "c".into(), password: Secret::from("pw") },
            Request::CardSave {
                card: CardInput {
                    id: None,
                    number: Some(Secret::from("4111111111111111")),
                    exp_month: 1,
                    exp_year: 2030,
                    name: String::new(),
                    nickname: String::new(),
                    address: None,
                },
            },
        ] {
            assert!(Sender::Page(Page::Passwords).permits(&r), "{r:?}");
            assert!(
                !Sender::Popup.permits(&r) && !Sender::Shell.permits(&r) && !Sender::Page(Page::Settings).permits(&r),
                "{r:?}"
            );
        }
        let pick = Request::CardFillPick { tab_id: 1, frame_id: "f".into(), card_id: "c".into() };
        let reply = Request::SaveCardReply { tab_id: 1, origin: "https://shop.example".into(), accept: true };
        assert!(Sender::Popup.permits(&pick) && Sender::Popup.permits(&reply));
        assert!(!Sender::Shell.permits(&pick) && !Sender::Page(Page::Passwords).permits(&reply));
        let json = serde_json::to_string(&Request::CardSave {
            card: CardInput {
                id: Some("x".into()),
                number: None,
                exp_month: 2,
                exp_year: 2031,
                name: "A".into(),
                nickname: "".into(),
                address: Some("addr".into()),
            },
        })
        .unwrap();
        assert!(json.contains("\"expMonth\":2") && !json.contains("cvc"));
        let m = parse_content(
            r#"{"lb":"cardSubmit","number":"4111 1111 1111 1111","expMonth":12,"expYear":2030,"name":"A"}"#,
        )
        .unwrap();
        assert!(matches!(m, ContentMessage::CardSubmit { exp_month: 12, .. }));
        assert!(parse_content_frame(r#"{"lb":"cardSubmit","number":"4111111111111111"}"#).is_err(), "main frame only");
        let focus =
            parse_content(r#"{"lb":"fieldFocus","field":"card","rect":{"x":1,"y":2,"width":30,"height":10}}"#).unwrap();
        assert!(matches!(focus, ContentMessage::FieldFocus { field: FieldKind::Card, .. }));
        let fill = PopupContent::CardFill {
            tab_id: 1,
            frame_id: "f".into(),
            origin: "o".into(),
            locked: false,
            cards: vec![CardChoice { id: "c".into(), label: "Visa •••• 1111".into(), name: "A".into() }],
        };
        assert_eq!(fill.kind(), PopupKind::CardFill);
        let save = PopupContent::SaveCard { tab_id: 1, origin: "o".into(), brand: "Visa".into(), last4: "1111".into() };
        assert_eq!(save.kind(), PopupKind::SaveCard);
        assert!(
            !PopupKind::CardFill.takes_focus() && PopupKind::SaveCard.takes_focus(),
            "the dropdown leaves the field focused"
        );
        assert_eq!(serde_json::to_string(&UnlockPurpose::CardFill).unwrap(), "\"cardFill\"");
        assert_eq!(serde_json::to_string(&UnlockPurpose::SaveCard).unwrap(), "\"saveCard\"");
        assert!(!serde_json::to_string(&save).unwrap().contains("number"));
    }

    #[test]
    fn extension_messages() {
        let id = "a".repeat(32);
        let page = format!("https://{id}.ext.krunk.invalid/popup.html");
        assert_eq!(classify(Channel::Ext, &page, true), Ok(Source::Privileged(Sender::Ext)));
        assert_eq!(
            classify(Channel::Tab, &page, true),
            Ok(Source::Privileged(Sender::Ext)),
            "an options page in a tab"
        );
        assert!(classify(Channel::Ext, "https://krunk.invalid/newtab.html", true).is_err());
        assert!(classify(Channel::Ext, &page, false).is_err(), "frames are never privileged");
        assert!(classify(Channel::Shell, &page, true).is_err());
        let call =
            Request::Ext { ext: id.clone(), op: "storage.get".into(), data: serde_json::json!({ "area": "local" }) };
        assert!(Sender::Ext.permits(&call));
        assert!(!Sender::Shell.permits(&call) && !Sender::Popup.permits(&call));
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&call), "{p:?}");
        }
        let list = Request::ExtList;
        let set = Request::ExtSet { id: id.clone(), enabled: Some(false), private: None };
        assert!(Sender::Page(Page::Extensions).permits(&list) && Sender::Page(Page::Extensions).permits(&set));
        assert!(
            !Sender::Ext.permits(&list) && !Sender::Page(Page::Settings).permits(&set) && !Sender::Shell.permits(&list)
        );
        let action = Request::ExtAction { id: id.clone(), rect: PopupRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 } };
        assert!(Sender::Shell.permits(&action) && !Sender::Popup.permits(&action) && !Sender::Ext.permits(&action));
        let parsed =
            parse_request(Sender::Ext, &format!(r#"{{"cmd":"ext","ext":"{id}","op":"tabs.query","reqId":7}}"#))
                .unwrap();
        assert_eq!(parsed.req_id, Some(7));
        assert!(matches!(parsed.request, Request::Ext { op, data, .. } if op == "tabs.query" && data.is_null()));
        assert!(parse_request(Sender::Ext, r#"{"cmd":"getSettings"}"#).is_err(), "nothing else from an extension page");
        let ev = Event::ExtActions {
            actions: vec![ExtAction {
                id,
                title: "T".into(),
                icon: None,
                badge: String::new(),
                badge_color: String::new(),
                has_popup: true,
            }],
        };
        assert!(ev.to_json().contains("\"event\":\"extActions\"") && ev.to_json().contains("\"hasPopup\":true"));
    }

    #[test]
    fn extension_install_confirmation_messages() {
        // 0.7.0 (R20): the confirmation is the host's popup; only it answers.
        assert!(!PopupKind::ExtInstall.shell_may_open() && PopupKind::ExtInstall.takes_focus());
        let reply = Request::ExtInstallReply { request_id: 4, accept: true };
        assert!(Sender::Popup.permits(&reply));
        for s in [Sender::Shell, Sender::Ext] {
            assert!(!s.permits(&reply), "{s:?}");
        }
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&reply), "{p:?}");
        }
        let c = PopupContent::ExtInstall {
            request_id: 4,
            name: "uBO Lite".into(),
            version: "1.0".into(),
            source: "Chrome Web Store".into(),
            warnings: vec!["Read and change all your data on all websites".into()],
            previous: None,
        };
        assert_eq!(c.kind(), PopupKind::ExtInstall);
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            json.contains("\"kind\":\"extInstall\"") && json.contains("\"requestId\":4") && !json.contains("previous"),
            "{json}"
        );
        // The store page's click: nothing but the click (the host reads the id from the tab).
        assert_eq!(parse_content(r#"{"lb":"extStoreClick"}"#), Ok(ContentMessage::ExtStoreClick {}));
        assert!(
            parse_content(r#"{"lb":"extStoreClick","id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#).is_err(),
            "no id from the page"
        );
        assert!(parse_content_frame(r#"{"lb":"extStoreClick"}"#).is_err(), "top frame only");
        assert!(crate::ext::is_store_origin("https://chromewebstore.google.com"));
        assert!(!crate::ext::is_store_origin("https://chromewebstore.google.com.evil.example"));
    }

    #[test]
    fn recovery_phrase_messages_are_for_the_passwords_page_only() {
        let all = [
            r#"{"cmd":"vaultRecoveryCreate","password":"pw"}"#,
            r#"{"cmd":"vaultRecoveryConfirm","answers":[{"position":3,"word":"zoo"}]}"#,
            r#"{"cmd":"vaultRecoveryCancel"}"#,
            r#"{"cmd":"vaultRecoveryCopy"}"#,
            r#"{"cmd":"vaultRecoverySave"}"#,
            r#"{"cmd":"vaultRecoveryRemove","password":"pw"}"#,
            r#"{"cmd":"vaultRecoveryCheck","phrase":"abandon"}"#,
            r#"{"cmd":"vaultRecover","phrase":"abandon","new":"new master"}"#,
        ];
        for json in all {
            assert!(parse_request(Sender::Page(Page::Passwords), json).is_ok(), "{json}");
            for sender in [Sender::Shell, Sender::Popup, Sender::Ext] {
                assert_eq!(parse_request(sender, json), Err(ProtocolError::Forbidden), "{sender:?} {json}");
            }
            for p in Page::ALL.iter().filter(|p| **p != Page::Passwords) {
                assert_eq!(parse_request(Sender::Page(*p), json), Err(ProtocolError::Forbidden), "{p:?} {json}");
            }
        }
        // The phrase and the words are secrets: never printed, unknown fields refused.
        let r = parse_request(
            Sender::Page(Page::Passwords),
            r#"{"cmd":"vaultRecover","phrase":"legal winner thank year","new":"hunter22"}"#,
        )
        .unwrap();
        let shown = format!("{r:?}");
        assert!(!shown.contains("winner") && !shown.contains("hunter22"), "{shown}");
        let c = parse_request(Sender::Page(Page::Passwords), all[1]).unwrap();
        assert!(!format!("{c:?}").contains("zoo"));
        assert!(parse_request(
            Sender::Page(Page::Passwords),
            r#"{"cmd":"vaultRecoveryConfirm","answers":[{"position":3,"word":"zoo","hint":1}]}"#
        )
        .is_err());
    }

    #[test]
    fn picture_in_picture_volume_messages() {
        // 0.7.0 (R8): the volume bar is the host's popup; only it sets the volume.
        assert!(!PopupKind::PipVolume.shell_may_open() && !PopupKind::PipVolume.takes_focus());
        let set = Request::PipVolume { tab_id: 1, volume: 40, muted: false };
        assert!(Sender::Popup.permits(&set) && !Sender::Shell.permits(&set) && !Sender::Ext.permits(&set));
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&set), "{p:?}");
        }
        let env = parse_request(Sender::Popup, r#"{"cmd":"pipVolume","tabId":3,"volume":25,"muted":true}"#).unwrap();
        assert_eq!(env.request, Request::PipVolume { tab_id: 3, volume: 25, muted: true });
        let bar = PopupContent::PipVolume { tab_id: 3, volume: 25, muted: true };
        assert_eq!(bar.kind(), PopupKind::PipVolume);
        assert_eq!(bar.toggle_anchor(), None);
        let json = serde_json::to_string(&bar).unwrap();
        assert!(json.contains("\"kind\":\"pipVolume\"") && json.contains("\"volume\":25"), "{json}");
        // The page reports each video's volume and mute; older scripts without them mean full volume.
        match parse_content(r#"{"lb":"media","videos":[{"index":0,"volume":30,"muted":true},{"index":1}]}"#).unwrap() {
            ContentMessage::Media { videos } => {
                assert_eq!((videos[0].volume, videos[0].muted), (30, true));
                assert_eq!((videos[1].volume, videos[1].muted), (100, false));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            parse_content(r#"{"lb":"media","videos":[{"index":0,"volume":101}]}"#),
            Err(ProtocolError::Invalid("videos"))
        );
    }

    #[test]
    fn picture_in_picture_messages() {
        assert!(PopupKind::Pip.shell_may_open() && PopupKind::Pip.takes_focus());
        let pick = Request::PipPick { tab_id: 1, index: 2 };
        assert!(Sender::Popup.permits(&pick) && !Sender::Shell.permits(&pick));
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&pick), "{p:?}");
        }
        let m = parse_content(
            r#"{"lb":"media","videos":[{"index":0,"label":"Trailer","width":640,"height":360,"playing":true,"pip":false},{"index":1}]}"#,
        )
        .unwrap();
        match &m {
            ContentMessage::Media { videos } => {
                assert_eq!(videos.len(), 2);
                assert_eq!(videos[0].label, "Trailer");
                assert!(videos[0].playing && !videos[1].playing && videos[1].label.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_content_frame(r#"{"lb":"media","videos":[]}"#).is_err(), "main frame only");
        assert!(parse_content(r#"{"lb":"media","videos":[{"index":0,"cvc":1}]}"#).is_err(), "unknown fields");
        let long = format!(r#"{{"lb":"media","videos":[{{"index":0,"label":"{}"}}]}}"#, "x".repeat(81));
        assert_eq!(parse_content(&long), Err(ProtocolError::Invalid("videos")));
        let many = format!(r#"{{"lb":"media","videos":[{}]}}"#, vec![r#"{"index":0}"#; 33].join(","));
        assert_eq!(parse_content(&many), Err(ProtocolError::Invalid("videos")));
        assert_eq!(parse_content(r#"{"lb":"media","videos":[{"index":40}]}"#), Err(ProtocolError::Invalid("videos")));
        assert_eq!(parse_content(r#"{"lb":"pipBackToTab"}"#), Ok(ContentMessage::PipBackToTab {}));
        assert!(parse_content_frame(r#"{"lb":"pipBackToTab"}"#).is_err());
        // 0.7.0: first paint (top frame only; nothing to say but that).
        assert_eq!(parse_content(r#"{"lb":"painted"}"#), Ok(ContentMessage::Painted {}));
        assert_eq!(parse_content(r#"{"lb":"shown"}"#), Ok(ContentMessage::Shown {}));
        assert!(parse_content(r#"{"lb":"shown","x":1}"#).is_err());
        assert!(parse_content_frame(r#"{"lb":"painted"}"#).is_err());
        assert!(parse_content(r#"{"lb":"painted","tab":3}"#).is_err());
        let chooser = PopupContent::Pip {
            tab_id: 3,
            videos: vec![VideoInfo {
                index: 0,
                label: "A".into(),
                width: 1,
                height: 1,
                playing: true,
                pip: false,
                volume: 100,
                muted: false,
            }],
        };
        assert_eq!(chooser.kind(), PopupKind::Pip);
        assert_eq!(chooser.toggle_anchor().as_deref(), Some("pip"));
        let json = serde_json::to_string(&chooser).unwrap();
        assert!(json.contains("\"kind\":\"pip\"") && json.contains("\"tabId\":3"));
        let menu: PopupContent = serde_json::from_str(r#"{"kind":"menu","zoom":1.0,"canReopen":false}"#).unwrap();
        assert!(matches!(menu, PopupContent::Menu { has_video: false, .. }), "older menus without the flag");
        let nav: NavState = serde_json::from_str(
            r#"{"tabId":1,"url":"","canGoBack":false,"canGoForward":false,"loading":false,"secure":false,"bookmarked":false,"blocked":0,"siteAllowed":false,"zoom":1.0,"private":false}"#,
        )
        .unwrap();
        assert_eq!((nav.videos, nav.pip), (0, false));
    }

    #[test]
    fn password_generator_messages() {
        let gen = Request::SuggestGenerate { tab_id: 1, frame_id: "f".into() };
        assert!(Sender::Popup.permits(&gen) && !Sender::Shell.permits(&gen));
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&gen), "{p:?}");
        }
        let undo = Request::PasswordSavedUndo { entry_id: "e".into() };
        let edit = Request::PasswordSavedEdit { entry_id: "e".into() };
        assert!(Sender::Popup.permits(&undo) && Sender::Popup.permits(&edit));
        assert!(!Sender::Shell.permits(&undo) && !Sender::Page(Page::Passwords).permits(&edit));
        let focus = parse_content(r#"{"lb":"fieldFocus","field":"newPassword","rect":{"x":1,"y":2,"width":30,"height":10},"autocomplete":"new-password","passwords":2,"formText":"Sign up"}"#).unwrap();
        match focus {
            ContentMessage::FieldFocus { field, autocomplete, passwords, form_text, .. } => {
                assert_eq!(field, FieldKind::NewPassword);
                assert_eq!((autocomplete.as_str(), passwords, form_text.as_str()), ("new-password", 2, "Sign up"));
            }
            m => panic!("{m:?}"),
        }
        let old =
            parse_content(r#"{"lb":"fieldFocus","field":"password","rect":{"x":1,"y":2,"width":30,"height":10}}"#)
                .unwrap();
        assert!(matches!(old, ContentMessage::FieldFocus { passwords: 0, .. }), "0.5 pages send no extras");
        let toast = PopupContent::PasswordSaved {
            tab_id: 1,
            entry_id: "e1".into(),
            origin: "https://a.example".into(),
            username: "u".into(),
            update: false,
        };
        assert_eq!(toast.kind(), PopupKind::PasswordSaved);
        assert!(serde_json::to_string(&toast).unwrap().contains(r#""entryId":"e1""#));
        let json = serde_json::to_string(&PopupContent::Suggest {
            tab_id: 1,
            frame_id: "f".into(),
            origin: "o".into(),
            locked: false,
            entries: vec![],
            generated: Some("Xy".into()),
        })
        .unwrap();
        assert!(json.contains(r#""generated":"Xy""#));
    }

    #[test]
    fn permission_requests_and_prompt() {
        use crate::permissions::{Kind, PermissionState};
        let list = Request::PermissionExceptions;
        let set = Request::PermissionSet {
            origin: "https://a.example".into(),
            kind: Kind::Geolocation,
            state: PermissionState::Allow,
        };
        assert!(Sender::Page(Page::Settings).permits(&list) && Sender::Page(Page::Settings).permits(&set));
        assert!(Sender::Popup.permits(&set) && !Sender::Popup.permits(&list));
        for r in [
            Request::BounceStatus,
            Request::BounceCleanNow,
            Request::DefaultBrowserStatus,
            Request::MakeDefaultBrowser,
            Request::DnsStatus,
            Request::DnsTest { index: 0 },
            Request::DnsParse { text: "x".into() },
        ] {
            assert!(Sender::Page(Page::Settings).permits(&r), "{r:?}");
            assert!(!Sender::Popup.permits(&r) && !Sender::Shell.permits(&r), "{r:?}");
            for p in Page::ALL.iter().filter(|p| **p != Page::Settings) {
                assert!(!Sender::Page(*p).permits(&r), "{p:?} {r:?}");
            }
        }
        assert!(!Sender::Shell.permits(&set));
        for p in Page::ALL.iter().filter(|p| **p != Page::Settings) {
            assert!(!Sender::Page(*p).permits(&list) && !Sender::Page(*p).permits(&set), "{p:?}");
        }
        let json = serde_json::to_string(&set).unwrap();
        assert!(json.contains("\"kind\":\"geolocation\"") && json.contains("\"state\":\"allow\""), "{json}");
        assert!(matches!(
            parse_content(r#"{"lb":"notificationPermission"}"#),
            Ok(ContentMessage::NotificationPermission {})
        ));
        assert!(parse_content_frame(r#"{"lb":"notificationPermission"}"#).is_err(), "main frame only");
        let c = PopupContent::Permission {
            kinds: vec![Kind::Camera, Kind::Microphone],
            tab_id: 1,
            prompt_id: 2,
            origin: "https://a.example".into(),
            loopback: false,
            private: false,
        };
        assert!(serde_json::to_string(&c).unwrap().contains("\"kinds\":[\"camera\",\"microphone\"]"));
    }

    #[test]
    fn downloads_popup() {
        assert!(PopupKind::Downloads.shell_may_open() && PopupKind::Downloads.takes_focus());
        let c = PopupContent::Downloads { items: vec![] };
        assert_eq!(c.kind(), PopupKind::Downloads);
        assert_eq!(c.toggle_anchor().as_deref(), Some("downloads"));
        let act = Request::DownloadAction { id: 1, action: DownloadAction::Pause };
        assert!(
            Sender::Popup.permits(&act) && Sender::Page(Page::Downloads).permits(&act) && !Sender::Shell.permits(&act)
        );
        assert!(Event::DownloadStarted.to_json().contains("downloadStarted"));
    }

    /// 0.15.0: only krunk://sessions reads or changes the session logs.
    #[test]
    fn session_log_permits() {
        let reqs = [
            Request::SessionList,
            Request::SessionDetail { id: 1 },
            Request::SessionDelete { id: 1 },
            Request::SessionClear,
            Request::SessionLogSet { enabled: Some(false), retention_days: None },
        ];
        for r in &reqs {
            assert!(Sender::Page(Page::Sessions).permits(r), "{r:?}");
            for s in [Sender::Shell, Sender::Popup, Sender::Agent, Sender::Ext, Sender::Runner] {
                assert!(!s.permits(r), "{s:?} {r:?}");
            }
            for p in Page::ALL.iter().filter(|p| **p != Page::Sessions) {
                assert!(!Sender::Page(*p).permits(r), "{p:?} {r:?}");
            }
        }
        assert!(Page::Sessions.agent_forbidden() && Page::Sessions.single_instance());
        let set: Request = serde_json::from_str(r#"{"cmd":"sessionLogSet","retentionDays":7}"#).unwrap();
        assert_eq!(set, Request::SessionLogSet { enabled: None, retention_days: Some(7) });
    }

    #[test]
    fn tasks_caption_and_mute_permits() {
        let get = Request::GetTasks;
        let kill = Request::KillTask { id: 7 };
        assert!(Sender::Page(Page::Tasks).permits(&get) && Sender::Page(Page::Tasks).permits(&kill));
        assert!(!Sender::Shell.permits(&get) && !Sender::Popup.permits(&kill));
        for p in Page::ALL.iter().filter(|p| **p != Page::Tasks) {
            assert!(!Sender::Page(*p).permits(&get) && !Sender::Page(*p).permits(&kill), "{p:?}");
        }
        let mute = Request::TabMute { id: 1, muted: true };
        assert!(Sender::Shell.permits(&mute) && !Sender::Popup.permits(&mute));
        let all = Request::BookmarkAllTabs;
        let ctl = Request::CaptionWindow { action: WindowAction::Minimize };
        assert!(Sender::Popup.permits(&all) && Sender::Popup.permits(&ctl) && !Sender::Shell.permits(&all));
        assert!(
            !Sender::Shell.permits(&ctl)
                && !Sender::Popup.permits(&Request::WindowControl { action: WindowAction::Close })
        );
        for p in Page::ALL {
            assert!(!Sender::Page(p).permits(&mute) && !Sender::Page(p).permits(&all), "{p:?}");
        }
        let c = PopupContent::CaptionMenu { maximized: false, can_reopen: true, can_bookmark_all: true };
        assert_eq!(c.kind(), PopupKind::CaptionMenu);
        assert!(PopupKind::CaptionMenu.takes_focus());
        let info = TabInfo {
            id: 1,
            title: "t".into(),
            url: "u".into(),
            favicon: None,
            loading: false,
            private: false,
            sleeping: false,
            audible: true,
            muted: false,
            auth_pending: false,
        };
        assert!(serde_json::to_string(&info).unwrap().contains("\"audible\":true"));
        let menu = serde_json::to_value(TabAction::MuteSite).unwrap();
        assert_eq!(menu, serde_json::json!("muteSite"));
    }

    #[test]
    fn update_notice_messages() {
        // The ⋮ menu's item asks for the prompt with the existing reply message.
        let show = parse_request(Sender::Popup, r#"{"cmd":"updateReply","choice":"show"}"#).map(|e| e.request);
        assert!(matches!(show, Ok(Request::UpdateReply { choice: UpdateChoice::Show })), "{show:?}");
        let reply = Request::UpdateReply { choice: UpdateChoice::Show };
        assert!(!Sender::Shell.permits(&reply), "the shell only shows the dot");
        for p in Page::ALL {
            assert_eq!(Sender::Page(p).permits(&reply), p == Page::Settings, "{p:?}");
        }
        let ev = Event::UpdateAvailable { version: Some("0.7.9".into()) }.to_json();
        assert!(ev.contains(r#""event":"updateAvailable""#) && ev.contains(r#""version":"0.7.9""#), "{ev}");
        let menu: PopupContent = serde_json::from_str(r#"{"kind":"menu","zoom":1,"canReopen":false}"#).unwrap();
        assert!(matches!(menu, PopupContent::Menu { update: None, .. }), "older menus have no notice");
    }

    #[test]
    fn bar_overflow_permits() {
        let rect = PopupRect { x: 900.0, y: 0.0, width: 24.0, height: 24.0 };
        let overflow = Request::ShowBookmarkFolder { id: 1, rect: Some(rect), from: Some(4) };
        assert!(Sender::Shell.permits(&overflow));
        // The dropdown opens subfolders in place, never an overflow of its own.
        assert!(Sender::Popup.permits(&Request::ShowBookmarkFolder { id: 7, rect: None, from: None }));
        assert!(!Sender::Popup.permits(&Request::ShowBookmarkFolder { id: 1, rect: None, from: Some(4) }));
        // 0.11.0: Back, middle-click and the painted answer; pages get none of them.
        for r in [Request::BookmarkFolderBack, Request::PopupShown { seq: 1, height: None }] {
            assert!(Sender::Popup.permits(&r) && !Sender::Shell.permits(&r), "{r:?}");
        }
        let bg = Request::OpenBookmarkBackground { id: 3 };
        assert!(Sender::Popup.permits(&bg) && Sender::Shell.permits(&bg));
        for p in Page::ALL {
            assert!(!Sender::Page(p).permits(&bg) && !Sender::Page(p).permits(&Request::BookmarkFolderBack), "{p:?}");
        }
        for p in Page::ALL {
            assert!(!Sender::Page(p).permits(&overflow), "{p:?}");
        }
        let parsed = parse_request(
            Sender::Shell,
            r#"{"cmd":"showBookmarkFolder","id":1,"rect":{"x":1,"y":2,"width":3,"height":4},"from":2}"#,
        );
        assert!(matches!(parsed.map(|e| e.request), Ok(Request::ShowBookmarkFolder { from: Some(2), .. })));
        let old = PopupContent::BookmarkFolder { id: 1, title: String::new(), items: vec![], offset: 3, back: None };
        assert!(serde_json::to_string(&old).unwrap().contains(r#""offset":3"#));
    }

    #[test]
    fn address_menu_permits() {
        let show = Request::ShowAddressMenu { x: 1.0, y: 2.0, selection: String::new(), has_text: false, dirty: false };
        let act = Request::AddressMenuAction { action: AddressMenuAction::Copy };
        assert!(Sender::Shell.permits(&show) && !Sender::Shell.permits(&act));
        assert!(Sender::Popup.permits(&act) && !Sender::Popup.permits(&show));
        for p in Page::ALL {
            assert!(!Sender::Page(p).permits(&show) && !Sender::Page(p).permits(&act), "{p:?}");
        }
        assert!(!PopupKind::AddressMenu.takes_focus(), "the address bar keeps the focus and its selection");
        let content = PopupContent::AddressMenu {
            can_undo: false,
            can_cut: true,
            can_copy: true,
            can_paste: true,
            paste_is_url: true,
            can_delete: true,
            can_select_all: true,
        };
        assert_eq!(content.kind(), PopupKind::AddressMenu);
        let ev = Event::AddressEdit { edit: AddressEdit::Insert { text: "x".into() } }.to_json();
        assert!(ev.contains(r#""event":"addressEdit""#) && ev.contains(r#""insert""#), "{ev}");
        let inline = Event::AddressSuggestions {
            text: "exa".into(),
            entries: vec![],
            inline: Some(InlineCompletion { text: "example.com".into(), url: "https://example.com/".into(), index: 1 }),
        }
        .to_json();
        assert!(inline.contains(r#""inline":{"text":"example.com""#), "{inline}");
    }

    #[test]
    fn address_suggestions_are_shell_and_popup_only() {
        let rect = PopupRect { x: 100.0, y: 60.0, width: 500.0, height: 0.0 };
        let ask = Request::AddressSuggest { text: "ru".into(), rect };
        let select = Request::AddressSuggestSelect { selected: None };
        let pick = Request::AddressSuggestPick { url: "https://rust-lang.org/".into() };
        assert!(Sender::Shell.permits(&ask) && Sender::Shell.permits(&select) && Sender::Shell.permits(&pick));
        assert!(Sender::Popup.permits(&pick) && !Sender::Popup.permits(&ask) && !Sender::Popup.permits(&select));
        for p in Page::ALL {
            assert!(
                !Sender::Page(p).permits(&ask) && !Sender::Page(p).permits(&select) && !Sender::Page(p).permits(&pick),
                "{p:?}"
            );
        }
        assert!(!PopupKind::AddressSuggest.takes_focus() && !PopupKind::AddressSuggest.shell_may_open());
        let content = PopupContent::AddressSuggest {
            entries: vec![AddressSuggestion {
                url: "https://duckduckgo.com/?q=ru".into(),
                title: "ru".into(),
                kind: AddressSuggestionKind::Search,
                fill: "ru".into(),
            }],
            selected: Some(0),
        };
        assert_eq!(content.kind(), PopupKind::AddressSuggest);
        let j = serde_json::to_string(&content).unwrap();
        assert!(
            j.contains(r#""kind":"addressSuggest""#)
                && j.contains(r#""kind":"search""#)
                && j.contains(r#""selected":0"#),
            "{j}"
        );
        let parsed = parse_request(
            Sender::Shell,
            r#"{"cmd":"addressSuggest","text":"ru","rect":{"x":1,"y":2,"width":300,"height":0}}"#,
        )
        .unwrap();
        assert!(matches!(parsed.request, Request::AddressSuggest { ref text, .. } if text == "ru"));
        let ev = Event::AddressSuggestions { text: "ru".into(), entries: vec![], inline: None }.to_json();
        assert!(ev.contains(r#""event":"addressSuggestions""#), "{ev}");
        // 0.15.0: the engine's suggestions follow in their own event.
        let row = AddressSuggestion {
            url: "https://duckduckgo.com/?q=why+do+we+yawn".into(),
            title: "why do we yawn".into(),
            kind: AddressSuggestionKind::SearchSuggestion,
            fill: "why do we yawn".into(),
        };
        let ev = Event::AddressSearchSuggestions { text: "why do".into(), entries: vec![row] }.to_json();
        assert!(
            ev.contains(r#""event":"addressSearchSuggestions""#) && ev.contains(r#""kind":"searchSuggestion""#),
            "{ev}"
        );
    }

    #[test]
    fn permission_prompt_is_host_opened_and_answered_by_the_popup() {
        let reply = Request::PermissionReply { prompt_id: 7, allow: false, remember: true };
        assert!(Sender::Popup.permits(&reply));
        assert!(!Sender::Shell.permits(&reply));
        assert!(Page::ALL.iter().all(|p| !Sender::Page(*p).permits(&reply)));
        let remove = Request::SitePermissionRemove { kind: "localNetwork".into() };
        assert!(Sender::Page(Page::SiteData).permits(&remove));
        assert!(!Sender::Popup.permits(&remove) && !Sender::Shell.permits(&remove));
        assert!(Page::ALL.iter().filter(|p| **p != Page::SiteData).all(|p| !Sender::Page(*p).permits(&remove)));
        assert!(!PopupKind::Permission.shell_may_open() && PopupKind::Permission.takes_focus());
        let content = PopupContent::Permission {
            kinds: vec![crate::permissions::Kind::LocalNetwork],
            tab_id: 1,
            prompt_id: 7,
            origin: "https://app.plex.tv".into(),
            loopback: false,
            private: true,
        };
        assert_eq!(content.kind(), PopupKind::Permission);
        let j = serde_json::to_string(&content).unwrap();
        assert!(j.contains(r#""promptId":7"#) && j.contains(r#""private":true"#), "{j}");
        let parsed =
            parse_request(Sender::Popup, r#"{"cmd":"permissionReply","promptId":7,"allow":true,"remember":false}"#)
                .unwrap();
        assert!(matches!(parsed.request, Request::PermissionReply { prompt_id: 7, allow: true, remember: false }));
    }

    #[test]
    fn import_requests_are_settings_page_only() {
        let run = Request::ImportRun {
            profile: "chrome:Default".into(),
            types: vec!["passwords".into()],
            primary_password: None,
        };
        for r in [Request::ImportDetect, run, Request::ImportCsv] {
            assert!(Sender::Page(Page::Settings).permits(&r));
            assert!(!Sender::Shell.permits(&r) && !Sender::Popup.permits(&r));
            for p in Page::ALL.iter().filter(|p| **p != Page::Settings) {
                assert!(!Sender::Page(*p).permits(&r), "{p:?} {r:?}");
            }
        }
        // The primary password never appears in Debug output.
        let parsed = parse_request(
            Sender::Page(Page::Settings),
            r#"{"cmd":"importRun","profile":"firefox:x","types":["passwords"],"primaryPassword":"hunter2"}"#,
        )
        .unwrap();
        assert!(!format!("{:?}", parsed.request).contains("hunter2"));
    }

    #[test]
    fn gpu_page_requests_are_gpu_page_only() {
        for r in [Request::GetGpuInfo, Request::OpenGpuInternals] {
            assert!(Sender::Page(Page::Gpu).permits(&r));
            assert!(!Sender::Shell.permits(&r) && !Sender::Popup.permits(&r));
            for p in Page::ALL.iter().filter(|p| **p != Page::Gpu) {
                assert!(!Sender::Page(*p).permits(&r), "{p:?} {r:?}");
            }
        }
        // Any page may open settings at a section; the section is optional on the wire.
        let open =
            parse_request(Sender::Page(Page::About), r#"{"cmd":"openPage","page":"settings","section":"privacy"}"#)
                .unwrap();
        assert_eq!(open.request, Request::OpenPage { page: "settings".into(), section: Some("privacy".into()) });
        assert!(parse_request(Sender::Page(Page::About), r#"{"cmd":"openPage","page":"gpu"}"#).is_ok());
    }

    #[test]
    fn ntp_image_picker_is_for_the_new_tab_and_settings_pages() {
        let r = Request::NtpPickImage;
        assert!(Sender::Page(Page::NewTab).permits(&r) && Sender::Page(Page::Settings).permits(&r));
        assert!(!Sender::Shell.permits(&r) && !Sender::Popup.permits(&r));
        for p in Page::ALL.iter().filter(|p| !matches!(p, Page::NewTab | Page::Settings)) {
            assert!(!Sender::Page(*p).permits(&r), "{p:?}");
        }
    }

    #[test]
    fn repair_is_for_the_gpu_and_about_pages() {
        let r = Request::RepairEngine;
        assert!(Sender::Page(Page::Gpu).permits(&r) && Sender::Page(Page::About).permits(&r));
        assert!(!Sender::Shell.permits(&r) && !Sender::Popup.permits(&r));
        for p in Page::ALL.iter().filter(|p| !matches!(p, Page::Gpu | Page::About)) {
            assert!(!Sender::Page(*p).permits(&r), "{p:?}");
        }
        // Nothing to choose: the host reinstalls the version its signed manifest names.
        assert_eq!(parse_request(Sender::Page(Page::Gpu), r#"{"cmd":"repairEngine"}"#).unwrap().request, r);
    }

    #[test]
    fn tab_moves_are_shell_and_popup_only() {
        let drag = Request::TabDragOut { id: 1, offset_x: 1.0, offset_y: 1.0, first_x: None };
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
        assert_eq!(parsed.request, Request::TabDragOut { id: 4, offset_x: 12.0, offset_y: 20.0, first_x: None });
        let parsed =
            parse_request(Sender::Shell, r#"{"cmd":"tabDragOut","id":4,"offsetX":12,"offsetY":20,"firstX":6}"#)
                .unwrap();
        assert_eq!(parsed.request, Request::TabDragOut { id: 4, offset_x: 12.0, offset_y: 20.0, first_x: Some(6.0) });
        let parsed =
            parse_request(Sender::Popup, r#"{"cmd":"tabMenuAction","id":4,"action":"moveToNewWindow"}"#).unwrap();
        assert_eq!(parsed.request, Request::TabMenuAction { id: 4, action: TabAction::MoveToNewWindow });
        let parsed = parse_request(Sender::Popup, r#"{"cmd":"command","command":{"command":"newWindow"}}"#).unwrap();
        assert_eq!(parsed.request, Request::Command { command: Command::NewWindow });
        assert_eq!(
            Event::TabDropHint { index: Some(1), ghost: None }.to_json(),
            r#"{"event":"tabDropHint","index":1}"#
        );
        let ghost = TabGhost { id: 7, title: "T".into(), favicon: None, private: false };
        assert_eq!(
            Event::TabDropHint { index: Some(1), ghost: Some(ghost) }.to_json(),
            r#"{"event":"tabDropHint","index":1,"ghost":{"id":7,"title":"T","favicon":null,"private":false}}"#
        );
        let menu = PopupContent::TabMenu {
            tab_id: 2,
            can_move: false,
            can_close_right: true,
            muted_site: false,
            has_site: true,
        };
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

    /// 0.16.0: the Home button. Only the shell asks; a plain click is Alt+Home.
    #[test]
    fn home_requests_are_shell_only() {
        let parsed = parse_request(Sender::Shell, r#"{"cmd":"home"}"#).unwrap();
        assert_eq!(parsed.request, Request::Home { new_tab: false });
        let parsed = parse_request(Sender::Shell, r#"{"cmd":"home","newTab":true}"#).unwrap();
        assert_eq!(parsed.request, Request::Home { new_tab: true });
        assert!(
            parse_request(Sender::Shell, r#"{"cmd":"home","url":"https://evil.example/"}"#).is_err(),
            "the target is the host's setting, never the sender's"
        );
        let home = Request::Home { new_tab: true };
        assert_eq!(Sender::Shell.permits(&home), Sender::Shell.permits(&Request::Reload));
        for s in [Sender::Popup, Sender::Ext, Sender::Agent] {
            assert!(!s.permits(&home), "{s:?}");
            assert!(parse_request(s, r#"{"cmd":"home"}"#).is_err(), "{s:?}");
        }
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&home), "{p:?}");
        }
    }

    /// 0.16.0: the context menu of an extension's toolbar icon, after the tab menu's pattern:
    /// the shell asks for it, only the popup sends a choice, and krunk://extensions keeps its
    /// own requests to itself.
    #[test]
    fn extension_menu_is_shown_by_the_shell_and_answered_by_the_popup() {
        let id = "abcdefghijklmnopabcdefghijklmnop";
        let show =
            Request::ShowExtMenu { id: id.into(), rect: PopupRect { x: 900.0, y: 60.0, width: 260.0, height: 185.0 } };
        let action = Request::ExtMenuAction { id: id.into(), action: ExtMenuAction::Disable };
        assert!(Sender::Shell.permits(&show) && !Sender::Popup.permits(&show));
        assert!(Sender::Popup.permits(&action));
        assert!(!Sender::Shell.permits(&action), "menu choices come from the popup");
        for s in [Sender::Ext, Sender::Agent] {
            assert!(!s.permits(&show) && !s.permits(&action), "{s:?}");
        }
        for p in Page::ALL.iter() {
            assert!(!Sender::Page(*p).permits(&show) && !Sender::Page(*p).permits(&action), "{p:?}");
        }
        // The management requests stay with krunk://extensions: neither the shell nor the popup
        // may send them.
        for r in [
            Request::ExtSet { id: id.into(), enabled: Some(false), private: None },
            Request::ExtRemove { id: id.into() },
            Request::ExtOpenOptions { id: id.into() },
        ] {
            assert!(Sender::Page(Page::Extensions).permits(&r));
            assert!(!Sender::Shell.permits(&r) && !Sender::Popup.permits(&r), "{r:?}");
        }
        let parsed = parse_request(
            Sender::Shell,
            &format!(r#"{{"cmd":"showExtMenu","id":"{id}","rect":{{"x":900,"y":60,"width":260,"height":185}}}}"#),
        )
        .unwrap();
        assert_eq!(parsed.request, show);
        for (name, action) in [
            ("details", ExtMenuAction::Details),
            ("options", ExtMenuAction::Options),
            ("manage", ExtMenuAction::Manage),
            ("disable", ExtMenuAction::Disable),
            ("uninstall", ExtMenuAction::Uninstall),
        ] {
            let json = format!(r#"{{"cmd":"extMenuAction","id":"{id}","action":"{name}"}}"#);
            assert_eq!(
                parse_request(Sender::Popup, &json).unwrap().request,
                Request::ExtMenuAction { id: id.into(), action }
            );
            assert!(parse_request(Sender::Shell, &json).is_err(), "{name}");
            assert!(parse_request(Sender::Page(Page::Extensions), &json).is_err(), "{name}");
        }
        assert!(
            parse_request(Sender::Popup, &format!(r#"{{"cmd":"extMenuAction","id":"{id}","action":"enable"}}"#))
                .is_err(),
            "nothing but the menu's items"
        );
        let menu =
            PopupContent::ExtMenu { id: id.into(), name: "Dark Reader".into(), has_options: true, enabled: true };
        assert_eq!(menu.kind(), PopupKind::ExtMenu);
        assert_eq!(menu.toggle_anchor(), None);
        let json = serde_json::to_string(&menu).unwrap();
        assert!(
            json.contains(r#""kind":"extMenu""#) && json.contains(r#""hasOptions":true"#) && json.contains(id),
            "{json}"
        );
        assert!(!PopupKind::ExtMenu.shell_may_open(), "the shell asks with showExtMenu, not showPopup");
        assert!(PopupKind::ExtMenu.takes_focus(), "Esc and Tab work in it");
        assert_eq!(PopupKind::ExtMenu.anchor(), crate::layout::PopupAnchor::Point, "a context menu: at the click");
    }

    #[test]
    fn sync_requests_come_from_krunk_sync_and_settings_only_asks_or_opens() {
        let all = [
            Request::SyncState,
            Request::SyncOpen,
            Request::SyncTurnOn { passwords: true },
            Request::SyncSetupConfirm { group: "ABCD".into(), understood: true },
            Request::SyncSetupCopy,
            Request::SyncSetupSave,
            Request::SyncRestore { secret: Secret::from("words"), recovery: false, vault: false },
            Request::SyncSetupVault { password: Secret::from("vault pw") },
            Request::SyncRestoreConfirm,
            Request::SyncConfirmRotation,
            Request::SyncFinishSecuring { secret: Secret::from("words"), recovery: false },
            Request::SyncJoin { passwords: false },
            Request::SyncJoinCode { code: "123456".into() },
            Request::SyncCancel,
            Request::SyncApprove { id: "a1".into(), share_passwords: false },
            Request::SyncApproveCancel,
            Request::SyncRemoveDevice { id: "d1".into() },
            Request::SyncTurnOff { delete_data: false },
            Request::SyncNow,
            Request::SyncSetCollection { name: "history".into(), on: false },
            Request::SyncRenameDevice { id: "d1".into(), name: "Laptop".into() },
        ];
        for r in &all {
            assert_eq!(Sender::Page(Page::Sync).permits(r), !matches!(r, Request::SyncOpen), "{r:?}");
            assert!(!Sender::Shell.permits(r) && !Sender::Popup.permits(r) && !Sender::Ext.permits(r), "{r:?}");
            let settings = matches!(
                r,
                Request::SyncState | Request::SyncOpen | Request::SyncNow | Request::SyncSetCollection { .. }
            );
            assert_eq!(Sender::Page(Page::Settings).permits(r), settings, "{r:?}");
            for page in Page::ALL.into_iter().filter(|p| *p != Page::Sync && *p != Page::Settings) {
                assert!(!Sender::Page(page).permits(r), "{page:?} {r:?}");
            }
        }
        // krunk://sync can ask for nothing else of the vault than its status.
        assert!(!Sender::Page(Page::Sync).permits(&Request::VaultList));
        assert!(
            !Sender::Page(Page::Sync).permits(&Request::VaultReveal { id: "x".into(), password: Secret::default() })
        );
        // The passphrase is a Secret: never in a debug line.
        let r = parse_request(
            Sender::Page(Page::Sync),
            r#"{"cmd":"syncRestore","secret":"apple-banana","recovery":false}"#,
        )
        .unwrap();
        assert!(!format!("{:?}", r.request).contains("apple"));
        assert!(parse_request(Sender::Page(Page::Sync), r#"{"cmd":"syncRestore","secret":"x","extra":1}"#).is_err());
        let v = crate::sync::view::SyncView::Off { message: None };
        assert_eq!(Event::Sync { view: v }.to_json(), r#"{"event":"sync","view":{"state":"off","message":null}}"#);
    }

    #[test]
    fn the_account_is_settings_only() {
        let all = [
            Request::AccountState { check: false },
            Request::AccountSignIn,
            Request::AccountCancel,
            Request::AccountSignOut,
            Request::AccountManage,
            Request::AccountAddDevice,
            Request::AccountReopen,
        ];
        for r in &all {
            assert!(Sender::Page(Page::Settings).permits(r), "{r:?}");
            assert!(!Sender::Shell.permits(r) && !Sender::Popup.permits(r) && !Sender::Ext.permits(r), "{r:?}");
            for page in Page::ALL.into_iter().filter(|p| *p != Page::Settings) {
                assert!(!Sender::Page(page).permits(r), "{page:?} {r:?}");
            }
        }
        let parsed = parse_request(Sender::Page(Page::Settings), r#"{"cmd":"accountState","check":true}"#).unwrap();
        assert_eq!(parsed.request, Request::AccountState { check: true });
        let parsed = parse_request(Sender::Page(Page::Settings), r#"{"cmd":"accountState"}"#).unwrap();
        assert_eq!(parsed.request, Request::AccountState { check: false });
        // 0.13.0: Add a device, from Settings only.
        let parsed = parse_request(Sender::Page(Page::Settings), r#"{"cmd":"accountAddDevice"}"#).unwrap();
        assert_eq!(parsed.request, Request::AccountAddDevice);
        assert!(parse_request(Sender::Shell, r#"{"cmd":"accountAddDevice"}"#).is_err());
        assert!(parse_request(Sender::Page(Page::Sync), r#"{"cmd":"accountAddDevice"}"#).is_err());
        assert!(
            parse_request(Sender::Page(Page::Settings), r#"{"cmd":"accountState","check":true,"token":"x"}"#).is_err()
        );
        let card = crate::account::CardState::SignedIn {
            email: "a@b.c".into(),
            device_name: "PC".into(),
            version: "0.11.0".into(),
            message: None,
        };
        let json = Event::Account { card }.to_json();
        assert_eq!(
            json,
            r#"{"event":"account","card":{"state":"signedIn","email":"a@b.c","deviceName":"PC","version":"0.11.0"}}"#
        );
        let menu = PopupContent::Menu {
            zoom: 1.0,
            can_reopen: false,
            has_video: false,
            update: None,
            account: Some(String::new()),
        };
        let menu_json = serde_json::to_value(&menu).unwrap();
        assert_eq!(menu_json["account"], "");
    }

    #[test]
    fn http_auth_answers_come_from_the_popup_only() {
        let reply = Request::HttpAuthReply { tab_id: 1, username: "u".into(), password: "p".into(), entry_id: None };
        let cancel = Request::HttpAuthCancel { tab_id: 1 };
        let unlock = Request::HttpAuthUnlock { tab_id: 1 };
        for r in [&reply, &cancel, &unlock] {
            assert!(Sender::Popup.permits(r), "{r:?}");
            assert!(!Sender::Shell.permits(r) && !Sender::Ext.permits(r), "{r:?}");
            for page in Page::ALL {
                assert!(!Sender::Page(page).permits(r), "{page:?} {r:?}");
            }
        }
        assert!(!PopupKind::HttpAuth.shell_may_open() && PopupKind::HttpAuth.takes_focus());
        let parsed =
            parse_request(Sender::Popup, r#"{"cmd":"httpAuthReply","tabId":4,"username":"tester","password":"pw"}"#)
                .unwrap();
        assert_eq!(
            parsed.request,
            Request::HttpAuthReply { tab_id: 4, username: "tester".into(), password: "pw".into(), entry_id: None }
        );
        assert!(!format!("{:?}", parsed.request).contains("pw\""), "the password is never printed");
        let parsed = parse_request(Sender::Popup, r#"{"cmd":"httpAuthReply","tabId":4,"entryId":"e9"}"#).unwrap();
        assert_eq!(
            parsed.request,
            Request::HttpAuthReply {
                tab_id: 4,
                username: String::new(),
                password: Secret::default(),
                entry_id: Some("e9".into())
            }
        );
        let c = PopupContent::HttpAuth {
            tab_id: 4,
            origin: "http://localhost:8094".into(),
            realm: "Krunk test area".into(),
            insecure: true,
            logins: vec![AuthLogin { entry_id: "e9".into(), username: "tester".into() }],
            locked: false,
            failed: false,
        };
        assert_eq!(c.kind(), PopupKind::HttpAuth);
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kind"], "httpAuth");
        assert_eq!(v["logins"][0]["entryId"], "e9");
    }

    #[test]
    fn history_menus_are_shell_and_popup_only() {
        let show =
            Request::ShowNavMenu { forward: false, rect: PopupRect { x: 4.0, y: 70.0, width: 320.0, height: 300.0 } };
        let jump = Request::NavToEntry { tab_id: 2, entry_id: 5 };
        assert!(Sender::Shell.permits(&show) && !Sender::Shell.permits(&jump), "the shell asks, the popup answers");
        assert!(Sender::Popup.permits(&jump) && !Sender::Popup.permits(&show));
        for page in Page::ALL {
            assert!(!Sender::Page(page).permits(&show) && !Sender::Page(page).permits(&jump), "{page:?}");
        }
        assert!(!Sender::Ext.permits(&show) && !Sender::Ext.permits(&jump));
        let parsed = parse_request(
            Sender::Shell,
            r#"{"cmd":"showNavMenu","forward":true,"rect":{"x":40,"y":70,"width":320,"height":300}}"#,
        )
        .unwrap();
        assert_eq!(
            parsed.request,
            Request::ShowNavMenu { forward: true, rect: PopupRect { x: 40.0, y: 70.0, width: 320.0, height: 300.0 } }
        );
        let parsed = parse_request(Sender::Popup, r#"{"cmd":"navToEntry","tabId":2,"entryId":9}"#).unwrap();
        assert_eq!(parsed.request, Request::NavToEntry { tab_id: 2, entry_id: 9 });
        // 0.18.0: middle-click opens the entry in a new tab; the popup only, like the jump.
        let clone = parse_request(Sender::Popup, r#"{"cmd":"navToEntryNewTab","tabId":2,"entryId":9}"#).unwrap();
        assert_eq!(clone.request, Request::NavToEntryNewTab { tab_id: 2, entry_id: 9 });
        assert!(!Sender::Shell.permits(&clone.request) && !Sender::Ext.permits(&clone.request));
        for page in Page::ALL {
            assert!(!Sender::Page(page).permits(&clone.request), "{page:?}");
        }
        assert!(parse_request(Sender::Popup, r#"{"cmd":"navToEntryNewTab","tabId":2,"entryId":9,"url":"x"}"#).is_err());
        assert!(parse_request(Sender::Popup, r#"{"cmd":"navToEntry","tabId":2,"entryId":9,"url":"x"}"#).is_err());
        let menu = PopupContent::NavMenu {
            tab_id: 2,
            forward: false,
            items: vec![crate::navmenu::Item { entry_id: 9, label: "P1".into(), url: "http://a/".into(), icon: None }],
        };
        assert_eq!(menu.kind(), PopupKind::NavMenu);
        assert!(PopupKind::NavMenu.takes_focus(), "arrows and Enter work in it");
        assert!(!PopupKind::NavMenu.shell_may_open(), "the shell asks with showNavMenu, not showPopup");
        let json = serde_json::to_string(&menu).unwrap();
        assert!(json.contains(r#""kind":"navMenu""#) && json.contains(r#""entryId":9"#), "{json}");
    }

    #[test]
    fn events_round_trip_and_shape() {
        let j = Event::ok(Some(3), vec![1, 2]).to_json();
        assert_eq!(j, r#"{"event":"reply","reqId":3,"ok":true,"data":[1,2]}"#);
        assert_eq!(Event::TabActive { active: 7 }.to_json(), r#"{"event":"tabActive","active":7}"#);
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
                    audible: false,
                    muted: false,
                    auth_pending: false,
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
                    login_offer: false,
                    bookmarks_bar: true,
                    camera: false,
                    microphone: false,
                    videos: 0,
                    pip: false,
                },
            },
            Event::ShellConfig {
                theme: Theme::Dark,
                search_name: "DuckDuckGo".into(),
                tab_hover_preview: true,
                appearance: Default::default(),
                colors: Default::default(),
            },
            Event::Popup {
                seq: None,
                content: PopupContent::Menu {
                    zoom: 1.0,
                    can_reopen: true,
                    has_video: false,
                    update: None,
                    account: None,
                },
            },
            Event::Popup {
                seq: None,
                content: PopupContent::Menu {
                    zoom: 1.0,
                    can_reopen: false,
                    has_video: false,
                    update: Some("0.7.9".into()),
                    account: Some("a@b.c".into()),
                },
            },
            Event::UpdateAvailable { version: Some("0.7.9".into()) },
            Event::UpdateAvailable { version: None },
            Event::Popup {
                seq: None,
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
                seq: None,
                content: PopupContent::SavePassword {
                    tab_id: 2,
                    origin: "https://a.example".into(),
                    username: "u".into(),
                    update: false,
                },
            },
            Event::Popup {
                seq: None,
                content: PopupContent::Fill {
                    tab_id: 2,
                    origin: "https://a.example".into(),
                    entries: vec![FillChoice { id: "e1".into(), username: "u".into() }],
                },
            },
            Event::PopupClosed,
            Event::TabDropHint { index: Some(2), ghost: None },
            Event::TabDropHint {
                index: Some(0),
                ghost: Some(TabGhost { id: 3, title: "t".into(), favicon: Some("data:,".into()), private: true }),
            },
            Event::BarDragAt { x: 1.0, y: 2.0 },
            Event::BarDragDrop { id: 4, x: 1.0, y: 2.0 },
            Event::BarDragEnd,
            Event::TabDropHint { index: None, ghost: None },
            Event::DevTools { open: true, dock: crate::settings::DevToolsDock::Right },
            Event::CaptionState { hover: Some(CaptionButton::Maximize), pressed: false },
            Event::CaptionState { hover: None, pressed: false },
            Event::WindowState { maximized: true, active: false, fullscreen: false },
            Event::Popup {
                seq: None,
                content: PopupContent::Unlock {
                    tab_id: 1,
                    origin: "https://a.example".into(),
                    purpose: UnlockPurpose::Fill,
                    error: Some("Wrong master password".into()),
                    wait_ms: 2000,
                    remain: false,
                },
            },
            Event::Popup {
                seq: None,
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
                    offset: 0,
                    back: Some("Bookmarks bar".into()),
                },
            },
            Event::Popup {
                seq: Some(7),
                content: PopupContent::BookmarkFolder {
                    id: 1,
                    title: "Bookmarks bar".into(),
                    items: vec![],
                    offset: 2,
                    back: None,
                },
            },
            Event::PopupClear,
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
                    speed: 0,
                    eta: None,
                    part_progress: Vec::new(),
                    resume: None,
                }],
            },
            Event::VaultState { exists: true, locked: false },
            Event::DownloadsChanged { active: 1 },
            Event::Theme { theme: Theme::Light, accent: String::new(), ui_font_px: 0, colors: Default::default() },
            Event::FocusAddressBar,
            Event::AddressEdit { edit: AddressEdit::Submit },
            Event::AddressEdit { edit: AddressEdit::Backspace },
            Event::BlurAddressBar,
            Event::AddressNavigated,
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
            seq: None,
            content: PopupContent::SavePassword { tab_id: 1, origin: "o".into(), username: "u".into(), update: true },
        }
        .to_json();
        assert!(!j.contains("password\":"), "{j}");
    }
    #[test]
    fn certificate_exception_commands_only_from_their_pages() {
        let sha = "ab".repeat(32);
        let proceed =
            format!(r#"{{"cmd":"certProceed","url":"https://a.example/","sha256":"{sha}","remember":false}}"#);
        let remove = format!(r#"{{"cmd":"certExceptionRemove","host":"a.example","port":443,"sha256":"{sha}"}}"#);
        let page = Sender::Page(Page::CertError);
        let settings = Sender::Page(Page::Settings);
        assert!(parse_request(page, &proceed).is_ok());
        assert!(parse_request(page, r#"{"cmd":"certBack"}"#).is_ok());
        assert!(parse_request(settings, r#"{"cmd":"certExceptions"}"#).is_ok());
        assert!(parse_request(settings, &remove).is_ok());
        // The interstitial can't read or change the list; Settings can't continue past a certificate.
        assert_eq!(parse_request(page, r#"{"cmd":"certExceptions"}"#), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(page, &remove), Err(ProtocolError::Forbidden));
        assert_eq!(parse_request(settings, &proceed), Err(ProtocolError::Forbidden));
        for s in [
            Sender::Shell,
            Sender::Popup,
            Sender::Ext,
            Sender::Agent,
            Sender::Runner,
            Sender::Page(Page::HttpsOnly),
            Sender::Page(Page::NewTab),
        ] {
            for json in [proceed.as_str(), remove.as_str(), r#"{"cmd":"certExceptions"}"#, r#"{"cmd":"certBack"}"#] {
                assert_eq!(parse_request(s, json), Err(ProtocolError::Forbidden), "{s:?} {json}");
            }
        }
        // Web content never gets them.
        assert!(parse_content(&proceed).is_err() && parse_content(&remove).is_err());
        // The page's fields are exactly these.
        assert!(parse_request(
            page,
            r#"{"cmd":"certProceed","url":"https://a.example/","sha256":"x","remember":true,"host":"b"}"#
        )
        .is_err());
    }
}
