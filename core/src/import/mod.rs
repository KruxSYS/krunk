//! Import from other browsers: Firefox (`places.sqlite`, `key4.db` + `logins.json`), the
//! Chromium family (`History`, `Bookmarks`, `Login Data`) and password CSV files; payment
//! cards from Firefox (`autofill-profiles.json`) and the Chromium family (`Web Data`).
//!
//! Everything here is pure parsing and merging on files the host has already copied out of
//! the other browser's profile (they are locked while it runs). Platform parts stay in the
//! host: DPAPI (`Unprotect`), the profile roots and the temporary copies. No password is ever
//! logged or printed; the login type redacts itself in `Debug`.

pub mod cards;
pub mod chromium;
pub mod csv;
pub mod firefox;
pub mod merge;
pub mod nss;
pub mod profiles;

use std::fmt;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use zeroize::Zeroizing;

/// A login read from another browser or a CSV file.
#[derive(Clone)]
pub struct Login {
    /// The site (any URL; the vault keeps its origin).
    pub url: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub note: Zeroizing<String>,
    /// Creation time in ms since the epoch, when the source has one.
    pub created_ms: Option<i64>,
}

impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Login").field("url", &self.url).field("username", &self.username).finish_non_exhaustive()
    }
}

/// One visit of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub ts_ms: i64,
}

/// Decrypts a blob the operating system protected for this user (DPAPI on Windows). The
/// host provides it; tests use a fake.
pub trait Unprotect {
    fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// A file the import needs is missing.
    NotFound(String),
    /// The database or file could not be read.
    Read(String),
    /// The content is not in the expected format.
    Format(String),
    /// The Firefox primary password is wrong.
    WrongPassword,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NotFound(what) => write!(f, "{what} was not found"),
            ImportError::Read(what) => write!(f, "cannot read {what}"),
            ImportError::Format(what) => write!(f, "{what}"),
            ImportError::WrongPassword => write!(f, "wrong primary password"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Opens a copied SQLite database. Without a write-ahead log next to it the file is opened
/// read-only and immutable (nothing touches the journal); with one (the browser was running
/// when the copy was taken) the copy is opened normally so SQLite applies the log to it. The
/// originals are never opened.
pub fn open_readonly(path: &Path) -> Result<Connection, ImportError> {
    if !path.is_file() {
        return Err(ImportError::NotFound(path.display().to_string()));
    }
    let wal = path.with_file_name(format!("{}-wal", path.file_name().map(|f| f.to_string_lossy()).unwrap_or_default()));
    let read = |e: rusqlite::Error| ImportError::Read(format!("{}: {e}", path.display()));
    if wal.is_file() {
        return Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(read);
    }
    let uri = format!("file:{}?immutable=1", path.to_string_lossy().replace('?', "%3F").replace('#', "%23"));
    Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).map_err(read)
}

/// WebKit time (microseconds since 1601-01-01) to milliseconds since the Unix epoch; 0 and
/// nonsense stay 0.
pub fn webkit_to_ms(us: i64) -> i64 {
    const EPOCH_DIFF_US: i64 = 11_644_473_600_000_000;
    if us <= EPOCH_DIFF_US {
        return 0;
    }
    (us - EPOCH_DIFF_US) / 1000
}

/// Microseconds since the Unix epoch (Firefox) to milliseconds.
pub fn us_to_ms(us: i64) -> i64 {
    if us <= 0 {
        0
    } else {
        us / 1000
    }
}

/// Only http(s) pages are imported into history and bookmarks.
pub(crate) fn is_web_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::path::PathBuf;

    /// The fixtures made by `core/tests/fixtures/import/make.py`.
    pub(crate) fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import").join(rel)
    }

    /// The generator's fake DPAPI: a blob is its prefix and the plaintext.
    pub(crate) struct FakeDpapi;
    impl Unprotect for FakeDpapi {
        fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            blob.strip_prefix(b"FAKEDPAPI")
                .map(|p| Zeroizing::new(p.to_vec()))
                .ok_or_else(|| "not a DPAPI blob".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_conversions() {
        assert_eq!(webkit_to_ms(0), 0);
        assert_eq!(webkit_to_ms(11_644_473_600_000_000 + 1_700_000_000_000_000), 1_700_000_000_000);
        assert_eq!(us_to_ms(1_700_000_000_123_456), 1_700_000_000_123);
        assert_eq!(us_to_ms(-5), 0);
    }

    #[test]
    fn login_debug_hides_the_password() {
        let l = Login {
            url: "https://a.example/".into(),
            username: "u".into(),
            password: Zeroizing::new("hunter2".into()),
            note: Zeroizing::new(String::new()),
            created_ms: None,
        };
        let d = format!("{l:?}");
        assert!(d.contains("a.example") && !d.contains("hunter2"), "{d}");
    }

    #[test]
    fn only_web_urls() {
        assert!(is_web_url("https://a.example/x"));
        assert!(!is_web_url("chrome://settings/"));
        assert!(!is_web_url("place:sort=8"));
        assert!(!is_web_url("javascript:void(0)"));
        assert!(!is_web_url("file:///C:/x.html"));
    }

    #[test]
    fn missing_database_is_not_found() {
        assert!(matches!(open_readonly(Path::new("/nonexistent/History")), Err(ImportError::NotFound(_))));
    }
}
