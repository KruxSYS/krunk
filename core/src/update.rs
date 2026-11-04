//! The update check: which manifest to fetch, whether to trust it, and
//! whether it offers something newer.
//!
//! The update server publishes `<base>/updates/win-x64/latest.json` and a
//! detached Ed25519 signature `latest.json.sig` (base64 of the 64 signature
//! bytes, made by `dist/server/publish-release.sh` with the release key). The
//! public key is built into Krunk. A manifest is only believed when its exact
//! bytes carry a valid signature; the installer it names must be on the same
//! origin as the update server, and the downloaded file must have the
//! manifest's size and SHA-256 before it is run. HTTPS is required unless
//! Krunk was built with the `dev-updates` feature (local testing only).
//!
//! Manifest (unknown fields are ignored, so later versions can add some):
//! ```json
//! { "version": "0.2.1", "released": "2026-11-05",
//!   "url": "/download/0.2.1/Krunk-Setup-0.2.1.exe",
//!   "sha256": "<64 hex digits>", "size": 143519795,
//!   "notes_url": "/download/0.2.1/NOTES.md", "min_supported": "0.2.0" }
//! ```
//! `min_supported` is the oldest version the installer can update in place;
//! older copies are pointed at the download page instead.

use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signature, VerifyingKey};
pub use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use url::Url;

/// The update channel of this build.
pub const CHANNEL: &str = "win-x64";
/// A manifest is a few hundred bytes; anything much larger is refused unread.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
pub const MAX_SIGNATURE_BYTES: usize = 256;
/// The largest installer Krunk downloads.
pub const MAX_INSTALLER_BYTES: u64 = 1 << 30;
/// First automatic check after start, then once a day.
pub const FIRST_CHECK_DELAY_MS: u32 = 60_000;
pub const CHECK_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// Why an update check or download was rejected. Logged as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// The configured update server address is unusable.
    BadBase(String),
    /// http:// in a build that requires https://.
    Insecure(String),
    TooLarge(&'static str),
    BadSignatureEncoding,
    /// The signature does not match the manifest and the built-in key.
    BadSignature,
    BadPublicKey,
    Malformed(String),
    /// The installer URL is not on the update server's origin.
    CrossOrigin(String),
    BadVersion(String),
    /// The download does not match the manifest.
    WrongSize {
        expected: u64,
        actual: u64,
    },
    WrongHash,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::BadBase(e) => write!(f, "bad update server address: {e}"),
            UpdateError::Insecure(u) => write!(f, "refusing a non-HTTPS address: {u}"),
            UpdateError::TooLarge(what) => write!(f, "{what} is too large"),
            UpdateError::BadSignatureEncoding => write!(f, "the signature file is not a base64 Ed25519 signature"),
            UpdateError::BadSignature => write!(f, "the manifest signature is not valid for Krunk's release key"),
            UpdateError::BadPublicKey => write!(f, "the built-in release key is not a valid Ed25519 public key"),
            UpdateError::Malformed(e) => write!(f, "malformed manifest: {e}"),
            UpdateError::CrossOrigin(u) => write!(f, "the installer is not on the update server: {u}"),
            UpdateError::BadVersion(v) => write!(f, "bad version: {v}"),
            UpdateError::WrongSize { expected, actual } => {
                write!(f, "the download has {actual} bytes, the manifest says {expected}")
            }
            UpdateError::WrongHash => write!(f, "the download's SHA-256 does not match the manifest"),
        }
    }
}

/// Where and how this build checks for updates.
#[derive(Debug, Clone)]
pub struct Config {
    /// `UPDATE_BASE_URL`, e.g. `https://krunk.example`.
    pub base: Url,
    pub public_key: [u8; 32],
    /// `http://` is accepted (dev-updates builds only).
    pub allow_http: bool,
}

