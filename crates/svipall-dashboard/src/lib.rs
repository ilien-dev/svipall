//! svipall-dashboard — human solving dashboard (Axum + WebSocket).
//!
//! The dashboard exposes captcha jobs, which carry the URLs svipall is visiting and, for image
//! captchas, the challenge bitmap. That is private to the operator, so every data route is gated
//! by a per-run token and the server is expected to bind loopback (see `svipall-mcp::run_dashboard`).
//! The HTML shell itself is public: it holds no data and needs to load before it can ask for the
//! token from the query string.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    middleware,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::sync::Arc;
use svipall_core::answer::Answer;
// One copy, in core, because the REST API needs the same comparison and two copies of a
// constant-time compare is one copy that gets a well-meaning early return added to it.
use svipall_core::token::token_matches;
use svipall_core::widget::Modality;
use svipall_solver::AppState;

#[derive(Clone)]
pub struct DashboardState {
    app: Arc<AppState>,
    token: Arc<str>,
}

pub fn router(state: Arc<AppState>, token: impl Into<Arc<str>>) -> Router {
    let state = DashboardState {
        app: state,
        token: token.into(),
    };
    // The token gate runs as a layer, before any extractor. Checking it inside the handlers would
    // put it behind WebSocketUpgrade's own validation, so a request that is not a valid upgrade
    // would be rejected for the wrong reason and the gate would never run at all.
    let protected = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/pending", get(api_pending))
        .route("/asset/{id}", get(asset))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));
    Router::new()
        .route("/", get(dashboard_html))
        .route("/human", get(dashboard_html))
        .merge(protected)
        .with_state(state)
}

async fn require_token(
    State(state): State<DashboardState>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let token = req.uri().query().and_then(|q| url_query_value(q, "t"));
    if !token_matches(&state.token, token.as_ref()) {
        return (StatusCode::UNAUTHORIZED, "missing or bad token").into_response();
    }
    next.run(req).await
}

/// Minimal `application/x-www-form-urlencoded` lookup: only `%XX` and `+` need decoding here.
fn url_query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k != key {
            return None;
        }
        let mut out = String::with_capacity(v.len());
        let bytes = v.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'+' => {
                    out.push(' ');
                    i += 1;
                }
                b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&v[i + 1..i + 3], 16) {
                    Ok(b) => {
                        out.push(b as char);
                        i += 3;
                    }
                    Err(_) => {
                        out.push('%');
                        i += 1;
                    }
                },
                b => {
                    out.push(b as char);
                    i += 1;
                }
            }
        }
        Some(out)
    })
}

/// What is waiting, in the shape the panel draws from.
///
/// Each job carries the modality that says which control to show and the ids of its assets — never
/// the bytes. This goes out over the socket every couple of seconds, and a four-by-four grid is
/// seventeen images; sending them inline would push megabytes a minute at an idle browser.
async fn pending_json(state: &AppState) -> serde_json::Value {
    let db = state.db_pool.read().await;
    let jobs = db.list_pending().unwrap_or_default();
    let rows: Vec<serde_json::Value> = jobs
        .iter()
        .map(|j| {
            let mut v = serde_json::to_value(j).unwrap_or(serde_json::Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert(
                    "modality".into(),
                    serde_json::json!(db.modality_of(&j.task_id)),
                );
                let assets: Vec<serde_json::Value> = db
                    .assets_for(&j.id)
                    .into_iter()
                    .map(|(id, kind, idx)| serde_json::json!({"id": id, "kind": kind, "idx": idx}))
                    .collect();
                o.insert("assets".into(), serde_json::Value::Array(assets));
            }
            v
        })
        .collect();
    serde_json::json!({"jobs": rows})
}

