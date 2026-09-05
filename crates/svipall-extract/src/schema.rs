//! Declarative CSS-to-JSON extraction.
//!
//! Asking for "the 30 products with price and URL" used to mean returning eight thousand
//! characters of markdown and letting the model parse it. This returns the thirty objects instead.
//!
//! The shape deliberately mirrors the `JsonCssExtractionStrategy` format that models already know
//! from Crawl4AI: there is no reason to invent a second dialect for the same idea and make every
//! caller learn it.

use super::heal::{self, Fingerprints, Healed};
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Key of the base selector in a `Fingerprints` map. A field cannot be called this.
pub const BASE_KEY: &str = "";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    #[default]
    Text,
    Html,
    Markdown,
    Attribute,
    Exists,
    Number,
    /// Repeated values from every match of the selector.
    List,
    /// Nested objects, one per match, built from `fields`.
    Nested,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    pub name: String,
    /// Omit to use the base element itself.
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: FieldType,
    #[serde(default)]
    pub attribute: Option<String>,
    /// Resolve a URL-bearing attribute against the page URL.
    #[serde(default)]
    pub absolute: bool,
    /// First capture group, or the whole match when there are no groups.
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Schema {
    #[serde(default)]
    pub name: Option<String>,
    pub base_selector: String,
    pub fields: Vec<Field>,
}

/// Selectors and regexes compiled once.
///
/// Compiling per item per field is the expensive part — `Selector::parse` dominates, and a 30-item
/// grid with 6 fields would otherwise pay for it 180 times.
pub struct CompiledSchema {
    name: String,
    base: Selector,
    base_src: String,
    fields: Vec<CompiledField>,
    /// What each selector found last time, so a selector that finds nothing today can be
    /// relocated. Empty unless the caller remembered something.
    fingerprints: Fingerprints,
}

#[derive(Clone)]
struct CompiledField {
    name: String,
    selector: Option<Selector>,
    selector_src: Option<String>,
    kind: FieldType,
    attribute: Option<String>,
    absolute: bool,
    regex: Option<regex::Regex>,
    default: Option<Value>,
    fields: Vec<CompiledField>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SchemaResult {
    pub name: String,
    pub matched: usize,
    pub items: Vec<Value>,
    /// Problems with the schema itself, reported rather than thrown: a bad selector should tell
    /// the caller which field is wrong, not fail the whole fetch.
    pub errors: Vec<String>,
    /// Selectors that matched nothing and were relocated by fingerprint. Empty is the normal case.
    pub healed: Vec<Healed>,
    /// What the selectors found on this page, for the caller to remember. Never sent to a model.
    #[serde(skip)]
    pub fingerprints: Fingerprints,
}

fn compile_fields(fields: &[Field], errors: &mut Vec<String>) -> Vec<CompiledField> {
    fields
        .iter()
        .filter_map(|f| {
            let selector = match &f.selector {
                Some(s) => match Selector::parse(s) {
                    Ok(sel) => Some(sel),
                    Err(_) => {
                        errors.push(format!("field '{}': invalid selector '{}'", f.name, s));
                        return None;
                    }
                },
                None => None,
            };
            let regex = match &f.regex {
                Some(r) => match regex::Regex::new(r) {
                    Ok(re) => Some(re),
                    Err(e) => {
                        errors.push(format!("field '{}': invalid regex: {e}", f.name));
                        return None;
                    }
                },
                None => None,
            };
            Some(CompiledField {
                name: f.name.clone(),
                selector,
                selector_src: f.selector.clone(),
                kind: f.kind,
                attribute: f.attribute.clone(),
                absolute: f.absolute,
                regex,
                default: f.default.clone(),
                fields: compile_fields(&f.fields, errors),
            })
        })
        .collect()
}

impl CompiledSchema {
    pub fn compile(schema: &Schema) -> Result<(Self, Vec<String>), String> {
        let base = Selector::parse(&schema.base_selector)
            .map_err(|_| format!("invalid base_selector '{}'", schema.base_selector))?;
        let mut errors = Vec::new();
        let fields = compile_fields(&schema.fields, &mut errors);
        Ok((
            Self {
                name: schema.name.clone().unwrap_or_else(|| "items".into()),
                base,
                base_src: schema.base_selector.clone(),
                fields,
                fingerprints: Fingerprints::new(),
            },
            errors,
        ))
    }

    pub fn from_value(v: &Value) -> Result<(Self, Vec<String>), String> {
        let schema: Schema =
            serde_json::from_value(v.clone()).map_err(|e| format!("schema is not valid: {e}"))?;
        Self::compile(&schema)
    }

