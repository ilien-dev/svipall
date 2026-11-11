//! Search without an API: read public engines' own result pages (the lite and html DuckDuckGo
//! endpoints, Bing, Brave, and Bing's news feed).
//!
//! An engine that answers is not an engine that answered the question. Every response is
//! classified (results, genuinely empty, a challenge, a block, a page we could not read), the
//! verdict is remembered per engine host and exit so a failing engine is left alone for a while,
//! and a search that ends with nothing says which engines refused and what to try.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use svipall_core::store::JsonMap;
use svipall_http::{HttpFetcher, HttpRequest};

#[derive(Debug, Clone, Default, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Shares no word with the query. Flagged and ranked last, never dropped.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub off_topic: bool,
}

/// What one engine's answer turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Results,
    /// The engine said it has nothing: a working engine and an honest zero.
    Empty,
    /// A captcha, an anomaly or a proof-of-work page instead of results.
    Challenge,
    /// Refused by status: 403, 429 and the like.
    Blocked,
    /// No results and no "no results" message either: a layout we cannot read, or a wall we do
    /// not recognise. Counted as a failure, because it is one for the caller.
    Unparsed,
    /// The request never got an answer: connection, proxy, timeout.
    Error,
    /// Not asked, because it failed recently.
    Skipped,
}

impl Outcome {
    fn failed(self) -> bool {
        matches!(
            self,
            Outcome::Challenge | Outcome::Blocked | Outcome::Unparsed | Outcome::Error
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub engine: String,
    pub outcome: Outcome,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub ms: u64,
    /// The query this attempt asked, when it was not the caller's own (the shortened retry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Locale {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// `params`, `query` (inferred from its language) or `none` (each engine's own default).
    pub from: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SearchOutcome {
    pub engine: String,
    pub results: Vec<SearchResult>,
    pub attempts: Vec<Attempt>,
    pub locale: Locale,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retried_with: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Web engines, in the order they are asked when none has a record yet.
pub const ENGINES: &[&str] = &["ddg", "ddg-html", "bing", "brave"];
/// Recent articles from a news feed. Asked in `auto` only after every web engine, because for a
/// question that is not about the news its results are beside the point.
pub const NEWS: &str = "news";

/// Where each engine's result page lives. The real hosts by default; a test points an engine at a
/// local fixture server instead, which is the only way to replay a real anomaly page on demand.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    overrides: std::collections::HashMap<String, String>,
}

impl Endpoints {
    /// Serve `engine` from `base` (scheme, host and path, without the query string).
    pub fn with(mut self, engine: &str, base: &str) -> Self {
        self.overrides.insert(engine.to_string(), base.to_string());
        self
    }

    pub fn base(&self, engine: &str) -> Option<String> {
        if let Some(b) = self.overrides.get(engine) {
            return Some(b.clone());
        }
        Some(
            match engine {
                "ddg" => "https://lite.duckduckgo.com/lite/",
                "ddg-html" => "https://html.duckduckgo.com/html/",
                "bing" => "https://www.bing.com/search",
                "brave" => "https://search.brave.com/search",
                NEWS => "https://www.bing.com/news/search",
                _ => return None,
            }
            .to_string(),
        )
    }
}

/// What the server lends a search: where the engines are, which exit each host leaves through,
/// and the least gap between two requests to one host.
/// A domain's fetcher and the exit it leaves through.
pub type FetcherFor<'a> = dyn Fn(&str) -> (Arc<dyn HttpFetcher>, Option<String>) + Send + Sync + 'a;

pub struct Env<'a> {
    pub endpoints: &'a Endpoints,
    /// The fetcher for a domain and the exit it leaves through (`None` is this machine).
    pub fetcher_for: &'a FetcherFor<'a>,
    pub min_gap: Duration,
}

pub struct Request<'a> {
    pub query: &'a str,
    pub limit: usize,
    /// `None`/`auto`, `all`/`merge`, or one engine key.
    pub engine: Option<&'a str>,
    pub lang: Option<&'a str>,
    pub region: Option<&'a str>,
}

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

/// One query parameter of a URL, still encoded.
fn param<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let (_, query) = url.split_once('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// Bing's click redirect, `/ck/a?…&u=a1<base64url of the target>&…`. A link that does not decode
/// to an http(s) URL is kept as it came: a wrapped link still works, a guessed one may not.
pub fn unwrap_redirect(href: &str) -> String {
    use base64::Engine as _;
    if href.contains("/ck/a?") {
        if let Some(enc) = param(href, "u").and_then(|u| u.strip_prefix("a1")) {
            let enc = enc.trim_end_matches('=');
            if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(enc) {
                if let Ok(s) = String::from_utf8(bytes) {
                    if s.starts_with("http://") || s.starts_with("https://") {
                        return s;
                    }
                }
            }
        }
    }
    // The news feed's click counter carries the target percent-encoded in `url=`.
    if href.contains("apiclick.aspx") {
        if let Some(u) = param(href, "url").and_then(|u| urlencoding::decode(u).ok()) {
            if u.starts_with("http://") || u.starts_with("https://") {
                return u.into_owned();
            }
        }
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
                ..Default::default()
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
                ..Default::default()
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
            let url = unwrap_redirect(a.value().attr("href")?);
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
                ..Default::default()
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
            ..Default::default()
        })
    })
    .collect()
}

