//! web_probe: does this address exist, and what is behind it — without reading it.
//!
//! Agents looped `curl -o /dev/null -w %{http_code}` and DNS lookups through a shell because the
//! only way here was a full fetch, whose ladder reads a 404 as something to climb past. Here a
//! 404 is the answer, a name that does not resolve says so without a request, and the order of
//! the answers is the order of the question.
mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use support::{Reply, Site};
use svipall::server::SvipallServer;
use tower::ServiceExt;

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

async fn site() -> Site {
    Site::start(vec![
        ("/ok", Reply::page("Fine", &[])),
        (
            "/file.bin",
            Reply::bytes(&vec![9u8; 5000], "application/octet-stream"),
        ),
        (
            "/moved",
            Reply::html("").with_status(302).header("Location", "/ok"),
        ),
    ])
    .await
}

fn urls(site: &Site) -> Vec<String> {
    vec![
        site.url("/ok"),
        site.url("/missing"),
        site.url("/file.bin"),
        site.url("/moved"),
        "http://no-such-host.invalid/x".to_string(),
    ]
}

fn check(out: &Value, urls: &[String]) {
    let r = out["results"].as_array().unwrap_or_else(|| panic!("{out}"));
    assert_eq!(r.len(), urls.len(), "{out}");
    for (i, u) in urls.iter().enumerate() {
        assert_eq!(r[i]["url"], u.as_str(), "input order: {out}");
    }
    assert_eq!(r[0]["status"], 200, "{out}");
    assert_eq!(r[0]["exists"], true, "{out}");
    assert!(
        r[0]["content_type"].as_str().unwrap().contains("text/html"),
        "{out}"
    );

    assert_eq!(r[1]["status"], 404, "{out}");
    assert_eq!(r[1]["exists"], false, "{out}");
    assert!(
        r[1].get("blocked_reason").is_none(),
        "a 404 is an answer: {out}"
    );

    assert_eq!(r[2]["length"], 5000, "{out}");
    assert_eq!(r[2]["content_type"], "application/octet-stream", "{out}");

    assert!(
        r[3]["final_url"]
            .as_str()
            .is_some_and(|f| f.ends_with("/ok")),
        "redirects are followed and named: {out}"
    );
    assert_eq!(r[3]["status"], 200, "{out}");

    assert_eq!(r[4]["resolves"], false, "{out}");
    assert!(r[4].get("status").is_none(), "nothing was requested: {out}");
}

#[tokio::test]
async fn web_probe_answers_status_type_length_and_dns_for_each_url_in_order() {
    let site = site().await;
    let urls = urls(&site);
    let out = server()
        .probe_json(serde_json::from_value(json!({"urls": urls})).unwrap())
        .await;
    check(&out, &urls);
    assert_eq!(
        site.hits("/ok"),
        2,
        "one request each for /ok and the redirect to it"
    );
}

#[tokio::test]
async fn the_rest_route_answers_the_same() {
    const KEY: &str = "a-key-that-is-long-enough";
    let site = site().await;
    let urls = urls(&site);
    let s = server();
    let app = svipall::rest::router(
        s.clone(),
        svipall::jobs::JobRunner::new(s, 1),
        KEY,
        "127.0.0.1",
    );
    let req = Request::builder()
        .method("POST")
        .uri("/v1/probe")
        .header("host", "127.0.0.1:8788")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"urls": urls}).to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    check(&serde_json::from_slice(&bytes).unwrap(), &urls);
}
