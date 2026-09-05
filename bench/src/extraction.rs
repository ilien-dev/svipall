//! How good svipall's extractor actually is, against the only public gold standard there is.
//!
//! Everything downstream of a fetch is built on the extracted text: the integrity verdict, the
//! quality signals, the tokens the model reads. svipall had no number for it — the extractor was
//! good because it looked good on the pages anyone happened to try.
//!
//! This measures it against the corpus from Bevendorff et al., *An Empirical Comparison of Web
//! Content Extraction Algorithms* (SIGIR 2023): eight annotated datasets, 3,985 pages, scored by
//! ROUGE-LSum F1. Two things make the comparison fair rather than flattering:
//!
//! * The metric is the paper's, implemented from its own definition, and unit-tested here.
//! * The baselines are **the study's own model outputs**, read from its `outputs/model-outputs`
//!   directory and scored by this same code. Nothing is reimplemented, so "Trafilatura scores X"
//!   means the extraction Trafilatura actually produced in the published run, not our idea of it.
//!
//! The corpus is not in this repository — it is 3,985 web pages behind Git LFS. `scripts/
//! fetch-extraction-corpus.sh` gets it; without it this command says so and stops, which is why
//! it is not part of the standard quality gate.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The eight datasets the study combined, by the names it stores them under.
const DATASETS: &[&str] = &[
    "cetd",
    "cleaneval",
    "cleanportaleval",
    "dragnet",
    "google-trends-2017",
    "l3s-gn1",
    "readability",
    "scrapinghub",
];

/// The baselines worth printing beside our own number. The study ran nineteen; these are the three
/// its conclusion names — the two most robust, and the fastest.
const BASELINES: &[&str] = &["readability", "trafilatura", "resiliparse"];

/// The three extraction paths svipall has, all measured, because they are not the same product.
///
/// `svipall` is what a fetch returns: markdown with `main_content_only` **on**, which picks a
/// content root and prunes by link density. `svipall-plain` is the same markdown with that
/// switched off. `svipall-raw` is `extract_text`, which walks the whole document — no root
/// selection, no pruning, nav and footer included.
///
/// The middle column exists because the first two runs of this benchmark did not have it. They
/// passed `ExtractOpts::default()`, whose `main_content_only` is `false`, while the product default
/// is `true` (`server.rs`, `p.main_content_only.unwrap_or(true)`; `web_fetch_many` and `web_crawl`
/// pin it on explicitly). So both published tables scored a path nobody runs, and the gap they
/// reported between "svipall" and "svipall-raw" was markdown rendering, not pruning. Naming the
/// three paths separately is how that stops being possible to do by accident.
const OURS: &[&str] = &["svipall", "svipall-vote", "svipall-plain", "svipall-raw"];

/// The extraction a `web_fetch` would have produced for this page.
pub fn ours(html: &str) -> String {
    markdown_as_text(&svipall_core::extraction::extract_markdown_opts(
        html,
        &svipall_core::extraction::ExtractOpts {
            main_content_only: true,
            ..Default::default()
        },
    ))
}

/// The same page read by three heuristics at once, with only what all of them condemn removed.
///
/// Off in the product until this column says it is better. That is the whole reason it is a column
/// and not a replacement.
fn ours_voted(html: &str) -> String {
    markdown_as_text(&svipall_core::extraction::extract_markdown_opts(
        html,
        &svipall_core::extraction::ExtractOpts {
            main_content_only: true,
            vote: Some(svipall_core::extraction::content::vote::Rule::Unanimous),
            ..Default::default()
        },
    ))
}

/// The same page with `main_content_only` on but the density pass turned down as far as its options
/// allow.
///
/// ▲ This exists to split one bucket in two. A phrase that survives here but not in the shipping
/// extraction was removed by a **threshold** — `min_text`, `max_link_density`, `min_score` — which
/// is a number that can be argued with. A phrase that does not survive here either was removed by
/// the **main-region selector**, or by the two clauses of the pruner that no option reaches
/// (`link_density > 0.8 && share < 0.4`, and a negative class bonus on a small block). Those are
/// structural decisions, not settings.
///
/// It is deliberately not "pruning off": `prune::analyze` has no such mode, and pretending it does
/// by passing extreme numbers would report the residual as if it were zero.
pub fn ours_permissive(html: &str) -> String {
    markdown_as_text(&svipall_core::extraction::extract_markdown_opts(
        html,
        &svipall_core::extraction::ExtractOpts {
            main_content_only: true,
            prune: Some(svipall_core::extraction::prune::PruneOpts {
                min_text: 0,
                max_link_density: 1.0,
                min_score: -1.0,
                thread: false,
            }),
            ..Default::default()
        },
    ))
}

/// The same markdown with boilerplate removal switched off, so the pruning's contribution is a
/// number rather than a belief.
fn ours_unpruned(html: &str) -> String {
    markdown_as_text(&svipall_core::extraction::extract_markdown_opts(
        html,
        &svipall_core::extraction::ExtractOpts::default(),
    ))
}

