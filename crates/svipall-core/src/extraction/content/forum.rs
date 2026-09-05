//! Telling a discussion thread from an article, which is the one page type worth the trouble.
//!
//! # Why this and not the router
//!
//! WCXB measures the extraction gap per page type, and forums are its widest: 0.808 for the leading
//! system against 0.466 for Readability, whose `unlikelyCandidates` pattern contains the literal
//! word `comment` and so strips a thread's posts by name. svipall's own measurement agrees — told a
//! thread is a thread, the vote scores **0.766** on WCXB's held-out forums against **0.675**
//! without. That is +0.09 on the pages in question, and it is the largest single gain any of the
//! extraction work has turned up.
//!
//! A seven-way page-type router was built to reach it and recovered almost none of it, because it
//! named forums right about a third of the time; it has since been retired. So this asks one
//! question instead of seven, and answers it from what the page declares about itself rather than
//! from a model.
//!
//! # What it is worth, measured
//!
//! Scanned over all 2,008 WCXB pages, against their human labels:
//!
//! | signal | precision | recall |
//! |---|---|---|
//! | `DiscussionForumPosting` or `SocialMediaPosting` | **1.000** | 0.402 |
//! | …plus `itemtype=".../Comment"` | 0.953 | 0.616 |
//! | `QAPage` or `schema.org/Question` | 0.625 | 0.213 |
//!
//! ▲ The first has **no false positives at all** across 1,844 non-forum pages. That is what makes
//! this worth having where a 72%-accurate router is not: it can be believed. `Comment` costs five
//! false positives out of 1,844 and buys twenty points of recall, which is the trade this takes.
//! `QAPage` is refused — FAQ blocks on service and product pages wear it, and at 0.625 precision it
//! would misroute more pages than it rescues.
//!
//! Sixty-one of the 164 forums declare nothing at all. Those are [`Evidence::Structural`]'s
//! problem: a thread is a run of sibling blocks that each carry an author and a date, which is the
//! shape Harvest (Weichselbraun et al., WI-IAT 2020, Apache-2.0) locates by requiring a candidate
//! path to yield several siblings and to cover most of the page's text.

use super::stats::Doc;
use ego_tree::NodeRef;
use scraper::{ElementRef, Node, Selector};
use std::sync::LazyLock;

/// Why this page was called a thread, in descending order of how much it can be believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
    /// The page declares a `DiscussionForumPosting` or a `SocialMediaPosting`.
    Posting,
    /// The page declares `schema.org/Comment` and nothing stronger.
    Comment,
    /// The page is built like a thread: repeated sibling blocks, each with an author and a date.
    Structural,
}

/// Elements that carry a microdata type. Read once, matched by suffix, because a publisher may
/// write `http://schema.org/...`, `https://schema.org/...` or the bare name.
static ITEMTYPE: LazyLock<Option<Selector>> = LazyLock::new(|| Selector::parse("[itemtype]").ok());
static LD_JSON: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("script[type=\"application/ld+json\"]").ok());

/// The types that mean "a person posted this, and others replied".
///
/// `SocialMediaPosting` is `DiscussionForumPosting`'s parent and Google documents them under the
/// same requirements, so a site that picked the general one is saying the same thing.
const POSTING_TYPES: &[&str] = &["discussionforumposting", "socialmediaposting"];

/// A comment, which is weaker: an article with a comment section wears it too. Kept because it is
/// worth twenty points of recall for five false positives in eighteen hundred pages.
const COMMENT_TYPE: &str = "comment";

/// How many posts it takes to be a thread rather than a page with a reply box.
const MIN_POSTS: usize = 3;
/// How much of the page's text the candidate posts must account for.
///
/// Harvest scores a candidate path by its coverage of the total page content and takes the best;
/// this is the same idea as a floor rather than a ranking, because the question here is only
/// whether such a run exists at all.
const MIN_COVERAGE: f32 = 0.25;
/// A post's own text, below which a "post" is a byline or a vote count.
const MIN_POST_TEXT: u32 = 40;

