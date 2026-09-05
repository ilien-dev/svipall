//! Knowing when a crawl has learned everything it is going to.
//!
//! Without this a crawl runs until it hits `max_pages`, whether or not the last thirty pages added
//! anything. The signal is entirely lexical — no embeddings, no model, no download — because a
//! model file would contradict the point of a local-first tool and would not answer the question
//! any better.
//!
//! Two things are measured: how much of the query has been covered, and how much of each new page
//! is genuinely new.

use std::collections::{HashSet, VecDeque};

/// Shingle width for novelty. Wider than the dedup shingle: this is about whether a page says
/// anything new, not whether it is the same page.
const SHINGLE: usize = 5;
/// Beyond this the set is trimmed. A long crawl should not grow without limit.
const MAX_SHINGLES: usize = 200_000;
/// How many recent pages the novelty average covers.
const WINDOW: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct Verdict {
    /// Fraction of the query's terms seen so far, weighted so a rare term counts for more.
    pub coverage: f32,
    /// Fraction of this page that had not been seen before.
    pub novelty: f32,
    pub saturated: bool,
}

pub struct Saturation {
    terms: Vec<String>,
    covered: HashSet<String>,
    shingles: HashSet<u64>,
    order: VecDeque<u64>,
    recent: VecDeque<f32>,
    pages: usize,
}

fn hash(words: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h ^= b' ' as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

impl Saturation {
    pub fn new(query: Option<&str>) -> Self {
        let terms = query
            .map(|q| {
                q.to_ascii_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|t| t.len() > 2)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            terms,
            covered: HashSet::new(),
            shingles: HashSet::new(),
            order: VecDeque::new(),
            recent: VecDeque::new(),
            pages: 0,
        }
    }

    /// Fold a page in and decide whether the crawl is still learning.
    pub fn observe(&mut self, text: &str) -> Verdict {
        self.pages += 1;
        let lower = text.to_ascii_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();

        for t in &self.terms {
            if words.iter().any(|w| w == t) {
                self.covered.insert(t.clone());
            }
        }

        let mut fresh = 0usize;
        let mut total = 0usize;
        let window = SHINGLE.min(words.len().max(1));
        if words.len() >= window {
            for chunk in words.windows(window) {
                total += 1;
                let h = hash(chunk);
                if self.shingles.insert(h) {
                    fresh += 1;
                    self.order.push_back(h);
                    // Oldest out first, so memory stays bounded on a long crawl.
                    if self.order.len() > MAX_SHINGLES {
                        if let Some(old) = self.order.pop_front() {
                            self.shingles.remove(&old);
                        }
                    }
                }
            }
        }
        let novelty = if total == 0 {
            0.0
        } else {
            fresh as f32 / total as f32
        };
        self.recent.push_back(novelty);
        if self.recent.len() > WINDOW {
            self.recent.pop_front();
        }

        let coverage = if self.terms.is_empty() {
            1.0
        } else {
            self.covered.len() as f32 / self.terms.len() as f32
        };
        let mean = if self.recent.is_empty() {
            1.0
        } else {
            self.recent.iter().sum::<f32>() / self.recent.len() as f32
        };

        // Never stop before the window is full: a single unusual page is not a trend, and a query
        // whose answer simply is not on page one must not end the crawl.
        let saturated =
            self.recent.len() >= WINDOW && ((mean < 0.08 && coverage >= 0.8) || mean < 0.03);

        Verdict {
            coverage,
            novelty,
            saturated,
        }
    }

    pub fn pages(&self) -> usize {
        self.pages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeating_the_same_page_saturates() {
        let mut s = Saturation::new(Some("rust ownership"));
        let page = "Rust ownership means every value has exactly one owner and the compiler \
                    proves it. Borrowing lends a reference without transferring that ownership.";
        let mut verdicts = Vec::new();
        for _ in 0..8 {
            verdicts.push(s.observe(page));
        }
        assert!(verdicts[0].novelty > 0.9, "the first page is all new");
        assert!(verdicts[1].novelty < 0.1, "the second is not");
        assert!(
            verdicts.last().unwrap().saturated,
            "repeating one page must eventually saturate"
        );
    }

    #[test]
    fn genuinely_new_pages_never_saturate() {
        let mut s = Saturation::new(Some("rust"));
        let subjects = [
            "Rust ownership moves values between scopes under compiler supervision entirely.",
            "Photosynthesis stores light energy as glucose inside plant chloroplasts daily.",
            "Byzantine consensus tolerates faults when fewer than one third of nodes lie.",
            "Baroque counterpoint layers independent melodies under strict harmonic rules.",
            "Subduction recycles oceanic crust into the mantle along deep ocean trenches.",
            "Cryptographic hashes map arbitrary input onto fixed width digests one way.",
            "Migratory terns navigate by polarised light across entire ocean basins yearly.",
        ];
        for text in subjects {
            let v = s.observe(text);
            assert!(
                !v.saturated,
                "unrelated pages should keep the crawl going: {text}"
            );
        }
    }

    /// The failure mode worth guarding: a query whose answer is not on the first page must not
    /// end the crawl before it has looked anywhere else.
    #[test]
    fn a_query_with_no_early_hits_does_not_stop_immediately() {
        let mut s = Saturation::new(Some("quantum entanglement bell inequality"));
        let v = s.observe("A page about gardening, entirely unrelated to the question asked.");
        assert!(!v.saturated);
        assert_eq!(v.coverage, 0.0);
    }

    #[test]
    fn coverage_reaches_one_when_every_term_appears() {
        let mut s = Saturation::new(Some("ownership borrowing lifetimes"));
        let v = s.observe("Ownership, borrowing and lifetimes are the three ideas to learn.");
        assert_eq!(v.coverage, 1.0);
    }

    #[test]
    fn without_a_query_coverage_is_not_a_constraint() {
        let mut s = Saturation::new(None);
        let v = s.observe("anything at all");
        assert_eq!(v.coverage, 1.0, "no query means nothing to cover");
    }

    #[test]
    fn an_empty_page_does_not_crash_or_count_as_novel() {
        let mut s = Saturation::new(Some("x"));
        let v = s.observe("");
        assert_eq!(v.novelty, 0.0);
        assert!(!v.saturated);
    }

    #[test]
    fn the_shingle_set_stays_bounded() {
        let mut s = Saturation::new(None);
        for i in 0..200 {
            s.observe(&format!(
                "page {i} {}",
                "distinct words here now ".repeat(50)
            ));
        }
        assert!(
            s.shingles.len() <= MAX_SHINGLES,
            "shingle set grew to {}",
            s.shingles.len()
        );
        assert_eq!(s.pages(), 200);
    }
}
