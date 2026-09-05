//! HTTP/3 over QUIC, on the same BoringSSL and the same identity as the TCP engine.
//!
//! Chrome never opens a first connection over QUIC. It learns from `Alt-Svc` that a site offers
//! h3 and uses it on a later visit, which is why nothing here decides *whether* to speak h3 — the
//! caller does, from what `core::altsvc` remembered — and why this fetcher is built with the TCP
//! one behind it. A site that advertised h3 and then does not answer over UDP is an ordinary thing
//! (a network that drops 443/udp, a stale advertisement, a load balancer that moved), and the
//! answer to it is the page, fetched the other way, not an error.
//!
//! What this deliberately does not do: carry a cookie jar. The TCP engine keeps one inside its
//! client; this one treats the caller's headers as authoritative and hands `set-cookie` straight
//! back. A tier that needs a session across requests uses a browser, which is where sessions live.

use crate::{FetcherConfig, HttpFetcher, HttpRequest, HttpResponse};
use quiche::h3::NameValue;
use std::sync::Arc;
use std::time::{Duration, Instant};
use svipall_core::IdentityProfile;
use tokio::net::UdpSocket;

/// The largest datagram we will read. QUIC requires a path MTU of at least 1200 and Chrome
/// declares 1472; anything larger than this buffer is a packet we were never told to expect.
const MAX_DATAGRAM: usize = 1500;

/// How many redirects to follow, matching the TCP engine so the two agree on what a fetch is.
const MAX_REDIRECTS: usize = 10;

/// How long the QUIC handshake gets, separately from the page.
///
/// This is the number that decides whether HTTP/3 can make the tool *slower*. A network that
/// refuses UDP says so at once; a network that silently **drops** it says nothing at all, and
/// without a deadline of its own the attempt would sit there for the whole navigation budget
/// before falling back to TCP — on every h3-advertising domain, on a first visit, for a caller
/// who only wanted one page. A handshake to a CDN that is answering takes tens of milliseconds,
/// so two seconds is generous and still bounds the bad case to something a person will not
/// notice twice. Once the connection is up the page gets the full budget: a slow large page is
/// not the same failure and must not be punished for it.
const H3_ESTABLISH_MS: u64 = 2_000;

pub struct H3Fetcher {
    identity: IdentityProfile,
    timeout: Duration,
    /// Where a request goes when h3 does not answer. Never optional: an h3 fetcher with nothing
    /// behind it would turn a dropped UDP port into a failed page.
    fallback: Arc<dyn HttpFetcher>,
}

impl H3Fetcher {
    pub fn new(cfg: &FetcherConfig, fallback: Arc<dyn HttpFetcher>) -> Self {
        Self {
            identity: cfg.identity.clone(),
            timeout: cfg.timeout,
            fallback,
        }
    }

    /// One request over one QUIC connection. Errors here mean the *transport* did not work, never
    /// that the page was unhappy: a 404 or a 403 is a successful fetch.
    async fn once(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        let url = url::Url::parse(&req.url)?;
        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("the url has no host"))?
            .to_string();
        let port = url.port_or_known_default().unwrap_or(443);

