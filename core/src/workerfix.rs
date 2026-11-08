//! "Send client hints" off in shared workers (Krunk 0.7.1, R12). A shared worker gets the
//! engine's user-agent metadata straight from the browser process and is no DevTools target
//! of any tab, so the only place Krunk can reach it is its script: while the setting is off,
//! a shared worker's main script gets one statement in front that removes
//! `navigator.userAgentData` before any of its own code runs (scripts it loads later with
//! `importScripts` run after that).
//!
//! The statement goes after a directive prologue (`'use strict';` keeps working) and on the
//! first line (line numbers stay). Not reached: the modules a module worker imports statically
//! (they run before its body; CEF fails a worker's own requests once Krunk handles them) and
//! `blob:`/`data:` workers (not loaded over the network).

/// What goes in front (a leading `;` also ends a directive written without one).
pub const PREFIX: &[u8] = b";try{delete WorkerNavigator.prototype.userAgentData}catch(e){};";

/// How much of a script's start is looked at for the prologue; past it the statement goes
/// at the very start (still valid; only a directive after a 4 KiB comment would lose effect).
pub const LOOKAHEAD: usize = 4096;

/// Is this response's MIME type a script?
pub fn is_script_mime(mime: &str) -> bool {
    let m = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(
        m.as_str(),
        "text/javascript"
            | "application/javascript"
            | "application/x-javascript"
            | "text/ecmascript"
            | "application/ecmascript"
            | "text/jscript"
            | "module"
    )
}

/// Inserts [`PREFIX`] into a script that arrives in pieces.
#[derive(Debug, Default)]
pub struct PrefixInserter {
    /// The start of the script while the place is not known yet.
    head: Vec<u8>,
    done: bool,
}

/// Where the statement goes in `head`, if that can be said yet (`end`: no more input).
fn insertion_point(b: &[u8], end: bool) -> Option<usize> {
    // UTF-16 (a byte-order mark): left alone, an ASCII insertion would break it.
    if b.starts_with(&[0xFF, 0xFE]) || b.starts_with(&[0xFE, 0xFF]) {
        return Some(usize::MAX);
    }
    if !end && (b.is_empty() || (b.len() < 3 && [0xEFu8, 0xBB, 0xBF].starts_with(b)) || b == [0xFF] || b == [0xFE]) {
        return None;
    }
    let mut i = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    // A hashbang line.
    if b[i..].starts_with(b"#!") {
        match b[i..].iter().position(|&c| c == b'\n') {
            Some(n) => i += n + 1,
            None => return if end { Some(b.len()) } else { None },
        }
    } else if b.len() == i + 1 && b[i] == b'#' && !end {
        return None;
    }
    let mut point: Option<usize> = None;
    loop {
        // Space and comments before a statement.
        let code = match skip_space(b, i, end) {
            Skip::At(p, _) => p,
            Skip::More => return None,
        };
        let q = match b.get(code) {
            None => return Some(point.unwrap_or(code)),
            Some(&q) => q,
        };
        if q != b'\'' && q != b'"' {
            return Some(point.unwrap_or(code));
        }
        // A string literal: a directive if it is a statement of its own.
        let close = match string_end(b, code, q) {
            Lit::End(p) => p,
            Lit::More => return if end { Some(point.unwrap_or(code)) } else { None },
            Lit::Bad => return Some(point.unwrap_or(code)),
        };
        match skip_space(b, close, end) {
            Skip::More => return None,
            Skip::At(p, newline) => match b.get(p) {
                Some(b';') => {
                    i = p + 1;
                    point = Some(i);
                }
                None => return Some(close),
                Some(&c) if newline && !continues(c) => {
                    i = close;
                    point = Some(close);
                }
                // `'a' + x`, `'a'.length`: an expression, not a directive.
                Some(_) => return Some(point.unwrap_or(code)),
            },
        }
    }
}

/// Characters that make a string literal on the previous line part of an expression.
fn continues(c: u8) -> bool {
    b"+-*/%&|^<>=,?.([`!:".contains(&c)
}

enum Skip {
    /// The first byte that is not space or comment, and whether a line ended on the way.
    At(usize, bool),
    More,
}

