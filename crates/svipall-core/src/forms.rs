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
///
/// Open shadow roots are walked too: a design system's search component keeps its input inside
/// one, where `document.querySelectorAll` never looks. Nothing is written to the page — the box
/// is described by its rectangle, not marked with an attribute a script could read.
pub const FIND_SEARCH_FORM_JS: &str = r#"(() => {
    const named = ['q', 'query', 's', 'search', 'keyword', 'keywords', 'term', 'wd', 'k'];
    const deep = (root, sel, out) => {
        for (const el of root.querySelectorAll(sel)) out.push(el);
        for (const host of root.querySelectorAll('*')) {
            if (host.shadowRoot) deep(host.shadowRoot, sel, out);
        }
        return out;
    };
    const inputs = deep(document,
        'input[type="search"], input[type="text"], input:not([type])', []);
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
        const form = el.form || el.closest('form');
        const hidden = [];
        if (form) {
            for (const f of form.querySelectorAll('input[type="hidden"][name]')) {
                if (f.name && f.name !== el.name) hidden.push([f.name, f.value || '']);
            }
        }
        return {
            action: form ? (form.getAttribute('action') || '') : '',
            method: form ? (form.getAttribute('method') || '') : '',
            field: el.getAttribute('name') || '',
            hidden: hidden,
            has_form: !!form,
            in_shadow: el.getRootNode() !== document,
            rect: {x: r.x, y: r.y, w: r.width, h: r.height},
        };
    }
    return null;
})()"#;

/// Find the control that opens a search box which is not on the page yet: the magnifier icon,
/// the "Search" button in a header. Described by its rectangle, so the pointer can go there the way
/// a person's would; nothing is clicked from here.
pub const FIND_SEARCH_TOGGLE_JS: &str = r#"(() => {
    const deep = (root, sel, out) => {
        for (const el of root.querySelectorAll(sel)) out.push(el);
        for (const host of root.querySelectorAll('*')) {
            if (host.shadowRoot) deep(host.shadowRoot, sel, out);
        }
        return out;
    };
    const words = /search|buscar|busca|pesquisa|recherch|suche|cerca|zoek/;
    const candidates = deep(document,
        'button, a, summary, [role="button"], [aria-label], [title], label', []);
    let best = null;
    for (const el of candidates) {
        if (el.tagName === 'INPUT') continue;
        const hay = [el.getAttribute('aria-label'), el.getAttribute('title'),
                     el.getAttribute('data-testid'), el.id,
                     typeof el.className === 'string' ? el.className : '',
                     (el.textContent || '').trim().slice(0, 40)].join(' ').toLowerCase();
        if (!words.test(hay)) continue;
        const r = el.getBoundingClientRect();
        const st = getComputedStyle(el);
        if (r.width < 8 || r.height < 8 || st.visibility === 'hidden' || st.display === 'none') continue;
        if (r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) continue;
        // A link to a search page is a page, not a control: it is followed by the URL it names.
        const href = el.tagName === 'A' ? (el.getAttribute('href') || '') : '';
        const control = !href || href === '#' || href.startsWith('javascript:');
        const score = (el.getAttribute('aria-label') ? 2 : 0) + (el.querySelector('svg') ? 1 : 0)
                    + (control ? 2 : 0) + (el.tagName === 'BUTTON' ? 1 : 0);
        if (!best || score > best.score) {
            best = {score, label: hay.trim().slice(0, 60),
                    href: control ? '' : new URL(href, location.href).href,
                    rect: {x: r.x, y: r.y, w: r.width, h: r.height}};
        }
    }
    return best;
})()"#;

/// Every `<link rel="search">` that points at an OpenSearch description, resolved against the
/// page it was found on.
///
/// The site saying, in a standard format, exactly how its search is addressed — read before
/// anything is guessed and before a browser is started.
pub fn opensearch_links(html: &str, page_url: &str) -> Vec<String> {
    let Ok(base) = url::Url::parse(page_url) else {
        return Vec::new();
    };
    let tag = regex::Regex::new(r"(?is)<link\b[^>]*>").expect("static regex");
    let mut out = Vec::new();
    for m in tag.find_iter(html) {
        let t = m.as_str();
        let rel = attr(t, "rel").unwrap_or_default().to_ascii_lowercase();
        let ty = attr(t, "type").unwrap_or_default().to_ascii_lowercase();
        if !rel.split_whitespace().any(|r| r == "search") || !ty.contains("opensearchdescription") {
            continue;
        }
        if let Some(href) = attr(t, "href").and_then(|h| base.join(h.trim()).ok()) {
            if matches!(href.scheme(), "http" | "https") && !out.contains(&href.to_string()) {
                out.push(href.to_string());
            }
        }
    }
    out
}