/// The text of the first `<tag>…</tag>` in `xml`, CDATA unwrapped and entities decoded.
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let start = xml.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = start + xml[start..].find(&format!("</{tag}>"))?;
    let raw = xml[start..end].trim();
    let raw = raw
        .strip_prefix("<![CDATA[")
        .and_then(|r| r.strip_suffix("]]>"))
        .unwrap_or(raw);
    let decoded = raw
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    // A description may carry markup of its own.
    let text = Html::parse_fragment(&decoded)
        .root_element()
        .text()
        .collect::<String>();
    Some(clean(&text))
}

/// An RSS feed's items. Not through the HTML parser: there `<link>` is a void element, and the
/// URL inside it would be thrown away.
pub fn parse_rss(xml: &str) -> Vec<SearchResult> {
    xml.split("<item>")
        .skip(1)
        .filter_map(|item| {
            let item = item.split("</item>").next()?;
            let url = unwrap_redirect(&xml_text(item, "link")?);
            Some(SearchResult {
                title: xml_text(item, "title").unwrap_or_default(),
                url,
                snippet: xml_text(item, "description").unwrap_or_default(),
                ..Default::default()
            })
        })
        .collect()
}

pub fn parse(engine: &str, html: &str) -> Vec<SearchResult> {
    match engine {
        "ddg" => parse_ddg_lite(html),
        "ddg-html" => parse_ddg_html(html),
        "bing" => parse_bing(html),
        "brave" => parse_brave(html),
        NEWS => parse_rss(html),
        _ => Vec::new(),
    }
}

/// Pages that are a test of the visitor rather than an answer. Each was seen from this client:
/// the duck-captcha anomaly page, the "flagged as suspicious" captcha, a JavaScript challenge, a
/// proof-of-work interstitial, a managed-challenge waiting room.
const CHALLENGE_MARKERS: &[&str] = &[
    "bots use duckduckgo too",
    "anomaly-modal",
    "flagged as being suspicious",
    "javascript is required to complete this challenge",
    "verifying your request",
    "<title>just a moment",
    "unusual traffic from your computer",
    "/sorry/index",
];

/// An engine saying in words that it found nothing.
const EMPTY_MARKERS: &[&str] = &[
    "no results found for",
    "no-results",
    "no results.",
    "there are no results for",
    "did not match any documents",
    "no se encontraron resultados",
];

/// The page without the bodies of its `<script>` and `<style>` elements, lowercased. A result
/// page's script config carries "There are no results for this question" whether or not it has
/// results, so markers are read from what the page shows, not from its code.
fn visible_lower(body: &str) -> String {
    let lower = body.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut rest = lower.as_str();
    loop {
        let next = ["<script", "<style"]
            .iter()
            .filter_map(|tag| rest.find(tag).map(|i| (i, *tag)))
            .min_by_key(|(i, _)| *i);
        let Some((i, tag)) = next else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..i]);
        let close = format!("</{}", &tag[1..]);
        match rest[i..].find(&close) {
            Some(j) => rest = &rest[i + j + close.len()..],
            None => return out,
        }
    }
}

/// What a response was, from its status, its body and what the parser found in it.
pub fn classify(engine: &str, status: u16, body: &str, parsed: usize) -> (Outcome, Option<String>) {
    // Results are results: a snippet about captchas does not make a result page a wall.
    if parsed > 0 && status < 400 {
        return (Outcome::Results, None);
    }
    let lower = visible_lower(body);
    if let Some(m) = CHALLENGE_MARKERS.iter().find(|m| lower.contains(*m)) {
        return (
            Outcome::Challenge,
            Some(format!(
                "http {status}, challenge page ({})",
                m.trim_start_matches('<')
            )),
        );
    }
    if status == 429 {
        return (Outcome::Blocked, Some("http 429, rate limited".into()));
    }
    if status >= 400 {
        return (Outcome::Blocked, Some(format!("http {status}")));
    }
    if status == 202 {
        return (
            Outcome::Challenge,
            Some("http 202 with no results: an anomaly page".into()),
        );
    }
    if EMPTY_MARKERS.iter().any(|m| lower.contains(m))
        || (engine == NEWS && lower.contains("<channel"))
    {
        return (Outcome::Empty, None);
    }
    (
        Outcome::Unparsed,
        Some(format!(
            "http {status}, {} bytes, no results and no 'no results' message: a wall or a changed layout",
            body.len()
        )),
    )
}

// ---- locale -------------------------------------------------------------------------------------

