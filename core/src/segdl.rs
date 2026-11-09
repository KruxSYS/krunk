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
///
/// Work stealing (0.7.0, R9): a connection that finishes its part takes the second half of the
/// part with the most bytes left ([`next_work`]), so a slow connection no longer holds up the
/// end. The part's `end` moves back under the lock its worker takes for every chunk; `busy` is
/// the chunk being written right now (past `next`), which a split never touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Part {
    pub start: u64,
    pub end: u64,
    pub next: u64,
    /// Bytes of the chunk being written now (claimed, not yet counted in `next`).
    pub busy: u64,
    /// A connection works on it.
    pub active: bool,
    /// That connection's speed, bytes per second (smoothed; 0 until known).
    pub rate: u64,
    /// Which connection (0-based), for the segment view.
    pub conn: u32,
}

impl Part {
    pub fn new(start: u64, end: u64) -> Part {
        Part { start, end, next: start, ..Part::default() }
    }

    pub fn done(&self) -> bool {
        self.next > self.end
    }

    /// Bytes nobody has claimed yet (what a split can take from).
    pub fn unclaimed(&self) -> u64 {
        (self.end + 1).saturating_sub(self.next + self.busy)
    }

    /// A chunk of `len` bytes arrived for this part: how many of them it still takes (fewer
    /// once a split has shortened it; 0 = the part is complete, stop the request). Call
    /// [`Part::end_write`] once they are written at `next`.
    pub fn begin_write(&mut self, len: u64) -> u64 {
        let n = len.min(self.unclaimed());
        self.busy = n;
        n
    }

    pub fn end_write(&mut self, written: u64) {
        self.next += written;
        self.busy = 0;
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
/// Space left free on the disk when a download in parts allocates its file.
pub const DISK_RESERVE: u64 = 512 * 1024 * 1024;

/// May a download in parts allocate `total` bytes for its file (`have` already allocated)
/// with `free` bytes free on the disk? The file is allocated at once, at the size the server
/// claims, so a server could fill the disk without sending a byte (0.11.7, audit C12).
pub fn fits_on_disk(total: u64, have: u64, free: u64) -> bool {
    total.saturating_sub(have).saturating_add(DISK_RESERVE) <= free
}

pub fn plan(total: u64, connections: u32) -> Vec<Part> {
    if total == 0 {
        return Vec::new();
    }
    let n = u64::from(connections.clamp(1, MAX_CONNECTIONS)).min((total / MIN_PART).max(1));
    let size = total.div_ceil(n);
    (0..n)
        .filter_map(|i| {
            let start = i * size;
            (start < total).then(|| Part::new(start, (start + size).min(total) - 1))
        })
        .collect()
}

/// A part is split only when it has at least this much left …
pub const MIN_STEAL: u64 = 2 * 1024 * 1024;
/// … and more than this much time at its connection's speed (it would finish soon anyway; a
/// new request costs a round trip).
pub const STEAL_MIN_MS: u64 = 1000;
/// Splits fall on this boundary (whole disk pages and cache lines).
pub const STEAL_ALIGN: u64 = 64 * 1024;

/// A part fetched at `rate` bytes/s is worth splitting from this many bytes left.
pub fn steal_threshold(rate: u64) -> u64 {
    MIN_STEAL.max(rate.saturating_mul(STEAL_MIN_MS) / 1000)
}

/// Work for free connection `conn` (fetching at `rate` bytes/s, 0 if not known yet): an
/// unfinished part nobody works on (a resumed download), else a piece of the active part that
/// would take longest to finish at its connection's speed (the most bytes when speeds are not
/// known), if it has [`steal_threshold`] left. The piece is sized by the speeds, so the victim
/// ends when the other connections, sharing the piece, do (half when a speed is not known). The part returned is
/// marked active.
pub fn next_work(parts: &mut Vec<Part>, conn: u32, rate: u64) -> Option<usize> {
    if let Some(i) = parts.iter().position(|p| !p.active && !p.done()) {
        parts[i].active = true;
        parts[i].conn = conn;
        return Some(i);
    }
    let time_left = |p: &Part| p.unclaimed() as f64 / p.rate.max(1) as f64;
    let v = parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.active && !p.done() && p.unclaimed() >= steal_threshold(p.rate))
        .max_by(|(_, a), (_, b)| time_left(a).total_cmp(&time_left(b)))
        .map(|(i, _)| i)?;
    let left = parts[v].unclaimed();
    let victim = parts[v].rate;
    // The piece taken is shared later by every other connection (they take from it in turn),
    // so the victim keeps what it can do while all of them do the rest.
    let others = rate
        + parts.iter().enumerate().filter(|(i, p)| *i != v && p.active && !p.done()).map(|(_, p)| p.rate).sum::<u64>();
    let keep = if victim > 0 && others > 0 {
        (left as u128 * u128::from(victim) / (u128::from(victim) + u128::from(others))) as u64
    } else {
        left / 2
    };
    let from = parts[v].next + parts[v].busy;
    let mid = (from + keep).next_multiple_of(STEAL_ALIGN).max(from + STEAL_ALIGN);
    if mid > parts[v].end {
        return None;
    }
    let mut new = Part::new(mid, parts[v].end);
    new.active = true;
    new.conn = conn;
    parts[v].end = mid - 1;
    parts.push(new);
    Some(parts.len() - 1)
}

/// Smoothed speed: `bytes` arrived over `dt_ms` (half-life 1 s).
pub fn smooth_rate(prev: u64, bytes: u64, dt_ms: u64) -> u64 {
    if dt_ms == 0 {
        return prev;
    }
    let inst = bytes as f64 * 1000.0 / dt_ms as f64;
    if prev == 0 {
        return inst.round() as u64;
    }
    let a = 1.0 - 0.5f64.powf(dt_ms as f64 / 1000.0);
    (prev as f64 + a * (inst - prev as f64)).max(0.0).round() as u64
}

/// What is left of a download in parts, saved so it can go on after a restart (0.7.0): the
/// unfinished ranges; everything else is already in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub next: u64,
}

