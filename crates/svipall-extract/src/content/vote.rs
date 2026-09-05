//! Several heuristics reading one page, and what to do when they disagree.
//!
//! ## Why a vote at all
//!
//! Bevendorff et al. (SIGIR 2023) benchmarked fourteen extractors over 3,985 annotated pages and
//! then built three ensembles on top of them. **All three beat every individual system**, in both
//! mean and median macro-average F1, and the paper's closing advice is explicit: *"Combining
//! multiple simple models may be a better way forward"* than a larger single one. Their method was
//! a token-level majority vote at a two-thirds threshold.
//!
//! ## Why unanimity rather than two thirds
//!
//! Because the threshold is not the point here. A vote gives something a single extractor cannot:
//! a per-page reading of how much its judges agreed. svipall's quality design says a page is
//! labelled and never withheld, and this is the extraction-level form of the same promise — so the
//! rule is that **a block is removed only when every voter condemns it**. Anything less than
//! unanimous is kept.
//!
//! That makes the failure mode one-sided by construction. A voter that misfires, a threshold that
//! is wrong for this page, a page type nobody tuned for: each can only ever cause boilerplate to be
//! *kept*, which costs tokens. None of them can cause content to be dropped, which costs the
//! answer. Cuconasu et al. (SIGIR 2024) is the reason that asymmetry is worth paying for — they
//! found that the documents which actually degrade an answer are the high-scoring, on-topic,
//! answer-free ones, while adding plainly irrelevant text *improved* accuracy by up to 35%.
//!
//! The two-thirds threshold from the paper is still available, as `Rule::Majority`, for a caller
//! that has asked for precision over recall. It is not the default and it never will be.
//!
//! ## The voters
//!
//! Three, chosen to fail differently rather than to agree:
//!
//! * [`super::candidates`] — Readability's competitive subtree scoring. Best median in SIGIR-23,
//!   worst on non-article page types in WCXB, English-shaped.
//! * [`super::blocks`] — Kohlschütter's shallow-text decision tree. Sequence-aware, and the only
//!   one that reads no characters and no punctuation.
//! * [`super::super::prune`] — svipall's own density pass, kept rather than replaced. It is the
//!   incumbent, it knows about `<pre>` and data tables, and a new system that cannot beat the one
//!   it replaces should not replace it silently.

use super::stats::Doc;
use super::{blocks, candidates};
use crate::prune;
use ego_tree::{NodeId, NodeRef};
use scraper::Node;
use std::collections::HashSet;

/// How many voices it takes to remove a block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Rule {
    /// Every voter must condemn it. Removal is only ever as aggressive as the mildest judge.
    #[default]
    Unanimous,
    /// Two thirds, as in the SIGIR-23 ensembles. More precise and able to lose content.
    Majority,
}

/// What the voters were asked and what they decided.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// Blocks to skip while rendering. Never a whole page: see `agreement`.
    pub drop: HashSet<NodeId>,
    /// The content root, when a voter identified one narrower than the document.
    pub root: Option<NodeId>,
    /// How much the voters agreed about what to remove, 0 to 1.
    ///
    /// Of every block at least one voter condemned, the share all of them did. One means they read
    /// the page the same way; near zero means they did not, and the page came back nearly whole
    /// because of it. This is the confidence signal the per-page-type policy is fitted against, and
    /// it costs nothing to compute because the sets already exist.
    pub agreement: f32,
    /// How many voters answered at all.
    pub voters: usize,
}

/// Ask every voter and combine them, under the default profile.
pub fn decide(root: NodeRef<'_, Node>, doc: &Doc, rule: Rule) -> Verdict {
    decide_with(
        root,
        doc,
        super::profile::Profile {
            rule,
            ..Default::default()
        },
    )
}

