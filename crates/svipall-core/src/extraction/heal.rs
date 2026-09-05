//! Relocating an element after the page it lived on was redesigned.
//!
//! A CSS selector names an element by the path the designer happened to give it. When the design
//! changes the path changes, the selector matches nothing, and a schema that worked for months
//! starts returning `null` in silence. The element is usually still there: same tag, most of the
//! same classes, the same kind of text, roughly the same place in the tree. That is what a
//! fingerprint records, and what `relocate` searches for.
//!
//! Lexical and structural only — no model, no embeddings. The signal is a handful of comparisons
//! that a person would make by eye, and the answer has to be explainable: "`.price` is gone;
//! `.cost` is the same tag with two of its three classes in the same place in the card".
//!
//! A relocation is accepted only when it is both good and unambiguous. A best candidate that
//! barely beats the runner-up is a coin toss dressed as a match, and a wrong field is worse than
//! an empty one — it is data that looks right.

use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Minimum similarity for a relocation to count.
pub const ACCEPT: f32 = 0.6;
/// Minimum lead over the runner-up. Below this the match is ambiguous and is refused.
pub const MARGIN: f32 = 0.15;
/// How many ancestor tags are recorded, nearest first.
const ANCESTORS: usize = 6;

/// What an element looked like the last time a selector found it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub tag: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub classes: Vec<String>,
    /// Attribute names other than `id` and `class`, sorted.
    #[serde(default)]
    pub attr_keys: Vec<String>,
    /// `log2` bucket of the text length: 0 for empty, 1 for 1..2 chars, and so on.
    #[serde(default)]
    pub text_len_bucket: u8,
    /// Share of digits in the text, in tenths. A price and a title differ here, a price and a
    /// count do not, which is the right amount of precision.
    #[serde(default)]
    pub digit_tenths: u8,
    #[serde(default)]
    pub depth: u16,
    /// Position among element siblings.
    #[serde(default)]
    pub sibling_index: u16,
    /// Ancestor tags, nearest first, at most `ANCESTORS`.
    #[serde(default)]
    pub ancestors: Vec<String>,
}

fn classes_of(el: ElementRef<'_>) -> Vec<String> {
    let mut c: Vec<String> = el.value().classes().map(str::to_string).collect();
    c.sort_unstable();
    c.dedup();
    c
}

fn text_len_bucket(len: usize) -> u8 {
    if len == 0 {
        0
    } else {
        (usize::BITS - len.leading_zeros()) as u8
    }
}

fn depth_of(el: ElementRef<'_>) -> u16 {
    el.ancestors().count().min(u16::MAX as usize) as u16
}

fn sibling_index(el: ElementRef<'_>) -> u16 {
    el.prev_siblings()
        .filter(|n| n.value().is_element())
        .count()
        .min(u16::MAX as usize) as u16
}

fn ancestor_tags(el: ElementRef<'_>) -> Vec<String> {
    el.ancestors()
        .filter_map(ElementRef::wrap)
        .map(|a| a.value().name().to_string())
        .take(ANCESTORS)
        .collect()
}

pub fn fingerprint(el: ElementRef<'_>) -> Fingerprint {
    let text: String = el.text().collect();
    let trimmed = text.trim();
    let chars = trimmed.chars().count();
    let digits = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    let mut attr_keys: Vec<String> = el
        .value()
        .attrs()
        .map(|(k, _)| k.to_string())
        .filter(|k| k != "id" && k != "class")
        .collect();
    attr_keys.sort_unstable();
    Fingerprint {
        tag: el.value().name().to_string(),
        id: el.value().id().map(str::to_string),
        classes: classes_of(el),
        attr_keys,
        text_len_bucket: text_len_bucket(chars),
        digit_tenths: (digits * 10).checked_div(chars).unwrap_or(0) as u8,
        depth: depth_of(el),
        sibling_index: sibling_index(el),
        ancestors: ancestor_tags(el),
    }
}

fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.iter().filter(|x| b.contains(x)).count();
    let union = a.len() + b.len() - inter;
    if union == 0 {
        1.0
    } else {
        inter as f32 / union as f32
    }
}

