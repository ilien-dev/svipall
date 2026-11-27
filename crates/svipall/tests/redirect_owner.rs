//! A verdict belongs to the site that gave it, not to the address that redirected there.
//!
//! A resolver (an identifier service, a link shortener) answers every request with a redirect to
//! somebody else's site. When that site refuses, limits or walls the visit, the pause, the
//! cooldown and the memory of the failure used to be filed under the resolver, and the next
//! identifier, which leads to a different site altogether, was refused for it.
//!
//! Hosts are public-looking names routed through a loopback fixture, one set per test.
mod support;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

fn cfg() -> svipall_core::Config {
    support::isolate();
    svipall_core::Config {
        request_min_interval_ms: 1,
        http_engine: "reqwest".into(),
        max_tier: "http".into(),
        auto_native_fallback: false,
        ..Default::default()
    }
}

fn no_browser() -> String {
    let path = support::isolate().join("not-a-browser.txt");
    std::fs::write(&path, "this is not a browser").unwrap();
    path.to_string_lossy().to_string()
}

fn params(site: &Site, url: &str) -> WebFetchParams {
    WebFetchParams {
        url: url.into(),
        proxy: Some(site.url("")),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        // Never wait: what is refused is refused at once, and says why.
        wait: Some(0),
        ..Default::default()
    }
}

fn to(location: &str) -> Reply {
    Reply::html("<html><body>Moved.</body></html>")
        .with_status(302)
        .header("location", location)
}

#[tokio::test]
async fn a_limit_met_behind_a_redirect_holds_the_site_that_set_it() {
    let (first, second) = (
        "http://resolver-a.test/10.1/one",
        "http://resolver-a.test/10.2/two",
    );
    let (limited, open) = (
        "http://limited-press.test/article/1",
        "http://open-press.test/article/2",
    );
    let site = Site::start(vec![
        (first, to(limited)),
        (second, to(open)),
        (
            limited,
            Reply::html("<html><body><p>Too many requests.</p></body></html>")
                .with_status(429)
                .header("retry-after", "600"),
        ),
        (open, Reply::page("An open article", &[])),
    ])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let v = server.fetch_json(params(&site, first)).await.value;
    assert_eq!(v["status"], 429, "{v}");
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("limited-press.test"),
        "the note names the site that asked for the pause: {v}"
    );

    // Another identifier, another publisher: the resolver did nothing wrong.
    let v = server.fetch_json(params(&site, second)).await.value;
    assert_eq!(v["status"], 200, "{v}");
    assert_eq!(v["title"], "An open article", "{v}");

    // The site that set the limit is the one on hold, asked directly or not.
    let v = server.fetch_json(params(&site, limited)).await.value;
    assert_eq!(v["blocked_reason"], "cooldown", "{v}");
    assert_eq!(site.hits(limited), 1);
}

#[tokio::test]
async fn a_wall_behind_a_redirect_is_not_remembered_against_the_resolver() {
    let (first, second) = (
        "http://resolver-b.test/10.1/one",
        "http://resolver-b.test/10.2/two",
    );
    let (walled, open) = (
        "http://walled-press.test/article/1",
        "http://free-press.test/article/2",
    );
    let site = Site::start(vec![
        (first, to(walled)),
        (second, to(open)),
        (walled, Reply::cloudflare()),
        (open, Reply::page("A free article", &[])),
    ])
    .await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            max_tier: "warm".into(),
            browser_path: no_browser(),
            ..cfg()
        },
        None,
    );
    let v = server.fetch_json(params(&site, first)).await.value;
    assert!(v["blocked_reason"].is_string(), "{v}");

    let v = server.fetch_json(params(&site, second)).await.value;
    assert!(
        v["remembered"].is_null(),
        "the resolver was not what failed: {v}"
    );
    assert_eq!(v["status"], 200, "{v}");
    assert_eq!(site.hits(open), 1);

    // What failed is remembered where it failed.
    let v = server.fetch_json(params(&site, walled)).await.value;
    assert!(v["remembered"].is_object(), "{v}");
}

#[tokio::test]
async fn a_site_that_redirects_within_itself_is_judged_as_before() {
    let (asked, moved) = ("http://one-site.test/old", "http://www.one-site.test/new");
    let site = Site::start(vec![
        (asked, to(moved)),
        (
            moved,
            Reply::html("<html><body><p>Too many requests.</p></body></html>")
                .with_status(429)
                .header("retry-after", "600"),
        ),
    ])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let v = server.fetch_json(params(&site, asked)).await.value;
    assert_eq!(v["status"], 429, "{v}");
    let v = server.fetch_json(params(&site, asked)).await.value;
    assert_eq!(v["blocked_reason"], "cooldown", "{v}");
    assert_eq!(site.hits(asked), 1);
}
