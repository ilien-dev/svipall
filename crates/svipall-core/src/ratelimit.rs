//! What a server says about its own pace, in the headers it sends with an ordinary answer.
//!
//! A 429 is the last thing a host says about its limit, not the first. Most APIs that have one
//! state it on every response: so many requests per interval, one at a time, so many left and
//! when the count starts again. Reading that is the difference between asking at the pace the
//! host named and finding the pace by being refused.
//!
//! Three spellings are read, none of them tied to a host:
//!
//! - a quota and its interval as two headers (`x-rate-limit-limit: 50`,
//!   `x-rate-limit-interval: 1s`), with `x-concurrency-limit` beside them;
//! - the `RateLimit-Policy` field (`100;w=60`, or `"name";q=100;w=60`);
//! - what is left and when it resets (`x-ratelimit-remaining: 0`, `x-ratelimit-reset: 30`, the
//!   same without `x-`, or the `RateLimit` field's `remaining=0, reset=30` / `r=0;t=30`).
//!
//! A quota with no interval beside it (`x-ratelimit-limit: 1000` alone) is a number without a
//! unit, an hour for one host and a day for another, and is not turned into a pace. What is left
//! matters only when it is nothing: spreading the rest of a daily quota over the rest of the day
//! would make a tool slow for an operator who wants ten answers now.

use std::time::Duration;

/// The longest gap a declaration is believed for. Past this it is a quota, not a pace.
const MAX_GAP: Duration = Duration::from_secs(60);
/// The shortest worth keeping: below it the host has declared no pace that binds anybody.
const MIN_GAP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Declared {
    /// The least time between two requests the host asks for.
    pub gap: Option<Duration>,
    /// The host answers one request at a time.
    pub alone: bool,
    /// The quota is used up, and starts again after this long.
    pub spent_for: Option<Duration>,
}

impl Declared {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn header<'a>(headers: &'a [(String, String)], names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.trim())
    })
}

/// `1s`, `60s`, `1m`, `1h`, or a bare number of seconds.
fn span(v: &str) -> Option<Duration> {
    let v = v.trim();
    let digits = v.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let n: f64 = digits.trim().parse().ok()?;
    let unit = match v[digits.len()..].trim().to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" => 1.0,
        "ms" => 0.001,
        "m" | "min" => 60.0,
        "h" => 3600.0,
        _ => return None,
    };
    (n.is_finite() && n > 0.0).then(|| Duration::from_secs_f64(n * unit))
}

/// The value of `key=` among the `;`- and `,`-separated members of a structured field.
fn member(field: &str, keys: &[&str]) -> Option<f64> {
    field.split([';', ',']).find_map(|part| {
        let (k, v) = part.split_once('=')?;
        keys.contains(&k.trim().to_ascii_lowercase().as_str())
            .then(|| v.trim().trim_matches('"').parse().ok())
            .flatten()
    })
}

fn gap(quota: f64, interval: Duration) -> Option<Duration> {
    if !(quota.is_finite() && quota >= 1.0) {
        return None;
    }
    let gap = interval.div_f64(quota);
    (gap >= MIN_GAP).then(|| gap.min(MAX_GAP))
}

