//! Compresses the bundled filter lists (assets/filters/*.txt) into OUT_DIR so
//! they are embedded deflated (~1.4 MB instead of ~4.5 MB) and inflated only
//! when the engine cache has to be rebuilt.

use std::path::PathBuf;

/// Must match `settings::FILTER_LISTS`.
const LISTS: &[&str] = &["easylist", "easyprivacy", "ublock-filters", "ublock-privacy", "ublock-unbreak"];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("../assets/filters");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for name in LISTS {
        let path = src.join(format!("{name}.txt"));
        println!("cargo:rerun-if-changed={}", path.display());
        let raw = std::fs::read(&path).unwrap_or_else(|e| panic!("missing filter list {}: {e}", path.display()));
        let packed = miniz_oxide::deflate::compress_to_vec(&raw, 9);
        std::fs::write(out.join(format!("{name}.txt.deflate")), packed).unwrap();
    }
    println!("cargo:rerun-if-changed=build.rs");
}
