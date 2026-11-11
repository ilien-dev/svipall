//! `web_search` leaves through the exit `web_route` set for the engine's host, like every other
//! request. Its own binary because routes are process-wide state.
mod support;

use support::{Reply, Site};
use svipall::search::Endpoints;
use svipall::server::SvipallServer;
use svipall::tools::{WebRouteParams, WebSearchParams};

#[tokio::test]
async fn a_search_goes_out_through_the_exit_routed_for_the_engine_host() {
    support::isolate();
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/search/ddg-html.results.html"),
    )
    .unwrap();
    // A plain HTTP proxy receives the absolute URL as the request target, so a fixture server
    // keyed by that URL is a proxy that knows one page. `engine.invalid` resolves nowhere: a
    // result can only have come through the exit.
    let proxy = Site::start(vec![("http://engine.invalid/html/", Reply::html(&page))]).await;
    let s = SvipallServer::with_store(None, svipall_core::Config::default(), None, None)
        .with_search_endpoints(
            Endpoints::default().with("ddg-html", "http://engine.invalid/html/"),
        );
    let search = || WebSearchParams {
        query: "rust async runtime".into(),
        engine: Some("ddg-html".into()),
        ..Default::default()
    };

    let direct = s.search_json(search()).await;
    assert_eq!(direct["count"], 0, "no route, no proxy: {direct}");

    s.route_json(WebRouteParams {
        domain: Some("engine.invalid".into()),
        proxy: Some(proxy.url("")),
        ..Default::default()
    })
    .await
    .unwrap();
    let routed = s.search_json(search()).await;
    assert!(
        routed["count"].as_u64().unwrap_or(0) > 0,
        "the search ignored the route: {routed}"
    );
    assert_eq!(proxy.hits("http://engine.invalid/html/"), 1, "{routed}");
}