impl Config {
    pub fn new(base: &str, public_key: [u8; 32], allow_http: bool) -> Result<Config, UpdateError> {
        let mut base = Url::parse(base).map_err(|e| UpdateError::BadBase(e.to_string()))?;
        check_scheme(&base, allow_http)?;
        if base.host_str().is_none() || base.cannot_be_a_base() {
            return Err(UpdateError::BadBase(base.to_string()));
        }
        if !base.username().is_empty() || base.password().is_some() || base.query().is_some() {
            return Err(UpdateError::BadBase("no credentials or query allowed".into()));
        }
        base.set_fragment(None);
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }
        VerifyingKey::from_bytes(&public_key).map_err(|_| UpdateError::BadPublicKey)?;
        Ok(Config { base, public_key, allow_http })
    }

    /// `<base>/updates/win-x64/latest.json`.
    pub fn manifest_url(&self) -> Url {
        #[allow(clippy::expect_used)] // a constant relative path on a checked base
        self.base.join(&format!("updates/{CHANNEL}/latest.json")).expect("valid manifest path")
    }

    pub fn signature_url(&self) -> Url {
        #[allow(clippy::expect_used)]
        self.base.join(&format!("updates/{CHANNEL}/latest.json.sig")).expect("valid signature path")
    }
}

fn check_scheme(u: &Url, allow_http: bool) -> Result<(), UpdateError> {
    match u.scheme() {
        "https" => Ok(()),
        "http" if allow_http => Ok(()),
        _ => Err(UpdateError::Insecure(u.to_string())),
    }
}

/// Reads a PEM `PUBLIC KEY` (SubjectPublicKeyInfo, as written by
/// `openssl pkey -pubout`) holding an Ed25519 key.
pub fn public_key_from_pem(pem: &str) -> Result<[u8; 32], UpdateError> {
    const SPKI_PREFIX: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];
    let body = pem
        .trim()
        .strip_prefix("-----BEGIN PUBLIC KEY-----")
        .and_then(|s| s.strip_suffix("-----END PUBLIC KEY-----"))
        .ok_or(UpdateError::BadPublicKey)?;
    let b64: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let der = Base64::decode_vec(&b64).map_err(|_| UpdateError::BadPublicKey)?;
    if der.len() != 44 || der[..12] != SPKI_PREFIX {
        return Err(UpdateError::BadPublicKey);
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&der[12..]);
    VerifyingKey::from_bytes(&key).map_err(|_| UpdateError::BadPublicKey)?;
    Ok(key)
}

#[derive(Deserialize)]
struct RawManifest {
    version: String,
    #[serde(default)]
    released: String,
    url: String,
    sha256: String,
    size: u64,
    #[serde(default)]
    notes_url: Option<String>,
    #[serde(default)]
    min_supported: Option<String>,
}

/// A signed, checked manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub released: String,
    /// The installer, on the update server's origin.
    pub url: Url,
    pub sha256: [u8; 32],
    pub size: u64,
    pub notes_url: Option<Url>,
    pub min_supported: Option<Version>,
}

/// Checks the signature over the exact manifest bytes, then the manifest.
pub fn verify(manifest: &[u8], signature: &[u8], cfg: &Config) -> Result<Manifest, UpdateError> {
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::TooLarge("the manifest"));
    }
    if signature.len() > MAX_SIGNATURE_BYTES {
        return Err(UpdateError::TooLarge("the signature"));
    }
    let sig_text = std::str::from_utf8(signature).map_err(|_| UpdateError::BadSignatureEncoding)?.trim();
    let sig_bytes = Base64::decode_vec(sig_text).map_err(|_| UpdateError::BadSignatureEncoding)?;
    let sig_bytes: [u8; 64] = sig_bytes.try_into().map_err(|_| UpdateError::BadSignatureEncoding)?;
    let key = VerifyingKey::from_bytes(&cfg.public_key).map_err(|_| UpdateError::BadPublicKey)?;
    key.verify_strict(manifest, &Signature::from_bytes(&sig_bytes)).map_err(|_| UpdateError::BadSignature)?;
    parse_manifest(manifest, cfg)
}

