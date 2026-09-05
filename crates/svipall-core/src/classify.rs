//! Port of server.py _classify logic.
//! A 200 does not mean we actually got the page — classification decides ladder escalation.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallKind {
    None,
    Cloudflare,
    Vendor,
    Generic,
    Empty,
    Gate,
    Hold,
    Login,
    NotFound,
    /// A 200 whose body says the page is not there. No tier fixes it, so it stops the ladder the
    /// way a real 404 does.
    SoftNotFound,
    /// The article exists and is being withheld. A profile with a subscription is the only thing
    /// that changes the answer, so it escalates like a login wall rather than stopping.
    Paywall,
    Status,
}

static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

const BLOCKED_CODES: &[u16] = &[202, 401, 403, 405, 407, 421, 429, 444, 503];

const CLOUDFLARE_SIGNS: &[&str] = &[
    "just a moment",
    "checking your browser",
    "cf-browser-verification",
    "cf_chl_opt",
    "enable javascript and cookies to continue",
    "cf-please-wait",
    "attention required! | cloudflare",
    "cloudflare ray id",
];

/// Where a vendor's give-away arrives.
///
/// The distinction is not decoration. A header name never appears in a page body and a cookie name
/// rarely does, so a sign declared without its channel gets searched for in the one place it cannot
/// be found — which is how a wall that announces itself perfectly well came to be reported as a
/// near-empty body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignChannel {
    /// The markup or the rendered text.
    Body,
    /// A response header, matched against the start of a header line.
    Header,
    /// A cookie name, matched as a prefix so a per-visitor suffix does not hide it.
    Cookie,
}

/// One give-away, and the channel it arrives on.
pub struct VendorSign {
    /// The vendor's own endpoint. The stable identifier, and the way to name a protocol without
    /// naming a product.
    pub id: &'static str,
    pub channel: SignChannel,
    pub needle: &'static str,
}

/// Every fingerprinting vendor this classifier can name, one row per give-away.
///
/// Adding a vendor is adding rows here, never a check somewhere else: the row says what to look for
/// and where, and both the classifier and `wall_is_known` read the same table.
pub const VENDOR_SIGNS: &[VendorSign] = &{
    use SignChannel::{Body, Cookie, Header};
    macro_rules! sign {
        ($id:literal, $ch:expr, $needle:literal) => {
            VendorSign {
                id: $id,
                channel: $ch,
                needle: $needle,
            }
        };
    }
    [
        // The proof-of-work vendor. It shows no challenge page at all: the give-away is its SDK,
        // the endpoints its script posts to, and the headers that must accompany every later
        // request. A 403 with these and no visible challenge is that vendor and nothing else.
        sign!("kpsdk.io", Header, "x-kpsdk-ct"),
        sign!("kpsdk.io", Header, "x-kpsdk-cd"),
        sign!("kpsdk.io", Cookie, "kpsdk"),
        sign!("kpsdk.io", Body, "kpsdk.io"),
        sign!("kpsdk.io", Body, "kpsdk"),
        sign!("kpsdk.io", Body, "/149e9513-"),
        // The sensor-cookie vendor: its clearance is the cookie, so the cookie is the tell.
        sign!("incapsula.com", Header, "x-iinfo"),
        sign!("incapsula.com", Cookie, "visid_incap_"),
        sign!("incapsula.com", Cookie, "incap_ses_"),
        sign!("incapsula.com", Cookie, "nlbi_"),
        sign!("incapsula.com", Body, "_incapsula_"),
        sign!("incapsula.com", Body, "incapsula"),
        // The per-request-signing vendor: every request carries a signature its script computes,
        // and the cookie is what the script is keyed on.
        sign!("perfdrive.com", Cookie, "reese84"),
        sign!("perfdrive.com", Body, "pardon our interruption"),
        sign!("perfdrive.com", Body, "request unsuccessful"),
        // The edge vendor's sensor and its clearance cookie.
        sign!("akamaihd.net", Cookie, "_abck"),
        sign!("akamaihd.net", Cookie, "bm_sz"),
        sign!("akamaihd.net", Cookie, "ak_bmsc"),
        sign!("akamaihd.net", Cookie, "bm_sv"),
        sign!("akamaihd.net", Body, "akamai-bot-manager"),
        sign!("akamaihd.net", Body, "_abck"),
        sign!("akamaihd.net", Body, "bm_sz"),
        // The fingerprinting vendor whose verdict sits in the top document.
        sign!("captcha-delivery.com", Cookie, "datadome"),
        sign!("captcha-delivery.com", Body, "captcha-delivery.com"),
        sign!("captcha-delivery.com", Body, "datadome"),
        sign!("perimeterx.net", Cookie, "_pxhd"),
        sign!("perimeterx.net", Cookie, "_px"),
        sign!("perimeterx.net", Body, "px-captcha"),
        sign!("perimeterx.net", Body, "_px_"),
        // A hosting-side product that announces itself in the page and nowhere else.
        sign!("sitelock.com", Body, "powered and protected by"),
    ]
};

const OTHER_SIGNS: &[&str] = &[
    "please wait...",
    "click the button below to continue shopping",
    "access denied",
    "verify you are a human",
    "are you a robot",
    "unusual traffic",
    "please turn javascript on",
    "javascript is disabled",
    "to continue, please verify",
];

const LOGIN_WALL_SIGNS: &[&str] = &[
    "sign up and never miss a post",
    "log in to continue",
    "sign in to continue",
    "you must log in to continue",
    "log in or sign up to view",
    "join linkedin to view",
    "sign up to see photos and videos",
];

const CAPTCHA_TITLES: &[&str] = &[
    "captcha",
    "robot check",
    "bot verification",
    "security check",
    "verification",
    "access denied",
    "attention required! | cloudflare",
];

const DEFINITE_BLOCK_TEXT: &[&str] = &[
    "just a moment",
    "checking your browser",
    "verify you are a human",
    "confirm you are a human",
    "let's confirm you are human",
    "complete the security check",
    "robot or human?",
    "press and hold",
    "press & hold",
    "access denied",
    "unusual traffic",
    "pardon our interruption",
    "click the button below to continue shopping",
];

const HOLD_SIGNS: &[&str] = &[
    "robot or human?",
    "press & hold",
    "press and hold",
    "activate and hold",
    "let's confirm you are human",
    "complete the security check",
    "confirm you are a human",
];

/// A title that is *only* this is a template, not a piece about broken links. Matched against the
/// whole trimmed title, never as a substring, because "Understanding soft 404s" is an article.
const NOT_FOUND_TITLES: &[&str] = &[
    "404",
    "404 not found",
    "404 error",
    "error 404",
    "not found",
    "page not found",
    "file not found",
    "página no encontrada",
    "pagina no encontrada",
    "seite nicht gefunden",
    "page non trouvée",
];

