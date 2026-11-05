//! Every domain-separation string of the sync protocol (PROTOCOL §4–§9), in one place.
//! Changing any of them breaks compatibility and the frozen test vectors.

/// The protocol and format version carried as `v` everywhere.
pub const VERSION: u8 = 1;

// HKDF info labels (salt = account_id unless noted).
pub fn enc(collection: &str, epoch: u32) -> String {
    format!("krunk-sync/v1/enc/{collection}/{epoch}")
}
pub fn id(collection: &str, epoch: u32) -> String {
    format!("krunk-sync/v1/id/{collection}/{epoch}")
}
pub fn ak_dir_mac(epoch: u32) -> String {
    format!("krunk-sync/v1/ak-dir-mac/{epoch}")
}
pub fn pak_dir_mac(pak_epoch: u32) -> String {
    format!("krunk-sync/v1/pak-dir-mac/{pak_epoch}")
}
pub fn ak_hist(epoch: u32) -> String {
    format!("krunk-sync/v1/ak-hist/{epoch}")
}
pub fn pak_hist(pak_epoch: u32) -> String {
    format!("krunk-sync/v1/pak-hist/{pak_epoch}")
}
pub fn device_name(epoch: u32) -> String {
    format!("krunk-sync/v1/device-name/{epoch}")
}
pub const RK_KEK: &str = "krunk-sync/v1/rk-kek";
pub const ESCROW_X25519: &str = "krunk-sync/v1/escrow-x25519";
pub const ESCROW_MAC_AK: &str = "krunk-sync/v1/escrow-mac/ak";
pub const ESCROW_MAC_PAK: &str = "krunk-sync/v1/escrow-mac/pak";
/// `rr_wrap` AAD prefix and commit label, per wrap kind (`pp` passphrase, `rk` recovery key).
pub fn rr_aad(kind: &str) -> String {
    format!("krunk-sync/v1/rr/{kind}")
}
pub fn rr_commit(kind: &str) -> String {
    format!("krunk-sync/v1/rr-commit/{kind}")
}

// AAD prefixes.
pub const AK_HIST_AAD: &str = "krunk-sync/v1/ak-hist";
pub const PAK_HIST_AAD: &str = "krunk-sync/v1/pak-hist";
pub const REC: &str = "krunk-sync/v1/rec";
pub const BOX: &str = "krunk-sync/v1/box";

// HPKE seals (PROTOCOL §3.3).
pub const SEAL: &str = "krunk-sync/v1/seal";

// Device approval (PROTOCOL §6.2).
pub const COMMIT: &str = "krunk-sync/v1/commit";
pub const APPROVE: &str = "krunk-sync/v1/approve";
pub const SAS: &str = "krunk-sync/v1/sas";
pub const K_AN: &str = "krunk-sync/v1/k-an";
pub const K_NA: &str = "krunk-sync/v1/k-na";
pub const CONFIRM: &str = "krunk-sync/v1/confirm";
