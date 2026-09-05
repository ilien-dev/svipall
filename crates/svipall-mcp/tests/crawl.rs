//! Crawling end to end, including the thing a crawler is judged on: surviving being killed.
//!
//! The resumption tests run two separate `SvipallServer`s over one database file, which is the real
//! shape of the failure — the process is gone, only the database is left.

mod support;

use serde_json::Value;
use std::sync::Arc;
use support::{Reply, Site};
use svipall_mcp::progress::{CrawlEvent, EventKind, ProgressSink};
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::WebCrawlParams;

/// A database file both "runs" share, deleted when the guard drops.
struct Db(std::path::PathBuf);

impl Db {
    fn new() -> Self {
        // A counter, not a timestamp: Windows clocks are coarse enough that two tests starting
        // together get the same nanosecond, and then they share a database and deadlock.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(support::isolate().join(format!("crawl-{n}.db")))
    }
    fn server(&self) -> SvipallServer {
        let store = svipall_core::cache::Store::open_at(&self.0).expect("open db");
        SvipallServer::with_store(
            None,
            svipall_core::Config::default(),
            None,
            Some(Arc::new(store)),
        )
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for ext in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
        }
    }
}

fn site_routes() -> Vec<(&'static str, Reply)> {
    vec![
        ("/", Reply::page("Index", &["/a", "/b", "/c", "/d", "/e"])),
        ("/a", Reply::page("Alpha", &["/f"])),
        ("/b", Reply::page("Bravo", &[])),
        ("/c", Reply::page("Charlie", &[])),
        ("/d", Reply::page("Delta", &[])),
        ("/e", Reply::page("Echo", &[])),
        ("/f", Reply::page("Foxtrot", &[])),
    ]
}

