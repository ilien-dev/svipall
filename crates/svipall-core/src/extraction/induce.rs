//! A schema for a listing page nobody has written selectors for yet.
//!
//! `schema` extracts what the caller asked for, and `heal` puts a selector back when a redesign
//! moves it. Neither can start from nothing: healing needs a fingerprint from a run that already
//! worked, so the first visit to an unknown listing had no way in but reading the markdown.
//!
//! What a listing actually is, structurally, is a parent whose children repeat: the same tag, the
//! same classes, the same shape of descendants, several times over. That is a thing to *find*, not
//! to guess at, and finding it needs no model and no network — only the tree that was already
//! parsed for everything else.
//!
//! The rule from `heal` carries over unchanged, because it is the important one: **an ambiguous
//! answer is not an answer.** A candidate that barely beats the runner-up is refused, and a field
//! that is missing from a quarter of the records is dropped. A wrong row is worse than no row —
//! silently, and for as long as nobody checks.

use scraper::{ElementRef, Html, Selector};
use std::collections::HashMap;

/// A record set the page repeats, and the fields its records share.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Induced {
    /// What to pass as `base_selector`.
    pub base_selector: String,
    pub fields: Vec<InducedField>,
    /// How many records the base selector matches.
    pub matched: usize,
    /// How far ahead of the runner-up this candidate scored, 0..1. Reported so a caller can see
    /// the answer was not a coin flip.
    pub margin: f32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InducedField {
    pub name: String,
    pub selector: String,
    /// `text` or `attribute`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<&'static str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub absolute: bool,
}

/// A record set has to repeat enough times to be a set rather than a coincidence. Two of anything
/// is a pair of siblings; three is a pattern.
const MIN_RECORDS: usize = 3;
/// A field present in fewer records than this is not a column of the table, it is something one
/// record happens to have.
const FIELD_PRESENCE: f32 = 0.75;
/// How far ahead the winner must be. Same value and the same reason as `heal::MARGIN`: a candidate
/// that ties with another is a guess, and a guess is not returned.
const MARGIN: f32 = 0.15;
/// Class tokens that carry no identity — framework noise, state flags and utility soup. Matching a
/// record set on `active` or `mb-4` produces a selector that breaks on the next hover.
const NOISE_CLASSES: &[&str] = &[
    "active",
    "selected",
    "current",
    "open",
    "closed",
    "hover",
    "focus",
    "first",
    "last",
    "odd",
    "even",
    "hidden",
    "show",
    "visible",
    "col",
    "row",
    "container",
    "wrapper",
    "inner",
    "outer",
];

/// A class token worth selecting on: not framework noise, not a hash, not a number.
fn stable_class(c: &str) -> bool {
    let c = c.trim();
    if c.len() < 3 || c.len() > 40 {
        return false;
    }
    if NOISE_CLASSES.contains(&c) {
        return false;
    }
    // Any digit at all disqualifies it. Between generated names (`css-1x2y3z`, `jsx-1234567890`)
    // and utility classes (`mb-4`, `col-md-6`), a class with a number in it is almost never the
    // name of the thing — and the cost of being wrong is asymmetric: refusing a good class means
    // this record set is skipped, while accepting a generated one means a selector that dies at
    // the site's next build, silently, after the caller has stored it.
    if c.chars().any(|ch| ch.is_ascii_digit()) {
        return false;
    }
    c.chars()
        .all(|ch| ch.is_ascii_alphabetic() || ch == '-' || ch == '_')
}

fn classes_of(el: ElementRef<'_>) -> Vec<String> {
    el.value()
        .classes()
        .filter(|c| stable_class(c))
        .map(str::to_string)
        .collect()
}

/// What makes two siblings "the same kind of thing": the tag, plus the stable classes, in order.
fn signature(el: ElementRef<'_>) -> String {
    let mut cls = classes_of(el);
    cls.sort();
    format!("{}.{}", el.value().name(), cls.join("."))
}

