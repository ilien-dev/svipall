//! Deciding what to crawl next.
//!
//! Breadth-first order treats every link as equally worth following, so a crawl with a `max_pages`
//! budget spends it on whatever the page happened to link first — pagination, tag indexes, author
//! archives — and may never reach what was asked for.
//!
//! The score is assembled from signals that are all O(1) per URL. Relevance uses the same
//! tokenisation as the BM25 filter, over the URL path and the anchor text, with the important
//! detail that the term statistics come from pages already fetched: the IDF is the site's own,
//! not a guess.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Seed,
    Feed,
    Sitemap,
    Link,
}

impl Source {
    /// How much a URL's provenance says about its worth. A feed is curated and recent; a link is
    /// whatever happened to be on the page.
    fn prior(self) -> f32 {
        match self {
            Source::Seed => 1.0,
            Source::Feed => 0.8,
            Source::Sitemap => 0.6,
            Source::Link => 0.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub url: String,
    pub depth: u16,
    pub source: Source,
    pub anchor: String,
    /// Score of the page this link was found on. Carrying it forward is what turns a greedy
    /// choice into a best-first search.
    pub parent_score: f32,
    /// Seconds since the epoch, from a sitemap `lastmod` or a feed date.
    pub lastmod: Option<i64>,
}

impl Candidate {
    pub fn seed(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            depth: 0,
            source: Source::Seed,
            anchor: String::new(),
            parent_score: 1.0,
            lastmod: None,
        }
    }
}

struct Scored {
    score: f32,
    candidate: Candidate,
}

impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}
impl Eq for Scored {}
impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap, and scores are never NaN by construction.
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Path fragments that reliably lead somewhere useful, and somewhere useless.
const GOOD_PATH: &[&str] = &[
    "/docs",
    "/doc/",
    "/guide",
    "/tutorial",
    "/reference",
    "/api",
    "/manual",
    "/blog",
    "/article",
    "/post",
    "/faq",
    "/help",
];
const BAD_PATH: &[&str] = &[
    "/tag/",
    "/tags/",
    "/author/",
    "/category/",
    "/print/",
    "/amp/",
    "/login",
    "/signup",
    "/cart",
    "/checkout",
    "?page=",
    "?sort=",
    "?filter=",
    "/feed/",
    "/rss",
];

/// How the queue is drained.
///
/// Three shapes, and they answer different questions. Best-first answers "what on this site is
/// about X", which is what a query means. Breadth-first answers "what is this site", which is what
/// a map means. Depth-first answers "what is at the end of this path", which is what a
/// documentation section or a paginated listing means — and it is the one that was missing: with a
/// breadth-first queue, chapter two of a manual is read after every page of chapter one's
/// navigation furniture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Highest-scoring first. What a query asks for.
    #[default]
    Best,
    /// Level by level.
    Breadth,
    /// Follow one branch to its end before starting the next.
    Depth,
}

pub struct Frontier {
    heap: BinaryHeap<Scored>,
    seen: HashSet<u64>,
    terms: Vec<String>,
    /// Document frequency per term, learned from pages already fetched.
    df: HashMap<String, u32>,
    docs: u32,
    now: i64,
    order: Order,
    /// Counts pushes, so two candidates at the same depth still have an order. Without it a
    /// depth-first walk fans out across whichever sibling the heap happens to return.
    pushed: u32,
}

