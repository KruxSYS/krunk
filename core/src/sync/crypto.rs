//! Primitives of the sync protocol (PROTOCOL §2, §3.3, §5): length-prefixed encoding,
//! HKDF-SHA256, HMAC-SHA256, XChaCha20-Poly1305, the passphrase KDF with its floors, the
//! recovery-root wraps, the escrow keypair and HPKE seals.

use super::{labels, AccountId, SyncError};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as _, OpModeR, OpModeS, Serializable};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub type Key = Zeroizing<[u8; 32]>;
type HmacSha256 = Hmac<Sha256>;
type HpkeAead = hpke::aead::ChaCha20Poly1305;
type HpkeKdf = hpke::kdf::HkdfSha256;
type HpkeKem = X25519HkdfSha256;

pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;

// ---------------------------------------------------------------- encoding

/// Appends `lp(x)`: 2-byte big-endian length, then `x`.
pub fn put_lp(out: &mut Vec<u8>, x: &[u8]) -> Result<(), SyncError> {
    let len = u16::try_from(x.len()).map_err(|_| SyncError::TooLarge)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(x);
    Ok(())
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// OS randomness.
pub fn random<const N: usize>() -> Result<[u8; N], SyncError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| SyncError::Random)?;
    Ok(b)
}

// ---------------------------------------------------------------- HKDF / HMAC

/// `HKDF-SHA256(ikm, salt, info)` → 32 bytes.
pub fn hkdf32(ikm: &[u8], salt: &[u8], info: &str) -> Key {
    let mut out = Zeroizing::new([0u8; 32]);
    #[allow(clippy::expect_used)] // 32 bytes is far below HKDF's 255 × 32 limit
    Hkdf::<Sha256>::new(Some(salt), ikm).expand(info.as_bytes(), out.as_mut()).expect("32-byte HKDF output");
    out
}

pub fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    #[allow(clippy::expect_used)] // HMAC takes keys of any length
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC key");
    m.update(msg);
    m.finalize().into_bytes().into()
}

/// Constant-time check of an HMAC-SHA256 tag.
pub fn hmac_verify(key: &[u8], msg: &[u8], tag: &[u8]) -> Result<(), SyncError> {
    #[allow(clippy::expect_used)]
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC key");
    m.update(msg);
    m.verify_slice(tag).map_err(|_| SyncError::Auth)
}

// ---------------------------------------------------------------- AEAD

/// XChaCha20-Poly1305 with a fresh random 24-byte nonce.
pub fn aead_seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<([u8; NONCE_LEN], Vec<u8>), SyncError> {
    let nonce = random::<NONCE_LEN>()?;
    Ok((nonce, aead_seal_with_nonce(key, &nonce, aad, plaintext)?))
}

/// Deterministic variant, for test vectors only (callers in production use [`aead_seal`]).
pub fn aead_seal_with_nonce(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, SyncError> {
    #[allow(clippy::expect_used)]
    let cipher = XChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    cipher.encrypt(&XNonce::from(*nonce), Payload { msg: plaintext, aad }).map_err(|_| SyncError::TooLarge)
}

pub fn aead_open(key: &[u8; 32], nonce: &[u8], aad: &[u8], ct: &[u8]) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| SyncError::Malformed("nonce"))?;
    #[allow(clippy::expect_used)]
    let cipher = XChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    cipher.decrypt(&XNonce::from(nonce), Payload { msg: ct, aad }).map(Zeroizing::new).map_err(|_| SyncError::Auth)
}

// ---------------------------------------------------------------- passphrase KDF (PROTOCOL §5.1)

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

/// The shipped parameters, which are also the floor: anything weaker is refused [R1].
pub const PASSPHRASE_KDF: KdfParams = KdfParams { m_kib: 131072, t: 3, p: 1 };

pub fn check_floor(p: &KdfParams) -> Result<(), SyncError> {
    // p is fixed at 1 so the cost can't be shifted onto attacker-friendly parallelism.
    if p.m_kib < PASSPHRASE_KDF.m_kib || p.t < PASSPHRASE_KDF.t || p.p != 1 || p.m_kib > 4 * 1024 * 1024 || p.t > 64 {
        return Err(SyncError::WeakKdf);
    }
    Ok(())
}

/// `KEK_pp = Argon2id(passphrase, salt, m, t, p)` (32 bytes). Re-wraps always use
/// [`PASSPHRASE_KDF`]; stored parameters are only accepted when they meet the floor.
pub fn passphrase_kek(passphrase: &[u8], salt: &[u8; 16], params: &KdfParams) -> Result<Key, SyncError> {
    check_floor(params)?;
    let p = argon2::Params::new(params.m_kib, params.t, params.p, Some(32)).map_err(|_| SyncError::WeakKdf)?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p);
    let mut out = Zeroizing::new([0u8; 32]);
    a.hash_password_into(passphrase, salt, out.as_mut()).map_err(|_| SyncError::WeakKdf)?;
    Ok(out)
}

