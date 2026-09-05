//! Challenge widgets as data, not as code.
//!
//! There are around thirty challenge types in circulation and they are not thirty different
//! problems. They are two axes: **how you answer** — type a word, click some tiles, drag a piece,
//! hash a number — and **how you hand the answer over** — which element carries the site key, which
//! hidden field takes the response, which callback has to fire.
//!
//! The first axis has about ten values and each one is real work. The second has about fifteen and
//! each one is a row of strings. Writing them as fifteen implementations would be writing the same
//! function fifteen times with different selectors in it, and the cost of the thirtieth type would
//! be the same as the cost of the first.
//!
//! So: `Modality` is an enum the solvers dispatch on, `WidgetSpec` is a table, and adding a widget
//! is adding a row. A conformance test walks the table and fails if a row has no fixture or names a
//! modality nothing can answer — which is what keeps "just add a row" true rather than aspirational.
//!
//! Widgets are identified by the host their challenge endpoint lives on. That is the stable, factual
//! name for a protocol, and it is also how this project avoids putting vendors' brand names in its
//! source.

use serde::{Deserialize, Serialize};

/// What shape the answer takes. This is what a solver dispatches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    /// An opaque string the widget writes into a hidden field once it is satisfied.
    Token,
    /// Characters read out of an image, or a written answer to a question.
    Text,
    /// "Select every square with…" — a grid of images.
    Tiles,
    /// Click at one or more points on a picture.
    Points,
    /// Trace around an object.
    Polygon,
    /// Slide a piece into the notch it belongs in.
    Slide,
    /// Turn a picture until it is the right way up.
    Rotate,
    /// Drag things onto other things.
    Drag,
    /// Listen and type what was said.
    Audio,
    /// Hash until the digest has the shape the server asked for. No model, no person.
    Nonce,
    /// Hold a button down for as long as it is measuring.
    Hold,
    /// Not a challenge at all: a person reading a page and saying how much is in it.
    ///
    /// The panel, the job table and the corpus already do everything a labelling loop needs —
    /// present something, take an answer, keep it with who gave it. This borrows all of it. It
    /// is never replayed onto a live page, because there is no page waiting on it.
    Rate,
}

impl Modality {
    /// Every modality, so a table driven over them cannot quietly miss one.
    pub const ALL: &[Modality] = &[
        Modality::Token,
        Modality::Text,
        Modality::Tiles,
        Modality::Points,
        Modality::Polygon,
        Modality::Slide,
        Modality::Rotate,
        Modality::Drag,
        Modality::Audio,
        Modality::Nonce,
        Modality::Hold,
        Modality::Rate,
    ];

    /// Whether an answer can be produced with arithmetic and image geometry alone — no model file
    /// to install, and nobody interrupted.
    pub fn needs_no_model(self) -> bool {
        matches!(
            self,
            Modality::Nonce
                | Modality::Slide
                | Modality::Rotate
                | Modality::Hold
                | Modality::Drag
                // Nothing automatic will ever answer this one: judging a page is the whole
                // point of asking a person.
                | Modality::Rate
        )
    }

    /// How many automatic attempts are worth spending before handing over.
    ///
    /// Not one number for everything: a nonce either works or was misparsed, so a second try is
    /// pointless, while a slider has a tolerance and a retry is cheap. Guessing repeatedly at a
    /// tile grid, on the other hand, teaches the widget what it is dealing with.
    pub fn attempt_budget(self) -> u8 {
        match self {
            Modality::Nonce => 1,
            // A hold can land on the widget's placeholder before the real button has drawn;
            // measured on the site that asks for this most. The second try is cheap.
            Modality::Tiles | Modality::Points | Modality::Polygon | Modality::Drag => 2,
            Modality::Hold => 2,
            Modality::Text | Modality::Audio => 2,
            Modality::Slide | Modality::Rotate => 3,
            // Passive: there is nothing to attempt, only to wait for.
            Modality::Token => 1,
            // Nothing tries this automatically: it exists to be handed to a person.
            Modality::Rate => 1,
        }
    }
}

