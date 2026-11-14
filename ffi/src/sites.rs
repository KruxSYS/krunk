//! Per-site records (Krunk for Android 0.4.0): the answers to permission prompts
//! (`core::permissions`, table `site_permissions`, one row per origin and kind) and the
//! grouping of cookies and visited origins into sites for the site-data screen
//! (`core::sitedata`, `core::cookies`). The kind is the app's own key (`camera`,
//! `geolocation`, ...): the store keeps any text. Private tabs never write here.
use crate::error::FfiError;
use crate::store::KrunkStore;
use lb_core::sitedata;

/// A stored answer: `allow` or block. No row means "ask".
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SitePermission {
    /// `scheme://host[:port]`.
    pub origin: String,
    pub kind: String,
    pub allow: bool,
}

/// A site (registrable domain) with what Krunk knows it holds.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SiteSummary {
    pub site: String,
    /// Cookies counted for the site (0 when the engine gave none).
    pub cookies: u32,
    /// Its web origins that were visited or hold a permission.
    pub origins: Vec<String>,
    /// Unix ms of the last visit, when known.
    pub last_used: Option<i64>,
}

fn permission(p: lb_core::permissions::SitePermission) -> SitePermission {
    SitePermission { origin: p.origin, kind: p.kind, allow: p.allow }
}

#[uniffi::export]
impl KrunkStore {
    /// The stored answer for the origin of `url` and `kind`, if any.
    pub fn site_permission(&self, url: String, kind: String) -> Result<Option<bool>, FfiError> {
        Ok(self.lock().permission(&url, &kind)?)
    }

    /// Remembers an answer; `Invalid` when `url` has no web origin.
    pub fn site_permission_set(&self, url: String, kind: String, allow: bool, now: i64) -> Result<(), FfiError> {
        if kind.is_empty() || kind.len() > 64 {
            return Err(FfiError::Invalid("Not a permission".into()));
        }
        Ok(self.lock().permission_set(&url, &kind, allow, now)?)
    }

    /// Back to "ask"; true when there was an answer.
    pub fn site_permission_remove(&self, url: String, kind: String) -> Result<bool, FfiError> {
        Ok(self.lock().permission_remove(&url, &kind)?)
    }

    pub fn site_permissions_for(&self, url: String) -> Result<Vec<SitePermission>, FfiError> {
        Ok(self.lock().permissions_for(&url)?.into_iter().map(permission).collect())
    }

    /// Every stored answer, by origin and kind.
    pub fn site_permissions_list(&self) -> Result<Vec<SitePermission>, FfiError> {
        Ok(self.lock().permissions_list()?.into_iter().map(permission).collect())
    }

    /// Forgets every answer of one origin; returns how many.
    pub fn site_permissions_clear_origin(&self, url: String) -> Result<u64, FfiError> {
        Ok(self.lock().permissions_clear_origin(&url)? as u64)
    }

    pub fn site_permissions_clear(&self) -> Result<(), FfiError> {
        Ok(self.lock().permissions_clear()?)
    }

    /// The sites Krunk knows of, by name: `cookie_domains` has one entry per cookie the engine
    /// listed (its domain, a leading dot allowed), `extra_origins` are origins known from
    /// elsewhere (open pages); the visited origins and the ones with a stored permission
    /// are added from the database, with the last visit per site.
    pub fn site_summaries(
        &self,
        cookie_domains: Vec<String>,
        extra_origins: Vec<String>,
    ) -> Result<Vec<SiteSummary>, FfiError> {
        let store = self.lock();
        let visits = store.visited_origins_last()?;
        let mut origins: Vec<String> = visits.iter().map(|(o, _)| o.clone()).collect();
        origins.extend(store.permissions_list()?.into_iter().map(|p| p.origin));
        origins.extend(extra_origins.into_iter().filter_map(|o| sitedata::web_origin(&o)));
        let cookies: Vec<sitedata::CookieInfo> = cookie_domains
            .into_iter()
            .filter(|d| !d.trim_start_matches('.').is_empty())
            .map(|domain| sitedata::CookieInfo {
                name: String::new(),
                value: String::new(),
                domain,
                path: String::new(),
                expires: None,
                secure: false,
                http_only: false,
                same_site: sitedata::SameSite::Unspecified,
                last_access: None,
            })
            .collect();
        let mut sites = sitedata::summarize(&cookies, &origins);
        sitedata::set_last_used(&mut sites, &visits);
        Ok(sites
            .into_iter()
            .map(|s| SiteSummary { site: s.site, cookies: s.cookies, origins: s.origins, last_used: s.last_used })
            .collect())
    }
}

