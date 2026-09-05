//! Deciding block by block, on word counts and link density alone.
//!
//! This is the `NumWordsRulesClassifier` from Kohlschütter, Fankhauser and Nejdl, *Boilerplate
//! Detection using Shallow Text Features* (WSDM 2010) — a C4.8 decision tree with five comparisons,
//! which is the whole model. It is here for one reason the other voter cannot offer: **it reads no
//! characters and no punctuation.** Words per block and the share of them inside links, for the
//! block, the one before it and the one after it. Nothing else.
//!
//! That matters because of what happens to the alternatives outside English. "Multilingual
//! Benchmarking of Main Content Extractors" (SIGIR 2025) put three extractors across five
//! languages and found Readability at 0.862 on English and 0.672 on Chinese, Trafilatura at 0.883
//! and 0.555 — and named the cause as character and comma counting. Boilerpipe, which is this
//! classifier, held up better on Russian (0.750 against Trafilatura's 0.759 with a far lower
//! variance) and is the reason this file exists rather than a second character-based heuristic.
//!
//! It is also the voter that sees a page as a sequence. Readability picks one subtree and keeps
//! everything in it; this asks of every block whether its neighbourhood reads like an article. On a
//! forum thread or a page whose content is split across sections — the two shapes WCXB reports
//! every extractor failing on — those are different questions, and the disagreement between the
//! two answers is exactly the signal the vote is built on.
//!
//! Reimplemented from the published tree; `boilerpipe` is Apache-2.0. The five constants are its
//! constants to six decimal places, so that a difference from the reference is a bug rather than a
//! judgement call. One thing is deliberately *not* the reference: how a word is counted. See
//! `words_of`.

use super::stats::Doc;
use ego_tree::{NodeId, NodeRef};
use scraper::{ElementRef, Node};
use std::collections::HashSet;

/// Elements that end a text block. Boilerpipe's own list, trimmed to what a DOM walk needs.
const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "aside",
    "nav",
    "header",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "dd",
    "dt",
    "blockquote",
    "pre",
    "td",
    "th",
    "figcaption",
    "form",
    "fieldset",
    "table",
    "tr",
    "ul",
    "ol",
    "dl",
];

/// One run of text, with the two numbers the tree asks about.
#[derive(Debug, Clone, Copy)]
pub struct Block {
    pub id: NodeId,
    pub words: u32,
    pub link_density: f32,
}

/// A neighbour, as the tree sees one: words and link density, no identity.
///
/// The reference implementation pads the sequence with a `TextBlock.EMPTY_START` so that the first
/// and last blocks are judged by the same five comparisons as the middle. Here the padding is a
/// `(0, 0.0)` pair rather than a fake node, because a padding block has no place in the document
/// and giving it one would mean inventing a `NodeId`.
type Neighbour = (u32, f32);

/// Whether the block is content, by the tree from the paper.
///
/// ```text
/// curr.link_density <= 0.333333
/// ├── prev.link_density <= 0.555556
/// │   └── curr.words <= 16 → next.words <= 15 → prev.words <= 4 ? BOILER : CONTENT
/// │                                           else CONTENT
/// │       else CONTENT
/// └── else: curr.words <= 40 → next.words <= 17 ? BOILER : CONTENT
///                            else CONTENT
/// else BOILER
/// ```
fn classify(prev: Neighbour, curr: Neighbour, next: Neighbour) -> bool {
    let (prev_words, prev_density) = prev;
    let (curr_words, curr_density) = curr;
    let (next_words, _) = next;

    if curr_density > 0.333_333 {
        return false;
    }
    if prev_density <= 0.555_556 {
        if curr_words > 16 {
            return true;
        }
        if next_words > 15 {
            return true;
        }
        prev_words > 4
    } else {
        if curr_words > 40 {
            return true;
        }
        next_words > 17
    }
}

