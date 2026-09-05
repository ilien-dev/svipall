//! What came back, judged as content rather than as a wall.
//!
//! `classify` answers "did we get through". This answers the question after it: the request
//! succeeded, so is what arrived the page, part of the page, or a husk of one. The two are kept
//! apart because they earn different rights — a wall may send the ladder up a tier, a thin page
//! may not, and conflating them turns "this article is short" into three wasted browser launches.
//!
//! Every rule here is **language-neutral on purpose**. The obvious extra signal is a stop-word
//! test, and the obvious way to write it is against English. Dodge et al. measured what that costs
//! on C4: a filter that looked reasonable removed African-American English at 42% and
//! Hispanic-aligned English at 32%, against 6.2% for the majority dialect. A tool that fetches the
//! whole web cannot afford a rule that reads "not English" as "not prose", so the tests below use
//! only shape — length, symbols, alphabetic share, repetition — and never vocabulary.
//!
//! Nothing here ever removes a document. It labels one.

pub mod calibration;
pub mod credibility;
pub mod diversity;
pub mod substance;

use serde::{Deserialize, Serialize};

/// How much of the page we appear to have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Nothing to report.
    Full,
    /// The text is cut off: what arrived is the start of something longer.
    Partial,
    /// It arrived whole and there is almost nothing in it.
    Thin,
}

/// Why a verdict is not `Full`. A bare score explains nothing; these are what the caller acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Fewer words than the shortest thing worth calling a document.
    ThinText,
    /// Text is a sliver of the markup that carried it — chrome, not content.
    LowTextRatio,
    /// Ends mid-thought, on an ellipsis: a teaser where an article should be.
    Truncated,
    /// Word shapes are not those of running text: a listing, a menu dump, a table of codes.
    NotProse,
    /// One phrase covers much of the page. Gopher's repetition rule, and the shape of a
    /// generated filler page.
    Repetitive,
    /// A specific page was asked for and the front page answered. `session::Verdict` has always
    /// known this and only ever told the throttle; the caller, who asked for the article, was
    /// handed the home page with nothing said about it.
    LandedElsewhere,
    /// Most of what came back also appears on every other page of this site: navigation, footer
    /// and cookie banner, with little else. Only a caller that has seen the rest of the site can
    /// know this, so a single fetch never reports it.
    MostlyBoilerplate,
}

impl Reason {
    /// The wire name, so a caller reads `thin_text` rather than a number.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::ThinText => "thin_text",
            Reason::LowTextRatio => "low_text_ratio",
            Reason::Truncated => "truncated",
            Reason::NotProse => "not_prose",
            Reason::Repetitive => "repetitive",
            Reason::LandedElsewhere => "landed_elsewhere",
            Reason::MostlyBoilerplate => "mostly_boilerplate",
        }
    }
}

/// Everything a verdict is drawn from, so the judgement stays a pure function with no I/O and no
/// second parse. Fields a given caller cannot know are left empty rather than guessed: a single
/// fetch has no view of the rest of the site, and saying so is better than inventing a number.
#[derive(Debug, Default, Clone, Copy)]
pub struct Evidence<'a> {
    /// Size of the markup the text was extracted from.
    pub html_len: usize,
    /// The extraction itself.
    pub text: &'a str,
    /// What was asked for, and what answered. Both or neither.
    pub requested_url: Option<&'a str>,
    pub final_url: Option<&'a str>,
    /// Share of this page.s blocks that also appear across the rest of the site, where the caller
    /// has seen enough of it to say. A crawl knows; a lone fetch does not.
    pub boilerplate_share: Option<f32>,
    /// The text the caller was actually handed, when it differs from the whole document.
    ///
    /// ▲ Which of the two a rule reads is a decision, not a detail. `LowTextRatio` is a ratio
    /// against the markup and so must keep reading the whole page; every other rule is about what
    /// arrived, and judging those on navigation and a footer the caller never received was scoring
    /// a different page from the one that was delivered.
    pub delivered: Option<&'a str>,
}

impl<'a> Evidence<'a> {
    /// The two things every caller has.
    pub fn new(html_len: usize, text: &'a str) -> Self {
        Self {
            html_len,
            text,
            ..Default::default()
        }
    }

    /// Where the request went, when the caller followed it.
    pub fn between(mut self, requested: &'a str, final_url: &'a str) -> Self {
        self.requested_url = Some(requested);
        self.final_url = Some(final_url);
        self
    }

    /// What the caller was handed, when the pruner removed anything.
    pub fn delivered(mut self, text: &'a str) -> Self {
        self.delivered = Some(text);
        self
    }

    /// What the rest of the site looks like, when the caller has seen it.
    pub fn against_site(mut self, share: f32) -> Self {
        self.boilerplate_share = Some(share);
        self
    }
}

/// Above this share of blocks shared with the rest of the site, the page is the site's frame with
/// something small inside it.
pub const MAX_BOILERPLATE_SHARE: f32 = 0.8;

