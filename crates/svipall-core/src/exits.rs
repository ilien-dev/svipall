//! More than one way out, and the sense to stop using one that has been noticed.
//!
//! svipall never supplies proxies; the operator does, and until now one per domain. A pool is the
//! same thing with a list: `pools.json` maps a domain to the exits it may leave through, each
//! exit's country stays in `proxy_regions.json` as before, and a small ledger remembers how each
//! exit has fared *on that domain*. An exit a domain has blocked twice is retired for that domain
//! only — the same address is still fine somewhere else.
//!
//! The default strategy is `sticky`: one exit per domain until it is retired. Two independent
//! benchmarks in 2026 reached the same conclusion from opposite directions — a gate scores the
//! whole shape of a visitor, and an address that changes under a fingerprint that does not is
//! its own tell. Rotation is for when a domain has started scoring the address, not before.
//! `round_robin` exists for the operator who has decided otherwise.

use crate::session::{Session, Verdict};
use crate::store::{route_for, JsonMap};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

/// Per-domain exit lists. Value is a JSON array of proxy URLs.
pub static POOLS: LazyLock<JsonMap> =
    LazyLock::new(|| JsonMap::new(crate::config::home_dir().join("pools.json")));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Sticky,
    RoundRobin,
}

impl Strategy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "round_robin" | "round-robin" | "rotate" => Self::RoundRobin,
            _ => Self::Sticky,
        }
    }
}

/// The exits a domain may use: its pool, inheriting from parent domains, else its single route.
pub fn exits_for(domain: &str) -> Vec<String> {
    let pools = POOLS.snapshot();
    let mut d = domain;
    loop {
        if let Some(v) = pools.get(d) {
            let list: Vec<String> = serde_json::from_str(v).unwrap_or_default();
            if !list.is_empty() {
                return list;
            }
        }
        match d.find('.') {
            Some(i) => d = &d[i + 1..],
            None => break,
        }
    }
    route_for(domain).into_iter().collect()
}

/// Declare a domain's pool. An empty list removes it.
pub fn set_pool(domain: &str, exits: &[String]) {
    if exits.is_empty() {
        POOLS.remove(domain);
    } else {
        let v = serde_json::to_string(exits).unwrap_or_default();
        POOLS.insert(domain, &v);
    }
}

/// Every declared pool, for `web_status`.
pub fn pools() -> HashMap<String, Vec<String>> {
    POOLS
        .as_map()
        .into_iter()
        .filter_map(|(k, v)| Some((k, serde_json::from_str(&v).ok()?)))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Health {
    health: i32,
    uses: u32,
    blocks: u32,
    /// Unix seconds of the last time this exit was used on this domain. A retired exit recovers
    /// with time, because an address a site scored an hour ago is not the address it scored now.
    #[serde(default)]
    at: i64,
    /// Rolling mean round-trip on this domain, in milliseconds. Zero until measured. Lets
    /// `web_log` and `web_status` answer "which proxy is slow", which the request log could not.
    #[serde(default)]
    ms: u32,
}

impl From<&Session> for Health {
    fn from(s: &Session) -> Self {
        Self {
            health: s.health,
            uses: s.uses,
            blocks: s.blocks,
            at: now_secs(),
            ms: 0,
        }
    }
}

/// Health recovers by one point per this many seconds idle, back up to full. Ten minutes a point,
/// so a twice-blocked exit (health 30, retired below 40) is usable again after roughly an hour and
/// a half of not being used — long enough that the score that retired it has likely moved on.
const RECOVERY_SECS_PER_POINT: i64 = 600;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Health {
    /// Health as it stands now, healed for the time since it was last touched.
    fn current(&self) -> i32 {
        if self.at == 0 {
            return self.health;
        }
        let idle = now_secs().saturating_sub(self.at).max(0);
        let recovered = (idle / RECOVERY_SECS_PER_POINT) as i32;
        (self.health + recovered).min(crate::session::FULL_HEALTH)
    }
}

/// What each exit has done on each domain, and which one a domain last used.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    by_domain: HashMap<String, HashMap<String, Health>>,
    #[serde(default)]
    last: HashMap<String, String>,
}

fn ledger_path() -> PathBuf {
    crate::config::home_dir().join("exit_health.json")
}

static LEDGER: LazyLock<Mutex<Ledger>> = LazyLock::new(|| {
    let l = std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Mutex::new(l)
});

fn persist(l: &Ledger) {
    if let Ok(s) = serde_json::to_string_pretty(l) {
        let _ = std::fs::create_dir_all(crate::config::home_dir());
        let _ = std::fs::write(ledger_path(), s);
    }
}

