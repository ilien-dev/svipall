//! Dropping page furniture before it reaches the model.
//!
//! `main_content_only` used to be a fixed list of tags to skip (`nav`, `footer`, `aside`, …). That
//! catches semantic markup and nothing else, so a sidebar built from `<div class="sidebar">`, a
//! "related posts" rail, or a comment thread all survived and were billed as context.
//!
//! This scores containers on how much of them is text versus links, plus a few cheap signals, in
//! two passes over the tree that is already parsed. Deliberately not `libreadability` or
//! `rs-trafilatura`: both take HTML and return HTML, so using either would mean parsing the
//! document a second time — roughly doubling the dominant cost — and neither can be told that a
//! data table or a `<pre>` block must survive.

use ego_tree::iter::Edge;
use ego_tree::NodeRef;
use regex::Regex;
use scraper::{ElementRef, Node};
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub struct PruneOpts {
    /// Below this many characters a container is a candidate for removal.
    pub min_text: u32,
    /// Fraction of a container's text that sits inside links before it reads as navigation.
    pub max_link_density: f32,
    pub min_score: f32,
    /// This page is a discussion thread, so a container called `comment` is the content.
    pub thread: bool,
}

impl Default for PruneOpts {
    fn default() -> Self {
        Self {
            min_text: 25,
            max_link_density: 0.5,
            min_score: 0.35,
            thread: false,
        }
    }
}

/// Nodes the walker should skip.
#[derive(Debug, Default)]
pub struct Pruned(HashSet<ego_tree::NodeId>);

impl Pruned {
    /// Wrap a set of node ids the caller decided on. The vote produces one of these directly, so a
    /// second way of saying "skip these nodes" does not have to exist alongside this one.
    pub fn from_ids(ids: HashSet<ego_tree::NodeId>) -> Self {
        Self(ids)
    }

    pub fn contains(&self, id: ego_tree::NodeId) -> bool {
        self.0.contains(&id)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct PruneReport {
    pub pruned: Pruned,
    /// False when dropping `nav`/`header`/`footer`/`aside` wholesale would leave almost nothing.
    ///
    /// A link index, a sitemap page or a documentation table of contents is *made* of navigation.
    /// Skipping page chrome there returns an empty document, which is worse than returning the
    /// links — and this was the behaviour before the density pass existed.
    pub skip_chrome: bool,
}

static NEGATIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(^|[-_ ])(nav|navbar|menu|sidebar|side-bar|footer|foot|header|masthead|breadcrumb|comments?|disqus|share|social|related|recommend|promo|banner|advert|ads?|popup|modal|cookie|consent|newsletter|subscribe|signup|pagination|pager|widget|toolbar|skip-link|sr-only|screen-reader|offscreen)([-_ ]|$)",
    )
    .unwrap()
});
/// ▲ The same list with `comments?` and `disqus` taken out, for a page that is a discussion.
///
/// On a thread the posts *are* the content and they are in containers called `comment`. The list
/// above condemns them by name — the same defect WCXB measures in Readability, whose
/// `unlikelyCandidates` pattern carries the same word and which scores 0.466 on forums against
/// 0.808 for the leader. svipall's own shipping path scores 0.556 on WCXB's development forums,
/// and this is why.
///
/// Everything else stays: a thread still has navigation, a footer and a share rail, and they are
/// still furniture. Only the word that means "the content" on this kind of page is dropped.
static NEGATIVE_ON_A_THREAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(^|[-_ ])(nav|navbar|menu|sidebar|side-bar|footer|foot|header|masthead|breadcrumb|share|social|related|recommend|promo|banner|advert|ads?|popup|modal|cookie|consent|newsletter|subscribe|signup|pagination|pager|widget|toolbar|skip-link|sr-only|screen-reader|offscreen)([-_ ]|$)",
    )
    .unwrap()
});
static POSITIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(article|post|content|entry|main|story|body|markdown|prose|docs?|readme)")
        .unwrap()
});

