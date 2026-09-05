//! SQLite persistence for solver jobs. WAL mode for speed.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub fn db_path() -> PathBuf {
    svipall_core::config::home_dir().join("jobs.db")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub task_id: String,
    pub job_type: String,
    pub status: String, // pending, solving, solved, failed
    pub sitekey: Option<String>,
    pub page_url: Option<String>,
    pub image_data: Option<String>,
    pub token: Option<String>,
    pub text: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub solved_at: Option<String>,
    pub attempts: i32,
    /// The shape of the challenge, as JSON: for a rate card, the page a person is being asked to
    /// judge.
    ///
    /// ▲ It was on the row and never on the record, so `list_pending` never selected it and the
    /// panel drew an empty box — a person asked to rate a page they could not see. Every label
    /// gathered that way was a guess about a blank pane.
    pub payload: Option<String>,
}

pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

/// One definition, applied to both the on-disk and the in-memory database. Keeping two copies is
/// how they drifted apart before: the in-memory one was missing the indexes entirely.
/// No index on `task_id` — the UNIQUE constraint already creates one.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        task_id TEXT UNIQUE NOT NULL,
        job_type TEXT NOT NULL,
        status TEXT NOT NULL,
        sitekey TEXT,
        page_url TEXT,
        image_data TEXT,
        token TEXT,
        text TEXT,
        error TEXT,
        created_at TEXT NOT NULL,
        solved_at TEXT,
        attempts INTEGER DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_status ON jobs(status);
    CREATE TABLE IF NOT EXISTS balances (key TEXT PRIMARY KEY, value REAL);";

/// Bumping this runs the steps in `migrate`.
///
/// This existed nowhere before, and that was a bug waiting to happen rather than a missing luxury:
/// every statement above says `IF NOT EXISTS`, so a database created by an earlier version keeps
/// its old columns forever. New code would work perfectly in development, where the file is always
/// fresh, and fail on any machine that had run the tool before.
const SCHEMA_VERSION: i64 = 3;

/// Columns added in v2, so a challenge can carry more than one base64 image.
///
/// `payload` and `answer` are small JSON: the shape of the challenge and the shape of the answer.
/// The *bytes* deliberately do not live here — see `assets`.
const MIGRATE_1_TO_2: &str = "
    ALTER TABLE jobs ADD COLUMN widget TEXT;
    ALTER TABLE jobs ADD COLUMN modality TEXT;
    ALTER TABLE jobs ADD COLUMN payload TEXT;
    ALTER TABLE jobs ADD COLUMN answer TEXT;
    ALTER TABLE jobs ADD COLUMN strategy TEXT;
    ALTER TABLE jobs ADD COLUMN latency_ms INTEGER;

    -- Bytes belong out of the row. A four-by-four grid is seventeen images, and `list_pending`
    -- pushes every pending row over the WebSocket every couple of seconds.
    CREATE TABLE IF NOT EXISTS assets (
      id         TEXT PRIMARY KEY,
      job_id     TEXT NOT NULL,
      kind       TEXT NOT NULL,
      idx        INTEGER,
      mime       TEXT NOT NULL,
      bytes      BLOB NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_assets_job ON assets(job_id);

    -- Append-only, so history survives. `record_report` rewrites a job's status, which is exactly
    -- why the old counts could not answer 'what worked here last month'.
    CREATE TABLE IF NOT EXISTS outcomes (
      at       INTEGER NOT NULL,
      widget   TEXT NOT NULL,
      modality TEXT NOT NULL,
      strategy TEXT NOT NULL,
      domain   TEXT,
      ok       INTEGER NOT NULL,
      ms       INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_outcomes_route ON outcomes(widget, modality, strategy);
";

/// v3: the corpus. A job now says who answered it (`source`: model, zeroshot, human) and whether
/// the answer was accepted (`ok`), which is what turns stored challenges into training data.
/// `outcomes.at` becomes the integer it was declared as — it had been written as an RFC 3339
/// string, which SQLite tolerated and every time-windowed query would have tripped over.
const MIGRATE_2_TO_3: &str = "
    ALTER TABLE jobs ADD COLUMN source TEXT;
    ALTER TABLE jobs ADD COLUMN ok INTEGER;
    CREATE TABLE outcomes_v3 (
      at       INTEGER NOT NULL,
      widget   TEXT NOT NULL,
      modality TEXT NOT NULL,
      strategy TEXT NOT NULL,
      domain   TEXT,
      ok       INTEGER NOT NULL,
      ms       INTEGER NOT NULL
    );
    INSERT INTO outcomes_v3
      SELECT COALESCE(CAST(strftime('%s', at) AS INTEGER), 0), widget, modality, strategy, domain, ok, ms
      FROM outcomes;
    DROP TABLE outcomes;
    ALTER TABLE outcomes_v3 RENAME TO outcomes;
    CREATE INDEX IF NOT EXISTS idx_outcomes_route ON outcomes(widget, modality, strategy);
    CREATE INDEX IF NOT EXISTS idx_outcomes_at ON outcomes(at);
    CREATE INDEX IF NOT EXISTS idx_assets_created ON assets(created_at);
";

/// Bring a database of any age up to the current schema.
fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let mut v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    // A database from before versioning existed reports 0 but already has the v1 tables. Treating
    // it as brand new would try to add columns that are there and fail on the first one.
    if v == 0 {
        let has_jobs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='jobs'",
            [],
            |r| r.get(0),
        )?;
        if has_jobs > 0 {
            v = 1;
        }
    }
    if v < 1 {
        conn.execute_batch(SCHEMA)?;
    }
    if v < 2 {
        conn.execute_batch(MIGRATE_1_TO_2)?;
    }
    if v < 3 {
        conn.execute_batch(MIGRATE_2_TO_3)?;
    }
    conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    Ok(())
}

