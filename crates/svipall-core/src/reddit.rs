//! Reddit addresses that stopped answering a visitor without an account, and the public page
//! that holds the same thing.
//!
//! The `.json` twin of every page, the old interface and the feeds were closed to signed-out
//! visitors in 2026: the first answers 403 whatever asks, the second redirects to a login form,
//! the third is an empty feed. That is the site's decision and nothing here argues with it. The
//! ordinary page at `www` is still served to anyone, and it carries the same posts and comments
//! (`svipall_extract::content::post_elements` reads them), so an address of a closed kind is
//! read as the page a person would have opened instead.
//!
//! Pure: which address maps to which. The fetch path decides when to ask (never for a caller
//! who brought a signed-in profile, whose addresses still answer as written).

use regex::Regex;
use std::sync::LazyLock;
use url::Url;

/// The elements a page leaves where something it asks for later will go.
static PARTIAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<faceplate-partial\b[^>]*>").unwrap());
static SRC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)\bsrc="([^"]*)""#).unwrap());

/// Is this the element where a feed's next part goes? A feed marks it as what loads after the
/// last post; a page of search results names the search and where it left off.
fn continues(tag: &str, src: &str) -> bool {
    tag.contains("slot=\"load-after\"") || (src.contains("/search/") && src.contains("cursor="))
}
const POST_OPEN: &str = "<shreddit-post ";
/// One search result, which is not a post element: the link every layout of the results page
/// gives a result exactly once.
const RESULT_OPEN: &str = "data-testid=\"post-title\"";
/// A feed's first answer holds three posts and asks for the rest as soon as it is on screen.
/// Under this many, what arrived is that opening and not the page a reader sees.
const OPENING_POSTS: usize = 10;

const PUBLIC_HOST: &str = "www.reddit.com";
/// Asks a community's page for its rules in full (`post_elements::RULES_FRAGMENT`).
const RULES_FRAGMENT: &str = svipall_extract::content::post_elements::RULES_FRAGMENT;
/// Hosts that are the same site. Anything else under the domain (an image host, the status
/// page) is left alone.
const SAME_SITE: &[&str] = &[
    "reddit.com",
    "www.reddit.com",
    "old.reddit.com",
    "new.reddit.com",
    "np.reddit.com",
];
/// Endings that named a machine-readable twin of a page.
const CLOSED_SUFFIXES: &[&str] = &["/.json", ".json", "/.rss", ".rss", "/.xml", ".xml"];
/// Parameters that only meant something to the JSON answer.
const DATA_PARAMS: &[&str] = &["limit", "raw_json", "count"];

/// The public page for an address of a closed kind, or `None` when the address is not Reddit's
/// or is already that page.
pub fn public_page(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    if !SAME_SITE.contains(&host.as_str()) {
        return None;
    }
    let path = u.path().to_string();
    // What the page itself asks for while it loads is already the public page's own.
    if path.starts_with("/svc/") {
        return None;
    }
    let stripped = CLOSED_SUFFIXES
        .iter()
        .find_map(|s| path.strip_suffix(s))
        .map(|p| format!("{}/", p.trim_end_matches('/')));
    // A community's rules are shown beside its feed, and nowhere else to a signed-out visitor:
    // the page named `about/rules` is an empty frame its script fills for an account.
    let segments: Vec<&str> = stripped
        .as_deref()
        .unwrap_or(&path)
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if let ["r", name, "about", "rules"] = segments.as_slice() {
        u.set_path(&format!("/r/{name}/"));
        u.set_query(None);
        u.set_fragment(Some(RULES_FRAGMENT));
        u.set_scheme("https").ok()?;
        u.set_host(Some(PUBLIC_HOST)).ok()?;
        return Some(u.into());
    }
    if stripped.is_none() && host == PUBLIC_HOST && u.scheme() == "https" {
        return None;
    }
    if let Some(p) = &stripped {
        u.set_path(p);
        let kept: Vec<(String, String)> = u
            .query_pairs()
            .filter(|(k, _)| !DATA_PARAMS.contains(&k.as_ref()))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if kept.is_empty() {
            u.set_query(None);
        } else {
            u.query_pairs_mut().clear().extend_pairs(kept);
        }
    }
    u.set_scheme("https").ok()?;
    u.set_host(Some(PUBLIC_HOST)).ok()?;
    Some(u.into())
}