        let peer = tokio::net::lookup_host((host.as_str(), port))
            .await?
            .next()
            .ok_or_else(|| anyhow::anyhow!("{host} did not resolve"))?;
        let socket = UdpSocket::bind(if peer.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .await?;
        socket.connect(peer).await?;
        let local = socket.local_addr()?;

        let mut cfg = quiche::Config::chrome(&[b"h3"])?;
        let mut scid = [0u8; 16];
        random(&mut scid);
        let scid = quiche::ConnectionId::from_ref(&scid);
        let mut conn = quiche::connect(Some(&host), &scid, local, peer, &mut cfg)?;

        let deadline = Instant::now() + self.timeout;
        let establish_by = Instant::now() + Duration::from_millis(H3_ESTABLISH_MS);
        let mut h3: Option<quiche::h3::Connection> = None;
        let mut stream: Option<u64> = None;
        let mut status = 0u16;
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        let mut done = false;

        let mut out = [0u8; MAX_DATAGRAM];
        let mut buf = [0u8; 65535];

        loop {
            // Everything the connection wants to say, before waiting to hear anything.
            loop {
                match conn.send(&mut out) {
                    Ok((n, _info)) => socket.send(&out[..n]).await.map(|_| ()).unwrap_or_default(),
                    Err(quiche::Error::Done) => break,
                    Err(e) => return Err(e.into()),
                }
            }

            if conn.is_closed() {
                anyhow::bail!(
                    "the QUIC connection closed before the response was complete: {:?}",
                    conn.peer_error().map(|e| e.reason.clone())
                );
            }
            if done {
                break;
            }

            // The handshake is finished: open HTTP/3 and ask for the page.
            //
            // `Config::chrome`, never `Config::new`. The SETTINGS frame goes out on the control
            // stream the moment this is built, and upstream's defaults send two settings — one of
            // them a draft codepoint Chrome does not use — where Chrome sends four and a GREASE.
            // Measured by `bench h3-ref`, asserted by `svipall-quic`'s `tests/settings.rs`.
            if h3.is_none() && conn.is_established() {
                let mut c = quiche::h3::Connection::with_transport(
                    &mut conn,
                    &quiche::h3::Config::chrome()?,
                )?;
                stream = Some(c.send_request(&mut conn, &self.request_headers(&url, req), true)?);
                h3 = Some(c);
            }

            if let Some(c) = h3.as_mut() {
                loop {
                    match c.poll(&mut conn) {
                        Ok((id, quiche::h3::Event::Headers { list, .. })) if Some(id) == stream => {
                            for h in list {
                                let name = String::from_utf8_lossy(h.name()).into_owned();
                                let value = String::from_utf8_lossy(h.value()).into_owned();
                                if name == ":status" {
                                    status = value.parse().unwrap_or(0);
                                } else {
                                    headers.push((name, value));
                                }
                            }
                        }
                        Ok((id, quiche::h3::Event::Data)) if Some(id) == stream => {
                            while let Ok(n) = c.recv_body(&mut conn, id, &mut buf) {
                                body.extend_from_slice(&buf[..n]);
                            }
                        }
                        Ok((id, quiche::h3::Event::Finished)) if Some(id) == stream => {
                            done = true;
                        }
                        Ok(_) => {}
                        Err(quiche::h3::Error::Done) => break,
                        Err(e) => return Err(e.into()),
                    }
                }
                if done {
                    // Say goodbye properly rather than vanishing: an abandoned connection is a
                    // minute of state held on someone else's server.
                    let _ = conn.close(true, 0x100, b"done");
                    continue;
                }
            }

            if h3.is_none() && Instant::now() > establish_by {
                anyhow::bail!(
                    "QUIC did not answer within {H3_ESTABLISH_MS}ms; this network probably drops UDP"
                );
            }
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| anyhow::anyhow!("h3 timed out after {:?}", self.timeout))?;
            let wait = conn.timeout().map_or(left, |t| t.min(left));
            match tokio::time::timeout(wait, socket.recv(&mut buf)).await {
                Ok(Ok(n)) => {
                    let from = peer;
                    let info = quiche::RecvInfo { to: local, from };
                    if let Err(e) = conn.recv(&mut buf[..n], info) {
                        // A single malformed datagram is not the end of a connection.
                        tracing::debug!("h3: dropped a datagram: {e}");
                    }
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => conn.on_timeout(),
            }
        }

        if status == 0 {
            anyhow::bail!("the h3 response carried no :status");
        }
        let header = |name: &str| -> String {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.to_ascii_lowercase())
                .unwrap_or_default()
        };
        let content_type = header("content-type");
        let body = decode(&body, &header("content-encoding"));
        Ok(HttpResponse {
            status,
            final_url: req.url.clone(),
            headers,
            content_type,
            body,
            http_version: "HTTP/3.0",
        })
    }

    /// The pseudo-headers, then the identity's navigation block, in Chrome's order. The caller's
    /// own headers are applied on top, as they are on the TCP path, so an API call can still set
    /// its own content type without disturbing the navigation fingerprint.
    fn request_headers(&self, url: &url::Url, req: &HttpRequest) -> Vec<quiche::h3::Header> {
        let authority = match url.port() {
            Some(p) => format!("{}:{p}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_string(),
        };
        let path = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_string(),
        };
        let mut out = vec![
            quiche::h3::Header::new(b":method", req.method.to_uppercase().as_bytes()),
            quiche::h3::Header::new(b":scheme", url.scheme().as_bytes()),
            quiche::h3::Header::new(b":authority", authority.as_bytes()),
            quiche::h3::Header::new(b":path", path.as_bytes()),
        ];
        let mut named: Vec<(String, String)> = self.identity.nav_headers();
        for (k, v) in &req.headers {
            let k = k.to_ascii_lowercase();
            // HTTP/3 has no `host`; `:authority` is that header, and sending both is a
            // contradiction a server is entitled to reject.
            if k == "host" || k == "connection" || k.starts_with(':') {
                continue;
            }
            match named.iter_mut().find(|(n, _)| *n == k) {
                Some(slot) => slot.1 = v.clone(),
                None => named.push((k, v.clone())),
            }
        }
        for (k, v) in &named {
            out.push(quiche::h3::Header::new(k.as_bytes(), v.as_bytes()));
        }
        out
    }
}

