//! Strong password generator (OS randomness, unbiased sampling).

use zeroize::Zeroizing;

const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGITS: &[u8] = b"23456789";
const SYMBOLS: &[u8] = b"!#$%&*+-=?@^_~";

pub const MIN_LEN: u32 = 8;
pub const MAX_LEN: u32 = 128;

/// Uniform random index below `n` (rejection sampling, no modulo bias).
fn below(n: usize) -> Result<usize, getrandom::Error> {
    debug_assert!(n > 0 && n <= 256);
    let limit = 256 - (256 % n);
    loop {
        let mut b = [0u8; 1];
        getrandom::fill(&mut b)?;
        if (b[0] as usize) < limit {
            return Ok(b[0] as usize % n);
        }
    }
}

/// Generates a password of `length` (clamped to 8..=128) that contains at
/// least one lower-case letter, upper-case letter, digit and (if enabled)
/// symbol. Look-alike characters (l, I, O, 0, 1) are left out.
pub fn generate(length: u32, symbols: bool) -> Result<Zeroizing<String>, getrandom::Error> {
    let len = length.clamp(MIN_LEN, MAX_LEN) as usize;
    let mut classes: Vec<&[u8]> = vec![LOWER, UPPER, DIGITS];
    if symbols {
        classes.push(SYMBOLS);
    }
    let all: Vec<u8> = classes.concat();
    let mut out = Zeroizing::new(Vec::with_capacity(len));
    for class in &classes {
        out.push(class[below(class.len())?]);
    }
    while out.len() < len {
        out.push(all[below(all.len())?]);
    }
    // Fisher-Yates so the guaranteed characters are not always first.
    for i in (1..out.len()).rev() {
        let j = below(i + 1)?;
        out.swap(i, j);
    }
    // All bytes are ASCII.
    Ok(Zeroizing::new(out.iter().map(|&b| b as char).collect()))
}

/// Settings of the generator (Krunk 0.6.0).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Passgen {
    pub length: u32,
    pub symbols: bool,
}

impl Default for Passgen {
    fn default() -> Passgen {
        Passgen { length: 20, symbols: true }
    }
}

impl Passgen {
    pub fn validate(&self) -> Result<(), String> {
        if self.length < MIN_LEN || self.length > MAX_LEN {
            return Err(format!("The length is {MIN_LEN} to {MAX_LEN}"));
        }
        Ok(())
    }
}

/// Is a focused password field one where a *new* password is being chosen (sign-up, change
/// password), so a generated one should be offered? `autocomplete` is the field's attribute,
/// `passwords` the number of visible password fields in its form, `text` the form's visible
/// text and button labels (truncated by the page).
pub fn wants_new_password(autocomplete: &str, passwords: u32, text: &str) -> bool {
    let ac = autocomplete.trim().to_ascii_lowercase();
    if ac == "new-password" {
        return true;
    }
    if ac == "current-password" || ac == "one-time-code" {
        return false;
    }
    if passwords >= 2 {
        return true;
    }
    if passwords == 0 {
        return false;
    }
    let t = text.to_ascii_lowercase();
    [
        "sign up",
        "signup",
        "register",
        "create account",
        "create an account",
        "create your account",
        "new password",
        "choose a password",
        "set a password",
        "set password",
        "change password",
        "reset password",
        "konto erstellen",
        "registrieren",
        "neues passwort",
        "inscription",
        "créer un compte",
        "nouveau mot de passe",
    ]
    .iter()
    .any(|k| t.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_password_detection() {
        assert!(wants_new_password("new-password", 1, ""));
        assert!(!wants_new_password("current-password", 2, "Sign up"), "the attribute wins");
        assert!(wants_new_password("", 2, ""), "password + confirm");
        assert!(wants_new_password("", 1, "Create your account · Sign up"));
        assert!(wants_new_password("off", 1, "Neues Passwort wählen"));
        assert!(!wants_new_password("", 1, "Sign in · Forgot password?"));
        assert!(!wants_new_password("username", 0, "Register"), "no password field: no offer");
        let p = Passgen::default();
        assert_eq!((p.length, p.symbols), (20, true));
        assert!(Passgen { length: 7, symbols: true }.validate().is_err());
        assert!(Passgen { length: 128, symbols: false }.validate().is_ok());
    }

    #[test]
    fn length_and_classes() {
        for len in [0, 8, 20, 64, 500] {
            let p = generate(len, true).unwrap();
            assert_eq!(p.len(), (len.clamp(MIN_LEN, MAX_LEN)) as usize);
            assert!(p.bytes().any(|b| LOWER.contains(&b)));
            assert!(p.bytes().any(|b| UPPER.contains(&b)));
            assert!(p.bytes().any(|b| DIGITS.contains(&b)));
            assert!(p.bytes().any(|b| SYMBOLS.contains(&b)));
        }
        let p = generate(30, false).unwrap();
        assert!(!p.bytes().any(|b| SYMBOLS.contains(&b)));
    }

    #[test]
    fn not_repeating() {
        let a = generate(20, true).unwrap();
        let b = generate(20, true).unwrap();
        assert_ne!(*a, *b);
    }

    #[test]
    fn roughly_uniform() {
        let mut counts = [0u32; 256];
        for _ in 0..200 {
            for b in generate(64, true).unwrap().bytes() {
                counts[b as usize] += 1;
            }
        }
        let alphabet = LOWER.len() + UPPER.len() + DIGITS.len() + SYMBOLS.len();
        let used = counts.iter().filter(|&&c| c > 0).count();
        assert_eq!(used, alphabet, "every character appears");
        let max = *counts.iter().max().unwrap();
        let min = *counts.iter().filter(|&&c| c > 0).min().unwrap();
        assert!(max < min * 3, "distribution too skewed: {min}..{max}");
    }
}