/// `KEK_rk = HKDF(rk_bytes, account_id, "krunk-sync/v1/rk-kek")` (PROTOCOL §5.2).
pub fn recovery_kek(rk: &[u8; 20], account_id: &AccountId) -> Key {
    hkdf32(rk, account_id, labels::RK_KEK)
}

// ---------------------------------------------------------------- recovery root (PROTOCOL §5.3)

/// Which secret a wrap of RR is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapKind {
    Passphrase,
    RecoveryKey,
    /// The password-vault master password (0.21.1 / Android 0.6.2): the one secret users type.
    Vault,
}

impl WrapKind {
    pub fn label(self) -> &'static str {
        match self {
            WrapKind::Passphrase => "pp",
            WrapKind::RecoveryKey => "rk",
            WrapKind::Vault => "vault",
        }
    }
}

/// `rr_wrap_X = {n, ct, commit}`; `ct` is exactly 48 bytes (32 + tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RrWrap {
    pub n: [u8; NONCE_LEN],
    pub ct: Vec<u8>,
    pub commit: [u8; 32],
}

fn rr_aad(kind: WrapKind, account_id: &AccountId, rr_gen: u32) -> Vec<u8> {
    let mut aad = labels::rr_aad(kind.label()).into_bytes();
    aad.extend_from_slice(account_id);
    aad.extend_from_slice(&rr_gen.to_be_bytes());
    aad
}

fn rr_commit_tag(kek: &[u8; 32], kind: WrapKind, account_id: &AccountId, rr_gen: u32) -> [u8; 32] {
    let k = hkdf32(kek, account_id, &labels::rr_commit(kind.label()));
    let mut msg = account_id.to_vec();
    msg.extend_from_slice(&rr_gen.to_be_bytes());
    hmac(k.as_ref(), &msg)
}

pub fn rr_wrap(
    kek: &[u8; 32],
    rr: &[u8; 32],
    kind: WrapKind,
    account_id: &AccountId,
    rr_gen: u32,
) -> Result<RrWrap, SyncError> {
    let n = random::<NONCE_LEN>()?;
    rr_wrap_with_nonce(kek, rr, kind, account_id, rr_gen, &n)
}

pub fn rr_wrap_with_nonce(
    kek: &[u8; 32],
    rr: &[u8; 32],
    kind: WrapKind,
    account_id: &AccountId,
    rr_gen: u32,
    n: &[u8; NONCE_LEN],
) -> Result<RrWrap, SyncError> {
    let ct = aead_seal_with_nonce(kek, n, &rr_aad(kind, account_id, rr_gen), rr)?;
    Ok(RrWrap { n: *n, ct, commit: rr_commit_tag(kek, kind, account_id, rr_gen) })
}

/// Checks the key commitment first (constant time), then the exact length, then decrypts [R18].
pub fn rr_unwrap(
    kek: &[u8; 32],
    w: &RrWrap,
    kind: WrapKind,
    account_id: &AccountId,
    rr_gen: u32,
) -> Result<Key, SyncError> {
    let expected = rr_commit_tag(kek, kind, account_id, rr_gen);
    let k = hkdf32(kek, account_id, &labels::rr_commit(kind.label()));
    let mut msg = account_id.to_vec();
    msg.extend_from_slice(&rr_gen.to_be_bytes());
    hmac_verify(k.as_ref(), &msg, &w.commit)?;
    debug_assert_eq!(expected, w.commit);
    if w.ct.len() != 32 + TAG_LEN {
        return Err(SyncError::Malformed("rr_wrap length"));
    }
    let pt = aead_open(kek, &w.n, &rr_aad(kind, account_id, rr_gen), &w.ct)?;
    let mut rr = Zeroizing::new([0u8; 32]);
    rr.copy_from_slice(&pt);
    Ok(rr)
}

// ---------------------------------------------------------------- X25519 keys and HPKE seals

/// An X25519 keypair as raw bytes (the private key is zeroized on drop).
pub struct KeyPair {
    pub sk: Key,
    pub pk: [u8; 32],
}

impl KeyPair {
    /// The public key of a 32-byte private key (clamped by X25519).
    pub fn from_secret(sk: [u8; 32]) -> KeyPair {
        let secret = x25519_dalek::StaticSecret::from(sk);
        let pk = x25519_dalek::PublicKey::from(&secret).to_bytes();
        KeyPair { sk: Zeroizing::new(sk), pk }
    }