/// Words that give a query's language away, and nothing else of the language: a list of every
/// word would be a dictionary, and these are enough to tell a question in Spanish from one in
/// English.
const LANG_HINTS: &[(&str, &[&str])] = &[
    (
        "es",
        &[
            "el", "la", "los", "las", "del", "que", "y", "para", "por", "con", "como", "una",
            "sin", "donde", "cual", "cuando", "mejor", "hacer", "precio", "es", "en", "de", "un",
        ],
    ),
    (
        "pt",
        &[
            "o", "os", "as", "do", "da", "dos", "das", "nao", "com", "uma", "em", "voce", "fazer",
            "de", "que", "para", "como",
        ],
    ),
    (
        "fr",
        &[
            "le", "les", "des", "du", "et", "est", "pour", "avec", "une", "dans", "comment",
            "quel", "faire", "de", "un",
        ],
    ),
    (
        "de",
        &[
            "der", "die", "das", "und", "ist", "mit", "fur", "nicht", "ein", "eine", "wie", "was",
            "machen",
        ],
    ),
    (
        "en",
        &[
            "the", "and", "how", "what", "for", "with", "is", "of", "to", "in", "best", "why",
            "does", "vs", "my", "can", "a",
        ],
    ),
];

/// The country a language is searched from when the caller named none.
fn default_region(lang: &str) -> &'static str {
    match lang {
        "es" => "es",
        "pt" => "br",
        "fr" => "fr",
        "de" => "de",
        "it" => "it",
        _ => "us",
    }
}

fn region_lang(region: &str) -> &'static str {
    match region {
        "es" | "mx" | "ar" | "co" | "cl" | "pe" | "ve" | "ec" | "uy" | "cr" | "gt" | "bo" => "es",
        "br" | "pt" => "pt",
        "fr" | "be" => "fr",
        "de" | "at" | "ch" => "de",
        "it" => "it",
        _ => "en",
    }
}

/// Fold case and the accents a person may or may not type: `Cómo` and `como` are one word.
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn words(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The query's language, when it says. `None` for a query of names and terms, which is most
/// technical queries: guessing there would only narrow the results.
pub fn infer_lang(query: &str) -> Option<&'static str> {
    let lower = query.to_lowercase();
    let toks = words(query);
    let mut scores: Vec<(&'static str, usize)> = LANG_HINTS
        .iter()
        .map(|(lang, hints)| {
            let mut n = toks.iter().filter(|t| hints.contains(&t.as_str())).count() * 2;
            n += match *lang {
                "es" => {
                    lower.chars().filter(|c| "ñ¿¡".contains(*c)).count() * 3
                        + lower.chars().filter(|c| "áíóú".contains(*c)).count()
                }
                "pt" => lower.chars().filter(|c| "ãõç".contains(*c)).count() * 3,
                "de" => lower.chars().filter(|c| "ßäöü".contains(*c)).count() * 3,
                "fr" => lower.chars().filter(|c| "èêàç".contains(*c)).count() * 2,
                _ => 0,
            };
            (*lang, n)
        })
        .collect();
    scores.sort_by_key(|s| std::cmp::Reverse(s.1));
    match scores.as_slice() {
        [(lang, best), (_, second), ..] if *best >= 2 && best > second => Some(lang),
        _ => None,
    }
}

pub fn locale(query: &str, lang: Option<&str>, region: Option<&str>) -> Locale {
    let norm = |s: &str| s.trim().to_lowercase().replace('_', "-");
    let mut lang = lang.map(norm).filter(|l| !l.is_empty());
    let mut region = region.map(norm).filter(|r| !r.is_empty());
    // `es-MX` as a language is a language and a country.
    if let Some((l, r)) = lang.clone().as_deref().and_then(|l| l.split_once('-')) {
        lang = Some(l.to_string());
        region = region.or_else(|| Some(r.to_string()));
    }
    if lang.is_some() || region.is_some() {
        return Locale {
            lang,
            region: region.map(|r| r.to_uppercase()),
            from: "params",
        };
    }
    match infer_lang(query) {
        Some(l) => Locale {
            lang: Some(l.to_string()),
            region: None,
            from: "query",
        },
        None => Locale {
            from: "none",
            ..Default::default()
        },
    }
}

/// DuckDuckGo's `kl`: one value naming country and language, `wt-wt` for neither.
fn ddg_kl(loc: &Locale) -> String {
    let region = loc.region.as_deref().map(str::to_lowercase);
    match (region.as_deref(), loc.lang.as_deref()) {
        (Some(r), Some(l)) => format!("{r}-{l}"),
        (Some(r), None) => format!("{r}-{}", region_lang(r)),
        (None, Some(l)) => format!("{}-{l}", default_region(l)),
        (None, None) => "wt-wt".into(),
    }
}

fn bing_locale(loc: &Locale) -> String {
    let mut s = String::new();
    if let Some(l) = &loc.lang {
        s.push_str(&format!("&setlang={}", urlencoding::encode(l)));
    }
    if let Some(r) = &loc.region {
        s.push_str(&format!("&cc={}", urlencoding::encode(r)));
    }
    s
}

pub fn engine_url(
    endpoints: &Endpoints,
    engine: &str,
    query: &str,
    loc: &Locale,
) -> Option<String> {
    let base = endpoints.base(engine)?;
    // Spaces as `+`, the way a search box's form submits them. Measured on the lite endpoint:
    // `q=rust%20async%20runtime&kl=wt-wt` drew its anomaly page every time, and the same query
    // with `+` (or with `%20` and no `kl`) a results page every time.
    let q = urlencoding::encode(query).replace("%20", "+");
    Some(match engine {
        // A GET, as the link a person follows would be. The form POST this used to send carried
        // a navigation's headers with no origin or referrer, and the lite endpoint answered it
        // with its anomaly page every time.
        "ddg" | "ddg-html" => format!("{base}?q={q}&kl={}", ddg_kl(loc)),
        "bing" => format!("{base}?q={q}{}", bing_locale(loc)),
        "brave" => format!("{base}?q={q}&source=web"),
        NEWS => format!("{base}?q={q}&format=rss{}", bing_locale(loc)),
        _ => return None,
    })
}

// ---- relevance ----------------------------------------------------------------------------------

/// Words no result is relevant for sharing.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "with", "is", "are", "be", "how",
    "what", "why", "when", "where", "which", "who", "do", "does", "my", "me", "you", "your", "it",
    "this", "that", "from", "by", "at", "as", "can", "vs", "about", "el", "la", "los", "las", "de",
    "del", "que", "y", "en", "para", "por", "con", "como", "un", "una", "es", "sin", "se", "su",
    "al", "lo", "mas", "o", "donde", "cual", "cuando", "mi", "tu", "os", "do", "da", "em", "com",
    "le", "les", "des", "du", "et", "est", "pour", "avec", "une", "dans", "der", "die", "das",
    "und", "ist", "mit", "fur", "ein", "eine", "wie", "was",
];

