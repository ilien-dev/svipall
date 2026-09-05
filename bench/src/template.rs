//! Does knowing the rest of a site help, and at what price?
//!
//! ▲ The gate on the one feature in this project that can *remove* text a caller would otherwise
//! have had. Everything else svipall does to a page is a label. `template::Template` strips blocks
//! the rest of the site repeats, and the only honest way to ship that is to measure what it takes
//! away as carefully as what it saves.
//!
//! ## This is not the corpus for it — `teco.rs` is
//!
//! TECO ships each key page with its sibling pages and is where this feature was actually priced;
//! see `bench/src/teco.rs`. What follows is the attempt made first, against a corpus that turned
//! out to have nothing to say, and it is kept because the negative is worth as much as the number.
//!
//! ## What is measured instead, and what it found
//!
//! WCXB carries 2,008 pages from 1,613 domains, and a minority of those domains contribute several
//! pages. That looked like a small multi-page corpus hiding inside a single-page one. For each
//! domain with enough pages: learn a template from `k` of them using the same shipping extraction a
//! fetch would produce, strip the rest with it, and score required-snippet recall and boilerplate
//! leak before and after.
//!
//! ▲ **Run, and WCXB cannot measure this.** 108 of its 1,283 domains contribute
//! more than one page, no domain contributes more than eight, and at every threshold tried the
//! template removed **zero blocks from zero pages**:
//!
//! ```text
//! learn n   sites   pages  touched  saved%
//! 3            12      46        0     0.0
//! 5             7      25        0     0.0
//! 8             3       9        0     0.0
//! 16            0       0        0     0.0   <- the shipping threshold
//! ```
//!
//! Which is the corpus behaving exactly as it was built to. WCXB samples *different kinds of page*
//! across the web; the handful of same-domain pairs in it were chosen for variety, and after
//! page-level pruning they share no block verbatim. The content-loss gate below therefore passes
//! trivially — it is comparing a page with itself — and **passing it is not evidence of anything**.
//!
//! The gate stays wired up regardless. It costs nothing, and the question it asks is the correct
//! one: not "did F1 rise" but **did any required snippet that survived extraction get taken away by
//! the template**. One is too many. TECO answered it — one word — which is why the feature ships
//! off by default.

use crate::wcxb::{contains_snippet, Page};
use std::collections::BTreeMap;
use svipall_core::template::Template;

/// Arming thresholds swept. `MIN_PAGES` is in this list on purpose: the constant that ships has to
/// be one of the numbers that was measured.
const SWEEP: &[u32] = &[3, 5, 8, 16];

/// What one arming threshold did.
#[derive(Debug, Default, Clone, Copy)]
pub struct Outcome {
    /// Domains with enough pages to learn from at this threshold.
    pub sites: usize,
    /// Pages the template was applied to.
    pub pages: usize,
    /// Pages where it removed at least one block.
    pub touched: usize,
    /// Characters removed, over characters delivered before the strip.
    pub saved: f64,
    /// Required snippets present before the strip.
    pub kept_before: usize,
    /// …and after. **Any drop here is a failure**, whatever it bought.
    pub kept_after: usize,
    /// Boilerplate snippets present before the strip, and after.
    pub leaked_before: usize,
    pub leaked_after: usize,
    pub wanted: usize,
    pub checked: usize,
}

impl Outcome {
    fn cells(&self) -> String {
        let pct = |a: usize, b: usize| {
            if b == 0 {
                "  --".to_string()
            } else {
                format!("{:>4.0}", 100.0 * a as f64 / b as f64)
            }
        };
        format!(
            "{:>6} {:>7} {:>8} {:>7.1} {:>8} {:>8} {:>8} {:>8}",
            self.sites,
            self.pages,
            self.touched,
            100.0 * self.saved,
            pct(self.kept_before, self.wanted),
            pct(self.kept_after, self.wanted),
            pct(self.leaked_before, self.checked),
            pct(self.leaked_after, self.checked),
        )
    }

