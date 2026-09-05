//! The HTTP surface, and the two things it must never get wrong.
//!
//! The first is the gate. This API can fetch, and it can read the logged-in browser profiles on
//! this machine, so a route that answers without a bearer key is a proxy wearing the operator's
//! identity — available to every process on the box and, through a rebound DNS name, to a page in
//! their own browser. Half the tests here are about a request that must *not* reach the server.
//!
//! The second is what a status code means. A page that came back blocked is a **200**: the call ran
//! and the answer is "there is a wall here". The CLI already promises this in its module doc, and a
//! client that read a wall as a 5xx would sit in a retry loop against something that is never going
//! to move. A 5xx is only ever about this installation.
//!
//! Everything drives the `Router` through `tower::ServiceExt::oneshot`. No socket is ever bound —
//! the same shape `svipall-solver/tests/api.rs` and the dashboard's own tests use.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use support::{Reply, Site};
use svipall_mcp::rest;
use svipall_mcp::server::SvipallServer;
use tower::ServiceExt;

const KEY: &str = "a-key-that-is-long-enough";

/// A server with a page cache of its own, so tests never touch the developer's `~/.svipall`.
fn server() -> SvipallServer {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = support::isolate().join(format!("rest-{n}.db"));
    let store = svipall_core::cache::Store::open_at(&path).expect("open db");
    let cfg = svipall_core::Config {
        // The tests point at a loopback fake site, and climbing to a browser would make them slow,
        // flaky and dependent on a Chrome being installed. What is under test is the HTTP layer.
        max_tier: "http".into(),
        ..Default::default()
    };
    SvipallServer::with_store(None, cfg, None, Some(Arc::new(store)))
}

fn app() -> axum::Router {
    let s = server();
    rest::router(s.clone(), runner(s), KEY, "127.0.0.1")
}

/// A runner that is built but never started: these tests drive routes, not jobs.
fn runner(s: SvipallServer) -> svipall_mcp::jobs::JobRunner {
    svipall_mcp::jobs::JobRunner::new(s, 2)
}

/// A request with everything a legitimate local client sends, and nothing a browser page would.
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

// ---- the gate ---------------------------------------------------------------------------------