/// The manifest's contents (after its signature was checked).
fn parse_manifest(bytes: &[u8], cfg: &Config) -> Result<Manifest, UpdateError> {
    let raw: RawManifest = serde_json::from_slice(bytes).map_err(|e| UpdateError::Malformed(e.to_string()))?;
    let version = parse_version(&raw.version)?;
    let min_supported = raw.min_supported.as_deref().map(parse_version).transpose()?;
    let manifest_url = cfg.manifest_url();
    let url = manifest_url.join(&raw.url).map_err(|e| UpdateError::Malformed(format!("url: {e}")))?;
    check_scheme(&url, cfg.allow_http)?;
    if url.origin() != cfg.base.origin() {
        return Err(UpdateError::CrossOrigin(url.to_string()));
    }
    let notes_url = match raw.notes_url.as_deref() {
        None | Some("") => None,
        Some(n) => {
            let u = manifest_url.join(n).map_err(|e| UpdateError::Malformed(format!("notes_url: {e}")))?;
            check_scheme(&u, cfg.allow_http)?;
            Some(u)
        }
    };
    let sha256 =
        parse_sha256(&raw.sha256).ok_or_else(|| UpdateError::Malformed("sha256 must be 64 hex digits".into()))?;
    if raw.size == 0 || raw.size > MAX_INSTALLER_BYTES {
        return Err(UpdateError::Malformed(format!("size {} is out of range", raw.size)));
    }
    if raw.released.len() > 64 {
        return Err(UpdateError::Malformed("released is too long".into()));
    }
    Ok(Manifest { version, released: raw.released, url, sha256, size: raw.size, notes_url, min_supported })
}

pub fn parse_version(v: &str) -> Result<Version, UpdateError> {
    Version::parse(v.trim()).map_err(|_| UpdateError::BadVersion(v.chars().take(40).collect()))
}

fn parse_sha256(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What a verified manifest means for the running version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// The manifest's version is this one, or older (never offered: no downgrades).
    UpToDate,
    /// Newer, and the installer can update this version in place.
    Install,
    /// Newer, but this version is older than `min_supported`: download it by hand.
    DownloadOnly,
}

pub fn offer(current: &Version, m: &Manifest) -> Offer {
    if m.version <= *current {
        Offer::UpToDate
    } else if m.min_supported.as_ref().is_some_and(|min| current < min) {
        Offer::DownloadOnly
    } else {
        Offer::Install
    }
}

/// SHA-256 of a download, fed in pieces as it arrives.
#[derive(Default)]
pub struct DownloadCheck {
    hasher: Sha256,
    bytes: u64,
}

impl DownloadCheck {
    pub fn update(&mut self, chunk: &[u8]) {
        self.hasher.update(chunk);
        self.bytes += chunk.len() as u64;
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The whole file arrived: does it match the manifest?
    pub fn finish(self, m: &Manifest) -> Result<(), UpdateError> {
        if self.bytes != m.size {
            return Err(UpdateError::WrongSize { expected: m.size, actual: self.bytes });
        }
        let digest: [u8; 32] = self.hasher.finalize().into();
        if digest != m.sha256 {
            return Err(UpdateError::WrongHash);
        }
        Ok(())
    }
}

/// The last check, kept in `update.json` and shown in Settings → About.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Status {
    /// When the last check finished (ms since the epoch; 0: never).
    pub last_check_ms: i64,
    /// What it found, in words ("Krunk is up to date", "Krunk 0.2.1 is
    /// available", or why it failed).
    pub last_result: String,
    pub last_ok: bool,
}