fn crawl(url: &str, max_pages: usize) -> WebCrawlParams {
    WebCrawlParams {
        url: url.into(),
        max_pages: Some(max_pages),
        max_depth: Some(3),
        mode: Some("http".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

fn urls(v: &Value) -> Vec<String> {
    v["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .map(|p| p["url"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test]
async fn a_crawl_follows_links_and_reports_why_it_stopped() {
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let out = db.server().crawl_json(crawl(&site.url("/"), 3)).await;

    assert_eq!(out["count"], 3, "{out}");
    assert_eq!(out["stopped_by"], "max_pages");
    assert!(out["crawl_id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(urls(&out).contains(&site.url("/")));
}

#[tokio::test]
async fn an_interrupted_crawl_resumes_without_refetching_what_it_had() {
    let site = Site::start(site_routes()).await;
    let db = Db::new();

    let first = db.server().crawl_json(crawl(&site.url("/"), 3)).await;
    let id = first["crawl_id"].as_str().expect("crawl_id").to_string();
    let done = urls(&first);
    assert_eq!(done.len(), 3);
    assert!(
        first["pending_links"].as_u64().unwrap_or(0) > 0,
        "nothing left to resume from: {first}"
    );

    // A different server, as if the process had been killed and started again.
    let second = db
        .server()
        .crawl_json(WebCrawlParams {
            url: String::new(),
            crawl_id: Some(id.clone()),
            max_pages: Some(7),
            ..Default::default()
        })
        .await;

    assert_eq!(second["crawl_id"], Value::String(id));
    assert_eq!(
        second["pages_before_resume"], 3,
        "the resumed crawl forgot what the first one fetched: {second}"
    );
    let more = urls(&second);
    for u in &more {
        assert!(
            !done.contains(u),
            "{u} was fetched twice across the interruption"
        );
    }
    let total: Vec<&String> = done.iter().chain(more.iter()).collect();
    assert_eq!(total.len(), 7, "the two halves should add up: {second}");

    // Every page of the site was requested exactly once, which is the whole claim.
    for path in ["/", "/a", "/b", "/c", "/d", "/e", "/f"] {
        assert_eq!(
            site.hits(path),
            1,
            "{path} was requested {} times",
            site.hits(path)
        );
    }
}

#[tokio::test]
async fn resuming_keeps_the_original_parameters() {
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let mut p = crawl(&site.url("/"), 2);
    p.include = Some("/a".into());
    let first = db.server().crawl_json(p).await;
    let id = first["crawl_id"].as_str().expect("id").to_string();

    let second = db
        .server()
        .crawl_json(WebCrawlParams {
            url: String::new(),
            crawl_id: Some(id),
            max_pages: Some(6),
            ..Default::default()
        })
        .await;

    // `include` came back from the database: only /a and what it links to are eligible.
    for u in urls(&second) {
        assert!(
            u.contains("/a") || u.contains("/f"),
            "{u} should have been filtered out by the stored include"
        );
    }
    assert_eq!(second["start_url"], site.url("/"), "{second}");
}

#[tokio::test]
async fn an_unknown_crawl_id_starts_a_crawl_rather_than_failing() {
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let mut p = crawl(&site.url("/"), 2);
    p.crawl_id = Some("does-not-exist".into());
    let out = db.server().crawl_json(p).await;

    assert_eq!(out["crawl_id"], "does-not-exist");
    assert_eq!(out["pages_before_resume"], 0);
    assert!(out["count"].as_u64().unwrap_or(0) >= 1, "{out}");
}

#[tokio::test]
async fn a_crawl_obeys_robots_by_default() {
    let mut routes = site_routes();
    routes.push((
        "/robots.txt",
        Reply::html("User-agent: *\nDisallow: /b\nDisallow: /c"),
    ));
    let site = Site::start(routes).await;
    let db = Db::new();
    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            url: site.url("/"),
            max_pages: Some(7),
            max_depth: Some(3),
            mode: Some("http".into()),
            ..Default::default()
        })
        .await;

    assert!(out["skipped_by_robots"].as_u64().unwrap_or(0) >= 2, "{out}");
    assert_eq!(site.hits("/b"), 0);
    assert_eq!(site.hits("/c"), 0);
}

#[tokio::test]
async fn identical_pages_are_reported_as_duplicates_instead_of_repeated() {
    let same = Reply::page("Same", &[]);
    let site = Site::start(vec![
        ("/", Reply::page("Index", &["/x", "/y"])),
        ("/x", same.clone()),
        ("/y", same),
    ])
    .await;
    let db = Db::new();
    let out = db.server().crawl_json(crawl(&site.url("/"), 3)).await;

    assert_eq!(out["duplicates_skipped"], 1, "{out}");
    let dup = out["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|p| p.get("duplicate_of").is_some())
        .expect("a duplicate entry");
    // ▲ Labelled, not emptied. This test used to assert the opposite — that a near-duplicate came
    // back as a four-field stub with its content dropped — which is the anti-discard contract
    // being broken by the crawl that wrote it, on by default. Two catalogue pages differing in one
    // price are near-duplicates and either one may be the one that was wanted; the token budget is
    // what caps a response, and `dedup` says what a page *is*.
    assert!(
        dup["content"].as_str().is_some_and(|c| c.contains("Same")),
        "a duplicate keeps its content and is told what it repeats: {dup}"
    );
    assert!(
        dup["similarity"].as_f64().is_some(),
        "and it is told how close: {dup}"
    );
}

/// Page two of a listing is not just another link: it is the rest of the answer. Without this the
/// crawl stops after page one and reports success.
#[tokio::test]
async fn a_paginated_listing_is_followed_past_the_first_page() {
    let site = Site::start(vec![
        ("/list", Reply::page("Page one", &[])),
        ("/list?page=2", Reply::page("Page two", &[])),
        ("/list?page=3", Reply::page("Page three", &[])),
    ])
    .await;
    let db = Db::new();
    let out = db
        .server()
        .crawl_json(crawl(&site.url("/list?page=1"), 3))
        .await;

    let fetched = urls(&out);
    assert!(
        fetched.iter().any(|u| u.contains("page=2")),
        "never reached page two: {fetched:?}"
    );
    assert_eq!(site.hits("/list"), 3, "should have walked three pages");
}

#[tokio::test]
async fn a_crawl_can_write_a_table_instead_of_filling_the_context() {
    // Two hundred pages through the model is the expensive way to copy a file: it reads every row,
    // pays for every row, and writes most of them back out to save them.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let out_dir = support::isolate();
    let path = out_dir.join("pages.csv");
    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            out_file: Some(path.to_string_lossy().to_string()),
            ..crawl(&site.url("/"), 3)
        })
        .await;

    assert!(
        out.get("pages").is_none(),
        "the rows came back anyway: {out}"
    );
    assert_eq!(out["format"], "csv", "{out}");
    let written = out["out_file"].as_str().expect("a path came back");
    let text = std::fs::read_to_string(written).expect("the file exists");
    assert!(text.starts_with("attempts,"), "{text}");
    assert!(
        text.contains("/a"),
        "every crawled page is in the file: {text}"
    );
    // Content carries newlines, correctly quoted, so lines() counts more than rows. What matters
    // is that the header names every column and the file is not empty.
    assert!(text.len() > 200, "{text}");
}

#[tokio::test]
async fn a_file_name_with_no_format_in_it_is_refused_rather_than_guessed() {
    // Guessing means a .txt full of CSV, or worse a .csv full of JSON, and whoever opens it next
    // has the bad afternoon.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            out_file: Some("pages.txt".into()),
            ..crawl(&site.url("/"), 2)
        })
        .await;
    assert!(out["out_file_error"].is_string(), "{out}");
    assert!(out.get("out_file").is_none(), "{out}");
}

/// A sitemap naming one page, dated years ago.
///
/// The `<loc>` is absolute and fixed, which a sitemap's is: the test server's port is not known
/// until it starts, so the URL in the sitemap deliberately is not one this server hosts. That is
/// the case being tested — the decision is made from the date and the cache, before any request.
const STALE_URL: &str = "http://example.invalid/a";

fn sitemap_site() -> Vec<(&'static str, Reply)> {
    let xml = "<?xml version=\"1.0\"?><urlset>\
        <url><loc>http://example.invalid/a</loc><lastmod>2020-01-01</lastmod></url>\
        </urlset>";
    vec![
        ("/", Reply::page("Index", &[])),
        ("/sitemap.xml", Reply::plain(xml)),
    ]
}

#[tokio::test]
async fn an_incremental_crawl_skips_what_the_site_says_has_not_moved() {
    // The second crawl of a site is almost entirely the first crawl again. A page the sitemap dates
    // to 2020, already read, is 395 of the 400 pages nobody needs to fetch twice.
    let site = Site::start(sitemap_site()).await;
    let db = Db::new();
    let store = svipall_core::cache::Store::open_at(&db.0).expect("open");
    // Already read, and more recently than the date the sitemap gives it.
    store
        .put(
            STALE_URL,
            STALE_URL,
            200,
            "http",
            None,
            None,
            "text/html",
            Some("Alpha"),
            "# Alpha",
            86_400,
            None,
        )
        .expect("stored");
    drop(store);

    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            since_last_crawl: Some(true),
            ..crawl(&site.url("/"), 10)
        })
        .await;

    assert_eq!(
        out["skipped_unchanged"], 1,
        "the page dated 2020 and already read should have been skipped: {out}"
    );
    // The seed is still crawled: an incremental run is a smaller crawl, never no crawl.
    assert_eq!(out["count"], 1, "{out}");
}

