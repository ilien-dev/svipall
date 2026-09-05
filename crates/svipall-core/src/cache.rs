//! Page cache, version history and retention, in one SQLite file (`~/.svipall/svipall.db`).
//!
//! Three things depend on this and none of them were possible before:
//!   * a repeated fetch costs a `304` instead of a full page, and a cursor continuation costs
//!     nothing at all;
//!   * an interrupted crawl can be resumed;
//!   * "what changed since last time" becomes a lookup rather than a re-read.
//!
//! `rusqlite` was already in the workspace for the captcha store, so this adds no build cost. The
//! captcha database stays separate on purpose: it belongs to another crate, has its own write path
//! from the solver workers, and merging them would couple the two for nothing.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Bumping this runs the migrations in `migrate`.
const SCHEMA_VERSION: i64 = 7;

/// Version 2 adds the notes an agent leaves for the next run.
///
/// Its own table rather than a column anywhere: this outlives a crawl, a session and a process,
/// which is the whole point of it.
const MIGRATE_1_TO_2: &str = "
    CREATE TABLE IF NOT EXISTS kv (
      key        TEXT PRIMARY KEY,
      value      TEXT NOT NULL,
      updated_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_kv_updated ON kv(updated_at);";

/// Version 3 adds the record of what was asked for and what answered.
///
/// "Why is this domain slow" and "which tier is actually carrying this crawl" were questions the
/// tool could not answer about itself: the ladder decided, logged a line to stderr, and forgot.
const MIGRATE_2_TO_3: &str = "
    CREATE TABLE IF NOT EXISTS request_log (
      at        INTEGER NOT NULL,
      domain    TEXT NOT NULL,
      url       TEXT NOT NULL,
      tier      TEXT NOT NULL,
      status    INTEGER NOT NULL,
      wall      TEXT,
      blocked   INTEGER NOT NULL,
      ms        INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_log_at ON request_log(at);
    CREATE INDEX IF NOT EXISTS idx_log_domain ON request_log(domain, at);";

/// Version 4 records which exit answered, so "which proxy is slow" and "which proxy is getting
/// this domain blocked" are questions the log can answer. Null for a request that used no proxy.
const MIGRATE_3_TO_4: &str = "
    ALTER TABLE request_log ADD COLUMN exit TEXT;
    CREATE INDEX IF NOT EXISTS idx_log_exit ON request_log(exit, at);";

/// Version 5 keeps how much of the page arrived, judged at the fetch.
///
/// It is stored rather than recomputed because the two paths do not have the same evidence: the
/// fetch sees the markup the text came out of, a cache hit sees only the text. Recomputing would
/// hand the same URL a different verdict depending on where the answer came from, which is worse
/// than not answering. Null on a row written before this column existed — and the reader tells
/// that apart from "nothing to report", which is an empty reason list.
const MIGRATE_4_TO_5: &str = "ALTER TABLE page ADD COLUMN quality TEXT;";

/// Version 6 adds the long operation nobody is waiting on.
///
/// A crawl was always resumable — `crawl_queue` survives a kill — but there was no way to tell a
/// crawl that *died* from one that finished: `crawl.status` is written `running` per batch and
/// nothing ever clears it. That is what this row is for, and it is why it is not redundant with the
/// table beside it.
///
/// For a crawl the id **is** the `crawl_id`, so the frontier a job resumes from is one lookup away
/// and there is one identity to learn rather than two. A kind with no crawl behind it simply has no
/// `crawl` row.
///
/// Note the word: the captcha store in `jobs.db` also calls its rows jobs. Those are challenges;
/// these are operations. The two databases stay apart for the reason stated at the top of this file.
const MIGRATE_5_TO_6: &str = "
    CREATE TABLE IF NOT EXISTS job (
      id               TEXT PRIMARY KEY,
      kind             TEXT NOT NULL,
      state            TEXT NOT NULL,
      params_json      TEXT NOT NULL,
      -- The site this job is aimed at, recorded when the job is queued. It cannot be read from
      -- `crawl.domain` instead: that row does not exist until the crawl has actually started, so
      -- the one-crawl-per-site rule would let every queued job through and only notice afterwards.
      domain           TEXT NOT NULL DEFAULT '',
      -- Which run of which process is carrying it, and when it last said so. Both halves are
      -- needed: two svipall processes can share this file, so a foreign owner alone proves nothing,
      -- and `crawl.updated_at` is written per batch, which can be half an hour inside a healthy run.
      owner            TEXT,
      heartbeat_at     INTEGER,
      -- Set by a cancel request. A flag rather than a state, because the job decides when it has
      -- actually stopped — and it stops at a page boundary, so its frontier is kept.
      cancel_requested INTEGER NOT NULL DEFAULT 0,
      -- The summary the synchronous call would have returned, deflated like a cached page is.
      result           BLOB,
      error            TEXT,
      created_at       INTEGER NOT NULL,
      started_at       INTEGER,
      finished_at      INTEGER
    );
    CREATE INDEX IF NOT EXISTS job_state ON job(state, created_at);
    CREATE INDEX IF NOT EXISTS job_live  ON job(state, heartbeat_at);
";

pub fn db_path() -> PathBuf {
    crate::config::home_dir().join("svipall.db")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// SQLite has no unsigned integers, so a `u64` hash round-trips through `i64`.
fn to_i64(v: u64) -> i64 {
    v as i64
}
fn from_i64(v: i64) -> u64 {
    v as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    /// Serve fresh hits, revalidate stale ones, store what comes back.
    ReadWrite,
    /// Serve hits, never write.
    Read,
    /// Always fetch, always store.
    Write,
    /// Ignore the cache entirely.
    Bypass,
    /// Fetch, store, and report what changed against the stored copy.
    Refresh,
}

impl CacheMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "read_write" | "readwrite" => Some(Self::ReadWrite),
            "read" | "read_only" => Some(Self::Read),
            "write" => Some(Self::Write),
            "bypass" | "off" | "none" => Some(Self::Bypass),
            "refresh" => Some(Self::Refresh),
            _ => None,
        }
    }
    pub fn may_read(self) -> bool {
        matches!(self, Self::ReadWrite | Self::Read)
    }
    pub fn may_write(self) -> bool {
        matches!(self, Self::ReadWrite | Self::Write | Self::Refresh)
    }
}

#[derive(Debug, Clone)]
pub struct CachedPage {
    pub url: String,
    pub final_url: String,
    pub status: u16,
    pub tier: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: String,
    pub title: Option<String>,
    pub markdown: String,
    pub simhash: u64,
    pub content_hash: u64,
    pub fetched_at: i64,
    pub expires_at: i64,
    /// How much of the page arrived, as judged when it was fetched. `None` on a row stored before
    /// the column existed, which is not the same as "nothing to report".
    pub quality: Option<String>,
}

/// The four 16-bit bands of a simhash, in the order the columns are declared.
///
/// `i64` rather than `u16` because that is what SQLite stores and comparing two different integer
/// types is how an index quietly stops being used.
fn bands(sim: u64) -> [i64; 4] {
    [
        ((sim >> 48) & 0xffff) as i64,
        ((sim >> 32) & 0xffff) as i64,
        ((sim >> 16) & 0xffff) as i64,
        (sim & 0xffff) as i64,
    ]
}

/// A page already in the cache that is this page again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearDuplicate {
    pub url: String,
    /// Differing bits out of 64. Zero is the identical text.
    pub distance: u32,
    pub fetched_at: i64,
}

impl CachedPage {
    pub fn is_fresh(&self) -> bool {
        self.expires_at > now()
    }
    pub fn age_secs(&self) -> i64 {
        (now() - self.fetched_at).max(0)
    }
}

#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    pub page_ttl_secs: i64,
    pub versions_per_url: usize,
    pub version_ttl_secs: i64,
    pub max_db_bytes: u64,
    /// Hysteresis: trimming down to the limit exactly would make every run trim again.
    pub target_db_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            page_ttl_secs: 7 * 86_400,
            versions_per_url: 20,
            version_ttl_secs: 90 * 86_400,
            max_db_bytes: 512 * 1024 * 1024,
            target_db_bytes: 384 * 1024 * 1024,
        }
    }
}

/// A crawl read back out of the database, ready to be continued.
#[derive(Debug, Clone)]
pub struct SavedCrawl {
    pub start_url: String,
    /// The `web_crawl` parameters the run was launched with, verbatim.
    pub params_json: String,
    /// Still queued: URL, depth, and the score it was queued at.
    pub pending: Vec<(String, u16, f32)>,
    /// Already fetched, so a resumed run neither refetches nor re-queues them.
    pub done: Vec<String>,
}

/// A long operation, as a reader sees it.
///
/// `pages_done` and `pending` come from the crawl the job is driving, not from a copy kept in the
/// job row: two answers to one question is how a poll starts disagreeing with a resume.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub state: String,
    #[serde(skip)]
    pub params_json: String,
    /// The site this job is aimed at, known from the moment it is queued.
    pub domain: String,
    pub cancel_requested: bool,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// From `crawl.pages_done`. Zero for a kind with no crawl behind it.
    pub pages_done: usize,
    /// What the crawl still has queued.
    pub pending: usize,
}

