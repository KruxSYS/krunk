//! The page context menu's image items (0.12.2): "Open image in new tab", "Save image as…",
//! "Copy image" and "Copy image address", as in Chrome. The host fetches and decodes the image
//! (`app`); what is decided without Windows is here: the bytes of a `data:` image, the file
//! name a saved image gets, and the bitmap a copied image puts on the clipboard.

use base64ct::{Base64, Encoding};

/// The largest image "Copy image" fetches.
pub const MAX_COPY_BYTES: usize = 64 * 1024 * 1024;
/// The largest decoded image it puts on the clipboard (pixels).
pub const MAX_COPY_PIXELS: u64 = 40_000_000;

/// The image items, in the menu's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAction {
    OpenInNewTab,
    Save,
    Copy,
    CopyAddress,
}

impl ImageAction {
    pub const ALL: [ImageAction; 4] =
        [ImageAction::OpenInNewTab, ImageAction::Save, ImageAction::Copy, ImageAction::CopyAddress];

    pub fn label(self) -> &'static str {
        match self {
            ImageAction::OpenInNewTab => "Open image in new tab",
            ImageAction::Save => "Save image as…",
            ImageAction::Copy => "Copy image",
            ImageAction::CopyAddress => "Copy image address",
        }
    }

    /// Does the AI panel's menu offer this item (0.16.0)? Its pictures are the agent's step
    /// screenshots, which the host has itself: they can be saved and copied. Their addresses
    /// are on the shell origin, which no tab reaches, so there is nothing to open in a tab and
    /// no address worth copying.
    pub fn in_panel(self) -> bool {
        matches!(self, ImageAction::Save | ImageAction::Copy)
    }
}

/// Is this an image the AI panel's menu has items for (0.16.0)? The pictures the panel shows:
/// a step screenshot on the shell origin (`/agent-shot/…`) or a `data:` image. The host has
/// the bytes of both; it could not get any other for the panel.
pub fn panel_image(url: &str) -> bool {
    crate::assets::agent_shot_request(url).is_some() || data_url(url).is_some()
}

/// Can the host get this image itself (for Copy and Save)? Web and file images and `data:`
/// ones; a `blob:` URL lives only in its page.
pub fn fetchable(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file:")
}

/// The MIME type and bytes of a `data:` URL (base64 or percent-encoded), `None` for anything
/// else or a broken one.
pub fn data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.get(..5).filter(|p| p.eq_ignore_ascii_case("data:")).map(|_| &url[5..])?;
    let (meta, payload) = rest.split_once(',')?;
    let mut parts = meta.split(';');
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let base64 = parts.any(|p| p.trim().eq_ignore_ascii_case("base64"));
    let bytes = if base64 {
        let decoded = percent_decode(payload);
        let clean: Vec<u8> = decoded.into_iter().filter(|b| !b.is_ascii_whitespace()).collect();
        let clean = std::str::from_utf8(&clean).ok()?;
        // Padding is optional in data: URLs.
        let padded = format!("{clean}{}", "=".repeat((4 - clean.len() % 4) % 4));
        Base64::decode_vec(&padded).ok()?
    } else {
        percent_decode(payload)
    };
    Some((if mime.is_empty() { "text/plain".into() } else { mime }, bytes))
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// The file extension for an image MIME type.
pub fn extension_for(mime: &str) -> Option<&'static str> {
    Some(match mime.split(';').next()?.trim().to_ascii_lowercase().as_str() {
        "image/png" | "image/apng" => "png",
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/bmp" | "image/x-ms-bmp" => "bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/tiff" => "tif",
        _ => return None,
    })
}

/// The name "Save image as…" suggests: the engine's suggestion (from the server or the
/// address), else the address's last segment, else "image"; with the MIME type's extension
/// when it has none.
pub fn file_name(suggested: &str, url: &str, mime: &str) -> String {
    let from_url = || {
        url::Url::parse(url)
            .ok()
            .filter(|u| u.scheme() != "data")
            .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(str::to_string)))
            .map(|s| String::from_utf8_lossy(&percent_decode(&s)).into_owned())
            .filter(|s| !s.trim().is_empty())
    };
    let mut name = Some(suggested.trim().to_string())
        .filter(|s| !s.is_empty() && s != "download")
        .or_else(from_url)
        .unwrap_or_else(|| "image".into());
    let has_ext = name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()));
    if !has_ext {
        if let Some(ext) = extension_for(mime) {
            name = format!("{name}.{ext}");
        }
    }
    name
}

