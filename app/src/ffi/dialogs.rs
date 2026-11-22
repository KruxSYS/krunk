//! Message boxes and the native file dialogs.

use std::path::{Path, PathBuf};
use windows::core::{w, Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IDataObject, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog, IShellItem,
    SHCreateItemFromParsingName, SHDoDragDrop, BHID_DataObject, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, SW_SHOWNORMAL};

pub fn error(title: &str, text: &str) {
    // SAFETY: modal message box without owner; strings outlive the call.
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONERROR) };
}

/// Krunk's own runtime files (next to the exe) are missing or damaged.
pub fn runtime_damaged(detail: &str) {
    error(
        "Krunk",
        &format!(
            "Krunk cannot start because some of its files are missing or damaged.\n\n{detail}\n\n\
             Extract the Krunk download again and keep all of its files together in one folder."
        ),
    );
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

/// The user's Downloads folder.
pub fn downloads_folder() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: the returned CoTaskMem string is read once and freed.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
        s.map(PathBuf::from)
    }
}

/// Native folder picker. Returns `None` if cancelled.
pub fn pick_folder(owner: HWND, title: &str, start: Option<&Path>) -> Option<PathBuf> {
    // SAFETY: creating and showing the standard file dialog on the UI thread.
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let base: IFileDialog = d.cast().ok()?;
        base.SetTitle(&HSTRING::from(title)).ok()?;
        if let Some(dir) = start {
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
                let _ = base.SetFolder(&item);
            }
        }
        let opts = base.GetOptions().ok()?;
        base.SetOptions(opts | windows::Win32::UI::Shell::FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
            .ok()?;
        base.Show(Some(owner)).ok()?;
        result_path(&base)
    }
}

/// Opens a downloaded file with its associated program.
pub fn open_path(path: &Path) {
    // SAFETY: ShellExecute "open" on an existing local file path.
    unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL) };
}

/// Hands an email link to the app Windows opens `mailto:` with (0.11.3). The caller checked
/// that it is a `mailto:` address.
pub fn open_mailto(mailto: &str) {
    // SAFETY: ShellExecute "open" on a mailto: URL (Windows starts its registered handler).
    unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(mailto), None, None, SW_SHOWNORMAL) };
}

/// Opens Explorer with the file selected.
pub fn show_in_folder(path: &Path) {
    let args = format!("/select,\"{}\"", path.display());
    // SAFETY: launching explorer.exe with a quoted local path argument.
    unsafe { ShellExecuteW(None, w!("open"), w!("explorer.exe"), &HSTRING::from(args), None, SW_SHOWNORMAL) };
}

/// Drags a file out of Krunk (0.27.0: a finished download from the downloads popup or page) to
/// Explorer, the desktop or another app. The data object is the shell's own for the file (what
/// Explorer itself hands out, so every drop target takes it) and the shell draws the drag
/// image. Copy, move and link are offered: the target picks as for a drag out of Explorer
/// (a move within one drive, a copy to another; Shift/Ctrl change it). Blocks in the system's
/// drag loop until the drop or Esc; the effect the target reported, or `None` if the drag
/// could not start. The caller checks that the mouse button is still held: the drag loop
/// drops at once where the pointer is otherwise.
pub fn drag_file_out(path: &Path) -> Option<DROPEFFECT> {
    // SAFETY: COM calls on the UI thread (OLE is initialised there by the engine); the item
    // and data object are owned references released on drop.
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let data: IDataObject = item.BindToHandler(None, &BHID_DataObject).ok()?;
        SHDoDragDrop(None, &data, None, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK).ok()
    }
}
