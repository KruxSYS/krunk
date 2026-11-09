//! Extensions (Krunk 0.6.0, R26): Krunk's own subset of Chrome's Manifest V3.
//!
//! CEF 156 has no extension API, so Krunk runs extensions itself: content scripts in an
//! isolated world of each tab (through DevTools), a `chrome.*` shim with the APIs Krunk can
//! answer correctly, extension pages on `https://<id>.ext.krunk.invalid`, and static
//! `declarativeNetRequest` rules compiled into the blocker. This module is the pure part:
//! manifests, match patterns, CRX3 and zip unpacking, the rule translation, the API survey
//! and the isolation rules. The host (`app/src/browser/ext.rs`) owns installation and
//! runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Largest package Krunk unpacks (bytes, compressed and uncompressed).
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;
/// Most files in a package.
pub const MAX_PACKAGE_FILES: usize = 5000;
/// Largest manifest.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// An extension id: 32 letters a–p (the first 128 bits of the SHA-256 of the public key).
pub const ID_LEN: usize = 32;
/// Extension pages live on this host suffix, one origin per extension.
pub const HOST_SUFFIX: &str = ".ext.krunk.invalid";
/// The isolated worlds the content scripts run in and the bindings the shim calls into: one
/// of each per extension, `krunk-ext-<id>` and `__krunkExt_<id>` (0.11.7, audit C4: with one
/// shared world, an extension could act as another one). The host takes the extension from
/// the binding's name, never from the message.
pub const WORLD_NAME: &str = "krunk-ext";
pub const BINDING_NAME: &str = "__krunkExt";

/// The isolated world of extension `id`'s content scripts.
pub fn world_name(id: &str) -> String {
    format!("{WORLD_NAME}-{id}")
}

/// The extension whose world this is.
pub fn ext_of_world(name: &str) -> Option<&str> {
    name.strip_prefix(WORLD_NAME)?.strip_prefix('-').filter(|id| valid_id(id))
}

/// The binding of extension `id`'s world.
pub fn binding_name(id: &str) -> String {
    format!("{BINDING_NAME}_{id}")
}

/// The extension whose binding this is.
pub fn ext_of_binding(name: &str) -> Option<&str> {
    name.strip_prefix(BINDING_NAME)?.strip_prefix('_').filter(|id| valid_id(id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtError {
    /// The package or manifest is not usable; the text goes to the user.
    Invalid(String),
    /// A limit (size, count) was exceeded.
    TooLarge(&'static str),
}

impl std::fmt::Display for ExtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtError::Invalid(s) => write!(f, "{s}"),
            ExtError::TooLarge(what) => write!(f, "the package is too large ({what})"),
        }
    }
}

// ------------------------------------------------------------------ ids

pub fn valid_id(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Chrome's id of a public key (DER, SubjectPublicKeyInfo): the first 16 bytes of its
/// SHA-256, each nibble mapped to `a`–`p`.
pub fn id_from_public_key(der: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(der);
    hash[..16].iter().flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect()
}

/// The id of an unpacked folder (Chrome does the same: the hash of the absolute path).
pub fn id_from_path(path: &Path) -> String {
    id_from_public_key(path.to_string_lossy().as_bytes())
}

/// The origin of an extension's pages.
pub fn origin(id: &str) -> String {
    format!("https://{id}{HOST_SUFFIX}")
}

/// The extension id of a URL on an extension origin.
pub fn id_of_url(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if u.scheme() != "https" {
        return None;
    }
    let host = u.host_str()?;
    let id = host.strip_suffix(HOST_SUFFIX)?;
    valid_id(id).then(|| id.to_string())
}

/// May an extension's content world in a document of `origin` (CDP's
/// `ExecutionContextDescription.origin`) be used: messages, deliveries, `scripting.*`
/// (0.11.7, audit C1)? The world is created in every document of a tab, Krunk's own pages
/// included. Those, extension pages and the engine's own pages are refused; anything else is
/// as before (web pages, `file:`, opaque origins).
pub fn content_context_allowed(origin: &str) -> bool {
    let origin = origin.trim();
    let Ok(u) = url::Url::parse(origin) else {
        // "null", "" or "://": an opaque origin (a data: or sandboxed document).
        return true;
    };
    if matches!(u.scheme(), "chrome" | "chrome-extension" | "chrome-untrusted" | "devtools") {
        return false;
    }
    match u.host_str() {
        Some(h) => {
            let h = h.trim_end_matches('.').to_ascii_lowercase();
            h != "krunk.invalid" && !h.ends_with(".krunk.invalid")
        }
        None => true,
    }
}

/// `scripting.executeScript` and `scripting.insertCSS` need the `scripting` permission, as in
/// Chrome (0.11.7, audit C1).
pub fn may_use_scripting(m: &Manifest) -> bool {
    m.permissions.iter().any(|p| p == "scripting")
}

// ------------------------------------------------------------------ manifest

/// The parts of a Manifest V3 manifest Krunk uses.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct Manifest {
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    pub description: String,
    pub icons: BTreeMap<String, String>,
    pub permissions: Vec<String>,
    pub optional_permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub content_scripts: Vec<ContentScript>,
    pub background: Option<Background>,
    pub action: Option<Action>,
    pub options_page: Option<String>,
    pub options_ui: Option<OptionsUi>,
    pub declarative_net_request: Option<Dnr>,
    pub web_accessible_resources: Vec<serde_json::Value>,
    pub default_locale: Option<String>,
    pub minimum_chrome_version: Option<String>,
    pub content_security_policy: Option<serde_json::Value>,
    pub commands: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct ContentScript {
    pub matches: Vec<String>,
    pub exclude_matches: Vec<String>,
    pub js: Vec<String>,
    pub css: Vec<String>,
    pub run_at: Option<String>,
    pub all_frames: bool,
    pub match_about_blank: bool,
    pub world: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct Background {
    pub service_worker: Option<String>,
    pub scripts: Vec<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct Action {
    pub default_popup: Option<String>,
    pub default_title: Option<String>,
    pub default_icon: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct OptionsUi {
    pub page: Option<String>,
    pub open_in_tab: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct Dnr {
    pub rule_resources: Vec<RuleResource>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct RuleResource {
    pub id: String,
    pub enabled: bool,
    pub path: String,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ExtError> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(ExtError::TooLarge("manifest"));
        }
        // Chrome accepts a UTF-8 BOM and comments do not occur in practice.
        let text = text.trim_start_matches('\u{feff}');
        let m: Manifest =
            serde_json::from_str(text).map_err(|e| ExtError::Invalid(format!("manifest.json is not valid: {e}")))?;
        if m.manifest_version != 3 {
            return Err(ExtError::Invalid(format!(
                "manifest_version {} is not supported: Krunk runs Manifest V3 extensions",
                m.manifest_version
            )));
        }
        if m.name.trim().is_empty() || m.name.chars().count() > 75 {
            return Err(ExtError::Invalid("the manifest needs a name (75 characters at most)".into()));
        }
        if m.version.is_empty() || !m.version.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(ExtError::Invalid(format!("the version \"{}\" is not 1 to 4 numbers", m.version)));
        }
        for cs in &m.content_scripts {
            if cs.matches.is_empty() {
                return Err(ExtError::Invalid("a content script has no matches".into()));
            }
            for p in cs.matches.iter().chain(&cs.exclude_matches) {
                MatchPattern::parse(p).ok_or_else(|| ExtError::Invalid(format!("bad match pattern \"{p}\"")))?;
            }
            for f in cs.js.iter().chain(&cs.css) {
                if !safe_relative_path(f) {
                    return Err(ExtError::Invalid(format!("bad script path \"{f}\"")));
                }
            }
        }
        for p in m.permissions.iter().chain(&m.host_permissions) {
            if p.chars().count() > 200 {
                return Err(ExtError::Invalid("a permission is too long".into()));
            }
        }
        Ok(m)
    }

    /// The options page, if any (`options_ui.page` wins over `options_page`).
    pub fn options(&self) -> Option<&str> {
        self.options_ui.as_ref().and_then(|o| o.page.as_deref()).or(self.options_page.as_deref())
    }

    /// The background script and whether it is an ES module.
    pub fn background_script(&self) -> Option<(&str, bool)> {
        let b = self.background.as_ref()?;
        let module = b.kind.as_deref() == Some("module");
        b.service_worker.as_deref().or(b.scripts.first().map(String::as_str)).map(|s| (s, module))
    }

    /// The largest icon (by its size key) that is a safe relative path.
    pub fn icon(&self) -> Option<&str> {
        self.icons
            .iter()
            .filter(|(_, p)| safe_relative_path(p))
            .max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0))
            .map(|(_, p)| p.as_str())
    }

    /// The action's icon: its `default_icon` (a path or a size map), else the extension's.
    pub fn action_icon(&self) -> Option<String> {
        let from_action = self.action.as_ref().and_then(|a| a.default_icon.as_ref()).and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(m) => m
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.parse::<u32>().unwrap_or(0), s.to_string())))
                .max_by_key(|(k, _)| *k)
                .map(|(_, s)| s),
            _ => None,
        });
        from_action.filter(|p| safe_relative_path(p)).or_else(|| self.icon().map(str::to_string))
    }

    /// Every host pattern the extension may touch: host permissions, content script matches
    /// and the `<all_urls>`/scheme patterns among the permissions (the "scope list").
    pub fn scope(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |p: &str| {
            if MatchPattern::parse(p).is_some() && !out.iter().any(|x| x == p) {
                out.push(p.to_string());
            }
        };
        for p in self.permissions.iter().chain(&self.host_permissions) {
            push(p);
        }
        for cs in &self.content_scripts {
            for p in &cs.matches {
                push(p);
            }
        }
        out
    }

    /// The API permissions (not host patterns).
    pub fn api_permissions(&self) -> Vec<&str> {
        self.permissions.iter().map(String::as_str).filter(|p| MatchPattern::parse(p).is_none()).collect()
    }
}