/// One job by id, joined to its crawl. Shared by every read path so the join is written once.
fn read_job(c: &Connection, id: &str) -> Option<JobRow> {
    c.query_row(
        "SELECT j.id, j.kind, j.state, j.params_json, j.domain, j.cancel_requested, j.created_at,
                j.started_at, j.finished_at, j.error,
                COALESCE(c.pages_done, 0),
                (SELECT COUNT(*) FROM crawl_queue q WHERE q.crawl_id = j.id AND q.state='pending')
         FROM job j LEFT JOIN crawl c ON c.id = j.id
         WHERE j.id = ?1",
        params![id],
        |r| {
            Ok(JobRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                state: r.get(2)?,
                params_json: r.get(3)?,
                domain: r.get(4)?,
                cancel_requested: r.get::<_, i64>(5)? != 0,
                created_at: r.get(6)?,
                started_at: r.get(7)?,
                finished_at: r.get(8)?,
                error: r.get(9)?,
                pages_done: r.get::<_, i64>(10)? as usize,
                pending: r.get::<_, i64>(11)? as usize,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

/// One line of the request log.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RequestLine {
    pub at: i64,
    pub domain: String,
    pub url: String,
    pub tier: String,
    pub status: u16,
    pub wall: Option<String>,
    pub blocked: bool,
    pub ms: u64,
    /// The exit this request left through, if any.
    pub exit: Option<String>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct HousekeepReport {
    pub pages_expired: usize,
    pub versions_trimmed: usize,
    pub pages_evicted: usize,
    pub bytes_after: u64,
}

/// Version 7 makes "have I seen this page before, anywhere" answerable.
///
/// The simhash was already stored on every page and only ever compared inside one batch, so a
/// result duplicated from a site fetched last week read as a fresh source. Answering it across the
/// whole cache needs an index, and Hamming distance has none.
///
/// ▲ Manku et al. (WWW 2007) supply the trick, and the pigeonhole principle is the whole proof:
/// split the 64 bits into four bands of sixteen, and two hashes within Hamming distance 3 can
/// differ in at most three bands, so **at least one band is equal**. Four equality indexes
/// therefore return every true near-duplicate and a small number of false ones, which the exact
/// distance then discards. It is lossless for `NEAR_DUPLICATE_BITS = 3` and would stop being so at
/// four, which is why that constant and these four bands are not independent numbers.
///
/// Backfilled in SQL rather than in Rust: the bands are a pure function of a column already on
/// disk, and a loop that reads a hundred thousand rows into a process to write them back is a slow
/// way to say `UPDATE`. The mask matters — SQLite's `>>` is arithmetic, so a simhash stored as a
/// negative `i64` would sign-extend without it.
const MIGRATE_6_TO_7: &str = "
    ALTER TABLE page ADD COLUMN band0 INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE page ADD COLUMN band1 INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE page ADD COLUMN band2 INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE page ADD COLUMN band3 INTEGER NOT NULL DEFAULT 0;
    UPDATE page SET band0 = (simhash >> 48) & 65535,
                    band1 = (simhash >> 32) & 65535,
                    band2 = (simhash >> 16) & 65535,
                    band3 =  simhash        & 65535;
    CREATE INDEX IF NOT EXISTS page_band0 ON page(band0);
    CREATE INDEX IF NOT EXISTS page_band1 ON page(band1);
    CREATE INDEX IF NOT EXISTS page_band2 ON page(band2);
    CREATE INDEX IF NOT EXISTS page_band3 ON page(band3);";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);

CREATE TABLE IF NOT EXISTS page (
  url_hash      INTEGER PRIMARY KEY,
  url           TEXT NOT NULL,
  final_url     TEXT NOT NULL,
  domain        TEXT NOT NULL,
  status        INTEGER NOT NULL,
  tier          TEXT NOT NULL,
  etag          TEXT,
  last_modified TEXT,
  content_type  TEXT NOT NULL DEFAULT '',
  title         TEXT,
  markdown      BLOB,
  simhash       INTEGER NOT NULL DEFAULT 0,
  content_hash  INTEGER NOT NULL DEFAULT 0,
  bytes         INTEGER NOT NULL DEFAULT 0,
  fetched_at    INTEGER NOT NULL,
  expires_at    INTEGER NOT NULL,
  hits          INTEGER NOT NULL DEFAULT 0,
  last_hit_at   INTEGER
);
CREATE INDEX IF NOT EXISTS page_domain   ON page(domain, fetched_at);
CREATE INDEX IF NOT EXISTS page_expires  ON page(expires_at);
CREATE INDEX IF NOT EXISTS page_evict    ON page(hits, last_hit_at);

-- Fingerprints only: 16 bytes per version answers "did it change" and "by how much" without
-- keeping every copy of every page forever.
CREATE TABLE IF NOT EXISTS page_version (
  url_hash     INTEGER NOT NULL,
  fetched_at   INTEGER NOT NULL,
  content_hash INTEGER NOT NULL,
  simhash      INTEGER NOT NULL,
  title        TEXT,
  bytes        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (url_hash, fetched_at)
) WITHOUT ROWID;

-- A crawl that stops halfway used to lose everything. These two tables make it resumable: the
-- frontier survives, so a crawl interrupted at page 40 of 200 continues from 41.
CREATE TABLE IF NOT EXISTS crawl (
  id          TEXT PRIMARY KEY,
  start_url   TEXT NOT NULL,
  domain      TEXT NOT NULL,
  params_json TEXT NOT NULL,
  status      TEXT NOT NULL,
  pages_done  INTEGER NOT NULL DEFAULT 0,
  stopped_by  TEXT,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS crawl_queue (
  crawl_id TEXT NOT NULL REFERENCES crawl(id) ON DELETE CASCADE,
  url      TEXT NOT NULL,
  depth    INTEGER NOT NULL,
  score    REAL NOT NULL,
  state    TEXT NOT NULL,
  PRIMARY KEY (crawl_id, url)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS crawl_queue_pick ON crawl_queue(crawl_id, state, score DESC);
"#;

pub struct Store {
    write: Mutex<Connection>,
    /// WAL allows unlimited concurrent readers alongside one writer, so reads never queue behind
    /// a write. A handful of read-only handles is all that model needs.
    readers: Vec<Mutex<Connection>>,
    next_reader: std::sync::atomic::AtomicUsize,
    path: PathBuf,
}

fn tune(conn: &Connection) -> Result<()> {
    // `busy_timeout` comes first on purpose: switching to WAL needs a brief exclusive lock, so two
    // instances starting at the same moment would otherwise get an immediate "database is locked"
    // with no timeout in effect yet to wait it out.
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA temp_store=MEMORY;
         PRAGMA mmap_size=268435456;
         PRAGMA auto_vacuum=INCREMENTAL;",
    )?;
    conn.set_prepared_statement_cache_capacity(32);
    Ok(())
}

impl Store {
    pub fn open() -> Result<Self> {
        let path = db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::open_at(&path)
    }

    pub fn open_at(path: &std::path::Path) -> Result<Self> {
        let write =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        tune(&write)?;
        write.execute_batch(SCHEMA)?;
        migrate(&write)?;

        let mut readers = Vec::new();
        for _ in 0..3 {
            let r = Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            r.set_prepared_statement_cache_capacity(32);
            readers.push(Mutex::new(r));
        }
        Ok(Self {
            write: Mutex::new(write),
            readers,
            next_reader: std::sync::atomic::AtomicUsize::new(0),
            path: path.to_path_buf(),
        })
    }

    /// In-memory store for tests. One connection serves both roles.
    pub fn open_memory() -> Result<Self> {
        let write = Connection::open_in_memory()?;
        write.execute_batch(SCHEMA)?;
        migrate(&write)?;
        Ok(Self {
            write: Mutex::new(write),
            readers: Vec::new(),
            next_reader: std::sync::atomic::AtomicUsize::new(0),
            path: PathBuf::from(":memory:"),
        })
    }

    fn with_read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if self.readers.is_empty() {
            return f(&self.write.lock().unwrap());
        }
        let i = self
            .next_reader
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % self.readers.len();
        f(&self.readers[i].lock().unwrap())
    }

    /// Cached copy of a URL, if there is one. Staleness is the caller's to judge.
    pub fn get(&self, url: &str) -> Option<CachedPage> {
        let key = to_i64(crate::domain::stable_hash(&crate::domain::normalize_url(
            url,
        )?));
        let page = self
            .with_read(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT url, final_url, status, tier, etag, last_modified, content_type, title,
                            markdown, simhash, content_hash, fetched_at, expires_at, quality
                     FROM page WHERE url_hash = ?1",
                )?;
                let row = stmt
                    .query_row(params![key], |r| {
                        let blob: Option<Vec<u8>> = r.get(8)?;
                        Ok(CachedPage {
                            url: r.get(0)?,
                            final_url: r.get(1)?,
                            status: r.get::<_, i64>(2)? as u16,
                            tier: r.get(3)?,
                            etag: r.get(4)?,
                            last_modified: r.get(5)?,
                            content_type: r.get(6)?,
                            title: r.get(7)?,
                            markdown: blob.map(|b| inflate(&b)).unwrap_or_default(),
                            simhash: from_i64(r.get(9)?),
                            content_hash: from_i64(r.get(10)?),
                            fetched_at: r.get(11)?,
                            expires_at: r.get(12)?,
                            quality: r.get(13)?,
                        })
                    })
                    .optional()?;
                Ok(row)
            })
            .ok()
            .flatten()?;
        // Best effort: a failed hit counter must never fail a lookup.
        let _ = self.write.lock().unwrap().execute(
            "UPDATE page SET hits = hits + 1, last_hit_at = ?1 WHERE url_hash = ?2",
            params![now(), key],
        );
        Some(page)
    }

    /// Pages already on disk within `max_bits` of this fingerprint, nearest first.
    ///
    /// ▲ What this answers that nothing else could: a `web_fetch` in one session, and a
    /// `fetch_many` in another a week later, both delivering the same wire story under different
    /// hostnames. `provenance::group` sees only inside one batch, so until now the second one read
    /// as a fresh source. The whole point of a cache that outlives a session is that it can say
    /// "you already have this".
    ///
    /// Never a filter. The page is still returned in full — this is a label the caller may act on,
    /// and `duplicate_of` has always been reported rather than obeyed.
    ///
    /// Bounded on purpose. `max_bits` above `NEAR_DUPLICATE_BITS` is refused rather than answered
    /// approximately: four bands hold the pigeonhole guarantee only to three bits, and returning
    /// "no duplicates" from a lookup that silently stopped being exhaustive is the kind of wrong
    /// answer that reads like a right one.
    pub fn find_near(&self, sim: u64, max_bits: u32, limit: usize) -> Vec<NearDuplicate> {
        if max_bits > crate::quality::provenance::NEAR_DUPLICATE_BITS {
            return Vec::new();
        }
        let b = bands(sim);
        let mut out = self
            .with_read(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT url, simhash, fetched_at FROM page
                     WHERE band0 = ?1 OR band1 = ?2 OR band2 = ?3 OR band3 = ?4",
                )?;
                let rows = stmt
                    .query_map(params![b[0], b[1], b[2], b[3]], |r| {
                        Ok((r.get::<_, String>(0)?, from_i64(r.get(1)?), r.get(2)?))
                    })?
                    .filter_map(|r| r.ok())
                    .filter_map(|(url, other, fetched_at)| {
                        let distance = crate::dedup::hamming(sim, other);
                        (distance <= max_bits).then_some(NearDuplicate {
                            url,
                            distance,
                            fetched_at,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(rows)
            })
            .unwrap_or_default();
        // Nearest first, and the more recent of two equals: a caller shown one duplicate should be
        // shown the closest one, and among identical texts the copy it is likelier to remember.
        out.sort_by_key(|d| (d.distance, std::cmp::Reverse(d.fetched_at)));
        out.truncate(limit);
        out
    }

    /// The pages of one domain already on disk, most recent first.
    ///
    /// ▲ The read query the cross-page work needs and the cache never had. `page_domain` has
    /// existed since the first schema and until now only `clear()` used it. Note what comes back:
    /// **markdown, never HTML** — the cache stores the rendered page, so anything learned from
    /// these pages is learned at block level, which is exactly what `dedup::Boilerplate` consumes.
    pub fn pages_for_domain(&self, domain: &str, limit: usize) -> Vec<CachedPage> {
        self.with_read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT url, final_url, status, tier, etag, last_modified, content_type, title,
                        markdown, simhash, content_hash, fetched_at, expires_at, quality
                 FROM page WHERE domain = ?1 ORDER BY fetched_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![domain, limit as i64], |r| {
                    let blob: Option<Vec<u8>> = r.get(8)?;
                    Ok(CachedPage {
                        url: r.get(0)?,
                        final_url: r.get(1)?,
                        status: r.get::<_, i64>(2)? as u16,
                        tier: r.get(3)?,
                        etag: r.get(4)?,
                        last_modified: r.get(5)?,
                        content_type: r.get(6)?,
                        title: r.get(7)?,
                        markdown: blob.map(|b| inflate(&b)).unwrap_or_default(),
                        simhash: from_i64(r.get(9)?),
                        content_hash: from_i64(r.get(10)?),
                        fetched_at: r.get(11)?,
                        expires_at: r.get(12)?,
                        quality: r.get(13)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect::<Vec<_>>();
            Ok(rows)
        })
        .unwrap_or_default()
    }

    /// When this machine first fetched anything from a domain, in epoch seconds.
    ///
    /// An observation, not a score. "First seen an hour ago" is worth telling a caller who is
    /// weighing a source; turning it into a number that ranks sites would reproduce exactly the
    /// bias the W3C Credible Web group warned its own signals carry — towards whoever has been
    /// online longest. It is also bounded by the cache's own retention, which is why it is
    /// reported as *first seen here* and never as when the site was created.
    pub fn site_first_seen(&self, domain: &str) -> Option<i64> {
        self.with_read(|c| {
            let mut stmt =
                c.prepare_cached("SELECT MIN(fetched_at) FROM page WHERE domain = ?1")?;
            Ok(stmt
                .query_row(params![domain], |r| r.get::<_, Option<i64>>(0))
                .optional()?
                .flatten())
        })
        .ok()
        .flatten()
    }

    /// Store a page and record its fingerprint. Returns what changed against the previous copy.
    #[allow(clippy::too_many_arguments)]
    pub fn put(
        &self,
        url: &str,
        final_url: &str,
        status: u16,
        tier: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        content_type: &str,
        title: Option<&str>,
        markdown: &str,
        ttl_secs: i64,
        // How much of the page arrived, serialised at the fetch, so a cache hit answers the same
        // way rather than re-deriving a verdict from evidence it no longer has.
        quality: Option<&str>,
    ) -> Result<Change> {
        let Some(norm) = crate::domain::normalize_url(url) else {
            anyhow::bail!("not a storable URL: {url}");
        };
        let key = to_i64(crate::domain::stable_hash(&norm));
        let content_hash = crate::domain::stable_hash(markdown);
        let sim = crate::dedup::simhash(markdown);
        let ts = now();
        let blob = deflate(markdown);
        let domain = crate::domain::domain_from_url(final_url);

        let previous = self.with_read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT content_hash, simhash, fetched_at FROM page WHERE url_hash = ?1",
            )?;
            Ok(stmt
                .query_row(params![key], |r| {
                    Ok((
                        from_i64(r.get(0)?),
                        from_i64(r.get(1)?),
                        r.get::<_, i64>(2)?,
                    ))
                })
                .optional()?)
        })?;

        let band = bands(sim);
        let conn = self.write.lock().unwrap();
        conn.prepare_cached(
            "INSERT INTO page (url_hash, url, final_url, domain, status, tier, etag, last_modified,
                               content_type, title, markdown, simhash, content_hash, bytes,
                               fetched_at, expires_at, quality, hits, last_hit_at,
                               band0, band1, band2, band3)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,0,NULL,
                     ?18,?19,?20,?21)
             ON CONFLICT(url_hash) DO UPDATE SET
               final_url=excluded.final_url, status=excluded.status, tier=excluded.tier,
               etag=excluded.etag, last_modified=excluded.last_modified,
               content_type=excluded.content_type, title=excluded.title,
               markdown=excluded.markdown, simhash=excluded.simhash,
               content_hash=excluded.content_hash, bytes=excluded.bytes,
               fetched_at=excluded.fetched_at, expires_at=excluded.expires_at,
               quality=excluded.quality, band0=excluded.band0, band1=excluded.band1,
               band2=excluded.band2, band3=excluded.band3",
        )?
        .execute(params![
            key,
            url,
            final_url,
            domain,
            status as i64,
            tier,
            etag,
            last_modified,
            content_type,
            title,
            blob,
            to_i64(sim),
            to_i64(content_hash),
            blob_len(&blob),
            ts,
            ts + ttl_secs,
            quality,
            band[0],
            band[1],
            band[2],
            band[3]
        ])?;

        conn.prepare_cached(
            "INSERT OR REPLACE INTO page_version (url_hash, fetched_at, content_hash, simhash, title, bytes)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )?
        .execute(params![key, ts, to_i64(content_hash), to_i64(sim), title, markdown.len() as i64])?;

        Ok(match previous {
            None => Change::New,
            Some((prev_hash, _, _)) if prev_hash == content_hash => Change::Unchanged,
            Some((_, prev_sim, prev_at)) => Change::Changed {
                similarity: crate::dedup::similarity(prev_sim, sim),
                previous_fetched_at: prev_at,
            },
        })
    }

    /// Extend a cached entry's life after a `304 Not Modified`, without touching its content.
    pub fn touch(&self, url: &str, ttl_secs: i64) -> Result<()> {
        let Some(norm) = crate::domain::normalize_url(url) else {
            return Ok(());
        };
        let key = to_i64(crate::domain::stable_hash(&norm));
        self.write.lock().unwrap().execute(
            "UPDATE page SET expires_at = ?1 WHERE url_hash = ?2",
            params![now() + ttl_secs, key],
        )?;
        Ok(())
    }

    /// Fingerprint history for a URL, newest first.
    pub fn versions(&self, url: &str, limit: usize) -> Vec<(i64, u64, u64)> {
        let Some(norm) = crate::domain::normalize_url(url) else {
            return Vec::new();
        };
        let key = to_i64(crate::domain::stable_hash(&norm));
        self.with_read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT fetched_at, content_hash, simhash FROM page_version
                 WHERE url_hash = ?1 ORDER BY fetched_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![key, limit as i64], |r| {
                    Ok((r.get(0)?, from_i64(r.get(1)?), from_i64(r.get(2)?)))
                })?
                .flatten()
                .collect();
            Ok(rows)
        })
        .unwrap_or_default()
    }

    /// Start or update a crawl's record.
    pub fn save_crawl(
        &self,
        id: &str,
        start_url: &str,
        params_json: &str,
        status: &str,
        pages_done: usize,
        stopped_by: Option<&str>,
    ) -> Result<()> {
        let ts = now();
        self.write.lock().unwrap().prepare_cached(
            "INSERT INTO crawl (id, start_url, domain, params_json, status, pages_done, stopped_by, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)
             ON CONFLICT(id) DO UPDATE SET status=excluded.status, pages_done=excluded.pages_done,
                                           stopped_by=excluded.stopped_by, updated_at=excluded.updated_at",
        )?
        .execute(params![
            id, start_url, crate::domain::domain_from_url(start_url), params_json,
            status, pages_done as i64, stopped_by, ts
        ])?;
        Ok(())
    }

    /// Replace a crawl's pending frontier. Written per batch, not per URL: a kill loses at most
    /// one batch, and WAL already makes each write atomic.
    pub fn save_frontier(&self, crawl_id: &str, pending: &[(String, u16, f32)]) -> Result<()> {
        let mut conn = self.write.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM crawl_queue WHERE crawl_id = ?1 AND state = 'pending'",
            params![crawl_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO crawl_queue (crawl_id, url, depth, score, state)
                 VALUES (?1,?2,?3,?4,'pending')",
            )?;
            for (url, depth, score) in pending {
                stmt.execute(params![crawl_id, url, *depth as i64, *score])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Mark URLs as fetched so a resumed crawl does not repeat them.
    pub fn mark_done(&self, crawl_id: &str, urls: &[String]) -> Result<()> {
        let mut conn = self.write.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO crawl_queue (crawl_id, url, depth, score, state) VALUES (?1,?2,0,0,'done')
                 ON CONFLICT(crawl_id, url) DO UPDATE SET state='done'",
            )?;
            for u in urls {
                stmt.execute(params![crawl_id, u])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// A crawl's saved parameters and its pending frontier, highest score first.
    pub fn load_crawl(&self, crawl_id: &str) -> Option<SavedCrawl> {
        self.with_read(|c| {
            let row: Option<(String, String)> = c
                .prepare_cached("SELECT start_url, params_json FROM crawl WHERE id = ?1")?
                .query_row(params![crawl_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let Some((start_url, params_json)) = row else {
                return Ok(None);
            };
            let mut stmt = c.prepare_cached(
                "SELECT url, depth, score, state FROM crawl_queue WHERE crawl_id = ?1 ORDER BY score DESC",
            )?;
            let mut pending = Vec::new();
            let mut done = Vec::new();
            for row in stmt
                .query_map(params![crawl_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)? as u16,
                        r.get::<_, f64>(2)? as f32,
                        r.get::<_, String>(3)?,
                    ))
                })?
                .flatten()
            {
                if row.3 == "pending" {
                    pending.push((row.0, row.1, row.2));
                } else {
                    done.push(row.0);
                }
            }
            Ok(Some(SavedCrawl { start_url, params_json, pending, done }))
        })
        .ok()
        .flatten()
    }

    /// Crawls that still have work queued.
    pub fn resumable_crawls(&self) -> Vec<(String, String, usize)> {
        self.with_read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT c.id, c.start_url, COUNT(q.url)
                 FROM crawl c JOIN crawl_queue q ON q.crawl_id = c.id AND q.state = 'pending'
                 GROUP BY c.id ORDER BY c.updated_at DESC LIMIT 20",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as usize))
                })?
                .flatten()
                .collect();
            Ok(rows)
        })
        .unwrap_or_default()
    }

    // ---- long jobs ----------------------------------------------------------------------------

    /// Record a job before anything runs it.
    ///
    /// Called synchronously by whoever accepts the request, so a poll arriving on the heels of the
    /// job id always finds a row. A 202 whose id then 404s is the one race worth designing out.
    /// An upsert, because an id that is being *resumed* already has a row: a crawl that was
    /// interrupted, cancelled or stopped at a budget is asked to carry on under the same handle,
    /// and that is a job returning to the queue rather than a new one. The old outcome is cleared
    /// so a poller does not read last run's `stopped_by` as this run's.
    pub fn create_job(&self, id: &str, kind: &str, domain: &str, params_json: &str) -> Result<()> {
        self.write
            .lock()
            .unwrap()
            .prepare_cached(
                "INSERT INTO job (id, kind, state, domain, params_json, created_at)
                 VALUES (?1,?2,'queued',?3,?4,?5)
                 ON CONFLICT(id) DO UPDATE SET
                   state='queued', domain=excluded.domain, params_json=excluded.params_json,
                   owner=NULL, heartbeat_at=NULL, cancel_requested=0,
                   result=NULL, error=NULL, started_at=NULL, finished_at=NULL",
            )?
            .execute(params![id, kind, domain, params_json, now()])?;
        Ok(())
    }

    /// Claim a queued job for this run. `false` when somebody else already has it.
    ///
    /// A compare-and-swap rather than a read followed by a write: two processes can share this
    /// file — the crawl tests already run two servers over one — and both taking the same job means
    /// two crawls of one site at twice the request rate.
    pub fn start_job(&self, id: &str, owner: &str) -> Result<bool> {
        let ts = now();
        let changed = self
            .write
            .lock()
            .unwrap()
            .prepare_cached(
                "UPDATE job SET state='running', owner=?2, started_at=?3, heartbeat_at=?3
                 WHERE id=?1 AND state IN ('queued','interrupted')",
            )?
            .execute(params![id, owner, ts])?;
        Ok(changed > 0)
    }

    /// Still alive.
    ///
    /// Best effort, like `save_crawl`: losing a heartbeat is a worse outcome than a crawl that
    /// fails because its bookkeeping could not be written. Called per page, which is what makes it
    /// a liveness signal at all — `crawl.updated_at` is written per *batch*, and a level of two
    /// hundred pages can be half an hour between writes.
    pub fn beat(&self, id: &str) {
        if let Ok(conn) = self.write.lock() {
            if let Ok(mut stmt) = conn
                .prepare_cached("UPDATE job SET heartbeat_at=?2 WHERE id=?1 AND state='running'")
            {
                let _ = stmt.execute(params![id, now()]);
            }
        }
    }

    /// The end of a job, whatever kind of end it was.
    pub fn finish_job(
        &self,
        id: &str,
        state: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        // Deflated like a cached page, and for the same reason: a two-hundred-page crawl summary is
        // mostly repeated markup and it sits in this file until it expires.
        let blob = result.map(deflate);
        self.write
            .lock()
            .unwrap()
            .prepare_cached(
                "UPDATE job SET state=?2, result=?3, error=?4, finished_at=?5 WHERE id=?1",
            )?
            .execute(params![id, state, blob, error, now()])?;
        Ok(())
    }

    /// Ask a job to stop. Returns the state it was in, or `None` when there is no such job.
    ///
    /// A request, not an act: the job decides when it has actually stopped, and it stops at a page
    /// boundary so that its frontier is written and the id can be resumed.
    pub fn request_cancel(&self, id: &str) -> Option<String> {
        let conn = self.write.lock().ok()?;
        let state: String = conn
            .query_row("SELECT state FROM job WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()
            .ok()
            .flatten()?;
        let _ = conn.execute("UPDATE job SET cancel_requested=1 WHERE id=?1", params![id]);
        Some(state)
    }

    /// Has this job been asked to stop?
    pub fn cancel_requested(&self, id: &str) -> bool {
        self.with_read(|c| {
            Ok(c.query_row(
                "SELECT cancel_requested FROM job WHERE id = ?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
                != 0)
        })
        .unwrap_or(false)
    }

    /// One job, with the crawl's own count of what it fetched.
    ///
    /// `pages_done` and `pending` are read from the crawl rather than copied into the job row.
    /// Duplicating them would give two answers to one question, and the crawl's is the one a resume
    /// actually uses.
    pub fn job(&self, id: &str) -> Option<JobRow> {
        self.with_read(|c| Ok(read_job(c, id))).ok().flatten()
    }

    /// The stored summary, inflated. Separate from `job` so a listing cannot carry it by accident.
    pub fn job_result(&self, id: &str) -> Option<String> {
        self.with_read(|c| {
            let blob: Option<Vec<u8>> = c
                .query_row("SELECT result FROM job WHERE id = ?1", params![id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten();
            Ok(blob.map(|b| inflate(&b)))
        })
        .ok()
        .flatten()
    }

    /// Jobs, newest first, optionally of one state.
    pub fn jobs(&self, state: Option<&str>, limit: usize) -> Vec<JobRow> {
        self.with_read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT id FROM job WHERE (?1 IS NULL OR state = ?1)
                 ORDER BY created_at DESC LIMIT ?2",
            )?;
            let ids: Vec<String> = stmt
                .query_map(params![state, limit as i64], |r| r.get::<_, String>(0))?
                .flatten()
                .collect();
            Ok(ids.iter().filter_map(|id| read_job(c, id)).collect())
        })
        .unwrap_or_default()
    }

    /// Jobs left running by a process that is gone, moved to `interrupted`.
    ///
    /// `interrupted` is resumable and that is the point: the frontier survived, so nothing is lost.
    /// The row exists only so a person can tell a crawl that *died* from one that finished — which
    /// `crawl.status` cannot say, because it is set to `running` per batch and never cleared.
    ///
    /// Both halves of the test matter. A foreign owner alone proves nothing, because two svipall
    /// processes can share this file; a stale heartbeat alone proves nothing, because this run's
    /// own job is between beats.
    /// `silent_for_secs` of zero means "any other run's job, however recently it spoke", which is
    /// what a test wants and what a single-process installation can safely assume on startup.
    pub fn adopt_orphaned_jobs(&self, owner: &str, silent_for_secs: i64) -> usize {
        let cutoff = now() - silent_for_secs;
        self.write
            .lock()
            .ok()
            .and_then(|c| {
                c.prepare_cached(
                    "UPDATE job SET state='interrupted'
                     WHERE state='running' AND owner IS NOT ?1
                       AND (heartbeat_at IS NULL OR heartbeat_at <= ?2)",
                )
                .ok()?
                .execute(params![owner, cutoff])
                .ok()
            })
            .unwrap_or(0)
    }

    /// Forget jobs that ended long enough ago, and fail ones nobody ever picked up.
    ///
    /// `interrupted` is never expired early: it is the state a person most wants to act on. The
    /// `crawl` rows are untouched — those belong to the page cache's own retention.
    pub fn expire_jobs(&self, keep_secs: i64) -> usize {
        let Ok(conn) = self.write.lock() else {
            return 0;
        };
        let dropped = conn
            .execute(
                "DELETE FROM job
                 WHERE state IN ('finished','stopped','cancelled','failed')
                   AND finished_at IS NOT NULL AND finished_at < ?1",
                params![now() - keep_secs],
            )
            .unwrap_or(0);
        // Only reachable if a process died between recording the job and spawning it.
        let _ = conn.execute(
            "UPDATE job SET state='failed', error='no runner picked this up', finished_at=?1
             WHERE state='queued' AND created_at < ?2",
            params![now(), now() - 3600],
        );
        dropped
    }

    /// The oldest queued job whose start domain has no job running on it.
    ///
    /// The admission rule, expressed where the answer already is. Two crawls of one site would
    /// fight over one frontier and, more to the point, over one address's reputation with that
    /// host — which is the scarcest thing a local-only tool has. `crawl.domain` is already written
    /// by `save_crawl`, so this is one `NOT EXISTS` and no in-memory set to keep correct across
    /// restarts.
    pub fn next_queued_job(&self) -> Option<JobRow> {
        self.with_read(|c| {
            let id: Option<String> = c
                .query_row(
                    "SELECT j.id FROM job j
                     WHERE j.state = 'queued'
                       AND (j.domain = '' OR NOT EXISTS (
                             SELECT 1 FROM job r
                             WHERE r.state = 'running' AND r.domain = j.domain))
                     ORDER BY j.created_at LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(id.and_then(|id| read_job(c, &id)))
        })
        .ok()
        .flatten()
    }

    pub fn size_bytes(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }

    pub fn page_count(&self) -> usize {
        self.with_read(|c| {
            Ok(c.query_row("SELECT COUNT(*) FROM page", [], |r| r.get::<_, i64>(0))? as usize)
        })
        .unwrap_or(0)
    }

    /// Delete what is past its time, then trim by size. Nothing here runs on a fetch.
    /// Record one request, after the fact.
    ///
    /// Append-only and best-effort: a log that can fail a fetch is worse than no log. Everything
    /// here is already known to the caller, so writing it costs one insert and no extra work.
    /// When this URL was last fetched, if it ever was.
    ///
    /// Cheaper than `get`, which reads the whole stored page: an incremental crawl asks this about
    /// every URL in a sitemap and needs none of the content to decide.
    pub fn last_fetched(&self, url: &str) -> Option<i64> {
        let hash = crate::domain::normalize_url(url)
            .map(|n| crate::domain::stable_hash(&n))
            .unwrap_or_else(|| crate::domain::stable_hash(url));
        self.with_read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT fetched_at FROM page WHERE url_hash = ?1",
                    [to_i64(hash)],
                    |r| r.get(0),
                )
                .optional()?)
        })
        .ok()
        .flatten()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_request(
        &self,
        url: &str,
        tier: &str,
        status: u16,
        wall: Option<&str>,
        blocked: bool,
        ms: u64,
        exit: Option<&str>,
    ) {
        let domain = crate::domain::domain_from_url(url);
        let _ = self.write.lock().unwrap().execute(
            "INSERT INTO request_log (at, domain, url, tier, status, wall, blocked, ms, exit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                now(),
                domain,
                url,
                tier,
                status as i64,
                wall,
                blocked as i64,
                ms as i64,
                exit
            ],
        );
    }

    /// What happened recently, newest first.
    ///
    /// `domain` narrows it; `since_secs` bounds it in time. Both optional, because the two
    /// questions people actually ask are "what has this domain been doing" and "what just
    /// happened".
    pub fn recent_requests(
        &self,
        domain: Option<&str>,
        since_secs: i64,
        limit: usize,
    ) -> Vec<RequestLine> {
        let after = now() - since_secs.max(0);
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT at, domain, url, tier, status, wall, blocked, ms, exit FROM request_log
                 WHERE at >= ?1 AND (?2 IS NULL OR domain = ?2)
                 ORDER BY at DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![after, domain, limit as i64], |r| {
                Ok(RequestLine {
                    at: r.get(0)?,
                    domain: r.get(1)?,
                    url: r.get(2)?,
                    tier: r.get(3)?,
                    status: r.get::<_, i64>(4)? as u16,
                    wall: r.get(5)?,
                    blocked: r.get::<_, i64>(6)? != 0,
                    ms: r.get::<_, i64>(7)? as u64,
                    exit: r.get(8)?,
                })
            })?;
            Ok(rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    /// One line per domain: how many requests, how many blocked, how slow.
    ///
    /// The shape of the question that matters — a domain that is 40% blocked and slow is a domain
    /// whose learned tier is wrong, and nothing else in the tool notices that on its own.
    pub fn request_summary(&self, since_secs: i64) -> Vec<(String, i64, i64, i64)> {
        let after = now() - since_secs.max(0);
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT domain, COUNT(*), SUM(blocked), CAST(AVG(ms) AS INTEGER)
                 FROM request_log WHERE at >= ?1
                 GROUP BY domain ORDER BY COUNT(*) DESC",
            )?;
            let rows = stmt.query_map([after], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            Ok(rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    /// Forget log lines older than `keep_secs`. Returns how many went.
    pub fn trim_log(&self, keep_secs: i64) -> usize {
        let before = now() - keep_secs.max(0);
        self.write
            .lock()
            .unwrap()
            .execute("DELETE FROM request_log WHERE at < ?1", [before])
            .unwrap_or(0)
    }

    /// Leave a note for the next run.
    ///
    /// An agent crawling a site over three sessions has nowhere to keep "the last id I saw was
    /// 4820" except its own context, which does not survive the session. This does, and it costs
    /// one row.
    ///
    /// Values are strings because the caller already has JSON if it wants structure, and a store
    /// that parses what it is given is a store that can reject it.
    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.write.lock().unwrap().execute(
            "INSERT INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value, now()],
        )?;
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> Option<String> {
        self.with_read(|conn| {
            Ok(conn
                .query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
                .ok())
        })
        .ok()
        .flatten()
    }

    /// Everything under a prefix, oldest key first, so a listing is stable.
    pub fn kv_list(&self, prefix: &str) -> Vec<(String, String)> {
        // A range rather than `LIKE`: `%` and `_` are wildcards there, so a key holding either —
        // and a URL holding an underscore is an ordinary key to want — would match more than the
        // caller asked for. A range needs no escaping and uses the primary key index.
        let upper = format!("{prefix}\u{10ffff}");
        self.with_read(|conn| {
            let mut stmt = conn
                .prepare("SELECT key, value FROM kv WHERE key >= ?1 AND key < ?2 ORDER BY key")?;
            let rows = stmt.query_map([prefix, upper.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    /// Forget one note. Returns whether there was one.
    pub fn kv_delete(&self, key: &str) -> bool {
        self.write
            .lock()
            .unwrap()
            .execute("DELETE FROM kv WHERE key = ?1", [key])
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    pub fn housekeep(&self, policy: &RetentionPolicy) -> HousekeepReport {
        let mut report = HousekeepReport::default();
        let conn = self.write.lock().unwrap();
        let cutoff = now() - policy.page_ttl_secs;

        report.pages_expired = conn
            .execute("DELETE FROM page WHERE fetched_at < ?1", params![cutoff])
            .unwrap_or(0);

        report.versions_trimmed = conn
            .execute(
                "DELETE FROM page_version WHERE fetched_at < ?1",
                params![now() - policy.version_ttl_secs],
            )
            .unwrap_or(0);
        // Keep only the newest N fingerprints per URL.
        report.versions_trimmed += conn
            .execute(
                "DELETE FROM page_version WHERE (url_hash, fetched_at) IN (
                     SELECT url_hash, fetched_at FROM (
                       SELECT url_hash, fetched_at,
                              ROW_NUMBER() OVER (PARTITION BY url_hash ORDER BY fetched_at DESC) AS rn
                       FROM page_version
                     ) WHERE rn > ?1)",
                params![policy.versions_per_url as i64],
            )
            .unwrap_or(0);

        // Size trim: least useful first, which is least read, longest ago, biggest.
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size > policy.max_db_bytes {
            let over = size.saturating_sub(policy.target_db_bytes);
            report.pages_evicted = conn
                .execute(
                    "DELETE FROM page WHERE url_hash IN (
                         SELECT url_hash FROM page
                         ORDER BY hits ASC, COALESCE(last_hit_at, fetched_at) ASC, bytes DESC
                         LIMIT (SELECT MAX(1, ?1 / MAX(1, AVG(bytes))) FROM page))",
                    params![over as i64],
                )
                .unwrap_or(0);
        }
        // Incremental, so there is never an exclusive lock over the whole file.
        let _ = conn.execute_batch("PRAGMA incremental_vacuum;");
        let _ = conn.execute_batch("PRAGMA optimize;");
        report.bytes_after = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        report
    }

    pub fn clear(&self, domain: Option<&str>) -> usize {
        let conn = self.write.lock().unwrap();
        let n = match domain {
            Some(d) => conn
                .execute("DELETE FROM page WHERE domain = ?1", params![d])
                .unwrap_or(0),
            None => conn.execute("DELETE FROM page", []).unwrap_or(0),
        };
        let _ = conn.execute_batch("PRAGMA incremental_vacuum;");
        n
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Change {
    New,
    Unchanged,
    Changed {
        similarity: f32,
        previous_fetched_at: i64,
    },
}

fn blob_len(b: &[u8]) -> i64 {
    b.len() as i64
}

/// Markdown compresses about four to one, and the cache is otherwise the fastest-growing thing
/// in `~/.svipall`.
fn deflate(s: &str) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    if e.write_all(s.as_bytes()).is_err() {
        return Vec::new();
    }
    e.finish().unwrap_or_default()
}

fn inflate(b: &[u8]) -> String {
    let mut out = String::new();
    if flate2::read::DeflateDecoder::new(b)
        .read_to_string(&mut out)
        .is_err()
    {
        return String::new();
    }
    out
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    // Version 1 is the initial schema, already applied by SCHEMA. Each later version applies its
    // own step, guarded by the version it upgrades from, because `CREATE TABLE IF NOT EXISTS` on
    // an existing database adds nothing: the new work would run perfectly in development, where the
    // file is always fresh, and do nothing on any machine that had run the tool before.
    if current < 2 {
        conn.execute_batch(MIGRATE_1_TO_2)?;
    }
    if current < 3 {
        conn.execute_batch(MIGRATE_2_TO_3)?;
    }
    if current < 4 {
        conn.execute_batch(MIGRATE_3_TO_4)?;
    }
    if current < 5 {
        conn.execute_batch(MIGRATE_4_TO_5)?;
    }
    if current < 6 {
        conn.execute_batch(MIGRATE_5_TO_6)?;
    }
    if current < 7 {
        conn.execute_batch(MIGRATE_6_TO_7)?;
    }
    conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    Ok(())
}

/// Cache lifetime for a response, honouring `Cache-Control` where it is stricter than ours.
///
/// A server asking for a year is ignored: for an agent, a page that is a little stale but correct
/// beats one pinned indefinitely, and nothing here is a CDN.
pub fn ttl_for(cache_control: Option<&str>, has_query: bool, default_secs: i64) -> Option<i64> {
    if let Some(cc) = cache_control {
        let lower = cc.to_ascii_lowercase();
        if lower.contains("no-store") {
            return None;
        }
        if let Some(idx) = lower.find("max-age=") {
            if let Ok(v) = lower[idx + 8..]
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("")
                .parse::<i64>()
            {
                return Some(v.min(7 * 86_400).min(default_secs.max(v.min(default_secs))));
            }
        }
    }
    // A URL with a query string is far more likely to be a search or a feed than a document.
    Some(if has_query { 300 } else { default_secs })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ▲ The gap this closes: `provenance::group` compares hashes inside one batch, so the same
    /// wire story fetched a week apart read as two independent sources. The cache is the only
    /// thing that outlives the batch, and until now it was never asked.
    #[test]
    fn a_page_fetched_last_week_is_recognised_when_it_comes_back_under_another_name() {
        let s = store();
        // A document, not a sentence. Manku's three bits is calibrated for pages of prose: over a
        // single sentence one changed word moves fourteen of the sixty-four, and calling that a
        // duplicate would be a wrong answer rather than a missing feature.
        let article: String = (0..40)
            .map(|i| {
                format!(
                    "The council approved measure {i} on Tuesday after a long debate about \
                     parking, drainage and the future of the old library building. "
                )
            })
            .collect();
        s.put(
            "https://first.test/story",
            "https://first.test/story",
            200,
            "http",
            None,
            None,
            "text/html",
            None,
            &article,
            3600,
            None,
        )
        .expect("stored");

        // The same story carrying a wire credit: a syndicated copy, not a new source.
        let syndicated = format!("{article}Distributed by the regional news wire.");
        let hits = s.find_near(
            crate::dedup::simhash(&syndicated),
            crate::quality::provenance::NEAR_DUPLICATE_BITS,
            5,
        );
        assert_eq!(
            hits.first().map(|h| h.url.as_str()),
            Some("https://first.test/story"),
            "the copy already on disk was not recognised: {hits:?}"
        );
    }

    #[test]
    fn a_page_about_something_else_is_not_a_duplicate_of_it() {
        let s = store();
        s.put(
            "https://a.test/weather",
            "https://a.test/weather",
            200,
            "http",
            None,
            None,
            "text/html",
            None,
            "Rain is forecast across the north west for most of the weekend, with winds easing \
             by Sunday evening and temperatures close to the seasonal average.",
            3600,
            None,
        )
        .expect("stored");
        let other = crate::dedup::simhash(
            "Quarterly revenue rose eleven per cent on stronger subscription growth, and the \
             board reiterated its guidance for the full year.",
        );
        assert!(s.find_near(other, 3, 5).is_empty());
    }

    /// The pigeonhole guarantee holds to three bits and no further. Answering a wider question
    /// with a narrower index would return "nothing like it" from a search that had quietly stopped
    /// looking everywhere.
    #[test]
    fn a_search_wider_than_the_index_can_answer_is_refused_rather_than_answered_partly() {
        let s = store();
        let text = "some text that is long enough to have a fingerprint worth comparing at all";
        s.put(
            "https://a.test/1",
            "https://a.test/1",
            200,
            "http",
            None,
            None,
            "text/html",
            None,
            text,
            3600,
            None,
        )
        .expect("stored");
        let sim = crate::dedup::simhash(text);
        assert_eq!(s.find_near(sim, 0, 5).len(), 1, "its own text, exactly");
        assert!(
            s.find_near(sim, crate::quality::provenance::NEAR_DUPLICATE_BITS + 1, 5)
                .is_empty(),
            "a lookup past the guarantee must refuse, not guess"
        );
    }

    /// A database written by the previous build already holds fingerprints. The upgrade has to
    /// make them findable without a re-fetch, or the feature would only work on pages stored after
    /// the day it shipped.
    #[test]
    fn pages_stored_before_the_bands_existed_are_findable_after_the_upgrade() {
        let dir = tempdir();
        let path = dir.join("v6.db");
        let text = "A long enough paragraph of ordinary prose that its fingerprint is stable and \
                    worth looking up again later on.";
        let sim = crate::dedup::simhash(text);
        {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            // v6's page table: everything the current one has except the four bands.
            conn.execute_batch(
                "CREATE TABLE page (
                   url_hash INTEGER PRIMARY KEY, url TEXT NOT NULL, final_url TEXT NOT NULL,
                   domain TEXT NOT NULL, status INTEGER NOT NULL, tier TEXT NOT NULL,
                   etag TEXT, last_modified TEXT, content_type TEXT NOT NULL DEFAULT '',
                   title TEXT, markdown BLOB, simhash INTEGER NOT NULL DEFAULT 0,
                   content_hash INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL DEFAULT 0,
                   fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
                   hits INTEGER NOT NULL DEFAULT 0, last_hit_at INTEGER, quality TEXT);",
            )
            .expect("v6 page table");
            conn.execute(
                "INSERT INTO page (url_hash, url, final_url, domain, status, tier, content_type,
                                   markdown, simhash, bytes, fetched_at, expires_at)
                 VALUES (?1,?2,?2,'old.test',200,'http','text/html',?3,?4,0,?5,?6)",
                params![
                    to_i64(crate::domain::stable_hash("https://old.test/a")),
                    "https://old.test/a",
                    deflate(text),
                    to_i64(sim),
                    now(),
                    now() + 3600
                ],
            )
            .expect("an old row");
            conn.execute_batch("PRAGMA user_version = 6;")
                .expect("stamp");
        }
        let s = Store::open_at(&path).expect("open a v6 database");
        assert_eq!(
            s.find_near(sim, 3, 5).first().map(|h| h.url.as_str()),
            Some("https://old.test/a"),
            "a fingerprint already on disk was not backfilled into the bands"
        );
    }

    #[test]
    fn a_domains_pages_come_back_newest_first_and_only_that_domains() {
        let s = store();
        for (i, url) in [
            "https://site.test/a",
            "https://site.test/b",
            "https://other.test/c",
        ]
        .iter()
        .enumerate()
        {
            s.put(
                url,
                url,
                200,
                "http",
                None,
                None,
                "text/html",
                None,
                &format!("page number {i} with enough words to be worth storing at all"),
                3600,
                None,
            )
            .expect("stored");
        }
        let pages = s.pages_for_domain("site.test", 10);
        assert_eq!(pages.len(), 2, "only this domain's pages: {pages:?}");
        assert!(pages.iter().all(|p| p.markdown.contains("page number")));
        assert!(
            pages[0].fetched_at >= pages[1].fetched_at,
            "newest first, so a template is learned from what the site looks like now"
        );
        assert!(s.pages_for_domain("never.test", 10).is_empty());
    }

    #[test]
    fn first_seen_is_the_oldest_page_of_the_site_and_nothing_for_a_site_never_visited() {
        let s = store();
        s.put(
            "https://seen.test/a",
            "https://seen.test/a",
            200,
            "http",
            None,
            None,
            "text/html",
            None,
            "the first page anyone here ever fetched from this site",
            3600,
            None,
        )
        .expect("stored");
        let first = s.site_first_seen("seen.test").expect("a first sighting");
        assert!((now() - first).abs() < 60);
        assert_eq!(s.site_first_seen("unseen.test"), None);
    }

    fn store() -> Store {
        Store::open_memory().expect("in-memory store")
    }

    /// A directory of this test's own, removed when the process is done with it.
    ///
    /// A counter rather than a timestamp: two tests starting together on Windows get the same
    /// nanosecond, and then they share a database and deadlock.
    fn tempdir() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("svipall-cache-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_tool_can_answer_questions_about_its_own_behaviour() {
        // "Why is this domain slow" and "which tier is carrying this crawl" were questions the
        // ladder could not answer: it decided, wrote a line to stderr, and forgot.
        let s = store();
        s.log_request("https://a.test/1", "http", 200, None, false, 120, None);
        s.log_request(
            "https://a.test/2",
            "warm",
            403,
            Some("cloudflare"),
            true,
            8_400,
            None,
        );
        s.log_request("https://b.test/x", "http", 200, None, false, 90, None);

        let recent = s.recent_requests(None, 3600, 10);
        assert_eq!(recent.len(), 3);
        let a = s.recent_requests(Some("a.test"), 3600, 10);
        assert_eq!(a.len(), 2, "{a:?}");
        assert!(a
            .iter()
            .any(|l| l.blocked && l.wall.as_deref() == Some("cloudflare")));
    }

    #[test]
    fn the_summary_names_the_domain_whose_learned_tier_is_wrong() {
        // A domain that is half blocked and slow is a domain the ladder has learned the wrong tier
        // for, and nothing else in the tool notices that on its own.
        let s = store();
        for _ in 0..4 {
            s.log_request(
                "https://bad.test/x",
                "warm",
                403,
                Some("datadome"),
                true,
                20_000,
                None,
            );
        }
        s.log_request("https://good.test/x", "http", 200, None, false, 100, None);
        let summary = s.request_summary(3600);
        let bad = summary
            .iter()
            .find(|(d, ..)| d == "bad.test")
            .expect("listed");
        assert_eq!(bad.1, 4, "four requests");
        assert_eq!(bad.2, 4, "all of them blocked");
        assert!(bad.3 > 10_000, "and slow: {}ms", bad.3);
    }

    #[test]
    fn the_log_is_bounded_by_time_rather_than_kept_forever() {
        let s = store();
        s.log_request("https://a.test/1", "http", 200, None, false, 10, None);
        assert_eq!(s.recent_requests(None, 3600, 10).len(), 1);
        // A line written this second is not yet older than anything, so trimming leaves it. That
        // is the boundary that matters: a retention pass must never eat what just happened.
        assert_eq!(s.trim_log(0), 0);
        assert_eq!(s.recent_requests(None, 3600, 10).len(), 1);
        // Trimming with a negative horizon is the only way to mean "everything", and `max(0)`
        // refuses it, so the log can never be emptied by a bad argument.
        assert_eq!(s.trim_log(-99), 0);
        assert_eq!(s.recent_requests(None, 3600, 10).len(), 1);
    }

    #[test]
    fn asking_about_a_domain_that_did_nothing_is_an_empty_answer() {
        let s = store();
        s.log_request("https://a.test/1", "http", 200, None, false, 10, None);
        assert!(s
            .recent_requests(Some("elsewhere.test"), 3600, 10)
            .is_empty());
        // "The last zero seconds" includes this second: the line was written in it.
        assert_eq!(s.recent_requests(None, 0, 10).len(), 1);
    }

    #[test]
    fn a_database_from_before_the_log_gains_it_rather_than_breaking() {
        let dir = tempdir();
        let path = dir.join("v2.db");
        {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            conn.execute_batch(SCHEMA).expect("schema");
            conn.execute_batch(MIGRATE_1_TO_2).expect("v2");
            conn.execute_batch("PRAGMA user_version = 2;")
                .expect("stamp");
        }
        let s = Store::open_at(&path).expect("open a v2 database");
        s.log_request("https://a.test/1", "http", 200, None, false, 10, None);
        assert_eq!(s.recent_requests(None, 3600, 10).len(), 1);
    }

    #[test]
    fn a_database_from_before_the_verdict_gains_the_column_and_reads_null_for_its_old_rows() {
        // Every machine that has run the tool has a v4 file with pages already in it. The upgrade
        // has to add the column without touching those rows, and an old row has to come back as
        // "nobody looked" rather than as a verdict nobody made.
        let dir = tempdir();
        let path = dir.join("v4.db");
        {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            conn.execute_batch(SCHEMA).expect("schema");
            conn.execute_batch(MIGRATE_1_TO_2).expect("v2");
            conn.execute_batch(MIGRATE_2_TO_3).expect("v3");
            conn.execute_batch(MIGRATE_3_TO_4).expect("v4");
            conn.execute_batch("PRAGMA user_version = 4;")
                .expect("stamp");
        }
        // A page stored by the old build, through the old schema.
        {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            conn.execute(
                "INSERT INTO page (url_hash, url, final_url, domain, status, tier, content_type,
                                   markdown, bytes, fetched_at, expires_at)
                 VALUES (?1,?2,?2,'x.test',200,'http','text/html',?3,0,?4,?5)",
                params![
                    to_i64(crate::domain::stable_hash(
                        &crate::domain::normalize_url("https://x.test/old").unwrap()
                    )),
                    "https://x.test/old",
                    deflate("# Old"),
                    now(),
                    now() + 3600
                ],
            )
            .expect("old row");
        }

        let s = Store::open_at(&path).expect("open a v4 database");
        let old = s.get("https://x.test/old").expect("the old page survived");
        assert_eq!(old.markdown, "# Old");
        assert_eq!(old.quality, None, "an old row had no verdict to keep");

        // And a page stored by the new build round-trips its verdict.
        s.put(
            "https://x.test/new",
            "https://x.test/new",
            200,
            "http",
            None,
            None,
            "text/html",
            None,
            "# New",
            3600,
            Some("{\"verdict\":\"thin\",\"reasons\":[\"thin_text\"]}"),
        )
        .expect("stored");
        let fresh = s.get("https://x.test/new").expect("hit");
        assert_eq!(
            fresh.quality.as_deref(),
            Some("{\"verdict\":\"thin\",\"reasons\":[\"thin_text\"]}")
        );
    }

    #[test]
    fn a_note_left_by_one_run_is_there_for_the_next_one() {
        // An agent crawling a site over three sessions has nowhere else to keep "the last id I saw
        // was 4820": its own context does not survive the session, and this does.
        let dir = tempdir();
        let path = dir.join("kv.db");
        {
            let s = Store::open_at(&path).expect("open");
            s.kv_set("shop/last_id", "4820").expect("set");
        }
        let s = Store::open_at(&path).expect("reopen");
        assert_eq!(s.kv_get("shop/last_id").as_deref(), Some("4820"));
    }

    #[test]
    fn writing_a_key_twice_replaces_it_rather_than_failing() {
        let dir = tempdir();
        let s = Store::open_at(&dir.join("kv.db")).expect("open");
        s.kv_set("k", "one").expect("set");
        s.kv_set("k", "two").expect("set again");
        assert_eq!(s.kv_get("k").as_deref(), Some("two"));
    }

    #[test]
    fn a_key_nobody_wrote_is_absent_rather_than_empty() {
        // The difference matters: "" is a value somebody stored, `None` is a question never asked.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("kv.db")).expect("open");
        assert_eq!(s.kv_get("never"), None);
        s.kv_set("empty", "").expect("set");
        assert_eq!(s.kv_get("empty").as_deref(), Some(""));
    }

    #[test]
    fn listing_by_prefix_returns_only_that_prefix() {
        let dir = tempdir();
        let s = Store::open_at(&dir.join("kv.db")).expect("open");
        for k in ["shop/a", "shop/b", "other/c"] {
            s.kv_set(k, "v").expect("set");
        }
        let listed: Vec<String> = s.kv_list("shop/").into_iter().map(|(k, _)| k).collect();
        assert_eq!(listed, vec!["shop/a".to_string(), "shop/b".to_string()]);
    }

    #[test]
    fn a_key_containing_a_wildcard_character_is_not_a_wildcard() {
        // `%` and `_` are wildcards to LIKE, and a URL with an underscore in it is an ordinary key
        // to want. Matching more than was asked for is a silent wrong answer.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("kv.db")).expect("open");
        s.kv_set("a_b/one", "1").expect("set");
        s.kv_set("axb/two", "2").expect("set");
        let listed: Vec<String> = s.kv_list("a_b/").into_iter().map(|(k, _)| k).collect();
        assert_eq!(listed, vec!["a_b/one".to_string()]);
    }

    #[test]
    fn forgetting_a_note_says_whether_there_was_one() {
        let dir = tempdir();
        let s = Store::open_at(&dir.join("kv.db")).expect("open");
        s.kv_set("k", "v").expect("set");
        assert!(s.kv_delete("k"));
        assert!(!s.kv_delete("k"), "deleting twice is not a second deletion");
        assert_eq!(s.kv_get("k"), None);
    }

    #[test]
    fn a_database_written_before_the_notes_existed_gains_them_rather_than_breaking() {
        // `CREATE TABLE IF NOT EXISTS` adds nothing to a database that already exists, so without a
        // real migration this works in development and fails on every machine that ran the tool
        // before.
        let dir = tempdir();
        let path = dir.join("old.db");
        {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            conn.execute_batch(SCHEMA).expect("v1 schema");
            conn.execute_batch("PRAGMA user_version = 1;").expect("v1");
        }
        let s = Store::open_at(&path).expect("open an old database");
        s.kv_set("k", "v").expect("the notes table exists now");
        assert_eq!(s.kv_get("k").as_deref(), Some("v"));
    }

    #[test]
    fn a_stored_page_comes_back_intact() {
        let s = store();
        let md = "# Title\n\nSome content that should survive compression.";
        assert_eq!(
            s.put(
                "https://x.test/a",
                "https://x.test/a",
                200,
                "http",
                Some("W/\"1\""),
                None,
                "text/html",
                Some("Title"),
                md,
                3600,
                None
            )
            .unwrap(),
            Change::New
        );
        let hit = s.get("https://x.test/a").expect("cache hit");
        assert_eq!(hit.markdown, md);
        assert_eq!(hit.etag.as_deref(), Some("W/\"1\""));
        assert_eq!(hit.title.as_deref(), Some("Title"));
        assert!(hit.is_fresh());
    }

    /// The tracking parameters that make the same page look like a hundred different ones.
    #[test]
    fn tracking_parameters_do_not_create_separate_entries() {
        let s = store();
        s.put(
            "https://x.test/a",
            "https://x.test/a",
            200,
            "http",
            None,
            None,
            "",
            None,
            "body",
            3600,
            None,
        )
        .unwrap();
        assert!(
            s.get("https://x.test/a?utm_source=newsletter").is_some(),
            "a tracking parameter should not miss the cache"
        );
        assert_eq!(s.page_count(), 1);
    }

    #[test]
    fn storing_the_same_content_twice_reports_unchanged() {
        let s = store();
        let md = "identical content both times around";
        s.put(
            "https://x.test/b",
            "https://x.test/b",
            200,
            "http",
            None,
            None,
            "",
            None,
            md,
            60,
            None,
        )
        .unwrap();
        assert_eq!(
            s.put(
                "https://x.test/b",
                "https://x.test/b",
                200,
                "http",
                None,
                None,
                "",
                None,
                md,
                60,
                None
            )
            .unwrap(),
            Change::Unchanged
        );
    }

    #[test]
    fn a_rewrite_reports_the_change_and_how_similar_it_is() {
        let s = store();
        let base =
            "The article body, which is long enough for a similarity score to mean something. "
                .repeat(6);
        s.put(
            "https://x.test/c",
            "https://x.test/c",
            200,
            "http",
            None,
            None,
            "",
            None,
            &base,
            60,
            None,
        )
        .unwrap();
        let edited = format!("{base}One more sentence at the end.");
        match s
            .put(
                "https://x.test/c",
                "https://x.test/c",
                200,
                "http",
                None,
                None,
                "",
                None,
                &edited,
                60,
                None,
            )
            .unwrap()
        {
            Change::Changed {
                similarity,
                previous_fetched_at,
            } => {
                assert!(
                    similarity > 0.9,
                    "a small edit reported similarity {similarity}"
                );
                assert!(previous_fetched_at > 0);
            }
            other => panic!("expected a change, got {other:?}"),
        }
    }

    #[test]
    fn version_history_accumulates_newest_first() {
        let s = store();
        for i in 0..3 {
            s.put(
                "https://x.test/d",
                "https://x.test/d",
                200,
                "http",
                None,
                None,
                "",
                None,
                &format!("revision {i}"),
                60,
                None,
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let v = s.versions("https://x.test/d", 10);
        assert!(v.len() >= 2, "expected several versions, got {}", v.len());
        assert!(v[0].0 >= v[1].0, "versions must come back newest first");
    }

    #[test]
    fn a_miss_is_a_miss() {
        assert!(store().get("https://x.test/never-fetched").is_none());
    }

    #[test]
    fn touch_extends_life_without_changing_content() {
        let s = store();
        s.put(
            "https://x.test/e",
            "https://x.test/e",
            200,
            "http",
            None,
            None,
            "",
            None,
            "body",
            -10,
            None,
        )
        .unwrap();
        assert!(
            !s.get("https://x.test/e").unwrap().is_fresh(),
            "should start stale"
        );
        s.touch("https://x.test/e", 3600).unwrap();
        let after = s.get("https://x.test/e").unwrap();
        assert!(after.is_fresh(), "304 revalidation should have extended it");
        assert_eq!(after.markdown, "body");
    }

    #[test]
    fn housekeeping_removes_expired_pages_and_keeps_current_ones() {
        let s = store();
        s.put(
            "https://x.test/old",
            "https://x.test/old",
            200,
            "http",
            None,
            None,
            "",
            None,
            "old",
            60,
            None,
        )
        .unwrap();
        s.put(
            "https://x.test/new",
            "https://x.test/new",
            200,
            "http",
            None,
            None,
            "",
            None,
            "new",
            60,
            None,
        )
        .unwrap();
        // Age the first entry past any plausible TTL.
        {
            let c = s.write.lock().unwrap();
            c.execute("UPDATE page SET fetched_at = 0 WHERE url LIKE '%old'", [])
                .unwrap();
        }
        let report = s.housekeep(&RetentionPolicy::default());
        assert_eq!(report.pages_expired, 1);
        assert!(s.get("https://x.test/old").is_none());
        assert!(
            s.get("https://x.test/new").is_some(),
            "a current page must survive"
        );
    }

    #[test]
    fn version_history_is_capped_per_url() {
        let s = store();
        let policy = RetentionPolicy {
            versions_per_url: 3,
            ..Default::default()
        };
        // Recent timestamps: dated ones would be removed by the age rule before the count rule
        // ever applies, which is correct behaviour but not what this test is about.
        let base = now();
        {
            let c = s.write.lock().unwrap();
            for i in 0..10i64 {
                c.execute(
                    "INSERT INTO page_version (url_hash, fetched_at, content_hash, simhash, title, bytes)
                     VALUES (1, ?1, ?2, 0, NULL, 0)",
                    params![base - i, i],
                ).unwrap();
            }
        }
        s.housekeep(&policy);
        let remaining: i64 = s
            .with_read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM page_version WHERE url_hash = 1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(remaining, 3, "only the newest versions should remain");
    }

    #[test]
    fn clearing_by_domain_leaves_other_domains_alone() {
        let s = store();
        s.put(
            "https://a.test/1",
            "https://a.test/1",
            200,
            "http",
            None,
            None,
            "",
            None,
            "a",
            60,
            None,
        )
        .unwrap();
        s.put(
            "https://b.test/1",
            "https://b.test/1",
            200,
            "http",
            None,
            None,
            "",
            None,
            "b",
            60,
            None,
        )
        .unwrap();
        assert_eq!(s.clear(Some("a.test")), 1);
        assert!(s.get("https://a.test/1").is_none());
        assert!(s.get("https://b.test/1").is_some());
    }

    /// The failure this exists to prevent: an interrupted crawl losing everything it had queued.
    #[test]
    fn an_interrupted_crawl_resumes_from_its_frontier() {
        let s = store();
        s.save_crawl("c1", "https://x.test/", "{}", "running", 3, None)
            .unwrap();
        s.mark_done(
            "c1",
            &["https://x.test/a".into(), "https://x.test/b".into()],
        )
        .unwrap();
        s.save_frontier(
            "c1",
            &[
                ("https://x.test/c".into(), 1, 0.9),
                ("https://x.test/d".into(), 2, 0.4),
            ],
        )
        .unwrap();

        let c = s.load_crawl("c1").expect("the crawl should reload");
        assert_eq!(c.start_url, "https://x.test/");
        assert_eq!(c.done.len(), 2, "fetched pages must not be refetched");
        assert_eq!(c.pending.len(), 2);
        assert_eq!(c.pending[0].0, "https://x.test/c", "highest score first");
    }

    #[test]
    fn saving_the_frontier_again_replaces_it_rather_than_appending() {
        let s = store();
        s.save_crawl("c2", "https://x.test/", "{}", "running", 0, None)
            .unwrap();
        s.save_frontier("c2", &[("https://x.test/1".into(), 1, 0.5)])
            .unwrap();
        s.save_frontier("c2", &[("https://x.test/2".into(), 1, 0.5)])
            .unwrap();
        let c = s.load_crawl("c2").unwrap();
        assert_eq!(c.pending.len(), 1);
        assert_eq!(c.pending[0].0, "https://x.test/2");
    }

    #[test]
    fn only_crawls_with_work_left_are_listed_as_resumable() {
        let s = store();
        s.save_crawl("busy", "https://x.test/", "{}", "running", 1, None)
            .unwrap();
        s.save_frontier("busy", &[("https://x.test/next".into(), 1, 1.0)])
            .unwrap();
        s.save_crawl(
            "done",
            "https://y.test/",
            "{}",
            "finished",
            5,
            Some("max_pages"),
        )
        .unwrap();

        let list = s.resumable_crawls();
        assert_eq!(
            list.len(),
            1,
            "a finished crawl has nothing to resume: {list:?}"
        );
        assert_eq!(list[0].0, "busy");
        assert_eq!(list[0].2, 1);
    }

    #[test]
    fn an_unknown_crawl_id_is_none() {
        assert!(store().load_crawl("nope").is_none());
    }

    #[test]
    fn cache_modes_parse_and_describe_their_permissions() {
        assert_eq!(CacheMode::parse("auto"), Some(CacheMode::ReadWrite));
        assert_eq!(CacheMode::parse("BYPASS"), Some(CacheMode::Bypass));
        assert_eq!(CacheMode::parse("nonsense"), None);
        assert!(CacheMode::ReadWrite.may_read() && CacheMode::ReadWrite.may_write());
        assert!(!CacheMode::Read.may_write());
        assert!(!CacheMode::Write.may_read());
        assert!(!CacheMode::Bypass.may_read() && !CacheMode::Bypass.may_write());
    }

    #[test]
    fn no_store_is_honoured_and_an_absurd_max_age_is_capped() {
        assert_eq!(ttl_for(Some("no-store"), false, 3600), None);
        assert_eq!(ttl_for(Some("max-age=60"), false, 3600), Some(60));
        assert_eq!(
            ttl_for(Some("max-age=31536000"), false, 3600),
            Some(3600),
            "a year-long max-age must not pin a page indefinitely"
        );
        assert_eq!(
            ttl_for(None, true, 3600),
            Some(300),
            "query URLs are short-lived"
        );
        assert_eq!(ttl_for(None, false, 3600), Some(3600));
    }

    #[test]
    fn compression_round_trips_including_empty_and_unicode() {
        for s in ["", "plain", "unicode ✓ ñ 中文", &"long ".repeat(5000)] {
            assert_eq!(inflate(&deflate(s)), s);
        }
    }

    // ---- long jobs ------------------------------------------------------------------------

    /// A job for a crawl that has already recorded progress, so the join has something to find.
    /// `save_crawl` is what a running crawl calls; this is the same call, once.
    fn job_with_crawl(s: &Store, id: &str, url: &str, pages_done: usize, status: &str) {
        s.create_job(id, "crawl", &crate::domain::domain_from_url(url), "{}")
            .expect("create");
        s.save_crawl(id, url, "{}", status, pages_done, None)
            .expect("crawl");
    }

    #[test]
    fn a_job_is_findable_the_moment_it_is_created_rather_than_when_it_starts() {
        // The race this designs out: a request is accepted and answers with an id, the client polls
        // it straight away, and the runner has not picked it up yet. A 404 there says "no such job"
        // about a job the caller was just handed.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        s.create_job("j1", "crawl", "x.test", r#"{"url":"https://x.test/"}"#)
            .expect("create");
        let row = s.job("j1").expect("the job must exist before it runs");
        assert_eq!(row.state, "queued");
        assert_eq!(row.pages_done, 0);
        assert!(!row.cancel_requested);
        assert!(row.started_at.is_none());
    }

    #[test]
    fn a_database_from_before_the_job_table_gains_it_rather_than_breaking() {
        // The trap CLAUDE.md names, and the only test that catches it: every statement is
        // `IF NOT EXISTS`, so forgetting the SCHEMA_VERSION bump works perfectly in development,
        // where the file is always new, and does nothing on every machine that ran the tool before.
        let dir = tempdir();
        let path = dir.join("v5.db");
        {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            conn.execute_batch(SCHEMA).expect("schema");
            conn.execute_batch(MIGRATE_1_TO_2).expect("v2");
            conn.execute_batch(MIGRATE_2_TO_3).expect("v3");
            conn.execute_batch(MIGRATE_3_TO_4).expect("v4");
            conn.execute_batch(MIGRATE_4_TO_5).expect("v5");
            conn.execute_batch("PRAGMA user_version = 5;")
                .expect("stamp");
        }
        let s = Store::open_at(&path).expect("open a v5 database");
        s.create_job("j1", "crawl", "", "{}")
            .expect("a v5 database must gain the job table");
        assert!(s.job("j1").is_some());
    }

    #[test]
    fn only_one_runner_can_claim_a_queued_job() {
        // Two svipall processes can share this file — the crawl tests already run two servers over
        // one — so a read-then-write claim lets both take the same job. That is two crawls of one
        // site at twice the request rate, from one address.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        s.create_job("j1", "crawl", "", "{}").expect("create");
        assert!(s.start_job("j1", "run-a").expect("claim"));
        assert!(
            !s.start_job("j1", "run-b").expect("claim"),
            "a second runner took a job that was already running"
        );
        assert_eq!(s.job("j1").expect("job").state, "running");
    }

    #[test]
    fn a_job_left_running_by_a_dead_process_becomes_resumable_rather_than_lost() {
        // `crawl.status` is written 'running' per batch and never cleared, so the crawl table
        // cannot tell death from completion. This is the whole reason the job row exists.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        job_with_crawl(&s, "dead", "https://x.test/", 3, "running");
        s.start_job("dead", "old-run").expect("claim");
        // Its last heartbeat was long ago: the process that owned it is gone.
        s.write
            .lock()
            .unwrap()
            .execute(
                "UPDATE job SET heartbeat_at = ?1 WHERE id = 'dead'",
                params![now() - 3600],
            )
            .expect("age it");

        job_with_crawl(&s, "alive", "https://y.test/", 1, "running");
        s.start_job("alive", "other-live-run").expect("claim");

        assert_eq!(s.adopt_orphaned_jobs("this-run", 300), 1);
        assert_eq!(s.job("dead").expect("job").state, "interrupted");
        assert_eq!(
            s.job("alive").expect("job").state,
            "running",
            "a live process's job was taken because its owner was merely foreign"
        );
        // And what makes `interrupted` worth reporting rather than fatal: it can be picked up.
        assert!(s.start_job("dead", "this-run").expect("claim"));
    }

    #[test]
    fn a_finished_job_is_forgotten_after_its_keep_and_an_interrupted_one_is_not() {
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        s.create_job("done", "crawl", "", "{}").expect("create");
        s.start_job("done", "run").expect("claim");
        s.finish_job("done", "finished", Some(r#"{"count":3}"#), None)
            .expect("finish");
        s.create_job("cut", "crawl", "", "{}").expect("create");
        s.start_job("cut", "old").expect("claim");
        let old = now() - 86_400 * 30;
        {
            let conn = s.write.lock().unwrap();
            conn.execute(
                "UPDATE job SET state='interrupted', finished_at=?1 WHERE id='cut'",
                params![old],
            )
            .expect("age it");
            conn.execute(
                "UPDATE job SET finished_at=?1 WHERE id='done'",
                params![old],
            )
            .expect("age it");
        }

        assert_eq!(s.expire_jobs(86_400 * 7), 1);
        assert!(s.job("done").is_none());
        assert!(
            s.job("cut").is_some(),
            "an interrupted job is the one a person most wants to see, so it is never swept"
        );
    }

    #[test]
    fn a_job_reports_the_pages_its_crawl_actually_fetched_rather_than_a_copy() {
        // Fails the moment anybody adds a `pages_done` column to `job`: two answers to one question
        // is how a poll starts disagreeing with what a resume would actually do.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        job_with_crawl(&s, "j1", "https://x.test/", 2, "running");
        assert_eq!(s.job("j1").expect("job").pages_done, 2);
        s.save_crawl("j1", "https://x.test/", "{}", "running", 9, None)
            .expect("crawl");
        assert_eq!(
            s.job("j1").expect("job").pages_done,
            9,
            "the job reported a copy rather than the crawl's own count"
        );
    }

    #[test]
    fn a_cancelled_job_keeps_its_request_where_a_restart_can_still_see_it() {
        // A flag in the row rather than only in memory: a process that dies between the request and
        // the job noticing must not lose the fact that somebody asked it to stop.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        s.create_job("j1", "crawl", "", "{}").expect("create");
        s.start_job("j1", "run").expect("claim");
        assert_eq!(s.request_cancel("j1").as_deref(), Some("running"));
        assert!(s.cancel_requested("j1"));
        assert!(s.job("j1").expect("job").cancel_requested);
        assert!(
            s.request_cancel("nope").is_none(),
            "cancelling a job that does not exist must be distinguishable from cancelling one"
        );
    }

    #[test]
    fn two_jobs_for_one_site_are_not_offered_at_the_same_time() {
        // One address, one reputation with that host. The benchmark's own regression came from
        // hitting one site too often in a day, so this is the rule and not a detail.
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        job_with_crawl(&s, "first", "https://shop.test/a", 0, "running");
        s.start_job("first", "run").expect("claim");
        job_with_crawl(&s, "same-site", "https://shop.test/b", 0, "queued");
        job_with_crawl(&s, "other-site", "https://news.test/", 0, "queued");

        let next = s.next_queued_job().expect("something is queued");
        assert_eq!(
            next.id, "other-site",
            "the queued job for a site already being crawled was offered anyway"
        );
    }

    #[test]
    fn a_stored_summary_survives_the_round_trip_and_is_not_carried_in_a_listing() {
        let dir = tempdir();
        let s = Store::open_at(&dir.join("jobs.db")).expect("open");
        s.create_job("j1", "crawl", "", "{}").expect("create");
        s.start_job("j1", "run").expect("claim");
        let summary = format!(r#"{{"count":200,"pages":["{}"]}}"#, "x".repeat(20_000));
        s.finish_job("j1", "finished", Some(&summary), None)
            .expect("finish");

        assert_eq!(s.job_result("j1").as_deref(), Some(summary.as_str()));
        let listed = s.jobs(None, 10);
        assert_eq!(listed.len(), 1);
        let wire = serde_json::to_string(&listed).expect("serialise");
        assert!(
            !wire.contains("xxxx"),
            "a listing carried the pages a job produced: {} bytes",
            wire.len()
        );
    }
}
