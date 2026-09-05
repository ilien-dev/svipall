//! Using a site's own search box.
//!
//! A crawler reaches what a site links to. It does not reach what a site only shows you if you ask
//! for it, and on a shop, a job board or a support site that is most of the content: nothing links
//! to the results for "waterproof boots", they exist only once somebody types it.
//!
//! The valuable part is not the one search. It is the **pattern**: a form with `action="/search"`
//! and a field named `q` says that `/search?q=anything` is a page, forever, without a browser. So
//! this reads the form and hands back a URL, and the ordinary fetch path takes it from there.
//!
//! A form that only works as a POST is reported as such rather than turned into a GET that quietly
//! returns the home page.

/// A search form as the page declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchForm {
    /// The form's `action`, as written — often relative, often empty.
    pub action: String,
    /// `get` or `post`, lowercased. Empty means the HTML default, which is `get`.
    pub method: String,
    /// The name of the text field the query goes in.
    pub field: String,
    /// Every other field the form would submit, so a rebuilt URL is the one the site expects.
    pub hidden: Vec<(String, String)>,
}

impl SearchForm {
    pub fn is_get(&self) -> bool {
        self.method.is_empty() || self.method.eq_ignore_ascii_case("get")
    }
}

/// The URL this form produces for a query, if it produces one at all.
///
/// `None` for a POST: turning it into a GET would return the home page with a `200`, which reads
/// as success everywhere downstream and is the worst possible way to be wrong.
pub fn url_for(page_url: &str, form: &SearchForm, query: &str) -> Option<String> {
    if !form.is_get() || form.field.trim().is_empty() {
        return None;
    }
    let base = url::Url::parse(page_url).ok()?;
    // An empty action means "this page", which is what the HTML spec says and what every site with
    // a search box on its results page relies on.
    let action = if form.action.trim().is_empty() {
        base.clone()
    } else {
        base.join(form.action.trim()).ok()?
    };
    if action.scheme() != "http" && action.scheme() != "https" {
        return None;
    }
    let mut out = action.clone();
    {
        let mut pairs = out.query_pairs_mut();
        pairs.clear();
        // The form's own action may already carry parameters — a category, a language, a version.
        // Dropping them is how a search of the documentation becomes a search of the whole site.
        for (k, v) in action.query_pairs() {
            if k != form.field.as_str() {
                pairs.append_pair(&k, &v);
            }
        }
        for (k, v) in &form.hidden {
            if k != &form.field && !k.trim().is_empty() {
                pairs.append_pair(k, v);
            }
        }
        pairs.append_pair(&form.field, query);
    }
    Some(out.to_string())
}

