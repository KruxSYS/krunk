//! framecap: records every frame the desktop compositor presents inside a screen rectangle,
//! with its presentation time, for frame-by-frame checks on the test VM (tab-switch flashes,
//! animation smoothness). Not shipped with Krunk.
//!
//!   framecap --rect X,Y,W,H [--step N] [--seconds S] [--gdi] --out PATH
//!
//! Writes PATH.raw (frames of W/N x H/N RGB bytes, back to back), PATH.csv (frame, ms since
//! start, source) and PATH.json (geometry and method). Desktop Duplication delivers only frames
//! that changed; the GDI fallback polls BitBlt and drops frames equal to the previous one.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

struct Args {
    rect: (i32, i32, i32, i32),
    step: i32,
    seconds: f64,
    gdi: bool,
    out: String,
}

fn parse_args() -> Result<Args, String> {
    let mut rect = None;
    let mut step = 1;
    let mut seconds = 10.0;
    let mut gdi = false;
    let mut out = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rect" => {
                let v: Vec<i32> = it
                    .next()
                    .ok_or("--rect needs X,Y,W,H")?
                    .split(',')
                    .map(|s| s.trim().parse().map_err(|_| "bad --rect"))
                    .collect::<Result<_, _>>()?;
                if v.len() != 4 || v[2] <= 0 || v[3] <= 0 {
                    return Err("--rect needs X,Y,W,H".into());
                }
                rect = Some((v[0], v[1], v[2], v[3]));
            }
            "--step" => step = it.next().and_then(|s| s.parse().ok()).filter(|&s| s >= 1).ok_or("bad --step")?,
            "--seconds" => seconds = it.next().and_then(|s| s.parse().ok()).ok_or("bad --seconds")?,
            "--gdi" => gdi = true,
            "--out" => out = it.next(),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    Ok(Args { rect: rect.ok_or("--rect is required")?, step, seconds, gdi, out: out.ok_or("--out is required")? })
}

/// Collects sampled RGB frames and their times.
struct Sink {
    raw: BufWriter<File>,
    csv: BufWriter<File>,
    frames: u64,
    last: Vec<u8>,
}

impl Sink {
    fn push(&mut self, ms: f64, rgb: &[u8], source: &str) -> std::io::Result<()> {
        self.raw.write_all(rgb)?;
        writeln!(self.csv, "{},{:.3},{}", self.frames, ms, source)?;
        self.frames += 1;
        self.last.clear();
        self.last.extend_from_slice(rgb);
        Ok(())
    }
}

fn qpc() -> i64 {
    let mut v = 0;
    unsafe { QueryPerformanceCounter(&mut v).ok() };
    v
}

fn qpf() -> f64 {
    let mut v = 0;
    unsafe { QueryPerformanceFrequency(&mut v).ok() };
    v as f64
}

/// Samples a BGRA image (`pitch` bytes per row, origin at `ox,oy`) into RGB with `step`.
fn sample(src: &[u8], pitch: usize, ox: i32, oy: i32, a: &Args, out: &mut Vec<u8>) {
    out.clear();
    let (_, _, w, h) = a.rect;
    let mut y = 0;
    while y < h {
        let row = (oy + y) as usize * pitch;
        let mut x = 0;
        while x < w {
            let p = row + (ox + x) as usize * 4;
            out.extend_from_slice(&[src[p + 2], src[p + 1], src[p]]);
            x += a.step;
        }
        y += a.step;
    }
}

fn find_output(rect: (i32, i32, i32, i32)) -> windows::core::Result<(IDXGIAdapter1, IDXGIOutput1, RECT)> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        let mut j = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(j) } {
            let desc = unsafe { output.GetDesc()? };
            let r = desc.DesktopCoordinates;
            if rect.0 >= r.left && rect.1 >= r.top && rect.0 + rect.2 <= r.right && rect.1 + rect.3 <= r.bottom {
                return Ok((adapter, output.cast()?, r));
            }
            j += 1;
        }
        i += 1;
    }
    Err(windows::core::Error::new(windows::core::HRESULT(-1), "no output contains the rectangle"))
}

