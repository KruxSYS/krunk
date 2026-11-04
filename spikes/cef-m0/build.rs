fn main() {
    // Load libcef.dll lazily so RunWinMain can report missing runtime files instead of the
    // bootstrap failing silently when the DLL's imports cannot be resolved.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-cdylib=/DELAYLOAD:libcef.dll");
    }
}
