//! Site icons fetched by the bookmark favicon probe (0.11.0) → the favicon store's format: a
//! 32×32 PNG of at most 48 KB. Windows Imaging Component decodes what sites serve (ICO with
//! PNG or BMP entries, PNG, GIF, JPEG, BMP, and WebP where the codec is installed); SVG is not
//! supported. Runs on a job thread (its own COM apartment).

use windows::core::GUID;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IStream, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    STREAM_SEEK_CUR,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

/// The side of a stored icon.
const SIZE: u32 = 32;
/// The favicon store takes PNGs up to this size (`storage::MAX_FAVICON_BYTES`).
const MAX_PNG: usize = 48 * 1024;

/// `bytes` (an image file) as a 32×32 PNG, or `None` if it is not an image WIC can read.
pub fn to_png32(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || looks_like_text(bytes) {
        return None;
    }
    // SAFETY: COM is initialised for this thread for the duration of the call and every COM
    // object is released (dropped) before CoUninitialize.
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let png = convert(bytes);
        if init.is_ok() {
            CoUninitialize();
        }
        png.filter(|p| p.len() <= MAX_PNG)
    }
}

/// An HTML error page served as `/favicon.ico` is not an image (and WIC need not see it).
fn looks_like_text(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]).trim_start().to_ascii_lowercase();
    head.starts_with("<!doctype") || head.starts_with("<html") || head.starts_with("<?xml") || head.starts_with("<svg")
}

unsafe fn convert(bytes: &[u8]) -> Option<Vec<u8>> {
    let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let input = factory.CreateStream().ok()?;
    input.InitializeFromMemory(bytes).ok()?;
    let decoder = factory.CreateDecoderFromStream(&input, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
    // An .ico has several sizes: the smallest one at least 32 px, else the largest.
    let count = decoder.GetFrameCount().ok()?.min(16);
    let mut best: Option<(u32, IWICBitmapFrameDecode)> = None;
    for i in 0..count {
        let Ok(frame) = decoder.GetFrame(i) else { continue };
        let (mut w, mut h) = (0, 0);
        if frame.GetSize(&mut w, &mut h).is_err() || w == 0 || h == 0 {
            continue;
        }
        let side = w.min(h);
        let better = match &best {
            None => true,
            Some((b, _)) => (side >= SIZE && (*b < SIZE || side < *b)) || (*b < SIZE && side > *b),
        };
        if better {
            best = Some((side, frame));
        }
    }
    let (_, frame) = best?;
    let converter = factory.CreateFormatConverter().ok()?;
    converter
        .Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;
    let scaler = factory.CreateBitmapScaler().ok()?;
    scaler.Initialize(&converter, SIZE, SIZE, WICBitmapInterpolationModeFant).ok()?;

    let out: IStream = CreateStreamOnHGlobal(Default::default(), true).ok()?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
    encoder.Initialize(&out, WICBitmapEncoderNoCache).ok()?;
    let mut target: Option<IWICBitmapFrameEncode> = None;
    encoder.CreateNewFrame(&mut target, std::ptr::null_mut()).ok()?;
    let target = target?;
    target.Initialize(None).ok()?;
    target.SetSize(SIZE, SIZE).ok()?;
    let mut format: GUID = GUID_WICPixelFormat32bppBGRA;
    target.SetPixelFormat(&mut format).ok()?;
    target.WriteSource(&scaler, std::ptr::null()).ok()?;
    target.Commit().ok()?;
    encoder.Commit().ok()?;

    let mut len = 0u64;
    out.Seek(0, STREAM_SEEK_CUR, Some(&mut len)).ok()?;
    let hglobal = GetHGlobalFromStream(&out).ok()?;
    let ptr = GlobalLock(hglobal) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let png = std::slice::from_raw_parts(ptr, len as usize).to_vec();
    let _ = GlobalUnlock(hglobal);
    Some(png)
}
