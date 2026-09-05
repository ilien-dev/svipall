//! Tool parameter structs for svipall MCP.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WebFetchParams {
    /// URL to fetch. Also `raw:<html>` for markup you already have (no request is made) and
    /// `file:///path/page.html` for a local file under a directory named in `local_roots`
    /// (default `~/.svipall/in`); both go through the same extraction as a fetched page.
    pub url: String,
    /// Mode: auto, http, browser, stealth, real, warm. Default auto.
    #[serde(default)]
    pub mode: Option<String>,
    /// Extraction: markdown, text, html. Default markdown.
    #[serde(default)]
    pub extraction: Option<String>,
    /// CSS selector to keep only those parts.
    #[serde(default)]
    pub css_selector: Option<String>,
    /// Only body content. Default true.
    #[serde(default)]
    pub main_content_only: Option<bool>,
    /// BM25 query filter for markdown.
    #[serde(default)]
    pub query: Option<String>,
    /// Timeout in ms for the whole ladder. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Proxy URL (overrides web_route).
    #[serde(default)]
    pub proxy: Option<String>,
    /// Profile name from web_login. Implies browser tiers.
    #[serde(default)]
    pub profile: Option<String>,
    /// Max tier allowed. Default from config (warm).
    #[serde(default)]
    pub max_tier: Option<String>,
    /// HTTP method for the http tier: GET (default), POST, PUT, DELETE, HEAD.
    #[serde(default)]
    pub method: Option<String>,
    /// Request body for POST/PUT (http tier only).
    #[serde(default)]
    pub body: Option<String>,
    /// Extra request headers (http tier only).
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Declarative CSS extraction. `{"base_selector": "div.product", "fields": [{"name": "title",
    /// "selector": "h2 a"}, {"name": "url", "selector": "a", "type": "attribute",
    /// "attribute": "href", "absolute": true}]}`. Field types: text (default), attribute, number,
    /// exists, list, html, markdown. Returns `extracted` instead of `content`, which is far cheaper
    /// than parsing markdown yourself. Give the schema a `name`: what its selectors find is
    /// remembered per domain, and a selector a redesign breaks is relocated by similarity and
    /// reported under `healed` with the selector to switch to.
    ///
    /// `schema: "auto"` on a listing you have no selectors for: the page's own repeated structure
    /// is read, the columns are named for what they hold (`title`, `url`, `price`, `date`, …), and
    /// the rows come back in `extracted` with the schema that produced them in `induced_schema` —
    /// keep that and pass it next time. A page with no clear record set returns neither, on
    /// purpose: a guessed row is worse than no row.
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    /// Cap the returned content, cutting on block boundaries. Default 25000.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// Continue a truncated response: pass the `cursor` from the previous result.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Include page metadata: canonical, language, author, dates, OpenGraph, JSON-LD, feeds.
    #[serde(default)]
    pub include_metadata: Option<bool>,
    /// Include outbound links split into internal/external, plus images.
    #[serde(default)]
    pub include_links: Option<bool>,
    /// Take off what the rest of this site puts on every page, once enough of it has been seen.
    ///
    /// ▲ **Off by default, and that is a measurement rather than caution.** Scored on TECO — the
    /// only corpus that ships each page's sibling pages — a template learned from sixteen siblings
    /// saved 3.4% of the delivered text on the pages it fired on and removed **one word of
    /// human-labelled main content** that the extractor had reached. One is too many for something
    /// that is on by default, so it is not. The record is still learned on every fetch, so turning
    /// this on works immediately rather than after sixteen more pages.
    ///
    /// A response it changed says so: `"template": {"learned_from": 16, "removed_blocks": 3}`.
    #[serde(default)]
    pub use_site_template: Option<bool>,
    /// Everything svipall measured about the page, under `quality_detail`: the full integrity
    /// verdict with its reasons, the optimisation level with the traits behind it, the structural
    /// signals those were read from, the substance label, what a near-duplicate lookup over the
    /// cache found, and the provenance observations — byline, publication date, outbound citations
    /// and when this machine first saw the site. Where there is enough history, each score also
    /// carries its percentile among the pages this machine has fetched, with the width of that
    /// claim. Off by default: the compact fields on every response are unchanged, and this is for
    /// a caller deciding whether to trust a source rather than one reading it.
    #[serde(default)]
    pub include_quality: Option<bool>,
    /// robots.txt policy: warn (default â the URL you named is fetched, and the answer says
    /// whether robots.txt disallows it), obey (refuse it), ignore (say nothing).
    #[serde(default)]
    pub robots: Option<String>,
    /// Skip images, fonts, stylesheets and video. On an image-heavy page that is most of the bytes,
    /// and none of it becomes text. Off by default: a page whose images fail to load renders
    /// differently, and some anti-bot scripts notice.
    #[serde(default)]
    pub text_only: Option<bool>,
    /// Ask for the mobile version of the page. Mobile layouts carry less navigation and fewer
    /// widgets, so they are usually a good deal smaller — sometimes half the tokens for the same
    /// article. Uses a phone user agent and viewport.
    #[serde(default)]
    pub mobile: Option<bool>,
    /// Write the content to this file instead of returning it. The path costs about twenty tokens;
    /// the content it replaces can cost forty thousand. Relative paths land in ~/.svipall/out/.
    /// With `schema` or `tables`, a `.csv`, `.json` or `.jsonl` name writes the rows in that format.
    #[serde(default)]
    pub out_file: Option<String>,
    /// Return every data table on the page as typed rows (`tables: [{caption, header, rows}]`)
    /// instead of prose. A 200-row table costs a fraction of its markdown and keeps its columns.
    /// Layout tables (navigation grids) are skipped.
    #[serde(default)]
    pub tables: Option<bool>,
    /// Scroll a page that loads as you go before reading it: `"auto"` scrolls until the document
    /// stops growing (up to 40 screens, one "load more" click allowed), a number caps the rounds.
    /// Implies a browser tier. The result reports `scrolled` rounds.
    #[serde(default)]
    pub scroll: Option<String>,
    /// Use a profile that exists only for this fetch and is deleted afterwards. Nothing is carried
    /// in from a previous visit and nothing is left behind — no cookies, no storage, no history.
    #[serde(default)]
    pub isolated: Option<bool>,
    /// Cache behaviour: auto (default), read, write, bypass, refresh. `auto` serves fresh copies
    /// and revalidates stale ones with If-None-Match, which costs a 304 instead of a page.
    #[serde(default)]
    pub cache: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebFetchManyParams {
    pub urls: Vec<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub extraction: Option<String>,
    #[serde(default)]
    pub max_tier: Option<String>,
    /// BM25 query filter applied to every page.
    #[serde(default)]
    pub query: Option<String>,
    /// Timeout in ms per URL. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct WebCrawlParams {
    /// Start URL. Crawl stays on this domain.
    pub url: String,
    /// Maximum pages to fetch. Default 20, max 200.
    #[serde(default)]
    pub max_pages: Option<usize>,
    /// Link depth from the start URL. Default 2.
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Only follow URLs containing this substring (e.g. "/docs/").
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub extraction: Option<String>,
    /// BM25 query filter applied to every page.
    #[serde(default)]
    pub query: Option<String>,
    /// Per-page content cap in chars. Default 8000.
    #[serde(default)]
    pub max_chars_per_page: Option<usize>,
    /// Timeout in ms per page. Default 45000.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// robots.txt policy: obey (default for crawling), warn, ignore.
    #[serde(default)]
    pub robots: Option<String>,
    /// Skip near-duplicate pages, reporting what they duplicate instead of their content.
    /// Default true.
    #[serde(default)]
    pub dedup: Option<bool>,
    /// Output shape: pages (default), llms.txt, llms-full.txt.
    #[serde(default)]
    pub output: Option<String>,
    /// More sites to crawl alongside `url`, under the same page and token budget. Each domain gets
    /// an equal share of the pages, so one large site cannot spend the whole run.
    #[serde(default)]
    pub also: Option<Vec<String>>,
    /// Seed from the site's sitemap and fetch only what its `lastmod` says has changed since this
    /// machine last read it. A page with no date is always fetched: silence is not "unchanged".
    #[serde(default)]
    pub since_last_crawl: Option<bool>,
    /// Write the pages to this file instead of returning them, as CSV, JSON or JSON Lines
    /// depending on the extension. What comes back is a path and a count.
    #[serde(default)]
    pub out_file: Option<String>,
    /// Scroll every page until it stops growing before reading it (`"auto"` or a round count),
    /// for sites whose listings load as you scroll. Implies browser tiers; slower.
    #[serde(default)]
    pub scroll: Option<String>,
    /// Ordering: best_first (default when a query is given), bfs, or dfs to follow one branch to
    /// its end before starting the next — what a manual or a paginated listing wants.
    #[serde(default)]
    pub strategy: Option<String>,
    /// Stop once the crawl stops learning anything new about `query`. Default true when a query
    /// is given.
    #[serde(default)]
    pub stop_when_saturated: Option<bool>,
    /// Whole-crawl token cap. Default 60000.
    #[serde(default)]
    pub max_tokens_total: Option<usize>,
    /// Give up after this long regardless. Default 120000.
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
    /// Resume the crawl with this id instead of starting a new one: the queue, the pages already
    /// fetched and the original parameters all come back. Every crawl returns its `crawl_id`, and
    /// `web_status` lists the ones that still have work left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crawl_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebCaptureParams {
    pub url: String,
    /// Only responses whose URL contains this, e.g. "/api/". Leave it out to see everything the
    /// page asked for, which is the way to find the endpoint in the first place.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Also fetch the response bodies, not just the URLs. Off by default because bodies are large;
    /// turn it on once you know which endpoint you want.
    #[serde(default)]
    pub bodies: Option<bool>,
    /// Per-body character cap. Default 20000.
    #[serde(default)]
    pub max_body: Option<usize>,
    /// How long to keep watching after the page loads, in ms. Default 3000: enough for the calls a
    /// page makes on arrival, without waiting for polling traffic.
    #[serde(default)]
    pub settle_ms: Option<u64>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSnapshotParams {
    pub url: String,
    /// Only nodes matching this text or role, with far fewer tokens than the whole tree. Use it
    /// when you already know what you are looking for.
    #[serde(default)]
    pub find: Option<String>,
    /// How deep into the page to look. Lower is cheaper; 3 or 4 is usually enough to reach the
    /// controls that matter.
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Cap on nodes returned. Default 200.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebActParams {
    pub url: String,
    /// Actions: {do: click|type|fill|press|scroll|wait|eval|goto|hover|select|hold, selector, ref, text, key, pixels, ms, script, url, value}. `{"do":"scroll","until":"stable"}` scrolls until the page stops loading more (`rounds` caps it). `ref` takes a reference from web_snapshot, e.g. "e12", and is the reliable way to name an element: no guessing selectors from prose.
    pub actions: Vec<serde_json::Value>,
    #[serde(default)]
    pub extraction: Option<String>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    /// Timeout in ms for the whole interaction. Default 90000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebScreenshotParams {
    pub url: String,
    /// Capture the whole scrollable page. Default false (viewport only).
    #[serde(default)]
    pub full_page: Option<bool>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    /// Return the PNG inline as image content too (default true, skipped above 3 MB).
    #[serde(default)]
    pub inline: Option<bool>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSearchParams {
    pub query: String,
    /// Engine: auto (default, first that answers), all (ask every engine and merge by agreement),
    /// or one of ddg, ddg-html, bing, brave.
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserOpenParams {
    /// Profile name from web_login to reuse its cookies. Default: fresh session profile.
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    /// Show the browser window. Default false (offscreen).
    #[serde(default)]
    pub visible: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserDoParams {
    #[serde(rename = "session_id")]
    pub session_id: String,
    /// Navigate here first (optional — omit to keep acting on the current page).
    #[serde(default)]
    pub url: Option<String>,
    /// Same action objects as web_act.
    #[serde(default)]
    pub actions: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub extraction: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserSessionParams {
    #[serde(rename = "session_id")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebLoginParams {
    /// Page to open in a visible browser window.
    pub url: String,
    /// Profile name to save cookies under. Default: the domain's auto profile (used by real/warm tiers automatically).
    #[serde(default)]
    pub profile: Option<String>,
    /// Seconds to wait for you to finish (close the window to finish early). Default 300.
    #[serde(default)]
    pub timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveImageParams {
    /// Base64 image or URL.
    pub image: String,
    /// Optional hint: is base64?
    #[serde(default)]
    pub is_base64: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveRecaptchaV2Params {
    pub sitekey: String,
    #[serde(rename = "pageUrl")]
    pub page_url: String,
    #[serde(default)]
    pub invisible: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveTurnstileParams {
    pub sitekey: String,
    #[serde(rename = "pageUrl")]
    pub page_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveHCaptchaParams {
    pub sitekey: String,
    #[serde(rename = "pageUrl")]
    pub page_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptchaStatusParams {
    #[serde(rename = "taskId")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportCaptchaParams {
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// true = the solution worked, false = it was rejected.
    pub good: bool,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebRouteParams {
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    /// ISO country the proxy exits from, e.g. "DE". Without it the browser keeps announcing the
    /// timezone and languages of *this* machine while the traffic leaves from somewhere else, and
    /// comparing those two is a one-line check any site can run. Declared, not detected: working
    /// it out would mean calling a geolocation service.
    #[serde(default)]
    pub country: Option<String>,
    /// Several exits for the domain instead of one. The domain keeps using the first one that
    /// works (`exit_strategy = "sticky"`) and moves to the next when the domain blocks it twice.
    /// Subdomains inherit the pool.
    #[serde(default)]
    pub proxies: Option<Vec<String>>,
    /// ISO country of each entry in `proxies`, by position. `country` applies to any without one.
    #[serde(default)]
    pub countries: Option<Vec<String>>,
    #[serde(default)]
    pub remove: Option<bool>,
    /// Check the exits instead of changing them. Fetches `check_url` (or a plain page) through each
    /// proxy configured for `domain`, reporting whether it answered, how long it took, and any
    /// DNS-leak or scheme problem. No geolocation is done: the country stays what you declared.
    #[serde(default)]
    pub check: Option<bool>,
    /// The URL a `check` fetches through each exit. Defaults to a lightweight, neutral endpoint.
    #[serde(default)]
    pub check_url: Option<String>,
}

/// `Default` is what makes a read-only `GET /v1/status` possible: all four fields below *mutate*,
/// so the REST layer needs a way to ask for the report and nothing else.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebStatusParams {
    /// Domain whose cooldown should be cleared.
    #[serde(default)]
    pub clear_cooldown: Option<String>,
    /// Domain whose learned tier should be forgotten.
    #[serde(default)]
    pub forget_tier: Option<String>,
    /// Domain whose reputation spend should be forgotten, for every exit that spent on it.
    #[serde(default)]
    pub clear_budget: Option<String>,
    /// Empty the page cache: `true` for everything, or a domain name for just that site.
    #[serde(default)]
    pub clear_cache: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct NoParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserSetupParams {
    /// status (default) | install | update | remove.
    #[serde(default)]
    pub action: Option<String>,
    /// chrome (default, supports the headful real/warm tiers) | chrome-headless-shell (smaller,
    /// but cannot run headful and is more detectable).
    #[serde(default)]
    pub artifact: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveAndContinueParams {
    /// The blocked page. The challenge is solved on this very page, not a copy of it.
    pub url: String,
    /// Profile whose cookies to reuse and update. Defaults to the domain's automatic profile.
    #[serde(default)]
    pub profile: Option<String>,
    /// Extraction for the unblocked page: markdown (default), text, html.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Seconds to wait for the challenge to clear. Default 120.
    #[serde(default)]
    pub timeout_s: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebDiffParams {
    /// URL to compare against its previously cached copy.
    pub url: String,
    /// Fetch a fresh copy first. Default true; false compares stored versions only.
    #[serde(default)]
    pub refetch: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebMapParams {
    /// Any URL on the site; the whole origin is mapped.
    pub url: String,
    /// Sources to use: robots, sitemap, feeds, links. Default: all of them.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    /// Maximum URLs to return. Default 1000.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only URLs containing this substring.
    #[serde(default)]
    pub include: Option<String>,
}

/// A note kept across sessions.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebNotesParams {
    /// get (default), set, list or delete.
    #[serde(default)]
    pub action: Option<String>,
    /// The note's name. A path-like key ("shop/last_id") groups notes for `list`.
    #[serde(default)]
    pub key: Option<String>,
    /// What to remember. Required for `set`; a string, so pass JSON if it has structure.
    #[serde(default)]
    pub value: Option<String>,
    /// For `list`: only notes whose key starts with this. Empty lists everything.
    #[serde(default)]
    pub prefix: Option<String>,
}

/// A question about what this installation has been doing.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebLogParams {
    /// recent (default) or summary.
    #[serde(default)]
    pub view: Option<String>,
    /// Only this domain.
    #[serde(default)]
    pub domain: Option<String>,
    /// How far back to look, in seconds. Default 3600.
    #[serde(default)]
    pub since_secs: Option<i64>,
    /// Maximum lines for `recent`. Default 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Ask a site's own search box.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSiteSearchParams {
    /// Any page of the site — usually the home page. Its search box is what gets used.
    pub url: String,
    /// What to search for.
    pub query: String,
    /// Fetch the results too, rather than only reporting the pattern. Default true.
    #[serde(default)]
    pub fetch: Option<bool>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

/// A page to check again later.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebWatchParams {
    /// add (default), list, remove or check.
    #[serde(default)]
    pub action: Option<String>,
    /// The page. Required for add, remove and a single check.
    #[serde(default)]
    pub url: Option<String>,
    /// How often it is worth looking, in seconds. Default 3600, floor 60.
    #[serde(default)]
    pub interval_secs: Option<i64>,
    /// A name to recognise it by in the list.
    #[serde(default)]
    pub label: Option<String>,
    /// Watch only the part of the page this CSS selector finds. Changes elsewhere are ignored,
    /// and if a redesign breaks the selector the region is relocated by fingerprint.
    #[serde(default)]
    pub css_selector: Option<String>,
}

/// Moving a logged-in profile between machines.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebProfileParams {
    /// list (default), export or import.
    #[serde(default)]
    pub action: Option<String>,
    /// The profile name, as used by web_login and `profile` on web_fetch.
    #[serde(default)]
    pub name: Option<String>,
    /// Where the archive goes, or comes from. Relative paths land in ~/.svipall/out/.
    #[serde(default)]
    pub file: Option<String>,
    /// Required for export and import. The archive is the session; there is no unencrypted form.
    #[serde(default)]
    pub password: Option<String>,
}