/// How heavily a page has been engineered for a search engine.
///
/// Not a quality score, and read in the opposite direction from one. Bevendorff et al. (ECIR 2024)
/// monitored three search engines for a year over 7,392 product-review queries and found rank
/// correlating at R² = .99 with affiliate links, .96 with heading-keyword echo and **−.88 with
/// lexical diversity**: the better a page ranks, the more referral links and keyword stuffing it
/// carries and the less varied its language. Lewandowski et al. (2021) put the share of the web
/// that is search-optimised at 80% or more, and Schultheiß et al. found an inverse relationship
/// between a page's optimisation level and its perceived expertise.
///
/// So this is reported, never acted on. It never removes a page, never reorders one on its own,
/// and never subtracts from a verdict — a heavily optimised page can still be the one that holds
/// the answer, and the caller is the one who gets to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Nothing that stands out. Most of the web is optimised; only the far end is worth saying.
    Ordinary,
    High,
}

/// What made it stand out. Named, because a level with no reasons is a number nobody can argue
/// with — and this is a measure that deserves to be argued with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trait {
    /// Referral links, in bulk and in proportion to how little else is here.
    AffiliateHeavy,
    /// The headings contain nothing the body does not: written for a query, not for a reader.
    HeadingsEchoTheBody,
    /// More anchors than prose. The listicle shape.
    LinkDense,
}

