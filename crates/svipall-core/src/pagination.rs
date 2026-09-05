//! Getting past the first page.
//!
//! A crawler that follows links treats page two as one more link among hundreds, competing with
//! every "about" and "terms" on the site — so a listing of forty pages usually ends after the
//! first, and the caller is told the crawl finished. It did not; it just never noticed there was
//! more.
//!
//! Two shapes cover almost all of it. Either the next page is a URL that differs by a number, or it
//! is a button that fetches more into the same document. The first is recognised here, from the
//! URL alone, and it costs nothing: no extra request, no rendering, no guessing at markup.

/// A page-number parameter found in a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageParam {
    /// The parameter name, e.g. `page`, or `path` when the number is a path segment.
    pub name: String,
    pub current: u64,
}

/// Query parameters that carry a page number in practice.
const PAGE_KEYS: &[&str] = &[
    "page",
    "p",
    "pagina",
    "pg",
    "start",
    "offset",
    "from",
    "skip",
    "pageindex",
    "page_number",
    "pagenum",
];

/// Find the page number in a URL, if it has one.
///
/// A query parameter wins over a path segment: `?page=2` is unambiguous, while a number in a path
/// might be an identifier. `/products/12345` is a product, not page 12345, so path numbers only
/// count when the segment before them says otherwise.
pub fn page_param(url: &str) -> Option<PageParam> {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    if let Some(q) = query {
        for pair in q.split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            let key = k.to_ascii_lowercase();
            if PAGE_KEYS.contains(&key.as_str()) {
                if let Ok(n) = v.parse::<u64>() {
                    return Some(PageParam {
                        name: k.to_string(),
                        current: n,
                    });
                }
            }
        }
    }
    // `/page/3/` and friends: the number only means a page when the segment before it says so.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for w in segments.windows(2) {
        let label = w[0].to_ascii_lowercase();
        if PAGE_KEYS.contains(&label.as_str()) {
            if let Ok(n) = w[1].parse::<u64>() {
                return Some(PageParam {
                    name: "path".into(),
                    current: n,
                });
            }
        }
    }
    None
}

/// The URL of the next page, if there is a sensible one.
///
/// Offsets step by whatever the current value suggests rather than by one: `?offset=20` is followed
/// by `?offset=40`, and incrementing it to 21 would fetch the same page shifted by a row.
pub fn next_page(url: &str) -> Option<String> {
    let p = page_param(url)?;
    let is_offset = matches!(
        p.name.to_ascii_lowercase().as_str(),
        "start" | "offset" | "from" | "skip"
    );
    // A zero offset gives no step size to infer, so treat it as the first page of an unknown
    // stride and leave it alone rather than guessing.
    if is_offset && p.current == 0 {
        return None;
    }
    let next = if is_offset {
        p.current * 2
    } else {
        p.current + 1
    };
    Some(replace_number(url, &p, next))
}

fn replace_number(url: &str, p: &PageParam, next: u64) -> String {
    if p.name == "path" {
        // Rebuild the path, changing only the number that follows a page label.
        let mut segments: Vec<String> = url.split('/').map(str::to_string).collect();
        for i in 0..segments.len().saturating_sub(1) {
            let label = segments[i].to_ascii_lowercase();
            if PAGE_KEYS.contains(&label.as_str()) && segments[i + 1].parse::<u64>().is_ok() {
                segments[i + 1] = next.to_string();
                break;
            }
        }
        return segments.join("/");
    }
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let rebuilt: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == p.name => format!("{k}={next}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", rebuilt.join("&"))
}

/// Selectors for the control that loads more into the same page.
///
/// Matched on what the button says rather than on a class name, because the text is what stays the
/// same between two sites and a class name is what does not.
pub const LOAD_MORE_JS: &str = r#"(() => {
    const wanted = ['load more', 'show more', 'see more', 'view more', 'more results',
                    'cargar mas', 'ver mas', 'mostrar mas', 'next page', 'siguiente'];
    const nodes = document.querySelectorAll('button, a[role="button"], div[role="button"], a');
    for (const el of nodes) {
        const t = (el.innerText || el.textContent || '').trim().toLowerCase()
            .normalize('NFD').replace(/[̀-ͯ]/g, '');
        if (t.length > 40 || !wanted.some(w => t.includes(w))) continue;
        const r = el.getBoundingClientRect();
        if (r.width < 20 || r.height < 10) continue;
        el.setAttribute('data-svipall-more', '1');
        return '[data-svipall-more="1"]';
    }
    return '';
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_number_in_the_query_is_found_and_advanced() {
        assert_eq!(
            next_page("https://x.test/list?page=1").as_deref(),
            Some("https://x.test/list?page=2")
        );
        assert_eq!(
            next_page("https://x.test/list?q=shoes&p=4&sort=new").as_deref(),
            Some("https://x.test/list?q=shoes&p=5&sort=new"),
            "the other parameters must survive untouched"
        );
    }

    #[test]
    fn an_offset_steps_by_its_own_size_rather_than_by_one() {
        // `?offset=21` fetches the same page shifted by a single row, which is the slowest possible
        // way to walk a list and looks nothing like a person paging through it.
        assert_eq!(
            next_page("https://x.test/list?offset=20").as_deref(),
            Some("https://x.test/list?offset=40")
        );
        assert_eq!(
            next_page("https://x.test/list?start=50").as_deref(),
            Some("https://x.test/list?start=100")
        );
    }

    #[test]
    fn a_zero_offset_gives_no_stride_to_infer_so_nothing_is_guessed() {
        assert_eq!(next_page("https://x.test/list?offset=0"), None);
    }

    #[test]
    fn a_page_number_in_the_path_is_found_when_it_is_labelled() {
        assert_eq!(
            next_page("https://x.test/blog/page/3/").as_deref(),
            Some("https://x.test/blog/page/4/")
        );
    }

    #[test]
    fn a_number_in_a_path_is_not_assumed_to_be_a_page() {
        // `/products/12345` is a product. Treating it as page 12345 and asking for 12346 would walk
        // a catalogue at random.
        assert_eq!(page_param("https://x.test/products/12345"), None);
        assert_eq!(next_page("https://x.test/products/12345"), None);
    }

    #[test]
    fn a_url_with_no_page_number_has_no_next_page() {
        assert_eq!(next_page("https://x.test/about"), None);
        assert_eq!(next_page("https://x.test/list?q=shoes"), None);
        assert_eq!(next_page("https://x.test/list?page=first"), None);
    }

    #[test]
    fn the_parameter_name_is_preserved_exactly_as_written() {
        // Rewriting `Page` as `page` would change the URL for a server that cares.
        let out = next_page("https://x.test/l?Page=2").expect("found");
        assert!(out.contains("Page=3"), "{out}");
    }

    #[test]
    fn the_first_page_parameter_wins_when_there_are_two() {
        let p = page_param("https://x.test/l?page=2&offset=40").expect("found");
        assert_eq!(p.name, "page");
        assert_eq!(p.current, 2);
    }
}
