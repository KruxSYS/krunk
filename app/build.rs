//! Windows link setup for LightBrowser.dll.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // libcef.dll is loaded on first use, so a missing or damaged runtime
        // folder can be reported with a message box instead of failing
        // silently inside the loader (the bootstrap shows nothing).
        println!("cargo:rustc-link-arg=/DELAYLOAD:libcef.dll");
    }
}
