//! What every content heuristic needs to know about every node, computed once.
//!
//! The published extractors all read the same handful of numbers off each container — how much
//! text is under it, how much of that text is inside links, how many tags it took to hold, how many
//! sentence-ish blocks and commas it contains, whether anything in it must survive whatever the
//! score says. Readability's candidate scoring, Kohlschütter's decision tree and the density pass
//! this crate already had are three different arguments over the same five figures.
//!
//! So they are gathered in one traversal and handed round, rather than recomputed per heuristic.
//! That is not only cheaper: it is what lets several heuristics disagree about the *same* page
//! rather than about three slightly different readings of it, which is the whole point of asking
//! more than one of them.
//!
//! Iterative on purpose. Real documents nest hundreds of levels deep — the fixture in `prune.rs`
//! goes three hundred — and a recursive pass over one of those overflows the stack.

use ego_tree::iter::Edge;
use ego_tree::{NodeId, NodeRef};
use scraper::Node;
use std::collections::HashMap;

/// What one subtree is made of.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    /// Characters of trimmed text under this node.
    pub text: u32,
    /// Of those, the ones inside an `<a>`.
    pub link_text: u32,
    /// Elements under this node, itself included.
    pub tags: u32,
    /// Paragraph-like children: `p`, `h1`–`h6`, `li`, `blockquote`.
    pub blocks: u32,
    /// Commas, which every extractor since Readability has used as a proxy for prose.
    pub commas: u32,
    /// A `<pre>` or `<code>` is here. Code is content whatever its density says.
    pub has_pre: bool,
    /// A `<table>` is here.
    pub has_data_table: bool,
}

impl Stats {
    fn merge(&mut self, child: &Stats) {
        self.text += child.text;
        self.link_text += child.link_text;
        self.tags += child.tags;
        self.blocks += child.blocks;
        self.commas += child.commas;
        self.has_pre |= child.has_pre;
        self.has_data_table |= child.has_data_table;
    }

    /// The share of this subtree's text that sits inside links. Navigation is near 1, prose near 0.
    pub fn link_density(&self) -> f32 {
        self.link_text as f32 / self.text.max(1) as f32
    }

    /// Characters of text per element. Prose is dense; a link rail is not.
    pub fn text_density(&self) -> f32 {
        self.text as f32 / self.tags.max(1) as f32
    }
}

/// Every node's statistics, plus how much text the document has at all.
pub struct Doc {
    stats: HashMap<NodeId, Stats>,
    root_text: u32,
}

impl Doc {
    /// One traversal of the subtree at `root`.
    pub fn of(root: NodeRef<'_, Node>) -> Self {
        let mut stats: HashMap<NodeId, Stats> = HashMap::new();
        let mut stack: Vec<Stats> = vec![Stats::default()];

        for edge in root.traverse() {
            match edge {
                Edge::Open(node) => match node.value() {
                    Node::Text(t) => {
                        let s = stack.last_mut().expect("stack never empties");
                        let trimmed = t.trim();
                        s.text += trimmed.len() as u32;
                        s.commas += trimmed.matches(',').count() as u32;
                    }
                    Node::Element(_) => stack.push(Stats::default()),
                    _ => {}
                },
                Edge::Close(node) => {
                    let Node::Element(el) = node.value() else {
                        continue;
                    };
                    let mut own = stack.pop().unwrap_or_default();
                    own.tags += 1;
                    match el.name() {
                        "a" => own.link_text += own.text,
                        "pre" | "code" => own.has_pre = true,
                        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "blockquote" => {
                            own.blocks += 1
                        }
                        "table" => own.has_data_table = true,
                        _ => {}
                    }
                    stats.insert(node.id(), own);
                    if let Some(parent) = stack.last_mut() {
                        parent.merge(&own);
                    }
                }
            }
        }

        // The document's own text, whether or not it hangs off a single element. Taking the max of
        // "the direct children together" and "the largest single subtree" covers both a body with
        // several sections and a body wrapped in one div, without a special case for either.
        let root_text = root
            .children()
            .filter_map(|c| stats.get(&c.id()))
            .map(|s| s.text)
            .sum::<u32>()
            .max(stats.values().map(|s| s.text).max().unwrap_or(0));

        Self { stats, root_text }
    }

    pub fn get(&self, id: NodeId) -> Option<&Stats> {
        self.stats.get(&id)
    }

    /// Text in the document. Zero means there is nothing here to judge.
    pub fn root_text(&self) -> u32 {
        self.root_text
    }

    /// This subtree's share of the document's text.
    pub fn share(&self, id: NodeId) -> f32 {
        match (self.stats.get(&id), self.root_text) {
            (Some(s), r) if r > 0 => s.text as f32 / r as f32,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subtree_carries_its_descendants_numbers() {
        let html = scraper::Html::parse_document(
            "<html><body><div id=\"a\"><p>Hello, world, again.</p>\
             <ul><li><a href=\"/x\">link text</a></li></ul></div></body></html>",
        );
        let doc = Doc::of(html.tree.root());
        let sel = scraper::Selector::parse("#a").expect("selector");
        let el = html.select(&sel).next().expect("the div");
        let s = doc.get(el.id()).expect("stats for the div");

        assert_eq!(
            s.text,
            "Hello, world, again.".len() as u32 + "link text".len() as u32
        );
        assert_eq!(s.link_text, "link text".len() as u32);
        assert_eq!(s.commas, 2);
        // One p and one li.
        assert_eq!(s.blocks, 2);
        assert!(!s.has_pre && !s.has_data_table);
        assert!(s.link_density() > 0.0 && s.link_density() < 1.0);
    }

    #[test]
    fn code_and_tables_are_flagged_all_the_way_up() {
        let html = scraper::Html::parse_document(
            "<html><body><div id=\"a\"><pre><code>let x = 1;</code></pre>\
             <table><tr><td>1</td></tr></table></div></body></html>",
        );
        let doc = Doc::of(html.tree.root());
        let sel = scraper::Selector::parse("#a").expect("selector");
        let el = html.select(&sel).next().expect("the div");
        let s = doc.get(el.id()).expect("stats");
        assert!(s.has_pre, "a pre under a container must reach it");
        assert!(s.has_data_table, "so must a table");
    }

    #[test]
    fn an_empty_document_has_no_text_to_judge() {
        let html = scraper::Html::parse_document("<html><body></body></html>");
        assert_eq!(Doc::of(html.tree.root()).root_text(), 0);
    }
}
