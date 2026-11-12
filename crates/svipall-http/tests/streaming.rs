//! A streamed body reaches the sink as it arrives, on every engine this build has.
//!
//! The server's download path (`svipall::body::Sink`) depends on one property of the engine: the
//! sink sees the headers, then the body piece by piece, and a transfer cut short leaves in the sink
//! what had arrived. An engine that collected the body first and handed it over at the end would
//! pass every test that only looks at a finished file, and lose a cut download whole. These run in
//! any build: `Engine::Reqwest` is the fallback client, always compiled; `Engine::Auto` is the
//! emulating client with `impersonate` (the default) and the fallback without it.

use std::sync::{Arc, Mutex};
use svipall_core::{IdentityProfile, Os};
use svipall_http::{BodySink, Engine, FetcherConfig, HttpRequest};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LEN: usize = 600_000;

fn body() -> Vec<u8> {
    (0..LEN as u64)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect()
}

/// `/whole` sends every byte; `/cut` sends half under the full `Content-Length` and hangs up.
async fn serve() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 2048];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let cut = String::from_utf8_lossy(&head).contains("GET /cut");
                let b = body();
                let send = if cut { LEN / 2 } else { LEN };
                let mut out = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                     Content-Length: {LEN}\r\nConnection: close\r\n\r\n"
                )
                .into_bytes();
                out.extend_from_slice(&b[..16_384]);
                let _ = sock.write_all(&out).await;
                for piece in b[16_384..send].chunks(16_384) {
                    let _ = sock.write_all(piece).await;
                    let _ = sock.flush().await;
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                }
            });
        }
    });
    port
}

#[derive(Default)]
struct Seen {
    status: u16,
    began: bool,
    writes: usize,
    bytes: Vec<u8>,
}

struct Recorder(Arc<Mutex<Seen>>);

impl BodySink for Recorder {
    fn begin(
        &mut self,
        status: u16,
        _final_url: &str,
        _headers: &[(String, String)],
    ) -> anyhow::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.status = status;
        s.began = true;
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> anyhow::Result<()> {
        let mut s = self.0.lock().unwrap();
        assert!(s.began, "a byte of the body arrived before the headers");
        s.writes += 1;
        s.bytes.extend_from_slice(chunk);
        Ok(())
    }
}

fn fetcher(engine: Engine) -> Arc<dyn svipall_http::HttpFetcher> {
    let mut cfg = FetcherConfig::new(IdentityProfile::for_major(
        svipall_core::MAX_EMULATED_CHROME,
        Os::Windows,
    ));
    cfg.engine = engine;
    svipall_http::build(cfg).unwrap()
}

async fn streamed(engine: Engine, path: &str) -> (anyhow::Result<usize>, Seen) {
    let port = serve().await;
    let seen = Arc::new(Mutex::new(Seen::default()));
    let mut sink = Recorder(seen.clone());
    let r = fetcher(engine)
        .send_streaming(
            HttpRequest::get(format!("http://127.0.0.1:{port}{path}")),
            &mut sink,
            Some(std::time::Duration::from_secs(20)),
        )
        .await
        .map(|r| r.body.len());
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    (r, seen)
}

async fn whole_body_arrives_in_pieces(engine: Engine) {
    let (r, seen) = streamed(engine, "/whole").await;
    assert_eq!(r.expect("the transfer"), 0, "the bytes went to the sink");
    assert_eq!(seen.status, 200);
    assert_eq!(seen.bytes, body(), "every byte, in order");
    assert!(
        seen.writes > 1,
        "one write is a collected body: {}",
        seen.writes
    );
}

async fn a_cut_transfer_leaves_what_arrived(engine: Engine) {
    let (r, seen) = streamed(engine, "/cut").await;
    assert!(r.is_err(), "a short body is an error");
    assert!(seen.began, "the headers reached the sink");
    assert!(
        !seen.bytes.is_empty() && seen.bytes.len() <= LEN / 2,
        "{} bytes reached the sink",
        seen.bytes.len()
    );
    assert_eq!(seen.bytes[..], body()[..seen.bytes.len()]);
}

#[tokio::test]
async fn the_fallback_client_streams_into_the_sink() {
    whole_body_arrives_in_pieces(Engine::Reqwest).await;
}

#[tokio::test]
async fn the_fallback_client_leaves_a_cut_transfer_in_the_sink() {
    a_cut_transfer_leaves_what_arrived(Engine::Reqwest).await;
}

#[tokio::test]
async fn the_default_client_streams_into_the_sink() {
    whole_body_arrives_in_pieces(Engine::Auto).await;
}

#[tokio::test]
async fn the_default_client_leaves_a_cut_transfer_in_the_sink() {
    a_cut_transfer_leaves_what_arrived(Engine::Auto).await;
}