/// How much `el` looks like the element the fingerprint was taken from, in `0..=1`.
///
/// The weights add up to one and are chosen so that a same-tag element with the same classes in
/// the same place scores near one, and a same-tag element anywhere else scores well under
/// `ACCEPT`: tag and classes are what a designer keeps, position is what they move.
pub fn similarity(fp: &Fingerprint, el: ElementRef<'_>) -> f32 {
    let mut s = 0.0;
    let tag = el.value().name();
    if tag == fp.tag {
        s += 0.25;
    } else if is_heading(tag) && is_heading(&fp.tag) {
        // An `h2` that became an `h3` is the same element with a new outline level.
        s += 0.15;
    }
    if let Some(id) = &fp.id {
        if el.value().id() == Some(id.as_str()) {
            s += 0.15;
        }
    }
    let classes = classes_of(el);
    if fp.classes.is_empty() && classes.is_empty() {
        s += 0.10;
    } else {
        s += 0.20 * jaccard(&fp.classes, &classes);
    }
    let mut attr_keys: Vec<String> = el
        .value()
        .attrs()
        .map(|(k, _)| k.to_string())
        .filter(|k| k != "id" && k != "class")
        .collect();
    attr_keys.sort_unstable();
    s += 0.05 * jaccard(&fp.attr_keys, &attr_keys);

    let text: String = el.text().collect();
    let trimmed = text.trim();
    let chars = trimmed.chars().count();
    let bucket = text_len_bucket(chars);
    let d = (bucket as i32 - fp.text_len_bucket as i32)
        .unsigned_abs()
        .min(4) as f32;
    s += 0.10 * (1.0 - d / 4.0);
    let digits = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    let tenths = (digits * 10).checked_div(chars).unwrap_or(0) as u8;
    let dd = (tenths as i32 - fp.digit_tenths as i32)
        .unsigned_abs()
        .min(5) as f32;
    s += 0.05 * (1.0 - dd / 5.0);

    // Order-insensitive: a wrapper inserted around the element shifts every ancestor by one
    // but changes nothing about where the element lives.
    let anc = ancestor_tags(el);
    let matching = fp.ancestors.iter().filter(|a| anc.contains(a)).count();
    let n = fp.ancestors.len().max(anc.len()).max(1);
    s += 0.15 * (matching as f32 / n as f32);

    let depth = depth_of(el);
    let dep = (depth as i32 - fp.depth as i32).unsigned_abs().min(6) as f32;
    s += 0.05 * (1.0 - dep / 6.0);
    s
}

fn is_heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// A relocation that cleared both bars.
#[derive(Debug, Clone, Copy)]
pub struct Match<'a> {
    pub element: ElementRef<'a>,
    pub score: f32,
    /// Lead over the runner-up.
    pub margin: f32,
}

/// The best candidate, if it clears `ACCEPT` and leads the runner-up by `MARGIN`.
///
/// With `twins_allowed`, a runner-up that has the same tag and classes as the winner does not
/// count against it: a base selector is meant to match every card, and two identical cards are
/// the expected shape rather than an ambiguity. A field inside one item gets no such leniency —
/// two look-alike spans in a card is exactly the case where guessing picks the wrong one.
fn best_of<'a>(
    candidates: impl Iterator<Item = ElementRef<'a>>,
    fp: &Fingerprint,
    twins_allowed: bool,
) -> Option<Match<'a>> {
    let mut scored: Vec<(ElementRef<'a>, f32)> =
        candidates.map(|el| (el, similarity(fp, el))).collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (element, score) = *scored.first()?;
    let shape = step(element, true);
    let second = scored
        .iter()
        .skip(1)
        .find(|(el, _)| !(twins_allowed && step(*el, true) == shape))
        .map(|(_, s)| *s)
        .unwrap_or(0.0);
    let margin = score - second;
    (score >= ACCEPT && margin >= MARGIN).then_some(Match {
        element,
        score,
        margin,
    })
}