/// A CSS selector matching every member of the group, and as little else as possible.
fn selector_for(tag: &str, classes: &[String]) -> String {
    if classes.is_empty() {
        tag.to_string()
    } else {
        format!("{}.{}", tag, classes.join("."))
    }
}

/// Visible text of an element, collapsed.
fn text_of(el: ElementRef<'_>) -> String {
    el.text().collect::<String>().split_whitespace().fold(
        String::new(),
        |mut acc: String, w: &str| {
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(w);
            acc
        },
    )
}

/// A slot inside a record: the selector that reaches it, and what it looks like across records.
#[derive(Default)]
struct Slot {
    seen: usize,
    total_len: usize,
    href: usize,
    src: usize,
    heading: usize,
    time: usize,
    currency: usize,
}

fn looks_like_money(s: &str) -> bool {
    let has_symbol = s.contains('$')
        || s.contains('€')
        || s.contains('£')
        || s.contains('¥')
        || s.to_ascii_uppercase().contains("EUR")
        || s.to_ascii_uppercase().contains("USD");
    has_symbol && s.chars().any(|c| c.is_ascii_digit())
}

/// Name a field after what it is, not after where it sits. `title` and `price` tell the caller
/// something; `text_3` tells them to go and look.
///
/// When nothing about the value says what it is, the page usually has: a site that calls a span
/// `titleline` or `sitestr` has already named the column, and borrowing that beats numbering.
/// Measured on a real listing, where the semantic guesses reached six of nine fields and the class
/// names covered the rest.
fn base_name(slot: &Slot, selector: &str) -> String {
    let n = slot.seen.max(1);
    if slot.href * 2 >= n {
        "url".to_string()
    } else if slot.src * 2 >= n {
        "image".to_string()
    } else if slot.heading * 2 >= n {
        "title".to_string()
    } else if slot.time * 2 >= n {
        "date".to_string()
    } else if slot.currency * 2 >= n {
        "price".to_string()
    } else if slot.total_len / n > 120 {
        "summary".to_string()
    } else {
        // The last class on the selector, which is the most specific one the page chose.
        selector
            .rsplit('.')
            .next()
            .filter(|c| *c != selector && c.len() >= 3)
            .map(|c| c.replace('-', "_"))
            .unwrap_or_else(|| "text".to_string())
    }
}

/// One candidate record set, before scoring.
struct Candidate {
    selector: String,
    members: usize,
    /// Total visible text across the records: a navigation list of one-word links is a repeat too,
    /// and it is not the listing anybody asked for.
    text: usize,
}

/// Find the record set a listing page repeats, and the fields its records share.
///
/// Returns `None` when the page has no repeated structure, when the best candidate does not beat
/// the runner-up by `MARGIN`, or when the winner has no field worth naming. Every one of those is
/// a refusal on purpose: the caller gets nothing rather than something plausible.
pub fn induce(html: &str) -> Option<Induced> {
    let doc = Html::parse_document(html);
    induce_doc(&doc)
}