    /// The schema's name, which is also what its fingerprints are remembered under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Attach what the selectors found on an earlier visit.
    pub fn with_fingerprints(mut self, fingerprints: Fingerprints) -> Self {
        self.fingerprints = fingerprints;
        self
    }
}

fn text_of(el: ElementRef<'_>) -> String {
    el.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Numbers as written by humans: `1,234.56`, `1.234,56`, `€19.99`.
fn parse_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    // Whichever separator comes last is the decimal one.
    let last_dot = cleaned.rfind('.');
    let last_comma = cleaned.rfind(',');
    let normalised = match (last_dot, last_comma) {
        (Some(d), Some(c)) if c > d => cleaned.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => cleaned.replace(',', ""),
        (None, Some(c)) => {
            // A lone comma is a decimal point when it is followed by one or two digits.
            if cleaned.len() - c - 1 <= 2 {
                cleaned.replace(',', ".")
            } else {
                cleaned.replace(',', "")
            }
        }
        _ => cleaned,
    };
    normalised.parse().ok()
}

fn field_value(field: &CompiledField, base: ElementRef<'_>, page_url: Option<&url::Url>) -> Value {
    let pick = |el: ElementRef<'_>| -> Value {
        let raw = match field.kind {
            FieldType::Attribute => field
                .attribute
                .as_deref()
                .and_then(|a| el.value().attr(a))
                .map(str::to_string),
            FieldType::Html => Some(el.inner_html()),
            FieldType::Markdown => Some(super::extract_markdown_opts(
                &el.html(),
                &super::ExtractOpts::default(),
            )),
            _ => Some(text_of(el)),
        };
        let Some(mut s) = raw else {
            return Value::Null;
        };
        if field.absolute {
            if let Some(base_url) = page_url {
                if let Ok(joined) = base_url.join(s.trim()) {
                    s = joined.to_string();
                }
            }
        }
        if let Some(re) = &field.regex {
            match re.captures(&s) {
                Some(c) => {
                    s = c
                        .get(1)
                        .or_else(|| c.get(0))
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default()
                }
                None => return Value::Null,
            }
        }
        match field.kind {
            FieldType::Number => parse_number(&s)
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            _ => Value::String(s.trim().to_string()),
        }
    };

    match field.kind {
        FieldType::Exists => Value::Bool(match &field.selector {
            Some(s) => base.select(s).next().is_some(),
            None => true,
        }),
        FieldType::List => {
            let items: Vec<Value> = match &field.selector {
                Some(s) => base.select(s).map(pick).collect(),
                None => vec![pick(base)],
            };
            Value::Array(items)
        }
        FieldType::Nested => {
            let build = |el: ElementRef<'_>| {
                let mut obj = Map::new();
                for sub in &field.fields {
                    obj.insert(sub.name.clone(), field_value(sub, el, page_url));
                }
                Value::Object(obj)
            };
            match &field.selector {
                Some(s) => Value::Array(base.select(s).map(build).collect()),
                None => build(base),
            }
        }
        _ => {
            let el = match &field.selector {
                Some(s) => base.select(s).next(),
                None => Some(base),
            };
            match el {
                Some(e) => {
                    let v = pick(e);
                    if v.is_null() {
                        field.default.clone().unwrap_or(Value::Null)
                    } else {
                        v
                    }
                }
                None => field.default.clone().unwrap_or(Value::Null),
            }
        }
    }
}

