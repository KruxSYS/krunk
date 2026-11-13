//! Internal origins and pages.
//!
//! The browser chrome and the built-in pages are served from embedded assets
//! under hosts in the reserved `.invalid` TLD, so a request can never reach
//! the network even if interception failed. The UI shell and the built-in
//! pages live on *different* origins: tabs may show built-in pages, but must
//! never be able to talk to the host with the shell's privileges.

use url::Url;

/// Origin of the browser chrome (tab strip, toolbar, popups).
pub const SHELL_ORIGIN: &str = "https://shell.krunk.invalid";
/// Origin of built-in pages shown inside tabs (new tab, settings, ...).
pub const PAGES_ORIGIN: &str = "https://krunk.invalid";
/// Scheme shown to the user in the address bar for built-in pages.
pub const DISPLAY_SCHEME: &str = "krunk://";
/// The scheme and origin used before the rename to Krunk. Typed `lb://` names
/// and stored URLs on the old origin are redirected to the Krunk pages.
pub const LEGACY_DISPLAY_SCHEME: &str = "lb://";
const LEGACY_PAGES_ORIGIN: &str = "https://lightbrowser.invalid";

/// A deep-link section name: a short lower-case word.
pub fn valid_section(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A built-in page shown inside a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    NewTab,
    History,
    Bookmarks,
    Passwords,
    Settings,
    Downloads,
    About,
    /// Shown when HTTPS-only mode could not upgrade a site.
    HttpsOnly,
    /// Cookies and site data of all sites.
    Storage,
    /// Cookies and storage of one site, opened from that site's tab.
    SiteData,
    /// The ad blocker: what it blocked on a tab's page, its filter lists.
    Adblock,
    /// Graphics: the renderer, whether it is hardware accelerated, the GPU switches.
    Gpu,
    /// The task manager: the engine's processes and what they cost (Krunk 0.6.0).
    Tasks,
    /// Session logs (0.15.0): sites, time in front and CPU use per browser run.
    Sessions,
    /// Installed extensions (Krunk 0.6.0).
    Extensions,
    /// Sync (beta, Krunk 0.9.0): setup, restore, approvals, devices.
    Sync,
    /// A page's source, formatted (0.11.0; "View page source", Ctrl+U, `view-source:`).
    Source,
    /// A tab whose page crashed or was ended ("Close page" on a page that stopped
    /// responding, 0.11.0): what happened, and Reload.
    Crashed,
    /// Userscripts and macros with their editor (0.12.0).
    Scripts,
    /// The WASM memory inspector (0.19.0), opened from a tab's ⋮ menu and bound to that tab.
    Wasm,
}

impl Page {
    pub const ALL: [Page; 20] = [
        Page::NewTab,
        Page::History,
        Page::Bookmarks,
        Page::Passwords,
        Page::Settings,
        Page::Downloads,
        Page::About,
        Page::HttpsOnly,
        Page::Storage,
        Page::SiteData,
        Page::Adblock,
        Page::Gpu,
        Page::Tasks,
        Page::Sessions,
        Page::Extensions,
        Page::Sync,
        Page::Source,
        Page::Crashed,
        Page::Scripts,
        Page::Wasm,
    ];

