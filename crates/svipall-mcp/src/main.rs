//! svipall-mcp binary — MCP stdio server.

use rmcp::{transport::stdio, ServiceExt};
use std::sync::Arc;
use svipall_mcp::server::SvipallServer;
use svipall_mcp::solver_engine::{SolveEngine, Solved};
use svipall_solver::{db, queue};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = svipall_core::config::load();
    // Logging to stderr so stdio is clean for MCP
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(
                cfg.log_level
                    .parse()
                    .unwrap_or_else(|_| "info".parse().unwrap()),
            ),
        )
        .with_writer(std::io::stderr)
        .init();
    svipall_core::ensure_dirs();
    svipall_core::evict_old_profiles();

    // Tokenised dashboard URL, surfaced through web_status so the operator can find it.
    let mut dashboard_url: Option<String> = None;
    // Try to open solver DB (optional — if fails, MCP still works without captcha)
    let solver_state = match db::Db::open() {
        Ok(db) => {
            // Interrupted jobs (process killed mid-solve) and captchas nobody ever solved would
            // otherwise linger on the dashboard forever; drop anything older than 30 minutes.
            let expired = db.expire_stale(30);
            if expired > 0 {
                tracing::info!("expired {} stale captcha jobs on startup", expired);
            }
            let queue = queue::JobQueue::new();
            let state = Arc::new(svipall_solver::AppState::new(db, queue));
            let dash_state = state.clone();
            let dash_port: u16 = std::env::var("SVIPALL_DASHBOARD_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(cfg.dashboard_port);
            // A fresh token every run. The dashboard carries the URLs being visited and accepts
            // captcha answers, so its data routes are not open to whoever can reach the port.
            let token = uuid::Uuid::new_v4().simple().to_string();
            dashboard_url = Some(format!("http://localhost:{}/human?t={}", dash_port, token));
            let bind = cfg.dashboard_bind.clone();
            tokio::spawn(async move {
                if let Err(e) = run_dashboard(dash_state, &bind, dash_port, token).await {
                    tracing::warn!("dashboard failed: {}", e);
                }
            });
            Some(state)
        }
        Err(e) => {
            tracing::warn!(
                "solver DB not available, captcha tools will be disabled: {}",
                e
            );
            None
        }
    };

    let server = SvipallServer::new(solver_state.clone(), cfg.clone(), dashboard_url);
    let pool = server.pool();

    // The same server over HTTP, when the operator asked for it. Off unless `rest_port` says
    // otherwise: the API grants everything the MCP tools do — including this machine's logged-in
    // profiles — so it is opened on purpose rather than by installing. It shares this
    // `SvipallServer`, so a REST fetch and an MCP fetch use one browser pool, one cache and one set
    // of learned tiers, which is the whole point of mounting it here rather than running a second
    // process.
    let rest_port: u16 = std::env::var("SVIPALL_REST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(cfg.rest_port);
    if rest_port != 0 {
        let (rest, bind) = (server.clone(), cfg.rest_bind.clone());
        tokio::spawn(async move {
            if let Err(e) = svipall_mcp::rest::serve(rest, &bind, rest_port).await {
                tracing::warn!("rest api failed: {}", e);
            }
        });
    }

    // Solver workers share the server's browser pool so token captchas are solved by loading the
    // real page in a stealth browser (with human assist), and images by local OCR.
    if let Some(state) = solver_state.clone() {
        let engine = Arc::new(SolveEngine::with_state(pool.clone(), &cfg, state.clone()));
        for i in 0..cfg.solver_workers.max(1) {
            let (w, e) = (state.clone(), engine.clone());
            tokio::spawn(async move {
                worker_loop(i, w, e).await;
            });
        }
    }

    let reaper = pool.clone();
    let housekeeping = solver_state.clone();
    let corpus_keep_days = cfg.corpus_keep_days;
    tokio::spawn(async move {
        let mut ticks: u64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            reaper.reap_idle().await;
            ticks += 1;
            // Every half hour: drop solved jobs and the browser caches inside idle profiles.
            // Without this the solver database and the profile directories only ever grew.
            if ticks.is_multiple_of(30) {
                if let Some(state) = &housekeeping {
                    let (rows, images, assets, expired) = {
                        let db = state.db_pool.read().await;
                        // A job nobody answered used to expire only at the next start, so a
                        // server left running for a week showed a week of dead cards.
                        let expired = db.expire_stale(30);
                        let (r, i, a) = db.housekeep(24, i64::from(corpus_keep_days));
                        (r, i, a, expired)
                    };
                    if expired > 0 {
                        tracing::info!(
                            "housekeeping: expired {expired} captcha jobs nobody answered"
                        );
                    }
                    if rows > 0 || images > 0 || assets > 0 {
                        tracing::info!(
                            "housekeeping: removed {rows} finished jobs, dropped {images} stored captcha images and {assets} corpus assets"
                        );
                    }
                }
                // The reputation ledger holds spend in memory between writes, so the loop that
                // tidies everything else is also where it reaches disk on a long-running server.
                svipall_core::reputation::flush();
                svipall_core::evict_old_profiles();
                svipall_core::prune_profile_cache(None);
                // The page cache is the fastest-growing thing in ~/.svipall, so it gets the same
                // treatment: expire by age, then trim by size, then reclaim the pages.
                if let Ok(store) = svipall_core::cache::Store::open() {
                    // A log nobody trims is a log that eventually costs more than it explains.
                    // Two weeks is long enough to answer "why was last Tuesday slow".
                    let dropped = store.trim_log(14 * 86_400);
                    if dropped > 0 {
                        tracing::debug!("request log: dropped {dropped} old lines");
                    }
                    let report = store.housekeep(&svipall_core::cache::RetentionPolicy::default());
                    if report.pages_expired > 0 || report.pages_evicted > 0 {
                        tracing::info!(
                            "page cache: expired {}, evicted {}, now {} bytes",
                            report.pages_expired,
                            report.pages_evicted,
                            report.bytes_after
                        );
                    }
                }
            }
        }
    });

    let service = server.clone().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;
    service.waiting().await?;
    server.pool().shutdown().await;
    Ok(())
}

