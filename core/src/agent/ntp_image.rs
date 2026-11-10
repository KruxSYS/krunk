//! `set_new_tab_background_url` (0.15.0): the checks of the address and of the downloaded
//! picture. The host downloads it (normal profile, no cookies), these decide whether it is used.
//!
//! Only `https://` addresses without credentials, at most [`MAX_BYTES`]; the picture must be a
//! PNG, JPEG, WebP, GIF or AVIF by its first bytes (they decide the file's extension), and the
//! server's content type must not say it is something else (SVG and HTML are refused).

/// The largest picture downloaded.
pub const MAX_BYTES: usize = 15 * 1024 * 1024;
/// How long the download may take.
pub const TIMEOUT_MS: i64 = 30_000;
/// The longest address accepted.
const MAX_URL: usize = 2048;

/// Checks the address the model gave.
pub fn check_url(input: &str) -> Result<url::Url, String> {
    let t = input.trim();
    if t.is_empty() || t.len() > MAX_URL || t.chars().any(char::is_control) {
        return Err("the address is not valid".into());
    }
    let u = url::Url::parse(t).map_err(|_| "the address is not valid".to_string())?;
    if u.scheme() != "https" {
        return Err("only https:// addresses are allowed".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("the address must not contain a user name or password".into());
    }
    let host = u.host_str().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err("the address has no host".into());
    }
    // Krunk's own pages answer on a reserved name; they are not a picture source.
    if host == "invalid" || host.ends_with(".invalid") {
        return Err("Krunk's own pages can't be used".into());
    }
    Ok(u)
}

/// The extension a picture's first bytes say it is: `png`, `jpg`, `webp`, `gif` or `avif`.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("gif")
    } else if is_avif(b) {
        Some("avif")
    } else {
        None
    }
}

/// An ISO-BMFF `ftyp` box whose major or a compatible brand is `avif` / `avis`.
fn is_avif(b: &[u8]) -> bool {
    if b.len() < 16 || &b[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let end = size.clamp(16, 256).min(b.len());
    let avif = |brand: &[u8]| brand == b"avif" || brand == b"avis";
    // Major brand at 8, minor version at 12, compatible brands from 16.
    avif(&b[8..12]) || b[16..end].chunks_exact(4).any(avif)
}

/// The base MIME type, lower-case (`image/png` of `Image/PNG; charset=x`).
fn base_mime(mime: &str) -> String {
    mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase()
}

/// Checks a finished download (`final_url` after redirects); the result is the extension.
pub fn check_download(final_url: &str, status: i32, mime: &str, body: &[u8]) -> Result<&'static str, String> {
    if !final_url.is_empty() && check_url(final_url).is_err() {
        return Err("the address redirected to one that is not allowed".into());
    }
    if !(200..300).contains(&status) {
        return Err(format!("the server answered HTTP {status}"));
    }
    if body.is_empty() {
        return Err("the server sent nothing".into());
    }
    if body.len() > MAX_BYTES {
        return Err(too_large());
    }
    let mime = base_mime(mime);
    if mime == "image/svg+xml" {
        return Err("SVG pictures are not supported; use PNG, JPEG, WebP, GIF or AVIF".into());
    }
    let generic = mime.is_empty() || mime == "application/octet-stream" || mime == "binary/octet-stream";
    if !generic && !mime.starts_with("image/") {
        return Err(format!("the address is not a picture (the server says {})", clip(&mime)));
    }
    sniff(body).ok_or_else(|| "the file is not a PNG, JPEG, WebP, GIF or AVIF picture".to_string())
}

pub fn too_large() -> String {
    format!("the picture is larger than {} MB", MAX_BYTES / (1024 * 1024))
}

fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(60).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn addresses() {
        assert!(check_url("https://example.com/a.png").is_ok());
        assert!(check_url("  https://cdn.example.com/x?y=1  ").is_ok());
        for (bad, why) in [
            ("http://example.com/a.png", "https"),
            ("ftp://example.com/a.png", "https"),
            ("file:///C:/a.png", "https"),
            ("data:image/png;base64,AAAA", "https"),
            ("javascript:alert(1)", "https"),
            ("https://user:pw@example.com/a.png", "user name"),
            ("https://user@example.com/a.png", "user name"),
            ("https://krunk.invalid/ntp-image/x.png", "own pages"),
            ("not a url", "not valid"),
            ("", "not valid"),
        ] {
            let e = check_url(bad).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
        assert!(check_url(&format!("https://example.com/{}", "a".repeat(3000))).is_err());
    }

    #[test]
    fn magic_bytes() {
        assert_eq!(sniff(PNG), Some("png"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0, 0, 0x10]), Some("jpg"));
        assert_eq!(sniff(b"RIFF\x24\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"GIF89a\x01\0\x01\0"), Some("gif"));
        assert_eq!(sniff(b"GIF87a\x01\0\x01\0"), Some("gif"));
        assert_eq!(sniff(b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf"), Some("avif"));
        assert_eq!(sniff(b"\0\0\0\x1cftypmif1\0\0\0\0mif1avifmiaf"), Some("avif"), "a compatible brand");
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"), None, "HEIC is not AVIF");
        assert_eq!(sniff(b"\0\0\0\x18ftypisom\0\0\0\0isomavc1"), None, "an MP4");
        assert_eq!(sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"), None);
        assert_eq!(sniff(b"<!doctype html><html>"), None);
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn downloads() {
        let ok = |mime: &str, body: &[u8]| check_download("https://example.com/a", 200, mime, body);
        assert_eq!(ok("image/png", PNG), Ok("png"));
        // The bytes decide the extension, not the content type or the address.
        assert_eq!(ok("image/jpeg", PNG), Ok("png"));
        assert_eq!(ok("", PNG), Ok("png"));
        assert_eq!(ok("application/octet-stream", PNG), Ok("png"));
        assert_eq!(ok("Image/PNG; charset=binary", PNG), Ok("png"));
        assert!(ok("text/html", PNG).unwrap_err().contains("not a picture"));
        assert!(ok("image/svg+xml", b"<svg/>").unwrap_err().contains("SVG"));
        assert!(ok("image/png", b"<html>").unwrap_err().contains("not a PNG"));
        assert!(ok("image/png", b"").unwrap_err().contains("nothing"));
        assert!(check_download("https://example.com/a", 404, "image/png", PNG).unwrap_err().contains("404"));
        assert!(check_download("http://example.com/a", 200, "image/png", PNG).unwrap_err().contains("redirected"));
        // 15 MB at most.
        let mut big = PNG.to_vec();
        big.resize(MAX_BYTES, 0);
        assert_eq!(ok("image/png", &big), Ok("png"));
        big.push(0);
        assert!(ok("image/png", &big).unwrap_err().contains("15 MB"));
    }
}
