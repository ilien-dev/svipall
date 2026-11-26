//! A page built of post and comment elements, read from those elements.
//!
//! Reddit's pages carry each post and each comment as one custom element whose attributes are the
//! record: title, author, score, time, depth. Its server streams the comments inside a
//! `<template>` that the page's own script then moves into place. The ordinary walk skips a
//! template, rightly, so it saw a megabyte of markup holding a few hundred characters, the ladder
//! called that an unrendered shell, and four browsers were sent after what the first answer
//! already held.
//!
//! So this reads the elements. Nothing here is hidden text: it is what the page shows a reader
//! once its script has run, and an advertisement is a different element that is never asked for.
//!
//! A fragment the page would have loaded next (more posts, more comments) is made of the same
//! elements and reads the same way, which is what makes the next page a plain fetch.

use crate::Rendered;
use scraper::{ElementRef, Html, Selector};
use std::sync::LazyLock;
use url::Url;

static POST: LazyLock<Option<Selector>> = LazyLock::new(|| Selector::parse("shreddit-post").ok());
static COMMENT: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("shreddit-comment").ok());
static POST_BODY: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("[slot=\"text-body\"]").ok());
static ARTICLE_BODY: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("[property=\"schema:articleBody\"]").ok());
static COMMENT_BODY: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("[slot=\"comment\"]").ok());
static PARTIAL: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("faceplate-partial[src]").ok());
/// One search result. The site serves two layouts of the same page, a visitor at a time, and
/// they differ in what the unit is called and in nothing this reads from it.
static RESULT: LazyLock<Option<Selector>> = LazyLock::new(|| {
    Selector::parse(
        "[data-testid=\"search-sdui-post\"], [data-testid=\"search-post-with-content-preview\"]",
    )
    .ok()
});
static RESULT_TITLE: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("a[data-testid=\"post-title\"][href]").ok());
static COMMUNITY: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("faceplate-hovercard[label]").ok());
static TIME: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("faceplate-timeago[ts]").ok());
static NUMBER: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("faceplate-number[number]").ok());
static RULE: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("faceplate-tracker[source=\"rules_widget\"]").ok());
static RULE_NAME: LazyLock<Option<Selector>> = LazyLock::new(|| Selector::parse("h2").ok());
static RULE_TEXT: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("[faceplate-auto-height-animator-content]").ok());
static TITLE: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("shreddit-title[title]").ok());

const COMMENT_TAG: &str = "shreddit-comment";
/// The fragment that asks a community's page for its rules in full instead of its posts. A
/// fragment never leaves the machine, so the page fetched is the page anyone is served.
pub const RULES_FRAGMENT: &str = "rules";
/// Where a path is resolved when the caller gave no address for the page.
const HOME: &str = "https://www.reddit.com/";
/// Quote marks past this depth say nothing a reader can still count.
const MAX_QUOTE_DEPTH: usize = 8;
/// What a "more posts" address carries about the visit it was served to rather than about the
/// page it names. Dropped, so the address is the same whoever reads it.
const SESSION_PARAMS: &[&str] = &[
    "navigationSessionId",
    "ad_posts_served",
    "feedLength",
    "adDistance",
    "distance",
];

