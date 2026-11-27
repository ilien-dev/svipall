//! The other forms of itself a page declares, and how one of them is read in the page's place.
//!
//! A page that renders nothing without script often says, in its head, where the same thing can
//! be had in a form that needs none: `<link rel="alternate">` with a type. Three types carry
//! content and are read here, none tied to a site:
//!
//! - a **feed** (`application/rss+xml`, `application/atom+xml`): a profile's or a channel's posts;
//! - an **activity** (`application/activity+json`): one post of any server that speaks
//!   ActivityPub, with its author, its time and its counts;
//! - an **embed** (`application/json+oembed`): the post as the site hands it to another page.
//!
//! This is the page's own word about itself, asked for with one more plain request. It is used
//! only for a page the first tier could not read, and only an address on the page's own site is
//! ever asked: a page cannot point this at somebody else's server.
//!
//! What comes back is turned into a small document and read by the ordinary extractor, so
//! hidden text, markup and links get the treatment any page gets. Two cautions are built in. A
//! feed counts only when it says it is the feed *of this page* (its channel link is the page's
//! address): a script-only article that links its site's feed is not that feed. And an embed
//! counts only when it holds words of its own, because many are a frame or a "view on" link.

use quick_xml::events::Event;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use url::Url;

/// The head is where these live. A megabyte of application script after it is not searched.
const HEAD_LIMIT: usize = 400_000;
/// Entries read from one feed.
const MAX_ITEMS: usize = 40;
/// An alternate larger than this is not a page's worth of anything.
const MAX_BODY: usize = 4_000_000;
/// Said under a post read from its activity or its embed: neither carries what was answered
/// to it, and a reader who wanted the thread should know to ask for the rendered page.
const ONLY_THE_POST: &str = "\n<p>Read from the form of this post its page declares, which \
    holds the post and not the replies to it. scroll=auto reads the rendered page.</p>";
/// Words an embed must hold outside its links to be a post and not a pointer to one.
const LEAST_EMBED_TEXT: usize = 12;

static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<link\b([^>]*)>").unwrap());
static ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\b([a-z-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());
static TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title\b[^>]*>(.*?)</title>").unwrap());
static ANCHOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<a\b.*?</a>").unwrap());
static ACTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<(script|style|iframe|object|embed)\b.*?</(script|style|iframe|object|embed)>|<(script|iframe|embed|object)\b[^>]*/?>")
        .unwrap()
});
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]*>").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Feed,
    Activity,
    Embed,
}

impl Kind {
    fn of(media_type: &str) -> Option<Self> {
        let t = media_type.trim().to_ascii_lowercase();
        let t = t.split(';').next().unwrap_or("").trim();
        match t {
            "application/rss+xml" | "application/atom+xml" => Some(Self::Feed),
            "application/activity+json" => Some(Self::Activity),
            "application/json+oembed" => Some(Self::Embed),
            _ => None,
        }
    }