/// A relative path inside the package: no traversal, no absolute path, no odd bytes.
pub fn safe_relative_path(p: &str) -> bool {
    let p = p.strip_prefix('/').unwrap_or(p);
    !p.is_empty()
        && p.len() <= 512
        && !p.contains('\\')
        && !p.contains("..")
        && !p.contains('\0')
        && !p.starts_with('/')
        && !p.split('/').any(|seg| seg.is_empty() || seg.ends_with('.') && seg != "." || seg.contains(':'))
        && !p.split('/').any(|seg| seg == ".")
}

// ------------------------------------------------------------------ match patterns

/// A Chrome match pattern: `<all_urls>` or `scheme://host/path` with `*` wildcards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPattern {
    /// `*` means http or https.
    pub scheme: String,
    /// `*`, `*.example.com` (the domain and its subdomains) or a host.
    pub host: String,
    /// The path glob (`*` matches anything).
    pub path: String,
}

impl MatchPattern {
    pub fn parse(p: &str) -> Option<MatchPattern> {
        if p == "<all_urls>" {
            return Some(MatchPattern { scheme: "*".into(), host: "*".into(), path: "/*".into() });
        }
        let (scheme, rest) = p.split_once("://")?;
        if !matches!(scheme, "*" | "http" | "https" | "file" | "ftp") {
            return None;
        }
        let (host, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if scheme != "file" {
            if host.is_empty() || host.contains(':') || host.contains('@') {
                return None;
            }
            let bare = host.strip_prefix("*.").unwrap_or(host);
            if bare != "*" && (bare.contains('*') || bare.is_empty()) {
                return None;
            }
        }
        if path.is_empty() || path.contains('\0') {
            return None;
        }
        Some(MatchPattern { scheme: scheme.to_string(), host: host.to_ascii_lowercase(), path: path.to_string() })
    }

    pub fn matches(&self, url: &str) -> bool {
        let Ok(u) = url::Url::parse(url) else { return false };
        let scheme_ok = match self.scheme.as_str() {
            "*" => matches!(u.scheme(), "http" | "https"),
            s => u.scheme() == s,
        };
        if !scheme_ok {
            return false;
        }
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        let host_ok = if self.scheme == "file" || self.host == "*" {
            true
        } else if let Some(domain) = self.host.strip_prefix("*.") {
            host == domain || host.ends_with(&format!(".{domain}"))
        } else {
            host == self.host
        };
        if !host_ok {
            return false;
        }
        let path = match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_string(),
        };
        glob_match(&self.path, &path)
    }

    /// Chrome's rule: a pattern whose host is `*` or a whole scheme counts as "all sites".
    pub fn is_all_sites(&self) -> bool {
        self.host == "*"
    }

    /// A short human form for the scope list.
    pub fn describe(&self) -> String {
        if self.host == "*" {
            return match self.scheme.as_str() {
                "*" => "all websites".into(),
                "file" => "local files".into(),
                s => format!("all {s} sites"),
            };
        }
        match self.host.strip_prefix("*.") {
            Some(d) => format!("{d} and its subdomains"),
            None => self.host.clone(),
        }
    }
}

/// `*` glob over a path (no other metacharacters).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Krunk's own origins and non-web schemes never get content scripts, whatever the pattern.
pub fn injectable_url(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let host = u.host_str().unwrap_or("");
    host != "krunk.invalid" && !host.ends_with(".krunk.invalid")
}

// ------------------------------------------------------------------ zip

/// One file of a zip archive, decompressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Reads a zip archive (stored or deflated entries) from memory, through its central
/// directory. Directory entries are skipped; unsafe names are refused.
pub fn unzip(bytes: &[u8]) -> Result<Vec<ZipEntry>, ExtError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(ExtError::TooLarge("archive"));
    }
    let bad = |s: &str| ExtError::Invalid(format!("not a usable zip archive: {s}"));
    // End of central directory: the last 64 KB may hold a comment.
    let tail_start = bytes.len().saturating_sub(65_557);
    let eocd = (tail_start..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| bad("no central directory"))?;
    let u16_at = |i: usize| -> usize { u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize };
    let u32_at =
        |i: usize| -> usize { u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize };
    let count = u16_at(eocd + 10);
    let cd_size = u32_at(eocd + 12);
    let cd_offset = u32_at(eocd + 16);
    if count > MAX_PACKAGE_FILES {
        return Err(ExtError::TooLarge("file count"));
    }
    if cd_offset.checked_add(cd_size).is_none_or(|end| end > bytes.len()) {
        return Err(bad("central directory out of range"));
    }
    let mut out = Vec::with_capacity(count);
    let mut total = 0usize;
    let mut pos = cd_offset;
    for _ in 0..count {
        if pos + 46 > bytes.len() || bytes[pos..pos + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err(bad("central directory entry"));
        }
        let method = u16_at(pos + 10);
        let flags = u16_at(pos + 8);
        let compressed = u32_at(pos + 20);
        let uncompressed = u32_at(pos + 24);
        let name_len = u16_at(pos + 28);
        let extra_len = u16_at(pos + 30);
        let comment_len = u16_at(pos + 32);
        let local = u32_at(pos + 42);
        let name_end = pos + 46 + name_len;
        if name_end > bytes.len() {
            return Err(bad("name out of range"));
        }
        let name = String::from_utf8_lossy(&bytes[pos + 46..name_end]).into_owned();
        pos = name_end + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        if flags & 1 != 0 {
            return Err(bad("encrypted entries"));
        }
        if !safe_relative_path(&name) {
            return Err(bad(&format!("unsafe file name \"{name}\"")));
        }
        total = total.saturating_add(uncompressed);
        if total > MAX_PACKAGE_BYTES || uncompressed > MAX_PACKAGE_BYTES {
            return Err(ExtError::TooLarge("unpacked size"));
        }
        if local + 30 > bytes.len() || bytes[local..local + 4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(bad("local header"));
        }
        let lname = u16_at(local + 26);
        let lextra = u16_at(local + 28);
        let start = local + 30 + lname + lextra;
        let end = start.checked_add(compressed).ok_or_else(|| bad("entry size"))?;
        if end > bytes.len() {
            return Err(bad("entry out of range"));
        }
        let raw = &bytes[start..end];
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, uncompressed.max(1))
                .map_err(|_| bad(&format!("\"{name}\" does not inflate")))?,
            m => return Err(bad(&format!("compression method {m}"))),
        };
        if data.len() != uncompressed {
            return Err(bad(&format!("\"{name}\" has the wrong size")));
        }
        out.push(ZipEntry { name, data });
    }
    Ok(out)
}

