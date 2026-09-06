//! The same server over HTTP, one endpoint per tool.
//!
//! svipall was reachable from an MCP client or a shell and from nothing else, which made "use it
//! from Python" mean "shell out and parse stdout". This is the same `SvipallServer` the MCP
//! transport and the CLI drive — the same browser pool, the same page cache, the same learned
//! tiers — behind `POST /v1/<tool>` with the tool's own JSON as the body.
//!
//! Two rules worth reading twice.
//!
//! **A page that was blocked is a 200.** The API call succeeded; the *page* did not. This is the
//! CLI's contract in its own words (`bin/svipall.rs`: "the exit code … never depends on what a site
//! said: a page that was blocked is a successful report of a block"), and REST inherits it verbatim.
//! A `500` is only ever about *this machine*. See `RestError`.
//!
//! **Every route is behind a bearer key, including on loopback.** A local port is not a boundary:
//! svipall carries logged-in profiles, cookies and the operator's exit address, so an
//! unauthenticated port is a proxy wearing their identity, open to every process on the box — and,
//! through DNS rebinding, to a page in their browser. See `require_key`.

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{header, StatusCode},
    middleware,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post, MethodRouter},
    Json, Router,
};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::jobs::{JobKind, JobRunner};
use crate::server::SvipallServer;
use crate::tools::*;
use svipall_core::config::KeySource;
use svipall_core::token::token_matches;

/// Everything a handler needs: the server, the key to check against, and whether the bind is
/// loopback.
///
/// A wrapper rather than `State<SvipallServer>` directly because the key and the loopback flag ride
/// along — the same reason `svipall_dashboard::DashboardState` exists. `SvipallServer` is `Clone`
/// with every field behind an `Arc`, so cloning this per request is three pointer bumps.
#[derive(Clone)]
struct Rest {
    server: SvipallServer,
    /// Long work nobody is waiting on.
    runner: JobRunner,
    key: Arc<str>,
    /// Loopback bind, so the `Host` check applies. Computed once, from `lan::reachable_off_box`.
    loopback: bool,
}

/// Every path this router serves. Walked by the conformance tests in both directions.
pub const ROUTES: &[&str] = &[
    "/v1/act",
    "/v1/browser_setup",
    "/v1/capture",
    "/v1/crawl",
    "/v1/diff",
    "/v1/fetch",
    "/v1/fetch_many",
    "/v1/log",
    "/v1/map",
    "/v1/notes",
    "/v1/profile",
    "/v1/route",
    "/v1/screenshot",
    "/v1/search",
    "/v1/site_search",
    "/v1/snapshot",
    "/v1/solve_and_continue",
    "/v1/status",
    "/v1/watch",
];

/// Tools that are deliberately not routes, and why.
///
/// A new `#[tool]` in `server.rs` fails `every_tool_this_server_exposes_is_either_a_route_or_a_named_exclusion`
/// until it is listed in one place or the other. That is what makes "a new tool is a new route"
/// true rather than aspirational — the same trick `svipall-core/tests/widgets.rs` plays on the
/// widget table.
pub const NOT_IN_REST: &[(&str, &str)] = &[
    (
        "browser_open",
        "a session is a resource HTTP cannot bound: a client that dies between open and close \
         leaks a real browser with no TTL and no cap. web_act covers the single-shot case",
    ),
    ("browser_do", "belongs to a session; see browser_open"),
    ("browser_close", "belongs to a session; see browser_open"),
    (
        "web_login",
        "opens a visible window on the operator's desktop and waits up to an hour for a person; \
         an HTTP request must not be able to do that",
    ),
    (
        "solve_image_captcha",
        "the solver API on dashboard_port already answers this in the 2captcha wire shape",
    ),
    (
        "solve_recaptcha_v2",
        "the solver API on dashboard_port already answers this in the 2captcha wire shape",
    ),
    (
        "solve_turnstile",
        "the solver API on dashboard_port already answers this in the 2captcha wire shape",
    ),
    (
        "solve_hcaptcha",
        "the solver API on dashboard_port already answers this in the 2captcha wire shape",
    ),
    (
        "captcha_status",
        "the solver API on dashboard_port already answers this in the 2captcha wire shape",
    ),
    (
        "report_captcha",
        "the solver API on dashboard_port already answers this in the 2captcha wire shape",
    ),
];

