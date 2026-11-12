//! `web_map` reports what the site serves: its links even when the page is cached, a robots file
//! only when there is one, its `llms.txt`, and only the sources that produced something.
//!
//! The site is reached through the fixture acting as its exit, under a name that is not loopback:
//! a loopback URL is never cached, and the cache is half of what is being tested.
mod support;
use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::{WebFetchParams, WebMapParams};

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 100,
            request_min_interval_ms: 1,
            max_tier: "http".into(),
            ..Default::default()
        },
        None,
    )
}

#[tokio::test]
async fn a_cached_home_page_still_gives_its_links_and_a_404_is_no_robots_file() {
    support::isolate();
    let domain = "map-cached.test";
    let home = format!("http://{domain}/");
    let llms = format!("http://{domain}/llms.txt");
    let site = Site::start(vec![
        (
            home.as_str(),
            Reply::page("Home", &["/about", "/pricing"]).header("Cache-Control", "max-age=3600"),
        ),
        (
            llms.as_str(),
            Reply::plain(
                "# Map test\n\n> A site.\n\n## Docs\n- [Guide](/docs/guide): start\n\
                 - [Reference](http://map-cached.test/docs/reference)\n",
            ),
        ),
        // Nothing at /robots.txt: the fixture answers 404 with an HTML page.
    ])
    .await;
    svipall_core::exits::set_pool(domain, &[site.url("").trim_end_matches('/').to_string()]);
    let s = server();

    // The page is read once by somebody else first, so the map's own visit is a cache hit.
    let first = s
        .fetch_json(WebFetchParams {
            url: home.clone(),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await
        .value;
    assert_eq!(first["status"], 200, "{first}");
    let again = s
        .fetch_json(WebFetchParams {
            url: home.clone(),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await
        .value;
    assert_eq!(again["from_cache"], true, "the page is cached: {again}");

    let map = s
        .map_json(WebMapParams {
            url: home.clone(),
            sources: None,
            limit: None,
            include: None,
        })
        .await
        .unwrap();
    let urls: Vec<(String, String)> = map["urls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["url"].as_str().unwrap_or_default().to_string(),
                u["source"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let has = |path: &str, source: &str| urls.iter().any(|(u, s)| u.ends_with(path) && s == source);
    assert!(
        has("/about", "links"),
        "links come back on a cache hit: {map}"
    );
    assert!(has("/pricing", "links"), "{map}");
    assert!(has("/docs/guide", "llms"), "llms.txt is read: {map}");
    assert!(has("/docs/reference", "llms"), "{map}");
    assert_eq!(
        map["robots"]["present"], false,
        "a 404 page is not a robots file: {map}"
    );
    let used: Vec<&str> = map["sources_used"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        used,
        ["llms", "links"],
        "only sources that produced URLs: {map}"
    );
}

#[tokio::test]
async fn an_html_shell_at_robots_txt_is_not_a_robots_file_and_empty_sources_are_not_listed() {
    support::isolate();
    let domain = "map-shell.test";
    let home = format!("http://{domain}/");
    let robots = format!("http://{domain}/robots.txt");
    let shell = "<!doctype html><html><head><title>App</title></head><body><div id=root></div>\
                 </body></html>";
    let site = Site::start(vec![
        (home.as_str(), Reply::page("Home", &["/one"])),
        // A single-page site answers every path with its shell, and a 200.
        (robots.as_str(), Reply::html(shell)),
    ])
    .await;
    svipall_core::exits::set_pool(domain, &[site.url("").trim_end_matches('/').to_string()]);
    let map = server()
        .map_json(WebMapParams {
            url: home.clone(),
            sources: None,
            limit: None,
            include: None,
        })
        .await
        .unwrap();
    assert_eq!(map["robots"]["present"], false, "{map}");
    assert_eq!(map["sources_used"], serde_json::json!(["links"]), "{map}");
}

/// A navigation drawn by a script has no links in the markup; the map renders the page once.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn links_drawn_by_a_script_are_read_from_a_rendered_visit() {
    let page = "<!doctype html><html><head><title>Script nav</title></head><body><main>\
                <h1>Script nav</h1><p>The navigation of this page is written by a script after \
                load, so the markup the server sends carries no links at all.</p></main>\
                <nav id=n></nav><script>for (const p of ['/x','/y']) { const a = \
                document.createElement('a'); a.href = p; a.textContent = p; \
                document.getElementById('n').appendChild(a); }</script></body></html>";
    let site = Site::start(vec![("/", Reply::html(page))]).await;
    support::isolate();
    let map = SvipallServer::new(None, svipall_core::Config::default(), None)
        .map_json(WebMapParams {
            url: site.url("/"),
            sources: Some(vec!["links".into()]),
            limit: None,
            include: None,
        })
        .await
        .unwrap();
    eprintln!("{map}");
    let text = map.to_string();
    assert!(text.contains("/x") && text.contains("/y"), "{map}");
}

#[tokio::test]
async fn a_site_with_no_sitemap_still_gives_its_links_and_keeps_visits_to_read_them() {
    // Found on a real site with no robots.txt, sitemap, llms.txt or feed: the map tried all
    // twelve conventional locations, which is the whole visit window of a host, so the thirteenth
    // visit — the home page, the one source that had anything — was refused. The answer was zero
    // URLs with nothing said, and nothing could be read from that host for a minute.
    support::isolate();
    let domain = "map-bare.test";
    let home = format!("http://{domain}/");
    let about = format!("http://{domain}/about");
    let site = Site::start(vec![
        (home.as_str(), Reply::page("Home", &["/about", "/pricing"])),
        (about.as_str(), Reply::page("About", &[])),
    ])
    .await;
    svipall_core::exits::set_pool(domain, &[site.url("").trim_end_matches('/').to_string()]);
    // The limit every installation has by default, not the hundred the tests above raise it to.
    let s = SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            max_tier: "http".into(),
            ..Default::default()
        },
        None,
    );
    let map = s
        .map_json(WebMapParams {
            url: home.clone(),
            sources: None,
            limit: None,
            include: None,
        })
        .await
        .unwrap();
    assert_eq!(map["sources_used"], serde_json::json!(["links"]), "{map}");
    assert_eq!(map["count"], 2, "{map}");
    assert!(
        map["note"]
            .as_str()
            .is_some_and(|n| n.contains("were not tried") && n.contains("sources")),
        "the locations left untried are named: {map}"
    );
    let page = s
        .fetch_json(WebFetchParams {
            url: about.clone(),
            wait: Some(0),
            ..Default::default()
        })
        .await
        .value;
    assert_eq!(
        page["status"], 200,
        "a page the map found can be read straight away: {page}"
    );

    // Asked for by name, every location of that source is tried: the caller chose the cost.
    let feeds = s
        .map_json(WebMapParams {
            url: home.clone(),
            sources: Some(vec!["feeds".into()]),
            limit: None,
            include: None,
        })
        .await
        .unwrap();
    assert!(feeds["note"].is_null(), "{feeds}");
}
