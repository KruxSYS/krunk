//! The Android update channel (`android-arm64`, Krunk for Android 0.5.0, website APK only).
//!
//! Same scheme as Windows ([`crate::update`]): `<base>/updates/android-arm64/latest.json` and a
//! detached Ed25519 signature `latest.json.sig` (base64 of the 64 signature bytes over the exact
//! file bytes), made by `dist/server/publish-android.sh` with the same release key. The public key
//! is built into the app. Only the user starts a check (Settings › About › Check for updates); this
//! module never does any I/O.
//!
//! Manifest (unknown fields are ignored):
//! ```json
//! { "channel": "android-arm64", "version": "0.5.0", "version_code": 7, "released": "2026-11-14",
//!   "package": "org.krunk.browser", "min_sdk": 26, "signing_cert_sha256": "<64 hex>",
//!   "notes": "<Markdown>", "notes_url": "/download/android/0.5.0/NOTES.md",
//!   "files": [ { "abi": "arm64-v8a", "url": "/download/android/0.5.0/…-arm64-v8a-release.apk",
//!                "size": 123, "sha256": "<64 hex>" }, … ] }
//! ```
//! Every file must be on the update server's origin. The app picks the file for the first ABI of
//! `Build.SUPPORTED_ABIS` that has one, and the `universal` file only when none has
//! ([`AndroidManifest::pick`]). An update is offered only for a higher `version_code`
//! ([`offer`]); the downloaded APK must then match size and SHA-256, and (checked by the app with
//! the package manager) carry this package, this `version_code` and the installed app's
//! certificate, which must also be `signing_cert_sha256`.

use crate::update::{check_scheme, check_signature, parse_sha256, parse_version, Config, UpdateError, Version};
use serde::Deserialize;
use url::Url;

/// The update channel of Krunk for Android's website APK.
pub const CHANNEL: &str = "android-arm64";
/// The ABI name of the file every device can install.
pub const UNIVERSAL: &str = "universal";
/// The manifest carries the release notes, so it may be larger than the Windows one.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_NOTES_CHARS: usize = 32 * 1024;
pub const MAX_FILES: usize = 8;
/// The largest APK Krunk downloads.
pub const MAX_APK_BYTES: u64 = 1 << 30;

/// `<base>/updates/android-arm64/latest.json`.
pub fn manifest_url(cfg: &Config) -> Url {
    #[allow(clippy::expect_used)] // a constant relative path on a checked base
    cfg.base.join(&format!("updates/{CHANNEL}/latest.json")).expect("valid manifest path")
}

pub fn signature_url(cfg: &Config) -> Url {
    #[allow(clippy::expect_used)]
    cfg.base.join(&format!("updates/{CHANNEL}/latest.json.sig")).expect("valid signature path")
}

#[derive(Deserialize)]
struct RawFile {
    abi: String,
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct RawManifest {
    channel: String,
    version: String,
    version_code: u64,
    #[serde(default)]
    released: String,
    package: String,
    min_sdk: u32,
    signing_cert_sha256: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    notes_url: Option<String>,
    files: Vec<RawFile>,
}

/// One APK of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkFile {
    /// `arm64-v8a`, `x86_64` or `universal`.
    pub abi: String,
    /// On the update server's origin.
    pub url: Url,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// A signed, checked Android manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidManifest {
    pub version: Version,
    pub version_code: u64,
    pub released: String,
    pub package: String,
    pub min_sdk: u32,
    pub signing_cert_sha256: [u8; 32],
    pub notes: String,
    pub notes_url: Option<Url>,
    pub files: Vec<ApkFile>,
}

impl AndroidManifest {
    /// The file for a device whose ABIs are `supported` (most preferred first, as
    /// `Build.SUPPORTED_ABIS`): the first one with its own file, else the universal one.
    pub fn pick(&self, supported: &[String]) -> Option<&ApkFile> {
        supported
            .iter()
            .filter(|abi| abi.as_str() != UNIVERSAL)
            .find_map(|abi| self.files.iter().find(|f| &f.abi == abi))
            .or_else(|| self.files.iter().find(|f| f.abi == UNIVERSAL))
    }
}

/// Checks the signature over the exact manifest bytes, then the manifest: the channel, that it is
/// for `package`, and that every file is on the update server.
pub fn verify(manifest: &[u8], signature: &[u8], cfg: &Config, package: &str) -> Result<AndroidManifest, UpdateError> {
    check_signature(manifest, signature, &cfg.public_key, MAX_MANIFEST_BYTES)?;
    parse(manifest, cfg, package)
}

