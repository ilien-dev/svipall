//! What a crawl does when a gate answers instead of the site.
//!
//! Its own test binary, and for a reason worth writing down: the reputation ledger is keyed by
//! host, `domain_from_url` drops the port, and every loopback test in this repository is served
//! from `127.0.0.1`. Spending an address's whole budget to exercise the gate therefore gates every
//! other test that happens to be running beside it. A separate binary is a separate process and a
//! separate ledger, which is the same reason `anti_discard.rs` stands apart.

mod support;

use std::sync::Arc;
use support::{Reply, Site};
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::WebCrawlParams;

/// A database file the two "runs" share, deleted when the guard drops.
struct Db(std::path::PathBuf);

impl Db {
    fn new() -> Self {
        Self(support::isolate().join("reputation-crawl.db"))
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
        ("/", Reply::page("Index", &["/a", "/b"])),
        ("/a", Reply::page("Alpha", &[])),
        ("/b", Reply::page("Bravo", &[])),
    ]
}

/// A URL a gate declined to request is not a URL the crawl has fetched.
///
/// `mark_done` is what a resume reads to decide it never has to ask again, and it used to be
/// handed the whole batch — refusals included. A page held back by a gate was therefore lost for
/// that crawl id, permanently, while the summary reported `max_pages` as if nothing had happened.
/// That is content withheld with a success report on top.
#[tokio::test]
async fn a_page_the_crawl_never_got_is_not_marked_done_and_the_crawl_says_why() {
    let site = Site::start(site_routes()).await;
    let db = Db::new();

    // Spend the address's whole standing with the site before the crawl starts, so the gate at the
    // top of `fetch_inner` answers without a request.
    let domain = svipall_core::domain_from_url(&site.url("/"));
    svipall_core::reputation::add(&domain, None, svipall_core::reputation::budget() * 2.0);

    let out = db
        .server()
        .crawl_json(WebCrawlParams {
            url: site.url("/"),
            max_pages: Some(3),
            max_depth: Some(2),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await;

    assert_eq!(
        site.hits("/"),
        0,
        "the gate answered before a request, so nothing should have gone out"
    );
    assert_eq!(
        out["stopped_by"], "over_budget",
        "a crawl that got nothing must say why rather than report max_pages: {out}"
    );
    assert_eq!(
        out["refused_without_asking"], 1,
        "and it must say how many URLs it declined to ask for: {out}"
    );

    let id = out["crawl_id"].as_str().expect("crawl_id").to_string();
    let saved = svipall_core::cache::Store::open_at(&db.0)
        .expect("open db")
        .load_crawl(&id)
        .expect("the crawl was saved");
    assert!(
        saved.done.is_empty(),
        "a page that was never requested must not be marked done: {:?}",
        saved.done
    );
    assert!(
        saved.pending.iter().any(|(u, _, _)| u == &site.url("/")),
        "and it must go back on the frontier so a resume can pick it up: {:?}",
        saved.pending
    );

    // The whole claim, end to end: with the standing back, the resume actually fetches the page
    // the gate held back rather than skipping it forever.
    svipall_core::reputation::clear(&domain);
    let resumed = db
        .server()
        .crawl_json(WebCrawlParams {
            url: String::new(),
            crawl_id: Some(id),
            max_pages: Some(2),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await;
    assert!(
        site.hits("/") > 0,
        "the resume must ask for the page the gate held back: {resumed}"
    );
}
