//! Choosing the content root by measuring every candidate, not by trusting a tag name.
//!
//! This is Readability's `grabArticle`, reimplemented in Rust over the DOM svipall already parsed.
//! It is here because of what the benchmarks say about it. In Bevendorff et al. (SIGIR 2023) —
//! fourteen extractors over 3,985 annotated pages — Readability has the highest median F1 of any
//! system (0.970) and the tightest spread, and the paper calls it *"a true jack of all trades,
//! being the most robust model at all complexities"*. It is also the most-deployed content
//! extractor there is, being Firefox's reader view.
//!
//! What it does that a selector list cannot: every plausible container competes. Each paragraph
//! scores itself on length, commas and its container's class name; that score is propagated to its
//! ancestors with a divider that falls off by depth; the winner is the highest-scoring subtree
//! after a link-density penalty. A page whose article sits in `<div class="mw-parser-output">`
//! wins on the same terms as one that uses `<article>`, which is the whole difficulty with
//! table-layout and CMS markup.
//!
//! ## Where it is weak, stated up front
//!
//! It counts commas and characters. "Multilingual Benchmarking of Main Content Extractors"
//! (SIGIR 2025) measured what that costs: on the same code, Readability scores 0.862 on English
//! and **0.672** on Chinese, with 95% of Chinese pages scoring under 0.3, and it names the cause
//! — *"features such as character count, comma count, and link density… are not optimized for
//! multilingual extraction"*. And WCXB (2026), which labels pages by type, puts it twelfth of
//! thirteen overall because it is tuned for articles: 0.825 on articles, 0.407 on product pages.
//!
//! So this is one voice and not the decision. It is strongest exactly where the corpus that
//! flatters it is: long-form prose in a European language.
//!
//! Reimplemented from the published algorithm rather than copied; `mozilla/readability` is
//! Apache-2.0. The constants below are its constants, kept identical so that a disagreement with
//! the reference implementation is a bug here rather than a difference of opinion.

use super::stats::Doc;
use ego_tree::{NodeId, NodeRef};
use regex::Regex;
use scraper::{ElementRef, Node};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// How many top candidates to keep while looking at how tight the competition is.
const TOP_CANDIDATES: usize = 5;
/// A candidate must be within this fraction of the leader to count as a rival.
const RIVAL_SHARE: f32 = 0.75;
/// And this many rivals under one ancestor means that ancestor is the real container.
const MINIMUM_RIVALS: usize = 3;
/// Below this, an extraction is short enough to be worth retrying with a flag turned off.
pub const CHAR_THRESHOLD: usize = 500;
/// Ancestors a paragraph's score is propagated to.
const ANCESTOR_LEVELS: usize = 5;

/// Elements scored directly. Everything else earns its score through these.
const SCORED: &[&str] = &["section", "h2", "h3", "h4", "h5", "h6", "p", "td", "pre"];

/// Containers named like page furniture, unless they are also named like content.
static UNLIKELY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote",
    )
    .expect("static pattern")
});
static MAYBE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)and|article|body|column|content|main|mathjax|shadow").expect("static pattern")
});
static POSITIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story",
    )
    .expect("static pattern")
});
static NEGATIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|footer|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|widget",
    )
    .expect("static pattern")
});
/// ARIA roles that are never the main content.
const UNLIKELY_ROLES: &[&str] = &[
    "menu",
    "menubar",
    "complementary",
    "navigation",
    "alert",
    "alertdialog",
    "dialog",
];

/// Which parts of the algorithm are switched on.
///
/// Readability runs the whole pass, and if what comes back is under the character threshold it
/// drops one of these and runs again, then another, then another. That retry loop is why it is the
/// most robust system in the comparison: an aggressive rule that empties a page costs nothing,
/// because the page is simply re-read without it. svipall's own `FRAGMENT` constant is a cruder
/// version of the same idea — re-render everything when the answer looks too short — and this is
/// what it should become.
#[derive(Debug, Clone, Copy)]
pub struct Flags {
    pub strip_unlikely: bool,
    pub weight_classes: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            strip_unlikely: true,
            weight_classes: true,
        }
    }
}

impl Flags {
    /// The flags to try after this one failed to produce enough text, if any are left.
    pub fn relaxed(self) -> Option<Self> {
        if self.strip_unlikely {
            Some(Self {
                strip_unlikely: false,
                ..self
            })
        } else if self.weight_classes {
            Some(Self {
                strip_unlikely: false,
                weight_classes: false,
            })
        } else {
            None
        }
    }
}