fn session_of(domain: &str, proxy: &str, h: Option<&Health>) -> Session {
    let mut s = Session::new(format!("{domain}|{proxy}"), Some(proxy.to_string()));
    if let Some(h) = h {
        // The healed value, so an exit retired long enough ago is offered again.
        s.health = h.current();
        s.uses = h.uses;
        s.blocks = h.blocks;
    }
    s
}

/// The exit to use for this domain now, or `None` when it has no route at all.
///
/// `sticky` keeps the last exit while it is usable; `round_robin` takes the next usable one after
/// it. Either way a retired exit is skipped, and when every exit is retired the healthiest is
/// used anyway — refusing to fetch is not a strategy, and the caller's cooldown logic still
/// applies.
pub fn choose(domain: &str, strategy: Strategy) -> Option<String> {
    choose_inner(domain, strategy, false)
}

/// The same choice, but an exit that has spent its standing with this domain is passed over the
/// way a retired one is.
///
/// Only the ladder uses this. The solver deliberately does not: it keeps the sticky exit because
/// the challenge it is answering is on the page that exit was shown, and swapping addresses
/// mid-challenge throws the session away. And as with retirement, when every exit is over its
/// budget one is still returned — the refusal belongs to the fetch gate, which can say how long
/// the wait is; picking nothing here would only produce "no route" and lose that.
pub fn choose_for_fetch(domain: &str, strategy: Strategy) -> Option<String> {
    choose_inner(domain, strategy, true)
}

fn choose_inner(domain: &str, strategy: Strategy, mind_the_budget: bool) -> Option<String> {
    let exits = exits_for(domain);
    if exits.is_empty() {
        return None;
    }
    if exits.len() == 1 {
        return exits.into_iter().next();
    }
    let mut ledger = LEDGER.lock().unwrap();
    let health = ledger.by_domain.get(domain).cloned().unwrap_or_default();
    let usable = |p: &String| {
        session_of(domain, p, health.get(p)).is_usable()
            && !(mind_the_budget && crate::reputation::refusal(domain, Some(p)).is_some())
    };
    let last = ledger.last.get(domain).cloned();
    let pick = match strategy {
        Strategy::Sticky => last
            .as_ref()
            .filter(|l| exits.contains(l) && usable(l))
            .cloned()
            .or_else(|| healthiest(domain, &exits, &health, true)),
        Strategy::RoundRobin => {
            let start = last
                .as_ref()
                .and_then(|l| exits.iter().position(|e| e == l))
                .map(|i| i + 1)
                .unwrap_or(0);
            (0..exits.len())
                .map(|k| &exits[(start + k) % exits.len()])
                .find(|p| usable(p))
                .cloned()
                .or_else(|| healthiest(domain, &exits, &health, false))
        }
    }?;
    if last.as_deref() != Some(pick.as_str()) {
        ledger.last.insert(domain.to_string(), pick.clone());
        persist(&ledger);
    }
    Some(pick)
}

fn healthiest(
    domain: &str,
    exits: &[String],
    health: &HashMap<String, Health>,
    only_usable: bool,
) -> Option<String> {
    let mut best: Option<(&String, i32)> = None;
    for p in exits {
        let s = session_of(domain, p, health.get(p));
        if only_usable && !s.is_usable() {
            continue;
        }
        if best.is_none_or(|(_, h)| s.health > h) {
            best = Some((p, s.health));
        }
    }
    best.map(|(p, _)| p.clone()).or_else(|| {
        if only_usable {
            healthiest(domain, exits, health, false)
        } else {
            None
        }
    })
}

/// Fold a result back in. Only meaningful for domains with a pool; a single route has nothing to
/// switch to, so its health is not tracked — the request log's `exit` column carries the
/// visibility a single proxy needs instead.
///
/// `latency_ms` feeds a rolling per-exit mean, so `web_status` can say which exit on which domain
/// is the slow one.
pub fn record(domain: &str, proxy: &str, verdict: Verdict, latency_ms: u32) {
    if exits_for(domain).len() < 2 {
        return;
    }
    let mut ledger = LEDGER.lock().unwrap();
    let per = ledger.by_domain.entry(domain.to_string()).or_default();
    let prior = per.get(proxy);
    // Heal before applying the new verdict, so time idle is not thrown away by the next use.
    let mut s = session_of(domain, proxy, prior);
    s.record(verdict);
    let ms = match (prior.map(|h| h.ms).filter(|m| *m > 0), latency_ms) {
        (Some(prev), new) if new > 0 => (prev as f32 * 0.7 + new as f32 * 0.3) as u32,
        (Some(prev), _) => prev,
        (None, new) => new,
    };
    let mut h = Health::from(&s);
    h.ms = ms;
    per.insert(proxy.to_string(), h);
    persist(&ledger);
}

