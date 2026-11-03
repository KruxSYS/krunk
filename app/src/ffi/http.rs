//! Minimal HTTPS GET over WinHTTP, used by the filter-list updater on a
//! worker thread. HTTPS only, size-capped, with timeouts.

use std::ffi::c_void;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

/// How the updater may reach the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyMode {
    /// The system (WinINet/WPAD) proxy configuration.
    System,
    /// An explicit `host:port` HTTP(S) proxy.
    Named(String),
}

/// Closes a WinHTTP handle on drop.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a handle returned by WinHTTP, closed exactly once.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn check(h: *mut c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!("{what} failed: {}", windows::core::Error::from_thread()))
    } else {
        Ok(Handle(h))
    }
}

/// Downloads `url` (must be https) into memory, refusing bodies larger
/// than `max_bytes`. Returns the body of a 200 response.
pub fn get(url: &str, max_bytes: usize, proxy: &ProxyMode, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let u = url::Url::parse(url).map_err(|e| e.to_string())?;
    if u.scheme() != "https" {
        return Err("only https:// URLs are fetched".into());
    }
    let host = u.host_str().ok_or("no host")?.to_string();
    let port = u.port_or_known_default().unwrap_or(443);
    let path = match u.query() {
        Some(q) => format!("{}?{q}", u.path()),
        None => u.path().to_string(),
    };
    // SAFETY: straightforward WinHTTP calls; every handle is owned by a
    // `Handle` and closed on all paths; buffers are sized correctly.
    unsafe {
        let session = match proxy {
            ProxyMode::System => check(
                WinHttpOpen(w!("LightBrowser-FilterUpdate/1.0"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, None, None, 0),
                "WinHttpOpen",
            )?,
            ProxyMode::Named(p) => check(
                WinHttpOpen(
                    w!("LightBrowser-FilterUpdate/1.0"),
                    WINHTTP_ACCESS_TYPE_NAMED_PROXY,
                    &HSTRING::from(p.as_str()),
                    w!("<local>"),
                    0,
                ),
                "WinHttpOpen",
            )?,
        };
        WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms).map_err(|e| e.to_string())?;
        let tls12 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | 0x2000; // | TLS 1.3
        let _ = WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&tls12.to_le_bytes()));
        let connect = check(WinHttpConnect(session.0, &HSTRING::from(host.as_str()), port, 0), "WinHttpConnect")?;
        let request = check(
            WinHttpOpenRequest(
                connect.0,
                w!("GET"),
                &HSTRING::from(path.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "WinHttpOpenRequest",
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| format!("send: {e}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("receive: {e}"))?;
        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        let mut index = 0u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            &mut index,
        )
        .map_err(|e| format!("status: {e}"))?;
        if status != 200 {
            return Err(format!("HTTP {status}"));
        }
        let mut body = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut avail).map_err(|e| format!("read: {e}"))?;
            if avail == 0 {
                break;
            }
            let want = (avail as usize).min(buf.len());
            let mut read = 0u32;
            WinHttpReadData(request.0, buf.as_mut_ptr() as *mut c_void, want as u32, &mut read)
                .map_err(|e| format!("read: {e}"))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
            if body.len() > max_bytes {
                return Err(format!("response larger than {max_bytes} bytes"));
            }
        }
        Ok(body)
    }
}
