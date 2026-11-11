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
        let resp = self.request(req, None)?.send().await?;
        let mut out = head_of(&resp);
        out.body = resp.bytes().await?.to_vec();
        Ok(out)
    }

    async fn send_streaming(
        &self,
        req: HttpRequest,
        sink: &mut (dyn crate::BodySink + Send),
        total: Option<std::time::Duration>,
    ) -> anyhow::Result<HttpResponse> {
        let mut resp = self.request(req, total)?.send().await?;
        let out = head_of(&resp);
        sink.begin(out.status, &out.final_url, &out.headers)?;
        // No read timeout per request in this client: a stall is bounded by `total` alone.
        while let Some(chunk) = resp.chunk().await? {
            sink.write(&chunk)?;
        }
        Ok(out)
    }
}

impl ReqwestFetcher {
    fn request(
        &self,
        req: HttpRequest,
        total: Option<std::time::Duration>,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
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
        if let Some(t) = total {
            r = r.timeout(t);
        }
        Ok(r)
    }
}

/// Status, address and headers of a response, with no body yet.
fn head_of(resp: &reqwest::Response) -> HttpResponse {
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
    crate::bodiless(
        resp.status().as_u16(),
        resp.url().to_string(),
        headers,
        http_version,
    )
}
