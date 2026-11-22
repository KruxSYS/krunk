//! Embedded UI assets and the rules for serving them.
//!
//! The shell origin serves `ui/shell/*`, the pages origin serves
//! `ui/pages/*`, and both serve `ui/shared/*` under `/shared/`. Nothing else
//! is reachable: a tab can never load the shell document and vice versa.

use crate::pages::{self, Page};
use crate::protocol::{Channel, AGENT_PATH, POPUP_PATH, RUNNER_PATH, SHELL_PATH};
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
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// A built-in new-tab wallpaper (0.7.0, R2; built by assets/wallpapers/make.py into
/// ui/shared/wallpapers/, served with the UI files).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    /// "dark" or "light": the colour of what the new tab page draws over it.
    pub look: String,
    /// The darkening it starts with (percent).
    pub dim: u8,
}

pub fn wallpapers() -> Vec<Wallpaper> {
    #[allow(clippy::expect_used)] // checked by a test
    serde_json::from_str(include_str!("../../ui/shared/wallpapers/wallpapers.json")).expect("wallpapers.json")
}

pub fn wallpaper(id: &str) -> Option<Wallpaper> {
    wallpapers().into_iter().find(|w| w.id == id)
}

/// The wallpaper image and its thumbnail on the internal origin.
pub fn wallpaper_url(id: &str) -> String {
    format!("https://krunk.invalid/shared/wallpapers/{id}.webp")
}

pub fn wallpaper_thumb_url(id: &str) -> String {
    format!("https://krunk.invalid/shared/wallpapers/{id}-thumb.webp")
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

/// Path of the tab-icon endpoint on the shell origin (0.10.0).
pub const TAB_ICON_PATH: &str = "/tab-icon";

/// The tab strip's address for a tab's favicon (`data:` URL in the model): the shell loads it by
/// URL instead of getting every favicon inlined in every tab-strip message. `v` changes with the
/// icon, so the image is cached until it does.
pub fn tab_icon_url(id: u32, data_url: &str) -> String {
    // FNV-1a: a short, stable fingerprint of the icon.
    let v = data_url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{TAB_ICON_PATH}?id={id}&v={v:016x}")
}

/// If `url` asks the shell origin for a tab's icon, the tab id.
pub fn tab_icon_request(url: &str) -> Option<u32> {
    let u = Url::parse(url).ok()?;
    if u.origin().ascii_serialization() != pages::SHELL_ORIGIN || u.path() != TAB_ICON_PATH {
        return None;
    }
    u.query_pairs().find(|(k, _)| k == "id").and_then(|(_, v)| v.parse().ok())
}

/// The PNG inside a `data:image/png;base64,` URL (the tab model's favicons).
pub fn png_of_data_url(data_url: &str) -> Option<Vec<u8>> {
    use base64ct::Encoding;
    base64ct::Base64::decode_vec(data_url.strip_prefix("data:image/png;base64,")?).ok()
}

/// Headers for a favicon response.
pub fn favicon_headers() -> &'static str {
    "Content-Type: image/png\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\n\
     Cache-Control: max-age=3600"
}

/// A step screenshot of the agent panel (0.12.0): `https://shell.krunk.invalid/agent-shot/…`
/// → the path (`/agent-shot/<token>/<n>.jpg`). Only on the shell origin, which no tab reaches.
pub fn agent_shot_request(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    (u.origin().ascii_serialization() == pages::SHELL_ORIGIN && u.path().starts_with("/agent-shot/"))
        .then(|| u.path().to_string())
}

pub fn agent_shot_headers() -> &'static str {
    "Content-Type: image/jpeg\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\n\
     Cache-Control: no-store"
}

/// The new tab page's background image: `https://krunk.invalid/ntp-image/<name>` → the file
/// name (only the host's own `ntp-background-<ms>.<ext>` names, no paths).
pub fn ntp_image_request(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    if u.origin().ascii_serialization() != pages::PAGES_ORIGIN {
        return None;
    }
    let name = u.path().strip_prefix("/ntp-image/")?;
    let ok = name.starts_with("ntp-background-")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !name.contains("..");
    ok.then(|| name.to_string())
}

pub fn ntp_image_url(name: &str) -> String {
    format!("{}/ntp-image/{name}", pages::PAGES_ORIGIN)
}

