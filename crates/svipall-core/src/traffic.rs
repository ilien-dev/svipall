//! Persistent, transactional visit admission across processes, tiers and identity modes.
//! This separate database is not the solver schema. Version 1 counts top-level visits, not the
//! browser's resource requests, redirects or challenge exchanges.
use rusqlite::{params, Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

pub struct Ledger(Mutex<Connection>);

impl Ledger {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS visits_v1 (route TEXT NOT NULL, at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS visit_route_v1 ON visits_v1(route, at);
            CREATE TABLE IF NOT EXISTS holds_v1 (route TEXT PRIMARY KEY, until INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS pacing_v1 (route TEXT PRIMARY KEY, at_ms INTEGER NOT NULL);",
        )?;
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

    /// Reserve atomically before any network work. Rejected calls do not extend a cooldown.
    pub fn reserve(
        &self,
        domain: &str,
        exit: Option<&str>,
        cfg: &crate::Config,
        now: u64,
    ) -> anyhow::Result<Option<u64>> {
        let route = Self::key(domain, exit);
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let until: u64 = tx.query_row(
            "SELECT COALESCE(MAX(until), 0) FROM holds_v1 WHERE route=?1",
            [&route],
            |r| r.get(0),
        )?;
        if until > now {
            return Ok(Some(until - now));
        }
        tx.execute(
            "DELETE FROM visits_v1 WHERE at <= ?1",
            [now.saturating_sub(86400)],
        )?;
        tx.execute("DELETE FROM holds_v1 WHERE until <= ?1", [now])?;
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM visits_v1 WHERE route=?1 AND at > ?2",
            params![route, now.saturating_sub(cfg.request_window_seconds)],
            |r| r.get(0),
        )?;
        let wait = if count >= cfg.request_limit {
            tx.execute(
                "INSERT OR REPLACE INTO holds_v1(route, until) VALUES (?1, ?2)",
                params![route, now.saturating_add(cfg.request_cooldown_seconds)],
            )?;
            Some(cfg.request_cooldown_seconds)
        } else {
            tx.execute(
                "INSERT INTO visits_v1(route, at) VALUES (?1, ?2)",
                params![route, now],
            )?;
            None
        };
        tx.commit()?;
        Ok(wait)
    }

    /// Preserve the server's full Retry-After across restarts; a successful concurrent response
    /// cannot shorten it. Ordinary backoff is also stored here after repeated refusal.
    pub fn hold(&self, domain: &str, exit: Option<&str>, until: u64) -> anyhow::Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO holds_v1(route, until) VALUES (?1, ?2)
            ON CONFLICT(route) DO UPDATE SET until=MAX(until, excluded.until)",
            params![Self::key(domain, exit), until],
        )?;
        Ok(())
    }

    pub fn remaining(&self, domain: &str, exit: Option<&str>, now: u64) -> anyhow::Result<u64> {
        let until: u64 = self.0.lock().unwrap().query_row(
            "SELECT COALESCE(MAX(until), 0) FROM holds_v1 WHERE route=?1",
            [Self::key(domain, exit)],
            |r| r.get(0),
        )?;
        Ok(until.saturating_sub(now))
    }
}
