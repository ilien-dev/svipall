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
    /// Always true outside this file's tests, whose server has a certificate made on the spot.
    verify_peer: bool,
}

/// What one exchange hands its caller as the response arrives.
enum Piece<'a> {
    /// The status and headers, once, before any of the body.
    Head {
        status: u16,
        headers: &'a [(String, String)],
    },
    /// The next piece of the body, as the server encoded it.
    Data(&'a [u8]),
}

type OnPiece<'a> = dyn FnMut(Piece<'_>) -> anyhow::Result<()> + Send + 'a;

impl H3Fetcher {
    pub fn new(cfg: &FetcherConfig, fallback: Arc<dyn HttpFetcher>) -> Self {
        Self {
            identity: cfg.identity.clone(),
            timeout: cfg.timeout,
            fallback,
            verify_peer: true,
        }
    }

    /// One request over one QUIC connection, collected. Errors here mean the *transport* did not
    /// work, never that the page was unhappy: a 404 or a 403 is a successful fetch.
    async fn once(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        let mut body: Vec<u8> = Vec::new();
        let (status, headers) = self
            .exchange(req, self.timeout, &mut |piece| {
                if let Piece::Data(d) = piece {
                    body.extend_from_slice(d);
                }
                Ok(())
            })
            .await?;
        let content_type = header_of(&headers, "content-type");
        let body = decode(&body, &header_of(&headers, "content-encoding"));
        Ok(HttpResponse {
            status,
            final_url: req.url.clone(),
            headers,
            content_type,
            body,
            http_version: "HTTP/3.0",
        })
    }