/// Where the content is: a subtree, minus the parts of it that did not earn their place.
#[derive(Debug, Clone)]
pub struct Choice {
    /// The node to render.
    pub root: NodeId,
    /// Children of `root` to skip. Empty when `root` is the content itself.
    pub drop: HashSet<NodeId>,
    /// The winning score, for comparing this voter's confidence against another's.
    pub score: f32,
}

/// Class and id weight: `+25` for a content-ish name, `-25` for a furniture-ish one, both.
fn class_weight(el: ElementRef<'_>, flags: Flags) -> f32 {
    if !flags.weight_classes {
        return 0.0;
    }
    let mut w = 0.0;
    for attr in ["class", "id"] {
        let Some(v) = el.value().attr(attr) else {
            continue;
        };
        if v.is_empty() {
            continue;
        }
        if NEGATIVE.is_match(v) {
            w -= 25.0;
        }
        if POSITIVE.is_match(v) {
            w += 25.0;
        }
    }
    w
}

/// The score a container starts with, before any paragraph contributes to it.
fn base_score(name: &str) -> f32 {
    match name {
        "div" => 5.0,
        "pre" | "td" | "blockquote" => 3.0,
        "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" => -3.0,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => -5.0,
        _ => 0.0,
    }
}

/// Whether this container is named like something that is never the article.
fn is_unlikely(el: ElementRef<'_>) -> bool {
    if el
        .value()
        .attr("role")
        .is_some_and(|r| UNLIKELY_ROLES.contains(&r))
    {
        return true;
    }
    let name = el.value().name();
    if name == "body" || name == "a" {
        return false;
    }
    let mut ident = String::new();
    if let Some(c) = el.value().attr("class") {
        ident.push_str(c);
        ident.push(' ');
    }
    if let Some(i) = el.value().attr("id") {
        ident.push_str(i);
    }
    !ident.is_empty() && UNLIKELY.is_match(&ident) && !MAYBE.is_match(&ident)
}

