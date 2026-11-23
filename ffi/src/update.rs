//! The Android update check (Krunk for Android 0.5.0, `direct` flavour only): which addresses to
//! fetch, whether the signed manifest is trustworthy, which APK fits this device, and whether a
//! download matches. All rules are in `core::update_android`; the app does the two requests
//! (only when the user asks) and the installation.
//!
//! The release key's public half is compiled in from `assets/keys/update-signing.pub` (the same
//! key that signs the Windows manifest). Only `update_verify` uses it; `update_verify_with_key`
//! exists for the app's unit tests, which cannot sign with the release key.
use lb_core::update::{public_key_from_pem, to_hex, Config, UpdateError};
use lb_core::update_android as ua;
use std::fmt;

const RELEASE_KEY_PEM: &str = include_str!("../../assets/keys/update-signing.pub");

/// Why a check or a download was refused. Each has its own message in the app.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum UpdateFailure {
    /// The built-in update address is unusable (a build mistake).
    BadAddress {
        detail: String,
    },
    /// The signature does not belong to the manifest and Krunk's release key.
    BadSignature,
    /// Signed, but not a usable Android manifest for this app.
    Malformed {
        detail: String,
    },
    /// A file is not on the update server.
    NotOnServer {
        detail: String,
    },
    TooLarge {
        detail: String,
    },
    WrongSize {
        expected: u64,
        actual: u64,
    },
    WrongHash,
}

impl fmt::Display for UpdateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateFailure::BadAddress { detail } => write!(f, "bad update address: {detail}"),
            UpdateFailure::BadSignature => write!(f, "the update information is not signed by Krunk"),
            UpdateFailure::Malformed { detail } => write!(f, "invalid update information: {detail}"),
            UpdateFailure::NotOnServer { detail } => write!(f, "file not on the update server: {detail}"),
            UpdateFailure::TooLarge { detail } => write!(f, "too large: {detail}"),
            UpdateFailure::WrongSize { expected, actual } => {
                write!(f, "the download has {actual} bytes, expected {expected}")
            }
            UpdateFailure::WrongHash => write!(f, "the download's SHA-256 does not match"),
        }
    }
}

impl std::error::Error for UpdateFailure {}