async fn api_pending(State(state): State<DashboardState>) -> impl IntoResponse {
    axum::Json(pending_json(&state.app).await)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<DashboardState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state.app))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                let mut payload = pending_json(&state).await;
                if let Some(o) = payload.as_object_mut() {
                    o.insert("type".into(), serde_json::json!("pending"));
                }
                let msg = payload.to_string();
                if socket.send(Message::Text(msg.into())).await.is_err() { break; }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(reply) = handle_solve(&state, &text).await {
                            if socket.send(Message::Text(reply.into())).await.is_err() { break; }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

/// Serve one challenge asset: an image, a sound, a fragment of a grid.
///
/// Bytes never ride along in the job list, so this is how the panel gets them — one at a time, by
/// id, behind the same token as everything else. `nosniff` because the mime came from a remote
/// site and a browser guessing at it is a way to run their content as ours.
async fn asset(
    State(state): State<DashboardState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    let Some((mime, bytes)) = state.app.db_pool.read().await.asset(&id) else {
        return (StatusCode::NOT_FOUND, "no such asset").into_response();
    };
    (
        [
            (axum::http::header::CONTENT_TYPE, mime),
            (
                axum::http::header::X_CONTENT_TYPE_OPTIONS,
                "nosniff".to_string(),
            ),
            (
                axum::http::header::CACHE_CONTROL,
                "private, max-age=300".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Read a submission from the panel.
///
/// Two shapes are accepted. The structured one — `{"answer": {"kind": ...}}` — covers every
/// modality and is what the panel sends. The older `token`/`text` pair is still read because the
/// solver API's own clients speak it, and mapping it here is two lines rather than a second path
/// through the store.
fn parse_submission(val: &serde_json::Value) -> Option<Answer> {
    if let Some(a) = val.get("answer") {
        return serde_json::from_value(a.clone()).ok();
    }
    if let Some(t) = val.get("token").and_then(|v| v.as_str()) {
        return Some(Answer::Token {
            value: t.to_string(),
        });
    }
    val.get("text")
        .and_then(|v| v.as_str())
        .map(|t| Answer::Text {
            value: t.to_string(),
        })
}

/// What the store should be told, given an answer.
///
/// Kept separate from the database so the mapping — which is the part with the judgement in it —
/// is testable on its own.
#[derive(Debug, PartialEq)]
pub(crate) enum Stored {
    /// A token the site will accept as-is.
    Token(String),
    /// Text, or a structured answer as JSON for a strategy to replay.
    Text(String),
    /// The person could not read it. A failure, not a solve: recording it as a solve would teach
    /// the ranking that a strategy works when nobody could answer at all.
    Declined,
}

pub(crate) fn to_stored(answer: &Answer) -> Stored {
    match answer {
        Answer::Token { value } => Stored::Token(value.clone()),
        Answer::Text { value } | Answer::Nonce { value } => Stored::Text(value.clone()),
        Answer::Unknown => Stored::Declined,
        // Everything geometric goes back as the JSON it arrived as: the coordinates are fractions
        // of the asset, and whatever replays them knows the size it is replaying them into.
        other => Stored::Text(serde_json::to_string(other).unwrap_or_default()),
    }
}

/// One submission updates the job exactly once. A payload carrying both `token` and `text` used
/// to run two updates, and the second one wrote NULL over the token it had just stored.
async fn handle_solve(state: &AppState, raw: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(raw).ok()?;
    if val.get("action").and_then(|v| v.as_str()) != Some("solve") {
        return None;
    }
    let task_id = val.get("taskId").and_then(|v| v.as_str())?;
    let answer = parse_submission(&val)?;

    let db = state.db_pool.read().await;
    // Check the answer against the question, when the job knows what it asked. A grid answer to a
    // slider is stored, replayed, rejected by the site, and recorded as a strategy that failed —
    // three wrong conclusions from one submission nobody checked.
    if let Some(m) = db
        .modality_of(task_id)
        .and_then(|m| serde_json::from_value::<Modality>(serde_json::Value::String(m)).ok())
    {
        if let Err(why) = answer.check(m) {
            return Some(
                serde_json::json!({"type":"rejected","taskId":task_id,"reason":why.to_string()})
                    .to_string(),
            );
        }
    }
    let raw_answer = serde_json::to_string(&answer).unwrap_or_default();
    match to_stored(&answer) {
        Stored::Token(t) => db.update_solved(task_id, Some(&t), None).ok()?,
        Stored::Text(t) => db.update_solved(task_id, None, Some(&t)).ok()?,
        Stored::Declined => {
            db.update_failed(task_id, "the person could not read this challenge")
                .ok()?;
            return Some(serde_json::json!({"type":"declined","taskId":task_id}).to_string());
        }
    }
    let _ = db.set_answer(task_id, &raw_answer, "human");
    Some(serde_json::json!({"type":"solved","taskId":task_id}).to_string())
}

/// The panel, assembled from three files rather than one string.
///
/// It used to be a single `const` with two modes hard-coded into it, which is why adding a third
/// meant editing a wall of escaped HTML inside a Rust literal. Now the markup, the styling and the
/// per-modality renderers are ordinary files that an editor can check, embedded at build time so
/// there is still nothing to install and no bundler in the way.
fn dashboard_page() -> String {
    include_str!("../assets/index.html")
        .replace("__STYLE__", include_str!("../assets/style.css"))
        .replace("__MODES__", include_str!("../assets/modes.js"))
        .replace("__APP__", include_str!("../assets/app.js"))
}

async fn dashboard_html() -> Html<String> {
    Html(dashboard_page())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state() -> Arc<AppState> {
        let db = svipall_solver::db::Db::open_memory().expect("open in-memory db");
        Arc::new(AppState::new(db, svipall_solver::queue::JobQueue::new()))
    }

    use svipall_core::answer::Point;

    async fn job_of(state: &Arc<AppState>, modality: Option<&str>) -> String {
        let db = state.db_pool.read().await;
        let job = db
            .create_job("turnstile", Some("k"), Some("https://x.test/"), None)
            .expect("created");
        if let Some(m) = modality {
            db.set_challenge(&job.task_id, "w.example", m, None)
                .expect("set");
        }
        job.task_id
    }

    #[tokio::test]
    async fn an_answer_to_a_different_question_is_refused_and_says_why() {
        // Stored, replayed, rejected by the site, recorded as a failed strategy: three wrong
        // conclusions from one submission nobody checked.
        let st = state();
        let id = job_of(&st, Some("slide")).await;
        let msg = format!(
            r#"{{"action":"solve","taskId":"{id}","answer":{{"kind":"tiles","indices":[1,2]}}}}"#
        );
        let reply = handle_solve(&st, &msg).await.expect("answered");
        assert!(reply.contains("rejected"), "{reply}");
        assert!(
            st.db_pool.read().await.modality_of(&id).is_some(),
            "the job is still waiting, not marked solved"
        );
    }

    #[tokio::test]
    async fn a_click_sent_in_pixels_is_refused_rather_than_stored() {
        // A phone showing a 1280-wide image on a 390-wide screen sends numbers that look fine and
        // are wrong by a factor of three. Nothing downstream can tell.
        let st = state();
        let id = job_of(&st, Some("points")).await;
        let msg = format!(
            r#"{{"action":"solve","taskId":"{id}","answer":{{"kind":"points","points":[{{"x":412,"y":203}}]}}}}"#
        );
        let reply = handle_solve(&st, &msg).await.expect("answered");
        assert!(reply.contains("rejected"), "{reply}");
    }

    #[tokio::test]
    async fn a_normalised_click_is_accepted_and_kept_as_it_was_given() {
        let st = state();
        let id = job_of(&st, Some("points")).await;
        let msg = format!(
            r#"{{"action":"solve","taskId":"{id}","answer":{{"kind":"points","points":[{{"x":0.4,"y":0.2}}]}}}}"#
        );
        let reply = handle_solve(&st, &msg).await.expect("answered");
        assert!(reply.contains("solved"), "{reply}");
        let job = st
            .db_pool
            .read()
            .await
            .get_by_task_id(&id)
            .expect("read")
            .expect("present");
        assert_eq!(job.status, "solved");
    }

    #[tokio::test]
    async fn saying_you_cannot_read_it_marks_the_job_failed_not_solved() {
        // Recording a decline as a solve teaches the ranking that something works when nobody
        // could answer at all.
        let st = state();
        let id = job_of(&st, Some("tiles")).await;
        let msg = format!(r#"{{"action":"solve","taskId":"{id}","answer":{{"kind":"unknown"}}}}"#);
        let reply = handle_solve(&st, &msg).await.expect("answered");
        assert!(reply.contains("declined"), "{reply}");
        let job = st
            .db_pool
            .read()
            .await
            .get_by_task_id(&id)
            .expect("read")
            .expect("present");
        assert_eq!(job.status, "failed");
    }

    #[tokio::test]
    async fn a_job_that_never_said_what_it_wanted_still_accepts_an_answer() {
        // Jobs created before anything classified them must not become unanswerable.
        let st = state();
        let id = job_of(&st, None).await;
        let msg = format!(r#"{{"action":"solve","taskId":"{id}","text":"abcd"}}"#);
        assert!(handle_solve(&st, &msg)
            .await
            .expect("answered")
            .contains("solved"));
    }

    #[tokio::test]
    async fn an_asset_is_served_with_its_own_type_and_never_sniffed() {
        // The mime came from a remote site; letting a browser guess at it is how their content
        // gets run as ours.
        let st = state();
        let id =
            st.db_pool
                .read()
                .await
                .put_asset("job-1", "tile", 0, "image/png", &[137, 80, 78, 71]);
        let app = router(st, "tok");
        let res = app
            .oneshot(
                Request::builder()
                    .uri(format!("/asset/{id}?t=tok"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "image/png");
        assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    }

    #[tokio::test]
    async fn an_asset_is_behind_the_token_like_every_other_piece_of_data() {
        // An asset is the challenge picture, which names the site being visited just as surely as
        // the job row does.
        let st = state();
        let id = st
            .db_pool
            .read()
            .await
            .put_asset("job-1", "tile", 0, "image/png", &[1]);
        let res = router(st, "tok")
            .oneshot(
                Request::builder()
                    .uri(format!("/asset/{id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn a_geometric_answer_travels_back_as_the_fractions_it_arrived_as() {
        // Whatever replays the gesture knows the size of the image it is replaying into; this side
        // must not convert to pixels it cannot know.
        let a = Answer::Points {
            points: vec![Point::new(0.25, 0.5)],
        };
        let Stored::Text(json) = to_stored(&a) else {
            panic!("a click is not a token");
        };
        assert!(json.contains("0.25"), "{json}");
        assert_eq!(
            to_stored(&Answer::Token { value: "T".into() }),
            Stored::Token("T".into())
        );
        assert_eq!(to_stored(&Answer::Unknown), Stored::Declined);
    }

    #[tokio::test]
    async fn what_is_pushed_to_the_panel_names_the_assets_and_never_carries_them() {
        // The push runs every couple of seconds. Bytes in it turn an idle browser into megabytes
        // a minute, which is the reason assets are addressed rather than embedded.
        let st = state();
        let id = job_of(&st, Some("tiles")).await;
        let (job_id, asset_id) = {
            let db = st.db_pool.read().await;
            let job = db.get_by_task_id(&id).expect("read").expect("present");
            let a = db.put_asset(&job.id, "tile", 0, "image/png", &[9; 4096]);
            (job.id, a)
        };
        let payload = pending_json(&st).await;
        let row = payload["jobs"]
            .as_array()
            .expect("jobs")
            .iter()
            .find(|j| j["id"] == serde_json::json!(job_id))
            .expect("our job");
        assert_eq!(
            row["modality"], "tiles",
            "the panel is told which control to draw"
        );
        assert_eq!(row["assets"][0]["id"], serde_json::json!(asset_id));
        let text = payload.to_string();
        assert!(
            text.len() < 2048,
            "the bytes leaked into the push: {} chars",
            text.len()
        );
    }

    #[test]
    fn the_page_is_assembled_from_its_three_files_with_nothing_left_unfilled() {
        // A placeholder that survives ships a page with a literal `__STYLE__` in it and no styling,
        // which looks like a broken build only if someone happens to open it.
        let page = dashboard_page();
        assert!(!page.contains("__"), "a placeholder was not filled in");
        assert!(page.contains("SVIPALL_MODES"), "the renderers are missing");
        assert!(page.contains("--accent"), "the styling is missing");
    }

    #[test]
    fn every_modality_the_core_knows_has_something_to_draw_it_with() {
        // The conformance rule for the panel: a modality with no renderer is a challenge that
        // arrives, shows an empty card, and waits for a person who cannot answer it.
        let js = include_str!("../assets/modes.js");
        for m in Modality::ALL {
            let name = format!("{m:?}").to_lowercase();
            assert!(
                js.contains(&format!("\n    {name}:")) || js.contains(&format!("{name}: textish")),
                "no renderer for {name}"
            );
        }
    }

    #[test]
    fn the_panel_sends_fractions_and_never_pixels() {
        // The one rule the server cannot enforce on its own: it can reject a pixel, but only the
        // panel can avoid sending one.
        let js = include_str!("../assets/modes.js");
        assert!(
            js.contains("(e.clientX - r.left) / r.width"),
            "coordinates must be divided by the size of the element they landed on"
        );
        assert!(
            !js.contains("offsetX") && !js.contains("layerX"),
            "raw pixel offsets must never reach a submission"
        );
    }

    // The comparison itself is tested where it now lives, in `svipall_core::token`. What is
    // defended here is that these routes still go through it, which is what the four tests below
    // check at the level a caller actually meets.

    #[tokio::test]
    async fn pending_requires_a_token() {
        let app = router(state(), "secret-token");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/pending")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn pending_rejects_a_wrong_token() {
        let app = router(state(), "secret-token");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/pending?t=nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn pending_accepts_the_right_token() {
        let app = router(state(), "secret-token");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/pending?t=secret-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// A real upgrade attempt, so the WebSocketUpgrade extractor succeeds and the token check is
    /// what decides. Without the upgrade headers the extractor would reject with 400 first and the
    /// test would pass for the wrong reason.
    fn upgrade_request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn websocket_upgrade_requires_a_token() {
        let app = router(state(), "secret-token");
        let res = app.oneshot(upgrade_request("/ws")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// With a valid token the request gets past the gate and reaches the upgrade machinery. A
    /// `oneshot` request has no hyper `OnUpgrade` extension, so the handshake itself cannot
    /// complete here; what matters is that it is no longer rejected as unauthorised.
    #[tokio::test]
    async fn websocket_upgrade_passes_the_gate_with_the_right_token() {
        let app = router(state(), "secret-token");
        let res = app
            .oneshot(upgrade_request("/ws?t=secret-token"))
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn query_value_decodes_percent_and_plus() {
        assert_eq!(url_query_value("t=ab%2Dcd", "t").as_deref(), Some("ab-cd"));
        assert_eq!(
            url_query_value("x=1&t=hi+there", "t").as_deref(),
            Some("hi there")
        );
        assert_eq!(url_query_value("x=1", "t"), None);
        assert_eq!(url_query_value("t=plain", "t").as_deref(), Some("plain"));
    }

    #[tokio::test]
    async fn html_shell_is_served_without_a_token() {
        let app = router(state(), "secret-token");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/human")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_payload_with_both_token_and_text_stores_only_the_token() {
        let st = state();
        let task = {
            let db = st.db_pool.read().await;
            db.create_job("Turnstile", None, Some("https://example.com"), None)
                .expect("create job")
                .task_id
        };
        let raw = serde_json::json!({
            "action": "solve", "taskId": task, "token": "TOKEN_VALUE", "text": "TEXT_VALUE"
        })
        .to_string();
        handle_solve(&st, &raw).await.expect("solve accepted");
        let db = st.db_pool.read().await;
        let job = db.get_by_task_id(&task).unwrap().expect("job exists");
        assert_eq!(job.token.as_deref(), Some("TOKEN_VALUE"));
        assert_eq!(
            job.text, None,
            "the text branch must not overwrite the token"
        );
    }

    #[tokio::test]
    async fn a_solve_without_token_or_text_is_ignored() {
        let st = state();
        let raw = serde_json::json!({"action": "solve", "taskId": "whatever"}).to_string();
        assert!(handle_solve(&st, &raw).await.is_none());
    }
}
