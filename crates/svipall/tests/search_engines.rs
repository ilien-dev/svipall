//! `web_search` against engines that answer the way the real ones did on the day the fixtures were
//! captured: a results page, a duck-captcha anomaly page served as 202, a 429 captcha, a genuinely
//! empty page, redirect-wrapped links and a news feed. Every engine points at a local fixture
//! server, so none of this reaches the network.
mod support;

use serde_json::Value;
use support::{Reply, Site};
use svipall::search::Endpoints;
use svipall::server::SvipallServer;
use svipall::tools::WebSearchParams;

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/search")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn page(name: &str) -> Reply {
    Reply::html(&fixture(name))
}

fn rss(name: &str) -> Reply {
    let mut r = Reply::html(&fixture(name));
    r.content_type = "application/rss+xml; charset=utf-8".into();
    r
}

/// Every engine the server knows points at `site`, so an engine a test forgot to serve answers
/// 404 locally instead of reaching the real host.
fn endpoints(site: &Site) -> Endpoints {
    let mut e = Endpoints::default();
    for (engine, path) in [
        ("ddg", "/ddg/"),
        ("ddg-html", "/ddg-html/"),
        ("bing", "/bing"),
        ("brave", "/brave"),
        ("news", "/news"),
    ] {
        e = e.with(engine, &site.url(path));
    }
    e
}

fn server(site: &Site) -> SvipallServer {
    support::isolate();
    let store = svipall_core::cache::Store::open_memory()
        .ok()
        .map(std::sync::Arc::new);
    let cfg = svipall_core::Config {
        request_min_interval_ms: 50,
        ..Default::default()
    };
    SvipallServer::with_store(None, cfg, None, store).with_search_endpoints(endpoints(site))
}

fn params(query: &str, engine: Option<&str>) -> WebSearchParams {
    WebSearchParams {
        query: query.into(),
        engine: engine.map(str::to_string),
        ..Default::default()
    }
}

fn attempt<'a>(out: &'a Value, engine: &str) -> &'a Value {
    out["attempts"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["engine"] == engine))
        .unwrap_or_else(|| panic!("no attempt for {engine}: {out}"))
}