impl Trait {
    pub fn as_str(self) -> &'static str {
        match self {
            Trait::AffiliateHeavy => "affiliate_heavy",
            Trait::HeadingsEchoTheBody => "headings_echo_the_body",
            Trait::LinkDense => "link_dense",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Optimization {
    pub level: Level,
    pub traits: Vec<Trait>,
}

/// Referral links this many or more, *and* dense enough to be the point of the page.
const AFFILIATE_FLOOR: u32 = 3;

/// Referral links per thousand words, above which the links are what the page is for.
///
/// Calibrated above what the study measured at the top of the rankings, not at it. Its
/// best-ranked product reviews averaged 3–6 referral links in 1,200–2,000 words of main content,
/// which is about 3 per thousand — and the point of the paper is that those pages are the problem.
/// Firing there would label the median commercial page, which is true and useless. This fires
/// where the links are plainly the reason the page exists.
const AFFILIATE_PER_KWORD: f32 = 5.0;

/// Below this many words a ratio is noise: three links on a stub is not a link farm, and dividing
/// by a two-line page produces a density that means nothing. The same guard `heading_echo` gets
/// from needing three headings.
const MIN_WORDS_FOR_DENSITY: usize = 200;
/// Heading words already in the body, above which the headings say nothing of their own.
const MAX_HEADING_ECHO: f32 = 0.95;
/// Anchors per hundred words, above which there is more navigation than prose.
const MAX_LINKS_PER_HWORD: f32 = 4.0;

/// How engineered this page is, from counts taken during the one parse.
///
/// Two traits are needed for `High`. One alone is ordinary: plenty of honest pages carry a few
/// referral links, and a glossary is legitimately link-dense. It is the combination that the study
/// found at the top of the rankings.
pub fn optimization(s: &crate::extraction::Signals, text: &str) -> Optimization {
    let count = text.split_whitespace().take(SAMPLE_WORDS).count();
    let dense_enough_to_judge = count >= MIN_WORDS_FOR_DENSITY;
    let words = count.max(1) as f32;
    let mut traits = Vec::new();

    if dense_enough_to_judge
        && s.affiliate_anchors >= AFFILIATE_FLOOR
        && s.affiliate_anchors as f32 * 1000.0 / words > AFFILIATE_PER_KWORD
    {
        traits.push(Trait::AffiliateHeavy);
    }
    // Only when there are enough headings for the share to mean anything: one heading that happens
    // to reuse two body words is not keyword stuffing.
    if s.headings >= 3 && s.heading_echo > MAX_HEADING_ECHO {
        traits.push(Trait::HeadingsEchoTheBody);
    }
    if dense_enough_to_judge && s.anchors as f32 * 100.0 / words > MAX_LINKS_PER_HWORD {
        traits.push(Trait::LinkDense);
    }

    let level = if traits.len() >= 2 {
        Level::High
    } else {
        Level::Ordinary
    };
    Optimization { level, traits }
}

/// Words read when turning counts into densities. Same reasoning as `SAMPLE_CHARS`: a ratio is a
/// statistic, and a bounded sample answers it.
const SAMPLE_WORDS: usize = 4_000;

/// Are these sources actually different sources?
///
/// Five results carrying one wire story are one source wearing five hostnames, and until something
/// says so they read as five confirmations. This says how many of a set are distinct — and stops
/// there. It is **not** a trustworthiness measure and never becomes one.
///
/// Estimating whether a source is *right* is a different problem with a known price: Dong et al.,
/// *Knowledge-Based Trust* (VLDB 2015), needed 2.8 billion extracted facts over 119 million pages
/// and a knowledge base to do it. That is not reproducible on one machine and is not what this is.
/// What is reported here is corroboration and provenance, under those names, and the caller draws
/// whatever inference it likes.
pub mod provenance {
    use serde::{Deserialize, Serialize};

    /// Hamming distance at which two 64-bit simhashes are the same document.
    ///
    /// Manku et al. (WWW 2007) validated `k = 3` over an 8-billion-page repository at Google, which
    /// remains the only measurement of this at web scale.
    pub const NEAR_DUPLICATE_BITS: u32 = 3;

    /// What a set of results is actually made of.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Corroboration {
        /// How many distinct documents are in the set.
        pub independent: usize,
        /// How many results the largest group of duplicates holds. `1` when nothing repeats.
        pub largest_group: usize,
    }

    /// Group a set of fingerprints, keeping the input order.
    ///
    /// Returns, for each input, the index of the first result it duplicates — `None` when it is
    /// the first of its kind. Nothing is removed: the caller is told which results say the same
    /// thing, and decides for itself what to do about it.
    pub fn group(hashes: &[u64]) -> (Vec<Option<usize>>, Corroboration) {
        let mut first_of_kind: Vec<usize> = Vec::new();
        let mut duplicate_of: Vec<Option<usize>> = Vec::with_capacity(hashes.len());
        let mut sizes: Vec<usize> = Vec::new();

        for &h in hashes {
            // An empty document hashes to zero, and two blank pages are not evidence of anything.
            let found = (h != 0).then(|| {
                first_of_kind
                    .iter()
                    .position(|&i| crate::dedup::hamming(hashes[i], h) <= NEAR_DUPLICATE_BITS)
            });
            match found.flatten() {
                Some(group_idx) => {
                    sizes[group_idx] += 1;
                    duplicate_of.push(Some(first_of_kind[group_idx]));
                }
                None => {
                    first_of_kind.push(duplicate_of.len());
                    sizes.push(1);
                    duplicate_of.push(None);
                }
            }
        }

        let c = Corroboration {
            independent: first_of_kind.len(),
            largest_group: sizes.iter().copied().max().unwrap_or(0),
        };
        (duplicate_of, c)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::dedup::simhash;

        #[test]
        fn one_wire_story_on_five_sites_is_one_source() {
            // The failure this exists for. Five hostnames, one newsroom, and until now five
            // confirmations.
            let wire = "The council voted on Tuesday to approve the harbour redevelopment after a \
                        debate that ran past midnight, with eleven in favour and four against.";
            let hashes: Vec<u64> = (0..5).map(|_| simhash(wire)).collect();
            let (dupes, c) = group(&hashes);
            assert_eq!(c.independent, 1);
            assert_eq!(c.largest_group, 5);
            assert_eq!(dupes[0], None, "the first of a kind is nobody's duplicate");
            assert!(dupes[1..].iter().all(|d| *d == Some(0)));
        }

        #[test]
        fn genuinely_different_reporting_is_counted_as_different() {
            let a = simhash("The council voted on Tuesday to approve the harbour redevelopment.");
            let b =
                simhash("Ferry operators say the eastern quay closure will cost them a season.");
            let c = simhash("A planning inspector has asked for the traffic study to be redone.");
            let (dupes, corr) = group(&[a, b, c]);
            assert_eq!(corr.independent, 3);
            assert_eq!(corr.largest_group, 1);
            assert!(dupes.iter().all(Option::is_none));
        }

        #[test]
        fn nothing_is_ever_removed_only_labelled() {
            let wire = simhash("One story, syndicated everywhere, word for word.");
            let (dupes, _) = group(&[wire, wire, wire]);
            assert_eq!(dupes.len(), 3, "every input still has an answer");
        }

        #[test]
        fn two_empty_pages_are_not_evidence_of_syndication() {
            let (dupes, c) = group(&[0, 0, 0]);
            assert_eq!(c.independent, 3, "an empty page duplicates nothing");
            assert!(dupes.iter().all(Option::is_none));
        }
    }
}

/// How alike a page must be to a site's own not-found page before it is one.
///
/// Bar-Yossef et al. (WWW 2004) named the method: a site that answers 200 for an address that is
/// not there is asked, once, what that page looks like, and everything else is compared to it. The
/// comparison is a simhash, so it works in any language and needs no phrase list — which is the
/// half of soft-404 detection that a vocabulary can never cover.
///
/// Manku et al. (WWW 2007) put near-duplicate web pages at a Hamming distance of 3 in 64 bits,
/// which is 0.953. This sits below that because a not-found template often echoes the address that
/// was asked for, and those few words are the only thing that differs between two of them.
pub const SOFT_404_SIMILARITY: f32 = 0.9;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Integrity {
    pub verdict: Verdict,
    pub reasons: Vec<Reason>,
}

impl Default for Integrity {
    /// A page nobody found anything to say about. The same as `full`, spelled so `Record` can
    /// derive its own default without choosing a verdict by field order.
    fn default() -> Self {
        Self::full()
    }
}

impl Integrity {
    /// Nothing worth saying about this page.
    pub fn full() -> Self {
        Self {
            verdict: Verdict::Full,
            reasons: Vec::new(),
        }
    }

