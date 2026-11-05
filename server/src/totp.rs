//! TOTP second factor (RFC 6238: HMAC-SHA1, 6 digits, 30 s, ±1 step) and backup codes
//! (docs/sync/PLAN.md §4.2). A small implementation instead of `totp-rs`: it is a few lines
//! over `hmac`/`sha1`, checked against the RFC's own vectors below.
//!
//! Backup codes are 12 Crockford base32 symbols (60 random bits), shown as `XXXX-XXXX-XXXX`.
//! They are stored as an HMAC under the server's token key, not with Argon2id as PLAN §4.2
//! first said: 60 random bits can't be guessed offline the way a password can, and one login
//! would otherwise cost up to ten Argon2id runs.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

pub const DIGITS: u32 = 6;
pub const PERIOD: u64 = 30;
pub const SECRET_LEN: usize = 20;
pub const BACKUP_CODES: usize = 10;

/// The 6-digit code for one time step.
pub fn code_at(secret: &[u8], step: u64) -> u32 {
    let mut m = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("HMAC takes any key length");
    m.update(&step.to_be_bytes());
    let h = m.finalize().into_bytes();
    let off = (h[h.len() - 1] & 0x0f) as usize;
    let bin = u32::from_be_bytes([h[off] & 0x7f, h[off + 1], h[off + 2], h[off + 3]]);
    bin % 10u32.pow(DIGITS)
}

/// Checks a code at `unix_time` with ±1 step of drift. Returns the matching step, which the
/// caller must store so the same (or an older) step is never accepted again.
pub fn verify(secret: &[u8], code: &str, unix_time: u64, last_step: u64) -> Option<u64> {
    let code = code.trim();
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let want: u32 = code.parse().ok()?;
    let now = unix_time / PERIOD;
    let mut found = None;
    // Check every candidate (no early exit), so timing doesn't tell which step matched.
    for step in [now.saturating_sub(1), now, now + 1] {
        if code_at(secret, step) == want && step > last_step {
            found = Some(step);
        }
    }
    found
}

/// RFC 4648 base32 without padding, as authenticator apps expect.
pub fn base32(bytes: &[u8]) -> String {
    const A: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(A[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(A[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn uri_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn otpauth_uri(secret_b32: &str, email: &str) -> String {
    format!(
        "otpauth://totp/Krunk:{}?secret={secret_b32}&issuer=Krunk&algorithm=SHA1&digits={DIGITS}&period={PERIOD}",
        uri_escape(email)
    )
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A new backup code, `XXXX-XXXX-XXXX`.
pub fn new_backup_code() -> String {
    let r = crate::crypto::random::<8>();
    let v = u64::from_be_bytes(r) >> 4; // 60 bits
    let s: String = (0..12).rev().map(|i| CROCKFORD[((v >> (i * 5)) & 31) as usize] as char).collect();
    format!("{}-{}-{}", &s[..4], &s[4..8], &s[8..])
}

/// The canonical form of a typed backup code (case, dashes and spaces ignored; O→0, I/L→1), or
/// None if it can't be one.
pub fn normalize_backup_code(s: &str) -> Option<String> {
    let mut out = String::with_capacity(12);
    for c in s.chars() {
        let c = match c.to_ascii_uppercase() {
            '-' | ' ' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !CROCKFORD.contains(&(c as u8)) || !c.is_ascii() {
            return None;
        }
        out.push(c);
        if out.len() > 12 {
            return None;
        }
    }
    (out.len() == 12).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 Appendix B, SHA-1 (8 digits there; the last 6 digits are ours).
    #[test]
    fn rfc6238_vectors() {
        let secret = b"12345678901234567890";
        for (t, code8) in [
            (59u64, 94287082u32),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
        ] {
            assert_eq!(code_at(secret, t / PERIOD), code8 % 1_000_000, "t={t}");
        }
    }

    #[test]
    fn window_and_replay() {
        let secret = b"12345678901234567890";
        let t = 1111111111u64;
        let now = t / PERIOD;
        let code = |s: u64| format!("{:06}", code_at(secret, s));
        assert_eq!(verify(secret, &code(now), t, 0), Some(now));
        assert_eq!(verify(secret, &code(now - 1), t, 0), Some(now - 1));
        assert_eq!(verify(secret, &code(now + 1), t, 0), Some(now + 1));
        assert_eq!(verify(secret, &code(now + 2), t, 0), None);
        assert_eq!(verify(secret, &code(now), t, now), None, "the same step twice is a replay");
        assert_eq!(verify(secret, &code(now - 1), t, now - 1), None);
        assert_eq!(verify(secret, "12345", t, 0), None);
        assert_eq!(verify(secret, "12a456", t, 0), None);
    }

    #[test]
    fn encodings() {
        // RFC 4648 §10.
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(&[0u8; 20]).len(), 32);
        let uri = otpauth_uri("MZXW6YTBOI", "a+b@example.org");
        assert_eq!(
            uri,
            "otpauth://totp/Krunk:a%2Bb%40example.org?secret=MZXW6YTBOI&issuer=Krunk&algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn backup_codes() {
        let c = new_backup_code();
        assert_eq!(c.len(), 14);
        let n = normalize_backup_code(&c).unwrap();
        assert_eq!(n, c.replace('-', ""));
        assert_eq!(normalize_backup_code(&c.to_lowercase()).as_deref(), Some(n.as_str()));
        assert_eq!(normalize_backup_code("o1il-0000-0000").as_deref(), Some("011100000000"));
        assert_eq!(normalize_backup_code("0000-0000-000"), None);
        assert_eq!(normalize_backup_code("0000-0000-0000-0"), None);
        assert_eq!(normalize_backup_code("0000-0000-000U"), None, "U is not Crockford");
        assert_ne!(new_backup_code(), new_backup_code());
    }
}
