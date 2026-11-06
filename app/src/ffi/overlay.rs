//! Overlay windows: the link-status label and the "Press Esc" hint. A layered,
//! click-through, non-activating window owned by a frame, drawn with GDI into a 32-bit
//! bitmap with real alpha (`UpdateLayeredWindow`): it can be rounded and
//! semi-transparent above the page, never takes the focus, and lets clicks through. The
//! colours, placement, elision and fades are `lb_core::overlay`.

use super::window::Window;
use lb_core::overlay::{premultiplied, rounded_mask, Palette, Rgba, Run};
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DeleteDC, DeleteObject,
    GetTextExtentPoint32W, RoundRect, SelectObject, SetBkMode, SetTextColor, TextOutW, AC_SRC_ALPHA, AC_SRC_OVER,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, FW_NORMAL, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, PS_SOLID,
    TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, IsWindowVisible, ShowWindow, UpdateLayeredWindow, SW_HIDE, SW_SHOWNOACTIVATE,
    ULW_ALPHA, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS: PCWSTR = w!("KrunkOverlay");

/// What to draw: text runs (plain, or a key cap) on a rounded background.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlaySpec {
    pub palette: Palette,
    pub runs: Vec<Run>,
    /// Text size and the padding around the text, in physical pixels.
    pub font_px: i32,
    pub pad_x: i32,
    pub pad_y: i32,
    pub radius: f64,
    /// Border width in pixels (0 = none).
    pub border: i32,
}

/// A layered overlay window.
pub struct Overlay {
    hwnd: HWND,
    last: RefCell<Option<(OverlaySpec, i32, i32)>>,
}

impl Overlay {
    /// Creates the (hidden) overlay owned by `owner`.
    pub fn create(owner: Window) -> Result<Overlay, String> {
        static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        REGISTERED.get_or_init(|| super::window::register_plain_class(CLASS)).clone()?;
        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        // SAFETY: creating an owned popup window on the UI thread with a registered class.
        let hwnd = unsafe {
            let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).map_err(|e| e.to_string())?;
            CreateWindowExW(
                ex,
                CLASS,
                w!("Krunk overlay"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                Some(owner.hwnd()),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW (overlay) failed: {e}"))?
        };
        Ok(Overlay { hwnd, last: RefCell::new(None) })
    }

    /// The width in pixels of `text` at `font_px` (the font the overlay draws with).
    pub fn measure(&self, text: &str, font_px: i32) -> i32 {
        // SAFETY: a memory DC of our own for measuring; every object is released.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let font = make_font(font_px);
            let old = SelectObject(dc, HGDIOBJ(font.0));
            let w = text_width(dc, text);
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteDC(dc);
            w
        }
    }

    /// The size the spec needs (width, height) in pixels.
    pub fn size_of(&self, spec: &OverlaySpec) -> (i32, i32) {
        let (runs_w, _) = self.layout_runs(spec);
        (runs_w + 2 * spec.pad_x, spec.font_px + 2 * spec.pad_y)
    }

    /// Draws `spec` and shows the overlay with its top-left corner at `(x, y)` (screen
    /// pixels) with the whole layer at `alpha` (0..255). Never activates.
    pub fn show(&self, spec: &OverlaySpec, x: i32, y: i32, alpha: u8) {
        *self.last.borrow_mut() = Some((spec.clone(), x, y));
        self.render(spec, x, y, alpha);
    }