/// The job routes. Not in `ROUTES`, because they are not a tool: they are how a caller follows one.
pub const JOB_ROUTES: &[&str] = &["/v1/jobs", "/v1/jobs/{id}", "/v1/jobs/{id}/stream"];

// ---- errors ---------------------------------------------------------------------------------

/// What went wrong with the *request*, never with the page.
///
/// A type rather than the dashboard's `(StatusCode, &str)` tuples because the `?` inside `tool()`
/// needs something to convert into: with nineteen routes there are two error sites per route, and
/// without a type every one of them rewrites the same `map_err`. It is twenty lines, has one
/// `IntoResponse`, and never leaves this module.
struct RestError {
    status: StatusCode,
    message: String,
}

impl RestError {
    /// The body is not this endpoint's shape. The caller's mistake, and the only thing besides the
    /// key that is decided before any work happens.
    fn bad_request(e: JsonRejection) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: e.body_text(),
        }
    }

    /// This installation could not carry the request out: no page cache, no browser, an unknown
    /// country code. Things the operator can fix — never a wall, which is a 200.
    fn internal<E: std::fmt::Display>(e: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

impl IntoResponse for RestError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

// ---- the gate -------------------------------------------------------------------------------

/// Bearer key, plus the two checks that make a loopback bind mean something.
///
/// A layer rather than a check inside each handler, for the reason the dashboard already documents:
/// inside a handler it would sit behind `Json`'s own validation, so an unauthenticated caller with a
/// malformed body would be rejected for the wrong reason and the gate would never run.
async fn require_key(
    State(state): State<Rest>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    // An empty expected key would accept `Authorization: Bearer ` from anyone, because an empty
    // string matches an empty string. Both mount points refuse to start without a key; this is the
    // second lock on the same door, because the cost of being wrong here is the whole machine.
    if state.key.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "this server has no api key configured"})),
        )
            .into_response();
    }

    // DNS rebinding. A page the operator has open at evil.test can be handed a DNS answer of
    // 127.0.0.1; its script then posts to http://evil.test:8788/v1/fetch and the *browser* delivers
    // it to loopback. Binding to 127.0.0.1 does not help, because the request originates on the box.
    // The key is the real defence — a page cannot read it — but a POST with a text/plain body is
    // preflight-exempt and is therefore *sent* regardless, so an endpoint with side effects would
    // fire on a request nobody authorised.
    //
    // No browser page is a legitimate client of this API. There is no CorsLayer here and there will
    // not be one: the absence of tower-http is the policy, not an oversight.
    if req.headers().contains_key(header::ORIGIN) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "this api is not reachable from a browser page"})),
        )
            .into_response();
    }
    // The attacker's page must send `Host: evil.test`. That is the tell — but only when the bind is
    // loopback: on a LAN address the operator's own hostname is a legitimate Host, and enforcing
    // loopback there would break the thing they asked for.
    if state.loopback {
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !host_is_loopback(host) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error": format!("unexpected Host {host:?} on a loopback listener")})),
            )
                .into_response();
        }
    }

    let offered = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim().to_string());
    if !token_matches(&state.key, offered.as_ref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "missing or bad bearer token"})),
        )
            .into_response();
    }
    next.run(req).await
}

/// Is this `Host` header one a loopback listener should answer to?
///
/// An empty `Host` counts: HTTP/2 omits it in favour of `:authority`, and a caller that sent none is
/// not a browser following a rebound name.
fn host_is_loopback(host: &str) -> bool {
    if host.is_empty() {
        return true;
    }
    // Strip the port. IPv6 arrives bracketed, so find the last colon outside the brackets.
    let name = match host.rfind(']') {
        Some(i) => &host[..=i],
        None => host.split(':').next().unwrap_or(host),
    };
    matches!(name, "localhost" | "127.0.0.1" | "[::1]" | "::1") || name.starts_with("127.")
}

// ---- the router -----------------------------------------------------------------------------

