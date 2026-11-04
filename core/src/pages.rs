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
}

impl Page {
    pub const ALL: [Page; 10] = [
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
        }
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
        let lower = input.trim().to_ascii_lowercase();
        let rest = lower.strip_prefix(DISPLAY_SCHEME).or_else(|| lower.strip_prefix(LEGACY_DISPLAY_SCHEME))?;
        Page::from_name(rest.trim_end_matches('/'))
    }

    /// Identifies a built-in page from the URL a tab reports. Only the exact
    /// pages origin counts; query strings and fragments are allowed.
    pub fn from_url(url: &str) -> Option<Page> {
        let u = Url::parse(url).ok()?;
        if u.origin().ascii_serialization() != PAGES_ORIGIN {
            return None;
        }
        let file = u.path().strip_prefix('/')?;
        Page::from_name(file.strip_suffix(".html")?)
    }
}

/// Address-bar text for a URL: built-in pages get their `krunk://` name.
pub fn display_url(url: &str) -> String {
    match Page::from_url(url) {
        Some(p) => p.display(),
        None => url.to_string(),
    }
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
    }
}
