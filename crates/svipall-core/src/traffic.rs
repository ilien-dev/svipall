//! Persistent, transactional visit admission across processes, tiers and identity modes.
//! This separate database is not the solver schema. It counts top-level visits, not the
//! browser's resource requests, redirects or challenge exchanges, and every process and session
//! on this machine shares it.
//!
//! Version 2 adds a digest of the domain beside each route, so everything held for a domain can be
//! cleared across its exits, and keeps the server's strikes beside its hold. Version 1's tables are
//! left in place for an older binary that may still be running; their rows are copied once.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

pub struct Ledger(Mutex<Connection>);

/// The answer to "may this visit go out now?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Charged. `visits_left` more fit in the current window.
    Granted { visits_left: u32 },
    /// The window is full. Nothing was written: the oldest visit leaves it in `wait` seconds.
    Full { wait: u64 },
    /// The server asked for a pause, or a hard block put the site on hold, for `wait` more seconds.
    Held { wait: u64 },
}

impl Admission {
    pub fn is_granted(&self) -> bool {
        matches!(self, Self::Granted { .. })
    }
}

/// What a 429 or 503 said about when to come back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backoff {
    /// The server named the delay.
    RetryAfter(u64),
    /// A 429 with no delay: one more strike.
    RateLimited,
    /// A 503 with no delay: a server error, so a short pause and no strike.
    ServerError,
}

/// The longest `Retry-After` honoured as given. A server that names longer is held this long.
pub const MAX_RETRY_AFTER_SECS: u64 = 3600;
/// The first strike's pause. Each further strike multiplies it by four, up to
/// `request_cooldown_seconds`.
pub const FIRST_STRIKE_SECS: u64 = 30;
/// Strikes older than this are forgotten: a site that rate-limited once an hour ago starts again
/// from the short pause.
pub const STRIKE_MEMORY_SECS: u64 = 1800;

/// How long a walk up the whole ladder that failed answers the next call to its origin.
pub const FAILURE_MEMORY_SECS: u64 = 600;

/// What `clear` removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Cleared {
    pub holds: usize,
    pub visits: usize,
    pub remembered_failures: usize,
}

/// A walk up the whole ladder that failed, as remembered for its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub at: u64,
    /// The highest tier the walk reached.
    pub top: String,
    /// The response the walk ended with, as JSON.
    pub record: String,
}

fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