async fn worker_loop(id: usize, state: Arc<svipall_solver::AppState>, engine: Arc<SolveEngine>) {
    tracing::info!("svipall-mcp worker {} started", id);
    loop {
        let job = state.queue.wait_pop().await;
        {
            let db = state.db_pool.read().await;
            let _ = db.update_status(&job.task_id, "solving");
        }
        match engine.solve(&job).await {
            Solved::Token(token) => {
                let db = state.db_pool.read().await;
                let _ = db.update_solved(&job.task_id, Some(&token), None);
                tracing::info!(worker = id, task_id = %job.task_id, "token solved");
            }
            Solved::Text(text) => {
                let db = state.db_pool.read().await;
                let _ = db.update_solved(&job.task_id, None, Some(&text));
                tracing::info!(worker = id, task_id = %job.task_id, "text solved");
            }
            // Not solvable automatically: keep it on the dashboard for a human instead of failing,
            // so captcha_status keeps returning "processing" (CAPCHA_NOT_READY) until someone solves.
            Solved::NeedsHuman(reason) => {
                let db = state.db_pool.read().await;
                let _ = db.set_needs_human(&job.task_id, &reason);
                tracing::info!(worker = id, task_id = %job.task_id, reason = %reason, "needs human — queued on dashboard");
            }
        }
    }
}

async fn run_dashboard(
    state: Arc<svipall_solver::AppState>,
    bind: &str,
    port: u16,
    token: String,
) -> anyhow::Result<()> {
    let dashboard_router = svipall_dashboard::router(state.clone(), token.as_str());
    let api_router = svipall_solver::api::router(state.clone());
    let app = dashboard_router.merge(api_router);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind, port)).await?;
    tracing::info!(
        "svipall dashboard + solver listening on http://localhost:{}/human?t={}",
        port,
        token
    );
    // Half these challenges are easier on a phone, and every coordinate the panel sends is a
    // fraction of the picture rather than a pixel, so it already works there. It is only
    // unreachable because the address printed above means the phone itself.
    if let Some(lan) =
        svipall_core::lan::dashboard_url(bind, svipall_core::lan::local_ipv4(), port, &token)
    {
        tracing::info!("reachable from a phone on this network at {}", lan);
    }
    axum::serve(listener, app).await?;
    Ok(())
}
