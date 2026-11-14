//! Going on with a download over one connection after a pause, an error or a restart of the
//! app (Krunk for Android 0.4.0): the decisions, free of any I/O. The host saves the engine's
//! response stream to a part file; whether what is there may be continued is decided here, by
//! the same rules as a download in parts ([`crate::segdl`]): the size is known, the server did
//! not compress the file and names a version of it (a strong ETag, else Last-Modified). The
//! request for the rest carries `Range` and `If-Range`; only an exact "206 Partial Content" for
//! the bytes asked for, of the same version, is appended. Anything else means the download
//! cannot go on, and the host says so instead of starting over silently.

use crate::segdl::{check_part, ContentRange, Head, Validator};

/// A response the host can continue later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumable {
    pub total: u64,
    pub validator: Validator,
}

/// The parts of a response the decisions need, from its status and header pairs (names in any
/// case; the first of a repeated header counts).
pub fn head(status: u16, headers: &[(String, String)]) -> Head {
    let mut h = Head { status, ..Head::default() };
    for (name, value) in headers {
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-range" if h.content_range.is_none() => h.content_range = parse_content_range(value),
            "content-length" if h.content_length.is_none() => h.content_length = value.parse().ok(),
            "etag" if h.etag.is_none() => h.etag = Some(value.to_string()),
            "last-modified" if h.last_modified.is_none() => h.last_modified = Some(value.to_string()),
            "content-encoding" if h.content_encoding.is_none() => h.content_encoding = Some(value.to_ascii_lowercase()),
            _ => {}
        }
    }
    h
}

fn parse_content_range(v: &str) -> Option<ContentRange> {
    // The same form `segdl::parse_head` reads: "bytes start-end/total".
    crate::segdl::parse_head(&format!("HTTP/1.1 0\r\nContent-Range: {v}")).content_range
}

/// `Accept-Ranges` of the header pairs, lower case.
fn accept_ranges(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("accept-ranges"))
        .map(|(_, v)| v.trim().to_ascii_lowercase())
}

/// Can a download of `url` that started with this response (the whole file, status 200) be
/// continued later? The reason otherwise is for the log.
pub fn resumable(url: &str, status: u16, headers: &[(String, String)]) -> Result<Resumable, &'static str> {
    let lower = url.trim_start().to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("not a web address");
    }
    if status != 200 {
        return Err("not a whole file");
    }
    if accept_ranges(headers).is_some_and(|a| a == "none") {
        return Err("the server does not send parts");
    }
    let h = head(status, headers);
    if h.content_encoding.as_deref().is_some_and(|e| e != "identity") {
        return Err("the server compresses the file");
    }
    let Some(total) = h.content_length.filter(|t| *t > 0) else { return Err("no size") };
    let validator = match (&h.etag, &h.last_modified) {
        (Some(e), _) if !e.starts_with("W/") && !e.is_empty() && e.len() <= 256 => Validator::ETag(e.clone()),
        (_, Some(m)) if !m.is_empty() && m.len() <= 64 => Validator::LastModified(m.clone()),
        _ => return Err("no validator (ETag or Last-Modified)"),
    };
    Ok(Resumable { total, validator })
}

/// The validator saved with a download (`downloads::Resume`): the ETag if there is one.
pub fn saved_validator(etag: Option<&str>, last_modified: Option<&str>) -> Option<Validator> {
    match (etag, last_modified) {
        (Some(e), _) if !e.is_empty() => Some(Validator::ETag(e.to_string())),
        (_, Some(m)) if !m.is_empty() => Some(Validator::LastModified(m.to_string())),
        _ => None,
    }
}

/// The request headers that ask for the file from byte `offset` on, of that version only.
pub fn range_headers(offset: u64, validator: &Validator) -> Vec<(String, String)> {
    vec![
        ("Range".to_string(), format!("bytes={offset}-")),
        ("If-Range".to_string(), validator.header_value().to_string()),
        // A compressed answer could not be appended to the bytes already there.
        ("Accept-Encoding".to_string(), "identity".to_string()),
    ]
}

/// What the answer to a range request means for the part file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The rest of the same file: append it.
    Append,
    /// The server sent the whole file again (it ignores ranges, or the file changed): the
    /// download cannot go on; starting over is the user's choice.
    WholeFile,
    /// The file changed or the answer does not fit: the same, with the reason for the log.
    Mismatch(&'static str),
}

/// Is this the answer for bytes `offset..total` of the same file?
pub fn check(status: u16, headers: &[(String, String)], offset: u64, total: u64, validator: &Validator) -> Verdict {
    if offset >= total {
        return Verdict::Mismatch("nothing left to ask for");
    }
    if status == 200 {
        return Verdict::WholeFile;
    }
    if status == 416 {
        return Verdict::Mismatch("the file changed on the server");
    }
    match check_part(&head(status, headers), offset, total - 1, total, validator) {
        Ok(()) => Verdict::Append,
        Err(why) => Verdict::Mismatch(why),
    }
}

