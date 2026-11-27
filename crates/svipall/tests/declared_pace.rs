//! The pace a host declares in its headers is kept, and a read that met the host's limit is
//! finished after the pause the host asked for instead of being handed back as a refusal.
//!
//! Hosts are public-looking names routed through a loopback fixture, one per test.
mod support;
use std::time::Instant;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

fn cfg() -> svipall_core::Config {
    support::isolate();
    svipall_core::Config {
        request_limit: 20,
        request_min_interval_ms: 1,
        http_engine: "reqwest".into(),
        max_tier: "http".into(),
        auto_native_fallback: false,
        ..Default::default()
    }
}

fn params(site: &Site, url: &str) -> WebFetchParams {
    WebFetchParams {
        url: url.into(),
        proxy: Some(site.url("")),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

fn json(body: &str) -> Reply {
    Reply::bytes(body.as_bytes(), "application/json")
}

#[tokio::test]
async fn a_host_that_names_its_pace_is_asked_at_that_pace() {
    let url = "http://named-pace.test/works";
    let site = Site::start(vec![(
        url,
        json(r#"{"items":[]}"#)
            .header("x-rate-limit-limit", "1")
            .header("x-rate-limit-interval", "2s"),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let first = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(first["status"], 200, "{first}");
    let pace = svipall_core::lane::declared(url, Some(&site.url(""))).expect("kept");
    assert_eq!((pace.gap_ms, pace.alone), (2000, false));

    let started = Instant::now();
    for _ in 0..2 {
        let v = server.fetch_json(params(&site, url)).await.value;
        assert_eq!(v["status"], 200, "{v}");
    }
    // Two more reads, each two seconds after the one before it.
    assert!(
        started.elapsed().as_millis() >= 3500,
        "two reads in {}ms against a declared two seconds each",
        started.elapsed().as_millis()
    );
}

#[tokio::test]
async fn a_read_refused_by_a_data_routes_limit_is_finished_after_the_pause() {
    let known = "http://paused-api.test/v1/known";
    let limited = "http://paused-api.test/v1/limited";
    let site = Site::start(vec![
        (known, json(r#"{"ok":true}"#)),
        (
            limited,
            json(r#"{"message":"too many requests"}"#)
                .with_status(429)
                .then(json(r#"{"title":"The answer"}"#)),
        ),
    ])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    // The route family has shown what it serves.
    let v = server.fetch_json(params(&site, known)).await.value;
    assert_eq!(v["status"], 200, "{v}");

    let started = Instant::now();
    let v = server.fetch_json(params(&site, limited)).await.value;
    assert_eq!(v["status"], 200, "{v}");
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert!(
        v["content"]
            .as_str()
            .unwrap_or_default()
            .contains("The answer"),
        "{v}"
    );
    assert_eq!(site.hits(limited), 2);
    let secs = started.elapsed().as_secs();
    assert!(
        (4..=12).contains(&secs),
        "paused {secs}s, not the five asked"
    );
    let attempts = v["attempts"].to_string();
    assert!(
        attempts.contains("429") && attempts.contains("waited"),
        "{v}"
    );
}

#[tokio::test]
async fn a_named_retry_after_is_waited_out_on_a_page_too() {
    let url = "http://polite-page.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("<html><body><p>Slow down.</p></body></html>")
            .with_status(429)
            .header("retry-after", "2")
            .then(Reply::page("The article", &[])),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let started = Instant::now();
    let v = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(v["status"], 200, "{v}");
    assert_eq!(v["title"], "The article", "{v}");
    assert_eq!(site.hits(url), 2);
    assert!(started.elapsed().as_secs() >= 2, "the pause was not kept");
}

#[tokio::test]
async fn a_limit_that_is_still_there_after_the_pause_is_reported_and_not_asked_again() {
    let url = "http://stubborn-page.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("<html><body><p>Slow down.</p></body></html>")
            .with_status(429)
            .header("retry-after", "1"),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let v = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(v["status"], 429, "{v}");
    assert_eq!(v["blocked_reason"], "http 429", "{v}");
    assert_eq!(site.hits(url), 2, "one pause, one more request, no third");
}

#[tokio::test]
async fn a_pause_that_does_not_fit_the_call_is_not_waited() {
    let url = "http://long-pause.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("<html><body><p>Slow down.</p></body></html>")
            .with_status(429)
            .header("retry-after", "600")
            .then(Reply::page("The article", &[])),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let started = Instant::now();
    let v = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(v["status"], 429, "{v}");
    assert_eq!(site.hits(url), 1);
    assert!(started.elapsed().as_secs() < 5);
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("asked for 600s"),
        "{v}"
    );
}

#[tokio::test]
async fn a_quota_declared_spent_holds_the_next_read_until_it_resets() {
    let url = "http://spent-quota.test/v1/search";
    let site = Site::start(vec![(
        url,
        json(r#"{"results":[1]}"#)
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", "40"),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let first = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(
        first["status"], 200,
        "the answer that came is still the answer: {first}"
    );
    let mut p = params(&site, url);
    p.wait = Some(0);
    let second = server.fetch_json(p).await.value;
    assert_eq!(second["blocked_reason"], "cooldown", "{second}");
    let left = second["cooldown_seconds_left"].as_u64().unwrap_or(0);
    assert!((30..=40).contains(&left), "{second}");
    assert_eq!(
        site.hits(url),
        1,
        "nothing was asked of a host that said it had no more"
    );
}

#[tokio::test]
async fn a_route_nobody_has_seen_that_refuses_in_json_is_paused_as_an_api() {
    let url = "http://shy-api.test/graph/search";
    let site = Site::start(vec![(
        url,
        json(r#"{"message":"Too Many Requests"}"#)
            .with_status(429)
            .then(json(r#"{"total":1,"data":[{"title":"Found"}]}"#)),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let v = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(v["status"], 200, "{v}");
    assert_eq!(site.hits(url), 2);
}