fn on_the_public_site(u: &Url) -> bool {
    u.scheme() == "https" && u.host_str() == Some(PUBLIC_HOST)
}

/// Is this a page of the public site? Its pages are answered at the first tier, so that tier is
/// asked first whatever an earlier refusal taught the ladder about the domain: one private
/// community answering 403 must not send every later read through a browser.
pub fn is_public_site(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| on_the_public_site(&u))
}

/// The address a feed asks for by itself once it is on screen, when the document is only the
/// feed's opening. A reader never sees the opening alone, so neither should a caller.
pub fn continuation(html: &str, url: &str) -> Option<String> {
    let page = Url::parse(url).ok()?;
    if !on_the_public_site(&page) || page.path().starts_with("/svc/") {
        return None;
    }
    let posts = html.matches(POST_OPEN).count() + html.matches(RESULT_OPEN).count();
    if posts >= OPENING_POSTS {
        return None;
    }
    // A search sometimes answers with no result at all and asks for every one of them; a page
    // with no post on it that is not a search is simply not a feed.
    let searching = page.path().trim_end_matches('/').ends_with("/search");
    if posts == 0 && !searching {
        return None;
    }
    let src = PARTIAL
        .find_iter(html)
        .filter_map(|tag| {
            let src = SRC.captures(tag.as_str())?.get(1)?.as_str();
            continues(tag.as_str(), src).then_some(src)
        })
        .last()?;
    let next = page.join(&src.replace("&amp;", "&")).ok()?;
    on_the_public_site(&next).then(|| next.into())
}

fn community_name(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The page a reader would have been on before asking for this one.
///
/// A feed's "more posts" address, a search, and the other addresses a page asks for as it runs
/// answer 403 to a visitor the site has not seen on one of its pages. That is a first visit, not
/// a wall: the community's own page is where a reader would have come from, and the front page
/// for anything that names no community. `None` for an address that is such a page already.
pub fn first_visit(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    if !on_the_public_site(&u) {
        return None;
    }
    let segments: Vec<&str> = u.path_segments()?.filter(|s| !s.is_empty()).collect();
    let named = u
        .query_pairs()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.into_owned());
    let community = match segments.as_slice() {
        ["svc", "shreddit", "r", name, ..] => Some((*name).to_string()),
        ["svc", ..] => named,
        // The community's page itself, whatever its sort: nowhere else to have come from.
        ["r", _] | ["r", _, "hot" | "new" | "top" | "rising" | "best"] | [] => return None,
        ["r", name, ..] => Some((*name).to_string()),
        _ => None,
    };
    Some(match community.filter(|n| community_name(n)) {
        Some(name) => format!("https://{PUBLIC_HOST}/r/{name}/"),
        None => format!("https://{PUBLIC_HOST}/"),
    })
}

#[cfg(test)]
mod tests {
    use super::{continuation, first_visit, public_page};

    fn opening(posts: usize) -> String {
        format!(
            "<html><body><shreddit-feed>{}<faceplate-partial slot=\"load-after\" \
             src=\"/svc/shreddit/community-more-posts/top/?t=WEEK&amp;after=abc%3D%3D&amp;name=rust\" \
             method=\"GET\"></faceplate-partial></shreddit-feed></body></html>",
            "<shreddit-post post-title=\"x\"></shreddit-post>".repeat(posts)
        )
    }

    #[test]
    fn a_feed_that_is_only_its_opening_names_what_it_asks_for_next() {
        assert_eq!(
            continuation(&opening(3), "https://www.reddit.com/r/rust/top/?t=week").as_deref(),
            Some("https://www.reddit.com/svc/shreddit/community-more-posts/top/?t=WEEK&after=abc%3D%3D&name=rust")
        );
    }