    /// Is there anything here the caller needs to be told?
    pub fn is_full(&self) -> bool {
        self.verdict == Verdict::Full
    }
}

/// Everything a fetch measured about a page, in one serialisable place.
///
/// ▲ This exists because of a bug it now makes impossible. The verdict was stored beside a cached
/// page and the optimisation level and substance label were not, so the same URL answered
/// differently depending on whether the cache happened to hold it — three of six fields present on
/// a fetch and absent on a hit, with nothing saying which had happened. A cache is supposed to
/// change how fast an answer arrives, never what the answer is.
///
/// The fields that cannot be re-derived on a hit are exactly the ones stored: a hit has the
/// markdown but not the markup it came out of, so `html_len` is gone and `Signals` with it, and
/// the classifier's answer costs a model load that a cache hit exists to avoid.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Record {
    pub integrity: Integrity,
    /// How engineered the page is. `None` means nobody measured it — which is a different
    /// statement from `Ordinary`, and the reason both are written down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimization: Option<Optimization>,
    /// What the classifier made of it, when this machine has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substance: Option<substance::Substance>,
}

impl Record {
    /// Read back what was stored beside a cached page.
    ///
    /// Rows written before this type existed hold a bare `Integrity`, and they are still readable:
    /// an old cache is not a reason to lose a verdict that was correctly recorded. Anything else —
    /// a truncated row, a format from the future — is `None`, because a page labelled from
    /// guesswork is worse than one labelled `None`.
    pub fn parse(raw: &str) -> Option<Record> {
        if let Ok(r) = serde_json::from_str::<Record>(raw) {
            return Some(r);
        }
        serde_json::from_str::<Integrity>(raw)
            .ok()
            .map(Record::from)
    }
}

impl From<Integrity> for Record {
    fn from(integrity: Integrity) -> Self {
        Self {
            integrity,
            ..Default::default()
        }
    }
}
/// Gopher's floor: fewer words than this and there is no document, whatever the markup claimed.
const MIN_WORDS: usize = 50;

/// The same floor counted in characters, and the reason both exist.
///
/// A word count is not the neutral measure it looks like. German says in 45 compound words what
/// English says in 55, and Chinese and Japanese put no spaces between words at all, so a
/// whitespace splitter reports one "word" for a full page. A word-count rule alone therefore reads
/// "not English" as "not a document" — the failure this module exists to avoid. A page is thin
/// only when it is short by *both* measures, which no real document is and every nav strip is.
const MIN_CHARS: usize = 300;

/// Gopher's word-shape window. Outside it the "words" are identifiers, hashes or CJK run through
/// a whitespace splitter, none of which is prose.
const MIN_MEAN_WORD_LEN: f32 = 3.0;
const MAX_MEAN_WORD_LEN: f32 = 10.0;

/// Gopher's symbol-to-word ratio. Above it the page is mostly hashes and ellipses.
const MAX_SYMBOL_RATIO: f32 = 0.1;

/// Gopher's alphabetic-word share. Below it the page is numbers and punctuation.
const MIN_ALPHA_SHARE: f32 = 0.8;

/// Gopher's 2-gram repetition ceiling: the share of characters the single commonest word pair may
/// account for before the page is filler.
const MAX_TOP_BIGRAM_SHARE: f32 = 0.20;

/// Below this share of the markup, the text is chrome. The shell rule in `classify` already
/// catches 2% as a wall; this is the band above it, where a page did arrive and is mostly frame.
const MIN_TEXT_RATIO: f64 = 0.05;

/// The character floor for the script in hand.
///
/// A Han or kana character carries about as much as an English word, not as much as an English
/// letter, so the same paragraph is roughly a third of the characters. Holding CJK to the Latin
/// floor would mark ordinary Japanese pages thin and ordinary English ones full — the same bias as
/// a stop-word test, arriving by arithmetic instead of by vocabulary.
fn min_chars(spaceless: bool) -> usize {
    if spaceless {
        MIN_CHARS / 3
    } else {
        MIN_CHARS
    }
}

/// A page whose markup is smaller than this is a small page, not a shell — the ratio rule would
/// otherwise flag every short, tidy document that has no boilerplate to dilute it.
const RATIO_FLOOR_HTML: usize = 20_000;

/// How much of the text the shape rules read.
///
/// They are statistics — mean word length, alphabetic share, how often the commonest pair recurs —
/// and a sample answers them as well as a whole document does. Bounding it is what keeps the cost
/// flat: this runs on every delivered page, beside a classification that is held to 400µs, and a
/// bigram table over an entire 200KB article is an order of magnitude more work than the parse
/// that produced it. The rules that must see the whole page — is it thin, is it cut off, is it a
/// sliver of its markup — are answered from the full text, and each of those is O(1) or a length.
const SAMPLE_CHARS: usize = 8_000;

/// The path part of a URL, so "landed on the front page" is a question about the path and not
/// about the host, the scheme or a port that a redirect may also have changed.
fn path_of(url: &str) -> String {
    let after_scheme = url.split_once("//").map(|(_, r)| r).unwrap_or(url);
    match after_scheme.find('/') {
        Some(i) => after_scheme[i..].to_string(),
        None => "/".to_string(),
    }
}

