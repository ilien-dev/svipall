//! Politeness: per-domain throttling and cooldowns. Port of server.py throttle logic.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use std::path::PathBuf;
use std::sync::LazyLock;

const COOLDOWN_SECONDS: u64 = 900; // 15 minutes after hard block

// The gap adapts to what the server actually does. Fixed gaps — 500ms and a flat 3s for browser
// tiers — punished fast hosts: a warm crawl of thirty pages spent a minute and a half asleep on a
// site answering in 120ms.
const HTTP_FLOOR: Duration = Duration::from_millis(100);
const HTTP_CEIL: Duration = Duration::from_millis(2_000);
const BROWSER_FLOOR: Duration = Duration::from_millis(400);
const BROWSER_CEIL: Duration = Duration::from_millis(5_000);
/// Never wait longer than this for a `Retry-After`, however long the server asks. Beyond it the
/// domain gets a cooldown instead, which the caller can see and clear.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// What a request did, so the next gap can be chosen from evidence.
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Ok { latency: Duration },
    Blocked,
    RateLimited { retry_after: Option<Duration> },
}

#[derive(Debug, Clone, Copy)]
struct Pace {
    ewma: Duration,
    /// Consecutive blocks. Each one doubles the gap, up to 16x.
    strikes: u32,
}

impl Pace {
    fn seed(tier_floor: Duration) -> Self {
        Self {
            // Start cautious but not slow: a new domain has no history to go on.
            ewma: tier_floor * 2,
            strikes: 0,
        }
    }

    fn gap(&self, tier: &str) -> Duration {
        let (floor, ceil, k) = if tier == "http" {
            (HTTP_FLOOR, HTTP_CEIL, 0.5)
        } else {
            (BROWSER_FLOOR, BROWSER_CEIL, 1.0)
        };
        let base = self.ewma.mul_f32(k).clamp(floor, ceil);
        base * 2u32.pow(self.strikes.min(4))
    }
}

/// The pacing key: a domain, and the exit it is being reached through.
///
/// Keying by domain alone made a pool pointless — ten exits shared one gap, one strike counter and
/// one slot, so rotating exits bought nothing. Rate is a property of the exit *on* the domain, so
/// it is keyed by both. A `Retry-After` and a hard cooldown are the server's word about the domain
/// itself, so those stay keyed by domain below.
fn pace_key(domain: &str, exit: Option<&str>) -> String {
    match exit {
        // U+0001 cannot appear in a hostname or a proxy URL, so it cannot collide.
        Some(e) => format!("{domain}\u{1}{e}"),
        None => domain.to_string(),
    }
}

/// Per `(domain, exit)`: the instant the most recently *scheduled* request goes out. Concurrent
/// callers each reserve the next free slot, so it may sit in the future.
static LAST_HIT: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static COOLDOWNS: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PACE: LazyLock<Mutex<HashMap<String, Pace>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Explicit `Retry-After` holds, per domain — the server asked the *site* to be left alone, not
/// one exit, so a hold applies whichever exit the next request would use.
static HOLDS: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cooldown_file() -> PathBuf {
    crate::config::home_dir().join("cooldowns.json")
}

/// Record how a request went. Feeds the gap for the next one.
/// What this domain normally costs, in milliseconds, from the rolling average the pacer keeps.
///
/// Zero until the domain has been seen. Used to judge a response that suddenly takes many times
/// longer than usual, which is what a tarpit feels like from the inside.
pub fn typical_ms(domain: &str, exit: Option<&str>) -> u64 {
    PACE.lock()
        .ok()
        .and_then(|m| {
            m.get(&pace_key(domain, exit))
                .map(|p| p.ewma.as_millis() as u64)
        })
        .unwrap_or(0)
}

