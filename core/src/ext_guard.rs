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

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn manifest(rest: &str) -> crate::ext::Manifest {
        crate::ext::Manifest::parse(&format!(r#"{{"manifest_version":3,"name":"T","version":"1"{rest}}}"#)).unwrap()
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
