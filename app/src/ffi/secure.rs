//! DPAPI key wrapping and a "private" clipboard for copied passwords.

use lb_core::vault::KeyWrapper;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, LocalFree, HANDLE, HLOCAL, HWND};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use zeroize::Zeroizing;

/// Extra entropy so other applications using DPAPI for the same user cannot
/// trivially unwrap our blob by accident.
const ENTROPY: &[u8] = b"LightBrowser vault key wrap v1";

/// DPAPI with the CurrentUser scope: the wrapped vault key can only be
/// unwrapped by the same Windows account (on the same machine, unless the
/// account roams its DPAPI master keys).
pub struct Dpapi;

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

/// Copies a DPAPI output blob and frees it (wiping it first).
fn take_blob(out: &CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
    // SAFETY: `out` was filled by DPAPI with `cbData` bytes at `pbData`,
    // allocated with LocalAlloc; we copy, wipe and free it once.
    unsafe {
        let slice = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
        let copy = Zeroizing::new(slice.to_vec());
        slice.fill(0);
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut std::ffi::c_void)));
        copy
    }
}

impl KeyWrapper for Dpapi {
    fn name(&self) -> &'static str {
        "dpapi"
    }

    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input/entropy point to live slices for the call; DPAPI
        // allocates `out`, which `take_blob` frees.
        unsafe {
            CryptProtectData(
                &input,
                w!("LightBrowser vault key"),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| format!("CryptProtectData: {e}"))?;
        }
        Ok(take_blob(&out).to_vec())
    }

    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| format!("CryptUnprotectData: {e}"))?;
        }
        Ok(take_blob(&out))
    }
}

const CF_UNICODETEXT: u32 = 13;

/// Copies `text` to the clipboard, marked so clipboard managers and the
/// Windows clipboard history / cloud clipboard skip it. Returns the
/// clipboard sequence number after the copy, for [`clear_clipboard_if`].
pub fn copy_private(owner: HWND, text: &str) -> Option<u32> {
    let wide: Zeroizing<Vec<u16>> = Zeroizing::new(text.encode_utf16().chain(std::iter::once(0)).collect());
    // SAFETY: standard clipboard sequence on the UI thread; the global
    // memory handed to SetClipboardData is owned by the system afterwards.
    unsafe {
        OpenClipboard(Some(owner)).ok()?;
        let result = (|| {
            EmptyClipboard().ok()?;
            let bytes = wide.len() * 2;
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(mem);
            if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_err() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            // Ask monitors and clipboard history not to keep it.
            set_dword(w!("ExcludeClipboardContentFromMonitorProcessing"), 0);
            set_dword(w!("CanIncludeInClipboardHistory"), 0);
            set_dword(w!("CanUploadToCloudClipboard"), 0);
            Some(())
        })();
        let _ = CloseClipboard();
        result?;
        Some(GetClipboardSequenceNumber())
    }
}

/// Adds a registered clipboard format holding a DWORD.
///
/// # Safety
/// The clipboard must be open.
unsafe fn set_dword(format: PCWSTR, value: u32) {
    // SAFETY: the caller holds the clipboard open; memory ownership passes
    // to the system on success.
    unsafe {
        let id = RegisterClipboardFormatW(format);
        if id == 0 {
            return;
        }
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, 4) else { return };
        let ptr = GlobalLock(mem) as *mut u32;
        if ptr.is_null() {
            let _ = GlobalFree(Some(mem));
            return;
        }
        ptr.write_unaligned(value);
        let _ = GlobalUnlock(mem);
        if SetClipboardData(id, Some(HANDLE(mem.0))).is_err() {
            let _ = GlobalFree(Some(mem));
        }
    }
}

/// Empties the clipboard if it still holds what we copied (its sequence
/// number is unchanged), so a later copy by the user is never wiped.
pub fn clear_clipboard_if(owner: HWND, sequence: u32) -> bool {
    // SAFETY: clipboard calls on the UI thread.
    unsafe {
        if GetClipboardSequenceNumber() != sequence || OpenClipboard(Some(owner)).is_err() {
            return false;
        }
        let ok = EmptyClipboard().is_ok();
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::vault::{KdfParams, Vault, VaultError};

    #[test]
    fn dpapi_round_trip() {
        let data = b"wrapped vault key bytes";
        let wrapped = Dpapi.protect(data).expect("CryptProtectData");
        assert_ne!(&wrapped[..], &data[..]);
        let back = Dpapi.unprotect(&wrapped).expect("CryptUnprotectData");
        assert_eq!(&back[..], &data[..]);
        // A tampered blob is rejected.
        let mut bad = wrapped.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 0xff;
        assert!(Dpapi.unprotect(&bad).is_err());
    }

    #[test]
    fn vault_with_dpapi() {
        let dir = std::env::temp_dir().join(format!("lb-dpapi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault.json");
        let mut v = Vault::create(&path, "master password", KdfParams::insecure_for_tests(), &Dpapi).unwrap();
        v.save_login("https://a.example/", "alice", Zeroizing::new("s3cret".into()), 1, &Dpapi).unwrap();
        drop(v);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"wrap\": \"dpapi\""));
        assert!(!text.contains("s3cret") && !text.contains("alice"));
        let v = Vault::open(&path, "master password", &Dpapi).unwrap();
        assert_eq!(v.for_origin("https://a.example/")[0].password.as_str(), "s3cret");
        assert_eq!(Vault::open(&path, "wrong password!", &Dpapi).unwrap_err(), VaultError::WrongPassword);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
