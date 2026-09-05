//! The two target sets the evasion benchmark can run, and the verdict rule each one uses.
//!
//! `hard12` is svipall's own list: twelve sites chosen because they have walls, scored by whether
//! the expected text came back and no wall was reported. It measures walls.
//!
//! `public31` is the list an independent benchmark published in May 2026 (seven stealth tools,
//! three sweeps, 651 verdicts), scored by that benchmark's own four-way rule. Twenty-five of its
//! thirty-one targets pass for every tool including unpatched automation; the signal lives in six
//! cells. It measures what the competition measures, which is a different thing, and the two
//! numbers must never be read as one.
//!
//! The verdict rule for `public31` is copied verbatim, vendor words included, because a verdict
//! that differs by one pattern is not comparable. That is the one place this workspace names a
//! vendor on purpose.

use regex::Regex;
use std::sync::LazyLock;

pub struct Target {
    pub name: &'static str,
    pub url: &'static str,
    /// Which kind of wall is known to sit in front of it, or `none`.
    pub wall: &'static str,
    /// Strings that only appear once the real content came back. Empty for `public31`, which is
    /// scored by verdict rather than by expected text.
    pub expect: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Set {
    Hard12,
    Public31,
    Vendors8,
}

impl Set {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "hard12" => Some(Self::Hard12),
            "public31" => Some(Self::Public31),
            "vendors8" => Some(Self::Vendors8),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Hard12 => "hard12",
            Self::Public31 => "public31",
            Self::Vendors8 => "vendors8",
        }
    }

    pub fn targets(self) -> &'static [Target] {
        match self {
            Self::Hard12 => HARD12,
            Self::Public31 => PUBLIC31,
            Self::Vendors8 => VENDORS8,
        }
    }
}

macro_rules! t {
    ($name:literal, $url:literal, $wall:literal, [$($e:literal),*]) => {
        Target { name: $name, url: $url, wall: $wall, expect: &[$($e),*] }
    };
}

pub const HARD12: &[Target] = &[
    t!("example", "https://example.com", "none", ["Example Domain"]),
    t!(
        "hackernews",
        "https://news.ycombinator.com",
        "none",
        ["comments", "points"]
    ),
    t!(
        "wikipedia",
        "https://en.wikipedia.org/wiki/Rust_(programming_language)",
        "none",
        ["memory safety"]
    ),
    t!(
        "nowsecure",
        "https://nowsecure.nl/",
        "turnstile",
        ["NOWSECURE", "OH YEAH"]
    ),
    t!(
        "g2",
        "https://www.g2.com/products/notion/reviews",
        "captcha-delivery.com",
        ["Notion"]
    ),
    t!(
        "idealista",
        "https://www.idealista.com/venta-viviendas/madrid-madrid/",
        "captcha-delivery.com",
        ["Madrid"]
    ),
    t!(
        "amazon",
        "https://www.amazon.com/s?k=echo+dot",
        "popular",
        ["Echo"]
    ),
    t!(
        "newegg",
        "https://www.newegg.com/p/pl?d=gpu",
        "bot-manager",
        ["GeForce", "Radeon"]
    ),
    t!(
        "zillow",
        "https://www.zillow.com/homes/for_sale/",
        "press-and-hold",
        ["homes"]
    ),
    t!(
        "crunchbase",
        "https://www.crunchbase.com/organization/anthropic",
        "managed-challenge",
        ["Anthropic"]
    ),
    t!(
        "indeed",
        "https://www.indeed.com/q-software-engineer-jobs.html",
        "managed-challenge",
        ["jobs"]
    ),
    t!(
        "stackoverflow",
        "https://stackoverflow.com/questions/tagged/rust",
        "managed-challenge",
        ["questions"]
    ),
];

