//! Small cryptographic helpers shared by the account code: random tokens, keyed hashes,
//! constant-time comparison and the TOTP-secret box (docs/sync/PLAN.md §4.2, §4.3).

use base64ct::{Base64UrlUnpadded, Encoding};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Outbound TLS (siteverify, SMTP) uses rustls with the ring provider; installing it twice is
/// harmless.
pub fn ensure_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    // The OS random source failing is not recoverable.
    getrandom::fill(&mut b).expect("the OS random source failed");
    b
}

/// A new opaque token (32 random bytes, base64url): session cookies, email links.
pub fn new_token() -> String {
    Base64UrlUnpadded::encode_string(&random::<32>())
}

/// HMAC-SHA256 over length-prefixed parts, so ("ab","c") and ("a","bc") differ.
pub fn hmac(key: &[u8; 32], label: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <HmacSha256 as hmac::KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    for p in std::iter::once(label.as_bytes()).chain(parts.iter().copied()) {
        m.update(&(p.len() as u32).to_be_bytes());
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub fn b64url(bytes: &[u8]) -> String {
    Base64UrlUnpadded::encode_string(bytes)
}

/// Strict base64url (no padding) of an exact length.
pub fn b64url_decode<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() > N.div_ceil(3) * 4 {
        return None;
    }
    Base64UrlUnpadded::decode_vec(s).ok()?.try_into().ok()
}

/// XChaCha20-Poly1305 with a random 24-byte nonce: `nonce || ciphertext`.
pub fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = random::<24>();
    let ct = cipher
        .encrypt(&XNonce::from(nonce), Payload { msg: plaintext, aad })
        .expect("encryption of a short message cannot fail");
    [nonce.as_slice(), &ct].concat()
}

pub fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 24 + 16 {
        return None;
    }
    let (nonce, ct) = sealed.split_at(24);
    let nonce = XNonce::try_from(nonce).ok()?;
    XChaCha20Poly1305::new(key.into()).decrypt(&nonce, Payload { msg: ct, aad }).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_parts_are_framed() {
        let k = [7u8; 32];
        assert_ne!(hmac(&k, "x", &[b"ab", b"c"]), hmac(&k, "x", &[b"a", b"bc"]));
        assert_ne!(hmac(&k, "x", &[b"a"]), hmac(&k, "y", &[b"a"]));
        assert_eq!(hmac(&k, "x", &[b"a"]), hmac(&k, "x", &[b"a"]));
    }

    #[test]
    fn tokens_and_box() {
        let t = new_token();
        assert_eq!(t.len(), 43);
        assert!(b64url_decode::<32>(&t).is_some());
        assert!(b64url_decode::<32>(&format!("{t}=")).is_none());
        assert!(b64url_decode::<32>("short").is_none());
        let k = [1u8; 32];
        let s = seal(&k, b"acct", b"secret");
        assert_eq!(open(&k, b"acct", &s).unwrap(), b"secret");
        assert!(open(&k, b"other", &s).is_none(), "the AAD binds the box to its account");
        assert!(open(&[2u8; 32], b"acct", &s).is_none());
        let mut flipped = s.clone();
        flipped[30] ^= 1;
        assert!(open(&k, b"acct", &flipped).is_none());
        assert!(ct_eq(b"abc", b"abc") && !ct_eq(b"abc", b"abd") && !ct_eq(b"abc", b"ab"));
    }
}