    pub fn generate() -> Result<KeyPair, SyncError> {
        Ok(KeyPair::from_secret(random::<32>()?))
    }
}

/// `esc_sk = HKDF(RR, account_id, "krunk-sync/v1/escrow-x25519")`.
pub fn escrow_keypair(rr: &[u8; 32], account_id: &AccountId) -> KeyPair {
    KeyPair::from_secret(*hkdf32(rr, account_id, labels::ESCROW_X25519))
}

/// Refuses public keys of small order: their shared secret is all zeros for every scalar.
pub fn check_public_key(pk: &[u8; 32]) -> Result<(), SyncError> {
    let probe = x25519_dalek::StaticSecret::from([0x5a; 32]);
    if probe.diffie_hellman(&x25519_dalek::PublicKey::from(*pk)).was_contributory() {
        Ok(())
    } else {
        Err(SyncError::LowOrderKey)
    }
}

/// What a seal carries (PROTOCOL §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealKind {
    Ak,
    Pak,
}

impl SealKind {
    pub fn label(self) -> &'static str {
        match self {
            SealKind::Ak => "ak",
            SealKind::Pak => "pak",
        }
    }
}

/// `info = "krunk-sync/v1/seal" ‖ lp(kind) ‖ account_id ‖ u32be(epoch) ‖ lp(recipient_id)`.
pub fn seal_info(kind: SealKind, account_id: &AccountId, epoch: u32, recipient: &str) -> Result<Vec<u8>, SyncError> {
    let mut info = labels::SEAL.as_bytes().to_vec();
    put_lp(&mut info, kind.label().as_bytes())?;
    info.extend_from_slice(account_id);
    info.extend_from_slice(&epoch.to_be_bytes());
    put_lp(&mut info, recipient.as_bytes())?;
    Ok(info)
}

/// An HPKE (RFC 9180, base mode) seal: `enc` (32 bytes) and the ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    pub enc: [u8; 32],
    pub ct: Vec<u8>,
}

/// Seals `pt` to `pk` (DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305, empty aad).
pub fn seal(pk: &[u8; 32], info: &[u8], pt: &[u8]) -> Result<Sealed, SyncError> {
    seal_with_rng(pk, info, pt, &mut hpke::rand_core::UnwrapErr(getrandom::SysRng))
}

/// Seal with a caller-supplied RNG (deterministic test vectors).
pub fn seal_with_rng(
    pk: &[u8; 32],
    info: &[u8],
    pt: &[u8],
    rng: &mut impl hpke::rand_core::CryptoRng,
) -> Result<Sealed, SyncError> {
    check_public_key(pk)?;
    let pk = <HpkeKem as hpke::Kem>::PublicKey::from_bytes(pk).map_err(|_| SyncError::LowOrderKey)?;
    let (enc, ct) =
        hpke::single_shot_seal_with_rng::<HpkeAead, HpkeKdf, HpkeKem>(&OpModeS::Base, &pk, info, pt, &[], rng)
            .map_err(|_| SyncError::Auth)?;
    let enc: [u8; 32] = enc.to_bytes().as_slice().try_into().map_err(|_| SyncError::Malformed("enc"))?;
    Ok(Sealed { enc, ct })
}

pub fn open(sk: &[u8; 32], info: &[u8], sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    check_public_key(&sealed.enc)?;
    let sk = <HpkeKem as hpke::Kem>::PrivateKey::from_bytes(sk).map_err(|_| SyncError::Malformed("private key"))?;
    let enc = <HpkeKem as hpke::Kem>::EncappedKey::from_bytes(&sealed.enc).map_err(|_| SyncError::Malformed("enc"))?;
    hpke::single_shot_open::<HpkeAead, HpkeKdf, HpkeKem>(&OpModeR::Base, &sk, &enc, info, &sealed.ct, &[])
        .map(Zeroizing::new)
        .map_err(|_| SyncError::Auth)
}

/// HPKE `DeriveKeyPair(ikm)` (RFC 9180 §7.1.3), for vectors and interoperability tests.
pub fn hpke_derive_keypair(ikm: &[u8]) -> ([u8; 32], [u8; 32]) {
    let (sk, pk) = HpkeKem::derive_keypair(ikm);
    let mut s = [0u8; 32];
    s.copy_from_slice(sk.to_bytes().as_slice());
    let mut p = [0u8; 32];
    p.copy_from_slice(pk.to_bytes().as_slice());
    (s, p)
}