/// Containers worth judging. Inline elements and text are scored through their parent.
const CANDIDATES: &[&str] = &[
    "div", "section", "aside", "nav", "ul", "ol", "li", "form", "header", "footer", "figure",
    "table", "dl",
];
/// Never removed, whatever the numbers say.
const PROTECTED: &[&str] = &["main", "article", "body", "html"];

fn class_bonus(el: ElementRef<'_>, thread: bool) -> f32 {
    let mut ident = String::new();
    if let Some(c) = el.value().attr("class") {
        ident.push_str(c);
        ident.push(' ');
    }
    if let Some(i) = el.value().attr("id") {
        ident.push_str(i);
    }
    if ident.is_empty() {
        return 0.0;
    }
    let negative = if thread {
        &NEGATIVE_ON_A_THREAD
    } else {
        &NEGATIVE
    };
    if negative.is_match(&ident) {
        -1.0
    } else if POSITIVE.is_match(&ident) {
        1.0
    } else {
        0.0
    }
}

/// Score every container, then decide what to drop.
///
/// The statistics come from `content::stats`, shared with every other heuristic that reads the
/// same page, so a page is measured once and argued over many times.
pub fn analyze(root: NodeRef<'_, Node>, opts: &PruneOpts) -> PruneReport {
    let doc = super::content::stats::Doc::of(root);
    let root_text = doc.root_text();
    if root_text == 0 {
        return PruneReport::default();
    }
    // How much text lives inside page chrome. If nearly all of it does, the page *is* navigation.
    //
    // Both remaining passes count open ancestors as they walk rather than asking each node to look
    // its ancestry up. `ancestors()` is O(depth) per node, so asking made the pass O(n·depth); a
    // counter incremented on the way in and decremented on the way out is O(n) and says the same
    // thing, because `traverse()` yields every element's Open before any descendant's and its
    // Close after all of them.
    let chrome_tags = ["nav", "header", "footer", "aside", "form", "dialog"];
    let mut chrome_text = 0u32;
    let mut open_chrome = 0usize;
    for edge in root.traverse() {
        let (node, opening) = match edge {
            Edge::Open(n) => (n, true),
            Edge::Close(n) => (n, false),
        };
        let Some(el) = ElementRef::wrap(node) else {
            continue;
        };
        if !chrome_tags.contains(&el.value().name()) {
            continue;
        }
        if opening {
            // Nested chrome is already counted through its ancestor.
            if open_chrome == 0 {
                chrome_text += doc.get(node.id()).map(|s| s.text).unwrap_or(0);
            }
            open_chrome += 1;
        } else {
            open_chrome -= 1;
        }
    }
    // The only question is whether anything survives. A ratio would wrongly keep the navigation on
    // a short article wrapped in a large menu, where dropping it is exactly what we want.
    let skip_chrome = root_text.saturating_sub(chrome_text) >= 30;

    let mut drop = HashSet::new();
    let mut dropped_text = 0u32;
    let mut open_dropped = 0usize;
    for edge in root.traverse() {
        let (node, opening) = match edge {
            Edge::Open(n) => (n, true),
            Edge::Close(n) => (n, false),
        };
        if !opening {
            if drop.contains(&node.id()) {
                open_dropped -= 1;
            }
            continue;
        }
        let Some(el) = ElementRef::wrap(node) else {
            continue;
        };
        let name = el.value().name();
        if PROTECTED.contains(&name) || !CANDIDATES.contains(&name) {
            continue;
        }
        // Anything under an already-dropped node is gone with it; do not double-count its text.
        if open_dropped > 0 {
            continue;
        }
        let Some(s) = doc.get(node.id()) else {
            continue;
        };
        if s.has_pre || s.has_data_table {
            continue;
        }

        let link_density = s.link_density();
        let text_density = s.text_density();
        let bonus = class_bonus(el, opts.thread);
        let score = 0.45 * (1.0 - link_density)
            + 0.30 * (text_density / 10.0).min(1.0)
            + 0.15 * (s.commas as f32 / 5.0).min(1.0)
            + 0.10 * bonus;

        let share = doc.share(node.id());
        // ▲ **A class name condemns on the name alone here, and that was measured and kept.**
        //
        // The clause below drops a small container whose class says `related`, `promo`, `share`,
        // `social` or `widget` without asking what is inside it, and on WCXB that clause plus the
        // link-density one account for 381 of the 910 required phrases the shipping extractor
        // drops. Exempting blocks that read like running text was built and swept — three commas,
        // two hundred characters, link density under a half, and tighter and looser than that:
        //
        // | commas/chars/density | held-out recall | held-out leak | held-out F1 |
        // |---|---|---|---|
        // | (no exemption)       | 93.3% | 11.3% | **0.870** |
        // | 4 / 300 / 0.25       | 93.3% | 11.4% | 0.865 |
        // | 3 / 200 / 0.35       | 93.6% | 11.8% | 0.862 |
        // | 2 / 120 / 0.50       | 93.8% | 12.6% | 0.859 |
        //
        // Every setting brings back more boilerplate than content, and F1 falls monotonically as
        // the rule loosens. The development split liked it (+1.0 recall); the held-out split did
        // not. Measured, rejected, and left here so the next person does not build it again.
        let remove = (s.text < opts.min_text && s.blocks == 0)
            || (link_density > opts.max_link_density && s.text < 200)
            || (link_density > 0.8 && share < 0.4)
            || (score < opts.min_score && share < 0.2)
            || (bonus < 0.0 && share < 0.25);

        if remove {
            drop.insert(node.id());
            dropped_text += s.text;
            open_dropped += 1;
        }
    }

    // Safety net. Readability-style extractors are notorious for occasionally returning an empty
    // page; here that failure is arithmetic we already have, so it costs nothing to refuse.
    let kept = root_text.saturating_sub(dropped_text);
    let floor = 200.max((root_text as f32 * 0.10) as u32);
    if kept < floor {
        return PruneReport {
            pruned: Pruned::default(),
            skip_chrome,
        };
    }
    PruneReport {
        pruned: Pruned(drop),
        skip_chrome,
    }
}

