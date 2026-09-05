//! The HTTP engine behind svipall's `http` tier.
//!
//! Why this is its own crate: the emulating engine pulls in BoringSSL, which needs cmake, perl,
//! llvm and nasm to build. Keeping it here means `cargo test -p svipall-core` and the solver and
//! dashboard crates never touch that toolchain, and quality control can treat the emulating path as
//! a separate step.
//!
//! Measured against `https://tls.peet.ws/api/all`, the difference is not subtle:
//!
//! | signal            | reqwest + rustls        | wreq (Chrome emulation)      |
//! |-------------------|-------------------------|------------------------------|
//! | negotiated        | HTTP/1.1                | h2                           |
//! | JA4               | `t13d2011_…` (no h2)    | `t13d1516h2_8daaf6152771_…`  |
//! | TLS extensions    | 11                      | 18                           |
//! | GREASE            | absent                  | present                      |
//!
//! The emulating engine also offers `X25519MLKEM768` with a key share, as Chrome 131+ does; the
//! benchmark asserts it against `tls.peet.ws` (`supported_groups` and `key_share`, never `ja4_r`,
//! which does not carry groups). The reqwest fallback does not offer it.

use std::time::Duration;
use svipall_core::IdentityProfile;

#[cfg(feature = "http3")]
mod h3_engine;
mod reqwest_engine;
#[cfg(feature = "impersonate")]
mod wreq_engine;

/// A request as the tier describes it, before any engine touches it.
#[derive(Debug, Clone, Default)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    /// Ordered, and deliberately not a map. Header order is part of the HTTP/2 fingerprint; the
    /// emulating engine preserves it, reqwest normalises it away, and that difference should be
    /// visible in the type rather than hidden.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: "GET".into(),
            ..Default::default()
        }
    }

    /// Add or replace a header by name, preserving the position of an existing one.
    pub fn set_header(&mut self, name: &str, value: &str) {
        if let Some(slot) = self
            .headers
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
        {
            slot.1 = value.to_string();
        } else {
            self.headers.push((name.to_string(), value.to_string()));
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub final_url: String,
    pub headers: Vec<(String, String)>,
    pub content_type: String,
    /// Raw bytes. Kept undecoded so a PDF is not mangled into a lossy `String` on the way through,
    /// which is what the old `r.text()` did to every response regardless of type.
    pub body: Vec<u8>,
    pub http_version: &'static str,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Body as text, lossily. Only call this once the content type says it is text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn looks_like_pdf(&self) -> bool {
        self.content_type.contains("application/pdf") || self.body.starts_with(b"%PDF-")
    }
}

#[async_trait::async_trait]
pub trait HttpFetcher: Send + Sync + 'static {
    fn engine(&self) -> &'static str;
    /// The identity this fetcher advertises. Callers must not invent their own User-Agent.
    fn identity(&self) -> &IdentityProfile;
    async fn send(&self, req: HttpRequest) -> anyhow::Result<HttpResponse>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Auto,
    Reqwest,
    Impersonate,
}

impl Engine {
    /// `SVIPALL_HTTP_ENGINE` beats config, config beats `Auto`.
    pub fn resolve(cfg_value: &str) -> Engine {
        let raw = std::env::var("SVIPALL_HTTP_ENGINE").unwrap_or_else(|_| cfg_value.to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "reqwest" | "plain" => Engine::Reqwest,
            "impersonate" | "wreq" | "chrome" => Engine::Impersonate,
            _ => Engine::Auto,
        }
    }
}

pub struct FetcherConfig {
    pub identity: IdentityProfile,
    pub engine: Engine,
    pub proxy: Option<String>,
    pub timeout: Duration,
}

impl FetcherConfig {
    pub fn new(identity: IdentityProfile) -> Self {
        Self {
            identity,
            engine: Engine::Auto,
            proxy: None,
            timeout: Duration::from_secs(30),
        }
    }
}

/// True when this binary can emulate a browser's TLS and HTTP/2 fingerprint.
pub const fn impersonation_available() -> bool {
    cfg!(feature = "impersonate")
}

/// Build a fetcher. `Auto` prefers emulation when it is compiled in; asking for `Impersonate` in a
/// binary built without it is an error rather than a silent downgrade, because a silent downgrade
/// is exactly the failure that is hard to notice.
pub fn build(cfg: FetcherConfig) -> anyhow::Result<std::sync::Arc<dyn HttpFetcher>> {
    match cfg.engine {
        Engine::Impersonate if !impersonation_available() => anyhow::bail!(
            "http_engine=impersonate was requested but this binary was built without \
             --features impersonate; rebuild with it or set http_engine=reqwest"
        ),
        #[cfg(feature = "impersonate")]
        Engine::Auto | Engine::Impersonate => {
            Ok(std::sync::Arc::new(wreq_engine::WreqFetcher::new(cfg)?))
        }
        _ => Ok(std::sync::Arc::new(reqwest_engine::ReqwestFetcher::new(
            cfg,
        )?)),
    }
}

/// True when this binary can speak HTTP/3.
pub const fn http3_available() -> bool {
    cfg!(feature = "http3")
}