/// Markdown as plain text, for comparison against extractors that only emit plain text.
///
/// ▲ **Only link and image targets are removed, and that was measured.** The loss diagnosis puts
/// 18 development phrases here: markdown inserts tokens the plain-text walk does not, so a list
/// marker between two sentences (`…is as follows:` `1.` `All queries…`) or a drop-cap rendered as
/// `**F**ree` breaks a phrase that was delivered whole. Stripping them back out was built and
/// measured, and it loses more than it recovers:
///
/// | stripped as well | dev recall | held-out recall |
/// |---|---|---|
/// | **nothing (ships)** | **86.3%** | **93.3%** |
/// | list markers | 86.2% | 93.0% |
/// | list markers and `*`/backtick runs | 86.2% | 92.9% |
///
/// The cause is in the pattern a list marker needs: `d+.` at the start of a line eats a year or
/// a price that begins a paragraph, and those are content. Eighteen phrases were not worth it.
///
/// Only link and image targets are removed. The rest of the syntax — hashes, asterisks, backticks —
/// is stripped by the tokenizer anyway, but a URL is a run of alphanumerics that would be counted
/// as words the extractor invented, and penalise svipall for a feature the baselines do not have.
pub fn markdown_as_text(md: &str) -> String {
    static LINK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"!?\[([^\]]*)\]\([^)]*\)").expect("static pattern")
    });
    LINK.replace_all(md, "$1").into_owned()
}

/// The floors `--assert` holds the extractor to.
///
/// Set from what was measured, with a little room: a floor at the measured number turns ordinary
/// variation into a red build, and a floor far below it stops being a gate. They exist for the same
/// reason the perf budgets do — a number nobody can regress past is worth more than a number in a
/// report nobody re-runs.
///
/// Raising one after an improvement is the intended way to use them. Lowering one is a decision
/// that should be argued for in the commit that does it.
pub mod floors {
    /// Median ROUGE-LSum F1 over the SIGIR-23 gold standard. Measured: 0.920.
    pub const SIGIR_MEDIAN_F1: f64 = 0.90;
    /// Mean word-level F1 over the WCXB development set. Measured: 0.805.
    pub const WCXB_DEV_F1: f64 = 0.78;
    /// The worst language on DAnIEL must stay above this. Measured: 0.608 (Chinese).
    pub const DANIEL_WORST_F1: f64 = 0.55;
    /// ▲ Share of required snippets that must survive. **Measured: 0.863** on WCXB development,
    /// 0.933 held out.
    ///
    /// The task metric, and the one this project should be judged by. A required snippet is a
    /// phrase a person marked as content; losing one is losing the sentence that answers the
    /// question, which an F1 of 0.92 on the same page will happily hide.
    ///
    /// ⚠ This number moved twice, and neither time was the extractor. It was first written as 0.72
    /// from an audit figure never re-derived here; running the harness gave 0.706; and then fixing
    /// how a phrase is compared — see `wcxb::contains_snippet` — gave 0.863. The 15.7 points
    /// between the last two were always being delivered to callers and were being counted as
    /// losses by markdown emphasis, typographic quotes and soft hyphens.
    pub const SNIPPET_RECALL: f64 = 0.84;
    /// Share of boilerplate snippets that may come back. **Measured: 0.131** on WCXB development.
    ///
    /// It rose with the recall for the same reason and by the same mechanism: a comparison that
    /// stops missing delivered phrases stops missing delivered boilerplate too. Symmetric, and
    /// the honest reading of both.
    pub const SNIPPET_LEAK: f64 = 0.15;
    /// Share of reachable gold words the pruned path may drop. Measured: 0.118.
    ///
    /// Part of this is the corpus rather than the extractor — 48% of Dragnet's ground truth is
    /// comments, which an extractor is meant to drop — so the floor is loose. It is here to catch a
    /// change that starts throwing away articles, not to certify the current number as good.
    pub const MAX_CONTENT_LOSS: f64 = 0.15;
}

/// One page scored under one extractor.
///
/// Precision and recall are kept apart because together they say only "worse"; apart they say
/// *how*. Low precision with high recall is an extractor that kept the navigation; the reverse is
/// one that threw the article away. The two want opposite fixes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Score {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

