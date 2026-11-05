//! Downloading a file in parts over several connections (Krunk 0.4.0): the decisions, free
//! of any I/O. When the engine announces a download, the host asks the server for its first
//! byte (`Range: bytes=0-0`). Only an exact "206 Partial Content" answer with the total size,
//! no content encoding and a validator (a strong ETag, else Last-Modified) lets the file be
//! fetched in parts; anything else stays with the engine's own download. Every part's answer
//! must match the same validator and the exact range asked for, so a file that changes on the
//! server is never stitched together from two versions.

/// Files smaller than this are not worth splitting.
pub const MIN_SEGMENTED: u64 = 8 * 1024 * 1024;
/// Each part is at least this big.
pub const MIN_PART: u64 = 1024 * 1024;
/// The most connections a download may use (the setting's upper bound).
pub const MAX_CONNECTIONS: u32 = 16;

/// `Content-Range: bytes start-end/total` (total `None` for `*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

/// The parts of a response head the decisions need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub content_range: Option<ContentRange>,
    pub content_length: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_encoding: Option<String>,
}

/// Parses a raw response head: the status line, then `Name: value` lines (CRLF or LF).
pub fn parse_head(raw: &str) -> Head {
    let mut lines = raw.lines();
    let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut h = Head { status, ..Head::default() };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_string();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-range" => h.content_range = parse_content_range(&value),
            "content-length" => h.content_length = value.parse().ok(),
            "etag" => h.etag = Some(value),
            "last-modified" => h.last_modified = Some(value),
            "content-encoding" => h.content_encoding = Some(value.to_ascii_lowercase()),
            _ => {}
        }
    }
    h
}

fn parse_content_range(v: &str) -> Option<ContentRange> {
    let rest = v.strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.trim().split_once('-')?;
    let (start, end): (u64, u64) = (start.trim().parse().ok()?, end.trim().parse().ok()?);
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    Some(ContentRange { start, end, total })
}

/// What makes the parts belong to one version of the file (sent back as `If-Range`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validator {
    ETag(String),
    LastModified(String),
}

impl Validator {
    pub fn header_value(&self) -> &str {
        match self {
            Validator::ETag(v) | Validator::LastModified(v) => v,
        }
    }
}

/// The probe found a file that can be fetched in parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub total: u64,
    pub validator: Validator,
}

/// Can the file the probe (`Range: bytes=0-0`) asked about be fetched in parts? The reason
/// otherwise is for the log.
pub fn probe(head: &Head) -> Result<Probed, &'static str> {
    if head.status != 206 {
        return Err("the server does not send parts");
    }
    let Some(cr) = head.content_range else { return Err("no Content-Range") };
    if cr.start != 0 || cr.end != 0 {
        return Err("the server sent another range");
    }
    let Some(total) = cr.total else { return Err("no total size") };
    if head.content_encoding.as_deref().is_some_and(|e| e != "identity") {
        return Err("the server compresses the file");
    }
    if total < MIN_SEGMENTED {
        return Err("too small to split");
    }
    let validator = match (&head.etag, &head.last_modified) {
        (Some(e), _) if !e.starts_with("W/") && e.len() <= 256 => Validator::ETag(e.clone()),
        (_, Some(m)) if m.len() <= 64 => Validator::LastModified(m.clone()),
        _ => return Err("no validator (ETag or Last-Modified)"),
    };
    Ok(Probed { total, validator })
}

/// Is `head` the answer for bytes `start..=end` of the same file (`total`, `validator`)?
pub fn check_part(head: &Head, start: u64, end: u64, total: u64, validator: &Validator) -> Result<(), &'static str> {
    if head.status != 206 {
        return Err("the file changed on the server");
    }
    match head.content_range {
        Some(cr) if cr.start == start && cr.end == end && cr.total == Some(total) => {}
        _ => return Err("the server sent another range"),
    }
    if head.content_encoding.as_deref().is_some_and(|e| e != "identity") {
        return Err("the server compresses the file");
    }
    let same = match validator {
        Validator::ETag(e) => head.etag.as_deref().is_none_or(|h| h == e),
        Validator::LastModified(m) => head.last_modified.as_deref().is_none_or(|h| h == m),
    };
    if !same {
        return Err("the file changed on the server");
    }
    Ok(())
}

/// One part of the file: bytes `start..=end`; `next` is the first byte not yet written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    pub start: u64,
    pub end: u64,
    pub next: u64,
}

impl Part {
    pub fn done(&self) -> bool {
        self.next > self.end
    }

    pub fn left(&self) -> u64 {
        if self.done() {
            0
        } else {
            self.end + 1 - self.next
        }
    }
}

