//! Dropping the third of every page nobody asked for.
//!
//! A news article is maybe forty kilobytes of text and half a megabyte of advertising, analytics,
//! consent frames and session recorders. Fetching all of it costs time on every page of a crawl,
//! and each of those third parties is another script running its own fingerprinting inside a
//! browser that has been carefully made to look ordinary.
//!
//! So: known advertising and tracking hosts are refused at the network layer, and consent banners
//! are hidden before extraction runs. Both lists are the ones the ad-blocking world already
//! maintains, in the two formats they are published in.
//!
//! The list is **downloaded once and cached**. That is a deliberate exception to "everything is
//! local", and it comes with a hard rule that is tested rather than assumed: with no network and no
//! cache, this returns an empty list and everything keeps working. A tool that will not start
//! because it could not reach a list has traded a small saving for its whole promise.

use std::collections::HashSet;

/// Hosts to refuse, and the rules for hiding what is left.
#[derive(Debug, Clone, Default)]
pub struct Blocklist {
    domains: HashSet<String>,
}

impl Blocklist {
    /// Nothing blocked. What an offline machine with no cache gets, and it must work.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.domains.len()
    }

    pub fn is_empty(&self) -> bool {
        self.domains.is_empty()
    }

    /// Read either published format, and skip whatever is neither.
    ///
    /// One parser for both because a list is identified by its contents, not by where it came
    /// from, and an operator who points the config at the other kind should not have to care.
    pub fn parse(text: &str) -> Self {
        let mut domains = HashSet::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("||") {
                // `||host^$third-party` and friends. A rule that carries a path or a wildcard is an
                // element rule, not a host rule: `||site.example/ads/banner.png` hides one image,
                // and reading it as a host rule takes the whole site down.
                let end = rest.find(['^', '$', '/', '*']).unwrap_or(rest.len());
                if matches!(rest.as_bytes().get(end), Some(b'/') | Some(b'*')) {
                    continue;
                }
                let host = rest[..end].trim().to_ascii_lowercase();
                if is_host(&host) {
                    domains.insert(host);
                }
                continue;
            }
            // Hosts format: an address, whitespace, then one or more names.
            let mut parts = line.split_whitespace();
            let first = parts.next().unwrap_or("");
            if first == "0.0.0.0" || first == "127.0.0.1" || first == "::1" {
                for name in parts {
                    let name = name.trim().to_ascii_lowercase();
                    // `0.0.0.0 localhost` appears in every hosts file ever written.
                    if is_host(&name) && name != "localhost" && !name.starts_with('#') {
                        domains.insert(name);
                    }
                }
            }
        }
        Self { domains }
    }

    /// Should this request be refused?
    ///
    /// A blocked domain blocks its subdomains too: lists name `doubleclick.net`, and what a page
    /// actually loads is `stats.g.doubleclick.net`. Matching only the exact name blocks almost
    /// nothing while looking like it works.
    pub fn blocks(&self, url: &str) -> bool {
        let Some(host) = host_of(url) else {
            return false;
        };
        let mut rest = host.as_str();
        loop {
            if self.domains.contains(rest) {
                return true;
            }
            match rest.split_once('.') {
                // Stop before a bare suffix: a list containing "net" would take the web with it.
                Some((_, tail)) if tail.contains('.') => rest = tail,
                _ => return false,
            }
        }
    }

    /// Fold another list in. Two lists cover more than either and cost one set union.
    pub fn merge(&mut self, other: Blocklist) {
        self.domains.extend(other.domains);
    }

    /// URL patterns for the browser, most general first, capped.
    ///
    /// The cap is not a nicety. A published list is a hundred thousand names, and handing that many
    /// glob patterns to a browser costs more per request than the requests they save. Ad lists are
    /// long-tailed, so the general names — the ones with fewest labels — are the ones that block
    /// most of the traffic; taking those first gets most of the saving for a fraction of the cost.
    ///
    /// Ordering is by shape and then by name, never by hash order: a browser configured differently
    /// on every run is a browser whose failures cannot be reproduced.
    pub fn patterns(&self, cap: usize) -> Vec<String> {
        let mut names: Vec<&String> = self.domains.iter().collect();
        names.sort_by(|a, b| {
            let labels = |s: &str| s.matches('.').count();
            labels(a)
                .cmp(&labels(b))
                .then_with(|| a.len().cmp(&b.len()))
                .then_with(|| a.cmp(b))
        });
        names
            .into_iter()
            .take(cap)
            .flat_map(|d| [format!("*://{d}/*"), format!("*://*.{d}/*")])
            .collect()
    }
}

