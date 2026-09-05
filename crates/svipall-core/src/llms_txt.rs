//! `llms.txt` generation.
//!
//! A crawl normally hands back a pile of pages. `llms.txt` is the emerging convention for handing a
//! model a *map* instead: one line per page, grouped, with a sentence of context. It is a fraction
//! of the tokens and often all that is needed to decide what to read properly.
//!
//! Pure functions, no I/O: what to include is the crawler's decision, not this module's.

use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct PageSummary {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// First path segment, which is how nearly every site already groups itself.
fn section_of(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return "Pages".into();
    };
    let first = parsed
        .path_segments()
        .and_then(|mut s| s.find(|seg| !seg.is_empty()));
    match first {
        // A path with one segment that looks like a document is a top-level page, not a section.
        Some(seg) if !seg.contains('.') => {
            let mut c = seg.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => "Pages".into(),
            }
        }
        _ => "Pages".into(),
    }
}

/// Markdown link text has to survive brackets in a title.
fn escape_label(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('[', r"\[")
        .replace(']', r"\]")
}

fn describe(summary: &PageSummary, body: Option<&str>) -> String {
    if let Some(d) = summary.description.as_deref() {
        let d = d.trim();
        if !d.is_empty() {
            return truncate(d, 200);
        }
    }
    // Fall back to the first real paragraph: a heading tells the reader nothing they cannot see
    // from the link text itself.
    let Some(body) = body else {
        return String::new();
    };
    crate::budget::blocks(body)
        .into_iter()
        .find(|b| !b.trim_start().starts_with('#') && b.len() > 40)
        .map(|b| truncate(&b.replace('\n', " "), 200))
        .unwrap_or_default()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    let window = &s[..cut];
    let end = window.rfind(' ').unwrap_or(cut);
    format!("{}…", window[..end].trim_end())
}