/// Opens a 32-byte key from a seal.
pub fn open_key(sk: &[u8; 32], info: &[u8], sealed: &Sealed) -> Result<Key, SyncError> {
    let pt = open(sk, info, sealed)?;
    let key: [u8; 32] = pt.as_slice().try_into().map_err(|_| SyncError::Malformed("sealed key"))?;
    Ok(Zeroizing::new(key))
}

#[cfg(test)]
pub(crate) mod test_rng {
    //! A deterministic CSPRNG stand-in for test vectors: HKDF-expanded blocks from a seed.
    use std::convert::Infallible;

    pub struct DetRng {
        seed: Vec<u8>,
        counter: u64,
    }

    impl DetRng {
        pub fn new(seed: &[u8]) -> DetRng {
            DetRng { seed: seed.to_vec(), counter: 0 }
        }

        /// Yields exactly `bytes` first, then derived blocks (to replay a known `ikmE`).
        pub fn replay(bytes: &[u8]) -> Replay {
            Replay { data: bytes.to_vec(), pos: 0 }
        }
    }

    impl hpke::rand_core::TryRng for DetRng {
        type Error = Infallible;
        fn try_next_u32(&mut self) -> Result<u32, Infallible> {
            let mut b = [0u8; 4];
            self.try_fill_bytes(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn try_next_u64(&mut self) -> Result<u64, Infallible> {
            let mut b = [0u8; 8];
            self.try_fill_bytes(&mut b)?;
            Ok(u64::from_le_bytes(b))
        }
        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
            for chunk in dst.chunks_mut(32) {
                let block = super::hkdf32(&self.seed, &self.counter.to_be_bytes(), "test-rng");
                self.counter += 1;
                chunk.copy_from_slice(&block[..chunk.len()]);
            }
            Ok(())
        }
    }
    impl hpke::rand_core::TryCryptoRng for DetRng {}

    pub struct Replay {
        data: Vec<u8>,
        pos: usize,
    }

    impl hpke::rand_core::TryRng for Replay {
        type Error = Infallible;
        fn try_next_u32(&mut self) -> Result<u32, Infallible> {
            let mut b = [0u8; 4];
            self.try_fill_bytes(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn try_next_u64(&mut self) -> Result<u64, Infallible> {
            let mut b = [0u8; 8];
            self.try_fill_bytes(&mut b)?;
            Ok(u64::from_le_bytes(b))
        }
        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
            for b in dst.iter_mut() {
                *b = self.data.get(self.pos).copied().unwrap_or(0);
                self.pos += 1;
            }
            Ok(())
        }
    }
    impl hpke::rand_core::TryCryptoRng for Replay {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACC: AccountId = [7u8; 16];

    #[test]
    fn lp_encoding() {
        let mut v = Vec::new();
        put_lp(&mut v, b"abc").unwrap();
        assert_eq!(v, [0, 3, b'a', b'b', b'c']);
        assert_eq!(put_lp(&mut Vec::new(), &vec![0u8; 70000]), Err(SyncError::TooLarge));
    }

    #[test]
    fn hkdf_matches_rfc5869_case_1() {
        // RFC 5869 A.1 (L = 42; we check the first 32 bytes).
        let ikm = [0x0b; 22];
        let salt: Vec<u8> = (0x00..=0x0c).collect();
        // The RFC's info is not UTF-8, so this goes through the crate with raw bytes.
        let mut okm = [0u8; 42];
        Hkdf::<Sha256>::new(Some(&salt), &ikm)
            .expand(&[0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9], &mut okm)
            .unwrap();
        assert_eq!(hex(&okm), "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865");
    }

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert!(hmac_verify(b"Jefe", b"what do ya want for nothing?", &hmac(b"Jefe", b"what do ya want for nothing?"))
            .is_ok());
        assert_eq!(hmac_verify(b"Jefe", b"x", &[0u8; 32]), Err(SyncError::Auth));
    }

    #[test]
    fn aead_round_trip_and_tamper() {
        let key = [3u8; 32];
        let (n, ct) = aead_seal(&key, b"aad", b"hello").unwrap();
        assert_eq!(aead_open(&key, &n, b"aad", &ct).unwrap().as_slice(), b"hello");
        assert_eq!(aead_open(&key, &n, b"other", &ct), Err(SyncError::Auth));
        assert_eq!(aead_open(&[4u8; 32], &n, b"aad", &ct), Err(SyncError::Auth));
        assert_eq!(aead_open(&key, &n[..23], b"aad", &ct), Err(SyncError::Malformed("nonce")));
    }

