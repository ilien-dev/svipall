//! Reading the challenge widget off a blocked page.
//!
//! `solve_turnstile` and friends need a sitekey, and the only way to get one used to be for the
//! caller to fetch the HTML, find the widget and copy the attribute out by hand. That is work the
//! classifier is already positioned to do: it has the page, and it has just decided the page is a
//! wall.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChallengeKind {
    Turnstile,
    RecaptchaV2,
    RecaptchaV3,
    HCaptcha,
    /// A proof-of-work widget: the page is asked to burn CPU, not to identify a bus. Solved here
    /// outright, with no model and nobody interrupted — see `crate::pow`.
    ProofOfWork,
}

impl ChallengeKind {
    /// The `solve_*` tool that handles this widget.
    pub fn tool(self) -> &'static str {
        match self {
            ChallengeKind::Turnstile => "solve_turnstile",
            ChallengeKind::RecaptchaV2 | ChallengeKind::RecaptchaV3 => "solve_recaptcha_v2",
            ChallengeKind::HCaptcha => "solve_hcaptcha",
            // Nothing to hand off: the ladder solves it in place.
            ChallengeKind::ProofOfWork => "solve_and_continue",
        }
    }

    /// Whether this widget can be answered without a model or a person.
    pub fn is_computable(self) -> bool {
        matches!(self, ChallengeKind::ProofOfWork)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Challenge {
    pub kind: ChallengeKind,
    pub sitekey: String,
    /// Turnstile's `data-action`, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Turnstile's `data-cdata`, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdata: Option<String>,
}

/// Attribute lookup that tolerates single quotes, double quotes and no quotes at all, because
/// challenge markup is frequently machine-generated and inconsistent.
fn attr_after(haystack: &str, at: usize, name: &str) -> Option<String> {
    // Look only within the same tag.
    let tail = &haystack[at..];
    let end = tail.find('>').unwrap_or(tail.len());
    let tag = &tail[..end];
    let idx = tag.find(name)?;
    let rest = tag[idx + name.len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let (delim, body) = match rest.chars().next()? {
        c @ ('"' | '\'') => (Some(c), &rest[1..]),
        _ => (None, rest),
    };
    let stop = match delim {
        Some(c) => body.find(c)?,
        None => body
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(body.len()),
    };
    let v = body[..stop].trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Proof-of-work widgets. Matched by the custom element or the class these schemes mount on, not by
/// a vendor name: the markup is the protocol here, and naming products in code is something this
/// project does not do.
static POW_WIDGET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<[a-z-]*(?:altcha|friendly-?captcha|procaptcha)[a-z-]*[^>]*|class=["'][^"']*(?:altcha|frc-captcha|procaptcha)"#)
        .expect("static regex")
});

static TURNSTILE_DIV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<div[^>]*class\s*=\s*["'][^"']*cf-turnstile"#).unwrap());
static RECAPTCHA_DIV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<div[^>]*class\s*=\s*["'][^"']*g-recaptcha"#).unwrap());
static HCAPTCHA_DIV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<div[^>]*class\s*=\s*["'][^"']*h-captcha"#).unwrap());
/// Cloudflare's managed interstitial hands the key to its own script rather than an element.
static CF_CHL_SITEKEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)["']?sitekey["']?\s*:\s*["']([0-9A-Za-z_\-]{8,})["']"#).unwrap()
});
/// `?render=<key>` on the reCAPTCHA v3 script tag.
static RECAPTCHA_RENDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)recaptcha/api\.js\?[^"'>]*render=([0-9A-Za-z_\-]{20,})"#).unwrap()
});

/// Find the challenge widget on a page, if there is one.
///
/// Only called once a page has been classified as a wall, so the cost is paid on the rare path.
pub fn detect(html: &str) -> Option<Challenge> {
    // Proof-of-work first: it is the one kind that costs nothing to answer, so recognising it
    // before anything else means never escalating a tier over a challenge that is arithmetic.
    if let Some(m) = POW_WIDGET.find(html) {
        // These widgets carry their parameters on the element rather than fetching them, so the
        // whole challenge is readable from the HTML the ladder already has.
        let challenge = attr_after(html, m.start(), "data-challengeurl")
            .or_else(|| attr_after(html, m.start(), "data-challenge"))
            .or_else(|| attr_after(html, m.start(), "data-sitekey"))
            .unwrap_or_default();
        return Some(Challenge {
            kind: ChallengeKind::ProofOfWork,
            sitekey: challenge,
            action: attr_after(html, m.start(), "data-difficulty"),
            cdata: attr_after(html, m.start(), "data-salt"),
        });
    }
    if let Some(m) = TURNSTILE_DIV.find(html) {
        if let Some(sitekey) = attr_after(html, m.start(), "data-sitekey") {
            return Some(Challenge {
                kind: ChallengeKind::Turnstile,
                sitekey,
                action: attr_after(html, m.start(), "data-action"),
                cdata: attr_after(html, m.start(), "data-cdata"),
            });
        }
    }
    if let Some(m) = HCAPTCHA_DIV.find(html) {
        if let Some(sitekey) = attr_after(html, m.start(), "data-sitekey") {
            return Some(Challenge {
                kind: ChallengeKind::HCaptcha,
                sitekey,
                action: None,
                cdata: None,
            });
        }
    }
    if let Some(m) = RECAPTCHA_DIV.find(html) {
        if let Some(sitekey) = attr_after(html, m.start(), "data-sitekey") {
            return Some(Challenge {
                kind: ChallengeKind::RecaptchaV2,
                sitekey,
                action: None,
                cdata: None,
            });
        }
    }
    if let Some(c) = RECAPTCHA_RENDER.captures(html) {
        return Some(Challenge {
            kind: ChallengeKind::RecaptchaV3,
            sitekey: c[1].to_string(),
            action: None,
            cdata: None,
        });
    }
    // Managed Cloudflare pages inline the key in a script object; only trust it when the page
    // otherwise looks like a Cloudflare challenge, or any JSON with a `sitekey` field would match.
    let low = html.to_ascii_lowercase();
    if low.contains("challenges.cloudflare.com") || low.contains("cf_chl_opt") {
        if let Some(c) = CF_CHL_SITEKEY.captures(html) {
            return Some(Challenge {
                kind: ChallengeKind::Turnstile,
                sitekey: c[1].to_string(),
                action: None,
                cdata: None,
            });
        }
    }
    None
}

/// A widget recognised on a page, however it was found.
#[derive(Debug, Clone, Serialize)]
pub struct Detected {
    /// The endpoint host, from `crate::widget::WIDGETS`.
    pub widget: &'static str,
    pub modality: crate::widget::Modality,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sitekey: Option<String>,
    /// How it was found. HTML is free, traffic costs a page load.
    pub source: &'static str,
}

/// Does this page carry the element the selector describes?
///
/// Not a CSS engine, and it must not become one: this runs on every blocked page, the classifier
/// has already decided the page is a wall, and the answer only has to be right rather than general.
/// What it does have to be is *tight*. Matching `#captcha` by looking for the word "captcha"
/// anywhere in the document reports that widget on every captcha page there is, and the caller is
/// told to solve a challenge that is not on the page.
fn container_matches(low: &str, selector: &str) -> bool {
    let mut rest = selector;
    let mut matched_any = false;
    while !rest.is_empty() {
        let (part, tail) = split_part(rest);
        rest = tail;
        if part.is_empty() {
            break;
        }
        if !part_matches(low, part) {
            return false;
        }
        matched_any = true;
    }
    matched_any
}

/// Peel one `.class`, `#id` or `[attr...]` off the front of a compound selector.
fn split_part(sel: &str) -> (&str, &str) {
    let bytes = sel.as_bytes();
    if bytes.first() == Some(&b'[') {
        return match sel.find(']') {
            Some(end) => (&sel[..=end], &sel[end + 1..]),
            None => (sel, ""),
        };
    }
    let end = sel[1..]
        .find(['.', '#', '['])
        .map(|i| i + 1)
        .unwrap_or(sel.len());
    (&sel[..end], &sel[end..])
}

fn part_matches(low: &str, part: &str) -> bool {
    if let Some(cls) = part.strip_prefix('.') {
        return class_token(low, &cls.to_ascii_lowercase());
    }
    if let Some(id) = part.strip_prefix('#') {
        // Exact, or `#captcha` matches `id="captcha-container"` and reports the wrong widget.
        let id = id.to_ascii_lowercase();
        return low.contains(&format!("id=\"{id}\"")) || low.contains(&format!("id='{id}'"));
    }
    let inner = part.trim_start_matches('[').trim_end_matches(']');
    // `[attr*=value]`, `[attr=value]` or a bare `[attr]`.
    let (name, value) = match inner.split_once('=') {
        Some((n, v)) => (
            n.trim_end_matches(['*', '^', '$', '~', '|']),
            Some(v.trim_matches(['"', '\''])),
        ),
        None => (inner, None),
    };
    let name = name.to_ascii_lowercase();
    if !low.contains(&format!("{name}=")) {
        return false;
    }
    match value {
        Some(v) if !v.is_empty() => low.contains(&v.to_ascii_lowercase()),
        _ => true,
    }
}

/// Is this word one of the classes on some element, rather than a substring of a longer one?
///
/// `.altcha` must not match `class="not-altcha-at-all"`.
fn class_token(low: &str, want: &str) -> bool {
    if want.is_empty() {
        return false;
    }
    let boundary = |c: Option<char>| match c {
        None => true,
        Some(c) => c.is_whitespace() || c == '"' || c == '\'',
    };
    let mut from = 0usize;
    while let Some(i) = low[from..].find(want) {
        let at = from + i;
        let before = low[..at].chars().next_back();
        let after = low[at + want.len()..].chars().next();
        if boundary(before) && boundary(after) {
            return true;
        }
        from = at + want.len();
    }
    false
}

/// Every widget the static HTML gives away.
///
/// Generic over the table rather than a function per widget, which is what makes a new widget a new
/// row. The old `detect` stays as the first hit of this, so existing callers do not change.
pub fn detect_all(html: &str) -> Vec<Detected> {
    let low = html.to_ascii_lowercase();
    let mut out = Vec::new();
    for w in crate::widget::WIDGETS {
        let hit = w.containers.iter().any(|c| container_matches(&low, c))
            || w.endpoints.iter().any(|e| low.contains(e));
        if !hit {
            continue;
        }
        let sitekey = w.key_attrs.iter().find_map(|a| {
            let at = low.find(a)?;
            // Back up to the start of the tag the attribute sits on. `attr_after` reads within a
            // single tag, so starting part-way through the previous one stops at that tag's `>`
            // before ever reaching the attribute it was asked for.
            let start = html[..at].rfind('<').unwrap_or(at);
            attr_after(html, start, a)
        });
        out.push(Detected {
            widget: w.id,
            modality: w.default_modality,
            sitekey,
            source: "html",
        });
    }
    out
}

/// The widget behind a piece of network traffic, for pages that inject their challenge from script
/// and leave nothing in the markup.
pub fn from_traffic(urls: &[String]) -> Vec<Detected> {
    let mut out: Vec<Detected> = Vec::new();
    for u in urls {
        if let Some(w) = crate::widget::from_url(u) {
            if !out.iter().any(|d| d.widget == w.id) {
                out.push(Detected {
                    widget: w.id,
                    modality: w.default_modality,
                    sitekey: None,
                    source: "traffic",
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of recognising these first: they cost nothing to answer, so escalating a
    /// tier over one would be spending minutes on arithmetic.
    #[test]
    fn the_generic_detector_finds_widgets_the_hand_written_one_never_knew_about() {
        // The point of the table: this widget has no bespoke code anywhere, only a row.
        let html = r#"<div class="frc-captcha" data-sitekey="FCMX1234"></div>"#;
        let found = detect_all(html);
        assert!(
            found.iter().any(|d| d.widget == "friendlycaptcha.com"),
            "{found:?}"
        );
        assert!(found
            .iter()
            .any(|d| d.modality == crate::widget::Modality::Nonce));
    }

    #[test]
    fn a_widget_injected_by_script_is_still_found_through_its_traffic() {
        // Nothing in the markup to match on, which is the case bespoke HTML rules always miss.
        let urls = vec![
            "https://example.com/app.js".to_string(),
            "https://challenges.cloudflare.com/turnstile/v0/api.js".to_string(),
        ];
        let found = from_traffic(&urls);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].widget, "challenges.cloudflare.com");
        assert_eq!(found[0].source, "traffic");
    }

    #[test]
    fn traffic_detection_does_not_report_the_same_widget_twice() {
        let urls = vec![
            "https://hcaptcha.com/1/api.js".to_string(),
            "https://hcaptcha.com/captcha/v1/x".to_string(),
        ];
        assert_eq!(from_traffic(&urls).len(), 1);
    }

    #[test]
    fn an_ordinary_page_yields_no_widgets_at_all() {
        assert!(detect_all("<html><body><p>nothing here</p></body></html>").is_empty());
        assert!(from_traffic(&["https://example.com/".to_string()]).is_empty());
    }

    #[test]
    fn a_proof_of_work_widget_is_recognised_as_computable() {
        for html in [
            r#"<altcha-widget data-challengeurl="/api/challenge"></altcha-widget>"#,
            r#"<div class="frc-captcha" data-sitekey="ABC123"></div>"#,
            r#"<div class="procaptcha" data-sitekey="X"></div>"#,
        ] {
            let c = detect(html).unwrap_or_else(|| panic!("missed: {html}"));
            assert_eq!(c.kind, ChallengeKind::ProofOfWork, "{html}");
            assert!(c.kind.is_computable());
        }
    }

    #[test]
    fn an_ordinary_widget_is_not_mistaken_for_computable() {
        // A false positive here would make the ladder try arithmetic on a challenge that needs a
        // browser, and report failure without ever escalating.
        let html = r#"<div class="cf-turnstile" data-sitekey="0x4AAA"></div>"#;
        let c = detect(html).expect("detected");
        assert_eq!(c.kind, ChallengeKind::Turnstile);
        assert!(!c.kind.is_computable());
    }

    #[test]
    fn a_page_with_no_widget_is_still_none() {
        assert!(detect("<html><body><p>altcha is mentioned in prose</p></body></html>").is_none());
    }

    #[test]
    fn a_turnstile_widget_yields_its_sitekey_action_and_cdata() {
        let html = r#"<html><body>
            <div class="cf-turnstile" data-sitekey="0x4AAAAAAADnPIDROrmt1Wwj"
                 data-action="login" data-cdata="session-123"></div>
        </body></html>"#;
        let c = detect(html).expect("challenge");
        assert_eq!(c.kind, ChallengeKind::Turnstile);
        assert_eq!(c.sitekey, "0x4AAAAAAADnPIDROrmt1Wwj");
        assert_eq!(c.action.as_deref(), Some("login"));
        assert_eq!(c.cdata.as_deref(), Some("session-123"));
        assert_eq!(c.kind.tool(), "solve_turnstile");
    }

    #[test]
    fn recaptcha_v2_is_recognised() {
        let html = r#"<div class="g-recaptcha" data-sitekey='6LeIxAcTAAAAAJcZVRqyHh71UMIEGNQ_MXjiZKhI'></div>"#;
        let c = detect(html).expect("challenge");
        assert_eq!(c.kind, ChallengeKind::RecaptchaV2);
        assert_eq!(c.sitekey, "6LeIxAcTAAAAAJcZVRqyHh71UMIEGNQ_MXjiZKhI");
        assert_eq!(c.kind.tool(), "solve_recaptcha_v2");
    }

    #[test]
    fn hcaptcha_is_recognised() {
        let html =
            r#"<div class="h-captcha" data-sitekey="10000000-ffff-ffff-ffff-000000000001"></div>"#;
        let c = detect(html).expect("challenge");
        assert_eq!(c.kind, ChallengeKind::HCaptcha);
        assert_eq!(c.kind.tool(), "solve_hcaptcha");
    }

    #[test]
    fn recaptcha_v3_is_read_from_the_script_url() {
        let html = r#"<script src="https://www.google.com/recaptcha/api.js?render=6LcAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"></script>"#;
        let c = detect(html).expect("challenge");
        assert_eq!(c.kind, ChallengeKind::RecaptchaV3);
        assert!(c.sitekey.starts_with("6Lc"));
    }

    #[test]
    fn a_managed_cloudflare_page_yields_its_inline_sitekey() {
        let html = r#"<script src="/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1"></script>
            <script>window._cf_chl_opt={cvId:'3',sitekey:'0x4AAAAAAABBBB'};</script>
            <script src="https://challenges.cloudflare.com/turnstile/v0/api.js"></script>"#;
        let c = detect(html).expect("challenge");
        assert_eq!(c.kind, ChallengeKind::Turnstile);
        assert_eq!(c.sitekey, "0x4AAAAAAABBBB");
    }

    /// A stray `sitekey` in unrelated JSON must not be mistaken for a challenge.
    #[test]
    fn an_unrelated_sitekey_field_is_not_a_challenge() {
        let html = r#"<script>var config = {"sitekey":"not-a-challenge-at-all"};</script>"#;
        assert!(detect(html).is_none());
    }

    #[test]
    fn a_page_with_no_widget_yields_nothing() {
        assert!(detect("<html><body><p>Just a page.</p></body></html>").is_none());
    }

    #[test]
    fn unquoted_and_single_quoted_attributes_both_parse() {
        assert_eq!(
            attr_after(
                r#"<div class="cf-turnstile" data-sitekey=abc123>"#,
                0,
                "data-sitekey"
            )
            .as_deref(),
            Some("abc123")
        );
        assert_eq!(
            attr_after(r#"<div data-sitekey='xyz'>"#, 0, "data-sitekey").as_deref(),
            Some("xyz")
        );
    }

    /// An attribute belonging to a later tag must not be picked up.
    #[test]
    fn attribute_lookup_stays_inside_its_own_tag() {
        let html = r#"<div class="cf-turnstile"></div><div data-sitekey="other"></div>"#;
        assert_eq!(attr_after(html, 0, "data-sitekey"), None);
        assert!(detect(html).is_none());
    }
}