    /// Required content the template took away. The number that decides whether this ships.
    pub fn content_lost(&self) -> usize {
        self.kept_before.saturating_sub(self.kept_after)
    }
}

/// The shipping extraction: what a `web_fetch` of this page would return before any template.
fn shipping(html: &str) -> String {
    svipall_core::extraction::extract_markdown_opts(
        html,
        &svipall_core::extraction::ExtractOpts {
            main_content_only: true,
            ..Default::default()
        },
    )
}

/// Score the cross-page template against the multi-page domains of a corpus already on disk.
///
/// Returns the number of failures, for `--assert`.
pub fn run(pages: &[Page], assert: bool) -> usize {
    // One extraction per page, reused at every threshold.
    let extracted: Vec<String> = pages.iter().map(|p| shipping(&p.html)).collect();
    let mut by_site: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in pages.iter().enumerate() {
        if !p.domain.is_empty() {
            by_site.entry(p.domain.clone()).or_default().push(i);
        }
    }
    let multi = by_site.values().filter(|v| v.len() > 1).count();
    println!("\n== Cross-page template (WCXB's multi-page domains) ==\n");
    if multi == 0 {
        println!(
            "no domain in this corpus contributes more than one page, so there is nothing to \
             learn from.\nTECO is the corpus for this and is not wired up: see the module doc of \
             bench/src/template.rs."
        );
        return 0;
    }
    println!(
        "{} of {} domains contribute more than one page.\n",
        multi,
        by_site.len()
    );
    println!(
        "{:<8} {:>6} {:>7} {:>8} {:>7} {:>8} {:>8} {:>8} {:>8}",
        "learn n", "sites", "pages", "touched", "saved%", "kept%", "kept'%", "leak%", "leak'%"
    );

    let mut failures = 0usize;
    for &min in SWEEP {
        let mut out = Outcome::default();
        for idxs in by_site.values() {
            if idxs.len() as u32 <= min {
                continue;
            }
            out.sites += 1;
            let mut t = Template::default();
            for &i in &idxs[..min as usize] {
                t.observe(&extracted[i]);
            }
            for &i in &idxs[min as usize..] {
                let before = &extracted[i];
                let (after, applied) = t.strip(before);
                out.pages += 1;
                if applied.removed_blocks > 0 {
                    out.touched += 1;
                }
                out.saved +=
                    before.len().saturating_sub(after.len()) as f64 / before.len().max(1) as f64;
                for s in &pages[i].with {
                    out.wanted += 1;
                    out.kept_before += usize::from(contains_snippet(before, s));
                    out.kept_after += usize::from(contains_snippet(&after, s));
                }
                for s in &pages[i].without {
                    out.checked += 1;
                    out.leaked_before += usize::from(contains_snippet(before, s));
                    out.leaked_after += usize::from(contains_snippet(&after, s));
                }
            }
        }
        if out.pages > 0 {
            out.saved /= out.pages as f64;
        }
        let mark = if min == svipall_core::template::MIN_PAGES {
            " <- shipping"
        } else {
            ""
        };
        println!("{:<8} {}{}", min, out.cells(), mark);

        // ▲ The gate, and it is absolute. Cross-page stripping is the first thing here able to
        // remove text a caller would otherwise have had, and the price of being wrong is a
        // sentence that held the answer. A saving is not a defence.
        if assert && min == svipall_core::template::MIN_PAGES && out.content_lost() > 0 {
            eprintln!(
                "FAIL: the site template removed {} required snippet(s) that survived extraction",
                out.content_lost()
            );
            failures += 1;
        }
    }
    println!(
        "\nkept%/kept'% are required snippets before and after the strip; leak%/leak'% are \
         boilerplate ones.\nThe gate is kept' = kept at the shipping threshold: a saving does not \
         buy a lost sentence."
    );
    failures
}
