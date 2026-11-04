//! Third-party cookie decisions. A cookie is third-party when the site
//! (registrable domain, "eTLD+1") of the request that sends or receives it
//! differs from the site of the tab's top-level page.

/// The site of a host: its registrable domain per the Public Suffix List
/// (`news.example.co.uk` -> `example.co.uk`). IP addresses, `localhost` and
/// hosts that are a public suffix themselves are their own site.
pub fn site_of(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return host;
    }
    psl::domain_str(&host).map(str::to_string).unwrap_or(host)
}

fn site_of_url(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?;
    Some(site_of(host))
}

/// Is a cookie of a request to `request_url` third-party in a tab showing
/// `top_url`? Unknown or non-web top-level pages (empty, `about:blank`)
/// make every cookie third-party; a request without a host never is.
pub fn is_third_party(top_url: &str, request_url: &str) -> bool {
    let Some(request_site) = site_of_url(request_url) else { return false };
    match site_of_url(top_url) {
        Some(top_site) => top_site != request_site,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites() {
        assert_eq!(site_of("news.example.co.uk"), "example.co.uk");
        assert_eq!(site_of("www.EXAMPLE.com."), "example.com");
        assert_eq!(site_of("example.com"), "example.com");
        assert_eq!(site_of("a.b.github.io"), "b.github.io", "private suffix");
        assert_eq!(site_of("127.0.0.1"), "127.0.0.1");
        assert_eq!(site_of("localhost"), "localhost");
        assert_eq!(site_of("[::1]"), "[::1]");
    }

    #[test]
    fn first_and_third_party() {
        let top = "https://www.news.example/article";
        // Same site, any subdomain or scheme or port: first party.
        assert!(!is_third_party(top, "https://cdn.news.example/a.js"));
        assert!(!is_third_party(top, "http://news.example:8080/"));
        // Other sites: third party.
        assert!(is_third_party(top, "https://ads.tracker.example/pixel"));
        assert!(is_third_party(top, "https://news.example.evil.com/"));
        assert!(is_third_party("https://user.github.io/", "https://other.github.io/"));
        // The verify page: 127.0.0.1 with a localhost iframe.
        assert!(is_third_party("http://127.0.0.1:8081/cookie-top.html", "http://localhost:8082/set-cookie"));
        assert!(!is_third_party("http://localhost:8082/x", "http://localhost:8082/set-cookie"));
        // No known top-level page: never first party; no host: nothing to decide.
        assert!(is_third_party("", "https://a.example/"));
        assert!(is_third_party("about:blank", "https://a.example/"));
        assert!(!is_third_party(top, "data:text/plain,x"));
    }
}
