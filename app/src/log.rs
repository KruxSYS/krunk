//! Minimal file logger (`krunk.log` in the data dir).
//!
//! Never log URLs with credentials, form data, passwords or vault contents.
//! Each start begins a new log; the previous one is kept as `krunk.old.log`
//! (so the lines before an update's restart survive it). Capped in size.

use std::io::Write;
use std::sync::Mutex;

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);
const MAX_BYTES: u64 = 1024 * 1024;

/// Opens the log; `rotate`: the previous one becomes `krunk.old.log` first (else this process
/// appends to the running Krunk's log).
pub fn init(path: &std::path::Path, rotate: bool) {
    if rotate {
        let _ = std::fs::rename(path, path.with_extension("old.log"));
    }
    let file = if rotate {
        std::fs::File::create(path)
    } else {
        std::fs::OpenOptions::new().create(true).append(true).open(path)
    };
    if let Ok(f) = file {
        if let Ok(mut g) = LOG.lock() {
            *g = Some(f);
        }
    }
}

pub fn write(level: &str, msg: &str) {
    let Ok(mut g) = LOG.lock() else { return };
    let Some(f) = g.as_mut() else { return };
    if f.metadata().map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        return;
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = writeln!(f, "{secs} {level} {msg}");
}

macro_rules! info {
    ($($t:tt)*) => { $crate::log::write("INFO", &format!($($t)*)) };
}
macro_rules! warning {
    ($($t:tt)*) => { $crate::log::write("WARN", &format!($($t)*)) };
}
pub(crate) use {info, warning};
