//! Tool parameter structs for svipall MCP.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WebFetchParams {
    /// The page. Also `raw:<html>` for markup you already have (no request is made) and
    /// `file:///path` for a local file under `~/.svipall/in` or a configured `local_roots` entry.
    pub url: String,
    /// Leave unset: auto learns the tier per domain. Forcing one (http, browser, stealth, real,
    /// warm) is for debugging and is slower or weaker.
    #[serde(default)]
    pub mode: Option<String>,
    /// markdown (default), text, or html (the raw markup, many times the tokens).
    #[serde(default)]
    pub extraction: Option<String>,
    /// Keep only the elements this CSS selector matches, e.g. "article" or "#prices".
    #[serde(default)]
    pub css_selector: Option<String>,
    /// Drop navigation, footers and sidebars. Default true; false returns the whole body.
    #[serde(default)]
    pub main_content_only: Option<bool>,
    /// Keep only the blocks relevant to these words (BM25), e.g. "shipping costs". The cheapest
    /// way to read a long page for one fact, and how much it saves is decided by the query, not
    /// the page: measured on two long articles, "robots.txt" and "caching headers" left 6% and
    /// 13% of the page, while "history of scraping" left 88%. Name the fact, not the topic.
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
    /// Highest tier the ladder may climb to: http, browser, stealth, real, warm (default).
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
    /// Rows instead of prose, returned as `extracted`. E.g. `{"name": "products", "base_selector":
    /// "div.product", "fields": [{"name": "title", "selector": "h2 a"}, {"name": "url", "selector":
    /// "a", "type": "attribute", "attribute": "href"}]}`; types text (default), attribute, number,
    /// exists, list, html, markdown. A named schema is remembered per domain, and a selector a
    /// redesign breaks is relocated and reported as `healed`.
    #[serde(default)]
    #[schemars(with = "Option<SchemaSpec>")]
    pub schema: Option<serde_json::Value>,
    /// Cap on the content returned, cut on block boundaries. Default 25000. A truncated result
    /// carries a `cursor`.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// Continue a truncated response from where it stopped: the `cursor` of the previous result.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Add `metadata`: canonical URL, language, author, dates, OpenGraph, JSON-LD, feeds.
    #[serde(default)]
    pub include_metadata: Option<bool>,
    /// Add `links`, split into internal and external, plus images.
    #[serde(default)]
    pub include_links: Option<bool>,
    // Off by default on a measurement, not caution: on TECO, the one corpus that ships sibling
    // pages, a template learned from sixteen siblings saved 3.4% of the delivered text and removed
    // one word of human-labelled main content. One is too many for a default. The record is still
    // learned on every fetch, so turning this on works at once.
    /// Strip what this site repeats on every page (banners, footers), learned from earlier fetches
    /// of the same site. Off by default: it can take a word of real content with it. A response it
    /// changed says `"template": {"learned_from": 16, "removed_blocks": 3}`.
    #[serde(default)]
    pub use_site_template: Option<bool>,
    /// Add `quality_detail`: integrity verdict with reasons, optimisation traits, near-duplicates
    /// in the cache, provenance (byline, date, citations). For judging a source; the compact
    /// `quality` field is always present.
    #[serde(default)]
    pub include_quality: Option<bool>,
    /// robots.txt policy: warn (default: fetch, and say whether robots.txt disallows it), obey
    /// (refuse a disallowed URL), ignore.
    #[serde(default)]
    pub robots: Option<String>,
    /// Skip images, fonts, stylesheets and video in browser tiers. Faster on heavy pages; off by
    /// default because some anti-bot scripts notice a page whose images never loaded.
    #[serde(default)]
    pub text_only: Option<bool>,
    /// Ask as a phone: phone identity and viewport. Only worth it where a site serves a lighter
    /// page to phones — a responsive site, which is most of them, returns the same bytes: measured
    /// byte-identical on two sites at both the http and browser tiers. It also costs a browser
    /// page of its own, since no warm page is reused, and rules out the native last resort.
    #[serde(default)]
    pub mobile: Option<bool>,
    /// Write the content to this file and return the path instead: measured, a 418-character
    /// response against the 34 746 characters of the page it wrote. Relative paths land in
    /// ~/.svipall/out/. With `schema` or `tables` a .csv, .json or .jsonl name writes the rows in
    /// that format.
    #[serde(default)]
    pub out_file: Option<String>,
    /// Return the page's data tables as typed rows, `tables: [{caption, header, rows}]`, instead
    /// of prose: a fraction of the markdown, columns kept. Layout tables are skipped.
    #[serde(default)]
    pub tables: Option<bool>,
    /// Scroll a page that loads as you go before reading it: "auto" until it stops growing (up to
    /// 40 screens, one "load more" click), or a number of rounds. Implies a browser tier.
    #[serde(default)]
    pub scroll: Option<String>,
    /// A throwaway browser profile for this fetch alone: no cookies in, nothing left behind.
    #[serde(default)]
    pub isolated: Option<bool>,
    /// auto (default: serve a fresh copy, revalidate a stale one), read, write, bypass, refresh.
    #[serde(default)]
    pub cache: Option<String>,
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
    /// Keep only the blocks relevant to these words (BM25), on every page.
    #[serde(default)]
    pub query: Option<String>,
    /// Rows instead of prose from every page, as on web_fetch: "auto" or your own selectors.
    #[serde(default)]
    #[schemars(with = "Option<SchemaSpec>")]
    pub schema: Option<serde_json::Value>,
    /// Every page's data tables as typed rows, as on web_fetch.
    #[serde(default)]
    pub tables: Option<bool>,
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
    /// Leave unset: auto learns the tier per domain.
    #[serde(default)]
    pub mode: Option<String>,
    /// markdown (default), text, or html.
    #[serde(default)]
    pub extraction: Option<String>,
    /// Rank pages by relevance to these words, keep only the relevant blocks, and stop when new
    /// pages add nothing (see `stop_when_saturated`).
    #[serde(default)]
    pub query: Option<String>,
    /// Rows instead of prose from every page, as on web_fetch: "auto" or your own selectors. With
    /// `out_file`, one row per item, each carrying its page's `url`.
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
    /// Resume an interrupted crawl by the `crawl_id` it returned: queue, pages already fetched and
    /// parameters all come back. web_status lists the ones with work left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crawl_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebCaptureParams {
    /// The page whose requests to record.
    pub url: String,
    /// Only responses whose URL contains this, e.g. "/api/". Leave it out the first time to see
    /// everything the page asked for.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Return the response bodies too. Off by default because they are large; turn it on once
    /// `pattern` names the endpoint you want.
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
    /// Only nodes whose name or role contains this, e.g. "add to cart" or "button". Far fewer
    /// tokens when you know what you are looking for.
    #[serde(default)]
    pub find: Option<String>,
    /// How deep into the page to look. 3 or 4 usually reaches the controls that matter.
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSearchParams {
    /// What to search for, as you would type it into a search box.
    pub query: String,
    /// auto (default: the first engine that answers), all (every engine, merged by agreement), or
    /// one of ddg, ddg-html, bing, brave.
    #[serde(default)]
    pub engine: Option<String>,
    /// Results to return. Default 10, max 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
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
    /// ISO country the proxy exits from, e.g. "DE", so the browser announces a matching timezone
    /// and language. Declared, never detected: that would need a geolocation service.
    #[serde(default)]
    pub country: Option<String>,
    /// Several exits instead of one: the domain sticks to the first that works and moves on when
    /// that one is blocked twice.
    #[serde(default)]
    pub proxies: Option<Vec<String>>,
    /// ISO country of each entry in `proxies`, by position; `country` covers the rest.
    #[serde(default)]
    pub countries: Option<Vec<String>>,
    /// Remove the route for `domain`.
    #[serde(default)]
    pub remove: Option<bool>,
    /// Test the exits configured for `domain` instead of changing them: did each answer, how fast,
    /// any DNS leak or scheme problem.
    #[serde(default)]
    pub check: Option<bool>,
    /// The URL a `check` fetches through each exit. Default: a small neutral page.
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
    /// "auto": read the page's own repeated structure; the schema used comes back as
    /// `induced_schema`, to pass next time.
    Auto(String),
    /// Your own: `{"name", "base_selector", "fields": [{"name", "selector", "type", "attribute"}]}`,
    /// as shown on web_fetch.
    Spec(serde_json::Map<String, serde_json::Value>),
}