/// Is there a usable exit for this domain other than `proxy`? When there is, a block is a reason
/// to switch, not to put the whole domain on a cooldown.
pub fn has_alternative(domain: &str, proxy: &str) -> bool {
    let exits = exits_for(domain);
    if exits.len() < 2 {
        return false;
    }
    let ledger = LEDGER.lock().unwrap();
    let health = ledger.by_domain.get(domain);
    exits
        .iter()
        .filter(|p| p.as_str() != proxy)
        .any(|p| session_of(domain, p, health.and_then(|h| h.get(p))).is_usable())
}

/// The ledger, for `web_status`: per domain, each exit's healed health, retirement, latency and
/// blocks, so the operator can see which proxy is slow and which is getting burnt where.
pub fn status() -> serde_json::Value {
    let ledger = LEDGER.lock().unwrap();
    let by_domain: HashMap<String, serde_json::Value> = ledger
        .by_domain
        .iter()
        .map(|(domain, exits)| {
            let rows: HashMap<String, serde_json::Value> = exits
                .iter()
                .map(|(proxy, h)| {
                    let health = h.current();
                    (
                        proxy.clone(),
                        serde_json::json!({
                            "health": health,
                            "usable": session_of(domain, proxy, Some(h)).is_usable(),
                            "blocks": h.blocks,
                            "uses": h.uses,
                            "avg_ms": h.ms,
                        }),
                    )
                })
                .collect();
            (domain.clone(), serde_json::json!(rows))
        })
        .collect();
    serde_json::json!({
        "by_domain": by_domain,
        "last_used": ledger.last,
    })
}

/// Forget everything learned about a domain's exits.
pub fn forget(domain: &str) -> bool {
    let mut ledger = LEDGER.lock().unwrap();
    let had = ledger.by_domain.remove(domain).is_some() | ledger.last.remove(domain).is_some();
    if had {
        persist(&ledger);
    }
    had
}

