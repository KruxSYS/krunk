//! Memory of the browser's process tree (ToolHelp32 + GetProcessMemoryInfo).

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};

/// Memory of one process.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcMem {
    pub working_set: u64,
    pub private: u64,
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

/// This process and all its descendants (the WebView2 browser, renderer,
/// GPU and utility processes started for our user data folder).
pub fn browser_tree() -> Vec<ProcMem> {
    let me = std::process::id();
    let mut pids = lb_core::memory::descendants(me, &snapshot());
    pids.push(me);
    pids.into_iter().map(memory).collect()
}