#[cfg(test)]
mod tests {
    use crate::{extract_markdown_opts, ExtractOpts};

    fn md_pruned(html: &str) -> String {
        extract_markdown_opts(
            html,
            &ExtractOpts {
                main_content_only: true,
                ..Default::default()
            },
        )
    }

    const ARTICLE: &str = r#"
      <html><body>
        <div class="sidebar"><ul>
          <li><a href="/1">Link one</a></li><li><a href="/2">Link two</a></li>
          <li><a href="/3">Link three</a></li><li><a href="/4">Link four</a></li>
        </ul></div>
        <div class="content">
          <p>The quick brown fox jumps over the lazy dog, repeatedly, with enthusiasm and care.</p>
          <p>Ownership in Rust means each value has a single owner, and the compiler enforces it.</p>
          <p>Borrowing lets you reference data without taking ownership, which avoids copying.</p>
        </div>
        <div class="related"><a href="/a">Related A</a> <a href="/b">Related B</a></div>
      </body></html>"#;

    #[test]
    fn a_link_rail_is_dropped_and_the_article_survives() {
        let out = md_pruned(ARTICLE);
        assert!(out.contains("quick brown fox"), "article lost: {out}");
        assert!(out.contains("Ownership in Rust"), "article lost: {out}");
        assert!(
            !out.contains("Link three"),
            "sidebar link rail survived pruning: {out}"
        );
    }

    #[test]
    fn code_blocks_are_never_pruned() {
        let html = format!(
            "<html><body><div class=\"content\">{}</div>\
             <div class=\"widget\"><pre><code>fn main() {{}}</code></pre></div></body></html>",
            "<p>Some prose that is long enough to matter for the ratio checks here.</p>".repeat(4)
        );
        let out = md_pruned(&html);
        assert!(out.contains("fn main()"), "code block pruned: {out}");
    }

    #[test]
    fn data_tables_are_never_pruned() {
        let html = format!(
            "<html><body><div class=\"content\">{}</div>\
             <div class=\"widget\"><table><tr><th>A</th><th>B</th></tr>\
             <tr><td>1</td><td>2</td></tr></table></div></body></html>",
            "<p>Some prose that is long enough to matter for the ratio checks here.</p>".repeat(4)
        );
        let out = md_pruned(&html);
        assert!(out.contains("| 1 | 2 |"), "data table pruned: {out}");
    }