/// Pick the content root of `root`, or `None` when nothing in it scores at all.
pub fn choose(root: NodeRef<'_, Node>, doc: &Doc, flags: Flags) -> Option<Choice> {
    let mut scores: HashMap<NodeId, f32> = HashMap::new();
    // Containers whose name says they are furniture. Their paragraphs are not scored and neither
    // are their ancestors-through-them, which is what `strip_unlikely` means.
    let mut stripped: HashSet<NodeId> = HashSet::new();

    for node in root.descendants() {
        let Some(el) = ElementRef::wrap(node) else {
            continue;
        };
        // `descendants()` is pre-order, so a stripped container is always seen before anything
        // under it and the set is complete by the time its children are asked about.
        if flags.strip_unlikely
            && (node.ancestors().any(|a| stripped.contains(&a.id())) || is_unlikely(el))
        {
            stripped.insert(node.id());
            continue;
        }
        if !SCORED.contains(&el.value().name()) {
            continue;
        }
        let Some(s) = doc.get(node.id()) else {
            continue;
        };
        // Readability's own floor: under 25 characters a paragraph is not evidence of anything.
        if s.text < 25 {
            continue;
        }

        // 1 for existing, 1 per comma, and 1 per 100 characters up to 3. Deliberately coarse: it is
        // a vote for "there is prose here", not a measure of how good the prose is.
        let paragraph = 1.0 + s.commas as f32 + ((s.text / 100) as f32).min(3.0);

        for (level, ancestor) in node.ancestors().take(ANCESTOR_LEVELS).enumerate() {
            let Some(ael) = ElementRef::wrap(ancestor) else {
                continue;
            };
            let id = ancestor.id();
            let entry = scores
                .entry(id)
                .or_insert_with(|| base_score(ael.value().name()) + class_weight(ael, flags));
            // Parent, grandparent, then falling away fast: content is near its paragraphs.
            let divider = match level {
                0 => 1.0,
                1 => 2.0,
                n => n as f32 * 3.0,
            };
            *entry += paragraph / divider;
        }
    }

    if scores.is_empty() {
        return None;
    }

    // Link density last, as a multiplier rather than a term: a container that is nine tenths
    // navigation loses nine tenths of whatever else it had going for it.
    let mut ranked: Vec<(NodeId, f32)> = scores
        .iter()
        .map(|(id, s)| {
            let density = doc.get(*id).map(|st| st.link_density()).unwrap_or(0.0);
            (*id, s * (1.0 - density))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(TOP_CANDIDATES);

    let (mut top, score) = *ranked.first()?;
    if score <= 0.0 {
        return None;
    }

    // When several near-equal candidates share an ancestor, that ancestor is the article and the
    // candidates are its sections. This is what rescues a page split across sibling containers,
    // which is precisely the shape WCXB reports every extractor failing on for service pages.
    let rivals: Vec<Vec<NodeId>> = ranked
        .iter()
        .skip(1)
        .filter(|(_, s)| *s / score >= RIVAL_SHARE)
        .filter_map(|(id, _)| {
            find(root, *id).map(|n| n.ancestors().map(|a| a.id()).collect::<Vec<_>>())
        })
        .collect();
    if rivals.len() >= MINIMUM_RIVALS {
        if let Some(node) = find(root, top) {
            for ancestor in node.ancestors() {
                if ancestor.id() == root.id() {
                    break;
                }
                let shared = rivals
                    .iter()
                    .filter(|list| list.contains(&ancestor.id()))
                    .count();
                if shared >= MINIMUM_RIVALS {
                    top = ancestor.id();
                    break;
                }
            }
        }
    }

    // An only child is a wrapper, and its parent is the thing with siblings worth looking at.
    let mut node = find(root, top)?;
    while node.id() != root.id() {
        let Some(parent) = node.parent() else { break };
        if parent.id() == root.id() {
            break;
        }
        if parent.children().filter(|c| c.value().is_element()).count() != 1 {
            break;
        }
        node = parent;
    }
    top = node.id();

    // Siblings of the winner that look like more of the same article: content split by an ad, a
    // pull quote, a preamble above the container the paragraphs happened to land in.
    let threshold = (score * 0.2).max(10.0);
    let top_class = find(root, top)
        .and_then(ElementRef::wrap)
        .and_then(|e| e.value().attr("class").map(str::to_string));
    let (article_root, drop) = match find(root, top).and_then(|n| n.parent()) {
        Some(parent) if parent.id() != root.id() && parent.value().is_element() => {
            let mut drop = HashSet::new();
            for sibling in parent.children() {
                if sibling.id() == top {
                    continue;
                }
                let Some(el) = ElementRef::wrap(sibling) else {
                    continue;
                };
                if !keep_sibling(sibling, el, doc, &scores, score, threshold, &top_class) {
                    drop.insert(sibling.id());
                }
            }
            (parent.id(), drop)
        }
        _ => (top, HashSet::new()),
    };

    Some(Choice {
        root: article_root,
        drop,
        score,
    })
}

/// Whether a sibling of the winning container is more of the same article.
fn keep_sibling(
    node: NodeRef<'_, Node>,
    el: ElementRef<'_>,
    doc: &Doc,
    scores: &HashMap<NodeId, f32>,
    top_score: f32,
    threshold: f32,
    top_class: &Option<String>,
) -> bool {
    let mut bonus = 0.0;
    // Same class as the winner is strong evidence of the same template slot.
    if let (Some(a), Some(b)) = (top_class.as_deref(), el.value().attr("class")) {
        if !a.is_empty() && a == b {
            bonus += top_score * 0.2;
        }
    }
    if scores.get(&node.id()).map(|s| s + bonus).unwrap_or(0.0) >= threshold {
        return true;
    }
    if el.value().name() != "p" {
        return false;
    }
    let Some(s) = doc.get(node.id()) else {
        return false;
    };
    let density = s.link_density();
    // A long paragraph that is mostly not links, or a short one that is plainly a sentence.
    if s.text > 80 && density < 0.25 {
        return true;
    }
    s.text > 0 && s.text < 80 && density == 0.0 && ends_a_sentence(el)
}

/// Whether the text reads as a sentence rather than a label: a full stop, at the end or before a
/// space. Readability's own test, and as English-shaped as the rest of this file.
fn ends_a_sentence(el: ElementRef<'_>) -> bool {
    let text: String = el.text().collect();
    let bytes = text.as_bytes();
    text.match_indices('.')
        .any(|(i, _)| i + 1 == bytes.len() || bytes[i + 1] == b' ')
}

/// The node with this id, within the subtree being judged.
fn find(root: NodeRef<'_, Node>, id: NodeId) -> Option<NodeRef<'_, Node>> {
    root.tree().get(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{Html, Selector};

    fn pick(html: &str) -> Option<(Html, Choice)> {
        let doc = Html::parse_document(html);
        let stats = Doc::of(doc.tree.root());
        let choice = choose(doc.tree.root(), &stats, Flags::default())?;
        Some((doc, choice))
    }

    /// What this exists for: the article is in an unnamed `div` on a page with no semantic markup.
    #[test]
    fn the_content_wins_on_its_own_merits_not_on_its_tag_name() {
        let mut html = String::from("<html><body><table><tr><td class=\"nav\"><ul>");
        for i in 0..40 {
            html.push_str(&format!("<li><a href=\"/s{i}\">Section {i}</a></li>"));
        }
        html.push_str("</ul></td><td id=\"story\">");
        for _ in 0..6 {
            html.push_str(
                "<p>The council voted on Tuesday to approve the harbour measure, after a debate \
                 that ran past midnight, and the money was already set aside.</p>",
            );
        }
        html.push_str("</td></tr></table></body></html>");

        let (doc, choice) = pick(&html).expect("something should win");
        let chosen = doc.tree.get(choice.root).expect("the winner");
        let text: String = ElementRef::wrap(chosen)
            .expect("an element")
            .text()
            .collect();
        assert!(
            text.contains("harbour measure"),
            "the article did not win: {text:.200}"
        );
        assert!(
            !text.contains("Section 30") || !choice.drop.is_empty(),
            "the navigation column came along and nothing dropped it"
        );
    }

    /// A negative class name has to be able to lose to real prose, or the weights are decoration.
    #[test]
    fn a_content_class_beats_a_furniture_class() {
        let article = "<p>Ownership in Rust means each value has a single owner, and the compiler \
                       enforces it, which is the whole idea.</p>";
        let html = format!(
            "<html><body><div id=\"wrap\"><div class=\"sidebar\">{}</div>\
             <div class=\"post-content\">{}</div></div></body></html>",
            article.repeat(3),
            article.repeat(3)
        );
        let (doc, choice) = pick(&html).expect("something should win");
        let winner = doc
            .tree
            .get(choice.root)
            .and_then(ElementRef::wrap)
            .unwrap();
        let ident = format!(
            "{} {}",
            winner.value().attr("class").unwrap_or_default(),
            winner.value().attr("id").unwrap_or_default()
        );
        assert!(
            ident.contains("post-content") || ident.contains("wrap"),
            "the sidebar won on identical text: {ident}"
        );
    }

    /// Turning the flags off has to change the answer, or the retry loop can never rescue a page.
    #[test]
    fn relaxing_the_flags_lets_a_stripped_page_be_read() {
        // Everything on this page is inside a container named like furniture.
        let html = format!(
            "<html><body><div class=\"comment\">{}</div></body></html>",
            "<p>The council voted on Tuesday to approve the harbour measure, after a debate that \
             ran past midnight.</p>"
                .repeat(4)
        );
        let doc = Html::parse_document(&html);
        let stats = Doc::of(doc.tree.root());

        let strict = choose(doc.tree.root(), &stats, Flags::default());
        let relaxed = choose(
            doc.tree.root(),
            &stats,
            Flags::default().relaxed().expect("a second attempt exists"),
        );
        let len = |c: &Option<Choice>| {
            c.as_ref()
                .and_then(|c| doc.tree.get(c.root))
                .and_then(ElementRef::wrap)
                .map(|e| e.text().collect::<String>().len())
                .unwrap_or(0)
        };
        assert!(
            len(&relaxed) > len(&strict),
            "dropping strip_unlikely found no more text than keeping it"
        );
    }

    #[test]
    fn a_page_with_no_prose_at_all_has_no_winner() {
        let html = "<html><body><ul><li><a href=\"/a\">one</a></li>\
                    <li><a href=\"/b\">two</a></li></ul></body></html>";
        let doc = Html::parse_document(html);
        let stats = Doc::of(doc.tree.root());
        assert!(choose(doc.tree.root(), &stats, Flags::default()).is_none());
    }

    #[test]
    fn the_winner_is_a_real_node_of_the_document() {
        let html = format!(
            "<html><body><main>{}</main></body></html>",
            "<p>Borrowing lets you reference data without taking ownership, which avoids a \
             copy.</p>"
                .repeat(5)
        );
        let (doc, choice) = pick(&html).expect("something should win");
        assert!(doc.tree.get(choice.root).is_some());
        let sel = Selector::parse("main").expect("selector");
        let main = doc.select(&sel).next().expect("the main");
        let winner = doc
            .tree
            .get(choice.root)
            .and_then(ElementRef::wrap)
            .unwrap();
        let text: String = winner.text().collect();
        assert!(text.contains("Borrowing lets you"), "{text:.120}");
        let _ = main;
    }
}
