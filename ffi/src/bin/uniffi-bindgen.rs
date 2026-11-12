//! `uniffi-bindgen` built from the workspace, so the generator always matches the `uniffi`
//! version in Cargo.lock (android/scripts/build-rust.sh runs it).
fn main() {
    uniffi::uniffi_bindgen_main()
}
