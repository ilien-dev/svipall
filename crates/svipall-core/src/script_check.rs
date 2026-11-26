//! A check that asks only whether the visitor runs script.
//!
//! Some sites answer a first visit with a page that is nothing but a hidden form and a few lines
//! of script: the script joins a value the server sent to itself, writes it in the form and
//! submits it, and the reply to that submission is the page that was asked for, with a cookie
//! that spares the next visit the same detour. Nothing about the visitor is measured and there
//! is nothing to solve. It is `pow` with the work taken out.
//!
//! A browser tier passes it by existing, three seconds and a browser later. This reads the form
//! and names the address its script would have submitted, so the first tier can ask for it.
//!
//! Deliberately narrow. The script is recognised by its exact shape, never evaluated, and
//! anything else on the page (a second form, a visible field, a script that does something more)
//! is a different page: `answer` says `None` and the ladder climbs as it always did.

use regex::Regex;
use std::sync::LazyLock;
use url::Url;

/// An interstitial is small. A page this size with a form on it is a page.
const MAX_DOCUMENT: usize = 32_000;
/// What the server sent to be doubled. Longer than this is not a nonce.
const MAX_SEED: usize = 256;
/// The field the script writes its result in.
const SOLUTION: &str = "solution";

static FORM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<form\b([^>]*)>").unwrap());
static INPUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<input\b([^>]*)>").unwrap());
static ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\b([a-z-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());
/// `(async e=>e+e)("seed")`: a function that returns its argument joined to itself, called on
/// the server's value. The three names are compared afterwards; the pattern cannot.
static DOUBLED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"\(\s*async\s+(\w+)\s*=>\s*(\w+)\s*\+\s*(\w+)\s*\)\s*\(\s*"([0-9A-Za-z_\-]+)"\s*\)"#,
    )
    .unwrap()
});

fn attr(tag: &str, name: &str) -> Option<String> {
    ATTR.captures_iter(tag).find_map(|c| {
        c.get(1)?
            .as_str()
            .eq_ignore_ascii_case(name)
            .then(|| c.get(2).or(c.get(3)).map(|m| m.as_str().to_string()))
            .flatten()
    })
}

fn unescape(v: &str) -> String {
    v.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// The address the page's own script would submit its form to, when the page is this check and
/// nothing else. `url` is where the page was served from.
pub fn answer(html: &str, url: &str) -> Option<String> {
    if html.len() > MAX_DOCUMENT {
        return None;
    }
    let mut forms = FORM.captures_iter(html);
    let form = forms.next()?.get(1)?.as_str().to_string();
    if forms.next().is_some() {
        return None;
    }
    if !attr(&form, "method").is_some_and(|m| m.eq_ignore_ascii_case("get")) {
        return None;
    }
    // The script writes the field by name and submits; without both it is doing something else.
    if !html.contains(&format!("namedItem(\"{SOLUTION}\")")) || !html.contains("requestSubmit()") {
        return None;
    }
    let mut doubled = DOUBLED.captures_iter(html);
    let c = doubled.next()?;
    if doubled.next().is_some() {
        return None;
    }
    let (a, b, d, seed) = (&c[1], &c[2], &c[3], &c[4]);
    if a != b || a != d || seed.len() > MAX_SEED {
        return None;
    }
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut has_solution = false;
    for tag in INPUT.captures_iter(html) {
        let tag = tag.get(1)?.as_str();
        // A field a person could see or fill is a form for a person.
        if !attr(tag, "type").is_some_and(|t| t.eq_ignore_ascii_case("hidden")) {
            return None;
        }
        let name = attr(tag, "name")?;
        if name == SOLUTION {
            has_solution = true;
            fields.push((name, format!("{seed}{seed}")));
        } else {
            fields.push((name, unescape(&attr(tag, "value").unwrap_or_default())));
        }
    }
    if !has_solution {
        return None;
    }
    let page = Url::parse(url).ok()?;
    let mut target = page
        .join(&unescape(&attr(&form, "action").unwrap_or_default()))
        .ok()?;
    // Never somewhere else: the check is the site's own, and so is where it is answered.
    if target.origin() != page.origin() {
        return None;
    }
    // The script adds what the address already carried, after the form's own fields.
    fields.extend(
        page.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned())),
    );
    target.query_pairs_mut().clear().extend_pairs(fields);
    Some(target.into())
}

/// Is this address one `answer` made? The page it leads to is filed under the address that was
/// asked for, not under the detour.
pub fn is_answer(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| u.query_pairs().any(|(k, _)| k == SOLUTION))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../fixtures/script-check.html");
    const URL: &str = "https://forum.test/r/rust/top/?t=week&sort=top";

    #[test]
    fn the_form_is_submitted_as_its_script_would_submit_it() {
        assert_eq!(
            answer(PAGE, URL).as_deref(),
            Some(
                "https://forum.test/r/rust/top/?solution=96d3dc23fd7cd75496d3dc23fd7cd754\
                 &js_challenge=1&jsc_token=2824be10929bdc60&jsc_orig_r=&t=week&sort=top"
            )
        );
        assert!(is_answer(&answer(PAGE, URL).unwrap()));
        assert!(!is_answer(URL));
    }

    #[test]
    fn an_ordinary_page_with_a_form_is_not_this() {
        let search = "<html><body><form method=\"GET\" action=\"/search\">\
                      <input type=\"text\" name=\"q\"></form></body></html>";
        assert_eq!(answer(search, URL), None);
        assert_eq!(answer("<html><body><p>Hello</p></body></html>", URL), None);
    }

    #[test]
    fn a_script_of_any_other_shape_is_left_to_a_browser() {
        // Not the same name three times: this is some other function.
        let other = PAGE.replace("(async e=>e+e)", "(async e=>e+n)");
        assert_eq!(answer(&other, URL), None);
        let harder = PAGE.replace("(async e=>e+e)", "(async e=>sha256(e))");
        assert_eq!(answer(&harder, URL), None);
        // A field somebody could type in.
        let visible = PAGE.replace(
            "<input type=\"hidden\" name=\"js_challenge\"",
            "<input type=\"text\" name=\"js_challenge\"",
        );
        assert_eq!(answer(&visible, URL), None);
        let two = PAGE.replace("</body>", "<form method=\"GET\"></form></body>");
        assert_eq!(answer(&two, URL), None);
        let post = PAGE.replace("method=\"GET\"", "method=\"POST\"");
        assert_eq!(answer(&post, URL), None);
    }

    #[test]
    fn the_answer_never_leaves_the_site_that_asked() {
        let elsewhere = PAGE.replace("action=\"/r/rust/top/\"", "action=\"https://other.test/x\"");
        assert_eq!(answer(&elsewhere, URL), None);
    }

    #[test]
    fn a_page_too_large_to_be_an_interstitial_is_not_read_as_one() {
        let big = PAGE.replace(
            "</body>",
            &format!("{}</body>", "<p>text</p>".repeat(4_000)),
        );
        assert_eq!(answer(&big, URL), None);
    }
}