/// How a stream that ended stands against the size the server named: complete only when every
/// byte arrived (or no size was named and the stream ended cleanly).
pub fn complete(received: u64, total: Option<u64>) -> bool {
    total.is_none_or(|t| t == received)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_sized_file_with_a_strong_etag_can_go_on() {
        let r = resumable("https://a.test/f.bin", 200, &h(&[("content-length", "1000"), ("ETag", "\"v1\"")])).unwrap();
        assert_eq!(r, Resumable { total: 1000, validator: Validator::ETag("\"v1\"".into()) });
        let r = resumable(
            "http://a.test/f",
            200,
            &h(&[("Content-Length", "5"), ("ETag", "W/\"weak\""), ("Last-Modified", "Sat, 14 Nov 2026 00:00:00 GMT")]),
        )
        .unwrap();
        assert_eq!(r.validator, Validator::LastModified("Sat, 14 Nov 2026 00:00:00 GMT".into()));
    }

    #[test]
    fn what_cannot_go_on() {
        let ok = [("Content-Length", "1000"), ("ETag", "\"v1\"")];
        assert!(resumable("blob:https://a.test/1", 200, &h(&ok)).is_err());
        assert!(resumable("data:text/plain,hi", 200, &h(&ok)).is_err());
        assert!(resumable("https://a.test/f", 206, &h(&ok)).is_err());
        assert!(resumable("https://a.test/f", 200, &h(&[("ETag", "\"v1\"")])).is_err());
        assert!(resumable("https://a.test/f", 200, &h(&[("Content-Length", "0"), ("ETag", "\"v1\"")])).is_err());
        assert!(resumable("https://a.test/f", 200, &h(&[("Content-Length", "10")])).is_err());
        assert!(resumable("https://a.test/f", 200, &h(&[("Content-Length", "10"), ("ETag", "W/\"x\"")])).is_err());
        let mut gz = ok.to_vec();
        gz.push(("Content-Encoding", "gzip"));
        assert!(resumable("https://a.test/f", 200, &h(&gz)).is_err());
        let mut none = ok.to_vec();
        none.push(("Accept-Ranges", "None"));
        assert!(resumable("https://a.test/f", 200, &h(&none)).is_err());
        let mut bytes = ok.to_vec();
        bytes.push(("accept-ranges", "bytes"));
        assert!(resumable("https://a.test/f", 200, &h(&bytes)).is_ok());
    }

    #[test]
    fn the_request_names_the_offset_and_the_version() {
        let v = Validator::ETag("\"v1\"".into());
        let hs = range_headers(400, &v);
        assert!(hs.contains(&("Range".into(), "bytes=400-".into())));
        assert!(hs.contains(&("If-Range".into(), "\"v1\"".into())));
        assert_eq!(saved_validator(Some("\"v1\""), Some("x")), Some(v));
        assert_eq!(saved_validator(None, Some("x")), Some(Validator::LastModified("x".into())));
        assert_eq!(saved_validator(None, None), None);
        assert_eq!(saved_validator(Some(""), None), None);
    }

    #[test]
    fn only_the_exact_rest_of_the_same_file_is_appended() {
        let v = Validator::ETag("\"v1\"".into());
        let good = h(&[("Content-Range", "bytes 400-999/1000"), ("ETag", "\"v1\"")]);
        assert_eq!(check(206, &good, 400, 1000, &v), Verdict::Append);
        // No validator in the answer: If-Range already tied it to the version.
        assert_eq!(check(206, &h(&[("Content-Range", "bytes 400-999/1000")]), 400, 1000, &v), Verdict::Append);
        assert_eq!(check(200, &h(&[("Content-Length", "1000")]), 400, 1000, &v), Verdict::WholeFile);
        assert!(matches!(check(416, &[], 400, 1000, &v), Verdict::Mismatch(_)));
        let other = h(&[("Content-Range", "bytes 400-999/1000"), ("ETag", "\"v2\"")]);
        assert!(matches!(check(206, &other, 400, 1000, &v), Verdict::Mismatch(_)));
        let wrong_start = h(&[("Content-Range", "bytes 0-999/1000")]);
        assert!(matches!(check(206, &wrong_start, 400, 1000, &v), Verdict::Mismatch(_)));
        let wrong_total = h(&[("Content-Range", "bytes 400-1999/2000")]);
        assert!(matches!(check(206, &wrong_total, 400, 1000, &v), Verdict::Mismatch(_)));
        let gz = h(&[("Content-Range", "bytes 400-999/1000"), ("Content-Encoding", "gzip")]);
        assert!(matches!(check(206, &gz, 400, 1000, &v), Verdict::Mismatch(_)));
        assert!(matches!(check(206, &good, 1000, 1000, &v), Verdict::Mismatch(_)));
        assert!(matches!(check(500, &[], 400, 1000, &v), Verdict::Mismatch(_)));
    }

    #[test]
    fn a_short_stream_is_never_complete() {
        assert!(complete(1000, Some(1000)));
        assert!(!complete(999, Some(1000)));
        assert!(!complete(1001, Some(1000)));
        assert!(complete(123, None));
    }

    #[test]
    fn repeated_headers_keep_the_first() {
        let hd = head(200, &h(&[("Content-Length", "10"), ("content-length", "20"), ("ETAG", "a"), ("etag", "b")]));
        assert_eq!(hd.content_length, Some(10));
        assert_eq!(hd.etag.as_deref(), Some("a"));
    }
}