impl Frontier {
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
            heap: BinaryHeap::new(),
            seen: HashSet::new(),
            terms,
            df: HashMap::new(),
            order: Order::Best,
            pushed: 0,
            docs: 0,
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }

    /// Fold a fetched page into the term statistics, so later scoring uses the site's own IDF
    /// rather than an empty corpus.
    pub fn observe_page(&mut self, text: &str) {
        self.docs += 1;
        if self.terms.is_empty() {
            return;
        }
        let lower = text.to_ascii_lowercase();
        for t in &self.terms {
            if lower.contains(t.as_str()) {
                *self.df.entry(t.clone()).or_insert(0) += 1;
            }
        }
    }

    fn relevance(&self, c: &Candidate) -> f32 {
        if self.terms.is_empty() {
            return 0.0;
        }
        let hay = format!("{} {}", c.url, c.anchor).to_ascii_lowercase();
        let mut score = 0.0;
        for t in &self.terms {
            if !hay.contains(t.as_str()) {
                continue;
            }
            // A term on few pages is worth more than one on all of them. Before any page has been
            // seen this is uniform, which is the right default.
            let df = self.df.get(t).copied().unwrap_or(0) as f32;
            let idf = ((self.docs as f32 - df + 0.5) / (df + 0.5) + 1.0).ln();
            score += idf.max(0.1);
        }
        score / self.terms.len() as f32
    }

    /// The same frontier, drained in a different order.
    pub fn ordered(mut self, order: Order) -> Self {
        self.order = order;
        self
    }

    /// Where this candidate sits in the queue, under the chosen order.
    ///
    /// The heap is a max-heap and knows nothing about any of this: every order is expressed as a
    /// number, which keeps one queue, one resume format and one set of edge cases.
    fn priority(&self, c: &Candidate) -> f32 {
        match self.order {
            Order::Best => self.score(c),
            // Shallower first, and later siblings after earlier ones.
            Order::Breadth => -(c.depth as f32) - 1e-6 * self.pushed as f32,
            // Deeper first, and the most recently found link before its siblings, which is what
            // makes it a walk down one branch rather than a sweep across all of them.
            Order::Depth => c.depth as f32 + 1e-6 * self.pushed as f32,
        }
    }

    fn score(&self, c: &Candidate) -> f32 {
        let lower = c.url.to_ascii_lowercase();
        let mut s = 0.0;

        s += 1.2 * self.relevance(c);
        s += 0.6 * c.source.prior();
        s += 0.4 * c.parent_score;
        s -= 0.25 * c.depth as f32;

        // Deep paths are usually further from anything anyone asked for.
        let segments = lower.matches('/').count().saturating_sub(2);
        s -= 0.08 * segments as f32;

        if GOOD_PATH.iter().any(|g| lower.contains(g)) {
            s += 0.5;
        }
        if BAD_PATH.iter().any(|b| lower.contains(b)) {
            s -= 0.6;
        }
        // Faceted navigation multiplies into thousands of near-identical pages.
        let params = lower.split('&').count().saturating_sub(1);
        if params > 3 {
            s -= 0.5;
        }
        if let Some(lastmod) = c.lastmod {
            let age_days = ((self.now - lastmod).max(0) as f32) / 86_400.0;
            s += 0.4 * (-age_days / 90.0).exp();
        }
        s
    }

    /// Queue a candidate. Returns false when it is a duplicate.
    ///
    /// Deduplication happens on the *normalised* URL, so `?utm_source=` variants of one page are
    /// recognised as the same page instead of being fetched again.
    pub fn push(&mut self, c: Candidate) -> bool {
        let Some(norm) = crate::domain::normalize_url(&c.url) else {
            return false;
        };
        if !self.seen.insert(crate::domain::stable_hash(&norm)) {
            return false;
        }
        self.pushed = self.pushed.saturating_add(1);
        let score = self.priority(&c);
        self.heap.push(Scored {
            score,
            candidate: c,
        });
        true
    }

    /// Re-queue a candidate with the score it already had, for a crawl being resumed.
    ///
    /// Recomputing it would be wrong: the saved score came from a frontier that had learned the
    /// site's own term statistics, and a fresh `Frontier` has an empty corpus. Rescoring would
    /// silently reorder the queue on every resume.
    pub fn restore(&mut self, url: String, depth: u16, score: f32) -> bool {
        let Some(norm) = crate::domain::normalize_url(&url) else {
            return false;
        };
        if !self.seen.insert(crate::domain::stable_hash(&norm)) {
            return false;
        }
        self.heap.push(Scored {
            score,
            candidate: Candidate {
                url,
                depth,
                source: Source::Link,
                anchor: String::new(),
                parent_score: score,
                lastmod: None,
            },
        });
        true
    }

    /// Put back a candidate that was popped but never handled.
    ///
    /// `push` refuses it, and rightly: the URL is already in `seen`, which is what stops a crawl
    /// fetching the same page twice. But a URL a gate declined to request was never fetched at
    /// all, and dropping it silently is how a page goes missing from a crawl that reports success.
    /// Popping is not handling, so this is the one place allowed to say so.
    pub fn requeue(&mut self, c: Candidate) {
        let score = self.priority(&c);
        self.heap.push(Scored {
            score,
            candidate: c,
        });
    }

    /// Record a URL as already handled without queueing it. A resumed crawl marks everything it
    /// fetched last time, so links back to those pages are not fetched twice.
    pub fn mark_seen(&mut self, url: &str) {
        if let Some(norm) = crate::domain::normalize_url(url) {
            self.seen.insert(crate::domain::stable_hash(&norm));
        }
    }

    /// Highest-scoring candidates first.
    pub fn pop_batch(&mut self, n: usize) -> Vec<(Candidate, f32)> {
        (0..n)
            .filter_map(|_| self.heap.pop())
            .map(|s| (s.candidate, s.score))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Everything still queued, for persisting a crawl that will be resumed.
    pub fn snapshot(&self) -> Vec<(String, u16, f32)> {
        self.heap
            .iter()
            .map(|s| (s.candidate.url.clone(), s.candidate.depth, s.score))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str) -> Candidate {
        Candidate {
            url: url.into(),
            depth: 1,
            source: Source::Link,
            anchor: String::new(),
            parent_score: 0.5,
            lastmod: None,
        }
    }

    fn order(f: &mut Frontier) -> Vec<String> {
        f.pop_batch(100).into_iter().map(|(c, _)| c.url).collect()
    }

    #[test]
    fn a_depth_first_crawl_follows_one_branch_to_its_end() {
        // With a breadth-first queue, chapter two of a manual is read after every page of chapter
        // one's navigation furniture. Depth-first is what "read this section" actually means.
        let mut f = Frontier::new(None).ordered(Order::Depth);
        f.push(Candidate {
            url: "https://x.test/a".into(),
            depth: 1,
            source: Source::Link,
            anchor: String::new(),
            parent_score: 0.5,
            lastmod: None,
        });
        f.push(Candidate {
            url: "https://x.test/a/b/c".into(),
            depth: 3,
            source: Source::Link,
            anchor: String::new(),
            parent_score: 0.5,
            lastmod: None,
        });
        f.push(Candidate {
            url: "https://x.test/a/b".into(),
            depth: 2,
            source: Source::Link,
            anchor: String::new(),
            parent_score: 0.5,
            lastmod: None,
        });
        let order: Vec<String> = f.pop_batch(3).into_iter().map(|(c, _)| c.url).collect();
        assert_eq!(
            order,
            vec![
                "https://x.test/a/b/c".to_string(),
                "https://x.test/a/b".to_string(),
                "https://x.test/a".to_string(),
            ]
        );
    }

    #[test]
    fn siblings_at_the_same_depth_still_have_an_order() {
        // Without a tiebreak a depth-first walk fans out across whichever sibling the heap happens
        // to return, which is neither depth-first nor reproducible.
        let mut f = Frontier::new(None).ordered(Order::Depth);
        for url in ["https://x.test/1", "https://x.test/2", "https://x.test/3"] {
            f.push(Candidate {
                url: url.into(),
                depth: 1,
                source: Source::Link,
                anchor: String::new(),
                parent_score: 0.5,
                lastmod: None,
            });
        }
        let order: Vec<String> = f.pop_batch(3).into_iter().map(|(c, _)| c.url).collect();
        assert_eq!(order[0], "https://x.test/3", "the newest link goes first");
        assert_eq!(order[2], "https://x.test/1");
    }

    #[test]
    fn a_breadth_first_crawl_finishes_a_level_before_starting_the_next() {
        let mut f = Frontier::new(None).ordered(Order::Breadth);
        for (url, depth) in [
            ("https://x.test/deep/a/b", 3u16),
            ("https://x.test/top", 1),
            ("https://x.test/mid/a", 2),
        ] {
            f.push(Candidate {
                url: url.into(),
                depth,
                source: Source::Link,
                anchor: String::new(),
                parent_score: 0.5,
                lastmod: None,
            });
        }
        let order: Vec<String> = f.pop_batch(3).into_iter().map(|(c, _)| c.url).collect();
        assert_eq!(order[0], "https://x.test/top");
        assert_eq!(order[2], "https://x.test/deep/a/b");
    }

    #[test]
    fn the_default_order_is_the_one_that_was_there_before() {
        // Changing what an unqualified crawl does would be a silent behaviour change for every
        // caller that never asked for a strategy.
        assert_eq!(Order::default(), Order::Best);
    }

    #[test]
    fn a_restored_candidate_keeps_the_score_it_was_saved_with() {
        let mut f = Frontier::new(Some("rust ownership"));
        f.restore("https://x.test/deep/deep/deep/page".into(), 4, 9.0);
        f.push(link("https://x.test/docs/ownership"));
        let out = f.pop_batch(2);
        assert_eq!(
            out[0].0.url, "https://x.test/deep/deep/deep/page",
            "rescoring on resume would have buried it: {out:?}"
        );
        assert!((out[0].1 - 9.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_page_already_fetched_is_not_queued_again() {
        let mut f = Frontier::new(None);
        f.mark_seen("https://x.test/a?utm_source=news");
        assert!(!f.push(link("https://x.test/a")), "normalised duplicate");
        assert!(f.is_empty());
    }

    #[test]
    fn the_highest_scoring_candidate_comes_out_first() {
        let mut f = Frontier::new(Some("ownership borrowing"));
        f.push(link("https://x.test/tag/misc"));
        f.push(link("https://x.test/docs/ownership-and-borrowing"));
        f.push(link("https://x.test/about"));
        assert_eq!(
            order(&mut f).first().map(String::as_str),
            Some("https://x.test/docs/ownership-and-borrowing")
        );
    }

    #[test]
    fn anchor_text_counts_as_much_as_the_path() {
        let mut f = Frontier::new(Some("lifetimes"));
        let mut anchored = link("https://x.test/p/123");
        anchored.anchor = "A guide to lifetimes".into();
        f.push(anchored);
        f.push(link("https://x.test/p/456"));
        assert_eq!(
            order(&mut f).first().map(String::as_str),
            Some("https://x.test/p/123")
        );
    }

    #[test]
    fn tracking_variants_are_recognised_as_the_same_page() {
        let mut f = Frontier::new(None);
        assert!(f.push(link("https://x.test/a")));
        assert!(
            !f.push(link("https://x.test/a?utm_source=twitter")),
            "a tracking parameter is not a different page"
        );
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn shallower_pages_win_all_else_equal() {
        let mut f = Frontier::new(None);
        let mut deep = link("https://x.test/a/b/c/d");
        deep.depth = 4;
        let mut shallow = link("https://x.test/e");
        shallow.depth = 1;
        f.push(deep);
        f.push(shallow);
        assert_eq!(
            order(&mut f).first().map(String::as_str),
            Some("https://x.test/e")
        );
    }

    #[test]
    fn faceted_and_archive_urls_sink() {
        let mut f = Frontier::new(None);
        f.push(link("https://x.test/products?page=7&sort=asc&filter=b&x=1"));
        f.push(link("https://x.test/guide/start"));
        f.push(link("https://x.test/author/someone"));
        let out = order(&mut f);
        assert_eq!(
            out.first().map(String::as_str),
            Some("https://x.test/guide/start")
        );
        assert!(out.last().unwrap().contains("page=7") || out.last().unwrap().contains("author"));
    }

    #[test]
    fn a_feed_entry_outranks_a_plain_link_from_the_same_depth() {
        let mut f = Frontier::new(None);
        let mut feed = link("https://x.test/n/1");
        feed.source = Source::Feed;
        f.push(feed);
        f.push(link("https://x.test/n/2"));
        assert_eq!(
            order(&mut f).first().map(String::as_str),
            Some("https://x.test/n/1")
        );
    }

    #[test]
    fn recent_pages_outrank_stale_ones() {
        let mut f = Frontier::new(None);
        let now = f.now;
        let mut fresh = link("https://x.test/fresh");
        fresh.lastmod = Some(now - 86_400);
        let mut old = link("https://x.test/old");
        old.lastmod = Some(now - 400 * 86_400);
        f.push(old);
        f.push(fresh);
        assert_eq!(
            order(&mut f).first().map(String::as_str),
            Some("https://x.test/fresh")
        );
    }

    /// The reason IDF is learned from fetched pages: a term on every page tells you nothing about
    /// which page to read next.
    #[test]
    fn a_term_that_appears_everywhere_stops_driving_the_ranking() {
        let mut common = Frontier::new(Some("rust guide"));
        for _ in 0..20 {
            common.observe_page("rust rust rust on every single page of this site");
        }
        let mut rare = Frontier::new(Some("rust guide"));
        for _ in 0..20 {
            rare.observe_page("nothing relevant on this page at all");
        }
        let c = link("https://x.test/p/rust");
        assert!(
            common.score(&c) < rare.score(&c),
            "a ubiquitous term should carry less weight, got {} vs {}",
            common.score(&c),
            rare.score(&c)
        );
    }

    #[test]
    fn a_snapshot_covers_everything_still_queued() {
        let mut f = Frontier::new(None);
        for i in 0..5 {
            f.push(link(&format!("https://x.test/p{i}")));
        }
        assert_eq!(f.snapshot().len(), 5);
        f.pop_batch(2);
        assert_eq!(f.snapshot().len(), 3);
    }

    #[test]
    fn an_unparseable_url_is_refused_rather_than_queued() {
        let mut f = Frontier::new(None);
        assert!(!f.push(link("not a url at all")));
        assert!(f.is_empty());
    }
}
