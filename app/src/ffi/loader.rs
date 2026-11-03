//! The embedded WebView2 loader.
//!
//! `WebView2Loader.dll` is compiled into the exe, written to
//! `%LOCALAPPDATA%\LightBrowser\bin\`, hash-checked while a handle that
//! denies writers is held open, and loaded by absolute path. Its two entry
//! points are resolved with `GetProcAddress`, so the exe has no import-table
//! dependency on the DLL.

use lb_core::integrity::sha256_hex;
use std::ffi::c_void;
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, ICoreWebView2EnvironmentOptions,
};
use windows::core::{s, Interface, HRESULT, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

const LOADER_DLL: &[u8] = include_bytes!("../../../assets/webview2/x64/WebView2Loader.dll");
/// SHA-256 of the bundled loader (NuGet Microsoft.Web.WebView2 1.0.4314.35).
pub const LOADER_SHA256: &str = "d3fce90bdeecb08596e377dea6a316354dd8949e32ff09b89d51be00c14e21b6";
pub const LOADER_VERSION: &str = "1.0.4314.35";
const FILE_NAME: &str = "WebView2Loader.dll";
const FILE_SHARE_READ: u32 = 0x1;

type CreateEnvironmentFn = unsafe extern "system" fn(PCWSTR, PCWSTR, *mut c_void, *mut c_void) -> HRESULT;
type GetVersionFn = unsafe extern "system" fn(PCWSTR, *mut PWSTR) -> HRESULT;

pub struct Loader {
    create_environment: CreateEnvironmentFn,
    get_version: GetVersionFn,
    // The module stays loaded for the life of the process.
    _module: HMODULE,
}

impl Loader {
    /// Extracts (if needed), verifies and loads the loader DLL from `bin_dir`.
    pub fn load(bin_dir: &Path) -> Result<Loader, String> {
        if sha256_hex(LOADER_DLL) != LOADER_SHA256 {
            return Err("The embedded WebView2 loader is damaged".into());
        }
        let path = bin_dir.join(FILE_NAME);
        // Hold a handle that allows other readers but no writers while we
        // verify and load, so the file cannot be swapped in between.
        let _guard = match open_verified(&path) {
            Ok(f) => f,
            Err(_) => {
                write_loader(&path)?;
                open_verified(&path)?
            }
        };
        let wide = HSTRING::from(path.as_os_str());
        // SAFETY: loading a DLL by absolute path whose contents we just verified;
        // dependencies are only searched in its own folder and System32.
        let module =
            unsafe { LoadLibraryExW(&wide, None, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|e| format!("Cannot load {}: {e}", path.display()))?;
        // SAFETY: resolving documented exports; the transmutes give them their
        // documented signatures from WebView2.h.
        unsafe {
            let create = GetProcAddress(module, s!("CreateCoreWebView2EnvironmentWithOptions"))
                .ok_or("WebView2Loader.dll lacks CreateCoreWebView2EnvironmentWithOptions")?;
            let version = GetProcAddress(module, s!("GetAvailableCoreWebView2BrowserVersionString"))
                .ok_or("WebView2Loader.dll lacks GetAvailableCoreWebView2BrowserVersionString")?;
            Ok(Loader {
                create_environment: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateEnvironmentFn>(
                    create,
                ),
                get_version: std::mem::transmute::<unsafe extern "system" fn() -> isize, GetVersionFn>(version),
                _module: module,
            })
        }
    }

    /// Version of the installed WebView2 runtime, or `None` if there is none.
    pub fn runtime_version(&self) -> Option<String> {
        let mut out = PWSTR::null();
        // SAFETY: documented signature; `out` receives a CoTaskMem string we free.
        let hr = unsafe { (self.get_version)(PCWSTR::null(), &mut out) };
        if hr.is_err() || out.is_null() {
            return None;
        }
        // SAFETY: `out` is a valid NUL-terminated wide string from the loader.
        let version = unsafe { out.to_string() }.ok();
        // SAFETY: freeing the string allocated by the loader with CoTaskMemAlloc.
        unsafe { CoTaskMemFree(Some(out.0 as *const c_void)) };
        version.filter(|v| !v.is_empty())
    }

    /// Starts creating the shared environment; `handler` is called on the UI thread.
    pub fn create_environment(
        &self,
        user_data_dir: &Path,
        options: &ICoreWebView2EnvironmentOptions,
        handler: &ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
    ) -> Result<(), String> {
        let dir = HSTRING::from(user_data_dir.as_os_str());
        // SAFETY: documented signature; the interfaces outlive the call and the
        // loader AddRefs what it keeps.
        let hr = unsafe {
            (self.create_environment)(PCWSTR::null(), PCWSTR(dir.as_ptr()), options.as_raw(), handler.as_raw())
        };
        hr.ok().map_err(|e| format!("CreateCoreWebView2EnvironmentWithOptions failed: {e}"))
    }
}

/// Opens the file denying writers and checks its hash.
fn open_verified(path: &PathBuf) -> Result<std::fs::File, String> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let mut data = Vec::with_capacity(LOADER_DLL.len());
    f.read_to_end(&mut data).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if sha256_hex(&data) != LOADER_SHA256 {
        return Err(format!("{} does not match the expected SHA-256", path.display()));
    }
    Ok(f)
}

fn write_loader(path: &Path) -> Result<(), String> {
    let tmp = path.with_extension("dll.tmp");
    std::fs::write(&tmp, LOADER_DLL).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Cannot replace {}: {e}", path.display()))
}
