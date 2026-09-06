//! Local route evidence. Privacy is a constraint, never a score that success can outweigh.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{LazyLock, Mutex};

static MEMORY: LazyLock<crate::store::JsonMap> = LazyLock::new(|| {
    crate::store::JsonMap::new(crate::config::home_dir().join("automatic_routes.json"))
});
static UPDATE: Mutex<()> = Mutex::new(());
const TTL: u64 = 86400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub tier: String,
    pub successes: f64,
    pub failures: f64,
    pub latency_ms: f64,
    pub updated: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    Useful,
    Delivered,
    Failed,
}

impl Sample {
    pub fn observe(&mut self, feedback: Feedback, latency_ms: u64, now: u64) {
        let decay = 0.5f64.powf(now.saturating_sub(self.updated) as f64 / 43200.0);
        self.successes = self.successes * decay + f64::from(feedback == Feedback::Useful);
        self.failures = if feedback == Feedback::Failed {
            self.failures * decay + 1.0
        } else {
            0.0
        };
        if feedback == Feedback::Failed {
            self.successes = self.successes.min(4.0) * 0.5;
        }
        self.latency_ms = self.latency_ms * 0.7 + latency_ms as f64 * 0.3;
        self.updated = now;
    }
}

/// Store a digest rather than URLs, query values or proxy credentials. One route family per
/// first path segment; configuration and browser changes deliberately start fresh evidence.
pub fn context(url: &str, exit: Option<&str>, environment: &str) -> String {
    let parsed = url::Url::parse(url).ok();
    let origin = parsed
        .as_ref()
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default();
    let family = parsed
        .as_ref()
        .and_then(|u| u.path_segments()?.find(|s| !s.is_empty()))
        .unwrap_or("");
    let input = serde_json::to_vec(&(origin, family, exit, environment)).unwrap_or_default();
    let domain = crate::domain_from_url(url);
    format!(
        "{:x}:{:x}",
        Sha256::digest(domain.as_bytes()),
        Sha256::digest(input)
    )
}

pub fn forget(domain: &str) {
    let _lock = UPDATE.lock().unwrap();
    let prefix = format!("{:x}:", Sha256::digest(domain.as_bytes()));
    for key in MEMORY.snapshot().keys().filter(|k| k.starts_with(&prefix)) {
        MEMORY.remove(key);
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load(key: &str) -> Vec<Sample> {
    MEMORY
        .get(key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn record(key: &str, tier: &str, feedback: Feedback, latency_ms: u64) {
    let _lock = UPDATE.lock().unwrap();
    let now = now();
    let mut rows = load(key);
    rows.retain(|r| now.saturating_sub(r.updated) <= TTL);
    let i = rows.iter().position(|r| r.tier == tier).unwrap_or_else(|| {
        rows.push(Sample {
            tier: tier.into(),
            successes: 0.0,
            failures: 0.0,
            latency_ms: latency_ms as f64,
            updated: now,
        });
        rows.len() - 1
    });
    rows[i].observe(feedback, latency_ms, now);
    MEMORY.insert(key, &serde_json::to_string(&rows).unwrap_or_default());
}

/// Promote a supported emulated winner. Skip routes refused repeatedly for 30 minutes, retaining
/// the strongest emulated probe even when all failed. Native always remains last, or is omitted
/// while its own failures cool down. No extra exploration traffic is generated.
pub fn plan(tiers: &[String], records: &[Sample], now: u64, native: bool) -> Vec<String> {
    let failed = |tier: &str| {
        records.iter().any(|r| {
            r.tier == tier
                && r.failures >= 1.9
                && r.successes < 1.9
                && now.saturating_sub(r.updated) < 1800
        })
    };
    let probe = tiers.last();
    let mut out: Vec<String> = tiers
        .iter()
        .filter(|t| Some(*t) == probe || !failed(t))
        .cloned()
        .collect();
    let winner = records
        .iter()
        .filter(|r| {
            out.contains(&r.tier)
                && r.successes >= 1.9
                && r.successes > r.failures
                && now.saturating_sub(r.updated) <= TTL
        })
        .max_by(|a, b| {
            let score = |r: &Sample| {
                (r.successes + 1.0)
                    / (r.successes + r.failures + 2.0)
                    / (1.0 + r.latency_ms / 30000.0)
            };
            score(a).total_cmp(&score(b))
        });
    if let Some(winner) = winner {
        out.retain(|t| t != &winner.tier);
        out.insert(0, winner.tier.clone());
    }
    if native {
        if let Some(tier) = tiers.iter().rev().find(|t| t.as_str() != "http") {
            let route = format!("native:{tier}");
            if !failed(&route) {
                out.push(route);
            }
        }
    }
    out
}