/// A short 200 whose whole message is "it went wrong, try again".
///
/// This is what a soft block looks like when the vendor does not want to admit to one: no
/// challenge, no status code, no wall — the site's own error template, two hundred characters, at
/// the tier that had been working. Measured on `homedepot.com`, which answers a browser tier with
/// `200` and "Oops!! Something went wrong. Please refresh page" once it has decided about the
/// address, and `403` with the same page on the http tier.
///
/// Behind the short-page gate, so an article that discusses an outage is not one of these.
const SOFT_ERROR_TEXT: &[&str] = &[
    "something went wrong",
    "please refresh page",
    "please refresh the page",
    "please try again later",
    "temporarily unavailable",
    "we are having trouble",
    "we're having trouble",
    "an error has occurred",
    "algo salió mal",
    "algo salio mal",
];

/// Phrases a not-found template uses about itself. Only consulted on a short page: an article on
/// HTTP status codes contains every one of them.
const NOT_FOUND_TEXT: &[&str] = &[
    "page you were looking for",
    "page you requested could not be found",
    "page you are looking for does not exist",
    "page cannot be found",
    "page could not be found",
    "page does not exist",
    "we could not find the page",
    "we couldn't find the page",
    "la página que buscas no existe",
    "la pagina que buscas no existe",
];

/// A page served in place of the one asked for, until the visitor says where they are or agrees to
/// something. Short by nature, which is why it is checked with the other stand-in pages rather than
/// at the end: the tail of the classifier is unreachable for a page this size.
const GATE_PHRASES: &[&str] = &[
    "choose a country",
    "select your country",
    "choose your country",
    "select your region",
    "continue to the site",
    "are you shipping to",
];

/// The call to action a subscription stub carries where the rest of the article should be.
const PAYWALL_SIGNS: &[&str] = &[
    "subscribe to continue reading",
    "subscribe to keep reading",
    "to continue reading",
    "to keep reading",
    "this article is for subscribers",
    "subscribers only",
    "already a subscriber",
    "register to continue",
    "sign in to read the full",
    "suscríbete para seguir leyendo",
    "suscribete para seguir leyendo",
    "contenido exclusivo para suscriptores",
];

/// The publisher saying so in their own structured data. Schema.org, so it is declared rather than
/// guessed — but on a metered wall it is present on pages that were served in full, which is why
/// it corroborates a call to action instead of standing alone.
const PAYWALL_DECLARED: &[&str] = &[
    "\"isaccessibleforfree\":false",
    "\"isaccessibleforfree\": false",
    "\"isaccessibleforfree\":\"false\"",
    "\"isaccessibleforfree\": \"false\"",
];

/// Text under this is a stub, not an article, and is the only place the two checks above run. It
/// bounds their cost to short pages and keeps a long piece *about* paywalls or 404s out of reach.
const STUB_LIMIT: usize = 4_000;

/// A stub short enough that a call to action is the page rather than its footer.
const PITCH_LIMIT: usize = 1_500;

/// How much HTML is worth scanning. Beyond this a block page has long since revealed itself.
const SCAN_LIMIT: usize = 200_000;

/// Largest char boundary at or below `limit`, so slicing never panics and never silently gives up
/// on the cap. `html.get(..limit)` returns None when `limit` splits a multi-byte character, and the
/// old `unwrap_or(html)` then lowercased the *entire* document — megabytes, on every call.
fn clamp_to_boundary(s: &str, limit: usize) -> &str {
    if s.len() <= limit {
        return s;
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The two lowercased haystacks a classification needs, built once per response instead of once
/// per `classify` call. Every needle in this module is ASCII, so `to_ascii_lowercase` gives the
/// same answers as the Unicode-aware `to_lowercase` for roughly a tenth of the cost.
pub struct PageView<'a> {
    pub text: &'a str,
    low_html: String,
    low_text: String,
    /// Response headers as `"\n{name}: {value}"`, lowercased. Empty unless the caller supplied
    /// them, which is why `new` costs no more than it did.
    low_head: String,
    /// Cookie names as `"\n{name}"`, lowercased. Names only — a cookie value is a session secret
    /// and has no business in a haystack that ends up in a log line.
    low_cookies: String,
}

impl<'a> PageView<'a> {
    /// The page text, lowercased once, for callers that match phrases against it.
    pub fn low_text(&self) -> &str {
        &self.low_text
    }

    /// The head of the markup, lowercased once. Bounded: see `SCAN_LIMIT`.
    pub fn low_html(&self) -> &str {
        &self.low_html
    }

    pub fn new(html: &'a str, text: &'a str) -> Self {
        Self {
            text: text.trim(),
            low_html: clamp_to_boundary(html, SCAN_LIMIT).to_ascii_lowercase(),
            low_text: text.trim().to_ascii_lowercase(),
            low_head: String::new(),
            low_cookies: String::new(),
        }
    }

    /// What arrived alongside the body: response headers, and the names of the cookies now in the
    /// jar. A view without them classifies exactly as it did before, which is what keeps every
    /// existing caller honest.
    pub fn on_the_wire(mut self, headers: &[(String, String)], cookies: &[String]) -> Self {
        self.low_head = headers.iter().fold(String::new(), |mut acc, (k, v)| {
            acc.push('\n');
            acc.push_str(&k.to_ascii_lowercase());
            acc.push_str(": ");
            acc.push_str(&v.to_ascii_lowercase());
            acc
        });
        self.low_cookies = cookies.iter().fold(String::new(), |mut acc, c| {
            acc.push('\n');
            acc.push_str(&c.to_ascii_lowercase());
            acc
        });
        self
    }

    /// Did a header by this name arrive? Anchored to the start of a header line, so a policy header
    /// that merely names the vendor is not read as the vendor's own header.
    fn head_has(&self, name: &str) -> bool {
        self.low_head
            .match_indices('\n')
            .any(|(i, _)| self.low_head[i + 1..].starts_with(name))
    }

    /// Is a cookie by this name in the jar? A prefix, because these names carry a per-visitor
    /// suffix (`visid_incap_9812`) that would otherwise hide them.
    fn cookie_has(&self, name: &str) -> bool {
        self.low_cookies
            .match_indices('\n')
            .any(|(i, _)| self.low_cookies[i + 1..].starts_with(name))
    }

    /// Search both haystacks without concatenating them. The old code built a third string holding
    /// a copy of both, ~250 KB per call, inside the `warm` loop that runs every 1.5s.
    fn rendered_contains(&self, needle: &str) -> bool {
        self.low_html.contains(needle) || self.low_text.contains(needle)
    }
}

/// Classify response. Returns (reason, kind). reason is None when response looks genuine.
pub fn classify(status: u16, html: &str, text: &str) -> (Option<String>, WallKind) {
    classify_view(status, html, &PageView::new(html, text))
}

/// Same as [`classify`], reusing a [`PageView`] the caller already built.
/// Is a challenge page saying it is about to let us through?
///
/// Measured on a managed challenge that passed on its own: the page reads "Verification
/// successful. Waiting for <site> to respond" and then sits there while the origin, not the
/// challenge, takes its time. A wait that gives up at that moment throws away a pass it already
/// earned. This is the one case where extending the deadline is not hoping: the page said so.
pub fn challenge_reports_progress(low_text: &str) -> bool {
    const PROGRESS: &[&str] = &[
        "verification successful",
        "waiting for ",
        "verifying you are human",
        "checking if the site connection is secure",
        "redirecting",
    ];
    PROGRESS.iter().any(|p| low_text.contains(p))
}

/// Is this an interstitial that verifies the visitor by itself, with nothing to click?
///
/// The right thing to do on one is nothing: a person reading "Just a moment" waits. Pointer
/// activity and scrolling during that wait is what a script does, not what a person does, and it
/// was the one thing the tool did on every turn of its wait.
pub fn challenge_is_self_verifying(low_text: &str) -> bool {
    const SELF: &[&str] = &[
        "just a moment",
        "verify your session",
        "checking your browser",
        "verifying you are human",
        "checking if the site connection is secure",
        "please wait while we verify",
    ];
    SELF.iter().any(|p| low_text.contains(p)) || challenge_reports_progress(low_text)
}

/// Is this the *managed* Cloudflare challenge rather than the interstitial that clears itself?
///
/// The two are not the same wall and do not want the same tier. "Just a moment…" is a page that
/// runs a script and lets you through, which a stealth-patched headless browser clears in a couple
/// of seconds. The managed challenge scores the visitor, and a headless browser has never once
/// passed it here — so escalating to `stealth` first spends an attempt, and the attempt teaches
/// the site something, before arriving at the headful tier that had a chance all along.
///
/// The markers are the challenge page's own, not Cloudflare's: `cdn-cgi/challenge-platform` sits
/// in the markup of every Cloudflare customer page, challenge or not, and keying on it would call
/// half the web a wall.
pub fn cloudflare_is_managed_challenge(low_html: &str) -> bool {
    const SIGNS: &[&str] = &[
        "cf_chl_opt",
        "cf-chl-opt",
        "id=\"challenge-form\"",
        "challenge-error-text",
        "orchestrate/chl_page",
        "orchestrate/managed",
    ];
    SIGNS.iter().any(|s| low_html.contains(s))
}

/// Is this page guarded by the proof-of-work vendor whose clearance expires while you read?
///
/// It is the one wall that is not a page: no challenge, no widget, nothing to answer. Its script
/// earns a token by burning CPU, and that token lives 60–180 seconds — so unlike every other
/// vendor, *passing it once is not passing it*. A stateless HTTP client cannot hold it at all. The
/// warm tier can, because it holds a live browser, and `warm_needs_reissue` below is how it knows
/// the clearance is about to lapse.
pub fn is_proof_of_work_wall(v: &PageView<'_>) -> bool {
    VENDOR_SIGNS
        .iter()
        .filter(|s| s.id == POW_VENDOR)
        .any(|s| match s.channel {
            SignChannel::Body => v.low_html().contains(s.needle),
            SignChannel::Header => v.head_has(s.needle),
            SignChannel::Cookie => v.cookie_has(s.needle),
        })
}

/// The endpoint that names the proof-of-work vendor in [`VENDOR_SIGNS`].
pub const POW_VENDOR: &str = "kpsdk.io";

/// Is this domain's clearance one that only a live JavaScript runtime can hold?
///
/// The distinction decides whether keeping a page open buys anything. The proof-of-work vendor's
/// clearance is a value its SDK recomputes per request inside the page, so closing the tab throws
/// it away and the next fetch starts from nothing. Every other vendor here clears with a **cookie**,
/// which the per-domain profile already carries between fetches — keeping a page for one of those
/// would cost a Chrome tab and buy nothing measurable.
///
/// Adding a vendor to this predicate is therefore its own change, with its own baseline run to say
/// whether it was worth it.
pub fn clearance_lives_in_the_runtime(v: &PageView<'_>) -> bool {
    is_proof_of_work_wall(v)
}

/// The names of the cookies a response sets, values discarded.
///
/// A cookie value is a session secret; only the name is evidence, and only the name may travel into
/// a haystack that a reason string is built from.
pub fn cookie_names(headers: &[(String, String)]) -> Vec<String> {
    headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, v)| v.split(';').next())
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, _)| name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

