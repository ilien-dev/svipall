//! Page metadata and classified links.
//!
//! `web_fetch` used to return markdown, a title and nothing else. Everything a model routinely
//! wants next — is this the canonical URL, when was it published, what language is it, which links
//! leave the site — had to be recovered by re-reading the prose, or was simply unavailable.

use scraper::{Html, Selector};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Cap on embedded JSON-LD. Some sites ship hundreds of kilobytes of product graph, and forwarding
/// that verbatim would cost more context than the page itself.
const JSON_LD_BUDGET: usize = 256 * 1024;
const JSON_LD_MAX_BLOBS: usize = 20;

#[derive(Debug, Default, Serialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robots: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// RSS/Atom autodiscovery links, which also seed feed-based crawling.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub feeds: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub open_graph: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub twitter: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub json_ld: Vec<Value>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub json_ld_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub nofollow: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Media {
    pub src: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alt: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Links {
    pub internal: Vec<Link>,
    pub external: Vec<Link>,
    pub media: Vec<Media>,
}

fn sel(s: &str) -> Option<Selector> {
    Selector::parse(s).ok()
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pull `@graph` and array wrappers apart so consumers see the objects, not the envelope.
fn flatten_json_ld(v: Value, out: &mut Vec<Value>) {
    match v {
        Value::Array(items) => {
            for i in items {
                flatten_json_ld(i, out);
            }
        }
        Value::Object(mut map) => {
            if let Some(graph) = map.remove("@graph") {
                flatten_json_ld(graph, out);
                // What remains is usually just `@context`, which describes the envelope rather
                // than carrying data. Emitting it would add a content-free object to every result.
                map.retain(|k, _| !k.starts_with('@') || k == "@type");
                if !map.is_empty() {
                    out.push(Value::Object(map));
                }
            } else {
                out.push(Value::Object(map));
            }
        }
        _ => {}
    }
}

/// First non-empty value among the JSON-LD keys given, searched across every blob.
fn from_json_ld(blobs: &[Value], keys: &[&str]) -> Option<String> {
    blobs.iter().find_map(|b| {
        keys.iter().find_map(|k| match b.get(*k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(clean(s)),
            // `author` is usually an object or a list of them.
            Some(Value::Object(o)) => o.get("name")?.as_str().map(clean),
            Some(Value::Array(a)) => a.first().and_then(|f| match f {
                Value::String(s) => Some(clean(s)),
                Value::Object(o) => o.get("name")?.as_str().map(clean),
                _ => None,
            }),
            _ => None,
        })
    })
}

pub(crate) fn metadata_from(doc: &Html, base_url: Option<&str>) -> Metadata {
    let mut m = Metadata::default();
    let base = base_url.and_then(|u| url::Url::parse(u).ok());
    let abs = |v: &str| -> String {
        match &base {
            Some(b) => b
                .join(v.trim())
                .map(|u| u.to_string())
                .unwrap_or_else(|_| v.trim().into()),
            None => v.trim().to_string(),
        }
    };

    // JSON-LD first: it is the most structured and the least likely to be marketing copy.
    if let Some(s) = sel(r#"script[type="application/ld+json"]"#) {
        let mut budget = JSON_LD_BUDGET;
        for node in doc.select(&s) {
            if m.json_ld.len() >= JSON_LD_MAX_BLOBS {
                m.json_ld_truncated = true;
                break;
            }
            let raw = node.text().collect::<String>();
            if raw.len() > budget {
                m.json_ld_truncated = true;
                break;
            }
            budget -= raw.len();
            // Malformed JSON-LD is common; skipping it is right, failing the fetch is not.
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                flatten_json_ld(v, &mut m.json_ld);
            }
        }
    }

    if let Some(s) = sel("meta") {
        for node in doc.select(&s) {
            let el = node.value();
            let content = el.attr("content").map(clean).filter(|c| !c.is_empty());
            let Some(content) = content else { continue };
            if let Some(prop) = el.attr("property") {
                if let Some(k) = prop.strip_prefix("og:") {
                    m.open_graph.insert(k.to_string(), content.clone());
                }
                match prop {
                    "article:published_time" => m.published.get_or_insert(content.clone()),
                    "article:modified_time" => m.modified.get_or_insert(content.clone()),
                    "article:author" => m.author.get_or_insert(content.clone()),
                    _ => &mut String::new(),
                };
            }
            if let Some(name) = el.attr("name") {
                let lower = name.to_ascii_lowercase();
                if let Some(k) = lower.strip_prefix("twitter:") {
                    m.twitter.insert(k.to_string(), content.clone());
                }
                match lower.as_str() {
                    "description" => {
                        m.description.get_or_insert(content.clone());
                    }
                    "author" => {
                        m.author.get_or_insert(content.clone());
                    }
                    "robots" => m.robots = Some(content.clone()),
                    "keywords" => {
                        m.keywords = content
                            .split(',')
                            .map(|k| k.trim().to_string())
                            .filter(|k| !k.is_empty())
                            .collect()
                    }
                    _ => {}
                }
            }
        }
    }

    // OpenGraph beats bare <meta>, JSON-LD beats both.
    m.title = from_json_ld(&m.json_ld, &["headline", "name"])
        .or_else(|| m.open_graph.get("title").cloned())
        .or_else(|| {
            sel("title")
                .and_then(|s| doc.select(&s).next())
                .map(|t| clean(&t.text().collect::<String>()))
                .filter(|t| !t.is_empty())
        })
        .or_else(|| {
            sel("h1")
                .and_then(|s| doc.select(&s).next())
                .map(|t| clean(&t.text().collect::<String>()))
                .filter(|t| !t.is_empty())
        });
    m.description = from_json_ld(&m.json_ld, &["description"])
        .or_else(|| m.open_graph.get("description").cloned())
        .or(m.description);
    m.author = from_json_ld(&m.json_ld, &["author", "creator"]).or(m.author);
    m.published = from_json_ld(&m.json_ld, &["datePublished", "dateCreated"]).or(m.published);
    m.modified = from_json_ld(&m.json_ld, &["dateModified"]).or(m.modified);
    m.site_name = m.open_graph.get("site_name").cloned();
    m.image = m.open_graph.get("image").map(|i| abs(i));

    m.canonical = sel(r#"link[rel="canonical"]"#)
        .and_then(|s| doc.select(&s).next())
        .and_then(|l| l.value().attr("href"))
        .map(&abs)
        .or_else(|| m.open_graph.get("url").map(|u| abs(u)));

    m.lang = sel("html")
        .and_then(|s| doc.select(&s).next())
        .and_then(|h| h.value().attr("lang"))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .or_else(|| m.open_graph.get("locale").cloned());

    if let Some(s) = sel(r#"link[rel="alternate"]"#) {
        for node in doc.select(&s) {
            let ty = node.value().attr("type").unwrap_or_default();
            if ty.contains("rss") || ty.contains("atom") {
                if let Some(h) = node.value().attr("href") {
                    m.feeds.push(abs(h));
                }
            }
        }
    }

    if let Some(t) = &m.published {
        m.published = Some(normalise_date(t));
    }
    if let Some(t) = &m.modified {
        m.modified = Some(normalise_date(t));
    }
    m
}

/// Leave ISO-8601 alone; otherwise hand the original back rather than guess wrong.
fn normalise_date(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 10 && t.as_bytes()[4] == b'-' && t.as_bytes()[7] == b'-' {
        return t.to_string();
    }
    t.to_string()
}

pub(crate) fn links_detailed(doc: &Html, base_url: &str) -> Links {
    let base = url::Url::parse(base_url).ok();
    let host = base
        .as_ref()
        .and_then(|b| b.host_str())
        .map(|h| h.trim_start_matches("www.").to_ascii_lowercase())
        .unwrap_or_default();
    let mut out = Links::default();
    let mut seen = std::collections::HashSet::new();

    if let Some(s) = sel("a[href]") {
        for a in doc.select(&s) {
            let Some(href) = a.value().attr("href") else {
                continue;
            };
            let Some(mut u) = super::resolve(&base, href) else {
                continue;
            };
            u.set_fragment(None);
            let url_s = u.to_string();
            if !seen.insert(url_s.clone()) {
                continue;
            }
            let link = Link {
                text: clean(&a.text().collect::<String>()),
                nofollow: a
                    .value()
                    .attr("rel")
                    .map(|r| r.to_ascii_lowercase().contains("nofollow"))
                    .unwrap_or(false),
                url: url_s,
            };
            let same = u
                .host_str()
                .map(|h| {
                    let h = h.trim_start_matches("www.").to_ascii_lowercase();
                    h == host
                        || h.ends_with(&format!(".{host}"))
                        || host.ends_with(&format!(".{h}"))
                })
                .unwrap_or(false);
            if same {
                out.internal.push(link);
            } else {
                out.external.push(link);
            }
        }
    }
    if let Some(s) = sel("img[src]") {
        for img in doc.select(&s) {
            let Some(src) = img.value().attr("src").or(img.value().attr("data-src")) else {
                continue;
            };
            let Some(u) = super::resolve(&base, src) else {
                continue;
            };
            out.media.push(Media {
                src: u.to_string(),
                alt: img.value().attr("alt").map(clean).unwrap_or_default(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::extraction::{parse_page, ParseWants};

    const PAGE: &str = r#"<!doctype html><html lang="en-GB">
      <head>
        <title>Fallback Title</title>
        <link rel="canonical" href="/canonical-path">
        <link rel="alternate" type="application/rss+xml" href="/feed.xml">
        <meta name="description" content="A meta description.">
        <meta name="robots" content="index,follow">
        <meta name="keywords" content="rust, scraping , ">
        <meta property="og:title" content="OpenGraph Title">
        <meta property="og:site_name" content="Example Site">
        <meta property="og:image" content="/img/hero.png">
        <meta name="twitter:card" content="summary">
        <meta property="article:published_time" content="2026-01-15T10:00:00Z">
        <script type="application/ld+json">
          {"@context":"https://schema.org","@graph":[
            {"@type":"Article","headline":"JSON-LD Headline",
             "author":{"@type":"Person","name":"Ada Lovelace"},
             "datePublished":"2026-02-01T08:30:00Z"}]}
        </script>
        <script type="application/ld+json">{ this is not json }</script>
      </head>
      <body>
        <a href="/about">About us</a>
        <a href="https://external.test/x" rel="nofollow">External</a>
        <a href="https://sub.example.com/y">Subdomain</a>
        <img src="/img/a.png" alt="An image">
      </body></html>"#;

    fn meta() -> crate::extraction::meta::Metadata {
        let wants = ParseWants {
            metadata: true,
            metadata_base_url: Some("https://example.com/page".into()),
            ..Default::default()
        };
        parse_page(PAGE, &wants).metadata.expect("metadata")
    }

    #[test]
    fn json_ld_wins_over_opengraph_which_wins_over_the_title_tag() {
        assert_eq!(meta().title.as_deref(), Some("JSON-LD Headline"));
    }

    #[test]
    fn a_graph_wrapper_is_flattened_and_broken_json_is_skipped() {
        let m = meta();
        assert_eq!(
            m.json_ld.len(),
            1,
            "the @graph envelope should be unwrapped"
        );
        assert_eq!(m.json_ld[0]["@type"], "Article");
    }

    #[test]
    fn an_author_object_yields_its_name() {
        assert_eq!(meta().author.as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn dates_prefer_json_ld_over_the_meta_tag() {
        assert_eq!(meta().published.as_deref(), Some("2026-02-01T08:30:00Z"));
    }

    #[test]
    fn canonical_and_image_are_absolute() {
        let m = meta();
        assert_eq!(
            m.canonical.as_deref(),
            Some("https://example.com/canonical-path")
        );
        assert_eq!(m.image.as_deref(), Some("https://example.com/img/hero.png"));
    }

    #[test]
    fn language_feeds_keywords_and_robots_are_captured() {
        let m = meta();
        assert_eq!(m.lang.as_deref(), Some("en-GB"));
        assert_eq!(m.feeds, vec!["https://example.com/feed.xml"]);
        assert_eq!(m.keywords, vec!["rust", "scraping"]);
        assert_eq!(m.robots.as_deref(), Some("index,follow"));
        assert_eq!(
            m.open_graph.get("site_name").map(String::as_str),
            Some("Example Site")
        );
        assert_eq!(m.twitter.get("card").map(String::as_str), Some("summary"));
    }

    #[test]
    fn links_are_split_internal_versus_external_with_subdomains_counted_as_internal() {
        let wants = ParseWants {
            links_detailed: Some("https://example.com/page".into()),
            ..Default::default()
        };
        let l = parse_page(PAGE, &wants).links_detailed.expect("links");
        assert!(l.internal.iter().any(|x| x.url.ends_with("/about")));
        assert!(
            l.internal.iter().any(|x| x.url.contains("sub.example.com")),
            "a subdomain is the same site"
        );
        assert_eq!(l.external.len(), 1);
        assert!(l.external[0].nofollow, "rel=nofollow should be reported");
        assert_eq!(l.media.len(), 1);
        assert_eq!(l.media[0].alt, "An image");
    }

    #[test]
    fn an_empty_document_produces_no_spurious_fields() {
        let wants = ParseWants {
            metadata: true,
            ..Default::default()
        };
        let m = parse_page("<html></html>", &wants).metadata.unwrap();
        let json = serde_json::to_value(&m).unwrap();
        assert!(
            json.as_object().unwrap().is_empty(),
            "empty metadata must serialise to nothing: {json}"
        );
    }
}