/// ROUGE-LSum, from the definition in the paper.
///
/// Not plain ROUGE-L. The summary-level variant scores each *reference sentence* against the whole
/// candidate and unions the matches, which is what stops an extractor from being rewarded for
/// returning the page in a different order — and rewarded it would be, because a longest common
/// subsequence over the whole document is order-sensitive in a way a reader is not.
///
///   P = Σ_i |LCS∪(T_i, C)| / |C|      R = Σ_i |LCS∪(T_i, C)| / |T|
///
/// where `T_i` is a reference sentence, `C` the candidate, and `LCS∪` the union over candidate
/// sentences of the words of `T_i` that take part in a longest common subsequence.
pub fn rouge_lsum(reference: &str, candidate: &str) -> Score {
    let ref_sents: Vec<Vec<String>> = sentences(reference);
    let cand_sents: Vec<Vec<String>> = sentences(candidate);
    let ref_len: usize = ref_sents.iter().map(|s| s.len()).sum();
    let cand_len: usize = cand_sents.iter().map(|s| s.len()).sum();
    if ref_len == 0 || cand_len == 0 {
        // An empty extraction of an empty page is not a success worth recording, and an empty
        // extraction of a real page is a zero. Both are zero here, and the caller counts pages.
        return Score::default();
    }

    // Every candidate token may be credited once, across all reference lines together.
    //
    // This is the step that is easy to leave out and impossible to leave out: without it a
    // candidate word that matches three reference lines is counted three times, `hits` can exceed
    // the candidate's own length, and precision comes out above 1. It did — the first run of this
    // benchmark reported an F1 of 1.003, which is what sent me looking.
    let mut budget: HashMap<&str, usize> = HashMap::new();
    for c in &cand_sents {
        for t in c {
            *budget.entry(t.as_str()).or_insert(0) += 1;
        }
    }

    let mut hits = 0usize;
    for r in &ref_sents {
        // Which words of this reference line took part in an LCS with *any* candidate line. Union,
        // so a reference line split across two candidate paragraphs is not punished for it.
        let mut matched = vec![false; r.len()];
        // Most pairs of lines share no word at all, and a pair with no word in common has an empty
        // subsequence by definition. Asking first turns the quadratic pass over a large page from
        // minutes into milliseconds and changes no score.
        let vocab: HashSet<&str> = r.iter().map(String::as_str).collect();
        for c in &cand_sents {
            if !c.iter().any(|t| vocab.contains(t.as_str())) {
                continue;
            }
            mark_lcs(r, c, &mut matched);
        }
        for (t, _) in r.iter().zip(&matched).filter(|(_, m)| **m) {
            if let Some(left) = budget.get_mut(t.as_str()) {
                if *left > 0 {
                    *left -= 1;
                    hits += 1;
                }
            }
        }
    }

    let precision = hits as f64 / cand_len as f64;
    let recall = hits as f64 / ref_len as f64;
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    Score {
        precision,
        recall,
        f1,
    }
}