/// Find the search box on a page and describe the form around it.
///
/// Matched on what the field is for rather than on a class name: `type=search`, the roles and
/// labels assistive technology uses, and the two or three field names every site has used for
/// twenty years. A class name is what differs between sites; this is what does not.
pub const FIND_SEARCH_FORM_JS: &str = r#"(() => {
    const named = ['q', 'query', 's', 'search', 'keyword', 'keywords', 'term', 'wd', 'k'];
    const inputs = Array.from(document.querySelectorAll(
        'input[type="search"], input[type="text"], input:not([type])'));
    const looksLikeSearch = (el) => {
        if ((el.type || '').toLowerCase() === 'search') return true;
        const hay = [el.name, el.id, el.placeholder, el.getAttribute('aria-label'),
                     el.getAttribute('title')].join(' ').toLowerCase();
        if (named.includes((el.name || '').toLowerCase())) return true;
        return /search|buscar|busca|rechercher|suche/.test(hay);
    };
    for (const el of inputs) {
        if (!looksLikeSearch(el)) continue;
        const r = el.getBoundingClientRect();
        // A hidden field named "q" is not the search box; it is somebody's tracking parameter.
        if (r.width < 40 || r.height < 10) continue;
        const form = el.closest('form');
        const hidden = [];
        if (form) {
            for (const f of form.querySelectorAll('input[type="hidden"][name]')) {
                if (f.name && f.name !== el.name) hidden.push([f.name, f.value || '']);
            }
        }
        el.setAttribute('data-svipall-search', '1');
        return {
            action: form ? (form.getAttribute('action') || '') : '',
            method: form ? (form.getAttribute('method') || '') : '',
            field: el.getAttribute('name') || '',
            hidden: hidden,
            selector: '[data-svipall-search="1"]',
            has_form: !!form,
        };
    }
    return null;
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn form(action: &str, method: &str, field: &str) -> SearchForm {
        SearchForm {
            action: action.into(),
            method: method.into(),
            field: field.into(),
            hidden: Vec::new(),
        }
    }

    #[test]
    fn a_search_box_becomes_a_url_anything_can_fetch() {
        // The prize: one browser visit turns into a pattern that needs no browser at all.
        let url = url_for(
            "https://shop.test/products",
            &form("/search", "get", "q"),
            "waterproof boots",
        )
        .expect("a GET form has a URL");
        assert_eq!(url, "https://shop.test/search?q=waterproof+boots");
    }

    #[test]
    fn a_relative_action_resolves_against_the_page_it_was_found_on() {
        let url = url_for(
            "https://x.test/docs/intro",
            &form("../find", "", "term"),
            "cache",
        )
        .expect("resolves");
        assert_eq!(url, "https://x.test/find?term=cache");
    }

    #[test]
    fn an_empty_action_means_this_page() {
        // What the spec says, and what every site with a search box on its results page relies on.
        let url =
            url_for("https://x.test/results", &form("", "get", "q"), "rust").expect("resolves");
        assert_eq!(url, "https://x.test/results?q=rust");
    }

    #[test]
    fn a_post_only_form_is_refused_rather_than_turned_into_a_get() {
        // A GET against a POST form returns the home page with a 200, which reads as success
        // everywhere downstream. That is the worst possible way to be wrong.
        assert_eq!(
            url_for("https://x.test/", &form("/search", "POST", "q"), "a"),
            None
        );
    }

    #[test]
    fn a_form_with_no_named_field_produces_nothing() {
        assert_eq!(
            url_for("https://x.test/", &form("/search", "get", ""), "a"),
            None
        );
    }

    #[test]
    fn the_query_is_escaped_rather_than_pasted_in() {
        let url = url_for("https://x.test/", &form("/s", "get", "q"), "a&b=c d#e").expect("built");
        assert!(url.contains("q=a%26b%3Dc+d%23e"), "{url}");
        assert_eq!(url.matches('?').count(), 1, "{url}");
    }

    #[test]
    fn the_forms_own_parameters_survive() {
        // Dropping them turns a search of the documentation into a search of the whole site.
        let url = url_for(
            "https://x.test/",
            &form("/search?section=docs&lang=en", "get", "q"),
            "cache",
        )
        .expect("built");
        assert!(url.contains("section=docs"), "{url}");
        assert!(url.contains("lang=en"), "{url}");
        assert!(url.contains("q=cache"), "{url}");
    }

    #[test]
    fn hidden_fields_are_submitted_because_the_site_is_expecting_them() {
        let mut f = form("/search", "get", "q");
        f.hidden = vec![
            ("csrf".into(), "abc".into()),
            ("scope".into(), "products".into()),
        ];
        let url = url_for("https://x.test/", &f, "boots").expect("built");
        assert!(url.contains("csrf=abc"), "{url}");
        assert!(url.contains("scope=products"), "{url}");
    }

    #[test]
    fn the_query_field_appears_once_even_if_the_action_already_had_it() {
        // Two `q=` parameters is a query nobody typed, and which one wins is up to the server.
        let url = url_for("https://x.test/", &form("/s?q=old", "get", "q"), "new").expect("built");
        assert_eq!(url.matches("q=").count(), 1, "{url}");
        assert!(url.ends_with("q=new"), "{url}");
    }

    #[test]
    fn a_form_pointing_somewhere_that_is_not_the_web_is_refused() {
        assert_eq!(
            url_for(
                "https://x.test/",
                &form("javascript:void(0)", "get", "q"),
                "a"
            ),
            None
        );
        assert_eq!(url_for("not a url", &form("/s", "get", "q"), "a"), None);
    }

    #[test]
    fn the_finder_matches_what_a_field_is_for_rather_than_what_it_is_called() {
        // A class name is what differs between sites; `type=search`, the aria label and the two or
        // three field names everyone has used for twenty years are what do not.
        assert!(FIND_SEARCH_FORM_JS.contains("type=\"search\""));
        assert!(FIND_SEARCH_FORM_JS.contains("aria-label"));
        assert!(
            FIND_SEARCH_FORM_JS.contains("r.width < 40"),
            "a hidden field named q is somebody's tracking parameter, not the search box"
        );
    }
}