/// The words of a query a relevant result would share.
pub fn terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words(query) {
        let keep = (w.len() >= 2 || w.chars().all(|c| c.is_ascii_digit()))
            && !STOPWORDS.contains(&w.as_str());
        if keep && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// On topic: shares a word with a one- or two-word query, two words with a longer one. One word is
/// not enough there, and that is measured: for a six-word question one engine answered from this
/// machine with dictionary entries for its first verb, every one of them sharing exactly that word.
fn on_topic(r: &SearchResult, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let url = urlencoding::decode(&r.url)
        .map(|u| u.into_owned())
        .unwrap_or_else(|_| r.url.clone());
    let hay = fold(&format!("{} {} {}", r.title, r.snippet, url));
    let need = if terms.len() <= 2 { 1 } else { 2 };
    terms.iter().filter(|t| hay.contains(t.as_str())).count() >= need
}

/// Mark what shares nothing with the query and move it after what does, keeping each group's order.
fn flag(results: Vec<SearchResult>, terms: &[String]) -> Vec<SearchResult> {
    let (mut on, mut off): (Vec<_>, Vec<_>) = results
        .into_iter()
        .map(|mut r| {
            r.off_topic = !on_topic(&r, terms);
            r
        })
        .partition(|r| !r.off_topic);
    on.append(&mut off);
    on
}

fn relevant(results: &[SearchResult]) -> usize {
    results.iter().filter(|r| !r.off_topic).count()
}

/// The query cut down to what carries it: quoted phrases whole, then the first content words.
/// `None` when that is the query already.
pub fn shorten(query: &str) -> Option<String> {
    let mut phrases = Vec::new();
    let mut rest = String::new();
    for (i, part) in query.split('"').enumerate() {
        if i % 2 == 1 && !part.trim().is_empty() {
            phrases.push(format!("\"{}\"", part.trim()));
        } else {
            rest.push(' ');
            rest.push_str(part);
        }
    }
    let content: Vec<&str> = rest
        .split_whitespace()
        .filter(|w| {
            let f = words(w);
            !f.is_empty() && !f.iter().all(|x| STOPWORDS.contains(&x.as_str()))
        })
        .collect();
    let room = if phrases.is_empty() { 4 } else { 2 };
    let mut out = phrases;
    out.extend(content.iter().take(room).map(|s| s.to_string()));
    let short = out.join(" ");
    let original = query.split_whitespace().count();
    (!short.is_empty() && short.split_whitespace().count() < original).then_some(short)
}

// ---- health -------------------------------------------------------------------------------------

/// How an engine has been doing from one exit, persisted so the next process knows too.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    /// Recent success, 0..1. Unseen engines start at the middle.
    pub score: f64,
    /// Consecutive failures.
    pub streak: u32,
    /// Unix seconds before which the engine is not asked.
    pub until: u64,
    pub last: String,
    pub at: u64,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            score: 0.5,
            streak: 0,
            until: 0,
            last: String::new(),
            at: 0,
        }
    }
}

static HEALTH: LazyLock<JsonMap> =
    LazyLock::new(|| JsonMap::new(svipall_core::config::home_dir().join("search_engines.json")));

