//! A stand-in for the three hosts behind a code-hosting URL, replaying answers captured from the
//! real API on 2026-11-11 (`tests/fixtures/github/*.body` and `*.headers`, verbatim except for the
//! caller's own address, which the rate-limit message quotes and is replaced by 203.0.113.7).
//! One is not captured: `secondary_limited` was never drawn from this machine, so it is built from
//! the shape the API documents for a secondary limit (429, `Retry-After`, quota headers that still
//! show requests left, the documented message).
//!
//! Two loopback ports play the web origin with the raw-file host, and the API, which the server is
//! pointed at through `SVIPALL_GITHUB_WEB`, `SVIPALL_GITHUB_API` and `SVIPALL_GITHUB_RAW`. It
//! lives on a thread of its own so that every `#[tokio::test]` (each its own runtime) shares it,
//! and it records every request with whether it carried an `Authorization` header.

#![allow(dead_code)] // Each test binary uses a different part of this.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One recorded request: the target as sent, and its `Authorization` value if any.
#[derive(Debug, Clone)]
pub struct Seen {
    pub target: String,
    pub authorization: Option<String>,
    /// Asked of the API origin, rather than the web origin or the raw host.
    pub api: bool,
}

pub struct Replay {
    pub port: u16,
    pub api_port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Replay {
    /// A URL on the web origin (which also plays the raw host).
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// A URL on the API origin, as a caller would name `api.github.com` directly.
    pub fn api_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.api_port)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// How many API requests went to targets starting with `prefix`.
    pub fn hits(&self, prefix: &str) -> usize {
        self.seen()
            .iter()
            .filter(|s| s.api && s.target.starts_with(prefix))
            .count()
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github")
}

pub fn fixture_text(name: &str) -> String {
    String::from_utf8(std::fs::read(fixtures().join(format!("{name}.body"))).unwrap()).unwrap()
}

/// Status, headers and body of one replayed answer.
type Answer = (u16, Vec<(String, String)>, Vec<u8>);

/// A captured answer: status line and headers from `<name>.headers`, body from `<name>.body`.
fn captured(name: &str) -> Answer {
    let head = std::fs::read_to_string(fixtures().join(format!("{name}.headers"))).unwrap();
    let body = std::fs::read(fixtures().join(format!("{name}.body"))).unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        // Framing is recomputed for the replayed body.
        .filter(|(k, _)| {
            ![
                "content-length",
                "transfer-encoding",
                "connection",
                "content-encoding",
            ]
            .contains(&k.to_ascii_lowercase().as_str())
        })
        .collect();
    (status, headers, body)
}

const R: &str = "/repos/dtolnay/anyhow";

/// What each request target answers. Anything else is a plain 404 page.
fn routes() -> HashMap<String, Answer> {
    let mut m = HashMap::new();
    let mut add = |target: String, name: &str| {
        m.insert(target, captured(name));
    };
    add(R.into(), "repo");
    add(format!("{R}/readme"), "readme");
    add(format!("{R}/contents"), "contents_root");
    add(format!("{R}/contents/src?ref=master"), "contents_src");
    add(format!("{R}/issues/323"), "issue_323");
    add(
        format!("{R}/issues/323/comments?per_page=100"),
        "issue_323_comments",
    );
    add(format!("{R}/issues/251"), "issue_251");
    add(format!("{R}/pulls/251"), "pull_251");
    add(
        format!("{R}/issues/251/comments?per_page=100"),
        "issue_251_comments",
    );
    add(
        format!("{R}/pulls/251/files?per_page=100"),
        "pull_251_files",
    );
    add(
        format!("{R}/pulls/251/comments?per_page=100"),
        "pull_251_review_comments",
    );
    add(
        format!("{R}/pulls/251/reviews?per_page=100"),
        "pull_251_reviews",
    );
    add(format!("{R}/releases/tags/1.0.104"), "release_tag");
    add(format!("{R}/releases?per_page=10"), "releases");
    add(
        format!("{R}/commits/87c8571326d60b90391d0555c9675b5ac6f044b1"),
        "commit",
    );
    // The raw-file host.
    add("/dtolnay/anyhow/master/src/lib.rs".into(), "raw_lib");
    // A repository whose API answers with the real rate-limit refusal, and whose page is HTML.
    add("/repos/limited/repo/issues/7".into(), "rate_limited");
    // And one that answers with a secondary limit: 429 with `Retry-After` while the hourly quota
    // still has requests left.
    add("/repos/secondary/repo/issues/9".into(), "secondary_limited");
    for page in [
        "/limited/repo/issues/7",
        "/plain/repo/issues/8",
        "/secondary/repo/issues/9",
    ] {
        m.insert(
            page.into(),
            (
                200,
                vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
                ISSUE_PAGE.as_bytes().to_vec(),
            ),
        );
    }
    m
}

/// The HTML page of an issue, served on the web origin when the API is not used.
pub const ISSUE_PAGE: &str = "<!doctype html><html><head><title>Issue seven</title></head>\
    <body><main><h1>Issue seven, read from its page</h1><p>The page of this issue is ordinary HTML. \
    It is served here so that a fetch which falls back from the API to the page has something \
    real to read, and the test can tell which of the two answered.</p></main></body></html>";

/// Answer one connection from the table, recording what was asked.
async fn serve(
    mut sock: tokio::net::TcpStream,
    t: Arc<HashMap<String, Answer>>,
    s: Arc<Mutex<Vec<Seen>>>,
    api: bool,
) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
    let authorization = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("authorization")
            .then(|| v.trim().to_string())
    });
    s.lock().unwrap().push(Seen {
        target: target.clone(),
        authorization,
        api,
    });
    let (status, headers, body) = t.get(&target).cloned().unwrap_or((
        404,
        vec![("Content-Type".into(), "text/html".into())],
        b"<html><body>not found</body></html>".to_vec(),
    ));
    let mut out = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in &headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(&body);
    let _ = sock.write_all(&out).await;
    let _ = sock.flush().await;
}

/// Start the replay server once per test binary and point the server's three origins at it: one
/// port for the web origin and the raw host, another for the API, as the real hosts differ.
pub fn replay() -> &'static Replay {
    static REPLAY: OnceLock<Replay> = OnceLock::new();
    REPLAY.get_or_init(|| {
        let table = Arc::new(routes());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let (t, s) = (table.clone(), seen.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let api = TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send((
                    web.local_addr().unwrap().port(),
                    api.local_addr().unwrap().port(),
                ))
                .unwrap();
                loop {
                    let (sock, is_api) = tokio::select! {
                        Ok((sock, _)) = web.accept() => (sock, false),
                        Ok((sock, _)) = api.accept() => (sock, true),
                    };
                    tokio::spawn(serve(sock, t.clone(), s.clone(), is_api));
                }
            });
        });
        let (port, api_port) = rx.recv().unwrap();
        std::env::set_var("SVIPALL_GITHUB_WEB", format!("http://127.0.0.1:{port}"));
        std::env::set_var("SVIPALL_GITHUB_RAW", format!("http://127.0.0.1:{port}"));
        std::env::set_var("SVIPALL_GITHUB_API", format!("http://127.0.0.1:{api_port}"));
        Replay {
            port,
            api_port,
            seen,
        }
    })
}
