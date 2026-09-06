//! A failed native attempt must still disclose the privacy exposure and retain the earlier page.
mod support;
use support::{Reply, Site};
use svipall_mcp::{server::SvipallServer, tools::WebFetchParams};

#[tokio::test]
#[ignore = "opens a local browser; one fixture renderer stalls until the fetch deadline"]
async fn failed_native_attempt_preserves_the_page_and_reports_exposure() {
    support::isolate();
    let url = "http://native-timeout.test/article";
    // Native headless uses the small default window; emulation uses a desktop viewport. The
    // busy fixture is confined to its renderer and the server closes both browser pools below.
    let html = "<html><head><title>Just a moment...</title></head><body><div id='cf-wrapper'>Checking your browser before accessing the site.</div><script>if(innerWidth < 1024) { while(true) {} }</script></body></html>";
    let site = Site::start(vec![(url, Reply::html(html))]).await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            max_tier: "browser".into(),
            request_min_interval_ms: 1,
            http_engine: "reqwest".into(),
            ..Default::default()
        },
        None,
    );
    assert!(server.pool().available(), "manual test requires a browser");
    let out = server
        .fetch_json(WebFetchParams {
            url: url.into(),
            proxy: Some(site.url("")),
            timeout: Some(12000),
            cache: Some("bypass".into()),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await
        .value;
    server.shutdown_configuration().await;
    let stored: std::collections::HashMap<String, String> = serde_json::from_str(
        &std::fs::read_to_string(svipall_core::config::home_dir().join("automatic_routes.json"))
            .unwrap(),
    )
    .unwrap();
    assert!(
        stored.values().any(
            |rows| serde_json::from_str::<Vec<svipall_core::automatic::Sample>>(rows)
                .unwrap()
                .iter()
                .any(|row| row.tier == "native:browser" && row.failures > 0.0)
        ),
        "the timeout must train the route backoff, not just report a failure"
    );
    assert_eq!(out["identity_used"], "emulated", "{out}");
    assert_eq!(out["native_fallback"], true, "{out}");
    assert!(out["privacy_notice"].is_string(), "{out}");
    assert!(
        out["content"]
            .as_str()
            .unwrap()
            .contains("Checking your browser"),
        "{out}"
    );
    assert!(
        out["stopped_reason"]
            .as_str()
            .unwrap()
            .starts_with("timeout"),
        "{out}"
    );
}
