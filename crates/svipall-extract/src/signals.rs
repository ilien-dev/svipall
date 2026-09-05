//! The structural counts a page's shape is read from, taken during the one parse.
//!
//! These are the page-level features from Bevendorff et al., *Is Google Getting Worse?* (ECIR
//! 2024), which measured them against rank across 7,392 product-review queries over a year. What
//! that study found is the reason they are collected at all — and the reason they are used the way
//! they are:
//!
//! | feature vs. rank      | R²   |
//! |-----------------------|------|
//! | affiliate links       | .99  |
//! | heading-keyword echo  | .96  |
//! | lexical diversity     | −.88 |
//!
//! A better-ranked page carries *more* affiliate links, *more* keyword stuffing and *less* lexical
//! variety. So these are not signals of quality — they are signals of **optimisation**, and
//! optimisation is what the ranking rewards. Reading them the way a search engine does would learn
//! to prefer exactly the pages the study calls spam. They are collected here as a negative signal
//! and nothing else.
//!
//! Two features the study used are deliberately **not** collected: Flesch reading ease and
//! function-word ratio. Both are calibrated on English, and a tool that fetches the whole web
//! cannot use a measure that reads "not English" as "badly written". What is left — counts,
//! densities, and the page compared against itself — says the same thing in every language.

use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Hosts that exist to carry a referral. Named by endpoint, never by brand, because the endpoint
/// is what is on the page and the brand is not.
const AFFILIATE_ENDPOINTS: &[&str] = &[
    "amzn.to",
    "s.click.aliexpress.com",
    "awin1.com",
    "anrdoezrs.net",
    "dpbolvw.net",
    "jdoqocy.com",
    "kqzyfj.com",
    "tkqlhce.com",
    "tqlkg.com",
    "hop.clickbank.net",
    "rover.ebay.com",
    "ebay.us",
    "flexlinks.com",
    "fxo.co",
    "refersion.com",
    "shareasale.com",
    "go.redirectingat.com",
    "shop-links.co",
    "go.skimresources.com",
];

/// Query parameters that carry a referral on the retailer's own host.
const AFFILIATE_PARAMS: &[&str] = &[
    "aff_id=",
    "affiliate_id=",
    "aff_platform=",
    "utm_medium=affiliate",
    "irclickid=",
    "ranmid=",
    "siteid=",
];

/// What a page looks like, structurally. Counts only: what they mean is decided in `quality`,
/// which is where the thresholds and their justification live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Signals {
    pub anchors: u32,
    /// Anchors that carry a referral. The single strongest correlate of rank the study found.
    pub affiliate_anchors: u32,
    pub images: u32,
    pub headings: u32,
    pub paragraphs: u32,
    /// Share of the words in this page's headings that also occur in its body.
    ///
    /// The page compared against itself, which is what makes it work in any language: a heading
    /// written to be a search query repeats the body's keywords and says nothing else, and that is
    /// true of keyword stuffing in Spanish exactly as in English. `1.0` means no heading contained
    /// a single word the body did not.
    pub heading_echo: f32,
}

static ANCHOR: LazyLock<Option<Selector>> = LazyLock::new(|| Selector::parse("a[href]").ok());
static IMAGE: LazyLock<Option<Selector>> = LazyLock::new(|| Selector::parse("img").ok());
static HEADING: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("h1, h2, h3, h4, h5, h6").ok());
static PARAGRAPH: LazyLock<Option<Selector>> = LazyLock::new(|| Selector::parse("p").ok());

/// Does this href exist to carry a referral?
pub fn is_affiliate(href: &str) -> bool {
    let low = href.to_ascii_lowercase();
    if AFFILIATE_ENDPOINTS.iter().any(|e| low.contains(e)) {
        return true;
    }
    if AFFILIATE_PARAMS.iter().any(|p| low.contains(p)) {
        return true;
    }
    // The largest network's tag rides on the retailer's own host, where `tag=` is otherwise an
    // ordinary parameter. Only there does it mean a referral.
    low.contains("amazon.") && (low.contains("?tag=") || low.contains("&tag="))
}

/// Count what the page is made of, from the document that has already been parsed.
pub(crate) fn signals_from(doc: &Html, body_text: &str) -> Signals {
    let count = |s: &Option<Selector>| -> u32 {
        s.as_ref()
            .map(|sel| doc.select(sel).count() as u32)
            .unwrap_or(0)
    };

    let affiliate_anchors = ANCHOR
        .as_ref()
        .map(|sel| {
            doc.select(sel)
                .filter(|a| a.value().attr("href").is_some_and(is_affiliate))
                .count() as u32
        })
        .unwrap_or(0);

    Signals {
        anchors: count(&ANCHOR),
        affiliate_anchors,
        images: count(&IMAGE),
        headings: count(&HEADING),
        paragraphs: count(&PARAGRAPH),
        heading_echo: heading_echo(doc, body_text),
    }
}