/// The public list, in the order it was published. Four categories: fingerprint panels, TLS
/// endpoints, production sites with a known wall, and high-traffic content sites.
pub const PUBLIC31: &[Target] = &[
    t!("sannysoft", "https://bot.sannysoft.com/", "panel", []),
    t!(
        "creepjs",
        "https://abrahamjuliot.github.io/creepjs/",
        "panel",
        []
    ),
    t!(
        "browserleaks",
        "https://browserleaks.com/javascript",
        "panel",
        []
    ),
    t!(
        "browserscan-bot",
        "https://www.browserscan.net/bot-detection",
        "panel",
        []
    ),
    t!(
        "pixelscan-bot",
        "https://pixelscan.net/bot-check",
        "panel",
        []
    ),
    t!(
        "pixelscan-fp",
        "https://pixelscan.net/fingerprint-check",
        "panel",
        []
    ),
    t!("tls-peet", "https://tls.peet.ws/api/all", "tls", []),
    t!(
        "browserleaks-tls",
        "https://tls.browserleaks.com/json",
        "tls",
        []
    ),
    t!(
        "bot-incolumitas",
        "https://bot.incolumitas.com/",
        "panel",
        []
    ),
    t!(
        "rebrowser-detector",
        "https://bot-detector.rebrowser.net/",
        "panel",
        []
    ),
    t!("nowsecure-cf", "https://nowsecure.nl/", "turnstile", []),
    t!(
        "crunchbase-cf",
        "https://www.crunchbase.com/",
        "managed-challenge",
        []
    ),
    t!(
        "canadianinsider",
        "https://www.canadianinsider.com/",
        "turnstile",
        []
    ),
    t!(
        "sedarplus",
        "https://www.sedarplus.ca/landingpage/",
        "waf",
        []
    ),
    t!("ceo-ca", "https://ceo.ca/scan", "content", []),
    t!(
        "stockwatch",
        "https://www.stockwatch.com/News/Search?hours=24&region=C",
        "content",
        []
    ),
    t!(
        "newsfilecorp",
        "https://www.newsfilecorp.com/search?q=gold",
        "content",
        []
    ),
    t!("medium", "https://medium.com/", "managed-challenge", []),
    t!("devto", "https://dev.to/", "content", []),
    t!("reddit", "https://www.reddit.com/", "content", []),
    t!(
        "github-explore",
        "https://github.com/explore",
        "content",
        []
    ),
    t!(
        "linkedin-jobs",
        "https://www.linkedin.com/jobs/search?keywords=engineer",
        "content",
        []
    ),
    t!(
        "amazon-product",
        "https://www.amazon.com/dp/B08N5WRWNW",
        "popular",
        []
    ),
    t!(
        "google-search",
        "https://www.google.com/search?q=anti-detect+browser",
        "popular",
        []
    ),
    t!(
        "instagram-post",
        "https://www.instagram.com/p/CwgPe2erjLY/",
        "popular",
        []
    ),
    t!(
        "tiktok-user",
        "https://www.tiktok.com/@tiktok",
        "popular",
        []
    ),
    t!(
        "booking-search",
        "https://www.booking.com/searchresults.html?ss=Victoria%2C+BC",
        "popular",
        []
    ),
    t!("x-explore", "https://x.com/explore", "popular", []),
    t!(
        "indeed-jobs",
        "https://www.indeed.com/jobs?q=engineer",
        "managed-challenge",
        []
    ),
    t!(
        "stackoverflow",
        "https://stackoverflow.com/questions/tagged/python",
        "managed-challenge",
        []
    ),
    t!(
        "glassdoor",
        "https://www.glassdoor.com/Reviews/index.htm",
        "captcha-delivery.com",
        []
    ),
];