    /// A page that is genuinely only navigation must still return its links, not an empty string.
    #[test]
    fn an_index_page_is_not_emptied() {
        let mut html = String::from("<html><body><nav><ul>");
        for i in 0..40 {
            html.push_str(&format!("<li><a href=\"/p{i}\">Page number {i}</a></li>"));
        }
        html.push_str("</ul></nav></body></html>");
        let out = md_pruned(&html);
        assert!(
            out.contains("Page number 3"),
            "the safety net should have refused this pruning: {out}"
        );
    }

    #[test]
    fn a_clean_article_is_left_alone() {
        let html = format!(
            "<html><body><article>{}</article></body></html>",
            "<p>Plain prose with nothing to strip, repeated for length and substance.</p>"
                .repeat(6)
        );
        let out = md_pruned(&html);
        assert_eq!(
            out.matches("Plain prose").count(),
            6,
            "a clean article must survive intact: {out}"
        );
    }

    /// Deeply nested markup is common enough in the wild (wrapper divs from CMS templates) that
    /// the scoring pass must not recurse. 300 levels is well past anything real.
    #[test]
    fn a_deep_dom_does_not_overflow_the_scoring_pass() {
        let depth = 300;
        let mut html = String::from("<html><body>");
        html.push_str(&"<div>".repeat(depth));
        html.push_str("deep content that is long enough to survive any pruning decision at all");
        html.push_str(&"</div>".repeat(depth));
        html.push_str("</body></html>");
        let out = md_pruned(&html);
        assert!(out.contains("deep content"), "{out}");
    }

    /// Nested chrome must be counted once, not once per level.
    ///
    /// The chrome tally decides `skip_chrome`, which decides whether `nav`, `header`, `footer` and
    /// friends are dropped wholesale. Counting a `<nav>` inside a `<header>` twice inflates the
    /// tally past the page's own text and turns the decision off on exactly the pages it is for.
    /// The old pass avoided that by asking each node for its ancestry; this one counts open
    /// ancestors as it walks, which is the same answer in linear time — and this is the fixture
    /// where the two would disagree if it were not.
    #[test]
    fn chrome_inside_chrome_is_counted_once() {
        let mut html = String::from("<html><body><header><nav><ul>");
        for i in 0..30 {
            html.push_str(&format!(
                "<li><a href=\"/s{i}\">Section number {i}</a></li>"
            ));
        }
        html.push_str("</ul></nav></header><div class=\"content\">");
        html.push_str(
            &"<p>Real prose that carries the page and is long enough to be worth keeping.</p>"
                .repeat(6),
        );
        html.push_str("</div></body></html>");

        let out = md_pruned(&html);
        assert!(out.contains("Real prose"), "the article was lost: {out}");
        assert!(
            !out.contains("Section number 12"),
            "double-counted chrome switched the chrome skip off: {out}"
        );
    }

    /// A container inside a dropped container is already gone, and must not be charged again.
    ///
    /// `dropped_text` feeds the safety net that abandons every removal when too little would
    /// survive. Charging a nested sidebar twice pushes the tally over the floor and reverts the
    /// whole pass, so the page comes back with its navigation intact and nothing says why.
    #[test]
    fn a_container_inside_a_dropped_one_is_not_charged_twice() {
        let mut html = String::from("<html><body><div class=\"content\">");
        html.push_str(
            &"<p>Real prose that carries the page and is long enough to be worth keeping.</p>"
                .repeat(8),
        );
        html.push_str("</div><div class=\"sidebar\"><div class=\"related\"><ul>");
        for i in 0..25 {
            html.push_str(&format!("<li><a href=\"/r{i}\">Related story {i}</a></li>"));
        }
        html.push_str("</ul></div></div></body></html>");

        let out = md_pruned(&html);
        assert!(out.contains("Real prose"), "the article was lost: {out}");
        assert!(
            !out.contains("Related story 9"),
            "the nested rail survived, which is what double-charging the drop tally does: {out}"
        );
    }
}