/// One tool, one route.
///
/// The closure adapts whatever the seam returns to `anyhow::Result<Value>`, so `crawl_json -> Value`,
/// `snapshot_json -> Result<Value>` and `fetch_json -> FetchOutcome` are each one line at the call
/// site and there is one handler body here rather than nineteen.
///
/// Deliberately not a macro. The workspace has two `macro_rules!` outside vendored code and both
/// build data tables rather than control flow; a macro here would hide the one thing a reader opens
/// a router file to see.
fn tool<P, F>(f: fn(SvipallServer, P) -> F) -> MethodRouter<Rest>
where
    P: serde::de::DeserializeOwned + Send + 'static,
    F: std::future::Future<Output = anyhow::Result<Value>> + Send + 'static,
{
    post(
        move |State(rest): State<Rest>, body: Result<Json<P>, JsonRejection>| async move {
            let Json(p) = body.map_err(RestError::bad_request)?;
            let active = rest.server.active().await.map_err(RestError::internal)?;
            f(active.as_ref().clone(), p)
                .await
                .map(Json)
                .map_err(RestError::internal)
        },
    )
}

/// The REST API, ready to be served. Binding happens at the call site, the way `run_dashboard`
/// already does it for the dashboard.
///
/// `bind` is used for one thing: deciding whether the `Host` check applies.
pub fn router(
    server: SvipallServer,
    runner: JobRunner,
    api_key: impl Into<Arc<str>>,
    bind: &str,
) -> Router {
    let state = Rest {
        server,
        runner,
        key: api_key.into(),
        loopback: !svipall_core::lan::reachable_off_box(bind),
    };

    let v1 = Router::new()
        .route(
            "/v1/fetch",
            tool(|s: SvipallServer, p: WebFetchParams| async move { Ok(s.fetch_json(p).await.value) }),
        )
        .route(
            "/v1/fetch_many",
            tool(|s: SvipallServer, p: WebFetchManyParams| async move {
                Ok(s.fetch_many_json(p).await)
            }),
        )
        // The only route that is not just `tool()`: a crawl may be handed back as a job instead of
        // waited on, and that decision is the caller's.
        .route("/v1/crawl", post(crawl))
        .route("/v1/jobs", get(jobs_list))
        .route("/v1/jobs/{id}", get(job_get).delete(job_delete))
        .route("/v1/jobs/{id}/stream", get(job_stream))
        .route(
            "/v1/act",
            tool(|s: SvipallServer, p: WebActParams| async move { Ok(s.act_json(p).await) }),
        )
        .route(
            "/v1/search",
            tool(|s: SvipallServer, p: WebSearchParams| async move { Ok(s.search_json(p).await) }),
        )
        .route(
            "/v1/site_search",
            tool(|s: SvipallServer, p: WebSiteSearchParams| async move {
                s.site_search_json(p).await
            }),
        )
        .route(
            "/v1/snapshot",
            tool(|s: SvipallServer, p: WebSnapshotParams| async move { s.snapshot_json(p).await }),
        )
        .route(
            "/v1/capture",
            tool(|s: SvipallServer, p: WebCaptureParams| async move { s.capture_json(p).await }),
        )
        .route(
            "/v1/screenshot",
            tool(|s: SvipallServer, p: WebScreenshotParams| async move {
                let shot = s.screenshot_json(p).await?;
                let mut v = shot.value;
                // The picture, when the seam said it was worth carrying. A REST client cannot be
                // handed a second content block the way an MCP one can, so it goes in the body.
                if shot.inline {
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert(
                            "image_base64".into(),
                            json!(base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                &shot.png
                            )),
                        );
                    }
                }
                Ok(v)
            }),
        )
        .route(
            "/v1/map",
            tool(|s: SvipallServer, p: WebMapParams| async move { s.map_json(p).await }),
        )
        .route(
            "/v1/diff",
            tool(|s: SvipallServer, p: WebDiffParams| async move { s.diff_json(p).await }),
        )
        .route(
            "/v1/watch",
            tool(|s: SvipallServer, p: WebWatchParams| async move { s.watch_json(p).await }),
        )
        .route(
            "/v1/notes",
            tool(|s: SvipallServer, p: WebNotesParams| async move { s.notes_json(p) }),
        )
        .route(
            "/v1/log",
            tool(|s: SvipallServer, p: WebLogParams| async move { s.log_json(p) }),
        )
        .route(
            "/v1/profile",
            tool(|s: SvipallServer, p: WebProfileParams| async move { s.profile_json(p) }),
        )
        .route(
            "/v1/route",
            tool(|s: SvipallServer, p: WebRouteParams| async move { s.route_json(p).await }),
        )
        .route(
            "/v1/browser_setup",
            tool(|s: SvipallServer, p: BrowserSetupParams| async move {
                s.browser_setup_json(p).await
            }),
        )
        .route(
            "/v1/solve_and_continue",
            tool(|s: SvipallServer, p: SolveAndContinueParams| async move {
                s.solve_and_continue_json(p).await
            }),
        )
        // The only route with a GET as well as a POST, and the only one where that distinction
        // carries weight: `WebStatusParams` has four *mutating* fields (clear_cooldown, clear_budget,
        // forget_tier, clear_cache), so a GET must never be able to reach them.
        .route(
            "/v1/status",
            get(status_get).post(
                |State(rest): State<Rest>, body: Result<Json<WebStatusParams>, JsonRejection>| async move {
                    let Json(p) = body.map_err(RestError::bad_request)?;
                    rest.server
                        .active().await.map_err(RestError::internal)?
                        .status_json(p)
                        .await
                        .map(Json)
                        .map_err(RestError::internal)
                },
            ),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_key));

    Router::new()
        // Public on purpose: it carries no data, and a container healthcheck should not need a key.
        .route("/v1/health", get(health))
        .merge(v1)
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "svipall-rest",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `GET /v1/status` — read-only by construction: default params carry none of the three clears.
async fn status_get(State(rest): State<Rest>) -> Result<Json<Value>, RestError> {
    rest.server
        .active()
        .await
        .map_err(RestError::internal)?
        .status_json(WebStatusParams::default())
        .await
        .map(Json)
        .map_err(RestError::internal)
}

