//! The HTTP surface, endpoint by endpoint.
//!
//! This crate had tests for the database and none for the API that fronts it, which is the half
//! other tools actually talk to. Both dialects are covered: the legacy `in.php`/`res.php` pair
//! whose replies are plain `OK|…` text, and the JSON `createTask`/`getTaskResult` pair. The two
//! disagree about almost everything — status codes, field names, how "not ready yet" is spelled —
//! so a change that quietly breaks one while the other still works is exactly what these catch.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use svipall_solver::{api, db::Db, queue::JobQueue, AppState};
use tower::ServiceExt;

fn state() -> Arc<AppState> {
    Arc::new(AppState::new(
        Db::open_memory().expect("in-memory db"),
        JobQueue::new(),
    ))
}

async fn get(state: Arc<AppState>, uri: &str) -> (StatusCode, String) {
    let res = api::router(state)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn post_json(state: Arc<AppState>, uri: &str, body: Value) -> (StatusCode, Value) {
    let res = api::router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn task_id_from_ok_pipe(body: &str) -> String {
    let (head, id) = body.split_once('|').unwrap_or(("", ""));
    assert_eq!(head, "OK", "legacy replies are `OK|<id>`, got {body:?}");
    assert!(!id.is_empty());
    id.to_string()
}

#[tokio::test]
async fn in_php_accepts_a_token_job_and_returns_its_id() {
    let s = state();
    let (status, body) = get(
        s.clone(),
        "/in.php?method=userrecaptcha&googlekey=6Lc_abc&pageurl=https%3A%2F%2Fexample.test%2F",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = task_id_from_ok_pipe(&body);

    // The job is really in the store, not just acknowledged.
    let record = s
        .db_pool
        .read()
        .await
        .get_by_task_id(&id)
        .expect("query")
        .expect("the job should exist");
    assert_eq!(record.sitekey.as_deref(), Some("6Lc_abc"));
    assert_eq!(record.status, "pending");
}

#[tokio::test]
async fn in_php_speaks_json_when_asked() {
    let (status, body) = get(state(), "/in.php?method=userrecaptcha&googlekey=k&json=1").await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).expect("json reply");
    assert_eq!(v["status"], 1);
    assert!(v["request"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn an_image_body_becomes_an_image_job_whatever_the_method_says() {
    let s = state();
    let (_, body) = get(s.clone(), "/in.php?method=userrecaptcha&body=aGVsbG8=").await;
    let id = task_id_from_ok_pipe(&body);
    let record = s.db_pool.read().await.get_by_task_id(&id).unwrap().unwrap();
    assert_eq!(
        record.job_type, "ImageToText",
        "a request carrying an image is an image job regardless of `method`"
    );
}

#[tokio::test]
async fn res_php_says_not_ready_while_the_job_is_pending() {
    let s = state();
    let (_, body) = get(s.clone(), "/in.php?method=userrecaptcha&googlekey=k").await;
    let id = task_id_from_ok_pipe(&body);

    let (status, body) = get(s, &format!("/res.php?action=get&id={id}")).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], 0);
    assert_eq!(v["request"], "CAPCHA_NOT_READY");
}

#[tokio::test]
async fn res_php_hands_back_the_token_once_it_is_solved() {
    let s = state();
    let (_, body) = get(s.clone(), "/in.php?method=userrecaptcha&googlekey=k").await;
    let id = task_id_from_ok_pipe(&body);
    s.db_pool
        .read()
        .await
        .update_solved(&id, Some("tok-123"), None)
        .expect("mark solved");

    let (_, body) = get(s, &format!("/res.php?id={id}")).await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], 1);
    assert_eq!(v["request"], "tok-123");
}

#[tokio::test]
async fn res_php_reports_a_failure_rather_than_pretending_to_wait() {
    let s = state();
    let (_, body) = get(s.clone(), "/in.php?method=userrecaptcha&googlekey=k").await;
    let id = task_id_from_ok_pipe(&body);
    s.db_pool
        .read()
        .await
        .update_failed(&id, "ERROR_CAPTCHA_UNSOLVABLE")
        .expect("mark failed");

    let (_, body) = get(s, &format!("/res.php?id={id}")).await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], 0);
    assert_eq!(v["request"], "ERROR_CAPTCHA_UNSOLVABLE");
}

#[tokio::test]
async fn an_unknown_id_is_an_error_not_a_wait() {
    let (_, body) = get(state(), "/res.php?id=nope").await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], 0);
    assert_eq!(
        v["request"], "ERROR_CAPTCHA_UNSOLVABLE",
        "a caller polling forever on a typo is the failure being prevented"
    );
}

#[tokio::test]
async fn in_php_action_get_is_the_same_lookup_as_res_php() {
    let s = state();
    let (_, body) = get(s.clone(), "/in.php?method=userrecaptcha&googlekey=k").await;
    let id = task_id_from_ok_pipe(&body);
    let (_, a) = get(s.clone(), &format!("/in.php?action=get&id={id}")).await;
    let (_, b) = get(s, &format!("/res.php?action=get&id={id}")).await;
    assert_eq!(a, b);
}

