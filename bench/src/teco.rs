//! The only corpus that can evaluate cross-page template detection, because it is the only one
//! that ships the other pages.
//!
//! TECO (Alarte & Silva, `arXiv:1409.6182`, BSD) downloaded 150 real websites — the key page *and
//! the pages reachable from it* — and had four engineers agree, per DOM node, on what is template
//! and what is content. The labels are HTML classes on the key page:
//!
//! | class | means |
//! |---|---|
//! | `TECO_mainContent` | this node is the page's main content |
//! | `TECO_notTemplate` | this node is not part of the site template |
//! | `TECO_mainMenu` | this node is the main menu |
//!
//! ▲ **Its condition of use is that results obtained with it are published.** Everything this
//! module measures belongs in `docs/extraction.md`, including a result that says the method did not
//! help — especially that one.
//!
//! ## What is scored
//!
//! Two questions, and the second is the one nothing else in this workspace can ask.
//!
//! 1. **Page level.** How much of `TECO_mainContent` does the shipping extractor return, and how
//!    much else does it return with it. Precision and recall over words, the same measure WCXB
//!    uses, so the two are readable side by side.
//! 2. **Cross page.** Learn a `template::Template` from the key page's *siblings* — real pages of
//!    the same site, which is the thing no other corpus provides — then strip the key page with it.
//!
//! ▲ The second one is what retired the feature from the default path. **One word** of
//! `TECO_mainContent` that the extractor had reached was removed by the template, and one is too
//! many for something on by default — so `use_site_template` is opt-in, and this run reports the
//! price rather than pretending it is zero.
//!
//! The gate that remains is `MAX_TEMPLATE_LOSS`: the cost may not *grow*. It has to reach zero
//! before this could ever be on by default again, and until then a build that makes it worse is a
//! build that fails.
//!
//! ## Layout
//!
//! From the corpus's own README, verified against the archive: each category zip extracts to a
//! directory of website directories, one per domain, holding the key page and everything reachable
//! from it. The key page is the one carrying the label classes; every other HTML file beside it is
//! a sibling. Nothing here assumes a fixed depth — the key page is *found*, not looked up.

use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One website: the labelled key page, and the pages of the same site around it.
pub struct Site {
    pub domain: String,
    pub html: String,
    /// The text four engineers agreed is this page's main content.
    pub gold: String,
    /// Other pages of the same site, capped — a site with four hundred pages teaches no more about
    /// its own template than its first two dozen do, and reading them all would dominate the run.
    pub siblings: Vec<String>,
}

/// Siblings read per site. Above `template::MIN_PAGES` so the shipping threshold can be exercised
/// rather than merely reported as unreachable.
const MAX_SIBLINGS: usize = 24;

/// What the opt-in cross-page template costs, in words of human-labelled main content, on the
/// pages this corpus labels. **Measured: 1**, on `communities.apple.com`, at `MIN_BLOCK = 120`
/// (it was 12 across 3 sites at 40).
///
/// ▲ Not a floor with room, the way the extraction floors have room. This is the exact measured
/// cost of a feature that is **off by default because of it**, and the gate is that it must not
/// grow. Zero is what it has to reach before `use_site_template` could ever become the default;
/// anything above this is a regression in something already known to be imperfect.
pub const MAX_TEMPLATE_LOSS: usize = 1;

/// Pages larger than this are skipped as siblings. TECO stores pages as downloaded, inlined assets
/// and all, and a single page can be tens of megabytes of base64.
const MAX_PAGE_BYTES: u64 = 4 << 20;

fn is_html(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}

/// Every HTML file under a directory, depth-first, bounded.
fn html_files(dir: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    // Sorted, so two machines read the same pages in the same order and get the same answer.
    paths.sort();
    for p in paths {
        if out.len() >= limit {
            return;
        }
        if p.is_dir() {
            html_files(&p, out, limit);
        } else if is_html(&p) {
            out.push(p);
        }
    }
}