/// One attribute of one tag, unquoted and with the five entities a URL can carry decoded.
fn attr(tag: &str, name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r#"(?is)\s{}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#,
        regex::escape(name)
    ))
    .ok()?;
    let c = re.captures(tag)?;
    let v = c.get(1).or(c.get(2)).or(c.get(3))?.as_str();
    Some(
        v.replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">"),
    )
}

/// The results-page template an OpenSearch description declares: the `Url` of type `text/html`
/// that is fetched by GET, else one of the feed types, resolved against the description itself.
pub fn opensearch_template(xml: &str, description_url: &str) -> Option<String> {
    let tag = regex::Regex::new(r"(?is)<(?:\w+:)?Url\b[^>]*>").expect("static regex");
    let mut best: Option<(u8, String)> = None;
    for m in tag.find_iter(xml) {
        let t = m.as_str();
        let Some(template) = attr(t, "template") else {
            continue;
        };
        let method = attr(t, "method").unwrap_or_default();
        if !method.is_empty() && !method.eq_ignore_ascii_case("get") {
            continue;
        }
        let ty = attr(t, "type").unwrap_or_default().to_ascii_lowercase();
        // A results page first: it is what a person would see. Suggestions are not results.
        let rank = match ty.as_str() {
            "text/html" | "application/xhtml+xml" => 3,
            t if t.contains("rss") || t.contains("atom") => 2,
            t if t.contains("suggestions") => continue,
            _ => 1,
        };
        if !template.contains("{searchTerms}") {
            continue;
        }
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, template));
        }
    }
    let (_, template) = best?;
    // Templates are meant to be absolute; a relative one is resolved the way a browser would.
    let base = url::Url::parse(description_url).ok()?;
    let braces_safe = template.replace('{', "%7B").replace('}', "%7D");
    let joined = base.join(&braces_safe).ok()?.to_string();
    Some(joined.replace("%7B", "{").replace("%7D", "}"))
}

/// An OpenSearch template with the query in it, every other parameter given its documented
/// default, and an optional parameter left out entirely.
pub fn fill_template(template: &str, query: &str) -> Option<String> {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let param = regex::Regex::new(r"\{([A-Za-z:]+)(\??)\}").expect("static regex");
    let filled = param.replace_all(template, |c: &regex::Captures| {
        let name = c.get(1).map(|m| m.as_str()).unwrap_or("");
        let optional = c.get(2).is_some_and(|m| !m.as_str().is_empty());
        match name.rsplit(':').next().unwrap_or(name) {
            "searchTerms" => enc(query),
            _ if optional => String::new(),
            "startPage" | "startIndex" => "1".into(),
            "count" => "10".into(),
            "language" => "*".into(),
            "inputEncoding" | "outputEncoding" => "UTF-8".into(),
            _ => String::new(),
        }
    });
    let mut u = url::Url::parse(&filled).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    // A parameter emptied because it was optional is dropped rather than sent as `page=`.
    let kept: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        u.set_query(None);
    } else {
        u.query_pairs_mut().clear().extend_pairs(kept);
    }
    Some(u.to_string())
}

/// A query endpoint worth trying on a site whose page offers no form and no description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The URL with the query in it.
    pub url: String,
    /// What about the site suggested it, for the report of what was tried.
    pub why: &'static str,
}