// ---- long jobs --------------------------------------------------------------------------------

/// A crawl request, plus the one thing that is about the *transport* rather than the crawl.
///
/// `WebCrawlParams` is the MCP tool's schema and must not grow an `async` flag for the benefit of
/// one protocol, so the wrapper lives here and flattens the rest.
#[derive(serde::Deserialize)]
struct CrawlRequest {
    /// Return a job id immediately instead of the pages. Default false.
    #[serde(default, rename = "async")]
    is_async: bool,
    #[serde(flatten)]
    params: WebCrawlParams,
}

/// `POST /v1/crawl` — synchronously by default, as a job when asked.
async fn crawl(
    State(rest): State<Rest>,
    body: Result<Json<CrawlRequest>, JsonRejection>,
) -> Result<axum::response::Response, RestError> {
    let Json(req) = body.map_err(RestError::bad_request)?;
    if !req.is_async {
        return Ok(Json(
            rest.server
                .active()
                .await
                .map_err(RestError::internal)?
                .crawl_json(req.params)
                .await,
        )
        .into_response());
    }
    // The runner mints the id. A client that chose its own would be choosing this server's primary
    // key and a URL path segment, and `resume_or_start` does not validate one — an unknown id
    // silently starts a fresh crawl under it. Resuming stays `{"crawl_id": "…"}`, which is the same
    // word the MCP tool and the CLI already use.
    let id = rest
        .runner
        .submit(JobKind::Crawl(Box::new(req.params)))
        .map_err(RestError::internal)?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, format!("/v1/jobs/{id}"))],
        Json(json!({
            "job_id": id,
            "kind": "crawl",
            "state": "queued",
            "stream": format!("/v1/jobs/{id}/stream"),
        })),
    )
        .into_response())
}

fn store_of(rest: &Rest) -> Result<Arc<svipall_core::cache::Store>, RestError> {
    rest.server
        .store()
        .cloned()
        .ok_or_else(|| RestError::internal("the page cache is unavailable, so there are no jobs"))
}

