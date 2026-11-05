//! Account passwords (docs/sync/PLAN.md §4.2): the policy, and hashing with
//! `Argon2id(HMAC-SHA256(pepper, nfkc(password)), 16-byte salt, m = 64 MiB, t = 3, p = 1)` as a
//! PHC string. At most 4 hashes run at once (256 MiB peak). An unknown account still costs one
//! hash against a dummy PHC, so timing doesn't reveal whether an account exists.

use crate::crypto;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use unicode_normalization::UnicodeNormalization;

pub const PEPPER_ID: i16 = 1;
pub const MIN_CHARS: usize = 10;
pub const MAX_CHARS: usize = 256;
pub const ARGON2_M_KIB: u32 = 65536;
pub const ARGON2_T: u32 = 3;
pub const ARGON2_P: u32 = 1;
pub const CONCURRENT_HASHES: usize = 4;

static COMMON: &str = include_str!("../data/common-passwords.txt");

fn common() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| COMMON.lines().filter(|l| !l.is_empty()).collect())
}

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyError {
    TooShort,
    TooLong,
    SameAsEmail,
    Common,
}

impl PolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            PolicyError::TooShort => "password_too_short",
            PolicyError::TooLong => "password_too_long",
            PolicyError::SameAsEmail => "password_is_email",
            PolicyError::Common => "password_common",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            PolicyError::TooShort => "Use at least 10 characters.",
            PolicyError::TooLong => "Use at most 256 characters.",
            PolicyError::SameAsEmail => "The password must not be your email address.",
            PolicyError::Common => "This password is too common. Choose another one.",
        }
    }
}

/// 10–256 characters after NFKC, not the email address, not a common password.
pub fn check_policy(password: &str, email: &str) -> Result<(), PolicyError> {
    let n: String = password.nfkc().collect();
    let chars = n.chars().count();
    if chars < MIN_CHARS {
        return Err(PolicyError::TooShort);
    }
    if chars > MAX_CHARS {
        return Err(PolicyError::TooLong);
    }
    let lower = n.to_lowercase();
    if lower == email.trim().to_lowercase() || lower == email.split('@').next().unwrap_or("").to_lowercase() {
        return Err(PolicyError::SameAsEmail);
    }
    if common().contains(lower.as_str()) {
        return Err(PolicyError::Common);
    }
    Ok(())
}

/// Hashes and verifies passwords. Cheap to clone.
#[derive(Clone)]
pub struct Passwords {
    pepper: Arc<[u8; 32]>,
    permits: Arc<Semaphore>,
    dummy: Arc<String>,
}

fn argon2() -> Argon2<'static> {
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(32)).expect("valid Argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn peppered(pepper: &[u8; 32], password: &str) -> [u8; 32] {
    let n: String = password.nfkc().collect();
    crypto::hmac(pepper, "krunk-password", &[n.as_bytes()])
}

fn hash_blocking(pepper: &[u8; 32], password: &str) -> Result<String, String> {
    let salt = crypto::random::<16>();
    argon2()
        .hash_password_with_salt(&peppered(pepper, password), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("argon2: {e}"))
}

fn verify_blocking(pepper: &[u8; 32], password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        // The parameters come from the PHC string, so older hashes stay verifiable.
        Ok(parsed) => Argon2::default().verify_password(&peppered(pepper, password), &parsed).is_ok(),
        Err(_) => false,
    }
}

impl Passwords {
    /// Computes the dummy hash once (about as long as one login).
    pub fn new(pepper: [u8; 32]) -> Result<Passwords, String> {
        let dummy = hash_blocking(&pepper, &crypto::new_token())?;
        Ok(Passwords {
            pepper: Arc::new(pepper),
            permits: Arc::new(Semaphore::new(CONCURRENT_HASHES)),
            dummy: Arc::new(dummy),
        })
    }

    pub async fn hash(&self, password: &str) -> Result<String, String> {
        let _permit = self.permits.acquire().await.map_err(|e| e.to_string())?;
        let (pepper, password) = (self.pepper.clone(), password.to_string());
        tokio::task::spawn_blocking(move || hash_blocking(&pepper, &password)).await.map_err(|e| e.to_string())?
    }

    /// Verifies against `phc`, or against the dummy hash when there is no account (and then
    /// always returns false). Either way exactly one Argon2id computation runs.
    pub async fn verify(&self, password: &str, phc: Option<&str>) -> bool {
        let Ok(_permit) = self.permits.acquire().await else { return false };
        let (pepper, password) = (self.pepper.clone(), password.to_string());
        let (target, real) = match phc {
            Some(p) => (p.to_string(), true),
            None => (self.dummy.as_str().to_string(), false),
        };
        let ok =
            tokio::task::spawn_blocking(move || verify_blocking(&pepper, &password, &target)).await.unwrap_or(false);
        ok && real
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy() {
        assert_eq!(check_policy("short", "a@b.cd"), Err(PolicyError::TooShort));
        assert_eq!(check_policy(&"x".repeat(257), "a@b.cd"), Err(PolicyError::TooLong));
        assert!(check_policy(&"é".repeat(256), "a@b.cd").is_ok(), "characters, not bytes");
        assert_eq!(check_policy("Alice@Example.org", "alice@example.org"), Err(PolicyError::SameAsEmail));
        assert_eq!(check_policy("alicelonger1", "alicelonger1@example.org"), Err(PolicyError::SameAsEmail));
        assert_eq!(check_policy("Password123", "a@b.cd"), Err(PolicyError::Common));
        assert_eq!(check_policy("QWERTYUIOP", "a@b.cd"), Err(PolicyError::Common), "case-insensitive");
        // NFKC: full-width digits count as their ASCII forms.
        assert_eq!(check_policy("１２３４５６７８９０", "a@b.cd"), Err(PolicyError::Common));
        assert!(check_policy("correct horse battery", "a@b.cd").is_ok());
        assert!(common().len() > 9000);
    }

    #[tokio::test]
    async fn hash_and_verify() {
        let p = Passwords::new([9u8; 32]).unwrap();
        let phc = p.hash("correct horse battery").await.unwrap();
        // The shipped parameters are in the PHC string.
        assert!(phc.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"), "{phc}");
        assert!(p.verify("correct horse battery", Some(&phc)).await);
        assert!(!p.verify("correct horse batterY", Some(&phc)).await);
        assert!(!p.verify("correct horse battery", None).await, "the dummy never matches");
        assert!(!p.verify("x", Some("not a phc")).await);
        // NFKC before hashing: a composed and a decomposed é are the same password.
        let phc2 = p.hash("caf\u{e9} au lait!").await.unwrap();
        assert!(p.verify("cafe\u{301} au lait!", Some(&phc2)).await);
        // Another pepper can't verify it.
        let other = Passwords::new([8u8; 32]).unwrap();
        assert!(!other.verify("correct horse battery", Some(&phc)).await);
    }
}
