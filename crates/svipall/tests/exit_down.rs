//! An exit that cannot be reached is the exit's fault, not the site's: it must not cost the exit
//! its standing on that domain, and the next fetch must leave through another one.
mod support;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

/// A loopback port nothing listens on: bound, then released.
async fn dead_proxy() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}")
}

fn server(engine: &str) -> SvipallServer {
    support::isolate();
    SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 8,
            request_min_interval_ms: 1,
            http_engine: engine.into(),
            max_tier: "stealth".into(),
            ..Default::default()
        },
        None,
    )
}

fn params(url: &str) -> WebFetchParams {
    WebFetchParams {
        url: url.into(),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

const ARTICLE: &str = "<main><h1>Exit report</h1><p>This report contains the requested observations, their context, and an explanation of the method, with enough substantive detail to inspect the result without relying on a title or an empty placeholder.</p></main>";

async fn a_dead_exit_is_passed_over_without_costing_it_anything(engine: &str, domain: &str) {
    let url = format!("http://{domain}/article");
    let site = Site::start(vec![(url.as_str(), Reply::html(ARTICLE))]).await;
    let dead = dead_proxy().await;
    let alive = site.url("");
    let alive = alive.trim_end_matches('/').to_string();
    svipall_core::exits::set_pool(domain, &[dead.clone(), alive.clone()]);
    let server = server(engine);

    let first = server.fetch_json(params(&url)).await.value;
    assert_eq!(first["blocked_reason"], "exit_down", "{first}");
    assert!(
        first["note"].as_str().unwrap_or("").contains(&dead),
        "the note names the exit that could not be reached: {first}"
    );
    let tiers_tried = first["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| !a.as_str().unwrap_or("").starts_with("exit_down"))
        .count();
    assert_eq!(
        tiers_tried, 1,
        "a dead exit is not climbed through every tier: {first}"
    );
    let status = svipall_core::exits::status();
    assert!(
        status["down"][&dead].is_object(),
        "the exit is reported down: {status}"
    );
    assert!(
        status["by_domain"][domain][&dead].is_null(),
        "an unreachable exit is not charged against the domain: {status}"
    );

    let second = server.fetch_json(params(&url)).await.value;
    assert_eq!(second["status"], 200, "{second}");
    assert!(second["content"]
        .as_str()
        .unwrap_or("")
        .contains("requested observations"));
}

#[tokio::test]
async fn a_dead_exit_is_passed_over_on_the_reqwest_engine() {
    a_dead_exit_is_passed_over_without_costing_it_anything("reqwest", "exit-down-reqwest.test")
        .await;
}

#[tokio::test]
async fn a_dead_exit_is_passed_over_on_the_impersonating_engine() {
    a_dead_exit_is_passed_over_without_costing_it_anything(
        "impersonate",
        "exit-down-impersonate.test",
    )
    .await;
}

#[tokio::test]
async fn a_407_is_the_exit_asking_for_credentials_not_the_site_refusing() {
    let domain = "exit-auth.test";
    let url = format!("http://{domain}/article");
    let site = Site::start(vec![(
        url.as_str(),
        Reply::plain("Proxy Authentication Required").with_status(407),
    )])
    .await;
    let other = dead_proxy().await;
    let exit = site.url("").trim_end_matches('/').to_string();
    svipall_core::exits::set_pool(domain, &[exit.clone(), other]);
    let out = server("reqwest").fetch_json(params(&url)).await.value;
    assert_eq!(out["blocked_reason"], "exit_down", "{out}");
    let status = svipall_core::exits::status();
    assert!(status["down"][&exit].is_object(), "{status}");
    assert!(
        status["by_domain"][domain][&exit].is_null(),
        "a 407 is not a block by the site: {status}"
    );
}