    /// One request over one QUIC connection, handing `on` the head and then each piece of the
    /// body as it arrives. An error from `on` stops the transfer and is returned as it is.
    async fn exchange(
        &self,
        req: &HttpRequest,
        timeout: Duration,
        on: &mut OnPiece<'_>,
    ) -> anyhow::Result<(u16, Vec<(String, String)>)> {
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
        if !self.verify_peer {
            cfg.verify_peer(false);
        }
        let mut scid = [0u8; 16];
        random(&mut scid);
        let scid = quiche::ConnectionId::from_ref(&scid);
        let mut conn = quiche::connect(Some(&host), &scid, local, peer, &mut cfg)?;

        let deadline = Instant::now() + timeout;
        let establish_by = Instant::now() + Duration::from_millis(H3_ESTABLISH_MS);
        let mut h3: Option<quiche::h3::Connection> = None;
        let mut stream: Option<u64> = None;
        let mut status = 0u16;
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut headed = false;
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
                            if !headed && status != 0 {
                                headed = true;
                                on(Piece::Head {
                                    status,
                                    headers: &headers,
                                })?;
                            }
                        }
                        Ok((id, quiche::h3::Event::Data)) if Some(id) == stream => {
                            while let Ok(n) = c.recv_body(&mut conn, id, &mut buf) {
                                on(Piece::Data(&buf[..n]))?;
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
                .ok_or_else(|| anyhow::anyhow!("h3 timed out after {timeout:?}"))?;
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
        Ok((status, headers))
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

/// A header's value, lowercased; empty when absent.
fn header_of(headers: &[(String, String)], name: &str) -> String {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default()
}

/// [`decode`] for a body that is not held whole: each piece goes in as it arrives, and what it
/// decodes to comes out at once. Unlike `decode`, a body that stops decoding is an error, because
/// the part already handed on cannot be taken back.
enum Unpack {
    Plain,
    Gzip(flate2::write::GzDecoder<Vec<u8>>),
    Deflate(flate2::write::ZlibDecoder<Vec<u8>>),
    Brotli(Box<brotli::DecompressorWriter<Vec<u8>>>),
    Zstd(zstd::stream::write::Decoder<'static, Vec<u8>>),
}

type Out<'a> = dyn FnMut(&[u8]) -> anyhow::Result<()> + 'a;

impl Unpack {
    fn new(encoding: &str) -> anyhow::Result<Self> {
        Ok(match encoding.trim() {
            "gzip" | "x-gzip" => Unpack::Gzip(flate2::write::GzDecoder::new(Vec::new())),
            "deflate" => Unpack::Deflate(flate2::write::ZlibDecoder::new(Vec::new())),
            "br" => Unpack::Brotli(Box::new(brotli::DecompressorWriter::new(
                Vec::new(),
                1 << 16,
            ))),
            "zstd" => Unpack::Zstd(zstd::stream::write::Decoder::new(Vec::new())?),
            _ => Unpack::Plain,
        })
    }

    fn plain(&self) -> bool {
        matches!(self, Unpack::Plain)
    }

    fn inner(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            Unpack::Plain => None,
            Unpack::Gzip(d) => Some(d.get_mut()),
            Unpack::Deflate(d) => Some(d.get_mut()),
            Unpack::Brotli(d) => Some(d.get_mut()),
            Unpack::Zstd(d) => Some(d.get_mut()),
        }
    }

    fn drain(&mut self, out: &mut Out<'_>) -> anyhow::Result<()> {
        if let Some(v) = self.inner() {
            if !v.is_empty() {
                let decoded = std::mem::take(v);
                out(&decoded)?;
            }
        }
        Ok(())
    }

    fn push(&mut self, piece: &[u8], out: &mut Out<'_>) -> anyhow::Result<()> {
        use std::io::Write;
        let written = match self {
            Unpack::Plain => return out(piece),
            Unpack::Gzip(d) => d.write_all(piece),
            Unpack::Deflate(d) => d.write_all(piece),
            Unpack::Brotli(d) => d.write_all(piece),
            Unpack::Zstd(d) => d.write_all(piece),
        };
        written.map_err(|e| anyhow::anyhow!("the h3 body did not decode as declared: {e}"))?;
        self.drain(out)
    }

    fn finish(&mut self, out: &mut Out<'_>) -> anyhow::Result<()> {
        use std::io::Write;
        let done = match self {
            Unpack::Plain => return Ok(()),
            Unpack::Gzip(d) => d.try_finish(),
            Unpack::Deflate(d) => d.try_finish(),
            Unpack::Brotli(d) => d.flush(),
            Unpack::Zstd(d) => d.flush(),
        };
        done.map_err(|e| anyhow::anyhow!("the h3 body did not decode as declared: {e}"))?;
        self.drain(out)
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

    /// The body to `sink` as it arrives, the way the TCP engines stream it, bounded by `total`
    /// rather than by the page timeout. A transport that fails before the response began falls
    /// back to the TCP engine's own streaming; once the sink has the headers, a failure is the
    /// caller's to see, because starting the response again would write it twice.
    async fn send_streaming(
        &self,
        req: HttpRequest,
        sink: &mut (dyn crate::BodySink + Send),
        total: Option<Duration>,
    ) -> anyhow::Result<HttpResponse> {
        let timeout = total.unwrap_or(self.timeout);
        let mut req = req;
        for _ in 0..MAX_REDIRECTS {
            let mut began: Option<Vec<(String, String)>> = None;
            let mut location: Option<String> = None;
            let mut unpack = Unpack::Plain;
            let url = req.url.clone();
            let sent = {
                let sink = &mut *sink;
                let (began, location, unpack) = (&mut began, &mut location, &mut unpack);
                self.exchange(&req, timeout, &mut |piece| match piece {
                    Piece::Head { status, headers } => {
                        if (300..400).contains(&status) {
                            if let Some((_, l)) = headers
                                .iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                            {
                                *location = Some(l.clone());
                                return Ok(());
                            }
                        }
                        *unpack = Unpack::new(&header_of(headers, "content-encoding"))?;
                        // Decoded here, so the encoding and the encoded length no longer describe
                        // the bytes the sink receives.
                        let shown: Vec<(String, String)> = headers
                            .iter()
                            .filter(|(k, _)| {
                                unpack.plain()
                                    || !(k.eq_ignore_ascii_case("content-encoding")
                                        || k.eq_ignore_ascii_case("content-length"))
                            })
                            .cloned()
                            .collect();
                        sink.begin(status, &url, &shown)?;
                        *began = Some(shown);
                        Ok(())
                    }
                    Piece::Data(d) if began.is_some() => unpack.push(d, &mut |b| sink.write(b)),
                    Piece::Data(_) => Ok(()),
                })
                .await
            };
            let status = match sent {
                Ok((status, _)) => status,
                Err(e) if began.is_none() && location.is_none() => {
                    tracing::debug!("h3 fell back to the tcp engine for {}: {e}", req.url);
                    return self.fallback.send_streaming(req, sink, total).await;
                }
                Err(e) => return Err(e),
            };
            if let Some(headers) = began {
                unpack.finish(&mut |b| sink.write(b))?;
                return Ok(crate::bodiless(status, req.url, headers, "HTTP/3.0"));
            }
            let Some(location) = location else {
                anyhow::bail!("the h3 response ended before its headers");
            };
            let here = url::Url::parse(&req.url)?;
            let next = here.join(&location)?;
            let same_origin = next.scheme() == here.scheme()
                && next.host_str() == here.host_str()
                && next.port_or_known_default() == here.port_or_known_default();
            req.url = next.to_string();
            if !same_origin || next.scheme() != "https" {
                return self.fallback.send_streaming(req, sink, total).await;
            }
        }
        anyhow::bail!("h3: more than {MAX_REDIRECTS} redirects")
    }
}

#[cfg(test)]
mod tests {
    //! The streaming path over a real QUIC connection: a server made in this process, on
    //! loopback, with a certificate generated for it and deleted with it.
    use super::*;
    use std::sync::Mutex;

    const LEN: usize = 1_500_000;

    fn body() -> Vec<u8> {
        (0..LEN as u64)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
            .collect()
    }

    /// What the server answers: the body, optionally gzip-encoded, optionally cut short.
    #[derive(Clone, Copy)]
    enum Serve {
        Whole,
        Gzip,
        /// Half of the body under a full `content-length`, then the connection is closed.
        Cut,
    }

    /// A one-connection HTTP/3 server on a thread of its own. Returns its port.
    fn server(serve: Serve) -> u16 {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let cert = quiche::selfsigned::generate("h3.test").expect("a certificate");
            let mut cfg = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
            cfg.load_cert_chain_from_pem_file(cert.cert_pem.to_str().unwrap())
                .unwrap();
            cfg.load_priv_key_from_pem_file(cert.key_pem.to_str().unwrap())
                .unwrap();
            cfg.set_application_protos(&[b"h3"]).unwrap();
            cfg.set_max_idle_timeout(10_000);
            cfg.set_max_recv_udp_payload_size(1500);
            cfg.set_initial_max_data(10_000_000);
            cfg.set_initial_max_stream_data_bidi_local(1_000_000);
            cfg.set_initial_max_stream_data_bidi_remote(1_000_000);
            cfg.set_initial_max_stream_data_uni(1_000_000);
            cfg.set_initial_max_streams_bidi(100);
            cfg.set_initial_max_streams_uni(100);
            let local = socket.local_addr().unwrap();
            let mut buf = [0u8; 65535];
            let mut out = [0u8; MAX_DATAGRAM];
            let (n, peer) = socket.recv_from(&mut buf).unwrap();
            let hdr = quiche::Header::from_slice(&mut buf[..n], quiche::MAX_CONN_ID_LEN).unwrap();
            let mut scid = [0u8; quiche::MAX_CONN_ID_LEN];
            random(&mut scid);
            let scid = quiche::ConnectionId::from_ref(&scid);
            let mut conn = quiche::accept(&scid, Some(&hdr.dcid), local, peer, &mut cfg).unwrap();
            conn.recv(
                &mut buf[..n],
                quiche::RecvInfo {
                    from: peer,
                    to: local,
                },
            )
            .unwrap();
            let payload = match serve {
                Serve::Gzip => {
                    use std::io::Write;
                    let mut e =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                    e.write_all(&body()).unwrap();
                    e.finish().unwrap()
                }
                Serve::Whole | Serve::Cut => body(),
            };
            let send_until = match serve {
                Serve::Cut => payload.len() / 2,
                _ => payload.len(),
            };
            let mut h3: Option<quiche::h3::Connection> = None;
            let mut stream: Option<u64> = None;
            let mut sent = 0usize;
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline && !conn.is_closed() {
                if conn.is_established() && h3.is_none() {
                    h3 = Some(
                        quiche::h3::Connection::with_transport(
                            &mut conn,
                            &quiche::h3::Config::new().unwrap(),
                        )
                        .unwrap(),
                    );
                }
                if let Some(h) = h3.as_mut() {
                    while let Ok((id, ev)) = h.poll(&mut conn) {
                        if let quiche::h3::Event::Headers { .. } = ev {
                            let mut head = vec![
                                quiche::h3::Header::new(b":status", b"200"),
                                quiche::h3::Header::new(
                                    b"content-type",
                                    b"application/octet-stream",
                                ),
                                quiche::h3::Header::new(
                                    b"content-length",
                                    payload.len().to_string().as_bytes(),
                                ),
                            ];
                            if let Serve::Gzip = serve {
                                head.push(quiche::h3::Header::new(b"content-encoding", b"gzip"));
                            }
                            h.send_response(&mut conn, id, &head, false).unwrap();
                            stream = Some(id);
                        }
                    }
                    if let Some(id) = stream {
                        while sent < send_until {
                            let end = (sent + 16_384).min(send_until);
                            let fin = end == payload.len();
                            match h.send_body(&mut conn, id, &payload[sent..end], fin) {
                                Ok(n) => sent += n,
                                Err(_) => break,
                            }
                        }
                        if sent >= send_until && send_until < payload.len() {
                            let _ = conn.close(true, 0x100, b"cut");
                        }
                    }
                }
                while let Ok((n, info)) = conn.send(&mut out) {
                    let _ = socket.send_to(&out[..n], info.to);
                }
                let wait = conn
                    .timeout()
                    .unwrap_or(Duration::from_millis(50))
                    .clamp(Duration::from_millis(1), Duration::from_millis(50));
                socket.set_read_timeout(Some(wait)).unwrap();
                match socket.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        let _ = conn.recv(&mut buf[..n], quiche::RecvInfo { from, to: local });
                    }
                    Err(_) => conn.on_timeout(),
                }
            }
            drop(cert);
        });
        port
    }