/// The text of every node the corpus labelled as main content.
///
/// Nested labels are the normal case — a labelled `<div>` containing labelled `<p>`s — so only the
/// outermost match is read, or the same sentence would be counted several times.
fn labelled_content(doc: &Html) -> String {
    let sel = Selector::parse(".TECO_mainContent").expect("static selector");
    let mut out = String::new();
    let labelled = |el: &ElementRef| {
        el.value()
            .attr("class")
            .is_some_and(|c| c.split_whitespace().any(|x| x == "TECO_mainContent"))
    };
    for el in doc.select(&sel) {
        if el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|a| labelled(&a))
        {
            continue;
        }
        for t in el.text() {
            let t = t.trim();
            if !t.is_empty() {
                out.push_str(t);
                out.push(' ');
            }
        }
    }
    out
}

/// Directories that are candidate sites: the children of `root`, or the children of its single
/// wrapper directory when the archive nests one (`forum/`, `pages/`).
///
/// ▲ `root` itself is never a site. The first version of this walked down from the root looking for
/// a labelled page, found one deep inside the *first* domain, and concluded the whole corpus was
/// one website — reporting 1 site out of 30 as cleanly as it would have reported 30. A directory is
/// a site because of where it sits, not because something under it happens to carry a label.
fn site_dirs(root: &Path) -> Vec<PathBuf> {
    let children = |d: &Path| -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(d) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        out.sort();
        out
    };
    let mut dirs = children(root);
    // A single wrapper directory is the archive's own root (`forum/`), not a website.
    while dirs.len() == 1 {
        let inner = children(&dirs[0]);
        if inner.is_empty() {
            break;
        }
        dirs = inner;
    }
    dirs
}

/// Load the sites under a TECO category directory.
pub fn load(root: &Path, limit: usize) -> Vec<Site> {
    let mut sites = Vec::new();
    for dir in site_dirs(root) {
        if sites.len() >= limit {
            break;
        }
        let mut pages = Vec::new();
        html_files(&dir, &mut pages, 400);
        let Some(key) = pages.iter().find(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.contains("TECO_mainContent"))
                .unwrap_or(false)
        }) else {
            continue;
        };
        let Ok(html) = std::fs::read_to_string(key) else {
            continue;
        };
        let gold = labelled_content(&Html::parse_document(&html));
        if gold.split_whitespace().count() < 20 {
            // A site whose labelled content is a handful of words cannot separate a good
            // extraction from a bad one.
            continue;
        }
        let siblings: Vec<String> = pages
            .iter()
            .filter(|p| *p != key)
            .filter(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX) <= MAX_PAGE_BYTES)
            .take(MAX_SIBLINGS)
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect();
        sites.push(Site {
            domain: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            html,
            gold,
            siblings,
        });
    }
    sites
}

/// The shipping extraction: what a `web_fetch` of this page would return.
fn shipping(html: &str) -> String {
    svipall_core::extraction::extract_markdown_opts(
        html,
        &svipall_core::extraction::ExtractOpts {
            main_content_only: true,
            ..Default::default()
        },
    )
}

fn words(s: &str) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for w in s
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
    {
        *m.entry(w.to_lowercase()).or_insert(0) += 1;
    }
    m
}