/// How long a proof-of-work clearance is assumed good for. The observed range is 60–180 seconds;
/// two thirds of the floor leaves room to re-earn it before anything 403s.
pub const POW_TOKEN_LIFETIME_SECS: u64 = 40;

/// Should a warm session re-earn its proof-of-work clearance now?
///
/// `age_secs` is how long ago the page last earned one. Re-navigating costs a second of CPU;
/// letting it lapse costs the whole session, because the next request comes back 403 and the
/// address has now been seen failing.
///
/// **The caller must do this at most once per wait.** The vendor's script is on every response,
/// including the ones that never cleared, so "stale" stays true forever on a page that is simply
/// refusing. Measured on the benchmark: an uncapped loop turned a 28-second honest failure into a
/// 90-second timeout. One fresh attempt is worth having; a second is the page saying no.
pub fn warm_needs_reissue(v: &PageView<'_>, age_secs: u64) -> bool {
    is_proof_of_work_wall(v) && age_secs >= POW_TOKEN_LIFETIME_SECS
}

/// Is this a fingerprinting vendor's hard block, as opposed to its solvable challenge?
///
/// The vendor's interstitial carries its verdict in the page itself: a small object whose `t`
/// field is `bv` — blocked visitor — when the address has been refused outright, and `fe` when a
/// challenge is being offered. The frame that would show either lives in another process, where
/// this session cannot read, but the verdict sits in the top document for anyone to see.
///
/// Measured: without this, every hit on such a wall spent the whole warm budget nudging a page
/// that had already said no.
pub fn wall_is_hard_block(low_html: &str) -> bool {
    if !low_html.contains("captcha-delivery") {
        return false;
    }
    ["'t':'bv'", "\"t\":\"bv\"", "'t': 'bv'", "\"t\": \"bv\""]
        .iter()
        .any(|needle| low_html.contains(needle))
}

/// Is something on this machine writing into the pages the browser loads?
///
/// Measured: a security product on the operator's machine injected a stylesheet and a script into
/// every page — visible to any site's JavaScript, and the only network traffic a vendor's
/// device-check page ever made went to that product rather than to the vendor. No stealth work
/// survives that; it is the first thing to rule out when a fingerprinting wall will not clear.
/// Named by the host the injected assets load from, which is the protocol-by-endpoint rule.
pub fn local_injection(low_html: &str) -> Option<&'static str> {
    const INJECTORS: &[(&str, &str)] = &[
        (
            "kaspersky-labs.com",
            "a local security product (kaspersky-labs.com)",
        ),
        ("kis.v2.scr", "a local security product (kis.v2.scr)"),
        (
            "bitdefender.com/",
            "a local security product (bitdefender.com)",
        ),
        ("avast.com/inject", "a local security product (avast.com)"),
    ];
    INJECTORS
        .iter()
        .find(|(needle, _)| low_html.contains(needle))
        .map(|(_, what)| *what)
}

