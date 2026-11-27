//! A route that serves data is paced as data: more visits in the window and a smaller charge
//! against the address, learned from what the route answered and unlearned when it refuses.
//!
//! Every host is a public-looking name routed through a loopback fixture, as in `admission.rs`,
//! and every test has a host of its own: the ledger and the lanes are shared by the binary.
mod support;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

fn cfg() -> svipall_core::Config {
    support::isolate();
    svipall_core::Config {
        request_limit: 2,
        request_window_seconds: 30,
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
        // Never wait for a slot: a visit that does not fit is refused, at once.
        wait: Some(0),
        ..Default::default()
    }
}

fn json(body: &str) -> Reply {
    Reply::bytes(body.as_bytes(), "application/json")
}

#[tokio::test]
async fn a_route_that_answers_with_data_is_not_stopped_at_the_page_limit() {
    let base = "http://registry-lane.test/pkg";
    let urls: Vec<String> = (0..8).map(|i| format!("{base}/{i}")).collect();
    let site = Site::start(
        urls.iter()
            .map(|u| {
                (
                    u.as_str(),
                    json(r#"{"name":"left-pad","versions":["1.0.0","1.1.0"]}"#),
                )
            })
            .collect(),
    )
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    // Two is the page limit; every one of these is past it from the third on.
    for u in &urls {
        let v = server.fetch_json(params(&site, u)).await.value;
        assert_eq!(v["status"], 200, "{u}: {v}");
        assert!(v.get("blocked_reason").is_none(), "{u}: {v}");
    }
    // The data lane has a limit too: five times the page's.
    let limit = svipall_core::lane::Lane::Data.limit(&cfg()) as usize;
    let more: Vec<String> = (0..limit).map(|i| format!("{base}/{}", i % 8)).collect();
    let mut refused = None;
    for u in &more {
        let v = server.fetch_json(params(&site, u)).await.value;
        if v["blocked_reason"] == "visit_limit" {
            refused = Some(v);
            break;
        }
    }
    let refused = refused.expect("the data window is finite");
    let note = refused["note"].as_str().unwrap_or_default();
    assert!(
        note.contains(&format!("{limit} visits per 30s")),
        "the refusal names the limit that applied: {refused}"
    );
}

#[tokio::test]
async fn a_page_keeps_the_limit_it_always_had() {
    let url = "http://page-lane.test/article";
    let site = Site::start(vec![(url, Reply::page("Article", &[]))]).await;
    let server = SvipallServer::new(None, cfg(), None);
    for _ in 0..2 {
        let v = server.fetch_json(params(&site, url)).await.value;
        assert_eq!(v["status"], 200, "{v}");
    }
    let third = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(third["blocked_reason"], "visit_limit", "{third}");
    assert_eq!(site.hits(url), 2);
}

#[tokio::test]
async fn a_data_route_that_refuses_is_paced_as_a_page_again() {
    let url = "http://turned-lane.test/v1/items";
    let site = Site::start(vec![(
        url,
        json(r#"{"items":[1,2,3]}"#).then(json(r#"{"error":"forbidden"}"#).with_status(403)),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let first = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(first["status"], 200, "{first}");
    assert_eq!(
        svipall_core::lane::of(url, Some(&site.url(""))),
        svipall_core::lane::Lane::Data
    );
    let second = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(second["status"], 403, "{second}");
    assert_eq!(
        svipall_core::lane::of(url, Some(&site.url(""))),
        svipall_core::lane::Lane::Page,
        "a route that refuses judges who asks"
    );
}

#[tokio::test]
async fn a_visit_to_a_data_route_costs_the_address_a_fraction() {
    let data = "http://cheap-lane.test/api/thing";
    let page = "http://dear-lane.test/article";
    let site = Site::start(vec![
        (data, json(r#"{"ok":true}"#)),
        (page, Reply::page("Article", &[])),
    ])
    .await;
    let exit = site.url("");
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 50,
            ..cfg()
        },
        None,
    );
    // The first visit to each is a visit to a route nobody has seen: a page, at a page's price.
    for u in [data, page] {
        let v = server.fetch_json(params(&site, u)).await.value;
        assert_eq!(v["status"], 200, "{v}");
    }
    let spent = |d: &str| svipall_core::reputation::spent(d, Some(&exit));
    let (data_before, page_before) = (spent("cheap-lane.test"), spent("dear-lane.test"));
    for _ in 0..10 {
        for u in [data, page] {
            let v = server.fetch_json(params(&site, u)).await.value;
            assert_eq!(v["status"], 200, "{v}");
        }
    }
    let data_cost = spent("cheap-lane.test") - data_before;
    let page_cost = spent("dear-lane.test") - page_before;
    assert!(page_cost > 9.0, "ten page visits at one each: {page_cost}");
    assert!(
        data_cost < page_cost * 0.2,
        "ten data visits cost {data_cost}, ten page visits {page_cost}"
    );
}

#[tokio::test]
async fn reads_that_start_together_on_a_route_nobody_has_seen_do_not_wait_out_a_pages_window() {
    let base = "http://burst-lane.test/v1";
    let urls: Vec<String> = (0..8).map(|i| format!("{base}/{i}")).collect();
    let site = Site::start(
        urls.iter()
            .map(|u| (u.as_str(), json(r#"{"suggestions":["a","b"]}"#)))
            .collect(),
    )
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let started = std::time::Instant::now();
    let reads = urls.iter().map(|u| {
        let (server, site) = (&server, &site);
        async move {
            let mut p = params(site, u);
            // Long enough for a page's window to free a slot, which is what must not be needed.
            p.wait = Some(60);
            server.fetch_json(p).await.value
        }
    });
    for v in futures::future::join_all(reads).await {
        assert_eq!(v["status"], 200, "{v}");
    }
    // Two were admitted as pages; the other six learned the lane from the first answer.
    assert!(
        started.elapsed().as_secs() < 15,
        "waited {}s, which is a page's window",
        started.elapsed().as_secs()
    );
}
