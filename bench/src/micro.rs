//! CPU budgets, no network.
//!
//! Two kinds of check. The timing budgets have 25% headroom before they fail, so an unloaded
//! machine passes and a real regression does not. The *structural* ones — how many times the DOM
//! is parsed, how many times state is read from disk — are exact and cannot flake, and they are
//! what actually pins the work down: a timing number drifts with the hardware, "exactly one parse"
//! does not.
//!
//! Fixtures are generated from a fixed seed rather than checked in, so the numbers are comparable
//! between machines without carrying blobs in the repository.

use std::time::{Duration, Instant};
use svipall_core::extraction::{self, MarkdownOpts, ParseWants};

/// Deterministic filler, so two machines measure the same document.
fn words(seed: u64, count: usize) -> String {
    const VOCAB: &[&str] = &[
        "ownership",
        "borrowing",
        "lifetime",
        "compiler",
        "reference",
        "mutable",
        "trait",
        "closure",
        "iterator",
        "pattern",
        "module",
        "crate",
        "generic",
        "async",
        "future",
    ];
    let mut s = seed | 1;
    let mut out = String::with_capacity(count * 8);
    for i in 0..count {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        out.push_str(VOCAB[(s as usize) % VOCAB.len()]);
        out.push(if i % 12 == 11 { '.' } else { ' ' });
    }
    out
}

/// A news-shaped page of roughly `target` bytes: nav, sidebar, article, comments, footer.
fn news_page(target: usize) -> String {
    let mut html = String::from(
        "<!doctype html><html lang=\"en\"><head><title>Benchmark Page</title>\
         <meta name=\"description\" content=\"A generated page\">\
         <link rel=\"canonical\" href=\"https://bench.test/article\"></head><body>\
         <nav class=\"site-nav\"><ul>",
    );
    for i in 0..40 {
        html.push_str(&format!(
            "<li><a href=\"/section/{i}\">Section {i}</a></li>"
        ));
    }
    html.push_str("</ul></nav><div class=\"sidebar\"><ul>");
    for i in 0..30 {
        html.push_str(&format!(
            "<li><a href=\"/related/{i}\">Related story {i}</a></li>"
        ));
    }
    html.push_str("</ul></div><main><article><h1>Headline</h1>");
    let mut i = 0u64;
    while html.len() < target {
        html.push_str(&format!("<p>{}</p>", words(i, 60)));
        if i % 7 == 3 {
            html.push_str("<pre><code>fn main() { println!(\"x\"); }</code></pre>");
        }
        if i % 11 == 5 {
            html.push_str(
                "<table><tr><th>Name</th><th>Value</th></tr>\
                 <tr><td>alpha</td><td>1</td></tr><tr><td>beta</td><td>2</td></tr></table>",
            );
        }
        i += 1;
    }
    html.push_str("</article></main><footer>Copyright</footer></body></html>");
    html
}