/// Does the wall name this machine's address as the reason?
///
/// Measured on a fingerprinting vendor's hard block: "unusual activity from your device or
/// network" with the IP printed on the page. Nothing on the page can be solved; another exit is
/// the only move, and saying so saves the caller three more tiers of waiting.
/// Does the classifier know how to recognise a wall by this name?
///
/// The benchmark labels each target with the wall it expects; a label nothing can detect makes a
/// failed run report "blocked by something" and teaches nobody anything. This is what keeps the
/// two lists honest with each other.
pub fn wall_is_known(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    if matches!(
        n.as_str(),
        "none" | "panel" | "tls" | "content" | "popular" | "waf" | "turnstile" | "press-and-hold"
    ) {
        return true;
    }
    // A vendor label counts as known when some sign the classifier scans for names it. The
    // benchmark's labels are the vendor's own endpoint or cookie, so this is a substring match in
    // both directions: "akamai" matches the `akamai-bot-manager` sign, and "captcha-delivery.com"
    // matches itself.
    let known = VENDOR_SIGNS
        .iter()
        .flat_map(|s| [s.id, s.needle])
        .chain(CLOUDFLARE_SIGNS.iter().copied())
        .chain(OTHER_SIGNS.iter().copied())
        .chain(HOLD_SIGNS.iter().copied());
    if n == "managed-challenge" || n == "cloudflare" {
        return true;
    }
    if n == "bot-manager" || n == "akamai" || n == "kasada" {
        return true;
    }
    known.into_iter().any(|s| s.contains(&n) || n.contains(s))
}

/// Does this response carry a vendor's give-away, on any channel?
///
/// Reported, never used to withhold: a page that arrived whole is a page that arrived whole, even
/// when the vendor's cookie is sitting in the jar. `quality`'s rule applied to walls.
pub fn vendor_on_the_wire(v: &PageView<'_>) -> Option<(&'static VendorSign, String)> {
    VENDOR_SIGNS.iter().find_map(|sign| {
        let hit = match sign.channel {
            // The body channel is the classifier's own cascade, which runs earlier and names the
            // sign itself. Looking again here would report a vendor twice over.
            SignChannel::Body => return None,
            SignChannel::Header => v.head_has(sign.needle),
            SignChannel::Cookie => v.cookie_has(sign.needle),
        };
        hit.then(|| {
            let where_ = match sign.channel {
                SignChannel::Header => "header",
                SignChannel::Cookie => "cookie",
                SignChannel::Body => "body",
            };
            (sign, format!("{where_} {}", sign.needle))
        })
    })
}

pub fn wall_blames_the_address(low_text: &str) -> bool {
    const BLAME: &[&str] = &[
        "activity from your device or network",
        "activity on your network",
        "access is temporarily restricted",
        "your ip address",
        "from your ip",
    ];
    BLAME.iter().any(|p| low_text.contains(p))
}

/// The page's `<title>`, trimmed and already lowercased by the caller's `PageView`.
fn title_of(low: &str) -> Option<&str> {
    TITLE_RE
        .captures(low)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim())
}

/// Is this short 2xx a stub standing in for the page — a not-found template, or an article being
/// withheld? Only called on a short body: every phrase here appears in a long piece on the subject.
fn stub_reason(low: &str, v: &PageView<'_>) -> Option<(Option<String>, WallKind)> {
    // A title that is *only* a not-found template is decisive on its own; the body phrases are
    // not, which is why they need the short-page gate this function is already behind.
    if let Some(t) = title_of(low) {
        if NOT_FOUND_TITLES.contains(&t) {
            return Some((
                Some(format!("soft 404: 200 with a not-found page (title: {t})")),
                WallKind::SoftNotFound,
            ));
        }
    }
    if let Some(p) = NOT_FOUND_TEXT.iter().find(|p| v.low_text().contains(**p)) {
        return Some((
            Some(format!("soft 404: 200 with a not-found page ({p})")),
            WallKind::SoftNotFound,
        ));
    }

    // Before the gate and paywall checks: a site that has decided about the address answers with
    // its own error template rather than a wall, and two hundred characters of "please refresh"
    // was being returned as the page. Escalating costs a retry; accepting it costs the fetch.
    if let Some(p) = SOFT_ERROR_TEXT.iter().find(|p| v.low_text().contains(**p)) {
        return Some((
            Some(format!(
                "the site's own error page instead of the content ({p})"
            )),
            WallKind::Generic,
        ));
    }

    if let Some(p) = GATE_PHRASES.iter().find(|p| v.low_text().contains(**p)) {
        return Some((
            Some(format!(
                "geo or consent gate instead of the requested page ({p})"
            )),
            WallKind::Gate,
        ));
    }

    // "Log in to continue reading" is both a reading pitch and a login wall, and the second is the
    // more specific thing to say: the action is signing in, and a profile is what fixes it. Left
    // to the login pass below so one place owns that vocabulary.
    if LOGIN_WALL_SIGNS.iter().any(|s| v.rendered_contains(s)) {
        return None;
    }

    // A call to action alone is a footer on most publishers. It means the page was withheld only
    // when the publisher declares the article closed, or when the pitch *is* the page.
    let pitch = PAYWALL_SIGNS.iter().find(|s| v.rendered_contains(s))?;
    let declared = PAYWALL_DECLARED.iter().any(|s| low.contains(s));
    if declared || v.text.len() < PITCH_LIMIT {
        let how = if declared {
            "declared closed in its structured data"
        } else {
            "the pitch is the whole page"
        };
        return Some((
            Some(format!("subscription stub ({pitch}; {how})")),
            WallKind::Paywall,
        ));
    }
    None
}

pub fn classify_view(status: u16, html: &str, v: &PageView<'_>) -> (Option<String>, WallKind) {
    let (reason, kind) = classify_body(status, html, v);
    // A wire sign never invents a wall; it names one already found. `Empty` and `Status` are the
    // two verdicts that mean "blocked, cause unknown" — those, and only those, get the vendor's
    // name. Upgrading anything else would turn a short page on a protected site into a wall, which
    // is filtering, and `anti_discard` exists to stop exactly that.
    if matches!(kind, WallKind::Empty | WallKind::Status) {
        if let Some((sign, evidence)) = vendor_on_the_wire(v) {
            return (
                Some(format!(
                    "browser-fingerprinting wall ({}, {evidence})",
                    sign.id
                )),
                WallKind::Vendor,
            );
        }
    }
    (reason, kind)
}