fn skip_space(b: &[u8], mut i: usize, end: bool) -> Skip {
    let mut newline = false;
    loop {
        match b.get(i) {
            None => return if end { Skip::At(i, newline) } else { Skip::More },
            Some(b' ' | b'\t' | 0x0B | 0x0C) => i += 1,
            Some(b'\n' | b'\r') => {
                newline = true;
                i += 1;
            }
            Some(b'/') => match b.get(i + 1) {
                None if !end => return Skip::More,
                Some(b'/') => match b[i..].iter().position(|&c| c == b'\n') {
                    Some(n) => i += n,
                    None => return if end { Skip::At(b.len(), newline) } else { Skip::More },
                },
                Some(b'*') => match b[i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(n) => {
                        if b[i + 2..i + 2 + n].iter().any(|&c| c == b'\n' || c == b'\r') {
                            newline = true;
                        }
                        i += n + 4;
                    }
                    None => return if end { Skip::At(i, newline) } else { Skip::More },
                },
                _ => return Skip::At(i, newline),
            },
            // A non-breaking space (UTF-8 C2 A0).
            Some(0xC2) if b.get(i + 1) == Some(&0xA0) => i += 2,
            Some(_) => return Skip::At(i, newline),
        }
    }
}

enum Lit {
    /// Just after the closing quote.
    End(usize),
    More,
    Bad,
}

fn string_end(b: &[u8], start: usize, quote: u8) -> Lit {
    let mut i = start + 1;
    loop {
        match b.get(i) {
            None => return Lit::More,
            Some(&c) if c == quote => return Lit::End(i + 1),
            Some(b'\\') => i += 2,
            Some(b'\n' | b'\r') => return Lit::Bad,
            Some(_) => i += 1,
        }
    }
}

impl PrefixInserter {
    pub fn new() -> PrefixInserter {
        PrefixInserter::default()
    }

    /// The next piece of the script; what can be passed on goes to `out`.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) {
        if self.done {
            out.extend_from_slice(input);
            return;
        }
        self.head.extend_from_slice(input);
        let point = insertion_point(&self.head, false).or_else(|| (self.head.len() >= LOOKAHEAD).then_some(0));
        if let Some(p) = point {
            self.emit(p, out);
        }
    }

    /// No more input: what is still held goes to `out`.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        if !self.done {
            let p = insertion_point(&self.head, true).unwrap_or(0);
            self.emit(p, out);
        }
    }

    fn emit(&mut self, point: usize, out: &mut Vec<u8>) {
        self.done = true;
        let head = std::mem::take(&mut self.head);
        if point == usize::MAX {
            out.extend_from_slice(&head);
            return;
        }
        // Never between the bytes of a BOM.
        let bom = if head.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
        let p = point.clamp(bom, head.len());
        out.extend_from_slice(&head[..p]);
        out.extend_from_slice(PREFIX);
        out.extend_from_slice(&head[p..]);
    }
}

/// A response filter's state: the inserter plus output that did not fit yet. CEF calls
/// `step` with pieces of the body and, after the last one, with an empty piece when output
/// is pending (`step` reports that by returning `true`).
#[derive(Debug, Default)]
pub struct FilterState {
    inserter: PrefixInserter,
    pending: Vec<u8>,
    sent: usize,
    finished: bool,
}

impl FilterState {
    pub fn new() -> FilterState {
        FilterState::default()
    }

