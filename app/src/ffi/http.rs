//! Minimal HTTPS GET over WinHTTP, used on worker threads by the filter-list
//! updater and the update check. HTTPS only (the update check of a
//! `dev-updates` build may use http://), size-capped, with timeouts.

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

/// WinHTTP's errors have no system message text (they live in winhttp.dll), so the
/// common ones are spelled out; the code stays for the rest.
fn failure(step: &str, e: &windows::core::Error) -> String {
    let code = e.code().0 as u32;
    let text = match code & 0xFFFF {
        _ if code & 0xFFFF_0000 != 0x8007_0000 => None,
        12002 => Some("the server did not answer in time"),
        12007 => Some("the server name could not be resolved"),
        12029 => Some("could not connect to the server"),
        12030 | 12031 => Some("the connection was interrupted"),
        12037 | 12038 | 12045 | 12057 | 12175 => Some("the server's certificate or TLS connection was not accepted"),
        12152 => Some("the server sent an invalid response"),
        12180 => Some("the proxy could not be detected"),
        _ => None,
    };
    match text {
        Some(t) => format!("{step}: {t} (0x{code:08X})"),
        None => format!("{step}: {e}"),
    }
}

fn check(h: *mut c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!("{what} failed: {}", windows::core::Error::from_thread()))
    } else {
        Ok(Handle(h))
    }
}

/// How a request is made.
pub struct Options<'a> {
    pub proxy: &'a ProxyMode,
    pub timeout_ms: i32,
    pub user_agent: &'a str,
    /// Also fetch http:// URLs (dev-updates builds only).
    pub allow_http: bool,
}

/// Downloads `url` (must be https) into memory, refusing bodies larger
/// than `max_bytes`. Returns the body of a 200 response.
pub fn get(url: &str, max_bytes: usize, proxy: &ProxyMode, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let opts = Options { proxy, timeout_ms, user_agent: "Krunk-FilterUpdate/1.0", allow_http: false };
    get_with(url, max_bytes, &opts)
}

/// [`get`] with explicit options.
pub fn get_with(url: &str, max_bytes: usize, opts: &Options) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    fetch(url, max_bytes as u64, opts, &mut |chunk| {
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(body)
}

/// Streams the body of a 200 response to `sink`, refusing more than
/// `max_bytes`. Returns the number of bytes received.
pub fn fetch(
    url: &str,
    max_bytes: u64,
    opts: &Options,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<u64, String> {
    let u = url::Url::parse(url).map_err(|e| e.to_string())?;
    let secure = match u.scheme() {
        "https" => true,
        "http" if opts.allow_http => false,
        _ => return Err("only https:// URLs are fetched".into()),
    };
    let host = u.host_str().ok_or("no host")?.to_string();
    let port = u.port_or_known_default().unwrap_or(if secure { 443 } else { 80 });
    let path = match u.query() {
        Some(q) => format!("{}?{q}", u.path()),
        None => u.path().to_string(),
    };
    // SAFETY: straightforward WinHTTP calls; every handle is owned by a
    // `Handle` and closed on all paths; buffers are sized correctly.
    unsafe {
        let agent = HSTRING::from(opts.user_agent);
        let session = match opts.proxy {
            ProxyMode::System => {
                check(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, None, None, 0), "WinHttpOpen")?
            }
            ProxyMode::Named(p) => check(
                WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_NAMED_PROXY, &HSTRING::from(p.as_str()), w!("<local>"), 0),
                "WinHttpOpen",
            )?,
        };
        let t = opts.timeout_ms;
        WinHttpSetTimeouts(session.0, t, t, t, t).map_err(|e| e.to_string())?;
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
                if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) },
            ),
            "WinHttpOpenRequest",
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| failure("send", &e))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| failure("receive", &e))?;
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
        .map_err(|e| failure("status", &e))?;
        if status != 200 {
            return Err(format!("HTTP {status}"));
        }
        let mut total: u64 = 0;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut avail).map_err(|e| failure("read", &e))?;
            if avail == 0 {
                break;
            }
            let want = (avail as usize).min(buf.len());
            let mut read = 0u32;
            WinHttpReadData(request.0, buf.as_mut_ptr() as *mut c_void, want as u32, &mut read)
                .map_err(|e| failure("read", &e))?;
            if read == 0 {
                break;
            }
            total += u64::from(read);
            if total > max_bytes {
                return Err(format!("response larger than {max_bytes} bytes"));
            }
            sink(&buf[..read as usize])?;
        }
        Ok(total)
    }
}
