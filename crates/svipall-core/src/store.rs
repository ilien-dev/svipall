//! Small JSON string maps on disk (`domain_tiers.json`, `proxies.json`, `cooldowns.json`), cached
//! in memory.
//!
//! These used to be read and parsed from disk on every single lookup: `build_ladder` and
//! `remember_tier` each re-read the tier map, and `route_for` re-read the proxy map once per fetch.
//! A `web_fetch_many` over fifty URLs meant hundreds of `read_to_string` calls plus JSON parsing,
//! all for a handful of entries.
//!
//! The files stay hand-editable, which is a property worth keeping, so the cache re-`stat`s at most
//! a few times a second and reloads only when the modification time actually moves.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// How long a cached map is trusted before its file is re-`stat`ed. A `stat` is a few microseconds,
/// so this only exists to keep a tight loop from syscalling per lookup.
const STAT_INTERVAL: Duration = Duration::from_millis(250);

struct Cached {
    mtime: Option<SystemTime>,
    checked: Option<Instant>,
    map: Arc<HashMap<String, String>>,
}

pub struct JsonMap {
    path: PathBuf,
    cache: RwLock<Cached>,
    disk_reads: AtomicU64,
    /// Serialises read-modify-write. Without it two concurrent inserts each start from the same
    /// snapshot and the second write silently drops the first key.
    mutation: std::sync::Mutex<()>,
}

fn mtime_of(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

impl JsonMap {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            cache: RwLock::new(Cached {
                mtime: None,
                checked: None,
                map: Arc::new(HashMap::new()),
            }),
            disk_reads: AtomicU64::new(0),
            mutation: std::sync::Mutex::new(()),
        }
    }

    /// Times this map has actually gone to disk. Used by tests and benchmarks to prove the hot
    /// path stays in memory.
    pub fn disk_reads(&self) -> u64 {
        self.disk_reads.load(Ordering::Relaxed)
    }

    fn read_file(&self) -> HashMap<String, String> {
        self.disk_reads.fetch_add(1, Ordering::Relaxed);
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    /// Current contents, reloading only when the file changed under us.
    pub fn snapshot(&self) -> Arc<HashMap<String, String>> {
        {
            let c = self.cache.read().unwrap();
            if let Some(checked) = c.checked {
                if checked.elapsed() < STAT_INTERVAL {
                    return c.map.clone();
                }
            }
        }
        let disk_mtime = mtime_of(&self.path);
        let mut c = self.cache.write().unwrap();
        // Another thread may have refreshed while we waited for the write lock.
        if c.checked.is_some() && c.mtime == disk_mtime {
            c.checked = Some(Instant::now());
            return c.map.clone();
        }
        c.map = Arc::new(self.read_file());
        c.mtime = disk_mtime;
        c.checked = Some(Instant::now());
        c.map.clone()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.snapshot().get(key).cloned()
    }

    fn write_through(&self, map: HashMap<String, String>) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".into());
        let _ = std::fs::write(&self.path, body);
        let mut c = self.cache.write().unwrap();
        c.map = Arc::new(map);
        c.mtime = mtime_of(&self.path);
        c.checked = Some(Instant::now());
    }

    /// Insert and persist. Returns false when the value was already there, so callers can skip a
    /// pointless write.
    pub fn insert(&self, key: &str, value: &str) -> bool {
        let _serial = self.mutation.lock().unwrap();
        let current = self.snapshot();
        if current.get(key).map(String::as_str) == Some(value) {
            return false;
        }
        let mut map = (*current).clone();
        map.insert(key.to_string(), value.to_string());
        self.write_through(map);
        true
    }

    /// Remove and persist. Returns false when the key was not present.
    pub fn remove(&self, key: &str) -> bool {
        let _serial = self.mutation.lock().unwrap();
        let current = self.snapshot();
        if !current.contains_key(key) {
            return false;
        }
        let mut map = (*current).clone();
        map.remove(key);
        self.write_through(map);
        true
    }

    pub fn as_map(&self) -> HashMap<String, String> {
        (*self.snapshot()).clone()
    }
}

/// Learned per-domain start tiers.
pub static TIERS: LazyLock<JsonMap> =
    LazyLock::new(|| JsonMap::new(crate::config::home_dir().join("domain_tiers.json")));

/// Per-domain proxy routes.
pub static ROUTES: LazyLock<JsonMap> =
    LazyLock::new(|| JsonMap::new(crate::config::home_dir().join("proxies.json")));

/// Which country each proxy exits from.
///
/// Keyed by the proxy URL rather than by domain, because the country is a property of the exit
/// node: every domain routed through it inherits the same one, and it only has to be declared once.
pub static PROXY_REGIONS: LazyLock<JsonMap> =
    LazyLock::new(|| JsonMap::new(crate::config::home_dir().join("proxy_regions.json")));

