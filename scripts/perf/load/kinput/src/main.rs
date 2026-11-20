//! kinput: SendInput steps for the 0.26.0 page-load harness (Windows only, runs on the test VM).
//!
//!   kinput.exe STEP...
//!     key:ctrl+l | key:enter | key:alt+left | key:f5   one key chord (down in order, up reversed)
//!     text:https://example.org/                        Unicode text, one character at a time
//!     click:X,Y                                        left click at screen pixels
//!     sleep:MS
//!     max:0                                            maximize the foreground window
//!     focus:chrome.exe                                 bring that program's largest visible
//!                                                      top-level window to the front, maximized
//!
//! After each step it prints `STEP<TAB>EPOCH_MS<TAB>QPC` with the wall-clock time (precise system time,
//! ms with µs fraction) taken right after the step's last event was injected: the same clock as
//! a page's `performance.timeOrigin`, so the driver can compare input and page timings directly,
//! and the QPC count (framecap's time line).

#[cfg(windows)]
mod win {
    use std::mem::size_of;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyInput {
        vk: u16,
        scan: u16,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MouseInput {
        dx: i32,
        dy: i32,
        data: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    union InputU {
        ki: KeyInput,
        mi: MouseInput,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Input {
        kind: u32,
        u: InputU,
    }
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn SendInput(n: u32, inputs: *const Input, size: i32) -> u32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
        fn SetProcessDPIAware() -> i32;
        fn GetForegroundWindow() -> isize;
        fn EnumWindows(cb: extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn GetWindowRect(hwnd: isize, rect: *mut [i32; 4]) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn GetWindow(hwnd: isize, cmd: u32) -> isize;
        fn SetForegroundWindow(hwnd: isize) -> i32;
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemTimePreciseAsFileTime(ft: *mut FileTime);
        fn QueryPerformanceCounter(count: *mut i64) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(h: isize, flags: u32, buf: *mut u16, len: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }

    const KEYEVENTF_KEYUP: u32 = 0x2;
    const KEYEVENTF_UNICODE: u32 = 0x4;
    const MOUSEEVENTF_LEFTDOWN: u32 = 0x2;
    const MOUSEEVENTF_LEFTUP: u32 = 0x4;

    fn key(vk: u16, scan: u16, flags: u32) -> Input {
        Input { kind: 1, u: InputU { ki: KeyInput { vk, scan, flags, time: 0, extra: 0 } } }
    }
    fn mouse(flags: u32) -> Input {
        Input { kind: 0, u: InputU { mi: MouseInput { dx: 0, dy: 0, data: 0, flags, time: 0, extra: 0 } } }
    }
    fn send(v: &[Input]) {
        unsafe { SendInput(v.len() as u32, v.as_ptr(), size_of::<Input>() as i32) };
    }

    pub fn epoch_ms() -> f64 {
        let mut ft = FileTime::default();
        unsafe { GetSystemTimePreciseAsFileTime(&mut ft) };
        let t = ((ft.high as u64) << 32) | ft.low as u64; // 100 ns since 1601
        (t as f64 / 10_000.0) - 11_644_473_600_000.0
    }

    fn image_name(hwnd: isize) -> String {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let h = unsafe { OpenProcess(0x1000, 0, pid) }; // PROCESS_QUERY_LIMITED_INFORMATION
        if h == 0 {
            return String::new();
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) };
        unsafe { CloseHandle(h) };
        if ok == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or("").to_ascii_lowercase()
    }

    static mut FOUND: Vec<(isize, i64)> = Vec::new();
    extern "system" fn each(hwnd: isize, _: isize) -> i32 {
        // Visible, unowned top-level windows only.
        if unsafe { IsWindowVisible(hwnd) } != 0 && unsafe { GetWindow(hwnd, 4) } == 0 {
            let mut r = [0i32; 4];
            unsafe { GetWindowRect(hwnd, &mut r) };
            let area = (r[2] - r[0]) as i64 * (r[3] - r[1]) as i64;
            #[allow(static_mut_refs)]
            unsafe {
                FOUND.push((hwnd, area))
            };
        }
        1
    }

    fn focus(exe: &str) -> Result<(), String> {
        #[allow(static_mut_refs)]
        unsafe {
            FOUND.clear();
            EnumWindows(each, 0);
        }
        #[allow(static_mut_refs)]
        let all: Vec<(isize, i64)> = unsafe { FOUND.clone() };
        let best = all
            .into_iter()
            .filter(|(h, area)| *area > 200 * 200 && image_name(*h) == exe.to_ascii_lowercase())
            .max_by_key(|(_, a)| *a)
            .ok_or_else(|| format!("no window of {exe}"))?;
        // An Alt tap first: Windows lets the process that sent the last input take the foreground.
        send(&[key(0x12, 0, 0), key(0x12, 0, KEYEVENTF_KEYUP)]);
        unsafe {
            ShowWindow(best.0, 3);
            SetForegroundWindow(best.0);
        }
        Ok(())
    }

    pub fn qpc() -> i64 {
        let mut c = 0i64;
        unsafe { QueryPerformanceCounter(&mut c) };
        c
    }

    fn vk_of(name: &str) -> Option<u16> {
        Some(match name {
            "ctrl" => 0x11,
            "alt" => 0x12,
            "shift" => 0x10,
            "enter" => 0x0D,
            "esc" => 0x1B,
            "tab" => 0x09,
            "left" => 0x25,
            "up" => 0x26,
            "right" => 0x27,
            "down" => 0x28,
            "home" => 0x24,
            "end" => 0x23,
            "f5" => 0x74,
            "f6" => 0x75,
            s if s.len() == 1 => s.as_bytes()[0].to_ascii_uppercase() as u16,
            _ => return None,
        })
    }

    pub fn init() {
        unsafe { SetProcessDPIAware() };
    }

    pub fn step(s: &str) -> Result<(), String> {
        let (kind, arg) = s.split_once(':').ok_or_else(|| format!("bad step {s}"))?;
        match kind {
            "key" => {
                let vks: Vec<u16> =
                    arg.split('+').map(|k| vk_of(k).ok_or_else(|| format!("unknown key {k}"))).collect::<Result<_, _>>()?;
                let mut v: Vec<Input> = vks.iter().map(|&vk| key(vk, 0, 0)).collect();
                v.extend(vks.iter().rev().map(|&vk| key(vk, 0, KEYEVENTF_KEYUP)));
                send(&v);
            }
            "text" => {
                for u in arg.encode_utf16() {
                    send(&[key(0, u, KEYEVENTF_UNICODE), key(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]);
                }
            }
            "click" => {
                let (x, y) = arg.split_once(',').ok_or("click:X,Y")?;
                let (x, y): (i32, i32) = (x.trim().parse().map_err(|_| "x")?, y.trim().parse().map_err(|_| "y")?);
                unsafe { SetCursorPos(x, y) };
                std::thread::sleep(std::time::Duration::from_millis(30));
                send(&[mouse(MOUSEEVENTF_LEFTDOWN), mouse(MOUSEEVENTF_LEFTUP)]);
            }
            "move" => {
                let (x, y) = arg.split_once(',').ok_or("move:X,Y")?;
                unsafe { SetCursorPos(x.trim().parse().map_err(|_| "x")?, y.trim().parse().map_err(|_| "y")?) };
            }
            "focus" => focus(arg)?,
            "max" => {
                unsafe { ShowWindow(GetForegroundWindow(), 3) };
            }
            "sleep" => std::thread::sleep(std::time::Duration::from_millis(arg.parse().map_err(|_| "ms")?)),
            _ => return Err(format!("unknown step {s}")),
        }
        Ok(())
    }
}

#[cfg(windows)]
fn main() {
    win::init();
    for s in std::env::args().skip(1) {
        if let Err(e) = win::step(&s) {
            eprintln!("kinput: {e}");
            std::process::exit(2);
        }
        let (t, q) = (win::epoch_ms(), win::qpc());
        println!("{s}\t{t:.3}\t{q}");
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("kinput runs on Windows only");
    std::process::exit(1);
}
