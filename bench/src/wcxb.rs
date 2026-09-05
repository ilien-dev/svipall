//! The modern half of the extraction benchmark, and the one that answers a different question.
//!
//! The SIGIR-23 gold standard says how svipall does on the web of 2013. WCXB (Foley, 2026;
//! CC-BY-4.0) says how it does on the web people actually fetch: 2,008 human-reviewed pages from
//! 1,613 domains, **labelled by page type** — article, service, product, collection, forum,
//! listing, documentation — and split 1,497 development / 511 held out.
//!
//! Two things make it worth carrying a second corpus rather than trusting the first.
//!
//! * The rankings invert. Readability is the most robust system in SIGIR-23 (median F1 0.970) and
//!   twelfth of thirteen here (0.674). An extractor fitted against either corpus alone is fitted
//!   to that corpus.
//! * Articles are solved and everything else is not. On articles the published systems land within
//!   two or three points of each other; on forums, collections and products they spread twenty to
//!   thirty. A single average hides exactly the pages where the work is.
//!
//! The metric here is WCXB's own — word-level multiset overlap, from its `evaluate.py` — rather
//! than ROUGE-LSum, so the numbers can be read against its published leaderboard without a
//! translation step. The `with[]` and `without[]` snippets are scored separately: they are a
//! second opinion on the same page, written by the corpus author instead of by us.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

/// The seven structural kinds WCXB distinguishes, in the order its leaderboard prints them.
pub const PAGE_TYPES: &[&str] = &[
    "article",
    "documentation",
    "service",
    "forum",
    "collection",
    "listing",
    "product",
];

/// One annotated page.
pub struct Page {
    pub page_type: String,
    /// The site it came from, so pages of one site can be grouped. WCXB carries this in its own
    /// manifest, which is the only reason the cross-page work can be measured at all — the other
    /// corpora ship pages with no way to tell which came from the same place.
    pub domain: String,
    pub html: String,
    pub gold: String,
    /// Snippets a correct extraction must contain.
    pub with: Vec<String>,
    /// Snippets from the boilerplate that it must not.
    pub without: Vec<String>,
}

/// Word-level precision, recall and F1 over the two texts as multisets of words.
///
/// This is `evaluate.py`'s measure, reimplemented: lowercase, `\w+`, count with multiplicity, and
/// take the overlap. It ignores word order entirely, which ROUGE-LSum does not — a deliberate
/// difference, not an oversight. Order matters when you are asking whether the *article* came back;
/// it matters much less on a product grid or a forum thread, where the gold is a set of blocks
/// whose sequence is a rendering decision.
pub fn word_f1(reference: &str, candidate: &str) -> crate::extraction::Score {
    let refc = counts(reference);
    let candc = counts(candidate);
    let ref_n: usize = refc.values().sum();
    let cand_n: usize = candc.values().sum();
    if ref_n == 0 || cand_n == 0 {
        return crate::extraction::Score::default();
    }
    let overlap: usize = candc
        .iter()
        .map(|(w, n)| *n.min(refc.get(w).unwrap_or(&0)))
        .sum();
    let precision = overlap as f64 / cand_n as f64;
    let recall = overlap as f64 / ref_n as f64;
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    crate::extraction::Score {
        precision,
        recall,
        f1,
    }
}

fn counts(text: &str) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for w in text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
    {
        *m.entry(w.to_lowercase()).or_insert(0) += 1;
    }
    m
}

/// Whether a snippet survives extraction, compared the way a reader would rather than byte for
/// byte.
///
/// ▲ **A sequence of words, not a substring.** The first version normalised whitespace and case and
/// then asked `haystack.contains(needle)`, which fails on text that was delivered in full:
///
/// - markdown puts emphasis *inside* a phrase, so `the eastern quay` is rendered
///   `the **eastern** quay` and does not contain the phrase as a substring;
/// - the corpus writes straight quotes and apostrophes where the page has typographic ones, and
///   `it's` never matches `it’s`;
/// - an entity, a soft hyphen or a non-breaking space anywhere in the phrase does the same.
///
/// None of those is a page losing content, and counting them as losses understated required-snippet
/// recall — the metric this project says it should be judged by.
///
/// So both sides are reduced to their words, rejoined with single spaces, and compared as a
/// substring of that. Two properties, and it takes both:
///
/// - **a sequence, not a bag** — the words must be present, in order, with nothing between them, so
///   a page that happens to contain all of them scattered is still a page that lost the phrase;
/// - **the edges may fall inside a word** — a footnote marker glued to the last word (`daily2`) or
///   a heading run into the paragraph after it (`ruminationmeditation`) is text the reader receives
///   intact, and an exact token-sequence match calls both of those losses. Measured: 290 phrases on
///   the development split alone.
pub fn contains_snippet(haystack: &str, snippet: &str) -> bool {
    let n = tokens(snippet).join(" ");
    if n.is_empty() {
        return false;
    }
    tokens(haystack).join(" ").contains(&n)
}

/// Can a word-based comparison judge this phrase at all?
///
/// ▲ One WCXB phrase is the `psql` elephant drawn in ASCII. It has no words, so nothing that
/// compares words can say whether it survived — and scoring it as lost would report the limits of
/// the instrument as a failure of the extractor. It is excluded from the counts, and the exclusion
/// is printed, which is the only honest way to leave a phrase out of a denominator.
pub fn is_measurable(snippet: &str) -> bool {
    !tokens(snippet).is_empty()
}

/// Runs of alphanumerics, lowercased. The same rule `word_f1` counts with, so the two measures
/// cannot disagree about what a word is.
///
/// Invisible characters come out first, through the extractor's own list. A soft hyphen inside a
/// word — `re{ad}opens`, which is what `&shy;` decodes to — is not a word boundary to a reader
/// and must not be one here either; without this it splits `reopens` into two tokens and the phrase
/// stops matching text that was delivered intact.
fn tokens(s: &str) -> Vec<String> {
    let clean = svipall_core::extraction::sanitize::strip_invisible_chars(s);
    clean
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// What the corpus author's own snippets say about a set of pages.
///
/// ▲ This is the closest thing in the workspace to a task metric, and it is now the headline rather
/// than a footnote. F1 measures how much of the gold came back; this measures whether the sentences
/// a person marked as *required* survived. Those are different questions, and Cuconasu et al.
/// (SIGIR 2024) is the reason the second one is the one that matters: what breaks an answer is the
/// on-topic page that does not contain it. A page scoring 0.92 F1 that dropped the one sentence
/// carrying the answer is a failure F1 cannot see and this can.
///
/// `with[]` snippets are 3-8 word phrases from the content that a correct extraction must include;
/// `without[]` are phrases from the chrome it must not. Both were written by the corpus author, not
/// by us, which is what makes them a second opinion rather than a restatement of our own scoring.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Snippets {
    /// Required snippets that survived.
    pub kept: usize,
    /// Required snippets in total.
    pub wanted: usize,
    /// Boilerplate snippets that came back anyway.
    pub leaked: usize,
    /// Boilerplate snippets in total.
    pub checked: usize,
    /// Pages that lost at least one required snippet. Counted per page, not per snippet: one page
    /// losing five phrases is one page to go and look at, not five.
    pub pages_losing: usize,
    pub pages: usize,
    /// Phrases with no words in them, which a word-based comparison cannot judge either way.
    pub wordless: usize,
}

