//! What to do differently once you know what kind of page you are reading.
//!
//! The forum detector says whether a page is a discussion; this says what that changes. Nothing
//! here decides *how much* is removed — that is the vote's, and its unanimity rule is what keeps a
//! misidentified page safe. A profile only ever changes which voices are asked and which containers
//! they are asked about.
//!
//! ## Why there is no longer a model behind this
//!
//! There was: a seven-class linear router over twenty-two structural ratios. It was built, trained
//! against WCXB's 1,358 development pages and measured, and it is gone. Two numbers retired it.
//! Told the *true* page type by the corpus itself, the extractor gained **+0.010** — that is a
//! ceiling, measured with an oracle rather than a prediction, and it is the same order as the
//! +0.003/+0.007 WCXB's own hybrid pipeline reported for routing. And the router recovered
//! **0.001** of it, because it named the profile right 72% of the time and the type right 52.8%
//! against a 50.3% baseline of always answering "article".
//!
//! What survives is the vocabulary and the table, because the forum detector uses both — and that
//! detector reads what a page *declares about itself* at precision 1.000 across both splits, rather
//! than predicting from features. When the cheaper signal is the more reliable one, the model is
//! the part to remove.
//!
//! ## What the corpus says each type needs
//!
//! WCXB names the structural cause of each failure, and each entry below answers one of them.
//!
//! | type | what breaks | what this does |
//! |---|---|---|
//! | article | nothing; every system is within 2–3 points | the plain vote |
//! | documentation | sidebar nav and version pickers read as content | the plain vote |
//! | forum | posts live in `class="comment"`, which article extractors strip | the candidate voter is *not* asked |
//! | collection | filter panels interleaved with a product grid | no single root is chosen |
//! | listing | repeated cards; one-node selection returns one card | no single root is chosen |
//! | product | the description is in JSON-LD, not the DOM | no single root is chosen |
//! | service | content across 5–15 `<section>`s; one-node selection keeps the hero | no single root is chosen |
//!
//! ▲ Two mechanisms, not seven. The article-shaped voter is silenced on forums because its
//! `unlikelyCandidates` regex contains the literal word `comment`, so on a thread it condemns the
//! content by name. And root selection is switched off wherever the content is not one subtree —
//! which is five of the seven types. Everything else is the same vote; a profile that changed more
//! than that would be a second extractor per page type, and there is no evidence that would help.

use super::vote::Rule;
use serde::{Deserialize, Serialize};

/// The seven structural kinds, in WCXB's own order.
///
/// Kept after the router was retired because it is the vocabulary the profile table is written in,
/// the label the corpus ships, and what the forum detector resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageType {
    Article,
    Documentation,
    Service,
    Forum,
    Collection,
    Listing,
    Product,
}

impl PageType {
    pub const ALL: &'static [PageType] = &[
        PageType::Article,
        PageType::Documentation,
        PageType::Service,
        PageType::Forum,
        PageType::Collection,
        PageType::Listing,
        PageType::Product,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PageType::Article => "article",
            PageType::Documentation => "documentation",
            PageType::Service => "service",
            PageType::Forum => "forum",
            PageType::Collection => "collection",
            PageType::Listing => "listing",
            PageType::Product => "product",
        }
    }

    pub fn parse(s: &str) -> Option<PageType> {
        Self::ALL
            .iter()
            .copied()
            .find(|t| t.as_str() == s.trim().to_ascii_lowercase())
    }
}

/// How to read one kind of page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// Ask the Readability-style voter. Off where its own vocabulary condemns the content.
    pub candidates: bool,
    /// Ask the shallow-text voter. On everywhere: it is the language-neutral one.
    pub blocks: bool,
    /// Ask the density pass. On everywhere: it is the incumbent.
    pub density: bool,
    /// Narrow the document to one subtree before rendering. Off where the content is not one.
    pub single_root: bool,
    /// How many voices it takes to remove a block.
    pub rule: Rule,
}

impl Default for Profile {
    /// What a page gets when nothing is known about it: every voter, one root, unanimity.
    ///
    /// This is deliberately the same as the article profile. Without a model there is no routing,
    /// and the fallback has to be the shape that is both commonest and safest to be wrong about.
    fn default() -> Self {
        Self {
            candidates: true,
            blocks: true,
            density: true,
            single_root: true,
            rule: Rule::Unanimous,
        }
    }
}

/// The profile for a page of this kind.
pub fn for_type(t: PageType) -> Profile {
    let plain = Profile::default();
    match t {
        // Solved: 0.91–0.93 for every leading system, and svipall is in that band.
        PageType::Article | PageType::Documentation => plain,

        // ▲ The one profile that silences a voter. Readability's `unlikelyCandidates` pattern
        // contains `comment` and `replies`, so on a discussion thread it strips exactly the posts
        // that are the content. Unanimity then makes that fatal in the other direction: with the
        // candidate voter condemning every post, one more agreeing voice would empty the page.
        // WCXB measures the damage — Readability scores 0.466 on forums against 0.808 for the
        // leader, its widest gap of the seven.
        PageType::Forum => Profile {
            candidates: false,
            single_root: false,
            ..plain
        },

        // Content in several places at once. A grid of cards, a list of entries, a description
        // that may only exist in JSON-LD, a marketing page across a dozen sections: in all four,
        // "the best single subtree" is a category error, so no subtree is chosen and the vote
        // works over the whole document.
        PageType::Collection | PageType::Listing | PageType::Product | PageType::Service => {
            Profile {
                single_root: false,
                ..plain
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_has_a_profile_and_none_of_them_silences_everyone() {
        for t in PageType::ALL {
            let p = for_type(*t);
            let voices = usize::from(p.candidates) + usize::from(p.blocks) + usize::from(p.density);
            assert!(
                voices >= 2,
                "{} is left with {voices} voter(s); unanimity over one voter is that voter \
                 deciding alone",
                t.as_str()
            );
        }
    }

    /// ▲ The rule is never relaxed by a profile. A page type is allowed to change who is asked and
    /// what they are asked about; if it could also lower the bar for removal, a misrouted page
    /// could lose content, and the whole safety argument for the router would be gone.
    #[test]
    fn no_profile_lowers_the_bar_for_removing_content() {
        for t in PageType::ALL {
            assert_eq!(
                for_type(*t).rule,
                Rule::Unanimous,
                "{} weakened the removal rule",
                t.as_str()
            );
        }
    }

    #[test]
    fn the_default_is_the_article_profile_because_that_is_the_safe_guess() {
        assert_eq!(Profile::default(), for_type(PageType::Article));
    }

    #[test]
    fn the_types_that_hold_content_in_several_places_do_not_pick_one() {
        for t in [
            PageType::Collection,
            PageType::Listing,
            PageType::Product,
            PageType::Service,
            PageType::Forum,
        ] {
            assert!(
                !for_type(t).single_root,
                "{} narrowed to one subtree",
                t.as_str()
            );
        }
        assert!(for_type(PageType::Article).single_root);
    }
}
