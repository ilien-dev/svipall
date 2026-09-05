//! HTTP/3 never costs a page.
//!
//! Every other property of the QUIC engine is about looking like Chrome, and those are asserted
//! where the packets are, in `svipall-quic`. This file defends the one property that is not about
//! fingerprints at all: a site advertised h3, and h3 did not work, and the caller still gets the
//! page. `Alt-Svc` is a hint from the *server*; a network that drops 443/udp, a stale
//! advertisement or a load balancer that moved are all ordinary, and none of them is a failed
//! fetch.

#![cfg(feature = "http3")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use svipall_core::{IdentityProfile, Os};
use svipall_http::{FetcherConfig, HttpFetcher, HttpRequest, HttpResponse};

/// A fetcher that answers instantly and counts how often it was asked.
struct Counted {
    identity: IdentityProfile,
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl HttpFetcher for Counted {
    fn engine(&self) -> &'static str {
        "counted"
    }
    fn identity(&self) -> &IdentityProfile {
        &self.identity
    }
    async fn send(&self, req: HttpRequest) -> anyhow::Result<HttpResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(HttpResponse {
            status: 200,
            final_url: req.url,
            headers: vec![("content-type".into(), "text/html".into())],
            content_type: "text/html".into(),
            body: b"<html><body>the page</body></html>".to_vec(),
            http_version: "HTTP/2.0",
        })
    }
}

fn identity() -> IdentityProfile {
    IdentityProfile::for_major(svipall_core::MAX_EMULATED_CHROME, Os::Windows)
}

#[tokio::test]
async fn a_site_that_does_not_answer_over_quic_still_returns_its_page() {
    let calls = Arc::new(AtomicUsize::new(0));
    let fallback: Arc<dyn HttpFetcher> = Arc::new(Counted {
        identity: identity(),
        calls: calls.clone(),
    });

    let mut cfg = FetcherConfig::new(identity());
    // Short, because the point is that failure is quick and quiet, not that it is patient.
    cfg.timeout = std::time::Duration::from_secs(5);
    let h3 = svipall_http::with_http3(&cfg, fallback).expect("h3 is compiled in");

    // Port 1, where nothing is listening and nothing ever will be.
    let r = h3
        .send(HttpRequest::get("https://127.0.0.1:1/"))
        .await
        .expect("a page, not an error");

    assert_eq!(r.status, 200, "the fallback's answer is the answer");
    assert_eq!(r.http_version, "HTTP/2.0", "this page came back over TCP");
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "the fallback was asked exactly once"
    );
}

#[tokio::test]
async fn an_exit_is_never_bypassed_by_speaking_quic_around_it() {
    // A proxy is reached by CONNECT, which QUIC does not do. An h3 fetcher built anyway would
    // leave through this machine's own address, which is the one thing an operator who set an
    // exit is asking not to happen.
    let fallback: Arc<dyn HttpFetcher> = Arc::new(Counted {
        identity: identity(),
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let mut cfg = FetcherConfig::new(identity());
    cfg.proxy = Some("http://127.0.0.1:9".into());
    assert!(
        svipall_http::with_http3(&cfg, fallback).is_none(),
        "h3 must decline rather than route around the exit"
    );
}

#[tokio::test]
#[ignore = "network"]
async fn a_real_site_answers_over_http3() {
    // Run by hand: `cargo test -p svipall-http --features http3 --test h3 -- --ignored`.
    // The fallback is a fetcher that fails, so a pass here can only mean QUIC carried the page.
    struct Never(IdentityProfile);
    #[async_trait::async_trait]
    impl HttpFetcher for Never {
        fn engine(&self) -> &'static str {
            "never"
        }
        fn identity(&self) -> &IdentityProfile {
            &self.0
        }
        async fn send(&self, _: HttpRequest) -> anyhow::Result<HttpResponse> {
            anyhow::bail!("the fallback was used, so this page did not come over h3")
        }
    }

    let mut cfg = FetcherConfig::new(identity());
    cfg.timeout = std::time::Duration::from_secs(20);
    let h3 = svipall_http::with_http3(&cfg, Arc::new(Never(identity()))).expect("h3");
    let r = h3
        .send(HttpRequest::get("https://cloudflare-quic.com/"))
        .await
        .expect("a page over h3");
    assert_eq!(r.http_version, "HTTP/3.0");
    assert!(r.status < 400, "status was {}", r.status);
    assert!(!r.body.is_empty(), "an empty body is not a page");
    eprintln!(
        "h3: {} {} bytes, content-type {}",
        r.status,
        r.body.len(),
        r.content_type
    );
}

#[tokio::test]
async fn a_network_that_swallows_udp_costs_seconds_rather_than_the_whole_budget() {
    // The one way HTTP/3 could make this tool slower than it was without it. A refused port
    // answers at once; a *dropped* one answers never, and the only thing bounding it is a clock.
    // 203.0.113.0/24 is TEST-NET-3, reserved by RFC 5737 and routed nowhere, so packets to it go
    // out and nothing ever comes back — the same shape as a firewall that drops instead of
    // refusing.
    let calls = Arc::new(AtomicUsize::new(0));
    let fallback: Arc<dyn HttpFetcher> = Arc::new(Counted {
        identity: identity(),
        calls: calls.clone(),
    });
    let mut cfg = FetcherConfig::new(identity());
    // A whole navigation budget, exactly as the server hands it over.
    cfg.timeout = std::time::Duration::from_secs(45);
    let h3 = svipall_http::with_http3(&cfg, fallback).expect("h3 is compiled in");

    let started = std::time::Instant::now();
    let r = h3
        .send(HttpRequest::get("https://203.0.113.1/"))
        .await
        .expect("a page, not an error");
    let waited = started.elapsed();

    assert_eq!(r.http_version, "HTTP/2.0", "the fallback answered");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        waited < std::time::Duration::from_secs(10),
        "gave up after {waited:?}; a dropped UDP port must not cost the page budget"
    );
}
