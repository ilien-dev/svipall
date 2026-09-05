//! Shared constants for the fetch ladder.

/// Tier order for `mode=auto`, cheapest first. String tiers are used throughout the codebase;
/// `build_ladder` and `tier_index` index into this slice.
pub const TIERS: &[&str] = &["http", "browser", "stealth", "real", "warm"];