// ------------------------------------------------------------------ CRX3

/// A parsed CRX3 package: the id of its first RSA key and the zip inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crx {
    pub id: String,
    pub zip: Vec<u8>,
}

/// Splits a CRX3 file (`Cr24`, version 3, a protobuf header with the signer's public keys,
/// then the zip). The id is the header's `signed_header_data.crx_id` (the hash of the
/// developer's key, as Chrome reads it; a Web Store package lists the store's key first), or
/// the first `sha256_with_rsa` key's hash when the header has no signed data. The signature
/// itself is not checked here (the host fetches packages over HTTPS from the Web Store and
/// requires the id to match the one it asked for).
pub fn parse_crx(bytes: &[u8]) -> Result<Crx, ExtError> {
    let bad = |s: &str| ExtError::Invalid(format!("not a CRX package: {s}"));
    if bytes.len() < 16 || &bytes[..4] != b"Cr24" {
        return Err(bad("missing Cr24 magic"));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != 3 {
        return Err(bad(&format!("version {version}; only CRX3 is supported")));
    }
    let header_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let header_end = 12usize.checked_add(header_len).filter(|e| *e <= bytes.len()).ok_or_else(|| bad("header"))?;
    let header = &bytes[12..header_end];
    // CrxFileHeader: repeated AsymmetricKeyProof sha256_with_rsa = 2 { bytes public_key = 1; }
    //                bytes signed_header_data = 10000 → SignedData { bytes crx_id = 1 }
    let mut key = None;
    let mut crx_id = None;
    for (field, value) in protobuf_fields(header) {
        if field == 2 && key.is_none() {
            if let Some((1, pk)) = protobuf_fields(value).next() {
                key = Some(pk.to_vec());
            }
        }
        if field == 10000 {
            if let Some((1, id)) = protobuf_fields(value).find(|(f, _)| *f == 1) {
                if id.len() == 16 {
                    crx_id =
                        Some(id.iter().flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect::<String>());
                }
            }
        }
    }
    let id = match (crx_id, key) {
        (Some(id), _) => id,
        (None, Some(k)) => id_from_public_key(&k),
        (None, None) => return Err(bad("no RSA public key in the header")),
    };
    Ok(Crx { id, zip: bytes[header_end..].to_vec() })
}

/// A CRX3 package whose signature by its own developer key checks out (0.7.0, R20): the
/// header's `crx_id` is the hash of one of its `sha256_with_rsa` keys, and that key signed
/// `"CRX3 SignedData\0" || len(signed_header_data) || signed_header_data || zip`. A Web Store
/// package also carries the store's proof (`store_signed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCrx {
    pub crx: Crx,
    pub store_signed: bool,
}

/// The Chrome Web Store's publisher key: the SHA-256 of its SubjectPublicKeyInfo
/// (Chromium `components/crx_file/crx_verifier.cc`, `kPublisherKeyHash`).
const WEBSTORE_KEY_HASH: [u8; 32] = [
    0x61, 0xf7, 0xf2, 0xa6, 0xbf, 0xcf, 0x74, 0xcd, 0x0b, 0xc1, 0xfe, 0x24, 0x97, 0xcc, 0x9b, 0x04, 0x25, 0x4c, 0x65,
    0x8f, 0x79, 0xf2, 0x14, 0x53, 0x92, 0x86, 0x7e, 0xa8, 0x36, 0x63, 0x67, 0xcf,
];

/// Checks a CRX3's developer signature (and notes the store's). Errors name what failed.
pub fn verify_crx(bytes: &[u8]) -> Result<VerifiedCrx, ExtError> {
    use sha2::Digest;
    let bad = |s: &str| ExtError::Invalid(format!("the package's signature is not valid: {s}"));
    let crx = parse_crx(bytes)?;
    let header_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let header = &bytes[12..12 + header_len];
    let signed_data = protobuf_fields(header)
        .find(|(f, _)| *f == 10000)
        .map(|(_, v)| v.to_vec())
        .ok_or_else(|| bad("no signed header data"))?;
    let mut message = b"CRX3 SignedData\x00".to_vec();
    message.extend_from_slice(&(signed_data.len() as u32).to_le_bytes());
    message.extend_from_slice(&signed_data);
    message.extend_from_slice(&crx.zip);
    let mut developer = false;
    let mut store = false;
    // sha256_with_rsa = 2 and sha256_with_ecdsa = 3 (the store signs with ECDSA P-256).
    for (field, proof) in protobuf_fields(header) {
        if field != 2 && field != 3 {
            continue;
        }
        let mut key = None;
        let mut sig = None;
        for (f, v) in protobuf_fields(proof) {
            match f {
                1 => key = Some(v),
                2 => sig = Some(v),
                _ => {}
            }
        }
        let (Some(key), Some(sig)) = (key, sig) else { continue };
        let is_dev = id_from_public_key(key) == crx.id;
        let is_store = sha2::Sha256::digest(key).as_slice() == WEBSTORE_KEY_HASH;
        if !is_dev && !is_store {
            continue;
        }
        let ok = if field == 2 {
            let Some(pkcs1) = spki_key(key, RSA_OID) else { return Err(bad("an RSA key does not parse")) };
            ring::signature::UnparsedPublicKey::new(&ring::signature::RSA_PKCS1_2048_8192_SHA256, pkcs1)
                .verify(&message, sig)
                .is_ok()
        } else {
            let Some(point) = spki_key(key, EC_OID) else { return Err(bad("an ECDSA key does not parse")) };
            ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_ASN1, point)
                .verify(&message, sig)
                .is_ok()
        };
        if !ok {
            return Err(bad(if is_dev {
                "the developer's signature does not match"
            } else {
                "the Web Store's signature does not match"
            }));
        }
        developer |= is_dev;
        store |= is_store;
    }
    if !developer {
        return Err(bad("no signature by the extension's own key"));
    }
    Ok(VerifiedCrx { crx, store_signed: store })
}

const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const EC_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];

/// The key inside a DER SubjectPublicKeyInfo of algorithm `oid`: the PKCS#1 RSAPublicKey for
/// rsaEncryption, the uncompressed point for id-ecPublicKey.
fn spki_key<'a>(der: &'a [u8], want: &[u8]) -> Option<&'a [u8]> {
    fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        let tag = *b.first()?;
        let first = *b.get(1)?;
        let (len, start) = if first & 0x80 == 0 {
            (usize::from(first), 2)
        } else {
            let n = usize::from(first & 0x7f);
            if n == 0 || n > 4 {
                return None;
            }
            let mut len = 0usize;
            for i in 0..n {
                len = (len << 8) | usize::from(*b.get(2 + i)?);
            }
            (len, 2 + n)
        };
        let body = b.get(start..start.checked_add(len)?)?;
        Some((tag, body, &b[start + len..]))
    }
    let (0x30, spki, _) = tlv(der)? else { return None };
    let (0x30, alg, rest) = tlv(spki)? else { return None };
    let (0x06, oid, _) = tlv(alg)? else { return None };
    if oid != want {
        return None;
    }
    let (0x03, bits, _) = tlv(rest)? else { return None };
    match bits.split_first() {
        Some((0, key)) => Some(key),
        _ => None,
    }
}