    /// Takes all of `input` (empty = the body is complete), writes up to `out.len()` bytes and
    /// returns (bytes written, more output pending).
    pub fn step(&mut self, input: &[u8], out: &mut [u8]) -> (usize, bool) {
        if self.sent == self.pending.len() {
            self.pending.clear();
            self.sent = 0;
        }
        if input.is_empty() {
            if !self.finished {
                self.finished = true;
                self.inserter.finish(&mut self.pending);
            }
        } else {
            self.inserter.push(input, &mut self.pending);
        }
        let n = (self.pending.len() - self.sent).min(out.len());
        out[..n].copy_from_slice(&self.pending[self.sent..self.sent + n]);
        self.sent += n;
        // Held back: the script's start while its prologue is read, or output that did not fit.
        let held = !self.inserter.done || self.sent < self.pending.len();
        (n, held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whole(src: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut p = PrefixInserter::new();
        p.push(src, &mut out);
        p.finish(&mut out);
        out
    }

    /// Every split of `src` into two pieces gives the same result as one piece.
    fn every_split(src: &[u8]) -> Vec<u8> {
        let one = whole(src);
        for cut in 0..=src.len() {
            let mut out = Vec::new();
            let mut p = PrefixInserter::new();
            p.push(&src[..cut], &mut out);
            p.push(&src[cut..], &mut out);
            p.finish(&mut out);
            assert_eq!(out, one, "split at {cut} of {:?}", String::from_utf8_lossy(src));
        }
        one
    }

    fn with(before: &str, after: &str) -> Vec<u8> {
        [before.as_bytes(), PREFIX, after.as_bytes()].concat()
    }

    #[test]
    fn the_statement_goes_after_the_directives_on_the_first_line() {
        assert_eq!(every_split(b"onconnect = f;\nx();"), with("", "onconnect = f;\nx();"));
        assert_eq!(every_split(b"'use strict';\nf();"), with("'use strict';", "\nf();"));
        assert_eq!(every_split(b"\"use strict\"; 'other';\nf();"), with("\"use strict\"; 'other';", "\nf();"));
        // A directive ended by the line: the statement's own `;` ends it.
        assert_eq!(every_split(b"'use strict'\nf();"), with("'use strict'", "\nf();"));
        assert_eq!(
            every_split(b"\xEF\xBB\xBF/* licence */\n// note\n'use strict';\nf();"),
            with("\u{feff}/* licence */\n// note\n'use strict';", "\nf();")
        );
        assert_eq!(every_split(b"#!/usr/bin/env node\nf();"), with("#!/usr/bin/env node\n", "f();"));
        // Modules: in front of the imports (they run first, and get the statement themselves).
        assert_eq!(every_split(b"import { a } from '/a.js';\na();"), with("", "import { a } from '/a.js';\na();"));
    }

    #[test]
    fn strings_that_are_not_directives_are_left_alone() {
        assert_eq!(every_split(b"'a' + b;"), with("", "'a' + b;"));
        assert_eq!(every_split(b"'use strict'\n+ x;"), with("", "'use strict'\n+ x;"));
        assert_eq!(every_split(b"'use strict'.length;"), with("", "'use strict'.length;"));
        assert_eq!(every_split(b"'it\\'s';\nf();"), with("'it\\'s';", "\nf();"));
        assert_eq!(every_split(b"'broken\nf();"), with("", "'broken\nf();"));
    }

    #[test]
    fn short_odd_and_long_scripts() {
        assert_eq!(every_split(b""), PREFIX.to_vec());
        assert_eq!(every_split(b"'use strict'"), with("'use strict'", ""));
        assert_eq!(every_split(b"// only a comment"), with("// only a comment", ""));
        assert_eq!(every_split(b"/* unterminated"), with("", "/* unterminated"));
        // UTF-16 is passed on unchanged.
        assert_eq!(every_split(b"\xFF\xFEa\x00"), b"\xFF\xFEa\x00".to_vec());
        // A prologue behind a comment longer than the lookahead: at the start.
        let long = format!("/*{}*/'use strict';f();", "x".repeat(2 * LOOKAHEAD));
        let mut out = Vec::new();
        let mut p = PrefixInserter::new();
        for chunk in long.as_bytes().chunks(1000) {
            p.push(chunk, &mut out);
        }
        p.finish(&mut out);
        assert_eq!(out, with("", &long));
    }

    #[test]
    fn script_types() {
        for m in ["text/javascript", "application/javascript; charset=utf-8", "TEXT/ECMASCRIPT"] {
            assert!(is_script_mime(m), "{m}");
        }
        for m in ["text/html", "application/json", "", "text/plain"] {
            assert!(!is_script_mime(m), "{m}");
        }
    }

    #[test]
    fn the_filter_state_follows_cefs_protocol() {
        // A body in three pieces, an output buffer of 7 bytes: everything arrives, in order,
        // and the last call (empty input) flushes what was held.
        let src = b"'use strict';\nonconnect = (e) => e.ports[0].postMessage(1);";
        let want = whole(src);
        let mut f = FilterState::new();
        let mut got = Vec::new();
        let mut buf = [0u8; 7];
        for piece in src.chunks(20) {
            let (n, _) = f.step(piece, &mut buf);
            got.extend_from_slice(&buf[..n]);
        }
        loop {
            let (n, more) = f.step(&[], &mut buf);
            got.extend_from_slice(&buf[..n]);
            if !more {
                break;
            }
        }
        assert_eq!(got, want);
        // A body shorter than any decision: held until the end, then complete.
        let mut f = FilterState::new();
        let mut big = [0u8; 256];
        let (n, more) = f.step(b"'use", &mut big);
        assert_eq!((n, more), (0, true));
        let (n, more) = f.step(&[], &mut big);
        assert!(!more);
        assert_eq!(&big[..n], whole(b"'use").as_slice());
    }
}
