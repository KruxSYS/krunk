//! Windows link setup for Krunk.dll, and the update check's server and key.
//!
//! `UPDATE_BASE_URL` (default `https://krunk.example`) is the update server;
//! `KRUNK_UPDATE_PUBKEY_FILE` (default `assets/keys/update-signing.pub`) the
//! Ed25519 public key (PEM) whose signatures the update check accepts. Only
//! builds with the `dev-updates` feature may use an `http://` server.

use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // libcef.dll is loaded on first use, so a missing or damaged runtime
        // folder can be reported with a message box instead of failing
        // silently inside the loader (the bootstrap shows nothing).
        println!("cargo:rustc-link-arg=/DELAYLOAD:libcef.dll");
    }

    println!("cargo:rerun-if-env-changed=UPDATE_BASE_URL");
    println!("cargo:rerun-if-env-changed=KRUNK_UPDATE_PUBKEY_FILE");
    let base = std::env::var("UPDATE_BASE_URL").unwrap_or_else(|_| "https://krunk.example".into());
    let dev = std::env::var_os("CARGO_FEATURE_DEV_UPDATES").is_some();
    let lower = base.to_ascii_lowercase();
    if !(lower.starts_with("https://") || (dev && lower.starts_with("http://"))) {
        panic!("UPDATE_BASE_URL must be https:// (http:// only with --features dev-updates): {base}");
    }
    if base.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\') {
        panic!("UPDATE_BASE_URL contains invalid characters: {base}");
    }
    println!("cargo:rustc-env=KRUNK_UPDATE_BASE_URL={base}");

    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let key = match std::env::var_os("KRUNK_UPDATE_PUBKEY_FILE") {
        Some(p) => PathBuf::from(p),
        None => manifest_dir.join("../assets/keys/update-signing.pub"),
    };
    let key = key.canonicalize().unwrap_or_else(|e| panic!("update public key {}: {e}", key.display()));
    let pem = std::fs::read_to_string(&key).unwrap_or_else(|e| panic!("update public key {}: {e}", key.display()));
    if !pem.trim_start().starts_with("-----BEGIN PUBLIC KEY-----") {
        panic!("{} is not a PEM public key (made by dist/server/gen-signing-key.sh)", key.display());
    }
    println!("cargo:rerun-if-changed={}", key.display());
    println!("cargo:rustc-env=KRUNK_UPDATE_PUBKEY_PATH={}", key.display());
}