fn capture_dxgi(a: &Args, sink: &mut Sink) -> windows::core::Result<()> {
    let (adapter, output, desktop) = find_output(a.rect)?;
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?
    };
    let (device, context) = (device.unwrap(), context.unwrap());
    let dup: IDXGIOutputDuplication = unsafe { output.DuplicateOutput(&device)? };
    let freq = qpf();
    let start = qpc();
    let deadline = Instant::now() + Duration::from_secs_f64(a.seconds);
    let mut staging: Option<ID3D11Texture2D> = None;
    let mut rgb = Vec::new();
    let (ox, oy) = (a.rect.0 - desktop.left, a.rect.1 - desktop.top);
    while Instant::now() < deadline {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        match unsafe { dup.AcquireNextFrame(50, &mut info, &mut res) } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
            Err(e) => return Err(e),
        }
        // A pointer-only update has no new desktop image.
        if info.LastPresentTime == 0 {
            unsafe { dup.ReleaseFrame()? };
            continue;
        }
        let tex: ID3D11Texture2D = res.unwrap().cast()?;
        if staging.is_none() {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut desc) };
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut t = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut t))? };
            staging = t;
        }
        let st = staging.as_ref().unwrap();
        unsafe { context.CopyResource(st, &tex) };
        unsafe { dup.ReleaseFrame()? };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(st, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let pitch = mapped.RowPitch as usize;
        let rows = (desktop.bottom - desktop.top) as usize;
        let src = unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, pitch * rows) };
        sample(src, pitch, ox, oy, a, &mut rgb);
        unsafe { context.Unmap(st, 0) };
        let ms = (info.LastPresentTime - start) as f64 * 1000.0 / freq;
        sink.push(ms, &rgb, "dxgi").map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    }
    Ok(())
}

fn capture_gdi(a: &Args, sink: &mut Sink) -> std::io::Result<()> {
    let (x, y, w, h) = a.rect;
    let freq = qpf();
    let start = qpc();
    let deadline = Instant::now() + Duration::from_secs_f64(a.seconds);
    let mut rgb = Vec::new();
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0).map_err(std::io::Error::other)?;
        let old = SelectObject(mem, dib.into());
        while Instant::now() < deadline {
            let t = qpc();
            if BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY).is_err() {
                continue;
            }
            let src = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
            sample(src, (w * 4) as usize, 0, 0, a, &mut rgb);
            if rgb != sink.last {
                sink.push((t - start) as f64 * 1000.0 / freq, &rgb, "gdi")?;
            }
        }
        SelectObject(mem, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
    Ok(())
}

fn main() {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("framecap: {e}\nusage: framecap --rect X,Y,W,H [--step N] [--seconds S] [--gdi] --out PATH");
            std::process::exit(2);
        }
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let open = |ext: &str| BufWriter::new(File::create(format!("{}.{ext}", a.out)).expect("cannot create output"));
    let mut sink = Sink { raw: open("raw"), csv: open("csv"), frames: 0, last: Vec::new() };
    writeln!(sink.csv, "frame,ms,source").unwrap();
    let mut method = "dxgi";
    if a.gdi {
        method = "gdi";
    } else if let Err(e) = capture_dxgi(&a, &mut sink) {
        if sink.frames > 0 {
            eprintln!("framecap: desktop duplication stopped: {e}");
        } else {
            eprintln!("framecap: desktop duplication unavailable ({e}); using GDI");
            method = "gdi";
        }
    }
    if method == "gdi" {
        capture_gdi(&a, &mut sink).expect("GDI capture failed");
    }
    sink.raw.flush().unwrap();
    sink.csv.flush().unwrap();
    let (sw, sh) = ((a.rect.2 + a.step - 1) / a.step, (a.rect.3 + a.step - 1) / a.step);
    let meta = format!(
        "{{\"rect\":[{},{},{},{}],\"step\":{},\"width\":{sw},\"height\":{sh},\"frames\":{},\"method\":\"{method}\",\"seconds\":{}}}\n",
        a.rect.0, a.rect.1, a.rect.2, a.rect.3, a.step, sink.frames, a.seconds
    );
    std::fs::write(format!("{}.json", a.out), meta).unwrap();
    println!("framecap: {} frames ({method})", sink.frames);
}
