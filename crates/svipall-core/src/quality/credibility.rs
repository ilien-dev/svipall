//! What can be *observed* about where a page came from.
//!
//! ▲ Read the module name as a warning rather than a promise. Nothing here scores a page, ranks
//! one, reorders a result set or subtracts from a verdict. Every field is an observation a caller
//! could have made by reading the page themselves, collected in one place because the fetch has
//! already parsed the document and they have not.
//!
//! The W3C Credible Web Community Group spent two years cataloguing signals of this kind and
//! published the finding that matters more than the catalogue: acting on them produces "a bias
//! towards larger, professional news organisations", because an outlet with a masthead, a style
//! guide and an archive going back a decade emits all of them and a specialist writing carefully
//! under a pseudonym emits none. A page with no byline and no date can be the best source on its
//! subject. So these are annotations, and the caller is the one who gets to weigh them.
//!
//! What is here is what the single parse already holds:
//!
//! | observation | where it comes from | what it does *not* mean |
//! |---|---|---|
//! | a named author | `<meta name=author>`, Open Graph, JSON-LD | that the name is real |
//! | a publication date | `article:published_time`, JSON-LD `datePublished` | that the page was written then |
//! | outbound citations | anchors to other registrable domains | that the sources say what is claimed |
//! | site first seen here | `MIN(fetched_at)` over this cache | when the site was created |
//!
//! The last one is the only one a stateless extractor could not produce, and it is bounded by the
//! cache's own retention: "first seen here" is a fact about this machine, not about the web.

use serde::{Deserialize, Serialize};

/// Observations about a page's provenance. Every field optional, because absent and zero are
/// different statements and only one of them is an accusation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Credibility {
    /// A byline the page put its own name to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// A publication date the page declares, verbatim: it is not parsed into a timestamp, because
    /// a normalised date would look verified and none of these are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// Anchors leaving this registrable domain, de-duplicated by host.
    ///
    /// Counted rather than judged. A page citing twelve other sites may be a well-sourced report
    /// or a link farm; `Optimization` is what says which, and it says it separately.
    pub outbound_citations: usize,
    /// Distinct hosts among them, which is what separates twelve references from twelve links to
    /// one shop.
    pub cited_hosts: usize,
    /// When this machine first fetched anything from this domain, in epoch seconds. `None` means
    /// no cache was consulted, or the site is new here — the caller is told which by the field's
    /// companion in the response, never by a silent zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_first_seen: Option<i64>,
}

impl Credibility {
    /// Is there anything here worth the tokens?
    ///
    /// An empty observation set is not reported at all, for the same reason `quality_reasons` is
    /// absent when there is nothing to say: a response full of nulls reads as a set of findings.
    pub fn is_empty(&self) -> bool {
        self.author.is_none()
            && self.published.is_none()
            && self.outbound_citations == 0
            && self.site_first_seen.is_none()
    }
}

