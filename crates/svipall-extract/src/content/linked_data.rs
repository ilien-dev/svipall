//! A page that shows nothing without script, read from what it declares as schema.org data.
//!
//! Many applications answer the first request with an empty root element and a script, and put
//! what the page is about in `<script type="application/ld+json">` beside it: the profile and its
//! latest posts, the post and the replies it was served with, the question and its answers, the
//! video and its top comment, the list of openings. They put it there for a search engine, which
//! runs no script either. The ordinary walk sees a few words of "enable JavaScript", the ladder
//! calls the page an unrendered shell, and a browser is started to render what the first answer
//! already said in so many words.
//!
//! So this reads the declaration. Nothing here knows a site: it reads the vocabulary. A post is
//! whatever has a `text` or an `articleBody`, its replies are its `comment`, a question's answers
//! are `acceptedAnswer` and `suggestedAnswer`, a profile page's posts are its `hasPart`, a list's
//! entries are its `itemListElement`, and the counts are `interactionStatistic`.
//!
//! It is a reading of last resort and says so in two ways. The caller (`declared_reading` in
//! `lib.rs`) asks for it only when the page shows less than it declares, so an article that
//! renders is read from its markup as it always was. And what the data says is text: every
//! character that could write markdown is escaped, because a field of somebody's JSON is not a
//! place a page gets to draw links or headings from.

use super::post_elements::{counted, when};
use crate::Rendered;
use scraper::{Html, Selector};
use serde_json::Value;
use std::sync::LazyLock;
use url::Url;

static LD: LazyLock<Option<Selector>> =
    LazyLock::new(|| Selector::parse("script[type=\"application/ld+json\"]").ok());

/// A page showing less text than this, and less than its data says, is read from the data. The
/// same line the ladder draws for a shell.
pub(crate) const SHELL_TEXT: usize = 3000;
/// Below this the data named a thing and said nothing about it.
const LEAST_TEXT: usize = 200;
/// One script's worth of data. Past this it is a catalogue, not a page.
const MAX_SCRIPT: usize = 2_000_000;
/// Replies below this depth are not followed. Deeper than a reader goes, and a bound on what a
/// hostile document can make this walk.
const MAX_DEPTH: usize = 8;
/// Entries read from one list of posts, comments, answers or items.
const MAX_ENTRIES: usize = 200;
/// One field of prose.
const MAX_FIELD: usize = 40_000;
/// A heading taken from the first line of a post that has no headline.
const MAX_HEADING: usize = 120;

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

    /// The same words in both, a heading in one.
    fn heading(&mut self, level: usize, words: &str) {
        self.push(
            format!("{} {}", "#".repeat(level), escape(words)),
            words.to_string(),
        );
    }

    fn finish(self) -> Rendered {
        Rendered {
            markdown: crate::finish(&self.md.join("\n\n")),
            text: crate::finish(&self.text.join("\n")),
        }
    }
}

/// The last word of a type: `https://schema.org/Comment` and `Comment` are one type, and a node
/// of several types is read as its first.
fn kind(v: &Value) -> &str {
    let t = match v.get("@type") {
        Some(Value::String(s)) => s.as_str(),
        Some(Value::Array(a)) => a.first().and_then(Value::as_str).unwrap_or(""),
        _ => "",
    };
    t.rsplit(['/', '#', ':']).next().unwrap_or(t)
}

/// A value that may be one thing or a list of them, as a list.
fn many(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().take(MAX_ENTRIES).collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(one) => vec![one],
    }
}

fn string<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| {
        v.get(*k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    })
}

/// A count given as a number or as the digits of one.
fn count(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)),
        Value::String(s) => s.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn tally(n: u64, one: &str, many: &str) -> String {
    counted(&grouped(n), one, many)
}

