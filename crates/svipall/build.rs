//! The target triple, so the binary can name the artefact it is.
//!
//! `std::env::consts::{OS, ARCH}` are close but not the same thing: Homebrew, Scoop, winget and
//! `install.sh` all pick a release artefact by target triple, and `x86_64-pc-windows-msvc` and
//! `x86_64-pc-windows-gnu` are different files. Cargo knows the answer at build time and nothing
//! at run time does, so it is stamped in here.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=SVIPALL_TARGET={target}");
}
