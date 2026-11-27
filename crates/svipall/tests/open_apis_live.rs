//! Open APIs, registries and file hosts, against the real ones. Network, so run by hand:
//!
//! ```text
//! cargo test -p svipall --test open_apis_live -- --ignored --nocapture
//! ```
//!
//! Thirty reads of each host that names no limit of its own, started together the way a research session starts them, with the
//! default configuration and nobody waiting longer than a fetch waits by default. Every one of
//! them is a read the host itself allows; the claim is that none is refused by this tool.
mod support;

use serde_json::{json, Value};
use svipall::server::SvipallServer;

const WORDS: [&str; 30] = [
    "allocator",
    "borrow",
    "closure",
    "deadlock",
    "enum",
    "future",
    "generic",
    "hash",
    "iterator",
    "json",
    "kernel",
    "lifetime",
    "macro",
    "network",
    "option",
    "parser",
    "queue",
    "runtime",
    "socket",
    "thread",
    "unicode",
    "vector",
    "wasm",
    "xml",
    "yield",
    "zero",
    "atomic",
    "buffer",
    "cache",
    "driver",
];
const PACKAGES: [&str; 30] = [
    "react",
    "vue",
    "lodash",
    "express",
    "axios",
    "chalk",
    "commander",
    "debug",
    "dotenv",
    "glob",
    "minimist",
    "rimraf",
    "semver",
    "uuid",
    "yargs",
    "zod",
    "vite",
    "rollup",
    "esbuild",
    "eslint",
    "prettier",
    "typescript",
    "jest",
    "mocha",
    "ws",
    "pg",
    "redis",
    "nanoid",
    "dayjs",
    "marked",
];

async fn burst(server: &SvipallServer, name: &str, urls: Vec<String>) -> (usize, Vec<String>) {
    burst_within(server, name, urls, 60_000).await
}

async fn burst_within(
    server: &SvipallServer,
    name: &str,
    urls: Vec<String>,
    timeout_ms: u64,
) -> (usize, Vec<String>) {
    let started = std::time::Instant::now();
    let reads = urls.iter().map(|u| async move {
        server
            .fetch_json(
                serde_json::from_value(json!({"url": u, "max_tokens": 300, "cache": "bypass",
                    "timeout": timeout_ms}))
                .unwrap(),
            )
            .await
            .value
    });
    let answers: Vec<Value> = futures::future::join_all(reads).await;
    let mut refused = Vec::new();
    let mut ok = 0;
    for (u, v) in urls.iter().zip(&answers) {
        if v["status"] == 200 && v.get("blocked_reason").is_none() && v["tier_used"] == "http" {
            ok += 1;
        } else {
            refused.push(format!(
                "{u}: status {} tier {} reason {}",
                v["status"], v["tier_used"], v["blocked_reason"]
            ));
        }
    }
    println!(
        "{name}: {ok}/{} read at the first tier in {:.1}s",
        urls.len(),
        started.elapsed().as_secs_f32()
    );
    (ok, refused)
}

#[tokio::test]
#[ignore = "network: reads real public APIs"]
async fn thirty_reads_at_once_of_an_open_api_are_all_answered() {
    support::isolate();
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);
    let hosts: Vec<(&str, Vec<String>)> = vec![
        (
            "registry.npmjs.org",
            PACKAGES
                .iter()
                .map(|p| format!("https://registry.npmjs.org/{p}/latest"))
                .collect(),
        ),
        (
            "cdn.jsdelivr.net",
            PACKAGES
                .iter()
                .map(|p| format!("https://cdn.jsdelivr.net/npm/{p}/package.json"))
                .collect(),
        ),
        (
            "raw.githubusercontent.com",
            WORDS
                .iter()
                .map(|w| {
                    format!("https://raw.githubusercontent.com/rust-lang/rust/master/README.md?{w}")
                })
                .collect(),
        ),
        (
            "suggestqueries.google.com",
            WORDS
                .iter()
                .map(|w| {
                    format!(
                        "https://suggestqueries.google.com/complete/search?client=firefox&q={w}"
                    )
                })
                .collect(),
        ),
    ];
    let results = futures::future::join_all(hosts.into_iter().map(|(name, urls)| {
        let server = &server;
        async move { (name, burst(server, name, urls).await) }
    }))
    .await;
    let mut failed = Vec::new();
    for (name, (ok, refused)) in results {
        if ok < 30 {
            failed.push(format!("{name}: {ok}/30\n  {}", refused.join("\n  ")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// A host that names a pace slower than the data lane's: one request a second, one at a time.
/// Thirty reads take the half minute and more the host asks for, and all of them arrive.
#[tokio::test]
#[ignore = "network: reads a real public API"]
async fn a_host_that_names_its_pace_answers_every_read_at_that_pace() {
    support::isolate();
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);
    let urls: Vec<String> = WORDS
        .iter()
        .map(|w| format!("https://api.crossref.org/works?query={w}&rows=1"))
        .collect();
    let (ok, refused) = burst_within(&server, "api.crossref.org", urls, 180_000).await;
    assert_eq!(
        ok,
        30,
        "{}",
        refused.join(
            "
"
        )
    );
    let pace = svipall_core::lane::declared("https://api.crossref.org/works", None);
    println!("declared: {pace:?}");
    assert!(pace.is_some_and(|p| p.gap_ms >= 20), "{pace:?}");
}