/// The page as a sequence of text blocks, in document order.
///
/// A block is the nearest block-level ancestor of a run of text. Nested block elements each get
/// their own entry only when they hold text directly; a `div` that contains three `p`s is not a
/// fourth block, because counting it would let one paragraph vote four times.
pub fn blocks(root: NodeRef<'_, Node>, doc: &Doc) -> Vec<Block> {
    let mut out = Vec::new();
    for node in root.descendants() {
        let Some(el) = ElementRef::wrap(node) else {
            continue;
        };
        if !BLOCK_TAGS.contains(&el.value().name()) {
            continue;
        }
        // Only blocks that hold text of their own, so a wrapper does not duplicate its children.
        let own: String = node
            .children()
            .filter_map(|c| match c.value() {
                Node::Text(t) => Some(t.to_string()),
                _ => None,
            })
            .collect();
        let inline: String = node
            .descendants()
            .take_while(|d| d.id() == node.id() || !is_block(*d))
            .filter_map(|d| match d.value() {
                Node::Text(t) => Some(t.to_string()),
                _ => None,
            })
            .collect();
        let text = if own.trim().is_empty() { inline } else { own };
        let words = words_of(&text);
        if words == 0 {
            continue;
        }
        let link_density = doc.get(node.id()).map(|s| s.link_density()).unwrap_or(0.0);
        out.push(Block {
            id: node.id(),
            words,
            link_density,
        });
    }
    out
}

fn is_block(node: NodeRef<'_, Node>) -> bool {
    ElementRef::wrap(node).is_some_and(|e| BLOCK_TAGS.contains(&e.value().name()))
}

/// Words, counted so that a script without spaces is not read as one word per sentence.
///
/// ▲ The reference implementation counts whitespace-separated runs, and that is a hidden language
/// assumption — the one this whole file exists to avoid. Chinese and Japanese put no spaces between
/// words, so a five-sentence Chinese paragraph counts as five "words", lands under every threshold
/// in the tree, and is condemned as boilerplate. That is not a hypothetical: it is what the first
/// version of this file did, and the fixture below is what caught it.
///
/// So a run of ideographs counts as one word per character. Chinese averages about one and a half
/// characters to a word, so this overcounts by roughly half — deliberately, because the thresholds
/// are lower bounds and overcounting errs towards calling text content, which is the direction that
/// loses nothing.
fn words_of(text: &str) -> u32 {
    text.split_whitespace()
        .map(|token| token.chars().filter(|c| is_ideograph(*c)).count().max(1) as u32)
        .sum()
}

/// Han, Hiragana, Katakana, Hangul and the CJK compatibility ranges — the scripts that are written
/// without spaces between words.
fn is_ideograph(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF      // Hiragana, Katakana
        | 0x3400..=0x4DBF    // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0xAC00..=0xD7AF    // Hangul syllables
        | 0xF900..=0xFAFF    // CJK Compatibility Ideographs
        | 0x20000..=0x2FA1F  // Extensions B onwards
    )
}