impl From<UpdateError> for UpdateFailure {
    fn from(e: UpdateError) -> Self {
        match e {
            UpdateError::BadBase(d) => UpdateFailure::BadAddress { detail: d },
            UpdateError::Insecure(d) => UpdateFailure::NotOnServer { detail: format!("not HTTPS: {d}") },
            UpdateError::TooLarge(what) => UpdateFailure::TooLarge { detail: what.to_string() },
            UpdateError::BadSignatureEncoding | UpdateError::BadSignature => UpdateFailure::BadSignature,
            UpdateError::BadPublicKey => UpdateFailure::BadAddress { detail: "bad public key".into() },
            UpdateError::Malformed(d) => UpdateFailure::Malformed { detail: d },
            UpdateError::CrossOrigin(d) => UpdateFailure::NotOnServer { detail: d },
            UpdateError::BadVersion(v) => UpdateFailure::Malformed { detail: format!("bad version {v}") },
            UpdateError::WrongSize { expected, actual } => UpdateFailure::WrongSize { expected, actual },
            UpdateError::WrongHash => UpdateFailure::WrongHash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UpdateUrls {
    pub manifest: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UpdateFile {
    pub abi: String,
    pub url: String,
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
}

/// A verified manifest.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AndroidUpdate {
    pub version: String,
    pub version_code: u64,
    pub released: String,
    pub min_sdk: u32,
    /// Lower-case hex SHA-256 of the signing certificate the APKs must carry.
    pub signing_cert_sha256: String,
    pub notes: String,
    pub notes_url: Option<String>,
    pub files: Vec<UpdateFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UpdateOffer {
    UpToDate,
    Install,
    NeedsNewerAndroid,
}

fn config(base: &str, allow_http: bool, pem: &str) -> Result<Config, UpdateFailure> {
    let key = public_key_from_pem(pem)?;
    Ok(Config::new(base, key, allow_http)?)
}

fn file_of(f: &ua::ApkFile) -> UpdateFile {
    UpdateFile { abi: f.abi.clone(), url: f.url.to_string(), size: f.size, sha256: to_hex(&f.sha256) }
}

fn record(m: &ua::AndroidManifest) -> AndroidUpdate {
    AndroidUpdate {
        version: m.version.to_string(),
        version_code: m.version_code,
        released: m.released.clone(),
        min_sdk: m.min_sdk,
        signing_cert_sha256: to_hex(&m.signing_cert_sha256),
        notes: m.notes.clone(),
        notes_url: m.notes_url.as_ref().map(|u| u.to_string()),
        files: m.files.iter().map(file_of).collect(),
    }
}

/// The two addresses a check fetches. `allow_http` only in QA and debug builds.
#[uniffi::export]
pub fn update_urls(base: String, allow_http: bool) -> Result<UpdateUrls, UpdateFailure> {
    let cfg = config(&base, allow_http, RELEASE_KEY_PEM)?;
    Ok(UpdateUrls { manifest: ua::manifest_url(&cfg).to_string(), signature: ua::signature_url(&cfg).to_string() })
}

/// Checks the manifest's signature with Krunk's release key, then its contents.
#[uniffi::export]
pub fn update_verify(
    base: String,
    allow_http: bool,
    package: String,
    manifest: Vec<u8>,
    signature: Vec<u8>,
) -> Result<AndroidUpdate, UpdateFailure> {
    update_verify_with_key(RELEASE_KEY_PEM.to_string(), base, allow_http, package, manifest, signature)
}

/// The same with another public key (PEM): for unit tests only; the app uses `update_verify`.
#[uniffi::export]
pub fn update_verify_with_key(
    public_key_pem: String,
    base: String,
    allow_http: bool,
    package: String,
    manifest: Vec<u8>,
    signature: Vec<u8>,
) -> Result<AndroidUpdate, UpdateFailure> {
    let cfg = config(&base, allow_http, &public_key_pem)?;
    let m = ua::verify(&manifest, &signature, &cfg, &package)?;
    Ok(record(&m))
}

/// Compares by versionCode: equal or lower is "up to date" (no downgrades).
#[uniffi::export]
pub fn update_offer(update: &AndroidUpdate, installed_version_code: u64, device_sdk: u32) -> UpdateOffer {
    if update.version_code <= installed_version_code {
        UpdateOffer::UpToDate
    } else if device_sdk < update.min_sdk {
        UpdateOffer::NeedsNewerAndroid
    } else {
        UpdateOffer::Install
    }
}

/// The file for this device (`Build.SUPPORTED_ABIS`, most preferred first).
#[uniffi::export]
pub fn update_pick_file(update: &AndroidUpdate, supported_abis: Vec<String>) -> Option<UpdateFile> {
    supported_abis
        .iter()
        .filter(|abi| abi.as_str() != ua::UNIVERSAL)
        .find_map(|abi| update.files.iter().find(|f| &f.abi == abi))
        .or_else(|| update.files.iter().find(|f| f.abi == ua::UNIVERSAL))
        .cloned()
}

/// Does a finished download (its size and lower-case hex SHA-256) match the file?
#[uniffi::export]
pub fn update_check_download(file: &UpdateFile, size: u64, sha256_hex: String) -> Result<(), UpdateFailure> {
    if size != file.size {
        return Err(UpdateFailure::WrongSize { expected: file.size, actual: size });
    }
    if !sha256_hex.eq_ignore_ascii_case(&file.sha256) {
        return Err(UpdateFailure::WrongHash);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PEM: &str = include_str!("../../core/tests/fixtures/update/test-signing.pub");
    const BODY: &[u8] = include_bytes!("../../core/tests/fixtures/update/android-latest.json");
    const SIG: &[u8] = include_bytes!("../../core/tests/fixtures/update/android-latest.json.sig");
    const PKG: &str = "org.krunk.browser";

    fn verify_test(body: &[u8], sig: &[u8]) -> Result<AndroidUpdate, UpdateFailure> {
        update_verify_with_key(
            TEST_PEM.into(),
            "https://krunk.example".into(),
            false,
            PKG.into(),
            body.into(),
            sig.into(),
        )
    }

    #[test]
    fn urls() {
        let u = update_urls("https://krunk.kruxsys.com".into(), false).unwrap();
        assert_eq!(u.manifest, "https://krunk.kruxsys.com/updates/android-arm64/latest.json");
        assert_eq!(u.signature, "https://krunk.kruxsys.com/updates/android-arm64/latest.json.sig");
        assert!(matches!(update_urls("http://10.0.2.2:18090".into(), false), Err(UpdateFailure::NotOnServer { .. })));
        assert!(update_urls("http://10.0.2.2:18090".into(), true).is_ok());
    }

    #[test]
    fn good_and_bad_signatures() {
        let u = verify_test(BODY, SIG).unwrap();
        assert_eq!(u.version, "0.5.0");
        assert_eq!(u.version_code, 7);
        assert_eq!(u.files.len(), 1);
        assert_eq!(
            u.files[0].url,
            "https://krunk.example/download/android/0.5.0/Krunk-android-0.5.0-arm64-v8a-release.apk"
        );
        let mut tampered = BODY.to_vec();
        tampered[10] ^= 1;
        assert_eq!(verify_test(&tampered, SIG), Err(UpdateFailure::BadSignature));
        assert_eq!(verify_test(BODY, b"garbage"), Err(UpdateFailure::BadSignature));
    }

    #[test]
    fn the_release_key_does_not_accept_test_signatures() {
        // The fixture is signed with the test key: Krunk's own key must refuse it.
        let r = update_verify("https://krunk.example".into(), false, PKG.into(), BODY.into(), SIG.into());
        assert_eq!(r, Err(UpdateFailure::BadSignature));
    }

    #[test]
    fn offers_files_and_downloads() {
        let u = verify_test(BODY, SIG).unwrap();
        assert_eq!(update_offer(&u, 5, 36), UpdateOffer::Install);
        assert_eq!(update_offer(&u, 7, 36), UpdateOffer::UpToDate);
        assert_eq!(update_offer(&u, 9, 36), UpdateOffer::UpToDate);
        assert_eq!(update_offer(&u, 5, 25), UpdateOffer::NeedsNewerAndroid);
        let f = update_pick_file(&u, vec!["arm64-v8a".into()]).unwrap();
        assert_eq!(f.abi, "arm64-v8a");
        assert_eq!(update_pick_file(&u, vec!["x86_64".into()]), None, "no x86_64 and no universal file");
        let sha = f.sha256.clone();
        assert_eq!(update_check_download(&f, 3, sha.to_uppercase()), Ok(()));
        assert_eq!(update_check_download(&f, 3, "00".repeat(32)), Err(UpdateFailure::WrongHash));
        assert_eq!(update_check_download(&f, 4, sha), Err(UpdateFailure::WrongSize { expected: 3, actual: 4 }));
    }
}