/// The length-delimited (wire type 2) fields of a protobuf message, in order.
fn protobuf_fields(mut b: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    std::iter::from_fn(move || {
        let (tag, rest) = varint(b)?;
        let (field, wire) = (tag >> 3, tag & 7);
        b = rest;
        match wire {
            0 => {
                let (_, rest) = varint(b)?;
                b = rest;
                Some((field, &[][..]))
            }
            1 => {
                b = b.get(8..)?;
                Some((field, &[][..]))
            }
            2 => {
                let (len, rest) = varint(b)?;
                let len = len as usize;
                let value = rest.get(..len)?;
                b = &rest[len..];
                Some((field, value))
            }
            5 => {
                b = b.get(4..)?;
                Some((field, &[][..]))
            }
            _ => None,
        }
    })
    .filter(|(_, v)| !v.is_empty())
}

fn varint(b: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (i, byte) in b.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, &b[i + 1..]));
        }
    }
    None
}

/// The Web Store's download address for an extension id (a CRX3 over HTTPS).
pub fn webstore_download_url(id: &str) -> Option<String> {
    valid_id(id).then(|| {
        format!(
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=156.0.0.0&acceptformat=crx3&x=id%3D{id}%26uc"
        )
    })
}

/// The extension id in a Web Store page address
/// (`https://chromewebstore.google.com/detail/<name>/<id>` or the old `chrome.google.com/webstore/detail/...`).
pub fn webstore_id(url: &str) -> Option<String> {
    let u = url::Url::parse(url.trim()).ok()?;
    let host = u.host_str()?;
    if !matches!(host, "chromewebstore.google.com" | "chrome.google.com") {
        return None;
    }
    u.path_segments()?.rfind(|s| valid_id(s)).map(str::to_string)
}

/// The Chrome Web Store's page origins (where "Add to Krunk" is offered).
pub fn is_store_origin(origin: &str) -> bool {
    matches!(origin, "https://chromewebstore.google.com" | "https://chrome.google.com")
}

/// The extension id of a Web Store package download (`clients2.google.com/service/update2/crx?…
/// &x=id%3D<id>%26uc`), the address the store and Chrome's updater fetch (0.7.0: such a download
/// is never saved, it goes to the install flow).
pub fn webstore_crx_id(url: &str) -> Option<String> {
    let u = url::Url::parse(url.trim()).ok()?;
    if u.scheme() != "https" || u.host_str()? != "clients2.google.com" || !u.path().starts_with("/service/update2/crx")
    {
        return None;
    }
    let x = u.query_pairs().find(|(k, _)| k == "x")?.1.into_owned();
    let id = url::form_urlencoded::parse(x.as_bytes()).find(|(k, _)| k == "id")?.1.into_owned();
    valid_id(&id).then_some(id)
}

/// What the install confirmation says an extension may do (0.7.0, R20), in Chrome's words where
/// it has them: the sites first, then the API permissions that matter. Empty: nothing special.
pub fn install_warnings(m: &Manifest) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let scope: Vec<MatchPattern> = m.scope().iter().filter_map(|p| MatchPattern::parse(p)).collect();
    if scope.iter().any(MatchPattern::is_all_sites) {
        out.push("Read and change all your data on all websites".into());
    } else if !scope.is_empty() {
        let mut hosts: Vec<String> = scope.iter().map(MatchPattern::describe).collect();
        hosts.dedup();
        let shown: Vec<String> = hosts.iter().take(4).cloned().collect();
        let more = hosts.len().saturating_sub(shown.len());
        out.push(format!(
            "Read and change your data on {}{}",
            shown.join(", "),
            if more > 0 {
                format!(" and {more} other site{}", if more == 1 { "" } else { "s" })
            } else {
                String::new()
            }
        ));
    }
    for p in m.api_permissions() {
        let w = match p {
            "tabs" | "webNavigation" => "Read your browsing history",
            "history" => "Read and change your browsing history",
            "bookmarks" => "Read and change your bookmarks",
            "cookies" => "Read and change cookies of the sites it can access",
            "downloads" => "Manage your downloads",
            "clipboardRead" => "Read data you copy and paste",
            "clipboardWrite" => "Change data you copy and paste",
            "notifications" => "Display notifications",
            "declarativeNetRequest" | "declarativeNetRequestWithHostAccess" => "Block content on any page",
            "management" => "Manage your apps, extensions and themes",
            "privacy" => "Change your privacy-related settings",
            "proxy" => "Read and change your proxy settings",
            "geolocation" => "Detect your physical location",
            "nativeMessaging" => "Communicate with cooperating native applications",
            "debugger" => "Access the page debugger backend",
            _ => continue,
        };
        if !out.iter().any(|x| x == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// Standard base64 (a manifest's `key`).
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64ct::Encoding;
    base64ct::Base64::decode_vec(s.trim()).ok()
}

// ------------------------------------------------------------------ files

/// The MIME type of an extension file by its name.
pub fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Content-Security-Policy of an extension's pages: its own files, inline styles (popups use
/// them), images and fonts from anywhere, fetches to anywhere (host permissions are the
/// extension's business), never framed by web pages, no plugins.
pub const PAGE_CSP: &str =
    "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob: https: http:; font-src 'self' data: https:; media-src 'self' data: blob: https:; \
     connect-src 'self' https: http: ws: wss: data: blob:; frame-src 'self' https:; frame-ancestors 'none'; \
     base-uri 'none'; object-src 'none'";

/// Resolves a URL on an extension origin to the file inside `root` (the unpacked package):
/// `(id, relative path)`; `None` for other hosts or unsafe paths. `/` is the action popup's
/// place only when the manifest says so; callers map special pages themselves.
pub fn resolve_file(url: &str) -> Option<(String, String)> {
    let id = id_of_url(url)?;
    let u = url::Url::parse(url).ok()?;
    let path = percent_decode(u.path().trim_start_matches('/'));
    if path.is_empty() || !safe_relative_path(&path) {
        return None;
    }
    Some((id, path))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The on-disk file of a package path (segments joined; the caller checked safety).
pub fn file_path(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    p
}

// ------------------------------------------------------------------ locales

/// `__MSG_name__` placeholders in manifest strings, from `_locales/<locale>/messages.json`.
/// One pass over `text`: a message's own text is never searched again (0.11.7, audit C6: a
/// message containing its own placeholder looped forever, or doubled until memory ran out,
/// before the install prompt).
pub fn localize(text: &str, messages: &serde_json::Value) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("__MSG_") {
        let Some(len) = rest[start + 6..].find("__") else { break };
        let name = &rest[start + 6..start + 6 + len];
        let value = messages
            .get(name)
            .or_else(|| messages.get(name.to_ascii_lowercase()))
            .and_then(|m| m.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("");
        out.push_str(&rest[..start]);
        out.push_str(value);
        rest = &rest[start + 6 + len + 2..];
    }
    out.push_str(rest);
    out
}

/// A locale folder name (`en`, `pt_BR`, `zh_Hant`): `default_locale` and the user's languages
/// become a path under `_locales`, so nothing else may (0.11.7, audit C5: an absolute or UNC
/// path replaced the folder, `..` left it).
pub fn valid_locale(name: &str) -> bool {
    let (lang, region) = name.split_once('_').map_or((name, None), |(l, r)| (l, Some(r)));
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_alphabetic())
        && region.is_none_or(|r| (1..=8).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_alphanumeric()))
}

// ------------------------------------------------------------------ API survey

/// The `chrome.*` namespaces Krunk's shim provides (fully or in part).
pub const SUPPORTED_APIS: &[&str] =
    &["runtime", "storage", "tabs", "action", "i18n", "alarms", "extension", "windows", "scripting"];
