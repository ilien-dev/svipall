//! A page that is an empty root and a script, but declares what it holds as schema.org data, is
//! an answer at the first tier.
//!
//! It used to be called an unrendered shell and sent to a browser for what the first answer
//! already said in a `<script type="application/ld+json">`.

mod support;

use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

const THREAD: &str = include_str!("../../svipall-extract/tests/fixtures/linked-data-thread.html");
const PROFILE: &str = include_str!("../../svipall-extract/tests/fixtures/linked-data-profile.html");

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    )
}

/// The fixture at the weight such a page has: an application's scripts around the declaration.
fn heavy(page: &str) -> String {
    let padding = "<script>window.__chunks = (window.__chunks || []).concat([1, 2, 3]);</script>\n"
        .repeat(3_000);
    page.replace("</head>", &format!("{padding}</head>"))
}

async fn read(path: &str, page: &str) -> serde_json::Value {
    let site = Site::start(vec![(path, Reply::html(page))]).await;
    server()
        .fetch_json(WebFetchParams {
            url: site.url(path),
            ..Default::default()
        })
        .await
        .value
}

#[tokio::test]
async fn a_thread_declared_as_data_is_delivered_by_the_first_tier() {
    let page = heavy(THREAD);
    assert!(page.len() > 200_000);
    let v = read("/profile/aurora.example/post/3l6ovee", &page).await;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert_eq!(v["tier_used"], "http", "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        content.starts_with("# We are opening the station logs to everyone."),
        "{content}"
    );
    assert!(
        content.contains("## Comments (4 of 8,779 loaded)"),
        "{content}"
    );
    assert!(
        content.contains("> **Aurora Station (@aurora.example)** · 2024-10-17 07:31 UTC"),
        "{content}"
    );
    assert!(!content.contains("JavaScript Required"), "{content}");
}

#[tokio::test]
async fn a_profile_declared_as_data_is_delivered_by_the_first_tier() {
    let v = read("/profile/aurora.example", &heavy(PROFILE)).await;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert_eq!(v["tier_used"], "http", "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(content.starts_with("# Aurora Station"), "{content}");
    assert!(content.contains("## Posts (3)"), "{content}");
    assert_eq!(v["title"], "@aurora.example on Skyline", "{v}");
}

#[tokio::test]
async fn a_shell_that_declares_only_its_name_is_still_a_shell() {
    let page = heavy(
        "<html><head><title>Skyline</title><script type=\"application/ld+json\">\
         {\"@context\":\"https://schema.org\",\"@type\":\"WebSite\",\"name\":\"Skyline\",\
         \"url\":\"https://skyline.test\"}</script></head>\
         <body><div id=\"root\"></div><noscript>JavaScript is required.</noscript></body></html>",
    );
    let site = Site::start(vec![("/home", Reply::html(&page))]).await;
    let v = server()
        .fetch_json(WebFetchParams {
            url: site.url("/home"),
            max_tier: Some("http".into()),
            ..Default::default()
        })
        .await
        .value;
    assert!(
        v["blocked_reason"].as_str().is_some(),
        "nothing was declared, so nothing was read: {v}"
    );
}