/// The same, against a document already parsed. One parse per response is the rule everywhere else
/// in this module and it holds here too.
pub fn induce_doc(doc: &Html) -> Option<Induced> {
    let root = doc.root_element();
    let mut candidates: Vec<Candidate> = Vec::new();

    // Group the element children of every parent by signature. A listing is a parent whose
    // children repeat; everything else in the tree is noise for this purpose.
    for parent in root.descendants().filter_map(ElementRef::wrap) {
        let mut groups: HashMap<String, Vec<ElementRef<'_>>> = HashMap::new();
        for child in parent.children().filter_map(ElementRef::wrap) {
            groups.entry(signature(child)).or_default().push(child);
        }
        for (_, members) in groups {
            if members.len() < MIN_RECORDS {
                continue;
            }
            let first = members[0];
            let classes = classes_of(first);
            // A group with no stable class is only addressable as "every <li> under here", which
            // is true of navigation as often as of a listing. Without a class to name it, the
            // selector would be a guess dressed as an answer.
            if classes.is_empty() {
                continue;
            }
            let text: usize = members.iter().map(|m| text_of(*m).len()).sum();
            // Records with almost nothing in them are a menu, not a listing.
            if text / members.len() < 20 {
                continue;
            }
            candidates.push(Candidate {
                selector: selector_for(first.value().name(), &classes),
                members: members.len(),
                text,
            });
        }
    }

    // More records and more text both mean "more likely the thing on the page", but neither alone:
    // a footer link list wins on count, a single article wins on text.
    let score = |c: &Candidate| (c.members as f32).sqrt() * (c.text as f32).sqrt();
    candidates.sort_by(|a, b| score(b).total_cmp(&score(a)));
    let best = candidates.first()?;
    let best_score = score(best);
    if best_score <= 0.0 {
        return None;
    }
    let runner_up = candidates.get(1).map(score).unwrap_or(0.0);
    let margin = ((best_score - runner_up) / best_score).clamp(0.0, 1.0);
    if margin < MARGIN {
        return None;
    }

    let base = Selector::parse(&best.selector).ok()?;
    let records: Vec<ElementRef<'_>> = doc.select(&base).collect();
    if records.len() < MIN_RECORDS {
        return None;
    }
    let fields = fields_of(&records, &best.selector);
    if fields.is_empty() {
        return None;
    }
    Some(Induced {
        base_selector: best.selector.clone(),
        fields,
        matched: records.len(),
        margin,
    })
}

/// How to reach one slot from its record.
///
/// A bare tag is not an answer when the record holds several of them: a listing row with an upvote
/// arrow and a headline has two `<a>`s, and `a` picks whichever comes first — on Hacker News, the
/// vote link, returned under the name `url`. Qualifying it with the nearest classed ancestor
/// inside the record turns that into `span.titleline a`, which names one element per record.
fn slot_selector(el: ElementRef<'_>, record: ElementRef<'_>, base_selector: &str) -> String {
    let tag = el.value().name();
    let classes = classes_of(el);
    if !classes.is_empty() {
        return selector_for(tag, &classes);
    }
    let mut parent = el.parent().and_then(ElementRef::wrap);
    while let Some(p) = parent {
        let pc = classes_of(p);
        if !pc.is_empty() {
            let psel = selector_for(p.value().name(), &pc);
            // The base selector qualifies nothing: every slot is already inside it.
            if psel != base_selector {
                return format!("{psel} {tag}");
            }
            return tag.to_string();
        }
        if p.id() == record.id() {
            break;
        }
        parent = p.parent().and_then(ElementRef::wrap);
    }
    tag.to_string()
}

