//! Incremental Server-Sent Events parser (WP-A).
//!
//! Bytes arrive in arbitrary chunks (a UTF-8 character or a line may be split); `feed` returns
//! every complete event so far. Handles `\n`, `\r\n` and `\r` line ends, `:` comments, multi-line
//! `data:` (joined with `\n`), `event:` names and a final event without a blank line (`finish`).

/// One event: its `event:` name (if any) and its data lines joined.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub fn new() -> SseParser {
        SseParser::default()
    }

    /// Feeds a chunk; returns the events it completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        Vec::new() // WP-A
    }

    /// The stream ended: a last event without its blank line.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let _ = (&self.event, &self.data);
        Vec::new() // WP-A
    }
}
