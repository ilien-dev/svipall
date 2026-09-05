//! The conformance test that makes "a new widget is a new row" true rather than aspirational.
//!
//! Every row in `svipall_core::widget::WIDGETS` has a fixture beside it: the markup a blocked page
//! from that widget actually carries. This walks the table and checks each one is recognised from
//! its own fixture, is not confused with another, and is answerable by something.
//!
//! A row without a fixture fails here. That is the whole point: adding a row is cheap, and the
//! cheapness is what would otherwise let a row be added that recognises nothing.

use svipall_core::challenge;
use svipall_core::widget::{Modality, WIDGETS};

/// Fixtures are named after the widget, with the punctuation of a hostname flattened.
fn fixture_name(id: &str) -> String {
    id.replace(['.', '/'], "-") + ".html"
}

fn fixture(id: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/widgets")
        .join(fixture_name(id));
    std::fs::read_to_string(path).ok()
}

#[test]
fn every_widget_in_the_table_has_a_page_to_recognise_it_from() {
    let missing: Vec<&str> = WIDGETS
        .iter()
        .filter(|w| fixture(w.id).is_none())
        .map(|w| w.id)
        .collect();
    assert!(
        missing.is_empty(),
        "no fixture for {missing:?} — a row that recognises nothing is worse than no row"
    );
}

#[test]
fn every_widget_is_found_on_its_own_page() {
    for w in WIDGETS {
        let html = fixture(w.id).expect("fixture");
        let found = challenge::detect_all(&html);
        assert!(
            found.iter().any(|d| d.widget == w.id),
            "{} was not recognised on its own page; found {:?}",
            w.id,
            found.iter().map(|d| d.widget).collect::<Vec<_>>()
        );
    }
}

#[test]
fn the_endpoint_alone_is_enough_when_the_markup_gives_nothing_away() {
    // Most of these inject their container from script, so a detector that only reads markup finds
    // an empty page. The traffic is the fallback, and it has to work on its own.
    for w in WIDGETS {
        let urls: Vec<String> = w
            .endpoints
            .iter()
            .map(|e| format!("https://{e}/api.js"))
            .collect();
        let found = challenge::from_traffic(&urls);
        assert!(
            found.iter().any(|d| d.widget == w.id),
            "{} is invisible in its own traffic",
            w.id
        );
    }
}

#[test]
fn the_key_a_solver_needs_is_read_off_the_page_not_left_to_the_caller() {
    // "There is a captcha" and "call this with this key" are different amounts of useful.
    for w in WIDGETS {
        if w.key_attrs.is_empty() {
            continue;
        }
        let html = fixture(w.id).expect("fixture");
        let found = challenge::detect_all(&html);
        let ours = found.iter().find(|d| d.widget == w.id).expect("found");
        assert!(
            ours.sitekey.as_deref().is_some_and(|k| !k.is_empty()),
            "{} carries {:?} on its page but no key was read",
            w.id,
            w.key_attrs
        );
    }
}

#[test]
fn every_modality_in_the_table_is_one_something_can_answer() {
    // A row whose modality nothing handles is a challenge that is recognised, reported, and then
    // sits there. The panel and the solve loop both key off this, so an unknown value here is a
    // silent dead end.
    for w in WIDGETS {
        assert!(
            Modality::ALL.contains(&w.default_modality),
            "{} declares a modality nothing knows about",
            w.id
        );
    }
}

#[test]
fn an_ordinary_page_is_not_mistaken_for_any_of_them() {
    // Fifteen container selectors matched loosely is fifteen chances to call a normal page a wall.
    let ordinary = "<!doctype html><html><head><title>Shop</title></head><body>\
         <h1>Blue shoes</h1><p>In stock. <a href=\"/cart\">Add to cart</a></p>\
         <form><input type=\"text\" name=\"q\"><button>Search</button></form>\
         </body></html>";
    assert!(
        challenge::detect_all(ordinary).is_empty(),
        "{:?}",
        challenge::detect_all(ordinary)
    );
}

#[test]
fn a_fixture_names_exactly_one_widget() {
    // Two widgets reported from one page means a selector is matching something generic, and the
    // caller is told to solve a challenge that is not there.
    for w in WIDGETS {
        let html = fixture(w.id).expect("fixture");
        let found = challenge::detect_all(&html);
        assert_eq!(
            found.len(),
            1,
            "{} recognised {:?}; one of those selectors is too loose",
            w.id,
            found.iter().map(|d| d.widget).collect::<Vec<_>>()
        );
    }
}
