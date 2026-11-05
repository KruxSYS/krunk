//! Device approval (PROTOCOL §6.2): the commitment, transcript, 6-digit SAS, directional box
//! keys, box encryption and the confirm tag. The SAS is safe against a relaying server
//! because N commits to its ephemeral key before A picks its own [R4].

use super::crypto::{self, Key};
use super::{labels, AccountId, SyncError};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

/// Direction byte in the box AAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// From the approving device A to the new device N.
    AToN,
    NToA,
}

impl Direction {
    fn byte(self) -> u8 {
        match self {
            Direction::AToN => 1,
            Direction::NToA => 2,
        }
    }
}

/// `commit_N = SHA-256("krunk-sync/v1/commit" ‖ epk_N ‖ nonce_N)`.
pub fn commit(epk_n: &[u8; 32], nonce_n: &[u8; 32]) -> [u8; 32] {
    crypto::sha256(&[labels::COMMIT.as_bytes(), epk_n, nonce_n])
}

/// The public facts both sides bind into the transcript.
pub struct Transcript<'a> {
    pub account_id: &'a AccountId,
    pub approval_id: &'a str,
    pub dev_id_a: &'a str,
    pub dev_id_n: &'a str,
    pub n_label: &'a str,
    pub n_platform: &'a str,
    pub commit_n: &'a [u8; 32],
    pub epk_a: &'a [u8; 32],
    pub nonce_a: &'a [u8; 32],
    pub epk_n: &'a [u8; 32],
    pub nonce_n: &'a [u8; 32],
}

impl Transcript<'_> {
    /// `T`: every field length-prefixed; N's label and platform enter as
    /// `SHA-256(lp(label) ‖ lp(platform))`.
    pub fn bytes(&self) -> Result<Vec<u8>, SyncError> {
        let mut who = Vec::new();
        crypto::put_lp(&mut who, self.n_label.as_bytes())?;
        crypto::put_lp(&mut who, self.n_platform.as_bytes())?;
        let who = crypto::sha256(&[&who]);
        let mut t = Vec::with_capacity(400);
        for field in [
            labels::APPROVE.as_bytes(),
            self.account_id,
            self.approval_id.as_bytes(),
            self.dev_id_a.as_bytes(),
            self.dev_id_n.as_bytes(),
            &who,
            self.commit_n,
            self.epk_a,
            self.nonce_a,
            self.epk_n,
            self.nonce_n,
        ] {
            crypto::put_lp(&mut t, field)?;
        }
        Ok(t)
    }
}

/// What both devices derive from the shared secret and the transcript.
pub struct Derived {
    /// 0..=999_999, shown as `NNN NNN`.
    pub sas: u32,
    pub k_an: Key,
    pub k_na: Key,
}

/// X25519 between ephemeral keys; an all-zero result (low-order peer key) is refused.
pub fn dh(esk: &[u8; 32], epk_peer: &[u8; 32]) -> Result<Key, SyncError> {
    crypto::check_public_key(epk_peer)?;
    let shared = x25519_dalek::StaticSecret::from(*esk).diffie_hellman(&x25519_dalek::PublicKey::from(*epk_peer));
    if !shared.was_contributory() {
        return Err(SyncError::LowOrderKey);
    }
    Ok(Zeroizing::new(shared.to_bytes()))
}

/// `prk = HKDF-Extract(SHA-256(T), dh)`; SAS, `k_AN`, `k_NA` by HKDF-Expand.
pub fn derive(dh: &[u8; 32], transcript: &[u8]) -> Derived {
    let salt = crypto::sha256(&[transcript]);
    let (_, hk) = Hkdf::<Sha256>::extract(Some(&salt), dh);
    let mut sas8 = [0u8; 8];
    let mut k_an = Zeroizing::new([0u8; 32]);
    let mut k_na = Zeroizing::new([0u8; 32]);
    #[allow(clippy::expect_used)] // lengths far below the HKDF limit
    {
        hk.expand(labels::SAS.as_bytes(), &mut sas8).expect("hkdf");
        hk.expand(labels::K_AN.as_bytes(), k_an.as_mut()).expect("hkdf");
        hk.expand(labels::K_NA.as_bytes(), k_na.as_mut()).expect("hkdf");
    }
    Derived { sas: (u64::from_be_bytes(sas8) % 1_000_000) as u32, k_an, k_na }
}

pub fn format_sas(sas: u32) -> String {
    format!("{:03} {:03}", sas / 1000, sas % 1000)
}

fn box_aad(dir: Direction) -> Vec<u8> {
    let mut a = labels::BOX.as_bytes().to_vec();
    a.push(dir.byte());
    a
}

