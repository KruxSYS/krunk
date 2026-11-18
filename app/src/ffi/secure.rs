//! DPAPI key wrapping and a "private" clipboard for copied passwords (and the plain
//! clipboard: addresses, 0.12.2 images).

use lb_core::vault::KeyWrapper;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, LocalFree, HANDLE, HLOCAL, HWND};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use zeroize::Zeroizing;

/// Extra entropy so other applications using DPAPI for the same user cannot
/// trivially unwrap our blob by accident. It keeps the browser's name before
/// the rename to Krunk: existing vaults were wrapped with it.
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
                w!("Krunk vault key"),
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

/// DPAPI for the account's refresh token in `account.json` (0.8.0): its own entropy and
/// description, so the vault key and the token can't be swapped for each other.
pub struct DpapiAccount;

const ACCOUNT_ENTROPY: &[u8] = b"Krunk account refresh token v1";

impl KeyWrapper for DpapiAccount {
    fn name(&self) -> &'static str {
        "dpapi-account"
    }

    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(ACCOUNT_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input/entropy point to live slices for the call; DPAPI allocates `out`, which
        // `take_blob` frees.
        unsafe {
            CryptProtectData(
                &input,
                w!("Krunk account"),
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
        let entropy = blob(ACCOUNT_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| format!("CryptUnprotectData: {e}"))?;
        }
        Ok(take_blob(&out))
    }
}

/// DPAPI for the sync state key in `sync.json` (0.9.0, PROTOCOL §8): its own entropy and
/// description, so it can't be swapped with the vault key or the account token.
pub struct DpapiSync;

const SYNC_ENTROPY: &[u8] = b"Krunk sync state v1";

impl KeyWrapper for DpapiSync {
    fn name(&self) -> &'static str {
        "dpapi-sync"
    }

    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(SYNC_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input/entropy point to live slices for the call; DPAPI allocates `out`, which
        // `take_blob` frees.
        unsafe {
            CryptProtectData(&input, w!("Krunk sync"), Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| format!("CryptProtectData: {e}"))?;
        }
        Ok(take_blob(&out).to_vec())
    }

    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        let input = blob(data);
        let entropy = blob(SYNC_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| format!("CryptUnprotectData: {e}"))?;
        }
        Ok(take_blob(&out))
    }
}

/// DPAPI for the AI providers' API keys in the `ai_keys` table (0.12.0): its own entropy and
/// description, so a key blob can't be swapped with the vault key, the account token or the
/// sync state.
pub struct DpapiAgent;

const AGENT_ENTROPY: &[u8] = b"Krunk AI provider key v1";

impl KeyWrapper for DpapiAgent {
    fn name(&self) -> &'static str {
        "dpapi-agent"
    }

    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(AGENT_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input/entropy point to live slices for the call; DPAPI allocates `out`, which
        // `take_blob` frees.
        unsafe {
            CryptProtectData(
                &input,
                w!("Krunk AI provider key"),
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
        let entropy = blob(AGENT_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| format!("CryptUnprotectData: {e}"))?;
        }
        Ok(take_blob(&out))
    }
}

/// DPAPI for the VPN profiles in the `vpn_profiles` table (0.14.0): the profile file and the
/// OpenVPN user name and password, with their own entropy and description, so a profile blob
/// can't be swapped with any other of Krunk's secrets.
pub struct DpapiVpn;

const VPN_ENTROPY: &[u8] = b"Krunk VPN profile v1";

impl KeyWrapper for DpapiVpn {
    fn name(&self) -> &'static str {
        "dpapi-vpn"
    }

    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(VPN_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input/entropy point to live slices for the call; DPAPI allocates `out`, which
        // `take_blob` frees.
        unsafe {
            CryptProtectData(
                &input,
                w!("Krunk VPN profile"),
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
        let entropy = blob(VPN_ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| format!("CryptUnprotectData: {e}"))?;
        }
        Ok(take_blob(&out))
    }
}

/// Decrypts a DPAPI blob made without extra entropy by another program of this user
/// (Chromium's `os_crypt` key and its legacy password blobs).
pub fn unprotect_plain(data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points to a live slice for the call; DPAPI allocates `out`, which
    // `take_blob` frees.
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|e| format!("CryptUnprotectData: {e}"))?;
    }
    Ok(take_blob(&out))
}

/// The secret of a generic Windows Credential Manager entry of this user (Firefox keeps its
/// OS key store key, which encrypts saved card numbers, under "Firefox Encrypted Storage":
/// `security/manager/ssl/CredentialManagerSecret.cpp`). `Ok(None)` when there is no entry.
pub fn read_generic_credential(target: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string alive for the call; on success
    // CredReadW allocates `cred`, which is read once and freed with CredFree below.
    unsafe {
        if let Err(e) = CredReadW(PCWSTR(wide.as_ptr()), CRED_TYPE_GENERIC, None, &mut cred) {
            if e.code() == ERROR_NOT_FOUND.to_hresult() {
                return Ok(None);
            }
            return Err(format!("CredReadW: {e}"));
        }
        if cred.is_null() {
            return Ok(None);
        }
        let c = &*cred;
        let secret = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
            Zeroizing::new(Vec::new())
        } else {
            let slice = std::slice::from_raw_parts_mut(c.CredentialBlob, c.CredentialBlobSize as usize);
            let copy = Zeroizing::new(slice.to_vec());
            slice.fill(0);
            copy
        };
        CredFree(cred as *const std::ffi::c_void);
        Ok(Some(secret))
    }
}