/// The title the page declares for itself. Its `<title>` is a placeholder until the script runs.
pub(crate) fn title(doc: &Html) -> Option<String> {
    let t = doc.select(TITLE.as_ref()?).next()?.attr("title")?.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// The page as its posts and comments, or `None` when it has neither.
pub(crate) fn read(doc: &Html, base: Option<&Url>) -> Option<Rendered> {
    let posts: Vec<ElementRef> = doc.select(POST.as_ref()?).collect();
    let comments: Vec<ElementRef> = doc.select(COMMENT.as_ref()?).collect();
    let results: Vec<ElementRef> = if posts.is_empty() && comments.is_empty() {
        doc.select(RESULT.as_ref()?).collect()
    } else {
        Vec::new()
    };
    if posts.is_empty() && comments.is_empty() && results.is_empty() {
        return None;
    }
    let home = Url::parse(HOME).ok();
    let base = base.or(home.as_ref());
    let mut out = Blocks::default();
    if !results.is_empty() {
        if let Some(t) = title(doc) {
            out.push(format!("# {t}"), t);
        }
        for r in &results {
            search_result(*r, base, &mut out);
        }
        if let Some(next) = next_page(doc, base) {
            out.push(format!("Next page: {next}"), String::new());
        }
        return Some(out.finish());
    }
    let thread = !comments.is_empty()
        || (posts.len() == 1 && posts[0].attr("view-context") == Some("CommentsPage"));
    if thread {
        if let Some(post) = posts.first() {
            thread_post(*post, base, &mut out);
        }
        let total = posts
            .first()
            .and_then(|p| p.attr("comment-count"))
            .and_then(|c| c.parse::<usize>().ok());
        let heading = match total {
            Some(t) if t > comments.len() => {
                format!("Comments ({} of {t} loaded)", comments.len())
            }
            _ => format!("Comments ({})", comments.len()),
        };
        if !comments.is_empty() || total.is_some() {
            out.push(format!("## {heading}"), heading);
        }
        for c in &comments {
            comment(*c, base, &mut out);
        }
    } else {
        if let Some(t) = title(doc) {
            out.push(format!("# {t}"), t);
        }
        let rules = rules(doc, base);
        if base.and_then(Url::fragment) == Some(RULES_FRAGMENT) && !rules.is_empty() {
            out.push("## Rules".into(), "Rules".into());
            for (i, (name, body)) in rules.into_iter().enumerate() {
                out.push(
                    format!("**{}. {name}**\n{}", i + 1, body.markdown),
                    format!("{}. {name}\n{}", i + 1, body.text),
                );
            }
            return Some(out.finish());
        }
        for p in &posts {
            feed_post(*p, base, &mut out);
        }
        // Named, not recited: a feed is read far more often than its rules are wanted.
        if !rules.is_empty() {
            let names: Vec<String> = rules
                .iter()
                .enumerate()
                .map(|(i, (name, _))| format!("{}. {name}", i + 1))
                .collect();
            let mut full = base.cloned();
            if let Some(u) = full.as_mut() {
                u.set_query(None);
                u.set_fragment(Some(RULES_FRAGMENT));
            }
            let line = format!(
                "Rules ({}): {}",
                full.map(String::from).unwrap_or_default(),
                names.join(" · ")
            );
            out.push(line.clone(), line);
        }
        if let Some(next) = next_page(doc, base) {
            out.push(format!("Next page: {next}"), String::new());
        }
    }
    Some(out.finish())
}

/// Markdown and plain text, block by block, so the two can never describe different pages.
#[derive(Default)]
struct Blocks {
    md: Vec<String>,
    text: Vec<String>,
}

impl Blocks {
    fn push(&mut self, md: String, text: String) {
        if !md.trim().is_empty() {
            self.md.push(md);
        }
        if !text.trim().is_empty() {
            self.text.push(text);
        }
    }

    fn finish(self) -> Rendered {
        Rendered {
            markdown: crate::finish(&self.md.join("\n\n")),
            text: crate::finish(&self.text.join("\n")),
        }
    }
}

fn attr<'a>(el: ElementRef<'a>, name: &str) -> Option<&'a str> {
    el.attr(name).map(str::trim).filter(|v| !v.is_empty())
}

