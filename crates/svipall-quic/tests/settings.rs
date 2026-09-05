//! The HTTP/3 SETTINGS frame, against a measured Chrome.
//!
//! `docs/http3.md` carried *"HTTP/3 SETTINGS — not compared against Chrome's"* as an open row for
//! as long as this crate has existed. The reason is structural: SETTINGS travels on a control
//! stream at 1-RTT, so unlike the ClientHello it cannot be read out of a datagram nobody answered.
//! `bench h3-ref` closes that by running a QUIC server a real Chrome completes a handshake with,
//! and the numbers below are what it read.
//!
//! This file is the assertion built on that reference, and it is offline: a client and a server
//! made in this process exchange datagrams through a buffer, and the server reads the client's
//! SETTINGS with `peer_settings_raw` — **the same accessor** `bench h3-ref` used on Chrome. So the
//! two halves of the comparison are taken the same way, which is the only reason comparing them
//! means anything.

use std::net::SocketAddr;

/// **Chrome for Testing 152.0.7977.75, four runs**, read by `bench h3-ref`.
///
/// Identical every run, in this order, with a GREASE appended. Unlike the TLS extension list —
/// which Chrome permutes per connection, so its *set* is the fingerprint and its order is not —
/// this order did not move across four connections, so it is asserted as given.
const CHROME_SETTINGS: &[(u64, u64)] = &[
    // QPACK_MAX_TABLE_CAPACITY
    (0x01, 65536),
    // MAX_FIELD_SECTION_SIZE
    (0x06, 262_144),
    // QPACK_BLOCKED_STREAMS
    (0x07, 100),
    // H3_DATAGRAM. Chrome sends this codepoint and **not** the draft one (0x276), which upstream
    // quiche emits beside it.
    (0x33, 1),
];

/// RFC 9114 section 7.2.4.1: reserved setting identifiers are `0x1f * N + 0x21`.
fn is_grease(id: u64) -> bool {
    id >= 0x21 && (id - 0x21).is_multiple_of(0x1f)
}

fn client_addr() -> SocketAddr {
    "127.0.0.1:1234".parse().unwrap()
}
fn server_addr() -> SocketAddr {
    "127.0.0.1:4433".parse().unwrap()
}

/// What the server read off one client connection: the raw SETTINGS, in wire order.
fn settings_our_client_sends() -> Vec<(u64, u64)> {
    let mut client_config = quiche::Config::chrome(&[b"h3"]).expect("a Chrome-shaped config");
    // The server certificate below is self-signed for a name that resolves nowhere. Verification
    // is what the engine does against a real site and is not what this test is about.
    client_config.verify_peer(false);

    let mut server_config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
    // Made here and deleted when it drops. Nothing about the certificate is under test — the
    // server simply needs one to finish a handshake with — and a repository is a poor place to
    // keep a private key that only ever authenticates a name resolving nowhere.
    let ca = quiche::selfsigned::generate("quic.test").expect("a certificate");
    server_config
        .load_cert_chain_from_pem_file(ca.cert_pem.to_str().unwrap())
        .unwrap();
    server_config
        .load_priv_key_from_pem_file(ca.key_pem.to_str().unwrap())
        .unwrap();
    server_config.set_application_protos(&[b"h3"]).unwrap();
    server_config.set_max_idle_timeout(5_000);
    server_config.set_max_recv_udp_payload_size(1500);
    server_config.set_initial_max_data(10_000_000);
    server_config.set_initial_max_stream_data_bidi_local(1_000_000);
    server_config.set_initial_max_stream_data_bidi_remote(1_000_000);
    server_config.set_initial_max_stream_data_uni(1_000_000);
    server_config.set_initial_max_streams_bidi(100);
    server_config.set_initial_max_streams_uni(100);
    server_config.enable_dgram(true, 65536, 65536);

    let mut scid = [0u8; quiche::MAX_CONN_ID_LEN];
    fill(&mut scid);
    let scid = quiche::ConnectionId::from_ref(&scid);
    let mut client = quiche::connect(
        Some("quic.test"),
        &scid,
        client_addr(),
        server_addr(),
        &mut client_config,
    )
    .expect("a client connection");

    let mut sscid = [0u8; quiche::MAX_CONN_ID_LEN];
    fill(&mut sscid);
    let sscid = quiche::ConnectionId::from_ref(&sscid);
    let mut server = quiche::accept(
        &sscid,
        None,
        server_addr(),
        client_addr(),
        &mut server_config,
    )
    .expect("a server connection");

    let h3_config = quiche::h3::Config::chrome().expect("a Chrome-shaped h3 config");
    let mut client_h3: Option<quiche::h3::Connection> = None;
    let mut server_h3: Option<quiche::h3::Connection> = None;

    // Twenty rounds is many more than a local handshake needs and still terminates if one stalls.
    for _ in 0..20 {
        flush(&mut client, &mut server);
        flush(&mut server, &mut client);

        if client.is_established() && client_h3.is_none() {
            client_h3 = Some(
                quiche::h3::Connection::with_transport(&mut client, &h3_config)
                    .expect("an h3 client"),
            );
        }
        if server.is_established() && server_h3.is_none() {
            server_h3 = Some(
                quiche::h3::Connection::with_transport(
                    &mut server,
                    &quiche::h3::Config::new().unwrap(),
                )
                .expect("an h3 server"),
            );
        }
        if let Some(h) = server_h3.as_mut() {
            while h.poll(&mut server).is_ok() {}
            if let Some(raw) = h.peer_settings_raw() {
                return raw.to_vec();
            }
        }
    }
    panic!("the server never read a SETTINGS frame from our own client");
}

