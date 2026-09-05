//! Ladder escalation logic — port of server.py _fetch_sync ladder.
//! Fast path: http tier ~100ms. Escalation only when classify detects a wall.

use crate::types::TIERS;
use std::collections::HashMap;

pub fn tier_index(t: &str) -> Option<usize> {
    TIERS.iter().position(|&x| x == t)
}

/// Learned per-domain start tiers (`~/.svipall/domain_tiers.json`), served from memory.
pub fn load_tiers() -> HashMap<String, String> {
    crate::store::TIERS.as_map()
}

/// Remember the tier that delivered the page. `http` is the default and needs no entry;
/// a domain that starts working on http again is forgotten so the next fetch is fast.
pub fn remember_tier(domain: &str, tier: &str) {
    if domain.is_empty() {
        return;
    }
    if tier == "http" {
        crate::store::TIERS.remove(domain);
    } else {
        crate::store::TIERS.insert(domain, tier);
    }
}

pub fn forget_tier(domain: &str) {
    crate::store::TIERS.remove(domain);
}

/// Build ladder tiers for mode and max_tier, respecting learned memory.
///
/// `h3_probe` says this domain advertises HTTP/3 and has not already refused to deliver over it.
/// When it does, one `http` attempt is put in front of whatever was learned — because what was
/// learned was learned over TCP, and QUIC is a different request rather than a repeat of a
/// known-failed one. The caller decides; this only shapes the list.
pub fn build_ladder(mode: &str, max_tier: &str, domain: &str, h3_probe: bool) -> Vec<String> {
    if mode != "auto" {
        return vec![mode.to_string()];
    }
    let mem = load_tiers();
    let start = mem.get(domain).map(|s| s.as_str()).unwrap_or("http");
    let i0 = tier_index(start).unwrap_or(0);
    let i_max = tier_index(max_tier).unwrap_or(TIERS.len() - 1);
    let mut tiers: Vec<String> = if i0 > i_max {
        // Learned tier is above the cap: the cap wins, single attempt.
        vec![TIERS[i_max].to_string()]
    } else {
        TIERS[i0..=i_max].iter().map(|s| s.to_string()).collect()
    };
    if h3_probe && tiers.first().map(String::as_str) != Some("http") {
        tiers.insert(0, "http".to_string());
    }
    tiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_respects_cap() {
        assert_eq!(build_ladder("http", "warm", "x.test", false), vec!["http"]);
        let full = build_ladder("auto", "warm", "never-seen.test", false);
        assert_eq!(full, vec!["http", "browser", "stealth", "real", "warm"]);
        assert_eq!(
            build_ladder("auto", "browser", "never-seen.test", false),
            vec!["http", "browser"]
        );
    }

    #[test]
    fn a_domain_that_offers_h3_gets_one_cheap_attempt_before_the_tier_it_was_walled_at() {
        // The gap this closes: `domain_tiers` remembers that a site needed a browser, so the next
        // fetch starts there and the http tier — the only place HTTP/3 is spoken — is never asked
        // again. But the tier was learned from a *TCP* failure, and QUIC is a different request,
        // not a repeat of a known-failed one.
        crate::remember_tier("walled.test", "warm");
        assert_eq!(
            build_ladder("auto", "warm", "walled.test", false),
            vec!["warm"],
            "without the probe, the learned tier is where it starts"
        );
        assert_eq!(
            build_ladder("auto", "warm", "walled.test", true),
            vec!["http", "warm"],
            "with it, one http attempt first and then straight back to what was learned"
        );
        crate::forget_tier("walled.test");
    }

    #[test]
    fn the_probe_never_duplicates_a_tier_the_ladder_already_starts_at() {
        assert_eq!(
            build_ladder("auto", "browser", "fresh.test", true),
            vec!["http", "browser"],
            "http is already first; asking for a probe must not add a second one"
        );
    }

    #[test]
    fn a_forced_mode_is_never_widened_by_the_probe() {
        // `mode` is the caller saying which tier to use. A probe that added one would answer a
        // different question than the one asked.
        assert_eq!(
            build_ladder("browser", "warm", "x.test", true),
            vec!["browser"]
        );
    }
}