impl Db {
    pub fn open() -> anyhow::Result<Self> {
        let path = db_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=10000;
             PRAGMA auto_vacuum=INCREMENTAL;",
        )?;
        Self::init(conn)
    }

    /// A throwaway database for tests: same schema, nothing on disk, no interference between tests.
    pub fn open_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    /// Take an already-open connection and bring it up to date.
    ///
    /// Exists so a test can build a database in an older shape and prove the migration carries it
    /// forward, which is the only way to catch the `IF NOT EXISTS` trap before a user does.
    pub fn adopt(conn: Connection) -> anyhow::Result<Self> {
        Self::init(conn)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM balances WHERE key='balance'",
            [],
            |r| r.get(0),
        )?;
        if count == 0 {
            conn.execute(
                "INSERT INTO balances (key, value) VALUES ('balance', 100.0)",
                [],
            )?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn create_job(
        &self,
        job_type: &str,
        sitekey: Option<&str>,
        page_url: Option<&str>,
        image_data: Option<&str>,
    ) -> anyhow::Result<JobRecord> {
        let id = Uuid::new_v4().to_string();
        let task_id = Uuid::new_v4().to_string()[..8].to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO jobs (id, task_id, job_type, status, sitekey, page_url, image_data, created_at, attempts) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, 0)",
            params![id, task_id, job_type, sitekey, page_url, image_data, now],
        )?;
        Ok(JobRecord {
            id,
            task_id,
            job_type: job_type.to_string(),
            status: "pending".to_string(),
            sitekey: sitekey.map(|s| s.to_string()),
            page_url: page_url.map(|s| s.to_string()),
            image_data: image_data.map(|s| s.to_string()),
            token: None,
            text: None,
            error: None,
            created_at: now,
            solved_at: None,
            attempts: 0,
            payload: None,
        })
    }

    pub fn get_by_task_id(&self, task_id: &str) -> anyhow::Result<Option<JobRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, task_id, job_type, status, sitekey, page_url, image_data, token, text, error, created_at, solved_at, attempts, payload FROM jobs WHERE task_id=?1")?;
        let mut rows = stmt.query(params![task_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(JobRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                job_type: row.get(2)?,
                status: row.get(3)?,
                sitekey: row.get(4)?,
                page_url: row.get(5)?,
                image_data: row.get(6)?,
                token: row.get(7)?,
                text: row.get(8)?,
                error: row.get(9)?,
                created_at: row.get(10)?,
                solved_at: row.get(11)?,
                attempts: row.get(12)?,
                payload: row.get(13)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_solved(
        &self,
        task_id: &str,
        token: Option<&str>,
        text: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status='solved', token=?1, text=?2, solved_at=?3 WHERE task_id=?4",
            params![token, text, now, task_id],
        )?;
        Ok(())
    }

    pub fn update_failed(&self, task_id: &str, error: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status='failed', error=?1 WHERE task_id=?2",
            params![error, task_id],
        )?;
        Ok(())
    }

    pub fn update_status(&self, task_id: &str, status: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status=?1 WHERE task_id=?2",
            params![status, task_id],
        )?;
        Ok(())
    }

    /// Auto-solving could not finish this job: park it as `human` (still "processing" to the API)
    /// with a hint, so the dashboard shows it for a person to solve.
    pub fn set_needs_human(&self, task_id: &str, reason: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status='human', error=?1 WHERE task_id=?2",
            params![reason, task_id],
        )?;
        Ok(())
    }

    pub fn list_pending(&self) -> anyhow::Result<Vec<JobRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, task_id, job_type, status, sitekey, page_url, image_data, token, text, error, created_at, solved_at, attempts, payload FROM jobs WHERE status IN ('pending','solving','human') ORDER BY CASE status WHEN 'human' THEN 0 WHEN 'solving' THEN 1 ELSE 2 END, created_at ASC LIMIT 100")?;
        let rows = stmt.query_map([], |row| {
            Ok(JobRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                job_type: row.get(2)?,
                status: row.get(3)?,
                sitekey: row.get(4)?,
                page_url: row.get(5)?,
                image_data: row.get(6)?,
                token: row.get(7)?,
                text: row.get(8)?,
                error: row.get(9)?,
                created_at: row.get(10)?,
                solved_at: row.get(11)?,
                attempts: row.get(12)?,
                payload: row.get(13)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_balance(&self) -> f64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM balances WHERE key='balance'", [], |r| {
            r.get(0)
        })
        .unwrap_or(100.0)
    }

    /// Fail non-terminal jobs older than `ttl_minutes` (orphaned by a restart or never solved by a
    /// human) so the dashboard does not accumulate stale work. Returns how many were expired.
    /// ▲ A rate card is exempt. The others are a page waiting on a wall — thirty minutes late and
    /// the fetch behind them has long since given up — but a rate card is a question left on a
    /// panel for whoever next opens it, and failing those after half an hour is why the human
    /// labelling path produced nothing. Nobody is blocked on the answer, so nothing is gained by
    /// throwing the question away.
    pub fn expire_stale(&self, ttl_minutes: i64) -> usize {
        let cutoff = (Utc::now() - chrono::Duration::minutes(ttl_minutes)).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status='failed', error=COALESCE(error,'expired: not solved in time') WHERE status IN ('pending','solving','human') AND COALESCE(modality,'') <> 'rate' AND created_at < ?1",
            params![cutoff],
        ).unwrap_or(0)
    }

    /// Retention. `expire_stale` only ever marked rows `failed`, so nothing was deleted, and every
    /// image captcha kept its full base64 payload in the row forever. On a machine that solves a
    /// few hundred image captchas that is hundreds of megabytes of `jobs.db` doing nothing.
    ///
    /// `corpus_days` is how long a finished job keeps its images so they can be exported as
    /// training data (`svipall solver export-corpus`); zero keeps none. A job row lives for the
    /// longer of `keep_hours` and the corpus window, and an asset never outlives its job — that
    /// used to be the case too, silently: `assets` was never cleaned at all.
    ///
    /// Returns (rows deleted, image payloads dropped, assets deleted).
    pub fn housekeep(&self, keep_hours: i64, corpus_days: i64) -> (usize, usize, usize) {
        let corpus_hours = corpus_days.max(0) * 24;
        let row_cutoff =
            (Utc::now() - chrono::Duration::hours(keep_hours.max(corpus_hours))).to_rfc3339();
        let corpus_cutoff = (Utc::now() - chrono::Duration::hours(corpus_hours)).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        // A solved image captcha needs its answer; the bitmap stays only inside the corpus window.
        let images = conn
            .execute(
                "UPDATE jobs SET image_data=NULL WHERE image_data IS NOT NULL AND status IN ('solved','failed') AND COALESCE(solved_at, created_at) < ?1",
                params![corpus_cutoff],
            )
            .unwrap_or(0);
        let deleted = conn
            .execute(
                "DELETE FROM jobs WHERE status IN ('solved','failed') AND COALESCE(solved_at, created_at) < ?1",
                params![row_cutoff],
            )
            .unwrap_or(0);
        let assets = conn
            .execute(
                "DELETE FROM assets WHERE job_id NOT IN (SELECT id FROM jobs) OR created_at < ?1",
                params![corpus_cutoff],
            )
            .unwrap_or(0);
        if deleted > 0 || assets > 0 {
            // Reclaim the pages rather than leaving the file at its high-water mark.
            let _ = conn.execute_batch("PRAGMA incremental_vacuum;");
        }
        (deleted, images, assets)
    }

    /// Everything the corpus knows about finished challenges since `since` (unix seconds), newest
    /// first. Bytes are not included; `asset` fetches them one at a time.
    pub fn corpus(
        &self,
        since: i64,
        modality: Option<&str>,
        source: Option<&str>,
    ) -> Vec<CorpusRow> {
        let since_iso = chrono::DateTime::from_timestamp(since, 0)
            .unwrap_or_default()
            .to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, task_id, job_type, widget, modality, payload, answer, source, ok, created_at, image_data IS NOT NULL
             FROM jobs
             WHERE status IN ('solved','failed') AND created_at >= ?1
               AND (?2 IS NULL OR modality = ?2) AND (?3 IS NULL OR source = ?3)
             ORDER BY created_at DESC",
        ) else {
            return Vec::new();
        };
        let rows: Vec<CorpusRow> = stmt
            .query_map(params![since_iso, modality, source], |r| {
                Ok(CorpusRow {
                    job_id: r.get(0)?,
                    task_id: r.get(1)?,
                    job_type: r.get(2)?,
                    widget: r.get(3)?,
                    modality: r.get(4)?,
                    payload: r.get(5)?,
                    answer: r.get(6)?,
                    source: r.get(7)?,
                    ok: r.get::<_, Option<i64>>(8)?.map(|v| v != 0),
                    created_at: r.get(9)?,
                    has_image: r.get::<_, i64>(10)? != 0,
                    assets: Vec::new(),
                })
            })
            .map(|it| it.flatten().collect())
            .unwrap_or_default();
        drop(stmt);
        rows.into_iter()
            .map(|mut row| {
                let Ok(mut s) = conn.prepare(
                    "SELECT id, kind, idx, mime FROM assets WHERE job_id = ?1 ORDER BY idx, rowid",
                ) else {
                    return row;
                };
                row.assets = s
                    .query_map([&row.job_id], |r| {
                        Ok(CorpusAsset {
                            id: r.get(0)?,
                            kind: r.get(1)?,
                            idx: r.get(2)?,
                            mime: r.get(3)?,
                        })
                    })
                    .map(|it| it.flatten().collect())
                    .unwrap_or_default();
                row
            })
            .collect()
    }

    /// The legacy single-image payload of a job, decoded.
    pub fn image_bytes(&self, task_id: &str) -> Option<Vec<u8>> {
        use base64::Engine as _;
        let conn = self.conn.lock().unwrap();
        let b64: Option<String> = conn
            .query_row(
                "SELECT image_data FROM jobs WHERE task_id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        base64::engine::general_purpose::STANDARD
            .decode(b64?.trim())
            .ok()
    }

    /// Record whether a solution was accepted, so the outcome is not simply acknowledged and
    /// discarded. Feeds the per-type success counts reported by `web_status`.
    pub fn record_report(
        &self,
        task_id: &str,
        good: bool,
        note: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let status = if good { "solved" } else { "failed" };
        let text = note.map(|n| format!("report: {n}"));
        conn.execute(
            "UPDATE jobs SET status = ?1, error = COALESCE(?2, error) WHERE task_id = ?3",
            params![status, text, task_id],
        )?;
        Ok(())
    }

    /// Solved and failed counts per challenge type, for reporting.
    pub fn outcomes_by_type(&self) -> Vec<(String, i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT job_type,
                    SUM(CASE WHEN status = 'solved' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END)
             FROM jobs GROUP BY job_type",
        ) else {
            return Vec::new();
        };
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Attach one image, sound or fragment to a job, and return the id it is served under.
    ///
    /// Bytes never travel in a job row: `list_pending` pushes every pending row over the socket
    /// every couple of seconds, and a four-by-four grid is seventeen images. The panel asks for
    /// these one at a time, by id.
    pub fn put_asset(
        &self,
        job_id: &str,
        kind: &str,
        idx: i64,
        mime: &str,
        bytes: &[u8],
    ) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO assets (id, job_id, kind, idx, mime, bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                job_id,
                kind,
                idx,
                mime,
                bytes,
                chrono::Utc::now().to_rfc3339()
            ],
        );
        id
    }

    /// One asset's mime type and bytes.
    pub fn asset(&self, id: &str) -> Option<(String, Vec<u8>)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT mime, bytes FROM assets WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()
    }

    /// The assets of one job, in the order the challenge lays them out.
    pub fn assets_for(&self, job_id: &str) -> Vec<(String, String, i64)> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) =
            conn.prepare("SELECT id, kind, idx FROM assets WHERE job_id = ?1 ORDER BY idx, rowid")
        else {
            return Vec::new();
        };
        stmt.query_map([job_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Say what a job is actually asking, so the panel can draw it and the server can check the
    /// answer against it.
    pub fn set_challenge(
        &self,
        task_id: &str,
        widget: &str,
        modality: &str,
        payload: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET widget = ?1, modality = ?2, payload = ?3 WHERE task_id = ?4",
            rusqlite::params![widget, modality, payload, task_id],
        )?;
        Ok(())
    }

    /// What this job is asking for, if it has been told.
    pub fn modality_of(&self, task_id: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT modality FROM jobs WHERE task_id = ?1",
            [task_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    /// Store the answer a person or a strategy gave, as it was given, and who gave it
    /// (`model`, `zeroshot`, `human`).
    pub fn set_answer(&self, task_id: &str, answer: &str, source: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET answer = ?1, source = ?2 WHERE task_id = ?3",
            rusqlite::params![answer, source, task_id],
        )?;
        Ok(())
    }

    /// Whether the answer was accepted by the page, once that is known.
    pub fn set_ok(&self, task_id: &str, ok: bool) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET ok = ?1, status = CASE WHEN ?1 THEN 'solved' ELSE 'failed' END, solved_at = ?2 WHERE task_id = ?3",
            rusqlite::params![ok as i64, Utc::now().to_rfc3339(), task_id],
        )?;
        Ok(())
    }

    /// A job for a challenge solved on the page itself, outside the queue, so what it showed and
    /// how it was answered join the corpus like everything else.
    pub fn create_local_job(
        &self,
        page_url: Option<&str>,
        widget: &str,
        modality: &str,
        payload: Option<&str>,
    ) -> anyhow::Result<JobRecord> {
        let job = self.create_job("in_page", None, page_url, None)?;
        self.set_challenge(&job.task_id, widget, modality, payload)?;
        self.update_status(&job.task_id, "solving")?;
        Ok(job)
    }

    /// Record what a strategy did, once, append-only.
    ///
    /// Deliberately not a column on `jobs`: `record_report` overwrites `status`, which is exactly
    /// why the history a ranking needs is not there today. A row here is never revised, so the
    /// tenth attempt on a widget does not erase what the first nine taught.
    pub fn record_outcome(
        &self,
        widget: &str,
        modality: &str,
        strategy: &str,
        domain: &str,
        ok: bool,
        ms: i64,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO outcomes (at, widget, modality, strategy, domain, ok, ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                chrono::Utc::now().timestamp(),
                widget,
                modality,
                strategy,
                domain,
                ok as i64,
                ms
            ],
        );
    }

    /// Successes and attempts per strategy, for ordering what to try next.
    pub fn strategy_history(&self) -> Vec<(String, u32, u32)> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) =
            conn.prepare("SELECT strategy, SUM(ok), COUNT(*) FROM outcomes GROUP BY strategy")
        else {
            return Vec::new();
        };
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u32,
                r.get::<_, i64>(2)? as u32,
            ))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Successes, attempts and mean latency per strategy **on one route** — a widget asking one
    /// modality — with the same across every route as the fallback.
    ///
    /// `strategy_history` above throws away the `widget` and `modality` columns it just stored,
    /// so it can only say "image-grid works"; this can say "image-grid works on hcaptcha and not
    /// on arkose", which is the question the ordering actually asks. The index
    /// `idx_outcomes_route` exists for exactly this query.
    ///
    /// Returns `(strategy, ok, tried, mean_ms)` for the route, and the same for everything, so the
    /// caller can prefer the route's own numbers once there are enough of them.
    pub fn route_history(&self, widget: &str, modality: &str) -> RouteHistory {
        let conn = self.conn.lock().unwrap();
        let read = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Vec<(String, u32, u32, u32)> {
            let Ok(mut stmt) = conn.prepare(sql) else {
                return Vec::new();
            };
            stmt.query_map(params, |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u32,
                    r.get::<_, i64>(2)? as u32,
                    r.get::<_, f64>(3)?.max(0.0) as u32,
                ))
            })
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
        };
        RouteHistory {
            route: read(
                "SELECT strategy, SUM(ok), COUNT(*), COALESCE(AVG(ms), 0)
                 FROM outcomes WHERE widget = ?1 AND modality = ?2 GROUP BY strategy",
                &[&widget, &modality],
            ),
            everywhere: read(
                "SELECT strategy, SUM(ok), COUNT(*), COALESCE(AVG(ms), 0)
                 FROM outcomes GROUP BY strategy",
                &[],
            ),
        }
    }

    /// Bytes the database occupies, for reporting through `web_status`.
    pub fn size_bytes(&self) -> u64 {
        std::fs::metadata(db_path()).map(|m| m.len()).unwrap_or(0)
    }

    /// Count an attempt against a job. The column existed from the start and was never written, so
    /// a job that kept failing looked identical to one on its first try.
    pub fn bump_attempts(&self, task_id: &str) -> i32 {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE jobs SET attempts = attempts + 1 WHERE task_id = ?1",
            params![task_id],
        );
        conn.query_row(
            "SELECT attempts FROM jobs WHERE task_id = ?1",
            params![task_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let conn = self.conn.lock().unwrap();
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE status IN ('pending','human')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let solving: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE status='solving'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let solved: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs WHERE status='solved'", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        (pending as usize, solving as usize, solved as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_memory().expect("in-memory db")
    }

    #[test]
    fn the_bytes_of_a_challenge_live_beside_the_job_not_inside_it() {
        // Seventeen images pushed over a socket every two seconds is the failure this avoids.
        let db = Db::open_memory().expect("open");
        let a = db.put_asset("job-1", "tile", 0, "image/png", &[1, 2, 3]);
        let b = db.put_asset("job-1", "tile", 1, "image/png", &[4, 5]);
        assert_ne!(a, b, "each asset is addressed on its own");
        assert_eq!(db.asset(&a), Some(("image/png".to_string(), vec![1, 2, 3])));
        let listed = db.assets_for("job-1");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, a, "order follows the layout of the challenge");
    }

    #[test]
    fn an_asset_that_does_not_exist_is_absent_rather_than_an_error() {
        assert_eq!(Db::open_memory().expect("open").asset("nope"), None);
    }

    #[test]
    fn a_job_remembers_what_it_is_asking_for() {
        // Without this the panel cannot draw the right control and the server cannot tell a good
        // answer from an answer to a different question.
        let db = Db::open_memory().expect("open");
        let job = db
            .create_job("turnstile", Some("site"), Some("https://x.test/"), None)
            .expect("created");
        let id = job.task_id.clone();
        db.set_challenge(&id, "challenges.example", "tiles", Some(r#"{"rows":3}"#))
            .expect("set");
        assert_eq!(db.modality_of(&id).as_deref(), Some("tiles"));
    }

    #[test]
    fn a_job_nobody_has_classified_reports_no_modality_rather_than_guessing() {
        let db = Db::open_memory().expect("open");
        let job = db
            .create_job("turnstile", Some("site"), Some("https://x.test/"), None)
            .expect("created");
        assert_eq!(db.modality_of(&job.task_id), None);
        assert_eq!(db.modality_of("no-such-job"), None);
    }

    #[test]
    fn what_a_strategy_did_is_kept_attempt_by_attempt_not_overwritten() {
        // The ranking needs the whole history. A column that the latest attempt overwrites is why
        // this lives in its own table rather than on `jobs`.
        let db = Db::open_memory().expect("open");
        db.record_outcome("a.example", "tiles", "grid-model", "shop.test", false, 900);
        db.record_outcome("a.example", "tiles", "grid-model", "shop.test", true, 850);
        db.record_outcome("a.example", "tiles", "grid-model", "shop.test", true, 700);
        let h = db.strategy_history();
        assert_eq!(h, vec![("grid-model".to_string(), 2, 3)]);
    }

    #[test]
    fn each_strategy_is_counted_on_its_own() {
        let db = Db::open_memory().expect("open");
        db.record_outcome("a.example", "nonce", "proof-of-work", "x.test", true, 40);
        db.record_outcome("b.example", "tiles", "grid-model", "y.test", false, 1200);
        let mut h = db.strategy_history();
        h.sort();
        assert_eq!(
            h,
            vec![
                ("grid-model".to_string(), 0, 1),
                ("proof-of-work".to_string(), 1, 1),
            ]
        );
    }

    #[test]
    fn a_route_learns_its_own_lesson_once_it_has_enough_of_them() {
        // The global tally says image-grid works two times in three. On arkose it has failed
        // three times out of three; that is what the ordering on arkose should see, and what the
        // ordering on hcaptcha should not.
        let db = Db::open_memory().expect("open");
        for ok in [true, true, false] {
            db.record_outcome("hcaptcha.com", "tiles", "image-grid", "a.test", ok, 800);
        }
        for _ in 0..3 {
            db.record_outcome(
                "client-api.arkoselabs.com",
                "tiles",
                "image-grid",
                "b.test",
                false,
                1500,
            );
        }
        let arkose = db.route_history("client-api.arkoselabs.com", "tiles");
        assert_eq!(arkose.for_strategy("image-grid", 3), Some((0, 3, 1500)));
        // Two data points on a route are not enough; the global numbers stand in.
        assert_eq!(arkose.for_strategy("image-grid", 4), Some((2, 6, 1150)));
        let hcaptcha = db.route_history("hcaptcha.com", "tiles");
        assert_eq!(hcaptcha.for_strategy("image-grid", 3), Some((2, 3, 800)));
        assert_eq!(hcaptcha.for_strategy("never-tried", 3), None);
        assert!(Db::open_memory()
            .unwrap()
            .route_history("x", "y")
            .route
            .is_empty());
    }

    #[test]
    fn a_machine_that_has_solved_nothing_yet_reports_no_history_rather_than_failing() {
        assert!(Db::open_memory()
            .expect("open")
            .strategy_history()
            .is_empty());
    }

    #[test]
    fn the_in_memory_schema_matches_the_on_disk_one() {
        let d = db();
        let conn = d.conn.lock().unwrap();
        // Both go through SCHEMA now; before, the in-memory copy silently lacked the indexes.
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_status'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "idx_status missing from the in-memory schema");
    }

    #[test]
    fn a_solved_job_keeps_its_answer_but_loses_the_bitmap() {
        let d = db();
        let job = d
            .create_job("ImageToText", None, None, Some("QUJDMTIz"))
            .unwrap();
        d.update_solved(&job.task_id, None, Some("ABC123")).unwrap();

        let (_deleted, images, _) = d.housekeep(24, 0);
        assert_eq!(images, 1, "the stored image should have been dropped");

        let after = d.get_by_task_id(&job.task_id).unwrap().unwrap();
        assert_eq!(
            after.text.as_deref(),
            Some("ABC123"),
            "the answer must survive"
        );
        assert_eq!(after.image_data, None, "the bitmap must not");
    }

    #[test]
    fn unsolved_jobs_are_never_deleted_by_housekeeping() {
        let d = db();
        let job = d
            .create_job("Turnstile", Some("k"), Some("https://x.test"), None)
            .unwrap();
        let (deleted, _, _) = d.housekeep(0, 0);
        assert_eq!(deleted, 0);
        assert!(
            d.get_by_task_id(&job.task_id).unwrap().is_some(),
            "a pending job must survive housekeeping"
        );
    }

    #[test]
    fn finished_jobs_past_the_retention_window_are_deleted() {
        let d = db();
        let job = d
            .create_job("Turnstile", None, Some("https://x.test"), None)
            .unwrap();
        d.update_solved(&job.task_id, Some("token-value"), None)
            .unwrap();
        // keep_hours = 0 makes anything already finished eligible.
        let (deleted, _, _) = d.housekeep(0, 0);
        assert_eq!(deleted, 1);
        assert!(d.get_by_task_id(&job.task_id).unwrap().is_none());
    }

    #[test]
    fn a_rejected_solution_is_recorded_rather_than_acknowledged_and_lost() {
        let d = db();
        let job = d
            .create_job("Turnstile", None, Some("https://x.test"), None)
            .unwrap();
        d.update_solved(&job.task_id, Some("a-token"), None)
            .unwrap();
        d.record_report(&job.task_id, false, Some("site rejected it"))
            .unwrap();

        let after = d.get_by_task_id(&job.task_id).unwrap().unwrap();
        assert_eq!(
            after.status, "failed",
            "a refused token must not stay marked solved"
        );
        assert!(after.error.unwrap().contains("site rejected it"));
    }

    #[test]
    fn outcomes_are_grouped_by_challenge_type() {
        let d = db();
        for (kind, good) in [
            ("Turnstile", true),
            ("Turnstile", false),
            ("HCaptcha", true),
        ] {
            let j = d
                .create_job(kind, None, Some("https://x.test"), None)
                .unwrap();
            d.record_report(&j.task_id, good, None).unwrap();
        }
        let mut rows = d.outcomes_by_type();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                ("HCaptcha".to_string(), 1, 0),
                ("Turnstile".to_string(), 1, 1),
            ]
        );
    }

    #[test]
    fn attempts_are_counted() {
        let d = db();
        let job = d
            .create_job("Turnstile", None, Some("https://x.test"), None)
            .unwrap();
        assert_eq!(d.get_by_task_id(&job.task_id).unwrap().unwrap().attempts, 0);
        assert_eq!(d.bump_attempts(&job.task_id), 1);
        assert_eq!(d.bump_attempts(&job.task_id), 2);
        assert_eq!(d.get_by_task_id(&job.task_id).unwrap().unwrap().attempts, 2);
    }

    /// ▲ A person cannot judge a page they cannot see. `payload` was written to the row and never
    /// selected back, so the panel drew an empty box and every label it collected was a guess
    /// about a blank pane. This is the whole reason the human labelling path produced nothing.
    #[test]
    fn what_a_person_is_asked_to_judge_reaches_the_panel() {
        let d = db();
        let job = d
            .create_job("rate", None, Some("https://x.test/article"), None)
            .unwrap();
        let payload = r#"{"text":"the eastern quay reopens on the fourteenth of November"}"#;
        d.set_challenge(&job.task_id, "page", "rate", Some(payload))
            .unwrap();

        let pending = d.list_pending().unwrap();
        let row = pending
            .iter()
            .find(|j| j.task_id == job.task_id)
            .expect("the job is waiting");
        assert_eq!(
            row.payload.as_deref(),
            Some(payload),
            "the panel was sent a card with nothing on it"
        );
        assert_eq!(
            d.get_by_task_id(&job.task_id)
                .unwrap()
                .unwrap()
                .payload
                .as_deref(),
            Some(payload)
        );
    }

    /// ▲ A rate card is not a page waiting on a wall. The others have a fetch blocked behind them
    /// and half an hour late that fetch is long gone; a rate card is a question left for whoever
    /// next opens the panel, and failing those after thirty minutes is why nobody ever answered
    /// one. Nothing is gained by throwing the question away.
    #[test]
    fn a_question_left_on_the_panel_outlives_the_reaper_that_clears_stuck_challenges() {
        let d = db();
        let stuck = d
            .create_job("Turnstile", None, Some("https://x.test"), None)
            .unwrap();
        let card = d
            .create_job("rate", None, Some("https://x.test/a"), None)
            .unwrap();
        d.set_challenge(&card.task_id, "page", "rate", Some(r#"{"text":"a page"}"#))
            .unwrap();

        assert_eq!(d.expire_stale(0), 1, "only the stuck challenge expires");
        assert_eq!(
            d.get_by_task_id(&stuck.task_id).unwrap().unwrap().status,
            "failed"
        );
        assert_eq!(
            d.get_by_task_id(&card.task_id).unwrap().unwrap().status,
            "pending",
            "the rate card was thrown away with nobody blocked on it"
        );
    }

    #[test]
    fn expire_stale_parks_old_work_without_deleting_it() {
        let d = db();
        let job = d
            .create_job("Turnstile", None, Some("https://x.test"), None)
            .unwrap();
        assert_eq!(d.expire_stale(0), 1);
        let after = d.get_by_task_id(&job.task_id).unwrap().unwrap();
        assert_eq!(after.status, "failed");
        assert!(after.error.is_some());
    }
}

/// What strategies have done on one route, and everywhere, as `(strategy, ok, tried, mean_ms)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteHistory {
    pub route: Vec<(String, u32, u32, u32)>,
    pub everywhere: Vec<(String, u32, u32, u32)>,
}

