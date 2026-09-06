//! Local proxy fixtures exercise public-domain policy without contacting external sites.
mod support;
use support::{Reply, Site};
use svipall_mcp::{server::SvipallServer, tools::WebFetchParams};

fn cfg() -> svipall_core::Config {
    support::isolate();
    svipall_core::Config {
        request_limit: 2,
        request_min_interval_ms: 1,
        http_engine: "reqwest".into(),
        max_tier: "http".into(),
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

#[tokio::test]
async fn a_visit_limit_keeps_unrequested_crawl_pages_pending() {
    let seed = "http://crawl-limit.test/seed";
    let root = "http://crawl-limit.test/";
    let site = Site::start(vec![
        (seed, Reply::page("Seed", &[])),
        (root, Reply::page("Root", &[])),
    ])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    for _ in 0..2 {
        assert_eq!(
            server.fetch_json(params(&site, seed)).await.value["status"],
            200
        );
    }
    svipall_core::store::ROUTES.insert("crawl-limit.test", &site.url(""));
    let out = server
        .crawl_json(svipall_mcp::tools::WebCrawlParams {
            url: root.into(),
            max_pages: Some(1),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(out["stopped_by"], "cooldown", "{out}");
    assert_eq!(out["refused_without_asking"], 1, "{out}");
    assert_eq!(site.hits(root), 0);
    svipall_core::store::ROUTES.remove("crawl-limit.test");
}

#[tokio::test]
async fn forced_mode_and_identity_changes_cannot_reset_visit_limit() {
    let url = "http://limit.test/article";
    let site = Site::start(vec![(url, Reply::page("Useful result", &[]))]).await;
    let config = cfg();
    let server = SvipallServer::new(None, config.clone(), None);
    for _ in 0..2 {
        let out = server.fetch_json(params(&site, url)).await.value;
        assert_eq!(out["status"], 200, "{out}");
    }
    let native = SvipallServer::new(
        None,
        svipall_core::Config {
            browser_identity: "native".into(),
            ..config
        },
        None,
    );
    let mut p = params(&site, url);
    p.mode = Some("http".into());
    let out = native.fetch_json(p).await.value;
    assert_eq!(out["wall_kind"], "cooldown", "{out}");
    assert!(out["cooldown_seconds_left"].as_u64().unwrap() > 0);
    assert_eq!(site.hits(url), 2);
}

#[tokio::test]
async fn retry_after_stops_escalation_and_survives_a_new_server() {
    let url = "http://backoff.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("Please wait")
            .with_status(429)
            .header("retry-after", "3600"),
    )])
    .await;
    let config = svipall_core::Config {
        max_tier: "warm".into(),
        ..cfg()
    };
    let server = SvipallServer::new(None, config.clone(), None);
    let out = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["status"], 429, "{out}");
    assert_eq!(out["identity_used"], "emulated");
    assert_eq!(out["cooldown_seconds_left"], 3600);
    assert!(out["content"].as_str().unwrap().contains("Please wait"));
    let other = SvipallServer::new(None, config, None);
    let out = other.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["wall_kind"], "cooldown", "{out}");
    assert_eq!(site.hits(url), 1);
}

#[tokio::test]
async fn attempt_cap_returns_the_page_and_never_opens_native() {
    let url = "http://attempt.test/article";
    let site = Site::start(vec![(url, Reply::cloudflare())]).await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            auto_max_attempts: 1,
            max_tier: "warm".into(),
            ..cfg()
        },
        None,
    );
    let out = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["status"], 403, "{out}");
    assert_eq!(out["native_fallback"], false);
    assert!(out["stopped_reason"]
        .as_str()
        .unwrap()
        .starts_with("attempt_limit"));
    assert!(!out["content"].as_str().unwrap().is_empty());
    assert_eq!(site.hits(url), 1);
}

#[tokio::test]
async fn short_success_never_triggers_privacy_fallback() {
    let url = "http://short.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("<html><body><main><p>Short useful answer.</p></main></body></html>"),
    )])
    .await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            max_tier: "warm".into(),
            request_limit: 4,
            ..cfg()
        },
        None,
    );
    for _ in 0..3 {
        let out = server.fetch_json(params(&site, url)).await.value;
        assert_eq!(out["status"], 200, "{out}");
        assert_eq!(out["tier_used"], "http", "{out}");
        assert_eq!(out["identity_used"], "emulated");
        assert_eq!(out["native_fallback"], false);
    }
    assert_eq!(site.hits(url), 3);
}

#[tokio::test]
#[ignore = "launches a real local browser against a loopback proxy"]
async fn native_is_last_and_an_opt_out_prevents_its_launch() {
    use svipall_mcp::browser::{BrowserPool, BrowserTier, PageOpts};
    let config = svipall_core::Config {
        request_limit: 12,
        max_tier: "browser".into(),
        ..cfg()
    };
    let native = BrowserPool::new(svipall_core::Config {
        browser_identity: "native".into(),
        ..config.clone()
    });
    assert!(native.available(), "this manual test requires a browser");
    let opts = PageOpts {
        tier: BrowserTier::Browser,
        profile_dir: None,
        proxy: None,
        visible: false,
        mobile: false,
        identity_seed: Some(984),
    };
    let (_, page) = native.page(&opts).await.unwrap();
    let width: u32 = page
        .evaluate("innerWidth")
        .await
        .unwrap()
        .into_value()
        .unwrap();
    native.shutdown().await;
    assert!(
        width < 1024,
        "fixture requires the browser's default small headless window"
    );
    let article = "<main><h1>Local native delivery</h1><p>This detailed report explains the experiment and its results. The browser delivered the requested document with useful information about several independent observations, including their context and the reasoning behind the conclusions. Readers can inspect the content, understand how it was collected, and decide whether the evidence answers their original question.</p></main>";
    let wall = format!("<html><head><title>Just a moment...</title></head><body><div id='cf-wrapper'>Checking your browser before accessing the site.</div><script>if(innerWidth === {width}) {{ document.title='Local native delivery'; document.body.innerHTML={}; }}</script></body></html>", serde_json::to_string(article).unwrap());
    for enabled in [true, false] {
        let url = if enabled {
            "http://native-last.test/article"
        } else {
            "http://native-off.test/article"
        };
        let site = Site::start(vec![(url, Reply::html(&wall))]).await;
        let server = SvipallServer::new(
            None,
            svipall_core::Config {
                auto_native_fallback: enabled,
                ..config.clone()
            },
            None,
        );
        let mut p = params(&site, url);
        p.timeout = Some(45000);
        let out = server.fetch_json(p).await.value;
        server.shutdown_configuration().await;
        if enabled {
            assert_eq!(out["identity_used"], "native", "{out}");
            assert_eq!(out["native_fallback"], true, "{out}");
            assert!(
                out["content"]
                    .as_str()
                    .unwrap()
                    .contains("Local native delivery"),
                "{out}"
            );
            assert!(out["privacy_notice"].is_string());
            let attempts = out["attempts"].as_array().unwrap();
            assert!(attempts[0].as_str().unwrap().starts_with("http:"), "{out}");
            assert!(
                attempts[1].as_str().unwrap().starts_with("browser:"),
                "{out}"
            );
            assert!(
                attempts[2].as_str().unwrap().starts_with("native:"),
                "{out}"
            );
        } else {
            assert_eq!(out["identity_used"], "emulated", "{out}");
            assert_eq!(out["native_fallback"], false, "{out}");
        }
    }
}
