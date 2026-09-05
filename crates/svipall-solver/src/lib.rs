//! svipall-solver — captcha job store and HTTP API shared by the MCP server.
//!
//! Provides the job queue, SQLite persistence and the HTTP endpoints (legacy `in.php`/`res.php`
//! plus `createTask`/`getTaskResult`). Actual solving lives in `svipall-mcp` (browser token
//! extraction, local OCR, human dashboard); this crate only stores and serves jobs.

pub mod api;
pub mod db;
pub mod queue;

pub use queue::{JobType, SolverJob};

use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AppState {
    pub db_pool: Arc<RwLock<db::Db>>,
    pub queue: Arc<queue::JobQueue>,
}

impl AppState {
    pub fn new(db: db::Db, queue: queue::JobQueue) -> Self {
        Self {
            db_pool: Arc::new(RwLock::new(db)),
            queue: Arc::new(queue),
        }
    }
}
