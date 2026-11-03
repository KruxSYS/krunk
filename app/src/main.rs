//! LightBrowser host executable.
//!
//! The browser itself only exists on Windows (WebView2). On other platforms
//! the binary is a stub so that `cargo build --workspace` works everywhere.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![deny(unsafe_code)]
#![cfg_attr(windows, deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(windows)]
mod browser;
#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod paths;

#[cfg(windows)]
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            log::write("ERROR", &e);
            ffi::dialogs::error("LightBrowser", &e);
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn run() -> Result<i32, String> {
    use log::info;

    ffi::process::init_ui_thread()?;
    let dirs = paths::DataDirs::resolve()?;
    log::init(&dirs.log_file());
    info!("LightBrowser {} starting, data dir {}", env!("CARGO_PKG_VERSION"), dirs.root().display());

    let store =
        lb_core::storage::Store::open(&dirs.database()).map_err(|e| format!("Cannot open the database: {e}"))?;
    let settings = store.load_settings().map_err(|e| format!("Cannot read settings: {e}"))?;

    let loader = ffi::loader::Loader::load(&dirs.bin())?;
    info!("WebView2Loader {} loaded", ffi::loader::LOADER_VERSION);
    let Some(version) = loader.runtime_version() else {
        log::write("WARN", "WebView2 runtime not found");
        ffi::dialogs::runtime_missing("");
        return Ok(2);
    };
    info!("WebView2 runtime version {version}");

    let _browser = browser::Browser::start(&loader, &dirs, settings, version)?;
    Ok(ffi::window::run_message_loop())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LightBrowser runs on Windows only; build with --target x86_64-pc-windows-gnu.");
    std::process::exit(1);
}
