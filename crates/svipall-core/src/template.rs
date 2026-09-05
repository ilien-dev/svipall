//! What a site's pages have in common, remembered across sessions.
//!
//! ▲ **The one thing a local tool can do that a stateless extractor cannot.** Trafilatura sees one
//! page and has to decide from that page alone what its navigation is. svipall has a cache: the
//! `page` table, indexed by domain, holding what this operator actually fetched, across sessions.
//! Alarte and Silva measured that templates are **40–50% of the data on the web**, and SIGIR-23
//! says outright that no public benchmark can evaluate cross-page methods because none of them
//! ships the sibling pages. That is a statement about benchmarks, not about crawlers.
//!
//! ## What it is
//!
//! One record per domain in `kv` under `template/<domain>`, following the `soft404/<domain>`
//! precedent: a count, per block, of how many of that domain's pages carried it. A block on most
//! pages of a site is the site, not the page.
//!
//! It is learned at **block level, from markdown**, and that is forced rather than chosen: the
//! cache stores the rendered page and never the HTML it came from, so there is no DOM to learn a
//! selector against. That turns out to be what `dedup::Boilerplate` already consumes.
//!
//! ## The two rules that keep it from eating content
//!
//! ▲ Cross-page stripping is the first thing in this project able to remove text a caller would
//! otherwise have had. Everything else labels. So:
//!
//! - **A strip never empties a page.** Alarte and Silva's HybEx orders it the other way round —
//!   extract the page's main content *first*, then infer the template from what is left — so that
//!   a site whose pages share a long passage cannot learn the passage. The cached markdown carries
//!   no marker of which blocks were content, so the same protection is obtained from its
//!   consequence: `MIN_KEPT_SHARE`. If the site's furniture is nearly all of this page, nothing is
//!   removed and the share is reported instead.
//! - **Nothing is stripped until the record is armed.** `MIN_PAGES` pages of one domain, because
//!   a "block on most pages" drawn from three pages is a block on three pages.
//!
//! ## ▲ Measured on TECO, and it is off by default because of what that said
//!
//! TECO is the only corpus that ships each page's siblings, so it is the only one that can score
//! this at all. Learned from sixteen siblings and applied to the labelled key page, over its
//! thirty forum sites:
//!
//! | `MIN_BLOCK` | sites it fired on | text saved there | **labelled content removed** |
//! |---|---|---|---|
//! | 40 | 4 of 11 | 7.6% | 12 words, on 3 sites |
//! | **120** | 2 of 11 | 3.4% | **1 word, on 1 site** |
//!
//! The gate on this feature is absolute — no page may lose a word of human-labelled content the
//! extractor had reached — and at no threshold does it hold. Raising `MIN_BLOCK` until one
//! particular corpus reports zero would be fitting to that corpus, which is the thing this project
//! refuses. So it ships **off**, exactly as the vote and the router did: built, measured, reachable
//! by asking (`use_site_template: true`), and honest about the price.
//!
//! The record is still learned on every fetch, because learning costs nothing and asking for it
//! should work immediately rather than sixteen pages later.
//!
//! ## What it is not
//!
//! Not a filter on which pages come back, not a score, and not silent: every response it changes
//! says so, with how many pages it was learned from and how many blocks it removed. A result that
//! differs between two sessions because of something a tool learned in between, and does not say
//! so, is worse than one that never improved.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Pages of one domain before anything is stripped.
///
/// Sixteen, from the multi-sequence-alignment literature on template extraction, which reports
/// that "16 sample pages are enough to extract the site template with good accuracy". It is
/// deliberately far above `Boilerplate`'s in-crawl threshold of five: a crawl's five pages arrive
/// together from one frontier and are alike by construction, while these accumulate from whatever
/// the operator happened to fetch, and are a much weaker sample of the site.
///
/// ⚠ **Still not fitted, and now for a known reason.** The sweep was built and run against WCXB
/// (`bench/src/template.rs`), and WCXB cannot answer: no domain in it contributes more than eight
/// pages, and at every threshold tried the template removed zero blocks from zero pages, because
/// its same-domain pages were sampled for variety and share nothing verbatim. So this is still a
/// number from the literature. The corpus that could fit it is TECO, which ships sibling pages;
/// `scripts/fetch-teco.sh` downloads it and nothing reads it yet.
pub const MIN_PAGES: u32 = 16;

