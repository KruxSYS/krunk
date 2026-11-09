//! Process-wide setup for the UI thread.

use windows::core::w;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::SetDllDirectoryW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

/// Per-monitor DPI awareness and removal of the current directory from the
/// DLL search path. Runs first in the browser process, before any window:
/// the bootstrap exe's embedded manifest does not declare DPI awareness.
pub fn init_process() {
    // SAFETY: process-wide settings applied once at startup on the main thread.
    unsafe {
        let _ = SetDllDirectoryW(w!(""));
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // Krunk's own identity for the taskbar and the media overlay (0.11.0).
    super::appid::set_process_app_id();
}

/// COM single-threaded apartment for the UI thread (file dialogs).
pub fn init_ui_thread() -> Result<(), String> {
    // SAFETY: called once on the UI thread before any COM use.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| format!("CoInitializeEx failed: {e}")) }
}

/// Is this process one of the engine's subprocesses (renderer, GPU, utility)?
pub fn is_subprocess() -> bool {
    std::env::args().skip(1).any(|a| a.starts_with("--type="))
}

/// Waits (up to `timeout_ms`) for process `pid` to exit. Used by "Restart
/// now": the new instance waits until the old one has released the profile.
pub fn wait_for_process(pid: u32, timeout_ms: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: opening a process handle only for waiting, closed afterwards.
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(h, timeout_ms);
            let _ = CloseHandle(h);
        }
    }
}

/// Ends process `pid` (a renderer of ours that stopped responding, 0.11.0).
pub fn terminate(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: a handle with terminate rights only, closed afterwards.
    unsafe {
        match OpenProcess(PROCESS_TERMINATE, false, pid) {
            Ok(h) => {
                let ok = TerminateProcess(h, 1).is_ok();
                let _ = CloseHandle(h);
                ok
            }
            Err(_) => false,
        }
    }
}

/// Starts a new instance of this executable with `args`.
pub fn relaunch(args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Creates the `Krunk.Running` mutex and keeps it until the process exits.
/// The installer waits while it exists (a running Krunk holds its files).
pub fn hold_running_mutex() {
    use windows::Win32::System::Threading::CreateMutexW;
    // SAFETY: an unnamed-security, non-owned mutex; the handle is deliberately
    // never closed, so the mutex lives exactly as long as this process.
    let _ = unsafe { CreateMutexW(None, false, w!("Krunk.Running")) };
}

/// Starts a downloaded installer with `args` (no shell, no elevation: the
/// installer is per user).
pub fn start_installer(path: &std::path::Path, args: &[&str]) -> Result<(), String> {
    std::process::Command::new(path).args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Bytes free for this user on the disk holding `dir` (0.11.7: a download in parts checks it
/// before it allocates its file).
pub fn disk_free_bytes(dir: &std::path::Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut free = 0u64;
    // SAFETY: the path string outlives the call; `free` is a valid out pointer, the other
    // outputs are not requested.
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

/// Is this copy installed (by the installer, which leaves its uninstaller
/// next to Krunk.exe), rather than a portable copy from the zip?
pub fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("uninstall.exe").is_file()))
        .unwrap_or(false)
}
