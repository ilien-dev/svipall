//! A wrong call gets an answer that makes the next call right.
//!
//! Every case here was found by calling the tools wrongly on purpose and reading what came back.
//! Most of it was true and useless: `task not found`, `bad url`, an empty crawl reported as a
//! success, a proxy that was not a URL stored without a word. None of these hit the network.
mod support;

use serde_json::{json, Value};
use svipall::server::SvipallServer;
use svipall::tools::{
    WebCrawlParams, WebDiffParams, WebFetchParams, WebMapParams, WebRouteParams, WebSearchParams,
    WebSiteSearchParams, WebWatchParams,
};

fn server() -> SvipallServer {
    support::isolate();
    let store = svipall_core::cache::Store::open_memory()
        .ok()
        .map(std::sync::Arc::new);
    SvipallServer::with_store(None, svipall_core::Config::default(), None, store)
}

fn text(v: &Value) -> String {
    v["content"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_malformed_url_is_named_as_such_not_as_a_policy_refusal() {
    let out = server()
        .fetch_json(WebFetchParams {
            url: "not a url".into(),
            ..Default::default()
        })
        .await;
    let note = out.value["note"].as_str().unwrap_or_default();
    assert!(note.contains("raw:<html>"), "{:?}", out.value);
    assert!(!note.contains("config.toml"), "{:?}", out.value);
}

#[tokio::test]
async fn a_tier_that_does_not_exist_is_refused_before_anything_is_requested() {
    for (mode, max_tier) in [(Some("bogus"), None), (None, Some("turbo"))] {
        let out = server()
            .fetch_json(WebFetchParams {
                url: "raw:<html><body><p>hi</p></body></html>".into(),
                mode: mode.map(str::to_string),
                max_tier: max_tier.map(str::to_string),
                ..Default::default()
            })
            .await;
        let error = out.value["error"].as_str().unwrap_or_default();
        assert!(error.contains("is not a tier"), "{:?}", out.value);
        assert!(error.contains("warm"), "{:?}", out.value);
        assert!(out.value.get("attempts").is_none(), "{:?}", out.value);
    }
}

#[tokio::test]
async fn a_cursor_from_nowhere_is_reported_rather_than_silently_dropped() {
    let out = server()
        .fetch_json(WebFetchParams {
            url: "raw:<html><body><p>Some text here.</p></body></html>".into(),
            cursor: Some("garbage".into()),
            ..Default::default()
        })
        .await;
    assert!(text(&out.value).contains("Some text"), "{:?}", out.value);
    assert!(
        out.value["cursor_error"]
            .as_str()
            .is_some_and(|m| m.contains("earlier result")),
        "{:?}",
        out.value
    );
}

#[tokio::test]
async fn a_crawl_from_a_non_url_is_an_error_not_an_empty_success() {
    let out = server()
        .crawl_json(WebCrawlParams {
            url: "not a url".into(),
            ..Default::default()
        })
        .await;
    assert!(
        out["error"].as_str().is_some_and(|m| m.contains("http")),
        "{out}"
    );
    assert!(
        out.get("crawl_id").is_none(),
        "an empty crawl was recorded: {out}"
    );
}

#[tokio::test]
async fn map_diff_and_site_search_say_what_url_they_need() {
    let s = server();
    let map = s
        .map_json(WebMapParams {
            url: "not a url".into(),
            sources: None,
            limit: None,
            include: None,
        })
        .await
        .expect_err("a non-url mapped");
    assert!(map.to_string().contains("http"), "{map}");
    assert!(!map.to_string().contains("-32603"), "{map}");

    let diff = s
        .diff_json(WebDiffParams {
            url: "not a url".into(),
            refetch: Some(false),
        })
        .await
        .expect_err("a non-url diffed");
    assert!(diff.to_string().contains("http"), "{diff}");

    let search = s
        .site_search_json(WebSiteSearchParams {
            url: "raw:<html><body><p>no form</p></body></html>".into(),
            query: "x".into(),
            fetch: None,
            timeout: None,
        })
        .await
        .expect_err("raw markup searched");
    assert!(search.to_string().contains("http(s)"), "{search}");
}

#[tokio::test]
async fn an_empty_search_asks_no_engine() {
    let out = server()
        .search_json(WebSearchParams {
            query: "   ".into(),
            engine: None,
            limit: None,
        })
        .await;
    assert!(
        out["error"].as_str().is_some_and(|m| m.contains("empty")),
        "{out}"
    );
    assert!(
        out["attempts"].as_array().is_none_or(Vec::is_empty),
        "engines were asked: {out}"
    );
}

#[tokio::test]
async fn a_proxy_that_is_not_a_url_is_refused_not_stored() {
    let s = server();
    let err = s
        .route_json(WebRouteParams {
            domain: Some("shop.example".into()),
            proxy: Some("not a url".into()),
            ..Default::default()
        })
        .await
        .expect_err("a non-url proxy was stored");
    assert!(err.to_string().contains("socks5h"), "{err}");
    let routes = s.route_json(WebRouteParams::default()).await.expect("list");
    assert!(routes["routes"].get("shop.example").is_none(), "{routes}");

    let err = s
        .route_json(WebRouteParams {
            domain: Some("shop.example".into()),
            proxies: Some(vec!["socks5h://u:p@10.0.0.5:1080".into(), "nope".into()]),
            ..Default::default()
        })
        .await
        .expect_err("a pool with a non-url was stored");
    assert!(err.to_string().contains("nope"), "{err}");
}

#[tokio::test]
async fn removing_a_watch_nobody_set_says_where_to_look() {
    let out = server()
        .watch_json(WebWatchParams {
            action: Some("remove".into()),
            url: Some("https://never.example/x".into()),
            interval_secs: None,
            label: None,
            css_selector: None,
        })
        .await
        .expect("remove");
    assert_eq!(out["removed"], json!(false), "{out}");
    assert!(
        out["note"]
            .as_str()
            .is_some_and(|m| m.contains("web_watch list")),
        "{out}"
    );
}
