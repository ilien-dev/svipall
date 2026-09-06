//! Controlled learning measurement; every network connection terminates on loopback.
mod support;
use support::{Reply, Site};
use svipall_mcp::{server::SvipallServer, tools::WebFetchParams};

#[tokio::test]
#[ignore = "measures three visits using a real local browser"]
async fn two_useful_browser_observations_skip_failed_http_on_the_third_visit() {
    support::isolate();
    let url = "http://learn-local.test/reports/article";
    let article = "<main><h1>Local learning report</h1><p>This detailed report explains the experiment and its results. The browser delivered the requested document with useful information about several independent observations, including their context and the reasoning behind the conclusions. Readers can inspect the content, understand how it was collected, and decide whether the evidence answers their original question. There are enough concrete observations here to support a meaningful assessment of the experiment and the circumstances in which its results apply.</p></main>";
    // A substantial local document keeps host-injected browser markup from dominating the
    // text/HTML ratio. This fixture measures routing, not extraction accuracy.
    let article = article.repeat(64);
    let html = format!("<html><head><title>Just a moment...</title></head><body><div id='cf-wrapper'>Checking your browser before accessing the site.</div><script>document.title='Local learning report';document.body.innerHTML={};</script></body></html>", serde_json::to_string(&article).unwrap());
    let site = Site::start(vec![(url, Reply::html(&html))]).await;
    let cfg = svipall_core::Config {
        max_tier: "browser".into(),
        request_min_interval_ms: 1,
        http_engine: "reqwest".into(),
        ..Default::default()
    };
    let server = SvipallServer::new(None, cfg, None);
    assert!(server.pool().available(), "manual test requires a browser");
    let mut measurements = Vec::new();
    for round in 0..3 {
        let start = std::time::Instant::now();
        let out = server
            .fetch_json(WebFetchParams {
                url: url.into(),
                proxy: Some(site.url("")),
                cache: Some("bypass".into()),
                robots: Some("ignore".into()),
                ..Default::default()
            })
            .await
            .value;
        assert_eq!(
            out["quality"], "full",
            "reasons={}, chars={}",
            out["quality_reasons"], out["chars"]
        );
        assert_eq!(out["identity_used"], "emulated", "{out}");
        assert_eq!(out["native_fallback"], false, "{out}");
        let attempts = out["attempts"].as_array().unwrap();
        let expected = if round == 2 { 1 } else { 2 };
        assert_eq!(attempts.len(), expected, "{out}");
        measurements.push(serde_json::json!({"visit":round + 1,"attempts":attempts,"elapsed_ms":start.elapsed().as_millis()}));
    }
    server.shutdown_configuration().await;
    println!(
        "{}",
        serde_json::json!({"measurement":"local route learning; no public-site success claim","visits":measurements})
    );
}
