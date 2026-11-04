//! One-time move of the data folder from the browser's old name
//! (`%LOCALAPPDATA%\LightBrowser`) to `%LOCALAPPDATA%\Krunk`.
//!
//! Runs before anything opens the data folder. The whole folder moves: the
//! engine profile (cookies, site data, `Local State` with the cookie key),
//! `browser.db`, `vault.json`, `engine.dat` and the filter lists. A rename
//! is tried first (same volume, atomic); if it fails the folder is copied to
//! a temporary sibling, the copy is renamed into place, and only then is
//! the old folder deleted, so an interrupted copy never leaves a half
//! profile under the new name.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Folder name used before the rename.
pub const LEGACY_DIR: &str = "LightBrowser";

/// What to do at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Nothing to migrate (no old folder, or the new one already exists).
    Nothing,
    /// Move the old folder to the new name.
    Move,
    /// The old browser is still running with that folder: ask the user to
    /// close it instead of copying a profile that is in use.
    WaitForLegacy,
}

/// Decides the migration. An existing new folder always wins: the old one is
/// then left alone (and logged), never merged.
pub fn plan(legacy_exists: bool, new_exists: bool, legacy_running: bool) -> Plan {
    match (legacy_exists, new_exists) {
        (true, false) if legacy_running => Plan::WaitForLegacy,
        (true, false) => Plan::Move,
        _ => Plan::Nothing,
    }
}

/// How the folder was moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    Renamed,
    /// Copied (with this many files) and the old folder removed.
    Copied {
        files: u64,
    },
    /// Copied, but the old folder could not be removed completely.
    CopiedOldKept {
        files: u64,
    },
}

/// Moves `from` to `to` (which must not exist). `rename` is `fs::rename` in
/// the app; tests pass a failing one to exercise the copy path.
pub fn move_dir(from: &Path, to: &Path, rename: impl Fn(&Path, &Path) -> io::Result<()>) -> io::Result<Moved> {
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    if rename(from, to).is_ok() {
        return Ok(Moved::Renamed);
    }
    let tmp = staging_path(to);
    let _ = fs::remove_dir_all(&tmp);
    let files = match copy_tree(from, &tmp) {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    if let Err(e) = rename(&tmp, to).or_else(|_| fs::rename(&tmp, to)) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    Ok(match fs::remove_dir_all(from) {
        Ok(()) => Moved::Copied { files },
        Err(_) => Moved::CopiedOldKept { files },
    })
}

fn staging_path(to: &Path) -> PathBuf {
    let mut name = to.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".migrating");
    to.with_file_name(name)
}

/// Copies a directory tree; returns the number of files. Symbolic links
/// are not followed (a profile has none).
fn copy_tree(from: &Path, to: &Path) -> io::Result<u64> {
    fs::create_dir_all(to)?;
    let mut files = 0;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            files += copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
            files += 1;
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("krunk-migrate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn seed(root: &Path) {
        fs::create_dir_all(root.join("profile/default/Network")).unwrap();
        fs::write(root.join("profile/Local State"), b"{\"os_crypt\":{}}").unwrap();
        fs::write(root.join("profile/default/Network/Cookies"), b"cookies").unwrap();
        fs::write(root.join("browser.db"), b"db").unwrap();
        fs::write(root.join("vault.json"), b"vault").unwrap();
        fs::write(root.join("engine.dat"), b"engine").unwrap();
    }

    fn assert_profile(root: &Path) {
        assert_eq!(fs::read(root.join("profile/default/Network/Cookies")).unwrap(), b"cookies");
        assert_eq!(fs::read(root.join("profile/Local State")).unwrap(), b"{\"os_crypt\":{}}");
        for (f, c) in [("browser.db", "db"), ("vault.json", "vault"), ("engine.dat", "engine")] {
            assert_eq!(fs::read(root.join(f)).unwrap(), c.as_bytes());
        }
    }

    #[test]
    fn decisions() {
        assert_eq!(plan(true, false, false), Plan::Move);
        assert_eq!(plan(true, false, true), Plan::WaitForLegacy);
        assert_eq!(plan(true, true, false), Plan::Nothing, "never merge into an existing Krunk folder");
        assert_eq!(plan(true, true, true), Plan::Nothing);
        assert_eq!(plan(false, false, false), Plan::Nothing);
        assert_eq!(plan(false, true, false), Plan::Nothing);
        assert_eq!(plan(false, false, true), Plan::Nothing);
    }

    #[test]
    fn rename_moves_everything() {
        let base = temp("rename");
        let (old, new) = (base.join(LEGACY_DIR), base.join("Krunk"));
        seed(&old);
        assert_eq!(move_dir(&old, &new, |a, b| fs::rename(a, b)).unwrap(), Moved::Renamed);
        assert!(!old.exists());
        assert_profile(&new);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn copy_then_delete_when_rename_fails() {
        let base = temp("copy");
        let (old, new) = (base.join(LEGACY_DIR), base.join("Krunk"));
        seed(&old);
        // Only the first rename (old → new) fails, as across volumes.
        let calls = std::cell::Cell::new(0);
        let rename = |a: &Path, b: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(io::Error::other("cross-device"))
            } else {
                fs::rename(a, b)
            }
        };
        assert_eq!(move_dir(&old, &new, rename).unwrap(), Moved::Copied { files: 5 });
        assert!(!old.exists(), "old folder removed after a complete copy");
        assert!(!base.join("Krunk.migrating").exists());
        assert_profile(&new);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn existing_target_is_never_overwritten() {
        let base = temp("exists");
        let (old, new) = (base.join(LEGACY_DIR), base.join("Krunk"));
        seed(&old);
        fs::create_dir_all(&new).unwrap();
        assert!(move_dir(&old, &new, |a, b| fs::rename(a, b)).is_err());
        assert_profile(&old);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn failed_copy_leaves_old_folder_and_no_new_one() {
        let base = temp("fail");
        let (old, new) = (base.join(LEGACY_DIR), base.join("Krunk"));
        // The source does not exist: rename and copy both fail.
        assert!(move_dir(&old, &new, |_, _| Err(io::Error::other("no"))).is_err());
        assert!(!new.exists());
        assert!(!base.join("Krunk.migrating").exists());
        let _ = fs::remove_dir_all(base);
    }
}