/// Read a response's headers. `now` is the clock in seconds since the epoch, for a reset given
/// as a moment rather than a delay.
pub fn read(headers: &[(String, String)], now: u64) -> Declared {
    let quota_and_interval = header(headers, &["x-rate-limit-limit"])
        .and_then(|l| l.parse::<f64>().ok())
        .zip(header(headers, &["x-rate-limit-interval"]).and_then(span))
        .and_then(|(q, i)| gap(q, i));
    let policy = header(headers, &["ratelimit-policy", "x-ratelimit-policy"]).and_then(|p| {
        // One policy is enough: the first is the one the response was counted against.
        let first = p.split(',').next().unwrap_or(p);
        let quota = member(first, &["q"]).or_else(|| first.split(';').next()?.trim().parse().ok());
        let window = member(first, &["w"])?;
        gap(quota?, Duration::from_secs_f64(window.max(0.0)))
    });

    let field = header(headers, &["ratelimit"]);
    let remaining = header(
        headers,
        &[
            "x-ratelimit-remaining",
            "ratelimit-remaining",
            "x-rate-limit-remaining",
        ],
    )
    .and_then(|v| v.parse::<f64>().ok())
    .or_else(|| field.and_then(|f| member(f, &["remaining", "r"])));
    let reset = header(
        headers,
        &["x-ratelimit-reset", "ratelimit-reset", "x-rate-limit-reset"],
    )
    .and_then(|v| v.parse::<f64>().ok())
    .or_else(|| field.and_then(|f| member(f, &["reset", "t"])));
    let spent_for = match (remaining, reset) {
        (Some(left), Some(reset)) if left <= 0.0 && reset > 0.0 => {
            // A moment (seconds or milliseconds since the epoch) or a delay in seconds.
            let secs = if reset > 1e12 {
                (reset / 1000.0 - now as f64).max(1.0)
            } else if reset > 1e9 {
                (reset - now as f64).max(1.0)
            } else {
                reset
            };
            Some(Duration::from_secs_f64(secs))
        }
        _ => None,
    };

    Declared {
        gap: quota_and_interval.or(policy),
        alone: header(headers, &["x-concurrency-limit"]) == Some("1"),
        spent_for,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_quota_and_its_interval_are_a_gap() {
        // As a metadata API for scholarly works sends them to an anonymous caller.
        let d = read(
            &h(&[
                ("x-rate-limit-limit", "1"),
                ("x-rate-limit-interval", "1s"),
                ("x-concurrency-limit", "1"),
            ]),
            0,
        );
        assert_eq!(d.gap, Some(Duration::from_secs(1)));
        assert!(d.alone);
        assert_eq!(d.spent_for, None);

        let fast = read(
            &h(&[
                ("X-Rate-Limit-Limit", "50"),
                ("X-Rate-Limit-Interval", "1s"),
            ]),
            0,
        );
        assert_eq!(fast.gap, Some(Duration::from_millis(20)));
        assert!(!fast.alone);
    }

    #[test]
    fn a_quota_without_an_interval_is_not_a_pace() {
        // A daily credit count: a thousand, nine hundred and ninety left, six hours to go.
        let d = read(
            &h(&[
                ("x-ratelimit-limit", "1000"),
                ("x-ratelimit-remaining", "990"),
                ("x-ratelimit-reset", "22207"),
            ]),
            0,
        );
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn nothing_left_is_a_pause_until_the_reset() {
        let delay = read(
            &h(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "30")]),
            1_800_000_000,
        );
        assert_eq!(delay.spent_for, Some(Duration::from_secs(30)));
        let moment = read(
            &h(&[
                ("X-RateLimit-Remaining", "0"),
                ("X-RateLimit-Reset", "1800000045"),
            ]),
            1_800_000_000,
        );
        assert_eq!(moment.spent_for, Some(Duration::from_secs(45)));
        let millis = read(
            &h(&[
                ("ratelimit-remaining", "0"),
                ("ratelimit-reset", "1800000045000"),
            ]),
            1_800_000_000,
        );
        assert_eq!(millis.spent_for, Some(Duration::from_secs(45)));
        // A reset already past is a second, never a negative wait.
        let past = read(
            &h(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1700000000"),
            ]),
            1_800_000_000,
        );
        assert_eq!(past.spent_for, Some(Duration::from_secs(1)));
    }

    #[test]
    fn the_structured_fields_say_the_same_things() {
        let d = read(
            &h(&[
                ("RateLimit-Policy", "100;w=60"),
                ("RateLimit", "limit=100, remaining=0, reset=12"),
            ]),
            0,
        );
        assert_eq!(d.gap, Some(Duration::from_millis(600)));
        assert_eq!(d.spent_for, Some(Duration::from_secs(12)));
        let named = read(
            &h(&[
                (
                    "ratelimit-policy",
                    "\"burst\";q=10;w=1, \"daily\";q=5000;w=86400",
                ),
                ("ratelimit", "\"burst\";r=3;t=1"),
            ]),
            0,
        );
        assert_eq!(named.gap, Some(Duration::from_millis(100)));
        assert_eq!(named.spent_for, None);
    }

    #[test]
    fn a_declaration_that_binds_nobody_or_makes_no_sense_is_ignored() {
        for pairs in [
            vec![("x-rate-limit-limit", "0"), ("x-rate-limit-interval", "1s")],
            vec![
                ("x-rate-limit-limit", "many"),
                ("x-rate-limit-interval", "1s"),
            ],
            vec![
                ("x-rate-limit-limit", "5"),
                ("x-rate-limit-interval", "soon"),
            ],
            vec![
                ("x-rate-limit-limit", "100000"),
                ("x-rate-limit-interval", "1s"),
            ],
            vec![("content-type", "application/json")],
        ] {
            assert!(read(&h(&pairs), 0).is_empty(), "{pairs:?}");
        }
        // A request an hour is a quota: believed, but only up to a minute between two.
        let slow = read(
            &h(&[("x-rate-limit-limit", "1"), ("x-rate-limit-interval", "1h")]),
            0,
        );
        assert_eq!(slow.gap, Some(MAX_GAP));
    }
}