/// The index: a heading, an optional tagline, then grouped links.
pub fn render_index(
    site: &str,
    tagline: Option<&str>,
    pages: &[(PageSummary, Option<String>)],
) -> String {
    let mut out = format!("# {site}\n\n");
    if let Some(t) = tagline {
        let t = t.trim();
        if !t.is_empty() {
            let _ = write!(out, "> {}\n\n", truncate(t, 300));
        }
    }

    let mut groups: std::collections::HashMap<String, Vec<&(PageSummary, Option<String>)>> =
        std::collections::HashMap::new();
    for p in pages {
        groups.entry(section_of(&p.0.url)).or_default().push(p);
    }

    // Biggest section first, then alphabetically, so the order is stable across runs.
    let mut names: Vec<String> = groups.keys().cloned().collect();
    names.sort_by(|a, b| groups[b].len().cmp(&groups[a].len()).then_with(|| a.cmp(b)));

    for name in names {
        let _ = write!(out, "## {name}\n\n");
        let mut entries = groups.remove(&name).unwrap_or_default();
        entries.sort_by(|a, b| a.0.url.cmp(&b.0.url));
        for (summary, body) in entries {
            let label = summary
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(&summary.url);
            let desc = describe(summary, body.as_deref());
            if desc.is_empty() {
                let _ = writeln!(out, "- [{}]({})", escape_label(label), summary.url);
            } else {
                let _ = writeln!(
                    out,
                    "- [{}]({}): {}",
                    escape_label(label),
                    summary.url,
                    desc
                );
            }
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// `llms-full.txt`: the index followed by every page's content.
pub fn render_full(
    index: &str,
    pages: &[(PageSummary, Option<String>)],
    max_tokens: usize,
) -> String {
    let mut out = String::from(index);
    let mut used = crate::budget::estimate_tokens(index);
    for (summary, body) in pages {
        let Some(body) = body else { continue };
        let title = summary.title.as_deref().unwrap_or(&summary.url);
        let section = format!("\n\n---\n\n# {title}\n\nSource: {}\n\n{body}", summary.url);
        let cost = crate::budget::estimate_tokens(&section);
        if used + cost > max_tokens {
            out.push_str("\n\n---\n\n*(truncated: token budget reached)*");
            break;
        }
        out.push_str(&section);
        used += cost;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(url: &str, title: &str, desc: Option<&str>) -> (PageSummary, Option<String>) {
        (
            PageSummary {
                url: url.into(),
                title: Some(title.into()),
                description: desc.map(str::to_string),
            },
            None,
        )
    }

    #[test]
    fn pages_are_grouped_by_their_first_path_segment() {
        let pages = vec![
            page("https://x.test/docs/a", "Doc A", None),
            page("https://x.test/docs/b", "Doc B", None),
            page("https://x.test/blog/one", "Post One", None),
            page("https://x.test/about", "About", None),
        ];
        let out = render_index("Example", None, &pages);
        assert!(out.contains("## Docs"), "{out}");
        assert!(out.contains("## Blog"), "{out}");
        // A single-segment path is a page, not a section of its own.
        assert!(out.contains("## About"), "{out}");
        assert!(out.contains("[Doc A](https://x.test/docs/a)"), "{out}");
    }

    #[test]
    fn the_largest_section_comes_first_and_the_order_is_stable() {
        let pages = vec![
            page("https://x.test/blog/1", "B1", None),
            page("https://x.test/docs/1", "D1", None),
            page("https://x.test/docs/2", "D2", None),
            page("https://x.test/docs/3", "D3", None),
        ];
        let a = render_index("Example", None, &pages);
        let b = render_index("Example", None, &pages);
        assert_eq!(a, b, "the same input must render identically");
        assert!(
            a.find("## Docs").unwrap() < a.find("## Blog").unwrap(),
            "the bigger section should lead: {a}"
        );
    }

    #[test]
    fn a_description_is_used_when_there_is_one() {
        let pages = vec![page(
            "https://x.test/docs/a",
            "Doc A",
            Some("How to begin."),
        )];
        let out = render_index("Example", None, &pages);
        assert!(out.contains("): How to begin."), "{out}");
    }

    #[test]
    fn without_a_description_the_first_real_paragraph_is_used() {
        let body = "# Heading\n\nThis is the opening paragraph of the document and it is long \
                    enough to be worth quoting.\n\nMore text.";
        let pages = vec![(
            PageSummary {
                url: "https://x.test/docs/a".into(),
                title: Some("Doc A".into()),
                description: None,
            },
            Some(body.to_string()),
        )];
        let out = render_index("Example", None, &pages);
        assert!(out.contains("This is the opening paragraph"), "{out}");
        assert!(
            !out.contains("# Heading"),
            "a heading is not a description: {out}"
        );
    }

    #[test]
    fn brackets_in_a_title_do_not_break_the_link() {
        let pages = vec![page("https://x.test/a", "A [draft] title", None)];
        let out = render_index("Example", None, &pages);
        assert!(out.contains(r"A \[draft\] title"), "{out}");
    }

    #[test]
    fn a_long_description_is_cut_on_a_word_boundary() {
        let long = "word ".repeat(200);
        let pages = vec![page("https://x.test/a", "A", Some(&long))];
        let out = render_index("Example", None, &pages);
        assert!(out.contains('…'), "should be truncated: {out}");
        assert!(out.lines().all(|l| l.len() < 300), "line too long: {out}");
    }

    #[test]
    fn the_tagline_becomes_a_blockquote() {
        let out = render_index("Example", Some("A site about things."), &[]);
        assert!(out.contains("> A site about things."), "{out}");
        assert!(out.starts_with("# Example"), "{out}");
    }

    #[test]
    fn full_output_stops_at_the_token_budget() {
        let pages: Vec<_> = (0..50)
            .map(|i| {
                (
                    PageSummary {
                        url: format!("https://x.test/p{i}"),
                        title: Some(format!("Page {i}")),
                        description: None,
                    },
                    Some("body text ".repeat(200)),
                )
            })
            .collect();
        let index = render_index("Example", None, &pages);
        // The index is the floor, not part of the budget: it is the point of the file. The budget
        // governs how much page content gets appended on top of it.
        let tight = render_full(&index, &pages, crate::budget::estimate_tokens(&index) + 100);
        assert!(
            tight.contains("truncated"),
            "the budget should have stopped it"
        );
        assert!(
            !tight.contains("---\n\n# Page 5"),
            "no page body should have fitted in 100 spare tokens"
        );

        let roomy = render_full(&index, &pages, 1_000_000);
        assert!(
            roomy.contains("# Page 0"),
            "a generous budget should include bodies"
        );
        assert!(roomy.contains("# Page 49"), "and reach the last page");
        assert!(!roomy.contains("truncated"));
    }

    #[test]
    fn an_untitled_page_falls_back_to_its_url() {
        let pages = vec![(
            PageSummary {
                url: "https://x.test/a".into(),
                title: None,
                description: None,
            },
            None,
        )];
        let out = render_index("Example", None, &pages);
        assert!(
            out.contains("[https://x.test/a](https://x.test/a)"),
            "{out}"
        );
    }
}
