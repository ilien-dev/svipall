//! This machine's public address stays out of everything a caller can read, not only `content`.
//!
//! The address is learned from a wall that names the visitor (see `svipall_core::own_ip`), and the
//! response already masked it in `content` and `title`. What these drive is the rest: a text file
//! written through `out_file` or `out_dir`, the request log, a watch, a capture. The tests share one
//! planted address from a documentation range (RFC 5737): only four are remembered, and tests that
//! each learned their own would push one another's out. The first-wall test has one of its own,
//! because it must not be known before that wall. A binary file is the exception, and is asserted
//! to be: the caller asked for the bytes, and they arrive as served.
mod support;

use serde_json::{json, Value};
use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::with_store(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            ..Default::default()
        },
        None,
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    )
}

fn local(url: String) -> WebFetchParams {
    WebFetchParams {
        url,
        robots: Some("ignore".into()),
        cache: Some("bypass".into()),
        ..Default::default()
    }
}

fn learn(ip: &str) {
    support::isolate();
    svipall_core::own_ip::learn(&[ip.parse().unwrap()]);
}

fn page(title: &str, line: &str) -> Reply {
    Reply::html(&format!(
        "<!doctype html><html><head><title>{title}</title></head><body><main><h1>{title}</h1>\
         <p>This page reports the state of the service and is refreshed every few minutes by the \
         operators on duty, so whoever reads it knows what changed.</p><p>{line}</p></main>\
         </body></html>"
    ))
}

