//! HTML tables to GFM.
//!
//! The previous code emitted `cell | ` from `<td>` and a newline from `<tr>`, in streaming order.
//! That cannot produce a valid GFM table: the separator row needs the column count, which is not
//! known until the row has been walked, and the leading pipe needs to be written before the first
//! cell. The output was pipe-separated text that no markdown renderer would read as a table.
//!
//! So a `<table>` is handled as a closed sub-extraction: collect the whole thing, then render it.

use scraper::{ElementRef, Selector};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Table {
    pub caption: Option<String>,
    /// May be empty: a table with no header row still renders, with an empty header, rather than
    /// inventing column names that were never in the document.
    pub header: Vec<String>,
    pub align: Vec<Align>,
    pub rows: Vec<Vec<String>>,
    pub width: usize,
}

/// Not every `<table>` is data. Plenty are layout scaffolding, and rendering those as tables
/// produces something far worse to read than the prose they contain.
pub enum TableShape {
    Data(Table),
    Layout,
}

fn sel(s: &str) -> Option<Selector> {
    Selector::parse(s).ok()
}

/// `<tr>` elements belonging to *this* table.
///
/// Descending blindly is the classic bug: a nested table's rows get mixed into the outer one and
/// every column lines up wrong from that point on.
fn own_rows<'a>(table: ElementRef<'a>) -> Vec<ElementRef<'a>> {
    fn walk<'a>(el: ElementRef<'a>, out: &mut Vec<ElementRef<'a>>) {
        for child in el.child_elements() {
            match child.value().name() {
                "table" => {} // a nested table owns its own rows
                "tr" => out.push(child),
                _ => walk(child, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(table, &mut out);
    out
}

fn attr_align(el: ElementRef<'_>) -> Align {
    let raw = el
        .value()
        .attr("align")
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            let style = el.value().attr("style")?.to_ascii_lowercase();
            let idx = style.find("text-align")?;
            Some(
                style[idx..]
                    .split(':')
                    .nth(1)?
                    .split(';')
                    .next()?
                    .trim()
                    .to_string(),
            )
        })
        .unwrap_or_default();
    match raw.as_str() {
        "left" => Align::Left,
        "center" => Align::Center,
        "right" => Align::Right,
        _ => Align::None,
    }
}

