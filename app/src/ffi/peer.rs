//! The VPN proxy's owner check (0.14.0): the local proxy accepts a connection only from Krunk's
//! own process tree (the browser process, or a child of it such as the engine's network
//! service). The owner of the peer's local port comes from the TCP table, the parent chain from
//! a ToolHelp snapshot; every link of the chain must have started after its parent, so a parent
//! id reused by Windows doesn't count. Called on the VPN's runtime threads.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// Processes found to be Krunk's, with when that was found (a cheap check for the next
/// connections; the network service opens many).
static ALLOWED: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());
const CACHE_TTL: Duration = Duration::from_secs(30);
const CACHE_MAX: usize = 64;
const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// May the connection from `peer` (the client end, on loopback) use the proxy?
pub fn peer_allowed(peer: SocketAddr) -> bool {
    let IpAddr::V4(ip) = peer.ip() else { return false };
    if !ip.is_loopback() {
        return false;
    }
    let Some(pid) = owner_pid(ip, peer.port()) else { return false };
    let me = std::process::id();
    if pid == me || cached(pid) {
        return true;
    }
    let ok = is_ours(pid, me);
    if ok {
        remember(pid);
    } else if log_refusal() {
        crate::log::warning!("vpn: refused a proxy connection from process {pid} (not Krunk's)");
    }
    ok
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

fn cached(pid: u32) -> bool {
    let Ok(mut c) = ALLOWED.lock() else { return false };
    c.retain(|(_, at)| at.elapsed() < CACHE_TTL);
    c.iter().any(|(p, _)| *p == pid)
}

fn remember(pid: u32) {
    if let Ok(mut c) = ALLOWED.lock() {
        if c.len() >= CACHE_MAX {
            c.remove(0);
        }
        c.push((pid, Instant::now()));
    }
}

/// The process that owns the TCP endpoint `ip:port` (IPv4).
fn owner_pid(ip: Ipv4Addr, port: u16) -> Option<u32> {
    // u32 elements: the table is a u32 count followed by rows of six u32.
    let mut buf: Vec<u32> = vec![0; 1024];
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
                let addr = u32::from_ne_bytes(ip.octets());
                return rows
                    .iter()
                    .find(|r| r.dwLocalAddr == addr && u16::from_be((r.dwLocalPort & 0xffff) as u16) == port)
                    .map(|r| r.dwOwningPid);
            }
            ERROR_INSUFFICIENT_BUFFER => buf.resize((size as usize).div_ceil(4) + 256, 0),
            _ => return None,
        }
    }
    None
}

/// Is `pid` a descendant of `me` (each link started after its parent)?
fn is_ours(pid: u32, me: u32) -> bool {
    let procs = super::procmem::snapshot();
    let Some(chain) = lb_core::memory::chain_to(pid, me, &procs) else { return false };
    let mut parent_start = match started(me) {
        Some(t) => t,
        None => return false,
    };
    // From the child of `me` down to `pid`.
    for p in chain.iter().rev() {
        match started(*p) {
            Some(t) if t >= parent_start => parent_start = t,
            _ => return false,
        }
    }
    true
}

/// When process `pid` was created (FILETIME ticks).
fn started(pid: u32) -> Option<u64> {
    // SAFETY: a query-only handle, closed after reading the times.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut c, mut e, mut k, mut u) =
            (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let r = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
        let _ = CloseHandle(h);
        r.ok()?;
        Some((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
    }
}
