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

#[cfg(test)]
mod tests {
    use super::*;

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
