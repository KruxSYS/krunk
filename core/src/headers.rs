//! Request headers (Krunk 0.6.0, Settings → Advanced → Request headers): the user agent
//! and its client hints, `Accept-Language`, custom headers and removals with optional
//! per-site scope, and the rules about which headers can be edited at all. The app applies
//! the user agent and hints through DevTools (`Emulation.setUserAgentOverride`, so the
//! headers and `navigator.userAgentData` agree), `Accept-Language` through the engine
//! preference, and the custom rules in its request hook. Everything here is pure.

use serde::{Deserialize, Serialize};

/// Longest header value accepted.
pub const MAX_VALUE_LEN: usize = 4096;
pub const MAX_RULES: usize = 50;

/// Headers the network stack adds after Krunk's hook (or that must not be forged): shown
/// read-only on the settings page with the reason.
pub const READ_ONLY: &[(&str, &str)] = &[
    ("Host", "set by the network stack from the address"),
    ("Connection", "managed by the network stack"),
    ("Content-Length", "computed from the request body"),
    ("Accept-Encoding", "the engine negotiates compression itself"),
    ("Cookie", "the cookie jar (see Cookies and site data)"),
    ("Referer", "the tracking-protection level decides what other sites learn"),
    ("Origin", "security-relevant: set by the engine"),
    ("DNT", "Settings → Privacy → Do Not Track"),
    ("Sec-GPC", "Settings → Privacy → Global Privacy Control"),
    ("Sec-Fetch-Site", "added by the network stack after Krunk's hook"),
    ("Sec-Fetch-Mode", "added by the network stack after Krunk's hook"),
    ("Sec-Fetch-Dest", "added by the network stack after Krunk's hook"),
    ("Sec-Fetch-User", "added by the network stack after Krunk's hook"),
    ("Priority", "added by the network stack after Krunk's hook"),
    ("Upgrade-Insecure-Requests", "added by the engine for navigations"),
];

/// Header-name prefixes that are never editable as a custom rule.
const FORBIDDEN_PREFIXES: &[&str] = &["sec-", "proxy-"];
/// Hop-by-hop and framing headers a custom rule may not touch either.
const FORBIDDEN: &[&str] = &["transfer-encoding", "upgrade", "te", "trailer", "keep-alive", "expect", "via"];
/// Edited through their own fields, not as custom rules.
const OWN_FIELDS: &[&str] = &["user-agent", "accept-language"];

/// One brand of the `Sec-CH-UA` list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Brand {
    pub brand: String,
    pub version: String,
}

/// The client hints (`Sec-CH-UA*` and `navigator.userAgentData`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientHints {
    /// The low-entropy brand list (major versions).
    pub brands: Vec<Brand>,
    /// The full version of the engine brand (`Sec-CH-UA-Full-Version-List`); empty: derived.
    pub full_version: String,
    pub platform: String,
    pub platform_version: String,
    pub architecture: String,
    pub bitness: String,
    pub model: String,
    pub mobile: bool,
    pub wow64: bool,
}

impl Default for ClientHints {
    fn default() -> ClientHints {
        ClientHints::for_engine("156")
    }
}

impl ClientHints {
    /// What Chromium `major` sends on 64-bit Windows 10/11 (the engine's own values).
    pub fn for_engine(major: &str) -> ClientHints {
        ClientHints {
            brands: vec![
                Brand { brand: "Not:A-Brand".into(), version: "8".into() },
                Brand { brand: "Chromium".into(), version: major.into() },
            ],
            full_version: String::new(),
            platform: "Windows".into(),
            platform_version: "15.0.0".into(),
            architecture: "x86".into(),
            bitness: "64".into(),
            model: String::new(),
            mobile: false,
            wow64: false,
        }
    }

    /// The engine's own values as a page reports them: `navigator.userAgentData` merged with
    /// `getHighEntropyValues()` (`{brands, mobile, platform, platformVersion, architecture,
    /// bitness, model, uaFullVersion, wow64}`).
    pub fn from_user_agent_data(v: &serde_json::Value) -> Option<ClientHints> {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let brands: Vec<Brand> = v
            .get("brands")?
            .as_array()?
            .iter()
            .filter_map(|b| {
                Some(Brand { brand: b.get("brand")?.as_str()?.into(), version: b.get("version")?.as_str()?.into() })
            })
            .collect();
        if brands.is_empty() || s("platform").is_empty() {
            return None;
        }
        Some(ClientHints {
            brands,
            full_version: s("uaFullVersion"),
            platform: s("platform"),
            platform_version: s("platformVersion"),
            architecture: s("architecture"),
            bitness: s("bitness"),
            model: s("model"),
            mobile: v.get("mobile").and_then(|x| x.as_bool()).unwrap_or(false),
            wow64: v.get("wow64").and_then(|x| x.as_bool()).unwrap_or(false),
        })
    }

