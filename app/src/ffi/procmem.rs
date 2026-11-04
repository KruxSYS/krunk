//! Memory of the browser's process tree (ToolHelp32 + GetProcessMemoryInfo)
//! and the kind of each engine process (from its `--type=` switch).

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};

/// What an engine process does (its `--type=` switch).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    /// No `--type=`: the browser process (us).
    #[default]
    Browser,
    Renderer,
    Gpu,
    Utility,
    Other,
}

impl Kind {
    pub fn from_command_line(cl: &str) -> Kind {
        let Some(rest) = cl.split(" --type=").nth(1) else { return Kind::Browser };
        match rest.split_whitespace().next().unwrap_or("") {
            "renderer" => Kind::Renderer,
            "gpu-process" => Kind::Gpu,
            "utility" => Kind::Utility,
            _ => Kind::Other,
        }
    }
}

/// Memory of one process.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcMem {
    pub working_set: u64,
    pub private: u64,
    pub kind: Kind,
}

/// `(pid, parent pid)` for every process on the system.
pub fn snapshot() -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    // SAFETY: standard ToolHelp32 enumeration with a correctly sized entry;
    // the snapshot handle is closed afterwards.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                out.push((e.th32ProcessID, e.th32ParentProcessID));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Working set and private bytes of a process (zero if it cannot be read).
pub fn memory(pid: u32) -> ProcMem {
    let mut m = ProcMem::default();
    // SAFETY: query-only handle, counters struct of the declared size, handle closed.
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid) {
            let mut c = PROCESS_MEMORY_COUNTERS_EX {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                ..Default::default()
            };
            if GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb).is_ok() {
                m.working_set = c.WorkingSetSize as u64;
                m.private = c.PrivateUsage as u64;
            }
            let _ = CloseHandle(h);
        }
    }
    m
}

/// The command line of another process (`NtQueryInformationProcess`,
/// `ProcessCommandLineInformation`).
pub fn command_line(pid: u32) -> Option<String> {
    use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
    use windows::Win32::Foundation::UNICODE_STRING;
    const PROCESS_COMMAND_LINE_INFORMATION: PROCESSINFOCLASS = PROCESSINFOCLASS(60);
    // SAFETY: query-only handle; the buffer is u64-aligned for the
    // UNICODE_STRING header, its size comes from the first call, and the
    // string is read within the buffer the kernel filled. Handle closed.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut len = 0u32;
        let _ = NtQueryInformationProcess(h, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(2)];
        let status = NtQueryInformationProcess(
            h,
            PROCESS_COMMAND_LINE_INFORMATION,
            buf.as_mut_ptr().cast(),
            (buf.len() * 8) as u32,
            &mut len,
        );
        let _ = CloseHandle(h);
        if status.is_err() {
            return None;
        }
        let us = &*(buf.as_ptr() as *const UNICODE_STRING);
        if us.Buffer.is_null() {
            return None;
        }
        let chars = std::slice::from_raw_parts(us.Buffer.0, usize::from(us.Length) / 2);
        Some(String::from_utf16_lossy(chars))
    }
}

/// This process and all its descendants (the engine's renderer, GPU and
/// utility processes).
pub fn browser_tree() -> Vec<ProcMem> {
    let me = std::process::id();
    let mut pids = lb_core::memory::descendants(me, &snapshot());
    pids.push(me);
    pids.into_iter()
        .map(|pid| ProcMem {
            kind: command_line(pid).map(|c| Kind::from_command_line(&c)).unwrap_or(Kind::Other),
            ..memory(pid)
        })
        .collect()
}