    /// What to ask the address for.
    pub fn accept(self) -> &'static str {
        match self {
            Self::Feed => "application/atom+xml, application/rss+xml, application/xml;q=0.9",
            Self::Activity => "application/activity+json, application/ld+json;q=0.9",
            Self::Embed => "application/json",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Feed => "feed",
            Self::Activity => "activity",
            Self::Embed => "embed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alternate {
    pub kind: Kind,
    pub url: String,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    ATTR.captures_iter(tag).find_map(|c| {
        c.get(1)?
            .as_str()
            .eq_ignore_ascii_case(name)
            .then(|| c.get(2).or(c.get(3)).map(|m| unescape(m.as_str())))
            .flatten()
    })
}

fn unescape(v: &str) -> String {
    v.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn escape(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn head(html: &str) -> &str {
    let mut end = html.len().min(HEAD_LIMIT);
    while !html.is_char_boundary(end) {
        end -= 1;
    }
    &html[..end]
}

/// The page's title as written, for the document read in its place.
pub fn page_title(html: &str) -> String {
    TITLE
        .captures(head(html))
        .and_then(|c| c.get(1))
        .map(|m| unescape(m.as_str().trim()))
        .unwrap_or_default()
}

/// The alternates a page declares that carry content, one of each kind, in the order worth
/// asking: a feed, an activity, an embed. Only web addresses on the page's own site.
pub fn declared(html: &str, page_url: &str) -> Vec<Alternate> {
    let Ok(page) = Url::parse(page_url) else {
        return Vec::new();
    };
    let Some(site) = page.host_str().map(str::to_ascii_lowercase) else {
        return Vec::new();
    };
    let mut found: Vec<Alternate> = Vec::new();
    for tag in LINK.captures_iter(head(html)) {
        let tag = &tag[1];
        let alternate = attr(tag, "rel").is_some_and(|r| {
            r.split_ascii_whitespace()
                .any(|w| w.eq_ignore_ascii_case("alternate"))
        });
        let Some(kind) = attr(tag, "type").as_deref().and_then(Kind::of) else {
            continue;
        };
        if !alternate || found.iter().any(|a| a.kind == kind) {
            continue;
        }
        let Some(url) = attr(tag, "href").and_then(|h| page.join(h.trim()).ok()) else {
            continue;
        };
        let same_site = url
            .host_str()
            .is_some_and(|h| crate::domain::same_site(&h.to_ascii_lowercase(), &site));
        if !matches!(url.scheme(), "http" | "https") || !same_site {
            continue;
        }
        found.push(Alternate {
            kind,
            url: url.into(),
        });
    }
    found.sort_by_key(|a| a.kind);
    found
}

/// `2016-03-16T14:44:31Z` as `2016-03-16 14:44 UTC`; anything else as it was written.
fn when(ts: &str) -> String {
    let utc = ts.ends_with('Z') || ts.ends_with("+00:00") || ts.ends_with("+0000");
    match (ts.get(..10), ts.get(10..11), ts.get(11..16)) {
        (Some(day), Some("T"), Some(clock)) if utc => format!("{day} {clock} UTC"),
        _ => ts.to_string(),
    }
}

fn counted(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn wrap(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head>\n\
         <body><main>\n{body}\n</main></body></html>\n",
        escape(title)
    )
}

/// Markup that does something rather than say something, removed before it is put in a document.
fn inert(html: &str) -> String {
    ACTIVE.replace_all(html, "").into_owned()
}

fn plain(html: &str) -> String {
    unescape(&TAG.replace_all(html, " "))
        .replace("&mdash;", " ")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Who an activity is attributed to: the name it gives, or the last word of the address.
fn actor(v: &Value) -> Option<String> {
    let who = match v.get("attributedTo")? {
        Value::Array(a) => a.first()?,
        other => other,
    };
    match who {
        Value::String(s) => {
            let last = s.trim_end_matches('/').rsplit('/').next()?.trim();
            (!last.is_empty()).then(|| {
                if last.starts_with('@') {
                    last.to_string()
                } else {
                    format!("@{last}")
                }
            })
        }
        Value::Object(_) => ["name", "preferredUsername"]
            .iter()
            .find_map(|k| who.get(*k).and_then(Value::as_str))
            .map(str::to_string),
        _ => None,
    }
}

fn total(v: &Value, key: &str) -> Option<u64> {
    v.get(key)?.get("totalItems")?.as_u64()
}

fn activity(body: &str, title: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let kind = v.get("type")?.as_str()?;
    if !matches!(
        kind,
        "Note" | "Article" | "Page" | "Question" | "Video" | "Event" | "Document" | "Audio"
    ) {
        return None;
    }
    let content = v
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| {
            v.get("contentMap")?
                .as_object()?
                .values()
                .find_map(Value::as_str)
        })
        .map(inert)
        .unwrap_or_default();
    let name = v.get("name").and_then(Value::as_str).unwrap_or("").trim();
    if plain(&content).is_empty() && name.is_empty() {
        return None;
    }
    let mut meta: Vec<String> = actor(&v).into_iter().collect();
    if let Some(at) = v.get("published").and_then(Value::as_str) {
        meta.push(when(at));
    }
    for (key, one, many) in [
        ("likes", "like", "likes"),
        ("shares", "share", "shares"),
        ("replies", "reply", "replies"),
    ] {
        if let Some(n) = total(&v, key) {
            meta.push(counted(n, one, many));
        }
    }
    let mut out = String::from("<article>\n");
    if !name.is_empty() {
        out.push_str(&format!("<h1>{}</h1>\n", escape(name)));
    }
    if !meta.is_empty() {
        out.push_str(&format!(
            "<p><strong>{}</strong></p>\n",
            escape(&meta.join(" · "))
        ));
    }
    out.push_str(&content);
    // A poll's choices, and what is attached, by name.
    for key in ["oneOf", "anyOf"] {
        let options: Vec<&str> = v
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|o| o.get("name")?.as_str())
            .collect();
        if !options.is_empty() {
            out.push_str("\n<ul>");
            for o in options {
                out.push_str(&format!("<li>{}</li>", escape(o)));
            }
            out.push_str("</ul>");
        }
    }
    for a in v
        .get("attachment")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(url), described) = (
            a.get("url").and_then(Value::as_str),
            a.get("name").and_then(Value::as_str).unwrap_or(""),
        ) else {
            continue;
        };
        if Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) {
            let label = if described.is_empty() { url } else { described };
            out.push_str(&format!(
                "\n<p>Attachment: <a href=\"{}\">{}</a></p>",
                escape(url),
                escape(label)
            ));
        }
    }
    out.push_str("\n</article>");
    out.push_str(ONLY_THE_POST);
    Some(wrap(title, &out))
}