fn is_host(s: &str) -> bool {
    !s.is_empty()
        && s.contains('.')
        && s.len() < 254
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn host_of(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url)
        .trim_start_matches("//");
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    is_host(&host).then_some(host)
}

/// Hide the consent banner rather than answering it.
///
/// Clicking "accept" would be answering on the operator's behalf, and the interesting part is that
/// it is not necessary: the article underneath is already in the DOM, and the banner is a fixed
/// overlay above it. Removing the overlay and restoring scrolling is enough, and it takes no
/// position on anything.
pub const HIDE_CONSENT_JS: &str = r#"(() => {
    const words = ['cookie', 'consent', 'gdpr', 'privacy-banner', 'cmp-', 'didomi', 'onetrust'];
    let hidden = 0;
    for (const el of document.querySelectorAll('div, section, aside, dialog, iframe')) {
        const id = ((el.id || '') + ' ' + (el.className || '')).toLowerCase();
        if (typeof id !== 'string' || !words.some(w => id.includes(w))) continue;
        const s = getComputedStyle(el);
        // Only overlays. A banner that scrolls with the page is part of the page.
        if (s.position !== 'fixed' && s.position !== 'sticky') continue;
        const r = el.getBoundingClientRect();
        if (r.width * r.height < 10000) continue;
        el.remove();
        hidden++;
    }
    // Banners lock the body while they are up, and removing one without this leaves a page that
    // cannot be scrolled, which breaks every screenshot and every lazy-loaded image below.
    if (hidden) {
        document.documentElement.style.overflow = '';
        document.body.style.overflow = '';
        document.body.style.position = '';
    }
    return hidden;
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_with_no_list_blocks_nothing_and_keeps_working() {
        // The hard rule. Refusing to start because a list could not be fetched trades a small
        // saving for the whole promise of the tool.
        let b = Blocklist::empty();
        assert!(b.is_empty());
        assert!(!b.blocks("https://ads.example.com/track.js"));
        assert!(!b.blocks("https://news.example.com/article"));
    }

    #[test]
    fn both_published_formats_are_read_by_the_same_parser() {
        let hosts = Blocklist::parse("# a comment\n0.0.0.0 ads.example\n127.0.0.1 track.example\n");
        assert!(hosts.blocks("https://ads.example/x.js"));
        assert!(hosts.blocks("http://track.example/"));

        let rules = Blocklist::parse("! a comment\n||ads.example^\n||track.example^$third-party\n");
        assert!(rules.blocks("https://ads.example/x.js"));
        assert!(rules.blocks("https://track.example/beacon"));
    }

    #[test]
    fn blocking_a_domain_blocks_what_pages_actually_load_from_it() {
        // Lists name `doubleclick.net`; pages load `stats.g.doubleclick.net`. Exact matching blocks
        // almost nothing while appearing to work.
        let b = Blocklist::parse("0.0.0.0 tracker.example");
        assert!(b.blocks("https://tracker.example/"));
        assert!(b.blocks("https://a.b.c.tracker.example/pixel.gif"));
        assert!(!b.blocks("https://nottracker.example/"));
        assert!(!b.blocks("https://tracker.example.org/"));
    }

    #[test]
    fn a_suffix_on_its_own_never_takes_the_web_down_with_it() {
        // Walking up the labels has to stop before the public suffix, or one bad line in a list
        // blocks every site there is.
        let b = Blocklist::parse("0.0.0.0 example.com");
        assert!(b.blocks("https://x.example.com/"));
        assert!(!b.blocks("https://other.com/"));
        // Even if a list somehow contains a bare suffix, it can never be reached by walking up.
        let bad = Blocklist::parse("||com^");
        assert!(!bad.blocks("https://anything.com/"));
    }

    #[test]
    fn element_rules_never_take_a_whole_host_with_them() {
        // `||site.example/ads/banner.png` hides one image. Reading it as a host rule blocks the
        // site.
        let b = Blocklist::parse("||site.example/ads/banner.png\n||other.example^");
        assert!(!b.blocks("https://site.example/article"));
        assert!(b.blocks("https://other.example/"));
    }

    #[test]
    fn the_lines_every_hosts_file_carries_are_not_mistaken_for_rules() {
        let b = Blocklist::parse(
            "# hosts\n127.0.0.1 localhost\n::1 localhost\n255.255.255.255 broadcasthost\n",
        );
        assert!(b.is_empty(), "{b:?}");
    }

    #[test]
    fn two_lists_together_cover_what_either_covers() {
        let mut b = Blocklist::parse("0.0.0.0 one.example");
        b.merge(Blocklist::parse("||two.example^"));
        assert!(b.blocks("https://one.example/"));
        assert!(b.blocks("https://two.example/"));
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn a_url_with_a_port_or_credentials_is_still_matched_on_its_host() {
        let b = Blocklist::parse("0.0.0.0 ads.example");
        assert!(b.blocks("https://ads.example:8443/x"));
        assert!(b.blocks("https://user:pw@ads.example/x"));
        assert!(b.blocks("//ads.example/x.js"), "protocol-relative");
    }

    #[test]
    fn something_that_is_not_a_url_is_not_blocked_and_does_not_panic() {
        let b = Blocklist::parse("0.0.0.0 ads.example");
        assert!(!b.blocks(""));
        assert!(!b.blocks("data:image/png;base64,iVBOR"));
        assert!(!b.blocks("about:blank"));
        assert!(!b.blocks("javascript:void(0)"));
    }

    #[test]
    fn garbage_in_a_list_is_skipped_rather_than_taken_as_a_rule() {
        // One bad line must not become a rule that blocks something real.
        let b = Blocklist::parse("not a rule\n0.0.0.0\n||\n||^\n0.0.0.0 ok.example\n");
        assert_eq!(b.len(), 1);
        assert!(b.blocks("https://ok.example/"));
    }

    #[test]
    fn the_patterns_handed_to_a_browser_are_the_same_two_runs_running() {
        // Hash order would configure the browser differently on every run, and a failure that
        // cannot be reproduced is a failure nobody can fix.
        let b = Blocklist::parse(
            "0.0.0.0 c.example
0.0.0.0 a.example
0.0.0.0 b.example
",
        );
        assert_eq!(b.patterns(10), b.patterns(10));
        assert!(b.patterns(10)[0].contains("a.example"));
    }

    #[test]
    fn the_most_general_names_are_kept_when_the_list_is_capped() {
        // A published list is a hundred thousand names; handing them all to a browser costs more
        // than the requests it saves. Ad lists are long-tailed, so the short names carry the
        // traffic.
        let b = Blocklist::parse(
            "0.0.0.0 deep.sub.tracker.example
0.0.0.0 ads.example
0.0.0.0 sub.other.example
",
        );
        let two = b.patterns(1);
        assert!(two.iter().any(|p| p.contains("ads.example")), "{two:?}");
        assert!(!two.iter().any(|p| p.contains("deep.sub")), "{two:?}");
    }

    #[test]
    fn a_pattern_covers_the_name_itself_and_everything_under_it() {
        let b = Blocklist::parse("0.0.0.0 ads.example");
        let p = b.patterns(10);
        assert!(p.contains(&"*://ads.example/*".to_string()), "{p:?}");
        assert!(p.contains(&"*://*.ads.example/*".to_string()), "{p:?}");
    }

    #[test]
    fn the_consent_script_removes_the_overlay_and_gives_the_page_back_its_scrolling() {
        // Removing the banner without this leaves a page that cannot scroll, which breaks every
        // screenshot and every image below the fold.
        assert!(HIDE_CONSENT_JS.contains("style.overflow = ''"));
        assert!(
            HIDE_CONSENT_JS.contains("s.position !== 'fixed'"),
            "only overlays; a banner that scrolls with the page is part of the page"
        );
    }
}