fn urls(out: &Value) -> Vec<String> {
    out["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| r["url"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The lite endpoint's anomaly page used to parse as "0 results", which is how 307 of 331 real
/// searches came back empty with nothing to say why.
#[tokio::test]
async fn a_challenge_page_is_named_as_one_and_a_zero_answer_says_which_engines_were_blocked() {
    let site = Site::start(vec![
        ("/ddg/", page("ddg-lite.anomaly.html").with_status(202)),
        ("/ddg-html/", page("ddg-html.anomaly.html").with_status(202)),
        (
            "/bing",
            Reply::html("<html><body>Too many</body></html>").with_status(429),
        ),
        ("/brave", page("brave.captcha.html").with_status(429)),
        ("/news", rss("news.empty.xml")),
    ])
    .await;
    let out = server(&site)
        .search_json(params("rust async runtime", None))
        .await;
    assert_eq!(out["count"], 0, "{out}");
    assert_eq!(attempt(&out, "ddg")["outcome"], "challenge", "{out}");
    assert_eq!(attempt(&out, "ddg-html")["outcome"], "challenge", "{out}");
    assert_eq!(attempt(&out, "bing")["outcome"], "blocked", "{out}");
    assert_eq!(attempt(&out, "brave")["outcome"], "challenge", "{out}");
    assert_eq!(attempt(&out, "news")["outcome"], "empty", "{out}");
    let note = out["note"].as_str().unwrap_or_default();
    for word in ["ddg", "bing", "brave", "blocked", "empty", "web_route"] {
        assert!(note.contains(word), "note lacks {word:?}: {out}");
    }
}

/// A genuinely empty answer is not a block, and the caller is told which it was.
#[tokio::test]
async fn a_genuinely_empty_page_is_reported_as_empty() {
    let site = Site::start(vec![("/ddg/", page("ddg-lite.empty.html"))]).await;
    let out = server(&site)
        .search_json(params("\"xqzjvwplmkqqfrrtzzvwbbnhhx\"", Some("ddg")))
        .await;
    assert_eq!(out["count"], 0, "{out}");
    assert_eq!(attempt(&out, "ddg")["outcome"], "empty", "{out}");
    let note = out["note"].as_str().unwrap_or_default();
    assert!(note.contains("ddg") && note.contains("empty"), "{out}");
}

/// The engine that answered 429 on 148 of 148 attempts was asked every single time. Once it has
/// failed it is skipped for a while, by this server and by the next one, and the engine that worked
/// goes first.
#[tokio::test]
async fn a_failing_engine_is_skipped_next_time_even_by_a_new_server_and_the_working_one_goes_first()
{
    let site = Site::start(vec![
        (
            "/ddg/",
            Reply::html("<html><body>Too many</body></html>").with_status(429),
        ),
        ("/ddg-html/", page("ddg-html.results.html")),
    ])
    .await;
    let first = server(&site)
        .search_json(params("rust async runtime", None))
        .await;
    assert_eq!(first["engine"], "ddg-html", "{first}");
    assert_eq!(site.hits("/ddg/"), 1);

    // A new server: the health was persisted, not kept in this one's memory.
    let second = server(&site)
        .search_json(params("rust async runtime", None))
        .await;
    assert_eq!(second["engine"], "ddg-html", "{second}");
    assert_eq!(
        site.hits("/ddg/"),
        1,
        "the failing engine was asked again: {second}"
    );
    assert_eq!(attempt(&second, "ddg")["outcome"], "skipped", "{second}");
    assert_eq!(
        second["attempts"][0]["engine"], "ddg-html",
        "the engine that worked is asked first: {second}"
    );

    // Named explicitly, it is asked anyway.
    let named = server(&site)
        .search_json(params("rust async runtime", Some("ddg")))
        .await;
    assert_eq!(site.hits("/ddg/"), 2, "{named}");
    assert_eq!(attempt(&named, "ddg")["outcome"], "blocked", "{named}");
}

/// One challenge is not a pattern: the anomaly page was measured to follow the query, so the next
/// search asks that engine again. Two in a row and it rests.
#[tokio::test]
async fn one_challenge_does_not_bench_an_engine_and_two_in_a_row_do() {
    let site = Site::start(vec![
        ("/ddg/", page("ddg-lite.anomaly.html").with_status(202)),
        ("/ddg-html/", page("ddg-html.anomaly.html").with_status(202)),
    ])
    .await;
    let s = server(&site);
    let first = s.search_json(params("rust async runtime", None)).await;
    assert_eq!(
        attempt(&first, "ddg-html")["outcome"],
        "challenge",
        "{first}"
    );
    let second = s.search_json(params("rust async runtime", None)).await;
    assert_eq!(
        attempt(&second, "ddg-html")["outcome"],
        "challenge",
        "asked again after one challenge: {second}"
    );
    let third = s.search_json(params("rust async runtime", None)).await;
    assert_eq!(
        attempt(&third, "ddg-html")["outcome"],
        "skipped",
        "resting after two: {third}"
    );
}

/// Result links arrive wrapped in the engine's click redirect; the caller wants the page.
#[tokio::test]
async fn redirect_wrapped_result_links_are_unwrapped_to_the_real_url() {
    let site = Site::start(vec![
        ("/bing", page("bing.results.html")),
        ("/news", rss("news.results.xml")),
    ])
    .await;
    let s = server(&site);
    let web = s
        .search_json(params("rust async runtime", Some("bing")))
        .await;
    let got = urls(&web);
    assert!(!got.is_empty(), "{web}");
    assert!(
        got.iter().all(|u| !u.contains("/ck/a?")),
        "a wrapped link survived: {got:?}"
    );
    assert_eq!(got[0], "https://rust-lang.org/", "{got:?}");

    let news = s
        .search_json(params("rust async runtime", Some("news")))
        .await;
    let got = urls(&news);
    assert!(!got.is_empty(), "{news}");
    assert!(
        got.iter().all(|u| !u.contains("apiclick")),
        "a wrapped link survived: {got:?}"
    );
    assert!(
        got.iter()
            .any(|u| u == "https://github.com/transact-rs/sqlx"),
        "{got:?}"
    );
}

/// The request pinned `cc=US&setlang=en` whatever the query was. Language and region now come
/// from the parameters, or from the query's own language when none is given.
#[tokio::test]
async fn locale_comes_from_the_parameters_or_from_the_query_language() {
    let site = Site::start(vec![
        ("/bing?q=pan+casero&setlang=es&cc=MX", page("bing.es.html")),
        (
            "/bing?q=c%C3%B3mo+hacer+pan+casero+sin+levadura&setlang=es",
            page("bing.es.html"),
        ),
    ])
    .await;
    let s = server(&site);
    let explicit = s
        .search_json(WebSearchParams {
            query: "pan casero".into(),
            engine: Some("bing".into()),
            lang: Some("es".into()),
            region: Some("mx".into()),
            ..Default::default()
        })
        .await;
    assert!(explicit["count"].as_u64().unwrap_or(0) > 0, "{explicit}");
    assert_eq!(explicit["locale"]["from"], "params", "{explicit}");

    let inferred = s
        .search_json(params("cómo hacer pan casero sin levadura", Some("bing")))
        .await;
    assert!(inferred["count"].as_u64().unwrap_or(0) > 0, "{inferred}");
    assert_eq!(inferred["locale"]["lang"], "es", "{inferred}");
    assert_eq!(inferred["locale"]["from"], "query", "{inferred}");
}

/// A long query that brings back pages sharing no word with it is tried once more, shortened; and
/// results that share nothing with the query are flagged, never dropped.
#[tokio::test]
async fn off_topic_results_are_flagged_and_a_long_query_is_retried_shorter() {
    let long = "rust async runtime comparison for embedded hobby projects";
    let site = Site::start(vec![
        (
            "/bing?q=rust+async+runtime+comparison+for+embedded+hobby+projects&setlang=en",
            page("bing.offtopic.html"),
        ),
        (
            "/bing?q=rust+async+runtime+comparison&setlang=en",
            page("bing.results.html"),
        ),
        ("/bing?q=xqzjvwplmk", page("bing.offtopic.html")),
    ])
    .await;
    let s = server(&site);
    let out = s.search_json(params(long, Some("bing"))).await;
    assert_eq!(
        out["retried_with"], "rust async runtime comparison",
        "{out}"
    );
    assert_eq!(urls(&out)[0], "https://rust-lang.org/", "{out}");

    let junk = s.search_json(params("xqzjvwplmk", Some("bing"))).await;
    let results = junk["results"].as_array().cloned().unwrap_or_default();
    assert!(results.len() >= 5, "nothing is withheld: {junk}");
    assert!(
        results.iter().all(|r| r["off_topic"] == true),
        "every result shares nothing with the query: {junk}"
    );
    assert!(
        junk["note"]
            .as_str()
            .is_some_and(|n| n.contains("off topic")),
        "{junk}"
    );
}

/// Engines are paced per host like any other site: two searches in a row on one engine do not go
/// out back to back.
#[tokio::test]
async fn two_searches_on_one_engine_are_paced() {
    let site = Site::start(vec![("/ddg-html/", page("ddg-html.results.html"))]).await;
    support::isolate();
    let cfg = svipall_core::Config {
        request_min_interval_ms: 800,
        ..Default::default()
    };
    let s =
        SvipallServer::with_store(None, cfg, None, None).with_search_endpoints(endpoints(&site));
    let first = s
        .search_json(params("rust async runtime", Some("ddg-html")))
        .await;
    assert!(first["count"].as_u64().unwrap_or(0) > 0, "{first}");
    let t0 = std::time::Instant::now();
    let second = s
        .search_json(params("rust async runtime", Some("ddg-html")))
        .await;
    assert!(second["count"].as_u64().unwrap_or(0) > 0, "{second}");
    assert!(
        t0.elapsed() >= std::time::Duration::from_millis(700),
        "the second request was not paced: {:?}",
        t0.elapsed()
    );
}

/// A fetch of an engine's host that was answered 429 with a Retry-After holds that host, and a
/// search used to sleep through the hold inside the call, up to two minutes. Now the held engine is
/// skipped, says why, and the next engine answers.
#[tokio::test]
async fn an_engine_host_held_by_retry_after_is_skipped_not_waited_for() {
    support::isolate();
    let host = "engine-held.test";
    let site = Site::start(vec![
        (
            "http://engine-held.test/ping",
            Reply::html("<html><body>slow down</body></html>")
                .with_status(429)
                .header("retry-after", "100"),
        ),
        (
            "http://engine-held.test/ddg/",
            page("ddg-lite.results.html"),
        ),
        ("/ddg-html/", page("ddg-html.results.html")),
    ])
    .await;
    svipall_core::store::ROUTES.insert(host, &site.url(""));
    let endpoints = endpoints(&site).with("ddg", &format!("http://{host}/ddg/"));
    support::isolate();
    let cfg = svipall_core::Config {
        request_min_interval_ms: 1,
        http_engine: "reqwest".into(),
        max_tier: "http".into(),
        ..Default::default()
    };
    let s = SvipallServer::with_store(None, cfg, None, None).with_search_endpoints(endpoints);
    let held = s
        .fetch_json(svipall::tools::WebFetchParams {
            url: format!("http://{host}/ping"),
            robots: Some("ignore".into()),
            cache: Some("bypass".into()),
            ..Default::default()
        })
        .await
        .value;
    assert_eq!(held["status"], 429, "{held}");

    let t0 = std::time::Instant::now();
    let out = s.search_json(params("rust async runtime", None)).await;
    let named = s
        .search_json(params("rust async runtime", Some("ddg")))
        .await;
    let took = t0.elapsed();
    svipall_core::store::ROUTES.remove(host);
    assert!(
        took < std::time::Duration::from_secs(20),
        "the search sat out the hold: {took:?}"
    );
    assert_eq!(
        site.hits("http://engine-held.test/ddg/"),
        0,
        "the held host was asked: {out}"
    );
    let ddg = attempt(&out, "ddg");
    assert_eq!(ddg["outcome"], "skipped", "{out}");
    assert!(
        ddg["detail"].as_str().is_some_and(|d| d.contains("hold")),
        "{out}"
    );
    assert!(
        out["count"].as_u64().unwrap_or(0) > 0,
        "another engine answered: {out}"
    );
    assert_eq!(attempt(&named, "ddg")["outcome"], "skipped", "{named}");
    assert!(
        named["note"].as_str().is_some_and(|n| n.contains("hold")),
        "the zero answer says the engine was held: {named}"
    );
}

/// Two real Bing pages for one query, "python dataclass default factory list", taken on 2026-11-12
/// ten minutes apart with the same request. The first answers for "python" alone: every result
/// shares that one word and no other, and the same happened to every multi-word query asked in
/// that stretch, through a browser too. The second is the whole query answered. The first is
/// returned whole, marked off topic, and said to be a one-word answer; the second is left alone.
#[tokio::test]
async fn a_one_word_answer_to_a_long_query_is_named_as_one_and_kept() {
    let q = "python dataclass default factory list";
    let site = Site::start(vec![("/bing", page("bing.one-word.html"))]).await;
    let out = server(&site).search_json(params(q, Some("bing"))).await;
    let bing = attempt(&out, "bing");
    assert_eq!(bing["outcome"], "results", "{out}");
    let detail = bing["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("one word") && detail.contains("\"python\""),
        "{out}"
    );
    let results = out["results"].as_array().expect("results");
    assert!(results.len() >= 5, "nothing withheld: {out}");
    assert!(results.iter().all(|r| r["off_topic"] == true), "{out}");
    let note = out["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("one word") && note.contains("python"),
        "{out}"
    );

    let site = Site::start(vec![("/bing", page("bing.whole-query.html"))]).await;
    let out = server(&site).search_json(params(q, Some("bing"))).await;
    let bing = attempt(&out, "bing");
    assert_eq!(bing["outcome"], "results", "{out}");
    assert!(bing.get("detail").is_none(), "{out}");
    assert!(out.get("note").is_none(), "{out}");
    let results = out["results"].as_array().expect("results");
    assert!(
        results.iter().take(5).all(|r| r.get("off_topic").is_none()),
        "{out}"
    );
}

/// The Brave results page, as it came back from the real engine on 2026-11-12 for "rust async
/// runtime". The http tier was refused with 429 twice, twenty minutes apart, so this page was
/// fetched once through the browser tier: it is the document after the page's scripts ran, saved
/// as is except for the coordinates the engine guessed from this machine's address.
#[tokio::test]
async fn a_real_brave_results_page_is_read() {
    let site = Site::start(vec![("/brave", page("brave.results.html"))]).await;
    let out = server(&site)
        .search_json(params("rust async runtime", Some("brave")))
        .await;
    assert_eq!(attempt(&out, "brave")["outcome"], "results", "{out}");
    assert_eq!(out["engine"], "brave", "{out}");
    let first = &out["results"][0];
    assert_eq!(
        first["url"], "https://rust-lang.github.io/async-book/08_ecosystem/00_chapter.html",
        "{out}"
    );
    assert_eq!(
        first["title"], "The Async Ecosystem - Asynchronous Programming in Rust",
        "{out}"
    );
    assert!(
        first["snippet"]
            .as_str()
            .unwrap_or_default()
            .starts_with("The word"),
        "{out}"
    );
    let urls = urls(&out);
    assert!(urls.len() >= 8, "{out}");
    assert!(
        urls.iter().all(|u| !u.contains("search.brave.com")),
        "no engine-internal link is a result: {out}"
    );
}