impl Status {
    pub fn from_json(json: &str) -> Status {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Is an automatic check due? `last_ms` is the last attempt (0: never).
pub fn check_due(last_ms: i64, now_ms: i64) -> bool {
    last_ms <= 0 || now_ms < last_ms || now_ms - last_ms >= CHECK_INTERVAL_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // Test-only keys (never used for releases).
    const TEST_SEED: [u8; 32] = [7; 32];
    const OTHER_SEED: [u8; 32] = [9; 32];

    fn key(seed: [u8; 32]) -> SigningKey {
        SigningKey::from_bytes(&seed)
    }

    fn cfg() -> Config {
        Config::new("https://krunk.example", key(TEST_SEED).verifying_key().to_bytes(), false).unwrap()
    }

    fn sign(seed: [u8; 32], body: &[u8]) -> Vec<u8> {
        Base64::encode_string(&key(seed).sign(body).to_bytes()).into_bytes()
    }

    fn manifest_json(version: &str, url: &str, size: u64) -> String {
        format!(
            r#"{{"version":"{version}","released":"2026-11-05","url":"{url}","sha256":"{}","size":{size},"notes_url":"/download/{version}/NOTES.md","min_supported":"0.2.0"}}"#,
            to_hex(&Sha256::digest(b"installer"))
        )
    }

    fn good() -> String {
        manifest_json("0.2.1", "/download/0.2.1/Krunk-Setup-0.2.1.exe", 9)
    }

    #[test]
    fn urls_follow_the_base() {
        let c = cfg();
        assert_eq!(c.manifest_url().as_str(), "https://krunk.example/updates/win-x64/latest.json");
        assert_eq!(c.signature_url().as_str(), "https://krunk.example/updates/win-x64/latest.json.sig");
        let sub = Config::new("https://example.org/krunk", c.public_key, false).unwrap();
        assert_eq!(sub.manifest_url().as_str(), "https://example.org/krunk/updates/win-x64/latest.json");
    }

    #[test]
    fn http_only_in_dev_builds() {
        let pk = cfg().public_key;
        assert!(matches!(Config::new("http://krunk.example", pk, false), Err(UpdateError::Insecure(_))));
        assert!(Config::new("http://127.0.0.1:18080", pk, true).is_ok());
        assert!(matches!(Config::new("ftp://krunk.example", pk, true), Err(UpdateError::Insecure(_))));
        assert!(Config::new("https://user:pw@krunk.example", pk, false).is_err());
        assert!(Config::new("not a url", pk, false).is_err());
    }

    #[test]
    fn a_signed_manifest_is_accepted() {
        let body = good();
        let m = verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &cfg()).unwrap();
        assert_eq!(m.version, Version::new(0, 2, 1));
        assert_eq!(m.url.as_str(), "https://krunk.example/download/0.2.1/Krunk-Setup-0.2.1.exe");
        assert_eq!(m.notes_url.unwrap().as_str(), "https://krunk.example/download/0.2.1/NOTES.md");
        assert_eq!(m.size, 9);
        assert_eq!(m.min_supported, Some(Version::new(0, 2, 0)));
        // A trailing newline in the .sig file is fine.
        let mut sig = sign(TEST_SEED, body.as_bytes());
        sig.push(b'\n');
        assert!(verify(body.as_bytes(), &sig, &cfg()).is_ok());
    }

    #[test]
    fn a_bad_signature_is_rejected() {
        let body = good();
        let sig = sign(TEST_SEED, body.as_bytes());
        // One byte of the manifest changed after signing.
        let tampered = body.replace("0.2.1", "0.2.2");
        assert_eq!(verify(tampered.as_bytes(), &sig, &cfg()), Err(UpdateError::BadSignature));
        // Even whitespace counts: the exact bytes are signed.
        assert_eq!(verify(format!("{body} ").as_bytes(), &sig, &cfg()), Err(UpdateError::BadSignature));
        // A flipped signature bit.
        let mut raw = Base64::decode_vec(std::str::from_utf8(&sig).unwrap()).unwrap();
        raw[10] ^= 1;
        let flipped = Base64::encode_string(&raw).into_bytes();
        assert_eq!(verify(body.as_bytes(), &flipped, &cfg()), Err(UpdateError::BadSignature));
        // Not a signature at all.
        assert_eq!(verify(body.as_bytes(), b"hello", &cfg()), Err(UpdateError::BadSignatureEncoding));
        assert_eq!(verify(body.as_bytes(), b"", &cfg()), Err(UpdateError::BadSignatureEncoding));
        let short = Base64::encode_string(&[1u8; 32]).into_bytes();
        assert_eq!(verify(body.as_bytes(), &short, &cfg()), Err(UpdateError::BadSignatureEncoding));
    }

    #[test]
    fn the_wrong_key_is_rejected() {
        let body = good();
        assert_eq!(verify(body.as_bytes(), &sign(OTHER_SEED, body.as_bytes()), &cfg()), Err(UpdateError::BadSignature));
    }

    #[test]
    fn malformed_manifests_are_rejected_even_when_signed() {
        let c = cfg();
        let check = |body: &str| verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &c);
        assert!(matches!(check("{"), Err(UpdateError::Malformed(_))));
        assert!(matches!(check("[]"), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(r#"{"version":"0.2.1"}"#), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace("\"0.2.1\"", "\"v2\"")), Err(UpdateError::BadVersion(_))));
        let hex = to_hex(&Sha256::digest(b"installer"));
        assert!(matches!(check(&good().replace(&hex, &hex[..62])), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace(&hex, &format!("{}zz", &hex[..62]))), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace("\"size\":9", "\"size\":-1")), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace("\"size\":9", "\"size\":\"9\"")), Err(UpdateError::Malformed(_))));
        // Unknown fields are allowed (later manifests may add some).
        assert!(check(&good().replace("{\"version\"", "{\"channel\":\"win-x64\",\"version\"")).is_ok());
    }

