//! Which addresses the agent may open or act on (WP-B1).
//!
//! It never acts on (reads, scripts, clicks, types into, screenshots) Krunk's own pages, extension
//! pages or DevTools, and never opens the sensitive built-in pages (passwords and payment
//! methods, sync, site data, storage).
//!
//! * [`may_navigate`]: `http`/`https` web addresses, `about:blank`, and Krunk's own pages by their
//!   `krunk://` name or real address, unless [`Page::agent_forbidden`] or an internal page that
//!   only Krunk itself opens (source viewer, crash page, HTTPS-only notice). Everything else is
//!   refused: `file:`, `javascript:`, `data:`, `view-source:`, `chrome:`, `devtools:`, other
//!   `about:` pages, the shell and extension origins (any other `*.invalid` host).
//! * [`may_act`]: only `http`/`https` pages that are not on an internal (`.invalid`) host, and
//!   `about:blank`.

use crate::pages::{is_blank, Page};
use url::Url;

const NOT_ALLOWED: &str = "the agent can only open web addresses (http or https) and some of Krunk's own pages";
const FORBIDDEN_PAGE: &str = "this Krunk page is not available to the agent";
const NO_ACT: &str =
    "the agent can't read or change this page (Krunk's own pages, extension pages and DevTools are off limits)";

/// Built-in pages the agent may open, unless forbidden (see [`Page::agent_forbidden`]).
fn page_allowed(p: Page) -> bool {
    !p.agent_forbidden() && !matches!(p, Page::Source | Page::Crashed | Page::HttpsOnly | Page::CertError)
}

/// The pages the agent may open, by their `krunk://` names (for the system prompt and context).
pub fn navigable_pages() -> Vec<String> {
    Page::ALL.into_iter().filter(|p| page_allowed(*p)).map(|p| format!("krunk://{}", p.name())).collect()
}

/// An internal host (`krunk.invalid`, `shell.krunk.invalid`, `<id>.ext.krunk.invalid`, the
/// legacy origin): anything in the reserved `.invalid` TLD.
fn internal_host(u: &Url) -> bool {
    match u.host_str() {
        Some(h) => {
            let h = h.trim_end_matches('.').to_ascii_lowercase();
            h == "invalid" || h.ends_with(".invalid")
        }
        None => true,
    }
}

/// May the agent load `url` in a tab? Errors are short reasons for the model.
pub fn may_navigate(url: &str) -> Result<(), &'static str> {
    let t = url.trim();
    if t.is_empty() {
        return Err(NOT_ALLOWED);
    }
    if let Some(p) = Page::from_display(t) {
        return if page_allowed(p) { Ok(()) } else { Err(FORBIDDEN_PAGE) };
    }
    if is_blank(t) {
        return Ok(());
    }
    let u = Url::parse(t).map_err(|_| NOT_ALLOWED)?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(NOT_ALLOWED);
    }
    if internal_host(&u) {
        return match Page::from_url(u.as_str()) {
            Some(p) if page_allowed(p) => Ok(()),
            Some(_) => Err(FORBIDDEN_PAGE),
            None => Err(NOT_ALLOWED),
        };
    }
    Ok(())
}

/// May the agent's page tools act on a tab that shows `url`?
pub fn may_act(url: &str) -> Result<(), &'static str> {
    let t = url.trim();
    if is_blank(t) {
        return Ok(());
    }
    let u = Url::parse(t).map_err(|_| NO_ACT)?;
    if matches!(u.scheme(), "http" | "https") && !internal_host(&u) {
        Ok(())
    } else {
        Err(NO_ACT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_table() {
        for ok in [
            "https://example.com/",
            "http://localhost:8094/agent.html",
            "HTTPS://Example.COM/a?b#c",
            "about:blank",
            "ABOUT:BLANK",
            "krunk://settings",
            "krunk://settings#privacy",
            "KRUNK://History/",
            "krunk://bookmarks",
            "krunk://downloads",
            "krunk://about",
            "krunk://adblock",
            "krunk://gpu",
            "krunk://tasks",
            "krunk://extensions",
            "krunk://scripts",
            "krunk://newtab",
            "lb://settings",
            "https://krunk.invalid/settings.html",
            "https://krunk.invalid/history.html?q=x",
        ] {
            assert_eq!(may_navigate(ok), Ok(()), "{ok}");
        }
        for bad in [
            "krunk://passwords",
            "krunk://sync",
            "krunk://sitedata",
            "krunk://storage",
            "KRUNK://Passwords",
            "lb://passwords",
            "krunk://source",
            "krunk://crashed",
            "krunk://https-only",
            "krunk://cert-error",
            "https://krunk.invalid/cert-error.html?url=https://a.example/",
            "krunk://nope",
            "https://krunk.invalid/passwords.html",
            "https://krunk.invalid/passwords.html#cards",
            "https://KRUNK.INVALID/sync.html",
            "https://krunk.invalid/sitedata.html?site=a.com",
            "https://krunk.invalid./settings.html",
            "https://krunk.invalid/source.html?url=https://a.com/",
            "https://krunk.invalid/shared/common.css",
            "https://shell.krunk.invalid/shell.html",
            "https://shell.krunk.invalid/agent.html",
            "https://abcdefghijklmnopabcdefghijklmnop.ext.krunk.invalid/popup.html",
            "https://lightbrowser.invalid/passwords.html",
            "file:///C:/Users/ann/secret.txt",
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "view-source:https://example.com/",
            "chrome://settings",
            "devtools://devtools/bundled/inspector.html",
            "about:settings",
            "about:srcdoc",
            "ftp://example.com/",
            "blob:https://example.com/123",
            "",
            "not a url",
        ] {
            assert!(may_navigate(bad).is_err(), "{bad}");
        }
        assert_eq!(may_navigate("krunk://passwords"), Err(FORBIDDEN_PAGE));
    }

    #[test]
    fn act_table() {
        for ok in ["https://example.com/", "http://127.0.0.1:8094/agent.html", "about:blank", "https://a.example/x?y"] {
            assert_eq!(may_act(ok), Ok(()), "{ok}");
        }
        for bad in [
            "https://krunk.invalid/settings.html",
            "https://krunk.invalid/newtab.html",
            "https://krunk.invalid/passwords.html",
            "https://shell.krunk.invalid/agent.html",
            "https://abcdefghijklmnopabcdefghijklmnop.ext.krunk.invalid/options.html",
            "https://Krunk.Invalid./history.html",
            "krunk://settings",
            "chrome://gpu",
            "devtools://devtools/bundled/inspector.html",
            "file:///C:/x.html",
            "data:text/html,x",
            "view-source:https://example.com/",
            "javascript:void(0)",
            "about:srcdoc",
            "",
        ] {
            assert!(may_act(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn navigable_list() {
        let list = navigable_pages();
        for p in ["krunk://settings", "krunk://scripts", "krunk://newtab", "krunk://history"] {
            assert!(list.iter().any(|x| x == p), "{p}");
        }
        for p in ["krunk://passwords", "krunk://sync", "krunk://storage", "krunk://sitedata", "krunk://source"] {
            assert!(!list.iter().any(|x| x == p), "{p}");
        }
        for p in &list {
            assert_eq!(may_navigate(p), Ok(()), "{p}");
        }
    }
}