/// Score both questions and print them. Returns the number of failures, for `--assert`.
pub fn run(root: &Path, assert: bool) -> usize {
    if !root.is_dir() {
        eprintln!(
            "no TECO corpus at {}\n\n\
             150 websites shipped WITH their sibling pages, labelled per DOM node (Alarte & Silva, \
             BSD). It is the only public corpus that can evaluate cross-page template detection. \
             Fetch a category with:\n\n    \
             scripts/fetch-teco.sh [target-dir] [category]\n\n\
             then point this command at it:\n\n    \
             cargo run -p svipall-bench --release -- extract --teco <target-dir>",
            root.display()
        );
        return 1;
    }

    let sites = load(root, 200);
    println!("\n== TECO (per-node labels, and the sibling pages nothing else ships) ==\n");
    if sites.is_empty() {
        eprintln!(
            "the directory is there but no labelled key page was found under it. TECO marks its \
             key pages with class TECO_mainContent; if none is present the archive is partial or \
             its layout has changed."
        );
        return 1;
    }

    // --- Page level -------------------------------------------------------------------------
    let mut scores = Vec::new();
    let mut with_siblings = 0usize;
    for s in &sites {
        let out = shipping(&s.html);
        scores.push(crate::wcxb::word_f1(&s.gold, &out));
        if s.siblings.len() as u32 >= svipall_core::template::MIN_PAGES {
            with_siblings += 1;
        }
    }
    let mean = |f: &dyn Fn(&crate::extraction::Score) -> f64| {
        scores.iter().map(f).sum::<f64>() / scores.len().max(1) as f64
    };
    println!(
        "{} sites, {} of them with at least {} sibling pages\n",
        sites.len(),
        with_siblings,
        svipall_core::template::MIN_PAGES
    );
    println!(
        "page level, against TECO_mainContent:   P {:.3}   R {:.3}   F1 {:.3}",
        mean(&|s| s.precision),
        mean(&|s| s.recall),
        mean(&|s| s.f1)
    );

    // --- Cross page -------------------------------------------------------------------------
    //
    // ▲ The measurement this corpus exists for, and the gate on the one feature in this project
    // able to remove text a caller would otherwise have had. The template is learned from the
    // *siblings* — never from the key page — and then applied to the key page, which is exactly the
    // order a fetch runs in.
    let mut armed = 0usize;
    let mut touched = 0usize;
    let mut saved = 0.0f64;
    let mut lost_sites: Vec<(&str, usize)> = Vec::new();
    let mut gold_words = 0usize;
    let mut gold_lost = 0usize;

    for s in &sites {
        let mut t = svipall_core::template::Template::default();
        for sib in &s.siblings {
            t.observe(&shipping(sib));
        }
        if !t.armed() {
            continue;
        }
        armed += 1;
        let before = shipping(&s.html);
        let (after, applied) = t.strip(&before);
        if applied.removed_blocks > 0 {
            touched += 1;
            saved += before.len().saturating_sub(after.len()) as f64 / before.len().max(1) as f64;
        }
        // Gold words the extractor reached, and how many of those the template then took away.
        let gold = words(&s.gold);
        let (wb, wa) = (words(&before), words(&after));
        let mut lost_here = 0usize;
        for (w, n) in &gold {
            let reached = wb.get(w).copied().unwrap_or(0).min(*n);
            let kept = wa.get(w).copied().unwrap_or(0).min(*n);
            gold_words += reached;
            gold_lost += reached.saturating_sub(kept);
            lost_here += reached.saturating_sub(kept);
        }
        if lost_here > 0 {
            lost_sites.push((&s.domain, lost_here));
        }
    }

    println!(
        "cross page: {armed} sites armed at {} pages, {touched} had template removed, \
         {:.1}% of the delivered text saved where it fired",
        svipall_core::template::MIN_PAGES,
        if touched > 0 {
            100.0 * saved / touched as f64
        } else {
            0.0
        }
    );

    let mut failures = 0usize;
    if gold_lost > 0 {
        lost_sites.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        // ▲ "No worse than measured", not "zero". Zero is the bar for anything on by default, and
        // this feature is not on by default precisely because it cannot clear it. Failing the build
        // over the known cost of an opt-in nobody enabled would make the gate noise; failing when
        // that cost *grows* is what keeps it meaningful.
        let over = gold_lost > MAX_TEMPLATE_LOSS;
        eprintln!(
            "{}   the site template removed {gold_lost} of {gold_words} labelled content words the \
             extractor had reached, on {} site(s). Measured cost {MAX_TEMPLATE_LOSS}; the feature \
             is off by default because of it, and must reach 0 before it could ever be on.",
            if over { "FAIL" } else { "ok  " },
            lost_sites.len()
        );
        for (d, n) in lost_sites.iter().take(10) {
            eprintln!("       {d}: -{n} words");
        }
        if assert && over {
            failures += 1;
        }
    } else if armed > 0 {
        println!(
            "ok   the site template removed none of the {gold_words} labelled content words the \
             extractor had reached"
        );
    }

    if armed == 0 {
        println!(
            "\nNo site had {} sibling pages to learn from, so the cross-page half measured \
             nothing. Fetch a larger category, or lower template::MIN_PAGES only with a reason.",
            svipall_core::template::MIN_PAGES
        );
    }
    println!(
        "\nTECO's condition of use is that results obtained with it are published. These belong in \
         docs/extraction.md."
    );
    failures
}
