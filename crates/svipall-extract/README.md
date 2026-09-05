# svipall-extract

HTML to text, Markdown, links, metadata and typed tables — in **one DOM parse**.

[![Crates.io](https://img.shields.io/crates/v/svipall-extract.svg?style=flat-square)](https://crates.io/crates/svipall-extract)
[![Docs.rs](https://img.shields.io/docsrs/svipall-extract?style=flat-square)](https://docs.rs/svipall-extract)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg?style=flat-square)](#license)

This is the extraction engine behind [svipall](https://github.com/ilien-dev/svipall), split out
so it can be used on its own. It has no async runtime, makes no network calls, and pulls in nothing
beyond `scraper`, `regex`, `serde` and `url`.

## Why it exists

Most extraction code parses the document again for every question you ask it: once for the text,
once for the links, once for the metadata, once per schema field. On a large page that is the
dominant cost, and it is entirely avoidable.

`svipall-extract` inverts the call: you declare everything you want up front in `ParseWants`, and
`parse_page` walks the DOM once to produce all of it.

```rust
use svipall_extract::{parse_page, MarkdownOpts, ParseWants};

let wants = ParseWants {
    text: true,
    title: true,
    markdown: Some(MarkdownOpts::default()),
    links_base: Some("https://example.com/".into()),
    metadata: true,
    tables: true,
    ..Default::default()
};

let parts = parse_page(html, &wants);

println!("{}", parts.title.unwrap_or_default());
println!("{}", parts.markdown.unwrap_or_default());
println!("{} links, {} tables", parts.links.len(), parts.tables.len());
```

`dom_parse_count()` returns how many times the DOM has actually been parsed, so "one parse per
response" is a property you can assert in a test rather than a claim in a README.

## What it does

| | |
|---|---|
| **Readable Markdown** | Boilerplate pruned by structural scoring, not by a hand-written blocklist of class names. Nested tags, entities and `script`/`style` bodies are handled by the DOM, not by regex. |
| **Declarative CSS extraction** | Describe the fields you want as JSON; get typed rows back. Field types: `text`, `attribute`, `number`, `exists`, `list`, `html`, `markdown`. |
| **Selectors that survive a redesign** | Each selector is fingerprinted where it matched. When a site moves the element, `heal` relocates it by structural similarity and reports the selector to switch to, instead of returning nothing. |
| **Schema induction** | With no schema at all, the page's own repeated structure can propose one — applied in the same parse. |
| **Typed tables** | Every data table as `{caption, header, rows}`. Layout tables used for navigation are skipped. |
| **Metadata** | JSON-LD, OpenGraph, canonical, language, feeds, authors, dates. |
| **Hidden-text sanitisation** | `sanitize` drops visually hidden nodes and invisible characters, so text meant only for a crawler does not reach whatever reads the output. |
| **BM25 filtering** | `bm25_filter` cuts a long document down to the blocks that answer a query, on block boundaries. |

## Selector healing

The part most worth stealing. A CSS selector is a guess about a page that its author is free to
change. Rather than storing the selector alone, `heal::fingerprint` records what the matched element
*looked like* — tag, classes, attributes, depth, position, text shape. When the selector later
matches nothing:

```rust
use svipall_extract::heal::{fingerprint, relocate};

// When it worked: remember the shape.
let fp = fingerprint(element);

// Later, after a redesign: find it again.
if let Some(m) = relocate(&document, &fp) {
    // m carries the element and the selector that would find it now.
}
```

`similarity` is the scoring function, exposed so you can set your own threshold.

## Install

```toml
[dependencies]
svipall-extract = "1.0"
```

## License

Dual licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The wider svipall project is AGPL-3.0-only; this crate is deliberately permissive so that anything
can depend on it.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
