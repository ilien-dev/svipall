//! Search without an API: scrape DuckDuckGo (lite + html), Bing and Brave, first engine
//! that yields results wins.

use scraper::{Html, Selector};
use serde::Serialize;
use svipall_http::{HttpFetcher, HttpRequest};

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct SearchOutcome {
    pub engine: String,
    pub results: Vec<SearchResult>,
    pub attempts: Vec<String>,
}

pub const ENGINES: &[&str] = &["ddg", "ddg-html", "bing", "brave"];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// DuckDuckGo wraps outbound links as `//duckduckgo.com/l/?uddg=<encoded>&rut=…`.
fn unwrap_ddg(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let rest = &href[pos + 5..];
        let enc = rest.split('&').next().unwrap_or(rest);
        if let Ok(d) = urlencoding::decode(enc) {
            return d.into_owned();
        }
    }
    if let Some(stripped) = href.strip_prefix("//") {
        return format!("https://{}", stripped);
    }
    href.to_string()
}

fn is_ad(url: &str) -> bool {
    // Engine-internal links (ad redirects, "ads by Microsoft" help page) are never results.
    url.contains("duckduckgo.com/y.js")
        || url.contains("duckduckgo.com/duckduckgo-help-pages")
        || url.contains("bing.com/aclick")
        || url.contains("ad_domain=")
        || url.starts_with("https://duckduckgo.com/?")
}