/// A clipboard DIB (`CF_DIB`: BITMAPINFOHEADER, 32 bits, bottom-up) of a `width`×`height`
/// image given as top-down, straight-alpha BGRA. Transparent parts are put on white: most
/// programs ignore a DIB's alpha (the clipboard's PNG keeps it). `None` if the sizes don't fit.
pub fn dib(width: u32, height: u32, bgra: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let row = w.checked_mul(4)?;
    if w == 0 || h == 0 || bgra.len() != row.checked_mul(h)? {
        return None;
    }
    let mut out = Vec::with_capacity(40 + bgra.len());
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes()); // positive: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(bgra.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution, colours
    for y in (0..h).rev() {
        for px in bgra[y * row..(y + 1) * row].chunks_exact(4) {
            let a = px[3] as u32;
            let over_white = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            out.extend_from_slice(&[over_white(px[0]), over_white(px[1]), over_white(px[2]), 255]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls() {
        let (mime, bytes) = data_url("data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"\x89PNG\r\n\x1a\n");
        // Unpadded, with whitespace and percent-escapes, upper-case scheme.
        assert_eq!(data_url("DATA:image/gif;base64,R0lG%0AODlh").unwrap().1, b"GIF89a");
        assert_eq!(
            data_url("data:image/svg+xml,%3Csvg%3E%3C/svg%3E").unwrap(),
            ("image/svg+xml".into(), b"<svg></svg>".to_vec())
        );
        assert_eq!(data_url("data:,hi").unwrap().0, "text/plain");
        assert!(data_url("https://a.example/x.png").is_none());
        assert!(data_url("data:image/png;base64").is_none());
        assert!(data_url("data:image/png;base64,***").is_none());
        // A trailing lone '%' is kept as it is.
        assert_eq!(percent_decode("a%2"), b"a%2");
        assert_eq!(percent_decode("%41%"), b"A%");
    }

    #[test]
    fn fetchable_urls() {
        assert!(
            fetchable("https://a.example/x.png") && fetchable("HTTP://a.example/") && fetchable("file:///C:/x.png")
        );
        assert!(!fetchable("blob:https://a.example/123") && !fetchable("data:image/png;base64,AA") && !fetchable(""));
    }

    #[test]
    fn panel_images_and_items() {
        assert!(panel_image("https://shell.krunk.invalid/agent-shot/0123456789abcdef0123456789abcdef/3.jpg"));
        assert!(panel_image("data:image/png;base64,iVBORw0KGgo="));
        for other in [
            "https://a.example/x.png",
            "https://krunk.invalid/agent-shot/ab/3.jpg",
            "https://shell.krunk.invalid/shared/logo.png",
            "blob:https://shell.krunk.invalid/1234",
            "data:image/png;base64,***",
            "",
        ] {
            assert!(!panel_image(other), "{other}");
        }
        let offered: Vec<ImageAction> = ImageAction::ALL.into_iter().filter(|a| a.in_panel()).collect();
        assert_eq!(offered, [ImageAction::Save, ImageAction::Copy]);
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("cat.jpg", "https://a.example/i?id=1", "image/jpeg"), "cat.jpg");
        assert_eq!(file_name("", "https://a.example/img/photo%20one.webp?x=1", "image/webp"), "photo one.webp");
        assert_eq!(file_name("", "https://a.example/render?id=5", "image/png"), "render.png");
        assert_eq!(file_name("download", "data:image/png;base64,AA", "image/png"), "image.png");
        assert_eq!(file_name("", "https://a.example/", "image/gif"), "image.gif");
        assert_eq!(file_name("", "https://a.example/x", "application/octet-stream"), "x");
        assert_eq!(file_name(".hidden", "https://a.example/", "image/png"), ".hidden.png");
        assert_eq!(extension_for("image/jpeg; charset=binary"), Some("jpg"));
    }

    #[test]
    fn dib_is_bottom_up_on_white() {
        // 2×2: top row opaque blue, transparent; bottom row half-transparent black, opaque red.
        let bgra = [255, 0, 0, 255, 9, 9, 9, 0, 0, 0, 0, 128, 0, 0, 255, 255];
        let d = dib(2, 2, &bgra).unwrap();
        assert_eq!(d.len(), 40 + 16);
        assert_eq!(&d[0..4], &40u32.to_le_bytes());
        assert_eq!(&d[4..12], &[2, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(&d[14..16], &32u16.to_le_bytes());
        // First stored row is the bottom one.
        assert_eq!(&d[40..44], &[127, 127, 127, 255]);
        assert_eq!(&d[44..48], &[0, 0, 255, 255]);
        assert_eq!(&d[48..52], &[255, 0, 0, 255]);
        assert_eq!(&d[52..56], &[255, 255, 255, 255]);
        assert!(dib(2, 2, &bgra[..12]).is_none());
        assert!(dib(0, 2, &[]).is_none());
    }

    #[test]
    fn labels_in_order() {
        let labels: Vec<_> = ImageAction::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(labels, ["Open image in new tab", "Save image as…", "Copy image", "Copy image address"]);
    }
}
