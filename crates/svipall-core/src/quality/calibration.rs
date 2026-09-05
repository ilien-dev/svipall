//! Where a number sits among the numbers this machine has seen.
//!
//! ▲ The gap this closes: every quality figure svipall reports is an absolute one, and an absolute
//! figure with nothing to compare it against is a number the caller cannot act on. "Optimisation:
//! high" says nothing about whether that is unusual for the pages this operator actually fetches;
//! "higher than 94% of the last four hundred" does.
//!
//! Three rules, and they are what keeps this from becoming a score:
//!
//! - **Below a floor of observations it refuses to answer.** A percentile out of eleven pages is
//!   arithmetic, not evidence. `MIN_OBSERVATIONS` is where it starts answering, and until then the
//!   caller is told *why* there is no answer rather than handed a confident-looking one.
//! - **The answer carries its own width.** A percentile estimated from n samples has a standard
//!   error of about `sqrt(p(1-p)/n)` — nine points at thirty observations, three and a half at two
//!   hundred. `Band` says which of those the caller is holding.
//! - **It is per class, never pooled.** Optimisation counts and substance confidences are not the
//!   same quantity and putting them in one histogram would produce a percentile of nothing.
//!
//! The distribution is this machine's own history, which is the only population svipall has and is
//! emphatically not the web: an operator who crawls documentation all day will find a shop's
//! optimisation score in the 99th percentile, and that is a true statement about their corpus.

use serde::{Deserialize, Serialize};

/// Buckets across `0.0..=1.0`. Twenty is five points wide, which is finer than the standard error
/// at any sample size this will realistically hold — a hundred buckets would quantise more finely
/// than the estimate underneath is worth.
const BUCKETS: usize = 20;

/// Below this many observations there is no percentile, only a small number of pages.
///
/// Thirty is where the standard error of a mid-range percentile first falls under ten points. It
/// is a floor on *saying something*, not on collecting: the histogram accumulates from the first
/// observation and simply declines to be read until it can be.
pub const MIN_OBSERVATIONS: u64 = 30;

/// Above this many, the estimate is worth about three points and the band narrows.
pub const CONFIDENT_OBSERVATIONS: u64 = 200;

/// How much the answer is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// Roughly ±9 points. Real, and not to be read to two decimal places.
    Wide,
    /// Roughly ±3 points.
    Narrow,
}

/// A number's place in its class, with the width of the claim attached.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Percentile {
    /// Share of past observations at or below this one, `0.0..=1.0`.
    pub value: f32,
    /// How many observations that is out of. Reported because a percentile without its n is a
    /// number pretending to be a measurement.
    pub observations: u64,
    pub band: Band,
}

/// One class's accumulated history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    /// Total observations, which is not `buckets.iter().sum()` after a very long run only because
    /// it would be — it is kept separately so a truncated or hand-edited row is detectable.
    pub n: u64,
    pub buckets: Vec<u32>,
}

impl Default for Distribution {
    fn default() -> Self {
        Self {
            n: 0,
            buckets: vec![0; BUCKETS],
        }
    }
}

impl Distribution {
    /// Read one back from `kv`, or start a fresh one.
    ///
    /// A row that does not parse, or one whose bucket count is from another build, is *discarded*
    /// rather than repaired: a percentile computed against a half-understood histogram would be
    /// wrong in a way nothing downstream could detect.
    pub fn parse(raw: Option<&str>) -> Distribution {
        raw.and_then(|s| serde_json::from_str::<Distribution>(s).ok())
            .filter(|d| d.buckets.len() == BUCKETS)
            .unwrap_or_default()
    }

    /// Record a value. Anything outside `0.0..=1.0` is clamped, and `NaN` is ignored — a model
    /// that emits a bad number must not be able to corrupt the history of every page after it.
    pub fn observe(&mut self, x: f32) {
        if x.is_nan() {
            return;
        }
        let i = bucket_of(x);
        self.buckets[i] = self.buckets[i].saturating_add(1);
        self.n = self.n.saturating_add(1);
    }