/// `scheme://host[:port]` of an http(s) address, or `None` (Krunk's own pages, `file:`, ...).
#[uniffi::export]
pub fn web_origin(url: String) -> Option<String> {
    sitedata::web_origin(&url)
}

/// The site (registrable domain, e.g. `example.co.uk`) a host belongs to; an IP address or
/// `localhost` is its own site.
#[uniffi::export]
pub fn site_of_host(host: String) -> String {
    lb_core::cookies::site_of(&host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_kept_per_origin_and_kind() {
        let s = KrunkStore::in_memory().unwrap();
        assert_eq!(s.site_permission("https://a.test/page".into(), "camera".into()).unwrap(), None);
        s.site_permission_set("https://a.test/page?x=1".into(), "camera".into(), true, 1).unwrap();
        s.site_permission_set("https://a.test".into(), "geolocation".into(), false, 2).unwrap();
        s.site_permission_set("http://a.test:8080/".into(), "camera".into(), false, 3).unwrap();
        assert_eq!(s.site_permission("https://a.test/other".into(), "camera".into()).unwrap(), Some(true));
        assert_eq!(s.site_permission("https://a.test/".into(), "geolocation".into()).unwrap(), Some(false));
        assert_eq!(s.site_permission("http://a.test:8080/".into(), "camera".into()).unwrap(), Some(false));
        assert_eq!(s.site_permission("https://sub.a.test/".into(), "camera".into()).unwrap(), None);
        // A kind Windows does not know is kept as well.
        s.site_permission_set("https://a.test/".into(), "persistentStorage".into(), true, 4).unwrap();
        let mine = s.site_permissions_for("https://a.test/".into()).unwrap();
        assert_eq!(
            mine.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(),
            ["camera", "geolocation", "persistentStorage"]
        );
        assert_eq!(s.site_permissions_list().unwrap().len(), 4);
        // Changing an answer replaces it; removing goes back to "ask".
        s.site_permission_set("https://a.test/".into(), "camera".into(), false, 5).unwrap();
        assert_eq!(s.site_permission("https://a.test/".into(), "camera".into()).unwrap(), Some(false));
        assert!(s.site_permission_remove("https://a.test/".into(), "camera".into()).unwrap());
        assert!(!s.site_permission_remove("https://a.test/".into(), "camera".into()).unwrap());
        assert_eq!(s.site_permissions_clear_origin("https://a.test/x".into()).unwrap(), 2);
        assert_eq!(s.site_permissions_list().unwrap().len(), 1);
        s.site_permissions_clear().unwrap();
        assert!(s.site_permissions_list().unwrap().is_empty());
    }

    #[test]
    fn what_is_not_a_site_is_refused() {
        let s = KrunkStore::in_memory().unwrap();
        assert!(matches!(
            s.site_permission_set("about:blank".into(), "camera".into(), true, 1),
            Err(FfiError::Invalid(_))
        ));
        assert!(matches!(
            s.site_permission_set("https://a.test/".into(), String::new(), true, 1),
            Err(FfiError::Invalid(_))
        ));
        assert_eq!(s.site_permission("not a url".into(), "camera".into()).unwrap(), None);
        assert_eq!(web_origin("https://a.test:8443/p?q#f".into()).as_deref(), Some("https://a.test:8443"));
        assert_eq!(web_origin("file:///sdcard/x".into()), None);
        assert_eq!(site_of_host("www.shop.example.co.uk".into()), "example.co.uk");
        assert_eq!(site_of_host("10.0.2.2".into()), "10.0.2.2");
    }

    #[test]
    fn sites_from_cookies_visits_and_permissions() {
        let s = KrunkStore::in_memory().unwrap();
        s.record_visit("https://www.news.example/a".into(), "A".into(), 1_000).unwrap();
        s.record_visit("https://m.news.example/b".into(), "B".into(), 5_000).unwrap();
        s.site_permission_set("https://cam.test/".into(), "camera".into(), true, 1).unwrap();
        let sites = s
            .site_summaries(
                vec![".news.example".into(), "www.news.example".into(), "tracker.test".into(), ".".into()],
                vec!["http://10.0.2.2:18080/page".into(), "about:blank".into()],
            )
            .unwrap();
        let names: Vec<&str> = sites.iter().map(|x| x.site.as_str()).collect();
        assert_eq!(names, ["10.0.2.2", "cam.test", "news.example", "tracker.test"]);
        let news = &sites[2];
        assert_eq!(news.cookies, 2);
        assert_eq!(news.origins, ["https://m.news.example", "https://www.news.example"]);
        assert_eq!(news.last_used, Some(5_000));
        assert_eq!((sites[3].cookies, sites[3].origins.len(), sites[3].last_used), (1, 0, None));
        assert_eq!(sites[0].origins, ["http://10.0.2.2:18080"]);
    }
}