/// The unfinished parts as saved segments (written bytes only: a chunk being written counts
/// once it is written).
pub fn snapshot(parts: &[Part]) -> Vec<Segment> {
    parts.iter().filter(|p| !p.done()).map(|p| Segment { start: p.start, end: p.end, next: p.next }).collect()
}

/// Parts from saved segments; `None` if they do not fit a file of `total` bytes.
pub fn restore(segments: &[Segment], total: u64) -> Option<Vec<Part>> {
    let mut parts: Vec<Part> = Vec::new();
    for s in segments {
        if s.start > s.end || s.end >= total || s.next < s.start || s.next > s.end + 1 {
            return None;
        }
        parts.push(Part { start: s.start, end: s.end, next: s.next, ..Part::default() });
    }
    let mut sorted = parts.clone();
    sorted.sort_by_key(|p| p.start);
    if sorted.windows(2).any(|w| w[0].end >= w[1].start) {
        return None;
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_space_for_the_whole_file() {
        let gib = 1024 * 1024 * 1024;
        assert!(fits_on_disk(gib, 0, 2 * gib));
        assert!(!fits_on_disk(2 * gib, 0, 2 * gib), "the reserve stays free");
        assert!(fits_on_disk(2 * gib, gib, 2 * gib), "a resumed file has its bytes already");
        assert!(!fits_on_disk(u64::MAX, 0, u64::MAX - 1), "a server's huge claim");
    }

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
        assert_eq!(parts[0], Part::new(0, 16 * 1024 * 1024 - 1));
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

    // ---------------------------------------------------------------- work stealing (0.7.0)

    /// A downloader on a simulated clock: connection `k` gets `speeds[k]` bytes per tick, a new
    /// request costs `latency` ticks, the bytes are a function of their offset and land in a
    /// buffer at their position. Runs until the file is complete (or `stop_at` ticks) and
    /// returns the ticks and the parts.
    struct Sim {
        speeds: Vec<u64>,
        latency: u32,
        steal: bool,
    }

    fn byte_at(offset: u64) -> u8 {
        (offset.wrapping_mul(2_654_435_761) >> 13) as u8
    }

    impl Sim {
        fn run(&self, parts: &mut Vec<Part>, out: &mut [u8], stop_at: Option<u32>) -> (u32, usize) {
            let n = self.speeds.len();
            // (part, ticks until the request answers) per connection.
            let mut work: Vec<Option<(usize, u32)>> = vec![None; n];
            let mut splits = 0;
            let mut rates = vec![0u64; n];
            for (k, w) in work.iter_mut().enumerate() {
                *w = next_work_sim(parts, k as u32, 0, false).map(|i| (i, self.latency));
            }
            let mut tick = 0;
            while parts.iter().any(|p| !p.done()) {
                if stop_at == Some(tick) {
                    break;
                }
                tick += 1;
                assert!(tick < 100_000, "stuck");
                for k in 0..n {
                    let Some((i, wait)) = work[k] else {
                        // An idle connection looks for work every tick.
                        work[k] = next_work_sim(parts, k as u32, rates[k], self.steal).map(|i| {
                            splits += 1;
                            (i, self.latency)
                        });
                        continue;
                    };
                    if wait > 0 {
                        work[k] = Some((i, wait - 1));
                        continue;
                    }
                    let take = parts[i].begin_write(self.speeds[k]);
                    let at = parts[i].next;
                    for o in at..at + take {
                        out[o as usize] = byte_at(o);
                    }
                    parts[i].end_write(take);
                    // One tick = 100 ms.
                    if take == self.speeds[k] {
                        parts[i].rate = smooth_rate(parts[i].rate, take, 100);
                    }
                    rates[k] = parts[i].rate;
                    if parts[i].done() {
                        parts[i].active = false;
                        work[k] = next_work_sim(parts, k as u32, rates[k], self.steal).map(|j| {
                            splits += 1;
                            (j, self.latency)
                        });
                    }
                }
            }
            (tick, splits)
        }
    }

    fn next_work_sim(parts: &mut Vec<Part>, conn: u32, rate: u64, steal: bool) -> Option<usize> {
        if steal {
            next_work(parts, conn, rate)
        } else {
            let i = parts.iter().position(|p| !p.active && !p.done())?;
            parts[i].active = true;
            parts[i].conn = conn;
            Some(i)
        }
    }

    fn sha(b: &[u8]) -> Vec<u8> {
        use sha2::Digest;
        sha2::Sha256::digest(b).to_vec()
    }

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn a_slow_connection_no_longer_holds_up_the_end() {
        let total = 48 * MIB;
        let source: Vec<u8> = (0..total).map(byte_at).collect();
        // Three connections at 640 KiB per tick, one at a tenth of that; 2 ticks per request.
        let speeds = vec![640 * 1024, 640 * 1024, 64 * 1024, 640 * 1024];
        let run = |steal: bool| {
            let mut parts = plan(total, 4);
            let mut out = vec![0u8; total as usize];
            let (ticks, splits) = Sim { speeds: speeds.clone(), latency: 2, steal }.run(&mut parts, &mut out, None);
            (ticks, splits, out, parts)
        };
        let (static_ticks, _, static_out, _) = run(false);
        let (ticks, splits, out, parts) = run(true);
        println!("48 MiB, 4 connections, one at a tenth: static {static_ticks} ticks, stealing {ticks} ticks, {splits} splits");
        assert_eq!(sha(&static_out), sha(&source), "static: every byte in place");
        assert_eq!(sha(&out), sha(&source), "stealing: every byte in place");
        assert!(
            (ticks as f64) < 0.7 * static_ticks as f64,
            "stealing {ticks} ticks vs static {static_ticks} ({splits} splits)"
        );
        // The parts still tile the file exactly.
        let mut sorted = parts.clone();
        sorted.sort_by_key(|p| p.start);
        assert_eq!(sorted[0].start, 0);
        assert_eq!(sorted.last().unwrap().end, total - 1);
        assert!(sorted.windows(2).all(|w| w[0].end + 1 == w[1].start));
        let planned: Vec<u64> = plan(total, 4).iter().map(|p| p.start).collect();
        assert!(sorted.iter().all(|p| p.start % STEAL_ALIGN == 0 || planned.contains(&p.start)), "splits are aligned");
        assert!(splits > 0 && splits < 64, "{splits} splits");
    }

    #[test]
    fn equal_connections_split_little_and_the_threshold_stops_splitting() {
        let total = 32 * MIB;
        let mut parts = plan(total, 4);
        let mut out = vec![0u8; total as usize];
        let (_, splits) = Sim { speeds: vec![256 * 1024; 4], latency: 1, steal: true }.run(&mut parts, &mut out, None);
        // With equal speeds the parts end together: nothing worth taking.
        assert!(splits <= 4 + 4, "{splits}");
        // Directly: a part with 1.5 MiB left is not split …
        let mut one = vec![Part { active: true, ..Part::new(0, 3 * MIB / 2 - 1) }];
        assert_eq!(next_work(&mut one, 1, 0), None);
        // … nor one with 6 MiB left whose connection does 8 MiB/s (under a second).
        let mut fast = vec![Part { active: true, rate: 8 * MIB, ..Part::new(0, 6 * MIB - 1) }];
        assert_eq!(next_work(&mut fast, 1, 8 * MIB), None);
        // 6 MiB at 1 MiB/s, taken by an equal connection: split in the middle, on a 64 KiB boundary.
        let mut even = vec![Part { active: true, rate: MIB, next: 100, ..Part::new(0, 6 * MIB - 1) }];
        assert_eq!(next_work(&mut even, 1, MIB), Some(1));
        assert_eq!(even[1].start, 3 * MIB + STEAL_ALIGN);
        assert_eq!((even[0].end + 1, even[1].end), (even[1].start, 6 * MIB - 1));
        assert!(even[1].active && even[1].conn == 1);
        // A slow part taken by a connection nine times faster keeps a tenth.
        let mut slow = vec![Part { active: true, rate: MIB / 10, ..Part::new(0, 10 * MIB - 1) }];
        assert_eq!(next_work(&mut slow, 2, 9 * MIB / 10), Some(1));
        assert_eq!(slow[0].end + 1, MIB);
        // The part that takes longest goes first, not the one with the most bytes.
        let mut two = vec![
            Part { active: true, rate: 10 * MIB, ..Part::new(0, 20 * MIB - 1) },
            Part { active: true, rate: MIB / 2, ..Part::new(20 * MIB, 25 * MIB - 1) },
        ];
        assert_eq!(next_work(&mut two, 3, MIB), Some(2));
        assert_eq!(two[2].end, 25 * MIB - 1, "split off the slow one");
        // A chunk being written is never split off.
        let mut busy = vec![Part { active: true, next: 0, busy: 7 * MIB, ..Part::new(0, 8 * MIB - 1) }];
        assert_eq!(next_work(&mut busy, 1, 0), None, "only 1 MiB unclaimed");
        let mut busy = vec![Part { active: true, next: 0, busy: 4 * MIB, ..Part::new(0, 8 * MIB - 1) }];
        assert_eq!(next_work(&mut busy, 1, 0), Some(1));
        assert!(busy[1].start >= 4 * MIB, "the chunk being written stays with its part");
    }

    #[test]
    fn a_shortened_part_takes_only_its_bytes() {
        let mut p = Part::new(0, 999);
        assert_eq!(p.begin_write(600), 600);
        p.end_write(600);
        p.end = 799; // split while idle
        assert_eq!(p.begin_write(600), 200);
        p.end_write(200);
        assert!(p.done());
        assert_eq!(p.begin_write(10), 0);
    }

    #[test]
    fn a_download_goes_on_from_saved_segments() {
        let total = 40 * MIB;
        let source: Vec<u8> = (0..total).map(byte_at).collect();
        let sim = Sim { speeds: vec![512 * 1024, 48 * 1024, 512 * 1024], latency: 2, steal: true };
        let mut parts = plan(total, 3);
        let mut out = vec![0u8; total as usize];
        sim.run(&mut parts, &mut out, Some(25));
        // Krunk quits: the unfinished segments are saved (as JSON) and the file stays.
        let saved = serde_json::to_string(&snapshot(&parts)).unwrap();
        let segments: Vec<Segment> = serde_json::from_str(&saved).unwrap();
        assert!(!segments.is_empty());
        let mut back = restore(&segments, total).unwrap();
        assert!(back.iter().all(|p| !p.active && !p.done()));
        sim.run(&mut back, &mut out, None);
        assert_eq!(sha(&out), sha(&source), "resumed: every byte in place");
        // Saved segments that do not fit the file are refused.
        assert!(restore(&[Segment { start: 0, end: total, next: 0 }], total).is_none());
        assert!(restore(&[Segment { start: 10, end: 20, next: 5 }], total).is_none());
        assert!(restore(&[Segment { start: 0, end: 20, next: 0 }, Segment { start: 15, end: 30, next: 15 }], total)
            .is_none());
        assert_eq!(restore(&[], total), Some(vec![]));
    }

    #[test]
    fn speeds_are_smoothed() {
        assert_eq!(smooth_rate(0, 1000, 1000), 1000);
        let r = smooth_rate(1000, 3000, 1000);
        assert!(r > 1000 && r < 3000 && r == 2000, "{r}");
        assert_eq!(smooth_rate(500, 0, 0), 500);
    }
}
