//! Sitemaps and feeds: knowing a site's URLs without crawling it.
//!
//! `web_crawl` discovered pages only by following links from the start URL, which misses anything
//! not linked from where you happen to begin, and pays a full page fetch for every step. A sitemap
//! answers the same question in one request.
//!
//! Parsing is streamed rather than tree-based: a 50,000-URL sitemap should not be materialised as a
//! DOM to read a list of strings out of it.

use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;

/// Guards against a decompression bomb, and against a mis-served binary eating all the memory.
const MAX_DECOMPRESSED: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_URLS: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapEntry {
    pub url: String,
    pub lastmod: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sitemap {
    /// A sitemap index: these are more sitemaps, not pages.
    Index(Vec<String>),
    Urls(Vec<SitemapEntry>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItem {
    pub url: String,
    pub title: String,
    pub published: Option<String>,
}

/// Decompress when the bytes actually are gzip.
///
/// Sniffing the magic number rather than trusting the extension: `.xml` served gzipped and `.gz`
/// served plain are both common enough to break an extension-based guess.
pub fn maybe_gunzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    if bytes.len() < 2 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Ok(bytes.to_vec());
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_DECOMPRESSED as u64)
        .read_to_end(&mut out)?;
    Ok(out)
}

fn reader(xml: &str) -> Reader<&[u8]> {
    let mut r = Reader::from_str(xml);
    let cfg = r.config_mut();
    cfg.trim_text(true);
    // Sitemaps in the wild are frequently not well-formed; recovering beats refusing the file.
    cfg.check_end_names = false;
    r
}

/// Local name of a tag, ignoring any namespace prefix.
fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

pub fn parse(bytes: &[u8], limits_max_urls: usize) -> anyhow::Result<Sitemap> {
    let bytes = maybe_gunzip(bytes)?;
    let text = String::from_utf8_lossy(&bytes);

    // A plain-text sitemap is one URL per line, and is explicitly allowed by the protocol.
    if !text.trim_start().starts_with('<') {
        let urls: Vec<SitemapEntry> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
            .take(limits_max_urls)
            .map(|l| SitemapEntry {
                url: l.to_string(),
                lastmod: None,
            })
            .collect();
        if urls.is_empty() {
            anyhow::bail!("not a sitemap: neither XML nor a list of URLs");
        }
        return Ok(Sitemap::Urls(urls));
    }

    let mut r = reader(&text);
    let mut buf = Vec::new();
    let mut in_sitemap_index = false;
    let mut entries: Vec<SitemapEntry> = Vec::new();
    let mut children: Vec<String> = Vec::new();
    let mut cur_loc = String::new();
    let mut cur_lastmod: Option<String> = None;
    let mut field: Option<Vec<u8>> = None;
    let mut saw_root = false;

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = local(e.name().as_ref()).to_vec();
                match name.as_slice() {
                    b"sitemapindex" => {
                        in_sitemap_index = true;
                        saw_root = true;
                    }
                    b"urlset" => saw_root = true,
                    b"url" | b"sitemap" => {
                        cur_loc.clear();
                        cur_lastmod = None;
                    }
                    b"loc" | b"lastmod" => field = Some(name),
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(f) = &field {
                    let value = t.unescape().unwrap_or_default().trim().to_string();
                    match f.as_slice() {
                        b"loc" => cur_loc = value,
                        b"lastmod" => cur_lastmod = Some(value).filter(|v| !v.is_empty()),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = local(e.name().as_ref()).to_vec();
                match name.as_slice() {
                    b"loc" | b"lastmod" => field = None,
                    b"url" | b"sitemap" if !cur_loc.is_empty() => {
                        if in_sitemap_index {
                            children.push(std::mem::take(&mut cur_loc));
                        } else if entries.len() < limits_max_urls {
                            entries.push(SitemapEntry {
                                url: std::mem::take(&mut cur_loc),
                                lastmod: cur_lastmod.take(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("malformed sitemap XML: {e}"),
            _ => {}
        }
        buf.clear();
    }

    if !saw_root {
        anyhow::bail!("not a sitemap: no <urlset> or <sitemapindex> root");
    }
    if in_sitemap_index {
        Ok(Sitemap::Index(children))
    } else {
        Ok(Sitemap::Urls(entries))
    }
}

/// RSS 2.0, Atom and RDF, which differ only in which tags carry the same three facts.
pub fn parse_feed(bytes: &[u8], max_items: usize) -> anyhow::Result<Vec<FeedItem>> {
    let bytes = maybe_gunzip(bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut r = reader(&text);
    let mut buf = Vec::new();
    let mut items = Vec::new();

    let mut in_item = false;
    let mut url = String::new();
    let mut title = String::new();
    let mut published: Option<String> = None;
    let mut field: Option<Vec<u8>> = None;

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local(e.name().as_ref()).to_vec();
                match name.as_slice() {
                    b"item" | b"entry" => {
                        in_item = true;
                        url.clear();
                        title.clear();
                        published = None;
                    }
                    // Atom keeps the URL in an attribute rather than the element body.
                    b"link" if in_item => {
                        let href = e.attributes().flatten().find(|a| a.key.as_ref() == b"href");
                        match href {
                            Some(a) => {
                                if url.is_empty() {
                                    url = String::from_utf8_lossy(&a.value).into_owned()
                                }
                            }
                            None => field = Some(name),
                        }
                    }
                    b"title" | b"pubDate" | b"published" | b"updated" | b"guid" if in_item => {
                        field = Some(name)
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if field.is_some() => {
                let value = t.unescape().unwrap_or_default().trim().to_string();
                if value.is_empty() {
                    continue;
                }
                match field.as_deref() {
                    Some(b"link") if url.is_empty() => url = value,
                    Some(b"title") if title.is_empty() => title = value,
                    Some(b"pubDate") | Some(b"published") | Some(b"updated") => {
                        published.get_or_insert(value);
                    }
                    Some(b"guid") if url.is_empty() && value.starts_with("http") => url = value,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local(e.name().as_ref()).to_vec();
                field = None;
                if (name == b"item" || name == b"entry") && in_item {
                    in_item = false;
                    if !url.is_empty() && items.len() < max_items {
                        items.push(FeedItem {
                            url: std::mem::take(&mut url),
                            title: std::mem::take(&mut title),
                            published: published.take(),
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("malformed feed XML: {e}"),
            _ => {}
        }
        buf.clear();
    }
    Ok(items)
}

/// Where to look for a sitemap when robots.txt does not say, in the order worth trying.
pub const SITEMAP_GUESSES: &[&str] = &[
    "/sitemap.xml",
    "/sitemap_index.xml",
    "/sitemap-index.xml",
    "/sitemap/sitemap.xml",
    "/sitemap.xml.gz",
];

/// Common feed paths, for sites without autodiscovery `<link>` tags.
pub const FEED_GUESSES: &[&str] = &["/feed", "/feed.xml", "/rss.xml", "/atom.xml", "/index.xml"];

#[cfg(test)]
mod tests {
    use super::*;

    const URLSET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
      <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
        <url><loc>https://example.com/a</loc><lastmod>2026-01-02</lastmod></url>
        <url><loc>https://example.com/b</loc></url>
      </urlset>"#;

    const INDEX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
      <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
        <sitemap><loc>https://example.com/sitemap-posts.xml</loc></sitemap>
        <sitemap><loc>https://example.com/sitemap-pages.xml</loc></sitemap>
      </sitemapindex>"#;

    #[test]
    fn a_urlset_yields_its_urls_and_dates() {
        match parse(URLSET.as_bytes(), DEFAULT_MAX_URLS).unwrap() {
            Sitemap::Urls(u) => {
                assert_eq!(u.len(), 2);
                assert_eq!(u[0].url, "https://example.com/a");
                assert_eq!(u[0].lastmod.as_deref(), Some("2026-01-02"));
                assert_eq!(u[1].lastmod, None);
            }
            other => panic!("expected urls, got {other:?}"),
        }
    }

    #[test]
    fn an_index_yields_child_sitemaps_not_pages() {
        match parse(INDEX.as_bytes(), DEFAULT_MAX_URLS).unwrap() {
            Sitemap::Index(children) => {
                assert_eq!(children.len(), 2);
                assert!(children[0].ends_with("sitemap-posts.xml"));
            }
            other => panic!("expected an index, got {other:?}"),
        }
    }

    #[test]
    fn a_namespaced_prefix_does_not_hide_the_tags() {
        let xml = r#"<sm:urlset xmlns:sm="http://www.sitemaps.org/schemas/sitemap/0.9">
            <sm:url><sm:loc>https://example.com/x</sm:loc></sm:url></sm:urlset>"#;
        match parse(xml.as_bytes(), DEFAULT_MAX_URLS).unwrap() {
            Sitemap::Urls(u) => assert_eq!(u[0].url, "https://example.com/x"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn gzip_is_detected_by_magic_bytes_not_by_extension() {
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(URLSET.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
        match parse(&gz, DEFAULT_MAX_URLS).unwrap() {
            Sitemap::Urls(u) => assert_eq!(u.len(), 2),
            other => panic!("{other:?}"),
        }
        // And plain bytes still pass straight through.
        assert_eq!(maybe_gunzip(b"plain").unwrap(), b"plain");
    }

    #[test]
    fn a_plain_text_sitemap_is_accepted() {
        let txt = "https://example.com/one\nhttps://example.com/two\n# not a url\n";
        match parse(txt.as_bytes(), DEFAULT_MAX_URLS).unwrap() {
            Sitemap::Urls(u) => assert_eq!(u.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_url_cap_is_honoured_without_materialising_everything() {
        let mut xml =
            String::from(r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#);
        for i in 0..50_000 {
            xml.push_str(&format!("<url><loc>https://example.com/p{i}</loc></url>"));
        }
        xml.push_str("</urlset>");
        match parse(xml.as_bytes(), 100).unwrap() {
            Sitemap::Urls(u) => assert_eq!(u.len(), 100, "cap not applied"),
            other => panic!("{other:?}"),
        }
    }

    /// Truncated or sloppy XML is tolerated — real sitemaps often are — but something that is not a
    /// sitemap at all has to be reported, or the caller would treat an error page as "no URLs".
    #[test]
    fn a_truncated_sitemap_recovers_and_a_non_sitemap_errors() {
        match parse(b"<urlset><url><loc>oops", DEFAULT_MAX_URLS) {
            Ok(Sitemap::Urls(u)) => assert!(u.is_empty(), "a truncated entry must not be emitted"),
            other => panic!("expected recovery, got {other:?}"),
        }
        assert!(parse(b"<html><body>a page</body></html>", DEFAULT_MAX_URLS).is_err());
        assert!(parse(b"totally not xml or urls", DEFAULT_MAX_URLS).is_err());
    }

    #[test]
    fn rss_items_are_read() {
        let rss = r#"<rss version="2.0"><channel>
            <title>Feed title</title>
            <item><title>First post</title><link>https://example.com/1</link>
                  <pubDate>Mon, 02 Feb 2026 10:00:00 GMT</pubDate></item>
            <item><title>Second post</title><link>https://example.com/2</link></item>
          </channel></rss>"#;
        let items = parse_feed(rss.as_bytes(), 50).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "First post");
        assert_eq!(items[0].url, "https://example.com/1");
        assert!(items[0].published.is_some());
        assert_eq!(items[1].published, None);
    }

    /// Atom keeps the URL in an attribute, which is the usual place a naive parser misses.
    #[test]
    fn atom_entries_take_the_url_from_the_link_attribute() {
        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom">
            <entry><title>Atom post</title>
                   <link href="https://example.com/atom-1" rel="alternate"/>
                   <published>2026-03-01T00:00:00Z</published></entry>
          </feed>"#;
        let items = parse_feed(atom.as_bytes(), 50).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://example.com/atom-1");
        assert_eq!(items[0].title, "Atom post");
    }

    #[test]
    fn a_feed_with_no_items_is_empty_not_an_error() {
        let empty = r#"<rss version="2.0"><channel><title>Nothing</title></channel></rss>"#;
        assert!(parse_feed(empty.as_bytes(), 50).unwrap().is_empty());
    }
}
