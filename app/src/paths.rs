//! Locations of the browser's data under `%LOCALAPPDATA%\Krunk`.

use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "Krunk";

#[derive(Debug, Clone)]
pub struct DataDirs {
    root: PathBuf,
}

/// `%LOCALAPPDATA%` (or its usual place under the user profile).
pub fn local_app_data() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
        .ok_or_else(|| "Cannot find the local application data folder (LOCALAPPDATA is not set)".to_string())
}

/// `%APPDATA%` (roaming; Firefox keeps its profiles there).
pub fn roaming_app_data() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Roaming")))
        .ok_or_else(|| "Cannot find the roaming application data folder (APPDATA is not set)".to_string())
}

impl DataDirs {
    /// Resolves (and creates) the data directory.
    pub fn resolve() -> Result<DataDirs, String> {
        let dirs = DataDirs { root: local_app_data()?.join(APP_DIR) };
        for d in [dirs.root.clone(), dirs.profile()] {
            std::fs::create_dir_all(&d).map_err(|e| format!("Cannot create {}: {e}", d.display()))?;
        }
        Ok(dirs)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The engine's profile root (CEF `root_cache_path`); the profile itself
    /// (cookies, cache, site data) is `profile\default`. A `profile\EBWebView`
    /// folder left by the WebView2-based versions is not used or touched.
    pub fn profile(&self) -> PathBuf {
        self.root.join("profile")
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("browser.db")
    }

    /// Downloaded filter lists and their update state.
    pub fn filters(&self) -> PathBuf {
        self.root.join("filters")
    }

    /// The compiled, serialized ad-blocking engine.
    pub fn engine_cache(&self) -> PathBuf {
        self.root.join("engine.dat")
    }

    /// The serialized scriptlet engine (same lists as `engine.dat`).
    pub fn scriptlet_cache(&self) -> PathBuf {
        self.root.join("scriptlets.dat")
    }

    /// The encrypted password vault.
    pub fn vault(&self) -> PathBuf {
        self.root.join("vault.json")
    }

    /// The last update check (`core::update::Status`).
    pub fn update_status(&self) -> PathBuf {
        self.root.join("update.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.root.join("krunk.log")
    }
}
