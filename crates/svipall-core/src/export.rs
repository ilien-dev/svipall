//! Results as a file, instead of as forty thousand tokens of context.
//!
//! A crawl of two hundred product pages is a table. Returning it through the model means the model
//! reads every row, pays for every row, and then writes most of them back out again to save them —
//! which is the expensive way to do a file copy.
//!
//! So the rows go to disk in whatever shape the next tool wants, and the model gets a path and a
//! count. Three formats, none of which needs a dependency: CSV for a spreadsheet, JSON for a
//! program, JSON Lines for anything that streams.

use serde_json::Value;

/// What to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
    Jsonl,
}

impl Format {
    /// Read the format off the file name, which is what the caller already had to choose.
    ///
    /// Asking for it separately means the two can disagree, and a `.csv` full of JSON is a bad
    /// afternoon for whoever opens it next.
    pub fn of(path: &str) -> Option<Format> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "csv" => Some(Format::Csv),
            "json" => Some(Format::Json),
            "jsonl" | "ndjson" => Some(Format::Jsonl),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Json => "json",
            Format::Jsonl => "jsonl",
        }
    }
}

/// Render rows in the chosen format.
pub fn render(rows: &[Value], format: Format) -> String {
    match format {
        Format::Csv => to_csv(rows),
        Format::Json => serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".into()),
        Format::Jsonl => to_jsonl(rows),
    }
}