fn no_such_job(id: &str) -> RestError {
    RestError {
        status: StatusCode::NOT_FOUND,
        message: format!("no job {id}"),
    }
}

/// `GET /v1/jobs` — never carries `result`. Ten finished two-hundred-page crawls would otherwise
/// be tens of megabytes of pages nobody asked for twice.
async fn jobs_list(
    State(rest): State<Rest>,
    axum::extract::Query(q): axum::extract::Query<JobQuery>,
) -> Result<Json<Value>, RestError> {
    let rows = store_of(&rest)?.jobs(q.state.as_deref(), q.limit.unwrap_or(50).clamp(1, 500));
    Ok(Json(json!({"count": rows.len(), "jobs": rows})))
}

#[derive(serde::Deserialize)]
struct JobQuery {
    state: Option<String>,
    limit: Option<usize>,
}

/// `GET /v1/jobs/{id}` — the row, plus the summary once it is terminal.
///
/// While it is running, `pages_done` is the honest partial answer. The client is never handed a
/// fabricated result.
async fn job_get(
    State(rest): State<Rest>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, RestError> {
    let store = store_of(&rest)?;
    let row = store.job(&id).ok_or_else(|| no_such_job(&id))?;
    let mut v = serde_json::to_value(&row).unwrap_or(Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "result".into(),
            store
                .job_result(&id)
                .and_then(|r| serde_json::from_str(&r).ok())
                .unwrap_or(Value::Null),
        );
    }
    Ok(Json(v))
}

