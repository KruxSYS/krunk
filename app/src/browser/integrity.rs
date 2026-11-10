//! Are Krunk's installed files this release's? (Krunk 0.7.0, `lb_core::integrity`)
//!
//! Twenty seconds after start a worker thread hashes the engine files next to Krunk.exe
//! (cached by size and modification time in the profile, so later starts only stat them)
//! against the list built into this copy. When the update check finds the manifest of this
//! very version, its `files.json` (Krunk.exe and Krunk.dll included) is checked the same way.
//! A file that differs, or the old codec-less engine, opens the repair prompt; krunk://gpu
//! shows the result either way.

use super::Browser;
use crate::ffi::http;
use crate::log::{info, warning};
use lb_core::integrity::{self, HashCache, Report};
use lb_core::update::Manifest;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;

const HASHES_KEY: &str = "fileHashes";
const FIRST_CHECK_MS: i64 = 20_000;
const FILES_TIMEOUT_MS: i32 = 30_000;

#[derive(Default)]
pub(super) struct IntegrityHost {
    report: RefCell<Report>,
    /// The release list was fetched (once per run).
    release_started: Cell<bool>,
}

/// The folder Krunk.exe runs from.
fn install_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf()))
}

/// The installer renames files still in use to `<name>.old-krunk` (installer/krunk.nsi); the
/// old engine processes are gone by now, so they can be deleted.
fn remove_moved_aside() {
    let Some(dir) = install_dir() else { return };
    for d in [dir.clone(), dir.join("locales")] {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "old-krunk") {
                match std::fs::remove_file(&p) {
                    Ok(()) => info!("removed {} (moved aside by the installer)", p.display()),
                    Err(err) => warning!("cannot remove {}: {err}", p.display()),
                }
            }
        }
    }
}

impl Browser {
    pub(super) fn init_integrity(&self) {
        remove_moved_aside();
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(FIRST_CHECK_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.check_engine_files();
            }
        });
    }

    fn check_engine_files(&self) {
        let Some(dir) = install_dir() else { return };
        let mut cache =
            HashCache::from_json(&self.store.borrow().kv_get(HASHES_KEY).ok().flatten().unwrap_or_default());
        self.spawn_job(
            "engine-files",
            move || {
                let expected = integrity::engine_files();
                let got = integrity::hash_installed(&dir, expected.keys(), &mut cache);
                let problems = integrity::compare(&expected, &mut |p| got.get(p).copied().flatten());
                let libcef = got.get("libcef.dll").copied().flatten();
                let report = Report {
                    checked: true,
                    flavor: Some(integrity::flavor_of(libcef.as_ref(), &expected)),
                    libcef: libcef.map(|h| integrity::to_hex(&h)[..16].to_string()).unwrap_or_default(),
                    engine_files: expected.len(),
                    problems,
                    ..Report::default()
                };
                (report, cache)
            },
            |b, (report, cache): (Report, HashCache)| {
                let _ = b.store.borrow().kv_set(HASHES_KEY, &cache.to_json());
                let mut r = b.integrity.report.borrow_mut();
                r.checked = report.checked;
                r.flavor = report.flavor;
                r.libcef = report.libcef;
                r.engine_files = report.engine_files;
                r.problems = report.problems;
                let healthy = r.healthy();
                let summary = r.summary();
                drop(r);
                if healthy {
                    info!(
                        "engine files: {} checked, libcef.dll {} (this release's engine)",
                        report.engine_files,
                        b.integrity.report.borrow().libcef
                    );
                } else {
                    warning!("engine files: {summary}");
                    b.offer_repair(summary);
                }
            },
        );
    }

    /// The update server's list for this very version (the manifest just checked out).
    pub(super) fn check_release_files(&self, m: &Manifest) {
        let Some((url, hash)) = m.files.clone() else { return };
        if self.integrity.release_started.replace(true) {
            return;
        }
        let Some(dir) = install_dir() else { return };
        let Some(proxy) = self.own_route() else { return };
        let allow_http = self.updates_allow_http();
        let version = m.version.to_string();
        let mut cache =
            HashCache::from_json(&self.store.borrow().kv_get(HASHES_KEY).ok().flatten().unwrap_or_default());
        self.spawn_job(
            "release-files",
            move || {
                let agent = format!("Krunk-Update/{} ({})", env!("CARGO_PKG_VERSION"), lb_core::update::CHANNEL);
                let opts =
                    http::Options { proxy: &proxy, timeout_ms: FILES_TIMEOUT_MS, user_agent: &agent, allow_http };
                let bytes = http::get_with(url.as_str(), integrity::MAX_FILES_JSON_BYTES, &opts)
                    .map_err(|e| format!("files.json: {e}"))?;
                let mut check = lb_core::update::DownloadCheck::default();
                check.update(&bytes);
                if check.sha256() != hash {
                    return Err("files.json does not match the signed manifest".to_string());
                }
                let expected = integrity::parse_files_json(&bytes, &version)?;
                let got = integrity::hash_installed(&dir, expected.keys(), &mut cache);
                let problems = integrity::compare(&expected, &mut |p| got.get(p).copied().flatten());
                Ok((expected.len(), problems, cache))
            },
            |b, r: Result<(usize, Vec<integrity::Problem>, HashCache), String>| match r {
                Ok((n, problems, cache)) => {
                    let _ = b.store.borrow().kv_set(HASHES_KEY, &cache.to_json());
                    let mut rep = b.integrity.report.borrow_mut();
                    rep.release_checked = true;
                    rep.release_problems = problems;
                    let healthy = rep.healthy();
                    let summary = rep.summary();
                    drop(rep);
                    if healthy {
                        info!("release files: {n} checked against the update server's list, all match");
                    } else {
                        warning!("release files: {summary}");
                        b.offer_repair(summary);
                    }
                }
                Err(e) => warning!("release files: not checked: {e}"),
            },
        );
    }

    /// For krunk://gpu and About.
    pub(super) fn integrity_json(&self) -> serde_json::Value {
        let r = self.integrity.report.borrow();
        serde_json::json!({
            "checked": r.checked,
            "flavor": r.flavor,
            "libcef": r.libcef,
            "engineFiles": r.engine_files,
            "problems": r.problems.iter().chain(&r.release_problems).collect::<Vec<_>>(),
            "releaseChecked": r.release_checked,
            "healthy": r.healthy(),
            "summary": r.summary(),
            "installed": crate::ffi::process::is_installed(),
        })
    }
}