    #[test]
    fn oversized_input_is_refused() {
        let c = cfg();
        let huge = format!("{}{}", good(), " ".repeat(MAX_MANIFEST_BYTES));
        assert_eq!(
            verify(huge.as_bytes(), &sign(TEST_SEED, huge.as_bytes()), &c),
            Err(UpdateError::TooLarge("the manifest"))
        );
        assert_eq!(verify(good().as_bytes(), &[b'A'; 300], &c), Err(UpdateError::TooLarge("the signature")));
        let big = manifest_json("0.2.1", "/x.exe", MAX_INSTALLER_BYTES + 1);
        assert!(matches!(verify(big.as_bytes(), &sign(TEST_SEED, big.as_bytes()), &c), Err(UpdateError::Malformed(_))));
        let zero = manifest_json("0.2.1", "/x.exe", 0);
        assert!(matches!(
            verify(zero.as_bytes(), &sign(TEST_SEED, zero.as_bytes()), &c),
            Err(UpdateError::Malformed(_))
        ));
    }

    #[test]
    fn the_installer_must_be_on_the_update_server() {
        let c = cfg();
        let check = |url: &str| {
            let body = manifest_json("0.2.1", url, 9);
            verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &c)
        };
        assert!(check("Krunk-Setup.exe").is_ok(), "relative to the manifest");
        assert!(check("https://krunk.example/download/x.exe").is_ok());
        assert!(matches!(check("https://evil.example/x.exe"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(check("//evil.example/x.exe"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(check("https://krunk.example:8443/x.exe"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(check("https://sub.krunk.example/x.exe"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(check("http://krunk.example/x.exe"), Err(UpdateError::Insecure(_))));
        assert!(matches!(check("file:///C:/x.exe"), Err(UpdateError::Insecure(_))));
        // Release notes may not be http:// either.
        let body = good().replace("\"notes_url\":\"/download", "\"notes_url\":\"http://krunk.example/download");
        assert!(matches!(
            verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &c),
            Err(UpdateError::Insecure(_))
        ));
    }

    #[test]
    fn dev_builds_accept_http_on_the_same_origin() {
        let c = Config::new("http://127.0.0.1:18080", key(TEST_SEED).verifying_key().to_bytes(), true).unwrap();
        let body = manifest_json("0.2.1", "/download/0.2.1/Krunk-Setup-0.2.1.exe", 9);
        let m = verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &c).unwrap();
        assert_eq!(m.url.as_str(), "http://127.0.0.1:18080/download/0.2.1/Krunk-Setup-0.2.1.exe");
        let other = manifest_json("0.2.1", "http://127.0.0.1:9999/x.exe", 9);
        assert!(matches!(
            verify(other.as_bytes(), &sign(TEST_SEED, other.as_bytes()), &c),
            Err(UpdateError::CrossOrigin(_))
        ));
    }

    #[test]
    fn only_newer_versions_are_offered() {
        let body = good();
        let m = verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &cfg()).unwrap();
        assert_eq!(offer(&Version::new(0, 2, 0), &m), Offer::Install);
        assert_eq!(offer(&Version::new(0, 2, 1), &m), Offer::UpToDate, "same version");
        assert_eq!(offer(&Version::new(0, 3, 0), &m), Offer::UpToDate, "no downgrades");
        assert_eq!(offer(&Version::new(0, 1, 9), &m), Offer::DownloadOnly, "older than min_supported");
        assert_eq!(offer(&Version::parse("0.2.1-beta.1").unwrap(), &m), Offer::Install, "pre-release < release");
        // Semver, not string order.
        let ten = manifest_json("0.10.0", "/x.exe", 9);
        let m10 = verify(ten.as_bytes(), &sign(TEST_SEED, ten.as_bytes()), &cfg()).unwrap();
        assert_eq!(offer(&Version::new(0, 9, 0), &m10), Offer::Install);
    }

    #[test]
    fn downloads_must_match_size_and_hash() {
        let body = good();
        let m = verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &cfg()).unwrap();
        let mut ok = DownloadCheck::default();
        ok.update(b"inst");
        ok.update(b"aller");
        assert_eq!(ok.bytes(), 9);
        assert_eq!(ok.finish(&m), Ok(()));
        let mut wrong = DownloadCheck::default();
        wrong.update(b"installex");
        assert_eq!(wrong.finish(&m), Err(UpdateError::WrongHash));
        let mut short = DownloadCheck::default();
        short.update(b"install");
        assert_eq!(short.finish(&m), Err(UpdateError::WrongSize { expected: 9, actual: 7 }));
    }

    #[test]
    fn pem_keys() {
        // `openssl genpkey -algorithm ed25519 | openssl pkey -pubout` of a test key.
        let pem = include_str!("../tests/fixtures/update/test-signing.pub");
        let key = public_key_from_pem(pem).unwrap();
        assert!(Config::new("https://krunk.example", key, false).is_ok());
        assert_eq!(
            public_key_from_pem("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----"),
            Err(UpdateError::BadPublicKey)
        );
        assert_eq!(public_key_from_pem("garbage"), Err(UpdateError::BadPublicKey));
        // The committed release key parses.
        let release = include_str!("../../assets/keys/update-signing.pub");
        assert!(public_key_from_pem(release).is_ok());
    }

    #[test]
    fn openssl_signatures_verify() {
        // Made with: openssl pkeyutl -sign -rawin -inkey test-signing.key -in latest.json | base64 -w0
        // (the exact command publish-release.sh uses).
        let pem = include_str!("../tests/fixtures/update/test-signing.pub");
        let c = Config::new("https://krunk.example", public_key_from_pem(pem).unwrap(), false).unwrap();
        let body = include_bytes!("../tests/fixtures/update/latest.json");
        let sig = include_bytes!("../tests/fixtures/update/latest.json.sig");
        let m = verify(body, sig, &c).unwrap();
        assert_eq!(m.version, Version::new(0, 2, 1));
        let mut tampered = body.to_vec();
        tampered[20] ^= 1;
        assert_eq!(verify(&tampered, sig, &c), Err(UpdateError::BadSignature));
    }

    #[test]
    fn status_round_trips() {
        let s = Status { last_check_ms: 5, last_result: "Krunk is up to date".into(), last_ok: true };
        assert_eq!(Status::from_json(&s.to_json()), s);
        assert_eq!(Status::from_json("garbage"), Status::default());
    }

    #[test]
    fn checks_are_daily() {
        assert!(check_due(0, 1_000));
        assert!(!check_due(1_000, 1_000 + CHECK_INTERVAL_MS - 1));
        assert!(check_due(1_000, 1_000 + CHECK_INTERVAL_MS));
        assert!(check_due(5_000, 1_000), "the clock went back");
    }
}
