//! A page the first tier cannot read, read at the first tier from an alternate form of itself
//! that it declares: a feed, an activity, an embed.
//!
//! One more plain request to the page's own site, instead of a browser.

mod support;

use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

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

/// An application's page: a root, a script, and whatever its head declares.
fn shell(title: &str, head: &str) -> String {
    let scripts = "<script>window.__chunks = (window.__chunks || []).concat([1, 2, 3]);</script>\n"
        .repeat(1_500);
    format!(
        "<!DOCTYPE html><html><head><title>{title}</title>\n{head}\n{scripts}</head>\
         <body><div id=\"root\"></div><noscript>JavaScript is required.</noscript></body></html>"
    )
}

async fn read(site: &Site, path: &str) -> serde_json::Value {
    server()
        .fetch_json(WebFetchParams {
            url: site.url(path),
            // The claim is about the first tier: nothing here may be answered by a browser.
            max_tier: Some("http".into()),
            ..Default::default()
        })
        .await
        .value
}

const NOTE: &str = r#"{"@context":"https://www.w3.org/ns/activitystreams","type":"Note","published":"2016-03-16T14:44:31Z","attributedTo":"https://social.test/users/aurora","content":"<p>Hello world</p>","likes":{"type":"Collection","totalItems":1152},"shares":{"type":"Collection","totalItems":605}}"#;

#[tokio::test]
async fn a_short_post_is_read_from_the_activity_its_page_declares() {
    let page = shell(
        "Aurora: \"Hello world\" - Social",
        r#"<link href="/api/oembed?format=json" rel="alternate" type="application/json+oembed">
           <link href="/users/aurora/statuses/1" rel="alternate" type="application/activity+json">"#,
    );
    let site = Site::start(vec![
        ("/@aurora/1", Reply::html(&page)),
        (
            "/users/aurora/statuses/1",
            Reply::bytes(NOTE.as_bytes(), "application/activity+json; charset=utf-8"),
        ),
        (
            "/api/oembed?format=json",
            Reply::bytes(
                br#"{"html":"<iframe src=\"/embed\"></iframe>"}"#,
                "application/json",
            ),
        ),
    ])
    .await;
    let v = read(&site, "/@aurora/1").await;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert_eq!(v["tier_used"], "http", "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("**@aurora · 2016-03-16 14:44 UTC · 1152 likes · 605 shares**"),
        "{content}"
    );
    assert!(content.contains("Hello world"), "{content}");
    assert!(
        content.contains("holds the post and not the replies to it"),
        "{content}"
    );
    assert_eq!(v["title"], "Aurora: \"Hello world\" - Social", "{v}");
    assert_eq!(
        v["url"],
        site.url("/@aurora/1"),
        "the page asked for is the page answered: {v}"
    );
    assert!(
        v["attempts"].to_string().contains("activity"),
        "the answer says where it was read from: {v}"
    );
    // The activity came first and was enough: the embed was never asked.
    assert_eq!(site.hits("/users/aurora/statuses/1"), 1);
    assert_eq!(site.hits("/api/oembed?format=json"), 0);
}

