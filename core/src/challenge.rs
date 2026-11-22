//! Bot-check (challenge) hosts (Krunk 0.28.0): captchas and anti-bot services a site uses to
//! tell people from bots. Blocking any part of them doesn't protect privacy, it only makes the
//! check fail or loop (GitHub's Arkose puzzle, Cloudflare Turnstile, DataDome's slider): the
//! site then treats the visitor as a bot. So at every protection level their requests, and the
//! requests their frames make, skip the filter lists; their cookies and referrer are left to the
//! engine; and bounce cleanup never wipes them (nor the big sign-in providers, which are
//! routinely only seen as a redirect hop). Pure; the app applies it (`browser::blocking`,
//! `browser::bounce`). See `docs/TRACKING_PROTECTION.md`.

use crate::cookies::site_of;

/// Hosts (and their subdomains) that only serve bot checks.
pub const CHALLENGE_HOSTS: &[&str] = &[
    // Arkose Labs / FunCaptcha (GitHub, Microsoft, EA, Roblox …) and GitHub's frame for it.
    "arkoselabs.com",
    "arkoselabs.cn",
    "funcaptcha.com",
    "octocaptcha.com",
    // Cloudflare Turnstile and managed challenges.
    "challenges.cloudflare.com",
    // hCaptcha.
    "hcaptcha.com",
    "hcaptcha.net",
    // reCAPTCHA's own domain (the google.com/gstatic.com parts are in `CHALLENGE_PATHS`).
    "recaptcha.net",
    // DataDome (js.datadome.co, its captcha pages on captcha-delivery.com).
    "datadome.co",
    "captcha-delivery.com",
    // HUMAN / PerimeterX.
    "perimeterx.net",
    "px-cdn.net",
    "px-cloud.net",
    "pxchk.net",
    // Radware Bot Manager (ShieldSquare).
    "perfdrive.com",
    // AWS WAF captcha.
    "awswaf.com",
    // GeeTest.
    "geetest.com",
    "geevisit.com",
];

/// Bot checks served from a shared host: host and path prefix.
pub const CHALLENGE_PATHS: &[(&str, &str)] =
    &[("www.google.com", "/recaptcha/"), ("google.com", "/recaptcha/"), ("www.gstatic.com", "/recaptcha/")];

/// Sign-in providers (registrable domains) bounce cleanup leaves alone: a sign-in through them
/// is a redirect hop by nature, and wiping them signs the user out everywhere.
pub const IDP_SITES: &[&str] =
    &["google.com", "microsoftonline.com", "live.com", "microsoft.com", "apple.com", "github.com", "okta.com"];

fn host_matches(host: &str, list: &[&str]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    list.iter().any(|h| host == *h || host.strip_suffix(h).is_some_and(|rest| rest.ends_with('.')))
}

fn parts(url: &str) -> Option<(String, String)> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    Some((u.host_str()?.to_ascii_lowercase(), u.path().to_string()))
}

/// Is `url` part of a bot check?
pub fn is_challenge_url(url: &str) -> bool {
    let Some((host, path)) = parts(url) else { return false };
    host_matches(&host, CHALLENGE_HOSTS) || CHALLENGE_PATHS.iter().any(|(h, p)| host == *h && path.starts_with(p))
}

/// Is this request part of a bot check: to a challenge URL, or made by a challenge document
/// (`initiator`: the origin of the document that made it, empty for the browser's own)?
/// An origin carries no path, so only `CHALLENGE_HOSTS` count for the initiator.
pub fn is_challenge_request(url: &str, initiator: &str) -> bool {
    if is_challenge_url(url) {
        return true;
    }
    parts(initiator).is_some_and(|(host, _)| host_matches(&host, CHALLENGE_HOSTS))
}

/// May bounce cleanup clear this site (a registrable domain)?
pub fn bounce_exempt(site: &str) -> bool {
    let site = site.trim_end_matches('.').to_ascii_lowercase();
    IDP_SITES.contains(&site.as_str()) || CHALLENGE_HOSTS.iter().any(|h| site_of(h) == site)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_urls() {
        for u in [
            "https://client-api.arkoselabs.com/v2/123/api.js",
            "https://github-api.arkoselabs.com/fc/gt2/public_key/x",
            "https://octocaptcha.com/?origin_page=github_signup",
            "https://www.arkoselabs.com/api/x",
            "https://challenges.cloudflare.com/turnstile/v0/api.js",
            "https://newassets.hcaptcha.com/captcha/v1/x/hcaptcha.html",
            "https://www.google.com/recaptcha/api2/anchor?k=x",
            "https://www.gstatic.com/recaptcha/releases/x/recaptcha__en.js",
            "https://www.recaptcha.net/recaptcha/api.js",
            "https://js.datadome.co/tags.js",
            "https://geo.captcha-delivery.com/captcha/?x",
            "https://client.perimeterx.net/PXabc/main.min.js",
            "HTTPS://CLIENT-API.ARKOSELABS.COM./x",
        ] {
            assert!(is_challenge_url(u), "{u}");
        }
        for u in [
            "https://www.google.com/search?q=recaptcha",
            "https://www.gstatic.com/images/x.png",
            "https://notarkoselabs.com/x",
            "https://arkoselabs.com.evil.example/x",
            "https://cloudflare.com/",
            "https://example.com/recaptcha/",
            "krunk://newtab",
            "",
        ] {
            assert!(!is_challenge_url(u), "{u}");
        }
    }

    #[test]
    fn requests_from_challenge_frames() {
        // Arkose's frame loads its assets from another host.
        assert!(is_challenge_request("https://cdn.example-assets.net/a.png", "https://client-api.arkoselabs.com"));
        assert!(is_challenge_request("https://arkoselabs.com/metrics/x", "https://github.com"), "EasyPrivacy's rule");
        assert!(!is_challenge_request("https://tracker.example/p", "https://github.com"));
        assert!(!is_challenge_request("https://tracker.example/p", ""));
        // A path-only entry doesn't make the whole of google.com a challenge initiator.
        assert!(!is_challenge_request("https://ads.example/p", "https://www.google.com"));
    }

    #[test]
    fn bounce_exemptions() {
        for s in ["google.com", "microsoftonline.com", "github.com", "arkoselabs.com", "hcaptcha.com", "cloudflare.com"]
        {
            assert!(bounce_exempt(s), "{s}");
        }
        assert!(bounce_exempt("GitHub.com."));
        assert!(!bounce_exempt("tracker.example"));
        assert!(!bounce_exempt("doubleclick.net"));
    }
}
