//! File integrity helpers (SHA-256).

use sha2::{Digest, Sha256};

/// Lower-case hex SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// The committed loader must match the recorded hash (the app embeds both).
    #[test]
    fn bundled_loader_matches_recorded_hash() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/webview2/");
        let dll = std::fs::read(format!("{root}x64/WebView2Loader.dll")).unwrap();
        let sums = std::fs::read_to_string(format!("{root}SHA256SUMS")).unwrap();
        let recorded = sums.split_whitespace().next().unwrap();
        assert_eq!(sha256_hex(&dll), recorded);
    }
}
