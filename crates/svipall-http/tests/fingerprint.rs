//! Network checks that the http tier really looks like a browser.
//!
//! Ignored by default; run with `cargo test -p svipall-http -- --ignored`.
//!
//! These assert *structural properties*, not exact JA4 hashes. A pinned hash would break on every
//! patch bump of the emulation library while telling us nothing new; cipher and extension counts,
//! the ALPN marker and the presence of GREASE are what actually separate a browser from rustls, and
//! they survive upgrades.

use svipall_core::{IdentityProfile, Os};
use svipall_http::{build, Engine, FetcherConfig, HttpRequest};

const PEET: &str = "https://tls.peet.ws/api/all";

/// JA4_a is `t13d` + two-digit cipher count + two-digit extension count + two-char ALPN.
fn ja4_ciphers(ja4: &str) -> u32 {
    ja4[4..6].parse().unwrap_or(0)
}
fn ja4_extensions(ja4: &str) -> u32 {
    ja4[6..8].parse().unwrap_or(0)
}
fn ja4_alpn(ja4: &str) -> &str {
    &ja4[8..10]
}

async fn probe(engine: Engine) -> serde_json::Value {
    let mut cfg = FetcherConfig::new(IdentityProfile::for_major(147, Os::host()));
    cfg.engine = engine;
    let f = build(cfg).expect("build fetcher");
    let resp = f
        .send(HttpRequest::get(PEET))
        .await
        .expect("tls.peet.ws unreachable");
    serde_json::from_slice(&resp.body).expect("peet returned non-JSON")
}

/// Both engines have to follow a redirect, and for a long time only one of them did.
///
/// `reqwest` sets `Policy::limited(10)` explicitly; the impersonating engine set nothing, and
/// `wreq`'s default is to follow none — so on the build everybody actually ships, any URL that
/// redirects came back as the 3xx stub. `https://x.com/explore` returned seventy-four bytes
/// reading "Found. Redirecting to /i/flow/login", and the classifier, reasonably, called that a
/// delivered page. Plain-HTTP GitHub redirects to HTTPS, which is the simplest form of the same
/// thing and the one every site does.
#[tokio::test]
#[ignore = "network"]
async fn both_engines_follow_a_redirect() {
    for engine in [Engine::Auto, Engine::Reqwest] {
        let mut cfg = FetcherConfig::new(IdentityProfile::for_major(147, Os::host()));
        cfg.engine = engine;
        let Ok(f) = build(cfg) else { continue };
        let resp = f
            .send(HttpRequest::get("http://github.com/"))
            .await
            .expect("github unreachable");
        assert!(
            (200..300).contains(&resp.status),
            "{:?} stopped at {} instead of following the redirect",
            engine,
            resp.status
        );
        assert!(
            resp.final_url.starts_with("https://"),
            "{:?} reported {} as the final URL",
            engine,
            resp.final_url
        );
    }
}

#[tokio::test]
#[ignore = "network"]
async fn http_tier_is_shaped_like_chrome() {
    let v = probe(Engine::Auto).await;
    let ja4 = v["tls"]["ja4"].as_str().expect("no ja4 in response");

    assert_eq!(
        ja4_alpn(ja4),
        "h2",
        "JA4 carries no h2 ALPN marker, so the handshake never offered HTTP/2: {ja4}"
    );
    assert!(
        v["http_version"].as_str().unwrap_or_default().contains('2'),
        "connection did not negotiate HTTP/2: {}",
        v["http_version"]
    );
    // rustls sends 20 ciphers and 11 extensions; Chrome is around 15 and 16.
    let ciphers = ja4_ciphers(ja4);
    assert!(
        (13..=17).contains(&ciphers),
        "cipher count {ciphers} is not Chrome-shaped (rustls sends 20): {ja4}"
    );
    let exts = ja4_extensions(ja4);
    assert!(
        (14..=20).contains(&exts),
        "extension count {exts} is not Chrome-shaped (rustls sends 11): {ja4}"
    );
    let grease = v["tls"]["extensions"]
        .as_array()
        .map(|a| {
            a.iter().any(|e| {
                e["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains("grease")
            })
        })
        .unwrap_or(false);
    assert!(
        grease,
        "no GREASE values sent; every real Chrome sends them"
    );
}

/// The User-Agent has to match the handshake. This is the pairing anti-bot vendors cross-check, and
/// it is what the pre-refactor code got wrong: a Chrome 131 UA over a rustls handshake.
#[tokio::test]
#[ignore = "network"]
async fn user_agent_agrees_with_the_emulation() {
    let mut cfg = FetcherConfig::new(IdentityProfile::for_major(147, Os::host()));
    cfg.engine = Engine::Auto;
    let f = build(cfg).expect("build fetcher");
    let resp = f.send(HttpRequest::get(PEET)).await.expect("unreachable");
    let v: serde_json::Value = serde_json::from_slice(&resp.body).expect("non-JSON");
    let seen = v["user_agent"].as_str().unwrap_or_default();
    assert_eq!(
        seen,
        f.identity().user_agent,
        "the server saw a different User-Agent than the identity claims"
    );
    assert!(
        seen.contains(&format!("Chrome/{}", f.identity().chrome_major)),
        "UA does not name the emulated major: {seen}"
    );
}

/// Documents why the feature exists at all: without it the fingerprint is measurably not Chrome.
#[tokio::test]
#[ignore = "network"]
async fn the_fallback_engine_is_measurably_worse() {
    let plain = probe(Engine::Reqwest).await;
    let ja4 = plain["tls"]["ja4"].as_str().unwrap_or_default();
    println!("reqwest ja4: {ja4}  http: {}", plain["http_version"]);
    if svipall_http::impersonation_available() {
        let good = probe(Engine::Auto).await;
        assert_ne!(
            good["tls"]["ja4"], plain["tls"]["ja4"],
            "both engines produced the same fingerprint; emulation is not taking effect"
        );
    }
}