/// How much of the headings is already in the body.
fn heading_echo(doc: &Html, body_text: &str) -> f32 {
    let Some(sel) = HEADING.as_ref() else {
        return 0.0;
    };
    // The body, as a set of lowercased words, bounded: a very long page does not need all of it to
    // answer whether its headings say anything new.
    let body: HashSet<String> = body_text
        .split_whitespace()
        .take(20_000)
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    if body.is_empty() {
        return 0.0;
    }

    let mut total = 0usize;
    let mut echoed = 0usize;
    for h in doc.select(sel) {
        for w in heading_words(h) {
            total += 1;
            if body.contains(&w) {
                echoed += 1;
            }
        }
    }
    if total == 0 {
        return 0.0;
    }
    echoed as f32 / total as f32
}

fn heading_words(h: ElementRef<'_>) -> Vec<String> {
    h.text()
        .flat_map(|t| t.split_whitespace())
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_referral_is_recognised_on_a_shortener_and_on_the_retailers_own_host() {
        assert!(is_affiliate("https://amzn.to/3xYzAbC"));
        assert!(is_affiliate(
            "https://www.amazon.co.uk/dp/B01?tag=someone-21"
        ));
        assert!(is_affiliate("https://x.test/out?aff_id=99"));
        assert!(is_affiliate("https://go.redirectingat.com/?id=1&url=x"));
    }

    #[test]
    fn an_ordinary_link_is_not_called_a_referral() {
        // `tag=` is an ordinary parameter nearly everywhere, and treating it as a referral would
        // report affiliate spam on every blog with a tag index.
        assert!(!is_affiliate("https://blog.test/posts?tag=rust"));
        assert!(!is_affiliate("https://en.wikipedia.org/wiki/Harbour"));
        assert!(!is_affiliate("/news/the-article"));
    }

    #[test]
    fn a_page_is_counted_by_what_it_is_made_of() {
        let html = r#"<html><body><main>
            <h1>Best anvils</h1><h2>Our picks</h2>
            <p>An anvil is heavy.</p><p>We tested twelve.</p>
            <img src="a.jpg"><img src="b.jpg">
            <a href="https://amzn.to/1">Buy</a>
            <a href="/about">About</a>
        </main></body></html>"#;
        let doc = Html::parse_document(html);
        let s = signals_from(
            &doc,
            "Best anvils Our picks An anvil is heavy We tested twelve",
        );
        assert_eq!(s.anchors, 2);
        assert_eq!(s.affiliate_anchors, 1);
        assert_eq!(s.images, 2);
        assert_eq!(s.headings, 2);
        assert_eq!(s.paragraphs, 2);
    }

    #[test]
    fn headings_that_only_repeat_the_body_are_measured_as_such() {
        // Keyword stuffing, in the form the study measured: the heading is the search query and
        // the body is the same words again.
        let stuffed = Html::parse_document(
            "<html><body><h1>best cheap anvils</h1><h2>cheap anvils best</h2>\
             <p>best cheap anvils are cheap anvils</p></body></html>",
        );
        let echo = signals_from(&stuffed, "best cheap anvils are cheap anvils").heading_echo;
        assert!(echo > 0.95, "{echo}");

        let written = Html::parse_document(
            "<html><body><h1>An unhurried defence of ironmongery</h1>\
             <p>The council voted on Tuesday to approve the measure.</p></body></html>",
        );
        let echo = signals_from(
            &written,
            "The council voted on Tuesday to approve the measure",
        )
        .heading_echo;
        assert!(echo < 0.4, "{echo}");
    }

    #[test]
    fn the_measure_says_the_same_thing_in_another_language() {
        // The point of comparing the page against itself rather than against a word list.
        let stuffed = Html::parse_document(
            "<html><body><h1>mejores yunques baratos</h1><h2>yunques baratos mejores</h2>\
             <p>mejores yunques baratos son yunques baratos</p></body></html>",
        );
        let echo =
            signals_from(&stuffed, "mejores yunques baratos son yunques baratos").heading_echo;
        assert!(echo > 0.95, "{echo}");
    }
}