/// The element in the whole document most like the fingerprint, if it is a clear winner.
pub fn relocate<'a>(doc: &'a Html, fp: &Fingerprint) -> Option<Match<'a>> {
    best_of(
        doc.root_element()
            .descendants()
            .filter_map(ElementRef::wrap),
        fp,
        true,
    )
}

/// The same search, confined to one subtree — a field is looked for inside its item.
pub fn relocate_within<'a>(root: ElementRef<'a>, fp: &Fingerprint) -> Option<Match<'a>> {
    best_of(
        root.descendants()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.id() != root.id()),
        fp,
        false,
    )
}

/// One step of a selector for `el`: its id when it has one, otherwise tag plus classes, plus a
/// positional qualifier only when `generic` is off.
fn step(el: ElementRef<'_>, generic: bool) -> String {
    let v = el.value();
    if let Some(id) = v.id() {
        return format!("#{}", css_escape(id));
    }
    let mut s = v.name().to_string();
    for c in classes_of(el) {
        s.push('.');
        s.push_str(&css_escape(&c));
    }
    if !generic {
        let same_tag_before = el
            .prev_siblings()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == v.name())
            .count();
        let same_tag_after = el
            .next_siblings()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == v.name())
            .count();
        if same_tag_before + same_tag_after > 0 {
            s.push_str(&format!(":nth-of-type({})", same_tag_before + 1));
        }
    }
    s
}

fn css_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        let plain = c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii();
        if plain && !(i == 0 && c.is_ascii_digit()) {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

/// A selector that finds `el` again, scoped under `stop` (exclusive) when given.
///
/// Classes are preferred to positions: a `generic` selector (tag and classes only) is tried
/// first because it survives a reorder and, for a base selector, matches the element's siblings
/// too — which is what a base selector is for. Positions are added only when the generic form
/// does not find the element.
pub fn selector_for(el: ElementRef<'_>, stop: Option<ElementRef<'_>>, exact: bool) -> String {
    let chain = |generic: bool| -> String {
        let mut parts = vec![step(el, generic)];
        if !parts[0].starts_with('#') {
            for a in el.ancestors().filter_map(ElementRef::wrap) {
                if let Some(s) = stop {
                    if a.id() == s.id() {
                        break;
                    }
                }
                let name = a.value().name();
                if name == "html" || name == "body" {
                    break;
                }
                let st = step(a, generic);
                let is_id = st.starts_with('#');
                parts.push(st);
                if is_id {
                    break;
                }
            }
        }
        parts.reverse();
        parts.join(" > ")
    };
    let generic = chain(true);
    if selects(&generic, el, stop, exact) {
        return generic;
    }
    chain(false)
}

/// Does `selector` reach `el`? With `exact`, it has to reach it *first* — the selector a field
/// gets is applied with `.next()`, so "among the matches" is not good enough there.
fn selects(selector: &str, el: ElementRef<'_>, stop: Option<ElementRef<'_>>, exact: bool) -> bool {
    let Ok(sel) = Selector::parse(selector) else {
        return false;
    };
    let root = match stop {
        Some(root) => root,
        None => {
            let mut top = el;
            while let Some(p) = top.parent().and_then(ElementRef::wrap) {
                top = p;
            }
            top
        }
    };
    let mut found = root.select(&sel);
    if exact {
        found.next().map(|e| e.id() == el.id()).unwrap_or(false)
    } else {
        found.any(|e| e.id() == el.id())
    }
}

/// A relocation that happened, for the caller to report and act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Healed {
    pub field: String,
    pub from: String,
    pub to: String,
    pub score: f32,
}