fn counted(n: &str, one: &str, many: &str) -> String {
    if n == "1" {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// `2026-03-04T09:15:30.120000+0000` as `2026-03-04 09:15 UTC`. Anything else is given as it is:
/// a time in a zone this did not expect is still a time.
fn when(ts: &str) -> String {
    let utc = ts.ends_with("+0000") || ts.ends_with('Z') || ts.ends_with("+00:00");
    match (ts.get(..10), ts.get(10..11), ts.get(11..16)) {
        (Some(day), Some("T"), Some(clock)) if utc => format!("{day} {clock} UTC"),
        _ => ts.to_string(),
    }
}

fn points(el: ElementRef) -> Option<String> {
    let score = counted(attr(el, "score")?, "point", "points");
    let ratio = attr(el, "upvote-ratio")
        .and_then(|r| r.parse::<f32>().ok())
        .filter(|r| (0.0..=1.0).contains(r));
    Some(match ratio {
        Some(r) => format!("{score} ({}% upvoted)", (r * 100.0).round() as u32),
        None => score,
    })
}

fn absolute(base: Option<&Url>, href: &str) -> String {
    base.and_then(|b| b.join(href).ok())
        .map(String::from)
        .unwrap_or_else(|| href.to_string())
}

/// Where a link post points, when that is somewhere other than the post itself.
fn outbound(post: ElementRef, base: Option<&Url>) -> Option<String> {
    let href = absolute(base, attr(post, "content-href")?);
    let own = absolute(base, attr(post, "permalink")?);
    (href != own).then_some(href)
}

fn post_body(post: ElementRef, base: Option<&Url>) -> Option<Rendered> {
    let slot = post.select(POST_BODY.as_ref()?).next()?;
    // The slot also holds the control that unfolds a long post; the text itself says what it is.
    let body = ARTICLE_BODY
        .as_ref()
        .and_then(|s| slot.select(s).next())
        .unwrap_or(slot);
    let r = crate::render_inside(body, base);
    (!r.text.trim().is_empty()).then_some(r)
}

fn thread_post(post: ElementRef, base: Option<&Url>, out: &mut Blocks) {
    if let Some(t) = attr(post, "post-title") {
        out.push(format!("# {t}"), t.to_string());
    }
    let meta: Vec<String> = [
        attr(post, "subreddit-prefixed-name").map(str::to_string),
        attr(post, "author").map(|a| format!("u/{a}")),
        points(post),
        attr(post, "comment-count").map(|c| counted(c, "comment", "comments")),
        attr(post, "created-timestamp").map(when),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut head = meta.join(" · ");
    if let Some(link) = outbound(post, base) {
        head.push_str(&format!("\nLink: {link}"));
    }
    out.push(head.clone(), head);
    if let Some(body) = post_body(post, base) {
        out.push(body.markdown, body.text);
    }
}

fn feed_post(post: ElementRef, base: Option<&Url>, out: &mut Blocks) {
    let Some(title) = attr(post, "post-title") else {
        return;
    };
    let meta: Vec<String> = [
        points(post),
        attr(post, "comment-count").map(|c| counted(c, "comment", "comments")),
        attr(post, "author").map(|a| format!("u/{a}")),
        attr(post, "subreddit-prefixed-name").map(str::to_string),
        attr(post, "created-timestamp").map(when),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut tail = meta.join(" · ");
    if let Some(link) = outbound(post, base) {
        tail.push_str(&format!("\nLink: {link}"));
    }
    let body = post_body(post, base);
    let escaped = title.replace('[', "\\[").replace(']', "\\]");
    let mut md = match attr(post, "permalink") {
        Some(p) => format!("**[{escaped}]({})**\n{tail}", absolute(base, p)),
        None => format!("**{title}**\n{tail}"),
    };
    let mut text = format!("{title}\n{tail}");
    if let Some(b) = body {
        md.push('\n');
        md.push_str(&b.markdown);
        text.push('\n');
        text.push_str(&b.text);
    }
    out.push(md, text);
}

/// A community's rules, as its page shows them beside the feed: each one's name, and the text
/// that unfolds under it.
fn rules(doc: &Html, base: Option<&Url>) -> Vec<(String, Rendered)> {
    let (Some(rule), Some(name), Some(text)) =
        (RULE.as_ref(), RULE_NAME.as_ref(), RULE_TEXT.as_ref())
    else {
        return Vec::new();
    };
    doc.select(rule)
        .filter_map(|r| {
            let title = r
                .select(name)
                .next()?
                .text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            // The name sits in the fold's handle; the text is the rest of the same fold.
            let fold = r
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|a| a.value().name() == "details")?;
            let body = fold
                .select(text)
                .next()
                .map(|b| crate::render_inside(b, base))
                .unwrap_or_default();
            (!title.is_empty()).then_some((title, body))
        })
        .collect()
}

/// One search result: the same line a feed gives a post, from the few elements a result has.
fn search_result(unit: ElementRef, base: Option<&Url>, out: &mut Blocks) {
    let Some(link) = RESULT_TITLE.as_ref().and_then(|s| unit.select(s).next()) else {
        return;
    };
    let title = attr(link, "aria-label")
        .map(str::to_string)
        .unwrap_or_else(|| link.text().collect::<String>().trim().to_string());
    let Some(href) = attr(link, "href") else {
        return;
    };
    if title.is_empty() {
        return;
    }
    let numbers: Vec<&str> = NUMBER
        .as_ref()
        .map(|s| unit.select(s).filter_map(|n| attr(n, "number")).collect())
        .unwrap_or_default();
    let meta: Vec<String> = [
        numbers.first().map(|n| counted(n, "point", "points")),
        numbers.get(1).map(|n| counted(n, "comment", "comments")),
        COMMUNITY
            .as_ref()
            .and_then(|s| unit.select(s).next())
            .and_then(|c| attr(c, "label"))
            .map(str::to_string),
        TIME.as_ref()
            .and_then(|s| unit.select(s).next())
            .and_then(|t| attr(t, "ts"))
            .map(when),
    ]
    .into_iter()
    .flatten()
    .collect();
    let tail = meta.join(" · ");
    let escaped = title.replace('[', "\\[").replace(']', "\\]");
    out.push(
        format!("**[{escaped}]({})**\n{tail}", absolute(base, href)),
        format!("{title}\n{tail}"),
    );
}

/// The comment element this node sits in, if any.
fn owner(el: ElementRef) -> Option<ElementRef> {
    el.ancestors()
        .filter_map(ElementRef::wrap)
        .find(|a| a.value().name() == COMMENT_TAG)
}

fn comment(el: ElementRef, base: Option<&Url>, out: &mut Blocks) {
    let depth = attr(el, "depth")
        .and_then(|d| d.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_QUOTE_DEPTH);
    let author = attr(el, "author").unwrap_or("[deleted]");
    let meta: Vec<String> = [
        attr(el, "score").map(|s| counted(s, "point", "points")),
        attr(el, "created").map(when),
    ]
    .into_iter()
    .flatten()
    .collect();
    // A reply is nested inside the comment it answers, so the first body found is only this
    // comment's when this comment is the nearest one above it.
    let body = COMMENT_BODY.as_ref().and_then(|s| {
        el.select(s)
            .find(|b| owner(*b).is_some_and(|o| o.id() == el.id()))
            .map(|b| crate::render_inside(b, base))
    });
    let mut head = format!("**u/{author}**");
    let mut plain = format!("u/{author}");
    for m in &meta {
        head.push_str(&format!(" · {m}"));
        plain.push_str(&format!(" · {m}"));
    }
    let (body_md, body_text) = body.map(|b| (b.markdown, b.text)).unwrap_or_default();
    let quote = "> ".repeat(depth);
    let md: String = std::iter::once(head.as_str())
        .chain(body_md.lines())
        .map(|l| format!("{quote}{l}"))
        .collect::<Vec<_>>()
        .join("\n");
    out.push(md, format!("{plain}\n{body_text}"));
}

/// The address the page would ask for when a reader scrolls to the end of a feed or of a page of
/// search results.
fn next_page(doc: &Html, base: Option<&Url>) -> Option<String> {
    let src = doc
        .select(PARTIAL.as_ref()?)
        // A feed marks the element as what loads after its last post; a page of search results
        // names the search and where it left off.
        .filter(|p| {
            p.attr("slot") == Some("load-after")
                || p.attr("src")
                    .is_some_and(|s| s.contains("/search/") && s.contains("cursor="))
        })
        .filter_map(|p| p.attr("src"))
        // The last one: a document that already holds a continuation holds its element too.
        .next_back()?;
    let mut url = base?.join(src).ok()?;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !SESSION_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(kept);
    Some(url.into())
}
