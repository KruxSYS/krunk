//! Locations of the browser's data under `%LOCALAPPDATA%\LightBrowser`.

use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "LightBrowser";

#[derive(Debug, Clone)]
pub struct DataDirs {
    root: PathBuf,
}

impl DataDirs {
    /// Resolves (and creates) the data directory.
    pub fn resolve() -> Result<DataDirs, String> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
            .ok_or("Cannot find the local application data folder (LOCALAPPDATA is not set)")?;
        let dirs = DataDirs { root: base.join(APP_DIR) };
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

    /// The encrypted password vault.
    pub fn vault(&self) -> PathBuf {
        self.root.join("vault.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.root.join("lightbrowser.log")
    }
}