/// Text from the data, unable to write markdown: the characters that open a link, emphasis, code
/// or markup are escaped everywhere, and the ones that open a heading or a quote at the start of
/// a line.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = line.trim_end_matches('\r');
        if line.trim_start().starts_with(['#', '>']) {
            out.push('\\');
        }
        for c in line.chars() {
            if matches!(c, '\\' | '[' | ']' | '*' | '`' | '<') {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

fn prose(v: &Value) -> Option<String> {
    let s = string(v, &["articleBody", "text", "description"])?;
    let cut = s
        .char_indices()
        .nth(MAX_FIELD)
        .map_or(s.len(), |(at, _)| at);
    Some(s[..cut].to_string())
}

/// Who: a name, a name and a handle, or the organisation hiring.
fn byline(v: &Value) -> Option<String> {
    let who = v
        .get("author")
        .or_else(|| v.get("creator"))
        .or_else(|| v.get("hiringOrganization"))?;
    let who = match who {
        Value::Array(a) => a.first()?,
        other => other,
    };
    if let Some(s) = who.as_str() {
        return Some(s.trim().to_string()).filter(|s| !s.is_empty());
    }
    let name = string(who, &["name"]);
    let handle = string(who, &["alternateName"]);
    match (name, handle) {
        (Some(n), Some(h)) if n != h => Some(format!("{n} ({h})")),
        (Some(n), _) => Some(n.to_string()),
        (None, Some(h)) => Some(h.to_string()),
        (None, None) => None,
    }
}

/// Where: a town and a country, when a posting says them.
fn place(v: &Value) -> Option<String> {
    let at = many(v.get("jobLocation").or_else(|| v.get("location")));
    let address = at.first()?.get("address")?;
    if let Some(s) = address.as_str() {
        return Some(s.trim().to_string()).filter(|s| !s.is_empty());
    }
    let parts: Vec<&str> = ["addressLocality", "addressRegion", "addressCountry"]
        .iter()
        .filter_map(|k| string(address, &[k]))
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// What an interaction counter counts, in the singular and the plural.
fn action(counter: &Value) -> Option<(String, String)> {
    let t = counter.get("interactionType")?;
    let name = match t {
        Value::String(s) => s.rsplit(['/', '#', ':']).next().unwrap_or(s),
        other => kind(other),
    };
    let word = name.strip_suffix("Action").unwrap_or(name);
    Some(match word {
        "Like" => ("like".into(), "likes".into()),
        "Share" => ("share".into(), "shares".into()),
        "Comment" => ("comment".into(), "comments".into()),
        "Reply" => ("reply".into(), "replies".into()),
        "Follow" | "Subscribe" => ("follower".into(), "followers".into()),
        "Watch" | "View" => ("view".into(), "views".into()),
        "Write" => ("post".into(), "posts".into()),
        "" => return None,
        other => {
            let w = other.to_ascii_lowercase();
            (w.clone(), format!("{w}s"))
        }
    })
}

/// The counts a thing carries, in the order it gave them, each once.
fn stats(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut has_comments = false;
    for c in many(v.get("interactionStatistic")) {
        let (Some((one, many)), Some(n)) = (action(c), count(c.get("userInteractionCount"))) else {
            continue;
        };
        has_comments |= one == "comment";
        out.push(tally(n, &one, &many));
    }
    if let Some(n) = count(v.get("upvoteCount")) {
        out.push(tally(n, "upvote", "upvotes"));
    }
    if let Some(n) = count(v.get("answerCount")) {
        out.push(tally(n, "answer", "answers"));
    }
    if !has_comments {
        if let Some(n) = count(v.get("commentCount")) {
            out.push(tally(n, "comment", "comments"));
        }
    }
    if let Some(d) = string(v, &["duration"]) {
        out.push(d.to_string());
    }
    out
}

fn date(v: &Value) -> Option<String> {
    string(
        v,
        &[
            "datePublished",
            "dateCreated",
            "uploadDate",
            "datePosted",
            "startDate",
        ],
    )
    .map(when)
}

/// Who, where, when and how much, on one line.
fn meta(v: &Value) -> Vec<String> {
    byline(v)
        .into_iter()
        .chain(place(v))
        .chain(date(v))
        .chain(stats(v))
        .collect()
}

fn link(v: &Value, base: Option<&Url>) -> Option<String> {
    let href = string(v, &["url"]).or_else(|| string(v, &["@id"]).filter(|s| s.contains("://")))?;
    let url = match base {
        Some(b) => b.join(href).ok()?,
        None => Url::parse(href).ok()?,
    };
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// A reply, an answer or a post in a list: who and when in bold, then what was said, quoted once
/// for each level it sits under.
fn entry(v: &Value, depth: usize, with_link: Option<&Url>, out: &mut Blocks) -> usize {
    if depth >= MAX_DEPTH || !v.is_object() {
        return 0;
    }
    let mut parts = meta(v).into_iter();
    let who = parts.next().unwrap_or_else(|| "[unknown]".into());
    let mut head = format!("**{}**", escape(&who));
    let mut plain = who;
    for m in parts {
        head.push_str(&format!(" · {}", escape(&m)));
        plain.push_str(&format!(" · {m}"));
    }
    let body = prose(v).unwrap_or_default();
    let link = with_link.and_then(|b| link(v, Some(b)));
    let quote = "> ".repeat(depth);
    let md: String = std::iter::once(head)
        .chain(escape(&body).lines().map(str::to_string))
        .chain(link.iter().map(|l| format!("Link: {l}")))
        .map(|l| format!("{quote}{l}"))
        .collect::<Vec<_>>()
        .join("\n");
    out.push(md, format!("{plain}\n{body}"));
    let mut read = 1;
    for reply in many(v.get("comment")) {
        read += entry(reply, depth + 1, None, out);
    }
    read
}

/// One entry of a list: its name as a link, what is known about it, what it says.
fn item(v: &Value, base: Option<&Url>, out: &mut Blocks) {
    let thing = v.get("item").filter(|i| i.is_object()).unwrap_or(v);
    let Some(name) = string(thing, &["headline", "name", "title"]).or_else(|| string(v, &["name"]))
    else {
        return;
    };
    let href = link(thing, base).or_else(|| link(v, base));
    let head = match &href {
        Some(h) => format!("**[{}]({h})**", escape(name)),
        None => format!("**{}**", escape(name)),
    };
    let meta = meta(thing).join(" · ");
    let body = prose(thing).unwrap_or_default();
    let md: Vec<String> = [head, escape(&meta), escape(&body)]
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect();
    out.push(md.join("\n"), format!("{name}\n{meta}\n{body}"));
}

/// A person's or an organisation's page: who they are, then what they posted.
fn profile(page: &Value, who: &Value, base: Option<&Url>, out: &mut Blocks) {
    let Some(name) = string(who, &["name", "alternateName"]) else {
        return;
    };
    out.heading(1, name);
    let mut line: Vec<String> = string(who, &["alternateName"])
        .filter(|h| *h != name)
        .map(str::to_string)
        .into_iter()
        .chain(stats(who))
        .collect();
    // What the person did, as against what was done to them: following, not followers.
    for c in many(who.get("agentInteractionStatistic")) {
        let (Some((one, many)), Some(n)) = (action(c), count(c.get("userInteractionCount"))) else {
            continue;
        };
        line.push(if one == "follower" {
            format!("following {}", grouped(n))
        } else {
            tally(n, &one, &many)
        });
    }
    if let Some(since) = string(page, &["dateCreated"]).or_else(|| string(who, &["dateCreated"])) {
        line.push(format!("since {}", when(since)));
    }
    let line = line.join(" · ");
    out.push(escape(&line), line);
    if let Some(about) = prose(who) {
        out.push(escape(&about), about);
    }
    let posts = many(page.get("hasPart"));
    if !posts.is_empty() {
        out.heading(2, &format!("Posts ({})", posts.len()));
        for p in posts {
            entry(p, 0, base.or(Some(&FALLBACK)), out);
        }
    }
}

/// A base for a link that is already absolute, when the caller gave no address for the page.
static FALLBACK: LazyLock<Url> = LazyLock::new(|| Url::parse("https://invalid.invalid/").unwrap());

/// A post, a question, a video, an article: its heading, who and when, what it says, and then
/// what was said back.
fn thing(v: &Value, base: Option<&Url>, out: &mut Blocks) {
    let body = prose(v);
    let named = string(v, &["headline", "name", "title"]);
    // A post without a headline is headed by its own first line, and the rest follows.
    let (heading, body) = match (named, body) {
        (Some(n), b) => (Some(n.to_string()), b),
        (None, Some(b)) => {
            let first = b.lines().next().unwrap_or("").trim();
            let cut = first
                .char_indices()
                .nth(MAX_HEADING)
                .map_or(first.len(), |(at, _)| at);
            let rest = b[first.len().min(b.len())..].trim_start().to_string();
            if cut < first.len() {
                (Some(format!("{}…", &first[..cut])), Some(b))
            } else {
                (
                    Some(first.to_string()),
                    Some(rest).filter(|r| !r.is_empty()),
                )
            }
        }
        (None, None) => (None, None),
    };
    if let Some(h) = heading.filter(|h| !h.is_empty()) {
        out.heading(1, &h);
    }
    let line = meta(v).join(" · ");
    out.push(escape(&line), line);
    if let Some(b) = body {
        out.push(escape(&b), b);
    }

    if let Some(a) = v.get("acceptedAnswer").filter(|a| a.is_object()) {
        out.heading(2, "Accepted answer");
        entry(a, 0, None, out);
    }
    let answers = many(v.get("suggestedAnswer"));
    if !answers.is_empty() {
        out.heading(2, &format!("Answers ({})", answers.len()));
        for a in answers {
            entry(a, 0, None, out);
        }
    }
    let comments = many(v.get("comment"));
    if !comments.is_empty() {
        // The heading says how many were read, which is only known once they are.
        let mut replies = Blocks::default();
        let read: usize = comments
            .iter()
            .map(|c| entry(c, 0, None, &mut replies))
            .sum();
        let total = count(v.get("commentCount")).or_else(|| {
            many(v.get("interactionStatistic"))
                .into_iter()
                .find_map(|c| {
                    (action(c)?.0 == "comment")
                        .then(|| count(c.get("userInteractionCount")))
                        .flatten()
                })
        });
        let label = match total {
            Some(t) if t as usize > read => format!("Comments ({read} of {} loaded)", grouped(t)),
            _ => format!("Comments ({read})"),
        };
        out.heading(2, &label);
        out.md.extend(replies.md);
        out.text.extend(replies.text);
    }
    for e in many(v.get("itemListElement")) {
        item(e, base, out);
    }
}

fn render(node: &Value, base: Option<&Url>) -> Rendered {
    let mut out = Blocks::default();
    let main = node.get("mainEntity").filter(|m| m.is_object());
    match (kind(node), main) {
        ("ProfilePage", Some(who)) => profile(node, who, base, &mut out),
        (_, Some(main)) => thing(main, base, &mut out),
        _ => thing(node, base, &mut out),
    }
    out.finish()
}

fn nodes(v: Value, out: &mut Vec<Value>) {
    match v {
        Value::Array(a) => a.into_iter().take(MAX_ENTRIES).for_each(|i| nodes(i, out)),
        Value::Object(mut o) => match o.remove("@graph") {
            Some(graph) => nodes(graph, out),
            None => out.push(Value::Object(o)),
        },
        _ => {}
    }
}

/// The page as its schema.org data says it, when that says something: the fullest of the things
/// it declares. `None` when it declares nothing, or only names.
pub(crate) fn read(doc: &Html, base: Option<&Url>) -> Option<Rendered> {
    let sel = LD.as_ref()?;
    let mut declared = Vec::new();
    for script in doc.select(sel) {
        let raw: String = script.text().collect();
        if raw.len() > MAX_SCRIPT {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(raw.trim()) {
            nodes(v, &mut declared);
        }
    }
    declared
        .iter()
        .map(|n| render(n, base))
        .max_by_key(|r| r.text.len())
        .filter(|r| r.text.len() >= LEAST_TEXT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_is_its_last_word() {
        let v: Value = serde_json::json!({"@type": "https://schema.org/Comment"});
        assert_eq!(kind(&v), "Comment");
        let v: Value = serde_json::json!({"@type": ["SocialMediaPosting", "Article"]});
        assert_eq!(kind(&v), "SocialMediaPosting");
        assert_eq!(kind(&serde_json::json!({})), "");
    }

    #[test]
    fn counts_are_grouped_and_agree_in_number() {
        assert_eq!(tally(1, "share", "shares"), "1 share");
        assert_eq!(tally(0, "comment", "comments"), "0 comments");
        assert_eq!(tally(1234567, "view", "views"), "1,234,567 views");
        assert_eq!(count(Some(&serde_json::json!("8,779"))), Some(8779));
        assert_eq!(count(Some(&serde_json::json!(12.0))), Some(12));
        assert_eq!(count(Some(&serde_json::json!("many"))), None);
    }

    #[test]
    fn text_cannot_open_a_link_a_heading_or_a_quote() {
        assert_eq!(
            escape("# not a heading\n> not a quote\nsee [here](x) and `code` <b>"),
            "\\# not a heading\n\\> not a quote\nsee \\[here\\](x) and \\`code\\` \\<b>"
        );
    }

    #[test]
    fn a_link_out_of_the_data_is_a_web_address_or_nothing() {
        let base = Url::parse("https://site.test/a/").unwrap();
        let at = |u: &str| link(&serde_json::json!({ "url": u }), Some(&base));
        assert_eq!(at("/p/1").as_deref(), Some("https://site.test/p/1"));
        assert_eq!(at("javascript:alert(1)"), None);
        assert_eq!(at("at://did:example/post/1"), None);
    }
}
