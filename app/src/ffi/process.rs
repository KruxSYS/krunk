//! Process-wide setup for the UI thread.

use windows::core::w;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::SetDllDirectoryW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

/// COM single-threaded apartment (WebView2 requires STA), per-monitor DPI
/// awareness and removal of the current directory from the DLL search path.
pub fn init_ui_thread() -> Result<(), String> {
    // SAFETY: called once at startup on the UI thread before any window exists.
    unsafe {
        let _ = SetDllDirectoryW(w!(""));
        // The manifest already declares PerMonitorV2; this is a fallback.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| format!("CoInitializeEx failed: {e}"))
    }
}