pub fn parse_ddg_lite(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let links: Vec<_> = doc.select(&sel("a.result-link")).collect();
    let snippets: Vec<String> = doc
        .select(&sel("td.result-snippet"))
        .map(|s| clean(&s.text().collect::<String>()))
        .collect();
    links
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let url = unwrap_ddg(a.value().attr("href")?);
            if is_ad(&url) {
                return None;
            }
            Some(SearchResult {
                title: clean(&a.text().collect::<String>()),
                url,
                snippet: snippets.get(i).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

pub fn parse_ddg_html(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let a_sel = sel("a.result__a");
    let s_sel = sel(".result__snippet");
    doc.select(&sel(".result"))
        .filter_map(|r| {
            let a = r.select(&a_sel).next()?;
            let url = unwrap_ddg(a.value().attr("href")?);
            if is_ad(&url) {
                return None;
            }
            let snippet = r
                .select(&s_sel)
                .next()
                .map(|s| clean(&s.text().collect::<String>()))
                .unwrap_or_default();
            Some(SearchResult {
                title: clean(&a.text().collect::<String>()),
                url,
                snippet,
            })
        })
        .collect()
}

pub fn parse_bing(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let a_sel = sel("h2 a");
    let p_sel = sel(".b_caption p, p");
    doc.select(&sel("li.b_algo"))
        .filter_map(|r| {
            let a = r.select(&a_sel).next()?;
            let url = a.value().attr("href")?.to_string();
            if is_ad(&url) {
                return None;
            }
            let snippet = r
                .select(&p_sel)
                .next()
                .map(|s| clean(&s.text().collect::<String>()))
                .unwrap_or_default();
            Some(SearchResult {
                title: clean(&a.text().collect::<String>()),
                url,
                snippet,
            })
        })
        .collect()
}

pub fn parse_brave(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let a_sel = sel("a[href]");
    let t_sel = sel(".title, .snippet-title, h2, h3");
    let d_sel = sel(".snippet-description, .snippet-content, .description");
    doc.select(&sel(
        "#results .snippet[data-type=\"web\"], #results .snippet",
    ))
    .filter_map(|r| {
        let a = r.select(&a_sel).find(|a| {
            a.value()
                .attr("href")
                .map(|h| h.starts_with("http"))
                .unwrap_or(false)
        })?;
        let url = a.value().attr("href")?.to_string();
        let title = r
            .select(&t_sel)
            .next()
            .map(|t| clean(&t.text().collect::<String>()))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| clean(&a.text().collect::<String>()));
        let snippet = r
            .select(&d_sel)
            .next()
            .map(|s| clean(&s.text().collect::<String>()))
            .unwrap_or_default();
        Some(SearchResult {
            title,
            url,
            snippet,
        })
    })
    .collect()
}

/// Search engines are exactly the sites that screen on TLS and HTTP/2 fingerprints, so these
/// requests go through the same emulating engine as everything else rather than a bare client.
async fn fetch_engine(
    fetcher: &dyn HttpFetcher,
    engine: &str,
    query: &str,
) -> Result<String, String> {
    let q = urlencoding::encode(query);
    let mut req = match engine {
        "ddg" => {
            let mut r = HttpRequest {
                url: "https://lite.duckduckgo.com/lite/".into(),
                method: "POST".into(),
                headers: fetcher.identity().nav_headers(),
                body: Some(format!("q={}&kl=us-en", urlencoding::encode(query)).into_bytes()),
            };
            r.set_header("content-type", "application/x-www-form-urlencoded");
            r
        }
        "ddg-html" => HttpRequest::get(format!(
            "https://html.duckduckgo.com/html/?q={}&kl=us-en",
            q
        )),
        "bing" => HttpRequest::get(format!(
            "https://www.bing.com/search?q={}&setlang=en&cc=US",
            q
        )),
        "brave" => HttpRequest::get(format!(
            "https://search.brave.com/search?q={}&source=web",
            q
        )),
        other => return Err(format!("unknown engine {}", other)),
    };
    if req.headers.is_empty() {
        req.headers = fetcher.identity().nav_headers();
    }
    let resp = fetcher.send(req).await.map_err(|e| e.to_string())?;
    if resp.status >= 400 {
        return Err(format!("http {}", resp.status));
    }
    Ok(resp.text())
}

pub fn parse(engine: &str, html: &str) -> Vec<SearchResult> {
    match engine {
        "ddg" => parse_ddg_lite(html),
        "ddg-html" => parse_ddg_html(html),
        "bing" => parse_bing(html),
        "brave" => parse_brave(html),
        _ => Vec::new(),
    }
}

pub async fn search(
    fetcher: &dyn HttpFetcher,
    query: &str,
    limit: usize,
    engine: Option<&str>,
) -> SearchOutcome {
    let order: Vec<&str> = match engine {
        Some(e) if e != "auto" => vec![e],
        _ => ENGINES.to_vec(),
    };
    let mut attempts = Vec::new();
    for eng in order {
        let t0 = std::time::Instant::now();
        match fetch_engine(fetcher, eng, query).await {
            Ok(html) => {
                let mut results = parse(eng, &html);
                let n = results.len();
                results.truncate(limit);
                attempts.push(format!(
                    "{}: {} results ({}ms)",
                    eng,
                    n,
                    t0.elapsed().as_millis()
                ));
                if !results.is_empty() {
                    return SearchOutcome {
                        engine: eng.to_string(),
                        results,
                        attempts,
                    };
                }
            }
            Err(e) => attempts.push(format!("{}: EXC {}", eng, e)),
        }
    }
    SearchOutcome {
        engine: "none".into(),
        results: Vec::new(),
        attempts,
    }
}

/// What counts as "the same result".
///
/// The shared normaliser drops fragments and tracking parameters, which is most of it. The extra
/// step here is the trailing slash: engines disagree about it constantly, and three spellings of
/// one page is three of the ten slots the caller asked for. Done locally rather than in the shared
/// normaliser because the crawler uses that one too, and there a wrong merge means a page never
/// fetched, while here it means one row fewer.
fn same_result(url: &str) -> String {
    let norm = svipall_core::domain::normalize_url(url).unwrap_or_else(|| url.to_string());
    match norm.split_once('?') {
        Some((base, query)) => format!("{}?{}", base.trim_end_matches('/'), query),
        None => norm.trim_end_matches('/').to_string(),
    }
}

/// Fold several engines' results into one ranking.
///
/// Every engine here is being read off its own HTML, and each one covers a different slice of the
/// web badly. Taking the first that answers — which is what `search` does — means the answer
/// depends on which engine happened to be up.
///
/// The rule is reciprocal rank fusion: a result's score is the sum of `1/(k + rank)` over the
/// engines that returned it. It needs no scores from the engines (they publish none), it is not
/// fooled by one engine returning fifty results and another returning five, and a page that two
/// engines both rank highly beats a page that one engine ranked first and the other never saw —
/// which is exactly the judgement a person makes when reading two result lists side by side.
pub fn merge(per_engine: &[(String, Vec<SearchResult>)], limit: usize) -> Vec<SearchResult> {
    // 60 is the constant the method is usually published with. Its only job is to stop the first
    // position from dominating the sum, and any value in that neighbourhood behaves the same.
    const K: f64 = 60.0;
    let mut scores: Vec<(f64, SearchResult, Vec<String>)> = Vec::new();
    for (engine, results) in per_engine {
        for (rank, r) in results.iter().enumerate() {
            let key = same_result(&r.url);
            let add = 1.0 / (K + rank as f64 + 1.0);
            match scores
                .iter_mut()
                .find(|(_, existing, _)| same_result(&existing.url) == key)
            {
                Some((score, existing, found_by)) => {
                    *score += add;
                    found_by.push(engine.clone());
                    // Keep the longest snippet: engines truncate differently and the longer one is
                    // the one with more of the page in it.
                    if r.snippet.len() > existing.snippet.len() {
                        existing.snippet = r.snippet.clone();
                    }
                }
                None => scores.push((add, r.clone(), vec![engine.clone()])),
            }
        }
    }
    scores.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Stable on ties so two identical runs agree.
            .then_with(|| a.1.url.cmp(&b.1.url))
    });
    scores.into_iter().take(limit).map(|(_, r, _)| r).collect()
}

