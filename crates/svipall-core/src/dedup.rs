//! Near-duplicate detection.
//!
//! A crawl of any real site returns the same page many times over: `?utm_source=` variants,
//! printer-friendly copies, paginated views whose body barely changes. Each one is billed to the
//! caller's context in full.
//!
//! SimHash rather than MinHash: one `u64` per page is cheap to store next to the cached row and
//! cheap to compare, and Hamming distance over it separates "the same page again" from "a different
//! page" perfectly well on prose. MinHash would buy set-similarity precision we have no use for, at
//! 64-128 hashes per document.

use std::collections::HashMap;

/// Shingle width. Three tokens is small enough to survive an edit and large enough that unrelated
/// documents do not collide.
const SHINGLE: usize = 3;

/// FNV-1a over a shingle. **Fixed and pinned by test**: these values are written to SQLite, so a
/// silent change to the algorithm would invalidate every stored fingerprint without any error.
fn shingle_hash(words: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            h ^= b' ' as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        for b in w.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    h
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

/// 64-bit SimHash of a document's text.
pub fn simhash(text: &str) -> u64 {
    let lower = text.to_ascii_lowercase();
    let ws = words(&lower);
    if ws.is_empty() {
        return 0;
    }
    let mut v = [0i32; 64];
    let window = SHINGLE.min(ws.len());
    for chunk in ws.windows(window) {
        let h = shingle_hash(chunk);
        for (bit, slot) in v.iter_mut().enumerate() {
            if h >> bit & 1 == 1 {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
    }
    let mut out = 0u64;
    for (bit, slot) in v.iter().enumerate() {
        if *slot > 0 {
            out |= 1 << bit;
        }
    }
    out
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Rough similarity, for reporting. 64 matching bits is identical.
pub fn similarity(a: u64, b: u64) -> f32 {
    1.0 - (hamming(a, b) as f32 / 64.0)
}

/// Fingerprints seen so far in one crawl.
///
/// A linear scan: a crawl caps out in the low hundreds of pages, where comparing a few hundred
/// `u64`s is free and an LSH index would be machinery with nothing to do.
#[derive(Debug, Default)]
pub struct DedupIndex {
    seen: Vec<(u64, String)>,
    threshold: u32,
}

impl DedupIndex {
    pub fn new(threshold: u32) -> Self {
        Self {
            seen: Vec::new(),
            threshold,
        }
    }

    /// Record a page, or report the URL it duplicates.
    pub fn insert_or_find(&mut self, hash: u64, url: &str) -> Option<(String, f32)> {
        // An empty document hashes to zero; two blank pages are not evidence of duplication.
        if hash != 0 {
            if let Some((h, u)) = self
                .seen
                .iter()
                .find(|(h, _)| hamming(*h, hash) <= self.threshold)
            {
                return Some((u.clone(), similarity(*h, hash)));
            }
        }
        self.seen.push((hash, url.to_string()));
        None
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

/// One markdown block, fingerprinted.
///
/// The same hash `Boilerplate` counts with, exposed so `template` can persist counts a later
/// process reads back. That makes it a stored value: changing how it is computed invalidates every
/// `template/<domain>` row silently, exactly as it would every stored simhash.
pub fn block_hash(block: &str) -> u64 {
    shingle_hash(&[block])
}

/// Blocks that appear on most pages of a site: the header, the footer, the cookie banner.
///
/// Density pruning misses these when they sit inside `<main>`, and they are paid for on every
/// single page. Across a crawl they are usually the largest single saving available.
#[derive(Debug, Default)]
pub struct Boilerplate {
    counts: HashMap<u64, u32>,
    pages: u32,
}

impl Boilerplate {
    /// Learn from a page's markdown blocks.
    pub fn observe(&mut self, blocks: &[&str]) {
        self.pages += 1;
        let mut once = std::collections::HashSet::new();
        for b in blocks {
            let h = shingle_hash(&[b]);
            // Count a repeated block once per page, or a page that repeats it inflates the count.
            if once.insert(h) {
                *self.counts.entry(h).or_insert(0) += 1;
            }
        }
    }

    /// Enough pages seen for the frequencies to mean anything.
    pub fn ready(&self) -> bool {
        self.pages >= 5
    }

    /// How much of this page is the site's own furniture, by characters.
    ///
    /// `strip` removes it; this says how much there was. A page that is 95% navigation and cookie
    /// banner is a page the caller should be told about, and only something that has seen the rest
    /// of the site — a crawl — can know it. `None` until enough pages have been seen for the
    /// frequencies to mean anything, which is not the same as zero.
    pub fn share(&self, markdown: &str, ratio: f32) -> Option<f32> {
        if !self.ready() {
            return None;
        }
        let floor = (self.pages as f32 * ratio).ceil() as u32;
        let blocks = crate::budget::blocks(markdown);
        let total: usize = blocks.iter().map(|b| b.len()).sum();
        if total == 0 {
            return None;
        }
        let shared: usize = blocks
            .iter()
            .filter(|b| {
                // Same rule as `strip`: a short block is a heading and repeats legitimately.
                b.len() >= 40 && self.counts.get(&shingle_hash(&[b])).copied().unwrap_or(0) >= floor
            })
            .map(|b| b.len())
            .sum();
        Some(shared as f32 / total as f32)
    }

    /// Drop the blocks that appear on at least `ratio` of the pages seen.
    pub fn strip(&self, markdown: &str, ratio: f32) -> String {
        if !self.ready() {
            return markdown.to_string();
        }
        let floor = (self.pages as f32 * ratio).ceil() as u32;
        crate::budget::blocks(markdown)
            .into_iter()
            .filter(|b| {
                // Headings are short and repeat legitimately; only drop bulk.
                b.len() < 40 || self.counts.get(&shingle_hash(&[b])).copied().unwrap_or(0) < floor
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The algorithm is written to SQLite. Changing it silently would invalidate every stored
    /// fingerprint with no error anywhere, so the values are pinned.
    #[test]
    fn simhash_is_pinned() {
        assert_eq!(simhash(""), 0);
        assert_eq!(simhash("the quick brown fox"), 2_600_275_743_550_894_345);
        // FNV-1a with this crate's own shingle joining; the exact value matters only in that it
        // must never change, because it is what makes stored fingerprints comparable over time.
        assert_eq!(shingle_hash(&["a"]), 12_642_967_877_113_212_044);
        assert_eq!(shingle_hash(&["a", "b"]), shingle_hash(&["a", "b"]));
        assert_ne!(shingle_hash(&["a", "b"]), shingle_hash(&["b", "a"]));
    }

    #[test]
    fn identical_text_hashes_identically() {
        let t = "Ownership in Rust means each value has a single owner.";
        assert_eq!(simhash(t), simhash(t));
        assert_eq!(hamming(simhash(t), simhash(t)), 0);
    }

    #[test]
    fn case_and_punctuation_do_not_change_the_hash() {
        assert_eq!(
            simhash("The Quick, Brown Fox!"),
            simhash("the quick brown fox")
        );
    }

    /// On a page-sized document — which is what this is for — a single edited word should barely
    /// move the hash, while unrelated prose should be far away. Short strings behave differently:
    /// with three-token shingles, one word out of twenty touches a sixth of the document.
    #[test]
    fn a_small_edit_stays_close_and_unrelated_text_stays_far() {
        let body = "Ownership in Rust means each value has a single owner and the compiler \
                    enforces that rule at every point in the program. Borrowing lets code read \
                    data without taking ownership of it. ";
        let base = body.repeat(12);
        let edited = format!("{base}One extra sentence appended at the very end of the page.");
        let other = "Photosynthesis converts light energy into chemical energy stored in glucose \
                     inside the chloroplasts of green plants, driven by chlorophyll absorbing \
                     photons across the visible spectrum. "
            .repeat(12);

        let near = hamming(simhash(&base), simhash(&edited));
        let far = hamming(simhash(&base), simhash(&other));
        assert!(
            near <= 3,
            "a small edit on a page-sized document moved {near} bits"
        );
        assert!(far >= 20, "unrelated prose is only {far} bits away");
    }

    #[test]
    fn the_index_reports_the_url_a_page_duplicates() {
        let mut idx = DedupIndex::new(3);
        let a = "Some page content that is reasonably long so the hash is stable.";
        assert_eq!(idx.insert_or_find(simhash(a), "https://x.test/1"), None);
        let hit = idx.insert_or_find(simhash(a), "https://x.test/2");
        let (url, sim) = hit.expect("the second copy should be recognised");
        assert_eq!(url, "https://x.test/1");
        assert!(sim > 0.95, "similarity reported as {sim}");
        assert_eq!(idx.len(), 1, "a duplicate must not be stored again");
    }

    #[test]
    fn different_pages_are_all_kept() {
        let mut idx = DedupIndex::new(3);
        // Genuinely different subjects, not the same filler with a counter in it.
        let subjects = [
            "Rust ownership moves values between scopes and the borrow checker proves it safe.",
            "Photosynthesis stores light energy as glucose inside chloroplasts of green plants.",
            "Byzantine consensus tolerates faulty nodes provided fewer than a third misbehave.",
            "Baroque counterpoint layers independent melodic lines under strict harmonic rules.",
            "Tectonic subduction recycles oceanic crust into the mantle along deep sea trenches.",
        ];
        for (i, text) in subjects.iter().enumerate() {
            assert_eq!(
                idx.insert_or_find(simhash(text), &format!("/p{i}")),
                None,
                "{text:?} was wrongly matched to an earlier page"
            );
        }
        assert_eq!(idx.len(), 5);
    }

    #[test]
    fn two_empty_pages_are_not_treated_as_duplicates() {
        let mut idx = DedupIndex::new(3);
        assert_eq!(idx.insert_or_find(simhash(""), "/a"), None);
        assert_eq!(
            idx.insert_or_find(simhash(""), "/b"),
            None,
            "an empty document is not evidence of anything"
        );
    }

    #[test]
    fn shared_boilerplate_is_stripped_and_unique_content_survives() {
        let nav = "Home About Contact Careers Privacy Terms Sitemap Support Blog Docs Press";
        let footer = "Copyright 2026 Example Incorporated. All rights reserved worldwide always.";
        let mut bp = Boilerplate::default();
        let pages: Vec<String> = (0..6)
            .map(|i| {
                format!("{nav}\n\nUnique article body number {i} with its own words.\n\n{footer}")
            })
            .collect();
        for p in &pages {
            bp.observe(&crate::budget::blocks(p));
        }
        assert!(bp.ready());
        let stripped = bp.strip(&pages[0], 0.6);
        assert!(
            stripped.contains("Unique article body number 0"),
            "unique content was removed: {stripped}"
        );
        assert!(
            !stripped.contains("All rights reserved"),
            "footer survived: {stripped}"
        );
        assert!(
            !stripped.contains("Careers Privacy"),
            "nav survived: {stripped}"
        );
    }

    #[test]
    fn how_much_of_a_page_is_the_sites_own_furniture_is_a_number_the_caller_can_have() {
        let nav = "Home About Contact Careers Privacy Terms Sitemap Support Blog Docs Press";
        let footer = "Copyright 2026 Example Incorporated. All rights reserved worldwide always.";
        let mut bp = Boilerplate::default();
        // A site of six pages: five substantial, one that is the frame with a line in it.
        let real: Vec<String> = (0..5)
            .map(|i| {
                format!(
                    "{nav}\n\nUnique article body number {i}, long enough to be the bulk of this \
                     page rather than a heading beside the furniture.\n\n{footer}"
                )
            })
            .collect();
        let husk = format!("{nav}\n\nBack soon.\n\n{footer}");
        for p in real.iter().chain(std::iter::once(&husk)) {
            bp.observe(&crate::budget::blocks(p));
        }

        let article = bp.share(&real[0], 0.6).expect("enough pages seen");
        let frame = bp.share(&husk, 0.6).expect("enough pages seen");
        assert!(frame > article, "husk {frame} vs article {article}");
        assert!(frame > 0.8, "a page that is nearly all furniture: {frame}");
    }

    #[test]
    fn the_share_is_absent_rather_than_zero_before_enough_pages() {
        // Zero would read as "this page is all its own", which is a claim nothing has earned yet.
        let mut bp = Boilerplate::default();
        let page = "A block of text long enough to count as bulk rather than as a heading.";
        bp.observe(&crate::budget::blocks(page));
        assert_eq!(bp.share(page, 0.6), None);
    }

    #[test]
    fn boilerplate_stripping_is_inert_before_enough_pages() {
        let mut bp = Boilerplate::default();
        let page =
            "Repeated header block that is long enough to be considered bulk content.\n\nBody.";
        bp.observe(&crate::budget::blocks(page));
        assert_eq!(bp.strip(page, 0.6), page, "must not act on one sample");
    }
}
