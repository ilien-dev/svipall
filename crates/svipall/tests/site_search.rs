//! `web_site_search` finds a site's search however the site offers it: declared in an OpenSearch
//! description, as a box the page draws (in a shadow root, behind an icon), or only as the query
//! endpoint its platform answers. And when it finds nothing, it says where it looked.
//!
//! The offline tests run with `max_tier = "http"`, so no browser is involved and the order of the
//! steps that need none is what is tested. The box finder needs a browser and is `#[ignore]`.
mod support;
use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebSiteSearchParams;

fn http_only() -> SvipallServer {
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

fn search(url: String, query: &str) -> WebSiteSearchParams {
    WebSiteSearchParams {
        url,
        query: query.into(),
        fetch: Some(true),
        timeout: Some(20_000),
    }
}

const RESULTS: &str = "<!doctype html><html><head><title>Results</title></head><body><main>\
    <h1>Search results for boots</h1><p>Three pages mention waterproof boots: the winter guide, \
    the care guide and the size chart, each of them with enough text to be a real page.</p>\
    </main></body></html>";

#[tokio::test]
async fn an_opensearch_description_is_used_without_a_browser() {
    let home = "<!doctype html><html><head><title>Docs</title>\
        <link rel=\"search\" type=\"application/opensearchdescription+xml\" \
              title=\"Docs\" href=\"/opensearch.xml\"></head><body><main><h1>Docs</h1>\
        <p>Documentation home with no search box in its markup at all, only the declaration in \
        its head that says how its search is addressed.</p></main></body></html>";
    let description = "<?xml version=\"1.0\"?>\
        <OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\">\
        <ShortName>Docs</ShortName>\
        <Url type=\"text/html\" template=\"/find?term={searchTerms}&amp;page={startPage?}\"/>\
        </OpenSearchDescription>";
    let site = Site::start(vec![
        ("/", Reply::html(home)),
        (
            "/opensearch.xml",
            Reply::bytes(
                description.as_bytes(),
                "application/opensearchdescription+xml",
            ),
        ),
        ("/find", Reply::html(RESULTS)),
    ])
    .await;
    let out = http_only()
        .site_search_json(search(site.url("/"), "boots"))
        .await
        .unwrap();
    assert_eq!(out["found"], true, "{out}");
    assert_eq!(out["source"], "opensearch", "{out}");
    assert_eq!(
        out["results_url"],
        site.url("/find?term=boots"),
        "the template filled, the optional page left out: {out}"
    );
    assert_eq!(out["results"]["status"], 200, "{out}");
    assert!(site.hits("/find") >= 1);
}

#[tokio::test]
async fn a_platform_endpoint_is_tried_by_shape_and_believed_only_when_it_answers() {
    // A page that advertises a `/wp-json/` REST root and has no form. `/?s=boots` serves the
    // home page again, as a site that ignores the parameter does, and must not be believed.
    let home = "<!doctype html><html><head><title>Blog</title>\
        <link rel=\"https://api.w.org/\" href=\"/wp-json/\"></head><body><main><h1>Blog</h1>\
        <p>A blog whose theme draws no search form, so the only way in is the route its \
        platform answers, which the page names in its head.</p></main></body></html>";
    let site = Site::start(vec![
        ("/", Reply::html(home)),
        (
            "/wp-json/wp/v2/search",
            Reply::bytes(
                br#"[{"id":7,"title":"Waterproof boots for winter","url":"/boots"}]"#,
                "application/json",
            ),
        ),
    ])
    .await;
    let out = http_only()
        .site_search_json(search(site.url("/"), "boots"))
        .await
        .unwrap();
    assert_eq!(out["found"], true, "{out}");
    assert_eq!(out["source"], "endpoint", "{out}");
    assert_eq!(
        out["results_url"],
        site.url("/wp-json/wp/v2/search?search=boots"),
        "{out}"
    );
    assert!(
        out["why"].as_str().unwrap_or("").contains("/wp-json/"),
        "{out}"
    );
}

#[tokio::test]
async fn a_site_with_no_search_says_everything_that_was_tried() {
    let home = "<!doctype html><html><head><title>Plain</title></head><body><main><h1>Plain</h1>\
        <p>A page with no form, no declaration and no platform behind it, which therefore has no \
        search that anything could find.</p></main></body></html>";
    let site = Site::start(vec![("/", Reply::html(home))]).await;
    let out = http_only()
        .site_search_json(search(site.url("/"), "boots"))
        .await
        .unwrap();
    assert_eq!(out["found"], false, "{out}");
    let note = out["note"].as_str().unwrap_or_default();
    assert!(note.contains("OpenSearch"), "{note}");
    assert!(note.contains("rendered page"), "{note}");
    assert!(
        note.contains("/search?q=boots"),
        "names the endpoints tried: {note}"
    );
    assert!(note.contains("/?s=boots"), "{note}");
}

fn browser_server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(None, svipall_core::Config::default(), None)
}

/// The box behind a magnifier icon: not in the page until the button is pressed.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_search_box_behind_an_icon_is_opened_and_found() {
    let home = "<!doctype html><html><head><title>Icon</title></head><body>\
        <header><button aria-label=\"Search\" id=open><svg width=20 height=20>\
        <circle cx=9 cy=9 r=7 stroke=black fill=none /></svg></button></header>\
        <main><h1>Icon</h1><p>The search form on this page only exists once the icon in the \
        header has been pressed, the way many publications hide it.</p></main>\
        <script>document.getElementById('open').addEventListener('click', () => {\
          const f = document.createElement('form'); f.action = '/find';\
          const i = document.createElement('input'); i.name = 'q'; i.type = 'search';\
          i.style.width = '240px'; f.appendChild(i); document.body.prepend(f); });</script>\
        </body></html>";
    let site = Site::start(vec![
        ("/", Reply::html(home)),
        ("/find", Reply::html(RESULTS)),
    ])
    .await;
    let out = browser_server()
        .site_search_json(search(site.url("/"), "boots"))
        .await
        .unwrap();
    eprintln!("{out}");
    assert_eq!(out["source"], "form", "{out}");
    assert_eq!(out["results_url"], site.url("/find?q=boots"), "{out}");
    assert!(
        out["opened_by"].as_str().unwrap_or("").contains("search"),
        "{out}"
    );
}

/// The box inside a component's open shadow root.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_search_box_inside_a_shadow_root_is_found() {
    let home = "<!doctype html><html><head><title>Shadow</title></head><body>\
        <site-search></site-search><main><h1>Shadow</h1><p>The search box on this page lives \
        inside a custom element's shadow root, where a query of the document never looks.</p>\
        </main><script>customElements.define('site-search', class extends HTMLElement {\
          constructor() { super(); const r = this.attachShadow({mode: 'open'});\
            r.innerHTML = '<form action=\"/find\"><input type=\"search\" name=\"query\" \
            style=\"width:240px\"></form>'; } });</script></body></html>";
    let site = Site::start(vec![
        ("/", Reply::html(home)),
        ("/find", Reply::html(RESULTS)),
    ])
    .await;
    let out = browser_server()
        .site_search_json(search(site.url("/"), "boots"))
        .await
        .unwrap();
    eprintln!("{out}");
    assert_eq!(out["source"], "form", "{out}");
    assert_eq!(out["results_url"], site.url("/find?query=boots"), "{out}");
    assert_eq!(out["in_shadow_root"], true, "{out}");
}