impl Ledger {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS visits_v1 (route TEXT NOT NULL, at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS holds_v1 (route TEXT PRIMARY KEY, until INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS pacing_v1 (route TEXT PRIMARY KEY, at_ms INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS visits_v2 (route TEXT NOT NULL, domain TEXT, at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS visit_route_v2 ON visits_v2(route, at);
            CREATE INDEX IF NOT EXISTS visit_domain_v2 ON visits_v2(domain);
            CREATE TABLE IF NOT EXISTS holds_v2 (route TEXT PRIMARY KEY, domain TEXT,
                until INTEGER NOT NULL, strikes INTEGER NOT NULL DEFAULT 0,
                struck_at INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS failures_v2 (route TEXT PRIMARY KEY, domain TEXT NOT NULL,
                at INTEGER NOT NULL, top TEXT NOT NULL, record TEXT NOT NULL);",
        )?;
        // Once per database: what version 1 had, carried over. Its rows have no domain, so they
        // can only be cleared through the direct route; they expire within a window or a hold.
        // Checked inside the write lock, so two processes opening it at once copy it once.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 2 {
            tx.execute_batch(
                "INSERT INTO visits_v2(route, domain, at) SELECT route, NULL, at FROM visits_v1;
                INSERT OR IGNORE INTO holds_v2(route, domain, until)
                    SELECT route, NULL, until FROM holds_v1;
                PRAGMA user_version = 2;",
            )?;
        }
        tx.commit()?;
        Ok(Self(Mutex::new(conn)))
    }

    /// Reserve minimum spacing across CLI processes and concurrent server requests.
    pub fn pace(
        &self,
        domain: &str,
        exit: Option<&str>,
        now_ms: u64,
        minimum_ms: u64,
    ) -> anyhow::Result<u64> {
        let route = Self::key(domain, exit);
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: u64 = tx.query_row(
            "SELECT COALESCE(MAX(at_ms), 0) FROM pacing_v1 WHERE route=?1",
            [&route],
            |r| r.get(0),
        )?;
        let scheduled = now_ms.max(previous.saturating_add(minimum_ms));
        tx.execute(
            "INSERT OR REPLACE INTO pacing_v1(route, at_ms) VALUES (?1, ?2)",
            params![route, scheduled],
        )?;
        tx.commit()?;
        Ok(scheduled.saturating_sub(now_ms))
    }

    fn key(domain: &str, exit: Option<&str>) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(domain, exit)).unwrap_or_default())
        )
    }

    /// Reserve atomically before any network work. A full window writes nothing: it is a wait for
    /// the next slot, never a hold of our own making.
    pub fn reserve(
        &self,
        domain: &str,
        exit: Option<&str>,
        cfg: &crate::Config,
        now: u64,
    ) -> anyhow::Result<Admission> {
        let route = Self::key(domain, exit);
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let until: u64 = tx.query_row(
            "SELECT COALESCE(MAX(until), 0) FROM holds_v2 WHERE route=?1",
            [&route],
            |r| r.get(0),
        )?;
        if until > now {
            return Ok(Admission::Held { wait: until - now });
        }
        tx.execute(
            "DELETE FROM visits_v2 WHERE at <= ?1",
            [now.saturating_sub(86400)],
        )?;
        let since = now.saturating_sub(cfg.request_window_seconds);
        let (count, oldest): (u32, Option<u64>) = tx.query_row(
            "SELECT COUNT(*), MIN(at) FROM visits_v2 WHERE route=?1 AND at > ?2",
            params![route, since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let admission = if count >= cfg.request_limit {
            let frees_at = oldest
                .unwrap_or(now)
                .saturating_add(cfg.request_window_seconds);
            Admission::Full {
                wait: frees_at.saturating_sub(now).max(1),
            }
        } else {
            tx.execute(
                "INSERT INTO visits_v2(route, domain, at) VALUES (?1, ?2, ?3)",
                params![route, digest(domain), now],
            )?;
            Admission::Granted {
                visits_left: cfg.request_limit - count - 1,
            }
        };
        tx.commit()?;
        Ok(admission)
    }

    /// Visits still free in the current window, without charging one.
    pub fn visits_left(
        &self,
        domain: &str,
        exit: Option<&str>,
        cfg: &crate::Config,
        now: u64,
    ) -> anyhow::Result<u32> {
        let count: u32 = self.0.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM visits_v2 WHERE route=?1 AND at > ?2",
            params![
                Self::key(domain, exit),
                now.saturating_sub(cfg.request_window_seconds)
            ],
            |r| r.get(0),
        )?;
        Ok(cfg.request_limit.saturating_sub(count))
    }

    /// Hold the route after a 429 or 503 and return how long for.
    ///
    /// A `Retry-After` is honoured as given, up to an hour. Without one, a 429 is a strike: the
    /// first pauses `FIRST_STRIKE_SECS`, each further strike four times longer, up to
    /// `request_cooldown_seconds`. A 503 without one is a server error and pauses only the first
    /// strike's length, adding no strike. A concurrent success cannot shorten a hold.
    pub fn backoff(
        &self,
        domain: &str,
        exit: Option<&str>,
        why: Backoff,
        cfg: &crate::Config,
        now: u64,
    ) -> anyhow::Result<u64> {
        let route = Self::key(domain, exit);
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (strikes, struck_at): (u32, u64) = tx
            .query_row(
                "SELECT strikes, struck_at FROM holds_v2 WHERE route=?1",
                [&route],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0));
        let strikes = if now.saturating_sub(struck_at) > STRIKE_MEMORY_SECS {
            0
        } else {
            strikes
        };
        let (wait, strikes, struck_at) = match why {
            Backoff::RetryAfter(s) => (s.clamp(1, MAX_RETRY_AFTER_SECS), strikes, struck_at),
            Backoff::ServerError => (FIRST_STRIKE_SECS, strikes, struck_at),
            Backoff::RateLimited => {
                let strikes = strikes + 1;
                let wait = FIRST_STRIKE_SECS
                    .saturating_mul(4u64.saturating_pow(strikes - 1))
                    .min(cfg.request_cooldown_seconds.max(FIRST_STRIKE_SECS));
                (wait, strikes, now)
            }
        };
        tx.execute(
            "INSERT INTO holds_v2(route, domain, until, strikes, struck_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(route) DO UPDATE SET until=MAX(until, excluded.until),
                domain=excluded.domain, strikes=excluded.strikes, struck_at=excluded.struck_at",
            params![route, digest(domain), now + wait, strikes, struck_at],
        )?;
        tx.commit()?;
        Ok(wait)
    }

    /// Seconds left on this route's hold.
    pub fn remaining(&self, domain: &str, exit: Option<&str>, now: u64) -> anyhow::Result<u64> {
        let until: u64 = self.0.lock().unwrap().query_row(
            "SELECT COALESCE(MAX(until), 0) FROM holds_v2 WHERE route=?1",
            [Self::key(domain, exit)],
            |r| r.get(0),
        )?;
        Ok(until.saturating_sub(now))
    }

    /// Routes held right now, for the status report. Domains are stored as digests, so a count.
    pub fn active_holds(&self, now: u64) -> anyhow::Result<u64> {
        Ok(self.0.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM holds_v2 WHERE until > ?1",
            [now],
            |r| r.get(0),
        )?)
    }

    /// Everything held against a domain, on every exit: holds and their strikes, the visits in the
    /// window, and a remembered failure. What `clear_cooldown` means.
    pub fn clear(&self, domain: &str) -> anyhow::Result<Cleared> {
        let d = digest(domain);
        let direct = Self::key(domain, None);
        let conn = self.0.lock().unwrap();
        Ok(Cleared {
            holds: conn.execute(
                "DELETE FROM holds_v2 WHERE domain=?1 OR route=?2",
                params![d, direct],
            )?,
            visits: conn.execute(
                "DELETE FROM visits_v2 WHERE domain=?1 OR route=?2",
                params![d, direct],
            )?,
            remembered_failures: conn.execute("DELETE FROM failures_v2 WHERE domain=?1", [d])?,
        })
    }

    /// Remember that a walk up the whole ladder failed for this origin and exit.
    pub fn remember_failure(
        &self,
        origin: &str,
        exit: Option<&str>,
        domain: &str,
        top: &str,
        record: &str,
        now: u64,
    ) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO failures_v2(route, domain, at, top, record)
                VALUES (?1, ?2, ?3, ?4, ?5)",
            params![Self::key(origin, exit), digest(domain), now, top, record],
        )?;
        Ok(())
    }

    /// The failure remembered for this origin and exit, if it is younger than `ttl` seconds.
    pub fn recall_failure(
        &self,
        origin: &str,
        exit: Option<&str>,
        now: u64,
        ttl: u64,
    ) -> anyhow::Result<Option<Failure>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT at, top, record FROM failures_v2 WHERE route=?1 AND at > ?2",
                params![Self::key(origin, exit), now.saturating_sub(ttl)],
                |r| {
                    Ok(Failure {
                        at: r.get(0)?,
                        top: r.get(1)?,
                        record: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Forget the failure for this origin and exit: a page came through.
    pub fn forget_failure(&self, origin: &str, exit: Option<&str>) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute(
            "DELETE FROM failures_v2 WHERE route=?1",
            [Self::key(origin, exit)],
        )?;
        Ok(())
    }
}
