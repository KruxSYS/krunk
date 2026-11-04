//! Embedded UI assets and the rules for serving them.
//!
//! The shell origin serves `ui/shell/*`, the pages origin serves
//! `ui/pages/*`, and both serve `ui/shared/*` under `/shared/`. Nothing else
//! is reachable: a tab can never load the shell document and vice versa.

use crate::pages::{self, Page};
use crate::protocol::{Channel, POPUP_PATH, SHELL_PATH};
use url::Url;

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

/// Which internal host a request is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Shell,
    Pages,
}

/// A resolved embedded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub host: Host,
    pub key: &'static str,
    pub bytes: &'static [u8],
    pub mime: &'static str,
}

/// Looks up an embedded file by key (`shell/shell.html`).
pub fn get(key: &str) -> Option<(&'static str, &'static [u8])> {
    UI_ASSETS.binary_search_by(|(k, _)| k.cmp(&key)).ok().map(|i| UI_ASSETS[i])
}

pub fn mime_for(key: &str) -> &'static str {
    match key.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Maps a request URL on one of our origins to an embedded file.
pub fn resolve(url: &str) -> Option<Asset> {
    let u = Url::parse(url).ok()?;
    let origin = u.origin().ascii_serialization();
    let host = if origin == pages::SHELL_ORIGIN {
        Host::Shell
    } else if origin == pages::PAGES_ORIGIN {
        Host::Pages
    } else {
        return None;
    };
    let path = u.path().strip_prefix('/')?;
    // Percent-encoded or traversal paths are never valid asset names.
    if path.is_empty() || path.contains("..") || path.contains('%') || path.contains('\\') || path.contains("//") {
        return None;
    }
    let key = match path.strip_prefix("shared/") {
        Some(rest) => format!("shared/{rest}"),
        None => match host {
            Host::Shell => format!("shell/{path}"),
            Host::Pages => format!("pages/{path}"),
        },
    };
    let (key, bytes) = get(&key)?;
    Some(Asset { host, key, bytes, mime: mime_for(key) })
}

/// Path of the cached-favicon endpoint on both internal origins.
pub const FAVICON_PATH: &str = "/favicon";

/// `/favicon?host=<host>` (same-origin, relative).
pub fn favicon_url(host: &str) -> String {
    let enc: String = url::form_urlencoded::byte_serialize(host.as_bytes()).collect();
    format!("{FAVICON_PATH}?host={enc}")
}

/// If `url` asks one of our origins for a cached favicon, the host name.
pub fn favicon_request(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    let origin = u.origin().ascii_serialization();
    if (origin != pages::SHELL_ORIGIN && origin != pages::PAGES_ORIGIN) || u.path() != FAVICON_PATH {
        return None;
    }
    let host = u.query_pairs().find(|(k, _)| k == "host").map(|(_, v)| v.to_ascii_lowercase())?;
    crate::settings::is_hostname(&host).then_some(host)
}

/// Headers for a favicon response.
pub fn favicon_headers() -> &'static str {
    "Content-Type: image/png\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\n\
     Cache-Control: max-age=3600"
}

/// An injected content script (`ui/inject/*.js`). These are embedded but
/// never served over the internal origins.
pub fn inject_script(name: &str) -> &'static str {
    get(&format!("inject/{name}")).and_then(|(_, b)| std::str::from_utf8(b).ok()).unwrap_or("")
}

/// Script that adds `css` to the page as a constructed stylesheet (not
/// blocked by the page's CSP, unlike a `<style>` element).
pub fn style_injection_script(css: &str) -> String {
    let lit = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(c){{try{{var s=new CSSStyleSheet();s.replaceSync(c);\
         document.adoptedStyleSheets=document.adoptedStyleSheets.concat([s]);}}catch(e){{\
         var e2=document.createElement('style');e2.textContent=c;\
         (document.head||document.documentElement).appendChild(e2);}}}})({lit});"
    )
}

/// Turns favicon PNG bytes into a `data:` URL for the tab strip (the shell
/// may only load images from itself or `data:`). Oversized icons are dropped.
pub fn png_data_url(png: &[u8]) -> Option<String> {
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
    if png.is_empty() || png.len() > 48 * 1024 || !png.starts_with(PNG_MAGIC) {
        return None;
    }
    use base64ct::Encoding;
    Some(format!("data:image/png;base64,{}", base64ct::Base64::encode_string(png)))
}

/// Content-Security-Policy for our documents: only our own scripts and
/// styles, no network access, no framing.
pub fn csp(host: Host) -> &'static str {
    match host {
        Host::Shell => {
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; \
             connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
        }
        Host::Pages => {
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; \
             connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
        }
    }
}

/// Response headers (CRLF separated, as `CreateWebResourceResponse` wants).
pub fn response_headers(asset: &Asset) -> String {
    format!(
        "Content-Type: {}\r\nContent-Security-Policy: {}\r\nX-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nCross-Origin-Opener-Policy: same-origin\r\n\
         Cross-Origin-Resource-Policy: same-origin\r\nCache-Control: no-cache",
        asset.mime,
        csp(asset.host)
    )
}