/// Wrap a fetcher so that it speaks HTTP/3, falling back to it when a site does not answer.
///
/// Deliberately a wrapper rather than another arm of `build`: h3 is never the whole story. Chrome
/// learns from `Alt-Svc` that a site offers it and uses it *next time*, so a first visit is always
/// TCP, and a site that advertised h3 and then goes quiet on UDP still has to produce a page. The
/// caller therefore always has a TCP fetcher, and this borrows it.
///
/// Returns `None` in a binary built without `--features http3`, so a caller can say so once at
/// startup instead of discovering it per request.
#[allow(unused_variables)]
pub fn with_http3(
    cfg: &FetcherConfig,
    fallback: std::sync::Arc<dyn HttpFetcher>,
) -> Option<std::sync::Arc<dyn HttpFetcher>> {
    #[cfg(feature = "http3")]
    {
        // A proxy is a TCP concept here: the exit is reached by CONNECT, and QUIC does not go
        // through one. An operator who set an exit means it, so h3 is declined rather than
        // quietly bypassing the proxy with this machine's own address.
        if cfg.proxy.is_some() {
            None
        } else {
            Some(std::sync::Arc::new(h3_engine::H3Fetcher::new(
                cfg, fallback,
            )))
        }
    }
    #[cfg(not(feature = "http3"))]
    None
}

/// One line for the log and for `web_status`, so a degraded build never goes unnoticed.
pub fn engine_report(active: &str) -> String {
    if active == "wreq" {
        "wreq (Chrome TLS and HTTP/2 emulation)".to_string()
    } else {
        "reqwest (degraded: TLS fingerprint is not Chrome; rebuild with --features impersonate)"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Why this tier does not rotate machines, stated as a test rather than a comment.
    ///
    /// Everything `fleet` draws — screen, cores, memory, GPU, pixel ratio — is a JavaScript
    /// surface. None of it reaches plain HTTP, where what is visible is the user agent, the client
    /// hints, `Accept-Language` and the shape of the handshake, all decided by the engine, the
    /// major and the OS. So a fetcher built on a drawn machine would be byte for byte the fetcher
    /// built without one, and giving every domain its own would rebuild the TLS profile, the
    /// connection pool and the cookie jar to no effect.
    ///
    /// The day a field that *is* visible over HTTP joins the draw, this fails, and whoever added it
    /// has to key the fetcher cache by machine as well as by exit.
    #[test]
    fn a_drawn_machine_never_changes_what_goes_on_the_wire() {
        use svipall_core::{Os, MAX_EMULATED_CHROME};
        let base = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows);
        for seed in 0..200u64 {
            let id = base.clone().as_machine(seed.wrapping_mul(0x9E37_79B9));
            assert_eq!(id.nav_headers(), base.nav_headers(), "seed {seed}");
            assert_eq!(id.user_agent, base.user_agent);
            assert_eq!(id.sec_ch_ua, base.sec_ch_ua);
            assert_eq!(id.sec_ch_ua_platform, base.sec_ch_ua_platform);
            assert_eq!(id.sec_ch_ua_mobile, base.sec_ch_ua_mobile);
            assert_eq!(id.accept_language, base.accept_language);
            assert_eq!(id.chrome_major, base.chrome_major);
            assert_eq!(id.os, base.os);
            assert_eq!(id.engine, base.engine);
        }
    }

    #[test]
    fn engine_resolves_from_config_words() {
        assert_eq!(Engine::resolve("reqwest"), Engine::Reqwest);
        assert_eq!(Engine::resolve("impersonate"), Engine::Impersonate);
        assert_eq!(Engine::resolve("auto"), Engine::Auto);
        assert_eq!(Engine::resolve(""), Engine::Auto);
        assert_eq!(Engine::resolve("nonsense"), Engine::Auto);
    }

    #[test]
    fn set_header_replaces_in_place_and_is_case_insensitive() {
        let mut r = HttpRequest::get("https://example.com");
        r.set_header("User-Agent", "a");
        r.set_header("Accept", "b");
        r.set_header("user-agent", "c");
        assert_eq!(
            r.headers.len(),
            2,
            "the third call replaced, it did not add"
        );
        assert_eq!(r.header("USER-AGENT"), Some("c"));
        assert_eq!(
            r.headers.iter().position(|(k, _)| k == "User-Agent"),
            Some(0),
            "replacing must not move the header"
        );
    }

    #[test]
    fn asking_for_impersonation_without_the_feature_is_an_error_not_a_downgrade() {
        let mut cfg =
            FetcherConfig::new(IdentityProfile::for_major(147, svipall_core::Os::Windows));
        cfg.engine = Engine::Impersonate;
        let built = build(cfg);
        assert_eq!(built.is_err(), !impersonation_available());
    }

    #[test]
    fn a_pdf_body_is_recognised_without_a_content_type() {
        let r = HttpResponse {
            status: 200,
            final_url: "https://example.com/a.pdf".into(),
            headers: vec![],
            content_type: "application/octet-stream".into(),
            body: b"%PDF-1.7 ...".to_vec(),
            http_version: "HTTP/2.0",
        };
        assert!(r.looks_like_pdf());
    }
}