/// Two targets each behind the four vendors that decide a modern block, scored by expected text
/// the way `hard12` is.
///
/// A third list rather than an extension of `hard12`, because `hard12`'s number only means anything
/// against `hard12`'s list: changing the list changes what the number is comparable to. This one
/// exists to answer a different question — how svipall does against each *vendor*, named — and it
/// is expected to fail more often than `hard12` does, which is the point of publishing it.
///
/// Kasada is the one svipall had never measured. It offers no visible challenge: it scores the
/// first request, then requires a cryptographic proof-of-work whose token (`x-kpsdk-ct`) expires in
/// 60–180 seconds and must be re-earned continuously. An HTTP client cannot do that at all; the
/// warm tier holds a live browser, which structurally can.
pub const VENDORS8: &[Target] = &[
    // Kasada: no challenge page, a 403/429 and `x-kpsdk-*` headers.
    t!(
        "kasada-hyatt",
        "https://www.hyatt.com/",
        "kasada",
        ["Hyatt"]
    ),
    t!(
        "kasada-twitch",
        "https://www.twitch.tv/directory",
        "kasada",
        ["Twitch"]
    ),
    // Akamai Bot Manager: `_abck` / `bm_sz`, a sensor POST, edge scoring.
    t!(
        "akamai-newegg",
        "https://www.newegg.com/p/pl?d=ssd",
        "akamai",
        ["Newegg"]
    ),
    t!(
        "akamai-homedepot",
        "https://www.homedepot.com/b/Appliances/N-5yc1vZbv1w",
        "akamai",
        ["Home Depot"]
    ),
    // The fingerprinting vendor whose verdict lives in the top document.
    t!(
        "datadome-g2",
        "https://www.g2.com/products/notion/reviews",
        "captcha-delivery.com",
        ["Notion"]
    ),
    t!(
        "datadome-idealista",
        "https://www.idealista.com/venta-viviendas/madrid-madrid/",
        "captcha-delivery.com",
        ["Madrid"]
    ),
    // Cloudflare's managed challenge, which is a wait rather than a puzzle.
    t!(
        "cloudflare-crunchbase",
        "https://www.crunchbase.com/organization/openai",
        "managed-challenge",
        ["OpenAI"]
    ),
    t!(
        "cloudflare-indeed",
        "https://www.indeed.com/q-rust-developer-jobs.html",
        "managed-challenge",
        ["job"]
    ),
];

/// The public benchmark's four-way verdict. Order of severity for "best of N attempts" is
/// `Ok < Gated < Blocked < Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Ok,
    Gated,
    Blocked,
    Error,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Gated => "gated",
            Self::Blocked => "blocked",
            Self::Error => "error",
        }
    }
}

const BLOCKED_STATUSES: &[u16] = &[403, 406, 429, 451, 503];

static GATE_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    // Pieces joined by `concat!`: a raw string cannot continue a line with `\`, and a normal
    // string that did would swallow the indentation into the pattern.
    Regex::new(concat!(
        "(?i)(just a moment|attention required|access denied|cloudflare|verifying|",
        "performing security verification|support id|please verify|are you a human|robot check|",
        "captcha|radware block page|shieldsquare|access to this page has been denied|",
        "security check|incapsula|imperva|^error$|blocked by|checking your browser)"
    ))
    .expect("gate title pattern")
});

static GATE_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        "(?is)(your support id is:|enable javascript and cookies to continue|cf-error-details|",
        "cdn-cgi/challenge-platform|/cdn-cgi/bm/cv/|data:image/svg.*cloudflare|shieldsquare|",
        r"perfdrive\.com|aperture\.js|ssjsinternal|_incapsula_resource|datadome|px-cdn\.net|/_px/|",
        r"akamaitechnologies\.com.{0,200}block|window\.location\.reload.{0,100}cookie)"
    ))
    .expect("gate body pattern")
});

