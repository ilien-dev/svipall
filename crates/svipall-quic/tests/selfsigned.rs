//! The ephemeral certificate the QUIC handshake reference serves.
//!
//! In `tests/` rather than beside the code because this crate sets `test = false`: it carries
//! upstream's `#[cfg(test)]` modules nowhere, and the invariants it is patched to hold live here.
//! See `PATCHES.md` entry 11 for why the certificate is generated rather than committed.

use quiche::selfsigned;

#[test]
fn the_certificate_and_key_are_written_and_quiche_loads_them() {
    let s = selfsigned::generate("quic.test").expect("a certificate");
    assert!(s.cert_pem.is_file(), "no certificate was written");
    assert!(s.key_pem.is_file(), "no key was written");

    // The point of the whole module: a quiche server config has to accept both.
    let mut cfg = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
    cfg.load_cert_chain_from_pem_file(s.cert_pem.to_str().unwrap())
        .expect("quiche loads the certificate");
    cfg.load_priv_key_from_pem_file(s.key_pem.to_str().unwrap())
        .expect("quiche loads the key");
}

#[test]
fn nothing_is_left_on_disk_afterwards() {
    // A generator that leaks a private key into the temporary directory on every run would be
    // worse than the committed one it replaced, not better.
    let (dir, cert) = {
        let s = selfsigned::generate("quic.test").expect("a certificate");
        (
            s.cert_pem.parent().expect("a directory").to_path_buf(),
            s.cert_pem.clone(),
        )
    };
    assert!(!cert.exists(), "the certificate outlived its owner");
    assert!(!dir.exists(), "the directory outlived its owner");
}

#[test]
fn every_run_is_a_different_key() {
    let a = selfsigned::generate("quic.test").expect("a certificate");
    let b = selfsigned::generate("quic.test").expect("a certificate");
    assert_ne!(
        a.spki_sha256, b.spki_sha256,
        "two runs produced the same key, so it is not ephemeral"
    );
}

#[test]
fn the_pin_has_the_shape_chromes_flag_accepts() {
    let s = selfsigned::generate("quic.test").expect("a certificate");
    let pin = s.spki_pin();
    // Thirty-two bytes is forty-four base64 characters, the last of them a pad.
    assert_eq!(pin.len(), 44, "pin was {pin:?}");
    assert!(pin.ends_with('='), "pin was {pin:?}");
    assert!(
        pin.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
        "pin was {pin:?}"
    );
}

#[test]
fn the_base64_encoder_agrees_with_a_known_vector() {
    // The encoder is written here rather than taken from a crate, so it gets a vector of its own
    // instead of only ever being exercised through a digest nothing else knows. This is SHA-256 of
    // the empty string, and its base64 is the one every other implementation produces.
    let empty_sha256 = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    assert_eq!(
        selfsigned::base64_pin(&empty_sha256),
        "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
    );
}