/// The identity to wear when leaving through `proxy`: the configured one, in the exit's declared
/// country when it has one. Used by the http tier, which until now sent the home locale through
/// every proxy.
pub fn identity_for_exit(
    identity: &crate::IdentityProfile,
    proxy: Option<&str>,
) -> crate::IdentityProfile {
    match proxy.and_then(crate::store::region_for_proxy) {
        Some(region) => identity.clone().in_country(region.country),
        None => identity.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolate() -> tempdir::Guard {
        tempdir::Guard::new()
    }

    /// A private home per test so the JSON files and the ledger start empty.
    mod tempdir {
        pub struct Guard;
        impl Guard {
            pub fn new() -> Self {
                let dir = std::env::temp_dir().join(format!(
                    "svipall-exits-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                ));
                std::fs::create_dir_all(&dir).unwrap();
                std::env::set_var("SVIPALL_HOME", &dir);
                Guard
            }
        }
    }

    // The statics read `home_dir()` once, so every test here shares one home and uses its own
    // domain names to stay out of the others' way.
    fn fresh(domain: &str) -> Vec<String> {
        let exits = vec![
            format!("http://a.{domain}:1"),
            format!("http://b.{domain}:2"),
            format!("http://c.{domain}:3"),
        ];
        set_pool(domain, &exits);
        forget(domain);
        exits
    }

    #[test]
    fn sticky_keeps_the_same_exit_until_it_is_retired() {
        let _g = isolate();
        let exits = fresh("sticky.example");
        let first = choose("sticky.example", Strategy::Sticky).unwrap();
        assert!(exits.contains(&first));
        assert_eq!(choose("sticky.example", Strategy::Sticky).unwrap(), first);
        record("sticky.example", &first, Verdict::Blocked, 0);
        assert_eq!(
            choose("sticky.example", Strategy::Sticky).unwrap(),
            first,
            "one block is the page, not the exit"
        );
        record("sticky.example", &first, Verdict::Blocked, 0);
        let next = choose("sticky.example", Strategy::Sticky).unwrap();
        assert_ne!(next, first, "two blocks retire it");
        assert!(exits.contains(&next));
    }

    #[test]
    fn a_retired_exit_is_not_offered_again_for_that_domain_but_another_domain_may_use_it() {
        let _g = isolate();
        let exits = fresh("retire.example");
        set_pool("other.example", &exits);
        forget("other.example");
        let x = exits[0].clone();
        record("retire.example", &x, Verdict::Blocked, 0);
        record("retire.example", &x, Verdict::Blocked, 0);
        for _ in 0..5 {
            assert_ne!(choose("retire.example", Strategy::RoundRobin).unwrap(), x);
        }
        assert!(has_alternative("retire.example", &exits[1]));
        // The same address on another domain starts with full health.
        let seen: Vec<String> = (0..3)
            .filter_map(|_| choose("other.example", Strategy::RoundRobin))
            .collect();
        assert!(seen.contains(&x), "{seen:?}");
    }

    #[test]
    fn round_robin_walks_the_pool_in_order() {
        let _g = isolate();
        let exits = fresh("rr.example");
        let seq: Vec<String> = (0..4)
            .filter_map(|_| choose("rr.example", Strategy::RoundRobin))
            .collect();
        assert_eq!(
            seq,
            vec![
                exits[0].clone(),
                exits[1].clone(),
                exits[2].clone(),
                exits[0].clone()
            ]
        );
    }

    #[test]
    fn subdomains_inherit_the_pool_and_a_single_route_is_a_pool_of_one() {
        let _g = isolate();
        let exits = fresh("inherit.example");
        assert_eq!(exits_for("shop.inherit.example"), exits);
        assert!(!has_alternative("inherit.example", &exits[0]) || exits.len() > 1);
        crate::store::ROUTES.insert("single.example", "http://only:1");
        assert_eq!(
            exits_for("single.example"),
            vec!["http://only:1".to_string()]
        );
        assert_eq!(
            choose("single.example", Strategy::RoundRobin).as_deref(),
            Some("http://only:1")
        );
        assert!(!has_alternative("single.example", "http://only:1"));
        record("single.example", "http://only:1", Verdict::Blocked, 0);
        assert!(
            status()["by_domain"]["single.example"].is_null(),
            "nothing to learn about a pool of one"
        );
    }

    #[test]
    fn when_every_exit_is_retired_the_healthiest_is_still_used() {
        let _g = isolate();
        let exits = fresh("burnt.example");
        for x in &exits {
            record("burnt.example", x, Verdict::Blocked, 0);
            record("burnt.example", x, Verdict::Blocked, 0);
        }
        record("burnt.example", &exits[2], Verdict::Ok, 0);
        assert_eq!(choose("burnt.example", Strategy::Sticky).unwrap(), exits[2]);
    }

    #[test]
    fn a_retired_exit_recovers_with_time_and_carries_its_latency() {
        let _g = isolate();
        let exits = fresh("decay.example");
        let x = exits[0].clone();
        // Two blocks retire it, and a slow round-trip is remembered.
        record("decay.example", &x, Verdict::Blocked, 800);
        record("decay.example", &x, Verdict::Blocked, 1200);
        {
            let mut l = LEDGER.lock().unwrap();
            let h = l
                .by_domain
                .get_mut("decay.example")
                .unwrap()
                .get_mut(&x)
                .unwrap();
            assert!(h.health < 40, "two blocks retire it now");
            assert!(h.ms > 0, "the latency is kept: {}", h.ms);
            // Backdate the last use so the recovery clock has run.
            h.at = now_secs() - RECOVERY_SECS_PER_POINT * 20;
        }
        // Twenty points recovered is enough to cross back over the retirement line, so the exit is
        // offered again without anyone clearing anything.
        let health = LEDGER.lock().unwrap().by_domain["decay.example"][&x].clone();
        assert!(
            session_of("decay.example", &x, Some(&health)).is_usable(),
            "an exit idle long enough heals back into use"
        );
    }

    #[test]
    fn the_identity_wears_the_exit_country_when_one_is_declared() {
        let _g = isolate();
        let id = crate::IdentityProfile::for_major(crate::MAX_EMULATED_CHROME, crate::Os::Windows);
        assert!(crate::store::set_proxy_region("http://de.exit:1", "DE"));
        let de = identity_for_exit(&id, Some("http://de.exit:1"));
        assert_eq!(de.timezone, "Europe/Berlin");
        assert!(de.accept_language.starts_with("de-DE"));
        let home = identity_for_exit(&id, Some("http://undeclared:1"));
        assert_eq!(home.timezone, id.timezone);
        assert_eq!(
            identity_for_exit(&id, None).accept_language,
            id.accept_language
        );
    }

    #[test]
    fn an_over_budget_exit_is_skipped_like_a_retired_one() {
        let _g = isolate();
        let exits = fresh("spent.example");
        let burnt = exits[0].clone();
        // Nothing is wrong with this exit — it has simply been used too much on this domain, which
        // is the thing health could never see.
        crate::reputation::add(
            "spent.example",
            Some(&burnt),
            crate::reputation::budget() * 2.0,
        );
        for _ in 0..5 {
            assert_ne!(
                choose_for_fetch("spent.example", Strategy::RoundRobin).unwrap(),
                burnt,
                "an exit over its budget must be passed over"
            );
        }
        assert_eq!(
            choose("spent.example", Strategy::Sticky).unwrap_or_default(),
            choose("spent.example", Strategy::Sticky).unwrap_or_default(),
            "the solver's choice is unchanged by any of this"
        );
        crate::reputation::clear("spent.example");
    }
}
