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
    /// The latest response on this route was a classified fingerprint/hold wall. Older saved
    /// samples lack this evidence and must not predict failures on routes they never attempted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint_wall: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    Useful,
    Delivered,
    Failed,
    FingerprintWall,
}

impl Sample {
    pub fn observe(&mut self, feedback: Feedback, latency_ms: u64, now: u64) {
        let decay = 0.5f64.powf(now.saturating_sub(self.updated) as f64 / 43200.0);
        let failed = matches!(feedback, Feedback::Failed | Feedback::FingerprintWall);
        self.successes = self.successes * decay + f64::from(feedback == Feedback::Useful);
        self.failures = if failed {
            self.failures * decay + 1.0
        } else {
            0.0
        };
        if failed {
            self.successes = self.successes.min(4.0) * 0.5;
        }
        self.latency_ms = self.latency_ms * 0.7 + latency_ms as f64 * 0.3;
        self.updated = now;
        self.fingerprint_wall = (feedback == Feedback::FingerprintWall).then_some(now);
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
            fingerprint_wall: None,
        });
        rows.len() - 1
    });
    rows[i].observe(feedback, latency_ms, now);
    MEMORY.insert(key, &serde_json::to_string(&rows).unwrap_or_default());
}

/// Fingerprinting, hold and positively identified managed challenges favor a headful session.
/// An ordinary self-verifying interstitial does not imply that requirement. Keep the ceiling and
/// native-last ordering, including when learning has promoted an emulated route to the front.
pub fn after_wall(
    tiers: &[String],
    current: usize,
    kind: &crate::WallKind,
    managed_challenge: bool,
) -> usize {
    let next = current.saturating_add(1).min(tiers.len());
    let headful_required = matches!(kind, crate::WallKind::Vendor | crate::WallKind::Hold)
        || (matches!(kind, crate::WallKind::Cloudflare) && managed_challenge);
    if !headful_required {
        return next;
    }
    let Some(route) = tiers.get(current) else {
        return next;
    };
    let headful = matches!(route.as_str(), "real" | "warm");
    tiers
        .iter()
        .enumerate()
        .skip(next)
        .find_map(|(i, tier)| {
            let stronger = tier == "warm" || (tier == "real" && !headful);
            (stronger || (headful && tier.starts_with("native:"))).then_some(i)
        })
        .unwrap_or(if headful { tiers.len() } else { next })
}

/// Promote a supported emulated winner. Skip routes refused repeatedly for 30 minutes, retaining
/// the strongest emulated probe even when all failed. Native always remains last, or is omitted
/// while its own failures cool down. A recent classified emulated fingerprint wall also skips
/// weaker untried routes when a headful probe is permitted; generic errors imply no such wall.
/// No extra exploration traffic is generated.
pub fn plan(tiers: &[String], records: &[Sample], now: u64, native: bool) -> Vec<String> {
    let failed = |tier: &str| {
        records.iter().any(|r| {
            r.tier == tier
                && r.failures >= 1.9
                && r.successes < 1.9
                && now.saturating_sub(r.updated) < 1800
        })
    };
    let fingerprint_wall = records
        .iter()
        .filter(|r| !r.tier.starts_with("native:"))
        .filter_map(|r| r.fingerprint_wall)
        .filter(|&observed| observed <= now && now - observed < 1800)
        .max()
        .filter(|_| tiers.iter().any(|t| matches!(t.as_str(), "real" | "warm")));
    let weak_probe = |tier: &str| {
        fingerprint_wall.is_some_and(|observed| {
            matches!(tier, "http" | "browser" | "stealth")
                && !records.iter().any(|r| {
                    r.tier == tier
                        && r.updated >= observed
                        && r.failures == 0.0
                        && r.fingerprint_wall.is_none()
                })
        })
    };
    let probe = tiers.last();
    let mut out: Vec<String> = tiers
        .iter()
        .filter(|t| Some(*t) == probe || (!failed(t) && !weak_probe(t)))
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