/// `tier` is here for the reputation ledger rather than for the pace: the surcharge a refusal
/// carries depends on how expensive the visit was, and this is the only place that learns a
/// request was refused *after* `throttle` charged it going out.
///
/// Note that a quiet block reaches this twice for one attempt — once as `Ok` on the 200, then as
/// `Blocked` once the page has been classified. Only the second surcharges. If a surcharge is ever
/// added to `Ok` as well, the same attempt will pay it twice.
pub fn observe(domain: &str, exit: Option<&str>, tier: &str, outcome: Outcome) {
    if matches!(outcome, Outcome::Blocked | Outcome::RateLimited { .. }) {
        // The visit itself was charged on the way out; this is what the verdict cost on top.
        crate::reputation::add(
            domain,
            exit,
            crate::reputation::tier_cost(tier) * (crate::reputation::BLOCKED_MULTIPLIER - 1.0),
        );
    }
    let mut map = PACE.lock().unwrap();
    let pace = map
        .entry(pace_key(domain, exit))
        .or_insert_with(|| Pace::seed(HTTP_FLOOR));
    match outcome {
        Outcome::Ok { latency } => {
            // Standard EWMA: recent behaviour dominates without discarding history.
            let blended = pace.ewma.mul_f32(0.7) + latency.mul_f32(0.3);
            pace.ewma = blended;
            pace.strikes = pace.strikes.saturating_sub(1);
            HOLDS.lock().unwrap().remove(domain);
        }
        Outcome::Blocked => pace.strikes = (pace.strikes + 1).min(4),
        Outcome::RateLimited { retry_after } => {
            pace.strikes = (pace.strikes + 1).min(4);
            if let Some(d) = retry_after {
                HOLDS
                    .lock()
                    .unwrap()
                    .insert(domain.to_string(), Instant::now() + d.min(MAX_RETRY_AFTER));
            }
        }
    }
}

/// `Retry-After`, which is either a number of seconds or an HTTP date.
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    let v = value.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // An HTTP date: work out how far away it is. Anything in the past means "now".
    let target = httpdate_secs(v)?;
    let now = now_secs();
    Some(Duration::from_secs(target.saturating_sub(now)))
}

/// Minimal RFC 7231 IMF-fixdate parser (`Sun, 06 Nov 1994 08:49:37 GMT`), which is the only form
/// servers still emit in practice.
fn httpdate_secs(v: &str) -> Option<u64> {
    let parts: Vec<&str> = v.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let day: u64 = parts[1].parse().ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[2])? as u64
        + 1;
    let year: u64 = parts[3].parse().ok()?;
    let hms: Vec<u64> = parts[4].split(':').filter_map(|p| p.parse().ok()).collect();
    if hms.len() != 3 {
        return None;
    }
    // Days since the epoch, via the civil-date algorithm.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hms[0] * 3600 + hms[1] * 60 + hms[2])
}

/// The gap this `(domain, exit)` has earned at this tier, before any waiting happens.
///
/// Separate from `throttle` so it can be asserted on directly: a test that measures elapsed wall
/// time to decide whether pacing works is a test that fails on a loaded machine.
pub fn gap_for(domain: &str, exit: Option<&str>, tier: &str) -> Duration {
    let base = {
        let map = PACE.lock().unwrap();
        map.get(&pace_key(domain, exit))
            .copied()
            .unwrap_or_else(|| {
                Pace::seed(if tier == "http" {
                    HTTP_FLOOR
                } else {
                    BROWSER_FLOOR
                })
            })
            .gap(tier)
    };
    // An address that has spent most of its standing with this host slows down before it is
    // stopped. The multiplier is capped well below the strike backoff's, because the browser
    // tiers' ceiling is already seconds and a crawl has a deadline.
    let pressure = crate::reputation::pressure(domain, exit);
    base.mul_f32(crate::reputation::gap_multiplier(pressure))
}

/// Wait until this domain, on this exit, may be hit again. Yields to the runtime instead of
/// blocking a worker.
pub async fn throttle(domain: &str, exit: Option<&str>, tier: &str) {
    let key = pace_key(domain, exit);
    // Charged here, before anything goes out, because this is the one call every rung of the
    // ladder passes through exactly once and it already holds all three of domain, exit and tier.
    // Anywhere else is somewhere a future rung could forget.
    crate::reputation::spend(domain, exit, tier, false);
    let gap = gap_for(domain, exit, tier);
    // An explicit Retry-After is about the site, not the exit, and outranks anything we would
    // have chosen ourselves.
    let hold = HOLDS.lock().unwrap().get(domain).copied();
    if let Some(until) = hold {
        let left = until.saturating_duration_since(Instant::now());
        if !left.is_zero() {
            tokio::time::sleep(left).await;
        }
    }
    let wait = {
        let mut map = LAST_HIT.lock().unwrap();
        let now = Instant::now();
        let wait = map
            .get(&key)
            .map(|last| (*last + gap).saturating_duration_since(now))
            .filter(|w| !w.is_zero());
        map.insert(key, now + wait.unwrap_or_default());
        wait
    };
    if let Some(d) = wait {
        tokio::time::sleep(d).await;
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read cooldowns.json tolerating both integer and float timestamps (older builds wrote floats).
fn read_file() -> HashMap<String, u64> {
    std::fs::read_to_string(cooldown_file())
        .ok()
        .and_then(|c| serde_json::from_str::<HashMap<String, serde_json::Value>>(&c).ok())
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| v.as_f64().map(|f| (k, f as u64)))
                .collect()
        })
        .unwrap_or_default()
}