/// Is this page a discussion thread?
///
/// `None` is "no reason to think so", which is not the same as "no". The caller uses it to pick an
/// extraction profile, and the profile it falls back to is the safe one.
pub fn detect(doc: &scraper::Html, stats: &Doc) -> Option<Evidence> {
    if let Some(e) = declared(doc) {
        return Some(e);
    }
    structural(doc, stats).then_some(Evidence::Structural)
}

/// The same question, from HTML rather than from a parsed document.
///
/// For callers outside the fetch funnel — the benchmark, and anything measuring the detector
/// against a corpus. It parses, which the funnel deliberately does only once per response, so a
/// fetch must not use it: `parse_page` calls [`detect`] on the tree it already has.
pub fn is_forum(html: &str) -> Option<Evidence> {
    let doc = scraper::Html::parse_document(html);
    let stats = Doc::of(doc.tree.root());
    detect(&doc, &stats)
}

/// What the page says about itself, in microdata or JSON-LD.
fn declared(doc: &scraper::Html) -> Option<Evidence> {
    let mut comment = false;
    if let Some(sel) = ITEMTYPE.as_ref() {
        for el in doc.select(sel) {
            let Some(v) = el.value().attr("itemtype") else {
                continue;
            };
            let name = v.rsplit('/').next().unwrap_or(v).to_ascii_lowercase();
            if POSTING_TYPES.contains(&name.as_str()) {
                return Some(Evidence::Posting);
            }
            comment |= name == COMMENT_TYPE;
        }
    }
    // Google recommends microdata for this type precisely because the text would otherwise be
    // duplicated, but JSON-LD is supported and plenty of sites use it.
    if let Some(sel) = LD_JSON.as_ref() {
        for script in doc.select(sel) {
            let body: String = script.text().collect();
            let low = body.to_ascii_lowercase();
            if POSTING_TYPES.iter().any(|t| low.contains(t)) {
                return Some(Evidence::Posting);
            }
        }
    }
    comment.then_some(Evidence::Comment)
}

/// Is the page built like a thread, whatever it declares?
///
/// The test is Harvest's, reduced from "find the posts" to "do posts exist": some parent has at
/// least [`MIN_POSTS`] element children that each carry their own prose, an author-ish link and a
/// date-ish element, and together account for a real share of the page.
///
/// Deliberately free of vocabulary. No class names, no `comment`, no stopword list — a thread in
/// Japanese on software nobody has heard of has the same shape as one on phpBB, and a class-name
/// rule would find neither.
/// Containers a repeated sibling set inside is never a thread.
///
/// ▲ **Three tags, and Harvest's longer discount list is measured and rejected.** It was tried —
/// `select`, `option`, `footer`, `header`, `nav`, `form`, `fieldset` and the table family — on the
/// argument that a `<select>` of forty countries and a footer of forty links wear the silhouette
/// this rule looks for. WCXB priced it: **one real forum lost** on the held-out split (structural
/// precision 0.700 → 0.667, recall 0.137 → 0.118) and **not one** of the false positives it was
/// added for removed. A second run with the table family taken back out changed neither number,
/// which also rules out the obvious explanation — phpBB laying posts out in a table.
///
/// The premise was wrong twice over: the shape it was meant to catch is not detected here anyway.
/// A footer of five repeated link blocks with dates does not fire this rule, so the false
/// positives it was added for were never footers.
///
/// Discounting containers is not wrong in principle; Harvest does it. On this corpus it is a cost
/// with no benefit, and a rule kept because it sounds right is what this project exists to refuse.
const SKIP_ANCESTORS: &[&str] = &["html", "head", "body"];