pub(crate) fn extract_from(
    doc: &Html,
    schema: &CompiledSchema,
    page_url: Option<&str>,
) -> SchemaResult {
    let base_url = page_url.and_then(|u| url::Url::parse(u).ok());
    let mut healed = Vec::new();
    let mut errors = Vec::new();
    let mut fingerprints = Fingerprints::new();

    // The base selector first. Nothing matched and a fingerprint remembers what it used to find:
    // look for that element and take a selector that reaches it and its siblings.
    let mut bases: Vec<ElementRef<'_>> = doc.select(&schema.base).collect();
    if bases.is_empty() {
        if let Some(fp) = schema.fingerprints.get(BASE_KEY) {
            match heal::relocate(doc, fp) {
                Some(m) => {
                    let to = heal::selector_for(m.element, None, false);
                    let found: Vec<ElementRef<'_>> = Selector::parse(&to)
                        .map(|s| doc.select(&s).collect())
                        .unwrap_or_default();
                    if found.is_empty() {
                        errors.push(format!(
                            "base_selector '{}' matched nothing; the element it used to find was \
                             relocated but no selector reaches it",
                            schema.base_src
                        ));
                    } else {
                        healed.push(Healed {
                            field: "base_selector".into(),
                            from: schema.base_src.clone(),
                            to,
                            score: m.score,
                        });
                        bases = found;
                    }
                }
                None => errors.push(format!(
                    "base_selector '{}' matched nothing and no similar element was found",
                    schema.base_src
                )),
            }
        }
    }

    // Then each top-level field, judged across every item: a field that matches nowhere on a page
    // that has items is a broken selector, not an optional field.
    let mut fields: Vec<CompiledField> = schema.fields.clone();
    if let Some(first) = bases.first().copied() {
        for f in &mut fields {
            let Some(sel) = &f.selector else { continue };
            let src = f.selector_src.clone().unwrap_or_default();
            let any = bases.iter().any(|b| b.select(sel).next().is_some());
            if any {
                continue;
            }
            let Some(fp) = schema.fingerprints.get(&f.name) else {
                continue;
            };
            match heal::relocate_within(first, fp) {
                Some(m) => {
                    let to = heal::selector_for(m.element, Some(first), true);
                    let parsed = Selector::parse(&to).ok();
                    match parsed {
                        Some(s) if first.select(&s).next().is_some() => {
                            healed.push(Healed {
                                field: f.name.clone(),
                                from: src,
                                to: to.clone(),
                                score: m.score,
                            });
                            f.selector = Some(s);
                            f.selector_src = Some(to);
                        }
                        _ => errors.push(format!(
                            "field '{}': selector '{src}' matched nothing; the element it used \
                             to find was relocated but no selector reaches it",
                            f.name
                        )),
                    }
                }
                None => errors.push(format!(
                    "field '{}': selector '{src}' matched nothing and no similar element was found",
                    f.name
                )),
            }
        }
    }

    let items: Vec<Value> = bases
        .iter()
        .map(|el| {
            let mut obj = Map::new();
            for f in &fields {
                obj.insert(f.name.clone(), field_value(f, *el, base_url.as_ref()));
            }
            Value::Object(obj)
        })
        .collect();

    // What was found, for next time. Only elements that were actually seen are remembered, so a
    // page that has no items does not overwrite what a page that had them taught us.
    if let Some(first) = bases.first().copied() {
        fingerprints.insert(BASE_KEY.to_string(), heal::fingerprint(first));
        for f in &fields {
            if let Some(sel) = &f.selector {
                if let Some(el) = bases.iter().find_map(|b| b.select(sel).next()) {
                    fingerprints.insert(f.name.clone(), heal::fingerprint(el));
                }
            }
        }
    }

    SchemaResult {
        name: schema.name.clone(),
        matched: items.len(),
        items,
        errors,
        healed,
        fingerprints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_page, MarkdownOpts, ParseWants};

    fn grid(n: usize) -> String {
        let mut html = String::from("<html><body>");
        for i in 0..n {
            html.push_str(&format!(
                "<div class=\"product\">\
                   <h3><a href=\"/p/{i}\">Product {i}</a></h3>\
                   <span class=\"price\">€{i}9.99</span>\
                   {}\
                   <ul><li class=\"spec\">spec-a{i}</li><li class=\"spec\">spec-b{i}</li></ul>\
                 </div>",
                if i % 2 == 0 {
                    "<span class=\"in-stock\">yes</span>"
                } else {
                    ""
                }
            ));
        }
        html.push_str("</body></html>");
        html
    }

    fn run(html: &str, schema_json: &str, url: Option<&str>) -> SchemaResult {
        let v: Value = serde_json::from_str(schema_json).unwrap();
        let (compiled, errs) = CompiledSchema::from_value(&v).unwrap();
        assert!(errs.is_empty(), "unexpected schema errors: {errs:?}");
        let wants = ParseWants {
            schema: Some(compiled),
            schema_base_url: url.map(str::to_string),
            ..Default::default()
        };
        parse_page(html, &wants).extracted.expect("schema result")
    }

    #[test]
    fn a_product_grid_yields_one_object_per_card() {
        let r = run(
            &grid(30),
            r#"{"name":"products","base_selector":"div.product",
                "fields":[{"name":"title","selector":"h3 a"}]}"#,
            None,
        );
        assert_eq!(r.matched, 30);
        assert_eq!(r.items[0]["title"], "Product 0");
        assert_eq!(r.items[29]["title"], "Product 29");
    }

    #[test]
    fn attributes_can_be_resolved_against_the_page_url() {
        let r = run(
            &grid(2),
            r#"{"base_selector":"div.product","fields":[
                {"name":"url","selector":"h3 a","type":"attribute","attribute":"href","absolute":true}]}"#,
            Some("https://shop.test/catalog/page"),
        );
        assert_eq!(r.items[0]["url"], "https://shop.test/p/0");
    }

    #[test]
    fn numbers_survive_currency_symbols_and_separators() {
        let r = run(
            &grid(2),
            r#"{"base_selector":"div.product","fields":[
                {"name":"price","selector":".price","type":"number"}]}"#,
            None,
        );
        assert_eq!(r.items[0]["price"], 9.99);
        assert_eq!(r.items[1]["price"], 19.99);
    }

    #[test]
    fn european_and_anglo_number_formats_both_parse() {
        assert_eq!(parse_number("1,234.56"), Some(1234.56));
        assert_eq!(parse_number("1.234,56"), Some(1234.56));
        assert_eq!(parse_number("€19,99"), Some(19.99));
        assert_eq!(parse_number("1,234"), Some(1234.0));
        assert_eq!(parse_number("no digits"), None);
    }

    #[test]
    fn exists_reports_a_boolean_per_item() {
        let r = run(
            &grid(4),
            r#"{"base_selector":"div.product","fields":[
                {"name":"stock","selector":".in-stock","type":"exists"}]}"#,
            None,
        );
        assert_eq!(r.items[0]["stock"], true);
        assert_eq!(r.items[1]["stock"], false);
    }

    #[test]
    fn a_list_field_collects_every_match() {
        let r = run(
            &grid(1),
            r#"{"base_selector":"div.product","fields":[
                {"name":"specs","selector":"li.spec","type":"list"}]}"#,
            None,
        );
        assert_eq!(r.items[0]["specs"].as_array().unwrap().len(), 2);
        assert_eq!(r.items[0]["specs"][1], "spec-b0");
    }

    #[test]
    fn a_missing_field_is_null_or_its_default() {
        let r = run(
            &grid(1),
            r#"{"base_selector":"div.product","fields":[
                {"name":"absent","selector":".nope"},
                {"name":"withdefault","selector":".nope","default":"n/a"}]}"#,
            None,
        );
        assert_eq!(r.items[0]["absent"], Value::Null);
        assert_eq!(r.items[0]["withdefault"], "n/a");
    }

    #[test]
    fn a_regex_narrows_the_captured_text() {
        let r = run(
            &grid(1),
            r#"{"base_selector":"div.product","fields":[
                {"name":"num","selector":"h3 a","regex":"Product (\\d+)"}]}"#,
            None,
        );
        assert_eq!(r.items[0]["num"], "0");
    }

    #[test]
    fn an_invalid_selector_is_a_reported_error_not_a_panic() {
        let v: Value = serde_json::from_str(
            r#"{"base_selector":"div.product","fields":[{"name":"p","selector":".pr ice{"}]}"#,
        )
        .unwrap();
        let (_, errs) = CompiledSchema::from_value(&v).unwrap();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("field 'p'"), "{errs:?}");
    }

    #[test]
    fn an_invalid_base_selector_is_an_error() {
        let v: Value = serde_json::from_str(r#"{"base_selector":"div{{{","fields":[]}"#).unwrap();
        assert!(CompiledSchema::from_value(&v).is_err());
    }

    #[test]
    fn a_base_selector_matching_nothing_yields_an_empty_result() {
        let r = run(
            &grid(3),
            r#"{"base_selector":"div.nothing-here","fields":[{"name":"t","selector":"h3"}]}"#,
            None,
        );
        assert_eq!(r.matched, 0);
        assert!(r.items.is_empty());
    }

    #[test]
    fn nested_fields_build_objects() {
        let html = "<div class=row><span class=k>colour</span><span class=v>red</span>\
                    <span class=k>size</span><span class=v>large</span></div>";
        let r = run(
            html,
            r#"{"base_selector":"div.row","fields":[
                {"name":"keys","selector":"span.k","type":"list"},
                {"name":"vals","selector":"span.v","type":"list"}]}"#,
            None,
        );
        assert_eq!(r.items[0]["keys"][0], "colour");
        assert_eq!(r.items[0]["vals"][1], "large");
    }

    /// The whole point: a schema request costs one parse, like everything else.
    #[test]
    fn extraction_costs_a_single_parse() {
        let v: Value = serde_json::from_str(
            r#"{"base_selector":"div.product","fields":[{"name":"t","selector":"h3"}]}"#,
        )
        .unwrap();
        let (compiled, _) = CompiledSchema::from_value(&v).unwrap();
        let before = crate::dom_parse_count();
        let wants = ParseWants {
            text: true,
            title: true,
            markdown: Some(MarkdownOpts::default()),
            schema: Some(compiled),
            ..Default::default()
        };
        let _ = parse_page(&grid(5), &wants);
        assert_eq!(crate::dom_parse_count() - before, 1);
    }
}