/// The blocks this voter calls boilerplate.
///
/// Returned as the set to drop rather than the set to keep, so it composes with the other voters
/// and with `Md::walk`, which already consults a drop set.
pub fn condemned(root: NodeRef<'_, Node>, doc: &Doc) -> HashSet<NodeId> {
    let seq = blocks(root, doc);
    let mut out = HashSet::new();
    for (i, b) in seq.iter().enumerate() {
        // The sequence is padded with empty neighbours at both ends, exactly as the reference
        // implementation pads with `TextBlock.EMPTY_START`, so the first and last blocks are judged
        // by the same tree as the middle.
        let prev = seq
            .get(i.wrapping_sub(1))
            .filter(|_| i > 0)
            .map(|p| (p.words, p.link_density))
            .unwrap_or((0, 0.0));
        let next = seq
            .get(i + 1)
            .map(|n| (n.words, n.link_density))
            .unwrap_or((0, 0.0));
        if !classify(prev, (b.words, b.link_density), next) {
            out.insert(b.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn dropped(html: &str) -> (Html, HashSet<NodeId>) {
        let doc = Html::parse_document(html);
        let stats = Doc::of(doc.tree.root());
        let out = condemned(doc.tree.root(), &stats);
        (doc, out)
    }

    fn text_of(doc: &Html, id: NodeId) -> String {
        doc.tree
            .get(id)
            .and_then(ElementRef::wrap)
            .map(|e| e.text().collect())
            .unwrap_or_default()
    }

    /// A link rail is condemned and the prose beside it is not.
    #[test]
    fn navigation_is_boilerplate_and_prose_is_not() {
        let mut html = String::from("<html><body><ul>");
        for i in 0..20 {
            html.push_str(&format!("<li><a href=\"/s{i}\">Section {i}</a></li>"));
        }
        html.push_str("</ul>");
        for _ in 0..4 {
            html.push_str(
                "<p>The council voted on Tuesday to approve the harbour measure after a debate \
                 that ran past midnight and the money was already set aside for it.</p>",
            );
        }
        html.push_str("</body></html>");

        let (doc, out) = dropped(&html);
        let condemned_text: String = out.iter().map(|id| text_of(&doc, *id)).collect();
        assert!(
            condemned_text.contains("Section 7"),
            "the link rail survived: {condemned_text:.200}"
        );
        assert!(
            !condemned_text.contains("harbour measure"),
            "the article was condemned: {condemned_text:.200}"
        );
    }

    /// ▲ The point of this voter: the same decision without reading a single character of prose.
    ///
    /// It failed the first time it was run. Chinese writes no spaces between words, so counting
    /// whitespace-separated runs made a five-sentence paragraph five "words" — under every
    /// threshold in the tree — and the article was condemned as boilerplate while the link rail
    /// beside it survived. `words_of` counts ideographs individually and this passes; the fixture
    /// stays so that the next person to touch the counter finds out the same way.
    #[test]
    fn a_page_with_no_latin_punctuation_is_still_read() {
        let mut html = String::from("<html><body><ul>");
        for i in 0..20 {
            html.push_str(&format!("<li><a href=\"/s{i}\">导航链接{i}</a></li>"));
        }
        html.push_str("</ul>");
        for _ in 0..4 {
            html.push_str(
                "<p>市议会星期二投票批准了港口议案 辩论一直持续到午夜之后 而这笔款项早已预留 \
                 这项决定将在下个月生效 并且需要进一步的审议</p>",
            );
        }
        html.push_str("</body></html>");

        let (doc, out) = dropped(&html);
        let condemned_text: String = out.iter().map(|id| text_of(&doc, *id)).collect();
        assert!(
            !condemned_text.contains("港口议案"),
            "the Chinese article was condemned: {condemned_text:.200}"
        );
        assert!(
            condemned_text.contains("导航链接"),
            "the Chinese link rail survived: {condemned_text:.200}"
        );
    }

    /// The tree itself, at the five boundaries the paper states. If any of these move, this is no
    /// longer the published classifier and the citation above is a lie.
    #[test]
    fn the_decision_tree_is_the_published_one() {
        // Above the link-density cut-off is boilerplate, whatever else is true.
        assert!(!classify((100, 0.0), (100, 0.34), (100, 0.0)));
        // A long block with a quiet neighbourhood is content.
        assert!(classify((100, 0.0), (17, 0.0), (0, 0.0)));
        // A short block whose neighbours are also short is not.
        assert!(!classify((4, 0.0), (16, 0.0), (15, 0.0)));
        // …unless what follows it is long.
        assert!(classify((4, 0.0), (16, 0.0), (16, 0.0)));
        // After a link-heavy block the bar rises: 40 words, or a long follower.
        assert!(!classify((100, 0.56), (40, 0.0), (17, 0.0)));
        assert!(classify((100, 0.56), (41, 0.0), (0, 0.0)));
        assert!(classify((100, 0.56), (40, 0.0), (18, 0.0)));
    }

    #[test]
    fn an_empty_page_condemns_nothing() {
        let (_, out) = dropped("<html><body></body></html>");
        assert!(out.is_empty());
    }
}
