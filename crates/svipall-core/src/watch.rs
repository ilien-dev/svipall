//! Pages worth checking again later.
//!
//! "Tell me when this changes" is a different job from "fetch this". The fetch is the easy half;
//! the hard half is remembering, across restarts, which pages are being watched, when each was last
//! looked at, and what it looked like — so that the answer is "this changed" rather than "here is
//! the page again, work it out".
//!
//! A watch is a row in the notes store, so it survives the process and needs no new table. The
//! comparison is the content hash the page cache already keeps, so a check that finds nothing costs
//! one conditional request and no parsing at all.
//!
//! What this deliberately is not: a scheduler that runs on its own timetable and phones out. A
//! watch is only ever checked while the server is running, and a check that is overdue by a week is
//! reported as overdue rather than quietly backfilled.

use serde::{Deserialize, Serialize};

/// The prefix every watch is stored under, so listing them is a prefix scan.
pub const PREFIX: &str = "watch/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    pub url: String,
    /// How often to look, in seconds.
    pub interval_secs: i64,
    /// Unix seconds of the last check. `None` means never looked at.
    ///
    /// An `Option` rather than a zero sentinel: zero is a real instant, and a watch first checked
    /// at the epoch would be treated as never checked forever. That is the kind of bug that only
    /// shows up in a test, which is exactly where it showed up.
    #[serde(default)]
    pub last_checked: Option<i64>,
    /// Unix seconds when the content last differed from the check before it.
    #[serde(default)]
    pub last_changed: Option<i64>,
    /// The content hash at the last check.
    #[serde(default)]
    pub last_hash: u64,
    /// How many times it has changed since the watch was added.
    #[serde(default)]
    pub changes: u32,
    /// Watch one region of the page rather than all of it. The region is found by this selector
    /// and, when the selector stops matching after a redesign, relocated by fingerprint like a
    /// schema field is.
    #[serde(default)]
    pub css_selector: Option<String>,
    /// What the caller called it, for reading a list back.
    #[serde(default)]
    pub label: String,
}

impl Watch {
    /// A new watch, never checked.
    ///
    /// The floor on the interval is not a policy about politeness; it is about what a watch can
    /// mean. Anything under a minute is a poll, and a poll of somebody else's site from a tool that
    /// tries not to be noticed is a contradiction.
    pub fn new(url: impl Into<String>, interval_secs: i64) -> Self {
        Self {
            url: url.into(),
            interval_secs: interval_secs.max(60),
            last_checked: None,
            last_changed: None,
            last_hash: 0,
            changes: 0,
            label: String::new(),
            css_selector: None,
        }
    }

    /// The key this watch is stored under.
    pub fn key(&self) -> String {
        format!("{PREFIX}{}", crate::domain::stable_hash(&self.url))
    }

    /// Is it time to look again?
    pub fn due(&self, now: i64) -> bool {
        match self.last_checked {
            None => true,
            Some(at) => now - at >= self.interval_secs,
        }
    }

    /// How overdue it is, in seconds. Zero when it is not.
    ///
    /// Reported rather than hidden: a watch that is a week late means the server was not running,
    /// and pretending otherwise turns "nothing changed" into a claim nobody checked.
    pub fn overdue_by(&self, now: i64) -> i64 {
        let Some(at) = self.last_checked else {
            return 0;
        };
        (now - at - self.interval_secs).max(0)
    }

    /// Record what a check found. Returns whether the page changed.
    ///
    /// The first check of a page never counts as a change: there was nothing to differ from, and
    /// reporting one would mean every watch fires once for no reason.
    pub fn observe(&mut self, hash: u64, now: i64) -> bool {
        let changed = self.last_checked.is_some() && hash != self.last_hash;
        if changed {
            self.changes += 1;
            self.last_changed = Some(now);
        }
        self.last_hash = hash;
        self.last_checked = Some(now);
        changed
    }
}

