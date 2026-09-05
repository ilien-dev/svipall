//! The ladder, against a local site that behaves exactly as each test needs.
//!
//! Everything here pins `max_tier: "http"`. That is not a way to avoid the browser tiers: it is
//! what makes the tests deterministic and fast. What is being checked is the part that decides
//! *whether* to escalate — classification, the cache, robots — and that part runs before any
//! browser would be launched. The browser tiers have their own tests in `stealth.rs`, which need
//! a real browser and are ignored by default.

mod support;

use serde_json::Value;
use support::{Reply, Site};
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::WebFetchParams;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        // In-memory: these tests are about the ladder, not about what the cache keeps on disk.
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    )
}

fn http(url: &str) -> WebFetchParams {
    WebFetchParams {
        url: url.into(),
        max_tier: Some("http".into()),
        ..Default::default()
    }
}

fn text(v: &Value) -> String {
    v.get("content")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_plain_page_is_answered_by_the_http_tier() {
    let site = Site::start(vec![("/", Reply::page("Hello", &[]))]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;

    assert_eq!(out.value["status"], 200);
    assert_eq!(out.value["tier_used"], "http", "{:?}", out.value);
    assert!(text(&out.value).contains("Hello"));
    assert!(
        out.value.get("blocked_reason").is_none(),
        "a plain page is not a wall: {:?}",
        out.value
    );
}

#[tokio::test]
async fn a_cloudflare_interstitial_is_reported_as_a_wall_and_not_as_content() {
    let site = Site::start(vec![("/", Reply::cloudflare())]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;

    assert_eq!(out.value["wall_kind"], "cloudflare", "{:?}", out.value);
    assert!(
        out.value["blocked_reason"].is_string(),
        "the caller has to be told why: {:?}",
        out.value
    );
    // The interstitial's own text is not the page, and passing it off as content is the failure
    // this whole classifier exists to prevent.
    assert!(
        out.value["note"].is_string(),
        "a wall without a next step is not actionable: {:?}",
        out.value
    );
}

#[tokio::test]
async fn a_login_wall_is_named_as_a_login_wall() {
    let site = Site::start(vec![("/", Reply::login_wall())]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;
    assert_eq!(out.value["wall_kind"], "login", "{:?}", out.value);
}

#[tokio::test]
async fn a_404_is_not_treated_as_a_wall() {
    let site = Site::start(vec![]).await;
    let out = server().fetch_json(http(&site.url("/missing"))).await;
    assert_eq!(out.value["status"], 404);
    assert_eq!(out.value["wall_kind"], "notfound", "{:?}", out.value);
}

#[tokio::test]
async fn the_second_fetch_of_a_page_costs_no_request() {
    let site = Site::start(vec![("/", Reply::page("Cached", &[]))]).await;
    let s = server();
    let cached = |url: &str| WebFetchParams {
        cache: Some("auto".into()),
        ..http(url)
    };
    let first = s.fetch_json(cached(&site.url("/"))).await;
    assert!(first.value.get("from_cache").is_none());

    let second = s.fetch_json(cached(&site.url("/"))).await;
    assert_eq!(second.value["from_cache"], true, "{:?}", second.value);
    assert_eq!(site.hits("/"), 1, "the cached read went to the network");
    assert!(text(&second.value).contains("Cached"));
}

/// A dev server changes every time you save a file, so caching it by default would hand back
/// yesterday's page. It takes an explicit `cache:` to opt in â which is what the test above does.
#[tokio::test]
async fn a_local_url_is_not_cached_unless_asked() {
    let site = Site::start(vec![("/", Reply::page("Dev", &[]))]).await;
    let s = server();
    for _ in 0..2 {
        let out = s.fetch_json(http(&site.url("/"))).await;
        assert!(out.value.get("from_cache").is_none(), "{:?}", out.value);
    }
    assert_eq!(site.hits("/"), 2);
}

#[tokio::test]
async fn cache_bypass_really_goes_back_to_the_site() {
    let site = Site::start(vec![("/", Reply::page("Fresh", &[]))]).await;
    let s = server();
    for _ in 0..2 {
        let mut p = http(&site.url("/"));
        p.cache = Some("bypass".into());
        let out = s.fetch_json(p).await;
        assert!(out.value.get("from_cache").is_none());
    }
    assert_eq!(site.hits("/"), 2);
}

#[tokio::test]
async fn robots_obey_refuses_before_spending_a_request() {
    let site = Site::start(vec![
        (
            "/robots.txt",
            Reply::plain("User-agent: *\nDisallow: /private"),
        ),
        ("/private/secret", Reply::page("Secret", &[])),
    ])
    .await;
    let mut p = http(&site.url("/private/secret"));
    p.robots = Some("obey".into());
    let out = server().fetch_json(p).await;

    assert_eq!(out.value["robots_disallowed"], true, "{:?}", out.value);
    assert_eq!(out.value["wall_kind"], "robots");
    assert_eq!(
        site.hits("/private/secret"),
        0,
        "refusing after fetching would defeat the point"
    );
}

#[tokio::test]
async fn robots_warn_fetches_the_page_and_says_so() {
    let site = Site::start(vec![
        (
            "/robots.txt",
            Reply::plain("User-agent: *\nDisallow: /private"),
        ),
        ("/private/secret", Reply::page("Secret", &[])),
    ])
    .await;
    let s = server();
    // Warn annotates from what is already known, so the origin's robots.txt has to have been read
    // once. That is exactly the cheap behaviour being asserted: it never fetches it itself.
    let mut probe = http(&site.url("/private/other"));
    probe.robots = Some("obey".into());
    let _ = s.fetch_json(probe).await;

    let out = s.fetch_json(http(&site.url("/private/secret"))).await;
    assert_eq!(out.value["robots_disallowed"], true, "{:?}", out.value);
    assert!(
        text(&out.value).contains("Secret"),
        "warn still returns the page"
    );
}

#[tokio::test]
async fn a_url_robots_allows_carries_no_warning() {
    let site = Site::start(vec![
        (
            "/robots.txt",
            Reply::plain("User-agent: *\nDisallow: /private"),
        ),
        ("/public", Reply::page("Public", &[])),
    ])
    .await;
    let mut p = http(&site.url("/public"));
    p.robots = Some("obey".into());
    let out = server().fetch_json(p).await;
    assert!(
        out.value.get("robots_disallowed").is_none(),
        "{:?}",
        out.value
    );
    assert!(text(&out.value).contains("Public"));
}

#[tokio::test]
async fn the_test_suite_never_writes_to_the_real_svipall_home() {
    let dir = support::isolate();
    assert_eq!(svipall_core::config::home_dir(), dir);
}

/// A `200` with a page of navigation and no article is not a wall — the classifier is right about
/// that — but it is also not what the caller asked for. The pacer has to hear about it, or svipall
/// keeps hammering a domain that has quietly stopped answering.
#[tokio::test]
async fn a_deep_request_that_lands_on_the_front_page_is_reported() {
    use svipall_core::session::{Response, Verdict};
    let deep = Response {
        status: 200,
        text_len: 40_000,
        requested_path: "/products/12345",
        final_path: "/",
        wall: false,
        elapsed_ms: 120,
        typical_ms: 120,
    };
    assert_eq!(Verdict::of(&deep), Verdict::Blocked);
}

#[tokio::test]
async fn an_ordinary_page_is_not_mistaken_for_a_block() {
    // The rule above must not fire on the healthy case it resembles, or every fetch backs off.
    let site = Site::start(vec![("/", Reply::page("Fine", &[]))]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;
    assert_eq!(out.value["status"], 200);
    assert!(out.value.get("blocked_reason").is_none(), "{:?}", out.value);
}

/// The path costs twenty tokens; the content it replaces can cost forty thousand.
#[tokio::test]
async fn asking_for_a_file_returns_a_path_instead_of_the_page() {
    let site = Site::start(vec![("/", Reply::page("Saved", &[]))]).await;
    let mut p = http(&site.url("/"));
    p.out_file = Some("ladder-test.md".into());
    let out = server().fetch_json(p).await;

    let path = out.value["out_file"].as_str().expect("a path");
    assert!(out.value.get("content").is_none(), "content came back too");
    let written = std::fs::read_to_string(path).expect("file exists");
    assert!(written.contains("Saved"), "{written}");
    let _ = std::fs::remove_file(path);
}

/// A tool call must not be able to write wherever it likes.
#[tokio::test]
async fn a_relative_path_cannot_climb_out_of_the_output_directory() {
    let site = Site::start(vec![("/", Reply::page("Contained", &[]))]).await;
    let mut p = http(&site.url("/"));
    p.out_file = Some("../../escaped.md".into());
    let out = server().fetch_json(p).await;

    let path = out.value["out_file"].as_str().expect("a path");
    assert!(path.contains("out"), "escaped the output directory: {path}");
    assert!(path.ends_with("escaped.md"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_wall_names_every_widget_on_it_not_just_the_one_with_a_solver_tool() {
    // The table in `svipall_core::widget` is what makes a new widget a row rather than a patch, so the
    // ladder has to read from it. Without this the caller is told "blocked" and left to go and find
    // out what is on the page by hand.
    let wall = Reply::html(
        "<!doctype html><html><head><title>Just a moment...</title></head><body>\
         <div id=\"cf-wrapper\">Checking your browser before accessing the site.</div>\
         <div class=\"frc-captcha\" data-sitekey=\"FCMX1234\"></div>\
         </body></html>",
    )
    .with_status(403)
    .header("cf-mitigated", "challenge");
    let site = Site::start(vec![("/", wall)]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;

    let widgets = out.value["widgets"].as_array().expect("listed");
    assert!(
        widgets
            .iter()
            .any(|w| w["widget"] == "friendlycaptcha.com" && w["modality"] == "nonce"),
        "{widgets:?}"
    );
}

#[tokio::test]
async fn an_ordinary_page_carries_no_widget_list_at_all() {
    // An empty array on every successful fetch is noise in the model's context for no gain.
    let site = Site::start(vec![("/", Reply::page("Home", &[]))]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;
    assert!(out.value.get("widgets").is_none(), "{:?}", out.value);
}

#[tokio::test]
async fn an_origin_the_operator_refused_is_never_requested_at_all() {
    // The point of a policy is that it runs before the request, not after the reply: a rule that
    // only inspects what came back has already made the request it was meant to prevent.
    let site = Site::start(vec![("/", Reply::page("Secret", &[]))]).await;
    let cfg = svipall_core::Config {
        block_origins: vec!["127.0.0.1".into()],
        ..svipall_core::Config::default()
    };
    let server = SvipallServer::with_store(None, cfg, None, None);
    let out = server.fetch_json(http(&site.url("/"))).await;

    assert_eq!(out.value["wall_kind"], "policy", "{:?}", out.value);
    assert!(
        out.value["blocked_reason"]
            .as_str()
            .is_some_and(|r| r.contains("127.0.0.1")),
        "the refusal has to name the rule: {:?}",
        out.value
    );
    assert_eq!(site.hits("/"), 0, "the site was contacted anyway");
}

#[tokio::test]
async fn an_installation_with_no_rules_fetches_exactly_as_before() {
    // The feature is a lever, not a default. Turning it on by accident would break every local
    // fixture and every operator fetching their own machine.
    let site = Site::start(vec![("/", Reply::page("Fine", &[]))]).await;
    let out = server().fetch_json(http(&site.url("/"))).await;
    assert_eq!(out.value["status"], 200, "{:?}", out.value);
}

#[tokio::test]
async fn a_note_survives_the_session_that_wrote_it() {
    // What the tool is for: an agent crawling a site over three days has nowhere else to keep
    // "the last id I saw was 4820", and its own context does not last that long.
    let dir = support::isolate();
    let db = dir.join("notes.db");
    let store = || std::sync::Arc::new(svipall_core::cache::Store::open_at(&db).expect("open"));
    let first =
        SvipallServer::with_store(None, svipall_core::Config::default(), None, Some(store()));
    first
        .notes_json(svipall_mcp::tools::WebNotesParams {
            action: Some("set".into()),
            key: Some("shop/last_id".into()),
            value: Some("4820".into()),
            prefix: None,
        })
        .expect("stored");

    let second =
        SvipallServer::with_store(None, svipall_core::Config::default(), None, Some(store()));
    let got = second
        .notes_json(svipall_mcp::tools::WebNotesParams {
            action: Some("get".into()),
            key: Some("shop/last_id".into()),
            value: None,
            prefix: None,
        })
        .expect("read");
    assert_eq!(got["found"], true, "{got}");
    assert_eq!(got["value"], "4820", "{got}");
}

#[tokio::test]
async fn a_note_nobody_wrote_is_reported_as_absent_rather_than_empty() {
    // "" is something somebody stored; not found is a question nobody has answered.
    let out = server()
        .notes_json(svipall_mcp::tools::WebNotesParams {
            action: Some("get".into()),
            key: Some("never/written".into()),
            value: None,
            prefix: None,
        })
        .expect("read");
    assert_eq!(out["found"], false, "{out}");
    assert!(out.get("value").is_none(), "{out}");
}

#[tokio::test]
async fn every_fetch_leaves_a_line_the_operator_can_read_back() {
    // The ladder used to decide, write a line to stderr and forget, so "which tier is carrying this
    // crawl" was a question the tool could not answer about itself.
    let site = Site::start(vec![
        ("/", Reply::page("Fine", &[])),
        ("/wall", Reply::cloudflare()),
    ])
    .await;
    let store = std::sync::Arc::new(svipall_core::cache::Store::open_memory().expect("db"));
    let server = SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        Some(store.clone()),
    );
    server.fetch_json(http(&site.url("/"))).await;
    server.fetch_json(http(&site.url("/wall"))).await;

    let out = server
        .log_json(svipall_mcp::tools::WebLogParams {
            view: Some("recent".into()),
            domain: None,
            since_secs: Some(3600),
            limit: Some(10),
        })
        .expect("read");
    let lines = out["requests"].as_array().expect("lines");
    assert_eq!(lines.len(), 2, "{out}");
    assert!(
        lines
            .iter()
            .any(|l| l["blocked"] == true && l["wall"] == "cloudflare"),
        "the wall has to be in the record: {out}"
    );
    assert!(
        lines.iter().any(|l| l["blocked"] == false),
        "and so does the page that worked: {out}"
    );
}

#[tokio::test]
async fn a_url_the_policy_refused_is_not_recorded_as_a_request() {
    // Nothing was requested, so counting it would make the summary say a domain is 100% blocked
    // when the tool never contacted it.
    let site = Site::start(vec![("/", Reply::page("Secret", &[]))]).await;
    let store = std::sync::Arc::new(svipall_core::cache::Store::open_memory().expect("db"));
    let cfg = svipall_core::Config {
        block_origins: vec!["127.0.0.1".into()],
        ..svipall_core::Config::default()
    };
    let server = SvipallServer::with_store(None, cfg, None, Some(store.clone()));
    server.fetch_json(http(&site.url("/"))).await;

    assert!(
        store.recent_requests(None, 3600, 10).is_empty(),
        "{:?}",
        store.recent_requests(None, 3600, 10)
    );
}

#[tokio::test]
async fn a_watch_reports_a_change_the_second_time_and_not_the_first() {
    // The first look at a page is never a change: there was nothing to differ from, and reporting
    // one would make every watch fire once, for nothing, the moment it is added.
    let site = Site::start(vec![("/", Reply::page("Notes", &[]))]).await;
    let store = std::sync::Arc::new(svipall_core::cache::Store::open_memory().expect("db"));
    let server = SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        Some(store.clone()),
    );
    let url = site.url("/");

    server
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("add".into()),
            url: Some(url.clone()),
            interval_secs: Some(60),
            label: Some("notes".into()),
            css_selector: None,
        })
        .await
        .expect("added");

    let first = server
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("check".into()),
            url: Some(url.clone()),
            interval_secs: None,
            label: None,
            css_selector: None,
        })
        .await
        .expect("checked");
    assert_eq!(first["results"][0]["changed"], false, "{first}");

    let second = server
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("check".into()),
            url: Some(url.clone()),
            interval_secs: None,
            label: None,
            css_selector: None,
        })
        .await
        .expect("checked");
    assert_eq!(
        second["results"][0]["changed"], false,
        "the page did not change: {second}"
    );

    let listed = server
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("list".into()),
            url: None,
            interval_secs: None,
            label: None,
            css_selector: None,
        })
        .await
        .expect("listed");
    assert_eq!(listed["count"], 1, "{listed}");
    assert_eq!(listed["watches"][0]["label"], "notes", "{listed}");
}

