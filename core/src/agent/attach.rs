//! Files the user attaches to a message in the agent panel (0.25.2): pictures, PDFs and text
//! files. The panel scales pictures down (longest side 1568 px, what vision models look at) and
//! reads text files as text; everything arrives with the `agentSend` request and is checked here
//! again: the kind comes from the bytes (never from the name or the claimed type), sizes and
//! counts are capped, and a picture needs a model that sees images.
//!
//! In the conversation a picture becomes an image part and a PDF a document part of the user's
//! message; a text file is quoted in the message's text with its name.

use super::{Document, Image, Part};
use crate::protocol::AgentFile;
use base64ct::{Base64, Encoding};

/// Files per message.
pub const MAX_FILES: usize = 10;
/// A picture after the panel scaled it (the providers take 5 MB at most).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// A PDF. Everything goes in one message from the panel, which the engine's bridge caps at
/// 8 M characters (`MAX_MESSAGE` in the renderer), base64 included.
pub const MAX_PDF_BYTES: usize = 5 * 1024 * 1024;
/// A text file's characters; a longer one is cut (and says so).
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Pictures and PDFs of one message together (5.5 MB: 7.3 M characters in base64).
pub const MAX_TOTAL_BYTES: usize = 5632 * 1024;
const MAX_NAME_CHARS: usize = 120;

/// A checked attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attachment {
    Image { name: String, image: Image },
    Pdf { name: String, doc: Document },
    Text { name: String, text: String, cut: bool, chars: usize },
}

impl Attachment {
    pub fn name(&self) -> &str {
        match self {
            Attachment::Image { name, .. } | Attachment::Pdf { name, .. } | Attachment::Text { name, .. } => name,
        }
    }
}

/// A file name for the model and the panel: no paths, no control characters, not too long.
fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base.chars().filter(|c| !c.is_control() && *c != '"').take(MAX_NAME_CHARS).collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "file".into()
    } else {
        s
    }
}

/// The picture type from the first bytes.
fn image_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

/// Checks the panel's files. `vision`: the model accepts images.
pub fn check(files: &[AgentFile], vision: bool) -> Result<Vec<Attachment>, String> {
    if files.len() > MAX_FILES {
        return Err(format!("At most {MAX_FILES} files go with one message."));
    }
    let mut total = 0usize;
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let name = clean_name(&f.name);
        if let Some(text) = &f.text {
            if text.contains('\0') {
                return Err(format!("{name} is not a text file."));
            }
            let chars = text.chars().count();
            let cut = chars > MAX_TEXT_CHARS;
            let text = if cut { text.chars().take(MAX_TEXT_CHARS).collect() } else { text.clone() };
            out.push(Attachment::Text { name, text, cut, chars });
            continue;
        }
        let Some(data) = f.data.as_deref() else {
            return Err(format!("{name} arrived empty."));
        };
        let bytes = Base64::decode_vec(data).map_err(|_| format!("{name} could not be read."))?;
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err("The files are too big together (at most 5.5 MB of pictures and PDFs).".into());
        }
        if let Some(mime) = image_mime(&bytes) {
            if !vision {
                return Err(format!(
                    "{name}: the selected model can't see pictures. Choose a model with vision, or attach text."
                ));
            }
            if bytes.len() > MAX_IMAGE_BYTES {
                return Err(format!("{name} is too big (pictures: at most {} MB).", MAX_IMAGE_BYTES >> 20));
            }
            out.push(Attachment::Image { name, image: Image { mime: mime.into(), base64: data.to_string() } });
        } else if bytes.starts_with(b"%PDF-") {
            if bytes.len() > MAX_PDF_BYTES {
                return Err(format!("{name} is too big (PDFs: at most {} MB).", MAX_PDF_BYTES >> 20));
            }
            let doc = Document { mime: "application/pdf".into(), name: name.clone(), base64: data.to_string() };
            out.push(Attachment::Pdf { name, doc });
        } else {
            return Err(format!("{name}: Krunk can attach pictures (PNG, JPEG, WebP, GIF), PDFs and text files."));
        }
    }
    Ok(out)
}