fn parse(bytes: &[u8], cfg: &Config, package: &str) -> Result<AndroidManifest, UpdateError> {
    let raw: RawManifest = serde_json::from_slice(bytes).map_err(|e| UpdateError::Malformed(e.to_string()))?;
    if raw.channel != CHANNEL {
        return Err(UpdateError::Malformed(format!("channel {:?} is not {CHANNEL}", short(&raw.channel))));
    }
    if raw.package != package {
        return Err(UpdateError::Malformed(format!("package {:?} is not {package}", short(&raw.package))));
    }
    let version = parse_version(&raw.version)?;
    if raw.version_code == 0 || raw.version_code > i32::MAX as u64 {
        return Err(UpdateError::Malformed(format!("version_code {} is out of range", raw.version_code)));
    }
    if raw.released.len() > 64 {
        return Err(UpdateError::Malformed("released is too long".into()));
    }
    if raw.notes.chars().count() > MAX_NOTES_CHARS {
        return Err(UpdateError::Malformed("notes are too long".into()));
    }
    let signing_cert_sha256 = parse_sha256(&raw.signing_cert_sha256.to_ascii_lowercase())
        .ok_or_else(|| UpdateError::Malformed("signing_cert_sha256 must be 64 hex digits".into()))?;
    let base = manifest_url(cfg);
    let same_origin = |what: &str, u: &str| -> Result<Url, UpdateError> {
        let url = base.join(u).map_err(|e| UpdateError::Malformed(format!("{what}: {e}")))?;
        check_scheme(&url, cfg.allow_http)?;
        if url.origin() != cfg.base.origin() {
            return Err(UpdateError::CrossOrigin(url.to_string()));
        }
        Ok(url)
    };
    let notes_url = match raw.notes_url.as_deref() {
        None | Some("") => None,
        Some(n) => Some(same_origin("notes_url", n)?),
    };
    if raw.files.is_empty() || raw.files.len() > MAX_FILES {
        return Err(UpdateError::Malformed(format!("{} files listed", raw.files.len())));
    }
    let mut files: Vec<ApkFile> = Vec::with_capacity(raw.files.len());
    for f in raw.files {
        let abi_ok = !f.abi.is_empty()
            && f.abi.len() <= 32
            && f.abi.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        if !abi_ok {
            return Err(UpdateError::Malformed(format!("bad abi {:?}", short(&f.abi))));
        }
        if files.iter().any(|g| g.abi == f.abi) {
            return Err(UpdateError::Malformed(format!("abi {} listed twice", f.abi)));
        }
        let url = same_origin("url", &f.url)?;
        if f.size == 0 || f.size > MAX_APK_BYTES {
            return Err(UpdateError::Malformed(format!("size {} is out of range", f.size)));
        }
        let sha256 =
            parse_sha256(&f.sha256).ok_or_else(|| UpdateError::Malformed("sha256 must be 64 hex digits".into()))?;
        files.push(ApkFile { abi: f.abi, url, size: f.size, sha256 });
    }
    Ok(AndroidManifest {
        version,
        version_code: raw.version_code,
        released: raw.released,
        package: raw.package,
        min_sdk: raw.min_sdk,
        signing_cert_sha256,
        notes: raw.notes,
        notes_url,
        files,
    })
}

fn short(s: &str) -> String {
    s.chars().take(40).collect()
}

/// What a verified manifest means for the installed app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// The same or an older build (never offered: no downgrades).
    UpToDate,
    /// Newer, and this device can run it.
    Install,
    /// Newer, but it needs a later Android than this device has.
    NeedsNewerAndroid,
}

/// Compares by `version_code`, which Android itself uses to allow an update.
pub fn offer(installed_version_code: u64, device_sdk: u32, m: &AndroidManifest) -> Offer {
    if m.version_code <= installed_version_code {
        Offer::UpToDate
    } else if device_sdk < m.min_sdk {
        Offer::NeedsNewerAndroid
    } else {
        Offer::Install
    }
}

