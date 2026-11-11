//! What a caller sends is read for what it means, and what a tool does not have is said out loud.
//!
//! Both transports, end to end: MCP through `call_tool` over an in-memory pipe (the same JSON-RPC
//! a client writes on stdio), and REST through the router. A number spelled as a string is a
//! number; a parameter the tool does not have refuses the call and names itself, because a caller
//! that sent `max_token: 50` and got 25 000 tokens back believed a limit was applied.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use std::sync::Arc;
use support::{Reply, Site};
use svipall::server::SvipallServer;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt as _;

const KEY: &str = "a-key-that-is-long-enough";

fn server() -> SvipallServer {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = support::isolate().join(format!("params-{n}.db"));
    let store = svipall_core::cache::Store::open_at(&path).expect("open db");
    let cfg = svipall_core::Config {
        max_tier: "http".into(),
        request_min_interval_ms: 1,
        update_check: false,
        ..Default::default()
    };
    SvipallServer::with_store(None, cfg, None, Some(Arc::new(store)))
}

fn long_page() -> Reply {
    let para = "<p>The harbour council met on Tuesday and voted on the redevelopment plan after a \
                debate that ran well past midnight, with members split on the costings.</p>"
        .repeat(60);
    Reply::html(&format!(
        "<!doctype html><html><head><title>Harbour</title></head><body><main>{para}</main></body></html>"
    ))
}

// ---- MCP --------------------------------------------------------------------------------------

/// A client on the other end of a pipe: initialise, then one `tools/call`, and its answer.
async fn mcp_call(tool: &str, arguments: Value) -> Value {
    let (client, served) = tokio::io::duplex(1 << 22);
    let (sr, sw) = tokio::io::split(served);
    let s = server();
    tokio::spawn(async move {
        if let Ok(running) = s.serve((sr, sw)).await {
            let _ = running.waiting().await;
        }
    });
    let (cr, mut cw) = tokio::io::split(client);
    let mut lines = BufReader::new(cr).lines();
    let messages = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": tool, "arguments": arguments}}),
    ];
    for m in messages {
        cw.write_all(format!("{m}\n").as_bytes()).await.unwrap();
    }
    loop {
        let line = tokio::time::timeout(std::time::Duration::from_secs(60), lines.next_line())
            .await
            .expect("an answer within a minute")
            .unwrap()
            .expect("the server hung up");
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == 2 {
            return v;
        }
    }
}

/// The JSON a tool returned, from its first text block.
fn tool_json(answer: &Value) -> Value {
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {answer}"));
    serde_json::from_str(text).unwrap_or_else(|_| panic!("not json: {text}"))
}

#[tokio::test]
async fn mcp_reads_a_number_sent_as_a_string_as_the_number() {
    let site = Site::start(vec![("/long", long_page())]).await;
    let answer = mcp_call(
        "web_fetch",
        json!({"url": site.url("/long"), "max_tokens": "60", "robots": "ignore",
               "cache": "bypass", "include_links": "false"}),
    )
    .await;
    assert!(answer["error"].is_null(), "refused: {answer}");
    let out = tool_json(&answer);
    assert_eq!(out["truncated"], true, "the limit was not applied: {out}");
    assert!(out["tokens_estimated"].as_u64().unwrap() <= 60, "{out}");
}

#[tokio::test]
async fn mcp_refuses_a_parameter_the_tool_does_not_have_and_names_it() {
    let site = Site::start(vec![("/long", long_page())]).await;
    let answer = mcp_call(
        "web_fetch",
        json!({"url": site.url("/long"), "max_token": 60, "robots": "ignore"}),
    )
    .await;
    let message = answer["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("the call ran with the limit ignored: {answer}"));
    assert!(message.contains("`max_token`"), "{message}");
    assert!(message.contains("did you mean `max_tokens`"), "{message}");
    assert_eq!(site.hits("/long"), 0, "nothing may be fetched: {message}");
}

// ---- REST -------------------------------------------------------------------------------------

async fn rest(path: &str, body: Value) -> (StatusCode, Value) {
    let s = server();
    let app = svipall::rest::router(
        s.clone(),
        svipall::jobs::JobRunner::new(s, 1),
        KEY,
        "127.0.0.1",
    );
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "127.0.0.1:8788")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 32 << 20)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn rest_reads_numbers_and_booleans_sent_as_strings() {
    let site = Site::start(vec![("/long", long_page())]).await;
    let (status, out) = rest(
        "/v1/fetch",
        json!({"url": site.url("/long"), "max_tokens": "60", "robots": "ignore",
               "cache": "bypass", "include_metadata": "true"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["truncated"], true, "{out}");
    assert!(out["metadata"].is_object(), "{out}");
}

#[tokio::test]
async fn rest_refuses_an_unknown_parameter_with_a_400_that_names_it() {
    let site = Site::start(vec![("/long", long_page())]).await;
    let (status, out) = rest(
        "/v1/fetch",
        json!({"url": site.url("/long"), "maxtokens": 60}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{out}");
    let msg = out["error"].as_str().unwrap_or_default();
    assert!(msg.contains("`maxtokens`") && msg.contains("max_tokens"), "{msg}");
    assert_eq!(site.hits("/long"), 0);

    // The crawl route takes one transport field of its own, and only that one.
    let (status, out) = rest(
        "/v1/crawl",
        json!({"url": site.url("/long"), "async": false, "max_pages": "1", "robots": "ignore"}),
    )
    .await;
    assert_ne!(status, StatusCode::BAD_REQUEST, "{out}");
    let (status, out) = rest(
        "/v1/crawl",
        json!({"url": site.url("/long"), "pages": 1}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{out}");
}
