//! The DLL entry points called by `Krunk.exe`, CEF's sandbox
//! bootstrap (`bootstrap.exe` renamed): it creates the sandbox broker or
//! target, loads `Krunk.dll` from its own folder and calls
//! `RunWinMain`. The same exe + DLL pair runs the browser process and every
//! subprocess (renderer, GPU, utility).

use super::app;
use crate::ffi::{dialogs, process};
use cef::*;
use std::cell::Cell;
use std::ffi::c_void;

thread_local! {
    static ARGS: Cell<Option<(*mut c_void, *mut u8)>> = const { Cell::new(None) };
}

/// The runtime files that must sit next to the exe.
const REQUIRED: &[&str] =
    &["libcef.dll", "chrome_elf.dll", "icudtl.dat", "resources.pak", "chrome_100_percent.pak", "locales\\en-US.pak"];

/// The V8 start-up snapshot, one of the two: CEF's binary distribution ships the Blink
/// context snapshot; Krunk's own cross-built engine (scripts/cef-build) ships V8's plain
/// snapshot, because Chromium cannot generate the context snapshot off Windows.
const V8_SNAPSHOT_ANY: &[&str] = &["v8_context_snapshot.bin", "snapshot_blob.bin"];
const V8_SNAPSHOT_MISSING: &str = "v8_context_snapshot.bin or snapshot_blob.bin";

/// The required runtime files that are not in `dir`.
fn missing_in(dir: &std::path::Path) -> Vec<&'static str> {
    let mut missing: Vec<&'static str> = REQUIRED.iter().copied().filter(|f| !dir.join(f).is_file()).collect();
    if !V8_SNAPSHOT_ANY.iter().any(|f| dir.join(f).is_file()) {
        missing.push(V8_SNAPSHOT_MISSING);
    }
    missing
}

fn missing_files() -> Vec<&'static str> {
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    match dir {
        Some(dir) => missing_in(&dir),
        None => {
            let mut all = REQUIRED.to_vec();
            all.push(V8_SNAPSHOT_MISSING);
            all
        }
    }
}

/// `(main args, sandbox info)` of this process for `cef::initialize`.
pub(super) fn process_args() -> Option<(MainArgs, *mut u8)> {
    let (instance, sandbox) = ARGS.with(|a| a.get())?;
    Some((MainArgs { instance: sys::HINSTANCE(instance.cast()) }, sandbox))
}

fn run(instance: *mut c_void, sandbox_info: *mut u8) -> i32 {
    let subprocess = process::is_subprocess();
    if !subprocess {
        process::init_process();
        // libcef.dll is delay-loaded: check before the first CEF call.
        let missing = missing_files();
        if !missing.is_empty() {
            dialogs::runtime_damaged(&format!("Missing:\n{}", missing.join("\n")));
            return 2;
        }
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let main_args = MainArgs { instance: sys::HINSTANCE(instance.cast()) };
    // Subprocesses run here until they exit; the browser process gets -1.
    let mut app = app::subprocess_app();
    let code = execute_process(Some(&main_args), Some(&mut app), sandbox_info);
    if code >= 0 {
        return code;
    }
    drop(app);
    ARGS.with(|a| a.set(Some((instance, sandbox_info))));
    crate::browser_main()
}

/// `RunWinMain` from `include/cef_sandbox_win.h` (GUI bootstrap).
#[unsafe(no_mangle)]
pub extern "C" fn RunWinMain(
    instance: *mut c_void,
    _command_line: *const u16,
    _show: i32,
    sandbox_info: *mut u8,
    _version_info: *mut c_void,
) -> i32 {
    run(instance, sandbox_info)
}

/// `RunConsoleMain` from `include/cef_sandbox_win.h` (console bootstrap,
/// `bootstrapc.exe`; not shipped, but the export keeps the DLL usable with it).
#[unsafe(no_mangle)]
pub extern "C" fn RunConsoleMain(
    _argc: i32,
    _argv: *const *const u8,
    sandbox_info: *mut u8,
    _version_info: *mut c_void,
) -> i32 {
    // SAFETY: the module handle of the running exe.
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
        .map(|h| h.0)
        .unwrap_or(std::ptr::null_mut());
    run(instance, sandbox_info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A folder with every required file except the snapshots, plus `extra`.
    fn runtime_dir(name: &str, extra: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("krunk-entry-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("locales")).unwrap();
        for f in REQUIRED.iter().chain(extra) {
            fs::write(dir.join(f), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn context_snapshot_alone_is_enough() {
        let dir = runtime_dir("ctx", &["v8_context_snapshot.bin"]);
        assert!(missing_in(&dir).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_blob_alone_is_enough() {
        let dir = runtime_dir("blob", &["snapshot_blob.bin"]);
        assert!(missing_in(&dir).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn no_snapshot_is_reported_once() {
        let dir = runtime_dir("none", &[]);
        assert_eq!(missing_in(&dir), vec![V8_SNAPSHOT_MISSING]);
        // Other missing files are still listed, the snapshot last.
        fs::remove_file(dir.join("icudtl.dat")).unwrap();
        assert_eq!(missing_in(&dir), vec!["icudtl.dat", V8_SNAPSHOT_MISSING]);
        let _ = fs::remove_dir_all(dir);
    }
}