/// Share of a domain's observed pages a block must appear on to be the site's frame.
///
/// 0.6 is `Boilerplate`'s own in-crawl ratio, kept so the two agree about what furniture is.
pub const MIN_SHARE: f32 = 0.6;

/// Least of a page that has to survive a strip. Below it, nothing is removed at all.
///
/// ▲ This is HybEx's content-first ordering in the one form the cached markdown supports. Alarte
/// and Silva extract each page's main content *before* inferring the site template, so a passage
/// shared across pages cannot be learned as furniture on the page it is the substance of. The
/// cache stores rendered markdown and keeps no marker of which blocks the page-level extractor
/// called content, so that exact ordering is not available here. What is available is its
/// consequence: if what the site says is furniture is nearly all of this page, then either the
/// page is furniture — which is worth *saying*, and `MostlyBoilerplate` says it — or the shared
/// passage is its content. Both answers are the same action, and it is to remove nothing.
///
/// Which way round it was is not guessed. The page comes back whole and the share is reported.
pub const MIN_KEPT_SHARE: f32 = 0.2;

/// Blocks shorter than this repeat legitimately: a heading, a date, a byline — and, TECO shows, a
/// sentence of real content that a site happens to put on many of its pages.
///
/// 40 was the figure `dedup::Boilerplate` uses in-crawl. On TECO it cost twelve words of labelled
/// main content; 120 costs one. Both numbers are in the module doc, and neither is zero, which is
/// why this feature is off by default rather than tuned until a corpus stops complaining.
const MIN_BLOCK: usize = 120;

/// Records above this many distinct blocks stop growing.
///
/// A cap rather than an eviction policy: the interesting blocks are the frequent ones, they arrive
/// early, and a site that produces ten thousand distinct long blocks is a site whose template this
/// was never going to find. Without it one pathological domain could grow a `kv` row without limit.
const MAX_BLOCKS: usize = 2_000;

/// The `kv` key one domain's record lives under.
pub fn key(domain: &str) -> String {
    format!("template/{domain}")
}

/// How many pages of a domain carried each block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Template {
    /// Pages of this domain observed. Not the number in the cache — the number this record was
    /// built from, which is what the frequencies are a fraction of.
    pub pages: u32,
    /// Block hash to the count of pages carrying it. Ordered, so the serialised form is stable and
    /// two machines that saw the same pages write the same row.
    counts: BTreeMap<u64, u32>,
}

/// What a strip actually did, for the caller to be told.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    /// Pages of this domain the template was learned from.
    pub learned_from: u32,
    /// Blocks removed from this page.
    pub removed_blocks: usize,
}

