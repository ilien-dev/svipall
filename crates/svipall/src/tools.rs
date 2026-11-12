//! Tool parameter structs for svipall MCP.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WebFetchParams {
    /// The page. Also `raw:<html>` (no request) or `file:///path` under `~/.svipall/in` or
    /// `local_roots`.
    pub url: String,
    /// Leave unset: auto learns the tier per domain. A forced tier is slower or weaker.
    #[serde(default)]
    pub mode: Option<String>,
    /// markdown (default), text, or html (the raw markup, many times the tokens).
    #[serde(default)]
    pub extraction: Option<String>,
    /// Keep only the elements this CSS selector matches, e.g. "article" or "#prices".
    #[serde(default)]
    pub css_selector: Option<String>,
    /// Drop navigation, footers and sidebars. Default true.
    #[serde(default)]
    pub main_content_only: Option<bool>,
    // Measured on two long articles: "robots.txt" and "caching headers" left 6% and 13% of the
    // page, "history of scraping" left 88%. How much it saves is decided by the query.
    /// Keep only the blocks relevant to these words, e.g. "shipping costs": the cheapest way to
    /// one fact. Name the fact, not the topic: a specific query left 6% of a long page, a broad
    /// one 88%.
    #[serde(default)]
    pub query: Option<String>,
    /// Timeout in ms for the whole ladder. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Proxy URL for this fetch only; web_route sets one per domain.
    #[serde(default)]
    pub proxy: Option<String>,
    /// Profile saved by web_login whose cookies to use. Implies a browser tier.
    #[serde(default)]
    pub profile: Option<String>,
    /// Highest tier to climb to: http, browser, stealth, real, warm (default).
    #[serde(default)]
    pub max_tier: Option<String>,
    /// GET (default), POST, PUT, DELETE, HEAD (http tier only).
    #[serde(default)]
    pub method: Option<String>,
    /// Request body for POST/PUT (http tier only).
    #[serde(default)]
    pub body: Option<String>,
    /// Extra request headers (http tier only).
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Rows instead of prose, as `extracted`. "auto", or e.g. `{"name": "products",
    /// "base_selector": "div.product", "fields": [{"name": "title", "selector": "h2 a"}, {"name":
    /// "url", "selector": "a", "type": "attribute", "attribute": "href"}]}`; types text, attribute,
    /// number, exists, list, html, markdown. "auto" returns the schema it used as
    /// `induced_schema`; a selector a redesign broke is relocated and reported as `healed`.
    #[serde(default)]
    #[schemars(with = "Option<SchemaSpec>")]
    pub schema: Option<serde_json::Value>,
    /// Cap on the content, cut on block boundaries. Default 25000; a cut result carries a `cursor`.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// The `cursor` of a truncated result, to continue it.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Add `metadata`: canonical URL, language, author, dates, OpenGraph, JSON-LD, feeds.
    #[serde(default)]
    pub include_metadata: Option<bool>,
    /// Add `links` (internal and external, absolute) and images.
    #[serde(default)]
    pub include_links: Option<bool>,
    // Off by default on a measurement, not caution: on TECO, the one corpus that ships sibling
    // pages, a template learned from sixteen siblings saved 3.4% of the delivered text and removed
    // one word of human-labelled main content. One is too many for a default. The record is still
    // learned on every fetch, so turning this on works at once.
    /// Strip what this site repeats on every page, learned from earlier fetches. Off by default:
    /// it can take real content with it.
    #[serde(default)]
    pub use_site_template: Option<bool>,
    /// Add `quality_detail`: verdict with reasons, near-duplicates, provenance.
    #[serde(default)]
    pub include_quality: Option<bool>,
    /// robots.txt: warn (default: fetch and say if disallowed), obey, ignore.
    #[serde(default)]
    pub robots: Option<String>,
    /// Skip images, fonts, stylesheets and video in browser tiers. Faster; some anti-bot scripts
    /// notice it.
    #[serde(default)]
    pub text_only: Option<bool>,
    // Measured byte-identical on two responsive sites at both the http and browser tiers; no warm
    // page is reused, and the native last resort is ruled out.
    /// Ask as a phone. Most sites send the same bytes, and it costs a browser page of its own.
    #[serde(default)]
    pub mobile: Option<bool>,
    // Measured: a 418-character response against the 34 746 characters of the page it wrote.
    /// Write to this file and return the path, not the content. A .md .txt .json .csv .html name
    /// gets the text (rows with schema/tables); any other name, or a binary body, gets the bytes
    /// as served, resumed by calling again. Relative paths land in ~/.svipall/out/.
    #[serde(default)]
    pub out_file: Option<String>,
    /// The page's data tables as typed rows, `{caption, header, rows}`, instead of prose.
    #[serde(default)]
    pub tables: Option<bool>,
    /// Scroll a page that loads as you go: "auto" until it stops growing, or a number of rounds.
    /// Implies a browser tier.
    #[serde(default)]
    pub scroll: Option<String>,
    /// A throwaway browser profile: no cookies in, none left behind.
    #[serde(default)]
    pub isolated: Option<bool>,
    /// auto (default: serve a fresh copy, revalidate a stale one), read, write, bypass, refresh.
    #[serde(default)]
    pub cache: Option<String>,
    /// Seconds to wait out a full visit window or a hold, up to 900. Unset: up to half the
    /// timeout; 0 never waits.
    #[serde(default)]
    pub wait: Option<u64>,
    /// Walk the ladder again though it failed on this site in the last 10 minutes.
    #[serde(default)]
    pub fresh: Option<bool>,
    /// Only these fields of a JSON answer, e.g. `items[:5].{name, owner.login}` (`.a`, `[0]`,
    /// `[1:5]`, `[*]`, `{a, b}`).
    #[serde(default)]
    pub json_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebFetchManyParams {
    /// The pages to fetch, in the order the results come back.
    pub urls: Vec<String>,
    /// Leave unset: auto learns the tier per domain.
    #[serde(default)]
    pub mode: Option<String>,
    /// markdown (default), text, or html.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Highest tier the ladder may climb to. Default warm.
    #[serde(default)]
    pub max_tier: Option<String>,
    /// Keep only the blocks relevant to these words, on every page.
    #[serde(default)]
    pub query: Option<String>,
    /// Rows instead of prose from every page, as on web_fetch.
    #[serde(default)]
    #[schemars(with = "Option<SchemaSpec>")]
    pub schema: Option<serde_json::Value>,
    /// Every page's data tables as typed rows, as on web_fetch.
    #[serde(default)]
    pub tables: Option<bool>,
    /// Timeout in ms per URL. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// As on web_fetch, per URL.
    #[serde(default)]
    pub wait: Option<u64>,
    /// Only these fields of every JSON answer, as on web_fetch.
    #[serde(default)]
    pub json_path: Option<String>,
    /// Cap per page. Default: an equal share of max_tokens_total.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// Cap on all pages together. Default 8000; a cut page carries a cursor for web_fetch.
    #[serde(default)]
    pub max_tokens_total: Option<usize>,
    /// Write each page whole to a file in this directory; return paths.
    #[serde(default)]
    pub out_dir: Option<String>,
    /// Write the whole set to this .json/.jsonl file; return a summary.
    #[serde(default)]
    pub out_file: Option<String>,
    // What `web_fetch` with `urls` asked of every page (`surface::translate`): any argument a
    // single fetch takes. Not in the schema, because nobody calls this operation with it by hand.
    #[serde(default)]
    #[schemars(skip)]
    pub each: Option<Box<WebFetchParams>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebProbeParams {
    /// The addresses, answered in this order.
    pub urls: Vec<String>,
    /// Timeout in ms per URL. Default 15000.
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
    /// Leave unset: auto learns the tier per domain.
    #[serde(default)]
    pub mode: Option<String>,
    /// markdown (default), text, or html.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Rank pages by relevance to these words, keep the relevant blocks, stop when new pages add
    /// nothing.
    #[serde(default)]
    pub query: Option<String>,
    /// Rows from every page, as on web_fetch; with `out_file`, one row per item with its `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<SchemaSpec>")]
    pub schema: Option<serde_json::Value>,
    /// Every page's data tables as typed rows, as on web_fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<bool>,
    /// Per-page content cap in chars. Default 8000.
    #[serde(default)]
    pub max_chars_per_page: Option<usize>,
    /// Timeout in ms per page. Default 45000.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// robots.txt policy: obey (default for crawling), warn, ignore.
    #[serde(default)]
    pub robots: Option<String>,
    /// Skip near-duplicate pages, naming what they duplicate. Default true.
    #[serde(default)]
    pub dedup: Option<bool>,
    /// Output shape: pages (default), llms.txt, llms-full.txt.
    #[serde(default)]
    pub output: Option<String>,
    /// More sites to crawl alongside `url`; each domain gets an equal share of the budget.
    #[serde(default)]
    pub also: Option<Vec<String>>,
    /// Fetch only what the sitemap's `lastmod` says changed since the last crawl; undated pages
    /// are always fetched.
    #[serde(default)]
    pub since_last_crawl: Option<bool>,
    /// Write the pages to this .csv, .json or .jsonl file; a path and a count come back.
    #[serde(default)]
    pub out_file: Option<String>,
    /// Scroll every page until it stops growing ("auto" or a round count). Implies browser tiers.
    #[serde(default)]
    pub scroll: Option<String>,
    /// best_first (default with a query), bfs, or dfs: one branch to its end, for a manual or a
    /// paginated listing.
    #[serde(default)]
    pub strategy: Option<String>,
    /// Stop once new pages add nothing about `query`. Default true with a query.
    #[serde(default)]
    pub stop_when_saturated: Option<bool>,
    /// Whole-crawl token cap. Default 60000.
    #[serde(default)]
    pub max_tokens_total: Option<usize>,
    /// Give up after this long regardless. Default 120000.
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
    /// Resume an interrupted crawl by the `crawl_id` it returned; web_status lists those with work
    /// left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crawl_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebCaptureParams {
    /// The page whose requests to record.
    pub url: String,
    /// Only responses whose URL contains this, e.g. "/api/". Omit it the first time.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Return the response bodies too, once `pattern` names the endpoint. Default false.
    #[serde(default)]
    pub bodies: Option<bool>,
    /// Per-body character cap. Default 20000.
    #[serde(default)]
    pub max_body: Option<usize>,
    /// How long to keep recording after the page loads, in ms. Default 3000.
    #[serde(default)]
    pub settle_ms: Option<u64>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    /// Profile saved by web_login whose cookies to use.
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSnapshotParams {
    /// The page to read.
    pub url: String,
    /// Only nodes whose name or role contains this, e.g. "add to cart" or "button".
    #[serde(default)]
    pub find: Option<String>,
    /// How deep to look; 3 or 4 usually reaches the controls.
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Cap on nodes returned. Default 200.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    /// Profile saved by web_login whose cookies to use.
    #[serde(default)]
    pub profile: Option<String>,
    /// Timeout in ms. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebActParams {
    /// The page to open first.
    pub url: String,
    /// Steps, in order, e.g. `[{"do":"type","ref":"e3","text":"shoes"},{"do":"press","key":
    /// "Enter"},{"do":"wait","selector":".results"}]`.
    #[schemars(with = "Vec<Action>")]
    pub actions: Vec<serde_json::Value>,
    /// markdown (default), text, or html for the final page.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    /// Profile saved by web_login whose cookies to use.
    #[serde(default)]
    pub profile: Option<String>,
    /// Proxy URL for this run only; web_route sets one per domain.
    #[serde(default)]
    pub proxy: Option<String>,
    /// Timeout in ms for the whole interaction. Default 90000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebScreenshotParams {
    /// The page to render.
    pub url: String,
    /// Capture the whole scrollable page. Default false (viewport only).
    #[serde(default)]
    pub full_page: Option<bool>,
    /// Browser tier: browser, stealth, real (default), warm.
    #[serde(default)]
    pub tier: Option<String>,
    /// Profile saved by web_login whose cookies to use.
    #[serde(default)]
    pub profile: Option<String>,
    /// Proxy URL for this run only; web_route sets one per domain.
    #[serde(default)]
    pub proxy: Option<String>,
    /// Return the PNG inline as image content too. Default true, skipped above 3 MB.
    #[serde(default)]
    pub inline: Option<bool>,
    /// Render the page as a phone would: phone user agent and viewport.
    #[serde(default)]
    pub mobile: Option<bool>,
    /// Timeout in ms. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebVideoParams {
    /// A video page, a page embedding a player, or a player address.
    pub url: String,
    /// Caption language to prefer, e.g. "es".
    #[serde(default)]
    pub lang: Option<String>,
    /// Keyframes to capture where the picture changes, up to 24; paths land in the timeline.
    #[serde(default)]
    pub frames: Option<u32>,
    /// Cap on the timeline returned. Default 25000.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// The `cursor` of a truncated result.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Write the timeline here and return the path.
    #[serde(default)]
    pub out_file: Option<String>,
    /// Profile saved by web_login whose cookies to use.
    #[serde(default)]
    pub profile: Option<String>,
    /// As on web_fetch.
    #[serde(default)]
    pub wait: Option<u64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebSearchParams {
    /// What to search for, as you would type it into a search box.
    pub query: String,
    /// auto (default: the first engine that answers), all (every engine, merged by agreement), or
    /// one of ddg, ddg-html, bing, brave, news (recent articles).
    #[serde(default)]
    pub engine: Option<String>,
    /// Results to return. Default 10, max 50.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Result language, e.g. es. Default: the query's.
    #[serde(default)]
    pub lang: Option<String>,
    /// Country, e.g. MX.
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct BrowserOpenParams {
    /// Profile saved by web_login whose cookies to reuse. Default: a fresh profile.
    #[serde(default)]
    pub profile: Option<String>,
    /// Proxy URL for this session; web_route sets one per domain.
    #[serde(default)]
    pub proxy: Option<String>,
    /// Show the browser window. Default false (offscreen).
    #[serde(default)]
    pub visible: Option<bool>,
    /// Navigate here as soon as the session is open: the first browser_do, in the same call.
    #[serde(default)]
    pub url: Option<String>,
    /// Steps to run once it is open, as in browser_do.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Action>>")]
    pub actions: Option<Vec<serde_json::Value>>,
    /// markdown (default), text, or html for the page returned.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Keep only the blocks relevant to these words.
    #[serde(default)]
    pub query: Option<String>,
    /// Timeout in ms for that first step. Default 90000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserDoParams {
    /// The `session_id` browser_open returned.
    #[serde(rename = "session_id")]
    pub session_id: String,
    /// Navigate here first. Omit to keep acting on the current page.
    #[serde(default)]
    pub url: Option<String>,
    /// Steps, in order, as in web_act; omit to only read the page.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Action>>")]
    pub actions: Option<Vec<serde_json::Value>>,
    /// markdown (default), text, or html for the page returned.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Keep only the blocks relevant to these words (BM25).
    #[serde(default)]
    pub query: Option<String>,
    /// Timeout in ms for the whole call. Default 90000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserSessionParams {
    /// The `session_id` browser_open returned.
    #[serde(rename = "session_id")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebLoginParams {
    /// The page to open in the visible window: the login form, or the blocked page itself.
    pub url: String,
    /// Profile to save the cookies under. Default: the domain's auto profile, which later fetches
    /// use on their own.
    #[serde(default)]
    pub profile: Option<String>,
    /// Seconds to wait for the person to finish; closing the window finishes early. Default 300.
    #[serde(default)]
    pub timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveImageParams {
    /// The captcha image, as base64 or as a URL.
    pub image: String,
    /// Say so when `image` is base64 and could be mistaken for a URL.
    #[serde(default)]
    pub is_base64: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveRecaptchaV2Params {
    /// The widget's site key, from the page's data-sitekey attribute.
    pub sitekey: String,
    /// The page the widget is on.
    #[serde(rename = "pageUrl")]
    pub page_url: String,
    /// The invisible variant, with no checkbox. Default false.
    #[serde(default)]
    pub invisible: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveTurnstileParams {
    /// The widget's site key, from the page's data-sitekey attribute.
    pub sitekey: String,
    /// The page the widget is on.
    #[serde(rename = "pageUrl")]
    pub page_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SolveHCaptchaParams {
    /// The widget's site key, from the page's data-sitekey attribute.
    pub sitekey: String,
    /// The page the widget is on.
    #[serde(rename = "pageUrl")]
    pub page_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptchaStatusParams {
    /// The `taskId` a solve_* tool returned.
    #[serde(rename = "taskId")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportCaptchaParams {
    /// The `taskId` a solve_* tool returned.
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// true: the site accepted the answer. false: it was rejected.
    pub good: bool,
    /// What the site said, if anything; kept with the outcome.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebRouteParams {
    /// The domain to route; subdomains inherit. Omit everything to list the routes.
    #[serde(default)]
    pub domain: Option<String>,
    /// Proxy URL, e.g. "socks5h://user:pass@host:1080" (socks5h resolves DNS at the exit).
    #[serde(default)]
    pub proxy: Option<String>,
    // Declared, never detected: detecting it would need a geolocation service.
    /// ISO country the proxy exits from, e.g. "DE", for a matching timezone and language.
    #[serde(default)]
    pub country: Option<String>,
    /// Several exits: the domain sticks to the first that works, moving on when it is blocked
    /// twice.
    #[serde(default)]
    pub proxies: Option<Vec<String>>,
    /// ISO country of each entry in `proxies`, by position; `country` covers the rest.
    #[serde(default)]
    pub countries: Option<Vec<String>>,
    /// Remove the route for `domain`.
    #[serde(default)]
    pub remove: Option<bool>,
    /// Test `domain`'s exits instead of changing them: answered, latency, DNS leak.
    #[serde(default)]
    pub check: Option<bool>,
    /// The URL a `check` fetches. Default: a small neutral page.
    #[serde(default)]
    pub check_url: Option<String>,
}

// `Default` is what makes a read-only `GET /v1/status` possible: every field below *mutates*, so
// the REST layer needs a way to ask for the report and nothing else.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WebStatusParams {
    /// Save browser and session policy settings for later calls; open sessions keep theirs.
    #[serde(default)]
    pub configure: Option<serde_json::Value>,
    /// Domain to clear on every exit: hard-block cooldown, server holds and strikes, visit window,
    /// remembered failure. Not `clear_budget`'s standing.
    #[serde(default)]
    pub clear_cooldown: Option<String>,
    /// More of the report: "all", or names from the summary's `sections`, comma-separated.
    #[serde(default)]
    pub sections: Option<String>,
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
    /// Cap on the content returned, as in web_fetch. Default 25000.
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
    /// Sources to use: robots, sitemap, llms, feeds, links. Default: all of them.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    /// Maximum URLs to return. Default 1000.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only URLs containing this substring.
    #[serde(default)]
    pub include: Option<String>,
}

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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSiteSearchParams {
    /// Any page of the site — usually the home page. Its search box is what gets used.
    pub url: String,
    /// What to search for.
    pub query: String,
    /// Fetch the results too, rather than only reporting the pattern. Default true.
    #[serde(default)]
    pub fetch: Option<bool>,
    /// Timeout in ms. Default 60000.
    #[serde(default)]
    pub timeout: Option<u64>,
}

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

// ---- Schema-only types --------------------------------------------------------------------------
//
// The two parameters below are `serde_json::Value` in Rust, because the browser and the extractor
// accept a little more than the model needs to know about (`evaluate` for `eval`, `js` for
// `script`). What the model reads is these: the shapes, the verbs, and what each field is for.
// `inline` keeps them out of a `$defs` table the model would have to page back to.

#[derive(JsonSchema)]
#[schemars(untagged, inline)]
#[allow(dead_code)]
pub enum SchemaSpec {
    /// "auto": the page's own repeated structure.
    Auto(String),
    /// Your own selectors, as on web_fetch.
    Spec(serde_json::Map<String, serde_json::Value>),
}

// One step of web_act or browser_do. A `//` comment on purpose: a doc comment here lands in both
// tools' schemas, where the model reads it twice and learns nothing.
#[derive(JsonSchema)]
#[schemars(inline)]
#[allow(dead_code)]
pub struct Action {
    /// verify: the element is there, visible, holding `value`, without the page. console: the
    /// page's log. hold: press and hold `ms`.
    #[serde(rename = "do")]
    pub kind: ActionKind,
    /// The element's `ref` from web_snapshot, e.g. "e12". Preferred.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// A CSS selector when there is no ref; for wait, the element to wait for.
    pub selector: Option<String>,
    /// type, fill: the text; `${NAME}` is filled from ~/.svipall/secrets.env.
    pub text: Option<String>,
    /// press: the key, e.g. Enter, Tab, ArrowDown.
    pub key: Option<String>,
    /// select: the option. verify: the value or text expected.
    pub value: Option<String>,
    /// wait: ms (default 1000; 10000 with a selector). hold: default 2000.
    pub ms: Option<u64>,
    /// scroll: how far. Default 600.
    pub pixels: Option<i64>,
    /// scroll: "stable" keeps scrolling until nothing more loads.
    pub until: Option<String>,
    /// scroll with until: cap on rounds.
    pub rounds: Option<u64>,
    /// eval: JavaScript run in the page; its value comes back.
    pub script: Option<String>,
    /// goto: where to navigate.
    pub url: Option<String>,
    /// screenshot: the whole scroll height. Default false.
    pub full_page: Option<bool>,
}

/// Every verb web_act and browser_do understand, with the other spellings each one accepts.
///
/// The one list: the schema's enum is built from the first column, the browser dispatches on both,
/// and an unknown verb is answered with all of it. Three copies of this used to disagree — the
/// error message had lost `verify`, `console` and `hold` long after they worked.
pub const ACTIONS: &[(&str, &[&str])] = &[
    ("click", &[]),
    ("type", &[]),
    ("fill", &[]),
    ("press", &[]),
    ("hover", &[]),
    ("select", &[]),
    ("scroll", &[]),
    ("wait", &[]),
    ("eval", &["evaluate"]),
    ("goto", &["navigate"]),
    ("verify", &[]),
    ("console", &[]),
    ("screenshot", &[]),
    ("hold", &["press_and_hold"]),
];

/// The verb an action spelling means, or `None` when it is not one.
pub fn action_verb(spelling: &str) -> Option<&'static str> {
    ACTIONS
        .iter()
        .find(|(verb, aliases)| *verb == spelling || aliases.contains(&spelling))
        .map(|(verb, _)| *verb)
}

/// What an unknown verb is answered with: the verb, and every spelling that would have worked.
pub fn unknown_action(spelling: &str) -> String {
    let accepted: Vec<String> = ACTIONS
        .iter()
        .map(|(verb, aliases)| {
            if aliases.is_empty() {
                (*verb).to_string()
            } else {
                format!("{verb} (or {})", aliases.join(", "))
            }
        })
        .collect();
    if spelling.is_empty() {
        format!("an action needs `do`, one of: {}", accepted.join(", "))
    } else {
        format!(
            "unknown action '{spelling}'; accepted: {}",
            accepted.join(", ")
        )
    }
}

/// The first action in a list whose verb is not one, as the message the caller gets. Checked
/// before a browser is started or a visit spent: a typo is not worth a page load to discover.
pub fn check_actions(actions: &[serde_json::Value]) -> Result<(), String> {
    for a in actions {
        let spelling = a
            .get("do")
            .or(a.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if action_verb(spelling).is_none() {
            return Err(unknown_action(spelling));
        }
    }
    Ok(())
}

/// The verbs web_act and browser_do understand: the first column of [`ACTIONS`].
#[allow(dead_code)]
pub struct ActionKind;

impl JsonSchema for ActionKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ActionKind".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let verbs: Vec<&str> = ACTIONS.iter().map(|(verb, _)| *verb).collect();
        schemars::json_schema!({ "type": "string", "enum": verbs })
    }
}