fn embed(body: &str, title: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let html = inert(v.get("html")?.as_str()?);
    // What it says outside its links: a frame says nothing, and "view on" is a link.
    let own = plain(&ANCHOR.replace_all(&html, " "));
    if own.chars().count() < LEAST_EMBED_TEXT {
        return None;
    }
    let author = v.get("author_name").and_then(Value::as_str).unwrap_or("");
    let mut out = String::from("<article>\n");
    if !author.is_empty() {
        out.push_str(&format!("<p><strong>{}</strong></p>\n", escape(author)));
    }
    out.push_str(&html);
    out.push_str("\n</article>");
    out.push_str(ONLY_THE_POST);
    Some(wrap(title, &out))
}

#[derive(Default)]
struct Entry {
    title: String,
    link: String,
    date: String,
    body: String,
}

/// The same address, whatever its scheme, its case or a slash at its end.
fn same_page(a: &str, b: &str) -> bool {
    let key = |u: &str| {
        Url::parse(u.trim()).ok().map(|u| {
            format!(
                "{}{}",
                u.host_str().unwrap_or("").to_ascii_lowercase(),
                u.path().trim_end_matches('/')
            )
        })
    };
    matches!((key(a), key(b)), (Some(x), Some(y)) if x == y)
}

fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|b| *b == b':').next().unwrap_or(name)
}

