//! Per-domain browser profiles. Port of server.py profile housekeeping.
use std::path::{Path, PathBuf};

use crate::config::home_dir;

pub fn auto_profiles_dir() -> PathBuf {
    home_dir().join("auto_profiles")
}

pub fn profiles_dir() -> PathBuf {
    home_dir().join("profiles")
}

pub fn screenshots_dir() -> PathBuf {
    home_dir().join("screenshots")
}

/// Per-domain browser profile path, optionally creating directory.
pub fn auto_profile_path(url: &str, create: bool) -> String {
    let domain = crate::domain::domain_from_url(url);
    let safe_domain = if domain.is_empty() {
        "unknown".to_string()
    } else {
        domain
    };
    let safe: String = safe_domain
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = auto_profiles_dir().join(safe);
    if create {
        let _ = std::fs::create_dir_all(&path);
    }
    path.to_string_lossy().to_string()
}

/// The file inside a browser profile that names the machine that profile wears.
///
/// Its *absence* is the signal that matters: a profile directory with browser state and no marker
/// predates the fleet, and it keeps the machine it has always reported. A profile whose hardware
/// changes between visits, with the same cookies still in it, has answered the question by itself.
const MACHINE_MARKER: &str = ".svipall-machine";

/// The random number that makes this installation's machines its own.
///
/// Without it the obvious derivation — hash the domain — would hand every installation of svipall
/// in the world the same machine for the same site. That is a short deck again, only wider: a
/// vendor collecting traffic would see one visitor per domain across every user of this tool.
/// Minted once, kept next to the profiles, and read from there ever after.
pub fn install_seed() -> u64 {
    static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *SEED.get_or_init(|| {
        let path = home_dir().join("machine.seed");
        if let Some(seed) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| u64::from_str_radix(t.trim(), 16).ok())
        {
            return seed;
        }
        // v4 is backed by the platform CSPRNG, which is the property wanted here.
        let seed = uuid::Uuid::new_v4().as_u128() as u64;
        let _ = std::fs::create_dir_all(home_dir());
        let _ = std::fs::write(&path, format!("{seed:016x}"));
        seed
    })
}

/// Mix an installation into a profile key. Split out from `machine_seed` so the mixing itself can
/// be tested without a home directory.
fn seed_with(install: u64, key: &str) -> u64 {
    crate::domain::stable_hash(&format!("{install:016x}|{key}"))
}

/// The machine this installation wears for `key` — a domain, or a named profile.
pub fn machine_seed(key: &str) -> u64 {
    seed_with(install_seed(), key)
}

/// The machine a profile wears, or `None` for one that predates the fleet.
///
/// The marker is read rather than recomputed, so a profile keeps its hardware even if the
/// installation seed is replaced or the domain is reached under another name. Tiers that keep no
/// profile on disk still get a machine; they just have nowhere to write it down, and deriving it
/// again from the same key gives the same answer.
pub fn seed_for_profile(dir: Option<&Path>, key: &str) -> Option<u64> {
    let Some(dir) = dir else {
        return Some(machine_seed(key));
    };
    let marker = dir.join(MACHINE_MARKER);
    if let Some(seed) = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|t| u64::from_str_radix(t.trim(), 16).ok())
    {
        return Some(seed);
    }
    // Browser state with no marker: this profile has been somewhere, wearing the machine that was
    // compiled in. It keeps it. Retiring the profile is what lets it be born again into the fleet,
    // and that path already exists.
    let inhabited = std::fs::read_dir(dir).is_ok_and(|mut e| {
        e.any(|entry| {
            entry
                .map(|entry| entry.file_name() != MACHINE_MARKER)
                .unwrap_or(false)
        })
    });
    if inhabited {
        return None;
    }
    let seed = machine_seed(key);
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&marker, format!("{seed:016x}"));
    Some(seed)
}

/// The machine to wear for a fetch, given where it goes and which profile it goes as.
///
/// The key is the same one the cookie jar is filed under — a named profile if there is one, the
/// domain otherwise — so hardware and cookies can never travel apart. That is the whole rule: a
/// site that recognises the visitor must recognise the machine too.
pub fn identity_seed_for(dir: Option<&Path>, url: &str, profile: Option<&str>) -> Option<u64> {
    let key = match profile {
        Some(name) => name.to_string(),
        None => crate::domain::domain_from_url(url),
    };
    seed_for_profile(dir, &key)
}

