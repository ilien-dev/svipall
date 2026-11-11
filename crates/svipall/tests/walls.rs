//! What a browser tier reports when the page behind a challenge is a server's error stub.
//!
//! Measured live on a managed challenge: the last tier came back with a 27-character
//! "403 Forbidden" document, its status was rewritten to 200 because nothing in it read as a wall,
//! and the fetch reported success. A status is only overruled by a page with text to show for it.
mod support;
use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_403_stub_at_a_browser_tier_is_not_reported_as_a_page() {
    support::isolate();
    let site = Site::start(vec![(
        "/",
        Reply::html("<html><head><title>403 Forbidden</title></head><body><h1>403 Forbidden</h1></body></html>")
            .with_status(403),
    )])
    .await;
    let s = SvipallServer::new(None, svipall_core::Config::default(), None);
    let out = s
        .fetch_json(WebFetchParams {
            url: site.url("/"),
            mode: Some("browser".into()),
            cache: Some("bypass".into()),
            ..Default::default()
        })
        .await
        .value;
    eprintln!("{out}");
    assert_ne!(out["status"], 200, "the stub is not a page: {out}");
}