#[tokio::test]
async fn a_profile_is_read_from_its_own_feed_and_not_from_somebody_elses() {
    // The feed names the page it is the feed of. A port is not part of that comparison, so the
    // fixture can say so without knowing the one it will be given.
    let feed = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Aurora Station</title>
<description>Public posts from @aurora</description><link>http://127.0.0.1/@aurora</link>
<item><link>http://127.0.0.1/@aurora/3</link><pubDate>Thu, 26 Nov 2026 21:09:00 +0000</pubDate>
<description>&lt;p&gt;The winter crew arrived today, fourteen people and one confused dog.&lt;/p&gt;</description></item>
<item><link>http://127.0.0.1/@aurora/2</link><pubDate>Wed, 25 Nov 2026 17:22:31 +0000</pubDate>
<description>&lt;p&gt;First light over the ridge after six weeks of dark.&lt;/p&gt;</description></item>
</channel></rss>"#;
    let head = r#"<link href="/@aurora.rss" rel="alternate" type="application/rss+xml">"#;
    let site = Site::start(vec![
        ("/@aurora", Reply::html(&shell("Aurora (@aurora)", head))),
        ("/blog/an-article", Reply::html(&shell("An article", head))),
        (
            "/@aurora.rss",
            Reply::bytes(feed.as_bytes(), "application/rss+xml; charset=utf-8"),
        ),
    ])
    .await;
    let v = read(&site, "/@aurora").await;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert_eq!(v["tier_used"], "http", "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(content.contains("# Aurora Station"), "{content}");
    assert!(
        content.contains("The winter crew arrived today"),
        "{content}"
    );
    assert!(content.contains("First light over the ridge"), "{content}");
    assert!(
        content.contains("[Thu, 26 Nov 2026 21:09:00 +0000](http://127.0.0.1/@aurora/3)"),
        "{content}"
    );

    // The same feed, linked from an article of the same site, is not that article.
    let v = read(&site, "/blog/an-article").await;
    assert!(v["blocked_reason"].as_str().is_some(), "{v}");
}

#[tokio::test]
async fn an_embed_with_the_post_in_it_is_read_and_a_frame_is_not() {
    let head =
        r#"<link rel="alternate" type="application/json+oembed" href="/oembed?url=status-20">"#;
    let site = Site::start(vec![
        ("/jack/status/20", Reply::html(&shell("jack on Social", head))),
        (
            "/oembed?url=status-20",
            Reply::bytes(
                br#"{"author_name":"jack","html":"<blockquote><p lang=\"en\">just setting up my account</p>&mdash; jack (@jack) <a href=\"/jack/status/20\">March 21, 2006</a></blockquote><script async src=\"/widgets.js\"></script>"}"#,
                "application/json",
            ),
        ),
        (
            "/video/1",
            Reply::html(&shell(
                "A video",
                r#"<link rel="alternate" type="application/json+oembed" href="/oembed?url=video-1">"#,
            )),
        ),
        (
            "/oembed?url=video-1",
            Reply::bytes(br#"{"title":"A video","html":"<iframe src=\"/embed/1\"></iframe>"}"#, "application/json"),
        ),
    ])
    .await;
    let v = read(&site, "/jack/status/20").await;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(content.contains("just setting up my account"), "{content}");
    assert!(content.contains("**jack**"), "{content}");
    assert!(!content.contains("widgets.js"), "{content}");

    let v = read(&site, "/video/1").await;
    assert!(
        v["blocked_reason"].as_str().is_some(),
        "a frame says nothing: {v}"
    );
}

#[tokio::test]
async fn an_alternate_on_another_site_is_never_asked() {
    let other = Site::start(vec![(
        "/users/aurora/statuses/1",
        Reply::bytes(NOTE.as_bytes(), "application/activity+json"),
    )])
    .await;
    let head = format!(
        r#"<link href="{}" rel="alternate" type="application/activity+json">"#,
        other
            .url("/users/aurora/statuses/1")
            .replace("127.0.0.1", "localhost")
    );
    let site = Site::start(vec![("/@aurora/1", Reply::html(&shell("Aurora", &head)))]).await;
    let v = read(&site, "/@aurora/1").await;
    assert!(v["blocked_reason"].as_str().is_some(), "{v}");
    assert_eq!(other.hits("/users/aurora/statuses/1"), 0);
}

#[tokio::test]
async fn a_page_that_can_be_read_is_not_asked_for_another_form() {
    let para =
        "The winter crew arrived today, fourteen people and one very confused dog. ".repeat(40);
    let page = format!(
        "<html><head><title>Readable</title>\
         <link href=\"/feed.rss\" rel=\"alternate\" type=\"application/rss+xml\">\
         <link href=\"/act\" rel=\"alternate\" type=\"application/activity+json\"></head>\
         <body><main><h1>Readable</h1><p>{para}</p></main></body></html>"
    );
    let site = Site::start(vec![
        ("/article", Reply::html(&page)),
        ("/feed.rss", Reply::plain("")),
        (
            "/act",
            Reply::bytes(NOTE.as_bytes(), "application/activity+json"),
        ),
    ])
    .await;
    let v = read(&site, "/article").await;
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert!(v["content"]
        .as_str()
        .unwrap_or_default()
        .contains("# Readable"));
    assert_eq!(site.hits("/feed.rss") + site.hits("/act"), 0);
}

#[tokio::test]
async fn an_alternate_that_fails_leaves_the_page_what_it_was() {
    let head = r#"<link href="/gone" rel="alternate" type="application/activity+json">"#;
    let site = Site::start(vec![
        ("/@aurora/1", Reply::html(&shell("Aurora", head))),
        (
            "/gone",
            Reply::html("<html><body>Not found</body></html>").with_status(404),
        ),
    ])
    .await;
    let v = read(&site, "/@aurora/1").await;
    let reason = v["blocked_reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("near-empty") || reason.contains("shell"),
        "{v}"
    );
    assert_eq!(site.hits("/gone"), 1);
}