/// The registrable-ish host of a URL, lowercased, without a leading `www.`.
///
/// "-ish" on purpose: a public-suffix list is a downloaded, dated artefact and this crate does not
/// fetch anything. Two hosts that differ only in subdomain therefore count as two, which
/// over-counts a site that publishes across `blog.` and `docs.`. That direction is the safe one —
/// the alternative is silently merging genuinely different sources.
fn host_of(url: &str) -> Option<String> {
    // A scheme with no authority has no host, and `mailto:someone@a.test` would otherwise read as
    // a citation of `a.test`. Only the two schemes that fetch a page count.
    let (scheme, after) = url.split_once("//")?;
    if !matches!(scheme, "" | "http:" | "https:") {
        return None;
    }
    let host = after
        .split(['/', '?', '#'])
        .next()?
        .split('@')
        .next_back()?
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

/// Read a page's own claims about itself.
///
/// `first_seen` is passed in rather than looked up: this crate does not open databases, and the
/// caller that has a cache is the one that can answer it.
pub fn observe(
    meta: Option<&crate::extraction::Metadata>,
    links: Option<&crate::extraction::Links>,
    final_url: &str,
    first_seen: Option<i64>,
) -> Credibility {
    let mut out = Credibility {
        site_first_seen: first_seen,
        ..Default::default()
    };

    if let Some(m) = meta {
        // Blank and whitespace are the same as absent. A `<meta name="author" content="">` is a
        // template that was never filled in, and reporting it as a byline would be reporting the
        // template.
        out.author = m
            .author
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        out.published = m
            .published
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }

    if let Some(l) = links {
        let own = host_of(final_url);
        let mut hosts: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut count = 0usize;
        // `Links::external` is already the set that left the site, but it is computed against the
        // requested URL and a redirect can move what "the site" means. Re-checking against the
        // final host costs one comparison and cannot be wrong in the direction that matters.
        for link in &l.external {
            let Some(h) = host_of(&link.url) else {
                continue;
            };
            if Some(&h) == own.as_ref() {
                continue;
            }
            count += 1;
            hosts.insert(h);
        }
        out.outbound_citations = count;
        out.cited_hosts = hosts.len();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::{Link, Links, Metadata};

    fn link(url: &str) -> Link {
        Link {
            url: url.into(),
            text: String::new(),
            nofollow: false,
        }
    }

    fn links(external: &[&str]) -> Links {
        Links {
            internal: Vec::new(),
            external: external.iter().map(|u| link(u)).collect(),
            media: Vec::new(),
        }
    }

    #[test]
    fn a_page_that_says_nothing_about_itself_produces_nothing_to_report() {
        let c = observe(None, None, "https://a.test/x", None);
        assert!(c.is_empty(), "{c:?}");
    }

    /// An unfilled template field is not a byline. Reporting it as one would turn a site's
    /// boilerplate into an observation about the page.
    #[test]
    fn a_blank_byline_is_absent_rather_than_present_and_empty() {
        let meta = Metadata {
            author: Some("   ".into()),
            published: Some(String::new()),
            ..Default::default()
        };
        let c = observe(Some(&meta), None, "https://a.test/x", None);
        assert_eq!(c.author, None);
        assert_eq!(c.published, None);
    }

    #[test]
    fn citations_are_counted_by_link_and_by_distinct_host() {
        let l = links(&[
            "https://source.test/a",
            "https://source.test/b",
            "https://other.test/c",
        ]);
        let c = observe(None, Some(&l), "https://mine.test/x", None);
        assert_eq!(c.outbound_citations, 3);
        assert_eq!(
            c.cited_hosts, 2,
            "three links to two sites is two sources, and the difference is the point"
        );
    }

    /// ▲ A redirect can move what "this site" means, and the link split was decided before it
    /// happened. Twelve links back to the page's own host are navigation, not sourcing.
    #[test]
    fn a_link_home_after_a_redirect_is_not_a_citation() {
        let l = links(&[
            "https://www.mine.test/other",
            "https://mine.test:443/again",
            "https://elsewhere.test/real",
        ]);
        let c = observe(None, Some(&l), "https://mine.test/x", None);
        assert_eq!(c.outbound_citations, 1);
        assert_eq!(c.cited_hosts, 1);
    }

    #[test]
    fn a_date_is_carried_verbatim_because_normalising_it_would_look_like_verifying_it() {
        let meta = Metadata {
            published: Some("  yesterday, probably  ".into()),
            ..Default::default()
        };
        let c = observe(Some(&meta), None, "https://a.test/x", None);
        assert_eq!(c.published.as_deref(), Some("yesterday, probably"));
    }

    #[test]
    fn a_hostless_or_bare_address_is_no_host_at_all() {
        assert_eq!(host_of("mailto:someone@a.test"), None);
        assert_eq!(host_of("https://localhost/x"), None);
        assert_eq!(host_of("/relative/path"), None);
        assert_eq!(host_of("https://WWW.A.TEST/x").as_deref(), Some("a.test"));
    }

    /// The one observation a stateless extractor cannot make, and the reason it is bounded: it is
    /// when this machine first saw the site, never when the site began.
    #[test]
    fn first_seen_is_carried_through_untouched() {
        let c = observe(None, None, "https://a.test/x", Some(1_700_000_000));
        assert_eq!(c.site_first_seen, Some(1_700_000_000));
        assert!(!c.is_empty());
    }
}
