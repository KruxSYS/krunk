//! Krunk as the default browser (0.12.1): its registration with Windows, whether Windows opens
//! web links with it, and the way to Windows Settings where the user makes it the default.
//! What is registered is decided in `lb_core::default_browser`.

use lb_core::default_browser::{self as db, RegValue};
use std::path::Path;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::{
    ApplicationAssociationRegistration, IApplicationAssociationRegistration, SHChangeNotify, ShellExecuteW,
    AL_EFFECTIVE, AT_URLPROTOCOL, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// The process's command line as Windows passed it (for `--single-argument`).
pub fn raw_command_line() -> String {
    // SAFETY: the returned string is owned by the process and lives as long as it.
    unsafe { windows::Win32::System::Environment::GetCommandLineW().to_string().unwrap_or_default() }
}

/// Registers Krunk at `exe` as a browser unless its open command is already this one.
/// Returns whether anything was written.
pub fn register(exe: &Path) -> Result<bool, String> {
    let exe = exe.display().to_string();
    if read_sz(&db::open_command_key(), "").as_deref() == Some(db::open_command(&exe).as_str()) {
        return Ok(false);
    }
    for v in db::registration(&exe) {
        write_sz(&v).map_err(|e| format!("{}\\{}: {e}", v.key, v.name))?;
    }
    // SAFETY: a notification without items.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    Ok(true)
}

/// Does Windows open http and https links with Krunk? `None` when it can't be asked.
pub fn is_default() -> Option<bool> {
    // SAFETY: COM for this call only; the ProgID strings are freed with CoTaskMemFree.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| {
            let reg: IApplicationAssociationRegistration =
                CoCreateInstance(&ApplicationAssociationRegistration, None, CLSCTX_INPROC_SERVER).ok()?;
            let query = |scheme: &str| -> Option<String> {
                let p = reg.QueryCurrentDefault(&HSTRING::from(scheme), AT_URLPROTOCOL, AL_EFFECTIVE).ok()?;
                let s = p.to_string().ok();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            };
            let (http, https) = (query("http"), query("https"));
            Some(db::is_default(http.as_deref(), https.as_deref()))
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// Opens Windows Settings at Krunk's page of Default apps (its "Set default" button), or at
/// Default apps where that page can't be opened.
pub fn open_settings() -> Result<(), String> {
    // SAFETY: COM as ShellExecute wants it, for this call only (a job thread).
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let opened = [db::SETTINGS_URI, db::SETTINGS_FALLBACK].iter().any(|uri| {
        // SAFETY: plain strings; ShellExecuteW returns a value > 32 on success.
        let r = unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from(*uri), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
        };
        r.0 as isize > 32
    });
    if init.is_ok() {
        // SAFETY: paired with the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    if opened {
        Ok(())
    } else {
        Err("Windows Settings could not be opened".into())
    }
}

fn read_sz(key: &str, name: &str) -> Option<String> {
    let key = HSTRING::from(key);
    let name = HSTRING::from(name);
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: `buf` holds `size` bytes; RegGetValueW writes a terminated string into it.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn write_sz(v: &RegValue) -> Result<(), String> {
    let key_name = HSTRING::from(v.key.as_str());
    let name = HSTRING::from(v.name);
    let wide: Vec<u16> = v.data.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a per-user key of Krunk's own (or a file type's OpenWithProgids list); the value
    // is written with its length and the key closed again.
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &key_name,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|e| e.message())?;
        let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
        let r = RegSetValueExW(key, &name, None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
        r.ok().map_err(|e| e.message())
    }
}
