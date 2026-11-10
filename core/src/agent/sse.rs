//! Incremental Server-Sent Events parser (WP-A).
//!
//! Bytes arrive in arbitrary chunks (a UTF-8 character or a line may be split); `feed` returns
//! every complete event so far. Handles `\n`, `\r\n` and `\r` line ends, `:` comments, multi-line
//! `data:` (joined with `\n`), `event:` names and a final event without a blank line (`finish`).
//! Lines are decoded only once complete, so a character split between chunks stays whole.

/// One event: its `event:` name (if any) and its data lines joined.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// A line longer than this is dropped (a broken or hostile server); the stream goes on.
const MAX_LINE: usize = 8 << 20;

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    /// The last chunk ended in `\r`: a `\n` starting the next one belongs to it.
    after_cr: bool,
    /// The first line was seen (a byte order mark before it is skipped).
    started: bool,
    /// The current line is over [`MAX_LINE`]: skip up to its end.
    skipping: bool,
}

impl SseParser {
    pub fn new() -> SseParser {
        SseParser::default()
    }

    /// Feeds a chunk; returns the events it completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        let mut chunk = chunk;
        if self.after_cr && !chunk.is_empty() {
            self.after_cr = false;
            if chunk[0] == b'\n' {
                chunk = &chunk[1..];
            }
        }
        let mut start = 0;
        let mut i = 0;
        while i < chunk.len() {
            let b = chunk[i];
            if b == b'\n' || b == b'\r' {
                if self.skipping {
                    self.skipping = false;
                    self.buf.clear();
                } else {
                    self.buf.extend_from_slice(&chunk[start..i]);
                    let line = std::mem::take(&mut self.buf);
                    self.line(&line, &mut out);
                }
                if b == b'\r' {
                    if i + 1 < chunk.len() {
                        if chunk[i + 1] == b'\n' {
                            i += 1;
                        }
                    } else {
                        self.after_cr = true;
                    }
                }
                start = i + 1;
            }
            i += 1;
        }
        if !self.skipping {
            self.buf.extend_from_slice(&chunk[start..]);
            if self.buf.len() > MAX_LINE {
                self.buf = Vec::new();
                self.skipping = true;
            }
        }
        out
    }

    /// The stream ended: a last event without its blank line.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.buf.is_empty() && !self.skipping {
            let line = std::mem::take(&mut self.buf);
            self.line(&line, &mut out);
        }
        self.buf.clear();
        self.skipping = false;
        self.dispatch(&mut out);
        out
    }

    fn line(&mut self, line: &[u8], out: &mut Vec<SseEvent>) {
        let mut line = line;
        if !self.started {
            self.started = true;
            line = line.strip_prefix("\u{feff}".as_bytes()).unwrap_or(line);
        }
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        if line[0] == b':' {
            return; // a comment (keep-alive, "OPENROUTER PROCESSING")
        }
        let text = String::from_utf8_lossy(line);
        let (field, value) = match text.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (text.as_ref(), ""),
        };
        match field {
            "data" => self.data.push(value.to_string()),
            "event" => self.event = Some(value.to_string()),
            _ => {} // id, retry and unknown fields
        }
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        let event = self.event.take();
        if self.data.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.data).join("\n");
        out.push(SseEvent { event: event.filter(|e| !e.is_empty()), data });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(event: Option<&str>, data: &str) -> SseEvent {
        SseEvent { event: event.map(String::from), data: data.into() }
    }

    /// Feeds `input` split at every possible position and in single bytes: same events.
    fn all_splits(input: &[u8]) -> Vec<SseEvent> {
        let mut whole = SseParser::new();
        let mut want = whole.feed(input);
        want.extend(whole.finish());
        for cut in 0..=input.len() {
            let mut p = SseParser::new();
            let mut got = p.feed(&input[..cut]);
            got.extend(p.feed(&input[cut..]));
            got.extend(p.finish());
            assert_eq!(got, want, "split at {cut}");
        }
        let mut p = SseParser::new();
        let mut got = Vec::new();
        for b in input {
            got.extend(p.feed(std::slice::from_ref(b)));
        }
        got.extend(p.finish());
        assert_eq!(got, want, "byte by byte");
        want
    }

    #[test]
    fn events_lines_and_comments() {
        let s = b": keep-alive\n\ndata: {\"a\":1}\n\nevent: ping\ndata: x\nid: 7\nretry: 100\n\ndata: [DONE]\n\n";
        assert_eq!(all_splits(s), vec![ev(None, "{\"a\":1}"), ev(Some("ping"), "x"), ev(None, "[DONE]")]);
    }

    #[test]
    fn line_ends_crlf_and_cr() {
        let s = b"data: one\r\n\r\ndata: two\r\rdata:three\n\n";
        assert_eq!(all_splits(s), vec![ev(None, "one"), ev(None, "two"), ev(None, "three")]);
        // A CR at the end of a chunk and its LF in the next one end one line, not two.
        let mut p = SseParser::new();
        assert!(p.feed(b"data: a\r").is_empty());
        assert!(p.feed(b"\ndata: b\r").is_empty());
        assert_eq!(p.feed(b"\n\r\n"), vec![ev(None, "a\nb")]);
    }

    #[test]
    fn multi_line_data_and_field_forms() {
        let s = b"data: first\ndata:second\ndata\ndata:  two spaces\n\n";
        assert_eq!(all_splits(s), vec![ev(None, "first\nsecond\n\n two spaces")]);
        // No data: nothing is dispatched, and the event name does not leak into the next one.
        assert_eq!(all_splits(b"event: lonely\n\ndata: x\n\n"), vec![ev(None, "x")]);
    }

    #[test]
    fn utf8_split_inside_a_character() {
        let s = "data: Grüße 👋 ok\n\n".as_bytes();
        let got = all_splits(s);
        assert_eq!(got, vec![ev(None, "Grüße 👋 ok")]);
    }

    #[test]
    fn final_event_without_blank_line_and_bom() {
        let mut p = SseParser::new();
        assert_eq!(p.feed("\u{feff}data: a\n\ndata: b".as_bytes()), vec![ev(None, "a")]);
        assert_eq!(p.finish(), vec![ev(None, "b")]);
        let mut p = SseParser::new();
        assert!(p.feed(b"event: done\ndata: z\n").is_empty());
        assert_eq!(p.finish(), vec![ev(Some("done"), "z")]);
        assert!(p.finish().is_empty());
    }

    #[test]
    fn overlong_line_is_dropped() {
        let mut p = SseParser::new();
        let big = vec![b'x'; MAX_LINE + 10];
        assert!(p.feed(b"data: ").is_empty());
        assert!(p.feed(&big).is_empty());
        assert!(p.feed(&big).is_empty());
        assert_eq!(p.feed(b"\n\ndata: ok\n\n"), vec![ev(None, "ok")]);
    }
}