impl Snippets {
    /// Share of required snippets that survived. The number to move.
    pub fn recall(&self) -> f64 {
        if self.wanted == 0 {
            return 1.0;
        }
        self.kept as f64 / self.wanted as f64
    }

    /// Share of boilerplate snippets that came back. The number to keep down.
    pub fn leak(&self) -> f64 {
        if self.checked == 0 {
            return 0.0;
        }
        self.leaked as f64 / self.checked as f64
    }

    /// Account for one page.s extraction.
    pub fn observe(&mut self, page: &Page, out: &str) {
        self.pages += 1;
        let mut lost_here = false;
        for s in &page.with {
            if !is_measurable(s) {
                self.wordless += 1;
                continue;
            }
            self.wanted += 1;
            if contains_snippet(out, s) {
                self.kept += 1;
            } else {
                lost_here = true;
            }
        }
        for s in &page.without {
            if !is_measurable(s) {
                self.wordless += 1;
                continue;
            }
            self.checked += 1;
            if contains_snippet(out, s) {
                self.leaked += 1;
            }
        }
        if lost_here {
            self.pages_losing += 1;
        }
    }

    fn merge(&mut self, other: &Snippets) {
        self.kept += other.kept;
        self.wanted += other.wanted;
        self.leaked += other.leaked;
        self.checked += other.checked;
        self.pages_losing += other.pages_losing;
        self.pages += other.pages;
        self.wordless += other.wordless;
    }

    fn cell(&self) -> String {
        if self.wanted == 0 {
            return format!("{:>12}", "-");
        }
        format!("{:>11.1}%", 100.0 * self.recall())
    }
}

