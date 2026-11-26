//! A page built of post and comment elements is an answer at the first tier.
//!
//! Its server streams the comments in a `<template>`, so the page is a megabyte of markup around
//! what the ordinary walk reads as a few hundred characters. That used to be called an unrendered
//! shell and sent up through every browser tier for what the first answer already held.

mod support;

use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

const THREAD: &str = include_str!("../../svipall-extract/tests/fixtures/post-elements-thread.html");

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

/// The fixture at the weight the real page has: scripts around the same elements.
fn heavy() -> String {
    let padding = "<script>window.__chunks = (window.__chunks || []).concat([1, 2, 3]);</script>\n"
        .repeat(3_000);
    THREAD.replace("</head>", &format!("{padding}</head>"))
}

#[tokio::test]
async fn a_thread_is_delivered_by_the_first_tier_with_its_comments() {
    let page = heavy();
    assert!(page.len() > 200_000);
    let site = Site::start(vec![(
        "/r/rustlearners/comments/1abc234/",
        Reply::html(&page),
    )])
    .await;
    let v = server()
        .fetch_json(WebFetchParams {
            url: site.url("/r/rustlearners/comments/1abc234/"),
            ..Default::default()
        })
        .await
        .value;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert_eq!(v["tier_used"], "http", "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("# Borrowing across an await point"),
        "{content}"
    );
    assert!(content.contains("## Comments (4 of 7 loaded)"), "{content}");
    assert!(
        content.contains("> > **u/helper_one** · 2 points"),
        "{content}"
    );
    assert_eq!(
        v["title"], "Borrowing across an await point : r/rustlearners",
        "{v}"
    );
    assert_eq!(site.hits("/r/rustlearners/comments/1abc234/"), 1);
}

#[tokio::test]
async fn the_same_weight_of_markup_with_no_post_in_it_is_still_a_shell() {
    let empty = heavy()
        .split("<shreddit-post")
        .next()
        .map(|head| format!("{head}</main></div></shreddit-app></body></html>"))
        .unwrap_or_default();
    let site = Site::start(vec![("/", Reply::html(&empty))]).await;
    let v = server()
        .fetch_json(WebFetchParams {
            url: site.url("/"),
            max_tier: Some("http".into()),
            ..Default::default()
        })
        .await
        .value;
    assert!(v["blocked_reason"].is_string(), "{v}");
}

const CHECK: &str = include_str!("../../svipall-core/fixtures/script-check.html");

#[tokio::test]
async fn a_script_check_is_answered_by_the_first_tier_and_the_page_filed_where_it_was_asked() {
    let page = heavy();
    let site = Site::start(vec![
        ("/r/rust/top/", Reply::html(CHECK)),
        (
            "/r/rust/top/?solution=96d3dc23fd7cd75496d3dc23fd7cd754&js_challenge=1\
             &jsc_token=2824be10929bdc60&jsc_orig_r=&t=week",
            Reply::html(&page),
        ),
    ])
    .await;
    let asked = site.url("/r/rust/top/?t=week");
    let v = server()
        .fetch_json(WebFetchParams {
            url: asked.clone(),
            ..Default::default()
        })
        .await
        .value;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert_eq!(v["tier_used"], "http", "{v}");
    assert!(
        v["content"]
            .as_str()
            .unwrap_or_default()
            .contains("## Comments (4 of 7 loaded)"),
        "{v}"
    );
    assert_eq!(v["url"], asked, "{v}");
    assert!(v.get("final_url").is_none(), "{v}");
    assert_eq!(site.hits("/r/rust/top/"), 2);
}

#[tokio::test]
async fn a_check_that_answers_with_itself_is_not_asked_a_third_time() {
    let site = Site::start(vec![("/r/rust/top/", Reply::html(CHECK))]).await;
    let v = server()
        .fetch_json(WebFetchParams {
            url: site.url("/r/rust/top/?t=week"),
            max_tier: Some("http".into()),
            ..Default::default()
        })
        .await
        .value;
    assert!(v["blocked_reason"].is_string(), "{v}");
    assert_eq!(site.hits("/r/rust/top/"), 2);
}
