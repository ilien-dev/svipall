//! Fallback engine. Correct HTTP, wrong fingerprint.
//!
//! rustls offers a cipher list, extension order and key-share set that no Chrome ever sent, and
//! exposes no way to change any of it. This exists so the crate still builds and runs without the
//! BoringSSL toolchain, not because it is good enough for a guarded site.

use crate::{FetcherConfig, HttpFetcher, HttpRequest, HttpResponse};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use svipall_core::IdentityProfile;

pub struct ReqwestFetcher {
    client: reqwest::Client,
    identity: IdentityProfile,
}

impl ReqwestFetcher {
    pub fn new(cfg: FetcherConfig) -> anyhow::Result<Self> {
        let mut b = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .cookie_store(true)
            .user_agent(cfg.identity.user_agent.clone())
            .redirect(reqwest::redirect::Policy::limited(10));
        if let Some(p) = &cfg.proxy {
            b = b.proxy(reqwest::Proxy::all(p)?);
        }
        Ok(Self {
            client: b.build()?,
            identity: cfg.identity,
        })
    }
}

#[async_trait::async_trait]
impl HttpFetcher for ReqwestFetcher {
    fn engine(&self) -> &'static str {
        "reqwest"
    }

    fn identity(&self) -> &IdentityProfile {
        &self.identity
    }

    async fn send(&self, req: HttpRequest) -> anyhow::Result<HttpResponse> {
        let method = reqwest::Method::from_bytes(req.method.to_uppercase().as_bytes())?;
        let mut headers = HeaderMap::new();
        for (k, v) in &req.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(name, value);
            }
        }
        let mut r = self.client.request(method, &req.url).headers(headers);
        if let Some(body) = req.body {
            r = r.body(body);
        }
        let resp = r.send().await?;
        let status = resp.status().as_u16();
        let final_url = resp.url().to_string();
        let http_version = match resp.version() {
            reqwest::Version::HTTP_2 => "HTTP/2.0",
            reqwest::Version::HTTP_3 => "HTTP/3.0",
            reqwest::Version::HTTP_10 => "HTTP/1.0",
            _ => "HTTP/1.1",
        };
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let content_type = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.to_ascii_lowercase())
            .unwrap_or_default();
        Ok(HttpResponse {
            status,
            final_url,
            headers,
            content_type,
            body: resp.bytes().await?.to_vec(),
            http_version,
        })
    }
}