/// Fingerprints by field name; the base selector is under `base`.
pub type Fingerprints = HashMap<String, Fingerprint>;

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> Html {
        Html::parse_document(html)
    }

    fn one<'a>(d: &'a Html, sel: &str) -> ElementRef<'a> {
        d.select(&Selector::parse(sel).unwrap()).next().expect(sel)
    }

    const BEFORE: &str = "<html><body><main><div class=\"card product\">\
        <h2 class=\"title\">Blue Cup</h2><span class=\"price\">$3.50</span>\
        <a class=\"buy btn\" href=\"/cup\">Buy</a></div>\
        <div class=\"card product\"><h2 class=\"title\">Red Pot</h2><span class=\"price\">$9.00</span>\
        <a class=\"buy btn\" href=\"/pot\">Buy</a></div></main></body></html>";

    #[test]
    fn an_element_survives_a_class_rename() {
        let before = doc(BEFORE);
        let fp = fingerprint(one(&before, "span.price"));
        let after = doc(&BEFORE.replace("class=\"price\"", "class=\"cost price-tag\""));
        let card = one(&after, "div.card");
        let m = relocate_within(card, &fp).expect("relocated");
        assert_eq!(m.element.value().name(), "span");
        assert!(m.element.text().collect::<String>().contains("$3.50"));
        assert!(m.score >= ACCEPT, "{}", m.score);
    }

    #[test]
    fn an_element_survives_being_moved_one_level_deeper() {
        let before = doc(BEFORE);
        let fp = fingerprint(one(&before, "h2.title"));
        let after = doc(&BEFORE.replace(
            "<h2 class=\"title\">Blue Cup</h2>",
            "<header><h3 class=\"title\">Blue Cup</h3></header>",
        ));
        let card = one(&after, "div.card");
        let m = relocate_within(card, &fp).expect("relocated");
        assert_eq!(m.element.value().name(), "h3");
        assert!(m.element.text().collect::<String>().contains("Blue Cup"));
    }

    #[test]
    fn two_equally_similar_candidates_are_a_miss_not_a_guess() {
        let before = doc(BEFORE);
        let fp = fingerprint(one(&before, "span.price"));
        // Two spans with the same classes and the same kind of text: nothing tells them apart.
        let after = doc(
            "<html><body><main><div class=\"card product\"><h2 class=\"title\">Blue Cup</h2>\
             <span class=\"amount\">$3.50</span><span class=\"amount\">$4.50</span></div></main></body></html>",
        );
        let card = one(&after, "div.card");
        assert!(
            relocate_within(card, &fp).is_none(),
            "ambiguous match was accepted"
        );
    }

    #[test]
    fn something_unrelated_never_clears_the_bar() {
        let before = doc(BEFORE);
        let fp = fingerprint(one(&before, "a.buy"));
        let after =
            doc("<html><body><p>Nothing here but prose.</p><ul><li>one</li></ul></body></html>");
        assert!(relocate(&after, &fp).is_none());
    }

    #[test]
    fn the_generated_selector_finds_exactly_that_element_and_its_siblings_for_a_base() {
        let d = doc(BEFORE);
        let card = one(&d, "div.card");
        let sel = selector_for(card, None, false);
        assert_eq!(sel, "main > div.card.product");
        let parsed = Selector::parse(&sel).unwrap();
        assert_eq!(
            d.select(&parsed).count(),
            2,
            "a base selector should reach every card"
        );

        let price = one(&d, "span.price");
        let rel = selector_for(price, Some(card), true);
        assert_eq!(rel, "span.price");
        assert_eq!(card.select(&Selector::parse(&rel).unwrap()).count(), 1);
    }

    #[test]
    fn a_position_is_added_only_when_classes_cannot_tell_siblings_apart() {
        let d = doc("<html><body><div id=\"box\"><span>a</span><span>b</span></div></body></html>");
        let second = d.select(&Selector::parse("span").unwrap()).nth(1).unwrap();
        let sel = selector_for(second, None, true);
        assert_eq!(sel, "#box > span:nth-of-type(2)");
        assert_eq!(
            d.select(&Selector::parse(&sel).unwrap())
                .next()
                .unwrap()
                .text()
                .collect::<String>(),
            "b"
        );
    }

    #[test]
    fn a_fingerprint_round_trips_through_json_and_tolerates_missing_fields() {
        let d = doc(BEFORE);
        let fp = fingerprint(one(&d, "span.price"));
        let s = serde_json::to_string(&fp).unwrap();
        assert_eq!(serde_json::from_str::<Fingerprint>(&s).unwrap(), fp);
        let old: Fingerprint = serde_json::from_str(r#"{"tag":"span"}"#).unwrap();
        assert_eq!(old.tag, "span");
        assert!(old.classes.is_empty());
    }
}