/// Namespaces Krunk never provides, with the reason the install summary shows.
pub const REFUSED_APIS: &[(&str, &str)] = &[
    ("webRequest", "blocking network interception (use declarativeNetRequest static rules)"),
    ("debugger", "the DevTools protocol of other tabs"),
    ("nativeMessaging", "native programs"),
    ("declarativeNetRequest.updateDynamicRules", "dynamic network rules"),
    ("declarativeNetRequest.updateSessionRules", "session network rules"),
    ("declarativeNetRequest.updateEnabledRulesets", "switching rulesets at runtime"),
    ("management", "other extensions"),
    ("proxy", "the proxy settings"),
    ("privacy", "the privacy settings"),
    ("contentSettings", "site settings"),
    ("cookies", "cookies of other sites"),
    ("history", "the browsing history"),
    ("bookmarks", "the bookmarks"),
    ("downloads", "downloads"),
    ("identity", "Google accounts"),
    ("webNavigation", "navigation events"),
    ("sidePanel", "a side panel"),
    ("offscreen", "offscreen documents"),
    ("tabCapture", "tab capture"),
    ("desktopCapture", "screen capture"),
    ("notifications", "system notifications"),
    ("contextMenus", "context-menu items"),
    ("commands", "keyboard commands"),
    ("permissions", "runtime permission changes"),
    ("tts", "speech"),
    ("gcm", "push messages"),
];

/// A `chrome.*` use found in the scripts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApiUse {
    pub api: String,
    pub supported: bool,
    pub reason: String,
}

/// Surveys the scripts for `chrome.<ns>` (and `browser.<ns>`) uses and says which Krunk
/// provides. A survey, not a guarantee: dynamic access is not seen.
pub fn survey_apis(sources: &[&str]) -> Vec<ApiUse> {
    let mut found: Vec<String> = Vec::new();
    for src in sources {
        let b = src.as_bytes();
        for prefix in ["chrome.", "browser."] {
            let mut start = 0;
            while let Some(i) = src[start..].find(prefix) {
                let at = start + i;
                let before = if at == 0 { b' ' } else { b[at - 1] };
                start = at + prefix.len();
                if before.is_ascii_alphanumeric() || before == b'_' || before == b'$' || before == b'.' {
                    continue;
                }
                let rest = &src[start..];
                let ns: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if ns.is_empty() || !ns.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    continue;
                }
                // A second level for the refused declarativeNetRequest methods.
                let name = if ns == "declarativeNetRequest" {
                    let after = &rest[ns.len()..];
                    let method: String = after
                        .strip_prefix('.')
                        .map(|m| m.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect())
                        .unwrap_or_default();
                    if method.is_empty() {
                        ns.clone()
                    } else {
                        format!("{ns}.{method}")
                    }
                } else {
                    ns.clone()
                };
                if !found.contains(&name) {
                    found.push(name);
                }
            }
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|api| {
            let refused = REFUSED_APIS.iter().find(|(n, _)| *n == api).map(|(_, why)| *why);
            let supported = refused.is_none()
                && (SUPPORTED_APIS.contains(&api.as_str()) || api.starts_with("declarativeNetRequest"));
            let reason = match refused {
                Some(why) => format!("not provided: {why}"),
                None if supported => String::new(),
                None => "not provided".into(),
            };
            ApiUse { api, supported, reason }
        })
        .collect()
}

// ------------------------------------------------------------------ declarativeNetRequest

/// Translates static declarativeNetRequest rules (a rule resource's JSON) into filter lines
/// for the blocker: `block` → a network filter, `allow` → an exception, `upgradeScheme` and
/// `redirect` are not translated (returned in `skipped`). The returned text has one line
/// per rule; `title` names the extension in the list's header.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnrTranslation {
    pub filters: String,
    pub translated: usize,
    pub skipped: usize,
}

pub fn translate_dnr(title: &str, json: &str) -> Result<DnrTranslation, ExtError> {
    let rules: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| ExtError::Invalid(format!("rules are not valid JSON: {e}")))?;
    let mut out = String::new();
    out.push_str(&format!("! Extension rules: {title}\n"));
    let mut t = DnrTranslation::default();
    for rule in rules.iter().take(30_000) {
        let action = rule.pointer("/action/type").and_then(|v| v.as_str()).unwrap_or("");
        let cond = rule.get("condition").cloned().unwrap_or(serde_json::Value::Null);
        let line = match action {
            "block" => dnr_filter(&cond, false),
            "allow" | "allowAllRequests" => dnr_filter(&cond, true),
            _ => None,
        };
        match line {
            Some(l) => {
                out.push_str(&l);
                out.push('\n');
                t.translated += 1;
            }
            None => t.skipped += 1,
        }
    }
    t.filters = out;
    Ok(t)
}

fn dnr_filter(cond: &serde_json::Value, exception: bool) -> Option<String> {
    let mut pattern = if let Some(re) = cond.get("regexFilter").and_then(|v| v.as_str()) {
        if re.len() > 500 {
            return None;
        }
        format!("/{re}/")
    } else {
        let f = cond.get("urlFilter").and_then(|v| v.as_str()).unwrap_or("*");
        if f.is_empty() || f.contains('\n') {
            return None;
        }
        // DNR's urlFilter syntax (||, |, ^, *) is the filter-list one.
        f.to_string()
    };
    if pattern == "*" || pattern == "||" {
        pattern = "*".into();
    }
    let mut opts: Vec<String> = Vec::new();
    let list = |key: &str| -> Vec<String> {
        cond.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(str::to_string).collect())
            .unwrap_or_default()
    };
    let types = list("resourceTypes");
    let excluded_types = list("excludedResourceTypes");
    let map_type = |t: &str| -> Option<&'static str> {
        Some(match t {
            "main_frame" => "document",
            "sub_frame" => "subdocument",
            "stylesheet" => "stylesheet",
            "script" => "script",
            "image" => "image",
            "font" => "font",
            "object" => "object",
            "xmlhttprequest" => "xmlhttprequest",
            "ping" => "ping",
            "media" => "media",
            "websocket" => "websocket",
            "other" => "other",
            _ => return None,
        })
    };
    let mapped: Vec<&str> = types.iter().filter_map(|t| map_type(t)).collect();
    if !types.is_empty() && mapped.is_empty() {
        return None;
    }
    opts.extend(mapped.iter().map(|t| t.to_string()));
    opts.extend(excluded_types.iter().filter_map(|t| map_type(t)).map(|t| format!("~{t}")));
    let mut domains: Vec<String> = Vec::new();
    domains.extend(list("initiatorDomains").into_iter().chain(list("domains")));
    domains
        .extend(list("excludedInitiatorDomains").into_iter().chain(list("excludedDomains")).map(|d| format!("~{d}")));
    if !domains.is_empty() {
        opts.push(format!("domain={}", domains.join("|")));
    }
    match cond.get("domainType").and_then(|v| v.as_str()) {
        Some("thirdParty") => opts.push("third-party".into()),
        Some("firstParty") => opts.push("~third-party".into()),
        _ => {}
    }
    if cond.get("isUrlFilterCaseSensitive").and_then(|v| v.as_bool()) == Some(true) {
        opts.push("match-case".into());
    }
    let mut line = String::new();
    if exception {
        line.push_str("@@");
    }
    line.push_str(&pattern);
    if !opts.is_empty() {
        line.push('$');
        line.push_str(&opts.join(","));
    }
    Some(line)
}

// ------------------------------------------------------------------ the content bundle

/// A content script ready for the bundle: the extension, its sources and where they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledScript {
    pub id: String,
    pub matches: Vec<String>,
    pub exclude_matches: Vec<String>,
    pub all_frames: bool,
    /// `document_start`, `document_end` or `document_idle` (the default).
    pub run_at: String,
    pub js: Vec<String>,
    pub css: Vec<String>,
}

/// An extension as the bundle needs it: id, manifest (for `getManifest`), locale messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledExtension {
    pub id: String,
    pub manifest_json: String,
    pub messages_json: String,
    pub scripts: Vec<BundledScript>,
}

