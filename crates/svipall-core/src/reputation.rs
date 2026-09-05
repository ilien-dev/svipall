//! What one address has already spent with one host, and when to stop spending it.
//!
//! This project's own benchmark lost a target to it: `README.md` records a cell that went from
//! passing to failing after the lists were run against one residential address several times in a
//! day, and `docs/rest.md` already calls an address's standing with a host "the scarcest thing a
//! local-only tool has". Nothing counted it. `throttle` spaces requests and `exits` scores an exit
//! after it fails — neither notices an address that is simply being used too much, because nothing
//! has failed yet.
//!
//! Worse, the one ledger keyed by `(domain, exit)` skips the case that matters most:
//! `exits::record` returns early for a pool of fewer than two, which is every machine with no
//! proxy — the benchmark's own configuration. Health and spend are not the same quantity. Health
//! is what a site has said about an exit; spend accumulates whether or not anything has been said.
//!
//! **Spend decays rather than resetting.** A window — a day, an hour — has a cliff: at midnight
//! eight visits become free again, and it needs a list of timestamps to answer "how many since".
//! A half-life is one float and one instant, is continuous, and reads like `exits`'s own healing
//! except multiplicative, which is the right shape for something that accumulates without bound.
//!
//! **The budget is a rate, not a daily total**, and the name of the constant must not pretend
//! otherwise: with decay, a steady spend settles at `rate * HALF_LIFE / ln 2`, so `budget` is
//! really "how much may stand outstanding at once".

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

/// What a request costs, by the tier that made it.
///
/// Not a count of visits: an `http` GET and a headful browser waiting out a challenge are not the
/// same event to the host that scores us, and charging them alike would either make a crawl
/// impossible or make a challenge free. Every tier in `crate::types::TIERS` has a price, and a
/// test walks that list so a tier added later cannot quietly cost nothing.
pub fn tier_cost(tier: &str) -> f32 {
    match tier {
        "http" => 1.0,
        "browser" => 3.0,
        "stealth" => 4.0,
        "real" => 8.0,
        // The patient tier is one line in the log and twenty seconds of a real browser answering a
        // challenge. It is the most expensive thing this tool does to an address.
        "warm" => 12.0,
        // An unknown tier is charged like the dearest known one rather than nothing: a mistake
        // that overcharges is visible, and a mistake that undercharges is the defect this module
        // exists to prevent.
        _ => 12.0,
    }
}

/// A request that came back walled cost the address twice over: it spent the visit, and it spent
/// the verdict the visit earned.
pub const BLOCKED_MULTIPLIER: f32 = 2.0;

/// Below this fraction of the budget nothing happens at all.
pub const SOFT_LINE: f32 = 0.7;

/// The most the pacer's gap may be stretched by pressure.
///
/// Four, not eight. The browser tiers' gap ceiling is five seconds, so eight would be forty
/// seconds a page against a crawl's two-minute deadline — a crawl in the pressure zone would
/// silently become a three-page crawl reporting `stopped_by: "time"`. Four is still a real brake
/// and leaves the deadline meaning what it says.
pub const MAX_GAP_MULTIPLIER: f32 = 4.0;

/// Defaults for the two knobs, and the values the calibration test is written against.
pub const DEFAULT_BUDGET: f32 = 250.0;
pub const DEFAULT_HALF_LIFE_HOURS: u32 = 6;

