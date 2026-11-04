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

/// Starts a new instance of this executable with `args`.
pub fn relaunch(args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
}