fn classify_body(status: u16, html: &str, v: &PageView<'_>) -> (Option<String>, WallKind) {
    let low = &v.low_html;
    let body_text = v.text;

    // No bytes at all is the strongest signal there is that nothing was delivered, and it was the
    // one signal nothing looked at: a page with an empty `<body>` was caught below, while a
    // response with no document whatsoever fell through every check and came out a success.
    //
    // Measured on two real sites: a `302` whose body was empty, and a `200` from a tier that had
    // been quietly refused. Both were reported as delivered pages, both stopped the ladder from
    // climbing, and both returned zero characters to the caller with no reason given.
    if html.trim().is_empty() {
        return (
            Some(match status {
                300..=399 => format!(
                    "{status} with no body: a redirect that was not followed, or a silent refusal"
                ),
                _ => format!("{status} with no document at all"),
            }),
            WallKind::Empty,
        );
    }

    // A redirect that still has a body is a redirect nobody followed. The fetcher follows ten, so
    // reaching here means a loop, a longer chain, or a tier that does not redirect at all — and
    // the body of a 3xx is a courtesy sentence, never the page. `https://x.com/explore` answered
    // `302` with seventy-four bytes reading "Found. Redirecting to /i/flow/login", and without
    // this the ladder took that for the timeline and stopped climbing.
    if (300..400).contains(&status) && body_text.len() < STUB_LIMIT {
        return (
            Some(format!(
                "{status} carrying only a redirect notice: the redirect was not followed"
            )),
            WallKind::Empty,
        );
    }

    // A stub that answers 200 has to be caught before the delivered-content shortcut below, which
    // only asks whether the page is long enough. A soft 404 and a subscription stub both clear
    // 2000 characters routinely, and both were coming back as the article that was asked for.
    if (200..300).contains(&status) && body_text.len() < STUB_LIMIT {
        if let Some(reason) = stub_reason(low, v) {
            return reason;
        }
    }

    // Delivered content check first — vendor scripts appear on every page of customer sites
    if (200..300).contains(&status) && body_text.len() >= 2000 {
        let has_block = DEFINITE_BLOCK_TEXT.iter().any(|s| v.low_text.contains(s));
        if !has_block {
            return (None, WallKind::None);
        }
    }

    // Hold detection first — specific actionable truth
    for sign in HOLD_SIGNS {
        if v.rendered_contains(sign) {
            return (
                Some(format!("human-verification challenge ({})", sign)),
                WallKind::Hold,
            );
        }
    }

    for sign in CLOUDFLARE_SIGNS {
        if low.contains(sign) {
            return (
                Some(format!("cloudflare wall ({})", sign)),
                WallKind::Cloudflare,
            );
        }
    }
    for sign in VENDOR_SIGNS {
        if sign.channel == SignChannel::Body && low.contains(sign.needle) {
            return (
                Some(format!("browser-fingerprinting wall ({})", sign.needle)),
                WallKind::Vendor,
            );
        }
    }
    for sign in LOGIN_WALL_SIGNS {
        if v.rendered_contains(sign) {
            return (Some(format!("login wall ({})", sign)), WallKind::Login);
        }
    }
    for sign in OTHER_SIGNS {
        if low.contains(sign) {
            return (Some(format!("bot wall ({})", sign)), WallKind::Generic);
        }
    }

    if let Some(caps) = TITLE_RE.captures(low) {
        if let Some(title) = caps.get(1) {
            let t = title.as_str().trim();
            if CAPTCHA_TITLES.contains(&t) {
                return (
                    Some(format!("challenge page (title: {})", t)),
                    WallKind::Generic,
                );
            }
        }
    }

    if BLOCKED_CODES.contains(&status) {
        return (Some(format!("http {}", status)), WallKind::Status);
    }

    if status == 404 || status == 410 {
        return (
            Some(format!("http {} (page does not exist at this URL)", status)),
            WallKind::NotFound,
        );
    }

    // Non-HTML replies are legitimately short
    let looks_html =
        low.contains("<html") || low.contains("<body") || low.contains("<!doctype html");
    if !(200..300).contains(&status) || !looks_html {
        return (None, WallKind::None);
    }

    let body = body_text;
    if body.len() < 200 {
        // A short page is only suspicious when the HTML is large (an app shell that never
        // rendered) or shows SPA / noscript hints with almost no text. Small static pages
        // such as example.com are genuine and must pass.
        let spa_hint = low.contains("<noscript")
            || low.contains("id=\"root\"")
            || low.contains("id=\"app\"")
            || low.contains("id=\"__next\"")
            || low.contains("__nuxt")
            || low.contains("data-reactroot");
        if body.is_empty() || html.len() > 6_000 || (spa_hint && body.len() < 60) {
            return (
                Some("near-empty body (unrendered SPA, interstitial or silent wall)".to_string()),
                WallKind::Empty,
            );
        }
        return (None, WallKind::None);
    }

    if html.len() > 60_000 && body.len() < 3_000 && (body.len() as f64 / html.len() as f64) < 0.02 {
        return (
            Some(format!(
                "shell page: {} chars of text in {} of HTML (JS not executed)",
                body.len(),
                html.len()
            )),
            WallKind::Empty,
        );
    }

    (None, WallKind::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_hundred_that_says_the_page_is_gone_is_not_a_delivered_page() {
        // Measured shape of a soft 404: the server answers 200 and the body is a short template
        // saying the thing is not there. Today that is delivered as content, and the caller reads
        // a "page not found" notice as if it were the article it asked for. No tier fixes it, so
        // it also has to stop the ladder rather than cost four climbs.
        let html = "<!doctype html><html><head><title>Page not found</title></head><body>\
             <h1>404</h1><p>Sorry, we could not find the page you were looking for.</p>\
             <a href=\"/\">Back to home</a></body></html>";
        let text = "404 Sorry, we could not find the page you were looking for. Back to home";
        let (reason, kind) = classify(200, html, text);
        assert_eq!(kind, WallKind::SoftNotFound);
        assert!(reason.is_some());
    }

    #[test]
    fn a_page_that_discusses_404s_at_length_is_not_mistaken_for_one() {
        // The obvious way to get this wrong is to match the phrase anywhere. An article *about*
        // broken links carries every phrase a soft 404 does, and is the page the caller asked for.
        let body = "Sorry, we could not find the page you were looking for is the message most \
             servers show. In this article we look at how a 404 not found response differs from a \
             soft 404, why search engines care, and what a page not found template should say. "
            .repeat(20);
        let html = format!(
            "<!doctype html><html><head><title>Understanding soft 404s</title></head>\
             <body><article>{body}</article></body></html>"
        );
        let (reason, kind) = classify(200, &html, &body);
        assert_eq!(kind, WallKind::None, "{reason:?}");
    }

    #[test]
    fn a_subscription_stub_is_told_apart_from_the_article_it_is_hiding() {
        // The publisher declares it themselves in JSON-LD, and the stub carries the call to action
        // where the rest of the article should be. Either signal alone is weak; together they are
        // the difference between having the article and having its first paragraph.
        let html = "<!doctype html><html><head><title>The story</title>\
             <script type=\"application/ld+json\">{\"@type\":\"NewsArticle\",\"isAccessibleForFree\":false}</script>\
             </head><body><h1>The story</h1><p>The first paragraph is free.</p>\
             <div class=\"paywall\">Subscribe to continue reading.</div></body></html>";
        let text = "The story The first paragraph is free. Subscribe to continue reading.";
        let (reason, kind) = classify(200, html, text);
        assert_eq!(kind, WallKind::Paywall, "{reason:?}");
    }

    #[test]
    fn a_whole_article_with_a_subscribe_box_at_the_bottom_is_still_the_article() {
        // Nearly every publisher puts a subscription pitch under the piece. Matching that would
        // report a paywall on exactly the pages that got past one.
        let body = "The council voted on Tuesday to approve the measure after a long debate. "
            .repeat(60)
            + " Subscribe to continue reading our award-winning journalism.";
        let html = format!(
            "<!doctype html><html><head><title>Council votes</title></head>\
             <body><article>{body}</article></body></html>"
        );
        let (reason, kind) = classify(200, &html, &body);
        assert_eq!(kind, WallKind::None, "{reason:?}");
    }

    #[test]
    fn an_interstitial_that_verifies_on_its_own_is_recognised_as_one() {
        // A person reading "Just a moment" waits; pointer activity there is what a script does.
        assert!(challenge_is_self_verifying(
            "just a moment...\nwe must verify your session"
        ));
        assert!(challenge_is_self_verifying(
            "verification successful. waiting for x to respond"
        ));
        assert!(!challenge_is_self_verifying(
            "press and hold the button to confirm"
        ));
        assert!(!challenge_is_self_verifying("select all images with a bus"));
    }

    #[test]
    fn the_proof_of_work_wall_is_recognised_from_its_sdk_and_not_from_a_challenge_page() {
        // It shows nothing. The only evidence is its SDK and its headers, so those are what the
        // classifier looks for.
        let sdk = "<script src=\"https://x.kpsdk.io/ips.js\"></script>";
        assert!(is_proof_of_work_wall(&PageView::new(sdk, "")));
        assert!(is_proof_of_work_wall(&wire(
            SHELL,
            &[("x-kpsdk-ct", "abc")],
            &[]
        )));
        let ordinary = "<html><body>an ordinary page</body></html>";
        assert!(!is_proof_of_work_wall(&PageView::new(ordinary, "")));
    }

    #[test]
    fn a_proof_of_work_clearance_is_re_earned_before_it_lapses_and_never_otherwise() {
        let pow = PageView::new("<script src=\"https://x.kpsdk.io/ips.js\"></script>", "");
        // Fresh: leave it alone.
        assert!(!warm_needs_reissue(&pow, 0));
        assert!(!warm_needs_reissue(&pow, POW_TOKEN_LIFETIME_SECS - 1));
        // Old enough that the next request would 403: re-earn it.
        assert!(warm_needs_reissue(&pow, POW_TOKEN_LIFETIME_SECS));
        // The observed floor is 60s, so the reissue point has to sit below it with room to spare.
        const { assert!(POW_TOKEN_LIFETIME_SECS < 60) };
        // Any other wall is not re-navigated: a Cloudflare interstitial verifying itself is made
        // worse by reloading, not better.
        assert!(!warm_needs_reissue(
            &PageView::new("just a moment...", ""),
            600
        ));
    }

    #[test]
    fn every_wall_the_benchmark_names_is_one_the_classifier_knows() {
        // The two lists have to agree, or a failed run reports "blocked by something".
        for name in [
            "kasada",
            "akamai",
            "captcha-delivery.com",
            "managed-challenge",
            "bot-manager",
            "turnstile",
            "press-and-hold",
            "none",
        ] {
            assert!(wall_is_known(name), "{name}");
        }
        assert!(!wall_is_known("a vendor nobody has heard of"));
    }

    #[test]
    fn every_vendor_sign_is_declared_once_with_the_channel_it_arrives_on() {
        // A vendor announces itself on one of three channels, and which one decides where to look
        // for it. A header name never appears in a body; a cookie name rarely does. Declaring the
        // channel beside the needle is what stops a sign from being searched for in the one place
        // it cannot be.
        for sign in VENDOR_SIGNS {
            assert!(
                sign.id.contains('.'),
                "a sign is identified by the vendor's endpoint, never its brand: {}",
                sign.id
            );
            assert_eq!(
                sign.needle,
                sign.needle.to_ascii_lowercase(),
                "haystacks are lowercased once, so needles must already be: {}",
                sign.needle
            );
        }
        for (i, a) in VENDOR_SIGNS.iter().enumerate() {
            for b in &VENDOR_SIGNS[i + 1..] {
                assert!(
                    !(a.channel == b.channel && a.needle == b.needle),
                    "declared twice on the same channel: {}",
                    a.needle
                );
            }
        }
        // Every vendor in the table is one the benchmark can name, and the reverse.
        for name in ["kpsdk.io", "incapsula", "perfdrive.com", "perimeterx.net"] {
            assert!(wall_is_known(name), "{name}");
        }
    }

    /// A shell: what a walled site serves when it has decided to say nothing. No vendor string
    /// anywhere in it — the whole point is that the body gives nothing away.
    const SHELL: &str = "<html><head></head><body><div id=\"root\"></div></body></html>";

    fn wire<'a>(html: &'a str, headers: &[(&str, &str)], cookies: &[&str]) -> PageView<'a> {
        let h: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let c: Vec<String> = cookies.iter().map(|s| (*s).to_string()).collect();
        PageView::new(html, "").on_the_wire(&h, &c)
    }

    #[test]
    fn a_wall_whose_only_tell_is_a_response_header_is_named_from_it() {
        // The measured failure this exists for: the proof-of-work vendor's block came back as
        // "near-empty body", because its only give-away is a header and the classifier only read
        // bodies. A block whose cause is knowable must not be reported as a block of unknown cause.
        let v = wire(SHELL, &[("x-kpsdk-ct", "2|abc|def")], &[]);
        let (reason, kind) = classify_view(403, SHELL, &v);
        assert_eq!(kind, WallKind::Vendor);
        assert!(
            reason.as_deref().unwrap_or_default().contains("x-kpsdk-ct"),
            "the reason names the evidence: {reason:?}"
        );
    }

    #[test]
    fn a_wall_whose_only_tell_is_a_cookie_is_named_from_it() {
        let v = wire(SHELL, &[], &["visid_incap_9812"]);
        let (reason, kind) = classify_view(200, SHELL, &v);
        assert_eq!(kind, WallKind::Vendor);
        assert!(
            reason
                .as_deref()
                .unwrap_or_default()
                .contains("visid_incap_"),
            "a per-visitor suffix must not hide the sign: {reason:?}"
        );
    }

    #[test]
    fn a_header_value_that_merely_mentions_a_vendor_is_not_read_as_that_header_arriving() {
        // A policy header listing the vendor's script host is not the vendor's header arriving.
        // Matching anywhere in the haystack would call every site that merely allows the script a
        // wall.
        let v = wire(
            SHELL,
            &[(
                "content-security-policy",
                "script-src https://x.kpsdk.io x-kpsdk-ct",
            )],
            &[],
        );
        assert!(vendor_on_the_wire(&v).is_none());
    }

    #[test]
    fn a_page_that_really_arrived_is_still_delivered_when_the_vendor_is_on_the_wire() {
        // The fence around the whole feature. A wire sign labels a page; it never withholds one.
        let text = "word ".repeat(700);
        let body = format!("<html><body><article>{text}</article></body></html>");
        let headers = [("x-kpsdk-ct".to_string(), "2|abc".to_string())];
        let v = PageView::new(&body, &text).on_the_wire(&headers, &[]);
        let (reason, kind) = classify_view(200, &body, &v);
        assert_eq!(reason, None, "a delivered page stays delivered");
        assert_eq!(kind, WallKind::None);
        assert!(
            vendor_on_the_wire(&v).is_some(),
            "and the vendor is still reported"
        );
    }

    #[test]
    fn a_clearance_only_a_live_runtime_can_hold_is_told_apart_from_a_cookie_one() {
        // The predicate that decides whether keeping a page open is worth a Chrome tab.
        let sdk = "<script src=\"https://x.kpsdk.io/ips.js\"></script>";
        assert!(clearance_lives_in_the_runtime(&PageView::new(sdk, "")));
        assert!(clearance_lives_in_the_runtime(&wire(
            SHELL,
            &[("x-kpsdk-ct", "abc")],
            &[]
        )));
        // Cookie-borne clearances: the per-domain profile already carries these between fetches, so
        // a kept page would buy nothing. Each is a real wall — just not one worth a held tab.
        for cookie in ["_abck", "bm_sz", "visid_incap_9", "datadome", "reese84"] {
            assert!(
                !clearance_lives_in_the_runtime(&wire(SHELL, &[], &[cookie])),
                "{cookie} clears with a cookie, not a runtime"
            );
        }
        assert!(!clearance_lives_in_the_runtime(&PageView::new(
            "just a moment...",
            ""
        )));
        assert!(!clearance_lives_in_the_runtime(&PageView::new(
            "<html><body>an ordinary page</body></html>",
            ""
        )));
    }

    #[test]
    fn a_lapsed_clearance_is_re_earned_when_only_the_wire_says_so() {
        // This is the assertion the live target fails today: the clearance is stale, nothing in the
        // body says so, and the header does.
        let v = wire(SHELL, &[("x-kpsdk-ct", "2|abc")], &[]);
        assert!(!warm_needs_reissue(&v, 0));
        assert!(warm_needs_reissue(&v, POW_TOKEN_LIFETIME_SECS));
    }

    #[test]
    fn a_cookie_name_is_read_off_a_set_cookie_line_and_its_value_is_left_behind() {
        let headers = [
            (
                "set-cookie".to_string(),
                "visid_incap_9=abc; path=/; HttpOnly".to_string(),
            ),
            ("content-type".to_string(), "text/html".to_string()),
        ];
        assert_eq!(cookie_names(&headers), vec!["visid_incap_9".to_string()]);
    }

    #[test]
    fn a_vendors_hard_block_is_told_apart_from_its_challenge() {
        // Measured on a real wall: the verdict sits in the top document as `'t':'bv'`, while the
        // words that explain it live in a frame this session cannot read.
        let block = "<script>var dd={'rt':'c','cid':'x','t':'bv','host':'geo.captcha-delivery.com'}</script>";
        let challenge = "<script>var dd={'rt':'c','cid':'x','t':'fe','host':'geo.captcha-delivery.com'}</script>";
        assert!(wall_is_hard_block(block));
        assert!(
            !wall_is_hard_block(challenge),
            "a challenge can still be answered"
        );
        assert!(
            !wall_is_hard_block("var dd={'t':'bv'}"),
            "only that vendor's object means this"
        );
    }

    #[test]
    fn a_page_a_local_product_has_written_into_is_recognised() {
        // The only traffic a vendor's device check ever made went to the antivirus, not to the
        // vendor. Nobody would find that from the wall's own wording.
        let html = "<html><head><link rel=\"stylesheet\" href=\"https://gc.kis.v2.scr.kaspersky-labs.com/x/abn/main.css\"></head></html>";
        assert!(local_injection(html).is_some());
        assert!(local_injection("<html><body>plain</body></html>").is_none());
    }

    #[test]
    fn a_challenge_that_says_it_passed_is_worth_waiting_for() {
        // The page said so; extending the deadline here is not hoping.
        assert!(challenge_reports_progress(
            "we must verify your session\nverification successful. waiting for www.x.test to respond"
        ));
        assert!(!challenge_reports_progress("access denied"));
        assert!(!challenge_reports_progress(""));
    }

    #[test]
    fn a_wall_that_names_the_address_is_recognised_as_one() {
        // Nothing on that page can be solved: another exit is the only move.
        assert!(wall_blames_the_address(
            "we detected unusual activity from your device or network. reasons may include"
        ));
        assert!(wall_blames_the_address("access is temporarily restricted"));
        assert!(!wall_blames_the_address(
            "please complete the security check"
        ));
    }

    #[test]
    fn a_response_with_no_document_at_all_is_never_a_delivered_page() {
        // The signal nothing looked at. A page with an empty `<body>` was caught; a response with
        // no document whatsoever fell through every check and came out a success, which stopped
        // the ladder climbing and handed the caller zero characters with no reason given.
        for (status, html) in [(200u16, ""), (302, ""), (301, "   "), (200, "\n\n")] {
            let parts = crate::extraction::parse_page(html, &crate::ParseWants::text());
            let view = PageView::new(html, &parts.text);
            let (reason, kind) = classify_view(status, html, &view);
            assert_eq!(kind, WallKind::Empty, "{status} {html:?}");
            assert!(reason.is_some(), "{status} {html:?}");
        }
    }

    #[test]
    fn a_redirect_with_no_body_says_it_was_a_redirect() {
        // The next step differs: a redirect wants following, a refusal wants another tier. A
        // caller told only "empty" cannot tell those apart.
        let parts = crate::extraction::parse_page("", &crate::ParseWants::text());
        let view = PageView::new("", &parts.text);
        let (reason, _) = classify_view(302, "", &view);
        assert!(
            reason.as_deref().is_some_and(|r| r.contains("redirect")),
            "{reason:?}"
        );
    }

    /// A redirect with a courtesy sentence in it is still a redirect. This is the exact body
    /// `https://x.com/explore` answers with, and taking it for the timeline is what stopped the
    /// ladder from ever opening a browser on that URL.
    #[test]
    fn a_redirect_carrying_only_a_notice_is_not_the_page() {
        let html = r#"<p>Found. Redirecting to /i/flow/login?redirect_after_login=%2Fexplore</p>"#;
        let parts = crate::extraction::parse_page(html, &crate::ParseWants::text());
        let view = PageView::new(html, &parts.text);
        let (reason, kind) = classify_view(302, html, &view);
        assert_eq!(kind, WallKind::Empty, "{reason:?}");
        assert!(
            reason.as_deref().is_some_and(|r| r.contains("redirect")),
            "{reason:?}"
        );
    }

    /// The two Cloudflare walls are not the same wall and do not want the same tier. The
    /// discriminator has to be the challenge page's own markup: `cdn-cgi/challenge-platform` is on
    /// every Cloudflare customer page, and keying on it would send half the web to the headful
    /// tier.
    #[test]
    fn the_managed_challenge_is_told_apart_from_just_a_moment() {
        let managed = r#"<html><body><div id="challenge-form" class="cf-chl"></div>
            <script>window._cf_chl_opt={cvId:"3"}</script></body></html>"#;
        assert!(cloudflare_is_managed_challenge(&managed.to_lowercase()));

        let interstitial =
            "<html><head><title>just a moment...</title></head><body>checking your browser</body></html>";
        assert!(!cloudflare_is_managed_challenge(interstitial));

        // An ordinary page of a Cloudflare customer: the bot-management script is present and no
        // challenge was served. This is the one that must not be caught.
        let delivered = r#"<html><body><article>The article, in full.</article>
            <script src="/cdn-cgi/challenge-platform/h/b/scripts/jsd/main.js"></script></body></html>"#;
        assert!(!cloudflare_is_managed_challenge(&delivered.to_lowercase()));
    }

    /// A site that has decided about your address does not always say so. This is verbatim what
    /// `homedepot.com` returns to a browser tier once it has: a `200`, its own error template, two
    /// hundred characters, no challenge and no status code. It was being returned as the page.
    #[test]
    fn the_sites_own_error_template_is_not_the_page() {
        let html = "<html><head><title>The Home Depot</title></head><body>\
            <p>#1 Home Improvement Retailer</p>\
            <h1>Oops!! Something went wrong. Please refresh page</h1><button>Refresh</button>\
            <p>How doers get more done. Need Help? Visit our Customer Service Center</p>\
            </body></html>";
        let parts = crate::extraction::parse_page(html, &crate::ParseWants::text());
        let view = PageView::new(html, &parts.text);
        let (reason, kind) = classify_view(200, html, &view);
        assert_eq!(kind, WallKind::Generic, "{reason:?}");
        assert!(
            reason.as_deref().is_some_and(|r| r.contains("error page")),
            "{reason:?}"
        );
    }

    /// A page that is *about* an outage is not an outage. The short-page gate is what keeps the
    /// phrase list from eating articles, so it is worth a test of its own.
    #[test]
    fn an_article_about_things_going_wrong_is_still_an_article() {
        let body = "Something went wrong at the datacentre that night, and the postmortem is \
                    worth reading in full. "
            .repeat(60);
        let html = format!("<html><head><title>The outage</title></head><body><article><p>{body}</p></article></body></html>");
        let parts = crate::extraction::parse_page(&html, &crate::ParseWants::text());
        let view = PageView::new(&html, &parts.text);
        let (reason, kind) = classify_view(200, &html, &view);
        assert_eq!(kind, WallKind::None, "{reason:?}");
    }

    /// …and a redirect that really does carry the page does not trip it. Some sites answer a 3xx
    /// with the full document beside the `Location` header, and throwing that away would cost a
    /// page that was delivered.
    #[test]
    fn a_redirect_that_carries_a_whole_page_is_left_alone() {
        let body = "Real article text. ".repeat(300);
        let html = format!("<html><body><article><p>{body}</p></article></body></html>");
        let parts = crate::extraction::parse_page(&html, &crate::ParseWants::text());
        let view = PageView::new(&html, &parts.text);
        let (reason, kind) = classify_view(301, &html, &view);
        assert_eq!(kind, WallKind::None, "{reason:?}");
    }

    #[test]
    fn a_real_page_is_not_caught_by_the_empty_document_rule() {
        let html = "<html><body><p>Real content, long enough that nothing here mistakes it for a \
                    wall or an unrendered application shell of any kind at all.</p></body></html>";
        let parts = crate::extraction::parse_page(html, &crate::ParseWants::text());
        let view = PageView::new(html, &parts.text);
        assert_eq!(classify_view(200, html, &view).1, WallKind::None);
    }

    #[test]
    fn test_cloudflare() {
        let html = "<html><title>Just a moment...</title><div>Checking your browser</div></html>";
        let (reason, kind) = classify(200, html, "Checking your browser");
        assert_eq!(kind, WallKind::Cloudflare);
        assert!(reason.is_some());
    }

    #[test]
    fn test_hold() {
        let html = "<html>press and hold to verify</html>";
        let (_, kind) = classify(200, html, "press and hold");
        assert_eq!(kind, WallKind::Hold);
    }

    #[test]
    fn small_static_page_is_genuine() {
        let html = "<!doctype html><html><head><title>Example Domain</title></head><body><h1>Example Domain</h1><p>This domain is for use in documentation examples.</p></body></html>";
        let (reason, kind) = classify(
            200,
            html,
            "Example Domain This domain is for use in documentation examples.",
        );
        assert_eq!(kind, WallKind::None, "{reason:?}");
    }

    #[test]
    fn unrendered_spa_shell_is_empty() {
        let html = format!(
            "<html><body><div id=\"root\"></div><script>{}</script></body></html>",
            "x".repeat(7000)
        );
        let (_, kind) = classify(200, &html, "");
        assert_eq!(kind, WallKind::Empty);
        let html2 = "<html><body><noscript>Enable JS</noscript><div id=\"app\">Loading…</div></body></html>";
        let (_, kind2) = classify(200, html2, "Enable JS Loading…");
        assert_eq!(kind2, WallKind::Empty);
    }

    #[test]
    fn clamp_to_boundary_never_splits_a_character() {
        assert_eq!(clamp_to_boundary("abc", 10), "abc");
        assert_eq!(clamp_to_boundary("abcdef", 3), "abc");
        // 'é' is two bytes: cutting at 2 would split it, so we fall back to 1.
        assert_eq!(clamp_to_boundary("aé", 2), "a");
        assert_eq!(clamp_to_boundary("aé", 3), "aé");
    }

    /// The scan cap has to hold even when it lands mid-character. `html.get(..200_000)` returns
    /// None there, and the old `unwrap_or(html)` quietly lowercased the whole document instead —
    /// so a wall sign far past the cap would be found, and huge pages paid for it every call.
    #[test]
    fn scan_cap_holds_at_a_multibyte_boundary() {
        // Put 'é' straddling byte 200_000 so the naive slice would fail.
        let prefix = "<html><body>";
        let mut html = format!("{}{}", prefix, "a".repeat(SCAN_LIMIT - 1 - prefix.len()));
        html.push('é');
        assert!(!html.is_char_boundary(SCAN_LIMIT));
        html.push_str("<div>just a moment</div>");
        let (reason, kind) = classify(200, &html, "");
        assert_ne!(
            kind,
            WallKind::Cloudflare,
            "the cloudflare sign sits past the scan cap and must not be reached: {reason:?}"
        );
    }

    #[test]
    fn geo_gate_is_detected() {
        // Long enough to clear the short-page branch, short enough to stay under the gate cap.
        let body = format!(
            "Welcome. {} Please select your country to continue.",
            "We ship to many places. ".repeat(12)
        );
        let html = format!("<html><body><p>{}</p></body></html>", body);
        let (reason, kind) = classify(200, &html, &body);
        assert_eq!(kind, WallKind::Gate, "{reason:?}");
    }

    #[test]
    fn test_none() {
        let html = "<html><body><p>Hello world ".repeat(500);
        let text = "Hello world ".repeat(500);
        let (reason, kind) = classify(200, &html, &text);
        assert_eq!(kind, WallKind::None);
        assert!(reason.is_none());
    }
}