/// Spend on one `(domain, exit)`, as of `at`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
struct Spend {
    spent: f32,
    /// Unix seconds when `spent` was last brought up to date.
    at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    by_key: HashMap<String, Spend>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn path() -> PathBuf {
    crate::config::home_dir().join("reputation.json")
}

/// The ledger, plus what it needs to know about writing itself out.
struct State {
    ledger: Ledger,
    dirty: bool,
    /// Whether the file on disk was unreadable rather than absent. A torn file must never be read
    /// as an empty one: that would hand back a full budget without saying a word, which is this
    /// mechanism failing open and silent.
    unreadable: bool,
    writes: u64,
    last_write: std::time::Instant,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(load()));

fn load() -> State {
    let (ledger, unreadable) = match std::fs::read_to_string(path()) {
        Ok(s) => match serde_json::from_str::<Ledger>(&s) {
            Ok(l) => (l, false),
            Err(e) => {
                tracing::warn!(
                    "reputation.json is unreadable ({e}); the file is kept and every address is \
                     treated as fully spent until it is repaired or removed"
                );
                (Ledger::default(), true)
            }
        },
        // Absent is the ordinary first run, and means nothing has been spent.
        Err(_) => (Ledger::default(), false),
    };
    State {
        ledger,
        dirty: false,
        unreadable,
        writes: 0,
        last_write: std::time::Instant::now(),
    }
}

/// The key: a domain and the exit it is being reached through.
///
/// The same shape `throttle::pace_key` uses, and for the same reason — U+0001 cannot occur in a
/// hostname or a proxy URL, so it cannot collide. `None` is the machine's own address, and it is a
/// key like any other. That is the whole difference from `exits`, which does not track an exit it
/// cannot switch away from: there is nothing to switch to, but there is still something to spend.
fn key(domain: &str, exit: Option<&str>) -> String {
    match exit {
        Some(e) => format!("{domain}\u{1}{e}"),
        None => domain.to_string(),
    }
}

impl Spend {
    /// Spend as it stands now, decayed for the time since it was last touched.
    fn current(&self, half_life: i64, now: i64) -> f32 {
        if half_life <= 0 {
            return self.spent;
        }
        let idle = (now - self.at).max(0) as f32;
        self.spent * 0.5f32.powf(idle / half_life as f32)
    }
}

/// The two knobs, read once. `config::load` re-reads the file on every call and this sits on the
/// hot path, so a change to either takes effect on the next start.
struct Knobs {
    budget: f32,
    half_life: i64,
}

static KNOBS: LazyLock<Knobs> = LazyLock::new(|| {
    let cfg = crate::config::load();
    Knobs {
        budget: cfg.reputation_budget.max(0.0),
        half_life: i64::from(cfg.reputation_half_life_hours) * 3600,
    }
});

/// What one address may have outstanding with one host. Zero turns the whole mechanism off.
pub fn budget() -> f32 {
    KNOBS.budget
}

/// How long it takes for half of what was spent to stop counting.
pub fn half_life_secs() -> i64 {
    KNOBS.half_life
}

fn half_life() -> i64 {
    KNOBS.half_life
}

/// Charge a visit. `blocked` is whether it came back walled.
pub fn spend(domain: &str, exit: Option<&str>, tier: &str, blocked: bool) {
    let cost = tier_cost(tier) * if blocked { BLOCKED_MULTIPLIER } else { 1.0 };
    add(domain, exit, cost);
}

/// Charge an amount directly. Used by the entry points that open a page without walking the
/// ladder, which pay their tier's price once.
pub fn add(domain: &str, exit: Option<&str>, cost: f32) {
    if budget() <= 0.0 || cost <= 0.0 {
        return;
    }
    let now = now_secs();
    let hl = half_life();
    let mut st = STATE.lock().unwrap();
    let e = st.ledger.by_key.entry(key(domain, exit)).or_insert(Spend {
        spent: 0.0,
        at: now,
    });
    // Decay first, then add: otherwise the quiet time since the last visit is thrown away by this
    // one, and a domain visited twice a day would accumulate as if it were visited twice an hour.
    e.spent = e.current(hl, now) + cost;
    e.at = now;
    st.dirty = true;
    let due = st.last_write.elapsed() >= FLUSH_EVERY;
    drop(st);
    if due {
        flush();
    }
}

/// How long the ledger may hold unwritten spend.
///
/// Not on every charge: a crawl at four in flight would then serialize and write the whole file
/// per page. Not only at shutdown either, because the process that matters here is the one that
/// was killed. Thirty seconds is the most spend a kill can lose, which is a page or two.
const FLUSH_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// Spend outstanding on this `(domain, exit)` right now.
pub fn spent(domain: &str, exit: Option<&str>) -> f32 {
    let now = now_secs();
    let hl = half_life();
    let st = STATE.lock().unwrap();
    if st.unreadable {
        return budget();
    }
    st.ledger
        .by_key
        .get(&key(domain, exit))
        .map(|s| s.current(hl, now))
        .unwrap_or(0.0)
}

/// Spend as a fraction of the budget. Zero when the mechanism is off.
pub fn pressure(domain: &str, exit: Option<&str>) -> f32 {
    let b = budget();
    if b <= 0.0 {
        return 0.0;
    }
    spent(domain, exit) / b
}

/// How much the pacer's gap should be stretched, given the pressure.
///
/// One below the soft line, then continuous up to `MAX_GAP_MULTIPLIER` at the budget. A step would
/// be a cliff an operator reads as "it broke"; a curve reads as "this is getting slower", which is
/// what is actually happening.
pub fn gap_multiplier(pressure: f32) -> f32 {
    if pressure <= SOFT_LINE {
        return 1.0;
    }
    let span = (1.0 - SOFT_LINE).max(f32::EPSILON);
    let t = ((pressure - SOFT_LINE) / span).min(1.0);
    MAX_GAP_MULTIPLIER.powf(t)
}

/// Why a fetch is being refused, and how long until it would not be.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub spent: f32,
    pub budget: f32,
    /// Seconds until decay brings the spend back under the budget.
    pub seconds_left: u64,
}