/// One object per line. A row that will not serialise is skipped rather than taking the file with
/// it: a partial export is worth more than none.
pub fn to_jsonl(rows: &[Value]) -> String {
    let mut out = String::new();
    for r in rows {
        if let Ok(line) = serde_json::to_string(r) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// A table whose columns are the union of every row's keys.
///
/// Order follows the order each row presents its keys in, first seen first, and a row that
/// introduces a new key adds it at the end. Worth being exact about what that means in practice:
/// the values here come from `serde_json`, whose objects are sorted maps, so the header comes out
/// alphabetical. That is a property of the input rather than a choice made here — hand this
/// ordered objects and it preserves their order.
pub fn to_csv(rows: &[Value]) -> String {
    let mut columns: Vec<String> = Vec::new();
    for r in rows {
        if let Some(o) = r.as_object() {
            for k in o.keys() {
                if !columns.iter().any(|c| c == k) {
                    columns.push(k.clone());
                }
            }
        }
    }
    if columns.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(
        &columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("\r\n");
    for r in rows {
        let line: Vec<String> = columns
            .iter()
            .map(|c| quote(&cell(r.get(c))))
            .collect::<Vec<_>>();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

/// One value as one cell.
///
/// A nested object or array becomes its JSON rather than `[object Object]`: the information is
/// still there for whoever needs it, and a spreadsheet shows it as text either way.
fn cell(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Quote a field the way every spreadsheet expects.
///
/// A leading `=`, `+`, `-` or `@` is prefixed with a quote character. Spreadsheets treat those as
/// the start of a formula, so a scraped cell reading `=cmd|'/c calc'!A1` is a live formula in the
/// file the moment somebody opens it. Escaping it is one character and closes the whole class.
fn quote(s: &str) -> String {
    let dangerous = s.starts_with(['=', '+', '@']) || (s.starts_with('-') && !looks_numeric(s));
    let needs_quotes = dangerous
        || s.contains(',')
        || s.contains('"')
        || s.contains('\n')
        || s.contains('\r')
        || s.starts_with(' ')
        || s.ends_with(' ');
    if !needs_quotes {
        return s.to_string();
    }
    let body = s.replace('"', "\"\"");
    if dangerous {
        format!("\"'{body}\"")
    } else {
        format!("\"{body}\"")
    }
}

fn looks_numeric(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            json!({"name": "Blue shoes", "price": 42.5, "stock": true}),
            json!({"name": "Red hat", "price": 9, "stock": false}),
        ]
    }

    #[test]
    fn every_row_lands_under_the_right_column() {
        // The header names the columns and each row fills them in the same order. Getting this
        // wrong shifts a whole table by one field and nothing about the file looks broken.
        let csv = to_csv(&rows());
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some("name,price,stock"));
        assert_eq!(lines.next(), Some("Blue shoes,42.5,true"));
        assert_eq!(lines.next(), Some("Red hat,9,false"));
    }

    #[test]
    fn the_header_order_is_the_order_the_rows_present_their_keys_in() {
        // Not a choice made here: these values come from `serde_json`, whose objects are sorted
        // maps, so the header comes out alphabetical. Worth pinning so nobody reads the code and
        // concludes otherwise.
        let v = vec![json!({"zebra": 1, "apple": 2})];
        assert!(to_csv(&v).starts_with("apple,zebra\r\n"), "{}", to_csv(&v));
    }

    #[test]
    fn a_column_that_only_some_rows_have_is_still_a_column() {
        let mixed = vec![json!({"a": 1}), json!({"a": 2, "b": 3})];
        let csv = to_csv(&mixed);
        assert!(csv.starts_with("a,b\r\n"), "{csv}");
        // The row without it gets an empty cell, not a short line.
        assert!(csv.contains("1,\r\n"), "{csv}");
    }

    #[test]
    fn a_scraped_cell_can_never_become_a_formula_in_a_spreadsheet() {
        // A cell reading `=cmd|'/c calc'!A1` is live the moment someone opens the file, and the
        // text came off a page nobody controls.
        let evil = vec![json!({"note": "=cmd|'/c calc'!A1", "b": "+1+1", "c": "@SUM(A1)"})];
        let csv = to_csv(&evil);
        assert!(csv.contains("\"'=cmd"), "{csv}");
        assert!(csv.contains("\"'+1+1\""), "{csv}");
        assert!(csv.contains("\"'@SUM(A1)\""), "{csv}");
    }

    #[test]
    fn a_negative_number_is_a_number_and_not_treated_as_a_formula() {
        // Quoting every leading minus would turn a price column into text.
        let v = vec![json!({"delta": "-12.5"})];
        assert!(to_csv(&v).contains("delta\r\n-12.5\r\n"), "{}", to_csv(&v));
    }

    #[test]
    fn commas_quotes_and_newlines_survive_the_round_trip() {
        let v = vec![json!({"t": "a,b \"quoted\" and\na newline"})];
        let csv = to_csv(&v);
        assert!(
            csv.contains("\"a,b \"\"quoted\"\" and\na newline\""),
            "{csv}"
        );
    }

    #[test]
    fn a_nested_value_keeps_its_content_instead_of_becoming_a_type_name() {
        let v = vec![json!({"tags": ["a", "b"], "meta": {"x": 1}})];
        let csv = to_csv(&v);
        assert!(csv.contains(r#"[""a"",""b""]"#), "{csv}");
        assert!(csv.contains("x"), "{csv}");
    }

    #[test]
    fn json_lines_is_one_row_per_line_and_nothing_else() {
        let out = to_jsonl(&rows());
        assert_eq!(out.lines().count(), 2);
        assert!(
            out.ends_with('\n'),
            "a streaming reader needs the last newline"
        );
        for line in out.lines() {
            serde_json::from_str::<Value>(line).expect("each line parses on its own");
        }
    }

    #[test]
    fn nothing_to_export_is_an_empty_file_rather_than_a_broken_one() {
        assert_eq!(to_csv(&[]), "");
        assert_eq!(to_jsonl(&[]), "");
        assert_eq!(render(&[], Format::Json), "[]");
    }

    #[test]
    fn rows_that_are_not_objects_do_not_take_the_export_down() {
        let odd = vec![json!("just a string"), json!({"a": 1})];
        let csv = to_csv(&odd);
        assert!(csv.starts_with("a\r\n"), "{csv}");
        assert_eq!(to_jsonl(&odd).lines().count(), 2);
    }

    #[test]
    fn the_format_comes_off_the_name_the_caller_already_chose() {
        // Asking for it separately means the two can disagree, and a .csv full of JSON is somebody
        // else's bad afternoon.
        assert_eq!(Format::of("out/items.csv"), Some(Format::Csv));
        assert_eq!(Format::of("items.JSON"), Some(Format::Json));
        assert_eq!(Format::of("items.jsonl"), Some(Format::Jsonl));
        assert_eq!(Format::of("items.ndjson"), Some(Format::Jsonl));
        assert_eq!(Format::of("items.parquet"), None);
        assert_eq!(Format::of("items"), None);
    }
}
