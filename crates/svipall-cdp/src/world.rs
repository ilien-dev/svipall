#![warn(clippy::all)] // Ours, unlike the rest of this crate: lint it properly.

//! The name of the isolated world, and the `sourceURL` of the script that creates it.
//!
//! Upstream hard-codes `__chromiumoxide_utility_world__` and
//! `____chromiumoxide_utility_world___evaluation_script__`. Both are observable. The name is
//! visible to anything else attached over the protocol, and the `sourceURL` shows up in stack
//! traces taken from code that ran in that world — which is exactly the "injected residue"
//! category a detector enumerates for: property and script names that no real browser session
//! produces. A constant that spells out the automation library is the easiest possible tell.
//!
//! So the names become opaque and per-process. The caller sets them once from its own identity, so
//! they are stable for the life of a browser (changing the name mid-session would silently create
//! a second world and leak execution contexts) and different between installations.

use std::sync::OnceLock;

static WORLD: OnceLock<Names> = OnceLock::new();

#[derive(Debug)]
struct Names {
    world: String,
    script_url: String,
}

/// Derive the pair from a caller-supplied seed. Only the first call has any effect: the name has
/// to stay put for as long as the browser lives.
pub fn seed(seed: u64) -> bool {
    WORLD.set(Names::from_seed(seed)).is_ok()
}

impl Names {
    fn from_seed(seed: u64) -> Self {
        // Two different mixes of the same seed, so the script URL is not the world name with a
        // suffix — a shared prefix would let one leak identify the other.
        let world = format!("__{:08x}", (seed ^ 0x9E37_79B9_7F4A_7C15) as u32);
        let script_url = format!(
            "__{:08x}",
            (seed.rotate_left(32) ^ 0xD1B5_4A32_D192_ED03) as u32
        );
        Self { world, script_url }
    }

    fn process_default() -> Self {
        // Nothing seeded us, so mix something that differs between runs. Not a secret, just not a
        // constant anyone can grep for.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        Self::from_seed((std::process::id() as u64) << 32 | nanos)
    }
}

fn names() -> &'static Names {
    WORLD.get_or_init(Names::process_default)
}

/// The isolated world svipall evaluates in.
pub fn utility_world_name() -> &'static str {
    &names().world
}

/// The `sourceURL` given to the script that creates that world.
pub fn evaluation_script_url() -> &'static str {
    &names().script_url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seeded_name_never_spells_out_the_library() {
        // The whole reason this module exists. Upstream shipped
        // `__chromiumoxide_utility_world__`, which is a free identification for anything that
        // enumerates script and context names.
        for s in [0u64, 1, 42, u64::MAX, 0x9E37_79B9_7F4A_7C15] {
            let n = Names::from_seed(s);
            for value in [&n.world, &n.script_url] {
                let low = value.to_lowercase();
                assert!(!low.contains("chromium"), "{value} names the library");
                assert!(!low.contains("oxide"), "{value} names the library");
                assert!(!low.contains("utility"), "{value} describes its purpose");
                assert!(!low.contains("world"), "{value} describes its purpose");
                assert!(!low.contains("puppeteer") && !low.contains("playwright"));
            }
        }
    }

    #[test]
    fn the_world_and_its_script_url_do_not_share_a_prefix() {
        // If the script URL were the world name plus a suffix, leaking either would give up both.
        let n = Names::from_seed(0xDEAD_BEEF);
        assert_ne!(n.world, n.script_url);
        assert!(!n.script_url.starts_with(&n.world));
        assert!(!n.world.starts_with(&n.script_url));
    }

    #[test]
    fn the_same_seed_gives_the_same_pair() {
        // Stability matters more than unpredictability here: a name that changed mid-session would
        // create a second isolated world and leak the first one's execution contexts.
        let a = Names::from_seed(7);
        let b = Names::from_seed(7);
        assert_eq!(a.world, b.world);
        assert_eq!(a.script_url, b.script_url);
    }

    #[test]
    fn different_seeds_give_different_names() {
        assert_ne!(Names::from_seed(1).world, Names::from_seed(2).world);
    }

    #[test]
    fn a_generated_name_is_a_plausible_identifier() {
        // It ends up in a JS execution-context name and a sourceURL, so it has to be inert.
        let n = Names::from_seed(12345);
        for value in [&n.world, &n.script_url] {
            assert!(value.starts_with("__"));
            assert!(
                value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{value} is not an identifier"
            );
        }
    }
}