/// Whether this `(domain, exit)` is over its budget, and by how long.
///
/// Deliberately not conditioned on the fetch mode or on whether the host is local: the budget is
/// about the address, not about the ladder, and a mode parameter that switched it off would be a
/// documented bypass. A local host simply never has an entry, because nothing charges one.
pub fn refusal(domain: &str, exit: Option<&str>) -> Option<Refusal> {
    let b = budget();
    if b <= 0.0 {
        return None;
    }
    let s = spent(domain, exit);
    if s <= b {
        return None;
    }
    // s = b * 2^(t / half_life)  =>  t = half_life * log2(s / b)
    let seconds_left = (half_life() as f32 * (s / b).log2()).ceil().max(0.0) as u64;
    Some(Refusal {
        spent: s,
        budget: b,
        seconds_left,
    })
}

/// Forget everything spent on a domain, whichever exit spent it.
pub fn clear(domain: &str) -> bool {
    let prefix = format!("{domain}\u{1}");
    let mut st = STATE.lock().unwrap();
    let before = st.ledger.by_key.len();
    st.ledger
        .by_key
        .retain(|k, _| k != domain && !k.starts_with(&prefix));
    let removed = st.ledger.by_key.len() != before;
    if removed {
        st.dirty = true;
    }
    removed
}

/// Everything outstanding, for `web_status`. Keys that have decayed to nothing are left out on the
/// way past, the way `list_cooldowns` prunes what has expired.
pub fn status() -> serde_json::Value {
    let now = now_secs();
    let hl = half_life();
    let b = budget();
    let st = STATE.lock().unwrap();
    let mut by_domain: HashMap<String, serde_json::Map<String, serde_json::Value>> = HashMap::new();
    for (k, s) in &st.ledger.by_key {
        let outstanding = s.current(hl, now);
        if outstanding < 0.5 {
            continue;
        }
        let (domain, exit) = match k.split_once('\u{1}') {
            Some((d, e)) => (d, e),
            // The machine's own address. Named rather than left blank, because a blank key in a
            // status report reads as a bug.
            None => (k.as_str(), "direct"),
        };
        let pressure = if b > 0.0 { outstanding / b } else { 0.0 };
        by_domain.entry(domain.to_string()).or_default().insert(
            exit.to_string(),
            serde_json::json!({
                "spent": (outstanding * 10.0).round() / 10.0,
                "pressure": (pressure * 100.0).round() / 100.0,
                "refusing": pressure > 1.0,
            }),
        );
    }
    serde_json::json!({
        "budget": b,
        "half_life_hours": hl / 3600,
        "soft_line": SOFT_LINE,
        "by_domain": by_domain,
        "unreadable": st.unreadable,
    })
}

