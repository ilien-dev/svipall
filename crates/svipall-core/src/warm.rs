//! Pages kept open between fetches, and knowing when not to.
//!
//! One vendor's clearance is not a cookie. Its SDK recomputes a value per request inside the live
//! page, so closing the tab throws the clearance away and the next fetch to that domain pays the
//! whole cost again — the profile directory cannot help, because there is nothing in it to keep.
//!
//! What is here is the **policy**, and nothing else: which page to hand back, when it is too old to
//! be worth having, when the cap says to let one go, and when a page has been refused often enough
//! that it is worse than a fresh one. All of it is generic over what a page *is*, so every rule is
//! tested against a number instead of Chromium — the same reason `solve_loop` is tested against a
//! fake surface. The tab itself lives in the browser pool.
//!
//! Two rules earn their own sentence.
//!
//! **Taking a page removes it.** Fetches run in parallel, and two of them driving one tab would be
//! a disaster. Removing on acquire makes exclusivity structural rather than remembered: the second
//! caller finds nothing and opens its own.
//!
//! **`now` is always a parameter.** Never `Instant::now()` inside — that is what makes ageing and
//! expiry testable without sleeping, the same discipline `warm_needs_reissue` and `Health::current`
//! already follow.

use crate::session::{Session, Verdict};
use std::time::{Duration, Instant};

/// A recognised proof-of-work path needs time to reach the one-renewal threshold and receive
/// the resulting page. Other walls start with the ordinary budget and extend only on progress.
pub fn wait_budget_ms(base: u64, maximum: u64, adaptive: bool, proof_of_work: bool) -> u64 {
    if adaptive && proof_of_work {
        base.max(crate::classify::POW_TOKEN_LIFETIME_SECS * 1000 + 10_000)
            .min(maximum)
    } else {
        base
    }
}

struct Slot<T> {
    key: String,
    value: T,
    used: Instant,
    /// The shared notion of "spent", not a second one. A page refused twice is retired on exactly
    /// the rule an exit is retired on.
    health: Session,
}

/// The pages currently being held, oldest use first.
pub struct Kept<T> {
    max: usize,
    ttl: Duration,
    slots: Vec<Slot<T>>,
}

impl<T> Kept<T> {
    /// `max` of zero keeps nothing at all — the off switch, and the control arm of any measurement
    /// of whether keeping pages was worth it.
    pub fn new(max: usize, ttl: Duration) -> Self {
        Self {
            max,
            ttl,
            slots: Vec::new(),
        }
    }

    /// Park a page. Returns everything the caller must now close: the evictions the cap forced, and
    /// the page itself when nothing is being kept.
    pub fn park(&mut self, key: impl Into<String>, value: T, now: Instant) -> Vec<T> {
        if self.max == 0 {
            return vec![value];
        }
        let key = key.into();
        let mut freed: Vec<T> = self
            .slots
            .iter()
            .position(|s| s.key == key)
            .map(|i| self.slots.remove(i).value)
            .into_iter()
            .collect();
        self.slots.push(Slot {
            key,
            value,
            used: now,
            health: Session::new("kept", None),
        });
        while self.slots.len() > self.max {
            // Least recently used: the domain being worked right now is the one to keep.
            let oldest = self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.used)
                .map(|(i, _)| i);
            match oldest {
                Some(i) => freed.push(self.slots.remove(i).value),
                None => break,
            }
        }
        freed
    }

    /// Hand back the page for this key, removing it. `None` when there is none, when it has aged
    /// out, or when it has been refused too often to be worth reusing.
    pub fn take(&mut self, key: &str, now: Instant) -> Option<T> {
        let i = self.slots.iter().position(|s| s.key == key)?;
        let fresh = now.duration_since(self.slots[i].used) < self.ttl;
        let usable = self.slots[i].health.is_usable();
        if !fresh || !usable {
            return None;
        }
        let mut slot = self.slots.remove(i);
        slot.used = now;
        Some(slot.value)
    }

    /// Everything too old to hand out. Removed here, so a page that expired while nobody asked for
    /// it is still closed rather than held until the process ends.
    pub fn expire(&mut self, now: Instant) -> Vec<T> {
        let ttl = self.ttl;
        Self::drain(&mut self.slots, |s| now.duration_since(s.used) >= ttl)
    }

    /// Everything whose key the predicate names — how a browser being reaped, a profile being
    /// retired, or a domain being reset takes its pages with it.
    pub fn drain_where(&mut self, f: impl Fn(&str) -> bool) -> Vec<T> {
        Self::drain(&mut self.slots, |s| f(&s.key))
    }

    /// Record what a fetch on this page came back with. `Some` when that retired it, and the caller
    /// must close what it hands back.
    pub fn record(&mut self, key: &str, v: Verdict) -> Option<T> {
        let i = self.slots.iter().position(|s| s.key == key)?;
        self.slots[i].health.record(v);
        (!self.slots[i].health.is_usable()).then(|| self.slots.remove(i).value)
    }

    /// The keys being held, for the status report.
    pub fn keys(&self) -> Vec<&str> {
        self.slots.iter().map(|s| s.key.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn drain(slots: &mut Vec<Slot<T>>, f: impl Fn(&Slot<T>) -> bool) -> Vec<T> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < slots.len() {
            if f(&slots[i]) {
                out.push(slots.remove(i).value);
            } else {
                i += 1;
            }
        }
        out
    }
}