#[tokio::test]
async fn adding_a_page_already_watched_keeps_what_it_has_learned() {
    // The point of a watch is its history. Re-adding a URL to change its interval must not throw
    // away the record of what it has done.
    let site = Site::start(vec![("/", Reply::page("Notes", &[]))]).await;
    let store = std::sync::Arc::new(svipall_core::cache::Store::open_memory().expect("db"));
    let server = SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        Some(store.clone()),
    );
    let url = site.url("/");
    let add = |interval| svipall_mcp::tools::WebWatchParams {
        action: Some("add".into()),
        url: Some(url.clone()),
        interval_secs: Some(interval),
        label: None,
        css_selector: None,
    };
    server.watch_json(add(60)).await.expect("added");
    server
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("check".into()),
            url: Some(url.clone()),
            interval_secs: None,
            label: None,
            css_selector: None,
        })
        .await
        .expect("checked");
    server.watch_json(add(86_400)).await.expect("re-added");

    let listed = server
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("list".into()),
            url: None,
            interval_secs: None,
            label: None,
            css_selector: None,
        })
        .await
        .expect("listed");
    assert_eq!(listed["count"], 1, "one url is one watch: {listed}");
    assert_eq!(listed["watches"][0]["interval_secs"], 86_400, "{listed}");
    assert!(
        listed["watches"][0]["last_checked"].is_i64(),
        "the check it already did was forgotten: {listed}"
    );
}

