//! Message boxes and opening URLs in the system browser.

use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, SW_SHOWNORMAL,
};

pub const WEBVIEW2_DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

pub fn error(title: &str, text: &str) {
    // SAFETY: modal message box without owner; strings outlive the call.
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONERROR) };
}

/// Yes/No question; returns `true` for Yes.
pub fn ask(title: &str, text: &str, warning: bool) -> bool {
    let icon = if warning { MB_ICONWARNING } else { Default::default() };
    // SAFETY: as above.
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_YESNO | icon) == IDYES }
}

/// Opens an https:// URL in the user's default browser.
pub fn open_external(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    // SAFETY: ShellExecute with the "open" verb on an https URL.
    unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL) };
}

/// Explains that the WebView2 runtime is missing and offers the download page.
pub fn runtime_missing(detail: &str) {
    let text = format!(
        "LightBrowser needs the Microsoft Edge WebView2 Runtime, which was not found on this PC.\n\n\
         It is included in Windows 11 and most Windows 10 installations, and can be installed for free \
         (the \"Evergreen Bootstrapper\" or \"Evergreen Standalone Installer\").\n\n\
         Download page:\n{WEBVIEW2_DOWNLOAD_URL}\n\n{detail}\nOpen the download page now?"
    );
    if ask("LightBrowser – WebView2 Runtime required", &text, true) {
        open_external(WEBVIEW2_DOWNLOAD_URL);
    }
}
