//! Exports the module's stack pointer, the one mutable global a Rust module has (on stable
//! Rust a mutable WebAssembly global can't be declared in source): the inspector's check of
//! mutable exported globals reads and writes it.
fn main() {
    println!("cargo:rustc-link-arg=--export=__stack_pointer");
    println!("cargo:rerun-if-changed=build.rs");
}
