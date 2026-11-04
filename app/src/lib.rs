//! Krunk.dll: the browser.
//!
//! `Krunk.exe` is CEF's sandbox bootstrap (`bootstrap.exe`, renamed).
//! It loads this DLL and calls `RunWinMain` (`ffi::cef::entry`) in the
//! browser process and in every engine subprocess. The browser only exists
//! on Windows; elsewhere the crate is empty so the workspace still builds.
// The cdylib must be called Krunk.dll (the bootstrap loads
// `<exe name>.dll`).
#![allow(non_snake_case)]
#![deny(unsafe_code)]
#![cfg_attr(all(windows, not(test)), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(windows)]
mod browser;
#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod paths;

/// The browser process, after the engine's subprocess check: data folder,
/// settings, then the engine and the browser window. Returns the exit code.
#[cfg(windows)]
fn browser_main() -> i32 {
    // A panic aborts the process (release builds): leave its message in the log.
    std::panic::set_hook(Box::new(|info| log::write("PANIC", &info.to_string())));
    match run() {
        Ok(code) => code,
        Err(e) => {
            log::write("ERROR", &e);
            ffi::dialogs::error("Krunk", &e);
            1
        }
    }
}

#[cfg(windows)]
fn run() -> Result<i32, String> {
    use log::info;

    // After "Restart now": wait until the previous instance has exited and
    // released the profile.
    if let Some(pid) = std::env::args().find_map(|a| a.strip_prefix("--wait-for-pid=").and_then(|p| p.parse().ok())) {
        ffi::process::wait_for_process(pid, 15_000);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    ffi::process::init_ui_thread()?;
    let migration = migrate_legacy_data()?;
    let dirs = paths::DataDirs::resolve()?;
    log::init(&dirs.log_file());
    info!("Krunk {} starting, data dir {}", env!("CARGO_PKG_VERSION"), dirs.root().display());
    if let Some(m) = migration {
        info!("{m}");
    }

    let store =
        lb_core::storage::Store::open(&dirs.database()).map_err(|e| format!("Cannot open the database: {e}"))?;
    let settings = store.load_settings().map_err(|e| format!("Cannot read settings: {e}"))?;

    // Diagnostics: refresh the filter lists without opening a window.
    if std::env::args().any(|a| a == "--update-filters") {
        return Ok(update_filters_headless(&dirs, &settings));
    }

    // The engine's segmentation platform keeps its own database of visited
    // URLs (ukm_db), outside history and untouched by clearing it. Its
    // switches crash this engine build, so it is removed at every start.
    let _ = std::fs::remove_dir_all(dirs.profile().join("segmentation_platform"));

    let args: Vec<String> = std::env::args().collect();
    let power = browser::PowerOptions::from_args(&args);
    if args.iter().any(|a| a == "--log-blocked") {
        browser::LOG_BLOCKED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let config = ffi::cef::EngineConfig {
        root_cache: dirs.profile(),
        cache: dirs.profile().join("default"),
        log_file: dirs.root().join("cef.log"),
        switches: settings.browser_switches(),
        user_agent: settings.user_agent.clone(),
    };
    info!("browser arguments: {}", settings.browser_arguments());
    let window = ffi::window::Window::create_main("Krunk")?;
    let start: ffi::cef::Start =
        Box::new(move || match browser::Browser::start(window, &dirs, store, settings, power) {
            Ok(b) => Some(Box::new(b) as Box<dyn std::any::Any>),
            Err(e) => {
                log::write("ERROR", &e);
                ffi::dialogs::error("Krunk", &e);
                None
            }
        });
    ffi::cef::run_engine(config, start)
}

/// Moves `%LOCALAPPDATA%\LightBrowser` (the browser's old name) to the
/// Krunk folder once, before anything opens it. Returns the line to log.
#[cfg(windows)]
fn migrate_legacy_data() -> Result<Option<String>, String> {
    use lb_core::migrate::{self, Moved, Plan};
    let base = paths::local_app_data()?;
    let (old, new) = (base.join(migrate::LEGACY_DIR), base.join(paths::APP_DIR));
    // The old browser's main window class: it is still running on that folder.
    let running = ffi::window::class_exists("LightBrowserWindow");
    match migrate::plan(old.is_dir(), new.exists(), running) {
        Plan::Nothing if old.is_dir() => Ok(Some(format!(
            "data migration: both {} and {} exist; using the Krunk folder",
            old.display(),
            new.display()
        ))),
        Plan::Nothing => Ok(None),
        Plan::WaitForLegacy => Err(format!(
            "LightBrowser is still running.\n\nClose LightBrowser, then start Krunk again: Krunk moves your \
             LightBrowser profile ({}) to {} the first time it starts.",
            old.display(),
            new.display()
        )),
        Plan::Move => match migrate::move_dir(&old, &new, |a, b| std::fs::rename(a, b)) {
            Ok(Moved::Renamed) => Ok(Some(format!("data migration: renamed {} to {}", old.display(), new.display()))),
            Ok(Moved::Copied { files }) => Ok(Some(format!(
                "data migration: copied {files} files from {} to {} and removed the old folder",
                old.display(),
                new.display()
            ))),
            Ok(Moved::CopiedOldKept { files }) => Ok(Some(format!(
                "data migration: copied {files} files from {} to {}; the old folder could not be removed completely",
                old.display(),
                new.display()
            ))),
            Err(e) => {
                Err(format!("Cannot move your LightBrowser profile ({}) to {}: {e}", old.display(), new.display()))
            }
        },
    }
}

/// `Krunk.exe --update-filters`: downloads the filter lists, rebuilds
/// engine.dat and writes the result to the log. Exit code 0 on success.
#[cfg(windows)]
fn update_filters_headless(dirs: &paths::DataDirs, settings: &lb_core::settings::Settings) -> i32 {
    use lb_core::filterlists::{self, UpdateState, MAX_LIST_BYTES};
    use lb_core::settings::{updater_proxy, UpdaterProxy};
    let dir = dirs.filters();
    let _ = std::fs::create_dir_all(&dir);
    let state = std::fs::read_to_string(dir.join("state.json")).map(|j| UpdateState::from_json(&j)).unwrap_or_default();
    let mode = match updater_proxy(&settings.proxy) {
        UpdaterProxy::Skip => {
            log::write("WARN", "filter update skipped: SOCKS proxy configured");
            return 3;
        }
        UpdaterProxy::Named(p) => ffi::http::ProxyMode::Named(p),
        UpdaterProxy::System => ffi::http::ProxyMode::System,
    };
    let fetch = |url: &str| ffi::http::get(url, MAX_LIST_BYTES, &mode, 30_000);
    let t = std::time::Instant::now();
    let r = filterlists::rebuild(&dir, &settings.filter_lists, state, Some(&fetch), None, lb_core::storage::now_ms());
    if let Some(cache) = &r.cache {
        let _ = lb_core::vault::write_atomic(&dirs.engine_cache(), cache);
    }
    if let Some(scriptlets) = &r.scriptlets {
        let _ = lb_core::vault::write_atomic(&dirs.scriptlet_cache(), scriptlets);
    }
    let _ = lb_core::vault::write_atomic(&dir.join("state.json"), r.state.to_json().as_bytes());
    let summary: Vec<String> = r.state.downloaded.iter().map(|(id, d)| format!("{id}={}B", d.bytes)).collect();
    log::write(
        "INFO",
        &format!(
            "filter update: downloaded [{}], error: {}, engine key {:016x}, {} ms",
            summary.join(", "),
            r.state.last_error.as_deref().unwrap_or("none"),
            r.key,
            t.elapsed().as_millis()
        ),
    );
    // Reload the caches exactly as startup does and check a few decisions.
    use lb_core::blocker::{Blocker, ResourceKind, ScriptletEngine};
    let key = filterlists::cache_key(&settings.filter_lists, &r.state);
    let t = std::time::Instant::now();
    let loaded = std::fs::read(dirs.engine_cache()).ok().and_then(|d| Blocker::deserialize(&d, key).ok());
    let Some(engine) = loaded else {
        log::write("ERROR", "filter self-test: engine.dat could not be loaded with the expected key");
        return 2;
    };
    let src = "https://news.example.com/";
    // Blocked or answered with uBO's stand-in (`$redirect`): either way the
    // real request does not go out.
    let checks = [
        ("https://securepubads.g.doubleclick.net/tag/js/gpt.js", ResourceKind::Script, true),
        ("https://www.google-analytics.com/analytics.js", ResourceKind::Script, true),
        ("https://example.com/app.js", ResourceKind::Script, false),
    ];
    let decisions_ok = checks.iter().all(|(u, k, blocked)| engine.check(u, src, *k).is_blocked() == *blocked);
    let loaded_in = t.elapsed().as_micros();
    // Scriptlets: the cache loads and YouTube gets uBO's ad defences.
    let t = std::time::Instant::now();
    let scriptlets = std::fs::read(dirs.scriptlet_cache()).ok().and_then(|d| ScriptletEngine::deserialize(&d).ok());
    let youtube = scriptlets.as_ref().and_then(|s| s.script_for("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
    let scriptlets_ok = youtube.as_ref().is_some_and(|js| js.contains("function "));
    log::write(
        "INFO",
        &format!(
            "filter self-test: cache loaded in {loaded_in} us, decisions {}; scriptlet engine loaded in {} us, \
             YouTube script {} bytes ({})",
            if decisions_ok { "OK" } else { "WRONG" },
            t.elapsed().as_micros(),
            youtube.as_ref().map_or(0, String::len),
            if scriptlets_ok { "OK" } else { "MISSING" }
        ),
    );
    let ok = decisions_ok && scriptlets_ok;
    match (ok, r.state.last_error.is_some()) {
        (false, _) => 2,
        (true, true) => 1,
        (true, false) => 0,
    }
}