fn write_file(data: &HashMap<String, u64>) {
    let path = cooldown_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(data).unwrap_or_default(),
    );
}

pub fn cooldown_left(domain: &str) -> u64 {
    if let Some(stamp) = COOLDOWNS.lock().unwrap().get(domain) {
        let elapsed = stamp.elapsed().as_secs();
        return COOLDOWN_SECONDS.saturating_sub(elapsed);
    }
    if let Some(ts) = read_file().get(domain) {
        return (ts + COOLDOWN_SECONDS).saturating_sub(now_secs());
    }
    0
}

pub fn set_cooldown(domain: &str) {
    COOLDOWNS
        .lock()
        .unwrap()
        .insert(domain.to_string(), Instant::now());
    let mut data = read_file();
    data.insert(domain.to_string(), now_secs());
    write_file(&data);
}

pub fn clear_cooldown(domain: &str) {
    COOLDOWNS.lock().unwrap().remove(domain);
    let mut data = read_file();
    if data.remove(domain).is_some() {
        write_file(&data);
    }
}

pub fn check_cooldown(domain: &str) -> Option<u64> {
    let left = cooldown_left(domain);
    if left > 0 {
        Some(left)
    } else {
        None
    }
}

/// Active cooldowns with seconds remaining (expired entries are dropped from the file).
pub fn list_cooldowns() -> HashMap<String, u64> {
    let now = now_secs();
    let mut data = read_file();
    let before = data.len();
    data.retain(|_, ts| *ts + COOLDOWN_SECONDS > now);
    if data.len() != before {
        write_file(&data);
    }
    data.into_iter()
        .map(|(k, ts)| (k, ts + COOLDOWN_SECONDS - now))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test uses its own domain: the pace map is process-wide state.
    fn domain(name: &str) -> String {
        format!("{name}-{}.test", std::process::id())
    }

    #[test]
    fn retry_after_reads_both_seconds_and_an_http_date() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after(" 5 "), Some(Duration::from_secs(5)));
        // A date in the past means the wait is already over, not a negative one.
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(Duration::ZERO)
        );
        let future = parse_retry_after("Fri, 01 Jan 2100 00:00:00 GMT").expect("a future date");
        assert!(future > Duration::from_secs(86_400));
        assert_eq!(parse_retry_after("not a date"), None);
    }

    #[test]
    fn a_fast_domain_earns_a_shorter_gap_than_a_slow_one() {
        let fast = domain("fast");
        let slow = domain("slow");
        for _ in 0..6 {
            observe(
                &fast,
                None,
                "http",
                Outcome::Ok {
                    latency: Duration::from_millis(80),
                },
            );
            observe(
                &slow,
                None,
                "http",
                Outcome::Ok {
                    latency: Duration::from_millis(3_000),
                },
            );
        }
        let map = PACE.lock().unwrap();
        let fast_gap = map[&fast].gap("browser");
        let slow_gap = map[&slow].gap("browser");
        assert!(
            fast_gap < slow_gap,
            "fast {fast_gap:?} should be quicker than slow {slow_gap:?}"
        );
        // The old code waited a flat 3s on every browser fetch regardless.
        assert!(
            fast_gap < Duration::from_millis(3_000),
            "still {fast_gap:?}"
        );
    }

    #[test]
    fn the_gap_stays_inside_the_tier_bounds() {
        let d = domain("bounds");
        for _ in 0..10 {
            observe(
                &d,
                None,
                "http",
                Outcome::Ok {
                    latency: Duration::from_millis(1),
                },
            );
        }
        let map = PACE.lock().unwrap();
        assert!(map[&d].gap("http") >= HTTP_FLOOR);
        assert!(map[&d].gap("browser") >= BROWSER_FLOOR);
        drop(map);

        let slow = domain("bounds-slow");
        for _ in 0..20 {
            observe(
                &slow,
                None,
                "http",
                Outcome::Ok {
                    latency: Duration::from_secs(60),
                },
            );
        }
        let map = PACE.lock().unwrap();
        assert!(map[&slow].gap("http") <= HTTP_CEIL);
        assert!(map[&slow].gap("browser") <= BROWSER_CEIL);
    }

    #[test]
    fn blocks_back_off_and_success_recovers() {
        let d = domain("strikes");
        observe(
            &d,
            None,
            "http",
            Outcome::Ok {
                latency: Duration::from_millis(100),
            },
        );
        let base = PACE.lock().unwrap()[&d].gap("http");
        for _ in 0..3 {
            observe(&d, None, "http", Outcome::Blocked);
        }
        let backed_off = PACE.lock().unwrap()[&d].gap("http");
        assert!(
            backed_off >= base * 4,
            "three blocks should multiply the gap: {base:?} -> {backed_off:?}"
        );
        for _ in 0..5 {
            observe(
                &d,
                None,
                "http",
                Outcome::Ok {
                    latency: Duration::from_millis(100),
                },
            );
        }
        assert_eq!(
            PACE.lock().unwrap()[&d].strikes,
            0,
            "success should clear the backoff"
        );
    }

    #[test]
    fn an_extreme_retry_after_is_capped() {
        let d = domain("retry");
        observe(
            &d,
            None,
            "http",
            Outcome::RateLimited {
                retry_after: Some(Duration::from_secs(3_600)),
            },
        );
        let hold = *HOLDS.lock().unwrap().get(&d).expect("a hold");
        let left = hold.saturating_duration_since(Instant::now());
        assert!(
            left <= MAX_RETRY_AFTER,
            "an hour-long Retry-After must be capped, was {left:?}"
        );
    }

    #[test]
    fn a_pool_does_not_share_one_gap_across_its_exits() {
        // The whole reason a pool is worth having: a block on one exit slows that exit on that
        // domain, not the others, and a Retry-After slows the domain whichever exit is next.
        let d = domain("pool");
        for _ in 0..3 {
            observe(&d, Some("http://a:1"), "http", Outcome::Blocked);
        }
        // The blocked exit has strikes; a fresh exit on the same domain does not.
        let a = PACE.lock().unwrap()[&pace_key(&d, Some("http://a:1"))].strikes;
        assert_eq!(a, 3, "three blocks, three strikes, on that exit alone");
        assert!(!PACE
            .lock()
            .unwrap()
            .contains_key(&pace_key(&d, Some("http://b:2"))));
        // A Retry-After is the domain's, so it is keyed by domain, not by exit.
        observe(
            &d,
            Some("http://a:1"),
            "http",
            Outcome::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
        );
        assert!(HOLDS.lock().unwrap().contains_key(&d));
    }

    #[tokio::test]
    async fn throttle_spaces_requests_and_never_starves() {
        // Recalibrated for the adaptive gap: an unseen domain starts at twice the tier floor
        // (2 x 100ms for http) rather than the old flat 500ms, so three calls span ~400ms.
        let d = domain("spacing");
        let t0 = Instant::now();
        throttle(&d, None, "http").await; // first: immediate
        throttle(&d, None, "http").await; // second: one gap
        throttle(&d, None, "http").await; // third: two gaps — must not collapse to zero or run away
        let el = t0.elapsed();
        assert!(
            el >= Duration::from_millis(150),
            "requests were not spaced at all: {el:?}"
        );
        assert!(el < Duration::from_millis(2_000), "far too slow: {el:?}");
    }

    /// The point of the change: a fast host is not made to wait as if it were slow.
    #[tokio::test]
    async fn a_fast_browser_domain_is_not_held_for_three_seconds() {
        let d = domain("browser-fast");
        for _ in 0..6 {
            observe(
                &d,
                None,
                "http",
                Outcome::Ok {
                    latency: Duration::from_millis(120),
                },
            );
        }
        let t0 = Instant::now();
        throttle(&d, None, "browser").await;
        throttle(&d, None, "browser").await;
        let el = t0.elapsed();
        assert!(
            el < Duration::from_millis(1_500),
            "the old fixed 3s gap is still in effect: {el:?}"
        );
    }

    /// The pacer is where a spent address is felt before it is refused.
    #[test]
    fn a_domain_near_its_budget_is_paced_more_slowly_than_a_fresh_one() {
        let fresh = domain("pace-fresh");
        let spent = domain("pace-spent");
        for _ in 0..6 {
            for d in [&fresh, &spent] {
                observe(
                    d,
                    None,
                    "browser",
                    Outcome::Ok {
                        latency: Duration::from_millis(500),
                    },
                );
            }
        }
        let before = gap_for(&spent, None, "browser");
        assert_eq!(
            before,
            gap_for(&fresh, None, "browser"),
            "two domains with the same history start from the same gap"
        );
        // Spend it up to the budget, which is the top of the pressure zone.
        crate::reputation::add(&spent, None, crate::reputation::budget());
        let after = gap_for(&spent, None, "browser");
        assert!(
            after > before,
            "a spent address must be paced more slowly: {before:?} -> {after:?}"
        );
        assert_eq!(
            gap_for(&fresh, None, "browser"),
            before,
            "and the domain that spent nothing is untouched"
        );
    }
}