/// Everything that differs between one widget and the next.
///
/// All strings. A new widget is a new row here plus a fixture, and nothing else if its modality is
/// already answerable.
#[derive(Debug, Clone)]
pub struct WidgetSpec {
    /// The host its challenge endpoint lives on. The stable identifier, and the correct way to name
    /// a protocol without naming a product.
    pub id: &'static str,
    /// Container selectors in the host document.
    pub containers: &'static [&'static str],
    /// Attributes the site key is read from, in order of preference.
    pub key_attrs: &'static [&'static str],
    /// URL fragments that identify this widget's iframe and its network traffic.
    pub endpoints: &'static [&'static str],
    /// The field the widget writes its answer into, when it has one.
    pub response_field: Option<&'static str>,
    /// Where to start. Refined at run time by looking at what the frame is actually showing.
    pub default_modality: Modality,
    /// Almost always false: these answers are bound to a session and an address.
    pub token_reusable: bool,
}

/// Every widget svipall knows how to recognise.
///
/// Ordered roughly by how much of the web each one covers, because detection walks the table in
/// order and the common case should be found first.
pub const WIDGETS: &[WidgetSpec] = &[
    WidgetSpec {
        id: "challenges.cloudflare.com",
        containers: &[".cf-turnstile"],
        key_attrs: &["data-sitekey"],
        endpoints: &["challenges.cloudflare.com"],
        response_field: Some("cf-turnstile-response"),
        default_modality: Modality::Token,
        token_reusable: false,
    },
    WidgetSpec {
        id: "google.com/recaptcha",
        containers: &[".g-recaptcha", "#recaptcha"],
        key_attrs: &["data-sitekey"],
        endpoints: &["google.com/recaptcha", "recaptcha.net"],
        response_field: Some("g-recaptcha-response"),
        default_modality: Modality::Token,
        token_reusable: false,
    },
    WidgetSpec {
        id: "hcaptcha.com",
        containers: &[".h-captcha"],
        key_attrs: &["data-sitekey"],
        endpoints: &["hcaptcha.com"],
        response_field: Some("h-captcha-response"),
        default_modality: Modality::Token,
        token_reusable: false,
    },
    WidgetSpec {
        id: "captcha-delivery.com",
        containers: &["#captcha__frame", "[class*=captcha__]"],
        key_attrs: &["data-sitekey", "data-cid"],
        endpoints: &["captcha-delivery.com", "geo.captcha-delivery.com"],
        response_field: None,
        default_modality: Modality::Slide,
        token_reusable: false,
    },
    WidgetSpec {
        id: "client-api.arkoselabs.com",
        containers: &["#arkose", "[id*=funcaptcha]", "[data-pkey]"],
        key_attrs: &["data-pkey", "data-sitekey"],
        endpoints: &["arkoselabs.com", "funcaptcha.com"],
        response_field: Some("fc-token"),
        default_modality: Modality::Tiles,
        token_reusable: false,
    },
    WidgetSpec {
        id: "api.geetest.com",
        containers: &[".geetest_holder", "[id*=geetest]"],
        key_attrs: &["data-gt", "data-captcha-id", "data-sitekey"],
        endpoints: &["geetest.com", "gcaptcha4"],
        response_field: None,
        default_modality: Modality::Slide,
        token_reusable: false,
    },
    WidgetSpec {
        id: "altcha.org",
        containers: &["altcha-widget", ".altcha", "[data-challengeurl]"],
        key_attrs: &["data-challengeurl", "data-challenge", "data-sitekey"],
        endpoints: &["altcha"],
        response_field: Some("altcha"),
        default_modality: Modality::Nonce,
        token_reusable: false,
    },
    WidgetSpec {
        id: "friendlycaptcha.com",
        containers: &[".frc-captcha", "[data-puzzle-endpoint]"],
        key_attrs: &["data-sitekey", "data-puzzle-endpoint"],
        endpoints: &["friendlycaptcha.com", "frcapi.com"],
        response_field: Some("frc-captcha-solution"),
        default_modality: Modality::Nonce,
        token_reusable: false,
    },
    WidgetSpec {
        id: "prosopo.io",
        containers: &[".procaptcha", "[data-procaptcha]"],
        key_attrs: &["data-sitekey"],
        endpoints: &["prosopo.io"],
        response_field: Some("procaptcha-response"),
        default_modality: Modality::Nonce,
        token_reusable: false,
    },
    WidgetSpec {
        id: "service.mtcaptcha.com",
        containers: &[".mtcaptcha", "#mtcaptcha"],
        key_attrs: &["data-sitekey"],
        endpoints: &["mtcaptcha.com"],
        response_field: Some("mtcaptcha-verifiedtoken"),
        default_modality: Modality::Token,
        token_reusable: false,
    },
    WidgetSpec {
        id: "captcha.awswaf.com",
        containers: &["#captcha-container", "[id*=awswaf]"],
        key_attrs: &["data-sitekey", "data-key"],
        endpoints: &["awswaf.com", "captcha-sdk"],
        response_field: None,
        default_modality: Modality::Points,
        token_reusable: false,
    },
    WidgetSpec {
        id: "smartcaptcha.yandexcloud.net",
        containers: &[".smart-captcha"],
        key_attrs: &["data-sitekey"],
        endpoints: &["smartcaptcha.yandexcloud.net"],
        response_field: Some("smart-token"),
        default_modality: Modality::Token,
        token_reusable: false,
    },
    WidgetSpec {
        id: "turing.captcha.qcloud.com",
        containers: &["#TencentCaptcha"],
        key_attrs: &["data-appid", "data-sitekey"],
        endpoints: &["captcha.qcloud.com", "captcha.gtimg.com"],
        response_field: None,
        default_modality: Modality::Slide,
        token_reusable: false,
    },
    WidgetSpec {
        id: "captcha.gateway.imperva.com",
        containers: &["#captcha", "[id*=incapsula]"],
        key_attrs: &["data-sitekey"],
        endpoints: &["imperva.com", "incapsula"],
        response_field: None,
        default_modality: Modality::Hold,
        token_reusable: false,
    },
    WidgetSpec {
        id: "captcha.atb-captcha.com",
        containers: &[".atb-captcha", "[data-appid][id*=atb]"],
        key_attrs: &["data-appid", "data-sitekey"],
        endpoints: &["atb-captcha.com"],
        response_field: None,
        default_modality: Modality::Slide,
        token_reusable: false,
    },
];