/// `DELETE /v1/jobs/{id}` — ask it to stop.
///
/// 200 rather than 204, and 200 for a job that had already finished: a caller wants to know whether
/// it stopped a running crawl or reaped a finished one. Deleting a terminal job is not an error.
async fn job_delete(
    State(rest): State<Rest>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, RestError> {
    // Through the runner, not the store: the flag has to reach the crawl that is running now as
    // well as the row that survives this process.
    let was = rest.runner.cancel(&id).ok_or_else(|| no_such_job(&id))?;
    let running = was == "running" || was == "queued";
    Ok(Json(json!({
        "job_id": id,
        "state": was,
        "cancel_requested": running,
        "note": if running {
            "a crawl stops between pages; its frontier is kept, so this id can be resumed"
        } else {
            "the job had already ended; nothing was cancelled"
        },
    })))
}

/// `GET /v1/jobs/{id}/stream` — the same job, as Server-Sent Events.
///
/// The first frame is always a `snapshot` built from the **store**, never from the channel. A late
/// subscriber that inferred "this started at page 0" from the first live event would draw a bar
/// from the beginning of work that is already half done, and that is the one way this can lie. Full
/// replay would need a durable event log written per page and trimmed by the housekeeper — real
/// cost for a consumer that is a progress bar — and the snapshot is the honest substitute.
async fn job_stream(
    State(rest): State<Rest>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::response::Response, RestError> {
    let store = store_of(&rest)?;
    let row = store.job(&id).ok_or_else(|| no_such_job(&id))?;
    let head = futures::stream::iter([Ok(snapshot_event(&row))]);

    // A job that is not running in this process — already finished, or owned by another run — has
    // no channel to join. Say where it is and close, rather than hanging on a stream that will
    // never carry anything.
    let Some(rx) = rest.runner.subscribe(&id) else {
        let tail = futures::stream::iter([Ok(done_event(&store, &id))]);
        return Ok(sse(futures::StreamExt::chain(head, tail)));
    };

    // `futures::stream::unfold` rather than `tokio_stream::wrappers::BroadcastStream`: that wrapper
    // would be a new dependency for six lines, and `futures` is already here.
    let live = futures::stream::unfold(Some((rx, store, id)), |state| async move {
        let (mut rx, store, id) = state?;
        match rx.recv().await {
            Ok(e) => {
                let ev = Event::default()
                    .event("progress")
                    .json_data(&e)
                    .unwrap_or_else(|_| Event::default().event("progress").data("{}"));
                Some((Ok(ev), Some((rx, store, id))))
            }
            // The subscriber fell behind the buffer. Told, rather than dropped: a slow reader is a
            // slow reader, not a reason to close its connection.
            Err(broadcast::error::RecvError::Lagged(n)) => {
                let ev = Event::default()
                    .event("lagged")
                    .data(json!({ "skipped": n }).to_string());
                Some((Ok(ev), Some((rx, store, id))))
            }
            // The sender is gone, so the run is over. Read the state it ended in rather than
            // guessing at one.
            Err(broadcast::error::RecvError::Closed) => {
                let ev = done_event(&store, &id);
                Some((Ok(ev), None))
            }
        }
    });

    Ok(sse(futures::StreamExt::chain(head, live)))
}

/// An event stream as a plain response.
///
/// `keep_alive` changes the concrete type, so the two branches above cannot both be `Sse<_>`
/// without a boxed stream *and* a boxed keep-alive wrapper. One `into_response()` is simpler than
/// either. The interval is axum's default — fifteen seconds, comfortably under every proxy idle
/// timeout, and this is loopback anyway, so there is no number here worth inventing.
fn sse<S>(stream: S) -> axum::response::Response
where
    S: futures::Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Where the job actually is, from the store. Always the first frame.
fn snapshot_event(row: &svipall_core::cache::JobRow) -> Event {
    Event::default()
        .event("snapshot")
        .data(serde_json::to_string(row).unwrap_or_else(|_| "{}".into()))
}

fn done_event(store: &svipall_core::cache::Store, id: &str) -> Event {
    let data = store
        .job(id)
        .and_then(|r| serde_json::to_string(&r).ok())
        .unwrap_or_else(|| json!({"id": id, "state": "gone"}).to_string());
    Event::default().event("done").data(data)
}

// ---- serving --------------------------------------------------------------------------------

/// Resolve the key, bind, log what the operator needs, and serve until ctrl-c.
///
/// Shared by `svipall serve` and by the mount inside `svipall-mcp`, so the key resolution, the
/// warning and the graceful shutdown exist once.
pub async fn serve(server: SvipallServer, bind: &str, port: u16) -> anyhow::Result<()> {
    let (key, source) = svipall_core::config::api_key();
    if key.is_empty() {
        anyhow::bail!("no api key: set SVIPALL_API_KEY, or api_key in config.toml");
    }
    // The runner is built and started here, so both mount points get one and neither can forget.
    // It adopts what a dead run left behind on its first tick, which is the moment an operator
    // finds out a crawl was killed rather than finished.
    let runner = JobRunner::new(server.clone(), server.config().max_jobs);
    runner.start();
    let reaper = server.clone();
    let housekeeping = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            reaper.reap_configuration().await;
        }
    });
    let app = router(server.clone(), runner, key.as_str(), bind);
    let listener = tokio::net::TcpListener::bind(format!("{bind}:{port}"))
        .await
        .map_err(|e| anyhow::anyhow!("cannot listen on {bind}:{port}: {e}"))?;

    // Straight to stderr rather than through `tracing`, and that is deliberate. The `svipall`
    // binary pins its filter at `warn` so a fetch's stdout stays clean, which means an `info!` here
    // is invisible — and the line a generated key appears on is the one thing an operator cannot
    // use this server without. A log level must not be able to hide it. stderr is free in both
    // binaries: the CLI writes its answer to stdout, and svipall-mcp speaks MCP there.
    eprintln!(
        "svipall REST API on http://{bind}:{port}/v1/ ({} routes)",
        ROUTES.len()
    );
    match &source {
        // Printed once, on the run that made it, and never again.
        KeySource::Generated(path) => eprintln!(
            "api key generated: {key}\n  kept in {}; it will not be printed again",
            path.display()
        ),
        KeySource::File(path) => eprintln!("api key read from {}", path.display()),
        KeySource::Env => eprintln!("api key from $SVIPALL_API_KEY"),
        KeySource::Config => eprintln!("api key from config.toml"),
    }
    if svipall_core::lan::reachable_off_box(bind) {
        eprintln!(
            "warning: {bind} is reachable from the network. The api key is now the only thing \
             between it and this machine's logged-in browser profiles."
        );
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    housekeeping.abort();
    server.shutdown_configuration().await;
    Ok(())
}