/// Splits `total` bytes into `connections` parts of about the same size, none smaller than
/// [`MIN_PART`] (so a smaller file uses fewer connections).
pub fn plan(total: u64, connections: u32) -> Vec<Part> {
    if total == 0 {
        return Vec::new();
    }
    let n = u64::from(connections.clamp(1, MAX_CONNECTIONS)).min((total / MIN_PART).max(1));
    let size = total.div_ceil(n);
    (0..n)
        .filter_map(|i| {
            let start = i * size;
            (start < total).then(|| Part { start, end: (start + size).min(total) - 1, next: start })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/67108864\r\nContent-Length: 1\r\nETag: \"abc123\"\r\nLast-Modified: Wed, 05 Nov 2026 10:00:00 GMT\r\nAccept-Ranges: bytes\r\n\r\n";

    #[test]
    fn parses_heads() {
        let h = parse_head(PROBE);
        assert_eq!(h.status, 206);
        assert_eq!(h.content_range, Some(ContentRange { start: 0, end: 0, total: Some(67_108_864) }));
        assert_eq!((h.content_length, h.etag.as_deref()), (Some(1), Some("\"abc123\"")));
        let h = parse_head("HTTP/2 200\ncontent-encoding: GZIP\ncontent-range: bytes 5-9/*\n");
        assert_eq!(h.status, 200);
        assert_eq!(h.content_encoding.as_deref(), Some("gzip"));
        assert_eq!(h.content_range, Some(ContentRange { start: 5, end: 9, total: None }));
        assert_eq!(parse_head("").status, 0);
        assert_eq!(parse_head("HTTP/1.1 206 x\r\nContent-Range: bytes 9-5/10\r\n").content_range, None);
        assert_eq!(parse_head("HTTP/1.1 206 x\r\nContent-Range: items 0-1/2\r\n").content_range, None);
    }

    #[test]
    fn probe_decisions() {
        let ok = parse_head(PROBE);
        assert_eq!(probe(&ok), Ok(Probed { total: 67_108_864, validator: Validator::ETag("\"abc123\"".into()) }));
        let with = |f: &dyn Fn(&mut Head)| {
            let mut h = ok.clone();
            f(&mut h);
            probe(&h)
        };
        assert!(with(&|h| h.status = 200).is_err(), "no ranges: 200 with the whole file");
        assert!(with(&|h| h.content_range = None).is_err());
        assert!(with(&|h| h.content_range = Some(ContentRange { start: 0, end: 0, total: None })).is_err());
        assert!(with(&|h| h.content_range = Some(ContentRange { start: 0, end: 99, total: Some(67_108_864) })).is_err());
        assert!(with(&|h| h.content_encoding = Some("gzip".into())).is_err());
        assert!(with(&|h| h.content_encoding = Some("identity".into())).is_ok());
        assert!(
            with(&|h| h.content_range = Some(ContentRange { start: 0, end: 0, total: Some(MIN_SEGMENTED - 1) }))
                .is_err(),
            "too small"
        );
        // A weak ETag does not count; Last-Modified does then.
        assert_eq!(
            with(&|h| h.etag = Some("W/\"x\"".into())).unwrap().validator,
            Validator::LastModified("Wed, 05 Nov 2026 10:00:00 GMT".into())
        );
        assert!(with(&|h| {
            h.etag = Some("W/\"x\"".into());
            h.last_modified = None;
        })
        .is_err());
    }

    #[test]
    fn parts_must_match() {
        let v = Validator::ETag("\"abc\"".into());
        let part = |s: &str| parse_head(&format!("HTTP/1.1 206 Partial Content\r\n{s}\r\n"));
        let good = part("Content-Range: bytes 100-199/1000\r\nETag: \"abc\"");
        assert_eq!(check_part(&good, 100, 199, 1000, &v), Ok(()));
        assert!(check_part(&good, 100, 299, 1000, &v).is_err(), "another range");
        assert!(check_part(&good, 100, 199, 2000, &v).is_err(), "another size");
        assert!(check_part(&part("Content-Range: bytes 100-199/1000\r\nETag: \"new\""), 100, 199, 1000, &v).is_err());
        assert_eq!(
            check_part(&part("Content-Range: bytes 100-199/1000"), 100, 199, 1000, &v),
            Ok(()),
            "no ETag repeated"
        );
        let changed = parse_head("HTTP/1.1 200 OK\r\nETag: \"new\"\r\n");
        assert!(check_part(&changed, 100, 199, 1000, &v).is_err(), "If-Range failed: the whole new file");
        let lm = Validator::LastModified("Tue".into());
        assert!(check_part(&part("Content-Range: bytes 0-9/10\r\nLast-Modified: Wed"), 0, 9, 10, &lm).is_err());
    }

    #[test]
    fn plans_parts() {
        let parts = plan(64 * 1024 * 1024, 4);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], Part { start: 0, end: 16 * 1024 * 1024 - 1, next: 0 });
        assert_eq!(parts[3].end, 64 * 1024 * 1024 - 1);
        // Contiguous and complete for odd sizes.
        for (total, n) in [(10_000_019u64, 3u32), (8 * 1024 * 1024 + 7, 16), (5_000_000, 16)] {
            let p = plan(total, n);
            assert_eq!(p[0].start, 0);
            assert_eq!(p.last().unwrap().end, total - 1);
            for w in p.windows(2) {
                assert_eq!(w[0].end + 1, w[1].start);
            }
            assert!(p.iter().all(|x| x.left() >= MIN_PART.min(total) / 2), "{total} {n}");
            assert!(p.len() as u64 <= (total / MIN_PART).max(1));
        }
        assert_eq!(plan(5_000_000, 16).len(), 4, "1 MB parts at least");
        assert_eq!(plan(10, 4).len(), 1);
        assert!(plan(0, 4).is_empty());
        assert_eq!(plan(100 * 1024 * 1024, 99).len(), MAX_CONNECTIONS as usize);
        let mut p = plan(3 * 1024 * 1024, 3)[1];
        assert_eq!(p.left(), 1024 * 1024);
        p.next = p.end + 1;
        assert!(p.done() && p.left() == 0);
    }
}