impl RouteHistory {
    /// The route's own numbers once it has seen `min_route` attempts of a strategy; the global
    /// numbers before that. A route with two data points is noise dressed as knowledge.
    pub fn for_strategy(&self, name: &str, min_route: u32) -> Option<(u32, u32, u32)> {
        let on_route = self
            .route
            .iter()
            .find(|(n, _, _, _)| n == name)
            .filter(|(_, _, tried, _)| *tried >= min_route);
        on_route
            .or_else(|| self.everywhere.iter().find(|(n, _, _, _)| n == name))
            .map(|(_, ok, tried, ms)| (*ok, *tried, *ms))
    }
}

/// One finished challenge, as the corpus sees it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CorpusRow {
    pub job_id: String,
    pub task_id: String,
    pub job_type: String,
    pub widget: Option<String>,
    pub modality: Option<String>,
    /// What the challenge asked, as JSON (prompt, grid shape, …).
    pub payload: Option<String>,
    /// The answer as given, as JSON.
    pub answer: Option<String>,
    /// `model`, `zeroshot` or `human`.
    pub source: Option<String>,
    /// Whether the page accepted it; `None` when never learned.
    pub ok: Option<bool>,
    pub created_at: String,
    /// The job still carries its legacy single-image payload.
    pub has_image: bool,
    pub assets: Vec<CorpusAsset>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CorpusAsset {
    pub id: String,
    pub kind: String,
    pub idx: Option<i64>,
    pub mime: String,
}