/// Ask every engine and merge what comes back.
///
/// Slower than `search` by one round of requests and better by however much the engines disagree,
/// which on anything narrow is a lot.
pub async fn search_all(fetcher: &dyn HttpFetcher, query: &str, limit: usize) -> SearchOutcome {
    let mut attempts = Vec::new();
    let mut per_engine: Vec<(String, Vec<SearchResult>)> = Vec::new();
    // Sequential rather than parallel: these are the same four hosts every time, and four
    // simultaneous searches from one address is a shape none of them see from a person.
    for eng in ENGINES {
        let t0 = std::time::Instant::now();
        match fetch_engine(fetcher, eng, query).await {
            Ok(html) => {
                let results = parse(eng, &html);
                attempts.push(format!(
                    "{}: {} results ({}ms)",
                    eng,
                    results.len(),
                    t0.elapsed().as_millis()
                ));
                if !results.is_empty() {
                    per_engine.push((eng.to_string(), results));
                }
            }
            Err(e) => attempts.push(format!("{eng}: EXC {e}")),
        }
    }
    let engine = if per_engine.is_empty() {
        "none".to_string()
    } else {
        per_engine
            .iter()
            .map(|(e, _)| e.as_str())
            .collect::<Vec<_>>()
            .join("+")
    };
    SearchOutcome {
        engine,
        results: merge(&per_engine, limit),
        attempts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(url: &str, title: &str, snippet: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: title.into(),
            snippet: snippet.into(),
        }
    }

    #[test]
    fn a_page_two_engines_agree_on_beats_one_engine_s_favourite() {
        // The judgement a person makes reading two result lists side by side.
        let a = vec![
            r("https://only.test/", "Only", ""),
            r("https://both.test/", "Both", ""),
        ];
        let b = vec![
            r("https://other.test/", "Other", ""),
            r("https://both.test/", "Both", ""),
        ];
        let merged = merge(&[("a".into(), a), ("b".into(), b)], 10);
        assert_eq!(merged[0].url, "https://both.test/", "{merged:?}");
    }

    #[test]
    fn the_same_page_under_two_spellings_is_one_result() {
        // Engines disagree about trailing slashes and tracking parameters, and three copies of one
        // page is three of the ten slots the caller asked for.
        let a = vec![r("https://x.test/page", "P", "short")];
        let b = vec![r(
            "https://x.test/page/?utm_source=b",
            "P",
            "a longer snippet",
        )];
        let merged = merge(&[("a".into(), a), ("b".into(), b)], 10);
        assert_eq!(merged.len(), 1, "{merged:?}");
        assert_eq!(
            merged[0].snippet, "a longer snippet",
            "the fuller snippet survives"
        );
    }

    #[test]
    fn one_engine_returning_fifty_results_does_not_drown_out_one_returning_five() {
        // Score by position, never by count: otherwise the chattiest engine wins every time.
        let many: Vec<SearchResult> = (0..50)
            .map(|i| r(&format!("https://many.test/{i}"), "M", ""))
            .collect();
        let few = vec![r("https://few.test/top", "F", "")];
        let merged = merge(&[("many".into(), many), ("few".into(), few)], 5);
        assert!(
            merged.iter().any(|x| x.url == "https://few.test/top"),
            "{merged:?}"
        );
    }

    #[test]
    fn merging_nothing_is_no_results_rather_than_a_panic() {
        assert!(merge(&[], 10).is_empty());
        assert!(merge(&[("a".into(), vec![])], 10).is_empty());
    }

    #[test]
    fn the_merged_order_is_the_same_two_runs_running() {
        let a = vec![r("https://b.test/", "B", ""), r("https://a.test/", "A", "")];
        let input = [("e".to_string(), a)];
        let names = |v: Vec<SearchResult>| v.into_iter().map(|x| x.url).collect::<Vec<_>>();
        assert_eq!(names(merge(&input, 10)), names(merge(&input, 10)));
    }

    #[test]
    fn ddg_lite_rows() {
        let html = r#"<table><tr><td><a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2F&rut=abc" class='result-link'>Rust Programming Language</a></td></tr>
        <tr><td class='result-snippet'>A language empowering everyone.</td></tr>
        <tr><td><a href="https://duckduckgo.com/y.js?ad_domain=x" class='result-link'>Ad</a></td></tr></table>"#;
        let r = parse_ddg_lite(html);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url, "https://www.rust-lang.org/");
        assert_eq!(r[0].title, "Rust Programming Language");
        assert_eq!(r[0].snippet, "A language empowering everyone.");
    }

    #[test]
    fn bing_rows() {
        let html = r#"<ol><li class="b_algo"><h2><a href="https://doc.rust-lang.org/book/">The Book</a></h2><div class="b_caption"><p>Learn Rust.</p></div></li></ol>"#;
        let r = parse_bing(html);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url, "https://doc.rust-lang.org/book/");
        assert_eq!(r[0].snippet, "Learn Rust.");
    }
}