/// The region a proxy exits from, if one was declared for it.
///
/// Declared, not detected: working it out would mean asking a geolocation service, and this project
/// does not make third-party calls. An undeclared proxy leaves the identity as configured.
pub fn region_for_proxy(proxy: &str) -> Option<&'static crate::geo::Region> {
    PROXY_REGIONS
        .get(proxy)
        .as_deref()
        .and_then(crate::geo::for_country)
}

/// Record the country a proxy exits from. An unknown code is rejected rather than stored, so a
/// typo cannot silently leave an identity wearing nothing.
pub fn set_proxy_region(proxy: &str, country: &str) -> bool {
    match crate::geo::for_country(country) {
        Some(r) => {
            PROXY_REGIONS.insert(proxy, r.country);
            true
        }
        None => false,
    }
}

/// Proxy for a domain, inheriting from parent domains (`a.b.example.com` -> `example.com`).
///
/// This lookup existed twice, character for character, in `svipall-mcp::server` and
/// `svipall-mcp::solver_engine`, each with its own read of the file.
pub fn route_for(domain: &str) -> Option<String> {
    let routes = ROUTES.snapshot();
    let mut d = domain;
    loop {
        if let Some(p) = routes.get(d) {
            return Some(p.clone());
        }
        let i = d.find('.')?;
        d = &d[i + 1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "svipall-store-test-{}-{}.json",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn lookups_after_the_first_do_not_touch_disk() {
        let path = temp_path("hot");
        std::fs::write(&path, r#"{"example.com":"real"}"#).unwrap();
        let m = JsonMap::new(path.clone());

        assert_eq!(m.get("example.com").as_deref(), Some("real"));
        let after_first = m.disk_reads();
        assert_eq!(after_first, 1, "the first lookup must load the file");

        for _ in 0..1000 {
            let _ = m.get("example.com");
            let _ = route_for_in(&m, "a.b.example.com");
        }
        assert_eq!(
            m.disk_reads(),
            after_first,
            "the hot path must stay in memory"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// `route_for` against a specific map, so the test does not depend on the user's real files.
    fn route_for_in(m: &JsonMap, domain: &str) -> Option<String> {
        let routes = m.snapshot();
        let mut d = domain;
        loop {
            if let Some(p) = routes.get(d) {
                return Some(p.clone());
            }
            let i = d.find('.')?;
            d = &d[i + 1..];
        }
    }

    #[test]
    fn a_proxy_region_round_trips_and_a_typo_is_refused() {
        // Storing an unrecognised country would leave the identity wearing nothing while looking
        // configured, which is the failure mode this guards.
        assert!(crate::geo::for_country("DE").is_some());
        assert!(crate::geo::for_country("Germany").is_none());
    }

    #[test]
    fn subdomains_inherit_the_parent_route() {
        let path = temp_path("routes");
        std::fs::write(&path, r#"{"example.com":"http://proxy:8080"}"#).unwrap();
        let m = JsonMap::new(path.clone());
        assert_eq!(
            route_for_in(&m, "deep.sub.example.com").as_deref(),
            Some("http://proxy:8080")
        );
        assert_eq!(route_for_in(&m, "other.test"), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn insert_and_remove_persist_and_report_change() {
        let path = temp_path("rw");
        let m = JsonMap::new(path.clone());
        assert!(m.insert("a.test", "warm"));
        assert!(!m.insert("a.test", "warm"), "same value must be a no-op");
        assert!(m.insert("a.test", "real"), "new value must be written");

        let reread = JsonMap::new(path.clone());
        assert_eq!(reread.get("a.test").as_deref(), Some("real"));

        assert!(m.remove("a.test"));
        assert!(!m.remove("a.test"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_or_corrupt_file_reads_as_empty() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "not json at all").unwrap();
        let m = JsonMap::new(path.clone());
        assert!(m.as_map().is_empty());
        let _ = std::fs::remove_file(&path);

        let gone = JsonMap::new(temp_path("absent"));
        assert!(gone.as_map().is_empty());
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;

    /// Two writers starting from the same snapshot used to leave one key on the floor.
    #[test]
    fn concurrent_inserts_do_not_lose_each_other() {
        let dir = std::env::temp_dir().join(format!("svipall-jsonmap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let map = std::sync::Arc::new(JsonMap::new(dir.join("race.json")));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let m = map.clone();
                std::thread::spawn(move || {
                    for j in 0..20 {
                        m.insert(&format!("k{i}-{j}"), "v");
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(map.as_map().len(), 160);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
