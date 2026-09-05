//! Crawling only what moved.
//!
//! The second crawl of a site is almost entirely the first crawl again. A documentation site of
//! four hundred pages changes five of them in a week, and re-reading the other three hundred and
//! ninety-five costs the same as the first run did — in requests, in the site's patience, and in
//! whatever the pages get turned into afterwards.
//!
//! Sitemaps already carry the answer. `<lastmod>` says when each URL changed, and the page cache
//! already records when each URL was last fetched. Comparing the two is the whole feature.
//!
//! The judgement that matters is what to do when the site says nothing. A URL with no `lastmod` is
//! not a URL that did not change — it is a site that does not publish the field, and treating
//! silence as "unchanged" turns an incremental crawl into a crawl that finds nothing after the
//! first run. So silence means fetch.

/// A URL as the sitemap describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub url: String,
    /// The site's `<lastmod>`, verbatim. Absent on most sitemaps.
    pub lastmod: Option<String>,
}

/// Why a URL is being fetched, or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Never seen before.
    New,
    /// The site says it changed after we last read it.
    Changed,
    /// The site publishes no date for it, so there is nothing to compare against.
    Undated,
    /// Seen, and the site's date is no newer than our copy.
    Unchanged,
}

impl Reason {
    pub fn fetch(self) -> bool {
        !matches!(self, Reason::Unchanged)
    }
    pub fn name(self) -> &'static str {
        match self {
            Reason::New => "new",
            Reason::Changed => "changed",
            Reason::Undated => "undated",
            Reason::Unchanged => "unchanged",
        }
    }
}

/// Decide about one URL, given when it was last fetched (unix seconds), if ever.
pub fn decide(entry: &Entry, last_fetched: Option<i64>) -> Reason {
    let Some(seen) = last_fetched else {
        return Reason::New;
    };
    let Some(raw) = entry.lastmod.as_deref() else {
        // Silence is not "unchanged". Reading it that way makes every run after the first find
        // nothing at all, on the majority of sitemaps, which publish no dates.
        return Reason::Undated;
    };
    match parse_time(raw) {
        Some(changed) if changed > seen => Reason::Changed,
        Some(_) => Reason::Unchanged,
        // A date nobody can parse is a date that says nothing.
        None => Reason::Undated,
    }
}

/// Split a sitemap into what is worth fetching and what is not.
pub fn plan(entries: &[Entry], last_fetched: impl Fn(&str) -> Option<i64>) -> Vec<(Entry, Reason)> {
    entries
        .iter()
        .map(|e| {
            let reason = decide(e, last_fetched(&e.url));
            (e.clone(), reason)
        })
        .collect()
}

/// Read the two date shapes sitemaps actually use, as unix seconds.
///
/// Written out rather than pulled in: `chrono` is already in the tree for the solver, but the
/// formats here are two fixed prefixes, and a date parser that accepts more than a sitemap can
/// contain is a parser that accepts a wrong answer.
pub fn parse_time(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let bytes = raw.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let num = |from: usize, to: usize| raw.get(from..to)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if bytes[4] != b'-' || bytes[7] != b'-' || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Time of day when it is there, midnight when it is not. Both forms are valid in a sitemap.
    let (hh, mm, ss) = if bytes.len() >= 19 && (bytes[10] == b'T' || bytes[10] == b' ') {
        (num(11, 13)?, num(14, 16)?, num(17, 19)?)
    } else {
        (0, 0, 0)
    };
    if !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..=60).contains(&ss) {
        return None;
    }
    // Days since the epoch, by the civil-from-days algorithm. No leap seconds, no timezone: a
    // sitemap's offset is at most hours, and this is used for "newer than", not for a clock.
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(url: &str, lastmod: Option<&str>) -> Entry {
        Entry {
            url: url.into(),
            lastmod: lastmod.map(str::to_string),
        }
    }

    #[test]
    fn a_page_the_site_says_changed_is_fetched_again() {
        let seen = parse_time("2026-01-01").expect("parses");
        assert_eq!(
            decide(&e("https://x.test/a", Some("2026-02-01")), Some(seen)),
            Reason::Changed
        );
    }

    #[test]
    fn a_page_that_has_not_moved_since_we_read_it_is_left_alone() {
        // The whole point: three hundred and ninety-five pages of a four hundred page site.
        let seen = parse_time("2026-03-01").expect("parses");
        let r = decide(&e("https://x.test/a", Some("2026-02-01")), Some(seen));
        assert_eq!(r, Reason::Unchanged);
        assert!(!r.fetch());
    }

    #[test]
    fn a_page_nobody_has_read_yet_is_always_fetched() {
        assert_eq!(
            decide(&e("https://x.test/new", Some("2020-01-01")), None),
            Reason::New
        );
        assert!(decide(&e("https://x.test/new", None), None).fetch());
    }

    #[test]
    fn a_site_that_publishes_no_dates_is_still_crawled() {
        // Treating silence as "unchanged" turns the second run into a run that finds nothing, on
        // the majority of sitemaps, which carry no lastmod at all.
        let seen = parse_time("2026-03-01").expect("parses");
        let r = decide(&e("https://x.test/a", None), Some(seen));
        assert_eq!(r, Reason::Undated);
        assert!(r.fetch(), "silence must not be read as unchanged");
    }

    #[test]
    fn a_date_nobody_can_parse_says_nothing_rather_than_something_wrong() {
        let seen = parse_time("2026-03-01").expect("parses");
        for bad in ["yesterday", "2026", "not-a-date", "", "9999-99-99"] {
            let r = decide(&e("https://x.test/a", Some(bad)), Some(seen));
            assert_eq!(r, Reason::Undated, "{bad}");
            assert!(r.fetch(), "{bad}");
        }
    }

    #[test]
    fn both_date_shapes_a_sitemap_can_carry_are_read() {
        let day = parse_time("2026-02-01").expect("date only");
        let with_time = parse_time("2026-02-01T12:30:00+00:00").expect("with a time");
        assert_eq!(with_time - day, 12 * 3_600 + 30 * 60);
        assert_eq!(parse_time("2026-02-01 12:30:00"), Some(with_time));
    }

    #[test]
    fn the_epoch_and_a_leap_day_both_land_where_they_should() {
        // The calendar arithmetic is the part that is easy to get quietly wrong, and a date that
        // is off by a day makes a page look unchanged when it is not.
        assert_eq!(parse_time("1970-01-01"), Some(0));
        assert_eq!(parse_time("1970-01-02"), Some(86_400));
        assert_eq!(parse_time("2024-02-29T00:00:00"), Some(1_709_164_800));
        assert!(parse_time("2000-03-01").unwrap() > parse_time("2000-02-29").unwrap());
    }

    #[test]
    fn a_plan_says_what_it_is_doing_and_why_for_every_url() {
        let seen = parse_time("2026-03-01").expect("parses");
        let entries = vec![
            e("https://x.test/old", Some("2026-01-01")),
            e("https://x.test/fresh", Some("2026-04-01")),
            e("https://x.test/quiet", None),
            e("https://x.test/never", Some("2026-01-01")),
        ];
        let plan = plan(&entries, |url| (!url.ends_with("never")).then_some(seen));
        let reasons: Vec<&str> = plan.iter().map(|(_, r)| r.name()).collect();
        assert_eq!(reasons, vec!["unchanged", "changed", "undated", "new"]);
        assert_eq!(plan.iter().filter(|(_, r)| r.fetch()).count(), 3);
    }
}
