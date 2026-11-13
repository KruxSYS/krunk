//! Checks of the extension runtime that need no browser (the 0.17.0 security review): who a
//! content script's `sender` is, what an extension may open, who may load an extension's files
//! (`web_accessible_resources`), the storage quota and what goes into the log.

use crate::urlclean::origin_of;

// ------------------------------------------------------------------ sender of a content script

/// `sender.url` and `sender.origin` of a content script, top frame or not: the address the
/// script gives (`says_url`, its `location.href`) only when it is on the origin the engine
/// reported for that execution context, else that origin's root, else nothing (an opaque
/// origin). Never the tab's address: that one changes when a navigation starts, while the
/// old document and its content script still run.
pub fn content_sender(engine_origin: &str, says_url: &str) -> (String, Option<String>) {
    let url = if !says_url.is_empty()
        && says_url.len() <= 4096
        && origin_of(says_url).is_some_and(|o| o == engine_origin)
    {
        says_url.to_string()
    } else if origin_of(engine_origin).is_some_and(|o| o == engine_origin) {
        format!("{engine_origin}/")
    } else {
        String::new()
    };
    let origin = origin_of(&url);
    (url, origin)
}

// ------------------------------------------------------------------ what an extension may open and script

fn krunk_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == "krunk.invalid" || h.ends_with(".krunk.invalid")
}

/// May extension `ext_id` open `url` (resolved, in Krunk's form) with `tabs.create`,
/// `tabs.update` or `windows.create`? Its own pages and the web. Not local files, not Krunk's
/// pages, not another extension's pages: an address an extension supplies is not the host's.
pub fn may_open(ext_id: &str, url: &str) -> bool {
    if url.starts_with(&format!("{}/", crate::ext::origin(ext_id))) {
        return true;
    }
    let Ok(u) = url::Url::parse(url) else { return false };
    matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !krunk_host(h))
}

/// Chrome's error for `scripting.*` on a page the extension has no host access to.
pub const NO_HOST_ACCESS: &str =
    "Cannot access contents of the page. Extension manifest must request permission to access the respective host.";

/// Has the extension host access to documents of `origin` (the engine's, `scheme://host[:port]`)
/// through its manifest: a host pattern among `host_permissions` or `permissions`? Web origins
/// only; as in Chrome a pattern's path does not matter. For `scripting.executeScript` and
/// `insertCSS` (content script `matches` grant nothing here).
pub fn host_access(m: &crate::ext::Manifest, origin: &str) -> bool {
    let Ok(u) = url::Url::parse(origin) else { return false };
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none_or(krunk_host) {
        return false;
    }
    let root = format!("{}/", origin.trim_end_matches('/'));
    m.permissions.iter().chain(&m.host_permissions).filter_map(|p| crate::ext::MatchPattern::parse(p)).any(|p| {
        let any_path = crate::ext::MatchPattern { path: "/*".into(), ..p };
        any_path.matches(&root)
    })
}

// ------------------------------------------------------------------ who may load an extension's files

/// The path of a request on an extension's origin as it is compared with the manifest's lists
/// (`sandbox.pages`, `web_accessible_resources`): percent-decoded like the file that is served
/// (`ext::resolve_file`), without leading slashes, and in lower case (the profile is on a
/// file system that ignores case, so `/Page.html` and `/p%61ge.html` are `page.html`).
pub fn package_path(url_path: &str) -> String {
    crate::ext::percent_decode(url_path).trim_start_matches('/').to_lowercase()
}

/// A path as a manifest writes it (a file name or a glob), for the comparison with
/// [`package_path`].
pub fn manifest_path(p: &str) -> String {
    p.trim_start_matches('/').to_lowercase()
}

/// What decides who may load an extension's files, from its manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebAccess {
    /// `web_accessible_resources`: (resource globs, the sites they are for).
    entries: Vec<(Vec<String>, Vec<crate::ext::MatchPattern>)>,
    /// Style sheets of content scripts: the shim links them into the page.
    css: Vec<String>,
    /// The manifest has sandboxed pages (their documents have an opaque origin).
    sandbox: bool,
}