#[tokio::test]
async fn a_site_with_no_sitemap_still_crawls_normally() {
    // Every failure path here has to end at "crawl the ordinary way". A missing sitemap turning an
    // incremental crawl into no crawl would be the worst possible way to fail.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            since_last_crawl: Some(true),
            ..crawl(&site.url("/"), 3)
        })
        .await;
    assert_eq!(out["count"], 3, "{out}");
    assert_eq!(out["skipped_unchanged"], 0, "{out}");
}

#[tokio::test]
async fn one_crawl_can_cover_several_sites_under_one_budget() {
    // "Research these three sites" is one job with one budget, not three crawls whose totals the
    // caller has to add up.
    let a = Site::start(vec![
        ("/", Reply::page("A index", &["/a1", "/a2", "/a3", "/a4"])),
        ("/a1", Reply::page("A one", &[])),
        ("/a2", Reply::page("A two", &[])),
        ("/a3", Reply::page("A three", &[])),
        ("/a4", Reply::page("A four", &[])),
    ])
    .await;
    let b = Site::start(vec![
        ("/", Reply::page("B index", &["/b1"])),
        ("/b1", Reply::page("B one", &[])),
    ])
    .await;
    let db = Db::new();
    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            also: Some(vec![b.url("/")]),
            ..crawl(&a.url("/"), 4)
        })
        .await;

    let fetched = urls(&out);
    assert!(
        fetched.iter().any(|u| u.starts_with(b.url("").as_str())),
        "the second site was never reached: {fetched:?}"
    );
    // Four pages, two sites: neither may take more than two.
    let from_a = fetched.iter().filter(|u| u.starts_with(&a.url(""))).count();
    assert!(
        from_a <= 2,
        "one site spent more than its share: {from_a} of {fetched:?}"
    );
}

#[tokio::test]
async fn a_single_site_crawl_is_unchanged_by_the_sharing_rule() {
    // The share only exists when there is something to share with. Applying it to one domain would
    // silently halve every ordinary crawl.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let out = db.server().crawl_json(crawl(&site.url("/"), 5)).await;
    assert_eq!(out["count"], 5, "{out}");
}

// ---- what a crawl says while it runs -----------------------------------------------------------

/// A sink that keeps every event, so a test can read what a listener would have seen.
struct Recording(std::sync::Mutex<Vec<CrawlEvent>>);

impl ProgressSink for Recording {
    fn report<'a>(&'a self, e: &'a CrawlEvent) -> futures::future::BoxFuture<'a, ()> {
        self.0.lock().expect("lock").push(e.clone());
        Box::pin(async {})
    }
}