/// Does a finished download match the manifest's file?
pub fn check_download(file: &ApkFile, size: u64, sha256: &[u8; 32]) -> Result<(), UpdateError> {
    if size != file.size {
        return Err(UpdateError::WrongSize { expected: file.size, actual: size });
    }
    if sha256 != &file.sha256 {
        return Err(UpdateError::WrongHash);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::{public_key_from_pem, to_hex};
    use base64ct::{Base64, Encoding};
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    // Test-only keys (never used for releases).
    const TEST_SEED: [u8; 32] = [7; 32];
    const OTHER_SEED: [u8; 32] = [9; 32];
    const PKG: &str = "org.krunk.browser";
    const CERT: &str = "c94b8915cd8b393a5fc48bc132a6de026374e76337c4341c59dd60328373e624";

    fn cfg() -> Config {
        Config::new("https://krunk.example", SigningKey::from_bytes(&TEST_SEED).verifying_key().to_bytes(), false)
            .unwrap()
    }

    fn sign(seed: [u8; 32], body: &[u8]) -> Vec<u8> {
        Base64::encode_string(&SigningKey::from_bytes(&seed).sign(body).to_bytes()).into_bytes()
    }

    fn hex_of(data: &[u8]) -> String {
        to_hex(&Sha256::digest(data))
    }

    fn good() -> String {
        format!(
            r#"{{"channel":"android-arm64","version":"0.5.0","version_code":7,"released":"2026-11-14",
"package":"org.krunk.browser","min_sdk":26,"signing_cert_sha256":"{CERT}","notes":"- Updates",
"notes_url":"/download/android/0.5.0/NOTES.md","files":[
{{"abi":"arm64-v8a","url":"/download/android/0.5.0/K-arm64-v8a.apk","size":3,"sha256":"{a}"}},
{{"abi":"universal","url":"/download/android/0.5.0/K-universal.apk","size":3,"sha256":"{u}"}},
{{"abi":"x86_64","url":"/download/android/0.5.0/K-x86_64.apk","size":3,"sha256":"{x}"}}]}}"#,
            a = hex_of(b"arm"),
            u = hex_of(b"uni"),
            x = hex_of(b"x86"),
        )
    }

    fn check(body: &str) -> Result<AndroidManifest, UpdateError> {
        verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &cfg(), PKG)
    }

    fn abis(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn urls_follow_the_base() {
        let c = cfg();
        assert_eq!(manifest_url(&c).as_str(), "https://krunk.example/updates/android-arm64/latest.json");
        assert_eq!(signature_url(&c).as_str(), "https://krunk.example/updates/android-arm64/latest.json.sig");
        // The Windows channel is unchanged.
        assert_eq!(c.manifest_url().as_str(), "https://krunk.example/updates/win-x64/latest.json");
    }

    #[test]
    fn a_signed_manifest_is_accepted() {
        let m = check(&good()).unwrap();
        assert_eq!(m.version, Version::new(0, 5, 0));
        assert_eq!(m.version_code, 7);
        assert_eq!(m.min_sdk, 26);
        assert_eq!(to_hex(&m.signing_cert_sha256), CERT);
        assert_eq!(m.notes, "- Updates");
        assert_eq!(m.notes_url.unwrap().as_str(), "https://krunk.example/download/android/0.5.0/NOTES.md");
        assert_eq!(m.files.len(), 3);
        assert_eq!(m.files[0].url.as_str(), "https://krunk.example/download/android/0.5.0/K-arm64-v8a.apk");
        // Unknown fields are fine; an upper-case certificate too.
        assert!(check(&good().replace("{\"channel\"", "{\"later\":1,\"channel\"")).is_ok());
        assert!(check(&good().replace(CERT, &CERT.to_ascii_uppercase())).is_ok());
    }

    #[test]
    fn a_bad_signature_is_rejected() {
        let body = good();
        let sig = sign(TEST_SEED, body.as_bytes());
        let tampered = body.replace("\"version_code\":7", "\"version_code\":8");
        assert_eq!(verify(tampered.as_bytes(), &sig, &cfg(), PKG), Err(UpdateError::BadSignature));
        // One byte anywhere.
        let mut one = body.clone().into_bytes();
        one[body.len() / 2] ^= 0x01;
        assert_eq!(verify(&one, &sig, &cfg(), PKG), Err(UpdateError::BadSignature));
        assert_eq!(verify(body.as_bytes(), b"not base64!", &cfg(), PKG), Err(UpdateError::BadSignatureEncoding));
        assert_eq!(verify(body.as_bytes(), b"", &cfg(), PKG), Err(UpdateError::BadSignatureEncoding));
    }

    #[test]
    fn the_wrong_key_is_rejected() {
        let body = good();
        assert_eq!(
            verify(body.as_bytes(), &sign(OTHER_SEED, body.as_bytes()), &cfg(), PKG),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn a_windows_manifest_is_not_an_android_one() {
        // Signed with the same key, so only the contents can tell them apart.
        let win = r#"{"version":"0.20.1","released":"2026-11-14","url":"/download/0.20.1/Krunk-Setup-0.20.1.exe","sha256":"00","size":9}"#;
        assert!(matches!(check(win), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace("android-arm64", "win-x64")), Err(UpdateError::Malformed(_))));
        assert!(matches!(
            check(&good().replace("\"package\":\"org.krunk.browser\"", "\"package\":\"org.evil\"")),
            Err(UpdateError::Malformed(_))
        ));
    }

    #[test]
    fn malformed_manifests_are_rejected_even_when_signed() {
        assert!(matches!(check("{"), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace("\"0.5.0\"", "\"v5\"")), Err(UpdateError::BadVersion(_))));
        assert!(matches!(
            check(&good().replace("\"version_code\":7", "\"version_code\":0")),
            Err(UpdateError::Malformed(_))
        ));
        assert!(matches!(
            check(&good().replace("\"version_code\":7", "\"version_code\":-1")),
            Err(UpdateError::Malformed(_))
        ));
        assert!(matches!(check(&good().replace(CERT, &CERT[..60])), Err(UpdateError::Malformed(_))));
        assert!(matches!(check(&good().replace("\"size\":3", "\"size\":0")), Err(UpdateError::Malformed(_))));
        let a = hex_of(b"arm");
        assert!(matches!(check(&good().replace(&a, &a[..62])), Err(UpdateError::Malformed(_))));
        assert!(matches!(
            check(&good().replace("\"abi\":\"x86_64\"", "\"abi\":\"arm64-v8a\"")),
            Err(UpdateError::Malformed(_))
        ));
        assert!(matches!(
            check(&good().replace("\"abi\":\"x86_64\"", "\"abi\":\"../x\"")),
            Err(UpdateError::Malformed(_))
        ));
        let no_files = good().split("\"files\":").next().unwrap().to_string() + "\"files\":[]}";
        assert!(matches!(check(&no_files), Err(UpdateError::Malformed(_))));
        let long_notes = good().replace("- Updates", &"x".repeat(MAX_NOTES_CHARS + 1));
        assert!(matches!(check(&long_notes), Err(UpdateError::Malformed(_))));
    }

    #[test]
    fn oversized_input_is_refused() {
        let huge = format!("{}{}", good(), " ".repeat(MAX_MANIFEST_BYTES));
        assert_eq!(check(&huge), Err(UpdateError::TooLarge("the manifest")));
        assert_eq!(verify(good().as_bytes(), &[b'A'; 300], &cfg(), PKG), Err(UpdateError::TooLarge("the signature")));
    }

    #[test]
    fn files_must_be_on_the_update_server() {
        let at = |url: &str| check(&good().replace("/download/android/0.5.0/K-x86_64.apk", url));
        assert!(at("K-x86_64.apk").is_ok(), "relative to the manifest");
        assert!(at("https://krunk.example/x.apk").is_ok());
        assert!(matches!(at("https://evil.example/x.apk"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(at("//evil.example/x.apk"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(at("https://krunk.example:8443/x.apk"), Err(UpdateError::CrossOrigin(_))));
        assert!(matches!(at("http://krunk.example/x.apk"), Err(UpdateError::Insecure(_))));
        assert!(matches!(at("file:///sdcard/x.apk"), Err(UpdateError::Insecure(_))));
        assert!(matches!(at("content://x/x.apk"), Err(UpdateError::Insecure(_))));
        let notes = good().replace("\"notes_url\":\"/download", "\"notes_url\":\"https://evil.example/download");
        assert!(matches!(check(&notes), Err(UpdateError::CrossOrigin(_))));
    }

    #[test]
    fn qa_builds_accept_http_on_the_same_origin() {
        let key = SigningKey::from_bytes(&TEST_SEED).verifying_key().to_bytes();
        let c = Config::new("http://10.250.136.3:18090", key, true).unwrap();
        let body = good();
        let m = verify(body.as_bytes(), &sign(TEST_SEED, body.as_bytes()), &c, PKG).unwrap();
        assert_eq!(m.files[2].url.as_str(), "http://10.250.136.3:18090/download/android/0.5.0/K-x86_64.apk");
        // Release builds refuse the address outright.
        assert!(matches!(Config::new("http://10.250.136.3:18090", key, false), Err(UpdateError::Insecure(_))));
    }

    #[test]
    fn the_device_gets_its_own_abi() {
        let m = check(&good()).unwrap();
        let pick = |list: &[&str]| m.pick(&abis(list)).map(|f| f.abi.clone());
        assert_eq!(pick(&["arm64-v8a", "armeabi-v7a", "armeabi"]).as_deref(), Some("arm64-v8a"));
        assert_eq!(pick(&["x86_64", "x86", "arm64-v8a"]).as_deref(), Some("x86_64"));
        assert_eq!(pick(&["armeabi-v7a"]).as_deref(), Some("universal"), "no own file: universal");
        assert_eq!(pick(&[]).as_deref(), Some("universal"));
        assert_eq!(pick(&["universal"]).as_deref(), Some("universal"));
        let without = check(&good().replace(
            &format!(
                r#"{{"abi":"universal","url":"/download/android/0.5.0/K-universal.apk","size":3,"sha256":"{}"}},"#,
                hex_of(b"uni")
            ),
            "",
        ))
        .unwrap();
        assert_eq!(without.files.len(), 2);
        assert_eq!(without.pick(&abis(&["armeabi-v7a"])), None);
    }

    #[test]
    fn a_32_bit_tv_gets_the_armv7_file() {
        // 0.12.0 lists an armeabi-v7a APK in the same manifest (Android TV devices such as the
        // Chromecast with Google TV are 32-bit only); 64-bit phones still get theirs.
        let body = good().replace(
            r#"{"abi":"universal""#,
            &format!(
                r#"{{"abi":"armeabi-v7a","url":"/download/android/0.5.0/K-armeabi-v7a.apk","size":3,"sha256":"{}"}},{{"abi":"universal""#,
                hex_of(b"v7a")
            ),
        );
        let m = check(&body).unwrap();
        assert_eq!(m.files.len(), 4);
        let pick = |list: &[&str]| m.pick(&abis(list)).map(|f| f.abi.clone());
        assert_eq!(pick(&["armeabi-v7a", "armeabi"]).as_deref(), Some("armeabi-v7a"));
        assert_eq!(pick(&["arm64-v8a", "armeabi-v7a", "armeabi"]).as_deref(), Some("arm64-v8a"));
        assert_eq!(pick(&["x86_64", "x86", "arm64-v8a"]).as_deref(), Some("x86_64"));
    }

    #[test]
    fn only_higher_version_codes_are_offered() {
        let m = check(&good()).unwrap();
        assert_eq!(offer(5, 36, &m), Offer::Install, "0.4.0 -> 0.5.0");
        assert_eq!(offer(6, 29, &m), Offer::Install, "0.5.0-qa -> 0.5.0");
        assert_eq!(offer(7, 36, &m), Offer::UpToDate, "same build");
        assert_eq!(offer(8, 36, &m), Offer::UpToDate, "no downgrades");
        let high = check(&good().replace("\"min_sdk\":26", "\"min_sdk\":30")).unwrap();
        assert_eq!(offer(5, 29, &high), Offer::NeedsNewerAndroid);
        assert_eq!(offer(5, 30, &high), Offer::Install);
        assert_eq!(offer(7, 29, &high), Offer::UpToDate);
    }

    #[test]
    fn downloads_must_match_size_and_hash() {
        let m = check(&good()).unwrap();
        let f = &m.files[0];
        let digest: [u8; 32] = Sha256::digest(b"arm").into();
        assert_eq!(check_download(f, 3, &digest), Ok(()));
        let other: [u8; 32] = Sha256::digest(b"arn").into();
        assert_eq!(check_download(f, 3, &other), Err(UpdateError::WrongHash));
        assert_eq!(check_download(f, 4, &digest), Err(UpdateError::WrongSize { expected: 3, actual: 4 }));
    }

    #[test]
    fn openssl_signatures_verify() {
        // Made with the test key and the command publish-android.sh uses:
        //   openssl pkeyutl -sign -rawin -inkey test-signing.key -in android-latest.json | base64 -w0
        let pem = include_str!("../tests/fixtures/update/test-signing.pub");
        let c = Config::new("https://krunk.example", public_key_from_pem(pem).unwrap(), false).unwrap();
        let body = include_bytes!("../tests/fixtures/update/android-latest.json");
        let sig = include_bytes!("../tests/fixtures/update/android-latest.json.sig");
        let m = verify(body, sig, &c, PKG).unwrap();
        assert_eq!(m.version_code, 7);
        let mut tampered = body.to_vec();
        tampered[30] ^= 1;
        assert_eq!(verify(&tampered, sig, &c, PKG), Err(UpdateError::BadSignature));
    }
}