/// Is this finished fetch worth keeping its page open for?
///
/// Four conditions, and every one of them is a refusal waiting to happen if dropped:
///
/// * a browser tier, because the http tier has no page to keep;
/// * not isolated and not wearing a borrowed machine — an isolated fetch promises to leave nothing
///   behind, and a page held open is something behind;
/// * the page actually **cleared**, because a page that never got through is not worth returning to;
/// * the clearance is one only a live runtime can hold. This is the narrow gate, and it is what
///   stops "every fetch leaks a tab".
pub fn should_keep(
    browser_tier: bool,
    isolated: bool,
    borrowed_machine: bool,
    cleared: bool,
    runtime_clearance: bool,
) -> bool {
    browser_tier && !isolated && !borrowed_machine && cleared && runtime_clearance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_has_time_but_never_exceeds_the_configured_budget() {
        assert_eq!(wait_budget_ms(20_000, 55_000, true, true), 50_000);
        assert_eq!(wait_budget_ms(20_000, 30_000, true, true), 30_000);
        assert_eq!(wait_budget_ms(20_000, 55_000, true, false), 20_000);
        assert_eq!(wait_budget_ms(20_000, 55_000, false, true), 20_000);
    }

    const TTL: Duration = Duration::from_secs(120);

    fn kept(max: usize) -> Kept<u32> {
        Kept::new(max, TTL)
    }

    #[test]
    fn a_parked_page_has_exactly_one_user_at_a_time() {
        // Fetches run in parallel. Two of them driving one tab is the failure this prevents.
        let t = Instant::now();
        let mut k = kept(2);
        assert!(k.park("a", 1, t).is_empty());
        assert_eq!(k.take("a", t), Some(1));
        assert_eq!(k.take("a", t), None, "handed out twice");
    }

    #[test]
    fn a_page_parked_too_long_ago_is_never_handed_out() {
        let t = Instant::now();
        let mut k = kept(2);
        k.park("a", 1, t);
        assert_eq!(k.take("a", t + TTL - Duration::from_secs(1)), Some(1));

        k.park("a", 1, t);
        assert_eq!(k.take("a", t + TTL), None, "a stale page was handed out");
        // And it comes back from the sweep, so it is closed rather than held forever.
        assert_eq!(k.expire(t + TTL), vec![1]);
        assert!(k.is_empty());
    }

    #[test]
    fn the_cap_evicts_the_page_that_has_gone_longest_unused() {
        let t = Instant::now();
        let mut k = kept(2);
        k.park("a", 1, t);
        k.park("b", 2, t + Duration::from_secs(1));
        let freed = k.park("c", 3, t + Duration::from_secs(2));
        assert_eq!(
            freed,
            vec![1],
            "the domain being worked now is the one to keep"
        );
        assert_eq!(k.len(), 2);
        assert_eq!(k.take("a", t + Duration::from_secs(2)), None);
        assert_eq!(k.take("c", t + Duration::from_secs(2)), Some(3));
    }

    #[test]
    fn a_page_whose_browser_was_reaped_is_dropped_not_handed_out() {
        let t = Instant::now();
        let mut k = kept(4);
        k.park("browser1|shop.example", 1, t);
        k.park("browser1|news.example", 2, t);
        k.park("browser2|shop.example", 3, t);
        let mut gone = k.drain_where(|key| key.starts_with("browser1"));
        gone.sort();
        assert_eq!(gone, vec![1, 2]);
        assert_eq!(k.keys(), vec!["browser2|shop.example"]);
    }

    #[test]
    fn a_kept_page_is_retired_by_the_shared_verdict_rule_and_not_a_new_one() {
        // Deliberately asserts the threshold that `session` owns rather than a copy of it: two
        // blocks retire, and one does not, because one block can be the page rather than the tab.
        let t = Instant::now();
        let mut k = kept(2);
        k.park("a", 1, t);
        assert_eq!(
            k.record("a", Verdict::Blocked),
            None,
            "one block is not proof"
        );
        assert_eq!(k.take("a", t), Some(1), "and it is still worth reusing");
        k.park("a", 1, t);
        k.record("a", Verdict::Blocked);
        assert_eq!(
            k.record("a", Verdict::Blocked),
            Some(1),
            "two blocks retire it"
        );
        assert!(k.is_empty());
    }

    #[test]
    fn only_a_verdict_the_shared_rule_calls_bad_retires_a_kept_page() {
        let t = Instant::now();
        let mut k = kept(2);
        k.park("a", 1, t);
        for _ in 0..20 {
            assert_eq!(k.record("a", Verdict::Ok), None, "success never retires");
        }
        // Rate limiting costs less than a block: the site is pacing us, not refusing us. Six of
        // them from full health land exactly on the retirement threshold, which is still usable —
        // the rule is "below", not "at". It takes a seventh.
        for _ in 0..6 {
            assert_eq!(k.record("a", Verdict::RateLimited), None);
        }
        assert_eq!(k.record("a", Verdict::RateLimited), Some(1));
    }

    #[test]
    fn a_cap_of_zero_keeps_nothing_at_all() {
        // The off switch has to actually be off — it is the control arm of the measurement.
        let t = Instant::now();
        let mut k = kept(0);
        assert_eq!(
            k.park("a", 1, t),
            vec![1],
            "handed straight back to be closed"
        );
        assert!(k.is_empty());
        assert_eq!(k.take("a", t), None);
    }

    #[test]
    fn parking_the_same_key_twice_never_holds_two_tabs_for_one_domain() {
        let t = Instant::now();
        let mut k = kept(4);
        k.park("a", 1, t);
        assert_eq!(k.park("a", 2, t), vec![1], "the one it replaced comes back");
        assert_eq!(k.len(), 1);
    }

    #[test]
    fn a_page_that_never_cleared_is_never_kept() {
        assert!(!should_keep(true, false, false, false, true));
    }

    #[test]
    fn an_isolated_fetch_leaves_nothing_behind_not_even_a_kept_page() {
        assert!(!should_keep(true, true, false, true, true));
    }

    #[test]
    fn a_page_wearing_a_borrowed_machine_is_never_kept() {
        // A one-off identity is not this machine, and handing its page to an ordinary fetch would
        // mean two fetches disagreeing about who they are.
        assert!(!should_keep(true, false, true, true, true));
    }

    #[test]
    fn a_cleared_page_behind_a_cookie_borne_wall_is_not_worth_keeping() {
        assert!(!should_keep(true, false, false, true, false));
    }

    #[test]
    fn a_cleared_page_behind_a_runtime_borne_clearance_is_kept() {
        assert!(should_keep(true, false, false, true, true));
    }

    #[test]
    fn nothing_is_kept_at_the_http_tier() {
        assert!(!should_keep(false, false, false, true, true));
    }
}
