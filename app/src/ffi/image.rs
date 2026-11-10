//! "Copy image" (0.12.2): an image file (PNG, JPEG, GIF, BMP, ICO, TIFF, WebP and AVIF where
//! Windows has the codec) decoded with Windows Imaging Component into what the clipboard gets:
//! a DIB and a PNG of the first frame. Runs on a job thread (its own COM apartment).

use lb_core::image_menu::{self, MAX_COPY_PIXELS};
use windows::core::GUID;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IStream, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    STREAM_SEEK_CUR,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

/// An image ready for the clipboard.
pub struct ClipboardImage {
    /// `CF_DIB` contents.
    pub dib: Vec<u8>,
    /// The registered "PNG" format's contents (keeps transparency).
    pub png: Vec<u8>,
}

/// `bytes` decoded for the clipboard; an error says why not (not an image Windows can read,
/// too large).
pub fn for_clipboard(bytes: &[u8]) -> Result<ClipboardImage, String> {
    // SAFETY: COM is initialised for this thread for the duration of the call and every COM
    // object is released (dropped) before CoUninitialize.
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let r = decode(bytes);
        if init.is_ok() {
            CoUninitialize();
        }
        r
    }
}

unsafe fn decode(bytes: &[u8]) -> Result<ClipboardImage, String> {
    let not_image = || "Windows can't read this image format".to_string();
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
    let input = factory.CreateStream().map_err(|e| e.message())?;
    input.InitializeFromMemory(bytes).map_err(|e| e.message())?;
    let decoder = factory
        .CreateDecoderFromStream(&input, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
        .map_err(|_| not_image())?;
    let frame = decoder.GetFrame(0).map_err(|_| not_image())?;
    let (mut w, mut h) = (0u32, 0u32);
    frame.GetSize(&mut w, &mut h).map_err(|_| not_image())?;
    if w == 0 || h == 0 {
        return Err(not_image());
    }
    if w as u64 * h as u64 > MAX_COPY_PIXELS {
        return Err(format!("the image is too large to copy ({w}×{h})"));
    }
    let converter = factory.CreateFormatConverter().map_err(|e| e.message())?;
    converter
        .Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .map_err(|_| not_image())?;
    let stride = w * 4;
    let mut bgra = vec![0u8; stride as usize * h as usize];
    converter.CopyPixels(std::ptr::null(), stride, &mut bgra).map_err(|_| not_image())?;
    let dib = image_menu::dib(w, h, &bgra).ok_or_else(not_image)?;
    let png = encode_png(&factory, &converter, w, h).ok_or("the image could not be converted")?;
    Ok(ClipboardImage { dib, png })
}

unsafe fn encode_png(factory: &IWICImagingFactory, source: &IWICFormatConverter, w: u32, h: u32) -> Option<Vec<u8>> {
    let out: IStream = CreateStreamOnHGlobal(Default::default(), true).ok()?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
    encoder.Initialize(&out, WICBitmapEncoderNoCache).ok()?;
    let mut target: Option<IWICBitmapFrameEncode> = None;
    encoder.CreateNewFrame(&mut target, std::ptr::null_mut()).ok()?;
    let target = target?;
    target.Initialize(None).ok()?;
    target.SetSize(w, h).ok()?;
    let mut format: GUID = GUID_WICPixelFormat32bppBGRA;
    target.SetPixelFormat(&mut format).ok()?;
    target.WriteSource(source, std::ptr::null()).ok()?;
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