/// Largest char boundary at or below `limit`, so slicing a sample never panics.
fn sample(s: &str, limit: usize) -> &str {
    match s.char_indices().nth(limit) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Judge a delivered page.
///
/// Everything it reads is already in hand at the one place a fetched document passes through, so
/// this costs one bounded pass over the text and no second parse.
pub fn assess(e: &Evidence<'_>) -> Integrity {
    let html_len = e.html_len;
    // The whole document, for the one rule that is a ratio against the markup it came out of.
    let whole = e.text.trim();
    // What the caller was handed, for every rule that is about the page they received.
    let text = e.delivered.unwrap_or(e.text).trim();
    let mut reasons = Vec::new();

    let head = sample(text, SAMPLE_CHARS);
    let words: Vec<&str> = head.split_whitespace().collect();
    let head_chars = head.chars().count();
    // Chinese, Japanese and Thai put no spaces between words, so a whitespace splitter hands back
    // one enormous "word" for a whole page. The give-away is what an encoded payload does not
    // share: almost no spaces *and* mostly non-ASCII letters. Read off the sample, and the second
    // half only when the first is true, so an ordinary page never pays for it.
    let spaceless = words.len() * 20 < head_chars
        && head
            .chars()
            .filter(|c| !c.is_ascii() && c.is_alphabetic())
            .count()
            * 2
            > head_chars;

    // Thinness is a property of the whole page, but answering it never costs more than the
    // threshold: both counts stop as soon as the page has cleared them.
    let floor = min_chars(spaceless);
    let short_of_words = text.split_whitespace().take(MIN_WORDS).count() < MIN_WORDS;
    if short_of_words && text.chars().take(floor).count() < floor {
        reasons.push(Reason::ThinText);
    }

    if !words.is_empty() {
        let letters: usize = words.iter().map(|w| w.chars().count()).sum();
        let mean = letters as f32 / words.len() as f32;
        let alpha = words
            .iter()
            .filter(|w| w.chars().any(|c| c.is_alphabetic()))
            .count() as f32
            / words.len() as f32;
        // `#` and an ellipsis are the two Gopher counts: the first is a stripped hyperlink, the
        // second is a teaser. Both are markers of text that was cut rather than written.
        let symbols =
            head.matches('#').count() + head.matches('…').count() + head.matches("...").count();
        let symbol_ratio = symbols as f32 / words.len() as f32;

        // In a spaceless script the "words" are whole paragraphs, so the length window would call
        // a page of ordinary prose a hash. It is not applied there; every other rule still is.
        let odd_shape = !spaceless && !(MIN_MEAN_WORD_LEN..=MAX_MEAN_WORD_LEN).contains(&mean);

        if odd_shape || alpha < MIN_ALPHA_SHARE || symbol_ratio > MAX_SYMBOL_RATIO {
            reasons.push(Reason::NotProse);
        }

        if top_bigram_share(&words) > MAX_TOP_BIGRAM_SHARE {
            reasons.push(Reason::Repetitive);
        }
    }

    // Only the unambiguous marker. "Read more" at the end of a page is a link on a whole article
    // as often as it is the end of a teaser, and guessing between them would mislabel the pages
    // that are fine.
    if text.ends_with('…') || text.ends_with("...") {
        reasons.push(Reason::Truncated);
    }

    if html_len >= RATIO_FLOOR_HTML && (whole.len() as f64 / html_len as f64) < MIN_TEXT_RATIO {
        reasons.push(Reason::LowTextRatio);
    }

    // Asked for something specific, answered by the front page. A redirect the site chose, not a
    // wall, so it is said rather than acted on — but it is the difference between the article and
    // the home page, and until now nothing said it.
    if let (Some(req), Some(fin)) = (e.requested_url, e.final_url) {
        if crate::session::is_deep(&path_of(req)) && crate::session::is_root(&path_of(fin)) {
            reasons.push(Reason::LandedElsewhere);
        }
    }

    if e.boilerplate_share
        .is_some_and(|s| s > MAX_BOILERPLATE_SHARE)
    {
        reasons.push(Reason::MostlyBoilerplate);
    }

    let verdict = if reasons.contains(&Reason::Truncated) {
        Verdict::Partial
    } else if reasons.is_empty() {
        Verdict::Full
    } else {
        Verdict::Thin
    };
    Integrity { verdict, reasons }
}

/// Share of words covered by the commonest adjacent pair. Gopher's repetition measure, counted in
/// words rather than characters so one pass over the slice answers it.
fn top_bigram_share(words: &[&str]) -> f32 {
    if words.len() < 2 * MIN_WORDS {
        // On a handful of words every pair is a large share of the page, and saying so is noise.
        return 0.0;
    }
    let mut counts: std::collections::HashMap<(&str, &str), u32> =
        std::collections::HashMap::with_capacity(words.len());
    for pair in words.windows(2) {
        *counts.entry((pair[0], pair[1])).or_insert(0) += 1;
    }
    let top = counts.values().copied().max().unwrap_or(0);
    top as f32 * 2.0 / words.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ▲ The bug: a page whose article is three sentences inside a thousand words of navigation
    /// was judged on the navigation. The caller receives the article, so the article is what has
    /// to be judged — otherwise a page can be called whole on text nobody was handed.
    #[test]
    fn thinness_is_judged_on_what_the_caller_received_not_on_the_whole_page() {
        let chrome: String = (0..40)
            .map(|i| {
                format!(
                    "Section {i} Home About Contact Careers Privacy Terms Sitemap Newsletter \n                     Advertise Subscribe Archive Jobs Press Legal Accessibility Feedback "
                )
            })
            .collect();
        let article = "Three short lines is all there was.";
        let whole = format!("{chrome}{article}");

        let judged_on_the_page = assess(&Evidence::new(whole.len() * 4, &whole));
        assert!(
            !judged_on_the_page.reasons.contains(&Reason::ThinText),
            "a thousand words of navigation clears the thinness floor, which is the problem: \n             {judged_on_the_page:?}"
        );

        let judged_on_the_delivery =
            assess(&Evidence::new(whole.len() * 4, &whole).delivered(article));
        assert!(
            judged_on_the_delivery.reasons.contains(&Reason::ThinText),
            "what arrived was three sentences: {judged_on_the_delivery:?}"
        );
    }

    /// The ratio is a statement about the markup, so it keeps reading the whole document. Pointing
    /// it at the pruned text would make every well-pruned page look like chrome.
    #[test]
    fn the_markup_ratio_still_reads_the_whole_document() {
        let article: String = std::iter::repeat_n(
            "The measure passed after a long debate about drainage and the old library. ",
            60,
        )
        .collect();
        // Markup twenty times the size of its text: the ratio rule's own case.
        let html_len = article.len() * 10;
        let kept = &article[..200];
        let i = assess(&Evidence::new(html_len, &article).delivered(kept));
        assert!(
            !i.reasons.contains(&Reason::LowTextRatio),
            "the document is text-poor against its markup but the delivery is not chrome: {i:?}"
        );

        let thin_markup = assess(&Evidence::new(html_len, &article[..100]).delivered(&article));
        assert!(
            thin_markup.reasons.contains(&Reason::LowTextRatio),
            "a document that is a sliver of its markup is still a sliver of its markup: \
             {thin_markup:?}"
        );
    }

    #[test]
    fn with_nothing_delivered_the_document_is_what_is_judged() {
        let short = "too little";
        assert_eq!(
            assess(&Evidence::new(1000, short)),
            assess(&Evidence::new(1000, short).delivered(short)),
        );
    }

    /// ▲ The bug this type exists for. A page fetched and the same page served from the cache must
    /// carry the same six fields; before `Record` the last two were dropped on the way to disk.
    #[test]
    fn everything_measured_survives_a_round_trip_through_the_cache() {
        let r = Record {
            integrity: Integrity {
                verdict: Verdict::Thin,
                reasons: vec![Reason::ThinText, Reason::LowTextRatio],
            },
            optimization: Some(Optimization {
                level: Level::High,
                traits: vec![Trait::AffiliateHeavy, Trait::LinkDense],
            }),
            substance: Some(substance::Substance {
                label: substance::Label::Junk,
                confidence: 0.87,
            }),
        };
        let wire = serde_json::to_string(&r).expect("serialises");
        assert_eq!(Record::parse(&wire), Some(r));
    }

    /// A row written before this type existed holds a bare `Integrity`. Losing a verdict that was
    /// correctly recorded because the format around it grew would be the same bug in reverse.
    #[test]
    fn a_row_from_an_older_build_still_reads() {
        let old = serde_json::to_string(&Integrity {
            verdict: Verdict::Partial,
            reasons: vec![Reason::Truncated],
        })
        .expect("serialises");
        let r = Record::parse(&old).expect("an old row is still a record");
        assert_eq!(r.integrity.verdict, Verdict::Partial);
        assert_eq!(r.integrity.reasons, vec![Reason::Truncated]);
        assert_eq!(r.optimization, None, "an old row measured no optimisation");
        assert_eq!(r.substance, None);
    }

    /// `None` and `Ordinary` are different statements and the wire keeps them apart: absent means
    /// nobody looked, which is what a caller has to be able to tell.
    #[test]
    fn not_measured_is_not_the_same_as_measured_and_ordinary() {
        let unmeasured = Record::from(Integrity::full());
        let ordinary = Record {
            optimization: Some(Optimization {
                level: Level::Ordinary,
                traits: Vec::new(),
            }),
            ..Record::from(Integrity::full())
        };
        let a = serde_json::to_string(&unmeasured).expect("serialises");
        let b = serde_json::to_string(&ordinary).expect("serialises");
        assert_ne!(a, b);
        assert!(!a.contains("optimization"), "absent, not null: {a}");
        assert_eq!(Record::parse(&a), Some(unmeasured));
        assert_eq!(Record::parse(&b), Some(ordinary));
    }

    #[test]
    fn nonsense_beside_a_page_is_no_label_rather_than_a_guessed_one() {
        assert_eq!(Record::parse("not json"), None);
        assert_eq!(Record::parse("{}"), None);
    }

    /// Roughly the shape of a short news piece: enough words, ordinary shapes, no repetition.
    fn article() -> String {
        "The council voted on Tuesday to approve the measure after a long debate that ran past \
         midnight. Supporters argued that the change was overdue and that the money had already \
         been set aside; opponents said the figures had never been published in full. A second \
         reading is expected before the end of the month."
            .to_string()
    }

    #[test]
    fn an_ordinary_article_has_nothing_said_about_it() {
        let a = article();
        let got = assess(&Evidence::new(a.len() * 3, &a));
        assert_eq!(got.verdict, Verdict::Full, "{got:?}");
        assert!(got.reasons.is_empty(), "{got:?}");
    }

    #[test]
    fn a_page_with_almost_no_words_is_thin_and_says_which_way() {
        let got = assess(&Evidence::new(4_000, "Home About Contact"));
        assert_eq!(got.verdict, Verdict::Thin);
        assert!(got.reasons.contains(&Reason::ThinText), "{got:?}");
    }

    #[test]
    fn a_teaser_that_stops_mid_thought_is_partial_rather_than_thin() {
        // The distinction earns its keep: a thin page is all there is, a partial one has more
        // behind it, and only the second is worth another attempt.
        let text = format!("{}…", article());
        let got = assess(&Evidence::new(text.len() * 3, &text));
        assert_eq!(got.verdict, Verdict::Partial, "{got:?}");
        assert!(got.reasons.contains(&Reason::Truncated));
    }

    #[test]
    fn text_that_is_a_sliver_of_its_markup_is_reported_as_frame_not_content() {
        let a = article();
        let got = assess(&Evidence::new(200_000, &a));
        assert!(got.reasons.contains(&Reason::LowTextRatio), "{got:?}");
    }

    #[test]
    fn a_small_tidy_page_is_not_punished_for_having_no_boilerplate_to_dilute() {
        // The ratio rule must not fire on a short document that is simply well made.
        let a = article();
        let got = assess(&Evidence::new(RATIO_FLOOR_HTML - 1, &a));
        assert!(!got.reasons.contains(&Reason::LowTextRatio), "{got:?}");
    }

    #[test]
    fn one_phrase_repeated_across_the_page_is_named_as_filler() {
        let text = "best cheap running shoes ".repeat(80);
        let got = assess(&Evidence::new(text.len() * 3, &text));
        assert!(got.reasons.contains(&Reason::Repetitive), "{got:?}");
    }

    #[test]
    fn a_page_of_identifiers_is_not_mistaken_for_running_text() {
        let text = "a1b2c3d4e5f6a7b8 ".repeat(100);
        let got = assess(&Evidence::new(text.len() * 3, &text));
        assert!(got.reasons.contains(&Reason::NotProse), "{got:?}");
    }

    #[test]
    fn a_page_in_another_language_is_judged_by_shape_and_nothing_else() {
        // The rule this pins is the one that would be easiest to get wrong: nothing here may read
        // "not English" as "not prose". Same sentence, four languages, one verdict.
        for text in [
            "El consejo votó el martes para aprobar la medida después de un largo debate que se \
             prolongó más allá de la medianoche. Los partidarios argumentaron que el cambio \
             llevaba tiempo pendiente y que el dinero ya estaba reservado; los detractores \
             dijeron que las cifras nunca se habían publicado por completo. Se espera una \
             segunda lectura antes de que termine el mes.",
            "Der Rat stimmte am Dienstag nach einer langen Debatte, die bis nach Mitternacht \
             dauerte, für die Annahme der Maßnahme. Die Befürworter argumentierten, die Änderung \
             sei überfällig und das Geld längst zurückgelegt; die Gegner sagten, die Zahlen seien \
             nie vollständig veröffentlicht worden. Eine zweite Lesung wird noch vor Monatsende \
             erwartet.",
            "Le conseil a voté mardi pour approuver la mesure après un long débat qui s'est \
             prolongé après minuit. Les partisans ont fait valoir que le changement se faisait \
             attendre et que l'argent avait déjà été mis de côté; les opposants ont dit que les \
             chiffres n'avaient jamais été publiés en entier. Une seconde lecture est attendue \
             avant la fin du mois.",
            "O conselho votou na terça-feira para aprovar a medida após um longo debate que se \
             prolongou pela meia-noite. Os apoiantes argumentaram que a mudança já ia tarde e que \
             o dinheiro estava reservado; os opositores disseram que os números nunca tinham sido \
             publicados na íntegra. Espera-se uma segunda leitura antes do fim do mês.",
        ] {
            let got = assess(&Evidence::new(text.len() * 3, text));
            assert_eq!(got.verdict, Verdict::Full, "{text:.40}… -> {got:?}");
        }
    }

    #[test]
    fn a_page_in_a_script_without_spaces_is_a_page_and_not_a_hash() {
        // The sharpest version of the same rule. A whitespace splitter reports one "word" for all
        // of this, so both a word-count floor and a word-length window would throw it out — and
        // it is an ordinary paragraph.
        let text = "理事会は火曜日、深夜過ぎまで続いた長い議論の末に、その措置を承認する\
             ことを決議した。賛成派は、この変更は以前から必要とされており、資金もすでに\
             確保されていると主張した。反対派は、数字が完全な形で公表されたことは一度も\
             ないと述べた。第二読会は今月末までに行われる見込みである。";
        let got = assess(&Evidence::new(text.len() * 3, text));
        assert_eq!(got.verdict, Verdict::Full, "{got:?}");
    }

    #[test]
    fn a_blob_of_base64_is_still_not_prose_even_though_it_has_no_spaces_either() {
        // The exemption above must not become a hole: the test for it is non-ASCII letters, which
        // is exactly what an encoded payload does not have.
        let text = "aGVsbG8gd29ybGQgdGhpcyBpcyBub3QgYSBzZW50ZW5jZSBhdCBhbGw".repeat(20);
        let got = assess(&Evidence::new(text.len() * 3, &text));
        assert!(got.reasons.contains(&Reason::NotProse), "{got:?}");
    }

    #[test]
    fn asking_for_an_article_and_getting_the_front_page_is_said_out_loud() {
        // The session layer has always seen this and only ever told the throttle. The caller, who
        // asked for one specific page, was handed the home page with nothing said about it.
        let a = article();
        let got = assess(
            &Evidence::new(a.len() * 3, &a)
                .between("https://x.test/2024/06/the-article", "https://x.test/"),
        );
        assert!(got.reasons.contains(&Reason::LandedElsewhere), "{got:?}");
    }

    #[test]
    fn a_redirect_that_still_lands_on_something_specific_is_not_reported() {
        // Sites move pages. Following a redirect to another real page is not losing the page, and
        // reporting it as such would fire on half the web.
        let a = article();
        let got = assess(&Evidence::new(a.len() * 3, &a).between(
            "https://x.test/2024/06/the-article",
            "https://x.test/news/the-article",
        ));
        assert!(!got.reasons.contains(&Reason::LandedElsewhere), "{got:?}");
    }

    #[test]
    fn a_page_that_is_almost_all_site_furniture_is_named_as_such_only_when_the_caller_can_know() {
        let a = article();
        let seen = assess(&Evidence::new(a.len() * 3, &a).against_site(0.95));
        assert!(
            seen.reasons.contains(&Reason::MostlyBoilerplate),
            "{seen:?}"
        );

        // And a lone fetch, which has not seen the rest of the site, says nothing rather than
        // guessing — the field is absent, not zero.
        let blind = assess(&Evidence::new(a.len() * 3, &a));
        assert!(
            !blind.reasons.contains(&Reason::MostlyBoilerplate),
            "{blind:?}"
        );
    }

    use crate::extraction::Signals;

    #[test]
    fn an_affiliate_listicle_is_named_as_heavily_engineered() {
        // The shape the study found at the top of every product-review ranking: referral links in
        // bulk, headings that are the search query again, and more anchors than sentences.
        let s = Signals {
            anchors: 40,
            affiliate_anchors: 18,
            images: 20,
            headings: 12,
            paragraphs: 12,
            heading_echo: 0.98,
        };
        let got = optimization(&s, &"best cheap anvil review ".repeat(60));
        assert_eq!(got.level, Level::High, "{got:?}");
        assert!(got.traits.contains(&Trait::AffiliateHeavy), "{got:?}");
        assert!(got.traits.contains(&Trait::HeadingsEchoTheBody), "{got:?}");
    }

    /// An article-length body: densities are ratios, and a ratio over 55 words is noise.
    fn long_article() -> String {
        article().repeat(20)
    }

    #[test]
    fn a_review_that_earns_a_commission_is_still_a_review() {
        // The study's own top-ranked pages carry three to six referral links in one to two thousand
        // words. Firing there would label the median commercial page — true, and useless.
        let s = Signals {
            anchors: 20,
            affiliate_anchors: 4,
            images: 3,
            headings: 4,
            paragraphs: 20,
            heading_echo: 0.5,
        };
        let got = optimization(&s, &long_article());
        assert_eq!(got.level, Level::Ordinary, "{got:?}");
        assert!(!got.traits.contains(&Trait::AffiliateHeavy), "{got:?}");
    }

    #[test]
    fn a_glossary_is_link_dense_and_that_alone_is_not_held_against_it() {
        let s = Signals {
            anchors: 400,
            affiliate_anchors: 0,
            images: 0,
            headings: 1,
            paragraphs: 40,
            heading_echo: 0.2,
        };
        let got = optimization(&s, &long_article());
        assert!(got.traits.contains(&Trait::LinkDense), "{got:?}");
        assert_eq!(got.level, Level::Ordinary, "{got:?}");
    }

    #[test]
    fn a_short_page_is_not_judged_by_a_ratio_that_cannot_mean_anything() {
        // Three links on a two-line page is not a link farm; it is a two-line page.
        let s = Signals {
            anchors: 8,
            affiliate_anchors: 4,
            images: 1,
            headings: 1,
            paragraphs: 1,
            heading_echo: 0.4,
        };
        let got = optimization(&s, "Back soon. We will return on Monday.");
        assert!(got.traits.is_empty(), "{got:?}");
    }

    #[test]
    fn a_page_with_nothing_notable_about_it_says_nothing() {
        let got = optimization(&Signals::default(), &long_article());
        assert_eq!(got.level, Level::Ordinary);
        assert!(got.traits.is_empty(), "{got:?}");
    }

    #[test]
    fn an_empty_extraction_is_thin_without_panicking_on_the_arithmetic() {
        let got = assess(&Evidence::new(0, ""));
        assert_eq!(got.verdict, Verdict::Thin);
        assert!(got.reasons.contains(&Reason::ThinText));
    }
}
