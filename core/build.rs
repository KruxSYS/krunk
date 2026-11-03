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
    embed_ui(&manifest.join("../ui"), &out);
    println!("cargo:rerun-if-changed=build.rs");
}

/// Writes `ui_assets.rs`: a table of every file under ui/shell, ui/pages and
/// ui/shared (ui/dev is test tooling and is never embedded).
fn embed_ui(ui: &std::path::Path, out: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", ui.display());
    let mut entries = Vec::new();
    for dir in ["shell", "pages", "shared"] {
        collect(&ui.join(dir), dir, &mut entries);
    }
    entries.sort();
    let mut code = String::from(
        "/// (key, bytes) for every embedded UI file, sorted by key.\npub static UI_ASSETS: &[(&str, &[u8])] = &[\n",
    );
    for (key, path) in &entries {
        println!("cargo:rerun-if-changed={}", path.display());
        code.push_str(&format!("    ({key:?}, include_bytes!({:?})),\n", path.display().to_string()));
    }
    code.push_str("];\n");
    std::fs::write(out.join("ui_assets.rs"), code).unwrap();
}

fn collect(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    println!("cargo:rerun-if-changed={}", dir.display());
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(&path, &format!("{prefix}/{name}"), out);
        } else {
            out.push((format!("{prefix}/{name}"), path.canonicalize().unwrap()));
        }
    }
}
