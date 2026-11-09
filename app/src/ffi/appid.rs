//! Krunk's Windows app identity (0.11.0). Without one, Windows named the media overlay
//! (the system media transport controls the engine registers for playing media) after the
//! engine: Chromium's logo instead of Krunk's. The browser process now carries an explicit
//! AppUserModelID, registered with Krunk's name and icon, and Krunk's own shortcuts carry the
//! same ID, so the taskbar keeps grouping its windows under a pinned Krunk.

use std::path::{Path, PathBuf};
use windows::core::{w, Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_RoamingAppData, IShellLinkW, SHGetKnownFolderPath,
    SetCurrentProcessExplicitAppUserModelID, ShellLink, KF_FLAG_DEFAULT,
};

/// The AppUserModelID of Krunk (the installer writes the same).
pub const APP_ID: &str = "Krunk.Browser";
const APP_ID_W: PCWSTR = w!("Krunk.Browser");
const REG_KEY: PCWSTR = w!("Software\\Classes\\AppUserModelId\\Krunk.Browser");

/// The browser process's identity: set first thing, before any window.
pub fn set_process_app_id() {
    // SAFETY: a process-wide setting with a static string, on the main thread at start.
    let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(APP_ID_W) };
}

/// The installed copy (`exe`): the ID's name and icon in the registry, and the ID on Krunk's
/// own shortcuts (Start menu, desktop, taskbar pins), where it is missing. Runs on a job
/// thread; a portable copy changes nothing on the system.
pub fn register_installed(exe: &Path) -> Vec<String> {
    let mut done = Vec::new();
    if write_registry(exe) {
        done.push("registry".to_string());
    }
    // SAFETY: COM for this job thread only; every object is released before CoUninitialize.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        for lnk in shortcuts() {
            if stamp(&lnk, exe) {
                done.push(lnk.display().to_string());
            }
        }
        if init.is_ok() {
            CoUninitialize();
        }
    }
    done
}

fn write_registry(exe: &Path) -> bool {
    let icon = format!("{},0", exe.display());
    // SAFETY: a per-user key of Krunk's own, values written with their lengths.
    unsafe {
        let mut key = HKEY::default();
        let created = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            REG_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if created.is_err() {
            return false;
        }
        let ok = set_sz(key, w!("DisplayName"), "Krunk") && set_sz(key, w!("IconUri"), &icon);
        let _ = RegCloseKey(key);
        ok
    }
}

/// SAFETY: `key` is an open key with write access.
unsafe fn set_sz(key: HKEY, name: PCWSTR, value: &str) -> bool {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
    RegSetValueExW(key, name, None, REG_SZ, Some(bytes)).is_ok()
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// The places a Krunk shortcut is: the Start menu, the desktop, the taskbar pins.
fn shortcuts() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = known_folder(&FOLDERID_Programs) {
        out.push(p.join("Krunk.lnk"));
    }
    if let Some(p) = known_folder(&FOLDERID_Desktop) {
        out.push(p.join("Krunk.lnk"));
    }
    if let Some(p) = known_folder(&FOLDERID_RoamingAppData) {
        let pins = p.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar");
        if let Ok(dir) = std::fs::read_dir(pins) {
            out.extend(
                dir.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"))),
            );
        }
    }
    out.into_iter().filter(|p| p.is_file()).collect()
}

/// Gives shortcut `lnk` Krunk's ID if it starts `exe` and has no (or another) ID.
/// SAFETY: COM is initialised on this thread.
unsafe fn stamp(lnk: &Path, exe: &Path) -> bool {
    let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return false };
    let Ok(file) = link.cast::<IPersistFile>() else { return false };
    let path = HSTRING::from(lnk.as_os_str());
    if file.Load(&path, STGM_READWRITE).is_err() {
        return false;
    }
    let mut target = [0u16; 1024];
    if link.GetPath(&mut target, std::ptr::null_mut(), 0).is_err() {
        return false;
    }
    let len = target.iter().position(|&c| c == 0).unwrap_or(target.len());
    let target = PathBuf::from(String::from_utf16_lossy(&target[..len]));
    if !same_file(&target, exe) {
        return false;
    }
    let Ok(store) = link.cast::<IPropertyStore>() else { return false };
    if current_id(&store, &PKEY_AppUserModel_ID).as_deref() == Some(APP_ID) {
        return false;
    }
    // windows-rs frees a PROPVARIANT when it is dropped (PropVariantClear → CoTaskMemFree of
    // the string), so the string must come from CoTaskMemAlloc. (0.11.0 before release pointed
    // it at Rust memory: heap corruption on the first start that stamped a shortcut.)
    let Some(value) = lpwstr_variant(APP_ID) else { return false };
    store.SetValue(&PKEY_AppUserModel_ID, &value).is_ok()
        && store.Commit().is_ok()
        && file.Save(PCWSTR::null(), true).is_ok()
}

/// A VT_LPWSTR PROPVARIANT owning a CoTaskMemAlloc copy of `s` (freed when it is dropped).
/// SAFETY: COM is initialised on this thread.
unsafe fn lpwstr_variant(s: &str) -> Option<PROPVARIANT> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let mem = CoTaskMemAlloc(wide.len() * 2) as *mut u16;
    if mem.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), mem, wide.len());
    let mut value = PROPVARIANT::default();
    (*value.Anonymous.Anonymous).vt = VT_LPWSTR;
    (*value.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(mem);
    Some(value)
}

/// SAFETY: COM is initialised on this thread.
unsafe fn current_id(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    // The value is cleared when it is dropped (windows-rs), never by hand as well.
    let v = store.GetValue(key).ok()?;
    let s = PropVariantToStringAlloc(&v).ok().map(|p| {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    });
    s.filter(|s| !s.is_empty())
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}
