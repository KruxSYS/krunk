//! Internal origins and pages.
//!
//! The browser chrome and the built-in pages are served from embedded assets
//! under hosts in the reserved `.invalid` TLD, so a request can never reach
//! the network even if interception failed. The UI shell and the built-in
//! pages live on *different* origins: tabs may show built-in pages, but must
//! never be able to talk to the host with the shell's privileges.

use url::Url;

/// Origin of the browser chrome (tab strip, toolbar, popups).
pub const SHELL_ORIGIN: &str = "https://shell.lightbrowser.invalid";
/// Origin of built-in pages shown inside tabs (new tab, settings, ...).
pub const PAGES_ORIGIN: &str = "https://lightbrowser.invalid";
/// Scheme shown to the user in the address bar for built-in pages.
pub const DISPLAY_SCHEME: &str = "lb://";

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
}

impl Page {
    pub const ALL: [Page; 8] = [
        Page::NewTab,
        Page::History,
        Page::Bookmarks,
        Page::Passwords,
        Page::Settings,
        Page::Downloads,
        Page::About,
        Page::HttpsOnly,
    ];

    /// Short name used in the display URL (`lb://<name>`) and file name.
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

    /// Parses `lb://settings` (case-insensitive, optional trailing slash).
    pub fn from_display(input: &str) -> Option<Page> {
        let lower = input.trim().to_ascii_lowercase();
        let rest = lower.strip_prefix(DISPLAY_SCHEME)?;
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

/// Address-bar text for a URL: built-in pages get their `lb://` name.
pub fn display_url(url: &str) -> String {
    match Page::from_url(url) {
        Some(p) => p.display(),
        None => url.to_string(),
    }
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
        assert_eq!(Page::from_display("LB://History/"), Some(Page::History));
    }

    #[test]
    fn lookalike_origins_rejected() {
        assert_eq!(Page::from_url("https://lightbrowser.invalid.evil.com/settings.html"), None);
        assert_eq!(Page::from_url("http://lightbrowser.invalid/settings.html"), None);
        assert_eq!(Page::from_url("https://lightbrowser.invalid:8443/settings.html"), None);
        assert_eq!(Page::from_url("https://lightbrowser.invalid/evil.html"), None);
        assert!(!is_shell_origin("https://shell.lightbrowser.invalid.attacker.net/"));
        assert!(!is_shell_origin(PAGES_ORIGIN));
        assert!(!is_pages_origin(SHELL_ORIGIN));
        assert!(is_shell_origin("https://shell.lightbrowser.invalid/shell.html"));
    }

    #[test]
    fn display() {
        assert_eq!(display_url(&Page::NewTab.url()), "");
        assert_eq!(display_url(&Page::Settings.url()), "lb://settings");
        assert_eq!(display_url("https://example.com/"), "https://example.com/");
    }
}
