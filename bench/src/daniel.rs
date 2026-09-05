//! The language half of the extraction benchmark.
//!
//! Every widely used extraction corpus is English, and the SIGIR-23 gold standard is no exception.
//! That is not a small omission for svipall, which answers in whatever language the page happens to
//! be written in. DAnIEL (Lejeune et al., 2012), as used by "Multilingual Benchmarking of Main
//! Content Extractors" (SIGIR 2025), is 1,689 news pages in Greek, English, Polish, Russian and
//! Chinese, each with a human reference extraction.
//!
//! What that study found, on the same code that scores 0.86 on English:
//!
//! | | Greek | English | Polish | Russian | Chinese |
//! |---|---|---|---|---|---|
//! | Readability  | 0.962 | 0.862 | 0.862 | 0.840 | **0.672** |
//! | Boilerpipe   | 0.961 | 0.847 | 0.861 | 0.750 | **0.611** |
//! | Trafilatura  | 0.868 | 0.883 | 0.800 | 0.759 | **0.555** |
//!
//! and the share of pages scoring under 0.3 going from 16% in English to **95%** in Chinese. The
//! named cause is the features: character counts and comma counts are not language-neutral, and
//! Chinese has neither spaces nor Latin punctuation. svipall's own thin-text rule was corrected for
//! the same reason. This is where that correction is checked rather than asserted.

use std::collections::HashMap;
use std::path::Path;

/// The five languages, in the order the study prints them.
pub const LANGUAGES: &[&str] = &["Greek", "English", "Polish", "Russian", "Chinese"];

pub struct Page {
    pub language: String,
    pub html: String,
    pub gold: String,
}

/// Load the corpus. A page without a reference, or a reference without a page, is skipped.
pub fn load(root: &Path) -> Vec<Page> {
    let langs: HashMap<String, String> = std::fs::read_to_string(root.join("doc_lg.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let html_dir = root.join("html");
    let ref_dir = root.join("reference");
    let Ok(entries) = std::fs::read_dir(&html_dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    ids.sort();

    let mut pages = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(language) = langs.get(&id) else {
            continue;
        };
        // The pages are archived web documents in their original encodings; anything that is not
        // valid UTF-8 is read lossily rather than dropped, because dropping it would quietly
        // change which languages are represented.
        let (Some(html), Some(gold)) = (
            read_lossy(&html_dir.join(&id)),
            read_lossy(&ref_dir.join(&id)),
        ) else {
            continue;
        };
        if gold.trim().is_empty() {
            continue;
        }
        pages.push(Page {
            language: language.clone(),
            html,
            gold,
        });
    }
    pages
}

fn read_lossy(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Score the corpus by language.
pub fn run(root: &Path, assert: bool) -> usize {
    if !root.join("html").is_dir() {
        eprintln!(
            "no DAnIEL corpus at {}\n\n\
             1,689 news pages in five languages. Fetch it once with:\n\n    \
             scripts/fetch-daniel.sh [target-dir]\n\n\
             then point this command at it:\n\n    \
             cargo run -p svipall-bench --release -- extract --daniel <target-dir>",
            root.display()
        );
        return 1;
    }

    let pages = load(root);
    if pages.is_empty() {
        eprintln!("the DAnIEL corpus is present but no page could be read");
        return 1;
    }

    let mut by_lang: HashMap<&str, Vec<crate::extraction::Score>> = HashMap::new();
    let mut poor: HashMap<&str, usize> = HashMap::new();
    for p in &pages {
        let s = crate::extraction::rouge_lsum(&p.gold, &crate::extraction::ours(&p.html));
        let key = LANGUAGES
            .iter()
            .find(|l| **l == p.language)
            .copied()
            .unwrap_or("other");
        by_lang.entry(key).or_default().push(s);
        if s.f1 <= 0.3 {
            *poor.entry(key).or_insert(0) += 1;
        }
    }

    println!("\n== DAnIEL (ROUGE-LSum, mean over pages) ==\n");
    println!(
        "{:<10} {:>6} {:>8} {:>8} {:>8} {:>10}",
        "language", "pages", "P", "R", "F1", "F1 <= 0.3"
    );
    for l in LANGUAGES.iter().chain(std::iter::once(&"other")) {
        let Some(v) = by_lang.get(*l).filter(|v| !v.is_empty()) else {
            continue;
        };
        let n = v.len() as f64;
        println!(
            "{l:<10} {:>6} {:>8.3} {:>8.3} {:>8.3} {:>9.0}%",
            v.len(),
            v.iter().map(|s| s.precision).sum::<f64>() / n,
            v.iter().map(|s| s.recall).sum::<f64>() / n,
            v.iter().map(|s| s.f1).sum::<f64>() / n,
            100.0 * *poor.get(*l).unwrap_or(&0) as f64 / n
        );
    }
    println!(
        "\nThe last column is the share of pages the extractor essentially failed on. It is the\n\
         number the multilingual study leads with, because a mean of 0.6 can be an extractor that\n\
         is mediocre everywhere or one that is excellent on two thirds of the pages and useless on\n\
         the rest, and only the second is a bug."
    );

    if !assert {
        return 0;
    }
    // The worst language, not the average. An extractor that is excellent in English and useless
    // in Chinese averages respectably, and the average is what hides it — which is the whole
    // finding of the multilingual study this corpus comes from.
    let worst = by_lang
        .values()
        .filter(|v| !v.is_empty())
        .map(|v| v.iter().map(|s| s.f1).sum::<f64>() / v.len() as f64)
        .fold(f64::INFINITY, f64::min);
    if worst.is_finite() && worst < crate::extraction::floors::DANIEL_WORST_F1 {
        eprintln!(
            "FAIL the worst language scores {worst:.3}, below the floor of {:.3}",
            crate::extraction::floors::DANIEL_WORST_F1
        );
        return 1;
    }
    eprintln!(
        "ok   DAnIEL worst language {worst:.3} (floor {:.3})",
        crate::extraction::floors::DANIEL_WORST_F1
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_corpus_is_reported_rather_than_scored_as_zero() {
        let nowhere = std::env::temp_dir().join("svipall-no-daniel-corpus-here");
        assert_eq!(run(&nowhere, false), 1);
    }
}
