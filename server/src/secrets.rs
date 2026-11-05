//! Self-provisioned server secrets (docs/sync/OPEN_QUESTIONS.md Q4).
//!
//! `krunk-sync init-secrets --dir /secrets --hold` is the compose `secrets` service, which starts
//! before the database and then stays idle (a one-shot would loop under the
//! `restart: unless-stopped` that ai-pipe applies to every service). It creates every missing
//! secret as a file of 32 random bytes in
//! base64url, written atomically, and never touches an existing one. The files live in the
//! named volume `krunk-secrets`, which only `db`, `api` and `backup` mount, read-only. So the
//! stack needs no `.env` for its own secrets, and they never appear in git, images or logs.
//!
//! The files are mode 0444: the postgres image reads its password file as uid 70, while this
//! service runs as 65532. The directory is reachable only through that volume.

use base64ct::{Base64UrlUnpadded, Encoding};
use std::fs;
use std::io::Write;
use std::path::Path;

/// The secrets the server needs; their file names in the secrets directory.
pub const NAMES: &[&str] = &["postgres_password", "token_hmac_key", "password_pepper", "totp_enc_key"];

/// What happened to each secret.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Created,
    Kept,
}

pub fn init(dir: &Path) -> Result<Vec<(&'static str, Action)>, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let mut out = Vec::new();
    for name in NAMES {
        let path = dir.join(name);
        if fs::read_to_string(&path).map(|s| !s.trim().is_empty()).unwrap_or(false) {
            out.push((*name, Action::Kept));
            continue;
        }
        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).map_err(|e| format!("random: {e}"))?;
        let value = Base64UrlUnpadded::encode_string(&raw);
        let tmp = dir.join(format!(".{name}.tmp"));
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
            f.write_all(value.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
            f.sync_all().map_err(|e| format!("{}: {e}", tmp.display()))?;
        }
        set_mode(&tmp)?;
        fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        out.push((*name, Action::Created));
    }
    Ok(out)
}

#[cfg(unix)]
fn set_mode(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o444)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_: &Path) -> Result<(), String> {
    Ok(())
}

/// Reads one secret file (trimmed); `None` if it is missing or empty.
pub fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("krunk-secrets-test-{}-{}", std::process::id(), rand_suffix()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_suffix() -> u64 {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        u64::from_le_bytes(b)
    }

    #[test]
    fn creates_once_then_keeps() {
        let d = tempdir();
        let first = init(&d).unwrap();
        assert!(first.iter().all(|(_, a)| *a == Action::Created));
        let values: Vec<String> = NAMES.iter().map(|n| read(&d.join(n)).unwrap()).collect();
        for v in &values {
            assert_eq!(v.len(), 43, "32 bytes in base64url");
            assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "URL-safe");
        }
        assert_eq!(values.iter().collect::<std::collections::HashSet<_>>().len(), NAMES.len(), "all different");
        // A second run keeps every value.
        assert!(init(&d).unwrap().iter().all(|(_, a)| *a == Action::Kept));
        let again: Vec<String> = NAMES.iter().map(|n| read(&d.join(n)).unwrap()).collect();
        assert_eq!(values, again);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(d.join("postgres_password")).unwrap().permissions().mode() & 0o777, 0o444);
        }
        // An empty file counts as missing and is replaced.
        fs::remove_file(d.join("totp_enc_key")).unwrap();
        fs::write(d.join("totp_enc_key"), "").unwrap();
        let third = init(&d).unwrap();
        assert!(third.contains(&("totp_enc_key", Action::Created)));
        assert!(third.contains(&("postgres_password", Action::Kept)));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refuses_a_missing_directory() {
        assert!(init(Path::new("/nonexistent/krunk")).is_err());
    }
}