/// Load one split. Missing or unreadable pages are skipped rather than faked.
pub fn load(root: &Path, split: &str) -> Vec<Page> {
    let meta: serde_json::Value = std::fs::read_to_string(root.join("metadata.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let files = meta.get("files").and_then(|f| f.as_object());

    let gt_dir = root.join(split).join("ground-truth");
    let html_dir = root.join(split).join("html");
    let Ok(entries) = std::fs::read_dir(&gt_dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    ids.sort();

    let mut pages = Vec::with_capacity(ids.len());
    for id in ids {
        let Ok(raw) = std::fs::read_to_string(gt_dir.join(format!("{id}.json"))) else {
            continue;
        };
        let Ok(v): Result<serde_json::Value, _> = serde_json::from_str(&raw) else {
            continue;
        };
        let gt = &v["ground_truth"];
        let gold = gt["main_content"].as_str().unwrap_or_default().to_string();
        if gold.trim().is_empty() {
            continue;
        }
        let Some(html) = read_gz(&html_dir.join(format!("{id}.html.gz"))) else {
            continue;
        };
        // The label lives in metadata.json for every page and, for most, a second time inside the
        // annotation. Prefer the manifest: it is what the published leaderboard groups by.
        let page_type = files
            .and_then(|f| f.get(&id))
            .and_then(|f| f["page_type"].as_str())
            .or_else(|| v["_internal"]["page_type"]["primary"].as_str())
            .unwrap_or("unknown")
            .to_string();
        let domain = files
            .and_then(|f| f.get(&id))
            .and_then(|f| f["domain"].as_str())
            .or_else(|| v["_internal"]["domain"].as_str())
            .unwrap_or_default()
            .to_string();
        pages.push(Page {
            page_type,
            domain,
            html,
            gold,
            with: strings(&gt["with"]),
            without: strings(&gt["without"]),
        });
    }
    pages
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn read_gz(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut out = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut out)
        .ok()?;
    Some(out)
}

/// How a page was read, for the columns the report compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// The shipping extractor: selectors plus the density pass.
    Shipping,
    /// The vote, with no idea what kind of page this is.
    Voted,
    /// The vote, told the type by the corpus. An oracle: the ceiling routing could reach.
    Typed,
    /// The vote, with the forum detector deciding. No model, and what a fetch gets today.
    Detected,
}

impl Read {
    pub const ALL: &'static [Read] = &[Read::Shipping, Read::Voted, Read::Detected, Read::Typed];

    pub fn label(self) -> &'static str {
        match self {
            Read::Shipping => "shipping",
            Read::Voted => "voted",
            Read::Typed => "typed",
            Read::Detected => "detected",
        }
    }
}

/// Extract one page the way this column says to.
///
/// ▲ `Typed` is the experiment the whole router rests on. It is the corpus's own label rather than a
/// prediction, so it answers "would knowing the page type help at all" separately from "can we tell
/// what type it is". Those are different questions and conflating them is how a routing layer gets
/// built for a gain that was never there: on this same corpus, routing pages to a better extractor
/// bought +0.003.
fn read_page(p: &Page, how: Read) -> String {
    use svipall_core::extraction::{content::vote::Rule, extract_markdown_opts, ExtractOpts};
    let base = ExtractOpts {
        main_content_only: true,
        ..Default::default()
    };
    let opts = match how {
        Read::Shipping => base,
        // Pinned to the article profile, which is the default one, so the forum detector cannot
        // fire. This is the vote with no idea what kind of page it is reading.
        Read::Voted => ExtractOpts {
            vote: Some(Rule::Unanimous),
            page_type: Some(svipall_core::extraction::content::profile::PageType::Article),
            ..base
        },
        Read::Typed => ExtractOpts {
            vote: Some(Rule::Unanimous),
            page_type: svipall_core::extraction::content::profile::PageType::parse(&p.page_type),
            ..base
        },
        // No model, no label: what a fetch gets from the page itself.
        Read::Detected => ExtractOpts {
            vote: Some(Rule::Unanimous),
            ..base
        },
    };
    crate::extraction::markdown_as_text(&extract_markdown_opts(&p.html, &opts))
}

/// Score both splits, by page type and by how the page was read.
///
/// The development set is where work happens; the held-out set is looked at to find out whether the
/// work generalised, and nothing is fitted against it. They are printed together so that the gap
/// between them is visible, because a widening gap is the only warning an overfitted extractor ever
/// gives.
pub fn run(root: &Path, assert: bool) -> usize {
    if !root.join("dev/ground-truth").is_dir() {
        eprintln!(
            "no WCXB corpus at {}\n\n\
             2,008 page-type-labelled pages (Foley 2026, CC-BY-4.0). Fetch it once with:\n\n    \
             scripts/fetch-wcxb.sh [target-dir]\n\n\
             then point this command at it:\n\n    \
             cargo run -p svipall-bench --release -- extract --wcxb <target-dir>",
            root.display()
        );
        return 1;
    }

    let dev = load(root, "dev");
    let test = load(root, "test");
    if dev.is_empty() {
        eprintln!("the corpus is present but no development page could be read");
        return 1;
    }

    let ways: Vec<Read> = Read::ALL.to_vec();

    println!("\n== WCXB (word-level F1, mean over pages) ==\n");
    println!(
        "{:<16} {:>6} {}",
        "page type",
        "pages",
        ways.iter()
            .map(|w| format!("{:>12}", w.label()))
            .collect::<String>()
    );

    let mut failures = 0usize;
    for split in ["dev", "test"] {
        let pages = if split == "dev" { &dev } else { &test };
        if pages.is_empty() {
            continue;
        }
        println!("-- {split} --");
        let scored = score(pages, &ways);
        for t in PAGE_TYPES {
            let Some(by_way) = scored.get(*t) else {
                continue;
            };
            println!(
                "{t:<16} {:>6} {}",
                by_way.first().map(Vec::len).unwrap_or(0),
                by_way.iter().map(|v| mean_cell(v)).collect::<String>()
            );
        }
        let all: Vec<Vec<crate::extraction::Score>> = (0..ways.len())
            .map(|i| {
                scored
                    .values()
                    .filter_map(|v| v.get(i))
                    .flatten()
                    .copied()
                    .collect()
            })
            .collect();
        println!(
            "{:<16} {:>6} {}",
            "ALL",
            all.first().map(Vec::len).unwrap_or(0),
            all.iter().map(|v| mean_cell(v)).collect::<String>()
        );
    }

    forum_report("dev", &dev);
    forum_report("test", &test);

    // ▲ The headline. Everything above is a score to argue about; this is the corpus author's own
    // second opinion on whether the extraction kept the sentences that mattered, and it is the
    // number the rest of this work is judged by.
    println!("\n== WCXB required-snippet recall (the task metric) ==\n");
    println!(
        "{:<16} {:>6} {}",
        "page type",
        "pages",
        ways.iter()
            .map(|w| format!("{:>12}", w.label()))
            .collect::<String>()
    );
    // The development totals are what `--assert` gates on. The held-out ones are printed and
    // deliberately not gated: a floor fitted against the split that exists to check generalisation
    // would quietly turn it into a second training set.
    let dev_snips = snippet_report("dev", &dev, &ways);
    // ▲ And where the losses went. The recall number says how many phrases were dropped; this says
    // which stage dropped them, which is the difference between a percentage and a piece of work.
    loss_report("dev", &dev);
    prune_sweep(&dev);
    let _held_out = snippet_report("test", &test, &ways);
    println!(
        "\nA required snippet is a phrase a person marked as content that must survive; a leak is a\n\
         phrase from the chrome that came back anyway. Pages, not snippets, are counted as losing:\n\
         one page dropping five phrases is one page to go and look at."
    );

    if dev.len() + test.len() < 1_500 {
        eprintln!(
            "only {} of 2,008 pages were readable - the corpus is incomplete",
            dev.len() + test.len()
        );
        failures += 1;
    }

    // The cross-page template, against the multi-page domains hiding inside this corpus. Its own
    // gate is absolute and lives with it: a saving does not buy a lost sentence.
    failures += crate::template::run(&dev, assert);

    if assert {
        // ▲ The task metric is gated first, because it is the one with a right answer. F1 is a
        // score to argue about; a required snippet that did not survive is a sentence a person
        // marked as content and the caller did not receive.
        if dev_snips.recall() < crate::extraction::floors::SNIPPET_RECALL {
            eprintln!(
                "FAIL required-snippet recall {:.3} is below the floor of {:.3} ({} pages lost \
                 content)",
                dev_snips.recall(),
                crate::extraction::floors::SNIPPET_RECALL,
                dev_snips.pages_losing
            );
            failures += 1;
        } else {
            eprintln!(
                "ok   required-snippet recall {:.3} (floor {:.3}), {} of {} pages lost content",
                dev_snips.recall(),
                crate::extraction::floors::SNIPPET_RECALL,
                dev_snips.pages_losing,
                dev_snips.pages
            );
        }
        if dev_snips.leak() > crate::extraction::floors::SNIPPET_LEAK {
            eprintln!(
                "FAIL boilerplate leak {:.3} is above the ceiling of {:.3}",
                dev_snips.leak(),
                crate::extraction::floors::SNIPPET_LEAK
            );
            failures += 1;
        } else {
            eprintln!(
                "ok   boilerplate leak {:.3} (ceiling {:.3})",
                dev_snips.leak(),
                crate::extraction::floors::SNIPPET_LEAK
            );
        }

        let scored = score(&dev, &[Read::Shipping]);
        let all: Vec<crate::extraction::Score> = scored
            .values()
            .filter_map(|v| v.first())
            .flatten()
            .copied()
            .collect();
        let mean = if all.is_empty() {
            0.0
        } else {
            all.iter().map(|s| s.f1).sum::<f64>() / all.len() as f64
        };
        if mean < crate::extraction::floors::WCXB_DEV_F1 {
            eprintln!(
                "FAIL WCXB development mean F1 {mean:.3} is below the floor of {:.3}",
                crate::extraction::floors::WCXB_DEV_F1
            );
            failures += 1;
        } else {
            eprintln!(
                "ok   WCXB development mean F1 {mean:.3} (floor {:.3})",
                crate::extraction::floors::WCXB_DEV_F1
            );
        }
    }
    failures
}

// ▲ The task metric, per page type and per reading.
//
// This is the report the rest of the work is judged by. It answers "did the sentences a person
// marked as required survive the extraction", which is the question Cuconasu et al. say decides
// whether a page helps or hurts an answer. The F1 table above answers "how much of the gold came
// back", which is a different and more forgiving question.

/// Where a required phrase was lost, for the pages that lose one.
///
/// ▲ The instrument this project did not have. 848 of WCXB's 1,476 development pages drop at least
/// one phrase a person marked as content, and until now that was one number with no way in. These
/// are three questions asked of the same page in order, and each has a different answer and a
/// different repair:
///
/// | bucket | meaning | what would fix it |
/// |---|---|---|
/// | `absent` | the phrase is not in the document's visible text at all | nothing here — the page needs JavaScript, or the corpus normalised differently |
/// | `rendering` | in the text, gone from unpruned markdown | the markdown walker: skipped tags, hidden-text rules, table handling |
/// | `selection` | in the unpruned markdown, gone from what ships | `main_content_only`: the main-region selector, or the density pass |
///
/// Nothing is inferred. Each bucket is one extraction of the same page, and a phrase is counted
/// once, in the first bucket that explains it.
#[derive(Debug, Default, Clone, Copy)]
pub struct Losses {
    /// Not in the HTML source either: the page needs JavaScript to produce it, or the corpus read
    /// it from a rendered DOM this file does not contain. Nothing a static extractor can do.
    pub not_in_source: usize,
    /// In the HTML source, gone from the plain-text walk. That one *is* ours: skipped tags, the
    /// hidden-text rules, or an element the walker does not descend into.
    pub absent: usize,
    pub rendering: usize,
    /// Removed by a density threshold — a number that can be argued with.
    pub threshold: usize,
    /// Removed by the main-region selector, or by the two pruner clauses no option reaches.
    pub structural: usize,
    /// Pages contributing to each, counted once however many phrases they lost.
    pub pages_not_in_source: usize,
    pub pages_absent: usize,
    pub pages_rendering: usize,
    pub pages_threshold: usize,
    pub pages_structural: usize,
}

impl Losses {
    fn merge(&mut self, o: &Losses) {
        self.not_in_source += o.not_in_source;
        self.absent += o.absent;
        self.rendering += o.rendering;
        self.threshold += o.threshold;
        self.structural += o.structural;
        self.pages_not_in_source += o.pages_not_in_source;
        self.pages_absent += o.pages_absent;
        self.pages_rendering += o.pages_rendering;
        self.pages_threshold += o.pages_threshold;
        self.pages_structural += o.pages_structural;
    }
    fn total(&self) -> usize {
        self.not_in_source + self.absent + self.rendering + self.threshold + self.structural
    }
}

/// Ask the three questions of every page that loses something.
fn diagnose(pages: &[&Page]) -> (Losses, Vec<(String, String, usize)>) {
    let mut out = Losses::default();
    // The worst pages by phrases lost in the one bucket that is ours to fix, so the next piece of
    // work has somewhere to start rather than a percentage.
    let mut worst: Vec<(String, String, usize)> = Vec::new();
    for p in pages {
        let shipping = crate::extraction::ours(&p.html);
        let missing: Vec<&String> = p
            .with
            .iter()
            .filter(|s| is_measurable(s) && !contains_snippet(&shipping, s))
            .collect();
        if missing.is_empty() {
            continue;
        }
        // Only the pages that lose something pay for the other three extractions.
        let raw = svipall_core::extraction::extract_text(&p.html);
        let whole =
            crate::extraction::markdown_as_text(&svipall_core::extraction::extract_markdown_opts(
                &p.html,
                &svipall_core::extraction::ExtractOpts::default(),
            ));
        let permissive = crate::extraction::ours_permissive(&p.html);
        let mut here = Losses::default();
        for s in missing {
            if !contains_snippet(&raw, s) {
                // Tags out, crudely, so a phrase that spans `<b>` still reads as one run of
                // words. Attribute values become tokens too, which can only make this *more*
                // forgiving — and a phrase found only in an attribute is not content anyway.
                static TAGS: std::sync::LazyLock<regex::Regex> =
                    std::sync::LazyLock::new(|| regex::Regex::new("<[^>]*>").expect("static"));
                let source = TAGS.replace_all(&p.html, " ");
                if contains_snippet(&source, s) {
                    here.absent += 1;
                } else {
                    here.not_in_source += 1;
                }
            } else if !contains_snippet(&whole, s) {
                here.rendering += 1;
            } else if contains_snippet(&permissive, s) {
                here.threshold += 1;
            } else {
                here.structural += 1;
            }
        }
        here.pages_not_in_source = usize::from(here.not_in_source > 0);
        here.pages_absent = usize::from(here.absent > 0);
        here.pages_rendering = usize::from(here.rendering > 0);
        here.pages_threshold = usize::from(here.threshold > 0);
        here.pages_structural = usize::from(here.structural > 0);
        let ours_to_fix = here.threshold + here.structural;
        if ours_to_fix > 0 {
            worst.push((p.page_type.clone(), p.domain.clone(), ours_to_fix));
        }
        out.merge(&here);
    }
    worst.sort_by_key(|(_, _, n)| std::cmp::Reverse(*n));
    (out, worst)
}

/// Print the diagnosis, per page type, for one split.
fn loss_report(split: &str, pages: &[Page]) {
    let mut by_type: HashMap<String, Losses> = HashMap::new();
    let mut worst_all: Vec<(String, String, usize)> = Vec::new();
    for t in PAGE_TYPES {
        // Borrowed, never cloned: these pages carry their own HTML and copying the corpus once per
        // page type is three quarters of a gigabyte for nothing.
        let subset: Vec<&Page> = pages.iter().filter(|p| p.page_type == **t).collect();
        if subset.is_empty() {
            continue;
        }
        let (l, mut w) = diagnose(&subset);
        worst_all.append(&mut w);
        by_type.insert((*t).to_string(), l);
    }

    println!("\n-- {split}: where the required phrase went --\n");
    println!(
        "{:<16} {:>9} {:>8} {:>10} {:>11} {:>11}",
        "page type", "no source", "walker", "rendering", "threshold", "structural"
    );
    let mut totals = Losses::default();
    for t in PAGE_TYPES {
        let Some(l) = by_type.get(*t) else { continue };
        totals.merge(l);
        println!(
            "{t:<16} {:>9} {:>8} {:>10} {:>11} {:>11}",
            l.not_in_source, l.absent, l.rendering, l.threshold, l.structural
        );
    }
    println!(
        "{:<16} {:>9} {:>8} {:>10} {:>11} {:>11}",
        "ALL",
        totals.not_in_source,
        totals.absent,
        totals.rendering,
        totals.threshold,
        totals.structural
    );
    let share = |n: usize| {
        if totals.total() == 0 {
            0.0
        } else {
            100.0 * n as f64 / totals.total() as f64
        }
    };
    println!(
        "\n{:.0}% absent from the page, {:.0}% lost rendering, {:.0}% lost to a density threshold, \
         {:.0}% lost to the region selector or a rule no option reaches.",
        share(totals.absent),
        share(totals.rendering),
        share(totals.threshold),
        share(totals.structural)
    );
    worst_all.sort_by_key(|(_, _, n)| std::cmp::Reverse(*n));
    if !worst_all.is_empty() {
        println!("\nworst pages, threshold + structural only (the buckets that are ours to fix):");
        for (t, d, n) in worst_all.iter().take(15) {
            println!("  {n:>3} phrases   {t:<14} {d}");
        }
    }
}

/// Sweep the density pass's three tunable numbers against the task metric.
///
/// ▲ This is what `ExtractOpts::prune` was made `Option` for. The constants in `prune.rs` were
/// never fitted against anything — the module says so — and the loss diagnosis puts 60 of the
/// development split's required phrases behind them. Every other bucket is either the tool's own
/// hidden-text rule or an experiment already rejected on the held-out split, so this is the last
/// lever with a number attached.
///
/// Fitted on development only. The held-out split is looked at once, at the end, by the caller.
fn prune_sweep(pages: &[Page]) {
    use svipall_core::extraction::prune::PruneOpts;
    let d = PruneOpts::default();
    let grid: Vec<(String, PruneOpts)> = vec![
        ("shipping".to_string(), d),
        ("min_text 0".into(), PruneOpts { min_text: 0, ..d }),
        ("min_text 60".into(), PruneOpts { min_text: 60, ..d }),
        (
            "link_density 0.35".into(),
            PruneOpts {
                max_link_density: 0.35,
                ..d
            },
        ),
        (
            "link_density 0.65".into(),
            PruneOpts {
                max_link_density: 0.65,
                ..d
            },
        ),
        (
            "min_score 0.20".into(),
            PruneOpts {
                min_score: 0.20,
                ..d
            },
        ),
        (
            "min_score 0.50".into(),
            PruneOpts {
                min_score: 0.50,
                ..d
            },
        ),
        (
            "loosest tried".into(),
            PruneOpts {
                min_text: 0,
                max_link_density: 0.65,
                min_score: 0.20,
                thread: false,
            },
        ),
    ];

    println!("\n-- density thresholds, against the task metric (development only) --\n");
    println!(
        "{:<20} {:>9} {:>8} {:>9}",
        "setting", "kept%", "leak%", "F1"
    );
    for (name, opts) in grid {
        let mut snips = Snippets::default();
        let mut f1 = 0.0f64;
        for p in pages {
            let out = crate::extraction::markdown_as_text(
                &svipall_core::extraction::extract_markdown_opts(
                    &p.html,
                    &svipall_core::extraction::ExtractOpts {
                        main_content_only: true,
                        prune: Some(opts),
                        ..Default::default()
                    },
                ),
            );
            snips.observe(p, &out);
            f1 += word_f1(&p.gold, &out).f1;
        }
        println!(
            "{name:<20} {:>8.1}% {:>7.1}% {:>9.3}",
            100.0 * snips.recall(),
            100.0 * snips.leak(),
            f1 / pages.len().max(1) as f64
        );
    }
}

fn snippet_report(split: &str, pages: &[Page], ways: &[Read]) -> Snippets {
    let mut by_type: HashMap<String, Vec<Snippets>> = HashMap::new();
    for p in pages {
        let slot = by_type
            .entry(p.page_type.clone())
            .or_insert_with(|| vec![Snippets::default(); ways.len()]);
        for (i, way) in ways.iter().enumerate() {
            slot[i].observe(p, &read_page(p, *way));
        }
    }

    println!("-- {split} --");
    let mut totals = vec![Snippets::default(); ways.len()];
    for t in PAGE_TYPES {
        let Some(row) = by_type.get(*t) else { continue };
        for (acc, s) in totals.iter_mut().zip(row) {
            acc.merge(s);
        }
        println!(
            "{t:<16} {:>6} {}   {:>6} losing",
            row.first().map(|s| s.pages).unwrap_or(0),
            row.iter().map(Snippets::cell).collect::<String>(),
            row.first().map(|s| s.pages_losing).unwrap_or(0)
        );
    }
    let all = totals.first().copied().unwrap_or_default();
    println!(
        "{:<16} {:>6} {}   {:>6} losing",
        "ALL",
        all.pages,
        totals.iter().map(Snippets::cell).collect::<String>(),
        all.pages_losing
    );
    println!(
        "{:<16} {:>6} {}",
        "boilerplate leak",
        "",
        totals
            .iter()
            .map(|s| if s.checked == 0 {
                format!("{:>12}", "-")
            } else {
                format!("{:>11.1}%", 100.0 * s.leak())
            })
            .collect::<String>()
    );
    // Said out loud, because a phrase quietly dropped from the denominator is a metric quietly
    // improved. One of WCXB's phrases is an ASCII-art elephant with no words in it.
    if all.wordless > 0 {
        println!(
            "{:<16} {:>6}   ({} phrase(s) had no words in them and cannot be judged by a \
             word-based comparison; excluded)",
            "not measurable", all.wordless, all.wordless
        );
    }
    all
}

/// How well the forum detector agrees with the human labels, per stage.
///
/// Precision and recall rather than accuracy, because forums are 8% of this corpus and an accuracy
/// would be 92% for a detector that never fires. And per stage, because the two stages have very
/// different characters: what a page declares about itself is nearly always true, while what it
/// looks like is a guess. Reporting them together hides which one is paying and which is costing.
fn forum_report(split: &str, pages: &[Page]) {
    use svipall_core::extraction::content::forum::Evidence;

    let mut rows: Vec<(Option<Evidence>, bool)> = Vec::with_capacity(pages.len());
    for p in pages {
        rows.push((
            svipall_core::extraction::content::forum::is_forum(&p.html),
            p.page_type == "forum",
        ));
    }

    let report = |name: &str, fires: &dyn Fn(Option<Evidence>) -> bool| {
        let (mut tp, mut fp, mut missed) = (0usize, 0usize, 0usize);
        for (found, is_forum) in &rows {
            match (fires(*found), is_forum) {
                (true, true) => tp += 1,
                (true, false) => fp += 1,
                (false, true) => missed += 1,
                (false, false) => {}
            }
        }
        let p = if tp + fp == 0 {
            0.0
        } else {
            tp as f64 / (tp + fp) as f64
        };
        let r = if tp + missed == 0 {
            0.0
        } else {
            tp as f64 / (tp + missed) as f64
        };
        println!(
            "  {name:<12} precision {p:.3} ({tp} right, {fp} wrong)   recall {r:.3} ({missed} missed)"
        );
    };

    println!("forum detector, {split}:");
    report("posting", &|e| e == Some(Evidence::Posting));
    report("comment", &|e| e == Some(Evidence::Comment));
    report("structural", &|e| e == Some(Evidence::Structural));
    report("posting+comment", &|e| {
        matches!(e, Some(Evidence::Posting | Evidence::Comment))
    });
    report("any", &|e| e.is_some());
}

/// Score every page of a split, grouped by page type, one vector per reading.
fn score(pages: &[Page], ways: &[Read]) -> HashMap<String, Vec<Vec<crate::extraction::Score>>> {
    let mut by_type: HashMap<String, Vec<Vec<crate::extraction::Score>>> = HashMap::new();
    for p in pages {
        let slot = by_type
            .entry(p.page_type.clone())
            .or_insert_with(|| vec![Vec::new(); ways.len()]);
        for (i, way) in ways.iter().enumerate() {
            slot[i].push(word_f1(&p.gold, &read_page(p, *way)));
        }
    }
    by_type
}

/// Mean F1 for one cell.
///
/// The mean, not the median, because WCXB's leaderboard reports the mean and a number that cannot
/// be compared to the published one is a number with no reference point.
fn mean_cell(scores: &[crate::extraction::Score]) -> String {
    if scores.is_empty() {
        return format!("{:>12}", "-");
    }
    let n = scores.len() as f64;
    format!("{:>12.3}", scores.iter().map(|s| s.f1).sum::<f64>() / n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One-off: the phrases the plain-text walk reaches and the markdown walk loses.
    ///
    /// The smallest bucket in the loss diagnosis and the only one left that is neither a rule nor a
    /// rejected experiment. Both walks read the same tree with the same exclusions, so a phrase in
    /// one and not the other is a rendering defect.
    #[test]
    #[ignore = "probe against the corpus; needs SVIPALL_WCXB"]
    fn what_the_markdown_walk_loses_that_the_text_walk_keeps() {
        let Ok(root) = std::env::var("SVIPALL_WCXB") else {
            return;
        };
        let mut shown = 0usize;
        for p in load(std::path::Path::new(&root), "dev") {
            let raw = svipall_core::extraction::extract_text(&p.html);
            let whole = crate::extraction::markdown_as_text(
                &svipall_core::extraction::extract_markdown_opts(
                    &p.html,
                    &svipall_core::extraction::ExtractOpts::default(),
                ),
            );
            for s in p.with.iter().filter(|s| {
                is_measurable(s) && contains_snippet(&raw, s) && !contains_snippet(&whole, s)
            }) {
                shown += 1;
                eprintln!("--- {} : {:?}", p.domain, &s[..s.len().min(100)]);
                let first = s
                    .split(|c: char| !c.is_alphanumeric())
                    .find(|w| w.len() > 3)
                    .unwrap_or("")
                    .to_lowercase();
                for (label, hay) in [("text", &raw), ("markdown", &whole)] {
                    let h = hay.to_lowercase();
                    match h.find(&first) {
                        Some(i) => eprintln!(
                            "    {label:<9}: {:?}",
                            &h[i.saturating_sub(30)..(i + 110).min(h.len())]
                        ),
                        None => eprintln!("    {label:<9}: {first:?} absent"),
                    }
                }
                if shown >= 8 {
                    return;
                }
            }
        }
        eprintln!("{shown} phrase(s) reached by the text walk and lost by the markdown walk");
    }

    /// One-off: for the phrases present in the HTML but missing from the plain-text walk, which of
    /// the walker's four exclusions is responsible.
    ///
    /// Read-only: it locates the text node in the DOM and reports what is above it. Nothing here
    /// changes what production does; the point is to find out which rule to argue with.
    #[test]
    #[ignore = "probe against the corpus; needs SVIPALL_WCXB"]
    fn which_rule_drops_the_text_the_walker_never_reaches() {
        let Ok(root) = std::env::var("SVIPALL_WCXB") else {
            return;
        };
        use scraper::{ElementRef, Html};
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        let mut shown = 0usize;
        for split in ["dev"] {
            for p in load(std::path::Path::new(&root), split) {
                let raw = svipall_core::extraction::extract_text(&p.html);
                let lost: Vec<&String> = p
                    .with
                    .iter()
                    .filter(|s| is_measurable(s) && !contains_snippet(&raw, s))
                    .collect();
                if lost.is_empty() {
                    continue;
                }
                let doc = Html::parse_document(&p.html);
                for s in lost {
                    // The first word of the phrase, to find where in the tree it lives.
                    let first = s
                        .split(|c: char| !c.is_alphanumeric())
                        .find(|w| w.len() > 3)
                        .unwrap_or("")
                        .to_lowercase();
                    if first.is_empty() {
                        continue;
                    }
                    let mut why = "not found in the tree";
                    'nodes: for node in doc.tree.nodes() {
                        let Some(t) = node.value().as_text() else {
                            continue;
                        };
                        if !t.to_lowercase().contains(&first) {
                            continue;
                        }
                        for a in node.ancestors() {
                            let Some(el) = ElementRef::wrap(a) else {
                                continue;
                            };
                            let name = el.value().name();
                            if [
                                "script", "style", "noscript", "template", "svg", "iframe", "head",
                                "canvas", "object", "embed",
                            ]
                            .contains(&name)
                            {
                                why = match name {
                                    "script" => "inside <script>",
                                    "style" => "inside <style>",
                                    "noscript" => "inside <noscript>",
                                    "template" => "inside <template>",
                                    "svg" => "inside <svg>",
                                    "iframe" => "inside <iframe>",
                                    "head" => "inside <head>",
                                    _ => "inside another skipped tag",
                                };
                                break 'nodes;
                            }
                            if el.value().attr("hidden").is_some() {
                                why = "hidden attribute";
                                break 'nodes;
                            }
                            if el.value().attr("aria-hidden") == Some("true") {
                                why = "aria-hidden";
                                break 'nodes;
                            }
                            if el
                                .value()
                                .attr("style")
                                .is_some_and(svipall_core::extraction::sanitize::is_visually_hidden)
                            {
                                why = "style hides it";
                                break 'nodes;
                            }
                        }
                        why = "reachable, but the phrase is split across nodes";
                        break 'nodes;
                    }
                    *counts.entry(why).or_insert(0) += 1;
                    if why.starts_with("reachable") && shown < 6 {
                        shown += 1;
                        eprintln!("  {}: {:?}", p.domain, &s[..s.len().min(110)]);
                        // What the walker actually produced around the phrase's first word.
                        let rt = raw.to_lowercase();
                        if let Some(i) = rt.find(&first) {
                            let a = i.saturating_sub(40);
                            let b = (i + 110).min(rt.len());
                            eprintln!("      walker: {:?}", &rt[a..b]);
                        } else {
                            eprintln!(
                                "      walker: the first word {first:?} is not in the text at all"
                            );
                        }
                    }
                }
            }
        }
        let mut rows: Vec<(&&str, &usize)> = counts.iter().collect();
        rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        eprintln!("\nwhy the walker never reached it:");
        for (why, n) in rows {
            eprintln!("  {n:>4}  {why}");
        }
    }

    /// One-off: for the pages that lose a phrase to the region selector, which selector was
    /// trusted, how much of the page it held, and where the phrase actually lives.
    #[test]
    #[ignore = "probe against the corpus; needs SVIPALL_WCXB"]
    fn what_the_region_selector_picked_on_the_pages_that_lose_content() {
        let Ok(root) = std::env::var("SVIPALL_WCXB") else {
            return;
        };
        use scraper::{Html, Selector};
        // The production list, read here to report what production would have chosen. Copied
        // deliberately and only for reporting: nothing scored depends on it.
        const MAIN: &[&str] = &[
            "main",
            "article",
            "[role=\"main\"]",
            "#main-content",
            "#main",
            "#content",
            ".main-content",
            ".post-content",
            ".article-body",
        ];
        let pages = load(std::path::Path::new(&root), "dev");
        let mut hits: HashMap<String, (usize, usize)> = HashMap::new();
        let mut none_trusted = 0usize;
        let mut shown = 0usize;
        for p in &pages {
            let shipping = crate::extraction::ours(&p.html);
            let whole = crate::extraction::markdown_as_text(
                &svipall_core::extraction::extract_markdown_opts(
                    &p.html,
                    &svipall_core::extraction::ExtractOpts::default(),
                ),
            );
            let permissive = crate::extraction::ours_permissive(&p.html);
            let structural: Vec<&String> = p
                .with
                .iter()
                .filter(|s| {
                    is_measurable(s)
                        && !contains_snippet(&shipping, s)
                        && contains_snippet(&whole, s)
                        && !contains_snippet(&permissive, s)
                })
                .collect();
            if structural.is_empty() {
                continue;
            }
            let doc = Html::parse_document(&p.html);
            let body_len: usize = Selector::parse("body")
                .ok()
                .and_then(|s| doc.select(&s).next())
                .map(|b| b.text().map(|t| t.trim().len()).sum())
                .unwrap_or(0);
            let mut chosen: Option<(&str, usize)> = None;
            for sel in MAIN {
                let Ok(s) = Selector::parse(sel) else {
                    continue;
                };
                let Some(el) = doc.select(&s).next() else {
                    continue;
                };
                let len: usize = el.text().map(|t| t.trim().len()).sum();
                if len >= 200 && len * 5 >= body_len {
                    chosen = Some((sel, len));
                    break;
                }
            }
            match chosen {
                Some((sel, len)) => {
                    let e = hits.entry(sel.to_string()).or_insert((0, 0));
                    e.0 += 1;
                    e.1 += structural.len();
                    if shown < 6 {
                        shown += 1;
                        eprintln!(
                            "{:<14} {:<28} region {sel} holds {len}/{body_len} = {:.0}%  lost {}",
                            p.page_type,
                            p.domain,
                            100.0 * len as f64 / body_len.max(1) as f64,
                            structural.len()
                        );
                        eprintln!("    e.g. {:?}", structural[0]);
                    }
                }
                None => none_trusted += 1,
            }
        }
        let mut rows: Vec<(&String, &(usize, usize))> = hits.iter().collect();
        rows.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
        eprintln!("\nregion trusted, on pages losing content structurally:");
        for (sel, (pages, phrases)) in rows {
            eprintln!("  {sel:<20} {pages:>4} pages   {phrases:>4} phrases lost");
        }
        eprintln!("  {:<20} {none_trusted:>4} pages   (no region trusted; the loss is the pruner's two untunable clauses)", "none");
    }

    /// One-off: does the new comparison ever call *lost* something the old one called kept?
    /// It must not — it is strictly more permissive by construction — and if it does, the two
    /// disagree about what a word is and the number cannot be trusted.
    #[test]
    #[ignore = "probe against the corpus; needs SVIPALL_WCXB"]
    fn the_new_comparison_is_never_stricter_than_the_old_one() {
        let Ok(root) = std::env::var("SVIPALL_WCXB") else {
            return;
        };
        fn old(haystack: &str, snippet: &str) -> bool {
            let flat = |s: &str| {
                s.split_whitespace()
                    .map(str::to_lowercase)
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let (h, n) = (flat(haystack), flat(snippet));
            !n.is_empty() && h.contains(&n)
        }
        for split in ["dev", "test"] {
            let pages = load(std::path::Path::new(&root), split);
            let (mut kept_old, mut kept_new, mut regressions) = (0usize, 0usize, 0usize);
            for p in &pages {
                let out = crate::extraction::ours(&p.html);
                for s in p.with.iter().filter(|s| is_measurable(s)) {
                    let (o, n) = (old(&out, s), contains_snippet(&out, s));
                    kept_old += usize::from(o);
                    kept_new += usize::from(n);
                    if o && !n {
                        regressions += 1;
                        if regressions <= 3 {
                            let flat = |t: &str| {
                                t.split_whitespace()
                                    .map(str::to_lowercase)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            };
                            let (fh, fs) = (flat(&out), flat(s));
                            let at = fh.find(&fs).unwrap_or(0);
                            let end = (at + fs.len() + 40).min(fh.len());
                            let start = at.saturating_sub(20);
                            eprintln!("--- snippet: {s:?}");
                            eprintln!("    context: {:?}", &fh[start..end]);
                            eprintln!("    snip tokens: {:?}", tokens(s));
                            let ht = tokens(&out);
                            let st = tokens(s);
                            let first = st.first().cloned().unwrap_or_default();
                            if let Some(i) = ht.iter().position(|w| *w == first) {
                                let j = (i + st.len() + 4).min(ht.len());
                                eprintln!("    hay tokens : {:?}", &ht[i..j]);
                            }
                        }
                    }
                }
            }
            eprintln!("{split}: {} pages, old kept {kept_old}, new kept {kept_new}, regressions {regressions}", pages.len());
            assert_eq!(
                regressions, 0,
                "{split}: the new comparison lost something the old one kept"
            );
        }
    }

    /// ▲ The four ways the substring comparison called a delivered phrase missing. Every one of
    /// these is text the caller received in full.
    #[test]
    fn a_phrase_that_was_delivered_counts_even_when_the_rendering_changed_it() {
        let want = "the eastern quay reopens on the fourteenth";
        for delivered in [
            // Markdown emphasis inside the phrase.
            "…said that the **eastern** quay reopens on the fourteenth of November.",
            // A link around part of it. The URL is gone by the time the metric sees this — every
            // comparison runs on `extraction::markdown_as_text`, which strips link targets —
            // so what is left is the label sitting inline in the sentence.
            "…said that the eastern quay reopens on the fourteenth.",
            // A heading marker at the start of the line.
            "## The eastern quay reopens on the fourteenth of November",
            // Non-breaking spaces and a soft hyphen, which is what an entity decodes to.
            "the\u{a0}eastern\u{a0}quay re\u{ad}opens on the fourteenth",
        ] {
            assert!(
                contains_snippet(delivered, want),
                "counted as lost, but it is right there: {delivered:?}"
            );
        }
    }

    /// Typography is the other half. The corpus writes straight quotes; pages use curly ones.
    #[test]
    fn a_curly_apostrophe_is_the_same_word_as_a_straight_one() {
        assert!(contains_snippet(
            "the council\u{2019}s decision was final",
            "the council's decision"
        ));
    }

    /// And it stays a *sequence*: the words have to be there, in order, next to each other. A bag
    /// of words that happens to contain them all is not the phrase.
    #[test]
    fn scattered_words_are_not_the_phrase() {
        assert!(!contains_snippet(
            "quay. Something else entirely. The eastern side. Fourteenth of never.",
            "the eastern quay reopens on the fourteenth"
        ));
        assert!(!contains_snippet("nothing like it at all", "eastern quay"));
        assert!(!contains_snippet("", "eastern quay"));
        assert!(!contains_snippet("eastern quay", ""));
    }

    fn page_with(with: &[&str], without: &[&str]) -> Page {
        Page {
            page_type: "article".into(),
            domain: "x.test".into(),
            html: String::new(),
            gold: "gold".into(),
            with: with.iter().map(|s| s.to_string()).collect(),
            without: without.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_perfect_extraction_keeps_everything_required_and_leaks_nothing() {
        let mut s = Snippets::default();
        s.observe(
            &page_with(
                &["the eastern quay reopens"],
                &["subscribe to our newsletter"],
            ),
            "The council said the eastern quay reopens on the fourteenth.",
        );
        assert_eq!(s.recall(), 1.0);
        assert_eq!(s.leak(), 0.0);
        assert_eq!(s.pages_losing, 0);
    }

    #[test]
    fn the_reverse_is_the_worst_case_and_reads_that_way() {
        let mut s = Snippets::default();
        s.observe(
            &page_with(
                &["the eastern quay reopens"],
                &["subscribe to our newsletter"],
            ),
            "Subscribe to our newsletter for more.",
        );
        assert_eq!(s.recall(), 0.0);
        assert_eq!(s.leak(), 1.0);
        assert_eq!(s.pages_losing, 1);
    }

    /// ▲ Pages, not snippets. One page that loses five phrases is one page to go and look at; a
    /// count of five would make a single bad extraction look like five separate problems and would
    /// make the report unusable for deciding what to fix.
    #[test]
    fn a_page_losing_several_phrases_is_one_page() {
        let mut s = Snippets::default();
        s.observe(
            &page_with(&["first phrase", "second phrase", "third phrase"], &[]),
            "only the first phrase came back",
        );
        assert_eq!(s.pages_losing, 1);
        assert_eq!(s.pages, 1);
        assert_eq!(s.kept, 1);
        assert_eq!(s.wanted, 3);
    }

    /// A page with no snippets must not drag the average down: absent is not zero.
    #[test]
    fn a_page_the_corpus_marked_nothing_on_is_neutral() {
        let mut s = Snippets::default();
        s.observe(&page_with(&[], &[]), "anything at all");
        assert_eq!(s.recall(), 1.0);
        assert_eq!(s.leak(), 0.0);
        assert_eq!(s.pages_losing, 0);
    }

    #[test]
    fn word_overlap_ignores_order_but_not_quantity() {
        let s = word_f1("alpha beta gamma", "gamma beta alpha");
        assert!((s.f1 - 1.0).abs() < 1e-9, "{s:?}");

        // Saying a word twice when the gold says it once is one hit, not two.
        let s = word_f1("alpha beta", "alpha alpha beta");
        assert!((s.precision - 2.0 / 3.0).abs() < 1e-9, "{s:?}");
        assert!((s.recall - 1.0).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn an_empty_side_scores_zero_rather_than_dividing_by_it() {
        assert_eq!(word_f1("alpha", "").f1, 0.0);
        assert_eq!(word_f1("", "alpha").f1, 0.0);
    }

    #[test]
    fn a_snippet_is_found_across_a_line_break_and_a_capital() {
        assert!(contains_snippet(
            "the Eastern quay\n  reopens  on the fourteenth",
            "eastern quay reopens on the"
        ));
        assert!(!contains_snippet("the eastern quay", "western quay"));
        assert!(!contains_snippet("anything at all", ""));
    }
}