#[cfg(test)]
mod heal_tests {
    use super::*;
    use crate::{parse_page, ParseWants};

    const V1: &str = "<html><body><main>\
        <div class=\"card\"><h2 class=\"title\">Cup</h2><span class=\"price\">3</span></div>\
        <div class=\"card\"><h2 class=\"title\">Pot</h2><span class=\"price\">9</span></div>\
        </main></body></html>";

    fn schema() -> CompiledSchema {
        let v: Value = serde_json::from_str(
            r#"{"name":"products","base_selector":"div.card","fields":[
                {"name":"title","selector":"h2.title"},
                {"name":"price","selector":"span.price","type":"number"}]}"#,
        )
        .unwrap();
        CompiledSchema::from_value(&v).unwrap().0
    }

    fn extract(html: &str, schema: CompiledSchema) -> SchemaResult {
        parse_page(
            html,
            &ParseWants {
                schema: Some(schema),
                ..Default::default()
            },
        )
        .extracted
        .unwrap()
    }

    #[test]
    fn a_field_whose_selector_broke_is_relocated_and_reported() {
        let first = extract(V1, schema());
        assert_eq!(first.matched, 2);
        assert!(first.healed.is_empty());
        assert!(first.fingerprints.contains_key("price"));

        let v2 = V1.replace("class=\"price\"", "class=\"cost\"");
        let second = extract(&v2, schema().with_fingerprints(first.fingerprints));
        assert_eq!(second.matched, 2);
        assert_eq!(second.items[1]["price"], 9.0, "{:?}", second.items);
        assert_eq!(second.healed.len(), 1, "{:?}", second.healed);
        assert_eq!(second.healed[0].field, "price");
        assert_eq!(second.healed[0].from, "span.price");
        assert_eq!(second.healed[0].to, "span.cost");
        assert!(second.errors.is_empty(), "{:?}", second.errors);
    }

    #[test]
    fn a_base_selector_that_broke_is_relocated_before_any_field_is_read() {
        let first = extract(V1, schema());
        let v2 = V1.replace("class=\"card\"", "class=\"product-card\"");
        let second = extract(&v2, schema().with_fingerprints(first.fingerprints));
        assert_eq!(second.matched, 2, "{:?}", second);
        assert_eq!(second.items[0]["title"], "Cup");
        assert_eq!(second.healed[0].field, "base_selector");
        assert_eq!(second.healed[0].to, "main > div.product-card");
    }

    #[test]
    fn without_a_memory_a_broken_selector_is_still_just_null() {
        let v2 = V1.replace("class=\"price\"", "class=\"cost\"");
        let r = extract(&v2, schema());
        assert_eq!(r.matched, 2);
        assert!(r.items[0]["price"].is_null());
        assert!(r.healed.is_empty());
        assert!(
            r.errors.is_empty(),
            "nothing to say when nothing was remembered"
        );
    }

    #[test]
    fn a_selector_that_still_matches_is_never_second_guessed() {
        let first = extract(V1, schema());
        // The remembered price element is gone, but the selector still finds one: keep it.
        let v2 = V1.replace("<span class=\"price\">3</span>", "<b class=\"price\">3</b>");
        let second = extract(&v2, schema().with_fingerprints(first.fingerprints));
        assert!(second.healed.is_empty());
        assert!(
            second.items[0]["price"].is_null(),
            "the first card really has no span.price"
        );
        assert_eq!(second.items[1]["price"], 9.0);
    }

    #[test]
    fn a_memory_that_matches_nothing_similar_is_an_error_not_a_guess() {
        let first = extract(V1, schema());
        let v2 = V1
            .replace("<span class=\"price\">3</span>", "")
            .replace("<span class=\"price\">9</span>", "");
        let second = extract(&v2, schema().with_fingerprints(first.fingerprints));
        assert!(second.healed.is_empty());
        assert_eq!(second.errors.len(), 1, "{:?}", second.errors);
        assert!(second.errors[0].contains("price"));
    }
}
