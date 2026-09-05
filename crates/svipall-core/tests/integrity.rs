//! The corpus that makes "we can tell a page from a husk" checkable rather than asserted.
//!
//! Every file in `fixtures/integrity/` is named for what it is: `<label>-<slug>.html`. The label
//! is either a wall the classifier must name, or — for a page that really was delivered — how much
//! of it arrived. The test walks the directory, so adding a case is adding a file.
//!
//! The gate that matters is the last one. A page wrongly called `thin` costs a caller a second
//! look; a husk wrongly called `full` is a wrong answer delivered silently, with the tool's word
//! behind it. So the corpus is allowed to be imprecise in one direction only.

use svipall_core::classify::{classify, WallKind};
use svipall_core::quality::{assess, Evidence, Verdict};

/// What a fixture's name claims about it.
#[derive(Debug, PartialEq)]
enum Label {
    /// The classifier must name this wall.
    Wall(WallKind),
    /// It is a real page, and this is how much of it arrived.
    Delivered(Verdict),
}

fn label_of(name: &str) -> Option<Label> {
    Some(match name.split('-').next()? {
        "full" => Label::Delivered(Verdict::Full),
        "thin" => Label::Delivered(Verdict::Thin),
        "partial" => Label::Delivered(Verdict::Partial),
        "softnotfound" => Label::Wall(WallKind::SoftNotFound),
        "paywall" => Label::Wall(WallKind::Paywall),
        "empty" => Label::Wall(WallKind::Empty),
        "gate" => Label::Wall(WallKind::Gate),
        "login" => Label::Wall(WallKind::Login),
        "generic" => Label::Wall(WallKind::Generic),
        _ => return None,
    })
}

struct Case {
    name: String,
    label: Label,
    html: String,
}

fn corpus() -> Vec<Case> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/integrity");
    let mut out: Vec<Case> = std::fs::read_dir(&dir)
        .expect("the corpus directory")
        .filter_map(|e| {
            let path = e.ok()?.path();
            if path.extension()? != "html" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let label = label_of(&name).unwrap_or_else(|| {
                panic!("{name}: the part before the first dash is not a label the test knows")
            });
            Some(Case {
                name,
                label,
                html: std::fs::read_to_string(&path).ok()?,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(out.len() >= 8, "the corpus is too small to mean anything");
    out
}

/// The extraction a fetch would have done, so the test judges what the caller would have seen.
fn read(html: &str) -> String {
    svipall_core::extraction::extract_text(html)
}

#[test]
fn every_label_the_test_knows_has_at_least_one_page_to_stand_for_it() {
    // A label with no fixture is a rule nothing checks.
    let cases = corpus();
    for label in [
        "full",
        "thin",
        "partial",
        "softnotfound",
        "paywall",
        "empty",
        "gate",
        "login",
        "generic",
    ] {
        assert!(
            cases.iter().any(|c| c.name.starts_with(label)),
            "no fixture for {label}"
        );
    }
}

#[test]
fn every_page_is_recognised_as_the_thing_it_is() {
    for c in corpus() {
        let text = read(&c.html);
        let (reason, kind) = classify(200, &c.html, &text);
        match &c.label {
            Label::Wall(want) => {
                assert_eq!(&kind, want, "{}: {reason:?}", c.name);
            }
            Label::Delivered(want) => {
                assert_eq!(
                    kind,
                    WallKind::None,
                    "{}: a real page was called a wall ({reason:?})",
                    c.name
                );
                let got = assess(&Evidence::new(c.html.len(), &text));
                assert_eq!(got.verdict, *want, "{}: {got:?}", c.name);
            }
        }
    }
}

#[test]
fn a_husk_is_never_reported_as_a_whole_page() {
    // The one asymmetry the corpus enforces. Calling an article thin costs a second look; calling
    // a husk whole is a wrong answer handed over with no warning attached to it.
    for c in corpus() {
        if matches!(c.label, Label::Delivered(Verdict::Full)) {
            continue;
        }
        let text = read(&c.html);
        let (_, kind) = classify(200, &c.html, &text);
        let whole = kind == WallKind::None
            && assess(&Evidence::new(c.html.len(), &text)).verdict == Verdict::Full;
        assert!(
            !whole,
            "{}: labelled {:?} and delivered as a whole page",
            c.name, c.label
        );
    }
}

#[test]
fn a_page_with_nothing_wrong_with_it_costs_nothing_to_say_so() {
    // The other half of the bargain: an ordinary page must produce no reasons at all, or every
    // fetch pays tokens to be told it is fine.
    for c in corpus() {
        if !matches!(c.label, Label::Delivered(Verdict::Full)) {
            continue;
        }
        let text = read(&c.html);
        let got = assess(&Evidence::new(c.html.len(), &text));
        assert!(got.reasons.is_empty(), "{}: {got:?}", c.name);
    }
}
