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

#[cfg(test)]
mod tests {
    use super::*;

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