/// Split into sentences, then into tokens, exactly as the reference implementation does.
///
/// Both halves matter, and getting either wrong quietly changes every number:
///
/// * **Sentences are lines.** `rougeLsum` splits on newlines and nothing else. Splitting on
///   sentence-final punctuation as well seems harmless and is not: it turns a paragraph into a
///   dozen three-word units, and a three-word unit finds a subsequence in almost any candidate. It
///   inflates every score toward 1 and flattens the differences the measure exists to show.
/// * **Tokens are lowercased alphanumerics.** Otherwise `Tuesday.` and `Tuesday` are different
///   words, and an extractor is penalised for where the annotator put a comma.
fn sentences(text: &str) -> Vec<Vec<String>> {
    text.split('\n')
        .map(|line| {
            line.split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(|w| w.to_lowercase())
                .collect::<Vec<_>>()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Mark the positions of `a` that take part in a longest common subsequence with `b`.
///
/// Linear space, by Hirschberg's divide and conquer (CACM 1975), rather than the full `n × m`
/// table this used to build. The table was the harness's own failure mode: on the corpus's larger
/// pages one call asked the allocator for tens of megabytes, thousands of times per page, and the
/// process died with no panic and no message — a truncated table read as a finished one. Three
/// separate runs stopped at three different pages, which is what a resource limit looks like and
/// what a bug in the extractor does not. Hirschberg costs the same time, `O(min(n, m))` memory,
/// and recurses only `log2(n)` deep.
fn mark_lcs(a: &[String], b: &[String], matched: &mut [bool]) {
    mark_lcs_at(a, b, 0, matched);
}

fn mark_lcs_at(a: &[String], b: &[String], base: usize, matched: &mut [bool]) {
    if a.is_empty() || b.is_empty() {
        return;
    }
    if a.len() == 1 {
        if b.contains(&a[0]) {
            matched[base] = true;
        }
        return;
    }
    if b.len() == 1 {
        if let Some(i) = a.iter().position(|t| *t == b[0]) {
            matched[base + i] = true;
        }
        return;
    }
    // Split `a` in half, score each half against all of `b` from its own end, and cut `b` where the
    // two halves together account for the most: that column is on some optimal path.
    let mid = a.len() / 2;
    let head = lcs_row(&a[..mid], b, false);
    let tail = lcs_row(&a[mid..], b, true);
    let split = (0..=b.len())
        .max_by_key(|&j| head[j] + tail[b.len() - j])
        .unwrap_or(0);
    mark_lcs_at(&a[..mid], &b[..split], base, matched);
    mark_lcs_at(&a[mid..], &b[split..], base + mid, matched);
}

/// The last row of the LCS length table for `a` against `b`, read forwards or, with `rev`, from
/// both sequences' far ends.
fn lcs_row(a: &[String], b: &[String], rev: bool) -> Vec<u32> {
    let m = b.len();
    let mut prev = vec![0u32; m + 1];
    let mut cur = vec![0u32; m + 1];
    for i in 0..a.len() {
        let ai = if rev { &a[a.len() - 1 - i] } else { &a[i] };
        cur[0] = 0;
        for j in 1..=m {
            let bj = if rev { &b[m - j] } else { &b[j - 1] };
            cur[j] = if ai == bj {
                prev[j - 1] + 1
            } else {
                prev[j].max(cur[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev
}

/// How much of the gold the recall-safe path found and the production path then dropped.
///
/// This is the anti-discard contract as arithmetic. `main_content_only: false` returns the whole
/// document, so anything of the gold that svipall can see at all is in it; turning pruning on can
/// only ever remove. The question that matters is therefore not "how good is the score" but "of
/// the content we could see, how much did we throw away", and it has a right answer: none.
///
/// Reported per page rather than in aggregate, because one page losing an article and a thousand
/// pages losing a word each are the same total and completely different problems.
#[derive(Debug, Default, Clone, Copy)]
pub struct Loss {
    /// Gold words the unpruned path found.
    pub reachable: usize,
    /// Of those, the ones the pruned path no longer returns.
    pub lost: usize,
}

pub fn loss(gold: &str, unpruned: &str, pruned: &str) -> Loss {
    let g = bag(gold);
    let mut reachable = 0usize;
    let mut lost = 0usize;
    let (u, p) = (bag(unpruned), bag(pruned));
    for (w, n) in &g {
        let seen = *u.get(w).unwrap_or(&0).min(n);
        let kept = *p.get(w).unwrap_or(&0).min(n);
        reachable += seen;
        lost += seen.saturating_sub(kept);
    }
    Loss { reachable, lost }
}

fn bag(text: &str) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for w in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        *m.entry(w.to_lowercase()).or_insert(0) += 1;
    }
    m
}

/// The ground truth for one dataset: page id to the text a person said was the content.
fn read_truth(root: &Path, dataset: &str) -> HashMap<String, String> {
    let path = root
        .join("datasets/combined/ground-truth")
        .join(format!("{dataset}.jsonl"));
    read_jsonl(&path)
}

/// One extractor's answers for one dataset, in the same shape as the ground truth.
fn read_model(root: &Path, dataset: &str, model: &str) -> HashMap<String, String> {
    let path = root
        .join("outputs/model-outputs")
        .join(dataset)
        .join(format!("{model}.jsonl"));
    read_jsonl(&path)
}

/// `{"page_id": ..., "plaintext": ...}` per line, which is the study's own interchange format.
fn read_jsonl(path: &Path) -> HashMap<String, String> {
    let Ok(body) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    body.lines()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            Some((
                v.get("page_id")?.as_str()?.to_string(),
                v.get("plaintext")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// The pages of one dataset, as `(page_id, html)`.
fn read_pages(root: &Path, dataset: &str, wanted: &HashSet<String>) -> Vec<(String, String)> {
    let dir = root.join("datasets/combined/html").join(dataset);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = entries
        .filter_map(|e| {
            let path: PathBuf = e.ok()?.path();
            if path.extension()? != "html" {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            if !wanted.contains(&id) {
                return None;
            }
            Some((id, std::fs::read_to_string(&path).ok()?))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Our own three numbers, side by side, or dashes when the dataset produced none.
fn ours_cells(scores: Option<&Vec<Score>>) -> String {
    match scores {
        Some(v) if !v.is_empty() => format!(
            "{:>7.3}{:>7.3}{:>7.3}",
            median_of(v, |s| s.precision),
            median_of(v, |s| s.recall),
            median_of(v, |s| s.f1)
        ),
        _ => format!("{:>7}{:>7}{:>7}", "-", "-", "-"),
    }
}

fn median_of(scores: &[Score], pick: impl Fn(&Score) -> f64) -> f64 {
    median(scores.iter().map(pick).collect())
}

/// Median, because the paper says to report one: an extractor that fails completely on a handful
/// of pages and does well on the rest has a mean that describes neither case.
fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = xs.len() / 2;
    if xs.len().is_multiple_of(2) {
        (xs[mid - 1] + xs[mid]) / 2.0
    } else {
        xs[mid]
    }
}

/// Median, mean and the interquartile range of one column.
///
/// All four, because a single figure of merit here is a lie in either direction. Bevendorff et al.
/// (SIGIR 2023, §4.4) found the per-page score distribution to be power-shaped with an
/// over-represented pile of zeros, so that for the better extractors the arithmetic mean "falls
/// only barely within" the interquartile range: the median flatters, the mean punishes outlier
/// pages twice, and neither of them says how wide the spread is.
#[derive(Debug, Clone, Copy, Default)]
pub struct Spread {
    pub median: f64,
    pub mean: f64,
    pub q1: f64,
    pub q3: f64,
}

fn spread_of(scores: &[Score], pick: impl Fn(&Score) -> f64) -> Spread {
    let mut xs: Vec<f64> = scores.iter().map(pick).collect();
    if xs.is_empty() {
        return Spread::default();
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Spread {
        median: median(xs.clone()),
        mean,
        q1: quantile(&xs, 0.25),
        q3: quantile(&xs, 0.75),
    }
}

/// Linear-interpolated quantile of an already-sorted slice.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

pub fn run(root: &Path, assert: bool) -> usize {
    if !root.join("datasets/combined/ground-truth").is_dir() {
        eprintln!(
            "no corpus at {}\n\n\
             The gold standard is 3,985 annotated pages from Bevendorff et al. (SIGIR 2023) and is\n\
             not carried in this repository. Fetch it once with:\n\n    \
             scripts/fetch-extraction-corpus.sh [target-dir]\n\n\
             then point this command at it:\n\n    \
             cargo run -p svipall-bench --release -- extract --corpus <target-dir>",
            root.display()
        );
        return 1;
    }

    // svipall's own extraction, plus whichever baselines the corpus carries answers for.
    let mut columns: Vec<&str> = vec!["svipall"];
    columns.extend(BASELINES);
    let mut totals: HashMap<&str, Vec<Score>> = HashMap::new();
    // Kept so the run can end by naming the pages it did worst on rather than only the average of
    // them: an average says there is a problem, a page says what the problem is.
    let mut worst: Vec<(f64, &str, String, usize, usize)> = Vec::new();
    // Pages the corpus cannot grade, counted rather than scored as zero.
    let mut skipped = 0usize;
    // The content-loss tally: gold the recall-safe path reached, and what pruning then dropped.
    let (mut reachable, mut lost_words) = (0usize, 0usize);
    let mut lost_pages: Vec<(usize, &str, String)> = Vec::new();

    // Precision and recall for svipall, F1 for the rest. Our own number is the one being
    // diagnosed, and F1 alone says only "worse" — the split says which way.
    println!(
        "{:<20} {:>6} {:>7}{:>7}{:>7}  {}",
        "dataset",
        "pages",
        "P",
        "R",
        "F1",
        OURS.iter()
            .skip(1)
            .chain(BASELINES)
            .map(|c| format!("{c:>14}"))
            .collect::<Vec<_>>()
            .join("")
    );

    for ds in DATASETS {
        let truth = read_truth(root, ds);
        if truth.is_empty() {
            continue;
        }
        let wanted: HashSet<String> = truth.keys().cloned().collect();
        let pages = read_pages(root, ds, &wanted);
        if pages.is_empty() {
            continue;
        }

        let mut per_column: HashMap<&str, Vec<Score>> = HashMap::new();
        let baseline_answers: HashMap<&str, HashMap<String, String>> = BASELINES
            .iter()
            .map(|m| (*m, read_model(root, ds, m)))
            .collect();

        for (id, html) in &pages {
            let Some(gold) = truth.get(id) else { continue };
            // Ten of the 3,985 pages have an empty ground truth. Nothing can be scored against
            // nothing: they enter every column as a zero, sink the mean, and fill the worst-pages
            // table with the corpus's own defects instead of the extractor's.
            if gold.trim().is_empty() {
                skipped += 1;
                continue;
            }
            // The extraction a fetch would have produced, through the same entry point and the
            // same options the product uses. Measuring anything else measures something nobody
            // runs — which is what the first two versions of this benchmark did.
            let mine = ours(html);
            let s = rouge_lsum(gold, &mine);
            per_column.entry("svipall").or_default().push(s);
            worst.push((s.f1, ds, id.clone(), gold.len(), mine.len()));

            per_column
                .entry("svipall-vote")
                .or_default()
                .push(rouge_lsum(gold, &ours_voted(html)));

            // The same markdown with main-content selection and pruning switched off, so what the
            // removal is worth is a difference between two measured columns.
            let plain = ours_unpruned(html);
            per_column
                .entry("svipall-plain")
                .or_default()
                .push(rouge_lsum(gold, &plain));

            // And what the removal cost. The unpruned path is the recall-safe one, so any gold
            // word it found and the pruned path did not is content svipall could see and threw
            // away — the one thing the quality design says must never happen.
            let l = loss(gold, &plain, &mine);
            reachable += l.reachable;
            if l.lost > 0 {
                lost_words += l.lost;
                lost_pages.push((l.lost, ds, id.clone()));
            }

            // And the plain-text walker, which shares neither the root selection nor the markdown
            // syntax, so the two effects stay apart.
            let raw = svipall_core::extraction::extract_text(html);
            per_column
                .entry("svipall-raw")
                .or_default()
                .push(rouge_lsum(gold, &raw));

            for m in BASELINES {
                if let Some(answer) = baseline_answers.get(m).and_then(|a| a.get(id)) {
                    per_column
                        .entry(m)
                        .or_default()
                        .push(rouge_lsum(gold, answer));
                }
            }
        }

        println!(
            "{ds:<20} {:>6} {}  {}",
            pages.len(),
            ours_cells(per_column.get("svipall")),
            OURS.iter()
                .skip(1)
                .chain(BASELINES)
                .map(|c| match per_column.get(c) {
                    Some(v) if !v.is_empty() => format!("{:>14.3}", median_of(v, |s| s.f1)),
                    _ => format!("{:>14}", "-"),
                })
                .collect::<String>()
        );

        for (c, v) in per_column {
            totals.entry(c).or_default().extend(v);
        }
    }

    if totals.is_empty() {
        eprintln!("\nthe corpus is present but no dataset could be read");
        return 1;
    }

    let pages = totals.get("svipall").map(Vec::len).unwrap_or(0);
    println!(
        "{:<20} {pages:>6} {}  {}",
        "ALL (median)",
        ours_cells(totals.get("svipall")),
        OURS.iter()
            .skip(1)
            .chain(BASELINES)
            .map(|c| match totals.get(c) {
                Some(v) if !v.is_empty() => format!("{:>14.3}", median_of(v, |s| s.f1)),
                _ => format!("{:>14}", "-"),
            })
            .collect::<String>()
    );
    println!(
        "\nROUGE-LSum, median over pages. P and R are svipall's own, split because F1 alone says\n\
         only \"worse\": low P with high R is an extractor keeping the navigation, and the reverse\n\
         is one throwing the article away. Baselines are the study's own published extractions,\n\
         scored by this same code."
    );

    // The distribution, not just its middle. A median of 0.92 with a first quartile of 0.55 is a
    // different extractor from a median of 0.92 with a first quartile of 0.88, and the two want
    // different work.
    println!(
        "\n{:<16} {:>8} {:>8} {:>17}",
        "column", "median", "mean", "IQR"
    );
    for c in OURS.iter().chain(BASELINES) {
        let Some(v) = totals.get(c).filter(|v| !v.is_empty()) else {
            continue;
        };
        let s = spread_of(v, |s| s.f1);
        println!(
            "{c:<16} {:>8.3} {:>8.3} {:>8.3} - {:<6.3}",
            s.median, s.mean, s.q1, s.q3
        );
    }
    if skipped > 0 {
        println!(
            "\n{skipped} pages were not scored: their ground truth is empty, so there is nothing \
             for an extraction\nto be right or wrong about."
        );
    }

    // ▲ The content-loss report. Everything above is a score to be argued about; this is the one
    // number with a right answer. `svipall-plain` is the recall-safe path, so every gold word it
    // returned is a word svipall could see. Any of those the production path no longer returns is
    // content that was thrown away, and the quality design says that must not happen.
    if reachable > 0 {
        println!(
            "\ncontent reachable and then dropped: {lost_words} of {reachable} gold words \
             ({:.2}%), on {} of {} pages",
            100.0 * lost_words as f64 / reachable as f64,
            lost_pages.len(),
            totals.get("svipall").map(Vec::len).unwrap_or(0)
        );
        if !lost_pages.is_empty() {
            lost_pages.sort_by_key(|p| std::cmp::Reverse(p.0));
            let shown: Vec<String> = lost_pages
                .iter()
                .take(10)
                .map(|(n, ds, id)| {
                    format!("{ds}/{} (-{n})", id.chars().take(10).collect::<String>())
                })
                .collect();
            println!("worst losses: {}", shown.join(", "));
        }
    }

    // The pages it did worst on, by name, so the next person does not have to guess which they
    // were. `gold` and `ours` are character counts: a huge `ours` beside a small `gold` is the
    // whole page coming back, and the opposite is the article being lost.
    worst.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    println!("\nworst pages:");
    println!(
        "{:<8} {:<20} {:<20} {:>8} {:>8}",
        "F1", "dataset", "page", "gold", "ours"
    );
    for (f1, ds, id, gold, ours) in worst.iter().take(15) {
        println!(
            "{f1:<8.3} {ds:<20} {:<20} {gold:>8} {ours:>8}",
            id.chars().take(18).collect::<String>()
        );
    }

    // A measurement first, and a gate only when asked. There was no threshold here at all until the
    // first reading existed: pinning one before knowing the number would have been choosing the
    // answer rather than measuring it.
    if !assert {
        return 0;
    }
    let mut failures = 0usize;
    let median = totals
        .get("svipall")
        .map(|v| median_of(v, |s| s.f1))
        .unwrap_or(0.0);
    if median < floors::SIGIR_MEDIAN_F1 {
        eprintln!(
            "FAIL median F1 {median:.3} is below the floor of {:.3}",
            floors::SIGIR_MEDIAN_F1
        );
        failures += 1;
    }
    let loss = if reachable > 0 {
        lost_words as f64 / reachable as f64
    } else {
        0.0
    };
    if loss > floors::MAX_CONTENT_LOSS {
        eprintln!(
            "FAIL {:.1}% of reachable content was dropped; the ceiling is {:.1}%",
            100.0 * loss,
            100.0 * floors::MAX_CONTENT_LOSS
        );
        failures += 1;
    }
    if failures == 0 {
        eprintln!(
            "ok   extraction: median F1 {median:.3} (floor {:.3}), content loss {:.1}% \n             (ceiling {:.1}%)",
            floors::SIGIR_MEDIAN_F1,
            100.0 * loss,
            100.0 * floors::MAX_CONTENT_LOSS
        );
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page shaped like the thing the pruner exists for: an article wrapped in a link rail.
    const NAV_AND_ARTICLE: &str = r#"<html><body>
        <div class="sidebar"><ul>
          <li><a href="/1">Link one</a></li><li><a href="/2">Link two</a></li>
          <li><a href="/3">Link three</a></li><li><a href="/4">Link four</a></li>
        </ul></div>
        <div class="content">
          <p>The quick brown fox jumps over the lazy dog, repeatedly, with enthusiasm and care.</p>
          <p>Ownership in Rust means each value has a single owner, and the compiler enforces it.</p>
          <p>Borrowing lets you reference data without taking ownership, which avoids copying.</p>
        </div>
      </body></html>"#;

    /// The benchmark has to score the extraction a fetch performs, not a neighbouring one.
    ///
    /// This is not hypothetical. Two published runs of this table passed `ExtractOpts::default()`,
    /// whose `main_content_only` is `false`, while `web_fetch` reads
    /// `p.main_content_only.unwrap_or(true)`. The tables measured the unpruned path and were read
    /// as measurements of the pruned one, so the reported difference between svipall and its own
    /// plain-text walker was markdown syntax rather than boilerplate removal. The mistake is
    /// invisible in the output and survives review; it needs an assertion.
    #[test]
    fn the_benchmark_scores_the_path_the_product_runs() {
        let mine = ours(NAV_AND_ARTICLE);
        assert!(
            mine.contains("Ownership in Rust"),
            "the article was lost: {mine}"
        );
        assert!(
            !mine.contains("Link three"),
            "the `svipall` column is scoring the unpruned path: {mine}"
        );

        let plain = ours_unpruned(NAV_AND_ARTICLE);
        assert!(
            plain.contains("Link three"),
            "the `svipall-plain` column must keep the navigation, or the two columns \
             measure the same thing: {plain}"
        );
    }

    /// The metric has to survive the corpus, not just its examples.
    ///
    /// The full-table LCS this replaced allocated `n × m` per pair of lines. On the largest pages
    /// that was tens of megabytes a call, thousands of calls a page, and the benchmark process died
    /// silently part-way through — three runs, three different pages, no panic and no message. A
    /// harness that stops early and says nothing is worse than one that is wrong out loud, so the
    /// size that broke it is pinned here.
    #[test]
    fn a_page_sized_line_does_not_exhaust_the_scorer() {
        let words: Vec<String> = (0..8_000).map(|i| format!("w{}", i % 700)).collect();
        let line = words.join(" ");
        let s = rouge_lsum(&line, &line);
        assert!(
            (s.f1 - 1.0).abs() < 1e-9,
            "a text scored against itself is 1: {s:?}"
        );

        // And the same length with only part of it kept, so the marking path is exercised too.
        let half: String = words[..4_000].join(" ");
        let s = rouge_lsum(&line, &half);
        assert!(s.precision > 0.99, "everything kept was in the gold: {s:?}");
        assert!(
            s.recall > 0.45 && s.recall < 0.55,
            "half the gold is half the recall: {s:?}"
        );
    }

    /// Skipping line pairs with no word in common must not change a score, only the time.
    #[test]
    fn the_disjoint_pair_shortcut_changes_no_score() {
        let gold = "the council voted on tuesday\na second reading is expected\nunrelated filler";
        let cand = "zzz qqq vvv\nthe council voted on tuesday\nmore zzz qqq";
        let s = rouge_lsum(gold, cand);
        // Five of the nine candidate words are the first gold line, in order.
        assert!((s.precision - 5.0 / 11.0).abs() < 1e-9, "{s:?}");
        assert!((s.recall - 5.0 / 12.0).abs() < 1e-9, "{s:?}");
    }

    /// The content-loss measure has to count what was *reachable*, not what was ideal.
    ///
    /// Gold that svipall never saw at all — text injected by JavaScript, or an annotation of a page
    /// that no longer matches its HTML — is not something pruning threw away, and charging it to
    /// pruning would make the one number with a right answer unusable.
    #[test]
    fn loss_is_measured_against_what_the_recall_path_reached() {
        let gold = "the council voted on tuesday and a second reading is expected";
        let unpruned =
            "menu home about the council voted on tuesday and a second reading is expected";
        // Pruning kept the article and dropped the menu: nothing of the gold was lost.
        let clean = loss(
            gold,
            unpruned,
            "the council voted on tuesday and a second reading is expected",
        );
        assert_eq!(clean.lost, 0, "{clean:?}");
        assert_eq!(clean.reachable, 11, "{clean:?}");

        // Pruning took half the article with it.
        let bad = loss(gold, unpruned, "the council voted on tuesday");
        assert_eq!(bad.lost, 6, "{bad:?}");

        // Gold the recall path never found is not pruning's doing.
        let unreachable = loss(
            gold,
            "the council voted on tuesday",
            "the council voted on tuesday",
        );
        assert_eq!(unreachable.reachable, 5, "{unreachable:?}");
        assert_eq!(unreachable.lost, 0, "{unreachable:?}");
    }

    #[test]
    fn an_exact_extraction_scores_one() {
        let t = "The council voted on Tuesday. A second reading is expected.";
        assert!((rouge_lsum(t, t).f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_extraction_of_a_real_page_scores_zero() {
        assert_eq!(rouge_lsum("The council voted on Tuesday.", "").f1, 0.0);
    }

    #[test]
    fn keeping_the_navigation_costs_precision_and_not_recall() {
        // The failure mode of a permissive extractor: everything the person marked is there, and
        // so is the rest of the page. Recall is perfect, so only precision can say anything.
        let gold = "The council voted on Tuesday. A second reading is expected.";
        let with_chrome = "Home About Contact Subscribe Careers Privacy Terms. \
             The council voted on Tuesday. A second reading is expected. \
             Copyright 2026 Example Incorporated. All rights reserved.";
        let f1 = rouge_lsum(gold, with_chrome).f1;
        assert!(f1 > 0.0 && f1 < 0.75, "{f1}");
    }

    #[test]
    fn dropping_half_the_article_costs_recall() {
        let gold = "The council voted on Tuesday. A second reading is expected.";
        let half = "The council voted on Tuesday.";
        let f1 = rouge_lsum(gold, half).f1;
        assert!(f1 > 0.5 && f1 < 1.0, "{f1}");
    }

    #[test]
    fn reordering_paragraphs_is_not_punished_the_way_a_plain_lcs_would() {
        // This is the whole reason the summary-level variant exists. Under plain ROUGE-L, moving
        // the second line above the first loses one of them entirely; under LSum each reference
        // line is matched against the candidate as a whole.
        let gold = "The council voted on Tuesday.\nA second reading is expected.";
        let swapped = "A second reading is expected.\nThe council voted on Tuesday.";
        assert!((rouge_lsum(gold, swapped).f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_score_can_never_exceed_one() {
        // The bug this pins produced a published-looking table with an F1 of 1.003 in it. A
        // candidate word that matches several reference lines was credited to each of them, so the
        // hit count passed the candidate's own length and precision went over 1.
        let gold = "the council voted\nthe council voted again\nthe council voted once more";
        let candidate = "the council voted";
        let f1 = rouge_lsum(gold, candidate).f1;
        assert!(f1 <= 1.0, "{f1}");
        assert!(f1 > 0.0, "{f1}");

        // And the general case: a candidate that repeats one line of the reference many times.
        let s = rouge_lsum("alpha beta\ngamma delta", &"alpha beta\n".repeat(50));
        assert!(s.f1 <= 1.0, "{s:?}");
        assert!(s.precision <= 1.0 && s.recall <= 1.0, "{s:?}");
    }

    #[test]
    fn punctuation_and_case_are_not_what_is_being_measured() {
        // An extractor is being scored on what it kept, not on whether it reproduced the
        // annotator's commas.
        let gold = "The council voted on Tuesday.";
        assert!((rouge_lsum(gold, "the COUNCIL voted, on Tuesday").f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_line_is_a_sentence_and_a_full_stop_is_not() {
        // The bug this pins cost the first run of this benchmark its meaning: splitting on `.` as
        // well as on newlines turns a paragraph into a dozen tiny units, each of which finds a
        // subsequence in almost anything, and every extractor scores near 1.
        let gold = "Alpha beta gamma. Delta epsilon zeta. Eta theta iota.";
        // Same words, one per unit, in an order no reader would accept.
        let shuffled = "gamma beta Alpha. zeta epsilon Delta. iota theta Eta.";
        let f1 = rouge_lsum(gold, shuffled).f1;
        assert!(f1 < 0.7, "a shuffle must cost something: {f1}");
    }

    #[test]
    fn a_missing_corpus_is_reported_rather_than_scored_as_zero() {
        let nowhere = std::env::temp_dir().join("svipall-no-corpus-here");
        assert_eq!(
            run(&nowhere, false),
            1,
            "a missing corpus is a failure to run"
        );
    }
}
