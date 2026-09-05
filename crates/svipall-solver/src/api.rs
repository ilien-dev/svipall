//! Captcha HTTP API (Axum).
//! Supports the legacy `in.php`/`res.php` flow and the modern `createTask`/`getTaskResult` flow.

use crate::{
    queue::{JobType, SolverJob},
    AppState,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct InPhpQuery {
    pub method: Option<String>,
    pub key: Option<String>,
    pub googlekey: Option<String>,
    pub sitekey: Option<String>,
    pub pageurl: Option<String>,
    pub invisible: Option<String>,
    pub json: Option<String>,
    pub body: Option<String>, // base64 image
    pub action: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    #[serde(rename = "clientKey")]
    pub client_key: Option<String>,
    pub task: Option<TaskObject>,
    pub key: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TaskObject {
    #[serde(rename = "type")]
    pub task_type: String,
    #[serde(rename = "websiteURL")]
    pub website_url: Option<String>,
    #[serde(rename = "websiteKey")]
    pub website_key: Option<String>,
    pub sitekey: Option<String>,
    pub url: Option<String>,
    pub body: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InPhpResponse {
    pub status: i32,
    pub request: String,
}

#[derive(Debug, Serialize)]
pub struct CreateTaskResponse {
    #[serde(rename = "errorId")]
    pub error_id: i32,
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
    #[serde(rename = "taskId")]
    pub task_id: Option<String>,
    pub status: Option<String>,
    pub request: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetTaskResultRequest {
    #[serde(rename = "clientKey")]
    pub client_key: Option<String>,
    #[serde(rename = "taskId")]
    pub task_id: Option<String>,
    pub key: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetTaskResultResponse {
    #[serde(rename = "errorId")]
    pub error_id: i32,
    pub status: String, // ready, processing
    pub solution: Option<Solution>,
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Solution {
    pub text: Option<String>,
    #[serde(rename = "gRecaptchaResponse")]
    pub g_recaptcha_response: Option<String>,
    pub token: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/in.php", post(handle_in_php).get(handle_in_php))
        .route("/res.php", get(handle_res_php).post(handle_res_php))
        .route("/createTask", post(handle_create_task))
        .route("/getTaskResult", post(handle_get_task_result))
        .route(
            "/getBalance",
            get(handle_get_balance).post(handle_get_balance),
        )
        .route("/health", get(handle_health))
        .route("/stats", get(handle_stats))
        .with_state(state)
}

async fn handle_in_php(
    State(state): State<Arc<AppState>>,
    Query(params): Query<InPhpQuery>,
) -> impl IntoResponse {
    // Query params drive the legacy GET flow; the JSON body flow is handled by createTask.
    let method = params
        .method
        .clone()
        .unwrap_or_else(|| "userrecaptcha".to_string());
    let googlekey = params.googlekey.clone().or(params.sitekey.clone());
    let pageurl = params.pageurl.clone();
    let image_body = params.body.clone();

    // Action=get -> res.php compat via in.php?action=get
    if params.action.as_deref() == Some("get") {
        if let Some(id) = params.id {
            return handle_res_lookup(&state, &id).await;
        }
    }

    let job_type = if method == "base64" || method == "normal" || image_body.is_some() {
        "ImageToText"
    } else {
        &method
    };
    let sitekey = googlekey.as_deref();
    let page_url = pageurl.as_deref();
    let image_data = image_body.as_deref();

    // Create job
    let db = state.db_pool.read().await;
    let record = match db.create_job(job_type, sitekey, page_url, image_data) {
        Ok(r) => r,
        Err(e) => {
            return Json(serde_json::json!({"status":0, "request": format!("ERROR: {}", e)}))
                .into_response();
        }
    };
    drop(db);

    // Push to queue
    state.queue.push(SolverJob {
        task_id: record.task_id.clone(),
        job_type: JobType::parse(job_type),
        sitekey: sitekey.map(|s| s.to_string()),
        page_url: page_url.map(|s| s.to_string()),
        image_data: image_data.map(|s| s.to_string()),
        created_at: chrono::Utc::now(),
    });

    let is_json = params.json.as_deref() == Some("1") || params.json.as_deref() == Some("true");
    if is_json {
        Json(serde_json::json!({"status":1, "request": record.task_id})).into_response()
    } else {
        // Legacy plain text: OK|task_id
        (StatusCode::OK, format!("OK|{}", record.task_id)).into_response()
    }
}

async fn handle_res_php(
    State(state): State<Arc<AppState>>,
    Query(params): Query<InPhpQuery>,
) -> impl IntoResponse {
    let action = params.action.clone().unwrap_or_else(|| "get".to_string());
    let id = params.id.clone().unwrap_or_default();

    if action == "getbalance" || action == "getBalance" {
        let db = state.db_pool.read().await;
        let bal = db.get_balance();
        drop(db);
        let is_json = params.json.as_deref() == Some("1");
        if is_json {
            return Json(serde_json::json!({"status":1, "request": format!("{}", bal)}))
                .into_response();
        } else {
            return (StatusCode::OK, format!("OK|{}", bal)).into_response();
        }
    }

    if action == "reportgood" || action == "reportbad" {
        // It used to answer OK_REPORT_RECORDED and record nothing, which made the name a lie and
        // threw away the only feedback there is about which challenge types actually work.
        if !id.is_empty() {
            let db = state.db_pool.read().await;
            if let Err(e) = db.record_report(&id, action == "reportgood", None) {
                tracing::debug!("report not recorded: {e}");
            }
        }
        return Json(serde_json::json!({"status":1, "request":"OK_REPORT_RECORDED"}))
            .into_response();
    }

    handle_res_lookup(&state, &id).await
}

async fn handle_res_lookup(state: &AppState, task_id: &str) -> axum::response::Response {
    let db = state.db_pool.read().await;
    let record = match db.get_by_task_id(task_id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Json(serde_json::json!({"status":0, "request":"ERROR_CAPTCHA_UNSOLVABLE"}))
                .into_response()
        }
        Err(e) => {
            return Json(serde_json::json!({"status":0, "request": format!("ERROR: {}", e)}))
                .into_response()
        }
    };
    drop(db);

    match record.status.as_str() {
        "solved" => {
            let result = record.token.clone().or(record.text.clone()).unwrap_or_default();
            // Check if caller expects json
            Json(serde_json::json!({"status":1, "request": result})).into_response()
        }
        "failed" => Json(serde_json::json!({"status":0, "request": record.error.unwrap_or_else(|| "ERROR_CAPTCHA_UNSOLVABLE".to_string())})).into_response(),
        _ => Json(serde_json::json!({"status":0, "request":"CAPCHA_NOT_READY"})).into_response(),
    }
}

async fn handle_create_task(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let task = payload.task.clone();
    let (job_type, sitekey, page_url, image_data) = if let Some(t) = task {
        let jt = t.task_type.clone();
        let sk = t.website_key.clone().or(t.sitekey.clone());
        let url = t.website_url.clone().or(t.url.clone());
        let body = t.body.clone().or(t.text.clone());
        (jt, sk, url, body)
    } else {
        ("Unknown".to_string(), None, None, None)
    };

    let db = state.db_pool.read().await;
    let record = match db.create_job(
        &job_type,
        sitekey.as_deref(),
        page_url.as_deref(),
        image_data.as_deref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            return Json(CreateTaskResponse {
                error_id: 1,
                error_code: Some(format!("ERROR: {}", e)),
                task_id: None,
                status: None,
                request: None,
            })
        }
    };
    drop(db);

    state.queue.push(SolverJob {
        task_id: record.task_id.clone(),
        job_type: JobType::parse(&job_type),
        sitekey,
        page_url,
        image_data,
        created_at: chrono::Utc::now(),
    });

    Json(CreateTaskResponse {
        error_id: 0,
        error_code: None,
        task_id: Some(record.task_id.clone()),
        status: Some("processing".to_string()),
        request: Some(record.task_id),
    })
}

async fn handle_get_task_result(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GetTaskResultRequest>,
) -> impl IntoResponse {
    let task_id = payload
        .task_id
        .clone()
        .or(payload.id.clone())
        .unwrap_or_default();
    let db = state.db_pool.read().await;
    let record = match db.get_by_task_id(&task_id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Json(GetTaskResultResponse {
                error_id: 1,
                status: "failed".to_string(),
                solution: None,
                error_code: Some("ERROR_CAPTCHA_UNSOLVABLE".to_string()),
            })
        }
        Err(e) => {
            return Json(GetTaskResultResponse {
                error_id: 1,
                status: "failed".to_string(),
                solution: None,
                error_code: Some(format!("ERROR: {}", e)),
            })
        }
    };
    drop(db);

    match record.status.as_str() {
        "solved" => {
            let sol = Solution {
                text: record.text.clone(),
                g_recaptcha_response: record.token.clone(),
                token: record.token.clone(),
            };
            Json(GetTaskResultResponse {
                error_id: 0,
                status: "ready".to_string(),
                solution: Some(sol),
                error_code: None,
            })
        }
        "failed" => Json(GetTaskResultResponse {
            error_id: 1,
            status: "failed".to_string(),
            solution: None,
            error_code: Some(
                record
                    .error
                    .unwrap_or_else(|| "ERROR_CAPTCHA_UNSOLVABLE".to_string()),
            ),
        }),
        _ => Json(GetTaskResultResponse {
            error_id: 0,
            status: "processing".to_string(),
            solution: None,
            error_code: None,
        }),
    }
}

async fn handle_get_balance(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db = state.db_pool.read().await;
    let bal = db.get_balance();
    Json(serde_json::json!({"status":1, "request": format!("{:.2}", bal), "balance": bal}))
}

async fn handle_health() -> impl IntoResponse {
    Json(
        serde_json::json!({"status":"ok", "service":"svipall-solver", "version": env!("CARGO_PKG_VERSION")}),
    )
}

async fn handle_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db = state.db_pool.read().await;
    let (pending, solving, solved) = db.stats();
    let qlen = state.queue.len();
    Json(
        serde_json::json!({"pending": pending, "solving": solving, "solved": solved, "queue": qlen}),
    )
}