/// Builds the script that `Page.addScriptToEvaluateOnNewDocument` installs in an isolated
/// world of every tab: the shim, then every content script wrapped with its match test. Since
/// 0.11.7 the host builds one bundle per extension, for that extension's own world.
/// Krunk's own origins are refused inside (`injectableUrl`), the host never registers the
/// bundle for its UI views anyway.
pub fn content_bundle(shim: &str, exts: &[BundledExtension]) -> String {
    let mut out = String::with_capacity(shim.len() + 4096);
    out.push_str("(function () {\n'use strict';\n");
    out.push_str(
        "if (!/^https?:$/.test(location.protocol) || /(^|\\.)krunk\\.invalid$/.test(location.hostname)) return;\n",
    );
    out.push_str("var __krunkShim = ");
    out.push_str(shim.trim());
    out.push_str(";\n");
    for e in exts {
        out.push_str(&format!(
            "(function () {{ var ext = __krunkShim({id}, {manifest}, {messages});\n",
            id = json_str(&e.id),
            manifest = e.manifest_json,
            messages = if e.messages_json.is_empty() { "{}" } else { &e.messages_json },
        ));
        for s in &e.scripts {
            out.push_str(&format!(
                "ext.run({matches}, {excludes}, {all_frames}, {run_at}, {css}, function (chrome, browser) {{\n",
                matches = serde_json::to_string(&s.matches).unwrap_or_else(|_| "[]".into()),
                excludes = serde_json::to_string(&s.exclude_matches).unwrap_or_else(|_| "[]".into()),
                all_frames = s.all_frames,
                run_at = json_str(&s.run_at),
                css = serde_json::to_string(&s.css).unwrap_or_else(|_| "[]".into()),
            ));
            for src in &s.js {
                out.push_str("try {\n");
                out.push_str(src);
                out.push_str("\n} catch (e) { ext.report(e); }\n");
            }
            out.push_str("});\n");
        }
        out.push_str("})();\n");
    }
    out.push_str("})();\n");
    out
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

// ------------------------------------------------------------------ installed state

/// What Krunk keeps about an installed extension (`extensions/state.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    /// Runs in private tabs too (off by default).
    pub allowed_in_private: bool,
    /// Where it came from: `file`, `folder:<path>`, `webstore:<id>`.
    pub source: String,
    pub installed_at: i64,
    pub updated_at: i64,
    /// The survey at install time.
    pub apis: Vec<ApiUse>,
    /// Rules translated / skipped.
    pub dnr_translated: usize,
    pub dnr_skipped: usize,
}

impl Default for Installed {
    fn default() -> Self {
        Installed {
            id: String::new(),
            name: String::new(),
            version: String::new(),
            description: String::new(),
            enabled: true,
            allowed_in_private: false,
            source: String::new(),
            installed_at: 0,
            updated_at: 0,
            apis: Vec::new(),
            dnr_translated: 0,
            dnr_skipped: 0,
        }
    }
}