const CF_UNICODETEXT: u32 = 13;

/// Copies `text` to the clipboard, marked so clipboard managers and the
/// Windows clipboard history / cloud clipboard skip it. Returns the
/// clipboard sequence number after the copy, for [`clear_clipboard_if`].
pub fn copy_private(owner: HWND, text: &str) -> Option<u32> {
    put_text(owner, text, true)
}

/// Puts ordinary text (a download's address) on the clipboard; `false` if that failed.
pub fn copy_text(owner: HWND, text: &str) -> bool {
    put_text(owner, text, false).is_some()
}

fn put_text(owner: HWND, text: &str, private: bool) -> Option<u32> {
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
            if private {
                // Ask monitors and clipboard history not to keep it.
                set_dword(w!("ExcludeClipboardContentFromMonitorProcessing"), 0);
                set_dword(w!("CanIncludeInClipboardHistory"), 0);
                set_dword(w!("CanUploadToCloudClipboard"), 0);
            }
            Some(())
        })();
        let _ = CloseClipboard();
        result?;
        Some(GetClipboardSequenceNumber())
    }
}

/// Puts a copied image on the clipboard (0.12.2): `dib` as `CF_DIB` and `png` as the
/// registered "PNG" format; `false` if that failed.
pub fn copy_image(owner: HWND, dib: &[u8], png: &[u8]) -> bool {
    const CF_DIB: u32 = 8;
    // SAFETY: standard clipboard sequence on the UI thread; memory handed to SetClipboardData
    // is owned by the system afterwards.
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return false;
        }
        let ok = EmptyClipboard().is_ok() && put_bytes(CF_DIB, dib);
        if ok && !png.is_empty() {
            // Optional: programs that read PNG get the transparency; the DIB alone is enough.
            let id = RegisterClipboardFormatW(w!("PNG"));
            if id != 0 {
                let _ = put_bytes(id, png);
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

/// Adds one clipboard format holding `data`.
///
/// # Safety
/// The clipboard must be open.
unsafe fn put_bytes(format: u32, data: &[u8]) -> bool {
    // SAFETY: the caller holds the clipboard open; memory ownership passes to the system on
    // success.
    unsafe {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, data.len()) else { return false };
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(mem));
            return false;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        let _ = GlobalUnlock(mem);
        if SetClipboardData(format, Some(HANDLE(mem.0))).is_err() {
            let _ = GlobalFree(Some(mem));
            return false;
        }
        true
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

/// The clipboard's text, if it holds any (at most 64 KiB, for "Paste and go").
pub fn clipboard_text(owner: HWND) -> Option<String> {
    const MAX_CHARS: usize = 65_536;
    // SAFETY: standard clipboard sequence on the UI thread; the handle from
    // GetClipboardData is owned by the system and only read while locked.
    unsafe {
        OpenClipboard(Some(owner)).ok()?;
        let result = (|| {
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let mem = windows::Win32::Foundation::HGLOBAL(h.0);
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while len < MAX_CHARS && *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(mem);
            Some(text)
        })();
        let _ = CloseClipboard();
        result
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

#[cfg(test)]
mod sync_tests {
    use super::*;
    use lb_core::sync::state;

    /// The Windows test exe of S5 (run on the VM): `sync.json` round-trips through DPAPI, and a
    /// blob of one entropy can't be opened with another.
    #[test]
    fn sync_state_round_trips_through_dpapi() {
        let dir = std::env::temp_dir().join(format!("krunk-dpapi-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.json");
        let st = state::SyncState {
            account_id: "0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0".into(),
            device_id: "7d4012d9-f415-4a68-9b92-e029ffd1db29".into(),
            dev_short: 2,
            device_sk: Zeroizing::new([7; 32]),
            last_pull_seq: 99,
            ..Default::default()
        };
        state::save(&path, &st, &DpapiSync).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"wrap\": \"dpapi-sync\"") && !text.contains("7d4012d9"));
        assert!(state::load(&path, &DpapiSync).unwrap().unwrap() == st);
        let blob = DpapiSync.protect(b"state key").unwrap();
        assert_eq!(&*DpapiSync.unprotect(&blob).unwrap(), b"state key");
        assert!(DpapiAccount.unprotect(&blob).is_err(), "the account entropy can't open it");
        assert!(Dpapi.unprotect(&blob).is_err(), "nor the vault's");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 0.12.0: an AI key round-trips and can't be opened with the other entropies.
    #[test]
    fn agent_key_round_trips_with_its_own_entropy() {
        let blob = DpapiAgent.protect(b"sk-test-key").unwrap();
        assert_eq!(&*DpapiAgent.unprotect(&blob).unwrap(), b"sk-test-key");
        assert!(Dpapi.unprotect(&blob).is_err() && DpapiSync.unprotect(&blob).is_err());
        assert!(DpapiAgent.unprotect(&Dpapi.protect(b"x").unwrap()).is_err());
    }
}