fn structural(doc: &scraper::Html, stats: &Doc) -> bool {
    let total = stats.root_text().max(1) as f32;
    for parent in doc.tree.root().descendants() {
        let Some(el) = ElementRef::wrap(parent) else {
            continue;
        };
        // A thread's posts are siblings under one container, never the whole document.
        if SKIP_ANCESTORS.contains(&el.value().name()) {
            continue;
        }
        let mut posts = 0usize;
        let mut covered = 0u32;
        for child in parent.children() {
            let Some(text) = stats.get(child.id()).map(|s| s.text) else {
                continue;
            };
            if text < MIN_POST_TEXT {
                continue;
            }
            if !looks_like_a_post(child) {
                continue;
            }
            posts += 1;
            covered += text;
        }
        if posts >= MIN_POSTS && covered as f32 / total >= MIN_COVERAGE {
            return true;
        }
    }
    false
}

/// One post: prose, somebody who wrote it, and when.
fn looks_like_a_post(node: NodeRef<'_, Node>) -> bool {
    let mut author = false;
    let mut dated = false;
    for d in node.descendants() {
        let Some(el) = ElementRef::wrap(d) else {
            continue;
        };
        match el.value().name() {
            // A machine-readable date. Harvest looks for exactly this first, then falls back to
            // parsing text; a `<time datetime>` is the half that needs no language.
            "time" if el.value().attr("datetime").is_some() => dated = true,
            // An author is a link to somewhere on this site with a short label. Harvest's own rule:
            // fewer than four words and under a hundred characters.
            "a" => {
                if author {
                    continue;
                }
                let Some(href) = el.value().attr("href") else {
                    continue;
                };
                if href.starts_with("http") && !href.contains("://") {
                    continue;
                }
                let label: String = el.text().collect();
                let label = label.trim();
                if !label.is_empty()
                    && label.len() < 100
                    && label.split_whitespace().count() < 4
                    && looks_like_a_profile(href)
                {
                    author = true;
                }
            }
            _ => {}
        }
        // Microdata says it outright when it is there, in any language.
        if let Some(prop) = el.value().attr("itemprop") {
            match prop.to_ascii_lowercase().as_str() {
                "author" => author = true,
                "datepublished" | "datemodified" => dated = true,
                _ => {}
            }
        }
        if author && dated {
            return true;
        }
    }
    author && dated
}

