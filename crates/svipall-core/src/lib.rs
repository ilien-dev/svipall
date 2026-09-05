//! svipall-core — core primitives for unrestricted web exploration.
//! English only. Port of webone server.py classification, ladder and housekeeping.

pub mod altsvc;
pub mod answer;
pub mod blocklist;
pub mod budget;
pub mod cache;
pub mod capacity;
pub mod challenge;
pub mod classify;
/// Is this identity internally consistent? The check Camoufox says it keeps failing.
pub mod coherence;
pub mod config;
pub mod dedup;
pub mod document;
pub mod domain;
pub mod exits;
pub mod export;
pub mod extraction;
pub mod fleet;
pub mod forms;
pub mod frontier;
pub mod geo;
pub mod growth;
pub mod identity;
pub mod incremental;
pub mod ladder;
pub mod lan;
pub mod llms_txt;
pub mod pagination;
pub mod pdf;
pub mod policy;
pub mod pow;
pub mod profiles;
/// What came back, judged as content rather than as a wall.
pub mod quality;
pub mod reputation;
pub mod robots;
pub mod saturation;
pub mod selectors;
pub mod session;
pub mod sitemap;
pub mod store;
pub mod template;
pub mod throttle;
/// Comparing a secret against what a caller sent, without leaking it a byte at a time.
pub mod token;
pub mod types;
pub mod warm;
pub mod watch;
pub mod widget;

pub use challenge::{Challenge, ChallengeKind};
pub use classify::{
    challenge_is_self_verifying, challenge_reports_progress, classify, classify_view,
    clearance_lives_in_the_runtime, cloudflare_is_managed_challenge, local_injection,
    wall_blames_the_address, wall_is_hard_block, PageView, WallKind,
};
pub use config::Config;
pub use domain::domain_from_url;
pub use extraction::{dom_parse_count, parse_page, MarkdownOpts, PageParts, ParseWants};
pub use identity::{Engine, IdentityProfile, Os, MAX_EMULATED_CHROME, MAX_EMULATED_FIREFOX};
pub use ladder::{build_ladder, forget_tier, load_tiers, remember_tier};
pub use profiles::{auto_profile_path, ensure_dirs, evict_old_profiles, prune_profile_cache};
pub use robots::{Robots, RobotsPolicy};
pub use store::route_for;
pub use throttle::{check_cooldown, clear_cooldown, list_cooldowns, set_cooldown, throttle};
pub use types::TIERS;
