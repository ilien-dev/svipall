//! web_fetch_many fits in the answer a client will accept, and says where the rest is.
//!
//! Each URL used to get 25 000 tokens and the set had no total, so twenty long pages made a
//! response of half a million characters; about 58 such answers were refused by the client as too
//! large, which is the same as getting nothing. The budget is measured here as the client measures
//! it: the length of the serialised response.
mod support;

use serde_json::{json, Value};
use support::{Reply, Site};
use svipall::server::SvipallServer;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            max_tier: "http".into(),
            ..Default::default()
        },
        None,
    )
}

fn long(i: usize) -> Reply {
    let para = format!(
        "<p>Page {i}: the harbour council met on Tuesday and voted on the redevelopment plan after \
         a debate that ran well past midnight, with members split on the costings.</p>"
    )
    .repeat(120);
    Reply::html(&format!(
        "<!doctype html><html><head><title>Page {i}</title></head><body><main>{para}\
         <p>END-OF-PAGE-{i}</p></main></body></html>"
    ))
}

async fn site(n: usize) -> (Site, Vec<String>) {
    let routes: Vec<(String, Reply)> = (0..n).map(|i| (format!("/p{i}"), long(i))).collect();
    let site = Site::start(
        routes
            .iter()
            .map(|(p, r)| (p.as_str(), r.clone()))
            .collect(),
    )
    .await;
    let urls = (0..n).map(|i| site.url(&format!("/p{i}"))).collect();
    (site, urls)
}

async fn many(args: Value) -> Value {
    server()
        .fetch_many_json(serde_json::from_value(args).expect("web_fetch_many params"))
        .await
}

/// The size a client sees: the tool result is this JSON as text.
const CLIENT_LIMIT: usize = 50_000;

#[tokio::test]
async fn twenty_five_long_pages_fit_the_answer_a_client_accepts() {
    let (_site, urls) = site(25).await;
    let out = many(json!({"urls": urls, "max_tier": "http"})).await;
    let size = out.to_string().len();
    eprintln!("web_fetch_many over 25 long pages: {size} chars");
    assert!(size < CLIENT_LIMIT - 5_000, "the answer is {size} chars");
    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 25);
    for (i, r) in results.iter().enumerate() {
        assert_eq!(r["url"], urls[i], "input order");
        assert_eq!(r["truncated"], true, "{r}");
        assert!(r["cursor"].is_string(), "{r}");
    }
    assert!(
        out["budget"]["per_url_tokens"].as_u64().unwrap() > 0,
        "{}",
        out["budget"]
    );
    assert!(
        out["continue"]
            .as_str()
            .is_some_and(|c| c.contains("cursor")),
        "{}",
        out["continue"]
    );
}

#[tokio::test]
async fn a_set_too_large_for_any_share_goes_to_a_file_rather_than_being_refused() {
    let (_site, urls) = site(70).await;
    let out = many(json!({"urls": urls, "max_tier": "http"})).await;
    let size = out.to_string().len();
    assert!(size < CLIENT_LIMIT, "the answer is {size} chars");
    let path = out["out_file"].as_str().unwrap_or_else(|| panic!("{out}"));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(written["results"].as_array().map(|r| r.len()), Some(70));
    assert_eq!(
        out["results"][69]["url"], urls[69],
        "the summary keeps input order"
    );
}

#[tokio::test]
async fn a_cursor_from_the_set_continues_on_web_fetch() {
    let (_site, urls) = site(3).await;
    let out = many(json!({"urls": urls, "max_tier": "http", "max_tokens": 200})).await;
    let first = &out["results"][1];
    let cursor = first["cursor"].as_str().expect("truncated");
    let next = server()
        .fetch_json(
            serde_json::from_value(json!({
                "url": urls[1], "cursor": cursor, "max_tier": "http", "max_tokens": 200,
            }))
            .unwrap(),
        )
        .await
        .value;
    assert!(next.get("stale_cursor").is_none(), "{next}");
    assert_ne!(next["content"], first["content"], "{next}");
}

#[tokio::test]
async fn max_tokens_caps_every_page() {
    let (_site, urls) = site(4).await;
    let out = many(json!({"urls": urls, "max_tier": "http", "max_tokens": 100})).await;
    for r in out["results"].as_array().unwrap() {
        assert!(r["tokens_estimated"].as_u64().unwrap() <= 100, "{r}");
    }
}

#[tokio::test]
async fn out_dir_writes_every_page_whole_and_returns_paths() {
    let (_site, urls) = site(5).await;
    let dir = support::isolate().join("many-out");
    let _ = std::fs::remove_dir_all(&dir);
    let out =
        many(json!({"urls": urls, "max_tier": "http", "out_dir": dir.to_string_lossy()})).await;
    for (i, r) in out["results"].as_array().unwrap().iter().enumerate() {
        assert!(r.get("content").is_none(), "{r}");
        let path = r["out_file"]
            .as_str()
            .unwrap_or_else(|| panic!("no path: {r}"));
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            text.contains(&format!("END-OF-PAGE-{i}")),
            "page {i} is not whole"
        );
    }
    assert!(out.to_string().len() < 5_000, "{out}");
}