/// One step of web_act or browser_do.
#[derive(JsonSchema)]
#[schemars(inline)]
#[allow(dead_code)]
pub struct Action {
    /// verify: is the element there, visible, holding `value`; answers without the page. console:
    /// what the page logged. hold: press and hold for `ms`, for a hold-to-verify widget.
    #[serde(rename = "do")]
    pub kind: ActionKind,
    /// The element, as a `ref` from web_snapshot, e.g. "e12". Preferred over `selector`.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// The element as a CSS selector when there is no ref; for wait, the element to wait for.
    pub selector: Option<String>,
    /// type, fill: what to type. `${NAME}` is replaced from ~/.svipall/secrets.env, so a password
    /// never appears here.
    pub text: Option<String>,
    /// press: the key, e.g. Enter, Tab, ArrowDown.
    pub key: Option<String>,
    /// select: the option. verify: the value or text expected.
    pub value: Option<String>,
    /// wait: how long (default 1000; with a selector, the deadline, 10000). hold: default 2000.
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

/// The verbs web_act and browser_do understand.
#[derive(JsonSchema)]
#[schemars(inline, rename_all = "lowercase")]
#[allow(dead_code)]
pub enum ActionKind {
    Click,
    Type,
    Fill,
    Press,
    Hover,
    Select,
    Scroll,
    Wait,
    Eval,
    Goto,
    Verify,
    Console,
    Screenshot,
    Hold,
}