impl WebAccess {
    /// `scripts`: the extension's content scripts, the manifest's and the registered ones.
    pub fn new(m: &crate::ext::Manifest, scripts: &[crate::ext::ContentScript]) -> WebAccess {
        let strings = |v: Option<&serde_json::Value>| -> Vec<String> {
            v.and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        // Manifest V3 entries only (`{ resources, matches }`); an entry for other extensions
        // only (`extension_ids`) opens nothing to the web.
        let entries = m
            .web_accessible_resources
            .iter()
            .filter(|e| e.is_object())
            .map(|e| {
                (
                    strings(e.get("resources")).iter().map(|r| manifest_path(r)).collect::<Vec<_>>(),
                    strings(e.get("matches"))
                        .iter()
                        .filter_map(|p| crate::ext::MatchPattern::parse(p))
                        .map(|p| crate::ext::MatchPattern { path: "/*".into(), ..p })
                        .collect::<Vec<_>>(),
                )
            })
            .filter(|(resources, sites)| !resources.is_empty() && !sites.is_empty())
            .collect();
        WebAccess {
            entries,
            css: scripts.iter().flat_map(|s| s.css.iter().map(|f| manifest_path(f))).collect(),
            sandbox: m.sandbox.as_ref().is_some_and(|s| !s.pages.is_empty()),
        }
    }

    /// Is the file listed under `web_accessible_resources` for some site? Such a file is
    /// served with `Access-Control-Allow-Origin: *` and may be the target of a redirect.
    pub fn web_accessible(&self, url_path: &str) -> bool {
        let path = package_path(url_path);
        self.entries.iter().any(|(resources, _)| resources.iter().any(|r| crate::ext::glob_match(r, &path)))
    }

    /// May a document of `site` (a web origin; `None`: an opaque one) load the file?
    fn open_to(&self, path: &str, site: Option<&str>) -> bool {
        if self.css.iter().any(|c| c == path) {
            return true;
        }
        self.entries.iter().any(|(resources, sites)| {
            resources.iter().any(|r| crate::ext::glob_match(r, path))
                && sites.iter().any(|p| match site {
                    Some(origin) => p.matches(&format!("{origin}/")),
                    // An opaque origin (a sandboxed frame, a data: document) has no address:
                    // what every site may load it may load too.
                    None => p.is_all_sites() && matches!(p.scheme.as_str(), "*" | "https"),
                })
        })
    }
}

/// May a request in a tab for `url` (a file of extension `ext_id`) be answered? `initiator`
/// is the origin of the document that asks, as the engine reports it (empty for the browser's
/// own navigations: the address bar, a tab the host opened, history), `top_url` the tab's
/// address. The extension's own documents and Krunk's pages (icons on krunk://extensions) load
/// anything; a web page, by a sub-resource, a frame, `fetch`, a link or `window.open`, only
/// what `web_accessible_resources` lists for its origin (and the content scripts' style
/// sheets); another extension nothing.
pub fn request_allowed(access: &WebAccess, ext_id: &str, url: &str, initiator: &str, top_url: &str) -> bool {
    let own = crate::ext::origin(ext_id);
    let initiator = initiator.trim_end_matches('/');
    if initiator.is_empty() || initiator == own || initiator == crate::pages::PAGES_ORIGIN {
        return true;
    }
    let Ok(u) = url::Url::parse(url) else { return false };
    let path = package_path(u.path());
    if initiator == "null" {
        // The extension's own sandboxed page (an opaque origin) in a tab of the extension
        // loads its scripts.
        return (access.sandbox && top_url.starts_with(&format!("{own}/"))) || access.open_to(&path, None);
    }
    let web = url::Url::parse(initiator)
        .is_ok_and(|i| matches!(i.scheme(), "http" | "https") && i.host_str().is_some_and(|h| !krunk_host(h)));
    web && access.open_to(&path, Some(initiator))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn manifest(rest: &str) -> crate::ext::Manifest {
        crate::ext::Manifest::parse(&format!(r#"{{"manifest_version":3,"name":"T","version":"1"{rest}}}"#)).unwrap()
    }

    #[test]
    fn a_web_page_loads_only_web_accessible_resources() {
        // A wallet's manifest: one widget for one site, a sandboxed page, style sheets.
        let m = manifest(
            r#","web_accessible_resources":[{"resources":["cashtag-widget.html","img/*.png"],"matches":["https://x.com/*","https://*.twitter.com/*"]},
                {"resources":["/Open/*"],"matches":["<all_urls>"]},{"resources":["for-extensions.js"],"extension_ids":["*"]},"legacy.js"],
              "sandbox":{"pages":["snaps/index.html"]},
              "content_scripts":[{"matches":["https://*/*"],"js":["c.js"],"css":["css/page.css"]}]"#,
        );
        let access = WebAccess::new(&m, &m.content_scripts);
        let own = crate::ext::origin(ID);
        let file = |p: &str| format!("{own}/{p}");
        let ask = |p: &str, initiator: &str, top: &str| request_allowed(&access, ID, &file(p), initiator, top);
        // The extension's own documents, the host's navigations, Krunk's extensions page.
        for initiator in [own.as_str(), "", "https://krunk.invalid"] {
            assert!(ask("home.html", initiator, "") && ask("scripts/background.js", initiator, ""), "{initiator}");
        }
        // A site: what is listed for it, whatever the spelling of the path.
        assert!(ask("cashtag-widget.html", "https://x.com", "https://x.com/home"));
        assert!(ask("Cashtag-Widget.html?a=1", "https://x.com", ""));
        assert!(ask("c%61shtag-widget.html", "https://x.com", ""));
        assert!(ask("img/logo.png", "https://mobile.twitter.com", ""));
        assert!(ask("open/any/file.js", "https://example.org", "") && ask("open/a.js", "http://localhost:8094", ""));
        assert!(ask("css/page.css", "https://example.org", ""), "a content script's style sheet");
        for (path, initiator) in [
            ("cashtag-widget.html", "https://evil.example"),
            ("cashtag-widget.html", "http://x.com"),
            ("home.html", "https://x.com"),
            ("manifest.json", "https://x.com"),
            ("scripts/background.js", "https://example.org"),
            ("h%6Fme.html", "https://x.com"),
            ("for-extensions.js", "https://example.org"),
            ("legacy.js", "https://example.org"),
            ("snaps/index.html", "https://example.org"),
        ] {
            assert!(!ask(path, initiator, &format!("{initiator}/")), "{path} from {initiator}");
        }
        // Another extension, and Krunk's shell, are not sites.
        let other = crate::ext::origin("ponmlkjihgfedcbaponmlkjihgfedcba");
        assert!(!ask("open/a.js", &other, "") && !ask("home.html", &other, ""));
        assert!(!ask("open/a.js", "file://", ""));
        // Opaque origins: the extension's sandboxed page in its own tab; elsewhere only what
        // every site may load.
        assert!(ask("snaps/bundle.js", "null", &file("home.html")));
        assert!(!ask("snaps/bundle.js", "null", "https://evil.example/"));
        assert!(ask("open/a.js", "null", "https://evil.example/") && !ask("img/logo.png", "null", "https://x.com/"));
        let plain = WebAccess::new(&manifest(""), &[]);
        assert!(!request_allowed(&plain, ID, &file("a.js"), "null", &file("home.html")), "no sandboxed pages");
        // CORS and redirects: listed for some site.
        assert!(access.web_accessible("/cashtag-widget.html") && access.web_accessible("/IMG/a.png"));
        assert!(access.web_accessible("/open/x") && !access.web_accessible("/home.html"));
        assert!(!access.web_accessible("/css/page.css") && !access.web_accessible("/for-extensions.js"));
        assert_eq!(package_path("//S%61ndbox/Page.HTML"), "sandbox/page.html");
    }

    #[test]
    fn an_extension_opens_its_own_pages_and_the_web_only() {
        let own = crate::ext::origin(ID);
        for ok in [format!("{own}/home.html#x"), "https://example.com/".into(), "http://localhost:8094/a".into()] {
            assert!(may_open(ID, &ok), "{ok}");
        }
        let other = crate::ext::origin("ponmlkjihgfedcbaponmlkjihgfedcba");
        for bad in [
            "file:///C:/Users/me/secret.txt".to_string(),
            "https://krunk.invalid/settings.html".into(),
            "https://shell.krunk.invalid/shell.html".into(),
            format!("{other}/page.html"),
            "chrome://gpu/".into(),
            "krunk://settings".into(),
            "about:blank".into(),
            "data:text/html,x".into(),
            "javascript:alert(1)".into(),
            own.clone(),
            String::new(),
        ] {
            assert!(!may_open(ID, &bad), "{bad}");
        }
    }

    #[test]
    fn scripting_needs_a_host_permission_for_the_documents_origin() {
        let m = manifest(r#","permissions":["scripting","activeTab"],"host_permissions":["https://*.example.com/app/*"]"#);
        assert!(host_access(&m, "https://www.example.com"));
        assert!(host_access(&m, "https://example.com"));
        assert!(!host_access(&m, "http://example.com") && !host_access(&m, "https://example.org"));
        // A wallet's patterns: every web page, no local file, none of Krunk's pages.
        let m = manifest(r#","host_permissions":["file://*/*","http://*/*","https://*/*"]"#);
        assert!(host_access(&m, "https://example.org") && host_access(&m, "http://localhost:8094"));
        for no in ["file://", "null", "", "https://krunk.invalid", &crate::ext::origin(ID)] {
            assert!(!host_access(&m, no), "{no}");
        }
        // Content script matches are not host permissions.
        let m = manifest(r#","content_scripts":[{"matches":["<all_urls>"],"js":["c.js"]}]"#);
        assert!(!host_access(&m, "https://example.org"));
        assert!(host_access(&manifest(r#","permissions":["<all_urls>"]"#), "https://example.org"));
    }

    #[test]
    fn a_content_script_is_named_by_its_documents_origin_not_by_a_pending_navigation() {
        // The page started a navigation to a bank and kept its own document: the engine still
        // reports the page's origin for the script's world.
        let (url, origin) = content_sender("https://evil.example", "https://evil.example/a?b#c");
        assert_eq!((url.as_str(), origin.as_deref()), ("https://evil.example/a?b#c", Some("https://evil.example")));
        // A script that names another site is not believed.
        let (url, origin) = content_sender("https://evil.example", "https://bank.example/login");
        assert_eq!((url.as_str(), origin.as_deref()), ("https://evil.example/", Some("https://evil.example")));
        let (url, origin) = content_sender("http://localhost:8094", "http://localhost:8094/ext.html");
        assert_eq!((url.as_str(), origin.as_deref()), ("http://localhost:8094/ext.html", Some("http://localhost:8094")));
        // about:blank and srcdoc frames have their parent's origin.
        let (url, origin) = content_sender("https://a.example", "about:blank");
        assert_eq!((url.as_str(), origin.as_deref()), ("https://a.example/", Some("https://a.example")));
        // Opaque origins (sandboxed frames, data:) and over-long addresses.
        for engine in ["null", "", "://", "file://"] {
            assert_eq!(content_sender(engine, "https://bank.example/"), (String::new(), None), "{engine}");
        }
        let long = format!("https://a.example/{}", "x".repeat(5000));
        assert_eq!(content_sender("https://a.example", &long).0, "https://a.example/");
    }
}