/// Verbatim port of the public benchmark's `_classify`, over the final status, the title and the
/// rendered HTML. `body` is scanned to 50 KB, as there.
pub fn public_verdict(status: Option<u16>, title: &str, body: &str) -> Verdict {
    if status.is_none() && title.is_empty() && body.is_empty() {
        return Verdict::Error;
    }
    if let Some(s) = status {
        if BLOCKED_STATUSES.contains(&s) {
            return Verdict::Blocked;
        }
    }
    if GATE_TITLE.is_match(title) {
        return Verdict::Gated;
    }
    let head = &body[..body.len().min(50_000)];
    // Slicing a UTF-8 string at a byte offset can land inside a character; back off to a boundary.
    let mut end = head.len();
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    if GATE_BODY.is_match(&body[..end]) {
        return Verdict::Gated;
    }
    if !body.is_empty() && body.len() < 800 && !body.to_ascii_lowercase().contains("<script") {
        return Verdict::Gated;
    }
    Verdict::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_rule_reads_a_challenge_title_as_gated_not_blocked() {
        assert_eq!(
            public_verdict(
                Some(200),
                "Just a moment...",
                "<html><script></script></html>"
            ),
            Verdict::Gated
        );
    }

    #[test]
    fn the_public_rule_reads_a_403_as_blocked_whatever_the_body_says() {
        assert_eq!(
            public_verdict(Some(403), "Welcome", &"x".repeat(5000)),
            Verdict::Blocked
        );
    }

    #[test]
    fn a_tiny_body_without_script_is_a_gate() {
        assert_eq!(
            public_verdict(Some(200), "Site", "<p>redirecting</p>"),
            Verdict::Gated
        );
        let real = format!("<html><script>x</script>{}</html>", "words ".repeat(300));
        assert_eq!(public_verdict(Some(200), "Site", &real), Verdict::Ok);
    }

    #[test]
    fn nothing_captured_at_all_is_an_error() {
        assert_eq!(public_verdict(None, "", ""), Verdict::Error);
    }

    #[test]
    fn a_vendor_signal_deep_in_the_body_still_counts() {
        let body = format!(
            "<html><script>a</script>{}<script src=\"/cdn-cgi/challenge-platform/h/b\"></script></html>",
            "filler ".repeat(1000)
        );
        assert_eq!(
            public_verdict(Some(200), "Real title", &body),
            Verdict::Gated
        );
    }

    #[test]
    fn a_multibyte_body_does_not_panic_at_the_scan_limit() {
        let body = "é".repeat(60_000);
        let _ = public_verdict(Some(200), "t", &body);
    }

    #[test]
    fn both_sets_have_unique_names_and_the_sizes_their_names_promise() {
        for (set, n) in [(Set::Hard12, 12), (Set::Public31, 31), (Set::Vendors8, 8)] {
            let t = set.targets();
            assert_eq!(t.len(), n, "{}", set.name());
            let mut names: Vec<_> = t.iter().map(|t| t.name).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), n, "duplicate name in {}", set.name());
        }
        assert!(HARD12.iter().all(|t| !t.expect.is_empty()));
        assert!(PUBLIC31.iter().all(|t| t.expect.is_empty()));
        // vendors8 is scored by expected text, like hard12.
        assert!(VENDORS8.iter().all(|t| !t.expect.is_empty()));
    }

    #[test]
    fn the_vendor_set_names_each_vendor_twice_and_only_vendors_the_classifier_knows() {
        // A vendor with one target is an anecdote. Two is the smallest number that can disagree
        // with itself, which is what makes a per-vendor row worth publishing.
        let mut by_wall: std::collections::BTreeMap<&str, usize> = Default::default();
        for t in VENDORS8 {
            *by_wall.entry(t.wall).or_default() += 1;
        }
        assert_eq!(by_wall.len(), 4, "four vendors: {by_wall:?}");
        assert!(by_wall.values().all(|n| *n == 2), "two each: {by_wall:?}");
        // Every wall named here must be one the classifier can actually recognise, or the run
        // reports "blocked by nothing in particular" and teaches us nothing.
        for t in VENDORS8 {
            assert!(
                svipall_core::classify::wall_is_known(t.wall),
                "{} names a wall the classifier does not know: {}",
                t.name,
                t.wall
            );
        }
    }

    #[test]
    fn the_frozen_sets_are_still_frozen() {
        // The whole argument for publishing a number is that its list did not move. A new vendor
        // target goes in vendors8; hard12 and public31 keep their exact membership.
        assert_eq!(HARD12.len(), 12);
        assert_eq!(PUBLIC31.len(), 31);
        assert!(HARD12.iter().any(|t| t.name == "g2"));
        assert!(PUBLIC31.iter().any(|t| t.name == "sannysoft"));
    }
}