impl Template {
    /// Read a domain's record back, or start an empty one.
    ///
    /// A row that does not parse is discarded rather than repaired: a template half-understood is
    /// a template that removes the wrong blocks, and the cost of starting again is sixteen pages.
    pub fn parse(raw: Option<&str>) -> Template {
        raw.and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    /// Enough pages of this domain for "on most pages" to mean anything.
    pub fn armed(&self) -> bool {
        self.pages >= MIN_PAGES
    }

    /// Learn from one page's markdown.
    ///
    /// Counts a repeated block once per page: a page that repeats its own footer twice must not
    /// count as two pages of evidence.
    pub fn observe(&mut self, markdown: &str) {
        let blocks = crate::budget::blocks(markdown);
        let total: usize = blocks.iter().map(|b| b.len()).sum();
        if total == 0 {
            return;
        }
        self.pages = self.pages.saturating_add(1);
        let mut once = std::collections::HashSet::new();
        for b in blocks {
            if !learnable(b) {
                continue;
            }
            let h = crate::dedup::block_hash(b);
            if !once.insert(h) {
                continue;
            }
            if let Some(c) = self.counts.get_mut(&h) {
                *c = c.saturating_add(1);
            } else if self.counts.len() < MAX_BLOCKS {
                self.counts.insert(h, 1);
            }
        }
    }

    /// Is this block the site's frame?
    fn is_template(&self, block: &str) -> bool {
        if !learnable(block) {
            return false;
        }
        let floor = (self.pages as f32 * MIN_SHARE).ceil() as u32;
        self.counts
            .get(&crate::dedup::block_hash(block))
            .copied()
            .unwrap_or(0)
            >= floor
    }

    /// How much of this page is the site's own furniture, by characters. `None` until armed, which
    /// is not the same as zero.
    pub fn share(&self, markdown: &str) -> Option<f32> {
        if !self.armed() {
            return None;
        }
        let blocks = crate::budget::blocks(markdown);
        let total: usize = blocks.iter().map(|b| b.len()).sum();
        if total == 0 {
            return None;
        }
        let shared: usize = blocks
            .iter()
            .filter(|b| self.is_template(b))
            .map(|b| b.len())
            .sum();
        Some(shared as f32 / total as f32)
    }

    /// Remove what the rest of the site says is furniture.
    ///
    /// Returns the page unchanged, and an `Applied` of zeroes, until the record is armed — and
    /// **also** if stripping would take everything. A page that is entirely template is a page
    /// about which the right thing to say is `MostlyBoilerplate`, not one to return empty.
    pub fn strip(&self, markdown: &str) -> (String, Applied) {
        if !self.armed() {
            return (markdown.to_string(), Applied::default());
        }
        let blocks = crate::budget::blocks(markdown);
        let total: usize = blocks.iter().map(|b| b.len()).sum();
        let kept: Vec<&str> = blocks
            .iter()
            .copied()
            .filter(|b| !self.is_template(b))
            .collect();
        let removed = blocks.len() - kept.len();
        let left: usize = kept.iter().map(|b| b.len()).sum();
        // ▲ The content guard. Either this page *is* the site's frame — worth saying, and
        // `MostlyBoilerplate` is what says it — or a passage the rest of the site repeats is this
        // page's substance. Both are the same action: remove nothing, report the share.
        if removed == 0 || total == 0 || (left as f32) < total as f32 * MIN_KEPT_SHARE {
            return (markdown.to_string(), Applied::default());
        }
        (
            kept.join("\n\n"),
            Applied {
                learned_from: self.pages,
                removed_blocks: removed,
            },
        )
    }
}

/// Can this block be the site's frame at all?
///
/// A heading, a date or a byline repeats legitimately and is a handful of characters.
fn learnable(block: &str) -> bool {
    block.len() >= MIN_BLOCK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(nav: &str, body: &str) -> String {
        format!(
            "{nav}\n\n{body}\n\nFooter: contact us, privacy policy, terms of service, cookie \
             preferences, accessibility statement, and the usual long legal line that every page \
             of this site carries along the bottom of it."
        )
    }

    // Long enough to clear `MIN_BLOCK`, which is what real site furniture looks like: the reason
    // that constant is 120 rather than 40 is that shorter blocks are as often a sentence of
    // content as a piece of navigation.
    const NAV: &str = "Home | About | Products | Support | Careers | Press | Contact | Sign in | \n                       Newsletter | Advertise | Archive | Jobs | Legal | Accessibility | Feedback";

    fn learned(n: u32) -> Template {
        let mut t = Template::default();
        for i in 0..n {
            t.observe(&page(
                NAV,
                &format!(
                    "Article number {i}, whose subject is entirely its own and shares nothing with \
                     the article beside it except the furniture around them both. It runs on for a \
                     while, as articles do, and says several things."
                ),
            ));
        }
        t
    }

    /// ▲ The rule that makes this safe to ship: sixteen pages before a single character is
    /// removed. "A block on most pages" drawn from three pages is a block on three pages.
    #[test]
    fn nothing_is_stripped_until_the_site_has_actually_been_seen() {
        let t = learned(MIN_PAGES - 1);
        let p = page(
            NAV,
            "One more article about something else entirely, at some length.",
        );
        let (out, applied) = t.strip(&p);
        assert_eq!(out, p, "a record that is not armed must change nothing");
        assert_eq!(applied, Applied::default());
        assert_eq!(t.share(&p), None, "and it must not claim to know a share");
    }

    #[test]
    fn once_it_is_armed_the_sites_own_furniture_goes_and_the_article_stays() {
        let t = learned(MIN_PAGES);
        let p = page(
            NAV,
            "The eastern quay reopens on the fourteenth of November, the harbour board said, \
             after a delay of eleven months and a dispute over dredging that reached the courts.",
        );
        let (out, applied) = t.strip(&p);
        assert!(
            out.contains("eastern quay"),
            "the article was removed: {out}"
        );
        assert!(
            !out.contains("Home | About"),
            "the navigation stayed: {out}"
        );
        assert!(!out.contains("privacy policy"), "the footer stayed: {out}");
        assert_eq!(applied.learned_from, MIN_PAGES);
        assert_eq!(applied.removed_blocks, 2);
    }

    /// ▲ HybEx's ordering, and the risk this whole feature carries. A site whose pages share one
    /// long passage must not have that passage learned away from the page it is the point of.
    #[test]
    fn a_passage_that_is_most_of_a_page_is_that_pages_content_however_often_it_repeats() {
        let shared = "This standing note appears at the top of every filing in this series and \
                      explains, at some length, the terms on which the figures below were prepared \
                      and the basis on which they may be relied upon by a reader.";
        let mut t = Template::default();
        for i in 0..40 {
            // A long page, where the shared note is a minor part: it is furniture here.
            t.observe(&format!(
                "{shared}\n\nFiling {i} reports the quarterly figures in full, line by line, with \
                 commentary running to several paragraphs about each of the divisions, the \
                 movements between them, and the reasons the board gives for each. It is a great \
                 deal of text and it is different every quarter."
            ));
        }
        assert!(t.armed());

        // The same note on a short page, where it *is* the page.
        let short = format!("{shared}\n\nNo filing this quarter.");
        let (out, _) = t.strip(&short);
        assert!(
            out.contains("standing note"),
            "the substance of a short page was taken away as template: {out}"
        );
    }

    #[test]
    fn a_page_repeating_its_own_footer_is_still_one_page_of_evidence() {
        let body = "Some article text of a reasonable length that goes on for a sentence or two, \n                    and then a little further so the page is a page.";
        let footer = "Footer: contact us, privacy policy, terms of service, and a long enough \n                     legal line to count.";
        let mut twice = Template::default();
        twice.observe(&format!(
            "{footer}

{body}

{footer}"
        ));
        let mut once = Template::default();
        once.observe(&format!(
            "{footer}

{body}"
        ));
        assert_eq!(twice, once, "one page counted as two pages of evidence");
        assert_eq!(once.pages, 1);
    }

    #[test]
    fn a_record_survives_the_round_trip_and_nonsense_starts_again() {
        let t = learned(MIN_PAGES);
        assert_eq!(Template::parse(Some(&t.to_json())), t);
        assert_eq!(Template::parse(None), Template::default());
        assert_eq!(Template::parse(Some("{{{")), Template::default());
    }

    /// A page that is nothing but the site's frame is reported, never returned empty. `strip` is a
    /// saving; emptying a response is the thing this project exists not to do.
    #[test]
    fn a_page_that_is_entirely_furniture_comes_back_whole() {
        let t = learned(MIN_PAGES);
        let all_frame = page(NAV, "");
        let (out, applied) = t.strip(&all_frame);
        assert_eq!(out, all_frame);
        assert_eq!(applied.removed_blocks, 0);
        assert!(
            t.share(&all_frame).is_some_and(|s| s > 0.9),
            "but it is said: {:?}",
            t.share(&all_frame)
        );
    }

    #[test]
    fn one_domains_record_is_not_anothers() {
        assert_ne!(key("a.test"), key("b.test"));
        assert!(key("a.test").starts_with("template/"));
    }
}