/// The slots that appear in most of the records, named for what they hold.
fn fields_of(records: &[ElementRef<'_>], base_selector: &str) -> Vec<InducedField> {
    let mut slots: HashMap<String, Slot> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for record in records {
        // One visit per selector per record: a record with three `<a>`s must not count `a` three
        // times and look more present than it is.
        let mut seen_here: HashMap<String, bool> = HashMap::new();
        for el in record.descendants().filter_map(ElementRef::wrap) {
            let sel = slot_selector(el, *record, base_selector);
            // The record is not one of its own columns. `descendants()` includes the element it
            // starts from, so without this every schema opened with a field whose selector was the
            // base selector and whose value was the whole row.
            if sel == base_selector {
                continue;
            }
            if seen_here.contains_key(&sel) {
                continue;
            }
            seen_here.insert(sel.clone(), true);
            let text = text_of(el);
            let entry = slots.entry(sel.clone()).or_default();
            if entry.seen == 0 {
                order.push(sel);
            }
            entry.seen += 1;
            entry.total_len += text.len();
            if el.value().attr("href").is_some() {
                entry.href += 1;
            }
            if el.value().attr("src").is_some() {
                entry.src += 1;
            }
            if matches!(el.value().name(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                entry.heading += 1;
            }
            if el.value().name() == "time" || el.value().attr("datetime").is_some() {
                entry.time += 1;
            }
            if looks_like_money(&text) {
                entry.currency += 1;
            }
        }
    }

    let need = (records.len() as f32 * FIELD_PRESENCE).ceil() as usize;
    // Which slots survive, in document order.
    let kept: Vec<String> = order
        .into_iter()
        .filter(|sel| {
            let Some(slot) = slots.get(sel) else {
                return false;
            };
            if slot.seen < need {
                return false;
            }
            // An element that holds nothing but other elements is scaffolding, not a field. The
            // exception is a link or an image, where the value is in the attribute.
            let empty = slot.total_len / slot.seen.max(1) == 0;
            !(empty && slot.href == 0 && slot.src == 0)
        })
        .collect();

    // Two slots can want the same name — a row with an upvote link and a headline link both ask to
    // be `url`. The unsuffixed name goes to whichever carries the most text, because that is the
    // one a caller reading `url` means, and the others are numbered. Without this the vote arrow
    // won on document order and the article link came back as `url_2`.
    let mut winner: HashMap<String, (String, usize)> = HashMap::new();
    for sel in &kept {
        let Some(slot) = slots.get(sel) else { continue };
        let base = base_name(slot, sel);
        let weight = slot.total_len / slot.seen.max(1);
        winner
            .entry(base)
            .and_modify(|(best_sel, best_weight)| {
                if weight > *best_weight {
                    *best_sel = sel.clone();
                    *best_weight = weight;
                }
            })
            .or_insert((sel.clone(), weight));
    }

    let mut used: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for sel in kept {
        let Some(slot) = slots.get(&sel) else {
            continue;
        };
        let base = base_name(slot, &sel);
        let name = if winner.get(&base).is_some_and(|(s, _)| *s == sel) {
            base
        } else {
            // Numbered from two, and only the runners-up are counted, so a schema never shows a
            // `url` beside a `url_1` or skips from `url_1` to `url_3`.
            let count = used.entry(base.clone()).or_insert(1);
            *count += 1;
            format!("{base}_{count}")
        };
        let n = slot.seen.max(1);
        let (kind, attribute, absolute) = if slot.href * 2 >= n {
            ("attribute", Some("href"), true)
        } else if slot.src * 2 >= n {
            ("attribute", Some("src"), true)
        } else {
            ("text", None, false)
        };
        out.push(InducedField {
            name,
            selector: sel,
            kind,
            attribute,
            absolute,
        });
        // A schema with thirty fields is a schema nobody reads. The records repeat; the useful
        // columns are the first handful.
        if out.len() >= 8 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"
      <html><body>
        <nav class="site-nav"><a href="/a">A</a><a href="/b">B</a><a href="/c">C</a></nav>
        <div class="results">
          <article class="product-card">
            <h3 class="product-title">Blue widget</h3>
            <a class="product-link" href="/p/1">See it</a>
            <span class="product-price">$19.99</span>
          </article>
          <article class="product-card">
            <h3 class="product-title">Red widget</h3>
            <a class="product-link" href="/p/2">See it</a>
            <span class="product-price">$24.50</span>
          </article>
          <article class="product-card">
            <h3 class="product-title">Green widget</h3>
            <a class="product-link" href="/p/3">See it</a>
            <span class="product-price">$31.00</span>
          </article>
          <article class="product-card">
            <h3 class="product-title">Grey widget</h3>
            <a class="product-link" href="/p/4">See it</a>
            <span class="product-price">$12.75</span>
          </article>
        </div>
      </body></html>"#;

    #[test]
    fn a_listing_names_its_own_records_and_columns() {
        let got = induce(LISTING).expect("a four-record listing is a record set");
        assert_eq!(got.base_selector, "article.product-card");
        assert_eq!(got.matched, 4);
        let names: Vec<&str> = got.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"title"), "{names:?}");
        assert!(names.contains(&"url"), "{names:?}");
        assert!(names.contains(&"price"), "{names:?}");
        let url = got.fields.iter().find(|f| f.name == "url").unwrap();
        assert_eq!(url.kind, "attribute");
        assert_eq!(url.attribute, Some("href"));
        assert!(url.absolute, "a link is only useful resolved");
    }

    #[test]
    fn the_navigation_is_not_the_listing() {
        // The nav repeats three times and the listing four. Counting alone would pick the nav on a
        // shorter page; the text each record carries is what tells them apart.
        let got = induce(LISTING).unwrap();
        assert!(!got.base_selector.contains("nav"));
    }

    #[test]
    fn a_page_with_nothing_repeated_is_refused() {
        let html =
            "<html><body><article class='post'><h1>One</h1><p>Only.</p></article></body></html>";
        assert_eq!(induce(html), None);
    }

    #[test]
    fn a_menu_of_one_word_links_is_not_a_listing() {
        let html = r#"<html><body><ul class="menu">
            <li class="menu-item"><a href="/1">Home</a></li>
            <li class="menu-item"><a href="/2">News</a></li>
            <li class="menu-item"><a href="/3">Jobs</a></li>
            <li class="menu-item"><a href="/4">Help</a></li>
        </ul></body></html>"#;
        assert_eq!(induce(html), None, "four short links are navigation");
    }

    /// Two links in a row both want to be `url`. The name goes to the one carrying the text,
    /// because that is the one a caller reading `url` means — measured on a real listing, where
    /// document order handed `url` to the upvote arrow and the article link came back as `url_2`.
    /// The bare `a` is qualified by the nearest classed ancestor for the same reason: `a` alone
    /// picks whichever comes first.
    #[test]
    fn the_link_that_carries_the_text_takes_the_name() {
        let html = r#"<html><body><div class="feed">
            <div class="story"><span class="vote"><a href="/up/1">▲</a></span><span class="headline"><a href="/a/1">A long enough headline to be the story</a></span></div>
            <div class="story"><span class="vote"><a href="/up/2">▲</a></span><span class="headline"><a href="/a/2">Another headline with real words in it</a></span></div>
            <div class="story"><span class="vote"><a href="/up/3">▲</a></span><span class="headline"><a href="/a/3">A third headline, also of some length</a></span></div>
        </div></body></html>"#;
        let got = induce(html).expect("three stories are a record set");
        let url = got
            .fields
            .iter()
            .find(|f| f.name == "url")
            .expect("a url field");
        assert_eq!(
            url.selector, "span.headline a",
            "the bare tag was not qualified, so `a` picked the vote link"
        );
        // The runner-up is numbered from two, never one, and never left unnumbered.
        assert!(got.fields.iter().any(|f| f.name == "url_2"));
        assert!(!got.fields.iter().any(|f| f.name == "url_1"));
    }

    #[test]
    fn a_generated_class_is_never_the_selector() {
        assert!(!stable_class("css-1x2y3z"), "generated");
        assert!(!stable_class("jsx-1234567890"), "generated");
        assert!(!stable_class("mb-4"), "utility");
        assert!(!stable_class("col-md-6"), "utility");
        assert!(!stable_class("active"), "state");
        assert!(!stable_class("ab"), "too short to mean anything");
        assert!(stable_class("product-card"));
        assert!(stable_class("search_result"));
    }

    #[test]
    fn a_field_missing_from_a_quarter_of_the_records_is_dropped() {
        let html = r#"<html><body><div class="list">
            <div class="entry"><h3 class="entry-title">One</h3><span class="badge">New</span><p class="entry-body">Body text one here.</p></div>
            <div class="entry"><h3 class="entry-title">Two</h3><p class="entry-body">Body text two here.</p></div>
            <div class="entry"><h3 class="entry-title">Three</h3><p class="entry-body">Body text three here.</p></div>
            <div class="entry"><h3 class="entry-title">Four</h3><p class="entry-body">Body text four here.</p></div>
        </div></body></html>"#;
        let got = induce(html).expect("four entries are a record set");
        let names: Vec<&str> = got.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(
            !got.fields.iter().any(|f| f.selector.contains("badge")),
            "one record in four is not a column: {names:?}"
        );
    }
}
