//! One TLS stack in the binaries we link, and it is BoringSSL.
//!
//! `reqwest`'s default features pull `native-tls` -> `openssl-sys`, which asks the linker for
//! `-lssl -lcrypto`. BoringSSL's own `-L` directories come first on that command line, so those
//! flags resolve to BoringSSL's static archives, which do not define the OpenSSL 3 entry points
//! `openssl-sys` references (`SSL_CTX_ctrl`, `ERR_get_error_all`, `SSL_read_ex`, ...) and the link
//! fails. The reqwest fallback engine speaks rustls; nothing linked here wants OpenSSL.
//!
//! `openssl-sys` itself stays in the lockfile as a build-dependency of `ort-sys`, which links into
//! build scripts and never into ours, so the markers checked are the ones only `default-tls`
//! brings in.

use std::path::Path;

fn workspace_file(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("workspace {name}: {e}"))
}

#[test]
fn no_native_tls_in_the_linked_graph() {
    let lock = workspace_file("Cargo.lock");

    let banned = ["hyper-tls", "tokio-native-tls"];
    let found: Vec<&str> = banned
        .into_iter()
        .filter(|name| lock.contains(&format!("\nname = \"{name}\"\n")))
        .collect();

    assert!(
        found.is_empty(),
        "native-tls crates in the lockfile: {found:?}. They drag in OpenSSL, which collides with \
         BoringSSL at link time; keep `reqwest` on rustls with `default-features = false`."
    );
}

#[test]
fn reqwest_is_declared_without_default_tls() {
    let manifest = workspace_file("Cargo.toml");
    let line = manifest
        .lines()
        .find(|l| l.starts_with("reqwest = "))
        .expect("workspace reqwest dependency");

    assert!(
        line.contains("default-features = false"),
        "workspace reqwest must set `default-features = false` to keep `default-tls` out: {line}"
    );
}