#[tokio::test]
async fn asking_for_tables_returns_rows_not_prose() {
    let site = Site::start(vec![(
        "/",
        Reply::html(
            "<html><body><p>Some prose</p><table><tr><th>Item</th><th>Price</th></tr>\
             <tr><td>Cup</td><td>3</td></tr><tr><td>Pot</td><td>9</td></tr></table></body></html>",
        ),
    )])
    .await;
    let mut p = http(&site.url("/"));
    p.tables = Some(true);
    let out = server().fetch_json(p).await;

    assert_eq!(out.value["table_count"], 1, "{:?}", out.value);
    let t = &out.value["tables"][0];
    assert_eq!(t["header"][0], "Item");
    assert_eq!(t["rows"][1][1], "9");
    assert!(out.value.get("content").is_none(), "prose came back too");
}

#[tokio::test]
async fn a_table_can_be_written_as_csv_instead_of_filling_the_context() {
    let site = Site::start(vec![(
        "/",
        Reply::html(
            "<html><body><table><tr><th>Item</th><th>Price</th></tr>\
             <tr><td>Cup</td><td>3</td></tr></table></body></html>",
        ),
    )])
    .await;
    let mut p = http(&site.url("/"));
    p.tables = Some(true);
    p.out_file = Some("ladder-tables.csv".into());
    let out = server().fetch_json(p).await;

    let path = out.value["out_file"].as_str().expect("a path");
    assert_eq!(out.value["format"], "csv");
    assert_eq!(out.value["rows"], 1);
    let written = std::fs::read_to_string(path).expect("file exists");
    assert!(written.starts_with("Item,Price"), "{written}");
    assert!(written.contains("Cup,3"), "{written}");
    assert!(
        out.value.get("tables").is_none(),
        "rows came back as well as the file"
    );
    let _ = std::fs::remove_file(path);
}

