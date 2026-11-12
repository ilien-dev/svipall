//! The signed proof-of-work challenge, in both of the wire formats its widget accepts.
//!
//! Written against the JSON a server sends, because that is the only thing a page hands over:
//! what comes back is the counter the server's own check would accept, found inside the bound the
//! challenge states, and never past the caller's patience.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use svipall_core::pow::{hex, Hash, Limits, Signed};

fn digest_form(salt: &str, secret: u64, max: u64) -> String {
    let challenge = hex(&Hash::Sha256.digest(&[salt.as_bytes(), secret.to_string().as_bytes()]));
    format!(
        r#"{{"algorithm":"SHA-256","challenge":"{challenge}","maxnumber":{max},"salt":"{salt}","signature":"s"}}"#
    )
}

fn roomy() -> Limits {
    Limits::within(Duration::from_secs(30))
}

#[test]
fn the_digest_form_is_answered_with_the_number_the_server_chose() {
    let c = Signed::parse(&digest_form("abc?expires=1794466351", 31_337, 50_000)).unwrap();
    let got = c.solve(&roomy()).expect("solved");
    assert_eq!(got.number, 31_337);
    assert!(got.derived_key.is_empty());
    assert!(c.accepts(got.number));
    assert!(!c.accepts(got.number + 1));
}

#[test]
fn the_search_stops_at_the_bound_the_server_stated() {
    // The number exists, but past `maxnumber`: a challenge that was misread, not one to grind.
    let c = Signed::parse(&digest_form("abc", 9_000, 100)).unwrap();
    assert_eq!(c.solve(&roomy()), None);
}

#[test]
fn the_derived_form_is_answered_with_the_counter_and_its_key() {
    // The shape one search engine's wall hands out, with a cost a test can afford. The prefix is
    // taken from the key of the counter the "server" picked, which is how a server makes one.
    let body = |prefix: &str| {
        format!(
            r#"{{"parameters":{{"algorithm":"PBKDF2/SHA-256","cost":50,"keyLength":32,"keyPrefix":"{prefix}","nonce":"45a27178a346b6b15426944219577c42","salt":"c7d0fd8cf85dc0b86d357306aaff3b0b","expiresAt":1794466351}},"signature":"s"}}"#
        )
    };
    let key = Signed::parse(&body("00")).unwrap().key_for(77).unwrap();
    let c = Signed::parse(&body(&hex(&key[..16]))).unwrap();
    let got = c.solve(&roomy()).expect("solved");
    assert_eq!(got.number, 77);
    assert_eq!(got.derived_key, hex(&key));
    assert!(c.accepts(77) && !c.accepts(76));
}

#[test]
fn the_derivation_is_the_standard_one() {
    // RFC 7914 section 11, PBKDF2-HMAC-SHA-256, one round. A wrong round count or a wrong
    // password layout would still "solve" a challenge this crate wrote for itself.
    let mut out = [0u8; 64];
    Hash::Sha256.derive(b"passwd", b"salt", 1, &mut out);
    assert_eq!(
        hex(&out),
        "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
         49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
    );
}

#[test]
fn a_cancelled_hunt_stops() {
    let c = Signed::parse(&digest_form("abc", 40_000_000, 50_000_000)).unwrap();
    let limits = roomy();
    limits.cancel.store(true, Ordering::Relaxed);
    let started = Instant::now();
    assert_eq!(c.solve(&limits), None);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_hunt_that_runs_out_of_time_stops() {
    let c = Signed::parse(&digest_form("abc", 40_000_000, 50_000_000)).unwrap();
    let started = Instant::now();
    assert_eq!(c.solve(&Limits::within(Duration::from_millis(50))), None);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn what_is_not_a_signed_challenge_is_refused() {
    assert_eq!(Signed::parse("not json"), None);
    assert_eq!(Signed::parse(r#"{"salt":"x"}"#), None);
    assert_eq!(
        Signed::parse(r#"{"algorithm":"MD5","challenge":"00","salt":"x"}"#),
        None
    );
    // A memory-hard derivation is the page's own widget's to run.
    assert_eq!(
        Signed::parse(
            r#"{"parameters":{"algorithm":"ARGON2ID","cost":2,"keyPrefix":"00","nonce":"00","salt":"00"},"signature":"s"}"#
        ),
        None
    );
}