#[tokio::test]
async fn a_call_without_a_bearer_token_is_refused_before_anything_runs() {
    let site = Site::start(vec![("/", Reply::page("Index", &[]))]).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/fetch")
        .header("host", "127.0.0.1:8788")
        .header("content-type", "application/json")
        .body(Body::from(json!({"url": site.url("/")}).to_string()))
        .expect("request");
    let (status, body) = send(app(), req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // JSON, not axum's plain-text default: a client parsing every reply must not have to special
    // case the one it gets when its credentials are wrong.
    assert!(body.get("error").is_some(), "expected a JSON error: {body}");
    // And the gate ran before the work did.
    assert_eq!(site.hits("/"), 0, "an unauthenticated call fetched a page");
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/status")
        .header("host", "127.0.0.1:8788")
        .header("authorization", "Bearer a-key-that-is-long-enougX")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let (status, _) = send(app(), req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_empty_api_key_locks_the_door_rather_than_opening_it() {
    // `token_matches("", Some(&""))` is true — same length, empty fold, zero — so a server that
    // forgot to configure a key would accept `Authorization: Bearer ` from anybody. Arithmetically
    // correct and operationally a hole, which is why the gate refuses an empty secret outright.
    let s = server();
    let app = rest::router(s.clone(), runner(s), "", "127.0.0.1");
    let req = Request::builder()
        .method("POST")
        .uri("/v1/status")
        .header("host", "127.0.0.1:8788")
        .header("authorization", "Bearer ")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let (status, _) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_page_in_a_browser_cannot_reach_the_api_by_pointing_a_name_at_loopback() {
    // A page at evil.test served a DNS answer of 127.0.0.1 posts to
    // http://evil.test:8788/v1/route and the *browser* delivers it to loopback. Binding to
    // 127.0.0.1 does not help: the request originates on the box. A POST with a text/plain body is
    // preflight-exempt, so it is sent whether or not CORS would have allowed the reply — which is
    // enough for an endpoint with side effects.
    let with_origin = Request::builder()
        .method("POST")
        .uri("/v1/status")
        .header("host", "127.0.0.1:8788")
        .header("origin", "http://evil.test")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let (status, _) = send(app(), with_origin).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "no browser page is a legitimate client of this api"
    );

    let rebound = Request::builder()
        .method("POST")
        .uri("/v1/status")
        .header("host", "evil.test:8788")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let (status, _) = send(app(), rebound).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the rebound name is the tell, and it is in the Host header"
    );

    // And the ordinary local caller still gets through.
    let (status, _) = send(app(), signed("POST", "/v1/status", json!({}))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_listener_on_the_network_does_not_enforce_a_loopback_host() {
    // The operator's own LAN address is a legitimate Host once they have asked for a LAN bind.
    // Enforcing loopback there would break the thing they configured; the key stays the defence.
    let s = server();
    let app = rest::router(s.clone(), runner(s), KEY, "0.0.0.0");
    let req = Request::builder()
        .method("POST")
        .uri("/v1/status")
        .header("host", "192.168.1.20:8788")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let (status, _) = send(app, req).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_health_endpoint_needs_no_key_and_carries_no_data() {
    let req = Request::builder()
        .uri("/v1/health")
        .body(Body::empty())
        .expect("request");
    let (status, body) = send(app(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "svipall-rest");
    assert!(body["version"].is_string());
    let obj = body.as_object().expect("object");
    assert_eq!(
        obj.len(),
        3,
        "a healthcheck is public, so it must say nothing else: {body}"
    );
}

// ---- what a status code means -----------------------------------------------------------------

#[tokio::test]
async fn a_page_that_answered_with_a_block_is_a_successful_call() {
    // The contract test. A wall is the *answer*, not the absence of one, and this is what catches
    // a future "if blocked, return 502" — which would put every client into a retry loop against
    // something that is never going to move.
    let site = Site::start(vec![("/walled", Reply::cloudflare())]).await;
    let (status, body) = send(
        app(),
        signed("POST", "/v1/fetch", json!({"url": site.url("/walled")})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a wall is not an HTTP failure");
    assert!(
        body.get("blocked_reason").is_some(),
        "the block must be reported in the body: {body}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_this_endpoints_shape_is_refused_before_the_browser_is_touched() {
    let site = Site::start(vec![("/", Reply::page("Index", &[]))]).await;
    // `url` is the one required field on WebFetchParams.
    let (status, body) = send(app(), signed("POST", "/v1/fetch", json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.get("error").is_some(), "{body}");
    assert_eq!(site.hits("/"), 0);
}

#[tokio::test]
async fn a_request_the_installation_cannot_carry_out_is_the_only_five_hundred() {
    // Deliberately an unknown country code rather than "no browser installed": a developer machine
    // has Chrome, and a test whose outcome depends on the environment fails for the wrong person.
    let (status, body) = send(
        app(),
        signed(
            "POST",
            "/v1/route",
            json!({"domain": "example.test", "proxy": "http://p.test:1", "country": "ZZ"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        body["error"].as_str().is_some_and(|e| !e.is_empty()),
        "a 500 must say what this machine could not do: {body}"
    );
}

// ---- the routes themselves ----------------------------------------------------------------------

#[tokio::test]
async fn every_tool_this_server_exposes_is_either_a_route_or_a_named_exclusion() {
    // The conformance test, in the spirit of `svipall-core/tests/widgets.rs`: adding a `#[tool]` to
    // server.rs fails here until somebody decides whether it is a route. Without it, `ROUTES` falls
    // quietly behind the tool list and the API grows a hole nobody notices.
    let names = server().tool_names();
    assert!(names.len() > 20, "the tool list looks wrong: {names:?}");
    for name in &names {
        let path = format!("/v1/{}", name.strip_prefix("web_").unwrap_or(name));
        if rest::ROUTES.contains(&path.as_str()) {
            continue;
        }
        let excluded = rest::NOT_IN_REST
            .iter()
            .find(|(tool, _)| tool == name)
            .unwrap_or_else(|| {
                panic!("the tool `{name}` is neither a route ({path}) nor a named exclusion")
            });
        assert!(
            !excluded.1.is_empty(),
            "`{name}` is excluded without saying why"
        );
    }
}

#[tokio::test]
async fn every_route_the_table_declares_answers_something_other_than_not_found() {
    // The other direction: a path in `ROUTES` that the router never registered would make the
    // conformance test above pass while the endpoint 404s. 400 is a fine answer here — most of
    // these have a required field — but 404 and 405 are not.
    for path in rest::ROUTES {
        let (status, _) = send(app(), signed("POST", path, json!({}))).await;
        assert_ne!(status, StatusCode::NOT_FOUND, "{path} is not routed");
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{path} does not accept POST"
        );
    }
}

#[tokio::test]
async fn the_job_routes_are_not_mistaken_for_tools() {
    // `/v1/jobs/…` is how a caller follows work, not a tool. Listing it in `ROUTES` would make the
    // conformance test above demand a `jobs` tool that does not and should not exist.
    for path in rest::JOB_ROUTES {
        assert!(
            !rest::ROUTES.contains(path),
            "{path} is a job route and also listed as a tool"
        );
    }
}

#[tokio::test]
async fn reading_the_status_over_get_cannot_clear_anything() {
    // `WebStatusParams` carries three mutating fields. A GET must never be able to reach them, so
    // it is wired to `Default` rather than to a body.
    let req = Request::builder()
        .uri("/v1/status")
        .header("host", "127.0.0.1:8788")
        .header("authorization", format!("Bearer {KEY}"))
        .body(Body::empty())
        .expect("request");
    let (status, body) = send(app(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("cleared").is_none(), "a GET cleared something");
}

#[tokio::test]
async fn the_same_fetch_through_http_and_through_the_seam_return_the_same_object() {
    // The test that says the REST layer adds nothing and hides nothing. If the two ever diverge,
    // one of the three ways to drive svipall is telling a different story about the same page.
    let site = Site::start(vec![("/a", Reply::page("Alpha", &[]))]).await;
    let direct = server()
        .fetch_json(svipall_mcp::tools::WebFetchParams {
            url: site.url("/a"),
            ..Default::default()
        })
        .await
        .value;
    let (status, body) = send(
        app(),
        signed("POST", "/v1/fetch", json!({"url": site.url("/a")})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for key in ["url", "title", "content", "status", "tier_used"] {
        assert_eq!(
            body.get(key),
            direct.get(key),
            "`{key}` differs between the seam and the route"
        );
    }
}