fn out_dir(name: &str) -> std::path::PathBuf {
    let dir = support::isolate().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read(path: &Value) -> String {
    let p = path.as_str().unwrap_or_else(|| panic!("no path: {path}"));
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// The first wall that names the address, written to a file: the address is learned from that
/// very page, after the file was written. The file is masked all the same.
#[tokio::test]
async fn the_first_wall_written_to_out_file_does_not_carry_the_address() {
    let wall = "<!doctype html><html><head><title>Access denied</title></head><body>\
        <h1>Access denied</h1><p>Your IP address: 198.51.100.71</p>\
        <p>Ray ID: 8c1f2a</p></body></html>";
    let site = Site::start(vec![("/blocked", Reply::html(wall).with_status(403))]).await;
    let file = out_dir("wall").join("wall.html");
    let out = server()
        .fetch_json(WebFetchParams {
            out_file: Some(file.to_string_lossy().into_owned()),
            max_tier: Some("http".into()),
            ..local(site.url("/blocked"))
        })
        .await
        .value;
    assert!(out["blocked_reason"].is_string(), "{out}");
    let text = read(&out["out_file"]);
    assert!(text.contains("Ray ID"), "the wall is written whole: {text}");
    assert!(!text.contains("198.51.100.71"), "{text}");
    assert!(text.contains("[own IP]"), "{text}");
    assert!(!out.to_string().contains("198.51.100.71"), "{out}");
}

/// A page's text written to a file, once the address is known.
#[tokio::test]
async fn a_text_out_file_is_masked() {
    learn("198.51.100.72");
    let site = Site::start(vec![(
        "/echo",
        page("Visitors", "Last visit from 198.51.100.72, and one from 192.0.2.10."),
    )])
    .await;
    let file = out_dir("text").join("echo.md");
    let out = server()
        .fetch_json(WebFetchParams {
            out_file: Some(file.to_string_lossy().into_owned()),
            ..local(site.url("/echo"))
        })
        .await
        .value;
    let text = read(&out["out_file"]);
    assert!(text.contains("Last visit from"), "{text}");
    assert!(!text.contains("198.51.100.72"), "{text}");
    assert!(text.contains("192.0.2.10"), "another address is not ours: {text}");
}

/// Rows written to a file are page text too.
#[tokio::test]
async fn table_rows_written_to_a_file_are_masked() {
    learn("198.51.100.72");
    let table = "<!doctype html><html><head><title>Hosts</title></head><body><main>\
        <h1>Hosts</h1><p>Every visitor that reached this page in the last hour, with where they \
        came from, so the operator can see who reads it.</p><table><thead><tr><th>Address</th>\
        <th>Seen</th></tr></thead><tbody><tr><td>198.51.100.72</td><td>now</td></tr>\
        <tr><td>192.0.2.10</td><td>earlier</td></tr></tbody></table></main></body></html>";
    let site = Site::start(vec![("/hosts", Reply::html(table))]).await;
    let file = out_dir("rows").join("hosts.csv");
    let out = server()
        .fetch_json(WebFetchParams {
            tables: Some(true),
            out_file: Some(file.to_string_lossy().into_owned()),
            ..local(site.url("/hosts"))
        })
        .await
        .value;
    let text = read(&out["out_file"]);
    assert!(text.contains("192.0.2.10"), "{text}");
    assert!(!text.contains("198.51.100.72"), "{text}");
}

/// web_fetch_many's `out_dir` (one file per page) and `out_file` (the whole set).
#[tokio::test]
async fn web_fetch_many_files_are_masked() {
    learn("198.51.100.72");
    let site = Site::start(vec![
        ("/a", page("Alpha", "Seen from 198.51.100.72 a moment ago.")),
        ("/b", page("Beta", "And again from 198.51.100.72 later on.")),
    ])
    .await;
    let s = server();
    let dir = out_dir("many-dir");
    let out = s
        .fetch_many_json(
            serde_json::from_value(json!({
                "urls": [site.url("/a"), site.url("/b")],
                "out_dir": dir.to_string_lossy(),
            }))
            .unwrap(),
        )
        .await;
    let results = out["results"].as_array().expect("results").clone();
    assert_eq!(results.len(), 2, "{out}");
    for r in &results {
        let text = read(&r["out_file"]);
        assert!(text.contains("from"), "{text}");
        assert!(!text.contains("198.51.100.72"), "{text}");
    }
    let set = out_dir("many-file").join("set.json");
    let out = s
        .fetch_many_json(
            serde_json::from_value(json!({
                "urls": [site.url("/a"), site.url("/b")],
                "out_file": set.to_string_lossy(),
            }))
            .unwrap(),
        )
        .await;
    let text = read(&out["out_file"]);
    assert!(text.contains("Alpha"), "{text}");
    assert!(!text.contains("198.51.100.72"), "{text}");
}

/// A crawl's rows to a file.
#[tokio::test]
async fn a_crawl_written_to_a_file_is_masked() {
    learn("198.51.100.72");
    let site = Site::start(vec![(
        "/",
        page("Home", "Served to 198.51.100.72 on this visit."),
    )])
    .await;
    let file = out_dir("crawl").join("pages.jsonl");
    let out = server()
        .crawl_json(
            serde_json::from_value(json!({
                "url": site.url("/"),
                "max_pages": 1,
                "out_file": file.to_string_lossy(),
            }))
            .unwrap(),
        )
        .await;
    let text = read(&out["out_file"]);
    assert!(text.contains("Served to"), "{text}");
    assert!(!text.contains("198.51.100.72"), "{text}");
}

/// A binary download stays byte for byte what was served, whatever its name says, even when the
/// address happens to be among its bytes.
#[tokio::test]
async fn a_binary_download_is_written_exactly_as_served() {
    learn("198.51.100.72");
    let mut body = vec![0u8, 1, 2, 3, 0xff, 0xfe];
    body.extend_from_slice(b"visitor 198.51.100.72 seen");
    body.extend_from_slice(&[0u8; 64]);
    let want = svipall::body::sha256_hex(&body);
    let site = Site::start(vec![(
        "/blob",
        Reply::bytes(&body, "application/octet-stream"),
    )])
    .await;
    let s = server();
    for name in ["blob.bin", "blob.txt"] {
        let file = out_dir("binary").join(name);
        let out = s
            .fetch_json(WebFetchParams {
                out_file: Some(file.to_string_lossy().into_owned()),
                ..local(site.url("/blob"))
            })
            .await
            .value;
        let written = std::fs::read(&file).unwrap_or_else(|e| panic!("{name}: {e} {out}"));
        assert_eq!(written, body, "{name}: {out}");
        assert_eq!(svipall::body::sha256_hex(&written), want, "{name}");
        assert_eq!(out["file"]["sha256"], want, "{name}: {out}");
    }
}

/// The request log names URLs, and a URL can carry the address.
#[tokio::test]
async fn the_request_log_does_not_carry_the_address() {
    learn("198.51.100.72");
    let site = Site::start(vec![("/seen", page("Seen", "Thanks for visiting."))]).await;
    let s = server();
    let url = site.url("/seen?from=198.51.100.72");
    let _ = s.fetch_json(local(url)).await;
    let log = s
        .log_json(serde_json::from_value(json!({"view": "recent"})).unwrap())
        .expect("the log");
    assert!(log["count"].as_u64().unwrap_or(0) >= 1, "{log}");
    assert!(log.to_string().contains("/seen?from="), "{log}");
    assert!(!log.to_string().contains("198.51.100.72"), "{log}");
}

/// A watch checks a page and says what it found. Nothing of the page's text is in that answer
/// today; this keeps it so.
#[tokio::test]
async fn a_watch_does_not_carry_the_address() {
    learn("198.51.100.72");
    let site = Site::start(vec![(
        "/status",
        page("Status", "Watched from 198.51.100.72."),
    )])
    .await;
    let s = server();
    let url = site.url("/status");
    let add = s
        .watch_json(serde_json::from_value(json!({"action": "add", "url": url})).unwrap())
        .await
        .expect("added");
    let check = s
        .watch_json(serde_json::from_value(json!({"action": "check", "url": url})).unwrap())
        .await
        .expect("checked");
    let list = s
        .watch_json(serde_json::from_value(json!({"action": "list"})).unwrap())
        .await
        .expect("listed");
    for v in [&add, &check, &list] {
        assert!(!v.to_string().contains("198.51.100.72"), "{v}");
    }
    assert_eq!(check["checked"], 1, "{check}");
}

/// What a page fetched while it loaded, read by web_capture.
#[tokio::test]
#[ignore = "opens a local browser"]
async fn a_capture_does_not_carry_the_address() {
    learn("198.51.100.72");
    let site = Site::start(vec![
        (
            "/app",
            Reply::html(
                "<!doctype html><html><head><title>App</title></head><body><p>Loading</p>\
                 <script>fetch('/api/me').then(r => r.json()).then(j => \
                 document.body.append(j.ip));</script></body></html>",
            ),
        ),
        (
            "/api/me",
            Reply::bytes(br#"{"ip":"198.51.100.72","name":"visitor"}"#, "application/json"),
        ),
    ])
    .await;
    let s = server();
    assert!(s.pool().available(), "manual test requires a browser");
    let out = s
        .capture_json(
            serde_json::from_value(json!({
                "url": site.url("/app"),
                "bodies": true,
                "settle_ms": 1500,
            }))
            .unwrap(),
        )
        .await;
    s.shutdown_configuration().await;
    let out = out.expect("a capture");
    println!("{out}");
    assert!(out.to_string().contains("visitor"), "the body was read: {out}");
    assert!(!out.to_string().contains("198.51.100.72"), "{out}");
}
