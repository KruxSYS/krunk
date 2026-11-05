//! Human-facing codes (PROTOCOL §3.2, §5.2, §6.1): Crockford base32, the recovery key with
//! its check symbol, and the link `user_code`.

use super::SyncError;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Check symbols: the 32 data symbols plus `*~$=U` (values 32–36).
const CHECK: &[u8; 37] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ*~$=U";
pub const RECOVERY_KEY_BYTES: usize = 20;
pub const USER_CODE_BYTES: usize = 5;

/// Encodes bytes as Crockford base32, 5 bits per symbol, most significant bit first. The
/// last symbol is zero-padded (lengths used here are multiples of 5 bytes, so there's none).
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

fn symbol_value(c: char) -> Option<u32> {
    let c = c.to_ascii_uppercase();
    let c = match c {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    ALPHABET.iter().position(|&a| char::from(a) == c).map(|p| p as u32)
}

/// Decodes exactly `n_bytes` bytes. `-` and spaces are ignored, case doesn't matter, and
/// `O`→0, `I`/`L`→1. Padding bits must be zero (one string, one value).
pub fn decode(s: &str, n_bytes: usize) -> Result<Vec<u8>, SyncError> {
    let symbols: Vec<char> = s.chars().filter(|c| *c != '-' && *c != ' ').collect();
    if symbols.len() != (n_bytes * 8).div_ceil(5) {
        return Err(SyncError::Malformed("code length"));
    }
    let mut out = Vec::with_capacity(n_bytes);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in symbols {
        let v = symbol_value(c).ok_or(SyncError::Malformed("code symbol"))?;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    if bits > 0 && acc & ((1 << bits) - 1) != 0 {
        return Err(SyncError::Malformed("code padding"));
    }
    out.truncate(n_bytes);
    Ok(out)
}

/// Crockford check symbol: the number modulo 37.
pub fn check_symbol(bytes: &[u8]) -> char {
    let r = bytes.iter().fold(0u32, |r, &b| (r * 256 + u32::from(b)) % 37);
    char::from(CHECK[r as usize])
}

/// `KRNK-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-C` (PROTOCOL §5.2).
pub fn format_recovery_key(rk: &[u8; RECOVERY_KEY_BYTES]) -> String {
    let body = encode(rk);
    let groups: Vec<&str> = (0..8).map(|i| &body[i * 4..i * 4 + 4]).collect();
    format!("KRNK-{}-{}", groups.join("-"), check_symbol(rk))
}

/// Parses a typed recovery key. The `KRNK` prefix is optional, and the check symbol is
/// required, so a typo is reported before any key derivation runs.
pub fn parse_recovery_key(s: &str) -> Result<[u8; RECOVERY_KEY_BYTES], SyncError> {
    let compact: String = s.chars().filter(|c| *c != '-' && *c != ' ').collect::<String>().to_ascii_uppercase();
    let compact = compact.strip_prefix("KRNK").unwrap_or(&compact);
    let mut chars: Vec<char> = compact.chars().collect();
    let check = chars.pop().ok_or(SyncError::Malformed("recovery key"))?;
    let body: String = chars.into_iter().collect();
    let bytes = decode(&body, RECOVERY_KEY_BYTES)?;
    let mut rk = [0u8; RECOVERY_KEY_BYTES];
    rk.copy_from_slice(&bytes);
    let check = match check {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    if check != check_symbol(&rk) {
        return Err(SyncError::Malformed("recovery key check symbol"));
    }
    Ok(rk)
}

/// The link `user_code`: 5 bytes as 8 symbols, shown `XXXX-XXXX` (PROTOCOL §6.1).
pub fn format_user_code(b: &[u8; USER_CODE_BYTES]) -> String {
    let s = encode(b);
    format!("{}-{}", &s[..4], &s[4..])
}

pub fn parse_user_code(s: &str) -> Result<[u8; USER_CODE_BYTES], SyncError> {
    let v = decode(s, USER_CODE_BYTES)?;
    let mut out = [0u8; USER_CODE_BYTES];
    out.copy_from_slice(&v);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for n in [5usize, 20] {
            for seed in 0u8..50 {
                let bytes: Vec<u8> = (0..n).map(|i| (i as u8).wrapping_mul(seed).wrapping_add(seed)).collect();
                assert_eq!(decode(&encode(&bytes), n).unwrap(), bytes);
            }
        }
        assert_eq!(encode(&[0xff; 5]), "ZZZZZZZZ");
        assert_eq!(encode(&[0; 5]), "00000000");
    }

    #[test]
    fn lenient_input_strict_values() {
        let code = format_user_code(&[0x12, 0x34, 0x56, 0x78, 0x9a]);
        assert_eq!(parse_user_code(&code).unwrap(), [0x12, 0x34, 0x56, 0x78, 0x9a]);
        assert_eq!(parse_user_code(&code.to_lowercase().replace('-', " ")).unwrap(), [0x12, 0x34, 0x56, 0x78, 0x9a]);
        assert_eq!(decode("OOOO-OOOO", 5).unwrap(), vec![0; 5], "O reads as 0");
        assert_eq!(decode("IIII-LLLL", 5).unwrap(), decode("1111-1111", 5).unwrap());
        assert!(decode("UUUU-UUUU", 5).is_err(), "U is only a check symbol");
        assert!(decode("ABCD-EFG", 5).is_err(), "too short");
        assert!(decode("ABCD-EFGH-J", 5).is_err(), "too long");
    }

    #[test]
    fn recovery_key_format_and_check() {
        let rk = [0xa5u8; RECOVERY_KEY_BYTES];
        let s = format_recovery_key(&rk);
        assert!(s.starts_with("KRNK-"));
        assert_eq!(s.split('-').count(), 10, "prefix + 8 groups + check");
        assert_eq!(parse_recovery_key(&s).unwrap(), rk);
        assert_eq!(parse_recovery_key(&s.to_lowercase().replace('-', "")).unwrap(), rk);
        assert_eq!(parse_recovery_key(&s["KRNK-".len()..]).unwrap(), rk, "prefix optional");
        // A single mistyped symbol is caught by the check symbol.
        let mut typo: Vec<char> = s.chars().collect();
        let i = 6;
        typo[i] = if typo[i] == 'A' { 'B' } else { 'A' };
        let typo: String = typo.into_iter().collect();
        assert_eq!(parse_recovery_key(&typo), Err(SyncError::Malformed("recovery key check symbol")));
        assert!(parse_recovery_key("KRNK-").is_err());
    }
}