    #[derive(Default)]
    struct Seen {
        begun: usize,
        headers: Vec<(String, String)>,
        writes: usize,
        bytes: Vec<u8>,
    }

    struct Recorder(Arc<Mutex<Seen>>);

    impl crate::BodySink for Recorder {
        fn begin(
            &mut self,
            _status: u16,
            _final_url: &str,
            headers: &[(String, String)],
        ) -> anyhow::Result<()> {
            let mut s = self.0.lock().unwrap();
            s.begun += 1;
            s.headers = headers.to_vec();
            Ok(())
        }
        fn write(&mut self, chunk: &[u8]) -> anyhow::Result<()> {
            let mut s = self.0.lock().unwrap();
            s.writes += 1;
            s.bytes.extend_from_slice(chunk);
            Ok(())
        }
    }

    /// A fallback that refuses, so an answer can only have come over QUIC.
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
            anyhow::bail!("the fallback was used")
        }
        async fn send_streaming(
            &self,
            _: HttpRequest,
            _: &mut (dyn crate::BodySink + Send),
            _: Option<Duration>,
        ) -> anyhow::Result<HttpResponse> {
            anyhow::bail!("the fallback was used")
        }
    }

    fn fetcher() -> H3Fetcher {
        let identity = IdentityProfile::for_major(
            svipall_core::MAX_EMULATED_CHROME,
            svipall_core::Os::Windows,
        );
        let mut cfg = FetcherConfig::new(identity.clone());
        cfg.timeout = Duration::from_secs(20);
        let mut f = H3Fetcher::new(&cfg, Arc::new(Never(identity)));
        f.verify_peer = false;
        f
    }

    async fn stream(serve: Serve) -> (anyhow::Result<HttpResponse>, Seen) {
        let port = server(serve);
        let seen = Arc::new(Mutex::new(Seen::default()));
        let mut sink = Recorder(seen.clone());
        let r = fetcher()
            .send_streaming(
                HttpRequest::get(format!("https://127.0.0.1:{port}/model.bin")),
                &mut sink,
                Some(Duration::from_secs(20)),
            )
            .await;
        let seen = std::mem::take(&mut *seen.lock().unwrap());
        (r, seen)
    }

    #[tokio::test]
    async fn a_body_over_quic_reaches_the_sink_as_it_arrives() {
        let (r, seen) = stream(Serve::Whole).await;
        let r = r.expect("a response over h3");
        assert_eq!(r.http_version, "HTTP/3.0");
        assert!(r.body.is_empty(), "the bytes went to the sink");
        assert_eq!(seen.begun, 1);
        assert_eq!(seen.bytes.len(), LEN);
        assert!(seen.bytes == body(), "every byte, in order");
        assert!(seen.writes > 1, "one write is a collected body");
    }

    #[tokio::test]
    async fn an_encoded_body_over_quic_is_decoded_on_the_way_and_described_as_decoded() {
        let (r, seen) = stream(Serve::Gzip).await;
        r.expect("a response over h3");
        assert!(seen.bytes == body(), "decoded, every byte");
        assert!(seen.writes > 1, "decoded as it arrived");
        // The sink checks what arrived against `content-length`; the encoded length is not it.
        for name in ["content-encoding", "content-length"] {
            assert!(
                !seen.headers.iter().any(|(k, _)| k == name),
                "{name} describes the encoded body: {:?}",
                seen.headers
            );
        }
    }

    #[tokio::test]
    async fn a_transfer_cut_over_quic_is_an_error_after_the_bytes_that_arrived() {
        let (r, seen) = stream(Serve::Cut).await;
        assert!(r.is_err(), "a cut body is the caller's to see");
        assert_eq!(
            seen.begun, 1,
            "the response began once and was not restarted"
        );
        assert!(!seen.bytes.is_empty() && seen.bytes.len() < LEN);
        assert!(seen.bytes[..] == body()[..seen.bytes.len()]);
    }
}