    /// Short name used in the display URL (`krunk://<name>`) and file name.
    pub fn name(self) -> &'static str {
        match self {
            Page::NewTab => "newtab",
            Page::History => "history",
            Page::Bookmarks => "bookmarks",
            Page::Passwords => "passwords",
            Page::Settings => "settings",
            Page::Downloads => "downloads",
            Page::About => "about",
            Page::HttpsOnly => "https-only",
            Page::Storage => "storage",
            Page::SiteData => "sitedata",
            Page::Adblock => "adblock",
            Page::Gpu => "gpu",
            Page::Tasks => "tasks",
            Page::Sessions => "sessions",
            Page::Extensions => "extensions",
            Page::Sync => "sync",
            Page::Source => "source",
            Page::Crashed => "crashed",
            Page::Scripts => "scripts",
            Page::Wasm => "wasm",
        }
    }

    /// Pages with secrets or privileged data the AI agent may never open (0.12.0): the vault and
    /// payment methods, sync, and the cookie and storage pages (they would bypass the agent's
    /// sensitive-data gate). 0.15.0: the session logs (a record of the user's browsing).
    /// 0.19.0: the WASM inspector (the agent has its own gated `wasm_*` tools).
    pub fn agent_forbidden(self) -> bool {
        matches!(self, Page::Passwords | Page::Sync | Page::SiteData | Page::Storage | Page::Sessions | Page::Wasm)
    }

    /// The real URL loaded in the tab.
    pub fn url(self) -> String {
        format!("{PAGES_ORIGIN}/{}.html", self.name())
    }

    /// What the address bar shows. The new-tab page shows an empty bar.
    pub fn display(self) -> String {
        match self {
            Page::NewTab => String::new(),
            p => format!("{DISPLAY_SCHEME}{}", p.name()),
        }
    }

    pub fn from_name(name: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Parses `krunk://settings` (case-insensitive, optional trailing slash).
    /// The old `lb://settings` form is accepted too, so it redirects.
    pub fn from_display(input: &str) -> Option<Page> {
        Page::from_display_with_section(input).map(|(p, _)| p)
    }

    /// `krunk://settings#privacy`: the page and the section (a deep link to a settings tab).
    /// The section is kept only when it is a plain lower-case word (`[a-z0-9-]`, 32 at most).
    pub fn from_display_with_section(input: &str) -> Option<(Page, Option<String>)> {
        let lower = input.trim().to_ascii_lowercase();
        let rest = lower.strip_prefix(DISPLAY_SCHEME).or_else(|| lower.strip_prefix(LEGACY_DISPLAY_SCHEME))?;
        let (name, section) = match rest.split_once('#') {
            Some((n, s)) => (n, Some(s)),
            None => (rest, None),
        };
        let page = Page::from_name(name.trim_end_matches('/'))?;
        Some((page, section.filter(|s| valid_section(s)).map(str::to_string)))
    }

    /// The real URL of a page with a section (`…/settings.html#privacy`).
    pub fn url_with_section(self, section: Option<&str>) -> String {
        match section.filter(|s| valid_section(s)) {
            Some(s) => format!("{}#{s}", self.url()),
            None => self.url(),
        }
    }

    /// Identifies a built-in page from the URL a tab reports. Only the exact
    /// pages origin counts; query strings and fragments are allowed.
    /// Pages that are only ever needed once: opening one again switches to the open tab
    /// (Chrome does the same for its settings, downloads and history pages).
    pub fn single_instance(self) -> bool {
        !matches!(self, Page::NewTab | Page::HttpsOnly | Page::SiteData | Page::Source | Page::Crashed | Page::Wasm)
    }

    pub fn from_url(url: &str) -> Option<Page> {
        let u = Url::parse(url).ok()?;
        if u.origin().ascii_serialization() != PAGES_ORIGIN {
            return None;
        }
        let file = u.path().strip_prefix('/')?;
        Page::from_name(file.strip_suffix(".html")?)
    }
}

/// Address-bar text for a URL: built-in pages get their `krunk://` name; an empty page
/// (`about:blank`, a new tab with the Blank setting) shows the empty bar with its placeholder
/// (0.7.2, R1).
pub fn display_url(url: &str) -> String {
    if let Some((target, _)) = source_target(url) {
        return format!("{VIEW_SOURCE}{target}");
    }
    // A crashed page keeps its address in the bar (Reload goes back there).
    if let Some(target) = crashed_target(url) {
        return target;
    }
    match Page::from_url(url) {
        Some(p) => p.display(),
        None if is_blank(url) => String::new(),
        None => url.to_string(),
    }
}

/// The address-bar form of a page's source (as in Chrome).
pub const VIEW_SOURCE: &str = "view-source:";

