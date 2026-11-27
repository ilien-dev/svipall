//! Whether a route answers people or programs, learned from what it has served.
//!
//! The visit window, the minimum gap and the address budget were all sized for a page: a document
//! a site serves to a person, behind whatever scores the visitor. A registry, a metadata API, a
//! raw file host or a suggestion endpoint is not that. Nothing scores the visitor there, the
//! caller asks for thirty things at once because thirty is the question, and the host says so
//! itself with a 429 when it has had enough. Three quarters of the refusals in one installation's
//! request log were this tool pacing such hosts like a newspaper.
//!
//! No list of hosts. A route is in the data lane because it has answered with data — JSON, plain
//! text, source, a feed, a file — and it leaves the lane the first time it refuses, so a host that
//! does judge its visitors is paced as a page again. A route never seen is a page, unless every
//! route its host is known for is data.
//!
//! The lane changes three numbers and nothing else: more visits in the window, a shorter gap, a
//! smaller charge against the address. A server's own backoff holds both lanes alike.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

static MEMORY: LazyLock<crate::store::JsonMap> =
    LazyLock::new(|| crate::store::JsonMap::new(crate::config::home_dir().join("lanes.json")));
static UPDATE: Mutex<()> = Mutex::new(());

/// What a route was last seen serving is believed this long.
const TTL: u64 = 30 * 86_400;
/// Route families remembered per host. The oldest is dropped past this.
const MAX_FAMILIES: usize = 64;
/// Families a host must be known for, every one of them data, before a family never seen is
/// taken for data too.
const HOST_EVIDENCE: usize = 3;

/// Visits a data route may make in the window, as a multiple of `request_limit`.
pub const DATA_VISITS: u32 = 5;
/// The minimum gap on a data route, as a fraction of `request_min_interval_ms`.
pub const DATA_GAP_DIVISOR: u64 = 4;
/// What a visit to a data route costs the address, as a fraction of its tier's price.
pub const DATA_COST: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lane {
    /// A document for a person. The limits as configured.
    #[default]
    Page,
    /// Data for a program.
    Data,
}

impl Lane {
    /// Visits allowed in one window.
    pub fn limit(self, cfg: &crate::Config) -> u32 {
        match self {
            Self::Page => cfg.request_limit,
            Self::Data => cfg.request_limit.saturating_mul(DATA_VISITS),
        }
    }

    /// The least time between two visits.
    pub fn min_interval_ms(self, cfg: &crate::Config) -> u64 {
        match self {
            Self::Page => cfg.request_min_interval_ms,
            Self::Data => (cfg.request_min_interval_ms / DATA_GAP_DIVISOR).max(1),
        }
    }

    /// The share of a tier's price one visit is charged.
    pub fn cost(self) -> f32 {
        match self {
            Self::Page => 1.0,
            Self::Data => DATA_COST,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Data => "data",
        }
    }
}

/// What one visit showed about its route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Served {
    /// JSON, text, source, a feed, a file.
    Data,
    /// A rendered document.
    Page,
    /// A refusal or a wall: whatever this route serves, it judges who asks.
    Refused,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
struct Seen {
    data: bool,
    at: u64,
}

/// What is known of one host: its route families and what each last served.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Host {
    families: BTreeMap<String, Seen>,
}

impl Host {
    pub fn observe(&mut self, family: &str, served: Served, now: u64) {
        self.families.retain(|_, s| now.saturating_sub(s.at) <= TTL);
        self.families.insert(
            family.to_string(),
            Seen {
                data: served == Served::Data,
                at: now,
            },
        );
        while self.families.len() > MAX_FAMILIES {
            let oldest = self
                .families
                .iter()
                .min_by_key(|(_, s)| s.at)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.families.remove(&k),
                None => break,
            };
        }
    }

    pub fn lane(&self, family: &str, now: u64) -> Lane {
        self.known(family, now).unwrap_or_default()
    }

    /// The lane, when there is evidence for one: `None` for a route nobody has seen answer, on
    /// a host that has not shown itself to be all data.
    pub fn known(&self, family: &str, now: u64) -> Option<Lane> {
        let fresh = |s: &&Seen| now.saturating_sub(s.at) <= TTL;
        if let Some(seen) = self.families.get(family).filter(fresh) {
            return Some(if seen.data { Lane::Data } else { Lane::Page });
        }
        let known: Vec<&Seen> = self.families.values().filter(fresh).collect();
        (known.len() >= HOST_EVIDENCE && known.iter().all(|s| s.data)).then_some(Lane::Data)
    }
}

/// The host's key and the route family inside it. Digests, as `automatic` stores them: no URL,
/// query value or proxy credential is written down.
fn keys(url: &str, exit: Option<&str>) -> Option<(String, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let origin = parsed.origin().ascii_serialization();
    let family = parsed
        .path_segments()
        .and_then(|mut s| s.find(|p| !p.is_empty()))
        .unwrap_or("");
    let domain = crate::domain_from_url(url);
    let host = format!(
        "{:x}:{:x}",
        Sha256::digest(domain.as_bytes()),
        Sha256::digest(serde_json::to_vec(&(origin, exit)).unwrap_or_default())
    );
    let family = format!("{:x}", Sha256::digest(family.as_bytes()));
    Some((host, family[..16].to_string()))
}