/// Access from a tab to our internal origins, checked for every request the
/// tab's pages make: a top-level navigation is decided by
/// [`allow_navigation`]; anything else (sub-resources, frames, fetches)
/// needs to come from a document on the same internal origin. Requests to
/// other URLs are not affected.
pub fn internal_request_allowed(url: &str, initiator: &str, top_level_navigation: bool) -> bool {
    let Ok(u) = Url::parse(url) else { return true };
    let origin = u.origin().ascii_serialization();
    if origin != pages::SHELL_ORIGIN && origin != pages::PAGES_ORIGIN {
        return true;
    }
    if top_level_navigation {
        return true;
    }
    // The shell origin is never reachable from a tab.
    origin == pages::PAGES_ORIGIN && initiator.trim_end_matches('/') == pages::PAGES_ORIGIN
}

/// Navigation policy for a browser view.
///
/// * The shell and popup views may only ever show their own document.
/// * A tab may not show the shell origin at all.
/// * A web page may not navigate a tab to a built-in page by itself; only
///   the host (address bar, menu, new tab) may (`host_initiated`). Built-in
///   pages may link to each other and to the web.
pub fn allow_navigation(channel: Channel, current: &str, target: &str, host_initiated: bool) -> bool {
    let target_url = match Url::parse(target) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let target_origin = target_url.origin().ascii_serialization();
    match channel {
        Channel::Shell => target_origin == pages::SHELL_ORIGIN && target_url.path() == SHELL_PATH,
        Channel::Popup => target_origin == pages::SHELL_ORIGIN && target_url.path() == POPUP_PATH,
        Channel::Tab => {
            if target_origin == pages::SHELL_ORIGIN {
                return false;
            }
            if target_origin == pages::PAGES_ORIGIN {
                return Page::from_url(target).is_some()
                    && (host_initiated
                        || Page::from_url(current).is_some()
                        || current.is_empty()
                        || current == "about:blank");
            }
            matches!(target_url.scheme(), "http" | "https" | "about" | "data" | "blob" | "file")
                && !(target_url.scheme() == "file" && !host_initiated)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_has_required_files() {
        assert!(UI_ASSETS.windows(2).all(|w| w[0].0 < w[1].0), "sorted for binary search");
        for key in [
            "shell/shell.html",
            "shell/shell.js",
            "shell/shell.css",
            "shell/popup.html",
            "pages/newtab.html",
            "pages/https-only.html",
            "inject/gpc.js",
            "inject/page-tokens.js",
        ] {
            assert!(get(key).is_some(), "{key} missing");
        }
        assert!(UI_ASSETS.iter().all(|(k, _)| !k.starts_with("dev/")), "dev tooling is not embedded");
    }

    #[test]
    fn resolves_per_origin() {
        let a = resolve("https://shell.lightbrowser.invalid/shell.html").unwrap();
        assert_eq!((a.host, a.key), (Host::Shell, "shell/shell.html"));
        assert!(a.mime.starts_with("text/html"));
        let a = resolve("https://lightbrowser.invalid/newtab.html?x#y").unwrap();
        assert_eq!(a.key, "pages/newtab.html");
        assert_eq!(resolve("https://lightbrowser.invalid/shared/common.css").unwrap().key, "shared/common.css");
        assert_eq!(resolve("https://shell.lightbrowser.invalid/shared/common.css").unwrap().key, "shared/common.css");
        // Cross-origin access to the other host's files is impossible.
        assert!(resolve("https://lightbrowser.invalid/shell.html").is_none());
        assert!(resolve("https://shell.lightbrowser.invalid/newtab.html").is_none());
        for bad in [
            "https://lightbrowser.invalid/../shell/shell.html",
            "https://lightbrowser.invalid/%2e%2e/shell/shell.html",
            "https://lightbrowser.invalid/shared/..%2fshell.html",
            "https://lightbrowser.invalid/",
            "https://lightbrowser.invalid/dev/mock-host.js",
            "https://lightbrowser.invalid/inject/gpc.js",
            "https://shell.lightbrowser.invalid/inject/page-tokens.js",
            "https://lightbrowser.invalid:8443/newtab.html",
            "http://lightbrowser.invalid/newtab.html",
            "https://evil.example/newtab.html",
        ] {
            assert!(resolve(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn internal_origins_only_from_themselves() {
        let page = "https://lightbrowser.invalid/history.html";
        let fav = "https://lightbrowser.invalid/favicon?host=bank.example";
        let shell = "https://shell.lightbrowser.invalid/shell.js";
        // Built-in pages load their own files and favicons.
        assert!(internal_request_allowed(fav, "https://lightbrowser.invalid", false));
        assert!(internal_request_allowed(
            "https://lightbrowser.invalid/shared/page.css",
            "https://lightbrowser.invalid/",
            false
        ));
        // Web pages cannot probe or embed them, nor anything on the shell origin.
        assert!(!internal_request_allowed(fav, "https://evil.example", false));
        assert!(!internal_request_allowed(page, "https://evil.example", false), "iframe or fetch");
        assert!(!internal_request_allowed(fav, "", false));
        assert!(!internal_request_allowed(fav, "null", false));
        assert!(!internal_request_allowed(shell, "https://lightbrowser.invalid", false));
        assert!(!internal_request_allowed(shell, "https://shell.lightbrowser.invalid", false), "no shell in tabs");
        // Top-level navigations are the navigation policy's job.
        assert!(internal_request_allowed(page, "", true));
        // Everything else is untouched.
        assert!(internal_request_allowed("https://cdn.example/a.js", "https://evil.example", false));
        assert!(internal_request_allowed("not a url", "", false));
    }

    #[test]
    fn headers_include_csp() {
        let a = resolve("https://lightbrowser.invalid/newtab.html").unwrap();
        let h = response_headers(&a);
        assert!(h.contains("Content-Security-Policy: default-src 'none'"));
        assert!(h.contains("frame-ancestors 'none'"));
        assert!(h.contains("connect-src 'none'"), "new tab page makes no network calls");
        assert!(h.contains("X-Frame-Options: DENY"));
        assert!(!h.contains('\n') || h.contains("\r\n"));
    }

    #[test]
    fn injected_scripts() {
        assert!(inject_script("gpc.js").contains("globalPrivacyControl"));
        assert!(inject_script("dnt.js").contains("doNotTrack"));
        assert!(inject_script("page-tokens.js").contains("pageTokens"));
        assert_eq!(inject_script("missing.js"), "");
        // CSS is passed as a JSON string literal, so it cannot break out.
        let js = style_injection_script("a{}</script>\"); alert(1); (\"");
        assert!(js.contains(r#"("a{}</script>\"); alert(1); (\"")"#), "{js}");
    }

    #[test]
    fn favicon_endpoint() {
        assert_eq!(favicon_url("a.example"), "/favicon?host=a.example");
        assert_eq!(
            favicon_request("https://lightbrowser.invalid/favicon?host=A.Example").as_deref(),
            Some("a.example")
        );
        assert_eq!(favicon_request("https://shell.lightbrowser.invalid/favicon?host=x.org").as_deref(), Some("x.org"));
        assert_eq!(favicon_request("https://lightbrowser.invalid/favicon?host=../../etc"), None);
        assert_eq!(favicon_request("https://lightbrowser.invalid/favicon?host="), None);
        assert_eq!(favicon_request("https://lightbrowser.invalid/favicon"), None);
        assert_eq!(favicon_request("https://evil.example/favicon?host=a.com"), None);
        assert_eq!(favicon_request("https://lightbrowser.invalid/favicon.ico?host=a.com"), None);
    }

    #[test]
    fn favicon_data_urls() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert_eq!(png_data_url(png).unwrap(), "data:image/png;base64,iVBORw0KGgpyZXN0");
        assert_eq!(png_data_url(b"<svg onload=x>"), None);
        assert_eq!(png_data_url(&[]), None);
    }

    #[test]
    fn navigation_policy() {
        let shell = "https://shell.lightbrowser.invalid/shell.html";
        let popup = "https://shell.lightbrowser.invalid/popup.html";
        let settings = Page::Settings.url();
        assert!(allow_navigation(Channel::Shell, "", shell, true));
        assert!(!allow_navigation(Channel::Shell, shell, "https://evil.example/", false));
        assert!(!allow_navigation(Channel::Shell, shell, popup, false));
        assert!(allow_navigation(Channel::Popup, "", popup, true));
        assert!(!allow_navigation(Channel::Popup, popup, shell, false));

        assert!(!allow_navigation(Channel::Tab, "https://a.example/", shell, true), "never the shell in a tab");
        assert!(allow_navigation(Channel::Tab, "https://a.example/", &settings, true), "address bar");
        assert!(!allow_navigation(Channel::Tab, "https://a.example/", &settings, false), "page-initiated");
        assert!(allow_navigation(Channel::Tab, &Page::NewTab.url(), &settings, false), "built-in links");
        assert!(allow_navigation(Channel::Tab, "", &Page::NewTab.url(), false));
        assert!(!allow_navigation(Channel::Tab, "", "https://lightbrowser.invalid/evil.html", true));
        assert!(allow_navigation(Channel::Tab, &settings, "https://a.example/", false));
        assert!(!allow_navigation(Channel::Tab, "https://a.example/", "file:///C:/Windows/win.ini", false));
        assert!(allow_navigation(Channel::Tab, "", "file:///C:/x.html", true));
        assert!(!allow_navigation(Channel::Tab, "", "javascript:alert(1)", true));
        assert!(!allow_navigation(Channel::Tab, "", "ms-settings:privacy", false));
    }
}
