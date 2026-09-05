//! The QUIC handshake this crate exists to shape.
//!
//! A QUIC ClientHello is the first thing a CDN sees, and the sites that offer HTTP/3 are exactly
//! the ones that look at it. Upstream quiche is not trying to look like a browser and does not
//! emit what Chrome emits; this crate's whole reason to exist as a patched copy is that it does.
//!
//! Everything here is offline and deterministic: no socket, no server, no network. The reference
//! these assertions encode was captured from a real Chrome for Testing 152 the same way, and is
//! written down in `docs/http3.md`.

mod support;

use support::first_flight;

/// Extension numbers, by their names rather than their digits.
const SERVER_NAME: u16 = 0;
const SUPPORTED_GROUPS: u16 = 10;
const SIGNATURE_ALGORITHMS: u16 = 13;
const ALPN: u16 = 16;
const COMPRESS_CERTIFICATE: u16 = 27;
const SUPPORTED_VERSIONS: u16 = 43;
const PSK_KEY_EXCHANGE_MODES: u16 = 45;
const KEY_SHARE: u16 = 51;
const QUIC_TRANSPORT_PARAMETERS: u16 = 57;
/// Application settings. Chrome uses the new codepoint; 17513 is the original.
const ALPS: u16 = 17613;
const TRUST_ANCHORS: u16 = 0xca34;
const ECH_GREASE: u16 = 65037;

fn chrome_config() -> quiche::Config {
    let mut cfg = quiche::Config::chrome(&[b"h3"]).expect("a config");
    cfg.verify_peer(false);
    cfg
}

#[test]
fn the_handshake_carries_the_two_extensions_no_other_rust_quic_stack_emits() {
    let ch = first_flight(&mut chrome_config());
    assert!(
        ch.has(ALPS),
        "application settings (17613) is absent; a QUIC ClientHello without it is not Chrome's"
    );
    assert!(
        ch.has(ECH_GREASE),
        "ECH GREASE (65037) is absent; Chrome offers one for every name that publishes no config"
    );
    assert!(
        ch.has(QUIC_TRANSPORT_PARAMETERS),
        "extension 57 is what makes this a QUIC handshake rather than a TCP one"
    );
}

#[test]
fn the_handshake_carries_every_extension_the_measured_chrome_sends() {
    let ch = first_flight(&mut chrome_config());
    for want in [
        SERVER_NAME,
        SUPPORTED_GROUPS,
        SIGNATURE_ALGORITHMS,
        ALPN,
        COMPRESS_CERTIFICATE,
        SUPPORTED_VERSIONS,
        PSK_KEY_EXCHANGE_MODES,
        KEY_SHARE,
        QUIC_TRANSPORT_PARAMETERS,
        ALPS,
        TRUST_ANCHORS,
        ECH_GREASE,
    ] {
        assert!(
            ch.has(want),
            "extension {want} (0x{want:04x}) is in Chrome's QUIC ClientHello and not in ours; \
             the whole list was {:04x?}",
            ch.order()
        );
    }
}

#[test]
fn the_extension_order_is_permuted_per_connection_rather_than_fixed() {
    // Chrome permutes: three captured runs shared not a single position. A fixed order is a
    // constant across every connection this machine ever makes, which is the easiest thing there
    // is to key on.
    let orders: Vec<Vec<u16>> = (0..8)
        .map(|_| first_flight(&mut chrome_config()).order())
        .collect();
    assert!(
        orders.iter().any(|o| *o != orders[0]),
        "eight handshakes produced one identical extension order: {:04x?}",
        orders[0]
    );
}

#[test]
fn the_cipher_list_and_session_id_are_the_ones_chrome_offers_over_quic() {
    let ch = first_flight(&mut chrome_config());
    assert_eq!(
        ch.ciphers,
        vec![0x1301, 0x1302, 0x1303],
        "Chrome offers exactly the three TLS 1.3 suites over QUIC, and no GREASE cipher"
    );
    assert_eq!(
        ch.session_id_len, 0,
        "Chrome sends an empty legacy_session_id over QUIC, unlike over TCP"
    );
}

#[test]
fn the_transport_parameters_carry_a_grease_of_their_own() {
    // There is no GREASE cipher and no GREASE extension in Chrome's QUIC ClientHello, but there is
    // a GREASE transport parameter, with a fresh random id every connection. Sending none is as
    // much of a tell as sending a fixed one.
    let a = first_flight(&mut chrome_config());
    let b = first_flight(&mut chrome_config());
    let grease = |ch: &support::ClientHello| -> Option<u64> {
        ch.transport_params
            .iter()
            .map(|(id, _)| *id)
            .find(|id| *id >= 27 && (id - 27).is_multiple_of(31))
    };
    let (ga, gb) = (grease(&a), grease(&b));
    assert!(ga.is_some(), "no GREASE transport parameter was sent");
    assert_ne!(
        ga, gb,
        "the GREASE transport parameter is the same on every connection"
    );
}

#[test]
fn the_transport_parameters_are_the_ones_chrome_declares() {
    let ch = first_flight(&mut chrome_config());
    let ids: Vec<u64> = ch.transport_params.iter().map(|(id, _)| *id).collect();
    // Everything Chrome sends that is not Google-private and not the GREASE.
    for want in [
        0x01, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0f, 0x11, 0x20,
    ] {
        assert!(
            ids.contains(&want),
            "transport parameter 0x{want:02x} is in Chrome's list and not in ours; ours is {ids:02x?}"
        );
    }
}
