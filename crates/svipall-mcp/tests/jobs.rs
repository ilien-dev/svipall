//! Work that outlives the request that asked for it.
//!
//! The claim under test is not that a crawl runs — `crawl.rs` covers that — but that a crawl nobody
//! is waiting on can be found, followed, stopped and, after the process that was running it is gone,
//! picked up where it stopped rather than started again.
//!
//! The resumability was already there: `crawl_queue` has survived a kill since crawls were written.
//! What these defend is the part that was missing — being able to *tell* that a crawl died, because
//! `crawl.status` is set to `running` per batch and nothing ever clears it.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use support::{Reply, Site};
use svipall_mcp::jobs::{JobKind, JobRunner};
use svipall_mcp::rest;
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::WebCrawlParams;
use tower::ServiceExt;

const KEY: &str = "a-key-that-is-long-enough";

/// A database file every "run" in a test shares, deleted when the guard drops. The same shape
/// `crawl.rs` uses, because the failure it models is the same one: the process is gone and only the
/// database is left.
struct Db(std::path::PathBuf);

impl Db {
    fn new() -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(support::isolate().join(format!("jobs-{n}.db")))
    }
    fn server(&self) -> SvipallServer {
        let store = svipall_core::cache::Store::open_at(&self.0).expect("open db");
        let cfg = svipall_core::Config {
            max_tier: "http".into(),
            ..Default::default()
        };
        SvipallServer::with_store(None, cfg, None, Some(Arc::new(store)))
    }
    fn store(&self) -> svipall_core::cache::Store {
        svipall_core::cache::Store::open_at(&self.0).expect("open db")
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

/// Wait for a job to reach a terminal state, or fail the test rather than hang CI.
async fn settle(store: &svipall_core::cache::Store, id: &str) -> String {
    for _ in 0..600 {
        if let Some(row) = store.job(id) {
            if matches!(
                row.state.as_str(),
                "finished" | "stopped" | "cancelled" | "failed"
            ) {
                return row.state;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job {id} never finished");
}

// ---- the runner ---------------------------------------------------------------------------------

#[tokio::test]
async fn submitting_a_crawl_returns_an_id_before_a_single_page_is_fetched() {
    // The point of a job: the caller is answered now, and the work happens after.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let runner = JobRunner::new(db.server(), 2);
    let id = runner
        .submit(JobKind::Crawl(Box::new(crawl(&site.url("/"), 3))))
        .expect("submit");

    assert_eq!(site.hits("/"), 0, "the crawl started before it was queued");
    assert_eq!(db.store().job(&id).expect("job").state, "queued");
}

#[tokio::test]
async fn the_job_id_is_the_crawl_id_so_there_is_only_one_handle_to_learn() {
    // A separate identity would mean two ids in every log line, a join on every poll, and a client
    // that has to learn "job 7 is crawl abc" before it can resume anything.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let runner = JobRunner::new(db.server(), 2);
    runner.start();
    let id = runner
        .submit(JobKind::Crawl(Box::new(crawl(&site.url("/"), 3))))
        .expect("submit");
    settle(&db.store(), &id).await;

    assert!(
        db.store().load_crawl(&id).is_some(),
        "the crawl is not filed under the job's id"
    );
    let result: Value =
        serde_json::from_str(&db.store().job_result(&id).expect("result")).expect("json");
    assert_eq!(result["crawl_id"], id);
    assert_eq!(result["count"], 3);
}

#[tokio::test]
async fn a_cancelled_job_stops_fetching() {
    // Fails if `DELETE` only marks the row: the crawl would run to its budget and report success
    // on work somebody had already called off.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let runner = JobRunner::new(db.server(), 2);
    runner.start();
    let id = runner
        .submit(JobKind::Crawl(Box::new(crawl(&site.url("/"), 7))))
        .expect("submit");

    // Cancel as soon as it is running, which is well before seven pages.
    for _ in 0..200 {
        if db.store().job(&id).is_some_and(|r| r.state == "running") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    runner.cancel(&id).expect("the job exists");

    let state = settle(&db.store(), &id).await;
    assert_eq!(state, "cancelled", "the job did not stop when asked");
    let result: Value =
        serde_json::from_str(&db.store().job_result(&id).expect("result")).expect("json");
    assert_eq!(result["stopped_by"], "cancelled");
    assert!(
        result["count"].as_u64().unwrap_or(99) < 7,
        "the crawl ran to its budget anyway: {result}"
    );
}

#[tokio::test]
async fn a_cancelled_crawl_keeps_its_frontier_so_it_can_be_picked_up_again() {
    // Cooperative cancellation earns its keep here. A crawl that left through `abort` would skip
    // `persist_crawl` and leak a browser page; leaving through the ordinary exit costs nothing and
    // keeps the work that was already done.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let runner = JobRunner::new(db.server(), 2);
    runner.start();
    let id = runner
        .submit(JobKind::Crawl(Box::new(crawl(&site.url("/"), 7))))
        .expect("submit");
    for _ in 0..200 {
        if db.store().job(&id).is_some_and(|r| r.state == "running") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    runner.cancel(&id).expect("the job exists");
    settle(&db.store(), &id).await;

    let resumable = db.store().resumable_crawls();
    assert!(
        resumable
            .iter()
            .any(|(cid, _, pending)| cid == &id && *pending > 0),
        "a cancelled crawl lost its frontier: {resumable:?}"
    );
}

#[tokio::test]
async fn a_killed_async_crawl_resumes_rather_than_starting_over() {
    // The headline. Runner A does some of the work and its process disappears; runner B, over a new
    // server on the same file, adopts the row as `interrupted` — which is the fact the crawl table
    // could never state — and a resume finishes the site without fetching anything twice.
    let site = Site::start(site_routes()).await;
    let db = Db::new();

    // Run A: three pages, then the "process" is gone. The job row is left saying `running`, which
    // is exactly the state a kill leaves behind.
    let first = db.server().crawl_json(crawl(&site.url("/"), 3)).await;
    let id = first["crawl_id"].as_str().expect("crawl_id").to_string();
    let store = db.store();
    store.create_job(&id, "crawl", "", "{}").expect("create");
    store.start_job(&id, "the-run-that-died").expect("claim");

    // Run B, in a fresh process: nothing has been heard from that owner.
    let runner = JobRunner::new(db.server(), 2);
    assert_eq!(
        store.adopt_orphaned_jobs("this-run", 0),
        1,
        "the killed job was not recognised as orphaned"
    );
    assert_eq!(store.job(&id).expect("job").state, "interrupted");

    runner.start();
    let resumed = runner
        .submit(JobKind::Crawl(Box::new(WebCrawlParams {
            crawl_id: Some(id.clone()),
            max_pages: Some(7),
            ..crawl(&site.url("/"), 7)
        })))
        .expect("submit");
    settle(&store, &resumed).await;

    // Every path of the fixture site was requested exactly once, across both runs.
    for path in ["/", "/a", "/b", "/c", "/d", "/e", "/f"] {
        assert_eq!(
            site.hits(path),
            1,
            "{path} was requested {} times across the interruption",
            site.hits(path)
        );
    }
}

#[tokio::test]
async fn two_crawls_of_one_site_do_not_run_at_the_same_time() {
    // One address, one reputation with that host. Two jobs on one site would double the request
    // rate at exactly the place this project has least room to spend it.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let runner = JobRunner::new(db.server(), 4);
    runner.start();
    let a = runner
        .submit(JobKind::Crawl(Box::new(crawl(&site.url("/"), 4))))
        .expect("submit");
    let b = runner
        .submit(JobKind::Crawl(Box::new(crawl(&site.url("/a"), 2))))
        .expect("submit");

    let store = db.store();
    let mut ever_two = false;
    for _ in 0..600 {
        if store.jobs(Some("running"), 10).len() > 1 {
            ever_two = true;
            break;
        }
        if store.job(&a).is_some_and(|r| r.finished_at.is_some())
            && store.job(&b).is_some_and(|r| r.finished_at.is_some())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!ever_two, "two crawls of one site ran at the same time");
    settle(&store, &a).await;
    settle(&store, &b).await;
}

// ---- the endpoints ------------------------------------------------------------------------------

fn app(db: &Db) -> (axum::Router, JobRunner) {
    let s = db.server();
    let runner = JobRunner::new(s.clone(), 2);
    (rest::router(s, runner.clone(), KEY, "127.0.0.1"), runner)
}

fn signed(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8788")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.oneshot(req).await.expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 8 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_synchronous_crawl_over_http_returns_exactly_what_the_tool_returns() {
    // Sync stays the default, and it is the same object the CLI prints and the model reads. One
    // shape across three ways of driving the same server.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let (app, _r) = app(&db);
    let (status, body) = send(
        app,
        signed(
            "POST",
            "/v1/crawl",
            json!({"url": site.url("/"), "max_pages": 3, "mode": "http", "robots": "ignore"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 3, "{body}");
    assert!(body["pages"].is_array());
}

#[tokio::test]
async fn an_asynchronous_crawl_answers_202_with_an_id_that_can_be_polled_immediately() {
    // The race worth designing out: a 202 hands back an id and the client's first poll 404s because
    // the runner had not picked the job up yet.
    let site = Site::start(site_routes()).await;
    let db = Db::new();
    let (app, _r) = app(&db);
    let res = app
        .clone()
        .oneshot(signed(
            "POST",
            "/v1/crawl",
            json!({"url": site.url("/"), "max_pages": 3, "async": true}),
        ))
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert!(
        res.headers().get("location").is_some(),
        "202 must say where the job is"
    );
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    let id = body["job_id"].as_str().expect("job_id").to_string();
    assert_eq!(body["state"], "queued");

    let (status, row) = send(app, signed("GET", &format!("/v1/jobs/{id}"), json!({}))).await;
    assert_eq!(status, StatusCode::OK, "the id it just handed out 404s");
    assert_eq!(row["id"], id);
}

#[tokio::test]
async fn polling_a_job_that_does_not_exist_is_a_404_rather_than_an_empty_job() {
    let db = Db::new();
    let (app, _r) = app(&db);
    let (status, body) = send(app, signed("GET", "/v1/jobs/nope", json!({}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.get("error").is_some(), "{body}");
}

#[tokio::test]
async fn a_listing_never_carries_the_pages_a_job_produced() {
    // Ten finished two-hundred-page crawls would otherwise be tens of megabytes of pages nobody
    // asked for a second time.
    let db = Db::new();
    let store = db.store();
    store.create_job("j1", "crawl", "", "{}").expect("create");
    store.start_job("j1", "run").expect("claim");
    let summary = format!(r#"{{"count":1,"pages":["{}"]}}"#, "y".repeat(5000));
    store
        .finish_job("j1", "finished", Some(&summary), None)
        .expect("finish");

    let (app, _r) = app(&db);
    let (status, body) = send(app, signed("GET", "/v1/jobs", json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    let wire = body.to_string();
    assert!(!wire.contains("yyyy"), "a listing carried the pages");
}

#[tokio::test]
async fn deleting_a_job_that_already_finished_is_not_an_error() {
    // A caller wants to know whether it stopped a running crawl or reaped a finished one, and both
    // are legitimate answers to the same request.
    let db = Db::new();
    let store = db.store();
    store.create_job("j1", "crawl", "", "{}").expect("create");
    store.start_job("j1", "run").expect("claim");
    store
        .finish_job("j1", "finished", Some("{}"), None)
        .expect("finish");

    let (app, _r) = app(&db);
    let (status, body) = send(app.clone(), signed("DELETE", "/v1/jobs/j1", json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "finished");
    assert_eq!(body["cancel_requested"], false);

    let (status, _) = send(app, signed("DELETE", "/v1/jobs/nope", json!({}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_late_subscriber_is_told_where_the_job_actually_is_before_it_is_told_anything_else() {
    // The one way this stream can lie: a subscriber that joins at page forty and infers "this
    // started at zero" from the first live event draws a bar from the beginning of work that is
    // already most of the way done. The mandatory snapshot frame makes that impossible.
    let db = Db::new();
    let store = db.store();
    store.create_job("j1", "crawl", "", "{}").expect("create");
    store.start_job("j1", "run").expect("claim");
    store
        .save_crawl("j1", "https://x.test/", "{}", "running", 40, None)
        .expect("crawl");

    let (app, _r) = app(&db);
    let res = app
        .oneshot(signed("GET", "/v1/jobs/j1/stream", json!({})))
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::OK);

    // Read frame by frame: `to_bytes` on a live SSE body never returns.
    let mut stream = res.into_body().into_data_stream();
    let chunk = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures::StreamExt::next(&mut stream),
    )
    .await
    .expect("the stream said nothing in five seconds")
    .expect("a frame")
    .expect("bytes");
    let text = String::from_utf8_lossy(&chunk).to_string();

    assert!(
        text.contains("event: snapshot"),
        "the first frame was not a snapshot: {text}"
    );
    assert!(
        text.contains("\"pages_done\":40"),
        "the snapshot did not say where the job actually is: {text}"
    );
}

#[tokio::test]
async fn subscribing_to_a_job_that_already_finished_ends_the_stream_instead_of_hanging() {
    // A job that is not running in this process has no channel to join. Saying where it ended and
    // closing is an answer; waiting forever on a stream that will never carry anything is not.
    let db = Db::new();
    let store = db.store();
    store.create_job("j1", "crawl", "", "{}").expect("create");
    store.start_job("j1", "run").expect("claim");
    store
        .finish_job("j1", "finished", Some(r#"{"count":3}"#), None)
        .expect("finish");

    let (app, _r) = app(&db);
    let res = app
        .oneshot(signed("GET", "/v1/jobs/j1/stream", json!({})))
        .await
        .expect("response");

    let body = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        axum::body::to_bytes(res.into_body(), 1 << 20),
    )
    .await
    .expect("the stream never ended")
    .expect("body");
    let text = String::from_utf8_lossy(&body).to_string();
    assert!(text.contains("event: snapshot"), "{text}");
    assert!(text.contains("event: done"), "{text}");
    assert!(text.contains("finished"), "{text}");
}

#[tokio::test]
async fn streaming_a_job_that_does_not_exist_is_a_404_rather_than_an_empty_stream() {
    let db = Db::new();
    let (app, _r) = app(&db);
    let res = app
        .oneshot(signed("GET", "/v1/jobs/nope/stream", json!({})))
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