    /// The `Sec-CH-UA` header value.
    pub fn sec_ch_ua(&self) -> String {
        self.brands.iter().map(|b| format!("\"{}\";v=\"{}\"", b.brand, b.version)).collect::<Vec<_>>().join(", ")
    }

    /// The full-version list: each brand with the full version where it is the engine
    /// brand (its version starts the full version), else its own version.
    pub fn full_version_list(&self, engine_full: &str) -> Vec<Brand> {
        let full = if self.full_version.is_empty() { engine_full } else { &self.full_version };
        self.brands
            .iter()
            .map(|b| Brand {
                brand: b.brand.clone(),
                version: if full.starts_with(&format!("{}.", b.version)) {
                    full.to_string()
                } else {
                    b.version.clone()
                },
            })
            .collect()
    }

    /// Parses a `Sec-CH-UA`-style list (`"Chromium";v="156", "Krunk";v="0.6"`); lenient
    /// about spaces and quotes.
    pub fn parse_brands(text: &str) -> Vec<Brand> {
        text.split(',')
            .filter_map(|part| {
                let (brand, version) = part.split_once(";v=").or_else(|| part.split_once(";"))?;
                let brand = brand.trim().trim_matches('"').trim();
                let version = version.trim().trim_start_matches("v=").trim_matches('"').trim();
                (!brand.is_empty() && !version.is_empty())
                    .then(|| Brand { brand: brand.into(), version: version.into() })
            })
            .collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.brands.is_empty() || self.brands.len() > 6 {
            return Err("Enter one to six brands".into());
        }
        for b in &self.brands {
            if !token_text(&b.brand, 64) || !token_text(&b.version, 32) {
                return Err("Brands and versions are printable ASCII without quotes".into());
            }
        }
        for (name, v, max) in [
            ("platform", &self.platform, 64),
            ("platform version", &self.platform_version, 32),
            ("architecture", &self.architecture, 32),
            ("bitness", &self.bitness, 8),
            ("model", &self.model, 64),
            ("full version", &self.full_version, 32),
        ] {
            if !token_text(v, max) {
                return Err(format!("The {name} is printable ASCII without quotes, at most {max} characters"));
            }
        }
        if self.platform.is_empty() {
            return Err("Enter a platform".into());
        }
        Ok(())
    }
}

fn token_text(s: &str, max: usize) -> bool {
    s.len() <= max && s.chars().all(|c| c.is_ascii_graphic() || c == ' ') && !s.contains('"')
}

/// A custom header: set (`value`) or removed (`None`), everywhere or on the listed sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HeaderRule {
    pub name: String,
    /// `None` removes the header.
    pub value: Option<String>,
    /// Registrable domains or hosts the rule applies to (empty: every site). Matched against
    /// the page's host (the rule follows the site you are on, not the request's target).
    pub sites: Vec<String>,
}

impl HeaderRule {
    /// Does the rule apply on a page at `host`?
    pub fn applies_to(&self, host: &str) -> bool {
        if self.sites.is_empty() {
            return true;
        }
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.sites.iter().any(|s| {
            let s = s.to_ascii_lowercase();
            host == s || host.ends_with(&format!(".{s}"))
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        valid_name(&self.name)?;
        if let Some(v) = &self.value {
            if v.len() > MAX_VALUE_LEN || v.chars().any(|c| c.is_control() || !c.is_ascii()) {
                return Err("The value is printable ASCII, at most 4096 characters".into());
            }
        }
        if self.sites.len() > 50 {
            return Err("At most 50 sites per rule".into());
        }
        for s in &self.sites {
            let ok = !s.is_empty()
                && s.len() <= 253
                && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                && !s.starts_with('.')
                && !s.ends_with('.');
            if !ok {
                return Err(format!("Not a site: {s}"));
            }
        }
        Ok(())
    }
}

/// Why a name cannot be a custom rule, or `Ok`.
pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("Enter a header name".into());
    }
    if !name.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '#' | '$' | '%' | '&' | '\'' | '*' | '+' | '^' | '`' | '|' | '~')
    }) {
        return Err("A header name is letters, digits and - only".into());
    }
    let lower = name.to_ascii_lowercase();
    if OWN_FIELDS.contains(&lower.as_str()) {
        return Err(format!("{name} has its own field above"));
    }
    if let Some((_, why)) = READ_ONLY.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Err(format!("{name} is read-only: {why}"));
    }
    if FORBIDDEN_PREFIXES.iter().any(|p| lower.starts_with(p)) || FORBIDDEN.contains(&lower.as_str()) {
        return Err(format!("{name} is managed by the engine"));
    }
    Ok(())
}