/// The source viewer for `target` (an http(s) address). `from`: the tab that shows it, whose
/// own copy is the fallback when the address cannot be loaded again (a POST result).
pub fn source_url(target: &str, from: Option<u32>) -> String {
    let mut u = Url::parse(&Page::Source.url()).expect("pages URL");
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("url", target);
        if let Some(t) = from {
            q.append_pair("tab", &t.to_string());
        }
    }
    u.into()
}

/// The address a source-viewer URL shows (http or https only) and its tab.
pub fn source_target(url: &str) -> Option<(String, Option<u32>)> {
    if Page::from_url(url) != Some(Page::Source) {
        return None;
    }
    let u = Url::parse(url).ok()?;
    let target = u.query_pairs().find(|(k, _)| k == "url").map(|(_, v)| v.into_owned())?;
    let t = Url::parse(&target).ok().filter(|t| matches!(t.scheme(), "http" | "https"))?;
    let tab = u.query_pairs().find(|(k, _)| k == "tab").and_then(|(_, v)| v.parse().ok());
    Some((t.into(), tab))
}

/// The crash page for a tab that showed `target` (0.11.0).
pub fn crashed_url(target: &str) -> String {
    let mut u = Url::parse(&Page::Crashed.url()).expect("pages URL");
    u.query_pairs_mut().append_pair("url", target);
    u.into()
}

/// The address a crash page offers to reload (http or https only).
pub fn crashed_target(url: &str) -> Option<String> {
    if Page::from_url(url) != Some(Page::Crashed) {
        return None;
    }
    let u = Url::parse(url).ok()?;
    let target = u.query_pairs().find(|(k, _)| k == "url").map(|(_, v)| v.into_owned())?;
    Url::parse(&target).ok().filter(|t| matches!(t.scheme(), "http" | "https")).map(String::from)
}

/// `about:blank` (what a new tab shows before its first page, or with the Blank setting).
pub fn is_blank(url: &str) -> bool {
    url.eq_ignore_ascii_case("about:blank")
}

/// Rewrites a URL stored before the rename (session tabs, bookmarks) to its
/// Krunk form: `lb://x` and `https://lightbrowser.invalid/x.html?q#f` become
/// the matching built-in page; anything else is returned unchanged.
pub fn migrate_legacy_url(url: &str) -> String {
    if let Some(p) =
        Page::from_display(url).filter(|_| url.trim().to_ascii_lowercase().starts_with(LEGACY_DISPLAY_SCHEME))
    {
        return p.url();
    }
    if let Ok(mut u) = Url::parse(url) {
        if u.origin().ascii_serialization() == LEGACY_PAGES_ORIGIN {
            let file = u.path().trim_start_matches('/').to_string();
            if file.strip_suffix(".html").and_then(Page::from_name).is_some() {
                u.set_host(Some(PAGES_ORIGIN.trim_start_matches("https://"))).ok();
                return u.to_string();
            }
        }
    }
    url.to_string()
}

/// True if `uri` is exactly on the shell origin.
pub fn is_shell_origin(uri: &str) -> bool {
    origin_is(uri, SHELL_ORIGIN)
}

/// True if `uri` is exactly on the built-in pages origin.
pub fn is_pages_origin(uri: &str) -> bool {
    origin_is(uri, PAGES_ORIGIN)
}