/// The user message's parts: `body` (context and the user's words), a list of the files, the
/// text files quoted, then the pictures and PDFs.
pub fn parts(body: String, files: Vec<Attachment>) -> Vec<Part> {
    if files.is_empty() {
        return vec![Part::Text(body)];
    }
    let mut text = body;
    text.push_str("\n\nAttached files:");
    for f in &files {
        let what = match f {
            Attachment::Image { .. } => "picture, below".to_string(),
            Attachment::Pdf { .. } => "PDF, below".to_string(),
            Attachment::Text { chars, cut, .. } if *cut => {
                format!("text, {chars} characters, the first {MAX_TEXT_CHARS} below")
            }
            Attachment::Text { chars, .. } => format!("text, {chars} characters, below"),
        };
        text.push_str(&format!("\n- \"{}\" ({what})", f.name()));
    }
    let mut media = Vec::new();
    for f in files {
        match f {
            Attachment::Text { name, text: body, cut, .. } => {
                text.push_str(&format!("\n\n<<<FILE \"{name}\"\n{body}"));
                if !body.ends_with('\n') {
                    text.push('\n');
                }
                if cut {
                    text.push_str("[the rest of the file was cut]\n");
                }
                text.push_str("FILE>>>");
            }
            Attachment::Image { image, .. } => media.push(Part::Image(image)),
            Attachment::Pdf { doc, .. } => media.push(Part::Document(doc)),
        }
    }
    let mut out = vec![Part::Text(text)];
    out.extend(media);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(b: &[u8]) -> String {
        Base64::encode_string(b)
    }
    fn file(name: &str, data: &[u8]) -> AgentFile {
        AgentFile { name: name.into(), mime: String::new(), data: Some(b64(data)), text: None }
    }
    fn text(name: &str, t: &str) -> AgentFile {
        AgentFile { name: name.into(), mime: "text/plain".into(), data: None, text: Some(t.into()) }
    }
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn kinds_come_from_the_bytes() {
        let a = check(&[file("C:\\Users\\me\\shot.jpg", PNG), file("a.pdf", b"%PDF-1.7\n...")], true).unwrap();
        match &a[0] {
            Attachment::Image { name, image } => {
                assert_eq!(name, "shot.jpg");
                assert_eq!(image.mime, "image/png");
            }
            x => panic!("{x:?}"),
        }
        assert!(matches!(&a[1], Attachment::Pdf { doc, .. } if doc.mime == "application/pdf"));
        assert!(check(&[file("x.png", b"MZ\x90\0")], true).unwrap_err().contains("pictures"));
        assert!(check(&[file("x.png", PNG)], false).unwrap_err().contains("can't see pictures"));
        // A PDF is fine without vision.
        assert!(check(&[file("a.pdf", b"%PDF-1.4")], false).is_ok());
        let bad = AgentFile { data: Some("not base64!".into()), ..file("a", b"") };
        assert!(check(&[bad], true).is_err());
    }

    #[test]
    fn limits() {
        let many: Vec<_> = (0..=MAX_FILES).map(|i| text(&format!("{i}.txt"), "x")).collect();
        assert!(check(&many, true).is_err());
        let long = "é".repeat(MAX_TEXT_CHARS + 5);
        match &check(&[text("l.txt", &long)], true).unwrap()[0] {
            Attachment::Text { text, cut, chars, .. } => {
                assert!(*cut && *chars == MAX_TEXT_CHARS + 5 && text.chars().count() == MAX_TEXT_CHARS);
            }
            x => panic!("{x:?}"),
        }
        assert!(check(&[text("b.bin", "a\0b")], true).is_err());
        let mut big = PNG.to_vec();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        assert!(check(&[file("big.png", &big)], true).unwrap_err().contains("too big"));
        assert_eq!(clean_name("  "), "file");
        assert_eq!(clean_name("a\"b\u{7}.txt"), "ab.txt");
    }

    #[test]
    fn message_parts() {
        let a = check(&[text("notes.md", "# Hi"), file("p.png", PNG), file("d.pdf", b"%PDF-1.7")], true).unwrap();
        let p = parts("USER\nsummarise".into(), a);
        assert_eq!(p.len(), 3);
        let Part::Text(t) = &p[0] else { panic!() };
        assert!(t.starts_with("USER\nsummarise\n\nAttached files:\n- \"notes.md\" (text, 4 characters, below)"));
        assert!(t.contains("- \"p.png\" (picture, below)\n- \"d.pdf\" (PDF, below)"));
        assert!(t.ends_with("<<<FILE \"notes.md\"\n# Hi\nFILE>>>"));
        assert!(matches!(p[1], Part::Image(_)) && matches!(p[2], Part::Document(_)));
        assert_eq!(parts("x".into(), Vec::new()), vec![Part::Text("x".into())]);
    }
}