/// `out_file` used to be silently ignored when a schema was given; the rows went into the
/// context anyway, which is the expensive outcome the parameter exists to prevent.
#[tokio::test]
async fn a_schema_fetch_honours_out_file_like_a_crawl_does() {
    let site = Site::start(vec![(
        "/",
        Reply::html(
            "<html><body><div class=\"p\"><h2>Cup</h2></div><div class=\"p\"><h2>Pot</h2></div>\
             </body></html>",
        ),
    )])
    .await;
    let mut p = http(&site.url("/"));
    p.schema = Some(serde_json::json!({
        "base_selector": "div.p", "fields": [{"name": "title", "selector": "h2"}]
    }));
    p.out_file = Some("ladder-schema.jsonl".into());
    let out = server().fetch_json(p).await;

    let path = out.value["out_file"].as_str().expect("a path");
    assert_eq!(out.value["extracted_count"], 2);
    let written = std::fs::read_to_string(path).expect("file exists");
    assert_eq!(written.lines().count(), 2, "{written}");
    assert!(written.contains("\"title\":\"Cup\""), "{written}");
    assert!(out.value.get("extracted").is_none());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn raw_html_is_extracted_without_any_request() {
    let out = server()
        .fetch_json(http(
            "raw:<html><body><h1>Inline</h1><p>No network here.</p></body></html>",
        ))
        .await;
    assert_eq!(out.value["status"], 200, "{:?}", out.value);
    assert!(
        text(&out.value).contains("No network here"),
        "{:?}",
        out.value
    );
    assert!(out.value.get("blocked_reason").is_none());
}

#[tokio::test]
async fn a_local_file_under_a_root_is_readable_and_one_outside_is_not() {
    let s = server();
    // `support::isolate()` pointed SVIPALL_HOME at a fresh directory; `in/` under it is the
    // default root.
    let home = svipall_core::config::home_dir();
    let inside = home.join("in");
    std::fs::create_dir_all(&inside).unwrap();
    let ok = inside.join("page.html");
    std::fs::write(&ok, "<html><body><p>From disk</p></body></html>").unwrap();
    let outside = home.join("secret.html");
    std::fs::write(&outside, "<html><body><p>Not for you</p></body></html>").unwrap();
    let url_of = |p: &std::path::Path| url::Url::from_file_path(p).unwrap().to_string();

    let good = s.fetch_json(http(&url_of(&ok))).await;
    assert!(text(&good.value).contains("From disk"), "{:?}", good.value);

    let bad = s.fetch_json(http(&url_of(&outside))).await;
    assert_eq!(bad.value["wall_kind"], "policy", "{:?}", bad.value);
    assert!(!text(&bad.value).contains("Not for you"));
}

/// What a schema found on one visit is remembered per domain, so when the site is redesigned the
/// next fetch relocates the selector instead of returning nulls in silence.
#[tokio::test]
async fn fingerprints_persist_between_fetches_of_the_same_domain() {
    let v1 = "<html><body><main><div class=\"card\"><h2>Cup</h2><span class=\"price\">3</span></div>\
              <div class=\"card\"><h2>Pot</h2><span class=\"price\">9</span></div></main></body></html>";
    let v2 = v1.replace("class=\"price\"", "class=\"cost\"");
    let site = Site::start(vec![("/v1", Reply::html(v1)), ("/v2", Reply::html(&v2))]).await;
    let s = server();
    let schema = serde_json::json!({
        "name": "products", "base_selector": "div.card",
        "fields": [{"name": "title", "selector": "h2"}, {"name": "price", "selector": "span.price"}]
    });

    let mut p = http(&site.url("/v1"));
    p.schema = Some(schema.clone());
    let first = s.fetch_json(p).await;
    assert_eq!(first.value["extracted"]["count"], 2, "{:?}", first.value);
    assert!(first.value.get("healed").is_none());

    let mut p = http(&site.url("/v2"));
    p.schema = Some(schema);
    let second = s.fetch_json(p).await;
    assert_eq!(
        second.value["extracted"]["items"][1]["price"], "9",
        "{:?}",
        second.value
    );
    let healed = second.value["healed"].as_array().expect("healed");
    assert_eq!(healed[0]["field"], "price");
    assert_eq!(healed[0]["to"], "span.cost");
    assert!(second.value["note"].as_str().unwrap().contains("relocated"));
}

/// A watch on a region hashes that region only, and survives the region's selector breaking.
#[tokio::test]
async fn a_watch_on_a_selector_ignores_changes_elsewhere_on_the_page() {
    let s = server();
    let dir = svipall_core::config::home_dir().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("watched.html");
    let page = |aside: &str, price: &str, cls: &str| {
        format!(
            "<html><body><aside>{aside}</aside><main><div class=\"{cls}\">Price: {price}</div></main></body></html>"
        )
    };
    std::fs::write(&file, page("v1", "3", "price")).unwrap();
    let url = url::Url::from_file_path(&file).unwrap().to_string();

    let add = s
        .watch_json(svipall_mcp::tools::WebWatchParams {
            action: Some("add".into()),
            url: Some(url.clone()),
            interval_secs: Some(60),
            label: None,
            css_selector: Some("div.price".into()),
        })
        .await
        .unwrap();
    assert_eq!(add["added"], true, "{add:?}");

    async fn check(s: &SvipallServer, url: &str) -> Value {
        let v = s
            .watch_json(svipall_mcp::tools::WebWatchParams {
                action: Some("check".into()),
                url: Some(url.to_string()),
                interval_secs: None,
                label: None,
                css_selector: None,
            })
            .await
            .unwrap();
        v["results"][0].clone()
    }
    let first = check(&s, &url).await;
    assert_eq!(
        first["changed"], false,
        "first observation is never a change: {first:?}"
    );

    std::fs::write(&file, page("v2 changed elsewhere", "3", "price")).unwrap();
    let second = check(&s, &url).await;
    assert_eq!(second["changed"], false, "{second:?}");

    std::fs::write(&file, page("v2 changed elsewhere", "4", "cost")).unwrap();
    let third = check(&s, &url).await;
    assert_eq!(third["changed"], true, "{third:?}");
    assert_eq!(third["healed"][0]["to"], "main > div.cost", "{third:?}");
}

/// A pool is declared once with its countries and removed whole; a typo in a country is refused
/// before anything is written.
#[tokio::test]
async fn a_pool_of_exits_is_stored_with_its_countries_and_removed_whole() {
    use svipall_mcp::tools::WebRouteParams;
    let s = server();
    let bad = s
        .route_json(WebRouteParams {
            domain: Some("Pool.Example".into()),
            proxy: None,
            country: None,
            proxies: Some(vec!["http://a:1".into(), "http://b:2".into()]),
            countries: Some(vec!["DE".into(), "XX".into()]),
            remove: None,
            ..Default::default()
        })
        .await;
    assert!(bad.is_err(), "an unknown country must be refused");

    let ok = s
        .route_json(WebRouteParams {
            domain: Some("Pool.Example".into()),
            proxy: None,
            country: Some("NL".into()),
            proxies: Some(vec!["http://a:1".into(), "http://b:2".into()]),
            countries: Some(vec!["DE".into()]),
            remove: None,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        ok["pools"]["pool.example"],
        serde_json::json!(["http://a:1", "http://b:2"])
    );
    assert_eq!(ok["proxy_regions"]["http://a:1"], "DE");
    assert_eq!(
        ok["proxy_regions"]["http://b:2"], "NL",
        "`country` covers the entries without one"
    );
    assert_eq!(ok["exit_strategy"], "sticky");
    assert_eq!(
        svipall_core::exits::exits_for("shop.pool.example"),
        vec!["http://a:1".to_string(), "http://b:2".to_string()],
        "subdomains inherit"
    );

    let gone = s
        .route_json(WebRouteParams {
            domain: Some("pool.example".into()),
            proxy: None,
            country: None,
            proxies: None,
            countries: None,
            remove: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(gone["pools"].get("pool.example").is_none(), "{gone:?}");
    assert!(svipall_core::exits::exits_for("pool.example").is_empty());
}

/// A document linked from a page reads like a page: the http tier converts it and the pipeline
/// sees prose, not bytes.
#[tokio::test]
async fn an_office_document_comes_back_as_markdown() {
    let rtf = r"{\rtf1\ansi Quarterly {\b revenue} rose\par}";
    let site = Site::start(vec![(
        "/report.rtf",
        Reply::plain(rtf).header("content-type", "application/rtf"),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/report.rtf"))).await;
    let body = text(&out.value);
    assert!(body.contains("**revenue**"), "{:?}", out.value);
    assert!(out.value.get("blocked_reason").is_none(), "{:?}", out.value);

    // The same document from disk, under the default local root.
    let dir = svipall_core::config::home_dir().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("report.rtf");
    std::fs::write(&file, rtf).unwrap();
    let url = url::Url::from_file_path(&file).unwrap().to_string();
    let local = server().fetch_json(http(&url)).await;
    assert!(
        text(&local.value).contains("**revenue**"),
        "{:?}",
        local.value
    );
}

/// A site that offers HTTP/3 is remembered by domain with the lifetime it named, and listed by
/// `web_status`. It is not spoken to over h3 unless `config.http3` says so and the binary was
/// built with a QUIC stack in it, which is what makes a plain install fetch exactly as it did
/// before HTTP/3 existed. What happens when it *is* on, including that the page survives a site
/// that advertised h3 and then does not answer, is in `svipall-http`s own `tests/h3.rs`.
#[tokio::test]
async fn a_site_that_offers_h3_is_remembered_and_not_spoken_to_over_it_by_default() {
    let site = Site::start(vec![(
        "/",
        Reply::page("Alt", &[]).header("alt-svc", "h3=\":443\"; ma=3600"),
    )])
    .await;
    let s = server();
    let out = s.fetch_json(http(&site.url("/"))).await;
    assert_eq!(out.value["tier_used"], "http");
    let status = s
        .status_json(serde_json::from_value(serde_json::json!({})).unwrap())
        .await
        .unwrap();
    let offered = status["h3_offered_by"].as_array().expect("list");
    assert_eq!(offered.len(), 1, "{status:?}");
    assert_eq!(offered[0][1], 443);
}

/// A 200 that carries a not-found template is a wall, not a page, and no tier can fix it.
#[tokio::test]
async fn a_soft_404_is_reported_as_one_instead_of_being_delivered_as_the_page() {
    // The failure this pins: the server answers 200, the body says the thing is not there, and
    // the caller reads that notice as the article it asked for.
    let site = Site::start(vec![(
        "/gone",
        Reply::html(
            "<!doctype html><html><head><title>Page not found</title></head><body>\
             <h1>404</h1><p>Sorry, the page you were looking for is not here.</p>\
             <a href=\"/\">Back to home</a></body></html>",
        ),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/gone"))).await;

    assert_eq!(out.value["status"], 200);
    assert_eq!(out.value["wall_kind"], "softnotfound", "{:?}", out.value);
    assert!(out.value["blocked_reason"].is_string(), "{:?}", out.value);
    // And the advice has to say that climbing is pointless, or the caller retries for nothing.
    let note = out.value["note"].as_str().unwrap_or_default();
    assert!(note.contains("cannot help"), "{note:?}");
}

/// A subscription stub is the article being withheld, so the advice is a profile, not a proxy.
#[tokio::test]
async fn a_subscription_stub_is_reported_as_a_paywall_and_not_as_the_article() {
    let site = Site::start(vec![(
        "/story",
        Reply::html(
            "<!doctype html><html><head><title>The story</title>\
             <script type=\"application/ld+json\">{\"@type\":\"NewsArticle\",\"isAccessibleForFree\":false}</script>\
             </head><body><h1>The story</h1><p>The first paragraph is free.</p>\
             <div class=\"paywall\">Subscribe to continue reading.</div></body></html>",
        ),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/story"))).await;

    assert_eq!(out.value["wall_kind"], "paywall", "{:?}", out.value);
    let note = out.value["note"].as_str().unwrap_or_default();
    assert!(note.contains("web_login"), "{note:?}");
}

/// A delivered page says how much of itself arrived, and says nothing when there is nothing to say.
#[tokio::test]
async fn a_delivered_page_carries_what_arrived_of_it() {
    let long = "The council voted on Tuesday to approve the measure after a long debate that ran \
                past midnight. Supporters argued the change was overdue and that the money had \
                already been set aside; opponents said the figures had never been published in \
                full. A second reading is expected before the end of the month.";
    let site = Site::start(vec![
        (
            "/article",
            Reply::html(&format!(
                "<!doctype html><html><head><title>Council votes</title></head>\
                 <body><main><h1>Council votes</h1><p>{long}</p></main></body></html>"
            )),
        ),
        (
            "/stub",
            Reply::html(
                "<!doctype html><html><head><title>Links</title></head>\
                 <body><main><a href=\"/a\">One</a> <a href=\"/b\">Two</a></main></body></html>",
            ),
        ),
    ])
    .await;
    let s = server();

    let whole = s.fetch_json(http(&site.url("/article"))).await;
    assert_eq!(whole.value["quality"], "full", "{:?}", whole.value);
    assert!(
        whole.value.get("quality_reasons").is_none(),
        "a page with nothing wrong with it should cost no tokens explaining that: {:?}",
        whole.value
    );

    // And the husk is labelled rather than dropped: the content is still there to read.
    let husk = s.fetch_json(http(&site.url("/stub"))).await;
    assert_eq!(husk.value["quality"], "thin", "{:?}", husk.value);
    let why = husk.value["quality_reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    assert!(why.contains(&"thin_text".to_string()), "{why:?}");
    assert!(
        husk.value.get("blocked_reason").is_none(),
        "thin is a label, not a wall: {:?}",
        husk.value
    );
    assert!(text(&husk.value).contains("One"), "{:?}", husk.value);
}

/// The same page gets the same label whether it came from the site or from the cache.
#[tokio::test]
async fn a_cached_page_keeps_the_verdict_the_fetch_gave_it() {
    // Otherwise the field's absence is ambiguous — "nothing wrong with it" and "nobody looked" read
    // the same on the wire, and the second fetch of a thin page looks like a full one.
    let site = Site::start(vec![(
        "/stub",
        Reply::html(
            "<!doctype html><html><head><title>Links</title></head>\
             <body><main><a href=\"/a\">One</a> <a href=\"/b\">Two</a></main></body></html>",
        ),
    )])
    .await;
    let s = server();
    // A local URL is not cached unless asked, which is what the test above this one pins.
    let cached = |url: &str| WebFetchParams {
        cache: Some("auto".into()),
        ..http(url)
    };

    let first = s.fetch_json(cached(&site.url("/stub"))).await;
    assert_eq!(first.value["quality"], "thin", "{:?}", first.value);

    let second = s.fetch_json(cached(&site.url("/stub"))).await;
    assert_eq!(second.value["from_cache"], true, "{:?}", second.value);
    assert_eq!(second.value["quality"], "thin", "{:?}", second.value);
    assert_eq!(
        second.value["quality_reasons"], first.value["quality_reasons"],
        "{:?}",
        second.value
    );
}

/// A soft 404 with no telltale words is caught by asking the site what a missing page looks like.
#[tokio::test]
async fn a_site_is_asked_once_what_its_missing_page_looks_like() {
    // The phrase list only reaches soft 404s written in a language it knows. Bar-Yossef's method
    // needs no vocabulary at all: fetch something that is certainly not there, and compare. This
    // page says nothing about being missing — in any language — and is still recognised.
    let husk = Reply::html(
        "<!doctype html><html><head><title>Ejemplo</title></head><body><main>\
         <h1>Vaya</h1><p>Lo sentimos mucho.</p><a href=\"/\">Inicio</a></main></body></html>",
    );
    let site = Site::start(vec![
        ("/articulo", husk.clone()),
        // Anything not listed answers with the same husk, which is what a soft 404 is.
        ("/svipall-probe-for-a-page-that-is-not-here-0f3a9c", husk),
    ])
    .await;
    let s = server();

    let out = s.fetch_json(http(&site.url("/articulo"))).await;
    assert_eq!(out.value["wall_kind"], "softnotfound", "{:?}", out.value);
    assert!(out.value["blocked_reason"].is_string(), "{:?}", out.value);
}

/// A site that answers 404 honestly is asked once and then left alone.
#[tokio::test]
async fn a_site_that_answers_404_honestly_is_never_probed_twice() {
    let site = Site::start(vec![
        (
            "/short",
            Reply::html(
                "<!doctype html><html><head><title>Short</title></head>\
                 <body><main><p>Brief.</p></main></body></html>",
            ),
        ),
        (
            "/svipall-probe-for-a-page-that-is-not-here-0f3a9c",
            Reply::html("<!doctype html><html><body><h1>404</h1></body></html>").with_status(404),
        ),
    ])
    .await;
    let s = server();

    for _ in 0..3 {
        let out = s.fetch_json(http(&site.url("/short"))).await;
        // Thin, but delivered: an honest 404 gives nothing to compare against, so nothing is
        // claimed. The page still comes back.
        assert_eq!(out.value["quality"], "thin", "{:?}", out.value);
        assert!(out.value.get("blocked_reason").is_none(), "{:?}", out.value);
    }
    assert_eq!(
        site.hits("/svipall-probe-for-a-page-that-is-not-here-0f3a9c"),
        1,
        "the answer is kept: one probe per site, ever"
    );
}

/// An affiliate listicle is labelled as engineered — and still returned, whole.
#[tokio::test]
async fn a_page_built_for_a_ranking_is_named_as_one_and_delivered_anyway() {
    // The shape Bevendorff et al. found at the top of every product-review ranking: referral links
    // in bulk and headings that are the search query again. Naming it is the point; withholding it
    // is not, because the page a ranking was built for can still be the page with the answer.
    let mut body = String::from(
        "<!doctype html><html><head><title>Best cheap anvils</title></head><body><main>",
    );
    for i in 0..12 {
        body.push_str(&format!(
            "<h2>best cheap anvils {i}</h2>\
             <p>best cheap anvils are the best cheap anvils for cheap anvil buyers who want \
             cheap anvils at the best cheap anvil prices available anywhere today.</p>\
             <a href=\"https://amzn.to/anvil{i}\">Buy now</a> \
             <a href=\"/more/{i}\">More</a> <a href=\"/also/{i}\">Also</a>"
        ));
    }
    body.push_str("</main></body></html>");
    let site = Site::start(vec![("/anvils", Reply::html(&body))]).await;

    let out = server().fetch_json(http(&site.url("/anvils"))).await;
    assert_eq!(out.value["optimization"], "high", "{:?}", out.value);
    let traits: Vec<String> = out.value["optimization_traits"]
        .as_array()
        .expect("traits")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        traits.contains(&"affiliate_heavy".to_string()),
        "{traits:?}"
    );

    // And it came back. The label is the whole intervention.
    assert!(out.value.get("blocked_reason").is_none(), "{:?}", out.value);
    assert!(text(&out.value).contains("anvil"), "{:?}", out.value);
}

/// ▲ "Measured, and ordinary" is a different statement from "nobody measured it", and the response
/// has to be able to make both. This test used to assert the opposite — that an ordinary page
/// carried no `optimization` field — which left a caller unable to tell an unremarkable page from
/// one svipall never looked at. The level is four tokens; the traits, which are the expensive part,
/// still ride along only when there are any.
#[tokio::test]
async fn an_ordinary_article_says_so_rather_than_saying_nothing() {
    let long = "The council voted on Tuesday to approve the harbour redevelopment after a debate \
                that ran past midnight. Supporters argued the change was overdue and that the \
                money had been set aside three budgets ago; opponents said the figures had never \
                been published in full. "
        .repeat(6);
    let site = Site::start(vec![(
        "/article",
        Reply::html(&format!(
            "<!doctype html><html><head><title>Harbour</title></head>\
             <body><main><h1>Harbour</h1><p>{long}</p></main></body></html>"
        )),
    )])
    .await;

    let out = server().fetch_json(http(&site.url("/article"))).await;
    assert_eq!(
        out.value["optimization"], "ordinary",
        "absent must mean 'not measured', so an ordinary page has to say ordinary: {:?}",
        out.value
    );
    assert!(
        out.value.get("optimization_traits").is_none(),
        "most of the web is optimised; listing nothing about all of it is noise: {:?}",
        out.value
    );
}

/// Several results carrying one story are one source, and the answer says so.
#[tokio::test]
async fn syndicated_copies_are_counted_once_and_still_all_returned() {
    // Without this, five hostnames repeating one wire story read as five confirmations. The label
    // is the whole intervention: every result still comes back, in order, with its content.
    let wire = "<!doctype html><html><head><title>Harbour</title></head><body><main><p>\
        The council voted on Tuesday to approve the harbour redevelopment after a debate that ran \
        past midnight. Eleven members voted in favour, four against, and two abstained on the \
        grounds that the costings had not been circulated in time to be read properly.\
        </p></main></body></html>";
    let own = "<!doctype html><html><head><title>Ferries</title></head><body><main><p>\
        Ferry operators say the eastern quay closure will cost them an entire season, and that no \
        temporary berth has been offered to them anywhere on the far side of the basin so far.\
        </p></main></body></html>";
    let site = Site::start(vec![
        ("/a", Reply::html(wire)),
        ("/b", Reply::html(wire)),
        ("/c", Reply::html(wire)),
        ("/d", Reply::html(own)),
    ])
    .await;

    let out = server()
        .fetch_many_json(
            serde_json::from_value(serde_json::json!({
                "urls": [site.url("/a"), site.url("/b"), site.url("/c"), site.url("/d")],
                "max_tier": "http",
            }))
            .unwrap(),
        )
        .await;

    assert_eq!(out["count"], 4, "nothing was dropped: {out:?}");
    assert_eq!(out["corroboration"]["independent"], 2, "{out:?}");
    assert_eq!(out["corroboration"]["largest_group"], 3, "{out:?}");

    let results = out["results"].as_array().expect("results");
    let at = |path: &str| {
        results
            .iter()
            .find(|r| r["url"].as_str().is_some_and(|u| u.ends_with(path)))
            .unwrap_or_else(|| panic!("{path} is missing: {results:?}"))
    };
    assert!(
        at("/a").get("same_text_as").is_none(),
        "the first of a kind repeats nobody: {:?}",
        at("/a")
    );
    assert!(at("/b")["same_text_as"].is_string(), "{:?}", at("/b"));
    assert!(
        at("/d").get("same_text_as").is_none(),
        "its own reporting is its own: {:?}",
        at("/d")
    );
    // And every one of them still carries its text.
    assert!(results.iter().all(|r| !text(r).is_empty()), "{results:?}");

    // ▲ The ordering, which is the other half. Three copies of one wire story followed by the one
    // page that says something else: the different page is what the caller should read second, and
    // the reorder is announced rather than done quietly. The caller's own first choice never moves.
    assert_eq!(out["reordered_for_diversity"], true, "{out:?}");
    assert!(
        results[0]["url"]
            .as_str()
            .is_some_and(|u| u.ends_with("/a")),
        "the caller's first choice moved: {:?}",
        results[0]
    );
    assert!(
        results[1]["url"]
            .as_str()
            .is_some_and(|u| u.ends_with("/d")),
        "the only result that adds anything was left at the bottom: {results:?}"
    );
}

/// ▲ A cache is supposed to change how fast an answer arrives, never what the answer is. Three of
/// the six quality fields used to be dropped on the way to disk, so the same URL answered
/// differently depending on whether the cache happened to hold it — with nothing in the response
/// saying which had happened.
#[tokio::test]
async fn a_cache_hit_carries_everything_the_fetch_measured() {
    let long = "The council voted on Tuesday to approve the harbour redevelopment after a debate \
                that ran past midnight, with eleven in favour and four against. "
        .repeat(8);
    let site = Site::start(vec![(
        "/article",
        Reply::html(&format!(
            "<!doctype html><html><head><title>Harbour</title></head>\
             <body><main><h1>Harbour</h1><p>{long}</p></main></body></html>"
        )),
    )])
    .await;

    // A loopback address is not cached unless asked: a dev server changes every time a file is
    // saved. Asked for here, because the cache is what is under test.
    let cached = || WebFetchParams {
        cache: Some("auto".into()),
        ..http(&site.url("/article"))
    };
    let s = server();
    let fetched = s.fetch_json(cached()).await.value;
    let hit = s.fetch_json(cached()).await.value;

    assert_eq!(
        hit["from_cache"], true,
        "the second read was not a hit: {hit}"
    );
    for field in ["quality", "optimization"] {
        assert_eq!(
            fetched.get(field),
            hit.get(field),
            "`{field}` changed between a fetch and a cache hit\nfetch: {fetched}\nhit:   {hit}"
        );
    }
}

/// The near-duplicate lookup is the one observation a stateless extractor cannot make at all: it
/// is the cache being asked whether it has seen this page before, under any name.
#[tokio::test]
async fn the_full_breakdown_recognises_a_page_already_in_the_cache_under_another_name() {
    let story: String = (0..40)
        .map(|i| {
            format!(
                "The council approved measure {i} on Tuesday after a long debate about parking, \
                 drainage and the future of the old library building. "
            )
        })
        .collect();
    let page = |extra: &str| {
        Reply::html(&format!(
            "<!doctype html><html><head><title>Harbour</title>\
             <meta name=\"author\" content=\"A Reporter\">\
             <meta property=\"article:published_time\" content=\"2026-03-01\"></head>\
             <body><main><p>{story}{extra}</p>\
             <p><a href=\"https://elsewhere.test/source\">the minutes</a></p></main></body></html>"
        ))
    };
    let site = Site::start(vec![
        ("/first", page("")),
        (
            "/syndicated",
            page("Distributed by the regional news wire."),
        ),
    ])
    .await;

    let s = server();
    // Stored, because what is under test is the cache being asked whether it has seen this before.
    let _ = s
        .fetch_json(WebFetchParams {
            cache: Some("auto".into()),
            ..http(&site.url("/first"))
        })
        .await;

    let detail = s
        .fetch_json(WebFetchParams {
            include_quality: Some(true),
            cache: Some("auto".into()),
            ..http(&site.url("/syndicated"))
        })
        .await
        .value;
    let q = &detail["quality_detail"];

    assert!(
        q["near_dup_of"][0]["url"]
            .as_str()
            .is_some_and(|u| u.ends_with("/first")),
        "the copy already on disk was not recognised: {detail}"
    );
    assert_eq!(q["provenance"]["author"], "A Reporter", "{detail}");
    assert_eq!(q["provenance"]["published"], "2026-03-01", "{detail}");
    assert_eq!(
        q["provenance"]["cited_hosts"], 1,
        "one outbound source, counted and not judged: {detail}"
    );
    assert!(
        q["provenance"]["site_first_seen"].is_i64(),
        "when this machine first saw the site: {detail}"
    );
    assert!(q["integrity"].is_object(), "{detail}");
    assert!(q["signals"].is_object(), "{detail}");

    // ▲ And it is a label. The page came back whole either way.
    assert!(text(&detail).contains("old library building"), "{detail}");

    // Nobody asked for the breakdown here, so nobody pays for it.
    let plain = s.fetch_json(http(&site.url("/syndicated"))).await.value;
    assert!(plain.get("quality_detail").is_none(), "{plain}");
    assert!(plain.get("metadata").is_none(), "{plain}");
    assert!(plain.get("links").is_none(), "{plain}");
}

/// A percentile out of eleven pages is arithmetic, not evidence — and the caller is told which of
/// the two they are holding rather than handed a confident-looking number.
#[tokio::test]
async fn a_calibration_with_no_history_behind_it_says_so() {
    let long = "Ferry operators say the eastern quay closure will cost them an entire season and \
                that no temporary berth has been offered anywhere on the far side of the basin. "
        .repeat(8);
    let site = Site::start(vec![(
        "/a",
        Reply::html(&format!(
            "<!doctype html><html><head><title>Ferries</title></head>\
             <body><main><p>{long}</p></main></body></html>"
        )),
    )])
    .await;

    let out = server()
        .fetch_json(WebFetchParams {
            include_quality: Some(true),
            ..http(&site.url("/a"))
        })
        .await
        .value;
    let c = &out["quality_detail"]["optimization_calibration"];
    assert!(
        c["unavailable"]
            .as_str()
            .is_some_and(|s| s.contains("not enough observations")),
        "one page is not a distribution, and saying nothing would read as a low score: {out}"
    );
    assert!(c.get("percentile").is_none(), "{out}");
}

/// ▲ The one capability a stateless extractor cannot have: the pages of this site already on disk.
///
/// Sixteen pages of one domain, then a seventeenth — and the navigation and footer that survived
/// page-level pruning on every one of them come off, because the rest of the site says what they
/// are. The response says it happened and how much it was learned from; a result that changes
/// between sessions because a tool learned something in between, and does not say so, is worse
/// than one that never improved.
#[tokio::test]
async fn a_site_seen_often_enough_has_its_own_furniture_taken_off() {
    let nav = "<nav><p>Home and About and Products and Support and Careers and Press and Contact \
               and Sign in, every one of them on every single page of this site.</p></nav>";
    let foot = "<footer><p>Contact us, privacy policy, terms of service, cookie preferences, \
                accessibility statement, and the usual long legal line at the bottom.</p></footer>";
    let article = |i: usize| {
        format!(
            "<p>Article number {i} is about its own subject and shares nothing with the article \
             beside it. It runs on for a while, as articles do, and it says several things that \
             the article before it did not say at all.</p>"
        )
    };
    let mut routes: Vec<(&'static str, Reply)> = Vec::new();
    let paths: Vec<&'static str> = (0..17)
        .map(|i| Box::leak(format!("/a{i}").into_boxed_str()) as &'static str)
        .collect();
    for (i, path) in paths.iter().enumerate() {
        routes.push((
            path,
            Reply::html(&format!(
                "<!doctype html><html><head><title>Piece {i}</title></head><body><main>{nav}{}{foot}</main></body></html>",
                article(i)
            )),
        ));
    }
    let site = Site::start(routes).await;
    let s = server();
    // ▲ Asked for. The site template is off by default: on TECO it removed one word of
    // human-labelled main content, and one is too many for something nobody switched on.
    let fetch = |path: &str| WebFetchParams {
        cache: Some("auto".into()),
        use_site_template: Some(true),
        ..http(&site.url(path))
    };

    // The first sixteen arm the record. Nothing is stripped from any of them.
    for path in &paths[..16] {
        let out = s.fetch_json(fetch(path)).await.value;
        assert!(
            out.get("template").is_none(),
            "a record that is not armed must change nothing: {out}"
        );
        assert!(text(&out).contains("Home and About"), "{out}");
    }

    let out = s.fetch_json(fetch(paths[16])).await.value;
    assert_eq!(
        out["template"]["learned_from"], 16,
        "the seventeenth page was read without the sixteen before it: {out}"
    );
    assert!(
        out["template"]["removed_blocks"].as_u64().unwrap_or(0) >= 2,
        "{out}"
    );
    assert!(
        text(&out).contains("Article number 16"),
        "the article was removed: {out}"
    );
    assert!(
        !text(&out).contains("Home and About"),
        "the navigation stayed: {out}"
    );
    assert!(
        !text(&out).contains("privacy policy"),
        "the footer stayed: {out}"
    );

    // ▲ And the cache answers the same way. A hit that handed back furniture the fetch beside it
    // had taken off would make the same URL two different pages.
    let hit = s.fetch_json(fetch(paths[16])).await.value;
    assert_eq!(hit["from_cache"], true, "{hit}");
    assert_eq!(
        hit["template"]["removed_blocks"], out["template"]["removed_blocks"],
        "{hit}"
    );
    // Seventeen rather than sixteen: the page that was fetched joined the record on its way past.
    assert_eq!(hit["template"]["learned_from"], 17, "{hit}");
    assert!(!text(&hit).contains("Home and About"), "{hit}");

    // The store keeps the page whole: this is a view, not a rewrite.
    assert!(
        s.store()
            .and_then(|st| st.get(&site.url(paths[16])))
            .is_some_and(|p| p.markdown.contains("Home and About")),
        "the cached row was rewritten rather than viewed"
    );
}

/// Whether h3 is on is three separate facts, and a caller looking at one page fetched over TCP
/// with an `Alt-Svc` in hand needs to see which of them said no.
#[tokio::test]
async fn web_status_says_whether_http3_is_built_and_whether_it_is_enabled() {
    let s = server();
    let status = s
        .status_json(serde_json::from_value(serde_json::json!({})).unwrap())
        .await
        .unwrap();
    let h3 = &status["http3"];
    assert_eq!(
        h3["built"],
        svipall_http::http3_available(),
        "the report must match the binary, not a wish"
    );
    assert_eq!(h3["enabled"], false, "off unless an operator turned it on");
    assert_eq!(h3["in_use"], false);
}

/// ▲ The default, and the reason it is the default. TECO — the only corpus that ships each page's
/// siblings — priced the site template at one word of human-labelled main content removed. One is
/// too many for something nobody switched on, so a fetch that does not ask for it gets the page
/// whole, however much of the site svipall has seen.
#[tokio::test]
async fn the_site_template_does_nothing_unless_it_is_asked_for() {
    let nav = "<nav><p>Home and About and Products and Support and Careers and Press and Contact \
               and Sign in and Newsletter and Advertise, every one of them on every single page \
               of this site, all the way down.</p></nav>";
    let article = |i: usize| {
        format!(
            "<p>Article number {i} is about its own subject and shares nothing with the article \
             beside it. It runs on for a while, as articles do, and it says several things that \
             the article before it did not say at all.</p>"
        )
    };
    let paths: Vec<&'static str> = (0..18)
        .map(|i| Box::leak(format!("/b{i}").into_boxed_str()) as &'static str)
        .collect();
    let routes: Vec<(&'static str, Reply)> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                *p,
                Reply::html(&format!(
                    "<!doctype html><html><head><title>Piece {i}</title></head><body><main>\
                     {nav}{}</main></body></html>",
                    article(i)
                )),
            )
        })
        .collect();
    let site = Site::start(routes).await;
    let s = server();

    for path in &paths {
        let out = s
            .fetch_json(WebFetchParams {
                cache: Some("auto".into()),
                ..http(&site.url(path))
            })
            .await
            .value;
        assert!(
            out.get("template").is_none(),
            "a fetch that did not ask for it had text taken off: {out}"
        );
        assert!(
            text(&out).contains("Home and About"),
            "the navigation was removed without being asked: {out}"
        );
    }

    // And the record was learned all the same, so asking for it works at once rather than in
    // another sixteen pages.
    let asked = s
        .fetch_json(WebFetchParams {
            cache: Some("auto".into()),
            use_site_template: Some(true),
            ..http(&site.url(paths[17]))
        })
        .await
        .value;
    assert!(
        asked["template"]["learned_from"].as_u64().unwrap_or(0) >= 16,
        "the record was not being learned while the feature was off: {asked}"
    );
    assert!(!text(&asked).contains("Home and About"), "{asked}");
}

/// A walled site's shell: what it serves once it has decided to say nothing. No vendor string
/// anywhere in the markup — that is the whole point.
const SHELL: &str = "<!doctype html><html><head><title></title></head><body>\
<div id=\"root\"></div></body></html>";

#[tokio::test]
async fn a_wall_that_shows_only_in_a_response_header_is_named_as_the_vendors_wall() {
    // The measured failure: the proof-of-work vendor blocks with a bare 403 and a header. Reported
    // as "http 403", that teaches nobody which wall it was or what would move it.
    let site = Site::start(vec![(
        "/",
        Reply::html(SHELL)
            .with_status(403)
            .header("x-kpsdk-ct", "2|abc|def"),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/"))).await.value;
    assert_eq!(out["wall_kind"], "vendor", "{out}");
    assert!(
        out["blocked_reason"]
            .as_str()
            .unwrap_or_default()
            .contains("x-kpsdk-ct"),
        "the reason names the evidence: {out}"
    );
}

#[tokio::test]
async fn a_wall_that_shows_only_in_a_cookie_is_named_as_the_vendors_wall() {
    // The sensor-cookie vendor answers 200 with a shell and sets its cookie. Without the cookie
    // channel this is indistinguishable from an app that simply has not rendered.
    let site = Site::start(vec![(
        "/",
        Reply::html(SHELL).header("set-cookie", "visid_incap_9812=abc; path=/; HttpOnly"),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/"))).await.value;
    assert_eq!(out["wall_kind"], "vendor", "{out}");
    assert_eq!(out["wall_vendor"], "incapsula.com", "{out}");
}

#[tokio::test]
async fn a_blocked_page_names_the_vendor_it_was_blocked_by() {
    let site = Site::start(vec![(
        "/",
        Reply::html(SHELL)
            .with_status(403)
            .header("x-kpsdk-ct", "2|abc"),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/"))).await.value;
    assert_eq!(out["wall_vendor"], "kpsdk.io", "{out}");
    assert_eq!(out["wall_evidence"], "header x-kpsdk-ct", "{out}");
}

#[tokio::test]
async fn a_page_that_arrived_is_still_delivered_whole_when_a_vendor_is_on_the_wire() {
    // The fence. A vendor on the wire labels a page; it never withholds one. A site behind one of
    // these products serves ordinary pages the rest of the time, and those are ordinary pages.
    let prose = "The article itself, at length. ".repeat(60);
    let body = format!("<!doctype html><html><body><article>{prose}</article></body></html>");
    let site = Site::start(vec![(
        "/",
        Reply::html(&body).header("x-kpsdk-ct", "2|abc"),
    )])
    .await;
    let out = server().fetch_json(http(&site.url("/"))).await.value;
    assert!(
        out.get("blocked_reason").is_none_or(|r| r.is_null()),
        "a delivered page stays delivered: {out}"
    );
    assert!(text(&out).contains("The article itself"), "{out}");
    assert_eq!(
        out["wall_vendor"], "kpsdk.io",
        "and the vendor is still reported: {out}"
    );
}

/// An address that has spent its standing with a host is told so, before anything goes out.
///
/// The URL is deliberately one that can never resolve: the whole claim of this gate is that no
/// request is made, and a host that does not exist is the only way to assert that without trusting
/// a counter. Today the same call comes back "no tier could fetch the page" after trying.
#[tokio::test]
async fn a_domain_over_its_budget_is_refused_before_anything_goes_out() {
    let s = server();
    let domain = "budget-gate.invalid";
    svipall_core::reputation::add(domain, None, svipall_core::reputation::budget() * 2.0);

    let out = s
        .fetch_json(WebFetchParams {
            url: format!("https://{domain}/page"),
            ..Default::default()
        })
        .await;
    let out = out.value;
    svipall_core::reputation::clear(domain);

    assert_eq!(out["blocked_reason"], "address_budget", "{out}");
    assert_eq!(out["wall_kind"], "reputation", "{out}");
    assert_eq!(
        out["attempts"].as_array().map(Vec::len),
        Some(0),
        "the point of the gate is that nothing was tried: {out}"
    );
    let left = out["reputation_seconds_left"].as_u64().unwrap_or(0);
    assert!(left > 0, "it must say how long the wait is: {out}");
    let note = out["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("clear_budget") && note.contains(domain),
        "the note must name the way out: {note}"
    );
}

/// The budget is reported and can be emptied, the way a cooldown can.
#[tokio::test]
async fn web_status_reports_the_budget_and_clear_budget_empties_it() {
    let s = server();
    let domain = "budget-status.invalid";
    svipall_core::reputation::add(domain, None, svipall_core::reputation::budget() * 0.9);

    let before = s
        .status_json(Default::default())
        .await
        .expect("status")
        .get("reputation")
        .cloned()
        .unwrap_or_default();
    assert!(
        before["by_domain"][domain]["direct"]["spent"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "an address that has spent must show up: {before}"
    );

    let after = s
        .status_json(svipall_mcp::tools::WebStatusParams {
            clear_budget: Some(domain.into()),
            ..Default::default()
        })
        .await
        .expect("status");
    assert!(
        after["reputation"]["by_domain"][domain].is_null(),
        "clear_budget must empty it: {}",
        after["reputation"]
    );
}

#[tokio::test]
async fn the_status_report_names_the_pages_being_held() {
    // A held page is a Chrome tab this process is keeping alive. That is exactly the kind of thing
    // an operator should be able to see, rather than infer from memory use.
    let out = server()
        .status_json(Default::default())
        .await
        .expect("status");
    let kept = out["browser"]["kept"]
        .as_array()
        .expect("browser.kept is an array");
    assert!(kept.is_empty(), "a fresh server is holding nothing: {out}");
}