#[tokio::test]
async fn out_file_writes_the_whole_set_and_returns_a_summary() {
    let (_site, urls) = site(6).await;
    let file = support::isolate().join("many.json");
    let out =
        many(json!({"urls": urls, "max_tier": "http", "out_file": file.to_string_lossy()})).await;
    assert_eq!(out["out_file"], file.to_string_lossy().as_ref(), "{out}");
    assert!(out.to_string().len() < 5_000, "{out}");
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let results = written["results"].as_array().unwrap();
    assert_eq!(results.len(), 6);
    for (i, r) in results.iter().enumerate() {
        assert!(
            r["content"]
                .as_str()
                .unwrap()
                .contains(&format!("END-OF-PAGE-{i}")),
            "page {i} is not whole in the file"
        );
    }
}

/// Two topics on one page, so a `query` is a view of it rather than all of it.
fn two_topics() -> Reply {
    let paras: String = (0..80)
        .map(|i| {
            let topic = if i % 2 == 0 {
                "quay cranes"
            } else {
                "pier lights"
            };
            format!(
                "<p>Paragraph {i} on the {topic} of the harbour, long enough that the page needs \
                 more than one part to read whole.</p>"
            )
        })
        .collect();
    Reply::html(&format!(
        "<!doctype html><html><head><title>Harbour</title></head><body><main><h1>Harbour</h1>\
         {paras}</main></body></html>"
    ))
}

/// A cursor a batch issued under `query` continues on web_fetch with the same view, whether the
/// caller repeats the query or only passes the cursor the `continue` note told it to.
#[tokio::test]
async fn a_cursor_the_set_issued_under_a_query_continues_on_web_fetch() {
    let site = Site::start(vec![("/h", two_topics()), ("/o", long(1))]).await;
    let urls = [site.url("/h"), site.url("/o")];
    let out = many(json!({"urls": urls, "max_tier": "http", "max_tokens": 150,
                          "query": "quay cranes"}))
    .await;
    let first = &out["results"][0];
    let cursor = first["cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("truncated: {out}"));
    for query in [Some("quay cranes"), None] {
        let mut args = json!({"url": urls[0], "cursor": cursor, "max_tier": "http",
                              "max_tokens": 150});
        if let Some(q) = query {
            args["query"] = json!(q);
        }
        let next = server()
            .fetch_json(serde_json::from_value(args).unwrap())
            .await
            .value;
        assert!(next.get("stale_cursor").is_none(), "{query:?}: {next}");
        let text = next["content"].as_str().unwrap_or_default();
        assert_ne!(next["content"], first["content"], "{next}");
        assert!(
            text.contains("quay cranes") && !text.contains("pier lights"),
            "{query:?}: the continuation is the view the cursor was issued for: {next}"
        );
    }
}

/// A cursor a batch issued under `json_path` continues on web_fetch given the same path, as the
/// set's `continue` note says to.
#[tokio::test]
async fn a_cursor_the_set_issued_under_json_path_continues_on_web_fetch() {
    let items: Vec<Value> = (1..=200)
        .map(|i| json!({"id": i, "name": format!("item number {i}"), "noise": "x".repeat(40)}))
        .collect();
    let body = json!({"items": items}).to_string();
    let site = Site::start(vec![(
        "/api",
        Reply::bytes(body.as_bytes(), "application/json"),
    )])
    .await;
    let url = site.url("/api");
    let path = "items[*].{id, name}";
    let out = many(json!({"urls": [url], "max_tier": "http", "max_tokens": 200,
                          "json_path": path}))
    .await;
    let first = &out["results"][0];
    let cursor = first["cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("truncated: {out}"));
    assert!(
        out["continue"]
            .as_str()
            .unwrap_or_default()
            .contains("json_path"),
        "{out}"
    );
    let next = server()
        .fetch_json(
            serde_json::from_value(json!({"url": url, "cursor": cursor, "max_tier": "http",
                                          "max_tokens": 200, "json_path": path}))
            .unwrap(),
        )
        .await
        .value;
    assert!(next.get("stale_cursor").is_none(), "{next}");
    let text = next["content"].as_str().unwrap_or_default();
    assert_ne!(next["content"], first["content"], "{next}");
    assert!(!text.contains("noise"), "still the projection: {next}");
    // And web_fetch's own hint for the part after this one names the path to pass again.
    assert!(
        next["continue"]
            .as_str()
            .is_some_and(|c| c.contains("json_path=")),
        "{next}"
    );
    assert!(
        !text.contains("\"id\": 1,"),
        "not restarted from the top: {next}"
    );
}