/// Query endpoints of the common publishing platforms, recognised by the shape of the URL and the
/// paths the page itself references, never by a product name. Most specific first; the generic
/// shapes every platform tends to answer close the list. Each is only a guess until the caller
/// has fetched it and seen the query come back.
pub fn endpoints(page_url: &str, html: &str, query: &str) -> Vec<Endpoint> {
    let Ok(base) = url::Url::parse(page_url) else {
        return Vec::new();
    };
    let host = base.host_str().unwrap_or("").to_ascii_lowercase();
    let lower = html.to_ascii_lowercase();
    let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let mut shapes: Vec<(&str, &'static str)> = Vec::new();
    // A newsletter platform's publications: an archive endpoint that searches, as JSON, and its
    // own archive page with the same search.
    if host.ends_with(".substack.com") || lower.contains("/api/v1/archive") {
        shapes.push((
            "/api/v1/archive?sort=new&search={q}&offset=0&limit=12",
            "the archive search of a newsletter publication",
        ));
        shapes.push((
            "/archive?sort=search&search={q}",
            "the archive page of a newsletter publication",
        ));
    }
    // A site that advertises a `/wp-json/` REST root: its search route, and the `?s=` page.
    if lower.contains("/wp-json/") || lower.contains("api.w.org") {
        shapes.push((
            "/wp-json/wp/v2/search?search={q}",
            "the search route of the site's /wp-json/ REST API",
        ));
        shapes.push(("/?s={q}", "the ?s= results page of a /wp-json/ site"));
    }
    // A wiki engine served from `/w/index.php`.
    if lower.contains("/w/index.php") || lower.contains("/w/api.php") {
        shapes.push((
            "/w/index.php?search={q}&fulltext=1",
            "the full-text search of a /w/index.php wiki",
        ));
    }
    // A forum that serves `/search.json`.
    if lower.contains("/search.json") {
        shapes.push(("/search.json?q={q}", "the /search.json route of a forum"));
    }
    for generic in ["/search?q={q}", "/search?query={q}", "/?s={q}"] {
        shapes.push((generic, "a generic search path"));
    }
    let mut out: Vec<Endpoint> = Vec::new();
    for (path, why) in shapes {
        let Ok(u) = base.join(&path.replace("{q}", &q)) else {
            continue;
        };
        let url = u.to_string();
        if !out.iter().any(|e| e.url == url) {
            out.push(Endpoint { url, why });
        }
    }
    out
}

/// Whether a fetched page answers the query rather than being some other page that came back
/// `200`: the query's words appear in it, and it is not the page the search started from.
///
/// The second half is what stops `/?s=boots` on a site that ignores `s` — which serves its home
/// page — from being reported as a search that works.
pub fn answers_query(results: &str, start_page: &str, query: &str) -> bool {
    let text = results.to_lowercase();
    let words: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_string)
        .collect();
    let echoed = if words.is_empty() {
        text.contains(&query.to_lowercase())
    } else {
        words.iter().all(|w| text.contains(w.as_str()))
    };
    echoed && results.trim() != start_page.trim()
}

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
    fn the_finders_leave_nothing_on_the_page_and_look_inside_shadow_roots() {
        for js in [FIND_SEARCH_FORM_JS, FIND_SEARCH_TOGGLE_JS] {
            assert!(
                !js.contains("setAttribute"),
                "a marker is readable by the page"
            );
            assert!(
                !js.contains("data-svipall"),
                "a marker is readable by the page"
            );
            assert!(js.contains("shadowRoot"), "open shadow roots are walked");
        }
    }

    #[test]
    fn an_opensearch_description_is_found_read_and_filled() {
        let html = r#"<head><link rel="stylesheet" href="/a.css">
            <link title="Site" type="application/opensearchdescription+xml" rel="search"
                  href="/opensearch.xml"></head>"#;
        assert_eq!(
            opensearch_links(html, "https://x.test/docs/"),
            vec!["https://x.test/opensearch.xml".to_string()]
        );
        let xml = r#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
            <Url type="application/x-suggestions+json" template="https://x.test/s?q={searchTerms}"/>
            <Url type="text/html" method="get"
                 template="https://x.test/find?term={searchTerms}&amp;p={startPage?}&amp;lang={language}"/>
            </OpenSearchDescription>"#;
        let t = opensearch_template(xml, "https://x.test/opensearch.xml").expect("a template");
        assert_eq!(
            t,
            "https://x.test/find?term={searchTerms}&p={startPage?}&lang={language}"
        );
        assert_eq!(
            fill_template(&t, "rain boots").as_deref(),
            Some("https://x.test/find?term=rain+boots&lang=*")
        );
        // A relative template is resolved against the description.
        let rel = r#"<Url type="text/html" template="/search?q={searchTerms}"/>"#;
        assert_eq!(
            opensearch_template(rel, "https://x.test/os.xml").as_deref(),
            Some("https://x.test/search?q={searchTerms}")
        );
    }

    #[test]
    fn endpoints_follow_the_shape_of_the_site_and_end_with_the_generic_ones() {
        let e = endpoints(
            "https://blog.test/",
            r#"<link rel="https://api.w.org/" href="https://blog.test/wp-json/">"#,
            "boots",
        );
        assert_eq!(
            e[0].url,
            "https://blog.test/wp-json/wp/v2/search?search=boots"
        );
        assert!(e
            .iter()
            .any(|x| x.url == "https://blog.test/search?q=boots"));
        let n = endpoints("https://someone.substack.com/", "", "boots");
        assert!(
            n[0].url.contains("/api/v1/archive?sort=new&search=boots"),
            "{n:?}"
        );
        // Each URL once, however many shapes produce it.
        let mut urls: Vec<&str> = e.iter().map(|x| x.url.as_str()).collect();
        urls.dedup();
        assert_eq!(urls.len(), e.len());
    }

    #[test]
    fn a_page_answers_a_query_only_when_it_carries_it_and_is_not_the_start_page() {
        let home = "# Shop\nAll our boots and coats.";
        assert!(
            !answers_query(home, home, "boots"),
            "the home page served again"
        );
        assert!(answers_query("Results for boots: 3 found", home, "boots"));
        assert!(!answers_query("Nothing here", home, "boots"));
        assert!(answers_query("rain and boots", home, "rain boots"));
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