    #[test]
    fn a_full_page_a_thread_and_another_site_ask_for_nothing_more() {
        let url = "https://www.reddit.com/r/rust/top/?t=week";
        assert_eq!(continuation(&opening(25), url), None);
        assert_eq!(
            continuation("<html><body><p>nothing</p></body></html>", url),
            None
        );
        assert_eq!(
            continuation(&opening(3), "https://example.com/r/rust/"),
            None
        );
        // A continuation is not continued by itself: the caller asks for the next one.
        assert_eq!(
            continuation(
                &opening(3),
                "https://www.reddit.com/svc/shreddit/community-more-posts/top/?name=rust"
            ),
            None
        );
        let elsewhere = opening(3).replace("src=\"/svc/", "src=\"https://other.test/svc/");
        assert_eq!(continuation(&elsewhere, url), None);
    }

    #[test]
    fn only_the_public_site_is_the_public_site() {
        assert!(super::is_public_site("https://www.reddit.com/r/rust/"));
        assert!(!super::is_public_site("https://old.reddit.com/r/rust/"));
        assert!(!super::is_public_site(
            "https://www.reddit.com.evil.test/r/rust/"
        ));
        assert!(!super::is_public_site("http://www.reddit.com/r/rust/"));
    }

    #[test]
    fn a_page_of_search_results_names_its_next_part_too() {
        let html = format!(
            "<html><body>{}<faceplate-partial loading=\"lazy\" \
             src=\"/svc/shreddit/r/rust/search/?q=allocator&amp;sort=top&amp;cursor=abc\">\
             </faceplate-partial></body></html>",
            "<a data-testid=\"post-title\" href=\"/r/rust/comments/1/\"></a>".repeat(7)
        );
        assert_eq!(
            continuation(&html, "https://www.reddit.com/r/rust/search/?q=allocator&sort=top")
                .as_deref(),
            Some("https://www.reddit.com/svc/shreddit/r/rust/search/?q=allocator&sort=top&cursor=abc")
        );
    }

    #[test]
    fn a_search_that_answered_with_no_result_yet_still_asks_for_them() {
        let html = "<html><body><faceplate-partial loading=\"lazy\" \
                    src=\"/svc/shreddit/search/?q=allocator&amp;cursor=abc\"></faceplate-partial>\
                    </body></html>";
        assert_eq!(
            continuation(html, "https://www.reddit.com/search/?q=allocator").as_deref(),
            Some("https://www.reddit.com/svc/shreddit/search/?q=allocator&cursor=abc")
        );
        // The same element on a page that is not a search continues nothing.
        assert_eq!(continuation(html, "https://www.reddit.com/r/rust/"), None);
    }

    #[test]
    fn an_address_a_page_asks_for_is_first_visited_from_its_community() {
        let rust = Some("https://www.reddit.com/r/rust/");
        assert_eq!(
            first_visit("https://www.reddit.com/svc/shreddit/community-more-posts/top/?t=WEEK&after=abc%3D%3D&name=rust").as_deref(),
            rust
        );
        assert_eq!(
            first_visit("https://www.reddit.com/svc/shreddit/r/rust/search/?q=a&cursor=b")
                .as_deref(),
            rust
        );
        assert_eq!(
            first_visit("https://www.reddit.com/r/rust/search/?q=allocator").as_deref(),
            rust
        );
        assert_eq!(
            first_visit("https://www.reddit.com/r/rust/comments/1abc234/a_title/").as_deref(),
            rust
        );
        assert_eq!(
            first_visit("https://www.reddit.com/search/?q=allocator").as_deref(),
            Some("https://www.reddit.com/")
        );
        assert_eq!(
            first_visit(
                "https://www.reddit.com/svc/shreddit/community-more-posts/top/?name=../../x"
            )
            .as_deref(),
            Some("https://www.reddit.com/")
        );
    }

