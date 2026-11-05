//! End-to-end encrypted sync: the crypto core (docs/sync/PROTOCOL.md).
//!
//! Pure functions only: no I/O, no clock, no network. The app wires this up in milestone S6,
//! and Android uses the same code through UniFFI. Every label is in [`labels`]; every format
//! is pinned by the frozen test vectors in `core/tests/fixtures/sync/`.

pub mod codes;
pub mod crypto;
pub mod dir;
pub mod envelope;
pub mod hlc;
pub mod labels;
pub mod normalize;
pub mod sas;

#[cfg(test)]
mod vectors;

/// `account_id`: 16 raw bytes (a UUIDv4 from the server), bound into every salt, AAD and MAC.
pub type AccountId = [u8; 16];

/// Why a sync crypto operation was refused. Never carries secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// Input that doesn't parse, or has the wrong length or version.
    Malformed(&'static str),
    /// Authentication failed: wrong key, tampered or swapped data.
    Auth,
    /// KDF parameters below the built-in floor (PROTOCOL §5.1).
    WeakKdf,
    /// A public key whose Diffie-Hellman result would be all zeros (low order).
    LowOrderKey,
    /// The OS random generator failed.
    Random,
    /// Larger than the protocol allows.
    TooLarge,
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Malformed(what) => write!(f, "malformed {what}"),
            SyncError::Auth => f.write_str("authentication failed"),
            SyncError::WeakKdf => f.write_str("key-derivation parameters below the minimum"),
            SyncError::LowOrderKey => f.write_str("invalid public key"),
            SyncError::Random => f.write_str("random number generator failed"),
            SyncError::TooLarge => f.write_str("too large"),
        }
    }
}

impl std::error::Error for SyncError {}