/// Times the ledger has gone to disk. Used by `bench micro` to keep the write volume a measured
/// number rather than a hope.
pub fn writes() -> u64 {
    STATE.lock().unwrap().writes
}

/// Whether anything is waiting to be written.
pub fn is_dirty() -> bool {
    STATE.lock().unwrap().dirty
}

/// Write the ledger out if anything has changed, dropping what has decayed to nothing.
///
/// Whole-file, but never on the hot path: callers flush on a timer and at shutdown, so a crawl
/// with four fetches in flight does not become one synchronous serialize-and-write per page. The
/// write goes to a temporary and is renamed, which is atomic on NTFS and POSIX alike — a kill
/// mid-write must not be able to leave a truncated file, because a truncated file is a full budget
/// handed back without a word.
pub fn flush() {
    let now = now_secs();
    let hl = half_life();
    let payload = {
        let mut st = STATE.lock().unwrap();
        if !st.dirty || st.unreadable {
            return;
        }
        st.ledger.by_key.retain(|_, s| s.current(hl, now) >= 0.5);
        st.dirty = false;
        st.writes += 1;
        st.last_write = std::time::Instant::now();
        serde_json::to_string_pretty(&st.ledger).ok()
    };
    let Some(body) = payload else {
        return;
    };
    let final_path = path();
    let tmp = final_path.with_extension("json.tmp");
    if let Some(parent) = final_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &final_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state is process-wide, so every test uses its own domain.
    fn domain(name: &str) -> String {
        format!("{name}-{}.test", std::process::id())
    }

    /// Put a key in the ledger with its clock already run back, which is how every time-dependent
    /// test in this tree works — there is no injectable clock.
    fn set(domain: &str, exit: Option<&str>, spent_then: f32, ago_secs: i64) {
        let mut st = STATE.lock().unwrap();
        st.ledger.by_key.insert(
            key(domain, exit),
            Spend {
                spent: spent_then,
                at: now_secs() - ago_secs,
            },
        );
    }

    #[test]
    fn every_tier_has_a_cost_and_they_rise_with_the_tier() {
        let costs: Vec<f32> = crate::types::TIERS.iter().map(|t| tier_cost(t)).collect();
        for (t, c) in crate::types::TIERS.iter().zip(&costs) {
            assert!(*c > 0.0, "tier {t} costs nothing");
        }
        for w in costs.windows(2) {
            assert!(
                w[1] > w[0],
                "a more patient tier must never be cheaper: {costs:?}"
            );
        }
    }

    #[test]
    fn spend_halves_every_half_life() {
        let d = domain("halving");
        set(&d, None, 100.0, half_life());
        let after = spent(&d, None);
        assert!(
            (after - 50.0).abs() < 1.0,
            "one half-life should halve it, was {after}"
        );
        set(&d, None, 100.0, half_life() * 4);
        let later = spent(&d, None);
        assert!(
            (later - 6.25).abs() < 0.5,
            "four half-lives is a sixteenth, was {later}"
        );
    }

    #[test]
    fn a_blocked_request_costs_double_a_delivered_one() {
        let ok = domain("cost-ok");
        let bad = domain("cost-blocked");
        spend(&ok, None, "warm", false);
        spend(&bad, None, "warm", true);
        let (a, b) = (spent(&ok, None), spent(&bad, None));
        assert!((b - a * 2.0).abs() < 0.01, "{b} should be twice {a}");
    }

    #[test]
    fn the_home_address_is_budgeted_like_an_exit() {
        // The defect this module exists for: `exits::record` returns early for a pool of one, so a
        // machine with no proxy — the benchmark's own — recorded nothing at all.
        let d = domain("home");
        spend(&d, None, "warm", true);
        assert!(
            spent(&d, None) > 0.0,
            "an address with no proxy must still be budgeted"
        );
    }

    #[test]
    fn two_exits_on_one_domain_do_not_share_a_budget() {
        let d = domain("pool");
        spend(&d, Some("http://a:1"), "warm", true);
        assert!(spent(&d, Some("http://a:1")) > 0.0);
        assert_eq!(spent(&d, Some("http://b:2")), 0.0);
        assert_eq!(spent(&d, None), 0.0, "and the home address is its own key");
    }

    #[test]
    fn one_exit_on_two_domains_does_not_share_a_budget_either() {
        let a = domain("site-a");
        let b = domain("site-b");
        spend(&a, Some("http://x:1"), "warm", true);
        assert_eq!(spent(&b, Some("http://x:1")), 0.0);
    }

    #[test]
    fn the_gap_multiplier_is_one_until_the_soft_line_then_climbs_to_the_cap() {
        assert_eq!(gap_multiplier(0.0), 1.0);
        assert_eq!(gap_multiplier(SOFT_LINE), 1.0);
        let mid = gap_multiplier((SOFT_LINE + 1.0) / 2.0);
        assert!(mid > 1.0 && mid < MAX_GAP_MULTIPLIER, "{mid}");
        assert!((gap_multiplier(1.0) - MAX_GAP_MULTIPLIER).abs() < 0.01);
        assert!(
            (gap_multiplier(9.0) - MAX_GAP_MULTIPLIER).abs() < 0.01,
            "the multiplier is capped, not unbounded"
        );
    }

    #[test]
    fn an_exhausted_budget_says_how_long_until_it_is_not() {
        let d = domain("exhausted");
        assert!(
            refusal(&d, None).is_none(),
            "an unseen address owes nothing"
        );
        // Twice the budget is exactly one half-life away from being under it.
        set(&d, None, budget() * 2.0, 0);
        let r = refusal(&d, None).expect("over budget");
        let expected = half_life() as u64;
        assert!(
            r.seconds_left.abs_diff(expected) <= 2,
            "expected about {expected}s, got {}",
            r.seconds_left
        );
        set(&d, None, budget() * 0.99, 0);
        assert!(
            refusal(&d, None).is_none(),
            "just under the budget is not over it"
        );
    }

    #[test]
    fn clearing_a_domain_forgets_every_exit_that_spent_on_it() {
        let d = domain("clearable");
        spend(&d, None, "warm", true);
        spend(&d, Some("http://a:1"), "warm", true);
        assert!(clear(&d));
        assert_eq!(spent(&d, None), 0.0);
        assert_eq!(spent(&d, Some("http://a:1")), 0.0);
        assert!(!clear(&d), "clearing what is already clear changes nothing");
    }

    #[test]
    fn a_torn_ledger_does_not_read_as_an_empty_one() {
        // A file that will not parse must not mean "nothing has been spent". Exercised against the
        // parse the loader does, because the process-wide state is loaded exactly once.
        let torn = "{\"by_key\":{\"a.test\":{\"spent\":1";
        assert!(
            serde_json::from_str::<Ledger>(torn).is_err(),
            "the fixture must actually be torn"
        );
        let whole = "{\"by_key\":{}}";
        assert!(serde_json::from_str::<Ledger>(whole).is_ok());
    }

    /// The calibration, written down as an assertion so the number cannot drift in silence.
    ///
    /// The costs come from the tier table and the ladder this benchmark actually walks: the
    /// hardest cell is learned at `warm`, is refused, and earns the fresh-profile retry — two warm
    /// rungs, doubled. The published procedure is a median of three runs.
    #[test]
    fn three_runs_of_the_published_protocol_fit_and_eight_spread_over_a_day_do_not() {
        let per_run = (tier_cost("warm") * 2.0) * BLOCKED_MULTIPLIER;
        assert_eq!(per_run, 48.0);

        let three = per_run * 3.0;
        assert!(
            three < SOFT_LINE * DEFAULT_BUDGET,
            "the documented three-run protocol must not even brake: {three} of {DEFAULT_BUDGET}"
        );

        // Eight runs an hour apart, each decaying until the last one lands.
        let hourly = 0.5f32.powf(1.0 / DEFAULT_HALF_LIFE_HOURS as f32);
        let eight: f32 = (0..8).map(|j| per_run * hourly.powi(j)).sum();
        assert!(
            eight > DEFAULT_BUDGET,
            "eight runs against one address in a day must be refused: {eight} of {DEFAULT_BUDGET}"
        );
    }
}