/// Undo whatever `content-encoding` the server used.
///
/// The identity advertises `gzip, deflate, br, zstd`, so a server may well use one — and it must,
/// because trimming that header to what is convenient here would be a difference from Chrome in
/// the one place this project refuses to have them. The TCP engine has its client decode for it;
/// over h3 it is ours to do, and a body handed on still compressed reaches the extractor as bytes
/// it cannot read.
///
/// A body that does not decode is returned as it came. Guessing is worse than passing through:
/// `quality` labels a page, it never invents one.
fn decode(body: &[u8], encoding: &str) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    let ok = match encoding.trim().to_ascii_lowercase().as_str() {
        "gzip" | "x-gzip" => flate2::read::GzDecoder::new(body)
            .read_to_end(&mut out)
            .is_ok(),
        "deflate" => flate2::read::ZlibDecoder::new(body)
            .read_to_end(&mut out)
            .is_ok(),
        "br" => brotli::BrotliDecompress(&mut std::io::Cursor::new(body), &mut out).is_ok(),
        "zstd" => zstd::stream::copy_decode(body, &mut out).is_ok(),
        // No encoding, or one nobody here knows. Either way the bytes are what they are.
        _ => return body.to_vec(),
    };
    if ok {
        out
    } else {
        tracing::debug!("h3: a body declared `{encoding}` and did not decode as it");
        body.to_vec()
    }
}

fn random(buf: &mut [u8]) {
    let mut x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    for b in buf.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
}

#[async_trait::async_trait]
impl HttpFetcher for H3Fetcher {
    fn engine(&self) -> &'static str {
        "quiche"
    }

    fn identity(&self) -> &IdentityProfile {
        &self.identity
    }

    async fn send(&self, req: HttpRequest) -> anyhow::Result<HttpResponse> {
        let mut req = req;
        for _ in 0..MAX_REDIRECTS {
            let resp = match self.once(&req).await {
                Ok(r) => r,
                // Not an error the caller should see: the site advertised h3 and did not answer
                // over it. The page is what was asked for, and TCP still has it.
                Err(e) => {
                    tracing::debug!("h3 fell back to the tcp engine for {}: {e}", req.url);
                    return self.fallback.send(req).await;
                }
            };
            if !(300..400).contains(&resp.status) {
                return Ok(resp);
            }
            let Some(location) = resp.header("location") else {
                return Ok(resp);
            };
            let here = url::Url::parse(&req.url)?;
            let next = here.join(location)?;
            let same_origin = next.scheme() == here.scheme()
                && next.host_str() == here.host_str()
                && next.port_or_known_default() == here.port_or_known_default();
            req.url = next.to_string();
            // A redirect off this origin lands on a different server, and nothing has said that
            // one speaks h3. Chrome would have to learn it from its own `Alt-Svc` first, and
            // trying anyway would spend the whole navigation budget on a UDP port that may simply
            // drop us before falling back.
            if !same_origin || next.scheme() != "https" {
                return self.fallback.send(req).await;
            }
        }
        anyhow::bail!("h3: more than {MAX_REDIRECTS} redirects")
    }
}