/// A flat grey PNG: the models are timed on a picture-sized input, not on what it shows.
#[cfg(feature = "onnx")]
fn solid_png(size: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(size, size, image::Rgb([128, 128, 128]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

struct Budget {
    name: &'static str,
    measured: Duration,
    limit: Duration,
}

impl Budget {
    fn ok(&self) -> bool {
        // 25% headroom: a busy machine should not turn into a red build.
        self.measured <= self.limit.mul_f32(1.25)
    }
}

fn time<T>(reps: u32, mut f: impl FnMut() -> T) -> Duration {
    // One warm-up so allocator and cache effects do not land in the measurement.
    let _ = f();
    let t0 = Instant::now();
    for _ in 0..reps {
        let _ = f();
    }
    t0.elapsed() / reps
}

pub fn run(assert: bool) -> usize {
    let page = news_page(200_000);
    let text = extraction::extract_text(&page);
    let markdown = extraction::extract_markdown_opts(&page, &extraction::ExtractOpts::default());
    eprintln!(
        "fixture: {} KB html, {} KB text, {} KB markdown\n",
        page.len() / 1024,
        text.len() / 1024,
        markdown.len() / 1024
    );

    let mut budgets = Vec::new();

    budgets.push(Budget {
        name: "classify / 200KB",
        measured: time(50, || svipall_core::classify(200, &page, &text)),
        limit: Duration::from_micros(400),
    });

    // Runs on every delivered page, so it is held to the same order as classification. One pass
    // over the text plus a bigram table over the first 20k words is the whole of it.
    budgets.push(Budget {
        name: "quality::assess / 200KB",
        measured: time(50, || {
            svipall_core::quality::assess(&svipall_core::quality::Evidence::new(page.len(), &text))
        }),
        limit: Duration::from_micros(250),
    });

    // Only when a model is installed. There is none embedded, so on most machines this simply
    // does not appear rather than measuring an absence.
    if svipall_mcp::substance::available() {
        budgets.push(Budget {
            name: "substance::assess / 200KB",
            measured: time(20, || svipall_mcp::substance::assess(&text)),
            limit: Duration::from_millis(3),
        });
    }

    // Three heuristics over one page, parse included, because that is what a fetch pays. It
    // measures 3.6 ms against the 4.1 ms of `parse_page everything` below — three opinions for
    // less than the parse they share, which is the whole reason they share it. The budget is set
    // where a regression is visible rather than where it is comfortable.
    budgets.push(Budget {
        name: "markdown, voted / 200KB",
        measured: time(10, || {
            extraction::extract_markdown_opts(
                &page,
                &extraction::ExtractOpts {
                    main_content_only: true,
                    vote: Some(extraction::content::vote::Rule::Unanimous),
                    ..Default::default()
                },
            )
        }),
        limit: Duration::from_millis(8),
    });

    // The site template, on the path a fetch actually runs it: one pass over the delivered
    // markdown's blocks, hashing each and looking it up. It is cheap by construction — the blocks
    // are already split for the token budget — and the budget is here so it stays that way. A
    // template pass that grew into a second extraction would be paid on every page of every site.
    {
        let mut learned = svipall_core::template::Template::default();
        for _ in 0..svipall_core::template::MIN_PAGES {
            learned.observe(&markdown);
        }
        budgets.push(Budget {
            name: "template::strip / 200KB",
            measured: time(50, || learned.strip(&markdown)),
            limit: Duration::from_millis(2),
        });
    }

    // The near-duplicate lookup, which is the only quality signal that touches the disk. Four
    // indexed equality probes and an exact distance over what they return — the pigeonhole
    // argument in `MIGRATE_6_TO_7` is what keeps it from being a scan. Measured against a store
    // holding a few hundred pages, which is what makes the index worth having at all.
    {
        let store = svipall_core::cache::Store::open_memory().expect("in-memory store");
        for i in 0..300 {
            let _ = store.put(
                &format!("https://bench.test/{i}"),
                &format!("https://bench.test/{i}"),
                200,
                "http",
                None,
                None,
                "text/html",
                None,
                &words(i as u64 + 1, 200),
                3600,
                None,
            );
        }
        let probe = svipall_core::dedup::simhash(&markdown);
        budgets.push(Budget {
            name: "cache::find_near / 300 pages",
            measured: time(50, || {
                store.find_near(
                    probe,
                    svipall_core::quality::provenance::NEAR_DUPLICATE_BITS,
                    3,
                )
            }),
            limit: Duration::from_millis(2),
        });
    }

    // Wrapper induction walks every element, groups the children of each parent by signature, and
    // measures the text of every group big enough to be a record set. On a page with nested
    // repeated structure that is the shape of an accidental quadratic, so it gets a budget of its
    // own rather than hiding inside the parse it shares.
    budgets.push(Budget {
        name: "induce / 200KB",
        measured: time(10, || extraction::parse_page(&page, &ParseWants::induced())),
        limit: Duration::from_millis(60),
    });

    budgets.push(Budget {
        name: "parse_page text+title / 200KB",
        measured: time(10, || {
            extraction::parse_page(
                &page,
                &ParseWants {
                    text: true,
                    title: true,
                    ..Default::default()
                },
            )
        }),
        limit: Duration::from_millis(14),
    });

    budgets.push(Budget {
        name: "parse_page everything / 200KB",
        measured: time(10, || {
            extraction::parse_page(
                &page,
                &ParseWants {
                    text: true,
                    title: true,
                    markdown: Some(MarkdownOpts {
                        main_content_only: true,
                        base_url: Some("https://bench.test/".into()),
                        ..Default::default()
                    }),
                    links_base: Some("https://bench.test/".into()),
                    metadata: true,
                    ..Default::default()
                },
            )
        }),
        limit: Duration::from_millis(20),
    });

    budgets.push(Budget {
        name: "bm25_filter / full page",
        measured: time(10, || {
            extraction::bm25_filter(&markdown, "ownership borrowing lifetime", 40)
        }),
        limit: Duration::from_millis(3),
    });

    budgets.push(Budget {
        name: "budget::take / full page",
        measured: time(20, || {
            svipall_core::budget::take(
                &markdown,
                &svipall_core::budget::BudgetOpts {
                    max_tokens: 5_000,
                    cursor: None,
                    overlap_blocks: 0,
                },
            )
        }),
        limit: Duration::from_millis(4),
    });

    budgets.push(Budget {
        name: "simhash / full page",
        measured: time(20, || svipall_core::dedup::simhash(&markdown)),
        limit: Duration::from_millis(5),
    });

    // The embedded models, on the CPU, on the picture size a challenge shows. A model that takes
    // a second per tile is a model nobody waits for; these are the numbers `docs/models.md`
    // quotes, measured here rather than recalled.
    #[cfg(feature = "onnx")]
    {
        let picture = solid_png(320);
        if svipall_models::detect().is_some() {
            budgets.push(Budget {
                name: "detect / 320px picture (embedded)",
                measured: time(5, || svipall_mcp::detect::detect(&picture, 0)),
                limit: Duration::from_millis(120),
            });
        }
        if svipall_models::segment().is_some() {
            budgets.push(Budget {
                name: "segment / 320px picture (embedded)",
                measured: time(5, || svipall_mcp::segment::cells(&picture, 0, 4, 4)),
                limit: Duration::from_millis(250),
            });
        }
    }

    let mut failures = 0;
    for b in &budgets {
        let status = if b.ok() { "ok  " } else { "OVER" };
        eprintln!(
            "{status} {:<34} {:>9.3?}  (budget {:?})",
            b.name, b.measured, b.limit
        );
        if !b.ok() {
            failures += 1;
        }
    }

    eprintln!("\nstructural checks (exact, cannot flake):");
    failures += structural();

    if !assert {
        if failures > 0 {
            eprintln!("\n({failures} over budget; pass --assert to make that a failure)");
        }
        return 0;
    }
    failures
}

/// Counts rather than timings. These are the checks that actually hold the design in place.
fn structural() -> usize {
    let mut failures = 0;
    let page = news_page(60_000);

    let before = extraction::dom_parse_count();
    let _ = extraction::parse_page(
        &page,
        &ParseWants {
            text: true,
            title: true,
            markdown: Some(MarkdownOpts {
                main_content_only: true,
                base_url: Some("https://bench.test/".into()),
                ..Default::default()
            }),
            links_base: Some("https://bench.test/".into()),
            metadata: true,
            links_detailed: Some("https://bench.test/".into()),
            ..Default::default()
        },
    );
    let parses = extraction::dom_parse_count() - before;
    let ok = parses == 1;
    eprintln!(
        "{} DOM parses for text+title+markdown+links+metadata: {parses} (expected 1)",
        if ok { "ok  " } else { "FAIL" }
    );
    if !ok {
        failures += 1;
    }

    // The tier map and proxy routes used to be re-read from disk on every single lookup.
    let path = std::env::temp_dir().join(format!("svipall-bench-{}.json", std::process::id()));
    let _ = std::fs::write(&path, r#"{"bench.test":"real"}"#);
    let map = svipall_core::store::JsonMap::new(path.clone());
    let _ = map.get("bench.test");
    let after_first = map.disk_reads();
    for _ in 0..10_000 {
        let _ = map.get("bench.test");
    }
    let reads = map.disk_reads() - after_first;
    let ok = reads == 0;
    eprintln!(
        "{} disk reads across 10,000 lookups: {reads} (expected 0)",
        if ok { "ok  " } else { "FAIL" }
    );
    if !ok {
        failures += 1;
    }
    let _ = std::fs::remove_file(&path);

    // The reputation ledger is charged on every rung of every fetch, and it writes a whole file.
    // Written per charge, a crawl of two hundred pages would be two hundred serialize-and-writes
    // on a runtime thread — so it batches, and the batching is asserted here rather than trusted.
    // The check above covers `JsonMap` and would never have noticed this.
    let before_writes = svipall_core::reputation::writes();
    for i in 0..10_000 {
        svipall_core::reputation::spend(&format!("bench-{}.test", i % 50), None, "http", false);
    }
    let ledger_writes = svipall_core::reputation::writes() - before_writes;
    let ok = ledger_writes <= 1;
    eprintln!(
        "{} ledger writes across 10,000 charges: {ledger_writes} (expected at most 1)",
        if ok { "ok  " } else { "FAIL" }
    );
    if !ok {
        failures += 1;
    }

    // Pruning has to actually remove the navigation and the sidebar, or it is costing tokens for
    // nothing.
    //
    // Named, not proportional. This used to report "pruning removed N% of the markdown" and pass
    // on any N > 0; it read 2% and was taken as a success for months. On this fixture 2% is what
    // the chrome is *worth* — the generated article body dwarfs seventy links — so the share was
    // never the question. What the chrome is, and whether the things that must survive did, is.
    let full = extraction::extract_markdown_opts(&page, &extraction::ExtractOpts::default());
    let pruned = extraction::extract_markdown_opts(
        &page,
        &extraction::ExtractOpts {
            main_content_only: true,
            ..Default::default()
        },
    );
    let saved = 100.0 - (pruned.len() as f64 * 100.0 / full.len().max(1) as f64);
    let removed = !pruned.contains("Section 17") && !pruned.contains("Related story 12");
    let kept = pruned.contains("Headline")
        && pruned.contains("fn main()")
        && pruned.contains("| alpha | 1 |");
    let ok = removed && kept && pruned.len() < full.len();
    eprintln!(
        "{} pruning dropped the nav and the sidebar, kept the article, the code and the table \
         ({saved:.0}% smaller)",
        if ok { "ok  " } else { "FAIL" }
    );
    if !ok {
        failures += 1;
    }

    failures
}