/// The user-agent client hint request headers (`Sec-CH-UA` and the high-entropy ones).
pub const CLIENT_HINT_HEADERS: &[&str] = &[
    "Sec-CH-UA",
    "Sec-CH-UA-Mobile",
    "Sec-CH-UA-Platform",
    "Sec-CH-UA-Platform-Version",
    "Sec-CH-UA-Arch",
    "Sec-CH-UA-Bitness",
    "Sec-CH-UA-Model",
    "Sec-CH-UA-Full-Version",
    "Sec-CH-UA-Full-Version-List",
    "Sec-CH-UA-WoW64",
    "Sec-CH-UA-Form-Factors",
];

/// The request-header settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Headers {
    /// `Accept-Language` as a comma list without q-values (the engine adds them); empty: the
    /// engine's default (the system language).
    pub accept_language: String,
    /// Client hints; `None`: the engine's own (no override unless the user agent is changed).
    pub hints: Option<ClientHints>,
    /// Send client hints at all (0.7.0, R12; on by default). Off: no `Sec-CH-UA*` header on
    /// any request, `Accept-CH`/`Critical-CH` change nothing, and pages and workers have no
    /// `navigator.userAgentData`.
    pub send_client_hints: bool,
    pub rules: Vec<HeaderRule>,
}

impl Default for Headers {
    fn default() -> Headers {
        Headers { accept_language: String::new(), hints: None, send_client_hints: true, rules: Vec::new() }
    }
}

impl Headers {
    /// Does this need the DevTools override (beyond a custom user-agent string)? The
    /// language does too: the override is the only way to change it at runtime (with the
    /// engine's own hints, so nothing else changes); no client hints at all does as well.
    pub fn overrides(&self) -> bool {
        self.hints.is_some() || !self.accept_language.is_empty() || !self.send_client_hints
    }

    /// Request-header changes beyond the custom rules: with client hints off, every
    /// `Sec-CH-UA*` header is removed (a safety net behind the override, which already keeps
    /// the engine from adding them).
    pub fn hint_removals(&self) -> Vec<(String, Option<String>)> {
        if self.send_client_hints {
            return Vec::new();
        }
        CLIENT_HINT_HEADERS.iter().map(|n| (n.to_string(), None)).collect()
    }