    #[test]
    fn kdf_floor() {
        assert!(check_floor(&PASSPHRASE_KDF).is_ok());
        for weak in [
            KdfParams { m_kib: 65536, t: 3, p: 1 },
            KdfParams { m_kib: 131072, t: 2, p: 1 },
            KdfParams { m_kib: 131072, t: 3, p: 4 },
            KdfParams { m_kib: 8, t: 1, p: 1 },
        ] {
            assert_eq!(passphrase_kek(b"x", &[0; 16], &weak).map(|_| ()), Err(SyncError::WeakKdf), "{weak:?}");
        }
    }

    #[test]
    fn rr_wrap_round_trip_and_rejections() {
        let kek = [9u8; 32];
        let rr = [1u8; 32];
        let w = rr_wrap(&kek, &rr, WrapKind::Passphrase, &ACC, 1).unwrap();
        assert_eq!(w.ct.len(), 48);
        assert_eq!(*rr_unwrap(&kek, &w, WrapKind::Passphrase, &ACC, 1).unwrap(), rr);
        assert_eq!(
            rr_unwrap(&[8u8; 32], &w, WrapKind::Passphrase, &ACC, 1).map(|_| ()),
            Err(SyncError::Auth),
            "wrong key"
        );
        assert_eq!(rr_unwrap(&kek, &w, WrapKind::RecoveryKey, &ACC, 1).map(|_| ()), Err(SyncError::Auth), "wrong kind");
        assert_eq!(
            rr_unwrap(&kek, &w, WrapKind::Passphrase, &ACC, 2).map(|_| ()),
            Err(SyncError::Auth),
            "wrong rr_gen"
        );
        assert_eq!(rr_unwrap(&kek, &w, WrapKind::Passphrase, &[8u8; 16], 1).map(|_| ()), Err(SyncError::Auth));
        let mut long = w.clone();
        long.ct.push(0);
        assert_eq!(
            rr_unwrap(&kek, &long, WrapKind::Passphrase, &ACC, 1).map(|_| ()),
            Err(SyncError::Malformed("rr_wrap length"))
        );
        let mut bad = w.clone();
        bad.ct[0] ^= 1;
        assert_eq!(rr_unwrap(&kek, &bad, WrapKind::Passphrase, &ACC, 1).map(|_| ()), Err(SyncError::Auth));
    }

    #[test]
    fn seal_round_trip_and_binding() {
        let kp = KeyPair::generate().unwrap();
        let info = seal_info(SealKind::Ak, &ACC, 3, "dev-1").unwrap();
        let s = seal(&kp.pk, &info, &[5u8; 32]).unwrap();
        assert_eq!(*open_key(&kp.sk, &info, &s).unwrap(), [5u8; 32]);
        // Any change of kind, epoch, account or recipient breaks it.
        for other in [
            seal_info(SealKind::Pak, &ACC, 3, "dev-1").unwrap(),
            seal_info(SealKind::Ak, &ACC, 4, "dev-1").unwrap(),
            seal_info(SealKind::Ak, &[0; 16], 3, "dev-1").unwrap(),
            seal_info(SealKind::Ak, &ACC, 3, "escrow").unwrap(),
        ] {
            assert_eq!(open(&kp.sk, &other, &s).map(|_| ()), Err(SyncError::Auth));
        }
        let other = KeyPair::generate().unwrap();
        assert_eq!(open(&other.sk, &info, &s).map(|_| ()), Err(SyncError::Auth));
    }

    #[test]
    fn low_order_points_are_rejected() {
        // u = 0 and u = 1 are among the small-order points listed for X25519 (RFC 7748 §6.1).
        for bad in [[0u8; 32], {
            let mut b = [0u8; 32];
            b[0] = 1;
            b
        }] {
            assert_eq!(check_public_key(&bad), Err(SyncError::LowOrderKey));
            assert_eq!(seal(&bad, b"i", b"x").map(|_| ()), Err(SyncError::LowOrderKey));
            let kp = KeyPair::generate().unwrap();
            assert_eq!(
                open(&kp.sk, b"i", &Sealed { enc: bad, ct: vec![0; 48] }).map(|_| ()),
                Err(SyncError::LowOrderKey)
            );
        }
    }

    #[test]
    fn escrow_keypair_is_deterministic() {
        let a = escrow_keypair(&[1u8; 32], &ACC);
        let b = escrow_keypair(&[1u8; 32], &ACC);
        assert_eq!(a.pk, b.pk);
        assert_ne!(a.pk, escrow_keypair(&[1u8; 32], &[2u8; 16]).pk);
        assert_ne!(a.pk, escrow_keypair(&[2u8; 32], &ACC).pk);
    }

    pub(crate) fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