/// The watches that are due, soonest-overdue first.
///
/// Ordered so that a run with a budget spends it on what has waited longest, rather than on
/// whichever key the store happened to return first.
pub fn due_now(watches: &[Watch], now: i64) -> Vec<&Watch> {
    let mut out: Vec<&Watch> = watches.iter().filter(|w| w.due(now)).collect();
    out.sort_by_key(|w| w.last_checked);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600;

    #[test]
    fn a_watch_that_has_never_been_checked_is_due_immediately() {
        let w = Watch::new("https://x.test/", HOUR);
        assert!(w.due(1_000_000));
        assert_eq!(w.overdue_by(1_000_000), 0, "never checked is not overdue");
    }

    #[test]
    fn a_watch_checked_a_moment_ago_is_not_due_again() {
        let mut w = Watch::new("https://x.test/", HOUR);
        w.observe(1, 1_000_000);
        assert!(!w.due(1_000_000 + 60));
        assert!(w.due(1_000_000 + HOUR));
    }

    #[test]
    fn the_first_look_at_a_page_is_never_a_change() {
        // Otherwise every watch fires once, for nothing, the moment it is added.
        let mut w = Watch::new("https://x.test/", HOUR);
        assert!(!w.observe(42, 1_000_000));
        assert_eq!(w.changes, 0);
        assert_eq!(w.last_changed, None);
    }

    #[test]
    fn a_different_page_is_a_change_and_the_same_page_is_not() {
        let mut w = Watch::new("https://x.test/", HOUR);
        w.observe(42, 1_000_000);
        assert!(!w.observe(42, 1_000_000 + HOUR), "unchanged");
        assert!(w.observe(43, 1_000_000 + 2 * HOUR), "changed");
        assert_eq!(w.changes, 1);
        assert_eq!(w.last_changed, Some(1_000_000 + 2 * HOUR));
        assert!(
            !w.observe(43, 1_000_000 + 3 * HOUR),
            "and back to unchanged"
        );
        assert_eq!(w.changes, 1);
    }

    #[test]
    fn a_page_that_changes_back_still_counts_both_times() {
        // A/B tests and rotating banners do exactly this, and a watch that only counted the first
        // one would report a site as stable while it was flipping every hour.
        let mut w = Watch::new("https://x.test/", HOUR);
        w.observe(1, 0);
        assert!(w.observe(2, HOUR));
        assert!(w.observe(1, 2 * HOUR));
        assert_eq!(w.changes, 2);
    }

    #[test]
    fn a_check_that_never_happened_is_reported_as_overdue() {
        // A week late means the server was not running. Hiding that turns "nothing changed" into a
        // claim nobody checked.
        let mut w = Watch::new("https://x.test/", HOUR);
        w.observe(1, 1_000_000);
        assert_eq!(w.overdue_by(1_000_000 + HOUR), 0, "on time is not overdue");
        assert_eq!(w.overdue_by(1_000_000 + 3 * HOUR), 2 * HOUR);
    }

    #[test]
    fn an_interval_nobody_could_mean_is_raised_to_something_that_can_be_meant() {
        // Anything under a minute is a poll, and a poll of somebody else's site from a tool that
        // tries not to be noticed is a contradiction.
        assert_eq!(Watch::new("https://x.test/", 1).interval_secs, 60);
        assert_eq!(Watch::new("https://x.test/", -5).interval_secs, 60);
        assert_eq!(Watch::new("https://x.test/", 86_400).interval_secs, 86_400);
    }

    #[test]
    fn the_longest_wait_goes_first() {
        // A run with a budget should spend it on what has waited longest, not on whichever key the
        // store happened to return first.
        let mut a = Watch::new("https://a.test/", HOUR);
        a.observe(1, 100);
        let mut b = Watch::new("https://b.test/", HOUR);
        b.observe(1, 50);
        let mut c = Watch::new("https://c.test/", HOUR);
        c.observe(1, 1_000_000);
        let all = [a, b, c];
        let due = due_now(&all, 1_000_000);
        let urls: Vec<&str> = due.iter().map(|w| w.url.as_str()).collect();
        assert_eq!(urls, vec!["https://b.test/", "https://a.test/"]);
    }

    #[test]
    fn two_watches_on_one_url_are_one_watch() {
        // Adding the same page twice should not double the requests it costs.
        let a = Watch::new("https://x.test/page", HOUR);
        let b = Watch::new("https://x.test/page", 86_400);
        assert_eq!(a.key(), b.key());
        assert!(a.key().starts_with(PREFIX));
    }

    #[test]
    fn a_watch_survives_being_written_down_and_read_back() {
        let mut w = Watch::new("https://x.test/", HOUR);
        w.label = "release notes".into();
        w.observe(7, 1_000_000);
        let text = serde_json::to_string(&w).expect("serialises");
        assert_eq!(serde_json::from_str::<Watch>(&text).expect("parses"), w);
    }

    #[test]
    fn a_watch_written_by_an_older_version_still_reads() {
        // Only the two fields a person supplies are required; everything else is bookkeeping this
        // side owns, and a stored watch missing them is a watch that has not been checked yet.
        let w: Watch = serde_json::from_str(r#"{"url":"https://x.test/","interval_secs":3600}"#)
            .expect("parses");
        assert_eq!(w.last_checked, None);
        assert!(w.due(1_000_000));
    }
}