/// Cell text with newlines flattened, since a table cell is one line. Pipes are left alone here:
/// escaping them is a markdown concern and happens in `render`, so a row exported as CSV or JSON
/// carries the text the page had.
fn clean_cell(s: &str) -> String {
    let flat = s.replace(['\n', '\r'], " ");
    flat.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The table as one object per row, named by the header. A table without a header names its
/// columns by position (`col_1`…), because a column name is never invented from a data cell.
/// `index` is added as `table` when the page had more than one table, so rows from several tables
/// can share one file and still be told apart.
pub fn to_rows(t: &Table, index: Option<usize>) -> Vec<serde_json::Value> {
    let has_header = t.header.iter().any(|h| !h.is_empty());
    let names: Vec<String> = (0..t.width)
        .map(|i| {
            let h = t.header.get(i).map(String::as_str).unwrap_or("");
            if has_header && !h.is_empty() {
                h.to_string()
            } else {
                format!("col_{}", i + 1)
            }
        })
        .collect();
    t.rows
        .iter()
        .map(|r| {
            let mut obj = serde_json::Map::new();
            if let Some(i) = index {
                obj.insert("table".into(), serde_json::Value::from(i));
            }
            for (name, cell) in names.iter().zip(r) {
                obj.insert(name.clone(), serde_json::Value::String(cell.clone()));
            }
            serde_json::Value::Object(obj)
        })
        .collect()
}

struct RawRow {
    cells: Vec<String>,
    all_header: bool,
    aligns: Vec<Align>,
}

/// Walk rows, expanding `colspan` and carrying `rowspan` down.
///
/// GFM has no spans, so `colspan` repeats the value across the columns it covered — more useful to
/// a reader than blanks — and `rowspan` fills the cell into the rows below, without which any real
/// Wikipedia table comes out misaligned from the first spanning cell onward.
fn collect_rows(
    table: ElementRef<'_>,
    render_cell: &mut dyn FnMut(ElementRef<'_>) -> String,
) -> Vec<RawRow> {
    let mut carry: Vec<Option<(String, u16)>> = Vec::new();
    let mut out = Vec::new();

    for tr in own_rows(table) {
        let mut cells: Vec<String> = Vec::new();
        let mut aligns: Vec<Align> = Vec::new();
        let mut all_header = true;
        let mut any_cell = false;

        // Anything still spanning from an earlier row occupies its column first.
        let mut col = 0usize;
        // A free function rather than a closure: it needs `carry` mutably at the same time as the
        // loop below, which a capturing closure would not allow.
        fn take_carry(
            carry: &mut [Option<(String, u16)>],
            cells: &mut Vec<String>,
            aligns: &mut Vec<Align>,
            col: &mut usize,
        ) {
            while let Some(Some((text, left))) = carry.get(*col).cloned() {
                cells.push(text.clone());
                aligns.push(Align::None);
                carry[*col] = if left > 1 {
                    Some((text, left - 1))
                } else {
                    None
                };
                *col += 1;
            }
        }

        for cell in tr.child_elements() {
            let name = cell.value().name();
            if name != "td" && name != "th" {
                continue;
            }
            any_cell = true;
            if name == "td" {
                all_header = false;
            }
            take_carry(&mut carry, &mut cells, &mut aligns, &mut col);

            let text = clean_cell(&render_cell(cell));
            let align = attr_align(cell);
            let colspan = cell
                .value()
                .attr("colspan")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(1)
                .clamp(1, 32);
            let rowspan = cell
                .value()
                .attr("rowspan")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(1)
                .clamp(1, 64);

            for _ in 0..colspan {
                if carry.len() <= col {
                    carry.resize(col + 1, None);
                }
                if rowspan > 1 {
                    carry[col] = Some((text.clone(), rowspan - 1));
                }
                cells.push(text.clone());
                aligns.push(align);
                col += 1;
            }
        }
        take_carry(&mut carry, &mut cells, &mut aligns, &mut col);

        if any_cell || !cells.is_empty() {
            out.push(RawRow {
                cells,
                all_header,
                aligns,
            });
        }
    }
    out
}

/// Decide whether this is a data table or page furniture.
pub fn parse(
    table: ElementRef<'_>,
    render_cell: &mut dyn FnMut(ElementRef<'_>) -> String,
) -> TableShape {
    let role = table
        .value()
        .attr("role")
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if role == "presentation" || role == "none" {
        return TableShape::Layout;
    }
    let has_nested = sel("table")
        .map(|s| table.select(&s).next().is_some())
        .unwrap_or(false);
    if has_nested {
        return TableShape::Layout;
    }

    let caption = sel("caption")
        .and_then(|s| table.select(&s).next())
        .map(|c| clean_cell(&c.text().collect::<String>()))
        .filter(|c| !c.is_empty());
    let has_th = sel("th")
        .map(|s| table.select(&s).next().is_some())
        .unwrap_or(false);

    let mut raw = collect_rows(table, render_cell);
    if raw.is_empty() {
        return TableShape::Layout;
    }
    let width = raw.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    if width <= 1 || (raw.len() <= 1 && !has_th) {
        return TableShape::Layout;
    }

    // Prose laid out in a grid: long cells mean paragraphs, not data.
    let total: usize = raw
        .iter()
        .flat_map(|r| r.cells.iter())
        .map(|c| c.len())
        .sum();
    let count: usize = raw.iter().map(|r| r.cells.len()).sum();
    let mean = total.checked_div(count).unwrap_or(0);
    let explicit = role == "table" || role == "grid" || caption.is_some() || has_th;
    if !explicit && mean > 200 {
        return TableShape::Layout;
    }

    // Header: an explicit <thead> row, else a first row that is entirely <th>, else none.
    let thead_rows: Vec<usize> = sel("thead")
        .and_then(|s| table.select(&s).next())
        .map(|thead| {
            let ids: Vec<_> = own_rows(thead).iter().map(|r| r.id()).collect();
            own_rows(table)
                .iter()
                .enumerate()
                .filter(|(_, r)| ids.contains(&r.id()))
                .map(|(i, _)| i)
                .collect()
        })
        .unwrap_or_default();

    let header_idx = thead_rows
        .last()
        .copied()
        .or_else(|| (raw.first().map(|r| r.all_header) == Some(true)).then_some(0));

    let (header, align) = match header_idx {
        Some(i) if i < raw.len() => {
            let row = raw.remove(i);
            (row.cells, row.aligns)
        }
        _ => (Vec::new(), Vec::new()),
    };

    let mut rows: Vec<Vec<String>> = raw.into_iter().map(|r| r.cells).collect();
    for r in &mut rows {
        r.resize(width, String::new());
    }
    let mut header = header;
    header.resize(width, String::new());
    let mut align = align;
    align.resize(width, Align::None);

    if rows.is_empty() {
        return TableShape::Layout;
    }
    TableShape::Data(Table {
        caption,
        header,
        align,
        rows,
        width,
    })
}

pub fn render(t: &Table, out: &mut String) {
    if let Some(c) = &t.caption {
        let _ = write!(out, "**{}**\n\n", c);
    }
    let row = |cells: &[String]| {
        let mut s = String::from("|");
        for c in cells {
            s.push(' ');
            // A pipe inside a cell would end the cell; this is the markdown-specific escape.
            s.push_str(&c.replace('|', "\\|"));
            s.push_str(" |");
        }
        s.push('\n');
        s
    };
    out.push_str(&row(&t.header));
    // The separator is what makes it a table rather than lines with pipes in them.
    out.push('|');
    for a in &t.align {
        out.push_str(match a {
            Align::Left => " :--- |",
            Align::Center => " :---: |",
            Align::Right => " ---: |",
            Align::None => " --- |",
        });
    }
    out.push('\n');
    for r in &t.rows {
        out.push_str(&row(r));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{extract_markdown_opts, ExtractOpts};

    fn md(html: &str) -> String {
        extract_markdown_opts(html, &ExtractOpts::default())
    }

    fn first_table(html: &str) -> Table {
        let doc = scraper::Html::parse_document(html);
        let sel = scraper::Selector::parse("table").unwrap();
        let el = doc.select(&sel).next().expect("a table");
        let mut plain = |c: ElementRef<'_>| c.text().collect::<String>();
        match parse(el, &mut plain) {
            TableShape::Data(t) => t,
            TableShape::Layout => panic!("layout, not data"),
        }
    }

    #[test]
    fn plain_cells_keep_pipes_and_newlines_are_flattened() {
        let t = first_table(
            "<table><tr><th>k</th><th>v</th></tr><tr><td>a | b</td><td>one\ntwo</td></tr></table>",
        );
        assert_eq!(
            t.rows[0][0], "a | b",
            "a row is the page's text, not markdown"
        );
        assert_eq!(t.rows[0][1], "one two");
        // The markdown path still escapes the pipe, or the cell would end early.
        let out =
            md("<table><tr><th>k</th><th>v</th></tr><tr><td>a | b</td><td>x</td></tr></table>");
        assert!(out.contains("a \\| b"), "{out}");
    }

    #[test]
    fn rows_become_objects_named_by_the_header() {
        let t = first_table(
            "<table><tr><th>Name</th><th>Price</th></tr><tr><td>Cup</td><td>3</td></tr>\
             <tr><td>Pot</td><td>9</td></tr></table>",
        );
        let rows = to_rows(&t, None);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["Name"], "Cup");
        assert_eq!(rows[1]["Price"], "9");
        assert!(
            rows[0].get("table").is_none(),
            "no index when there is one table"
        );
    }

    #[test]
    fn a_table_without_a_header_names_columns_by_position_and_carries_its_index() {
        let t = first_table(
            "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>",
        );
        let rows = to_rows(&t, Some(2));
        assert_eq!(rows[0]["col_1"], "a");
        assert_eq!(rows[1]["col_2"], "d");
        assert_eq!(rows[0]["table"], 2);
    }

    #[test]
    fn thead_produces_valid_gfm() {
        let out = md("<table><thead><tr><th>A</th><th>B</th></tr></thead>\
             <tbody><tr><td>1</td><td>2</td></tr></tbody></table>");
        assert!(out.contains("| A | B |"), "header row missing: {out}");
        assert!(
            out.contains("| --- | --- |"),
            "separator row missing — this is what made the old output invalid: {out}"
        );
        assert!(out.contains("| 1 | 2 |"), "body row missing: {out}");
    }

    #[test]
    fn a_table_without_th_still_renders_with_an_empty_header() {
        let out = md("<table><tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr></table>");
        assert!(out.contains("| --- | --- |"), "{out}");
        assert!(out.contains("| 1 | 2 |"), "{out}");
        assert!(
            !out.contains("Col 1"),
            "column names must not be invented: {out}"
        );
    }

    #[test]
    fn colspan_repeats_the_value_across_the_columns_it_covered() {
        let out =
            md("<table><tr><th>A</th><th>B</th></tr><tr><td colspan=\"2\">wide</td></tr></table>");
        assert!(out.contains("| wide | wide |"), "{out}");
    }

    #[test]
    fn rowspan_carries_the_cell_into_following_rows() {
        let out = md("<table><tr><th>A</th><th>B</th></tr>\
             <tr><td rowspan=\"2\">tall</td><td>x</td></tr>\
             <tr><td>y</td></tr></table>");
        assert!(out.contains("| tall | x |"), "{out}");
        assert!(
            out.contains("| tall | y |"),
            "without rowspan carry the second row shifts left: {out}"
        );
    }

    #[test]
    fn a_layout_table_is_not_rendered_as_a_table() {
        let out = md(
            "<table role=\"presentation\"><tr><td><p>Just some prose in a layout cell.</p></td></tr></table>",
        );
        assert!(!out.contains("---"), "layout table became a table: {out}");
        assert!(out.contains("Just some prose"), "{out}");
    }

    #[test]
    fn a_single_column_table_is_layout() {
        let out = md("<table><tr><td>only</td></tr><tr><td>one</td></tr></table>");
        assert!(!out.contains("| --- |"), "{out}");
        assert!(out.contains("only"), "{out}");
    }

    #[test]
    fn nested_table_rows_do_not_leak_into_the_outer_one() {
        let out = md(
            "<table><tr><th>A</th><th>B</th></tr>\
             <tr><td>1</td><td><table><tr><td>inner1</td><td>inner2</td></tr></table></td></tr></table>",
        );
        // The outer table contains a nested one, so it is treated as layout rather than producing
        // a grid whose columns are silently wrong.
        assert!(out.contains("inner1"), "inner content lost: {out}");
    }

    #[test]
    fn a_pipe_in_a_cell_is_escaped() {
        let out = md("<table><tr><th>A</th><th>B</th></tr><tr><td>a|b</td><td>c</td></tr></table>");
        assert!(
            out.contains("a\\|b"),
            "unescaped pipe breaks the table: {out}"
        );
    }

    #[test]
    fn a_newline_in_a_cell_becomes_a_space() {
        let out = md(
            "<table><tr><th>A</th><th>B</th></tr><tr><td>one<br>two</td><td>c</td></tr></table>",
        );
        assert!(out.contains("| one two |"), "{out}");
    }

    #[test]
    fn alignment_attributes_become_gfm_alignment() {
        let out = md(
            "<table><tr><th align=\"center\">A</th><th align=\"right\">B</th></tr>\
             <tr><td>1</td><td>2</td></tr></table>",
        );
        assert!(out.contains(":---:"), "centre alignment lost: {out}");
        assert!(out.contains(" ---: "), "right alignment lost: {out}");
    }

    #[test]
    fn a_caption_is_kept_above_the_table() {
        let out = md(
            "<table><caption>Prices</caption><tr><th>A</th><th>B</th></tr>\
             <tr><td>1</td><td>2</td></tr></table>",
        );
        assert!(out.contains("**Prices**"), "{out}");
    }
}