fn load(key: &str) -> Host {
    MEMORY
        .get(key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// The lane this address is in, from what its route and its host have served before.
pub fn of(url: &str, exit: Option<&str>) -> Lane {
    known(url, exit).unwrap_or_default()
}

/// `of`, or `None` while nothing has been seen that decides it. A visit that finds a page's
/// window full may wait a moment for a first answer to decide, rather than be refused as a page.
pub fn known(url: &str, exit: Option<&str>) -> Option<Lane> {
    let (host, family) = keys(url, exit)?;
    load(&host).known(&family, crate::automatic::now())
}

/// Record what a visit showed. Written only when it changes what is known or the entry is a day
/// old, so a burst against one route is one write.
pub fn observe(url: &str, exit: Option<&str>, served: Served) {
    let Some((key, family)) = keys(url, exit) else {
        return;
    };
    let _lock = UPDATE.lock().unwrap();
    let now = crate::automatic::now();
    let mut host = load(&key);
    let data = served == Served::Data;
    if host
        .families
        .get(&family)
        .is_some_and(|s| s.data == data && now.saturating_sub(s.at) < 86_400)
    {
        return;
    }
    host.observe(&family, served, now);
    MEMORY.insert(&key, &serde_json::to_string(&host).unwrap_or_default());
}

/// Forget every lane learned for a domain.
pub fn forget(domain: &str) {
    let _lock = UPDATE.lock().unwrap();
    let prefix = format!("{:x}:", Sha256::digest(domain.as_bytes()));
    for key in MEMORY.snapshot().keys().filter(|k| k.starts_with(&prefix)) {
        MEMORY.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_is_a_page_until_it_has_served_data() {
        let mut host = Host::default();
        assert_eq!(host.lane("works", 100), Lane::Page);
        assert_eq!(host.known("works", 100), None, "a guess, not a finding");
        host.observe("works", Served::Data, 100);
        assert_eq!(host.lane("works", 101), Lane::Data);
        // Another family of the same host has shown nothing yet.
        assert_eq!(host.lane("docs", 101), Lane::Page);
    }

    #[test]
    fn a_refusal_puts_a_route_back_among_the_pages() {
        let mut host = Host::default();
        host.observe("works", Served::Data, 100);
        host.observe("works", Served::Refused, 200);
        assert_eq!(host.known("works", 201), Some(Lane::Page));
        host.observe("works", Served::Data, 300);
        assert_eq!(host.lane("works", 301), Lane::Data);
    }

    #[test]
    fn a_host_known_only_for_data_lends_its_lane_to_a_route_never_seen() {
        let mut host = Host::default();
        host.observe("a", Served::Data, 100);
        host.observe("b", Served::Data, 100);
        assert_eq!(host.lane("new", 101), Lane::Page, "two are not enough");
        host.observe("c", Served::Data, 100);
        assert_eq!(host.lane("new", 101), Lane::Data);
        // One page, or one refusal, and the host is no longer only data.
        host.observe("d", Served::Page, 102);
        assert_eq!(host.lane("new", 103), Lane::Page);
        assert_eq!(
            host.lane("a", 103),
            Lane::Data,
            "what was seen still stands"
        );
    }

    #[test]
    fn what_a_route_served_is_forgotten_after_a_month() {
        let mut host = Host::default();
        host.observe("works", Served::Data, 100);
        assert_eq!(host.lane("works", 100 + TTL), Lane::Data);
        assert_eq!(host.lane("works", 101 + TTL), Lane::Page);
    }

    #[test]
    fn a_host_keeps_a_bounded_number_of_families() {
        let mut host = Host::default();
        for i in 0..(MAX_FAMILIES as u64 + 10) {
            host.observe(&format!("f{i}"), Served::Data, 100 + i);
        }
        assert_eq!(host.families.len(), MAX_FAMILIES);
        assert!(!host.families.contains_key("f0"), "the oldest went first");
    }

    #[test]
    fn the_data_lane_scales_the_limits_an_operator_set() {
        let cfg = crate::Config {
            request_limit: 12,
            request_min_interval_ms: 1000,
            ..Default::default()
        };
        assert_eq!(Lane::Page.limit(&cfg), 12);
        assert_eq!(Lane::Data.limit(&cfg), 60);
        assert_eq!(Lane::Page.min_interval_ms(&cfg), 1000);
        assert_eq!(Lane::Data.min_interval_ms(&cfg), 250);
        assert!(Lane::Data.cost() < Lane::Page.cost());
        let tight = crate::Config {
            request_min_interval_ms: 1,
            ..Default::default()
        };
        assert_eq!(Lane::Data.min_interval_ms(&tight), 1, "never zero");
    }

    #[test]
    fn the_key_holds_no_address() {
        let (host, family) = keys(
            "https://api.example.test/works?mailto=me@example.test",
            None,
        )
        .unwrap();
        assert!(!host.contains("example") && !family.contains("works"));
        let (other, same_family) = keys("https://api.example.test/works/W1", None).unwrap();
        assert_eq!((host.clone(), family), (other, same_family));
        let (via_exit, _) = keys("https://api.example.test/works", Some("http://p:1")).unwrap();
        assert_ne!(host, via_exit, "an exit is another address");
    }
}