/// Move everything one side wants to send into the other.
fn flush(from: &mut quiche::Connection, to: &mut quiche::Connection) {
    let mut buf = [0u8; 1500];
    while let Ok((written, info)) = from.send(&mut buf) {
        let recv = quiche::RecvInfo {
            from: info.from,
            to: info.to,
        };
        let _ = to.recv(&mut buf[..written], recv);
    }
}

fn fill(buf: &mut [u8]) {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut at = 0;
    while at < buf.len() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(at);
        for b in h.finish().to_le_bytes() {
            if at < buf.len() {
                buf[at] = b;
                at += 1;
            }
        }
    }
}

#[test]
fn the_settings_are_the_ones_chrome_sends_in_the_order_chrome_sends_them() {
    let ours = settings_our_client_sends();
    let named: Vec<(u64, u64)> = ours
        .iter()
        .copied()
        .filter(|(id, _)| !is_grease(*id))
        .collect();
    assert_eq!(
        named, CHROME_SETTINGS,
        "our SETTINGS are not Chrome's. Chrome's were measured by `bench h3-ref`; the whole of \
         ours, GREASE included, was {ours:?}"
    );
}

#[test]
fn the_draft_datagram_codepoint_upstream_adds_is_not_sent() {
    // Upstream quiche writes 0x276 and 0x33 together whenever datagrams are on. Chrome sends only
    // 0x33, so the pair is a constant no browser produces — and it is the kind of extra that a
    // server logging raw settings can key on for free.
    let ours = settings_our_client_sends();
    assert!(
        !ours.iter().any(|(id, _)| *id == 0x276),
        "the draft H3_DATAGRAM codepoint is still being sent: {ours:?}"
    );
}

#[test]
fn a_grease_setting_is_sent_last_and_is_fresh_every_connection() {
    // Chrome appends exactly one reserved setting, with a fresh identifier and a fresh value on
    // every connection. Sending none is as much of a tell as sending a fixed one.
    let a = settings_our_client_sends();
    let b = settings_our_client_sends();

    let grease = |s: &[(u64, u64)]| -> (u64, u64) {
        let last = *s.last().expect("some settings");
        assert!(
            is_grease(last.0),
            "the last setting is not a reserved identifier: {s:?}"
        );
        assert_eq!(
            s.iter().filter(|(id, _)| is_grease(*id)).count(),
            1,
            "Chrome sends exactly one GREASE setting: {s:?}"
        );
        last
    };
    assert_ne!(
        grease(&a),
        grease(&b),
        "the GREASE setting is identical on two connections"
    );
}