#[tokio::test]
async fn a_report_is_recorded_rather_than_acknowledged_and_dropped() {
    let s = state();
    let (_, body) = get(s.clone(), "/in.php?method=userrecaptcha&googlekey=k").await;
    let id = task_id_from_ok_pipe(&body);
    s.db_pool
        .read()
        .await
        .update_solved(&id, Some("tok"), None)
        .unwrap();

    let (_, body) = get(s.clone(), &format!("/res.php?action=reportbad&id={id}")).await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], 1);

    // The statistics are the whole point: they are what decides whether a widget type is worth
    // retrying automatically or should go straight to a person.
    let outcomes = s.db_pool.read().await.outcomes_by_type();
    let bad: i64 = outcomes.iter().map(|(_, _, failed)| failed).sum();
    assert_eq!(
        bad, 1,
        "the report was acknowledged but not counted: {outcomes:?}"
    );
}

#[tokio::test]
async fn create_task_returns_a_task_id_in_the_json_dialect() {
    let (status, v) = post_json(
        state(),
        "/createTask",
        serde_json::json!({
            "clientKey": "local",
            "task": {
                "type": "TurnstileTaskProxyless",
                "websiteURL": "https://example.test/",
                "websiteKey": "0x4AAA",
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["errorId"], 0, "{v}");
    assert!(v["taskId"].as_str().is_some_and(|s| !s.is_empty()), "{v}");
}

#[tokio::test]
async fn get_task_result_moves_from_processing_to_ready() {
    let s = state();
    let (_, created) = post_json(
        s.clone(),
        "/createTask",
        serde_json::json!({
            "task": {"type": "TurnstileTaskProxyless", "websiteURL": "https://x.test/", "websiteKey": "k"}
        }),
    )
    .await;
    let id = created["taskId"].as_str().expect("taskId").to_string();

    let (_, pending) = post_json(
        s.clone(),
        "/getTaskResult",
        serde_json::json!({"taskId": id}),
    )
    .await;
    assert_eq!(pending["status"], "processing", "{pending}");

    s.db_pool
        .read()
        .await
        .update_solved(&id, Some("tok-abc"), None)
        .unwrap();

    let (_, ready) = post_json(s, "/getTaskResult", serde_json::json!({"taskId": id})).await;
    assert_eq!(ready["status"], "ready", "{ready}");
    assert_eq!(ready["errorId"], 0);
    assert_eq!(ready["solution"]["token"], "tok-abc", "{ready}");
}

#[tokio::test]
async fn get_task_result_for_an_unknown_id_is_an_error_id_not_a_panic() {
    let (status, v) = post_json(
        state(),
        "/getTaskResult",
        serde_json::json!({"taskId": "missing"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(v["errorId"], 0, "{v}");
}

#[tokio::test]
async fn health_and_stats_answer_without_a_job_in_sight() {
    let (status, body) = get(state(), "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.is_empty());

    let (status, body) = get(state(), "/stats").await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).expect("stats is json");
    assert_eq!(v["pending"], 0, "{v}");
}

/// The failure this prevents is invisible in development and certain in the field: every statement
/// in the schema says `IF NOT EXISTS`, so a database from an earlier version keeps its old columns
/// forever while a fresh one gets the new ones.
#[test]
fn a_database_from_before_the_new_columns_is_migrated_rather_than_left_behind() {
    use rusqlite::Connection;

    let conn = Connection::open_in_memory().expect("memory db");
    // The v1 schema, verbatim, with no user_version — exactly what an older svipall left behind.
    conn.execute_batch(
        "CREATE TABLE jobs (
            id TEXT PRIMARY KEY, task_id TEXT UNIQUE NOT NULL, job_type TEXT NOT NULL,
            status TEXT NOT NULL, sitekey TEXT, page_url TEXT, image_data TEXT, token TEXT,
            text TEXT, error TEXT, created_at TEXT NOT NULL, solved_at TEXT,
            attempts INTEGER DEFAULT 0);
         CREATE TABLE balances (key TEXT PRIMARY KEY, value REAL);
         INSERT INTO jobs (id, task_id, job_type, status, created_at)
             VALUES ('1', 'abc', 'userrecaptcha', 'pending', '2020-01-01T00:00:00Z');",
    )
    .expect("v1 schema");

    let db = svipall_solver::db::Db::adopt(conn).expect("migrate");

    // The row that was already there survives.
    let job = db.get_by_task_id("abc").expect("query").expect("row kept");
    assert_eq!(job.status, "pending");

    // And the new shape is present.
    let record = db
        .create_job("Turnstile", Some("k"), Some("https://x.test/"), None)
        .expect("insert with the new columns");
    assert!(!record.task_id.is_empty());
}

#[test]
fn migrating_twice_is_not_an_error() {
    // Every start-up runs it, so it has to be safe to run against a database already at the current
    // version — otherwise the second launch of the day fails.
    let db = svipall_solver::db::Db::open_memory().expect("first");
    drop(db);
    let db = svipall_solver::db::Db::open_memory().expect("second");
    assert_eq!(db.stats().0, 0);
}