const PROFILE_CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "ShaderCache",
    "GrShaderCache",
    "component_crx_cache",
    "extensions_crx_cache",
];

const MAX_PROFILES: usize = 60;

pub fn prune_profile_cache(profile: Option<&str>) {
    let Some(p) = profile else { return };
    let root = PathBuf::from(p);
    for parent in [root.clone(), root.join("Default")] {
        for name in PROFILE_CACHE_DIRS {
            let target = parent.join(name);
            if target.is_dir() {
                let _ = std::fs::remove_dir_all(&target);
            }
        }
    }
}

pub fn evict_old_profiles() {
    let dir = auto_profiles_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut profiles: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    if profiles.len() <= MAX_PROFILES {
        return;
    }
    profiles.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    for stale in profiles.iter().take(profiles.len() - MAX_PROFILES) {
        let _ = std::fs::remove_dir_all(stale);
    }
}

pub fn ensure_dirs() {
    let _ = std::fs::create_dir_all(home_dir());
    let _ = std::fs::create_dir_all(auto_profiles_dir());
    let _ = std::fs::create_dir_all(profiles_dir());
    let _ = std::fs::create_dir_all(screenshots_dir());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that cleans up after itself, so the disk tests never share state.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "svipall-profiles-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_named_profile_carries_its_machine_across_sites() {
        // The name is the visitor; the URL is only where they went. Two sites under one named
        // profile are one person, and one person is one machine.
        assert_eq!(
            identity_seed_for(None, "https://a.example/x", Some("shopper")),
            identity_seed_for(None, "https://b.example/y", Some("shopper"))
        );
    }

    #[test]
    fn without_a_name_the_machine_follows_the_domain() {
        // The same key the cookie jar uses, so hardware and cookies never travel apart.
        assert_eq!(
            identity_seed_for(None, "https://www.example.com/a?b=c", None),
            Some(machine_seed("example.com"))
        );
        assert_ne!(
            identity_seed_for(None, "https://example.com/", None),
            identity_seed_for(None, "https://example.org/", None)
        );
    }
    #[test]
    fn the_same_key_is_always_the_same_machine() {
        // A domain that comes back tomorrow has to be the machine it was today.
        assert_eq!(machine_seed("example.com"), machine_seed("example.com"));
    }

    #[test]
    fn different_keys_are_different_machines() {
        assert_ne!(machine_seed("example.com"), machine_seed("example.org"));
    }

    #[test]
    fn two_installations_disagree_about_the_same_domain() {
        // The whole point of an installation seed: a vendor collecting traffic must not see one
        // machine per domain shared by every user of this tool.
        assert_ne!(seed_with(1, "example.com"), seed_with(2, "example.com"));
    }

    #[test]
    fn a_profile_that_predates_the_fleet_keeps_its_machine() {
        // It already has cookies. Same cookie, new hardware, is the question answered for free.
        let dir = scratch("legacy");
        std::fs::write(dir.join("Local State"), "{}").unwrap();
        assert_eq!(seed_for_profile(Some(&dir), "example.com"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fresh_profile_is_minted_and_remembered() {
        let dir = scratch("fresh");
        let first = seed_for_profile(Some(&dir), "example.com").expect("a fresh profile is minted");
        assert!(
            dir.join(MACHINE_MARKER).exists(),
            "the machine was not written down"
        );
        // Written down, so it survives a change of installation seed.
        assert_eq!(seed_for_profile(Some(&dir), "other.example"), Some(first));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_profile_directory_that_does_not_exist_yet_is_fresh() {
        let parent = scratch("unborn");
        let dir = parent.join("nested");
        assert!(seed_for_profile(Some(&dir), "example.com").is_some());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn a_tier_without_a_profile_still_gets_a_machine() {
        // The cheap tiers keep nothing on disk; they still must not all wear one machine.
        assert_eq!(
            seed_for_profile(None, "example.com"),
            Some(machine_seed("example.com"))
        );
    }
}
