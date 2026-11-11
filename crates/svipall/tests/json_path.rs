//! `json_path` on web_fetch and web_fetch_many: an API's answer cut down to the fields asked for,
//! where it used to be written to a file so a script could do the cutting.
mod support;

use serde_json::{json, Value};
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

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

fn api() -> Reply {
    let items: Vec<Value> = (1..=30)
        .map(|i| {
            json!({"id": i, "full_name": format!("org/repo-{i}"), "stars": i * 10,
                   "owner": {"login": format!("user{i}"), "avatar_url": "https://x.test/a.png"},
                   "description": "a long description nobody asked for ".repeat(5)})
        })
        .collect();
    Reply::bytes(
        json!({"total_count": 30, "items": items}).to_string().as_bytes(),
        "application/json; charset=utf-8",
    )
}

fn params(url: String, path: &str) -> WebFetchParams {
    serde_json::from_value(json!({
        "url": url, "json_path": path, "cache": "bypass", "robots": "ignore",
    }))
    .expect("web_fetch takes json_path")
}

#[tokio::test]
async fn json_path_returns_only_the_fields_asked_for() {
    let site = Site::start(vec![("/search", api())]).await;
    let out = server()
        .fetch_json(params(
            site.url("/search?q=x"),
            "items[:3].{full_name, owner.login}",
        ))
        .await
        .value;
    let content: Value = serde_json::from_str(out["content"].as_str().unwrap_or_default())
        .unwrap_or_else(|_| panic!("content is not the projection: {out}"));
    assert_eq!(
        content,
        json!([
            {"full_name": "org/repo-1", "owner.login": "user1"},
            {"full_name": "org/repo-2", "owner.login": "user2"},
            {"full_name": "org/repo-3", "owner.login": "user3"},
        ]),
        "{out}"
    );
    assert!(out.get("json_path_error").is_none(), "{out}");
}

#[tokio::test]
async fn json_path_works_on_every_page_of_web_fetch_many() {
    let site = Site::start(vec![("/a", api()), ("/b", api())]).await;
    let out = server()
        .fetch_many_json(
            serde_json::from_value(json!({
                "urls": [site.url("/a"), site.url("/b")],
                "json_path": "total_count",
                "max_tier": "http",
            }))
            .expect("web_fetch_many takes json_path"),
        )
        .await;
    for r in out["results"].as_array().unwrap() {
        assert_eq!(r["content"], "30", "{r}");
    }
}

#[tokio::test]
async fn a_path_on_a_page_that_is_not_json_says_so_and_withholds_nothing() {
    let site = Site::start(vec![("/page", Reply::page("Harbour", &[]))]).await;
    let out = server()
        .fetch_json(params(site.url("/page"), "items[0]"))
        .await
        .value;
    assert!(
        out["json_path_error"]
            .as_str()
            .is_some_and(|e| e.contains("not JSON")),
        "{out}"
    );
    assert!(
        out["content"].as_str().unwrap_or_default().contains("Harbour"),
        "{out}"
    );
}

#[tokio::test]
async fn a_path_that_names_nothing_says_which_keys_there_are() {
    let site = Site::start(vec![("/search", api())]).await;
    let out = server()
        .fetch_json(params(site.url("/search"), "results[0].name"))
        .await
        .value;
    assert_eq!(out["content"], "null", "{out}");
    let note = out["json_path_note"].as_str().unwrap_or_default();
    assert!(note.contains("items") && note.contains("total_count"), "{out}");
}