    /// Re-shows the last drawing at another opacity (fades).
    pub fn set_alpha(&self, alpha: u8) {
        if let Some((spec, x, y)) = self.last.borrow().clone() {
            self.render(&spec, x, y, alpha);
        }
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: valid window handle.
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    pub fn hide(&self) {
        *self.last.borrow_mut() = None;
        // SAFETY: valid window handle.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn layout_runs(&self, spec: &OverlaySpec) -> (i32, Vec<(Run, i32, i32)>) {
        // Each run: its x offset and width (key caps have their own padding).
        let mut placed = Vec::new();
        let mut x = 0;
        for run in &spec.runs {
            let (text, extra) = match run {
                Run::Text(t) => (t.as_str(), 0),
                Run::Key(k) => (k.as_str(), spec.font_px / 2 * 2 + 6),
            };
            let w = self.measure(text, spec.font_px) + extra;
            placed.push((run.clone(), x, w));
            x += w;
        }
        (x, placed)
    }

    fn render(&self, spec: &OverlaySpec, x: i32, y: i32, alpha: u8) {
        let (width, height) = self.size_of(spec);
        let (width, height) = (width.max(1), height.max(1));
        let (_, runs) = self.layout_runs(spec);
        let mask = rounded_mask(width, height, spec.radius);
        let Palette { bg, fg, border, keycap_bg, keycap_border } = spec.palette;
        // SAFETY: GDI drawing into our own DIB; every handle is released; the pixel buffer
        // is exactly width*height*4 bytes as CreateDIBSection allocates it.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let Ok(bmp): Result<HBITMAP, _> = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(dc);
                return;
            };
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
                let _ = DeleteDC(dc);
                return;
            }
            let old_bmp = SelectObject(dc, HGDIOBJ(bmp.0));
            let n = (width * height) as usize;
            let px = std::slice::from_raw_parts_mut(bits as *mut [u8; 4], n);
            // Opaque background (and border ring) first; the alpha is applied at the end.
            let inner = if spec.border > 0 {
                let b = spec.border;
                let mut m = vec![0u8; n];
                let im = rounded_mask(width - 2 * b, height - 2 * b, (spec.radius - f64::from(b)).max(0.0));
                for yy in 0..(height - 2 * b).max(0) {
                    for xx in 0..(width - 2 * b).max(0) {
                        m[((yy + b) * width + xx + b) as usize] = im[(yy * (width - 2 * b) + xx) as usize];
                    }
                }
                Some(m)
            } else {
                None
            };
            for (i, p) in px.iter_mut().enumerate() {
                let c = match &inner {
                    Some(m) if m[i] < 255 => blend(border, bg, m[i]),
                    _ => bg,
                };
                *p = [c.2, c.1, c.0, 255];
            }
            // Text and key caps.
            let font = make_font(spec.font_px);
            let old_font = SelectObject(dc, HGDIOBJ(font.0));
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, colorref(fg));
            let text_y = spec.pad_y;
            for (run, rx, rw) in &runs {
                let x0 = spec.pad_x + rx;
                match run {
                    Run::Text(t) => {
                        let wide: Vec<u16> = t.encode_utf16().collect();
                        let _ = TextOutW(dc, x0, text_y, &wide);
                    }
                    Run::Key(k) => {
                        let pen = CreatePen(PS_SOLID, 1, colorref(blend(keycap_border, bg, 255)));
                        let brush = CreateSolidBrush(colorref(blend(keycap_bg, bg, 255)));
                        let old_pen = SelectObject(dc, HGDIOBJ(pen.0));
                        let old_brush = SelectObject(dc, HGDIOBJ(brush.0));
                        let top = text_y - 3;
                        let _ = RoundRect(dc, x0, top, x0 + rw, top + spec.font_px + 6, 6, 6);
                        SelectObject(dc, old_pen);
                        SelectObject(dc, old_brush);
                        let _ = DeleteObject(HGDIOBJ(pen.0));
                        let _ = DeleteObject(HGDIOBJ(brush.0));
                        let wide: Vec<u16> = k.encode_utf16().collect();
                        let inset = (rw - text_width(dc, k)) / 2;
                        let _ = TextOutW(dc, x0 + inset, text_y, &wide);
                    }
                }
            }
            SelectObject(dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
            // GDI leaves the alpha byte at 0 where it drew: set every pixel's alpha from the
            // rounded mask and the background's own alpha, premultiplied.
            for (i, p) in px.iter_mut().enumerate() {
                let colour = (p[2], p[1], p[0]);
                *p = premultiplied(colour, mask[i], bg.3);
            }
            let blend_fn = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: alpha,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let dst = POINT { x, y };
            let size = SIZE { cx: width, cy: height };
            let src = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&dst),
                Some(&size),
                Some(dc),
                Some(&src),
                COLORREF(0),
                Some(&blend_fn),
                ULW_ALPHA,
            );
            SelectObject(dc, old_bmp);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(dc);
            if !IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        // SAFETY: destroying our own window.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// `top` over `bottom` at `alpha` (straight alpha blend of the colour channels).
fn blend(top: Rgba, bottom: Rgba, alpha: u8) -> Rgba {
    let a = u32::from(top.3) * u32::from(alpha) / 255;
    let mix = |t: u8, b: u8| ((u32::from(t) * a + u32::from(b) * (255 - a)) / 255) as u8;
    Rgba(mix(top.0, bottom.0), mix(top.1, bottom.1), mix(top.2, bottom.2), 255)
}

fn colorref(c: Rgba) -> COLORREF {
    COLORREF(u32::from(c.0) | (u32::from(c.1) << 8) | (u32::from(c.2) << 16))
}

/// Segoe UI at `px` pixels, grayscale anti-aliased (ClearType needs an opaque background).
unsafe fn make_font(px: i32) -> HFONT {
    CreateFontW(
        -px,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        ANTIALIASED_QUALITY,
        DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
        w!("Segoe UI"),
    )
}

unsafe fn text_width(dc: HDC, text: &str) -> i32 {
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return 0;
    }
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
    size.cx
}