    #[test]
    fn a_community_page_and_another_site_have_nowhere_to_come_from() {
        assert_eq!(first_visit("https://www.reddit.com/r/rust/"), None);
        assert_eq!(
            first_visit("https://www.reddit.com/r/rust/top/?t=week"),
            None
        );
        assert_eq!(first_visit("https://www.reddit.com/"), None);
        assert_eq!(
            first_visit("https://example.com/svc/shreddit/community-more-posts/top/?name=rust"),
            None
        );
    }

    fn page(url: &str) -> Option<String> {
        public_page(url)
    }

    #[test]
    fn a_json_twin_is_read_as_its_page() {
        assert_eq!(
            page("https://www.reddit.com/r/rust/top.json?t=week&limit=100").as_deref(),
            Some("https://www.reddit.com/r/rust/top/?t=week")
        );
        assert_eq!(
            page("https://www.reddit.com/r/rust/comments/1abc234/a_title/.json").as_deref(),
            Some("https://www.reddit.com/r/rust/comments/1abc234/a_title/")
        );
        assert_eq!(
            page("https://www.reddit.com/r/rust/.json").as_deref(),
            Some("https://www.reddit.com/r/rust/")
        );
        assert_eq!(
            page("https://www.reddit.com/r/kobo/search.json?q=kindle%20books&restrict_sr=1&sort=top&raw_json=1").as_deref(),
            Some("https://www.reddit.com/r/kobo/search/?q=kindle+books&restrict_sr=1&sort=top")
        );
    }

    #[test]
    fn rules_are_read_where_the_site_shows_them() {
        let shown = Some("https://www.reddit.com/r/rust/#rules");
        assert_eq!(
            page("https://www.reddit.com/r/rust/about/rules.json").as_deref(),
            shown
        );
        assert_eq!(
            page("https://www.reddit.com/r/rust/about/rules/").as_deref(),
            shown
        );
        assert_eq!(
            page("https://old.reddit.com/r/rust/about/rules").as_deref(),
            shown
        );
        assert_eq!(page("https://www.reddit.com/r/rust/#rules"), None);
    }

    #[test]
    fn a_feed_is_read_as_its_page() {
        assert_eq!(
            page("https://www.reddit.com/r/rust/top/.rss?t=week").as_deref(),
            Some("https://www.reddit.com/r/rust/top/?t=week")
        );
        assert_eq!(
            page("https://www.reddit.com/r/rust.rss").as_deref(),
            Some("https://www.reddit.com/r/rust/")
        );
    }

    #[test]
    fn the_other_interfaces_are_the_same_site() {
        assert_eq!(
            page("https://old.reddit.com/r/rust/top/?t=week").as_deref(),
            Some("https://www.reddit.com/r/rust/top/?t=week")
        );
        assert_eq!(
            page("http://np.reddit.com/r/rust/comments/1abc234/a_title/").as_deref(),
            Some("https://www.reddit.com/r/rust/comments/1abc234/a_title/")
        );
        assert_eq!(
            page("https://reddit.com/r/rust").as_deref(),
            Some("https://www.reddit.com/r/rust")
        );
    }

    #[test]
    fn the_public_page_is_left_as_it_is() {
        assert_eq!(page("https://www.reddit.com/r/rust/top/?t=week"), None);
        assert_eq!(
            page("https://www.reddit.com/r/rust/comments/1abc234/a_title/"),
            None
        );
        // What the page asks for as it loads, a feed's next page among them.
        assert_eq!(
            page("https://www.reddit.com/svc/shreddit/community-more-posts/top/?t=WEEK&name=rust"),
            None
        );
    }

    #[test]
    fn nothing_that_is_not_the_site_is_touched() {
        assert_eq!(page("https://example.com/data.json"), None);
        assert_eq!(page("https://i.redd.it/abc.json"), None);
        assert_eq!(page("https://preview.reddit.com/x.json"), None);
        assert_eq!(page("https://notreddit.com/r/rust.json"), None);
        assert_eq!(page("file:///c/reddit.com/x.json"), None);
        assert_eq!(page("not a url"), None);
    }
}
