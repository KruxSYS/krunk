//! The VPN proxy's owner check (0.14.0) on Windows: the local proxy accepts a connection only
//! from the browser process itself or from one of its own `Krunk.exe` subprocesses (the
//! engine's network service, renderers, GPU and utility processes: CEF starts them from the same
//! exe, no `browser_subprocess_path` is set). Programs the browser starts through the shell
//! (a downloaded file opened, mail, external protocols, installers) are children with another
//! image and never pass. The logic, with its caches, is `lb_core::vpnpeer`; this is its view
//! of the system: the TCP table (`GetExtendedTcpTable`, the full 4-tuple of the client's
//! ESTABLISHED row), a ToolHelp snapshot for the parent chain, `GetProcessTimes` and
//! `QueryFullProcessImageNameW`. Called on the VPN's blocking threads.

use lb_core::vpnpeer::{PeerCheck, Refusal, System, TcpRow};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_ESTAB, TCP_TABLE_OWNER_PID_ALL,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// The check of this browser process (made on first use).
static CHECK: OnceLock<Option<PeerCheck>> = OnceLock::new();

/// May the connection from `peer` (the client end, on loopback) to the proxy's end `proxy` use
/// the proxy?
pub fn peer_allowed(proxy: SocketAddr, peer: SocketAddr) -> bool {
    let Some(check) = CHECK.get_or_init(|| {
        let c = PeerCheck::new(std::process::id(), &Win);
        if c.is_none() {
            crate::log::warning!("vpn: the browser process's own image could not be read; the proxy refuses everyone");
        }
        c
    }) else {
        return false;
    };
    match check.check(&Win, proxy, peer, Instant::now()) {
        Ok(()) => true,
        Err(why) => {
            if log_refusal() {
                let text = match &why {
                    Refusal::NotLoopback => "not from loopback".to_string(),
                    Refusal::NoOwner => "no process owns the connection".to_string(),
                    Refusal::Gone(p) => format!("process {p} is gone"),
                    Refusal::NotDescendant(p) => format!("process {p} is not Krunk's"),
                    Refusal::OtherImage(p, file) => format!("process {p} ({file}) is not Krunk's own program"),
                    Refusal::Cached(p) => format!("process {p} was refused a moment ago"),
                };
                crate::log::warning!("vpn: refused a proxy connection: {text}");
            }
            false
        }
    }
}

/// At most one refusal line every 10 s (a program could knock in a loop).
fn log_refusal() -> bool {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return false };
    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

/// The system as the check sees it.
struct Win;

impl System for Win {
    fn tcp_rows(&self) -> Option<Vec<TcpRow>> {
        tcp_rows()
    }

    fn processes(&self) -> Vec<(u32, u32)> {
        super::procmem::snapshot()
    }

    fn created(&self, pid: u32) -> Option<u64> {
        let h = Process::open(pid)?;
        let (mut c, mut e, mut k, mut u) =
            (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: a live query handle and four FILETIMEs to write.
        unsafe { GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) }.ok()?;
        Some((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
    }

    fn image(&self, pid: u32) -> Option<String> {
        let h = Process::open(pid)?;
        let mut buf = vec![0u16; 32 * 1024];
        let mut len = buf.len() as u32;
        // SAFETY: a live query handle; `buf` has `len` writable UTF-16 units and the call
        // writes at most that many, setting `len` to what it wrote.
        unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
        Some(String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]))
    }
}

/// A query-only process handle, closed on drop.
struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> Option<Process> {
        // SAFETY: opens a query-only handle (owned by the returned value).
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok().map(Process)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: a handle from OpenProcess, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The IPv4 TCP table with the owner of each row.
fn tcp_rows() -> Option<Vec<TcpRow>> {
    // u32 elements: the table is a u32 count followed by rows of six u32.
    let mut buf: Vec<u32> = vec![0; 4096];
    for _ in 0..4 {
        let mut size = (buf.len() * 4) as u32;
        // SAFETY: `buf` has `size` writable bytes; the call fills it or says how many it needs.
        let r = unsafe {
            GetExtendedTcpTable(
                Some(buf.as_mut_ptr().cast()),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        match r {
            NO_ERROR => {
                let table = buf.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
                // SAFETY: on success the buffer holds a MIB_TCPTABLE_OWNER_PID with
                // `dwNumEntries` rows, all inside the `size` bytes written.
                let rows: &[MIB_TCPROW_OWNER_PID] = unsafe {
                    let n = (*table).dwNumEntries as usize;
                    let max = (size as usize).saturating_sub(4) / std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
                    std::slice::from_raw_parts((*table).table.as_ptr(), n.min(max))
                };
                let addr = |a: u32, p: u32| {
                    SocketAddrV4::new(Ipv4Addr::from(a.to_ne_bytes()), u16::from_be((p & 0xffff) as u16))
                };
                return Some(
                    rows.iter()
                        .map(|r| TcpRow {
                            local: addr(r.dwLocalAddr, r.dwLocalPort),
                            remote: addr(r.dwRemoteAddr, r.dwRemotePort),
                            established: r.dwState == MIB_TCP_STATE_ESTAB.0 as u32,
                            pid: r.dwOwningPid,
                        })
                        .collect(),
                );
            }
            ERROR_INSUFFICIENT_BUFFER => buf.resize((size as usize).div_ceil(4) + 256, 0),
            _ => return None,
        }
    }
    None
}
