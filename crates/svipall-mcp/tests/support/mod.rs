//! A local HTTP server that answers exactly what a test told it to, plus a `~/.svipall` of its own.
//!
//! The ladder was the largest untested part of the codebase, and the reason was always the same:
//! every interesting case ("this page is a Cloudflare interstitial", "this one is a login wall",
//! "this crawl was killed halfway") needs a site that behaves in a specific way on demand. Real
//! sites cannot be asked to do that, and mocking the fetcher would test everything except the
//! ladder. So the tests get a server instead — a few hundred microseconds per request, and
//! deterministic.
//!
//! Deliberately hand-written HTTP/1.1 rather than axum: the point is to be able to answer things a
//! framework would not let us, and the whole reader is thirty lines.

#![allow(dead_code)] // Each test binary uses a different part of this.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Point `svipall_core::config::home_dir()` at a directory of this test binary's own, so learned
/// tiers, cooldowns and profiles never touch the developer's real `~/.svipall`.
pub fn isolate() -> std::path::PathBuf {
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "svipall-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("SVIPALL_HOME", &dir);
        dir
    })
    .clone()
}

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn html(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// A page with a title, a real paragraph of text and the links it should hand the crawler.
    pub fn page(title: &str, links: &[&str]) -> Self {
        let body = links
            .iter()
            .map(|l| format!("<li><a href=\"{l}\">{l}</a></li>"))
            .collect::<Vec<_>>()
            .join("");
        Self::html(&format!(
            "<!doctype html><html><head><title>{title}</title></head><body><main>\
             <h1>{title}</h1><p>This is the body of {title}. It exists so the page has enough \
             text to survive extraction and classification, which both refuse to treat a page \
             that is only navigation as real content.</p><ul>{body}</ul></main></body></html>"
        ))
    }

    /// robots.txt, sitemaps and feeds are not HTML, and treating them as HTML is exactly how a
    /// line-oriented format loses its lines.
    pub fn plain(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/plain; charset=utf-8".into(),
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }

    /// The Cloudflare interstitial, as it actually arrives: a 403 whose body is a waiting room.
    pub fn cloudflare() -> Self {
        Self::html(
            "<!doctype html><html><head><title>Just a moment...</title></head><body>\
             <div id=\"cf-wrapper\">Checking your browser before accessing the site.</div>\
             </body></html>",
        )
        .with_status(403)
        .header("cf-mitigated", "challenge")
    }

    pub fn login_wall() -> Self {
        Self::html(
            "<!doctype html><html><head><title>Sign in</title></head><body>\
             <form><p>Please sign in to continue reading this article.</p>\
             <input type=\"password\" name=\"password\"></form></body></html>",
        )
        .with_status(401)
    }
}

pub struct Site {
    pub port: u16,
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Site {
    /// Start serving `routes` on a loopback port the OS picks. The task lives as long as the test.
    pub async fn start(routes: Vec<(&str, Reply)>) -> Self {
        let table: Arc<HashMap<String, Reply>> = Arc::new(
            routes
                .into_iter()
                .map(|(p, r)| (p.to_string(), r))
                .collect(),
        );
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let (t, h) = (table.clone(), hits.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (t, h) = (t.clone(), h.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 2048];
                    // Headers only: nothing here reads a request body.
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let path = target.split('?').next().unwrap_or("/").to_string();
                    *h.lock().unwrap().entry(path.clone()).or_insert(0) += 1;

                    let reply = t.get(&target).or_else(|| t.get(&path)).cloned();
                    let reply = reply.unwrap_or_else(|| {
                        Reply::html("<html><body>not found</body></html>").with_status(404)
                    });
                    let mut out = format!(
                        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                        reply.status,
                        reply.content_type,
                        reply.body.len()
                    );
                    for (k, v) in &reply.headers {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    out.push_str("\r\n");
                    out.push_str(&reply.body);
                    let _ = sock.write_all(out.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        Self { port, hits }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// How many times a path was actually requested. This is how "the cache served it" and "the
    /// resumed crawl did not fetch it again" are proved, rather than inferred.
    pub fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}