/// Compares two dotted versions the way Chrome does (missing parts are 0).
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "manifest_version": 3, "name": "Krunk fixture", "version": "1.2", "description": "test",
        "icons": {"16": "icons/16.png", "48": "icons/48.png"},
        "permissions": ["storage", "tabs", "webRequest"],
        "host_permissions": ["*://*.example.com/*"],
        "content_scripts": [{"matches": ["https://*.example.com/*", "http://localhost/*"], "js": ["cs.js"], "css": ["cs.css"], "run_at": "document_end"}],
        "background": {"service_worker": "bg.js"},
        "action": {"default_popup": "popup.html", "default_title": "Fixture", "default_icon": {"16": "icons/16.png"}},
        "options_ui": {"page": "options.html", "open_in_tab": true},
        "declarative_net_request": {"rule_resources": [{"id": "r", "enabled": true, "path": "rules.json"}]}
    }"#;

    /// 0.11.7, audit C1: an extension's world on Krunk's own pages is never used.
    #[test]
    fn content_worlds_only_outside_krunk() {
        for refused in [
            "https://krunk.invalid",
            "https://KRUNK.invalid.",
            "https://shell.krunk.invalid",
            "https://abcdefghijklmnopabcdefghijklmnop.ext.krunk.invalid",
            "chrome://gpu",
            "devtools://devtools",
            "chrome-extension://abc",
        ] {
            assert!(!content_context_allowed(refused), "{refused}");
        }
        for allowed in [
            "https://example.com",
            "http://localhost:8094",
            "https://krunk.invalid.example.com",
            "https://notkrunk.invalid.test",
            "file://",
            "null",
            "",
            "://",
        ] {
            assert!(content_context_allowed(allowed), "{allowed}");
        }
    }

    /// 0.11.7, audit C4: worlds and bindings name their extension, and nothing else matches.
    #[test]
    fn worlds_and_bindings_per_extension() {
        let id = "abcdefghijklmnopabcdefghijklmnop";
        assert_eq!(world_name(id), format!("krunk-ext-{id}"));
        assert_eq!(binding_name(id), format!("__krunkExt_{id}"));
        assert_eq!(ext_of_world(&world_name(id)), Some(id));
        assert_eq!(ext_of_binding(&binding_name(id)), Some(id));
        for bad in ["krunk-ext", "krunk-ext-", "krunk-extabcdefghijklmnopabcdefghijklmnop", "krunk-ext-ABC", "other"] {
            assert_eq!(ext_of_world(bad), None, "{bad}");
        }
        for bad in ["__krunkExt", "__krunkExt_", "__krunkExt_zzzz", "__krunkExtabcdefghijklmnopabcdefghijklmnop"] {
            assert_eq!(ext_of_binding(bad), None, "{bad}");
        }
        // A bundle carries one extension: its own id goes to the shim.
        let e = BundledExtension {
            id: id.into(),
            manifest_json: "{}".into(),
            messages_json: String::new(),
            scripts: vec![],
        };
        let b = content_bundle("function () {}", std::slice::from_ref(&e));
        assert!(b.contains(&format!("\"{id}\"")));
    }

    #[test]
    fn scripting_needs_its_permission() {
        let m = |perms: &str| {
            Manifest::parse(&format!(r#"{{"manifest_version":3,"name":"x","version":"1","permissions":{perms}}}"#))
                .unwrap()
        };
        assert!(may_use_scripting(&m(r#"["storage","scripting"]"#)));
        assert!(!may_use_scripting(&m(r#"["storage","tabs","activeTab"]"#)));
        assert!(!may_use_scripting(&m(r#"[]"#)));
        assert!(!may_use_scripting(&Manifest::parse(MANIFEST).unwrap()));
    }

    #[test]
    fn manifest_parses_and_derives() {
        let m = Manifest::parse(MANIFEST).unwrap();
        assert_eq!(m.name, "Krunk fixture");
        assert_eq!(m.options(), Some("options.html"));
        assert_eq!(m.background_script(), Some(("bg.js", false)));
        assert_eq!(m.icon(), Some("icons/48.png"));
        assert_eq!(m.action_icon().as_deref(), Some("icons/16.png"));
        assert_eq!(m.scope(), vec!["*://*.example.com/*", "https://*.example.com/*", "http://localhost/*"]);
        assert_eq!(m.api_permissions(), vec!["storage", "tabs", "webRequest"]);
        assert!(Manifest::parse(r#"{"manifest_version": 2, "name": "x", "version": "1"}"#).is_err(), "MV2 refused");
        assert!(Manifest::parse(r#"{"manifest_version": 3, "name": "", "version": "1"}"#).is_err());
        assert!(Manifest::parse(r#"{"manifest_version": 3, "name": "x", "version": "1.a"}"#).is_err());
        assert!(Manifest::parse(r#"{"manifest_version": 3, "name": "x", "version": "1", "content_scripts": [{"matches": ["nope"], "js": ["a.js"]}]}"#).is_err());
        assert!(Manifest::parse(r#"{"manifest_version": 3, "name": "x", "version": "1", "content_scripts": [{"matches": ["<all_urls>"], "js": ["../a.js"]}]}"#).is_err());
        assert!(
            Manifest::parse("\u{feff}{\"manifest_version\":3,\"name\":\"b\",\"version\":\"1\",\"unknown_key\":1}")
                .is_ok(),
            "BOM and unknown keys"
        );
    }

    #[test]
    fn ids_and_origins() {
        let id = id_from_public_key(b"not really a key");
        assert!(valid_id(&id));
        assert_ne!(id, id_from_public_key(b"another"));
        assert!(!valid_id("short") && !valid_id(&"q".repeat(32)));
        assert_eq!(origin(&id), format!("https://{id}.ext.krunk.invalid"));
        assert_eq!(id_of_url(&format!("https://{id}.ext.krunk.invalid/popup.html")), Some(id.clone()));
        assert_eq!(id_of_url("https://krunk.invalid/newtab.html"), None);
        assert_eq!(id_of_url(&format!("http://{id}.ext.krunk.invalid/")), None, "https only");
        let (rid, path) = resolve_file(&format!("https://{id}.ext.krunk.invalid/icons/a%20b.png?x=1")).unwrap();
        assert_eq!((rid.as_str(), path.as_str()), (id.as_str(), "icons/a b.png"));
        assert!(resolve_file(&format!("https://{id}.ext.krunk.invalid/a%00b")).is_none(), "a NUL in the path");
        assert_eq!(
            resolve_file(&format!("https://{id}.ext.krunk.invalid/%2e%2e/x")).map(|(_, p)| p).as_deref(),
            Some("x"),
            "the URL parser folds an encoded traversal too"
        );
        assert_eq!(
            resolve_file(&format!("https://{id}.ext.krunk.invalid/a/../x")).map(|(_, p)| p).as_deref(),
            Some("x"),
            "the URL parser folds a plain one"
        );
        assert!(resolve_file(&format!("https://{id}.ext.krunk.invalid/")).is_none());
        assert_eq!(file_path(Path::new("C:\\x"), "a/b.js"), PathBuf::from("C:\\x").join("a").join("b.js"));
        assert_eq!(
            webstore_id("https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh"),
            Some("eimadpbcbfnmbkopoojfekhnkhdbieeh".into())
        );
        assert_eq!(
            webstore_id("https://chrome.google.com/webstore/detail/eimadpbcbfnmbkopoojfekhnkhdbieeh?hl=en"),
            Some("eimadpbcbfnmbkopoojfekhnkhdbieeh".into())
        );
        assert_eq!(webstore_id("https://example.com/eimadpbcbfnmbkopoojfekhnkhdbieeh"), None);
        assert!(webstore_download_url("eimadpbcbfnmbkopoojfekhnkhdbieeh")
            .unwrap()
            .contains("id%3Deimadpbcbfnmbkopoojfekhnkhdbieeh"));
        assert!(webstore_download_url("x").is_none());
    }

    #[test]
    fn match_patterns() {
        let p = MatchPattern::parse("https://*.example.com/*").unwrap();
        assert!(p.matches("https://www.example.com/a?b=1") && p.matches("https://example.com/"));
        assert!(!p.matches("http://www.example.com/") && !p.matches("https://example.com.evil/"));
        let all = MatchPattern::parse("<all_urls>").unwrap();
        assert!(all.matches("http://a/") && all.matches("https://b/x") && all.is_all_sites());
        assert!(!all.matches("ftp://a/") && !all.matches("file:///x"), "<all_urls> is http(s) here");
        let path = MatchPattern::parse("*://example.org/docs/*.html").unwrap();
        assert!(path.matches("https://example.org/docs/a/b.html") && !path.matches("https://example.org/docs/a.txt"));
        assert!(
            MatchPattern::parse("https://*example.com/").is_none() && MatchPattern::parse("https://a:1/").is_none()
        );
        assert!(MatchPattern::parse("chrome://*/*").is_none());
        assert_eq!(
            MatchPattern::parse("*://*.wikipedia.org/*").unwrap().describe(),
            "wikipedia.org and its subdomains"
        );
        assert_eq!(all.describe(), "all websites");
        assert!(glob_match("/a*c", "/abbbc") && glob_match("*", "") && !glob_match("/a*c", "/ab"));
        assert!(injectable_url("https://example.com/") && !injectable_url("https://krunk.invalid/newtab.html"));
        assert!(!injectable_url("https://shell.krunk.invalid/shell.html") && !injectable_url("file:///x.html"));
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        // A stored (uncompressed) zip, written by hand.
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in entries {
            let off = out.len() as u32;
            let crc = 0u32; // not checked
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            cd.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn zip_and_crx() {
        let z = zip_of(&[("manifest.json", b"{}"), ("dir/", b""), ("a/b.js", b"x")]);
        let files = unzip(&z).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "manifest.json");
        assert_eq!(files[1].data, b"x");
        assert!(unzip(&zip_of(&[("../evil", b"x")])).is_err(), "traversal refused");
        assert!(unzip(b"not a zip").is_err());
        // A deflated entry made with miniz.
        let data = b"hello hello hello hello".repeat(20);
        let packed = miniz_oxide::deflate::compress_to_vec(&data, 6);
        let mut z = zip_of(&[("big.txt", &packed)]);
        z[8] = 8; // method deflate in the local header
        let cd = z.windows(4).rposition(|w| w == [0x50, 0x4b, 0x01, 0x02]).unwrap();
        z[cd + 10] = 8;
        let unc = (data.len() as u32).to_le_bytes();
        z[22..26].copy_from_slice(&unc);
        z[cd + 24..cd + 28].copy_from_slice(&unc);
        assert_eq!(unzip(&z).unwrap()[0].data, data);
        // CRX3: a header with one sha256_with_rsa proof holding a public key.
        let key = b"\x30\x0d\x06\x09fake-key-bytes";
        let proof = [vec![0x0a, key.len() as u8], key.to_vec()].concat(); // field 1, bytes
        let header = [vec![0x12, proof.len() as u8], proof].concat(); // field 2, message
        let zip = zip_of(&[("manifest.json", b"{}")]);
        let mut crx = b"Cr24".to_vec();
        crx.extend_from_slice(&3u32.to_le_bytes());
        crx.extend_from_slice(&(header.len() as u32).to_le_bytes());
        crx.extend_from_slice(&header);
        crx.extend_from_slice(&zip);
        let parsed = parse_crx(&crx).unwrap();
        assert_eq!(parsed.id, id_from_public_key(key));
        assert_eq!(parsed.zip, zip);
        // With signed header data: the crx_id wins (a Web Store package lists the store's key first).
        let crx_id: Vec<u8> = (0u8..16).collect();
        let signed = [vec![0x0a, 16], crx_id.clone()].concat();
        let header2 = [header.clone(), vec![0x82, 0xf1, 0x04, signed.len() as u8], signed].concat();
        let mut crx2 = b"Cr24".to_vec();
        crx2.extend_from_slice(&3u32.to_le_bytes());
        crx2.extend_from_slice(&(header2.len() as u32).to_le_bytes());
        crx2.extend_from_slice(&header2);
        crx2.extend_from_slice(&zip);
        let want: String = crx_id.iter().flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect();
        assert_eq!(parse_crx(&crx2).unwrap().id, want);
        assert!(parse_crx(b"Cr24\x02\x00\x00\x00").is_err(), "CRX2 refused");
        assert!(parse_crx(&zip).is_err());
    }

    #[test]
    fn install_warnings_and_store_downloads() {
        let m = |json: &str| Manifest::parse(json).unwrap();
        let ubo = m(
            r#"{"manifest_version":3,"name":"uBO Lite","version":"1.0","permissions":["declarativeNetRequest","storage","scripting","activeTab"],"host_permissions":["<all_urls>"]}"#,
        );
        assert_eq!(
            install_warnings(&ubo),
            ["Read and change all your data on all websites", "Block content on any page"]
        );
        let few = m(
            r#"{"manifest_version":3,"name":"x","version":"1","permissions":["tabs","storage"],"host_permissions":["https://*.example.com/*","https://a.test/*","https://b.test/*","https://c.test/*","https://d.test/*"]}"#,
        );
        assert_eq!(
            install_warnings(&few),
            [
                "Read and change your data on example.com and its subdomains, a.test, b.test, c.test and 1 other site",
                "Read your browsing history"
            ]
        );
        assert!(install_warnings(&m(r#"{"manifest_version":3,"name":"x","version":"1","permissions":["storage"]}"#))
            .is_empty());
        let id = "a".repeat(32);
        let url = format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion=156.0.0.0&acceptformat=crx3&x=id%3D{id}%26uc");
        assert_eq!(webstore_crx_id(&url).as_deref(), Some(id.as_str()));
        assert_eq!(webstore_download_url(&id).and_then(|u| webstore_crx_id(&u)).as_deref(), Some(id.as_str()));
        assert!(webstore_crx_id("https://evil.example/service/update2/crx?x=id%3Daaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .is_none());
        assert!(webstore_crx_id(
            "http://clients2.google.com/service/update2/crx?x=id%3Daaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
        .is_none());
        assert!(webstore_crx_id("https://clients2.google.com/service/update2/crx?x=id%3Dnot-an-id").is_none());
    }

    /// A package fetched from the live store: `KRUNK_CRX=/path/x.crx cargo test -p core -- --ignored store_crx`.
    #[test]
    #[ignore]
    fn store_crx_from_the_live_store() {
        let path = std::env::var("KRUNK_CRX").expect("KRUNK_CRX");
        let v = verify_crx(&std::fs::read(path).unwrap()).unwrap();
        assert!(v.store_signed, "signed by the Web Store");
        println!("{} store-signed", v.crx.id);
    }

    #[test]
    fn crx_signatures_are_checked() {
        // core/tests/fixtures/crx/make.py: throwaway RSA keys, a one-file zip.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crx");
        let read = |n: &str| std::fs::read(dir.join(n)).unwrap();
        let ok = verify_crx(&read("signed.crx")).unwrap();
        assert!(valid_id(&ok.crx.id) && !ok.store_signed);
        let files = unzip(&ok.crx.zip).unwrap();
        assert_eq!(files[0].name, "manifest.json");
        let e = verify_crx(&read("tampered.crx")).unwrap_err().to_string();
        assert!(e.contains("does not match"), "{e}");
        let e = verify_crx(&read("wrong-key.crx")).unwrap_err().to_string();
        assert!(e.contains("own key"), "{e}");
        // The unsigned test package above has no signed data at all.
        assert!(verify_crx(b"Cr24\x03\x00\x00\x00\x00\x00\x00\x00PK").is_err());
        // A SubjectPublicKeyInfo that is not RSA is refused.
        assert!(spki_key(&[0x30, 0x03, 0x30, 0x01, 0x00], RSA_OID).is_none());
        // An ECDSA P-256 developer key works too (the store signs that way).
        let ec = verify_crx(&read("signed-ec.crx")).unwrap();
        assert!(valid_id(&ec.crx.id) && !ec.store_signed);
    }

    #[test]
    fn api_survey_and_locales() {
        let uses = survey_apis(&[
            "chrome.storage.local.get(); chrome.runtime.onMessage.addListener(f); browser.tabs.query({});",
            "chrome.webRequest.onBeforeRequest.addListener(); chrome.declarativeNetRequest.updateDynamicRules({}); xchrome.fake.x; chrome.Foo.bar",
        ]);
        let names: Vec<&str> = uses.iter().map(|u| u.api.as_str()).collect();
        assert_eq!(names, vec!["declarativeNetRequest.updateDynamicRules", "runtime", "storage", "tabs", "webRequest"]);
        assert!(uses.iter().filter(|u| u.supported).count() == 3);
        assert!(uses.iter().any(|u| u.api == "webRequest" && u.reason.contains("blocking network")));
        let msgs = serde_json::json!({"appName": {"message": "Dark Reader"}, "desc": {"message": "d"}});
        assert_eq!(localize("__MSG_appName__: __MSG_desc__ __MSG_missing__", &msgs), "Dark Reader: d ");
        assert_eq!(mime_for("a/b.JS"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("x.bin"), "application/octet-stream");
        assert_eq!(version_cmp("1.2", "1.2.0"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("1.10", "1.9"), std::cmp::Ordering::Greater);
    }

    /// 0.11.7, audit C6: messages that name themselves are inserted once, not searched again.
    #[test]
    fn localize_is_one_pass() {
        let msgs = serde_json::json!({"a": {"message": "__MSG_a__"}, "b": {"message": "__MSG_a____MSG_a__"}, "c": {"message": "C"}});
        assert_eq!(localize("__MSG_a__", &msgs), "__MSG_a__");
        assert_eq!(localize("x __MSG_b__ y", &msgs), "x __MSG_a____MSG_a__ y");
        assert_eq!(localize("__MSG_c__-__MSG_C__-__MSG_open", &msgs), "C-C-__MSG_open");
        assert_eq!(localize("plain", &msgs), "plain");
    }

    /// 0.11.7, audit C5: only locale names become a path under `_locales`.
    #[test]
    fn locale_names() {
        for ok in ["en", "de", "pt_BR", "zh_Hant", "es_419", "fil"] {
            assert!(valid_locale(ok), "{ok}");
        }
        for bad in [
            "",
            "e",
            "english",
            "\\\\evil\\share",
            "//evil/share",
            "C:\\x",
            "../..",
            "en/../..",
            "en_",
            "en_US_x",
            "en-US",
            "en_..",
            "ab\u{0}",
        ] {
            assert!(!valid_locale(bad), "{bad:?}");
        }
    }

    #[test]
    fn dnr_translation() {
        let rules = r#"[
          {"id":1,"priority":1,"action":{"type":"block"},"condition":{"urlFilter":"||ads.example.com^","resourceTypes":["script","image"]}},
          {"id":2,"priority":2,"action":{"type":"allow"},"condition":{"urlFilter":"||ads.example.com/ok","initiatorDomains":["good.example"]}},
          {"id":3,"action":{"type":"redirect","redirect":{"url":"https://x/"}},"condition":{"urlFilter":"a"}},
          {"id":4,"action":{"type":"block"},"condition":{"regexFilter":"^https://t\\.co/.*","domainType":"thirdParty","isUrlFilterCaseSensitive":true}},
          {"id":5,"action":{"type":"block"},"condition":{"resourceTypes":["unknown_kind"]}},
          {"id":6,"action":{"type":"block"},"condition":{"excludedInitiatorDomains":["me.example"],"excludedResourceTypes":["main_frame"]}}
        ]"#;
        let t = translate_dnr("Fixture", rules).unwrap();
        assert_eq!((t.translated, t.skipped), (4, 2));
        let lines: Vec<&str> = t.filters.lines().collect();
        assert_eq!(lines[0], "! Extension rules: Fixture");
        assert_eq!(lines[1], "||ads.example.com^$script,image");
        assert_eq!(lines[2], "@@||ads.example.com/ok$domain=good.example");
        assert_eq!(lines[3], "/^https://t\\.co/.*/$third-party,match-case");
        assert_eq!(lines[4], "*$~document,domain=~me.example");
        assert!(translate_dnr("x", "nope").is_err());
    }

    #[test]
    fn bundle_shape() {
        let e = BundledExtension {
            id: "a".repeat(32),
            manifest_json: "{\"name\":\"x\"}".into(),
            messages_json: String::new(),
            scripts: vec![BundledScript {
                id: "a".repeat(32),
                matches: vec!["<all_urls>".into()],
                exclude_matches: vec![],
                all_frames: false,
                run_at: "document_idle".into(),
                js: vec!["console.log(1)".into()],
                css: vec!["cs.css".into()],
            }],
        };
        let b = content_bundle("function (id) { return { run: function () {}, report: function () {} }; }", &[e]);
        assert!(
            b.contains("krunk\\.invalid")
                && b.contains("ext.run([\"<all_urls>\"], [], false, \"document_idle\", [\"cs.css\"]")
        );
        assert!(b.contains("console.log(1)") && b.starts_with("(function () {"));
    }
}