    pub fn validate(&self) -> Result<(), Vec<(&'static str, String)>> {
        let mut errs = Vec::new();
        if !self.accept_language.is_empty() {
            let ok = self.accept_language.len() <= 256
                && self.accept_language.split(',').all(|t| {
                    let t = t.trim();
                    !t.is_empty() && t.len() <= 35 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                });
            if !ok {
                errs.push(("headers.acceptLanguage", "A comma list of language tags like en-GB, en, de".to_string()));
            }
        }
        if let Some(h) = &self.hints {
            if let Err(e) = h.validate() {
                errs.push(("headers.hints", e));
            }
        }
        if self.rules.len() > MAX_RULES {
            errs.push(("headers.rules", format!("At most {MAX_RULES} custom headers")));
        }
        for r in &self.rules {
            if let Err(e) = r.validate() {
                errs.push(("headers.rules", format!("{}: {e}", r.name)));
                break;
            }
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }

    /// The rules in force on a page at `host`: later rules win over earlier ones with the
    /// same name.
    pub fn rules_for(&self, host: &str) -> Vec<(String, Option<String>)> {
        let mut out: Vec<(String, Option<String>)> = Vec::new();
        for r in self.rules.iter().filter(|r| r.applies_to(host)) {
            out.retain(|(n, _)| !n.eq_ignore_ascii_case(&r.name));
            out.push((r.name.clone(), r.value.clone()));
        }
        out
    }

    /// The `Accept-Language` list as the engine wants it (no spaces).
    pub fn accept_language_list(&self) -> String {
        self.accept_language.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(",")
    }
}

/// The DevTools `Emulation.setUserAgentOverride` parameters for a user agent, language and
/// hints (`engine_full` is the engine's full version for the full-version list). Without
/// hints the override carries no metadata, so the engine sends no client hints at all (a
/// custom user-agent string then looks like a browser without them, as in 0.5); the language
/// is only part of the override when the hints are (it is the profile preference otherwise).
pub fn user_agent_override(
    user_agent: &str,
    accept_language: &str,
    hints: Option<&ClientHints>,
    engine_full: &str,
) -> serde_json::Value {
    let mut params = serde_json::json!({ "userAgent": user_agent });
    if !accept_language.is_empty() && hints.is_some() {
        params["acceptLanguage"] = serde_json::Value::String(accept_language.to_string());
    }
    if let Some(h) = hints {
        params["platform"] = serde_json::Value::String(h.platform.clone());
        params["userAgentMetadata"] = serde_json::json!({
            "brands": h.brands,
            "fullVersionList": h.full_version_list(engine_full),
            "platform": h.platform,
            "platformVersion": h.platform_version,
            "architecture": h.architecture,
            "model": h.model,
            "mobile": h.mobile,
            "bitness": h.bitness,
            "wow64": h.wow64,
        });
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_hints_can_be_off() {
        let on = Headers::default();
        assert!(on.send_client_hints && !on.overrides() && on.hint_removals().is_empty());
        // Older settings (0.6.0) have no field: on.
        let old: Headers = serde_json::from_str(r#"{"acceptLanguage":"","rules":[]}"#).unwrap();
        assert!(old.send_client_hints);
        let off = Headers { send_client_hints: false, ..Headers::default() };
        assert!(off.overrides(), "the override without metadata is what switches them off");
        let removed: Vec<String> = off.hint_removals().into_iter().map(|(n, v)| {
            assert!(v.is_none());
            n.to_ascii_lowercase()
        }).collect();
        for h in ["sec-ch-ua", "sec-ch-ua-mobile", "sec-ch-ua-platform", "sec-ch-ua-full-version-list", "sec-ch-ua-arch"] {
            assert!(removed.contains(&h.to_string()), "{h}");
        }
        // The override without hints carries no metadata (so the engine adds no hints).
        let p = user_agent_override("UA/1", "", None, "156.0.1.2");
        assert!(p.get("userAgentMetadata").is_none());
        assert_eq!(serde_json::to_value(&off).unwrap()["sendClientHints"], false);
    }

    #[test]
    fn hints_defaults_and_header_text() {
        let h = ClientHints::for_engine("156");
        assert_eq!(h.sec_ch_ua(), "\"Not:A-Brand\";v=\"8\", \"Chromium\";v=\"156\"");
        let full = h.full_version_list("156.0.8078.80");
        assert_eq!(full[1].version, "156.0.8078.80", "the engine brand gets the full version");
        assert_eq!(full[0].version, "8", "the other brand keeps its own");
        let mut custom = h.clone();
        custom.full_version = "157.0.1.2".into();
        custom.brands[1].version = "157".into();
        assert_eq!(custom.full_version_list("156.0.8078.80")[1].version, "157.0.1.2");
        assert_eq!(
            ClientHints::parse_brands("\"Chromium\";v=\"156\", \"Krunk\";v=\"0.6\""),
            [
                Brand { brand: "Chromium".into(), version: "156".into() },
                Brand { brand: "Krunk".into(), version: "0.6".into() }
            ]
        );
        assert_eq!(ClientHints::parse_brands("Chromium;156, nonsense").len(), 1);
        assert!(h.validate().is_ok());
        assert!(ClientHints { brands: vec![], ..h.clone() }.validate().is_err());
        assert!(ClientHints { platform: "Win\"dows".into(), ..h.clone() }.validate().is_err());
        assert!(ClientHints { platform: String::new(), ..h }.validate().is_err());
    }

    #[test]
    fn rule_names_and_scopes() {
        assert!(valid_name("X-Krunk-Test").is_ok());
        assert!(valid_name("Accept").is_ok());
        assert!(valid_name("").is_err());
        assert!(valid_name("Bad Name").is_err());
        assert!(valid_name("User-Agent").unwrap_err().contains("own field"));
        assert!(valid_name("cookie").unwrap_err().contains("read-only"));
        assert!(valid_name("Sec-CH-UA-Platform").unwrap_err().contains("managed"));
        assert!(valid_name("Proxy-Authorization").is_err());
        assert!(valid_name("Transfer-Encoding").is_err());
        let r = HeaderRule { name: "X-A".into(), value: Some("1".into()), sites: vec!["example.com".into()] };
        assert!(r.applies_to("example.com") && r.applies_to("www.Example.com.") && !r.applies_to("notexample.com"));
        assert!(HeaderRule { sites: vec![], ..r.clone() }.applies_to("anything.test"));
        assert!(r.validate().is_ok());
        assert!(HeaderRule { value: Some("bad\u{e9}".into()), ..r.clone() }.validate().is_err());
        assert!(HeaderRule { sites: vec![".bad".into()], ..r }.validate().is_err());
    }

    #[test]
    fn headers_settings() {
        let mut h = Headers::default();
        assert!(!h.overrides() && h.validate().is_ok());
        h.accept_language = "de-DE, de, en".into();
        assert_eq!(h.accept_language_list(), "de-DE,de,en");
        assert!(h.overrides(), "the language needs the override too");
        h.rules = vec![
            HeaderRule { name: "X-A".into(), value: Some("1".into()), sites: vec![] },
            HeaderRule { name: "x-a".into(), value: Some("2".into()), sites: vec!["a.example".into()] },
            HeaderRule { name: "Accept".into(), value: None, sites: vec!["b.example".into()] },
        ];
        assert_eq!(h.rules_for("www.a.example"), [("x-a".to_string(), Some("2".to_string()))], "the later rule wins");
        assert_eq!(h.rules_for("other.test"), [("X-A".to_string(), Some("1".to_string()))]);
        assert_eq!(h.rules_for("b.example").len(), 2);
        assert!(h.validate().is_ok());
        h.accept_language = "de DE".into();
        assert_eq!(h.validate().unwrap_err()[0].0, "headers.acceptLanguage");
        h.accept_language.clear();
        h.rules.push(HeaderRule { name: "Cookie".into(), value: None, sites: vec![] });
        assert_eq!(h.validate().unwrap_err()[0].0, "headers.rules");
        let json = serde_json::to_string(&Headers::default()).unwrap();
        assert!(json.contains("\"hints\":null") && json.contains("\"rules\":[]"));
    }

    #[test]
    fn engine_hints_from_user_agent_data() {
        let v = serde_json::json!({ "brands": [{ "brand": "Not:A-Brand", "version": "8" }, { "brand": "Chromium", "version": "156" }],
            "mobile": false, "platform": "Windows", "platformVersion": "19.0.0", "architecture": "x86", "bitness": "64", "model": "",
            "uaFullVersion": "156.0.8078.80", "wow64": false });
        let h = ClientHints::from_user_agent_data(&v).unwrap();
        assert_eq!(
            (h.platform.as_str(), h.platform_version.as_str(), h.full_version.as_str()),
            ("Windows", "19.0.0", "156.0.8078.80")
        );
        assert_eq!(h.full_version_list("x")[1].version, "156.0.8078.80", "the reported full version wins");
        assert!(ClientHints::from_user_agent_data(&serde_json::json!({ "brands": [] })).is_none());
    }

    #[test]
    fn devtools_override_parameters() {
        let p = user_agent_override("UA/1", "", None, "156.0.1.2");
        assert_eq!(p, serde_json::json!({ "userAgent": "UA/1" }));
        assert_eq!(user_agent_override("UA/1", "de", None, "156.0.1.2"), p, "no language without hints");
        let h = ClientHints { platform: "Krunk OS".into(), ..ClientHints::for_engine("156") };
        let p = user_agent_override("UA/1", "de-DE,de", Some(&h), "156.0.1.2");
        assert_eq!(p["acceptLanguage"], "de-DE,de");
        assert_eq!(p["platform"], "Krunk OS");
        assert_eq!(p["userAgentMetadata"]["platform"], "Krunk OS");
        assert_eq!(p["userAgentMetadata"]["fullVersionList"][1]["version"], "156.0.1.2");
        assert_eq!(p["userAgentMetadata"]["mobile"], false);
    }
}