/// `box = AEAD(k, nonce, pt, "krunk-sync/v1/box" ‖ direction)` → `nonce ‖ ct`.
pub fn box_seal(k: &[u8; 32], dir: Direction, pt: &[u8]) -> Result<Vec<u8>, SyncError> {
    let (n, ct) = crypto::aead_seal(k, &box_aad(dir), pt)?;
    let mut out = n.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn box_seal_with_nonce(
    k: &[u8; 32],
    dir: Direction,
    pt: &[u8],
    n: &[u8; crypto::NONCE_LEN],
) -> Result<Vec<u8>, SyncError> {
    let mut out = n.to_vec();
    out.extend_from_slice(&crypto::aead_seal_with_nonce(k, n, &box_aad(dir), pt)?);
    Ok(out)
}

pub fn box_open(k: &[u8; 32], dir: Direction, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    if sealed.len() < crypto::NONCE_LEN + crypto::TAG_LEN {
        return Err(SyncError::Malformed("box"));
    }
    let (n, ct) = sealed.split_at(crypto::NONCE_LEN);
    crypto::aead_open(k, n, &box_aad(dir), ct)
}

/// N's proof that the typed SAS matched: `HMAC(k_NA, "krunk-sync/v1/confirm")`.
pub fn confirm_tag(k_na: &[u8; 32]) -> [u8; 32] {
    crypto::hmac(k_na, labels::CONFIRM.as_bytes())
}

pub fn confirm_verify(k_na: &[u8; 32], tag: &[u8]) -> Result<(), SyncError> {
    crypto::hmac_verify(k_na, labels::CONFIRM.as_bytes(), tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::KeyPair;

    fn transcript_bytes(c: &[u8; 32], ea: &[u8; 32], na: &[u8; 32], en: &[u8; 32], nn: &[u8; 32]) -> Vec<u8> {
        Transcript {
            account_id: &[1u8; 16],
            approval_id: "appr-1",
            dev_id_a: "dev-a",
            dev_id_n: "dev-n",
            n_label: "Krunk 0.3 on Windows 11",
            n_platform: "windows",
            commit_n: c,
            epk_a: ea,
            nonce_a: na,
            epk_n: en,
            nonce_n: nn,
        }
        .bytes()
        .unwrap()
    }

    #[test]
    fn both_sides_agree_and_a_relay_cannot_match() {
        let a = KeyPair::generate().unwrap();
        let n = KeyPair::generate().unwrap();
        let (na, nn) = ([1u8; 32], [2u8; 32]);
        let c = commit(&n.pk, &nn);
        let t = transcript_bytes(&c, &a.pk, &na, &n.pk, &nn);
        let da = derive(&dh(&a.sk, &n.pk).unwrap(), &t);
        let dn = derive(&dh(&n.sk, &a.pk).unwrap(), &t);
        assert_eq!(da.sas, dn.sas);
        assert_eq!(*da.k_an, *dn.k_an);
        assert_ne!(*da.k_an, *da.k_na, "directional keys differ");
        assert!(da.sas < 1_000_000);
        // A man in the middle has a different DH with each side: the SAS (almost surely) and
        // the keys differ, and the confirm tag from N doesn't verify at A.
        let m = KeyPair::generate().unwrap();
        let t_a = transcript_bytes(&commit(&m.pk, &nn), &a.pk, &na, &m.pk, &nn);
        let at_a = derive(&dh(&a.sk, &m.pk).unwrap(), &t_a);
        let t_n = transcript_bytes(&c, &m.pk, &na, &n.pk, &nn);
        let at_n = derive(&dh(&n.sk, &m.pk).unwrap(), &t_n);
        assert_eq!(confirm_verify(&at_a.k_na, &confirm_tag(&at_n.k_na)), Err(SyncError::Auth));
    }

    #[test]
    fn commitment_binds_key_and_nonce() {
        assert_ne!(commit(&[1; 32], &[2; 32]), commit(&[1; 32], &[3; 32]));
        assert_ne!(commit(&[1; 32], &[2; 32]), commit(&[4; 32], &[2; 32]));
    }

    #[test]
    fn boxes_are_directional() {
        let k = [7u8; 32];
        let b = box_seal(&k, Direction::AToN, b"payload").unwrap();
        assert_eq!(box_open(&k, Direction::AToN, &b).unwrap().as_slice(), b"payload");
        assert_eq!(box_open(&k, Direction::NToA, &b).map(|_| ()), Err(SyncError::Auth), "reflection");
        assert_eq!(box_open(&k, Direction::AToN, &b[..30]).map(|_| ()), Err(SyncError::Malformed("box")));
    }

    #[test]
    fn sas_format() {
        assert_eq!(format_sas(7), "000 007");
        assert_eq!(format_sas(482917), "482 917");
    }

    #[test]
    fn low_order_ephemeral_is_refused() {
        let a = KeyPair::generate().unwrap();
        assert_eq!(dh(&a.sk, &[0u8; 32]).map(|_| ()), Err(SyncError::LowOrderKey));
    }
}