    /// Where `x` falls, or `None` when there is not enough history to say.
    ///
    /// The value is the share of observations in strictly lower buckets plus half of this bucket's
    /// own — the mid-rank convention, which stops a value that is merely the commonest from
    /// reading as an extreme one.
    pub fn percentile(&self, x: f32) -> Option<Percentile> {
        if self.n < MIN_OBSERVATIONS || x.is_nan() {
            return None;
        }
        let i = bucket_of(x);
        let below: u64 = self.buckets[..i].iter().map(|c| *c as u64).sum();
        let here = self.buckets[i] as u64;
        let value = (below as f64 + here as f64 / 2.0) / self.n as f64;
        Some(Percentile {
            value: value as f32,
            observations: self.n,
            band: if self.n >= CONFIDENT_OBSERVATIONS {
                Band::Narrow
            } else {
                Band::Wide
            },
        })
    }

    /// Why there is no percentile yet, in the caller's terms. `None` once there is one.
    pub fn why_not(&self) -> Option<String> {
        (self.n < MIN_OBSERVATIONS).then(|| {
            format!(
                "not enough observations yet: {} of {MIN_OBSERVATIONS} needed before a percentile \
                 means anything",
                self.n
            )
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

fn bucket_of(x: f32) -> usize {
    let clamped = x.clamp(0.0, 1.0);
    ((clamped * BUCKETS as f32) as usize).min(BUCKETS - 1)
}

/// The `kv` key one class accumulates under. Prefixed so `kv_list("calib/")` enumerates them and
/// housekeeping can see them as a group.
pub fn key(class: &str) -> String {
    format!("calib/{class}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(n: u64, x: f32) -> Distribution {
        let mut d = Distribution::default();
        for _ in 0..n {
            d.observe(x);
        }
        d
    }

    /// ▲ The rule that keeps this from being a score. Eleven pages is a handful of pages, and a
    /// percentile drawn from them would read exactly like one drawn from four hundred.
    #[test]
    fn a_handful_of_pages_produces_no_percentile_and_says_why() {
        let d = filled(MIN_OBSERVATIONS - 1, 0.5);
        assert_eq!(d.percentile(0.9), None);
        assert!(
            d.why_not().is_some_and(|s| s.contains("not enough")),
            "the caller has to be told why, not handed silence"
        );
    }

    #[test]
    fn once_there_is_history_the_answer_arrives_with_its_width() {
        let mut d = filled(MIN_OBSERVATIONS, 0.1);
        let p = d.percentile(0.9).expect("enough observations");
        assert!(p.value > 0.9, "well above a history of low values: {p:?}");
        assert_eq!(p.band, Band::Wide);
        assert_eq!(p.observations, MIN_OBSERVATIONS);

        for _ in 0..CONFIDENT_OBSERVATIONS {
            d.observe(0.1);
        }
        assert_eq!(
            d.percentile(0.9).expect("still enough").band,
            Band::Narrow,
            "two hundred observations is worth about three points, and the band should say so"
        );
    }

    /// Mid-rank, not "everything below". A value that is simply the commonest one must not read as
    /// the top of the distribution.
    #[test]
    fn the_commonest_value_sits_in_the_middle_rather_than_at_the_top() {
        let d = filled(100, 0.5);
        let p = d.percentile(0.5).expect("enough");
        assert!(
            (p.value - 0.5).abs() < 0.01,
            "the one value everything else is should be the median, got {}",
            p.value
        );
    }

    #[test]
    fn a_number_out_of_range_or_no_number_at_all_cannot_corrupt_the_history() {
        let mut d = Distribution::default();
        d.observe(f32::NAN);
        assert_eq!(d.n, 0, "a model that emits NaN must not be recorded");
        d.observe(5.0);
        d.observe(-2.0);
        assert_eq!(d.n, 2);
        assert_eq!(d.buckets[BUCKETS - 1], 1, "clamped to the top bucket");
        assert_eq!(d.buckets[0], 1, "clamped to the bottom");
    }

    #[test]
    fn history_survives_the_round_trip_and_a_row_from_another_build_is_discarded() {
        let d = filled(40, 0.25);
        assert_eq!(Distribution::parse(Some(&d.to_json())), d);
        assert_eq!(Distribution::parse(None).n, 0);
        assert_eq!(Distribution::parse(Some("not json")).n, 0);
        assert_eq!(
            Distribution::parse(Some(r#"{"n":900,"buckets":[1,2,3]}"#)).n,
            0,
            "a histogram with the wrong shape is no history, not a repairable one"
        );
    }

    #[test]
    fn classes_do_not_share_a_key() {
        assert_ne!(key("optimization"), key("substance"));
        assert!(key("optimization").starts_with("calib/"));
    }
}