fn origin_is(uri: &str, origin: &str) -> bool {
    Url::parse(uri).is_ok_and(|u| u.origin().ascii_serialization() == origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_urls_with_sections() {
        assert_eq!(
            Page::from_display_with_section("krunk://settings#privacy"),
            Some((Page::Settings, Some("privacy".into())))
        );
        assert_eq!(
            Page::from_display_with_section("KRUNK://Settings/#Advanced"),
            Some((Page::Settings, Some("advanced".into())))
        );
        assert_eq!(Page::from_display_with_section("krunk://settings"), Some((Page::Settings, None)));
        assert_eq!(
            Page::from_display_with_section("krunk://settings#<script>"),
            Some((Page::Settings, None)),
            "odd sections are dropped"
        );
        assert_eq!(Page::from_display_with_section("krunk://nope#x"), None);
        assert_eq!(Page::Settings.url_with_section(Some("privacy")), "https://krunk.invalid/settings.html#privacy");
        assert_eq!(Page::Settings.url_with_section(Some("Bad Section")), Page::Settings.url());
        assert_eq!(Page::Gpu.name(), "gpu");
        assert!(Page::Gpu.single_instance());
        assert_eq!(Page::from_display("krunk://gpu"), Some(Page::Gpu));
    }

    /// 0.16.0: `krunk://extensions#<id>` shows one extension (the name in the menu of its
    /// toolbar icon). An extension id is as long as a section may be, and all of it is kept.
    #[test]
    fn an_extension_id_is_a_section_of_the_extensions_page() {
        let id = "abcdefghijklmnopabcdefghijklmnop";
        assert!(crate::ext::valid_id(id) && valid_section(id));
        assert_eq!(Page::Extensions.url_with_section(Some(id)), format!("https://krunk.invalid/extensions.html#{id}"));
        assert_eq!(
            Page::from_display_with_section(&format!("krunk://extensions#{id}")),
            Some((Page::Extensions, Some(id.to_string())))
        );
        assert!(Page::Extensions.single_instance(), "an open extensions page is switched to and sent there");
        assert_eq!(Page::from_url(&Page::Extensions.url_with_section(Some(id))), Some(Page::Extensions));
    }

    #[test]
    fn round_trips() {
        for p in Page::ALL {
            assert_eq!(Page::from_url(&p.url()), Some(p));
            if p != Page::NewTab {
                assert_eq!(Page::from_display(&p.display()), Some(p));
            }
        }
        assert_eq!(Page::from_display("KRUNK://History/"), Some(Page::History));
        assert_eq!(Page::from_display("lb://settings"), Some(Page::Settings), "old scheme redirects");
        assert_eq!(Page::from_display("LB://History/"), Some(Page::History));
        assert_eq!(Page::from_display("lb://nope"), None);
    }

    #[test]
    fn legacy_urls_migrate() {
        assert_eq!(migrate_legacy_url("lb://history"), Page::History.url());
        assert_eq!(migrate_legacy_url("https://lightbrowser.invalid/settings.html"), Page::Settings.url());
        assert_eq!(
            migrate_legacy_url("https://lightbrowser.invalid/history.html?q=x#y"),
            "https://krunk.invalid/history.html?q=x#y"
        );
        // Unknown files, other hosts, look-alikes and web URLs stay as they are.
        for keep in [
            "https://lightbrowser.invalid/evil.html",
            "https://shell.lightbrowser.invalid/shell.html",
            "https://lightbrowser.invalid.evil.com/settings.html",
            "http://lightbrowser.invalid/settings.html",
            "https://example.com/lb://x",
            "krunk://settings",
            "https://example.com/",
        ] {
            assert_eq!(migrate_legacy_url(keep), keep);
        }
    }

    #[test]
    fn lookalike_origins_rejected() {
        assert_eq!(Page::from_url("https://krunk.invalid.evil.com/settings.html"), None);
        assert_eq!(Page::from_url("http://krunk.invalid/settings.html"), None);
        assert_eq!(Page::from_url("https://krunk.invalid:8443/settings.html"), None);
        assert_eq!(Page::from_url("https://krunk.invalid/evil.html"), None);
        assert!(!is_shell_origin("https://shell.krunk.invalid.attacker.net/"));
        assert!(!is_shell_origin(PAGES_ORIGIN));
        assert!(!is_pages_origin(SHELL_ORIGIN));
        assert!(is_shell_origin("https://shell.krunk.invalid/shell.html"));
    }

    #[test]
    fn display() {
        assert_eq!(display_url(&Page::NewTab.url()), "");
        assert_eq!(display_url(&Page::Settings.url()), "krunk://settings");
        assert_eq!(display_url("https://example.com/"), "https://example.com/");
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("about:blank#x"), "about:blank#x");
    }
}