pub fn ntp_image_headers(name: &str) -> String {
    let mime = match name.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        // 0.15.0: the agent's set_new_tab_background_url takes AVIF too.
        "avif" => "image/avif",
        _ => "application/octet-stream",
    };
    format!("Content-Type: {mime}\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\nCache-Control: max-age=86400")
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

/// 0.15.0: the inline `<style>` at the top of every Krunk page: html and body take the theme's
/// background colour from the very first frame (before, Chromium's dark canvas #121212 could show
/// for a frame). The pages' CSP allows exactly this text by its SHA-256.
pub const FIRST_PAINT_STYLE: &str =
    "html,body{background:#f3f4f6}@media (prefers-color-scheme:dark){html,body{background:#1f2023}}";

/// Content-Security-Policy for our documents: only our own scripts and
/// styles, no network access, no framing.
pub fn csp(host: Host) -> &'static str {
    match host {
        Host::Shell => {
            // Extension icons come from the extensions' own origins (0.6.0).
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https://*.ext.krunk.invalid; \
             font-src 'self'; connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
        }
        Host::Pages => {
            // krunk://extensions shows the extensions' icons (0.6.0). 0.15.0: the pages' one
            // inline block, the theme colour before the stylesheets ([`FIRST_PAINT_STYLE`]).
            "default-src 'none'; script-src 'self'; style-src 'self' 'sha256-qx71rF15hsv9TAP+d8lS0a1m7Tb/DaZ1Nmz4NgE08Iw='; img-src 'self' data: https://*.ext.krunk.invalid; \
             font-src 'self'; connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
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
///
/// `committed` is the address of the document the tab shows now (the engine's last address
/// change), not the address the tab was opened for. Since 0.11.7 (audit C3) a built-in page
/// is reached without the host only from a committed built-in page and never by a server
/// redirect: a popup (`window.open`) or a fresh `about:blank` used to count as the page it
/// was opened for, and a redirect from a site typed on the new-tab page counted as the
/// new-tab page.
pub fn allow_navigation(
    channel: Channel,
    committed: &str,
    target: &str,
    host_initiated: bool,
    redirected: bool,
) -> bool {
    let target_url = match Url::parse(target) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let target_origin = target_url.origin().ascii_serialization();
    match channel {
        Channel::Shell => target_origin == pages::SHELL_ORIGIN && target_url.path() == SHELL_PATH,
        Channel::Popup => target_origin == pages::SHELL_ORIGIN && target_url.path() == POPUP_PATH,
        // 0.12.0: the agent panel and the macro runner stay on their own document.
        Channel::Agent => target_origin == pages::SHELL_ORIGIN && target_url.path() == AGENT_PATH,
        Channel::Runner => target_origin == pages::SHELL_ORIGIN && target_url.path() == RUNNER_PATH,
        // An extension's background or popup view stays on extension origins (its own: the
        // host's hook checks the id).
        Channel::Ext => crate::ext::id_of_url(target).is_some(),
        Channel::Tab => {
            if target_origin == pages::SHELL_ORIGIN {
                return false;
            }
            if target_origin == pages::PAGES_ORIGIN {
                return Page::from_url(target).is_some()
                    && (host_initiated || (!redirected && Page::from_url(committed).is_some()));
            }
            // Chromium's own chrome://gpu (from krunk://gpu's button) is the only chrome:// page,
            // and only when the host navigates there.
            if target_url.scheme() == "chrome" {
                return host_initiated && target_url.host_str() == Some("gpu") && target_url.path() == "/";
            }
            matches!(target_url.scheme(), "http" | "https" | "about" | "data" | "blob" | "file")
                && !(target_url.scheme() == "file" && !host_initiated)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_first_paint_style_is_the_one_inline_block_and_its_hash_is_allowed() {
        use base64ct::Encoding;
        use sha2::{Digest, Sha256};
        let hash = base64ct::Base64::encode_string(&Sha256::digest(super::FIRST_PAINT_STYLE.as_bytes()));
        assert!(super::csp(super::Host::Pages).contains(&format!("'sha256-{hash}'")));
        assert!(!super::csp(super::Host::Shell).contains("sha256-"), "the shell has no inline style");
        let mut pages = 0;
        for (k, body) in super::UI_ASSETS.iter().filter(|(k, _)| k.starts_with("pages/") && k.ends_with(".html")) {
            let html = std::str::from_utf8(body).unwrap();
            let inline: Vec<&str> =
                html.split("<style>").skip(1).map(|t| t.split("</style>").next().unwrap()).collect();
            if html.contains("shared/common.css") {
                assert_eq!(inline, [super::FIRST_PAINT_STYLE], "{k}");
                assert!(html.contains(r#"<meta name="color-scheme" content="light dark">"#), "{k}");
                assert!(html.find("<style>") < html.find("<link rel=\"stylesheet\""), "{k}: before the stylesheets");
                pages += 1;
            } else {
                assert!(inline.is_empty(), "{k}");
            }
        }
        assert!(pages >= 16, "{pages}");
    }

    #[test]
    fn agent_shots_only_on_the_shell_origin() {
        assert_eq!(
            agent_shot_request("https://shell.krunk.invalid/agent-shot/ab12/3.jpg").as_deref(),
            Some("/agent-shot/ab12/3.jpg")
        );
        assert_eq!(agent_shot_request("https://krunk.invalid/agent-shot/ab12/3.jpg"), None);
        assert_eq!(agent_shot_request("https://shell.krunk.invalid/shell.html"), None);
        assert!(agent_shot_headers().contains("no-store"));
    }

    use super::*;

    #[test]
    fn built_in_wallpapers_are_served_and_small() {
        let list = wallpapers();
        assert!(list.len() >= 8, "R2 asks for at least 8, got {}", list.len());
        assert!(list.iter().any(|w| w.look == "dark") && list.iter().any(|w| w.look == "light"));
        let mut total = 0;
        for w in &list {
            assert!(w.look == "dark" || w.look == "light", "{w:?}");
            assert!(w.dim <= 80, "{w:?}");
            for key in [format!("shared/wallpapers/{}.webp", w.id), format!("shared/wallpapers/{}-thumb.webp", w.id)] {
                let (_, bytes) = get(&key).unwrap_or_else(|| panic!("{key} is not embedded"));
                assert_eq!(&bytes[..4], b"RIFF", "{key}");
                assert_eq!(&bytes[8..12], b"WEBP", "{key}");
                total += bytes.len();
            }
            assert_eq!(mime_for(&format!("{}.webp", w.id)), "image/webp");
            assert_eq!(
                resolve(&wallpaper_url(&w.id)).map(|a| a.key.to_string()),
                Some(format!("shared/wallpapers/{}.webp", w.id))
            );
        }
        assert!(total <= 4 * 1024 * 1024, "wallpapers take {total} bytes; the budget is 4 MB");
        assert!(wallpaper("no-such").is_none());
    }

    #[test]
    fn ntp_image_requests() {
        assert_eq!(
            ntp_image_request("https://krunk.invalid/ntp-image/ntp-background-17.png").as_deref(),
            Some("ntp-background-17.png")
        );
        assert_eq!(
            ntp_image_request(&ntp_image_url("ntp-background-1.webp")).as_deref(),
            Some("ntp-background-1.webp")
        );
        assert!(ntp_image_request("https://krunk.invalid/ntp-image/../browser.db").is_none());
        assert!(ntp_image_request("https://krunk.invalid/ntp-image/vault.json").is_none());
        assert!(ntp_image_request("https://shell.krunk.invalid/ntp-image/ntp-background-1.png").is_none());
        assert!(ntp_image_headers("ntp-background-1.jpg").starts_with("Content-Type: image/jpeg"));
        assert!(ntp_image_headers("ntp-background-1.avif").starts_with("Content-Type: image/avif"));
    }

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
            "inject/notify.js",
            "inject/page-tokens.js",
        ] {
            assert!(get(key).is_some(), "{key} missing");
        }
        assert!(UI_ASSETS.iter().all(|(k, _)| !k.starts_with("dev/")), "dev tooling is not embedded");
    }

    #[test]
    fn resolves_per_origin() {
        let a = resolve("https://shell.krunk.invalid/shell.html").unwrap();
        assert_eq!((a.host, a.key), (Host::Shell, "shell/shell.html"));
        assert!(a.mime.starts_with("text/html"));
        let a = resolve("https://krunk.invalid/newtab.html?x#y").unwrap();
        assert_eq!(a.key, "pages/newtab.html");
        assert_eq!(resolve("https://krunk.invalid/shared/common.css").unwrap().key, "shared/common.css");
        assert_eq!(resolve("https://shell.krunk.invalid/shared/common.css").unwrap().key, "shared/common.css");
        // Cross-origin access to the other host's files is impossible.
        assert!(resolve("https://krunk.invalid/shell.html").is_none());
        assert!(resolve("https://shell.krunk.invalid/newtab.html").is_none());
        for bad in [
            "https://krunk.invalid/../shell/shell.html",
            "https://krunk.invalid/%2e%2e/shell/shell.html",
            "https://krunk.invalid/shared/..%2fshell.html",
            "https://krunk.invalid/",
            "https://krunk.invalid/dev/mock-host.js",
            "https://krunk.invalid/inject/notify.js",
            "https://shell.krunk.invalid/inject/page-tokens.js",
            "https://krunk.invalid:8443/newtab.html",
            "http://krunk.invalid/newtab.html",
            "https://evil.example/newtab.html",
        ] {
            assert!(resolve(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn internal_origins_only_from_themselves() {
        let page = "https://krunk.invalid/history.html";
        let fav = "https://krunk.invalid/favicon?host=bank.example";
        let shell = "https://shell.krunk.invalid/shell.js";
        // Built-in pages load their own files and favicons.
        assert!(internal_request_allowed(fav, "https://krunk.invalid", false));
        assert!(internal_request_allowed("https://krunk.invalid/shared/page.css", "https://krunk.invalid/", false));
        // Web pages cannot probe or embed them, nor anything on the shell origin.
        assert!(!internal_request_allowed(fav, "https://evil.example", false));
        assert!(!internal_request_allowed(page, "https://evil.example", false), "iframe or fetch");
        assert!(!internal_request_allowed(fav, "", false));
        assert!(!internal_request_allowed(fav, "null", false));
        assert!(!internal_request_allowed(shell, "https://krunk.invalid", false));
        assert!(!internal_request_allowed(shell, "https://shell.krunk.invalid", false), "no shell in tabs");
        // Top-level navigations are the navigation policy's job.
        assert!(internal_request_allowed(page, "", true));
        // Everything else is untouched.
        assert!(internal_request_allowed("https://cdn.example/a.js", "https://evil.example", false));
        assert!(internal_request_allowed("not a url", "", false));
    }

    #[test]
    fn headers_include_csp() {
        let a = resolve("https://krunk.invalid/newtab.html").unwrap();
        let h = response_headers(&a);
        assert!(h.contains("Content-Security-Policy: default-src 'none'"));
        assert!(h.contains("frame-ancestors 'none'"));
        assert!(h.contains("connect-src 'none'"), "new tab page makes no network calls");
        assert!(h.contains("X-Frame-Options: DENY"));
        assert!(!h.contains('\n') || h.contains("\r\n"));
    }

    #[test]
    fn injected_scripts() {
        assert!(inject_script("notify.js").contains("requestPermission"));
        assert!(inject_script("gpc.js").is_empty(), "0.28.0: GPC and DNT are the engine's own");
        // 0.28.0: no Krunk script replaces Function.prototype.toString (bot defences see it).
        assert!(!inject_script("pointer-lock.js").contains("Function.prototype, 'toString'"));
        assert!(inject_script("page-tokens.js").contains("pageTokens"));
        assert_eq!(inject_script("missing.js"), "");
        // CSS is passed as a JSON string literal, so it cannot break out.
        let js = style_injection_script("a{}</script>\"); alert(1); (\"");
        assert!(js.contains(r#"("a{}</script>\"); alert(1); (\"")"#), "{js}");
    }

    #[test]
    fn tab_icon_endpoint() {
        let a = tab_icon_url(7, "data:image/png;base64,AAAA");
        assert!(a.starts_with("/tab-icon?id=7&v="), "{a}");
        assert_ne!(a, tab_icon_url(7, "data:image/png;base64,AAAB"), "a new icon, a new address");
        assert_eq!(a, tab_icon_url(7, "data:image/png;base64,AAAA"));
        assert_eq!(tab_icon_request(&format!("https://shell.krunk.invalid{a}")), Some(7));
        assert_eq!(tab_icon_request(&format!("https://krunk.invalid{a}")), None, "the shell's only");
        assert_eq!(tab_icon_request("https://evil.example/tab-icon?id=7"), None);
        assert_eq!(tab_icon_request("https://shell.krunk.invalid/tab-icon?id=x"), None);
        assert_eq!(tab_icon_request("https://shell.krunk.invalid/tab-icon"), None);
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert_eq!(png_of_data_url(&png_data_url(png).unwrap()).as_deref(), Some(&png[..]));
        assert_eq!(png_of_data_url("data:image/svg+xml;base64,PHN2Zz4="), None);
        assert_eq!(png_of_data_url("data:image/png;base64,!!"), None);
    }

    #[test]
    fn favicon_endpoint() {
        assert_eq!(favicon_url("a.example"), "/favicon?host=a.example");
        assert_eq!(favicon_request("https://krunk.invalid/favicon?host=A.Example").as_deref(), Some("a.example"));
        assert_eq!(favicon_request("https://shell.krunk.invalid/favicon?host=x.org").as_deref(), Some("x.org"));
        assert_eq!(favicon_request("https://krunk.invalid/favicon?host=../../etc"), None);
        assert_eq!(favicon_request("https://krunk.invalid/favicon?host="), None);
        assert_eq!(favicon_request("https://krunk.invalid/favicon"), None);
        assert_eq!(favicon_request("https://evil.example/favicon?host=a.com"), None);
        assert_eq!(favicon_request("https://krunk.invalid/favicon.ico?host=a.com"), None);
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
        let shell = "https://shell.krunk.invalid/shell.html";
        let popup = "https://shell.krunk.invalid/popup.html";
        let settings = Page::Settings.url();
        assert!(allow_navigation(Channel::Shell, "", shell, true, false));
        assert!(!allow_navigation(Channel::Shell, shell, "https://evil.example/", false, false));
        assert!(!allow_navigation(Channel::Shell, shell, popup, false, false));
        assert!(allow_navigation(Channel::Popup, "", popup, true, false));
        assert!(!allow_navigation(Channel::Popup, popup, shell, false, false));

        assert!(!allow_navigation(Channel::Tab, "https://a.example/", shell, true, false), "never the shell in a tab");
        assert!(allow_navigation(Channel::Tab, "https://a.example/", &settings, true, false), "address bar");
        assert!(!allow_navigation(Channel::Tab, "https://a.example/", &settings, false, false), "page-initiated");
        assert!(allow_navigation(Channel::Tab, &Page::NewTab.url(), &settings, false, false), "built-in links");
        // 0.11.7 (audit C3): only the host opens a built-in page in a tab that shows nothing
        // yet or about:blank (a popup from window.open), and never through a redirect.
        assert!(!allow_navigation(Channel::Tab, "", &Page::NewTab.url(), false, false), "popup");
        assert!(!allow_navigation(Channel::Tab, "about:blank", &settings, false, false), "about:blank popup");
        assert!(
            !allow_navigation(
                Channel::Tab,
                &Page::NewTab.url(),
                "https://krunk.invalid/source.html?url=x",
                false,
                true
            ),
            "a site typed on the new-tab page redirects to a built-in page"
        );
        // The host's flag is used up by its own navigation: a redirect after it is the server's.
        assert!(!allow_navigation(Channel::Tab, "https://a.example/", &settings, false, true), "a server redirect");
        assert!(allow_navigation(Channel::Tab, "", &Page::NewTab.url(), true, false), "a new tab");
        assert!(
            allow_navigation(Channel::Tab, &settings, "https://a.example/", false, true),
            "built-in page to the web, redirected"
        );
        assert!(!allow_navigation(Channel::Tab, "", "https://krunk.invalid/evil.html", true, false));
        assert!(allow_navigation(Channel::Tab, &settings, "https://a.example/", false, false));
        assert!(!allow_navigation(Channel::Tab, "https://a.example/", "file:///C:/Windows/win.ini", false, false));
        assert!(allow_navigation(Channel::Tab, "", "file:///C:/x.html", true, false));
        assert!(!allow_navigation(Channel::Tab, "", "javascript:alert(1)", true, false));
        assert!(!allow_navigation(Channel::Tab, "", "ms-settings:privacy", false, false));
        // chrome://gpu only, only from the host.
        assert!(allow_navigation(Channel::Tab, &Page::Gpu.url(), "chrome://gpu/", true, false));
        assert!(!allow_navigation(Channel::Tab, &Page::Gpu.url(), "chrome://gpu/", false, false));
        assert!(!allow_navigation(Channel::Tab, "", "chrome://settings/", true, false));
        assert!(!allow_navigation(Channel::Tab, "", "chrome://gpu/../settings", true, false));
        assert!(!allow_navigation(Channel::Shell, "", "chrome://gpu/", true, false));
    }
}