/// The longest an engine is left alone, however often it failed.
const MAX_SKIP: u64 = 6 * 3600;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Per engine, host and exit: the same engine through a proxy is a different reputation. The exit
/// is hashed so a proxy's credentials are not written out again.
fn health_key(engine: &str, host: &str, exit: Option<&str>) -> String {
    let exit = exit
        .map(|e| format!("{:x}", md5::compute(e.as_bytes()))[..8].to_string())
        .unwrap_or_else(|| "home".into());
    format!("{engine} {host} {exit}")
}

fn health(key: &str) -> Health {
    HEALTH
        .get(key)
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// `useful` is whether the results were mostly about the query: an engine that answers every
/// question with dictionary entries for one of its words is answering, and is not working.
fn record(key: &str, outcome: Outcome, useful: bool, retry_after: Option<u64>) {
    let mut h = health(key);
    let t = now();
    match outcome {
        Outcome::Results => {
            h.score = if useful {
                h.score * 0.7 + 0.3
            } else {
                h.score * 0.9
            };
            h.streak = 0;
            h.until = 0;
        }
        Outcome::Empty => {
            h.streak = 0;
            h.until = 0;
        }
        Outcome::Skipped => return,
        failed => {
            h.streak += 1;
            // Doubling from a base that fits the failure. A refusal by status is the engine saying
            // so and starts at ten minutes. A transport error is more often ours than theirs and
            // starts at one. A challenge costs nothing the first time: the lite endpoint's anomaly
            // page was measured to follow the query (a famous pangram drew it every time, `tokio`
            // asked right after did not), so one challenge says little about the next search; two
            // in a row start at two minutes.
            let (base, strikes): (u64, u32) = match failed {
                Outcome::Blocked => (600, h.streak),
                Outcome::Unparsed => (300, h.streak),
                Outcome::Challenge => (120, h.streak.saturating_sub(1)),
                _ => (60, h.streak),
            };
            h.score *= if failed == Outcome::Challenge {
                0.8
            } else {
                0.6
            };
            let skip = if strikes == 0 {
                0
            } else {
                base.saturating_mul(1 << (strikes - 1).min(10))
                    .min(MAX_SKIP)
            };
            h.until = t + skip.max(retry_after.unwrap_or(0).min(MAX_SKIP));
        }
    }
    h.last = serde_json::to_value(outcome)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    h.at = t;
    if let Ok(v) = serde_json::to_string(&h) {
        HEALTH.insert(key, &v);
    }
}

// ---- asking -------------------------------------------------------------------------------------

/// Host, plus the port when there is one: the key the pacer and the health records use.
fn host_of(url: &str) -> (String, String) {
    let domain = svipall_core::domain_from_url(url);
    let host = match url::Url::parse(url).ok().and_then(|u| u.port()) {
        Some(p) => format!("{domain}:{p}"),
        None => domain.clone(),
    };
    (domain, host)
}

struct Planned {
    engine: &'static str,
    key: String,
    host: String,
    exit: Option<String>,
    fetcher: Arc<dyn HttpFetcher>,
    url: String,
    health: Health,
}

fn plan(env: &Env<'_>, engine: &'static str, query: &str, loc: &Locale) -> Option<Planned> {
    let url = engine_url(env.endpoints, engine, query, loc)?;
    let (domain, host) = host_of(&url);
    let (fetcher, exit) = (env.fetcher_for)(&domain);
    let key = health_key(engine, &host, exit.as_deref());
    let mut health = health(&key);
    // A host the fetch path put on cooldown is resting for search too.
    let cooling = svipall_core::throttle::cooldown_left(&domain);
    if cooling > 0 {
        health.until = health.until.max(now() + cooling);
        health.last = "on cooldown".into();
    }
    Some(Planned {
        engine,
        health,
        key,
        host,
        exit,
        fetcher,
        url,
    })
}

fn skipped(p: &Planned) -> Attempt {
    let left = p.health.until.saturating_sub(now());
    Attempt {
        engine: p.engine.to_string(),
        outcome: Outcome::Skipped,
        count: 0,
        status: None,
        detail: Some(if p.health.last == "on cooldown" {
            format!("host on cooldown; asked again in {left}s")
        } else {
            format!("{} on its last try; asked again in {left}s", p.health.last)
        }),
        ms: 0,
        query: None,
    }
}

async fn ask(env: &Env<'_>, p: &Planned, terms: &[String]) -> (Attempt, Vec<SearchResult>) {
    let exit = p.exit.as_deref();
    svipall_core::throttle::throttle_at_least(&p.host, exit, "http", env.min_gap).await;
    let mut req = HttpRequest::get(p.url.clone());
    req.headers = p.fetcher.identity().nav_headers();
    let t0 = Instant::now();
    let sent = p.fetcher.send(req).await;
    let ms = t0.elapsed().as_millis() as u64;
    let mut attempt = Attempt {
        engine: p.engine.to_string(),
        outcome: Outcome::Error,
        count: 0,
        status: None,
        detail: None,
        ms,
        query: None,
    };
    let resp = match sent {
        Ok(r) => r,
        Err(e) => {
            attempt.detail = Some(format!("{e:#}"));
            record(&p.key, Outcome::Error, false, None);
            return (attempt, Vec::new());
        }
    };
    let body = resp.text();
    let results = flag(parse(p.engine, &body), terms);
    let (outcome, detail) = classify(p.engine, resp.status, &body, results.len());
    let useful = relevant(&results) * 2 >= results.len();
    let retry_after = resp
        .header("retry-after")
        .and_then(svipall_core::throttle::parse_retry_after);
    use svipall_core::throttle::{observe, Outcome as Pace};
    // A 429 is a strike, never a `RateLimited` hold: the hold is slept through by the next request
    // to the host, and a search must not sit out a minute inside a call. The engine rests on its
    // own record instead, which is reported, not slept.
    let pace = match outcome {
        o if o.failed() || resp.status == 429 => Pace::Blocked,
        _ => Pace::Ok {
            latency: t0.elapsed(),
        },
    };
    observe(&p.host, exit, "http", pace);
    // A captcha served with 429 or 403 is reported as the challenge it is, and rests the engine
    // like the refusal by status it also is: that one answered 429 on 148 of 148 real attempts.
    let rests_as = if outcome == Outcome::Challenge && resp.status >= 400 {
        Outcome::Blocked
    } else {
        outcome
    };
    record(&p.key, rests_as, useful, retry_after.map(|d| d.as_secs()));
    attempt.outcome = outcome;
    attempt.count = results.len();
    attempt.status = Some(resp.status);
    attempt.detail = detail;
    (attempt, results)
}

/// The order to ask web engines in: by recent success, the static order breaking ties; engines in
/// their skip window are reported, not asked — unless every one is, in which case the one whose
/// window ends first is asked anyway, so a search never ends without asking anybody.
fn order(mut plans: Vec<Planned>) -> (Vec<Planned>, Vec<Planned>) {
    let t = now();
    let (mut ready, mut resting): (Vec<_>, Vec<_>) =
        plans.drain(..).partition(|p| p.health.until <= t);
    ready.sort_by(|a, b| {
        b.health
            .score
            .partial_cmp(&a.health.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if ready.is_empty() && !resting.is_empty() {
        resting.sort_by_key(|p| p.health.until);
        ready.push(resting.remove(0));
    }
    (ready, resting)
}

struct Round {
    engine: String,
    results: Vec<SearchResult>,
    attempts: Vec<Attempt>,
}

async fn round(env: &Env<'_>, req: &Request<'_>, query: &str, loc: &Locale) -> Round {
    let mode = req.engine.unwrap_or("auto");
    let terms = terms(query);
    let mut attempts = Vec::new();

    if !matches!(mode, "auto" | "all" | "merge") {
        // Named: asked whatever its record says, and the answer still goes on the record.
        let known = ENGINES
            .iter()
            .chain(std::iter::once(&NEWS))
            .find(|e| **e == mode);
        let Some(p) = known.and_then(|e| plan(env, e, query, loc)) else {
            attempts.push(Attempt {
                engine: mode.to_string(),
                outcome: Outcome::Error,
                count: 0,
                status: None,
                detail: Some(format!(
                    "unknown engine; use auto, all, {} or {NEWS}",
                    ENGINES.join(", ")
                )),
                ms: 0,
                query: None,
            });
            return Round {
                engine: "none".into(),
                results: Vec::new(),
                attempts,
            };
        };
        let (a, results) = ask(env, &p, &terms).await;
        attempts.push(a);
        let results = flag(results, &terms);
        let engine = if results.is_empty() { "none" } else { p.engine };
        return Round {
            engine: engine.into(),
            results,
            attempts,
        };
    }

    let plans: Vec<Planned> = ENGINES
        .iter()
        .filter_map(|e| plan(env, e, query, loc))
        .collect();
    let (ready, resting) = order(plans);

    if matches!(mode, "all" | "merge") {
        // Sequential rather than parallel: four simultaneous searches from one address is a shape
        // none of these hosts see from a person.
        let mut per_engine: Vec<(String, Vec<SearchResult>)> = Vec::new();
        for p in &ready {
            let (a, results) = ask(env, p, &terms).await;
            attempts.push(a);
            if !results.is_empty() {
                per_engine.push((p.engine.to_string(), results));
            }
        }
        attempts.extend(resting.iter().map(skipped));
        let engine = if per_engine.is_empty() {
            "none".to_string()
        } else {
            per_engine
                .iter()
                .map(|(e, _)| e.as_str())
                .collect::<Vec<_>>()
                .join("+")
        };
        let merged = merge(&per_engine, usize::MAX);
        return Round {
            engine,
            results: flag(merged, &terms),
            attempts,
        };
    }

    // auto: the first engine whose results are mostly about the query wins; failing that, the
    // engine with the most that are.
    let mut best: Option<(String, Vec<SearchResult>)> = None;
    for p in &ready {
        let (a, results) = ask(env, p, &terms).await;
        attempts.push(a);
        if results.is_empty() {
            continue;
        }
        let results = flag(results, &terms);
        let good = relevant(&results);
        let enough = good * 2 >= results.len();
        if best.as_ref().is_none_or(|(_, b)| good > relevant(b)) {
            best = Some((p.engine.to_string(), results));
        }
        if enough {
            break;
        }
    }
    attempts.extend(resting.iter().map(skipped));
    let settled = best
        .as_ref()
        .is_some_and(|(_, b)| relevant(b) * 2 >= b.len());
    if !settled {
        // Last: the news feed, unless it is resting.
        if let Some(p) = plan(env, NEWS, query, loc) {
            if p.health.until > now() {
                attempts.push(skipped(&p));
            } else {
                let (a, results) = ask(env, &p, &terms).await;
                attempts.push(a);
                let results = flag(results, &terms);
                if !results.is_empty()
                    && best
                        .as_ref()
                        .is_none_or(|(_, b)| relevant(&results) > relevant(b))
                {
                    best = Some((NEWS.to_string(), results));
                }
            }
        }
    }
    match best {
        Some((engine, results)) => Round {
            engine,
            results,
            attempts,
        },
        None => Round {
            engine: "none".into(),
            results: Vec::new(),
            attempts,
        },
    }
}

/// Did anybody answer at all? A retry cannot help a search every engine refused.
fn someone_answered(attempts: &[Attempt]) -> bool {
    attempts
        .iter()
        .any(|a| matches!(a.outcome, Outcome::Results | Outcome::Empty))
}

pub async fn run(env: &Env<'_>, req: &Request<'_>) -> SearchOutcome {
    let loc = locale(req.query, req.lang, req.region);
    let mut r = round(env, req, req.query, &loc).await;
    let mut retried_with = None;

    let weak = r.results.is_empty() || relevant(&r.results) * 2 < r.results.len();
    if weak && someone_answered(&r.attempts) {
        if let Some(short) = shorten(req.query) {
            let mut again = round(env, req, &short, &loc).await;
            for a in &mut again.attempts {
                a.query = Some(short.clone());
            }
            // Relevance is judged against the caller's words, not the shortened ones.
            let original_terms = terms(req.query);
            again.results = flag(again.results, &original_terms);
            if relevant(&again.results) > relevant(&r.results) {
                let mut attempts = std::mem::take(&mut r.attempts);
                attempts.append(&mut again.attempts);
                r = Round {
                    engine: again.engine,
                    results: again.results,
                    attempts,
                };
            } else {
                r.attempts.append(&mut again.attempts);
            }
            retried_with = Some(short);
        }
    }
    r.results.truncate(req.limit);
    let note = note(&r.results, &r.attempts, retried_with.as_deref());
    SearchOutcome {
        engine: r.engine,
        results: r.results,
        attempts: r.attempts,
        locale: loc,
        retried_with,
        note,
    }
}

fn named(attempts: &[Attempt], pick: impl Fn(Outcome) -> bool) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for a in attempts.iter().filter(|a| pick(a.outcome)) {
        let label = match &a.detail {
            Some(d) => format!("{} ({d})", a.engine),
            None => a.engine.clone(),
        };
        if !seen
            .iter()
            .any(|s| s.starts_with(&format!("{} ", a.engine)) || *s == a.engine)
        {
            seen.push(label);
        }
    }
    seen
}

/// What a caller needs when the answer is nothing, or mostly beside the point.
fn note(results: &[SearchResult], attempts: &[Attempt], retried: Option<&str>) -> Option<String> {
    if results.is_empty() {
        let mut parts = Vec::new();
        let refused = named(attempts, |o| {
            matches!(o, Outcome::Blocked | Outcome::Challenge | Outcome::Unparsed)
        });
        if !refused.is_empty() {
            parts.push(format!("blocked or challenged: {}", refused.join(", ")));
        }
        let empty = named(attempts, |o| o == Outcome::Empty);
        if !empty.is_empty() {
            parts.push(format!("genuinely empty: {}", empty.join(", ")));
        }
        let errors = named(attempts, |o| o == Outcome::Error);
        if !errors.is_empty() {
            parts.push(format!("no answer: {}", errors.join(", ")));
        }
        let resting = named(attempts, |o| o == Outcome::Skipped);
        if !resting.is_empty() {
            parts.push(format!(
                "skipped after failing recently: {}",
                resting.join(", ")
            ));
        }
        let mut s = format!("No results; {}.", parts.join("; "));
        if let Some(q) = retried {
            s.push_str(&format!(" A shortened query (\"{q}\") was tried too."));
        }
        s.push_str(
            " Try: other or fewer words; lang/region; engine=news for recent events; a named \
             engine that was not blocked; web_route a proxy for a blocked engine's host; or \
             web_fetch the engine's result page, which climbs to a browser.",
        );
        return Some(s);
    }
    let off = results.iter().filter(|r| r.off_topic).count();
    if off * 2 > results.len() {
        let mut s = format!(
            "{off} of {} results share no word with the query (marked off topic, ranked last).",
            results.len()
        );
        if let Some(q) = retried {
            s.push_str(&format!(" A shortened query (\"{q}\") did no better."));
        }
        s.push_str(" Try a shorter or more specific query, or lang/region.");
        return Some(s);
    }
    None
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
/// web badly. Taking the first that answers means the answer depends on which engine happened to
/// be up.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn r(url: &str, title: &str, snippet: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: title.into(),
            snippet: snippet.into(),
            ..Default::default()
        }
    }

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/search")
            .join(name);
        std::fs::read_to_string(path).unwrap()
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

    #[test]
    fn a_click_redirect_unwraps_and_a_broken_one_is_kept() {
        let wrapped = "https://www.bing.com/ck/a?!&&p=abc&u=a1aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw&ntb=1";
        assert_eq!(unwrap_redirect(wrapped), "https://rust-lang.org/");
        let broken = "https://www.bing.com/ck/a?!&&p=abc&u=a1%%%&ntb=1";
        assert_eq!(unwrap_redirect(broken), broken);
        let news = "http://www.bing.com/news/apiclick.aspx?ref=FexRss&url=https%3a%2f%2fgithub.com%2fx&c=1";
        assert_eq!(unwrap_redirect(news), "https://github.com/x");
    }

    #[test]
    fn every_captured_page_classifies_as_what_it_was() {
        for (file, engine, status, want) in [
            ("ddg-lite.results.html", "ddg", 200, Outcome::Results),
            ("ddg-lite.empty.html", "ddg", 200, Outcome::Empty),
            ("ddg-lite.anomaly.html", "ddg", 202, Outcome::Challenge),
            ("ddg-html.results.html", "ddg-html", 200, Outcome::Results),
            ("ddg-html.anomaly.html", "ddg-html", 202, Outcome::Challenge),
            ("bing.results.html", "bing", 200, Outcome::Results),
            ("bing.es.html", "bing", 200, Outcome::Results),
            ("brave.captcha.html", "brave", 429, Outcome::Challenge),
            ("news.results.xml", NEWS, 200, Outcome::Results),
            ("news.empty.xml", NEWS, 200, Outcome::Empty),
        ] {
            let body = fixture(file);
            let n = parse(engine, &body).len();
            assert_eq!(
                classify(engine, status, &body, n).0,
                want,
                "{file}: {n} parsed"
            );
        }
    }

    #[test]
    fn words_inside_a_script_are_not_the_page_saying_it_found_nothing() {
        // A live results page carries "There are no results for this question" in its script
        // config whether or not it has results; a page we could not parse must not read as empty.
        let body = r#"<html><head><script>var c={"m":"There are no results for this question"};</script>
            <style>.no-results{}</style></head><body><div id="layout-we-do-not-know"></div></body></html>"#;
        assert_eq!(classify("bing", 200, body, 0).0, Outcome::Unparsed);
        let said = "<html><body><p>There are no results for <b>x</b></p></body></html>";
        assert_eq!(classify("bing", 200, said, 0).0, Outcome::Empty);
    }

    #[test]
    fn the_query_language_is_inferred_only_when_it_shows() {
        assert_eq!(infer_lang("cómo hacer pan casero sin levadura"), Some("es"));
        assert_eq!(infer_lang("mejor precio de un portátil"), Some("es"));
        assert_eq!(infer_lang("how to pin a tokio runtime"), Some("en"));
        assert_eq!(infer_lang("tokio runtime"), None);
        assert_eq!(infer_lang("rust async runtime"), None);
    }

    #[test]
    fn locale_params_win_and_a_tagged_language_carries_its_country() {
        let l = locale("anything", Some("es-MX"), None);
        assert_eq!(
            (l.lang.as_deref(), l.region.as_deref()),
            (Some("es"), Some("MX"))
        );
        assert_eq!(ddg_kl(&l), "mx-es");
        assert_eq!(ddg_kl(&locale("tokio runtime", None, None)), "wt-wt");
        assert_eq!(ddg_kl(&locale("cómo hacer pan", None, None)), "es-es");
    }

    #[test]
    fn a_long_query_shortens_to_its_first_content_words_and_keeps_its_phrases() {
        assert_eq!(
            shorten("rust async runtime comparison for embedded hobby projects").as_deref(),
            Some("rust async runtime comparison")
        );
        assert_eq!(
            shorten("\"exact phrase here\" with some other words added").as_deref(),
            Some("\"exact phrase here\" some other")
        );
        assert_eq!(shorten("tokio runtime"), None);
    }

    #[test]
    fn off_topic_results_are_flagged_and_ranked_last_never_dropped() {
        let t = terms("pan casero");
        let out = flag(
            vec![
                r("https://a.test/", "Facebook", "log in"),
                r("https://b.test/pan", "Receta de Pán", ""),
            ],
            &t,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "https://b.test/pan");
        assert!(out[1].off_topic);
    }
}