#[cfg(test)]
mod corpus_tests {
    use super::*;

    fn finish(d: &Db, task_id: &str, ok: bool, back_hours: i64) {
        let _ = d.set_answer(task_id, r#"{"kind":"tiles","indices":[1]}"#, "model");
        let _ = d.set_ok(task_id, ok);
        // Age the row by hand: retention is judged on these timestamps.
        let when = (Utc::now() - chrono::Duration::hours(back_hours)).to_rfc3339();
        let conn = d.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET created_at = ?1, solved_at = ?1 WHERE task_id = ?2",
            params![when, task_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE assets SET created_at = ?1 WHERE job_id = (SELECT id FROM jobs WHERE task_id = ?2)",
            params![when, task_id],
        )
        .unwrap();
    }

    #[test]
    fn assets_die_with_their_job_and_orphans_are_swept() {
        let d = Db::open_memory().unwrap();
        let job = d
            .create_local_job(Some("https://x.example/"), "w", "tiles", Some("{}"))
            .unwrap();
        d.put_asset(&job.id, "tile", 0, "image/png", b"png");
        finish(&d, &job.task_id, true, 24 * 40);
        let (rows, _, assets) = d.housekeep(24, 30);
        assert_eq!(rows, 1);
        assert_eq!(assets, 1, "an asset never outlives its job");
        assert!(d.assets_for(&job.id).is_empty());
    }

    #[test]
    fn a_finished_job_keeps_its_images_until_the_corpus_window_closes() {
        let d = Db::open_memory().unwrap();
        let job = d.create_local_job(None, "w", "tiles", Some("{}")).unwrap();
        d.put_asset(&job.id, "tile", 0, "image/png", b"png");
        finish(&d, &job.task_id, false, 24 * 5);
        let (rows, _, assets) = d.housekeep(24, 30);
        assert_eq!(
            (rows, assets),
            (0, 0),
            "five days old is inside a thirty-day window"
        );
        let (rows, _, assets) = d.housekeep(24 * 10, 0);
        assert_eq!(
            (rows, assets),
            (0, 1),
            "a zero-day corpus keeps no images; the row stays for its answer"
        );
        assert_eq!(d.corpus(0, None, None).len(), 1);
    }

    #[test]
    fn the_corpus_lists_what_was_asked_who_answered_and_whether_it_worked() {
        let d = Db::open_memory().unwrap();
        let a = d
            .create_local_job(None, "w", "tiles", Some(r#"{"prompt":"buses"}"#))
            .unwrap();
        d.put_asset(&a.id, "tile", 1, "image/png", b"one");
        d.put_asset(&a.id, "tile", 0, "image/png", b"zero");
        finish(&d, &a.task_id, true, 1);
        let b = d.create_local_job(None, "w", "text", None).unwrap();
        let _ = d.set_answer(&b.task_id, r#"{"kind":"text","value":"ab12"}"#, "human");
        let _ = d.set_ok(&b.task_id, false);
        let pending = d.create_local_job(None, "w", "tiles", None).unwrap();

        let all = d.corpus(0, None, None);
        assert_eq!(all.len(), 2, "unfinished jobs are not corpus: {all:?}");
        let tiles = d.corpus(0, Some("tiles"), None);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].payload.as_deref(), Some(r#"{"prompt":"buses"}"#));
        assert_eq!(tiles[0].source.as_deref(), Some("model"));
        assert_eq!(tiles[0].ok, Some(true));
        assert_eq!(
            tiles[0].assets.iter().map(|x| x.idx).collect::<Vec<_>>(),
            vec![Some(0), Some(1)],
            "in layout order"
        );
        let human = d.corpus(0, None, Some("human"));
        assert_eq!(human.len(), 1);
        assert_eq!(human[0].task_id, b.task_id);
        assert!(
            d.corpus(Utc::now().timestamp() + 60, None, None).is_empty(),
            "a window in the future holds nothing"
        );
        assert_eq!(
            d.get_by_task_id(&pending.task_id).unwrap().unwrap().status,
            "solving"
        );
    }

    #[test]
    fn outcomes_are_queryable_by_time_after_the_migration() {
        // A v2 database wrote `at` as text; v3 must carry those rows over as integers.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch(MIGRATE_1_TO_2).unwrap();
        conn.execute(
            "INSERT INTO outcomes (at, widget, modality, strategy, domain, ok, ms) VALUES ('2026-01-02T03:04:05.678+00:00','w','tiles','image-grid','d',1,0)",
            [],
        )
        .unwrap();
        conn.execute_batch("PRAGMA user_version = 2;").unwrap();
        let d = Db::adopt(conn).unwrap();
        d.record_outcome("w", "tiles", "image-grid", "d", false, 0);
        let conn = d.conn.lock().unwrap();
        let ats: Vec<i64> = conn
            .prepare("SELECT at FROM outcomes ORDER BY at")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(ats.len(), 2);
        assert_eq!(ats[0], 1_767_323_045, "parsed from the old text");
        assert!(ats[1] > 1_700_000_000, "written as unix seconds now");
    }
}