/// Does this href point at a person rather than at another article?
///
/// Every forum engine ever written puts its members somewhere like `/user/`, `/u/`, `/members/` or
/// `?u=`. This is the one place a vocabulary is used, and it is a vocabulary of URL shapes rather
/// than of words, so it does not care what language the forum is in.
fn looks_like_a_profile(href: &str) -> bool {
    let low = href.to_ascii_lowercase();
    const SHAPES: &[&str] = &[
        "/user/",
        "/users/",
        "/u/",
        "/member/",
        "/members/",
        "/profile/",
        "/people/",
        "/account/",
        "memberlist",
        "profile.php",
        "?u=",
        "&u=",
        "/@",
    ];
    SHAPES.iter().any(|s| low.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A regression guard on the shape the rule is *for*: repeated sibling blocks, each with an
    /// author link, a machine-readable date and enough prose to be a post.
    #[test]
    fn repeated_posts_under_an_ordinary_container_are_a_thread() {
        let post = |i: usize| {
            format!(
                "<div><a href=\"/u/{i}\">User {i}</a>\
                 <time datetime=\"2026-01-0{i}\">January</time>\
                 <p>Everything you might want to know about section {i} of this shop, written out \
                 at enough length that it clears the floor for a post.</p></div>"
            )
        };
        let thread: String = (1..6).map(post).collect();
        let html = format!(
            "<!doctype html><html><body><main><div class=\"x\">{thread}</div></main></body></html>"
        );
        assert_eq!(
            is_forum(&html),
            Some(Evidence::Structural),
            "the structural test stopped working"
        );
    }
    use scraper::Html;

    fn detect_in(html: &str) -> Option<Evidence> {
        let doc = Html::parse_document(html);
        let stats = Doc::of(doc.tree.root());
        detect(&doc, &stats)
    }

    /// The signal with no false positives on 1,844 pages: the page says what it is.
    #[test]
    fn a_declared_posting_is_believed() {
        let micro = "<html><body><div itemscope \
                     itemtype=\"https://schema.org/DiscussionForumPosting\">\
                     <p>The harbour measure passed.</p></div></body></html>";
        assert_eq!(detect_in(micro), Some(Evidence::Posting));

        let ld = "<html><head><script type=\"application/ld+json\">\
                  {\"@type\":\"SocialMediaPosting\",\"text\":\"hello\"}</script></head>\
                  <body><p>hi</p></body></html>";
        assert_eq!(detect_in(ld), Some(Evidence::Posting));

        // The parent type and the bare name are the same claim written differently.
        let bare = "<html><body><div itemtype=\"http://schema.org/Comment\" itemscope>\
                    <p>quite so</p></div></body></html>";
        assert_eq!(detect_in(bare), Some(Evidence::Comment));
    }

    /// ▲ Refused on purpose. `QAPage` sits on FAQ blocks all over service and product pages: 0.625
    /// precision on the corpus, which would misroute more pages than it rescues.
    #[test]
    fn a_question_and_answer_page_is_not_evidence_of_a_forum() {
        let faq = "<html><body><div itemscope itemtype=\"https://schema.org/QAPage\">\
                   <p>How do I reset my password? Use the link on the sign-in page.</p>\
                   </div></body></html>";
        assert_eq!(detect_in(faq), None);
    }

    /// A thread that declares nothing, in a language with no Latin script, on software nobody has
    /// heard of. The structural test has to find it without reading a word.
    #[test]
    fn an_undeclared_thread_is_found_by_its_shape() {
        let mut html = String::from("<html><body><div class=\"x1\">");
        for i in 0..6 {
            html.push_str(&format!(
                "<div class=\"y2\">\
                 <a href=\"/u/{i}\">利用者{i}</a>\
                 <time datetime=\"2026-03-0{}T10:00:00Z\">3月</time>\
                 <p>港湾の議案は火曜日に可決されました。審議は深夜まで続き、資金はすでに \
                 確保されていました。この決定は来月から有効になります。</p></div>",
                i + 1
            ));
        }
        html.push_str("</div></body></html>");
        assert_eq!(detect_in(&html), Some(Evidence::Structural));
    }

    /// An article with a byline and a date must not read as a thread, however many links it has.
    #[test]
    fn an_article_is_not_a_thread() {
        let mut html = String::from(
            "<html><body><article><h1>Harbour measure passes</h1>\
             <a href=\"/authors/jane\">Jane Doe</a><time datetime=\"2026-03-01\">March</time>",
        );
        for _ in 0..8 {
            html.push_str(
                "<p>The council voted on Tuesday to approve the harbour measure, after a debate \
                 that ran past midnight and the money was already set aside.</p>",
            );
        }
        html.push_str("</article></body></html>");
        assert_eq!(detect_in(&html), None);
    }

    /// Three near-empty blocks with a name and a date in each are a vote list, not a discussion.
    #[test]
    fn a_run_of_bylines_is_not_a_thread() {
        let mut html = String::from("<html><body><div>");
        for i in 0..8 {
            html.push_str(&format!(
                "<div><a href=\"/user/{i}\">member{i}</a>\
                 <time datetime=\"2026-03-01\">today</time></div>"
            ));
        }
        html.push_str("</div><article><p>");
        html.push_str(&"The real article text goes on at some length here. ".repeat(30));
        html.push_str("</p></article></body></html>");
        assert_eq!(detect_in(&html), None);
    }

    #[test]
    fn an_empty_page_is_not_a_thread() {
        assert_eq!(detect_in("<html><body></body></html>"), None);
    }
}