fn feed(body: &str, title: &str, page_url: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(body);
    let mut entries: Vec<Entry> = Vec::new();
    let mut current: Option<Entry> = None;
    let (mut channel_title, mut channel_about, mut channel_link) =
        (String::new(), String::new(), String::new());
    let mut field: Option<Vec<u8>> = None;
    let mut depth_in_entry = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local(e.name().as_ref()).to_vec();
                if matches!(name.as_slice(), b"item" | b"entry") {
                    current = Some(Entry::default());
                    depth_in_entry = 0;
                    continue;
                }
                if current.is_some() {
                    depth_in_entry += 1;
                }
                // Atom keeps a link in an attribute.
                if name == b"link" {
                    let href = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"href")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned());
                    let rel = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"rel")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned());
                    if let (Some(h), true) = (href, rel.as_deref().is_none_or(|r| r == "alternate"))
                    {
                        match current.as_mut() {
                            Some(c) if c.link.is_empty() => c.link = h,
                            None if channel_link.is_empty() => channel_link = h,
                            _ => {}
                        }
                    }
                }
                field = Some(name);
            }
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"link" {
                    let href = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"href")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned());
                    let rel = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"rel")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned());
                    if let (Some(h), true) = (href, rel.as_deref().is_none_or(|r| r == "alternate"))
                    {
                        match current.as_mut() {
                            Some(c) if c.link.is_empty() => c.link = h,
                            None if channel_link.is_empty() => channel_link = h,
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Text(t)) => {
                let value = t.unescape().map(|v| v.into_owned()).unwrap_or_default();
                put(
                    field.as_deref(),
                    value.trim(),
                    current.as_mut(),
                    depth_in_entry,
                    (&mut channel_title, &mut channel_about, &mut channel_link),
                );
            }
            Ok(Event::CData(t)) => {
                let value = String::from_utf8_lossy(&t.into_inner()).into_owned();
                put(
                    field.as_deref(),
                    value.trim(),
                    current.as_mut(),
                    depth_in_entry,
                    (&mut channel_title, &mut channel_about, &mut channel_link),
                );
            }
            Ok(Event::End(e)) => {
                let name = local(e.name().as_ref()).to_vec();
                if matches!(name.as_slice(), b"item" | b"entry") {
                    if let Some(done) = current.take() {
                        if entries.len() < MAX_ITEMS {
                            entries.push(done);
                        }
                    }
                } else if current.is_some() {
                    depth_in_entry = depth_in_entry.saturating_sub(1);
                }
                field = None;
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    // The feed of this page, or it is somebody's feed and not this page's content.
    if entries.is_empty() || !same_page(&channel_link, page_url) {
        return None;
    }
    let mut out = String::new();
    let heading = if channel_title.is_empty() {
        title
    } else {
        &channel_title
    };
    out.push_str(&format!("<h1>{}</h1>\n", escape(heading)));
    if !channel_about.is_empty() {
        out.push_str(&format!("<p>{}</p>\n", escape(&plain(&channel_about))));
    }
    for e in entries {
        let said = inert(&e.body);
        if plain(&said).is_empty() && e.title.is_empty() {
            continue;
        }
        out.push_str("<article>\n");
        if !e.title.is_empty() {
            out.push_str(&format!("<h2>{}</h2>\n", escape(&e.title)));
        }
        let link = Url::parse(e.link.trim())
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        let stamp = if e.date.is_empty() { "Post" } else { &e.date };
        match link {
            Some(u) => out.push_str(&format!(
                "<p><strong><a href=\"{}\">{}</a></strong></p>\n",
                escape(u.as_str()),
                escape(&when(stamp))
            )),
            None => out.push_str(&format!(
                "<p><strong>{}</strong></p>\n",
                escape(&when(stamp))
            )),
        }
        out.push_str(&said);
        out.push_str("\n</article>\n");
    }
    Some(wrap(title, &out))
}

fn put(
    field: Option<&[u8]>,
    value: &str,
    entry: Option<&mut Entry>,
    depth: usize,
    channel: (&mut String, &mut String, &mut String),
) {
    let (Some(field), false) = (field, value.is_empty()) else {
        return;
    };
    match entry {
        // Only the entry's own fields: an author's name inside it is not its title.
        Some(e) if depth == 1 => match field {
            b"title" => e.title = value.to_string(),
            b"link" if e.link.is_empty() => e.link = value.to_string(),
            b"pubDate" | b"published" | b"date" => e.date = value.to_string(),
            b"updated" if e.date.is_empty() => e.date = value.to_string(),
            // The fuller of what the entry says wins.
            b"description" | b"summary" | b"content" | b"encoded" if value.len() > e.body.len() => {
                e.body = value.to_string()
            }
            _ => {}
        },
        Some(_) => {}
        None => match field {
            b"title" if channel.0.is_empty() => *channel.0 = value.to_string(),
            b"description" | b"subtitle" if channel.1.is_empty() => *channel.1 = value.to_string(),
            b"link" if channel.2.is_empty() => *channel.2 = value.to_string(),
            _ => {}
        },
    }
}

/// The document to read in the page's place, from what one alternate answered. `None` when the
/// answer is not content: a profile's activity with nothing said, somebody else's feed, an embed
/// that is a frame.
pub fn document(kind: Kind, body: &str, page_title: &str, page_url: &str) -> Option<String> {
    if body.len() > MAX_BODY {
        return None;
    }
    match kind {
        Kind::Feed => feed(body, page_title, page_url),
        Kind::Activity => activity(body, page_title),
        Kind::Embed => embed(body, page_title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "https://social.test/@aurora/1";

    #[test]
    fn a_page_names_its_alternates_and_only_its_own() {
        let html = r#"<html><head><title>Aurora &amp; friends</title>
            <link rel="stylesheet" href="/a.css">
            <link href="https://social.test/api/oembed?format=json&amp;url=https%3A%2F%2Fsocial.test%2F%40aurora%2F1" rel="alternate" type="application/json+oembed">
            <link href="/users/aurora/statuses/1" rel="alternate" type="application/activity+json">
            <link href="https://social.test/users/aurora/statuses/1" rel="alternate" type="application/ld+json; profile=&quot;https://www.w3.org/ns/activitystreams&quot;">
            <link rel="alternate" type="application/rss+xml" href="https://elsewhere.test/feed.rss">
            <link rel="alternate" type="application/rss+xml" href="javascript:alert(1)">
            <link rel="alternate" hreflang="es" href="https://social.test/es/@aurora/1">
            <link rel="alternate" type="application/activity+json" href="/second">
            </head><body></body></html>"#;
        let found = declared(html, PAGE);
        assert_eq!(
            found,
            vec![
                Alternate {
                    kind: Kind::Activity,
                    url: "https://social.test/users/aurora/statuses/1".into()
                },
                Alternate {
                    kind: Kind::Embed,
                    url: "https://social.test/api/oembed?format=json&url=https%3A%2F%2Fsocial.test%2F%40aurora%2F1".into()
                },
            ]
        );
        assert_eq!(page_title(html), "Aurora & friends");
        // A sibling host of the same site is the site: an embed service usually is one.
        let sub = r#"<link rel="alternate" type="application/json+oembed" href="https://publish.social.test/oembed?url=x">"#;
        assert_eq!(declared(sub, PAGE).len(), 1);
        assert!(declared(html, "not a url").is_empty());
    }

    #[test]
    fn an_activity_is_a_post_with_who_when_and_how_many() {
        let body = r#"{"@context":"https://www.w3.org/ns/activitystreams","id":"https://social.test/users/aurora/statuses/1","type":"Note","published":"2016-03-16T14:44:31Z","attributedTo":"https://social.test/users/aurora","content":"<p>Hello <a href=\"https://social.test/tags/world\">#world</a></p><script>track()</script>","likes":{"type":"Collection","totalItems":1152},"shares":{"type":"Collection","totalItems":1},"attachment":[{"type":"Document","url":"https://files.social.test/a.png","name":"A dog in the snow"},{"type":"Document","url":"javascript:x"}]}"#;
        let doc = document(Kind::Activity, body, "Aurora", PAGE).unwrap();
        assert!(doc.contains("<title>Aurora</title>"), "{doc}");
        assert!(
            doc.contains("<strong>@aurora · 2016-03-16 14:44 UTC · 1152 likes · 1 share</strong>"),
            "{doc}"
        );
        assert!(
            doc.contains("<p>Hello <a href=\"https://social.test/tags/world\">#world</a></p>"),
            "{doc}"
        );
        assert!(!doc.contains("track()"), "{doc}");
        assert!(
            doc.contains("<a href=\"https://files.social.test/a.png\">A dog in the snow</a>"),
            "{doc}"
        );
        assert!(!doc.contains("javascript:"), "{doc}");
    }

    #[test]
    fn an_activity_that_says_nothing_is_not_a_reading() {
        // A person, which is what a profile's activity is: who they are, and none of their posts.
        let person = r#"{"type":"Person","preferredUsername":"aurora","summary":"<p>Notes.</p>"}"#;
        assert_eq!(document(Kind::Activity, person, "t", PAGE), None);
        let empty = r#"{"type":"Note","content":"<p> </p>"}"#;
        assert_eq!(document(Kind::Activity, empty, "t", PAGE), None);
        assert_eq!(document(Kind::Activity, "<html>", "t", PAGE), None);
    }

    #[test]
    fn an_embed_counts_when_it_holds_words_of_its_own() {
        let post = r#"{"author_name":"aurora","html":"<blockquote><p lang=\"en\">just setting up the station</p>&mdash; aurora (@aurora) <a href=\"https://social.test/@aurora/1\">March 21, 2006</a></blockquote>\n<script async src=\"https://social.test/widgets.js\"></script>"}"#;
        let doc = document(Kind::Embed, post, "Aurora", PAGE).unwrap();
        assert!(doc.contains("<strong>aurora</strong>"), "{doc}");
        assert!(doc.contains("just setting up the station"), "{doc}");
        assert!(!doc.contains("widgets.js"), "{doc}");
        // A frame, and a link that says where the post is.
        let frame =
            r#"{"author_name":"a","html":"<iframe src=\"https://social.test/embed/1\"></iframe>"}"#;
        assert_eq!(document(Kind::Embed, frame, "t", PAGE), None);
        let pointer = r#"{"author_name":"a","html":"<blockquote><a href=\"https://social.test/@aurora/1\"><div>Post by @aurora@social.test</div><div>View on Social</div></a></blockquote>"}"#;
        assert_eq!(document(Kind::Embed, pointer, "t", PAGE), None);
    }

    #[test]
    fn a_feed_is_read_when_it_is_the_feed_of_this_page() {
        let feed = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
  <channel>
    <title>Aurora Station</title>
    <description>Public posts from @aurora@social.test</description>
    <link>https://social.test/@aurora</link>
    <image><url>https://files.social.test/a.png</url><title>Not the channel</title><link>https://social.test/@aurora</link></image>
    <item>
      <guid isPermaLink="true">https://social.test/@aurora/3</guid>
      <link>https://social.test/@aurora/3</link>
      <pubDate>Thu, 26 Nov 2026 21:09:00 +0000</pubDate>
      <description>&lt;p&gt;The winter crew arrived today.&lt;/p&gt;</description>
      <media:content url="https://files.social.test/dog.png" type="image/png"><media:description type="plain">Not the post</media:description></media:content>
    </item>
    <item>
      <link>https://social.test/@aurora/2</link>
      <pubDate>Wed, 25 Nov 2026 17:22:31 +0000</pubDate>
      <description><![CDATA[<p>First light over the ridge.</p>]]></description>
    </item>
  </channel>
</rss>"#;
        let doc = document(
            Kind::Feed,
            feed,
            "Aurora (@aurora)",
            "https://social.test/@aurora/",
        )
        .unwrap();
        assert!(doc.contains("<h1>Aurora Station</h1>"), "{doc}");
        assert!(
            doc.contains("<p>Public posts from @aurora@social.test</p>"),
            "{doc}"
        );
        assert!(
            doc.contains(
                "<a href=\"https://social.test/@aurora/3\">Thu, 26 Nov 2026 21:09:00 +0000</a>"
            ),
            "{doc}"
        );
        assert!(
            doc.contains("<p>The winter crew arrived today.</p>"),
            "{doc}"
        );
        assert!(doc.contains("<p>First light over the ridge.</p>"), "{doc}");
        assert!(
            !doc.contains("Not the post") && !doc.contains("Not the channel"),
            "{doc}"
        );
        // The same feed linked from an article of the site is the site's feed, not the article.
        assert_eq!(
            document(Kind::Feed, feed, "t", "https://social.test/blog/an-article"),
            None
        );
    }

    #[test]
    fn an_atom_feed_reads_the_same() {
        let feed = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Station log</title>
<link rel="self" href="https://log.test/feed.atom"/><link href="https://log.test/"/>
<entry><title>Day one</title><link href="https://log.test/day-one"/><published>2026-11-01T08:00:00Z</published>
<author><name>Not a title</name></author>
<content type="html">&lt;p&gt;We landed.&lt;/p&gt;</content></entry></feed>"#;
        let doc = document(Kind::Feed, feed, "t", "https://log.test").unwrap();
        assert!(doc.contains("<h2>Day one</h2>"), "{doc}");
        assert!(
            doc.contains("<a href=\"https://log.test/day-one\">2026-11-01 08:00 UTC</a>"),
            "{doc}"
        );
        assert!(doc.contains("<p>We landed.</p>"), "{doc}");
        assert!(!doc.contains("Not a title"), "{doc}");
        assert_eq!(
            document(Kind::Feed, "<rss><channel>", "t", "https://log.test"),
            None
        );
    }
}