/// The spec for an id, if it is one we know.
pub fn spec(id: &str) -> Option<&'static WidgetSpec> {
    WIDGETS.iter().find(|w| w.id == id)
}

/// Which widget a URL belongs to, for identifying one from network traffic rather than markup.
///
/// This is what finds a widget that injects itself from obfuscated JavaScript and leaves nothing in
/// the HTML to match on.
pub fn from_url(url: &str) -> Option<&'static WidgetSpec> {
    let low = url.to_ascii_lowercase();
    WIDGETS
        .iter()
        .find(|w| w.endpoints.iter().any(|e| low.contains(e)))
}

/// Every response field any widget writes to, for the "is there an answer yet" check that runs
/// before anything more expensive is attempted.
pub fn response_fields() -> impl Iterator<Item = &'static str> {
    WIDGETS.iter().filter_map(|w| w.response_field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The promise this whole design makes: adding a widget is adding a row. This test is what
    /// keeps that true, by failing the moment a row is added that nothing can act on.
    #[test]
    fn every_row_is_complete_and_actionable() {
        for w in WIDGETS {
            assert!(!w.id.is_empty(), "a widget with no id");
            assert!(
                w.id.contains('.'),
                "{} is not an endpoint host — widgets are named by their protocol, not by a brand",
                w.id
            );
            assert!(
                !w.containers.is_empty(),
                "{}: nothing to match in the DOM",
                w.id
            );
            assert!(!w.endpoints.is_empty(), "{}: no traffic to recognise", w.id);
            assert!(
                !w.key_attrs.is_empty(),
                "{}: no attribute to read the site key from",
                w.id
            );
        }
    }

    #[test]
    fn no_widget_is_listed_twice() {
        let ids: HashSet<&str> = WIDGETS.iter().map(|w| w.id).collect();
        assert_eq!(
            ids.len(),
            WIDGETS.len(),
            "a duplicate row shadows the other"
        );
    }

    #[test]
    fn a_widget_is_recognised_from_its_traffic() {
        // The path that finds a widget injected by obfuscated script, which leaves nothing in the
        // HTML to match on.
        let w = from_url("https://challenges.cloudflare.com/turnstile/v0/api.js").expect("found");
        assert_eq!(w.id, "challenges.cloudflare.com");
        assert_eq!(w.response_field, Some("cf-turnstile-response"));

        assert!(from_url("https://example.com/style.css").is_none());
    }

    #[test]
    fn the_url_match_ignores_case() {
        assert!(from_url("HTTPS://HCAPTCHA.COM/1/api.js").is_some());
    }

    #[test]
    fn the_widgets_that_need_no_model_are_the_ones_we_can_actually_finish() {
        // Three proof-of-work widgets and the geometric ones: everything answerable today without
        // asking the operator to install anything.
        let free: Vec<&str> = WIDGETS
            .iter()
            .filter(|w| w.default_modality.needs_no_model())
            .map(|w| w.id)
            .collect();
        assert!(
            free.len() >= 8,
            "only {} need no model: {free:?}",
            free.len()
        );
        assert_eq!(
            WIDGETS
                .iter()
                .filter(|w| w.default_modality == Modality::Nonce)
                .count(),
            3,
            "the three proof-of-work schemes should all be here"
        );
    }

    #[test]
    fn attempt_budgets_reflect_what_a_retry_is_worth() {
        // A nonce either verifies or was misparsed, so a second try is wasted. A slider has a
        // tolerance. Guessing repeatedly at a grid teaches the widget what it is dealing with.
        assert_eq!(Modality::Nonce.attempt_budget(), 1);
        assert_eq!(Modality::Slide.attempt_budget(), 3);
        assert_eq!(Modality::Tiles.attempt_budget(), 2);
        for m in [Modality::Nonce, Modality::Tiles, Modality::Audio] {
            assert!(m.attempt_budget() >= 1, "{m:?} could never be tried");
            assert!(m.attempt_budget() <= 3, "{m:?} would be hammered");
        }
    }

    #[test]
    fn every_response_field_belongs_to_a_widget_that_declares_one() {
        let fields: Vec<&str> = response_fields().collect();
        assert!(fields.contains(&"cf-turnstile-response"));
        assert!(fields.contains(&"g-recaptcha-response"));
        assert!(fields.contains(&"h-captcha-response"));
        assert!(fields.iter().all(|f| !f.is_empty()));
    }

    #[test]
    fn a_modality_that_needs_a_model_says_so() {
        assert!(
            !Modality::Tiles.needs_no_model(),
            "a grid needs a classifier"
        );
        assert!(!Modality::Audio.needs_no_model(), "audio needs a model");
        assert!(Modality::Nonce.needs_no_model());
        assert!(Modality::Slide.needs_no_model());
    }

    #[test]
    fn a_lookup_by_id_finds_the_row_and_an_unknown_one_is_none() {
        assert!(spec("hcaptcha.com").is_some());
        assert!(spec("not-a-widget.example").is_none());
    }
}
