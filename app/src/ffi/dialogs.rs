//! Message boxes and opening URLs in the system browser.

use std::path::{Path, PathBuf};
use windows::core::{w, Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog, IShellItem,
    SHCreateItemFromParsingName, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    SIGDN_FILESYSPATH,
};
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

/// File type filter for the file dialogs, e.g. `("HTML files", "*.html;*.htm")`.
pub type Filter<'a> = (&'a str, &'a str);

fn configure(
    dialog: &IFileDialog,
    title: &str,
    filter: Filter<'_>,
    folder: Option<&Path>,
) -> windows::core::Result<()> {
    let name = HSTRING::from(filter.0);
    let spec = HSTRING::from(filter.1);
    let all_name = HSTRING::from("All files");
    let all_spec = HSTRING::from("*.*");
    let specs = [
        COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) },
        COMDLG_FILTERSPEC { pszName: PCWSTR(all_name.as_ptr()), pszSpec: PCWSTR(all_spec.as_ptr()) },
    ];
    // SAFETY: COM calls on a live dialog; the strings outlive the calls.
    unsafe {
        dialog.SetTitle(&HSTRING::from(title))?;
        dialog.SetFileTypes(&specs)?;
        if let Some(dir) = folder {
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
                let _ = dialog.SetFolder(&item);
            }
        }
    }
    Ok(())
}

fn result_path(dialog: &IFileDialog) -> Option<PathBuf> {
    // SAFETY: reading the chosen item's file-system path and freeing it.
    unsafe {
        let item = dialog.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
        s.map(PathBuf::from)
    }
}

/// Native "Open" dialog. Returns `None` if cancelled.
pub fn open_file(owner: HWND, title: &str, filter: Filter<'_>) -> Option<PathBuf> {
    // SAFETY: creating and showing the standard file dialog on the UI thread.
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let base: IFileDialog = d.cast().ok()?;
        configure(&base, title, filter, None).ok()?;
        let opts = base.GetOptions().ok()?;
        base.SetOptions(opts | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST).ok()?;
        base.Show(Some(owner)).ok()?;
        result_path(&base)
    }
}

/// Native "Save as" dialog. Returns `None` if cancelled.
pub fn save_file(
    owner: HWND,
    title: &str,
    default_name: &str,
    filter: Filter<'_>,
    folder: Option<&Path>,
) -> Option<PathBuf> {
    // SAFETY: creating and showing the standard file dialog on the UI thread.
    unsafe {
        let d: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let base: IFileDialog = d.cast().ok()?;
        configure(&base, title, filter, folder).ok()?;
        let _ = base.SetFileName(&HSTRING::from(default_name));
        let opts = base.GetOptions().ok()?;
        base.SetOptions(opts | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST).ok()?;
        base.Show(Some(owner)).ok()?;
        result_path(&base)
    }
}

/// Opens a downloaded file with its associated program.
pub fn open_path(path: &Path) {
    // SAFETY: ShellExecute "open" on an existing local file path.
    unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL) };
}

/// Opens Explorer with the file selected.
pub fn show_in_folder(path: &Path) {
    let args = format!("/select,\"{}\"", path.display());
    // SAFETY: launching explorer.exe with a quoted local path argument.
    unsafe { ShellExecuteW(None, w!("open"), w!("explorer.exe"), &HSTRING::from(args), None, SW_SHOWNORMAL) };
}