/// Ask the voters this kind of page calls for.
pub fn decide_with(
    root: NodeRef<'_, Node>,
    doc: &Doc,
    profile: super::profile::Profile,
) -> Verdict {
    let rule = profile.rule;
    // The units of the decision. Everything is resolved at block granularity so that three
    // heuristics with three different shapes — a subtree, a sequence, a set of containers — are
    // answering the same question about the same things.
    let units: Vec<NodeId> = blocks::blocks(root, doc)
        .into_iter()
        .map(|b| b.id)
        .collect();
    if units.is_empty() {
        return Verdict {
            drop: HashSet::new(),
            root: None,
            agreement: 1.0,
            voters: 0,
        };
    }

    let mut ballots: Vec<HashSet<NodeId>> = Vec::new();
    let mut content_root = None;

    // Voter 1: Readability. Everything outside the subtree it chose, plus the siblings it refused.
    // Silenced on a page type whose content its own vocabulary condemns - see `profile`.
    if let Some(choice) = profile
        .candidates
        .then(|| candidates::choose(root, doc, candidates::Flags::default()))
        .flatten()
    {
        let inside: HashSet<NodeId> = root
            .tree()
            .get(choice.root)
            .map(|n| n.descendants().map(|d| d.id()).collect())
            .unwrap_or_default();
        let mut condemned: HashSet<NodeId> = units
            .iter()
            .copied()
            .filter(|id| !inside.contains(id))
            .collect();
        for id in &choice.drop {
            if let Some(node) = root.tree().get(*id) {
                condemned.extend(
                    node.descendants()
                        .map(|d| d.id())
                        .filter(|d| units.contains(d)),
                );
            }
        }
        ballots.push(condemned);
        // Narrowing to one subtree is only right when the content *is* one subtree. On a grid, a
        // thread or a page of sections it is a category error, and the profile says so.
        if profile.single_root {
            content_root = Some(choice.root);
        }
    }

    // Voter 2: Kohlschütter, which already answers in exactly these units.
    if profile.blocks {
        ballots.push(blocks::condemned(root, doc));
    }

    // Voter 3: the density pass. It condemns containers, so a block is condemned when it or an
    // ancestor of it is.
    let report = if profile.density {
        prune::analyze(root, &prune::PruneOpts::default())
    } else {
        prune::PruneReport::default()
    };
    if !report.pruned.is_empty() {
        let condemned: HashSet<NodeId> = units
            .iter()
            .copied()
            .filter(|id| {
                root.tree().get(*id).is_some_and(|n| {
                    report.pruned.contains(n.id())
                        || n.ancestors().any(|a| report.pruned.contains(a.id()))
                })
            })
            .collect();
        ballots.push(condemned);
    }

    let voters = ballots.len();
    let needed = match rule {
        Rule::Unanimous => voters,
        // Two thirds, rounded up, as the paper does.
        Rule::Majority => voters.div_ceil(3) * 2,
    };

    let mut drop = HashSet::new();
    let (mut any, mut all) = (0usize, 0usize);
    for id in &units {
        let votes = ballots.iter().filter(|b| b.contains(id)).count();
        if votes > 0 {
            any += 1;
        }
        if votes == voters {
            all += 1;
        }
        if voters > 0 && votes >= needed {
            drop.insert(*id);
        }
    }

    Verdict {
        drop,
        root: content_root,
        agreement: if any == 0 {
            1.0
        } else {
            all as f32 / any as f32
        },
        voters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{ElementRef, Html};

    const ARTICLE: &str = "<p>The council voted on Tuesday to approve the harbour measure, after \
                           a debate that ran past midnight, and the money was already set aside \
                           for it three budgets ago.</p>";

    fn page() -> String {
        let mut html = String::from("<html><body><nav class=\"site-nav\"><ul>");
        for i in 0..30 {
            html.push_str(&format!("<li><a href=\"/s{i}\">Section {i}</a></li>"));
        }
        html.push_str("</ul></nav><div class=\"post-content\">");
        html.push_str(&ARTICLE.repeat(5));
        html.push_str("</div><div class=\"related\"><ul>");
        for i in 0..15 {
            html.push_str(&format!("<li><a href=\"/r{i}\">Related story {i}</a></li>"));
        }
        html.push_str("</ul></div></body></html>");
        html
    }

    fn verdict_of(html: &str, rule: Rule) -> (Html, Verdict) {
        let doc = Html::parse_document(html);
        let stats = Doc::of(doc.tree.root());
        let v = decide(doc.tree.root(), &stats, rule);
        (doc, v)
    }

    fn text_of(doc: &Html, ids: &HashSet<NodeId>) -> String {
        ids.iter()
            .filter_map(|id| doc.tree.get(*id))
            .filter_map(ElementRef::wrap)
            .map(|e| e.text().collect::<String>())
            .collect()
    }

    #[test]
    fn what_every_voter_condemns_is_dropped_and_the_article_is_not() {
        let html = page();
        let (doc, v) = verdict_of(&html, Rule::Unanimous);
        assert!(v.voters >= 2, "not enough voters answered: {v:?}");
        let dropped = text_of(&doc, &v.drop);
        assert!(
            !dropped.contains("harbour measure"),
            "the article was dropped: {dropped:.200}"
        );
        assert!(
            dropped.contains("Section 12") || dropped.contains("Related story 7"),
            "nothing recognisable as chrome was dropped: {dropped:.200}"
        );
    }

    /// ▲ The rule the whole design rests on. One voter saying "keep" is enough to keep.
    #[test]
    fn a_single_dissenting_voter_is_enough_to_keep_a_block() {
        let html = page();
        let doc = Html::parse_document(&html);
        let stats = Doc::of(doc.tree.root());
        let unanimous = decide(doc.tree.root(), &stats, Rule::Unanimous);
        let majority = decide(doc.tree.root(), &stats, Rule::Majority);
        assert!(
            unanimous.drop.len() <= majority.drop.len(),
            "unanimity removed more than a majority did, which cannot be right: \
             {} vs {}",
            unanimous.drop.len(),
            majority.drop.len()
        );
        assert!(
            unanimous.drop.is_subset(&majority.drop),
            "the two rules disagree about direction, not only degree"
        );
    }

    /// Agreement has to move, or it is not a signal.
    #[test]
    fn agreement_is_high_on_an_obvious_page_and_lower_on_an_ambiguous_one() {
        let (_, obvious) = verdict_of(&page(), Rule::Unanimous);

        // A page that is almost entirely a list of links with a sentence in each: a forum thread,
        // a listing, a documentation index. The voters are built to read this differently.
        let mut html = String::from("<html><body><div class=\"thread\">");
        for i in 0..25 {
            html.push_str(&format!(
                "<div class=\"comment\"><p><a href=\"/u{i}\">member{i}</a> wrote: the harbour \
                 measure passed and the money was set aside, which is what matters here.</p></div>"
            ));
        }
        html.push_str("</div></body></html>");
        let (_, ambiguous) = verdict_of(&html, Rule::Unanimous);

        assert!(
            obvious.agreement >= ambiguous.agreement,
            "a page every heuristic reads the same way should not score lower agreement \
             than one they read differently: {} vs {}",
            obvious.agreement,
            ambiguous.agreement
        );
    }

    #[test]
    fn a_page_with_nothing_in_it_is_left_alone() {
        let (_, v) = verdict_of("<html><body></body></html>", Rule::Unanimous);
        assert!(v.drop.is_empty());
        assert_eq!(v.voters, 0);
        assert_eq!(v.agreement, 1.0);
    }

    /// A forum thread is the shape WCXB reports every article extractor destroying: the posts live
    /// in containers named `comment`, which Readability is built to strip. Unanimity is what keeps
    /// them, because the other two voters see prose.
    #[test]
    fn a_forum_thread_keeps_its_posts() {
        let mut html = String::from("<html><body><div class=\"thread\">");
        for i in 0..12 {
            html.push_str(&format!(
                "<div class=\"comment\"><p>Post {i}: the council voted on Tuesday to approve the \
                 harbour measure, after a debate that ran past midnight, and the money was \
                 already set aside.</p></div>"
            ));
        }
        html.push_str("</div></body></html>");
        let (doc, v) = verdict_of(&html, Rule::Unanimous);
        let dropped = text_of(&doc, &v.drop);
        assert!(
            !dropped.contains("Post 6"),
            "the thread was stripped as comments: {dropped:.200}"
        );
    }
}
