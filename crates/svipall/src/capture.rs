//! The requests a page makes while it loads, and what came back.
//!
//! Most of the modern web renders from JSON its own JavaScript fetched a moment earlier. Scraping
//! the rendered HTML means reconstructing, badly, a structure the page already received cleanly:
//! prices become strings with currency symbols glued on, pagination becomes a guess, and every
//! layout change breaks it.
//!
//! Watching the traffic instead hands back that JSON. It is smaller than the HTML, already typed,
//! and stable across redesigns — the markup is what changes, the endpoint behind it rarely does.
//! It also gives the crawler somewhere to go next: an endpoint that took `?page=1` will take
//! `?page=2`, which is worth more than any number of followed links.
//!
//! The filtering is pure and tested. Only the subscription needs a browser.

/// One response the page received.
#[derive(Debug, Clone, PartialEq)]
pub struct Captured {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub mime: String,
    /// Body, when it was fetched and was small enough to keep.
    pub body: Option<String>,
}

/// Which responses are worth keeping.
///
/// Images, fonts, stylesheets and the document itself are noise here: the document is what the
/// other tools already return, and the rest carries no data. What is left is the traffic that
/// exists to carry values — JSON, and the occasional XML feed.
pub fn is_interesting(mime: &str, resource_type: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    let t = resource_type.to_ascii_lowercase();
    if matches!(
        t.as_str(),
        "image" | "font" | "stylesheet" | "media" | "manifest" | "ping" | "document"
    ) {
        return false;
    }
    m.contains("json")
        || m.contains("javascript") && m.contains("json")
        || m.contains("xml") && !m.contains("html")
        || t == "xhr"
        || t == "fetch"
}

/// Whether a URL matches the caller's pattern.
///
/// Substring rather than a regular expression, and deliberately: the caller is naming an endpoint
/// they saw, not writing a grammar, and a mistyped pattern that silently matches nothing is worse
/// than one that matches too much.
pub fn matches(url: &str, pattern: Option<&str>) -> bool {
    match pattern {
        None | Some("") => true,
        Some(p) => url.to_lowercase().contains(&p.to_lowercase()),
    }
}

/// Trim a captured body to a budget, keeping it parseable when possible.
///
/// A truncated JSON document is not JSON, so an oversized body is reported as truncated text rather
/// than handed back as something that looks structured and will not parse.
pub fn cap_body(body: &str, max: usize) -> (String, bool) {
    if body.len() <= max {
        return (body.to_string(), false);
    }
    let cut: String = body.chars().take(max).collect();
    (cut, true)
}

/// The endpoints worth calling again, deduplicated by shape.
///
/// Two URLs that differ only in a page number are one endpoint, and reporting both teaches the
/// caller nothing. Digits in the query are collapsed so the shape is what gets compared.
pub fn endpoints(captured: &[Captured]) -> Vec<String> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for c in captured {
        let shape = shape_of(&c.url);
        if !seen.iter().any(|(s, _)| *s == shape) {
            seen.push((shape, c.url.clone()));
        }
    }
    seen.into_iter().map(|(_, url)| url).collect()
}

fn shape_of(url: &str) -> String {
    // A *run* of digits collapses to one marker, not one marker per digit: otherwise `page=1` and
    // `page=37` have different shapes and the deduplication does nothing on exactly the case it
    // exists for.
    let mut out = String::with_capacity(url.len());
    let mut in_digits = false;
    for c in url.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(url: &str) -> Captured {
        Captured {
            url: url.into(),
            method: "GET".into(),
            status: 200,
            mime: "application/json".into(),
            body: None,
        }
    }

    #[test]
    fn the_traffic_that_carries_data_is_kept_and_the_rest_is_not() {
        assert!(is_interesting("application/json", "xhr"));
        assert!(is_interesting("application/json; charset=utf-8", "fetch"));
        assert!(is_interesting("text/xml", "other"));
        for (mime, kind) in [
            ("image/png", "image"),
            ("font/woff2", "font"),
            ("text/css", "stylesheet"),
            ("video/mp4", "media"),
        ] {
            assert!(!is_interesting(mime, kind), "{mime} kept");
        }
    }

    #[test]
    fn the_page_itself_is_not_captured_as_traffic() {
        // It is what every other tool already returns; capturing it again doubles the tokens.
        assert!(!is_interesting("text/html", "document"));
    }

    #[test]
    fn no_pattern_means_everything_and_matching_ignores_case() {
        assert!(matches("https://x.test/api/v1", None));
        assert!(matches("https://x.test/api/v1", Some("")));
        assert!(matches("https://x.test/API/v1", Some("api")));
        assert!(!matches("https://x.test/other", Some("api")));
    }

    #[test]
    fn an_oversized_body_is_reported_as_truncated_not_passed_off_as_json() {
        // Half a JSON document is not JSON, and handing it back as if it were sends the caller to
        // a parse error instead of to the truth.
        let big = "x".repeat(500);
        let (body, truncated) = cap_body(&big, 100);
        assert!(truncated);
        assert_eq!(body.len(), 100);

        let (body, truncated) = cap_body("small", 100);
        assert!(!truncated);
        assert_eq!(body, "small");
    }

    #[test]
    fn endpoints_that_differ_only_by_a_page_number_are_one_endpoint() {
        // Listing every page of the same call teaches the caller nothing about where to go next.
        let list = vec![
            cap("https://x.test/api/items?page=1"),
            cap("https://x.test/api/items?page=2"),
            cap("https://x.test/api/items?page=37"),
            cap("https://x.test/api/users?page=1"),
        ];
        let out = endpoints(&list);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].contains("items"));
        assert!(out[1].contains("users"));
    }

    #[test]
    fn genuinely_different_endpoints_are_all_reported() {
        let list = vec![
            cap("https://x.test/api/a"),
            cap("https://x.test/api/b"),
            cap("https://x.test/api/c"),
        ];
        assert_eq!(endpoints(&list).len(), 3);
    }

    #[test]
    fn nothing_captured_is_no_endpoints_rather_than_a_panic() {
        assert!(endpoints(&[]).is_empty());
    }
}
