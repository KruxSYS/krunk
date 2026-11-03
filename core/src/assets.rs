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

/// Navigation policy for a WebView.
///
/// * The shell and popup WebViews may only ever show their own document.
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
        for key in ["shell/shell.html", "shell/shell.js", "shell/shell.css", "shell/popup.html", "pages/newtab.html"] {
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
            "https://lightbrowser.invalid:8443/newtab.html",
            "http://lightbrowser.invalid/newtab.html",
            "https://evil.example/newtab.html",
        ] {
            assert!(resolve(bad).is_none(), "{bad}");
        }
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