impl Recording {
    fn new() -> Self {
        Self(std::sync::Mutex::new(Vec::new()))
    }
    fn of(&self, kind: EventKind) -> Vec<CrawlEvent> {
        self.0
            .lock()
            .expect("lock")
            .iter()
            .filter(|e| e.kind == kind)
            .cloned()
            .collect()
    }
}

#[tokio::test]
async fn a_crawl_reports_every_page_rather_than_every_batch() {
    // This used to report once at the bottom of the while loop, which is once per depth level: a
    // max_depth=2 crawl said three things about its whole run. Enough to know a crawl was alive,
    // not enough to know where it was — and useless as the source of an event stream.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let sink = Recording::new();
    let out = db
        .server()
        .crawl_json_with(crawl(&site.url("/"), 5), Some(&sink))
        .await;

    assert_eq!(out["count"], 5, "{out}");
    assert_eq!(
        sink.of(EventKind::Page).len(),
        5,
        "one event per page, not per level"
    );
    assert_eq!(sink.of(EventKind::Started).len(), 1);
    assert_eq!(sink.of(EventKind::Finished).len(), 1);

    // The counters climb rather than repeating: a bar built from these must actually move.
    let done: Vec<usize> = sink
        .of(EventKind::Page)
        .iter()
        .map(|e| e.pages_done)
        .collect();
    assert_eq!(done, vec![1, 2, 3, 4, 5], "pages_done did not advance");
    let finished = sink.of(EventKind::Finished);
    assert_eq!(finished[0].stopped_by.as_deref(), Some("max_pages"));
}

#[tokio::test]
async fn a_progress_event_names_the_url_and_the_tier_that_answered_without_carrying_the_page() {
    // The stream must not cost what the result costs. A crawl of two hundred pages that put its
    // markdown in every event would send its whole output twice.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let sink = Recording::new();
    db.server()
        .crawl_json_with(crawl(&site.url("/"), 3), Some(&sink))
        .await;

    for e in sink.of(EventKind::Page) {
        assert!(e.url.starts_with("http"), "an event with no url: {e:?}");
        assert_eq!(e.tier.as_deref(), Some("http"), "{e:?}");
        assert_eq!(e.status, Some(200), "{e:?}");
        let wire = serde_json::to_string(&e).expect("serialise");
        assert!(
            wire.len() < 512,
            "an event grew too big to send per page: {wire}"
        );
    }
}

#[tokio::test]
async fn a_crawl_finishes_after_the_only_listener_hangs_up() {
    // `Progress` has claimed this in a comment since it was written — "a client that has stopped
    // listening must not take the crawl down with it" — and nothing checked it. Now that a sink can
    // be a channel with no receiver left, the claim has to be structural: `report` returns nothing,
    // so there is no error for a caller to propagate even if it wanted to.
    struct HungUp {
        seen: std::sync::atomic::AtomicUsize,
        tx: tokio::sync::broadcast::Sender<CrawlEvent>,
    }
    impl ProgressSink for HungUp {
        fn report<'a>(&'a self, e: &'a CrawlEvent) -> futures::future::BoxFuture<'a, ()> {
            self.seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Every receiver is gone, so this is an Err on every call. It must be survivable.
            let _ = self.tx.send(e.clone());
            Box::pin(async {})
        }
    }

    let (tx, rx) = tokio::sync::broadcast::channel(8);
    drop(rx);
    let sink = HungUp {
        seen: std::sync::atomic::AtomicUsize::new(0),
        tx,
    };

    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let out = db
        .server()
        .crawl_json_with(crawl(&site.url("/"), 3), Some(&sink))
        .await;

    assert_eq!(out["count"], 3, "the crawl did not finish: {out}");
    assert!(
        sink.seen.load(std::sync::atomic::Ordering::Relaxed) >= 5,
        "the sink stopped being called after the first failed send"
    );
}

#[tokio::test]
async fn a_resumed_crawl_does_not_tell_a_listener_it_is_starting_from_zero() {
    // A listener that joins a resumed job and reads `Started { pages_done: 0 }` draws a bar from
    // the beginning of work that is already half done.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let first = db.server().crawl_json(crawl(&site.url("/"), 3)).await;
    let id = first["crawl_id"].as_str().expect("crawl_id").to_string();

    let sink = Recording::new();
    db.server()
        .crawl_json_with(
            WebCrawlParams {
                crawl_id: Some(id),
                ..crawl(&site.url("/"), 6)
            },
            Some(&sink),
        )
        .await;

    let started = sink.of(EventKind::Started);
    assert_eq!(started.len(), 1);
    assert_eq!(
        started[0].pages_done, 3,
        "a resumed crawl announced itself as starting from nothing"
    );
}
