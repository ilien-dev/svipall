//! svipall MCP server — rmcp based, stdio transport.
//! English only. Web exploration tools with a real tier ladder plus captcha tools.

use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::*,
    tool, tool_router, ErrorData as McpError, ServerHandler,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use svipall_core::cache::{CacheMode, CachedPage};
use svipall_core::classify::WallKind;
use svipall_core::profiles::identity_seed_for;
use svipall_core::{budget, extraction};
use svipall_core::{
    classify_view, domain_from_url, throttle, Config, IdentityProfile, PageParts, PageView,
    ParseWants,
};
use svipall_http::{Engine, FetcherConfig, HttpFetcher, HttpRequest};
use svipall_solver::queue::{JobType, SolverJob};

use crate::browser::{named_profile, save_png, BrowserPool, BrowserTier, PageOpts};
use crate::progress::{CrawlEvent, EventKind, ProgressSink};
use crate::search;
use crate::solver_engine::SolveEngine;
use crate::tools::*;

/// Extensions a crawl never follows, because there is no text behind them.
///
/// `.pdf`, `.xml`, `.rss`, `.atom` and `.json` used to be on this list, which meant the crawler
/// refused exactly the formats that carry readable content: PDFs, sitemaps and feeds.
const BINARY_EXT: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".ico", ".zip", ".tar", ".mp4", ".mp3",
    ".avi", ".mov", ".css", ".js", ".woff", ".woff2", ".ttf", ".otf", ".eot", ".exe", ".dmg",
];

#[derive(Clone)]
pub struct SvipallServer {
    live_policy: Option<Arc<tokio::sync::Mutex<LivePolicy>>>,
    tool_router: ToolRouter<Self>,
    solver_state: Option<Arc<svipall_solver::AppState>>,
    /// The http tier's engine, and the identity everything else must agree with.
    fetcher: Arc<dyn HttpFetcher>,
    proxy_fetchers: Arc<StdMutex<HashMap<String, Arc<dyn HttpFetcher>>>>,
    identity: Arc<IdentityProfile>,
    /// The identity the http tier wears. Equal to `identity` unless `http_firefox` is set, in
    /// which case it is a coherent Firefox while the browser tiers stay Chrome.
    http_identity: Arc<IdentityProfile>,
    /// Page cache and version history. None when the database could not be opened; everything
    /// keeps working without it, just without caching.
    store: Option<Arc<svipall_core::cache::Store>>,
    pool: Arc<BrowserPool>,
    native_pool: Arc<BrowserPool>,
    traffic: Arc<anyhow::Result<svipall_core::traffic::Ledger>>,
    cfg: Arc<Config>,
    /// Tokenised human-dashboard URL, or None when the solver DB could not be opened.
    dashboard_url: Option<Arc<str>>,
    /// Advertising and tracking hosts, loaded on first use. Empty until then, empty forever on a
    /// machine that cannot reach the sources, and that is a working state rather than an error.
    blocklist: Arc<tokio::sync::OnceCell<svipall_core::blocklist::Blocklist>>,
}

struct LivePolicy {
    signature: String,
    current: Option<Arc<SvipallServer>>,
    retired: Vec<Arc<SvipallServer>>,
}

#[derive(Debug)]
struct VisitRefusal {
    kind: &'static str,
    seconds: u64,
    detail: String,
}

impl std::fmt::Display for VisitRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}; wait {}s before retrying",
            self.kind, self.detail, self.seconds
        )
    }
}
impl std::error::Error for VisitRefusal {}

/// What a browser tier is being asked for, beyond the URL.
///
/// One value rather than five positional flags: the arguments are all booleans and options of the
/// same shapes, and a caller that swaps two of them compiles fine and behaves differently.
struct BrowserWants<'a> {
    text_only: bool,
    mobile: bool,
    proxy: Option<String>,
    profile: Option<&'a str>,
    /// Scroll rounds before reading the document; zero reads it as loaded.
    scroll: u32,
    /// Use a profile that exists only for this fetch, and delete it afterwards.
    isolated: bool,
}

/// Somewhere to say how a long job is going.
///
/// Held only when the client asked for it. A progress notification sent to a client that never
/// requested one is noise it has no way to interpret, and the protocol says so.
#[derive(Clone)]
pub struct Progress {
    peer: rmcp::service::Peer<rmcp::RoleServer>,
    token: ProgressToken,
}

impl Progress {
    fn new(peer: rmcp::service::Peer<rmcp::RoleServer>, token: ProgressToken) -> Self {
        Self { peer, token }
    }
}

impl ProgressSink for Progress {
    fn report<'a>(&'a self, e: &'a CrawlEvent) -> futures::future::BoxFuture<'a, ()> {
        Box::pin(async move {
            // Failure is ignored on purpose: a client that has stopped listening must not take the
            // crawl down with it.
            let _ = self
                .peer
                .notify_progress(ProgressNotificationParam {
                    progress_token: self.token.clone(),
                    progress: e.pages_done as f64,
                    total: e.total.map(|t| t as f64),
                    message: Some(e.message()),
                })
                .await;
        })
    }
}

/// Raw bytes a tier came back with. Deliberately unparsed: the ladder parses once, afterwards,
/// with everything the caller asked for, instead of each tier parsing for its own purposes.
struct TierOutcome {
    status: u16,
    html: String,
    final_url: String,
    content_type: String,
    /// Rounds of scrolling done before the document was read, when the caller asked for it.
    scroll_rounds: Option<u32>,
    /// Response headers, for cache validators, `Retry-After`, and the vendor signs that arrive on
    /// no other channel.
    headers: Vec<(String, String)>,
    /// The names of the cookies this response left behind. **Names only** — a cookie value is a
    /// session secret and must never reach a log line or the outward JSON, and a name is all the
    /// evidence a vendor sign needs.
    cookies: Vec<String>,
    /// What the warm wait did and why it stopped. `None` at every other tier, because no other
    /// tier waits.
    warm: Option<Value>,
    /// The key this attempt's page is filed under when it was worth keeping. `None` when nothing
    /// was kept — an isolated fetch, the http tier, or a page not worth returning to.
    kept_key: Option<String>,
}

impl TierOutcome {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Result of the whole ladder. `links` is filled when the caller asked for them (crawl), so the
/// crawler never re-parses a page the ladder already parsed.
pub struct FetchOutcome {
    pub value: Value,
    pub links: Vec<String>,
    pub final_url: String,
}

/// A screenshot and the JSON about it.
///
/// Two protocols want the same picture in different shapes — MCP as a second `Content`, REST as
/// base64 in the body — and only one of them should own the rule about when the picture is worth
/// sending at all. `inline` is that rule, decided once in `screenshot_json`.
pub struct ShotOutcome {
    pub value: Value,
    pub png: Vec<u8>,
    /// The caller asked for the picture and it is small enough to be worth carrying.
    pub inline: bool,
}

fn ok(v: Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}

fn err<E: std::fmt::Display>(e: E) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

/// How much this installation should trust HTTP/3 with one domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum H3Plan {
    /// Not built, not enabled, behind an exit, not https, never advertised, or already refused.
    Off,
    /// Advertised and never tried from here: worth one cheap attempt.
    Probe,
    /// Advertised and known to deliver: ask for it first, with the full budget.
    Use,
}

fn build_fetcher(
    identity: &IdentityProfile,
    engine: Engine,
    proxy: Option<&str>,
) -> Arc<dyn HttpFetcher> {
    let mut cfg = FetcherConfig::new(identity.clone());
    cfg.engine = engine;
    cfg.proxy = proxy.map(str::to_string);
    svipall_http::build(cfg).unwrap_or_else(|e| {
        tracing::warn!("http engine: {e}; falling back to reqwest");
        let mut fallback = FetcherConfig::new(identity.clone());
        fallback.engine = Engine::Reqwest;
        fallback.proxy = proxy.map(str::to_string);
        svipall_http::build(fallback).expect("reqwest engine always builds")
    })
}

/// Whether this outcome is a gate answering rather than a page arriving, and which gate it was.
///
/// The gates at the top of `fetch_inner` all say the same thing in the same shape — a
/// `blocked_reason`, no attempts and no status — because none of them made a request. That is the
/// difference between "the site refused us", which is an answer worth keeping, and "we decided not
/// to ask", which leaves the URL exactly as untried as it was before.
fn never_requested(v: &Value) -> Option<&'static str> {
    let untried = v
        .get("attempts")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        && v.get("status").and_then(Value::as_u64).unwrap_or(0) == 0;
    if !untried {
        return None;
    }
    match v.get("blocked_reason").and_then(Value::as_str) {
        Some("address_budget") => Some("over_budget"),
        Some("cooldown") => Some("cooldown"),
        Some("traffic_state") => Some("traffic_state"),
        Some("timeout") if v["network_attempted"] == false => Some("timeout"),
        _ => None,
    }
}

/// Why a warm wait stopped.
///
/// One value, decided in one place, so the log line and the fetch JSON cannot disagree about it.
/// Before this the wait had four scattered `break`s and logged only one of them: an operator could
/// not tell a pass from a timeout from a refusal, and those are three different next moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WarmEnd {
    /// The page arrived, or turned into something no wait improves.
    Cleared,
    /// A not-found template, a login wall, a gate or a subscription stub. Waiting changes none of
    /// them.
    WallKindStop,
    /// The wall named this machine's address. Another exit is the only move.
    Blamed,
    Deadline,
    /// The page said it was nearly through, the deadline moved once, and it still did not arrive.
    ExtendedThenDeadline,
}

impl WarmEnd {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cleared => "cleared",
            Self::WallKindStop => "wall-kind",
            Self::Blamed => "blamed",
            Self::Deadline => "deadline",
            Self::ExtendedThenDeadline => "extended-then-deadline",
        }
    }
}

/// The attempt line for a tier that raised.
///
/// The whole chain, not the outermost context: "launching browser" says where it failed, and the
/// cause underneath — the browser's own last words, a missing display, a directory another
/// instance holds — is what the reader needs to do something about it. One line, so the report
/// stays a list.
fn exc_attempt(route: &str, e: &anyhow::Error, ms: u128) -> String {
    let chain = format!("{e:#}");
    let one_line = chain.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{route}: EXC {one_line} ({ms}ms)")
}

/// Should the wait stop now, and what would it say if asked why?
///
/// Pure, so the truth table — especially the extension, which is allowed exactly once and only on
/// the page's own word — is testable without a browser.
fn warm_should_stop(
    delivered: bool,
    kind: &WallKind,
    blamed: bool,
    past_deadline: bool,
    already_extended: bool,
    reports_progress: bool,
) -> Option<WarmEnd> {
    if delivered {
        return Some(WarmEnd::Cleared);
    }
    if matches!(
        kind,
        WallKind::NotFound
            | WallKind::Login
            | WallKind::Gate
            | WallKind::SoftNotFound
            | WallKind::Paywall
    ) {
        return Some(WarmEnd::WallKindStop);
    }
    // Before the deadline check: "this address is refused" is the more specific answer, and the
    // one that names the fix.
    if blamed {
        return Some(WarmEnd::Blamed);
    }
    if past_deadline {
        return match (already_extended, reports_progress) {
            (false, true) => None,
            (true, _) => Some(WarmEnd::ExtendedThenDeadline),
            _ => Some(WarmEnd::Deadline),
        };
    }
    None
}

fn is_local(domain: &str) -> bool {
    domain == "localhost"
        || domain.starts_with("127.")
        || domain == "::1"
        || domain == "[::1]"
        || domain.ends_with(".local")
        || domain.starts_with("192.168.")
        || domain.starts_with("10.")
}

fn same_site(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!(".{}", b)) || b.ends_with(&format!(".{}", a))
}

fn looks_binary(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_lowercase();
    BINARY_EXT.iter().any(|e| path.ends_with(e))
}

/// Put everything the fetch measured onto a response.
///
/// ▲ One place, so a page served from the cache is labelled exactly as the fetch labelled it. That
/// was the whole bug: the verdict was stored and re-applied on a hit while the optimisation level
/// and the substance label were not, so the same URL answered with six fields or three depending on
/// whether the cache happened to hold it, and nothing in the response said which had happened.
///
/// The verdict is always present on a delivered page — its absence would otherwise mean both
/// "nothing wrong with it" and "nobody looked". The reasons cost tokens only when there are any.
/// `optimization` is likewise always present when it was measured, so an *absent* field means
/// nobody measured it rather than "ordinary".
fn insert_quality(obj: &mut serde_json::Map<String, Value>, r: &svipall_core::quality::Record) {
    obj.insert("quality".into(), json!(r.integrity.verdict));
    if !r.integrity.reasons.is_empty() {
        obj.insert(
            "quality_reasons".into(),
            json!(r
                .integrity
                .reasons
                .iter()
                .map(|x| x.as_str())
                .collect::<Vec<_>>()),
        );
    }
    if let Some(opt) = &r.optimization {
        obj.insert("optimization".into(), json!(opt.level));
        if !opt.traits.is_empty() {
            obj.insert(
                "optimization_traits".into(),
                json!(opt.traits.iter().map(|t| t.as_str()).collect::<Vec<_>>()),
            );
        }
    }
    // What a trained classifier makes of the page, when this machine has one. Absent rather than
    // guessed: "svipall cannot say" and "this page is junk" must never read the same. It labels
    // and nothing else — the page is returned either way, in the same position, with the same
    // content.
    if let Some(sub) = &r.substance {
        obj.insert("substance".into(), json!(sub.label));
        obj.insert("substance_confidence".into(), json!(sub.confidence));
    }
}

/// Add to a page's verdict something only this caller could know.
///
/// A single fetch sees one page; a crawl sees the site around it, and so can say that what came
/// back was almost entirely the site's own furniture. The verdict is widened rather than recomputed
/// because the fetch already looked at evidence the crawl no longer holds.
fn add_quality_reason(
    obj: &mut serde_json::Map<String, Value>,
    reason: svipall_core::quality::Reason,
) {
    let list = obj
        .entry("quality_reasons")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(arr) = list.as_array_mut() {
        if !arr.iter().any(|v| v.as_str() == Some(reason.as_str())) {
            arr.push(json!(reason.as_str()));
        }
    }
    // Any reason at all means the page is not whole. `partial` says something more specific than
    // `thin` and is left alone.
    if obj.get("quality").and_then(|v| v.as_str()) == Some("full") {
        obj.insert(
            "quality".into(),
            json!(svipall_core::quality::Verdict::Thin),
        );
    }
}

/// Everything stored beside a cached page, if the row was written by a build that recorded it.
fn stored_quality(raw: Option<&str>) -> Option<svipall_core::quality::Record> {
    svipall_core::quality::Record::parse(raw?)
}

/// Record one observation in its class's running distribution, and say where it fell.
///
/// The order matters and is the honest one: the page is added to the history *before* it is
/// placed in it, so a value is never compared against a distribution that excludes it. With a few
/// hundred observations the difference is a rounding error; with thirty it is the difference
/// between a percentile and a statement about everyone else.
///
/// Best effort throughout. A failed `kv` write must never fail a fetch — the worst case is one
/// page missing from a histogram.
fn calibrate(
    store: Option<&svipall_core::cache::Store>,
    class: &str,
    x: f32,
) -> Option<serde_json::Value> {
    use svipall_core::quality::calibration::Distribution;
    let store = store?;
    let key = svipall_core::quality::calibration::key(class);
    let mut dist = Distribution::parse(store.kv_get(&key).as_deref());
    dist.observe(x);
    let _ = store.kv_set(&key, &dist.to_json());
    match dist.percentile(x) {
        Some(p) => Some(json!({
            "percentile": p.value,
            "observations": p.observations,
            "band": p.band,
        })),
        // Said rather than omitted: "no percentile" and "the percentile is low" must not look the
        // same, and a caller shown neither would assume the second.
        None => dist.why_not().map(|why| json!({ "unavailable": why })),
    }
}

/// What this machine has learned about a site's own furniture.
fn site_template(
    store: Option<&svipall_core::cache::Store>,
    domain: &str,
) -> svipall_core::template::Template {
    match store {
        Some(s) => svipall_core::template::Template::parse(
            s.kv_get(&svipall_core::template::key(domain)).as_deref(),
        ),
        None => svipall_core::template::Template::default(),
    }
}

/// Take the site's frame off a page, and say so on the response.
///
/// ▲ Applied on a fresh fetch and on a cache hit alike, from the same persisted record, so the two
/// answer identically. The cache stores the page at full fidelity — this is a view of it, not a
/// rewrite of it, and clearing the record brings the furniture back.
///
/// The announcement is the other half. A result that changes between two sessions because a tool
/// learned something in between, and does not say so, is worse than one that never improved.
fn apply_template(
    obj: &mut serde_json::Map<String, Value>,
    template: &svipall_core::template::Template,
    markdown: &str,
    asked: bool,
) -> String {
    if !asked {
        return markdown.to_string();
    }
    let (out, applied) = template.strip(markdown);
    if applied.removed_blocks > 0 {
        obj.insert(
            "template".into(),
            json!({
                "learned_from": applied.learned_from,
                "removed_blocks": applied.removed_blocks,
            }),
        );
    }
    out
}

/// Human-readable next step for a wall that survived the ladder.
fn guidance(kind: &WallKind, domain: &str, tier: &str, solver: bool, dashboard: &str) -> String {
    let login_hint = format!("web_login(url) opens a visible window: pass the check once by hand and the {} profile keeps the cookies for later real/warm fetches.", domain);
    match kind {
        WallKind::Hold | WallKind::Generic | WallKind::Cloudflare => {
            let mut s = format!("Challenge still present at tier {}. {} Or route the domain through a proxy with web_route.", tier, login_hint);
            if solver { s.push_str(&format!(" If the page exposes a sitekey, solve_turnstile / solve_recaptcha_v2 / solve_hcaptcha return a token; humans can also solve at {}.", dashboard)); }
            s
        }
        WallKind::Vendor => format!("Browser-fingerprinting wall (DataDome / PerimeterX / Incapsula) at tier {}. {} A residential proxy via web_route usually helps too.", tier, login_hint),
        WallKind::Login => "Login wall. Use web_login(url, profile=NAME) to sign in once, then pass profile=NAME to web_fetch / web_act.".to_string(),
        WallKind::Gate => "Geo or consent gate instead of the page. Use web_act to dismiss it (click the accept/continue button) or web_route to change the exit country.".to_string(),
        WallKind::Empty => format!("Page did not render text at tier {}. Try web_act with a wait action, or a css_selector for the region you need.", tier),
        WallKind::NotFound => "The URL does not exist (404/410). Check the address; escalating tiers cannot help.".to_string(),
        WallKind::SoftNotFound => "The page answered 200 but says it does not exist. Check the address; escalating tiers cannot help, and the stub is not the page you asked for.".to_string(),
        WallKind::Paywall => format!("The article exists and is being withheld behind a subscription. {login_hint} Only a profile that is signed in changes the answer; a proxy does not."),
        WallKind::Status => format!("Hard HTTP block at tier {}; domain is on a 15 min cooldown. Use web_route with a proxy, or web_status(clear_cooldown=\"{}\") to retry sooner.", tier, domain),
        WallKind::None => String::new(),
    }
}

/// Send a result to a file and hand back the path.
///
/// The saving is the whole point: a path is twenty tokens and the content it stands in for is often
/// tens of thousands. Relative paths are rooted under `~/.svipall/out/` rather than the working
/// directory, so a tool call cannot be talked into writing anywhere it likes.
/// Where a named output file lands.
///
/// An absolute path is taken as written; a relative one keeps only its file name, so
/// `../../etc/passwd` becomes `passwd` under the output directory rather than an escape.
fn out_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let root = svipall_core::config::home_dir().join("out");
    let requested = std::path::Path::new(name);
    let path = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested.file_name().unwrap_or(requested.as_os_str()))
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn write_out(name: &str, content: &str) -> anyhow::Result<(std::path::PathBuf, usize)> {
    let path = out_path(name)?;
    std::fs::write(&path, content)?;
    Ok((path, content.len()))
}

/// Path (with query) of a URL, for judging whether a request landed where it asked to.
fn path_of(url: &str) -> &str {
    match url.find("://") {
        Some(i) => match url[i + 3..].find('/') {
            Some(j) => &url[i + 3 + j..],
            None => "/",
        },
        None => url,
    }
}

/// Every way out of "there is no browser", in one message.
///
/// The old text was a single line naming one config key. When a tier is skipped the ladder still
/// delivers whatever the http tier managed, so the note has to say that too — otherwise the caller
/// reads a blocked_reason and assumes it got nothing.
/// Append the causes that live on this machine to a blocked page's note.
///
/// Both of these defeat every stealth measure in the project, both are invisible from the page, and
/// until now both were only ever said in a log nobody reads while a fetch was failing. Order is by
/// how completely each one explains the block: something rewriting the page beats something wrong
/// with the browser, and either beats the site.
pub(crate) fn local_causes(note: &str, injection: Option<&str>, browser: Option<&str>) -> String {
    let mut out = note.to_string();
    if let Some(what) = injection {
        out.push_str(&format!(
            " This page was modified by {what} before the site's own scripts ran; anti-bot vendors \
             see that. Exclude svipall and its browser from the product's web protection, or \
             disable its script injection, and try again."
        ));
    }
    if let Some(advice) = browser {
        out.push(' ');
        out.push_str(advice);
    }
    out
}

/// Store key for the newest stable Chrome major `browser_setup` has seen.
const LATEST_STABLE_KEY: &str = "browser/latest_stable_major";

fn no_browser_hint() -> String {
    let platform = crate::provision::platform().unwrap_or("this platform");
    format!(
        "no Chromium-based browser found, so browser tiers are unavailable and the result below is \
         whatever the http tier managed. Any of these fixes it: (1) install Microsoft Edge or \
         Google Chrome — Edge ships with Windows; (2) browser_setup(action=\"install\") downloads \
         Chrome for Testing for {platform} (~190 MB) into ~/.svipall/browser; (3) set browser_path in \
         ~/.svipall/config.toml; (4) set the SVIPALL_BROWSER environment variable."
    )
}

thread_local! {
    /// Schema compilation problems from the current fetch, so they can be reported next to the
    /// result rather than swallowed or turned into a tool error.
    static SCHEMA_ERRORS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn take_schema_errors() -> Vec<String> {
    SCHEMA_ERRORS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// Scheme, host **and port**. robots.txt is per origin, and an origin includes the port: rules
/// served on `:8443` do not govern `:443`, and looking one up under the bare host would both apply
/// the wrong rules and fetch robots.txt from the wrong place.
fn origin_of(url: &url::Url) -> Option<String> {
    Some(url.origin().ascii_serialization())
}

/// One robots.txt cache for the process: `robots_for` fills it, `robots_cached` only reads it.
fn robots_cache() -> &'static RobotsCache {
    static CACHE: std::sync::OnceLock<RobotsCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Parsed robots.txt per origin, with the time it was fetched.
type RobotsCache = StdMutex<HashMap<String, (Arc<svipall_core::Robots>, Instant)>>;

fn jump(tiers: &[String], i: usize, target: &str) -> usize {
    match tiers.iter().position(|t| t == target) {
        Some(j) if j > i => j,
        _ => i + 1,
    }
}

/// What a resumed crawl brings back from the database.
#[derive(Default)]
struct ResumeState {
    /// Still queued, with the score it was queued at.
    pending: Vec<(String, u16, f32)>,
    /// Already fetched, so it is neither refetched nor re-queued.
    done: Vec<String>,
}

impl SvipallServer {
    pub fn new(
        solver_state: Option<Arc<svipall_solver::AppState>>,
        cfg: Config,
        dashboard_url: Option<String>,
    ) -> Self {
        Self::with_store(solver_state, cfg, dashboard_url, None)
    }

    /// Same, with the page cache supplied rather than opened from `~/.svipall`. Tests use it to get a
    /// database of their own; `None` means the default one.
    pub fn with_store(
        solver_state: Option<Arc<svipall_solver::AppState>>,
        cfg: Config,
        dashboard_url: Option<String>,
        store: Option<Arc<svipall_core::cache::Store>>,
    ) -> Self {
        let pool = Arc::new(BrowserPool::new(cfg.clone()));
        let mut native_cfg = cfg.clone();
        native_cfg.browser_identity = "native".into();
        let native_pool = Arc::new(BrowserPool::new(native_cfg));
        let traffic = Arc::new(svipall_core::traffic::Ledger::open(
            &svipall_core::config::home_dir().join("traffic.sqlite3"),
        ));
        // The browser binary decides which Chrome we may claim to be; see IdentityProfile::resolve.
        let identity = IdentityProfile::resolve(pool.browser_major(), &cfg);
        let store = store.or_else(|| match svipall_core::cache::Store::open() {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                tracing::warn!("page cache unavailable, continuing without it: {e}");
                None
            }
        });
        let engine = Engine::resolve(&cfg.http_engine);
        // The http tier can wear Firefox while the browser tiers stay Chrome. A domain that never
        // needs a browser then presents a Firefox that is Firefox in TLS, headers and UA together.
        let http_identity = if cfg.http_firefox {
            IdentityProfile::resolve_firefox(&cfg)
        } else {
            identity.clone()
        };
        let fetcher = build_fetcher(&http_identity, engine, None);
        tracing::info!(
            "http engine: {} | identity: {:?} {} on {:?} (browser: Chrome {})",
            svipall_http::engine_report(fetcher.engine()),
            http_identity.engine,
            http_identity.chrome_major,
            http_identity.os,
            identity.chrome_major
        );
        Self {
            live_policy: None,
            tool_router: Self::tool_router(),
            solver_state,
            fetcher,
            proxy_fetchers: Arc::new(StdMutex::new(HashMap::new())),
            http_identity: Arc::new(http_identity),
            identity: Arc::new(identity),
            store,
            pool,
            native_pool,
            traffic,
            cfg: Arc::new(cfg),
            dashboard_url: dashboard_url.map(Arc::from),
            blocklist: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Long-running transports refresh policy at request boundaries. Tests and benchmarks keep
    /// their explicitly supplied configuration unless they opt in to this application behavior.
    pub fn with_live_configuration(mut self) -> Self {
        let current = Arc::new(self.clone());
        self.live_policy = Some(Arc::new(tokio::sync::Mutex::new(LivePolicy {
            signature: serde_json::to_string(self.cfg.as_ref()).unwrap_or_default(),
            current: Some(current),
            retired: Vec::new(),
        })));
        self
    }

    pub async fn active(&self) -> anyhow::Result<Arc<Self>> {
        let Some(live) = &self.live_policy else {
            return Ok(Arc::new(self.clone()));
        };
        let mut state = live.lock().await;
        let mut cfg = svipall_core::config::load_in(&svipall_core::config::home_dir())?;
        let signature = serde_json::to_string(&cfg)?;
        if signature != state.signature {
            crate::provision::ensure_browser(&mut cfg).await?;
            let next = Arc::new(Self::with_store(
                self.solver_state.clone(),
                cfg,
                self.dashboard_url.as_ref().map(|s| s.to_string()),
                self.store.clone(),
            ));
            let old = state.current.replace(next).unwrap_or_else(|| {
                let mut old = self.clone();
                old.live_policy = None;
                Arc::new(old)
            });
            state.retired.push(old);
            state.signature = signature;
        }
        // In-flight calls own an Arc and explicitly opened browser sessions retain their pool.
        // Completed generations can close immediately; a configuration update never interrupts
        // a request that already started.
        let mut i = 0;
        while i < state.retired.len() {
            if Arc::strong_count(&state.retired[i]) == 1
                && state.retired[i].pool.session_ids().await.is_empty()
            {
                let old = state.retired.remove(i);
                old.pool.shutdown().await;
                old.native_pool.shutdown().await;
            } else {
                i += 1;
            }
        }
        Ok(state.current.clone().unwrap_or_else(|| {
            let mut current = self.clone();
            current.live_policy = None;
            Arc::new(current)
        }))
    }

    pub async fn reap_configuration(&self) {
        if let Ok(active) = self.active().await {
            active.pool.reap_idle().await;
            active.native_pool.reap_idle().await;
        }
    }

    pub fn solve_engine(&self) -> SolveEngine {
        match &self.solver_state {
            Some(state) => SolveEngine::with_state(self.pool.clone(), &self.cfg, state.clone()),
            None => SolveEngine::new(self.pool.clone(), &self.cfg),
        }
    }

    pub async fn shutdown_configuration(&self) {
        self.pool.shutdown().await;
        self.native_pool.shutdown().await;
        if let Some(live) = &self.live_policy {
            let state = live.lock().await;
            if let Some(current) = &state.current {
                current.pool.shutdown().await;
                current.native_pool.shutdown().await;
            }
            for old in &state.retired {
                old.pool.shutdown().await;
                old.native_pool.shutdown().await;
            }
        }
    }

    /// Serve a cached copy when the caller allows it and the copy is still fresh.
    ///
    /// Only the http tier revalidates: a 304 is worth having there, whereas a browser tier has to
    /// render the page regardless and would save nothing.
    fn cache_lookup(&self, p: &WebFetchParams, mode: CacheMode) -> Option<(CachedPage, bool)> {
        if !mode.may_read() {
            return None;
        }
        // A dev server changes under you, so a local URL is not cached unless the caller asked
        // for caching by name. An explicit `cache:` is an instruction, not a default.
        if is_local(&domain_from_url(&p.url)) && p.cache.is_none() {
            return None;
        }
        let store = self.store.as_ref()?;
        let hit = store.get(&p.url)?;
        Some((hit.clone(), hit.is_fresh()))
    }

    /// robots.txt for a URL's origin, cached for an hour.
    ///
    /// A 4xx means there are no rules. A 5xx or a timeout is treated as "allow", which departs
    /// from RFC 9309's "disallow everything": for a tool an operator runs on their own machine, a
    /// transient 503 on robots.txt should not quietly make a domain unreachable.
    /// Boxed because fetching robots.txt goes through the ladder, which consults robots.txt: the
    /// cycle is one level deep at runtime (the inner fetch says `ignore`) but the compiler sees a
    /// recursive async fn and needs the indirection.
    fn robots_for<'a>(
        &'a self,
        url: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<svipall_core::Robots>> + Send + 'a>,
    > {
        Box::pin(async move {
            let cache = robots_cache();
            let parsed = url::Url::parse(url).ok()?;
            let origin = origin_of(&parsed)?;
            if let Some(r) = self.robots_cached(url) {
                return Some(r);
            }
            let out = self
                .fetch_json(WebFetchParams {
                    url: format!("{origin}/robots.txt"),
                    extraction: Some("html".into()),
                    // A sitemap is not prose and must not go through the token budget: trimmed at
                    // eight thousand tokens, a fifty-thousand-URL sitemap becomes a few hundred
                    // URLs and the crawl silently misses the rest.
                    max_tokens: Some(1_000_000),
                    max_tier: Some("http".into()),
                    timeout: Some(5_000),
                    cache: Some("bypass".into()),
                    robots: Some("ignore".into()),
                    ..Default::default()
                })
                .await;
            let status = out
                .value
                .get("status")
                .and_then(|s| s.as_u64())
                .unwrap_or(0);
            let body = out
                .value
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or_default();
            // Only a 2xx is a real robots.txt. Anything else means "no rules we can see".
            let robots = if (200..300).contains(&status) {
                svipall_core::Robots::parse(body)
            } else {
                svipall_core::Robots::default()
            };
            cache
                .lock()
                .unwrap()
                .insert(origin, (Arc::new(robots.clone()), Instant::now()));
            Some(robots)
        })
    }

    /// robots.txt for an origin *only if it has already been fetched*.
    ///
    /// A single `web_fetch` a person asked for should not cost a second request just to annotate
    /// the answer, so `warn` reports what it already knows and stays silent otherwise. Crawling,
    /// which obeys, goes through `robots_for` and does pay for the fetch.
    fn robots_cached(&self, url: &str) -> Option<svipall_core::Robots> {
        let parsed = url::Url::parse(url).ok()?;
        let origin = origin_of(&parsed)?;
        let cache = robots_cache().lock().unwrap();
        let (r, at) = cache.get(&origin)?;
        (at.elapsed() < Duration::from_secs(3600)).then(|| (**r).clone())
    }

    /// Whether robots.txt allows this exact URL, given what is already known about the origin.
    fn robots_allows(&self, url: &str, robots: &svipall_core::Robots) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return true;
        };
        let path = match parsed.query() {
            Some(q) => format!("{}?{}", parsed.path(), q),
            None => parsed.path().to_string(),
        };
        robots.allows("svipall", &path)
    }

    /// Where a human can solve what svipall could not. Carries the per-run token.
    fn dashboard(&self) -> &str {
        self.dashboard_url
            .as_deref()
            .unwrap_or("the dashboard (unavailable: solver DB failed to open)")
    }

    /// Where this installation is allowed to go.
    ///
    /// Built per call rather than cached: it is three vector clones against a network request, and
    /// an operator who edits the config expects the next fetch to obey it.
    fn origin_policy(&self) -> svipall_core::policy::OriginPolicy {
        svipall_core::policy::OriginPolicy {
            allow: self.cfg.allow_origins.clone(),
            block: self.cfg.block_origins.clone(),
            refuse_private: self.cfg.refuse_private_addresses,
            local_roots: if self.cfg.local_roots.is_empty() {
                vec![svipall_core::config::home_dir().join("in")]
            } else {
                self.cfg.local_roots.iter().map(PathBuf::from).collect()
            },
        }
    }

    /// What this schema's selectors found the last time they ran on this domain, so a selector a
    /// redesign broke can be relocated. Empty without a store, without a schema, or on a first
    /// visit.
    fn selector_memory(
        &self,
        p: &WebFetchParams,
        url: &str,
    ) -> svipall_core::extraction::Fingerprints {
        let (Some(store), Some(schema)) = (self.store.as_ref(), p.schema.as_ref()) else {
            return Default::default();
        };
        let name = schema
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("items");
        svipall_core::selectors::load(store, &domain_from_url(url), name)
    }

    /// Rows to a file named by its extension, the way `web_crawl` has always done it. Returns
    /// false when the name carries no format, after saying so in the result.
    fn export_rows_into(obj: &mut serde_json::Map<String, Value>, name: &str, rows: &[Value]) {
        match svipall_core::export::Format::of(name) {
            Some(format) => {
                let text = svipall_core::export::render(rows, format);
                match write_out(name, &text) {
                    Ok((path, bytes)) => {
                        obj.insert("out_file".into(), json!(path.to_string_lossy()));
                        obj.insert("format".into(), json!(format.name()));
                        obj.insert("bytes".into(), json!(bytes));
                        obj.insert("rows".into(), json!(rows.len()));
                    }
                    Err(e) => {
                        // Returning the rows instead would defeat the request in the most
                        // expensive way available.
                        obj.insert("out_file_error".into(), json!(e.to_string()));
                    }
                }
            }
            None => {
                obj.insert(
                    "out_file_error".into(),
                    json!(
                        "name the file .csv, .json or .jsonl so the format is not \
                           something the two of us can disagree about"
                    ),
                );
            }
        }
    }

    /// Markup handed over inline or read from a local file, shaped like a response so the rest of
    /// the http tier — document sniffing, parsing, classification — does not know the difference.
    async fn local_response(url: &str) -> anyhow::Result<svipall_http::HttpResponse> {
        let (body, content_type) = if let Some(html) = url.strip_prefix("raw:") {
            (html.as_bytes().to_vec(), "text/html".to_string())
        } else {
            let path = svipall_core::policy::file_url_path(url)
                .ok_or_else(|| anyhow::anyhow!("not a file URL"))?;
            let body = tokio::fs::read(&path).await?;
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let ct = match ext.as_str() {
                "html" | "htm" | "xhtml" => "text/html",
                "pdf" => "application/pdf",
                "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "pptx" => {
                    "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                }
                "odt" => "application/vnd.oasis.opendocument.text",
                "ods" => "application/vnd.oasis.opendocument.spreadsheet",
                "odp" => "application/vnd.oasis.opendocument.presentation",
                "epub" => "application/epub+zip",
                "rtf" => "application/rtf",
                "doc" => "application/msword",
                "csv" => "text/csv",
                "xml" | "rss" | "atom" => "application/xml",
                "json" => "application/json",
                _ => "text/plain",
            };
            (body, ct.to_string())
        };
        Ok(svipall_http::HttpResponse {
            status: 200,
            final_url: url.to_string(),
            headers: Vec::new(),
            content_type,
            body,
            http_version: "local",
        })
    }

    /// The advertising and tracking hosts, fetched at most once per run.
    ///
    /// Behind a `OnceCell` rather than loaded at startup: most runs never turn this on, and a
    /// hundred thousand lines parsed for a fetch that was going to be a single page is a cost
    /// nobody asked for.
    async fn ad_hosts(&self) -> &svipall_core::blocklist::Blocklist {
        self.blocklist
            .get_or_init(|| async {
                crate::blocklists::load(&self.cfg.blocklist_sources, &self.identity).await
            })
            .await
    }

    /// The page cache, for a caller that needs to read what this installation has seen — the
    /// CLI's training-set export, which is the only thing outside the server that does.
    pub fn store(&self) -> Option<&Arc<svipall_core::cache::Store>> {
        self.store.as_ref()
    }

    /// Every tool this server exposes over MCP, by name.
    ///
    /// The `#[tool_router]` macro generates a private `tool_router()`, so this three-line accessor
    /// is the way in. It exists for one caller: the conformance test that fails when a new `#[tool]`
    /// is neither a REST route nor a named exclusion, which is what keeps `rest::ROUTES` from
    /// quietly falling behind `server.rs`.
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        names.sort();
        names
    }

    /// The config this server was built with, for a caller that has to bind a port or size a queue.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn pool(&self) -> Arc<BrowserPool> {
        self.pool.clone()
    }

    pub fn fetcher(&self) -> Arc<dyn HttpFetcher> {
        self.fetcher.clone()
    }

    fn fetcher_for(&self, proxy: Option<&str>) -> Arc<dyn HttpFetcher> {
        let Some(p) = proxy else {
            return self.fetcher.clone();
        };
        let mut map = self.proxy_fetchers.lock().unwrap();
        // A pool of exits means a client per exit; bounded so a long session over many domains
        // does not keep every client it ever built. Evict one, not all: `clear()` here threw away
        // the cookie jar of every exit to make room for one, so a pool of ten lost every session
        // it held whenever the eleventh domain showed up.
        if map.len() >= 16 && !map.contains_key(p) {
            if let Some(victim) = map.keys().next().cloned() {
                map.remove(&victim);
            }
        }
        map.entry(p.to_string())
            .or_insert_with(|| {
                // The exit's country, not the machine's: an `accept-language` from home leaving
                // through a proxy abroad is a one-line check any site can run.
                let identity = svipall_core::exits::identity_for_exit(&self.http_identity, Some(p));
                build_fetcher(&identity, Engine::resolve(&self.cfg.http_engine), Some(p))
            })
            .clone()
    }

    /// Charge a page opened outside the ladder.
    ///
    /// `throttle` charges every rung the ladder walks, but a browser opened by `web_act`,
    /// `web_snapshot`, `web_capture`, `web_map`, `web_site_search`, `web_screenshot`,
    /// `browser_open` or the solver never passes through it — and those are real visits that a
    /// host scores exactly like a fetch. Left uncharged, an agent told "this address has spent its
    /// budget" could go on spending it through any of them, which would make the whole ledger a
    /// suggestion.
    ///
    /// `web_route check` is deliberately not here: it is a probe of the operator's own exits, and
    /// charging it would make diagnosing a pool cost the thing the pool exists to protect.
    async fn charge_visit(
        &self,
        url: &str,
        tier: BrowserTier,
        proxy: Option<&str>,
    ) -> anyhow::Result<()> {
        let domain = domain_from_url(url);
        if is_local(&domain) {
            return Ok(());
        }
        self.admit_visit(&domain, proxy)?;
        self.pace_visit(&domain, proxy, tier.as_str()).await?;
        let left = self.pending_hold(&domain, proxy)?;
        anyhow::ensure!(left == 0, "cooldown: wait {left}s before retrying");
        Ok(())
    }

    async fn pace_visit(
        &self,
        domain: &str,
        proxy: Option<&str>,
        tier: &str,
    ) -> anyhow::Result<()> {
        let ledger = self
            .traffic
            .as_ref()
            .as_ref()
            .map_err(|e| anyhow::anyhow!("traffic ledger unavailable: {e}"))?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;
        let delay = ledger.pace(domain, proxy, now_ms, self.cfg.request_min_interval_ms)?;
        tokio::time::sleep(Duration::from_millis(delay)).await;
        throttle::throttle_at_least(
            domain,
            proxy,
            tier,
            Duration::from_millis(self.cfg.request_min_interval_ms),
        )
        .await;
        Ok(())
    }

    fn admit_visit(&self, domain: &str, proxy: Option<&str>) -> Result<(), VisitRefusal> {
        if let Some(left) = svipall_core::check_cooldown(domain) {
            return Err(VisitRefusal {
                kind: "cooldown",
                seconds: left,
                detail: "site backoff is active".into(),
            });
        }
        if let Some(r) = svipall_core::reputation::refusal(domain, proxy) {
            return Err(VisitRefusal {
                kind: "address_budget",
                seconds: r.seconds_left,
                detail: "address budget is exhausted".into(),
            });
        }
        let ledger = self.traffic.as_ref().as_ref().map_err(|e| VisitRefusal {
            kind: "traffic_state",
            seconds: 0,
            detail: e.to_string(),
        })?;
        if let Some(left) = ledger
            .reserve(domain, proxy, &self.cfg, svipall_core::automatic::now())
            .map_err(|e| VisitRefusal {
                kind: "traffic_state",
                seconds: 0,
                detail: e.to_string(),
            })?
        {
            return Err(VisitRefusal {
                kind: "cooldown",
                seconds: left,
                detail: "visit limit or server backoff applies to this site and exit".into(),
            });
        }
        Ok(())
    }

    fn pending_hold(&self, domain: &str, proxy: Option<&str>) -> anyhow::Result<u64> {
        let ledger = self
            .traffic
            .as_ref()
            .as_ref()
            .map_err(|e| anyhow::anyhow!("traffic ledger unavailable: {e}"))?;
        Ok(ledger
            .remaining(domain, proxy, svipall_core::automatic::now())?
            .max(svipall_core::throttle::cooldown_left(domain)))
    }

    /// The exit a domain leaves through now: one of its pool, or its single route, or none.
    fn exit_for(&self, domain: &str) -> Option<String> {
        svipall_core::exits::choose_for_fetch(
            domain,
            svipall_core::exits::Strategy::parse(&self.cfg.exit_strategy),
        )
    }

    /// The newest stable Chrome major the provisioner has learned, from the store.
    ///
    /// Read rather than fetched: a status call is not the place to reach the network, and a tool
    /// that phones a release channel to answer "how are you" has a network dependency it does not
    /// need. `browser_setup` writes it whenever it looks the channel up for its own reasons.
    pub(crate) fn latest_stable_major(&self) -> Option<u16> {
        self.store
            .as_ref()?
            .kv_get(LATEST_STABLE_KEY)
            .and_then(|v| v.trim().parse().ok())
    }

    fn remember_latest_stable(&self, version: &str) {
        if let (Some(store), Some(major)) = (
            self.store.as_deref(),
            crate::browser::major_of_public(version),
        ) {
            let _ = store.kv_set(LATEST_STABLE_KEY, &major.to_string());
        }
    }

    fn routes(&self) -> HashMap<String, String> {
        svipall_core::store::ROUTES.as_map()
    }

    fn is_html_type(content_type: &str) -> bool {
        content_type.is_empty() || content_type.contains("html") || content_type.contains("xhtml")
    }

    /// Everything the ladder will need from one document, decided before it is parsed so that a
    /// single parse can satisfy classification, the response body, the title and the crawl links.
    fn wants_for(
        p: &WebFetchParams,
        extraction_kind: &str,
        content_type: &str,
        final_url: &str,
        want_links: bool,
        memory: svipall_core::extraction::Fingerprints,
    ) -> ParseWants {
        // A schema request wants objects, not prose: skip markdown rendering entirely.
        let markdown = (Self::is_html_type(content_type)
            && extraction_kind == "markdown"
            && p.schema.is_none()
            && !p.tables.unwrap_or(false))
        .then(|| svipall_core::MarkdownOpts {
            main_content_only: p.main_content_only.unwrap_or(true),
            css_selector: p.css_selector.clone(),
            base_url: Some(final_url.to_string()),
            // The product runs the fitted defaults. The field exists so the benchmark can sweep
            // them; a tool parameter for it would ask the caller to tune an extractor.
            prune: None,
            vote: None,
            page_type: None,
        });
        // Compiled here, once per fetch. Errors are carried out rather than thrown: a mistyped
        // selector should name the offending field, not fail the whole request.
        let mut schema_errors = Vec::new();
        // `schema: "auto"` is the caller saying they have no selectors and would like the page to
        // supply them. Anything else is a schema they wrote.
        let induce_schema = p.schema.as_ref().and_then(|v| v.as_str()) == Some("auto");
        let schema = p.schema.as_ref().filter(|_| !induce_schema).and_then(|v| {
            match svipall_core::extraction::CompiledSchema::from_value(v) {
                Ok((c, errs)) => {
                    schema_errors.extend(errs);
                    Some(c.with_fingerprints(memory.clone()))
                }
                Err(e) => {
                    schema_errors.push(e);
                    None
                }
            }
        });
        SCHEMA_ERRORS.with(|c| *c.borrow_mut() = schema_errors);
        ParseWants {
            // Classification always needs the rendered text.
            text: true,
            title: true,
            markdown,
            links_base: want_links.then(|| final_url.to_string()),
            schema,
            induce_schema,
            schema_base_url: Some(final_url.to_string()),
            // The provenance observations read the byline and the publication date, so a caller
            // asking for the full quality breakdown needs the metadata whether or not they asked
            // for it separately. It is the same parse either way.
            metadata: p.include_metadata.unwrap_or(false) || p.include_quality.unwrap_or(false),
            metadata_base_url: Some(final_url.to_string()),
            links_detailed: (p.include_links.unwrap_or(false)
                || p.include_quality.unwrap_or(false))
            .then(|| final_url.to_string()),
            tables: p.tables.unwrap_or(false),
            // Always: the degree of optimisation is reported on every delivered page, and there is
            // no second parse to collect it from later.
            signals: true,
        }
    }

    fn render_content(
        parts: &PageParts,
        html: &str,
        extraction: &str,
        content_type: &str,
        query: Option<&String>,
    ) -> String {
        let content = if !Self::is_html_type(content_type) || extraction == "html" {
            html.to_string()
        } else if extraction == "text" {
            parts.text.clone()
        } else {
            parts.markdown.clone().unwrap_or_default()
        };
        match query {
            Some(q) if !q.trim().is_empty() => extraction::bm25_filter(&content, q, 40),
            _ => content,
        }
    }

    /// Apply the token budget and fold the paging fields into the response.
    ///
    /// Ordering matters: relevance filtering (BM25) has already happened in `render_content`, so
    /// the budget trims what is left rather than keeping whatever happened to come first.
    fn budget_into(&self, value: &mut Value, content: String, p: &WebFetchParams) {
        // Asked for a file: the content goes there whole, and what comes back is a path. No token
        // budget applies, because the budget exists to protect a context this is bypassing.
        if let Some(name) = p.out_file.as_deref().filter(|n| !n.trim().is_empty()) {
            let obj = value.as_object_mut().expect("tool results are objects");
            match write_out(name, &content) {
                Ok((path, bytes)) => {
                    obj.insert("out_file".into(), json!(path.to_string_lossy()));
                    obj.insert("bytes".into(), json!(bytes));
                    obj.insert("chars".into(), json!(content.chars().count()));
                }
                Err(e) => {
                    // Falling back to returning the content would defeat the request in the most
                    // expensive way possible, so say so and return nothing.
                    obj.insert("out_file_error".into(), json!(e.to_string()));
                }
            }
            return;
        }
        let max = p.max_tokens.unwrap_or(self.cfg.max_tokens_per_fetch).max(1);
        let out = budget::take(
            &content,
            &budget::BudgetOpts {
                max_tokens: max,
                cursor: p.cursor.as_deref().and_then(budget::Cursor::decode),
                overlap_blocks: self.cfg.overlap_blocks,
            },
        );
        let obj = value.as_object_mut().expect("tool results are objects");
        obj.insert("chars".into(), json!(out.content.chars().count()));
        obj.insert("tokens_estimated".into(), json!(out.tokens));
        obj.insert("content".into(), Value::String(out.content));
        if out.truncated {
            obj.insert("truncated".into(), json!(true));
            obj.insert("blocks_returned".into(), json!(out.blocks_returned));
            obj.insert("total_blocks".into(), json!(out.total_blocks));
            if let Some(c) = out.next_cursor {
                obj.insert("cursor".into(), json!(c));
                obj.insert(
                    "continue".into(),
                    json!("call web_fetch again with this cursor for the rest"),
                );
            }
        }
        if out.stale_cursor {
            obj.insert("stale_cursor".into(), json!(true));
            obj.insert(
                "note".into(),
                json!("the page changed since that cursor was issued; restarted from the top"),
            );
        }
    }

    // ---- tiers -----------------------------------------------------------------------

    /// Does this page look like what the site serves for an address that does not exist?
    ///
    /// Bar-Yossef et al. (WWW 2004): the only reliable way to recognise a soft 404 is to ask the
    /// site for something that certainly is not there and compare. The answer is kept per domain,
    /// so it costs one request per site ever — and it is only ever asked about a page that has
    /// already come back thin enough to be worth the question. Being a fingerprint rather than a
    /// phrase list, it works in every language, which is the half of this a vocabulary cannot do.
    async fn matches_the_sites_missing_page(
        &self,
        p: &WebFetchParams,
        domain: &str,
        final_url: &str,
        text: &str,
        proxy: Option<&str>,
    ) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        let key = format!("soft404/{domain}");
        let known = match store.kv_get(&key) {
            Some(v) => v,
            None => {
                let answer = self.probe_missing_page(p, final_url, proxy).await;
                let _ = store.kv_set(&key, &answer);
                answer
            }
        };
        // A site that answers 404 honestly has no fingerprint to compare against, and the stored
        // word says so rather than leaving us to probe it again on every thin page.
        let Ok(fingerprint) = known.parse::<u64>() else {
            return false;
        };
        svipall_core::dedup::similarity(fingerprint, svipall_core::dedup::simhash(text))
            >= svipall_core::quality::SOFT_404_SIMILARITY
    }

    /// Ask a site once for an address that is not there, and record what came back.
    ///
    /// `honest` when it answered with a status that means the page is missing, or when the request
    /// did not produce a page we could learn anything from — in both cases there is nothing to
    /// compare against, and recording that stops the question being asked again.
    async fn probe_missing_page(
        &self,
        p: &WebFetchParams,
        final_url: &str,
        proxy: Option<&str>,
    ) -> String {
        // Fixed rather than random, so the fingerprint is reproducible and a rerun of the probe
        // agrees with the stored one. Long and self-describing, so a site cannot plausibly have it.
        const NOWHERE: &str = "/svipall-probe-for-a-page-that-is-not-here-0f3a9c";
        let Some((scheme, rest)) = final_url.split_once("://") else {
            return "honest".into();
        };
        let host = rest.split('/').next().unwrap_or_default();
        if host.is_empty() {
            return "honest".into();
        }
        let probe = WebFetchParams {
            url: format!("{scheme}://{host}{NOWHERE}"),
            // The probe is one plain GET. It never carries the caller's method, body or headers,
            // and it never climbs: a wall on the probe is simply nothing learned.
            max_tier: Some("http".into()),
            timeout: p.timeout,
            ..Default::default()
        };
        match self.tier_http(&probe, proxy, None).await {
            Ok(o) if (200..300).contains(&o.status) => {
                let text = extraction::extract_text(&o.html);
                if text.trim().is_empty() {
                    return "honest".into();
                }
                svipall_core::dedup::simhash(&text).to_string()
            }
            _ => "honest".into(),
        }
    }

    /// Record what one h3 probe was worth, from whichever arm of the ladder saw the outcome.
    ///
    /// A method rather than two copies of the same five lines, because the two call sites are an
    /// error path and a success path and those are exactly the pair that drift apart.
    fn record_h3_probe(&self, probing: bool, tier: &str, domain: &str, delivered: bool) {
        if !probing || tier != "http" {
            return;
        }
        if let Some(store) = self.store.as_ref() {
            svipall_core::altsvc::remember_result(
                store,
                domain,
                delivered,
                chrono::Utc::now().timestamp(),
            );
        }
    }

    /// Everything this fetch needs to know about HTTP/3 for one domain, decided once.
    ///
    /// Two facts, and they are not the same. The site *advertised* h3 (`Alt-Svc`), and this
    /// machine has or has not *reached* it that way — a site can offer h3 while the network in
    /// between drops UDP on the floor. Keeping both is what stops a probe from being paid on every
    /// fetch for ever, and what lets the first attempt have a short budget and later ones a full
    /// one.
    fn h3_plan(&self, domain: &str, url: &str, proxy: Option<&str>) -> H3Plan {
        if !self.cfg.http3 || !svipall_http::http3_available() {
            return H3Plan::Off;
        }
        // A proxy is reached by CONNECT and QUIC does not go through one; an operator who set an
        // exit means it. And `Alt-Svc` was read off an https response, so a plaintext url has no
        // h3 to speak.
        if proxy.is_some() || !url.starts_with("https://") {
            return H3Plan::Off;
        }
        let Some(store) = self.store.as_ref() else {
            return H3Plan::Off;
        };
        let now = chrono::Utc::now().timestamp();
        if !svipall_core::altsvc::offers(store, domain, now) {
            return H3Plan::Off;
        }
        match svipall_core::altsvc::verdict(store, domain, now) {
            svipall_core::altsvc::Verdict::Works => H3Plan::Use,
            svipall_core::altsvc::Verdict::Untried => H3Plan::Probe,
            svipall_core::altsvc::Verdict::Fails => H3Plan::Off,
        }
    }

    /// The fetcher to use for this url, which is the h3 one only when a site has said it offers it.
    ///
    /// Three conditions, and all three matter. The operator turned it on; this binary was built
    /// with a QUIC stack in it; and *this domain advertised `Alt-Svc: h3` at some point and the
    /// advertisement has not expired*. That last one is Chrome's own rule — a browser never opens
    /// a first connection over QUIC — and it is what keeps a first visit indistinguishable from
    /// what svipall did before h3 existed.
    fn h3_or(
        &self,
        base: Arc<dyn HttpFetcher>,
        proxy: Option<&str>,
        url: &str,
    ) -> Arc<dyn HttpFetcher> {
        let domain = domain_from_url(url);
        let plan = self.h3_plan(&domain, url, proxy);
        if plan == H3Plan::Off {
            return base;
        }
        let mut cfg = FetcherConfig::new(base.identity().clone());
        // The full navigation budget, tried and untried alike. What bounds the bad case is not
        // this number but the handshake deadline inside the engine: a network that silently drops
        // UDP is caught in two seconds whatever the page budget says, and a slow large page over a
        // connection that *did* come up should not be punished for someone else's firewall.
        cfg.timeout = Duration::from_millis(self.cfg.browser_timeout_ms.max(1_000));
        tracing::debug!("h3: {domain} advertised it, going over QUIC ({plan:?})");
        svipall_http::with_http3(&cfg, base.clone()).unwrap_or(base)
    }
    async fn tier_http(
        &self,
        p: &WebFetchParams,
        proxy: Option<&str>,
        revalidate: Option<(Option<String>, Option<String>)>,
    ) -> anyhow::Result<TierOutcome> {
        let fetcher = self.h3_or(self.fetcher_for(proxy), proxy, &p.url);
        let mut req = HttpRequest {
            url: p.url.clone(),
            method: p.method.as_deref().unwrap_or("GET").to_uppercase(),
            // Chrome's navigation header set, in Chrome's order. The emulating engine keeps that
            // order on the wire; header order is part of the HTTP/2 fingerprint.
            headers: fetcher.identity().nav_headers(),
            body: None,
        };
        // Revalidate a stale copy instead of downloading it again. A 304 is a few hundred bytes
        // and no parsing at all, which is the whole reason the validators are stored.
        if let Some(validators) = revalidate {
            if let Some(etag) = validators.0 {
                req.set_header("if-none-match", &etag);
            }
            if let Some(lm) = validators.1 {
                req.set_header("if-modified-since", &lm);
            }
        }
        let mut has_ct = false;
        if let Some(h) = &p.headers {
            for (k, v) in h {
                if k.eq_ignore_ascii_case("content-type") {
                    has_ct = true;
                }
                req.set_header(k, v);
            }
        }
        if let Some(b) = &p.body {
            if !has_ct && b.trim_start().starts_with(['{', '[']) {
                req.set_header("content-type", "application/json");
            }
            req.body = Some(b.clone().into_bytes());
        }
        let r = if p.url.starts_with("raw:") || p.url.starts_with("file://") {
            Self::local_response(&p.url).await?
        } else {
            fetcher.send(req).await?
        };
        // Only decode to text once the type says it is text. `r.text()` on every response is what
        // used to mangle PDFs into lossy UTF-8 before anything could look at them.
        let html = if svipall_core::pdf::looks_like_pdf(&r.body, &r.content_type) {
            // CPU-bound, synchronous, and capable of panicking on a malformed file: it belongs on
            // a blocking thread behind a timeout, not on the reactor.
            let body = r.body.clone();
            let extracted = tokio::time::timeout(
                Duration::from_secs(30),
                tokio::task::spawn_blocking(move || {
                    svipall_core::pdf::extract(&body, &svipall_core::pdf::PdfLimits::default())
                }),
            )
            .await;
            match extracted {
                Ok(Ok(Ok(doc))) => {
                    let note = if doc.truncated {
                        format!(
                            "\n\n*(truncated at {} pages)*",
                            svipall_core::pdf::PdfLimits::default().max_pages
                        )
                    } else {
                        String::new()
                    };
                    format!("{}{}", doc.text, note)
                }
                Ok(Ok(Err(e))) => format!("Could not read this PDF: {e}"),
                Ok(Err(e)) => format!("PDF extraction task failed: {e}"),
                Err(_) => "PDF extraction timed out after 30s".to_string(),
            }
        } else if let Some(kind) =
            svipall_core::document::looks_like_document(&r.body, &r.content_type, &r.final_url)
        {
            // Same shape as the PDF path: off the reactor, behind a timeout, and every failure is
            // content the caller can read rather than an error that hides the document.
            let body = r.body.clone();
            let extracted = tokio::time::timeout(
                Duration::from_secs(30),
                tokio::task::spawn_blocking(move || {
                    svipall_core::document::extract(
                        &body,
                        kind,
                        &svipall_core::document::DocLimits::default(),
                    )
                }),
            )
            .await;
            match extracted {
                Ok(Ok(Ok(doc))) => doc.markdown,
                Ok(Ok(Err(e))) => format!("Could not read this {} document: {e}", kind.name()),
                Ok(Err(e)) => format!("Document conversion task failed: {e}"),
                Err(_) => "Document conversion timed out after 30s".to_string(),
            }
        } else if Self::is_html_type(&r.content_type) || r.content_type.contains("text") {
            r.text()
        } else {
            String::from_utf8_lossy(&r.body).into_owned()
        };
        // Extracted PDF or document text is already prose; telling the pipeline it is HTML would
        // send it through the markdown walker for nothing.
        let content_type = if svipall_core::pdf::looks_like_pdf(&r.body, &r.content_type)
            || svipall_core::document::looks_like_document(&r.body, &r.content_type, &r.final_url)
                .is_some()
        {
            "text/plain".to_string()
        } else {
            r.content_type
        };
        Ok(TierOutcome {
            status: r.status,
            html,
            final_url: r.final_url,
            content_type,
            scroll_rounds: None,
            cookies: svipall_core::classify::cookie_names(&r.headers),
            warm: None,
            kept_key: None,
            headers: r.headers,
        })
    }

    /// Arrive with a history instead of out of nowhere.
    ///
    /// A browser that opens `about:blank` and goes straight to a deep URL has no referrer, no
    /// back-history, no cookies the site set on its own front page and no record of ever having
    /// been there — a visitor who materialised on the article. The patient tiers are the ones
    /// facing the scoring panels, and they are also the ones that keep a profile per domain, so
    /// this is paid once: the first visit calls at the front door, reads for as long as a person
    /// would, and then follows the link. Later visits on the same profile skip it, because by then
    /// the history is real.
    ///
    /// Best effort throughout. A front page that will not load must not cost the page that was
    /// asked for.
    async fn warm_up(
        &self,
        page: &svipall_cdp::page::Page,
        tier: BrowserTier,
        url: &str,
        first_visit: bool,
    ) {
        // A profile that has been here before does not walk in again: that would be a visitor who
        // returns to the front page before every single article, which is its own pattern.
        if !first_visit || !matches!(tier, BrowserTier::Real | BrowserTier::Warm) {
            return;
        }
        let Ok(parsed) = url::Url::parse(url) else {
            return;
        };
        if parsed.host_str().is_none() {
            return;
        }
        let root = format!("{}/", parsed.origin().ascii_serialization());
        // Already the front page: there is nothing to arrive from.
        if parsed.path() == "/" && parsed.query().is_none() {
            return;
        }
        if self.pool.navigate(page, &root).await.is_err() {
            return;
        }
        // A glance, not a read: somebody who came for the deep link looks at the front page for a
        // couple of seconds and clicks. Anything longer is measured against the fetch it delays,
        // and `dwell` saturates at nine seconds for any page worth reading — far too much to spend
        // before the page that was actually asked for.
        let seed = self.pool.identity().noise_seed;
        self.pool.nudge(page).await;
        tokio::time::sleep(crate::behavior::dwell(300, seed)).await;
    }

    fn profile_dir_for(
        &self,
        tier: BrowserTier,
        url: &str,
        profile: Option<&str>,
    ) -> Option<PathBuf> {
        let dir = match tier {
            BrowserTier::Real | BrowserTier::Warm => Some(match profile {
                Some(name) => named_profile(name),
                None => PathBuf::from(svipall_core::auto_profile_path(url, false)),
            }),
            _ => profile.map(named_profile),
        };
        // Native and emulated sessions must never reuse each other's cookies or browser files.
        // Named profiles stay explicit; automatic fallback is disabled for them.
        let dir = if self.cfg.browser_identity == "native" && dir.is_none() {
            Some(PathBuf::from(svipall_core::auto_profile_path(url, false)))
        } else {
            dir
        };
        dir.map(|p| {
            if self.cfg.browser_identity == "native" && profile.is_none() {
                let mut path = p.into_os_string();
                path.push(".native");
                PathBuf::from(path)
            } else {
                p
            }
        })
    }

    /// A profile that exists only for one fetch.
    ///
    /// Under `sessions/`, which is the directory the pool already deletes when it is done with it,
    /// so isolation is a naming decision rather than a second lifetime to get wrong. A fresh id
    /// each time means a fresh browser: that is the cost of isolation, and it is the point of it.
    fn isolated_profile() -> PathBuf {
        crate::browser::sessions_dir().join(format!("once-{}", uuid::Uuid::new_v4().simple()))
    }

    async fn tier_browser(
        &self,
        tier: BrowserTier,
        url: &str,
        wants: BrowserWants<'_>,
    ) -> anyhow::Result<TierOutcome> {
        let BrowserWants {
            text_only,
            mobile,
            proxy,
            profile,
            scroll,
            isolated,
        } = wants;
        // An isolated fetch overrides every other choice, including a named profile: asking for
        // both is asking for two different things, and the safe reading of "isolated" is the one
        // that carries nothing in and leaves nothing behind.
        let profile_dir = if isolated {
            Some(Self::isolated_profile())
        } else {
            self.profile_dir_for(tier, url, profile)
        };
        // Isolation draws a fresh emulated identity and clears profile state. This reduces
        // linkability but cannot guarantee anonymity, especially with explicit native mode.
        let first_visit = profile_dir.as_ref().is_some_and(|d| !d.exists());
        let identity_seed = if isolated {
            Some(uuid::Uuid::new_v4().as_u128() as u64)
        } else {
            identity_seed_for(profile_dir.as_deref(), url, profile)
        };
        // Read before the browser is launched, because launching creates the directory: asked
        // afterwards, every profile looks like one that has been here before.
        let opts = PageOpts {
            mobile,
            tier,
            profile_dir: profile_dir.clone(),
            proxy,
            visible: false,
            identity_seed,
        };
        // A page held from an earlier fetch of this domain, when there is one. Never for an
        // isolated fetch or one wearing a borrowed machine: both promise to share nothing with
        // anything, and a shared tab is the opposite of that.
        let kept_key = (!isolated && !mobile)
            .then(|| BrowserPool::kept_key(&opts, &domain_from_url(url), text_only));
        let (pooled, page, reused) = match &kept_key {
            Some(k) => self.pool.warm_page(&opts, k).await?,
            None => {
                let (p, pg) = self.pool.page(&opts).await?;
                (p, pg, false)
            }
        };
        if text_only && !reused {
            // Asked for prose, so the pictures are pure cost. Best effort: a browser that refuses
            // the request should still fetch the page. A reused page already has the rule in place
            // — it is page state that survives navigation, and `kept_key` carries `text_only` so a
            // page with the wrong rule is never handed over.
            let _ = self.pool.block_heavy_resources(&page).await;
        }
        if self.cfg.block_ads {
            let list = self.ad_hosts().await;
            let _ = self.pool.block_tracking(&page, list).await;
        }
        let result = async {
            // A request through the existing document retains SDK closures and still reaches the
            // network (even for cache=bypass). A full Page.navigate would destroy that runtime.
            // Only accept a complete HTML response; shells, redirects and challenges fall back
            // to normal navigation so extraction never mistakes the old document for fresh data.
            if reused && scroll == 0 {
                if let Some(live) = self.pool.fetch_in_document(&page, url).await {
                    let parts = extraction::parse_page(&live.html, &ParseWants::text());
                    let cookies = self.pool.cookie_names(&page).await;
                    let view = PageView::new(&live.html, &parts.text).on_the_wire(&live.headers, &cookies);
                    if classify_view(live.status, &live.html, &view).0.is_none()
                        && parts.text.trim().len() >= 200
                    {
                        return Ok((live.status, live.html, live.url, None, live.headers, cookies,
                            Some(json!({"ended":"cleared","document_reused":true,"network_fetch":true})), Some(true)));
                    }
                }
            }
            self.warm_up(&page, tier, url, first_visit).await;
            // After the front door, whose headers are not this page's; before navigating, because
            // the document's response is the first thing back and a later listener has missed it.
            let mut watch = crate::wire::DocumentWatch::start(&page).await.ok();
            let mut status = self.pool.navigate(&page, url).await?;
            if let Some(w) = watch.as_mut() {
                w.drain();
            }
            if self.cfg.block_ads {
                // Before the HTML is read, not after: a consent overlay that is still up when the
                // document is taken becomes part of the extracted text.
                let hidden = self.pool.hide_consent(&page).await;
                if hidden > 0 {
                    tracing::debug!(count = hidden, "removed consent overlays");
                }
            }
            // Filled only by the warm tier: what the wait did, and why it stopped.
            let mut warm: Option<Value> = None;
            let mut scroll_rounds = None;
            if scroll > 0 {
                let deadline = Instant::now() + Duration::from_millis(self.cfg.browser_timeout_ms);
                match self.pool.scroll_until_stable(&page, scroll, deadline).await {
                    Ok((rounds, _)) => scroll_rounds = Some(rounds),
                    Err(e) => tracing::debug!(error = %e, "scrolling stopped early"),
                }
            }
            let (mut html, mut final_url) = self.pool.content(&page).await?;
            // What the vendor said on a channel the body does not carry. Read beside the document
            // so the two always describe the same moment.
            let mut headers: Vec<(String, String)> = watch
                .as_ref()
                .map(|w| w.headers().to_vec())
                .unwrap_or_default();
            let mut cookies = self.pool.cookie_names(&page).await;

            // One parse feeds both the "did we actually get the page" check and the wall
            // classification. Previously each of those parsed the document for itself, so a warm
            // wait ran three parses of a large page every 1.5s.
            let look = |h: &str, headers: &[(String, String)], cookies: &[String]| {
                let parts = extraction::parse_page(h, &ParseWants::text());
                let view = PageView::new(h, &parts.text).on_the_wire(headers, cookies);
                let delivered = {
                    let (r, k) = classify_view(200, h, &view);
                    r.is_none() || (k == WallKind::Empty && !parts.text.trim().is_empty())
                };
                (delivered, view.text.is_empty())
            };
            // A challenge page that already resolved itself leaves real content behind a 403, and
            // the status Chromium reports may belong to a sub-resource: rendered text wins.
            if status >= 400 && look(&html, &headers, &cookies).0 {
                status = 200;
            }
            if tier == BrowserTier::Warm {
                let initial_view = PageView::new(&html, "").on_the_wire(&headers, &cookies);
                let warm_budget = svipall_core::warm::wait_budget_ms(
                    self.cfg.warm_wait_ms, self.cfg.warm_max_wait_ms, self.cfg.warm_adaptive,
                    svipall_core::classify::is_proof_of_work_wall(&initial_view));
                let wait_started = Instant::now();
                let hard_deadline = wait_started + Duration::from_millis(self.cfg.warm_max_wait_ms);
                let mut deadline = wait_started + Duration::from_millis(warm_budget);
                let mut extended = false;
                // The proof-of-work vendor's clearance expires while the page sits there, so a
                // warm wait long enough to be useful is also long enough to lose it. This is when
                // it was last earned, and whether it has already been re-earned in this wait.
                let mut cleared_at = Instant::now();
                let mut reissued = false;
                // The strategies that answer challenges used to run only from `solve_and_continue`;
                // a plain fetch nudged the page and hoped. Now every turn of the wait is a turn of
                // the strategy loop, so a press-and-hold, a hash puzzle or an audio clip is
                // answered here rather than reported as a wall the caller has to come back for.
                let engine = match &self.solver_state {
                    Some(st) => SolveEngine::with_state(self.pool.clone(), &self.cfg, st.clone()),
                    None => SolveEngine::new(self.pool.clone(), &self.cfg),
                };
                let started = Instant::now();
                let mut iterations = 0u32;
                // What the page looked like just before a reissue, so the log can say whether
                // re-earning the clearance achieved anything. The baseline cannot answer that.
                let mut before_reissue: Option<(WallKind, usize)> = None;
                let mut reissue_changed = false;
                // The wait leaves by exactly one door, carrying why it left and what it was
                // looking at — so the log line cannot describe a different exit than the one taken.
                let (ending, last_kind) = loop {
                    iterations += 1;
                    let parts = extraction::parse_page(&html, &ParseWants::text());
                    let view = PageView::new(&html, &parts.text).on_the_wire(&headers, &cookies);
                    let (reason, kind) = classify_view(status, &html, &view);
                    if let Some((k, len)) = before_reissue.take() {
                        reissue_changed = k != kind || html.len() != len;
                    }
                    // A wall that names this machine's address cannot be solved on the page.
                    // Waiting the full budget on it is twenty seconds spent learning nothing. The
                    // words are inside the vendor's frame, in another process; the verdict is in the
                    // top document.
                    let blamed = svipall_core::wall_blames_the_address(view.low_text())
                        || (kind == WallKind::Vendor
                            && svipall_core::wall_is_hard_block(view.low_html()));
                    // One decision, one exit. Measured: a managed challenge that reads
                    // "verification successful, waiting for the site to respond" at the deadline is
                    // a pass already earned, and giving up then throws it away — so the deadline
                    // moves once, and only when the page itself says so.
                    let progress = svipall_core::challenge_reports_progress(view.low_text())
                        && (!self.cfg.warm_adaptive || Instant::now() < hard_deadline);
                    let past = Instant::now() > deadline;
                    if let Some(end) =
                        warm_should_stop(reason.is_none(), &kind, blamed, past, extended, progress)
                    {
                        break (end, kind);
                    }
                    if past && !extended {
                        extended = true;
                        deadline = if self.cfg.warm_adaptive {
                            (Instant::now() + Duration::from_secs(15)).min(hard_deadline)
                        } else {
                            Instant::now() + Duration::from_secs(15)
                        };
                    }
                    // One short turn: probe, run what applies, nudge if nothing does. Bounded so
                    // the classifier above looks at the page again soon after.
                    let left = deadline.saturating_duration_since(Instant::now());
                    let turn = Duration::from_secs(4).min(left.max(Duration::from_millis(500)));
                    // Three kinds of turn. Something to answer: answer it. An interstitial that
                    // is verifying on its own: do nothing, the way a person reading "Just a
                    // moment" does nothing — pointer activity there is the script's tell, not the
                    // person's. Anything else: the small idle activity that lets a widget see a
                    // visitor.
                    let actionable = engine.probe_only(&page).await.is_some();
                    // The proof-of-work vendor issues a clearance that lapses in about a minute.
                    // Nothing on the page says so and there is nothing to answer, so waiting the
                    // way we wait for an interstitial spends the whole budget and then 403s. A
                    // re-navigation re-runs its script and earns a fresh token, which is the one
                    // thing a stateless client cannot do at all.
                    // Once, and only once. Measured: on a page that never clears, the script is on
                    // every response, so "the token is stale" stays true forever and a loop here
                    // turned a 28-second failure into a 90-second timeout. One fresh attempt is
                    // worth having; the second is the page telling us the answer is no.
                    if !reissued
                        && !actionable
                        && svipall_core::classify::warm_needs_reissue(
                            &view,
                            cleared_at.elapsed().as_secs(),
                        )
                    {
                        tracing::info!(
                            domain = %domain_from_url(url),
                            age_secs = cleared_at.elapsed().as_secs(),
                            "proof-of-work clearance is stale; re-earning it once"
                        );
                        reissued = true;
                        before_reissue = Some((kind.clone(), html.len()));
                        let _ = self.pool.navigate(&page, url).await;
                        self.pool.settle(&page, Duration::from_millis(1500)).await;
                        cleared_at = Instant::now();
                    } else if !actionable
                        && svipall_core::challenge_is_self_verifying(view.low_text())
                    {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                    } else if let Err(e) = engine.wait_and_submit(&page, turn).await {
                        tracing::debug!("warm attempt: {e}");
                    }
                    let (h, u) = self.pool.content(&page).await?;
                    html = h;
                    final_url = u;
                    // Beside the document read, so a re-navigation's headers replace the ones that
                    // came with the page it replaced.
                    if let Some(w) = watch.as_mut() {
                        w.drain();
                        headers = w.headers().to_vec();
                    }
                    cookies = self.pool.cookie_names(&page).await;
                    if look(&html, &headers, &cookies).0 {
                        status = 200;
                    }
                };
                // Once, on the way out. Everything an operator needs to tell a pass from a timeout
                // from a refusal, and — the question the baseline could never answer — whether
                // re-earning the clearance changed anything at all.
                tracing::info!(
                    domain = %domain_from_url(url),
                    ended = ending.as_str(),
                    iterations,
                    secs = started.elapsed().as_secs_f32(),
                    reissued,
                    reissue_changed,
                    wall = ?last_kind,
                    "warm wait ended"
                );
                warm = Some(json!({
                    "ended": ending.as_str(),
                    "iterations": iterations,
                    "secs": (started.elapsed().as_secs_f32() * 10.0).round() / 10.0,
                    "reissued": reissued,
                    "reissue_changed": reissue_changed,
                    "budget_ms": warm_budget,
                    "adaptive": self.cfg.warm_adaptive,
                }));
            }
            // Is this page worth holding open between fetches? The runtime-clearance test comes
            // first and costs no parse — it reads the head of the markup, the response headers and
            // the cookie names. Only when that says yes, which for nearly every domain it does not,
            // is the page classified to see whether it actually cleared. So the extra parse happens
            // on the pages the feature exists for and nowhere else.
            let keep = {
                let cheap = PageView::new(&html, "").on_the_wire(&headers, &cookies);
                svipall_core::clearance_lives_in_the_runtime(&cheap)
                    .then(|| look(&html, &headers, &cookies).0)
            };
            Ok::<_, anyhow::Error>((
                status,
                html,
                final_url,
                scroll_rounds,
                headers,
                cookies,
                warm,
                keep,
            ))
        }
        .await;
        // Park it or close it — read off `result` rather than after `?`, so an error cannot carry
        // the page away unclosed, and before the isolated cleanup below. The decision itself is
        // `warm::should_keep`, which is where it is tested; nothing is re-decided here.
        let runtime_clearance = result.as_ref().ok().and_then(|r| r.7);
        let worth_keeping = svipall_core::warm::should_keep(
            true,
            isolated,
            mobile,
            runtime_clearance.unwrap_or(false),
            runtime_clearance.is_some(),
        );
        let kept_key = kept_key.filter(|_| worth_keeping);
        match &kept_key {
            Some(k) => self.pool.keep_page(k, pooled, page).await,
            None => self.pool.close_page(page).await,
        }
        if isolated {
            // The internally generated once-profile owns a dedicated browser. Close that process
            // before removing its directory; an open pooled browser keeps it locked on Windows.
            // Retirement already bounds the wait for the process to release its files.
            if let Some(dir) = &profile_dir {
                if !self.pool.retire_profile(dir).await {
                    tracing::warn!(
                        "could not remove isolated browser profile {}",
                        dir.display()
                    );
                }
            }
        }
        let (status, html, final_url, scroll_rounds, headers, cookies, warm, _) = result?;
        Ok(TierOutcome {
            status,
            html,
            final_url,
            content_type: "text/html".into(),
            scroll_rounds,
            headers,
            cookies,
            warm,
            kept_key,
        })
    }

    // ---- ladder ----------------------------------------------------------------------

    pub async fn fetch_json(&self, p: WebFetchParams) -> FetchOutcome {
        self.fetch_json_opts(p, false).await
    }

    /// `want_links` folds link collection into the same single parse, so the crawler never has to
    /// re-parse a page the ladder already parsed.
    ///
    /// One choke point, so the request log records every path through the ladder rather than the
    /// ones somebody remembered to instrument.
    pub async fn fetch_json_opts(&self, p: WebFetchParams, want_links: bool) -> FetchOutcome {
        let started = std::time::Instant::now();
        let mut out = self.fetch_json_inner(p, want_links).await;
        if out.value["identity_used"] == "native" || out.value["native_fallback"] == true {
            out.value["privacy_notice"] = json!("Native mode exposes real browser and device characteristics. Separate cookies do not prevent fingerprint linking; the network exit is unchanged.");
        }
        if let Some(store) = self.store.as_ref() {
            let wall = out.value["wall_kind"].as_str().unwrap_or("");
            // A URL the policy refused was never requested, so it is not a request.
            if wall != "policy" {
                store.log_request(
                    out.value["url"].as_str().unwrap_or_default(),
                    out.value["tier_used"].as_str().unwrap_or("-"),
                    out.value["status"].as_u64().unwrap_or(0) as u16,
                    (!wall.is_empty() && wall != "none").then_some(wall),
                    out.value["blocked_reason"].is_string(),
                    started.elapsed().as_millis() as u64,
                    out.value["exit"].as_str(),
                );
            }
        }
        out
    }

    async fn fetch_json_inner(&self, p: WebFetchParams, want_links: bool) -> FetchOutcome {
        let timeout = p.timeout.unwrap_or(60_000);
        let url = p.url.clone();

        // The operator's own rule about where this machine may go, checked before a request is
        // made rather than after one comes back. Inert unless configured.
        if let Err(why) = self.origin_policy().check(&url) {
            return FetchOutcome {
                value: json!({
                    "url": url,
                    "blocked_reason": why.to_string(),
                    "wall_kind": "policy",
                    "note": "This origin is refused by this installation's configuration \
                             (allow_origins / block_origins / refuse_private_addresses in \
                             ~/.svipall/config.toml). Nothing was requested.",
                }),
                links: Vec::new(),
                final_url: url,
            };
        }

        // A URL a person named is retrieval they asked for, so the default is to report what
        // robots.txt says rather than to refuse. `obey` has to be asked for, and then it refuses
        // before spending a request.
        let policy = p
            .robots
            .as_deref()
            .and_then(svipall_core::RobotsPolicy::parse)
            .unwrap_or(svipall_core::RobotsPolicy::Warn);
        let disallowed = match policy {
            svipall_core::RobotsPolicy::Ignore => false,
            svipall_core::RobotsPolicy::Obey => self
                .robots_for(&url)
                .await
                .is_some_and(|r| !self.robots_allows(&url, &r)),
            svipall_core::RobotsPolicy::Warn => self
                .robots_cached(&url)
                .is_some_and(|r| !self.robots_allows(&url, &r)),
        };
        if disallowed && policy == svipall_core::RobotsPolicy::Obey {
            return FetchOutcome {
                value: json!({
                    "url": url, "status": 0,
                    "blocked_reason": "robots.txt disallows this URL",
                    "wall_kind": "robots", "robots_disallowed": true,
                    "note": "Pass robots=\"warn\" to fetch it anyway and be told, or robots=\"ignore\" to say nothing.",
                    "attempts": [],
                }),
                links: Vec::new(),
                final_url: url,
            };
        }
        let annotate = |mut o: FetchOutcome| {
            if disallowed {
                if let Some(obj) = o.value.as_object_mut() {
                    obj.insert("robots_disallowed".into(), json!(true));
                }
            }
            o
        };
        match tokio::time::timeout(
            Duration::from_millis(timeout),
            self.fetch_inner(&p, want_links),
        )
        .await
        {
            Ok(o) => annotate(o),
            Err(_) => FetchOutcome {
                value: json!({"url": url, "status": 0, "blocked_reason": "timeout", "wall_kind": "timeout", "note": format!("No tier answered within {}ms. Raise timeout or lower max_tier.", timeout), "attempts": []}),
                links: Vec::new(),
                final_url: url,
            },
        }
    }

    async fn fetch_inner(&self, p: &WebFetchParams, want_links: bool) -> FetchOutcome {
        let fetch_started = Instant::now();
        let url = p.url.clone();
        let cache_mode = p
            .cache
            .as_deref()
            .and_then(CacheMode::parse)
            .unwrap_or(CacheMode::ReadWrite);

        // Validators from a stale copy, so the http tier can ask "still the same?" instead of
        // downloading the page again.
        let mut revalidate: Option<(Option<String>, Option<String>)> = None;
        let mut stale_copy: Option<CachedPage> = None;

        // A fresh cached copy short-circuits the whole ladder. This is also what makes a cursor
        // continuation free: page two of a long document is a cache hit, not a second download.
        if let Some((hit, fresh)) = self.cache_lookup(p, cache_mode) {
            if !fresh && (hit.etag.is_some() || hit.last_modified.is_some()) {
                revalidate = Some((hit.etag.clone(), hit.last_modified.clone()));
                stale_copy = Some(hit.clone());
            }
            if fresh && p.schema.is_none() && !p.tables.unwrap_or(false) {
                let mut value = json!({
                    "url": url, "final_url": hit.final_url, "status": hit.status,
                    "tier_used": hit.tier, "title": hit.title,
                    "attempts": [format!("cache: hit, {}s old", hit.age_secs())],
                    "from_cache": true,
                });
                let obj = value.as_object_mut().expect("object");
                if let Some(q) = stored_quality(hit.quality.as_deref()) {
                    insert_quality(obj, &q);
                }
                // The same view of the same page a fresh fetch would give. Applied here too, or a
                // cache hit would answer with furniture the fetch beside it had taken off.
                let template =
                    site_template(self.store.as_deref(), &domain_from_url(&hit.final_url));
                let markdown = apply_template(
                    obj,
                    &template,
                    &hit.markdown,
                    p.use_site_template.unwrap_or(false),
                );
                let content = match &p.query {
                    Some(q) if !q.trim().is_empty() => extraction::bm25_filter(&markdown, q, 40),
                    _ => markdown,
                };
                self.budget_into(&mut value, content, p);
                return FetchOutcome {
                    value,
                    links: Vec::new(),
                    final_url: hit.final_url,
                };
            }
        }
        let mode = p.mode.as_deref().unwrap_or("auto");
        let extraction_kind = p.extraction.as_deref().unwrap_or("markdown");
        let domain = domain_from_url(&url);
        let local =
            is_local(&domain) || !(url.starts_with("http://") || url.starts_with("https://"));
        let proxy = p
            .proxy
            .clone()
            .or_else(|| if local { None } else { self.exit_for(&domain) });
        let max_tier = p
            .max_tier
            .clone()
            .unwrap_or_else(|| self.cfg.max_tier.clone());

        if !local {
            if let Some(left) = svipall_core::check_cooldown(&domain) {
                return FetchOutcome {
                    value: json!({"url": url, "status": 0, "blocked_reason": "cooldown", "wall_kind": "status", "cooldown_seconds_left": left, "attempts": [],
                        "note": format!("This site is cooling down. Wait {left}s before trying again; changing identity does not reset the limit.")}),
                    links: Vec::new(),
                    final_url: url,
                };
            }
        }

        // What this address has already spent with this host. Checked after the cooldown, because
        // if the site has said no its word outranks our own accounting; and not conditioned on the
        // mode, because the budget is about the address rather than the ladder and a mode
        // parameter that switched it off would be a documented bypass. A local host never has an
        // entry, since nothing charges one.
        if let Some(r) = svipall_core::reputation::refusal(&domain, proxy.as_deref()) {
            return FetchOutcome {
                value: json!({"url": url, "status": 0, "blocked_reason": "address_budget",
                    "wall_kind": "reputation", "reputation_seconds_left": r.seconds_left,
                    "reputation_spent": (r.spent * 10.0).round() / 10.0, "reputation_budget": r.budget,
                    "attempts": [],
                    "note": format!("This address has spent its standing with {} ({:.0} of {:.0}); \
                                     nothing was requested. It falls back under the line in {}s. \
                                     Use web_route with a proxy, or web_status(clear_budget=\"{}\").",
                                    domain, r.spent, r.budget, r.seconds_left, domain)}),
                links: Vec::new(),
                final_url: url,
            };
        }

        // One http attempt in front of a learned tier when the domain advertises h3: what was
        // learned was learned over TCP, and QUIC is a different request rather than a repeat.
        let h3_probe = !local && self.h3_plan(&domain, &url, proxy.as_deref()) != H3Plan::Off;
        let mut tiers: Vec<String> =
            if local && (mode == "auto" || url.starts_with("raw:") || url.starts_with("file://")) {
                vec!["http".to_string()]
            } else {
                svipall_core::build_ladder(
                    mode,
                    &max_tier,
                    if self.cfg.browser_identity == "auto" {
                        ""
                    } else {
                        &domain
                    },
                    h3_probe,
                )
            };
        // Scrolling is something only a browser can do, so a fetch that asks for it never starts
        // at the http tier: the answer would be the first screen, which is what the caller is
        // trying not to get.
        if scroll_rounds(p) > 0 && !(url.starts_with("raw:") || url.starts_with("file://")) {
            tiers.retain(|t| t != "http");
            if tiers.is_empty() {
                tiers.push("browser".to_string());
            }
        }
        // A saved profile only matters inside a browser: start at `real` right away.
        if mode == "auto" && p.profile.is_some() && !local {
            let j = jump(&tiers, 0, "real");
            if j < tiers.len() {
                tiers.drain(..j);
            }
        }
        if p.method
            .as_deref()
            .map(|m| !m.eq_ignore_ascii_case("GET"))
            .unwrap_or(false)
            || p.body.is_some()
        {
            tiers = vec!["http".to_string()];
        }

        let automatic = mode == "auto" && self.cfg.browser_identity == "auto" && !local;
        let route_context = svipall_core::automatic::context(
            &url,
            proxy.as_deref(),
            &format!(
                "{:?}|{:?}|{}|{:?}|{}|{}",
                self.pool.browser_major(),
                self.identity,
                p.mobile.unwrap_or(false),
                p.profile,
                p.isolated.unwrap_or(false),
                scroll_rounds(p)
            ),
        );
        if automatic {
            let native_allowed = self.cfg.auto_native_fallback
                && p.profile.is_none()
                && !p.isolated.unwrap_or(false)
                && !p.mobile.unwrap_or(false)
                && p.body.is_none()
                && p.method
                    .as_deref()
                    .is_none_or(|m| m.eq_ignore_ascii_case("GET"));
            tiers = svipall_core::automatic::plan(
                &tiers,
                &svipall_core::automatic::load(&route_context),
                svipall_core::automatic::now(),
                native_allowed,
            );
        }
        let mut attempts: Vec<String> = Vec::new();
        let mut made = 0;
        let mut native_attempted = false;
        let mut stopped: Option<String> = None;
        let mut stopped_wait = 0;
        let mut stopped_kind = "error";
        let mut last_identity = if self.cfg.browser_identity == "native" {
            "native"
        } else {
            "emulated"
        };
        let mut last: Option<(TierOutcome, PageParts, String, WallKind, String)> = None;
        // Set when a persistent profile has been refused by a wall that judges the session, for
        // the one retry on a profile nobody has seen. See the note where it is set.
        let mut retire_profile = false;
        let mut i = 0;
        while i < tiers.len() {
            if mode == "auto" && made >= self.cfg.auto_max_attempts {
                stopped = Some(format!(
                    "attempt_limit: stopped after {made} transport attempts"
                ));
                stopped_kind = "attempt_limit";
                break;
            }
            let route = tiers[i].clone();
            let native = route.starts_with("native:");
            let tier = route.strip_prefix("native:").unwrap_or(&route).to_string();
            let identity_mode = if native || self.cfg.browser_identity == "native" {
                "native"
            } else {
                "emulated"
            };
            let attempt_pool = if native {
                &self.native_pool
            } else {
                &self.pool
            };
            let bt = BrowserTier::parse(&tier);
            if bt.is_some() && !attempt_pool.available() {
                attempts.push(format!("{}: SKIP {}", tier, no_browser_hint()));
                break;
            }
            if !local {
                if let Err(e) = self.admit_visit(&domain, proxy.as_deref()) {
                    stopped_wait = e.seconds;
                    stopped_kind = e.kind;
                    stopped = Some(e.to_string());
                    break;
                }
                let pacing_budget = Duration::from_millis(p.timeout.unwrap_or(60_000))
                    .saturating_sub(fetch_started.elapsed())
                    .saturating_sub(Duration::from_millis(100));
                match tokio::time::timeout(
                    pacing_budget,
                    self.pace_visit(&domain, proxy.as_deref(), &tier),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        stopped_kind = "traffic_state";
                        stopped = Some(e.to_string());
                        break;
                    }
                    Err(_) => {
                        stopped_kind = "timeout";
                        stopped = Some("timeout: pacing would exceed the fetch deadline; no further request was sent".into());
                        break;
                    }
                }
                match self.pending_hold(&domain, proxy.as_deref()) {
                    Ok(0) => {}
                    Ok(left) => {
                        stopped_wait = left;
                        stopped_kind = "cooldown";
                        stopped = Some(format!(
                            "cooldown: wait {left}s; a concurrent visit triggered backoff"
                        ));
                        break;
                    }
                    Err(e) => {
                        stopped_kind = "traffic_state";
                        stopped = Some(e.to_string());
                        break;
                    }
                }
            }
            let t0 = Instant::now();
            let remaining = Duration::from_millis(p.timeout.unwrap_or(60_000))
                .saturating_sub(fetch_started.elapsed())
                .saturating_sub(Duration::from_millis(100));
            if remaining.is_zero() {
                stopped = Some("timeout: no time remains for another route".into());
                stopped_kind = "timeout";
                break;
            }
            made += 1;
            if native {
                attempts.push(
                    "native: last-resort fallback; real browser characteristics are exposed".into(),
                );
            }
            native_attempted |= native;
            let outcome = tokio::time::timeout(remaining, async {
                match bt {
                    None => self.tier_http(p, proxy.as_deref(), revalidate.take()).await,
                    Some(b) => {
                        let mut runner = self.clone();
                        if native {
                            runner.pool = self.native_pool.clone();
                            let mut cfg = self.cfg.as_ref().clone();
                            cfg.browser_identity = "native".into();
                            runner.cfg = Arc::new(cfg);
                        }
                        runner
                            .tier_browser(
                                b,
                                &url,
                                BrowserWants {
                                    text_only: p.text_only.unwrap_or(false),
                                    mobile: p.mobile.unwrap_or(false),
                                    proxy: proxy.clone(),
                                    profile: p.profile.as_deref(),
                                    scroll: scroll_rounds(p),
                                    isolated: p.isolated.unwrap_or(false) || retire_profile,
                                },
                            )
                            .await
                    }
                }
            })
            .await;
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(_) => {
                    stopped = Some(format!(
                        "timeout: {route} exhausted the remaining fetch time"
                    ));
                    if automatic {
                        svipall_core::automatic::record(
                            &route_context,
                            &route,
                            svipall_core::automatic::Feedback::Failed,
                            t0.elapsed().as_millis() as u64,
                        );
                    }
                    stopped_kind = "timeout";
                    break;
                }
            };
            let ms = t0.elapsed().as_millis();
            let o = match outcome {
                Ok(o) => o,
                Err(e) => {
                    attempts.push(exc_attempt(&route, &e, ms));
                    if automatic {
                        svipall_core::automatic::record(
                            &route_context,
                            &route,
                            svipall_core::automatic::Feedback::Failed,
                            ms as u64,
                        );
                    }
                    // A probe that threw is a probe that did not deliver, and it has to be
                    // remembered as one. Without this the `continue` below skips the only place
                    // that records it, and the same wasted attempt is paid on every future fetch
                    // of this domain — which is the exact cost the memory exists to remove.
                    self.record_h3_probe(h3_probe, &tier, &domain, false);
                    i += 1;
                    continue;
                }
            };
            last_identity = if bt.is_none() {
                "emulated"
            } else {
                identity_mode
            };
            let rate_limited = matches!(o.status, 429 | 503);
            if !local && rate_limited {
                let wait = o
                    .header("retry-after")
                    .and_then(throttle::parse_retry_after)
                    .map(|d| d.as_secs())
                    .unwrap_or(self.cfg.request_cooldown_seconds)
                    .max(self.cfg.request_cooldown_seconds);
                if let Ok(ledger) = self.traffic.as_ref() {
                    if let Err(e) = ledger.hold(
                        &domain,
                        proxy.as_deref(),
                        svipall_core::automatic::now().saturating_add(wait),
                    ) {
                        tracing::warn!("cannot persist server backoff: {e}");
                        svipall_core::set_cooldown(&domain);
                    }
                }
                stopped = Some(format!("cooldown: server requested backoff; wait {wait}s"));
                stopped_wait = wait;
                stopped_kind = "cooldown";
            }
            // Feed the pace: this is what lets a fast host be crawled quickly and a hostile one
            // be backed off, instead of every domain paying the same fixed gap.
            if !local {
                let latency = Duration::from_millis(ms as u64);
                throttle::observe(
                    &domain,
                    proxy.as_deref(),
                    &tier,
                    match o.status {
                        429 | 503 => throttle::Outcome::RateLimited {
                            retry_after: o
                                .header("retry-after")
                                .and_then(throttle::parse_retry_after),
                        },
                        401 | 403 | 407 => throttle::Outcome::Blocked,
                        _ => throttle::Outcome::Ok { latency },
                    },
                );
                // Only the statuses that are a verdict on their own are recorded here. A 2xx/3xx
                // is not: what it really means is decided after the page is classified, below,
                // and recording an Ok here as well would count a quiet block as one success and
                // one failure of the same exit.
                if let Some(exit) = proxy.as_deref() {
                    let v = match o.status {
                        429 | 503 => Some(svipall_core::session::Verdict::RateLimited),
                        401 | 403 | 407 => Some(svipall_core::session::Verdict::Blocked),
                        _ => None,
                    };
                    if let Some(v) = v {
                        svipall_core::exits::record(&domain, exit, v, ms as u32);
                    }
                }
            }
            // 304 Not Modified: the stored copy is still current, so serve it and push its expiry
            // out. This is the cheapest possible successful fetch.
            if o.status == 304 {
                if let Some(cached) = stale_copy.take() {
                    if let Some(store) = &self.store {
                        let _ = store.touch(&url, 3600);
                    }
                    attempts.push(format!("{}: 304 not modified ({}ms)", tier, ms));
                    let mut value = json!({
                        "url": url, "final_url": cached.final_url, "status": cached.status,
                        "tier_used": tier, "title": cached.title, "attempts": attempts,
                        "from_cache": true, "revalidated": true,
                    });
                    let obj = value.as_object_mut().expect("object");
                    if let Some(q) = stored_quality(cached.quality.as_deref()) {
                        insert_quality(obj, &q);
                    }
                    let template =
                        site_template(self.store.as_deref(), &domain_from_url(&cached.final_url));
                    let markdown = apply_template(
                        obj,
                        &template,
                        &cached.markdown,
                        p.use_site_template.unwrap_or(false),
                    );
                    let content = match &p.query {
                        Some(q) if !q.trim().is_empty() => {
                            extraction::bm25_filter(&markdown, q, 40)
                        }
                        _ => markdown,
                    };
                    self.budget_into(&mut value, content, p);
                    return FetchOutcome {
                        value,
                        links: Vec::new(),
                        final_url: cached.final_url,
                    };
                }
            }

            // The one parse per tier attempt. Everything below reads from `parts`.
            let wants = Self::wants_for(
                p,
                extraction_kind,
                &o.content_type,
                &o.final_url,
                want_links,
                self.selector_memory(p, &o.final_url),
            );
            let parts = extraction::parse_page(&o.html, &wants);
            let view = PageView::new(&o.html, &parts.text).on_the_wire(&o.headers, &o.cookies);
            let (mut reason, mut kind) = classify_view(o.status, &o.html, &view);
            // `Empty` means "needs JavaScript to render". Once a browser tier has executed the
            // page and it is still short, that is simply a short page — deliver it.
            if kind == WallKind::Empty
                && bt.is_some()
                && (200..300).contains(&o.status)
                && !parts.text.trim().is_empty()
            {
                reason = None;
            }

            // How much of the page arrived. Judged here rather than where the response is built,
            // because the answer decides whether the site is worth asking what its missing-page
            // template looks like: a page that came back whole is never worth the extra request.
            // ▲ What the rest of this site looks like, from the pages of it already on disk. This
            // is what makes `MostlyBoilerplate` a reason `assess` can reach rather than one the
            // crawl bolted on afterwards against a different text — and it now fires on a lone
            // `web_fetch` too, because the record outlives the crawl that built it.
            let template = site_template(self.store.as_deref(), &domain);
            let mut evidence = svipall_core::quality::Evidence::new(o.html.len(), &parts.text)
                .between(&url, &o.final_url);
            if let Some(share) = parts.markdown.as_deref().and_then(|md| template.share(md)) {
                evidence = evidence.against_site(share);
            }
            // ▲ Judge the page the caller receives. The ratio rule keeps reading the whole
            // document — it is a ratio against the markup — and everything else now reads the
            // pruned text, so a page whose article is three lines inside a thousand words of
            // navigation is no longer called whole on the navigation.
            if let Some(d) = parts.delivered.as_deref() {
                evidence = evidence.delivered(d);
            }
            let integrity = svipall_core::quality::assess(&evidence);
            // Anything the probe can be asked of: it needs a host to ask. `raw:` and `file://`
            // have none. A loopback address does, and a development server serves soft 404s like
            // any other — excluding it would only mean the rule is never exercised.
            let has_a_host = url.starts_with("http://") || url.starts_with("https://");
            if reason.is_none()
                && !integrity.is_full()
                && has_a_host
                && self
                    .matches_the_sites_missing_page(
                        p,
                        &domain,
                        &o.final_url,
                        &parts.text,
                        proxy.as_deref(),
                    )
                    .await
            {
                reason = Some(
                    "soft 404: this is the page the site serves for addresses that do not exist"
                        .to_string(),
                );
                kind = WallKind::SoftNotFound;
            }
            // Now that the page has been parsed and classified, the pacer can be told what really
            // happened rather than what the status code claimed. A `200` with an empty body, or a
            // request for a deep page that landed on the front page, are blocks that the earlier
            // status-only feed counted as successes — and so kept hammering a domain that had
            // already shut the door.
            if !local {
                let verdict =
                    svipall_core::session::Verdict::of(&svipall_core::session::Response {
                        status: o.status,
                        text_len: parts.text.len(),
                        requested_path: path_of(&url),
                        final_path: path_of(&o.final_url),
                        wall: kind != WallKind::None,
                        elapsed_ms: ms as u64,
                        typical_ms: throttle::typical_ms(&domain, proxy.as_deref()),
                    });
                if let Some(exit) = proxy.as_deref() {
                    if (200..400).contains(&o.status) {
                        svipall_core::exits::record(&domain, exit, verdict, ms as u32);
                    }
                }
                // A held page gets the same verdict, on the same rule. `Verdict::of` knows three
                // quiet refusals the classifier does not — an empty 200, a deep request landing on
                // the front page, a sudden collapse in speed — and those are exactly how a tab goes
                // bad without saying so. Two of them retire it.
                if let Some(k) = o.kept_key.as_deref() {
                    attempt_pool.record_kept(k, verdict).await;
                }
                if verdict != svipall_core::session::Verdict::Ok && (200..400).contains(&o.status) {
                    // Only the quiet cases: an explicit 4xx/5xx was already reported above.
                    throttle::observe(
                        &domain,
                        proxy.as_deref(),
                        &tier,
                        match verdict {
                            svipall_core::session::Verdict::Blocked => throttle::Outcome::Blocked,
                            _ => throttle::Outcome::RateLimited { retry_after: None },
                        },
                    );
                }
            }
            // What this site says about HTTP/3, from whichever tier answered.
            //
            // It used to be read only at the http tier, and that was a trap with a long fuse: a
            // domain whose learned tier is `browser` never makes an http request again, so once
            // the advertisement expired — a day, by the specification's default — it could never
            // be re-learned, and h3 was off for that domain for ever. Every tier has the response
            // headers, so every tier can hear it.
            if let Some(store) = self.store.as_ref() {
                if let Some(alt) = o
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("alt-svc"))
                    .map(|(_, v)| v.as_str())
                {
                    if let Some((port, ma)) = svipall_core::altsvc::parse(alt) {
                        svipall_core::altsvc::remember(
                            store,
                            &domain_from_url(&o.final_url),
                            port,
                            ma,
                            chrono::Utc::now().timestamp(),
                        );
                    }
                }
            }
            // Was the h3 probe worth its round trip? The answer is whether the http tier delivered
            // a page, not whether QUIC connected: a wall served over HTTP/3 is still a wall, and
            // paying an extra attempt to be told so on every future fetch is the cost this
            // remembers away. It expires, because a dropped UDP port is usually the network.
            self.record_h3_probe(h3_probe, &tier, &domain, reason.is_none());
            if automatic
                && !rate_limited
                && !matches!(o.status, 401 | 407)
                && !matches!(
                    kind,
                    WallKind::NotFound
                        | WallKind::Gate
                        | WallKind::SoftNotFound
                        | WallKind::Login
                        | WallKind::Paywall
                )
            {
                use svipall_core::automatic::Feedback;
                svipall_core::automatic::record(
                    &route_context,
                    &route,
                    if reason.is_some() {
                        Feedback::Failed
                    } else if (200..300).contains(&o.status)
                        && integrity.verdict == svipall_core::quality::Verdict::Full
                    {
                        Feedback::Useful
                    } else {
                        Feedback::Delivered
                    },
                    ms as u64,
                );
            }
            let Some(reason) = reason else {
                attempts.push(format!("{}: {} ({}ms) OK", tier, o.status, ms));
                if mode == "auto" && !local && !automatic {
                    svipall_core::remember_tier(&domain, &tier);
                }
                if retire_profile {
                    // The fresh profile got through where the kept one could not: the kept one
                    // is what the wall remembers, and carrying it forward would earn the same
                    // refusal next time.
                    let flagged = PathBuf::from(svipall_core::auto_profile_path(&url, false));
                    if flagged.is_dir() && self.pool.retire_profile(&flagged).await {
                        attempts.push(format!("{tier}: retired the profile the wall remembered"));
                    }
                }
                let mut value = json!({
                    "url": url, "final_url": o.final_url, "status": o.status, "tier_used": tier,
                    "exit": proxy,
                    "identity_used": last_identity,
                    "native_fallback": native,
                    "title": parts.title, "attempts": attempts,
                });
                let obj = value.as_object_mut().expect("object");
                // A vendor on the wire is reported on a page that arrived, too. It says who is
                // watching this domain — worth knowing before the next fetch — and saying it here
                // is what keeps it a label rather than a reason to withhold.
                if let Some((sign, evidence)) = svipall_core::classify::vendor_on_the_wire(&view) {
                    obj.insert("wall_vendor".into(), json!(sign.id));
                    obj.insert("wall_evidence".into(), json!(evidence));
                }
                // A wait that cleared is still worth explaining: how long it took and whether
                // the clearance had to be re-earned is what says the tier is being used well.
                if let Some(w) = &o.warm {
                    obj.insert("warm".into(), w.clone());
                }
                // Everything measured about the page, in one record.
                //
                // This is a label, never a filter: the content is returned either way, and a
                // caller that wants the odd, thin, ugly page that happens to hold the answer still
                // gets it. `optimization` rides along at both levels, because a caller cannot tell
                // "measured, ordinary" from "not measured" when only one of the two is ever
                // written — and it is reported *only*: it never reorders anything, never subtracts
                // from the verdict, and never keeps a page from being returned.
                let record = svipall_core::quality::Record {
                    integrity,
                    optimization: parts
                        .signals
                        .as_ref()
                        .map(|s| svipall_core::quality::optimization(s, &parts.text)),
                    substance: crate::substance::assess(&parts.text),
                };
                insert_quality(obj, &record);

                // Everything measured, for a caller weighing a source rather than reading it.
                //
                // ▲ Observations, never a score. Nothing under here reorders a result, subtracts
                // from a verdict or keeps a page from being returned; the W3C Credible Web group's
                // own finding about signals of this kind — that acting on them favours large
                // professional publishers — is why it stops at reporting. The near-duplicate
                // lookup is the one thing here a stateless extractor could not do at all: it is
                // the cache being asked whether it has seen this page before, under any name.
                if p.include_quality.unwrap_or(false) {
                    let near: Vec<Value> = self
                        .store
                        .as_ref()
                        .map(|s| {
                            s.find_near(
                                svipall_core::dedup::simhash(
                                    parts.delivered.as_deref().unwrap_or(&parts.text),
                                ),
                                svipall_core::quality::provenance::NEAR_DUPLICATE_BITS,
                                3,
                            )
                        })
                        .unwrap_or_default()
                        .into_iter()
                        // Its own cached copy is not a duplicate of itself.
                        .filter(|d| d.url != url && d.url != o.final_url)
                        .map(|d| json!({ "url": d.url, "distance": d.distance }))
                        .collect();
                    let cred = svipall_core::quality::credibility::observe(
                        parts.metadata.as_ref(),
                        parts.links_detailed.as_ref(),
                        &o.final_url,
                        self.store.as_ref().and_then(|s| s.site_first_seen(&domain)),
                    );
                    let mut detail = serde_json::Map::new();
                    detail.insert("integrity".into(), json!(record.integrity));
                    if let Some(opt) = &record.optimization {
                        detail.insert("optimization".into(), json!(opt));
                        // Where this page's optimisation sits among the pages this machine has
                        // fetched. Two traits out of three is the `High` threshold, so the score
                        // is that count over three.
                        if let Some(c) = calibrate(
                            self.store.as_deref(),
                            "optimization",
                            opt.traits.len() as f32 / 3.0,
                        ) {
                            detail.insert("optimization_calibration".into(), c);
                        }
                    }
                    if let Some(s) = &parts.signals {
                        detail.insert("signals".into(), json!(s));
                    }
                    if let Some(sub) = &record.substance {
                        detail.insert("substance".into(), json!(sub));
                        if let Some(c) =
                            calibrate(self.store.as_deref(), "substance", sub.confidence)
                        {
                            detail.insert("substance_calibration".into(), c);
                        }
                    }
                    if !cred.is_empty() {
                        detail.insert("provenance".into(), json!(cred));
                    }
                    if !near.is_empty() {
                        detail.insert("near_dup_of".into(), json!(near));
                    }
                    obj.insert("quality_detail".into(), Value::Object(detail));
                }
                if let Some(rounds) = o.scroll_rounds {
                    obj.insert("scrolled".into(), json!(rounds));
                }
                if let Some(exit) = &proxy {
                    obj.insert("exit".into(), json!(exit));
                }
                let mut schema_errors = take_schema_errors();
                if let Some(ex) = &parts.extracted {
                    schema_errors.extend(ex.errors.iter().cloned());
                    if !ex.healed.is_empty() {
                        obj.insert("healed".into(), json!(ex.healed));
                        obj.insert(
                            "note".into(),
                            json!(format!(
                                "{} selector(s) no longer matched and were relocated by \
                                 fingerprint; update the schema with the `to` selectors so the \
                                 next fetch does not depend on the relocation",
                                ex.healed.len()
                            )),
                        );
                    }
                    // Remember what was found, so the next redesign can be survived too. Only
                    // when something was found: a page with no items teaches nothing.
                    if ex.matched > 0 {
                        if let Some(store) = self.store.as_ref() {
                            svipall_core::selectors::save(
                                store,
                                &domain_from_url(&o.final_url),
                                &ex.name,
                                &ex.fingerprints,
                            );
                        }
                    }
                }
                if !schema_errors.is_empty() {
                    obj.insert("schema_errors".into(), json!(schema_errors));
                }
                // Asked for, not merely parsed: `include_quality` needs the same two things and
                // must not silently turn on two large blocks the caller did not request.
                if let (Some(m), true) = (&parts.metadata, p.include_metadata.unwrap_or(false)) {
                    obj.insert("metadata".into(), json!(m));
                }
                if let (Some(l), true) = (&parts.links_detailed, p.include_links.unwrap_or(false)) {
                    obj.insert(
                        "links".into(),
                        json!({
                            "internal_count": l.internal.len(),
                            "external_count": l.external.len(),
                            // Capped: a link farm should not be able to dominate the response.
                            "internal": l.internal.iter().take(50).collect::<Vec<_>>(),
                            "external": l.external.iter().take(50).collect::<Vec<_>>(),
                            "images": l.media.iter().take(30).collect::<Vec<_>>(),
                        }),
                    );
                }
                let out_name = p
                    .out_file
                    .as_deref()
                    .filter(|n| !n.trim().is_empty())
                    .map(str::to_string);
                if p.tables.unwrap_or(false) {
                    // Typed rows, not prose. Several tables in one file are told apart by an index.
                    let many = parts.tables.len() > 1;
                    let rows: Vec<Value> = parts
                        .tables
                        .iter()
                        .enumerate()
                        .flat_map(|(i, t)| {
                            svipall_core::extraction::table::to_rows(t, many.then_some(i))
                        })
                        .collect();
                    obj.insert("table_count".into(), json!(parts.tables.len()));
                    match &out_name {
                        Some(name) => Self::export_rows_into(obj, name, &rows),
                        None => {
                            obj.insert("tables".into(), json!(parts.tables));
                        }
                    }
                }
                // The schema the page's own structure suggested, so the caller can keep it and stop
                // paying for the induction on every visit. Named `schema` in the payload because
                // that is exactly what it is: the value to pass back next time.
                if let Some(i) = &parts.induced {
                    obj.insert(
                        "induced_schema".into(),
                        json!({
                            "base_selector": i.base_selector,
                            "fields": i.fields,
                            "matched": i.matched,
                            "margin": i.margin,
                        }),
                    );
                }
                match parts.extracted {
                    // The point of a schema: hand back the objects, not the prose they came from.
                    Some(ex) => {
                        obj.insert("extracted_count".into(), json!(ex.matched));
                        match &out_name {
                            Some(name) => Self::export_rows_into(obj, name, &ex.items),
                            None => {
                                obj.insert(
                                    "extracted".into(),
                                    json!({"name": ex.name, "count": ex.matched, "items": ex.items}),
                                );
                            }
                        }
                    }
                    None if p.tables.unwrap_or(false) => {}
                    None => {
                        let content = Self::render_content(
                            &parts,
                            &o.html,
                            extraction_kind,
                            &o.content_type,
                            p.query.as_ref(),
                        );
                        // Store the markdown at full fidelity, before any query filter or token
                        // budget. That is what lets a cursor continuation come from the cache.
                        if cache_mode.may_write() && (!local || p.cache.is_some()) {
                            if let (Some(store), Some(md)) = (&self.store, &parts.markdown) {
                                let ttl = svipall_core::cache::ttl_for(
                                    o.header("cache-control"),
                                    url.contains('?'),
                                    3600,
                                );
                                if let Some(ttl) = ttl {
                                    match store.put(
                                        &url,
                                        &o.final_url,
                                        o.status,
                                        &tier,
                                        o.header("etag"),
                                        o.header("last-modified"),
                                        &o.content_type,
                                        parts.title.as_deref(),
                                        md,
                                        ttl,
                                        // Everything measured travels with the page: a cache hit
                                        // has the text but not the markup it came out of, and
                                        // asking a model again is what a hit exists to avoid, so
                                        // neither could be arrived at a second time.
                                        serde_json::to_string(&record).ok().as_deref(),
                                    ) {
                                        Ok(change) => {
                                            if cache_mode == CacheMode::Refresh {
                                                obj.insert("change".into(), json!(change));
                                            }
                                            // ▲ Learn only from a page the cache did not already
                                            // hold in this form. Re-fetching one URL must not be
                                            // able to arm a record on its own: sixteen pages of a
                                            // domain has to mean sixteen pages.
                                            //
                                            // From the stored markdown, which is the page before
                                            // this record was applied to it — learning from the
                                            // stripped view would freeze the counts the moment the
                                            // template started working.
                                            if change != svipall_core::cache::Change::Unchanged {
                                                let mut t = site_template(Some(store), &domain);
                                                t.observe(md);
                                                let _ = store.kv_set(
                                                    &svipall_core::template::key(&domain),
                                                    &t.to_json(),
                                                );
                                            }
                                        }
                                        Err(e) => tracing::debug!("cache write skipped: {e}"),
                                    }
                                }
                            }
                        }
                        // The site's own furniture comes off what the caller receives, never off
                        // what was stored: the row above is the page at full fidelity, and this is
                        // a view of it.
                        //
                        // Markdown only. The record counts markdown blocks, so pointing it at raw
                        // HTML or at the plain-text walk would be matching one thing against the
                        // hash of another — no strip, or worse, the wrong one.
                        let content = if extraction_kind == "markdown"
                            && Self::is_html_type(&o.content_type)
                        {
                            apply_template(
                                obj,
                                &template,
                                &content,
                                p.use_site_template.unwrap_or(false),
                            )
                        } else {
                            content
                        };
                        self.budget_into(&mut value, content, p);
                    }
                }
                return FetchOutcome {
                    value,
                    links: parts.links,
                    final_url: o.final_url,
                };
            };
            attempts.push(format!("{}: {} ({}ms) -> {}", route, o.status, ms, reason));
            let terminal = rate_limited
                || matches!(o.status, 401 | 407)
                || matches!(
                    kind,
                    WallKind::NotFound
                        | WallKind::Gate
                        | WallKind::SoftNotFound
                        | WallKind::Login
                        | WallKind::Paywall
                );
            let next = if terminal {
                tiers.len()
            } else if automatic {
                i + 1
            } else {
                match kind {
                    // A page that says it does not exist says the same thing from every tier, so it
                    // stops the ladder the way a real 404 does rather than costing four climbs.
                    WallKind::NotFound | WallKind::Gate | WallKind::SoftNotFound => tiers.len(),
                    // A subscription stub is the article withheld, not the page hidden from a bot:
                    // only a signed-in profile changes the answer, exactly like a login wall.
                    WallKind::Login | WallKind::Paywall => {
                        if p.profile.is_some() {
                            tiers.len()
                        } else {
                            jump(&tiers, i, "real")
                        }
                    }
                    WallKind::Vendor | WallKind::Hold => jump(&tiers, i, "real"),
                    // "Just a moment…" is a script that lets you through, and a stealth-patched
                    // headless browser clears it in seconds. The *managed* challenge scores the
                    // visitor instead, and headless has never passed one here — so it goes straight to
                    // the headful tier rather than spending an attempt, and teaching the site
                    // something, on the way.
                    WallKind::Cloudflare => {
                        let to = if svipall_core::cloudflare_is_managed_challenge(
                            &o.html.to_lowercase(),
                        ) {
                            "real"
                        } else {
                            "stealth"
                        };
                        jump(&tiers, i, to)
                    }
                    _ => i + 1,
                }
            };
            // A hold that would not clear on a persistent profile is the profile, not the page.
            // Measured: the widget's own vendor keeps the session it once flagged, in the
            // cookies; the same page opened on a profile nobody has seen cleared twice out of
            // two. So: once, on a fresh profile, and if that works the flagged one is retired
            // rather than carried into the next fetch.
            if next >= tiers.len()
                && bt.is_some()
                && !retire_profile
                && !local
                && p.profile.is_none()
                && !p.isolated.unwrap_or(false)
                && kind == WallKind::Hold
                && !automatic
            {
                retire_profile = true;
                attempts.push(format!("{tier}: retrying on a fresh profile"));
                tiers.push(tier.clone());
            }
            last = Some((o, parts, reason, kind, tier));
            i = next;
        }

        if let Some(note) = &stopped {
            if made > 0 {
                attempts.push(note.clone());
            }
        }
        let Some((o, parts, reason, kind, tier)) = last else {
            return FetchOutcome {
                value: json!({"url": url, "status": 0, "blocked_reason": if stopped.is_some() { stopped_kind } else { "no tier could fetch the page" }, "wall_kind": stopped_kind, "attempts": attempts,
                    "native_fallback": native_attempted,
                    "network_attempted": made > 0,
                    "cooldown_seconds_left": stopped_wait,
                    "note": stopped.as_deref().unwrap_or("Every tier raised an error (see attempts). Check the URL, network, or browser_path.")}),
                links: Vec::new(),
                final_url: url,
            };
        };
        let exhausted = matches!(tier.as_str(), "real" | "warm");
        if mode == "auto"
            && !local
            && exhausted
            && matches!(kind, WallKind::Status | WallKind::Vendor)
        {
            // A domain with another usable exit is not cooled down: the block was this exit's,
            // its health took the hit, and the next fetch leaves through a different one.
            match proxy.as_deref() {
                Some(exit) if svipall_core::exits::has_alternative(&domain, exit) => {
                    tracing::info!(domain, exit, "wall on this exit; the pool has another");
                }
                _ => {
                    // The domain has hard-blocked us. Whatever page we were holding for it is
                    // the page it refused, and reusing it would be starting the next attempt from
                    // the worse of the two states.
                    svipall_core::set_cooldown(&domain);
                    self.pool
                        .release_kept(|k| k.contains(&format!("|{domain}|")))
                        .await;
                }
            }
        }
        let content = Self::render_content(
            &parts,
            &o.html,
            extraction_kind,
            &o.content_type,
            p.query.as_ref(),
        );
        // Read the widget off the page rather than asking the caller to go and find it. This is
        // the difference between "there is a captcha" and "call solve_turnstile with this key".
        let challenge = matches!(
            kind,
            WallKind::Cloudflare | WallKind::Generic | WallKind::Hold | WallKind::Vendor
        )
        .then(|| svipall_core::challenge::detect(&o.html))
        .flatten();
        // Everything else the page carries, straight off the table. A wall often has more than
        // one widget on it, and the one with a dedicated tool is not always the one in the way.
        let widgets = matches!(
            kind,
            WallKind::Cloudflare | WallKind::Generic | WallKind::Hold | WallKind::Vendor
        )
        .then(|| svipall_core::challenge::detect_all(&o.html))
        .filter(|w| !w.is_empty());
        let note = match (&challenge, self.solver_state.is_some()) {
            (Some(c), true) => format!(
                "{} The page exposes a {:?} widget: call {}(sitekey=\"{}\", pageUrl=\"{}\").",
                guidance(&kind, &domain, &tier, false, self.dashboard()),
                c.kind,
                c.kind.tool(),
                c.sitekey,
                o.final_url
            ),
            _ => guidance(
                &kind,
                &domain,
                &tier,
                self.solver_state.is_some(),
                self.dashboard(),
            ),
        };
        // What is wrong on this machine is the first thing to rule out when a wall will not clear,
        // and the last thing anyone looks at, because until now it was only ever said in a log.
        let note = local_causes(
            &note,
            svipall_core::local_injection(&o.html.to_ascii_lowercase()),
            self.pool.advice(self.latest_stable_major()).as_deref(),
        );
        // Rebuilt rather than carried out of the tier loop: this runs once, on the way out, and a
        // borrow living across every tier would pin the last tier's body for the whole climb.
        let wire = svipall_core::classify::vendor_on_the_wire(
            &PageView::new(&o.html, &parts.text).on_the_wire(&o.headers, &o.cookies),
        )
        .map(|(sign, evidence)| (sign.id, evidence));
        let mut value = json!({
            "url": url, "final_url": o.final_url, "status": o.status, "tier_used": tier,
            "identity_used": last_identity,
            "native_fallback": native_attempted,
            "stopped_reason": stopped,
            "cooldown_seconds_left": stopped_wait,
            "blocked_reason": reason, "wall_kind": format!("{:?}", kind).to_lowercase(),
            "wall_vendor": wire.as_ref().map(|(id, _)| *id),
            "wall_evidence": wire.as_ref().map(|(_, e)| e.as_str()),
            "warm": o.warm,
            "note": note,
            "challenge": challenge,
            "widgets": widgets,
            "title": parts.title, "attempts": attempts,
            "content": content.chars().take(2000).collect::<String>(),
        });
        // A blocked page's own markup is exactly what an operator inspects to understand the
        // wall, and two thousand characters of it is the head of a stylesheet. Asked for a file,
        // the whole document goes there — as it does for a page that was delivered.
        if let Some(name) = p.out_file.as_deref().filter(|n| !n.trim().is_empty()) {
            if let Some(obj) = value.as_object_mut() {
                match write_out(name, &o.html) {
                    Ok((path, bytes)) => {
                        obj.insert("out_file".into(), json!(path.to_string_lossy()));
                        obj.insert("bytes".into(), json!(bytes));
                    }
                    Err(e) => {
                        obj.insert("out_file_error".into(), json!(e.to_string()));
                    }
                }
            }
        }
        FetchOutcome {
            value,
            links: parts.links,
            final_url: o.final_url,
        }
    }

    async fn fetch_with_ladder(&self, params: WebFetchParams) -> Result<CallToolResult, McpError> {
        ok(self.fetch_json(params).await.value)
    }

    /// Fetch several URLs with bounded parallelism, preserving order.
    async fn fetch_all(
        &self,
        urls: Vec<String>,
        template: &WebFetchParams,
        want_links: bool,
    ) -> Vec<FetchOutcome> {
        // What the machine can carry, not just what the config asked for. Overshooting does not
        // crawl faster: it makes every page slow, and a slow page reads to the ladder as a wall,
        // so the domain learns the wrong tier and collects a cooldown it never earned.
        let needs_browser = template.mode.as_deref().is_some_and(|m| m != "http")
            || template.max_tier.as_deref().is_some_and(|t| t != "http");
        let par = svipall_core::capacity::concurrency(svipall_core::capacity::Load {
            cores: svipall_core::capacity::cores(),
            open_browsers: self.pool.open_browsers().await,
            configured: self.cfg.parallelism,
            needs_browser,
        });
        futures::stream::iter(urls.into_iter().map(|u| {
            let p = WebFetchParams {
                url: u,
                ..template.clone()
            };
            async move { self.fetch_json_opts(p, want_links).await }
        }))
        .buffered(par)
        .collect()
        .await
    }

    #[allow(clippy::too_many_arguments)] // one internal helper; splitting into a struct would not read better
    async fn interact(
        &self,
        tier: BrowserTier,
        url: &str,
        profile: Option<&str>,
        proxy: Option<String>,
        actions: &[Value],
        mobile: bool,
        extraction_kind: &str,
        query: Option<&str>,
    ) -> anyhow::Result<Value> {
        let profile_dir = self.profile_dir_for(tier, url, profile);
        let opts = PageOpts {
            mobile,
            tier,
            identity_seed: identity_seed_for(profile_dir.as_deref(), url, profile),
            profile_dir,
            proxy,
            visible: false,
        };
        self.charge_visit(url, opts.tier, opts.proxy.as_deref())
            .await?;
        let (_pooled, page) = self.pool.page(&opts).await?;
        let result = async {
            let status = self.pool.navigate(&page, url).await?;
            let results = self.pool.run_actions(&page, actions).await;
            let (html, final_url) = self.pool.content(&page).await?;
            Ok::<_, anyhow::Error>((status, results, html, final_url))
        }
        .await;
        self.pool.close_page(page).await;
        let (status, results, html, final_url) = result?;
        let p = WebFetchParams {
            url: url.to_string(),
            query: query.map(|q| q.to_string()),
            ..Default::default()
        };
        let wants = Self::wants_for(
            &p,
            extraction_kind,
            "text/html",
            &final_url,
            false,
            Default::default(),
        );
        let parts = extraction::parse_page(&html, &wants);
        let content = Self::render_content(
            &parts,
            &html,
            extraction_kind,
            "text/html",
            p.query.as_ref(),
        );
        Ok(
            json!({"url": url, "final_url": final_url, "status": status, "tier_used": format!("{:?}", tier).to_lowercase(), "title": parts.title, "actions": results, "chars": content.chars().count(), "content": content}),
        )
    }
}

#[tool_router]
impl SvipallServer {
    #[tool(
        description = "Fetch any web page, auto-escalating until anti-bot is defeated. mode=auto climbs http -> browser -> stealth -> real -> warm and remembers the working tier per domain. Never set mode manually unless debugging. Supports method/body/headers for API calls on the http tier."
    )]
    async fn web_fetch(
        &self,
        params: Parameters<WebFetchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.fetch_with_ladder(params.0).await
    }

    #[tool(
        description = "Fetch several URLs in parallel (bounded) with the same auto escalation as web_fetch. Results keep the input order."
    )]
    async fn web_fetch_many(
        &self,
        params: Parameters<WebFetchManyParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.fetch_many_json(params.0).await)
    }

    /// The body of `web_fetch_many`, as plain JSON, so it can be exercised without the tool
    /// envelope — the same shape `fetch_json` and `crawl_json` already have.
    pub async fn fetch_many_json(&self, p: WebFetchManyParams) -> Value {
        let template = WebFetchParams {
            mode: p.mode,
            extraction: p.extraction,
            max_tier: p.max_tier,
            query: p.query,
            timeout: p.timeout,
            main_content_only: Some(true),
            ..Default::default()
        };
        let mut results: Vec<Value> = self
            .fetch_all(p.urls, &template, false)
            .await
            .into_iter()
            .map(|o| o.value)
            .collect();

        // Five results carrying one wire story are one source wearing five hostnames, and without
        // this they read as five confirmations. Nothing is removed — each result is told which
        // earlier one it repeats, and the caller decides what that is worth.
        let hashes: Vec<u64> = results
            .iter()
            .map(|v| {
                v.get("content")
                    .and_then(Value::as_str)
                    .map(svipall_core::dedup::simhash)
                    .unwrap_or(0)
            })
            .collect();
        let (duplicate_of, corroboration) = svipall_core::quality::provenance::group(&hashes);
        for (i, of) in duplicate_of.iter().enumerate() {
            let Some(first) = of else { continue };
            let Some(source) = results
                .get(*first)
                .and_then(|v| v.get("final_url").or_else(|| v.get("url")))
                .cloned()
            else {
                continue;
            };
            if let Some(obj) = results[i].as_object_mut() {
                obj.insert("same_text_as".into(), source);
            }
        }
        // ▲ Put the different ones where they will be read. Cuconasu et al. (SIGIR 2024) measured
        // that what breaks a generated answer is the on-topic page that does not contain it — four
        // copies of one wire story at the top of a list is exactly that shape — and that adding
        // *distant* documents raised accuracy. This is a reordering and only a reordering: every
        // result comes back, the caller's first choice stays first, and a set with nothing
        // redundant in it is returned in the order it was asked for.
        let ordering = svipall_core::quality::diversity::order(&hashes);
        let reordered = ordering.iter().enumerate().any(|(pos, &i)| pos != i);
        let mut by_index: Vec<Option<Value>> = results.into_iter().map(Some).collect();
        let results: Vec<Value> = ordering
            .iter()
            .filter_map(|&i| by_index[i].take())
            .collect();

        let mut out = json!({
            "count": results.len(),
            // How many distinct documents these results actually are. Corroboration, not trust:
            // it says the same thing was said N times independently, never that it is true.
            "corroboration": corroboration,
            "results": results,
        });
        // Said when it happened, because a result set that comes back in a different order than it
        // was asked for and does not say so is a surprise rather than a feature.
        if reordered {
            out["reordered_for_diversity"] = json!(true);
        }
        out
    }

    #[tool(
        description = "Interact with a page in a real browser: click, type, fill, press, hover, select, scroll, wait, eval, goto, screenshot. Returns action results and the final page content. Actions: [{do:'click',selector:'#x'},{do:'type',selector:'input',text:'hi'},{do:'press',key:'Enter'},{do:'wait',ms:1500}|{do:'wait',selector:'.done'},{do:'eval',script:'document.title'},{do:'scroll',pixels:800}]"
    )]
    async fn web_act(&self, params: Parameters<WebActParams>) -> Result<CallToolResult, McpError> {
        ok(self.act_json(params.0).await)
    }

    /// `web_act` without the protocol wrapper, for the CLI, the REST API and tests.
    ///
    /// Infallible on purpose, and this is the one seam where that matters. A timeout or a browser
    /// error here is a *successful report of a failed interaction* — "web_act timed out after
    /// 90000ms" is the answer, not the absence of one — and an infallible signature is what stops a
    /// later caller turning it into a 500.
    pub async fn act_json(&self, p: WebActParams) -> Value {
        let tier =
            BrowserTier::parse(p.tier.as_deref().unwrap_or("real")).unwrap_or(BrowserTier::Real);
        let domain = domain_from_url(&p.url);
        let proxy = p.proxy.clone().or_else(|| self.exit_for(&domain));
        let timeout = Duration::from_millis(p.timeout.unwrap_or(90_000));
        let fut = self.interact(
            tier,
            &p.url,
            p.profile.as_deref(),
            proxy,
            &p.actions,
            false,
            p.extraction.as_deref().unwrap_or("markdown"),
            None,
        );
        match tokio::time::timeout(timeout, fut).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => json!({"url": p.url, "status": 0, "error": e.to_string()}),
            Err(_) => {
                json!({"url": p.url, "status": 0, "error": format!("web_act timed out after {}ms", timeout.as_millis())})
            }
        }
    }

    /// Load a crawl to continue, or mint an id for a new one.
    ///
    /// On resume the stored parameters win: the caller passes an id, not a whole request again.
    /// The three budgets are the exception, because raising them is the reason to resume a crawl
    /// that stopped on `max_pages`, `budget` or `time` in the first place.
    fn resume_or_start(&self, mut p: WebCrawlParams) -> (WebCrawlParams, String, ResumeState) {
        let mut state = ResumeState::default();
        let requested = p.crawl_id.clone();
        let (Some(id), Some(store)) = (requested.clone(), self.store.as_ref()) else {
            let id = requested
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()[..16].to_string());
            return (p, id, state);
        };
        let Some(saved) = store.load_crawl(&id) else {
            return (p, id, state);
        };
        if let Ok(stored) = serde_json::from_str::<WebCrawlParams>(&saved.params_json) {
            let (pages, tokens, duration) = (p.max_pages, p.max_tokens_total, p.max_duration_ms);
            p = stored;
            p.max_pages = pages.or(p.max_pages);
            p.max_tokens_total = tokens.or(p.max_tokens_total);
            p.max_duration_ms = duration.or(p.max_duration_ms);
            p.crawl_id = Some(id.clone());
        }
        p.url = saved.start_url;
        state.pending = saved.pending;
        state.done = saved.done;
        (p, id, state)
    }

    /// Write the crawl's progress. Best effort on purpose: losing resumability is a worse outcome
    /// than a crawl that fails because its bookkeeping could not be written.
    #[allow(clippy::too_many_arguments)]
    fn persist_crawl(
        &self,
        id: &str,
        start: &str,
        params_json: &str,
        status: &str,
        pages_done: usize,
        stopped_by: Option<&str>,
        fetched: &[String],
        frontier: &svipall_core::frontier::Frontier,
    ) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        // The row has to exist before its queue rows reference it.
        if let Err(e) = store.save_crawl(id, start, params_json, status, pages_done, stopped_by) {
            tracing::debug!("crawl {id} not saved: {e}");
            return;
        }
        if !fetched.is_empty() {
            if let Err(e) = store.mark_done(id, fetched) {
                tracing::debug!("crawl {id} progress not saved: {e}");
            }
        }
        if let Err(e) = store.save_frontier(id, &frontier.snapshot()) {
            tracing::debug!("crawl {id} frontier not saved: {e}");
        }
    }

    #[tool(
        description = "Crawl a site by following same-domain links breadth-first and return every page as markdown. max_pages (default 20), max_depth (default 2), include (substring filter for links). Returns a crawl_id; pass it back as crawl_id to resume where it stopped."
    )]
    async fn web_crawl(
        &self,
        params: Parameters<WebCrawlParams>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // A crawl of two hundred pages is minutes of silence otherwise, and a caller with no signal
        // cannot tell a slow crawl from a hung one. Only sent when the client asked for it, by
        // passing a progress token — nothing is pushed at a client that did not.
        let progress = context
            .meta
            .get_progress_token()
            .map(|token| Progress::new(context.peer.clone(), token));
        ok(self
            .crawl_json_with(params.0, progress.as_ref().map(|p| p as &dyn ProgressSink))
            .await)
    }

    /// The crawl itself. Public so tests and the bench can drive it without the MCP wrapper.
    pub async fn crawl_json(&self, p: WebCrawlParams) -> Value {
        self.crawl_json_with(p, None).await
    }

    /// The crawl, reporting to whoever is listening.
    ///
    /// `Option<&dyn>` rather than a null sink, because the doctrine above is that a progress
    /// notification sent to a client that never asked for one is noise — and `None` costs no
    /// allocation on the path the CLI, the bench and the tests take.
    pub async fn crawl_json_with(
        &self,
        p: WebCrawlParams,
        progress: Option<&dyn ProgressSink>,
    ) -> Value {
        let (p, crawl_id, resume) = self.resume_or_start(p);
        // Saved before `template` moves the fields out, so a resume gets the request verbatim.
        let params_json = serde_json::to_string(&p).unwrap_or_else(|_| "{}".into());
        let already = resume.done.len();
        let start = p.url.clone();
        let domain = domain_from_url(&start);
        // Tests and dev servers run against loopback; making them wait would only slow the suite.
        let local_crawl = is_local(&domain);
        let max_pages = p.max_pages.unwrap_or(20).clamp(1, 200);
        let max_depth = p.max_depth.unwrap_or(2);
        let cap = p.max_chars_per_page.unwrap_or(8000);
        let template = WebFetchParams {
            mode: p.mode,
            extraction: p.extraction,
            query: p.query,
            scroll: p.scroll.clone(),
            timeout: Some(p.timeout.unwrap_or(45_000)),
            main_content_only: Some(true),
            ..Default::default()
        };
        let query = template.query.clone();
        let robots_policy = p
            .robots
            .as_deref()
            .and_then(svipall_core::RobotsPolicy::parse)
            // Crawling is the automated, high-volume path robots.txt exists for, so it obeys by
            // default. A single `web_fetch` a person asked for is a different situation.
            .unwrap_or(svipall_core::RobotsPolicy::Obey);
        let robots = if robots_policy == svipall_core::RobotsPolicy::Ignore {
            None
        } else {
            self.robots_for(&start).await
        };
        let allowed = |u: &str, robots: &Option<svipall_core::Robots>| -> bool {
            let Some(r) = robots else { return true };
            let Ok(parsed) = url::Url::parse(u) else {
                return true;
            };
            let path = match parsed.query() {
                Some(q) => format!("{}?{}", parsed.path(), q),
                None => parsed.path().to_string(),
            };
            r.allows("svipall", &path)
        };

        let dedup_on = p.dedup.unwrap_or(true);
        let mut dedup = svipall_core::dedup::DedupIndex::new(3);
        let mut boilerplate = svipall_core::dedup::Boilerplate::default();
        let saturation_on = p.stop_when_saturated.unwrap_or(query.is_some());
        let mut novelty = svipall_core::saturation::Saturation::new(query.as_deref());
        let token_budget = p.max_tokens_total.unwrap_or(self.cfg.max_tokens_total);
        let deadline = Instant::now() + Duration::from_millis(p.max_duration_ms.unwrap_or(120_000));

        let mut used_tokens = 0usize;
        let mut duplicates = 0usize;
        let mut skipped_by_robots = 0usize;
        // URLs a gate declined to request. Reported, because a crawl that came back short for this
        // reason is not a crawl that ran out of pages.
        let mut refused = 0usize;
        let mut stopped_by = "frontier_empty";
        let mut coverage = 0.0f32;

        // Best-first when there is a query to aim at, breadth-first otherwise. Blind BFS spends a
        // page budget on whatever a page happened to link first. Depth-first has to be asked for:
        // it is the right answer for a manual or a paginated listing and the wrong one for a site
        // nobody has looked at yet.
        use svipall_core::frontier::Order;
        let order = match p.strategy.as_deref() {
            Some("dfs" | "depth_first") => Order::Depth,
            Some("bfs" | "breadth_first") => Order::Breadth,
            _ if query.is_some() => Order::Best,
            _ => Order::Breadth,
        };
        let best_first = order == Order::Best;
        let mut frontier = svipall_core::frontier::Frontier::new(query.as_deref()).ordered(order);
        // A resumed crawl starts from what it had queued, not from the seed. Pages fetched last
        // time are marked seen so a link back to one of them does not spend a request, and their
        // fingerprints come back so deduplication still spans the interruption.
        for u in &resume.done {
            frontier.mark_seen(u);
            if dedup_on {
                if let Some(hit) = self.store.as_ref().and_then(|s| s.get(u)) {
                    dedup.insert_or_find(hit.simhash, u);
                }
            }
        }
        // Every domain this crawl is allowed on. One is the ordinary case; several share one budget,
        // which is what "research these three sites" actually means.
        let mut domains: Vec<String> = vec![domain.clone()];
        for extra in p.also.iter().flatten() {
            let d = domain_from_url(extra);
            if !d.is_empty() && !domains.iter().any(|x| same_site(x, &d)) {
                domains.push(d);
            }
        }
        // An equal share each, so one large site cannot spend the whole run before the others are
        // touched. Rounded up: with three domains and ten pages, four each is better than three
        // each and one page nobody can use.
        let per_domain = max_pages.div_ceil(domains.len().max(1));
        let mut taken: HashMap<String, usize> = HashMap::new();
        let on_a_wanted_site = |u: &str, domains: &[String]| {
            let d = domain_from_url(u);
            domains.iter().any(|x| same_site(x, &d))
        };

        let mut skipped_unchanged = 0usize;
        if resume.pending.is_empty() && resume.done.is_empty() {
            for extra in p.also.iter().flatten() {
                frontier.push(svipall_core::frontier::Candidate::seed(extra.clone()));
            }
            // The second crawl of a site is almost entirely the first crawl again. A documentation
            // site of four hundred pages changes five in a week; re-reading the other 395 costs the
            // same as the first run did, in requests and in the site's patience.
            if p.since_last_crawl.unwrap_or(false) {
                let (queued, skipped) = self.seed_from_sitemap(&start, &mut frontier).await;
                skipped_unchanged = skipped;
                tracing::info!(queued, skipped, "incremental seed from sitemap");
            }
            // Always seed the start URL too: a sitemap that is missing, unreachable or partial must
            // not turn a crawl into no crawl at all.
            frontier.push(svipall_core::frontier::Candidate::seed(start.clone()));
        } else {
            for (u, d, score) in &resume.pending {
                frontier.restore(u.clone(), *d, *score);
            }
        }
        let mut pages: Vec<Value> = Vec::new();
        // Carrying `already` rather than zero, so a listener that joins a resumed crawl is never
        // told it is starting from the beginning of a job that is half done.
        if let Some(sink) = progress {
            let mut e = CrawlEvent::new(&crawl_id, EventKind::Started, already, frontier.len());
            e.total = Some(max_pages);
            sink.report(&e).await;
        }
        let mut depth = resume
            .pending
            .iter()
            .map(|(_, d, _)| *d as usize)
            .min()
            .unwrap_or(0);
        while !frontier.is_empty() && already + pages.len() < max_pages && depth <= max_depth {
            // Checked here and after each page, which are the two places the crawl is already
            // between things. It leaves through the ordinary exit below, so `persist_crawl` writes
            // the frontier and a cancelled crawl can be picked up again — which costs nothing and
            // is strictly better than being killed.
            if progress.is_some_and(|s| s.should_stop()) {
                stopped_by = "cancelled";
                break;
            }
            if Instant::now() > deadline {
                stopped_by = "time";
                break;
            }
            if used_tokens >= token_budget {
                stopped_by = "budget";
                break;
            }
            let room = max_pages - already - pages.len();
            // Best-first takes the highest-scoring candidates; breadth-first drains the level.
            // A depth-first walk that takes a whole level at a time is not a depth-first walk: the
            // batch has to be small enough that the links from one page reach the queue before the
            // next page is chosen.
            let take = if best_first || order == Order::Depth {
                room.min(self.cfg.parallelism.max(1))
            } else {
                room
            };
            let popped = frontier.pop_batch(take);
            let scores: HashMap<String, f32> =
                popped.iter().map(|(c, s)| (c.url.clone(), *s)).collect();
            let mut batch: Vec<String> = Vec::new();
            for (c, _) in &popped {
                // One site cannot spend the whole run. Checked here, against what was actually
                // fetched, rather than when links are queued: the queue is a guess and this is not.
                if domains.len() > 1 {
                    let d = domain_from_url(&c.url);
                    let used = taken.entry(d.clone()).or_insert(0);
                    if *used >= per_domain {
                        continue;
                    }
                    *used += 1;
                }
                batch.push(c.url.clone());
            }
            if batch.is_empty() {
                // Everything left belongs to a site that has had its share, so there is nothing to
                // do with the rest of the budget.
                stopped_by = "per_domain_share";
                break;
            }

            // Refuse disallowed URLs before spending a request on them.
            let (batch, blocked): (Vec<String>, Vec<String>) = batch.into_iter().partition(|u| {
                robots_policy != svipall_core::RobotsPolicy::Obey || allowed(u, &robots)
            });
            skipped_by_robots += blocked.len();

            let outcomes = self
                .fetch_all(batch.clone(), &template, depth < max_depth)
                .await;
            // A gate that answered before a request went out has not fetched that URL, and it must
            // not be recorded as fetched: `mark_done` is what a resume reads to decide it never
            // has to ask again, so a page held back by a cooldown or a spent budget would be lost
            // for this crawl id — content withheld with a success report on top. Every such URL
            // goes back on the frontier, including the rest of the batch behind it, and the crawl
            // stops rather than draining the queue into refusal stubs at full speed.
            let stop_at = batch
                .iter()
                .zip(&outcomes)
                .position(|(_, o)| never_requested(&o.value).is_some());
            if let Some(at) = stop_at {
                stopped_by = never_requested(&outcomes[at].value).unwrap_or("over_budget");
                for u in &batch[at..] {
                    refused += 1;
                    frontier.requeue(svipall_core::frontier::Candidate {
                        url: u.clone(),
                        depth: depth as u16,
                        source: svipall_core::frontier::Source::Link,
                        anchor: String::new(),
                        parent_score: 0.0,
                        lastmod: None,
                    });
                }
            }
            let kept = stop_at.unwrap_or(batch.len());
            let fetched: Vec<String> = batch[..kept].to_vec();
            for (u, o) in batch.iter().zip(outcomes).take(kept) {
                let mut v = o.value.clone();
                let body = v
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_string();

                // ▲ A near-identical page is *labelled*, not emptied. This used to replace the
                // page with a four-field stub, which is the anti-discard contract's own
                // prohibition — "quality labels a page, never filters one" — being broken inside
                // the house that wrote it, on by default. Two pages of a catalogue that differ in
                // one price are near-identical and one of them may be the one that was wanted. The
                // token budget is what caps the response; `dedup` says what a page is, and the
                // caller decides.
                let mut duplicate: Option<(String, f32)> = None;
                if dedup_on && !body.is_empty() {
                    let hash = svipall_core::dedup::simhash(&body);
                    if let Some((of, similarity)) = dedup.insert_or_find(hash, u) {
                        duplicates += 1;
                        duplicate = Some((of, similarity));
                        if let Some(sink) = progress {
                            let mut e = CrawlEvent::new(
                                &crawl_id,
                                EventKind::Duplicate,
                                already + pages.len(),
                                frontier.len(),
                            )
                            .from_fetch(u, &o.value);
                            e.total = Some(max_pages);
                            sink.report(&e).await;
                        }
                    }
                }

                // Header, footer and cookie banner appear on every page; they survive density
                // pruning when they sit inside <main>, and across a crawl they are the biggest
                // single saving available.
                //
                // A duplicate is not observed: it is one page seen twice, and counting its blocks
                // again would make a page that happens to repeat look like the site's frame.
                if duplicate.is_none() {
                    boilerplate.observe(&svipall_core::budget::blocks(&body));
                }
                // How much of it was furniture, measured before it is taken away. The fetch could
                // not know this; by the time a crawl is a few pages in, it can.
                if boilerplate
                    .share(&body, 0.6)
                    .is_some_and(|s| s > svipall_core::quality::MAX_BOILERPLATE_SHARE)
                {
                    if let Some(obj) = v.as_object_mut() {
                        add_quality_reason(obj, svipall_core::quality::Reason::MostlyBoilerplate);
                    }
                }
                let body = boilerplate.strip(&body, 0.6);

                // A duplicate adds no coverage by definition; feeding it to the saturation
                // rule would make a site that repeats itself look thoroughly explored.
                if saturation_on && duplicate.is_none() {
                    let verdict = novelty.observe(&body);
                    coverage = verdict.coverage;
                    if verdict.saturated {
                        stopped_by = "saturation";
                    }
                }

                used_tokens += svipall_core::budget::estimate_tokens(&body);
                let truncated: String = body.chars().take(cap).collect();
                v["content"] = Value::String(truncated);
                if let Some((of, similarity)) = duplicate {
                    v["duplicate_of"] = json!(of);
                    v["similarity"] = json!(similarity);
                }
                if robots_policy == svipall_core::RobotsPolicy::Warn && !allowed(u, &robots) {
                    v["robots_disallowed"] = json!(true);
                }
                v["depth"] = json!(depth);
                pages.push(v);
                // Per page, not per batch. This used to fire once at the bottom of the while loop,
                // so a max_depth=2 crawl reported three times in its whole run — enough to say a
                // crawl was alive, not enough to say where it was.
                if let Some(sink) = progress {
                    let mut e = CrawlEvent::new(
                        &crawl_id,
                        EventKind::Page,
                        already + pages.len(),
                        frontier.len(),
                    )
                    .from_fetch(u, &o.value);
                    e.total = Some(max_pages);
                    sink.report(&e).await;
                }
                if stopped_by == "saturation" {
                    break;
                }
                // Term statistics from pages actually fetched, so ranking uses the site's own IDF.
                frontier.observe_page(&body);
                // Asking for the next page 40 ms after receiving this one is the cheapest signal a
                // crawler emits: nobody reads that fast and no browser renders that fast. The wait
                // is proportional to how much there was to read, and it overlaps with the pacer's
                // own gap rather than adding to it.
                if !local_crawl {
                    let dwell = crate::behavior::dwell(body.len(), self.identity.noise_seed);
                    let paced = Duration::from_millis(
                        svipall_core::throttle::typical_ms(&domain, None)
                            .min(dwell.as_millis() as u64),
                    );
                    tokio::time::sleep(dwell.saturating_sub(paced)).await;
                }

                // Page two of a listing competes with every "about" and "terms" on the site when it
                // is just one more link, so a forty-page catalogue reliably ends after the first
                // page and the crawl reports success. Queued explicitly, at the score of the page
                // it continues, it stays ahead of the furniture.
                if let Some(next) = svipall_core::pagination::next_page(u) {
                    if on_a_wanted_site(&next, &domains) {
                        frontier.push(svipall_core::frontier::Candidate {
                            url: next,
                            depth: depth as u16,
                            source: svipall_core::frontier::Source::Link,
                            anchor: String::new(),
                            parent_score: scores.get(u).copied().unwrap_or(0.5) + 0.5,
                            lastmod: None,
                        });
                    }
                }

                // Links came out of the ladder's own parse; the crawler never re-parses.
                let parent_score = scores.get(u).copied().unwrap_or(0.5);
                for link in o.links {
                    if !on_a_wanted_site(&link, &domains) || looks_binary(&link) {
                        continue;
                    }
                    if let Some(inc) = &p.include {
                        if !link.contains(inc.as_str()) {
                            continue;
                        }
                    }
                    frontier.push(svipall_core::frontier::Candidate {
                        url: link,
                        depth: depth as u16 + 1,
                        source: svipall_core::frontier::Source::Link,
                        anchor: String::new(),
                        parent_score,
                        lastmod: None,
                    });
                }

                // The second cancellation point, and it belongs *here* rather than beside the
                // progress report a few lines up: this page has already been fetched and paid for,
                // and its links are the frontier a resume would continue from. Stopping before they
                // are queued throws that away, so a cancelled crawl would come back with nothing
                // left to resume — measured, not guessed.
                if progress.is_some_and(|s| s.should_stop()) {
                    stopped_by = "cancelled";
                    break;
                }
            }
            // One transaction per batch. A kill between batches costs at most this batch, and what
            // survives on disk is exactly the queue that was still pending.
            self.persist_crawl(
                &crawl_id,
                &start,
                &params_json,
                "running",
                already + pages.len(),
                None,
                &fetched,
                &frontier,
            );
            depth += 1;
            // A gate answering instead of the site is not a reason to keep popping the frontier:
            // the next URL would meet the same gate and the whole queue would empty in
            // milliseconds without a single request.
            if matches!(
                stopped_by,
                "saturation" | "over_budget" | "cooldown" | "traffic_state" | "timeout"
            ) {
                break;
            }
        }
        self.persist_crawl(
            &crawl_id,
            &start,
            &params_json,
            if frontier.is_empty() {
                "finished"
            } else {
                "stopped"
            },
            already + pages.len(),
            Some(stopped_by),
            &[],
            &frontier,
        );
        if already + pages.len() >= max_pages && stopped_by == "frontier_empty" {
            stopped_by = "max_pages";
        }
        if let Some(sink) = progress {
            let mut e = CrawlEvent::new(
                &crawl_id,
                EventKind::Finished,
                already + pages.len(),
                frontier.len(),
            );
            e.total = Some(max_pages);
            e.stopped_by = Some(stopped_by.to_string());
            sink.report(&e).await;
        }

        let summary = json!({
            "start_url": start,
            "domain": domain,
            "crawl_id": crawl_id,
            "count": pages.len(),
            "pages_before_resume": already,
            "stopped_by": stopped_by,
            "coverage": coverage,
            "duplicates_skipped": duplicates,
            "skipped_by_robots": skipped_by_robots,
            "refused_without_asking": refused,
            "skipped_unchanged": skipped_unchanged,
            "tokens_estimated": used_tokens,
            "truncated": !frontier.is_empty(),
            "pending_links": frontier.len(),
        });

        // `llms.txt` is a map of the site rather than its contents: a fraction of the tokens, and
        // usually enough to decide what is actually worth reading.
        match p.output.as_deref() {
            Some(kind @ ("llms.txt" | "llms-full.txt")) => {
                let summaries: Vec<(svipall_core::llms_txt::PageSummary, Option<String>)> = pages
                    .iter()
                    .filter(|v| v.get("duplicate_of").is_none())
                    .map(|v| {
                        (
                            svipall_core::llms_txt::PageSummary {
                                url: v["url"].as_str().unwrap_or_default().to_string(),
                                title: v["title"].as_str().map(str::to_string),
                                description: v
                                    .pointer("/metadata/description")
                                    .and_then(|d| d.as_str())
                                    .map(str::to_string),
                            },
                            v["content"].as_str().map(str::to_string),
                        )
                    })
                    .collect();
                let index = svipall_core::llms_txt::render_index(&domain, None, &summaries);
                let content = if kind == "llms-full.txt" {
                    svipall_core::llms_txt::render_full(&index, &summaries, token_budget)
                } else {
                    index
                };
                let mut value = summary;
                let obj = value.as_object_mut().expect("object");
                obj.insert("format".into(), json!(kind));
                obj.insert("content".into(), json!(content));
                value
            }
            _ => {
                let mut value = summary;
                let obj = value.as_object_mut().expect("object");
                // Two hundred pages through the model's context is the expensive way to copy a
                // file: it reads every row, pays for every row, and writes most of them back out
                // again to save them. A path and a count is the same information.
                match p.out_file.as_deref().filter(|n| !n.trim().is_empty()) {
                    Some(name) => Self::export_rows_into(obj, name, &pages),
                    None => {
                        obj.insert("pages".into(), json!(pages));
                    }
                }
                value
            }
        }
    }

    #[tool(
        description = "The JSON a page fetched while it loaded. Most sites render from an API their own JavaScript called a moment earlier, and that response is smaller, already typed, and far more stable than the HTML built from it. Use it to find a site's real endpoint — an endpoint that took page=1 will take page=2, which beats following links. Call it without a pattern first to see what the page asked for, then again with `pattern` and `bodies=true`."
    )]
    async fn web_capture(
        &self,
        params: Parameters<WebCaptureParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.capture_json(params.0).await.map_err(err)?)
    }

    /// The capture itself, callable without the MCP wrapper so tests can drive it.
    pub async fn capture_json(&self, p: WebCaptureParams) -> anyhow::Result<Value> {
        use futures::StreamExt;
        use svipall_cdp::cdp::browser_protocol::network::{
            EnableParams, EventResponseReceived, GetResponseBodyParams,
        };

        if !self.pool.available() {
            anyhow::bail!("{}", no_browser_hint());
        }
        let tier = BrowserTier::parse(p.tier.as_deref().unwrap_or("stealth"))
            .unwrap_or(BrowserTier::Stealth);
        let profile_dir = p.profile.as_deref().map(crate::browser::named_profile);
        let opts = PageOpts {
            mobile: false,
            tier,
            identity_seed: identity_seed_for(profile_dir.as_deref(), &p.url, p.profile.as_deref()),
            profile_dir,
            proxy: self.exit_for(&domain_from_url(&p.url)),
            visible: false,
        };
        self.charge_visit(&p.url, opts.tier, opts.proxy.as_deref())
            .await?;
        let (_pooled, page) = self.pool.page(&opts).await?;

        let result = async {
            page.execute(EnableParams::default()).await?;
            let mut events = page.event_listener::<EventResponseReceived>().await?;
            // Collect while the page loads. The listener has to be live *before* navigation or the
            // calls a page makes on arrival — the interesting ones — are already gone.
            // A shared buffer rather than the task's return value: the collector is stopped by
            // aborting it, and an aborted task's result is discarded — which silently threw away
            // everything it had gathered.
            let seen = std::sync::Arc::new(StdMutex::new(
                Vec::<(String, crate::capture::Captured)>::new(),
            ));
            let sink = seen.clone();
            let collector = tokio::spawn(async move {
                while let Some(ev) = events.next().await {
                    let r = &ev.response;
                    if !crate::capture::is_interesting(&r.mime_type, &format!("{:?}", ev.r#type)) {
                        continue;
                    }
                    let mut buf = match sink.lock() {
                        Ok(b) => b,
                        Err(_) => break,
                    };
                    buf.push((
                        ev.request_id.inner().clone(),
                        crate::capture::Captured {
                            url: r.url.clone(),
                            method: "GET".into(),
                            status: r.status as u16,
                            mime: r.mime_type.clone(),
                            body: None,
                        },
                    ));
                    if buf.len() >= 300 {
                        break;
                    }
                }
            });
            self.pool.navigate(&page, &p.url).await?;
            tokio::time::sleep(Duration::from_millis(p.settle_ms.unwrap_or(3_000))).await;
            collector.abort();
            let mut found = seen.lock().map(|b| b.clone()).unwrap_or_default();

            found.retain(|(_, c)| crate::capture::matches(&c.url, p.pattern.as_deref()));
            if p.bodies.unwrap_or(false) {
                let max = p.max_body.unwrap_or(20_000);
                for (id, c) in found.iter_mut() {
                    if let Ok(b) = page.execute(GetResponseBodyParams::new(id.clone())).await {
                        let (text, _) = crate::capture::cap_body(&b.result.body, max);
                        c.body = Some(text);
                    }
                }
            }
            Ok::<_, anyhow::Error>(found)
        }
        .await;
        self.pool.close_page(page).await;
        let found = result?;

        let list: Vec<crate::capture::Captured> = found.into_iter().map(|(_, c)| c).collect();
        Ok(json!({
            "url": p.url,
            "captured": list.len(),
            "endpoints": crate::capture::endpoints(&list),
            "responses": list.iter().map(|c| json!({
                "url": c.url, "status": c.status, "mime": c.mime, "body": c.body,
            })).collect::<Vec<_>>(),
        }))
    }

    #[tool(
        description = "The page as a structure you can act on: every button, link and field with its role, its accessible name and a short reference. Use this instead of web_fetch when the next step is to click or type, and instead of a screenshot always — it is deterministic, needs no vision model, and costs a fraction of the tokens. Pass `find` to get only what matches, `max_depth` to go shallower."
    )]
    async fn web_snapshot(
        &self,
        params: Parameters<WebSnapshotParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.snapshot_json(params.0).await.map_err(err)?)
    }

    /// The snapshot itself, callable without the MCP wrapper so tests can drive it.
    pub async fn snapshot_json(&self, p: WebSnapshotParams) -> anyhow::Result<Value> {
        let tier =
            BrowserTier::parse(p.tier.as_deref().unwrap_or("real")).unwrap_or(BrowserTier::Real);
        let profile_dir = p
            .profile
            .as_deref()
            .map(crate::browser::named_profile)
            .or_else(|| {
                Some(std::path::PathBuf::from(svipall_core::auto_profile_path(
                    &p.url, true,
                )))
            });
        let opts = PageOpts {
            mobile: false,
            tier,
            identity_seed: identity_seed_for(profile_dir.as_deref(), &p.url, p.profile.as_deref()),
            profile_dir,
            proxy: self.exit_for(&domain_from_url(&p.url)),
            visible: false,
        };
        if !self.pool.available() {
            anyhow::bail!("{}", no_browser_hint());
        }
        self.charge_visit(&p.url, opts.tier, opts.proxy.as_deref())
            .await?;
        let (_pooled, page) = self.pool.page(&opts).await?;
        let result = async {
            self.pool
                .navigate(&page, &p.url)
                .await
                .map_err(|e| anyhow::anyhow!("navigate: {e}"))?;
            let raw = page
                .evaluate(crate::snapshot::WALK_JS)
                .await
                .map_err(|e| anyhow::anyhow!("walking the page: {e}"))?
                .into_value::<Vec<Value>>()
                .unwrap_or_default();
            let (_, final_url) = self.pool.content(&page).await?;
            Ok::<_, anyhow::Error>((raw, final_url))
        }
        .await;
        self.pool.close_page(page).await;
        let (raw, final_url) = result?;

        let all = crate::snapshot::prune(&raw, p.max_depth, p.limit.unwrap_or(200));
        let shown = match p.find.as_deref() {
            Some(needle) => crate::snapshot::find(&all, needle),
            None => all.clone(),
        };
        let rendered = crate::snapshot::render(&shown);
        Ok(json!({
            "url": p.url,
            "final_url": final_url,
            "nodes": shown.len(),
            "nodes_total": all.len(),
            "interactive": shown.iter().filter(|n| crate::snapshot::is_interactive(&n.role)).count(),
            "snapshot": rendered,
            "tokens_estimated": svipall_core::budget::estimate_tokens(&rendered),
        }))
    }

    #[tool(
        description = "Remember something between sessions. An agent crawling a site over three days has nowhere to keep 'the last id I saw was 4820' — its own context does not survive the session, and this does. actions: get, set, list, delete. Keys are path-like ('shop/last_id') so `list` with a prefix groups them."
    )]
    async fn web_notes(
        &self,
        params: Parameters<WebNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.notes_json(params.0).map_err(err)?)
    }

    #[tool(
        description = "What this installation has actually been doing: which tier answered, which wall appeared, how long it took, per domain. `view=summary` is the one that matters — a domain that is half blocked and slow is a domain whose learned tier is wrong, and nothing else notices that on its own."
    )]
    async fn web_log(&self, params: Parameters<WebLogParams>) -> Result<CallToolResult, McpError> {
        ok(self.log_json(params.0).map_err(err)?)
    }

    pub fn log_json(&self, p: WebLogParams) -> anyhow::Result<Value> {
        let Some(store) = self.store.as_ref() else {
            anyhow::bail!("the log needs the database, which could not be opened");
        };
        let since = p.since_secs.unwrap_or(3_600).max(0);
        match p.view.as_deref().unwrap_or("recent") {
            "summary" => {
                let rows: Vec<Value> = store
                    .request_summary(since)
                    .into_iter()
                    .map(|(domain, requests, blocked, avg_ms)| {
                        json!({
                            "domain": domain,
                            "requests": requests,
                            "blocked": blocked,
                            "avg_ms": avg_ms,
                        })
                    })
                    .collect();
                Ok(json!({"view": "summary", "since_secs": since, "domains": rows}))
            }
            "recent" => {
                let lines = store.recent_requests(
                    p.domain.as_deref(),
                    since,
                    p.limit.unwrap_or(50).clamp(1, 500),
                );
                Ok(json!({
                    "view": "recent",
                    "since_secs": since,
                    "count": lines.len(),
                    "requests": lines,
                }))
            }
            other => anyhow::bail!("unknown view '{other}'; use recent or summary"),
        }
    }

    #[tool(
        description = "Search a site using its own search box. A crawler only reaches what a site links to, and on a shop or a job board most of the content is only shown to somebody who asks for it. Returns the URL pattern the form produces — `/search?q=...` — which is the real prize: every later query is then an ordinary web_fetch with no browser at all."
    )]
    async fn web_site_search(
        &self,
        params: Parameters<WebSiteSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.site_search_json(params.0).await.map_err(err)?)
    }

    pub async fn site_search_json(&self, p: WebSiteSearchParams) -> anyhow::Result<Value> {
        use svipall_core::forms::{SearchForm, FIND_SEARCH_FORM_JS};
        if let Err(why) = self.origin_policy().check(&p.url) {
            anyhow::bail!("{why}");
        }
        let opts = PageOpts {
            mobile: false,
            tier: BrowserTier::Stealth,
            profile_dir: None,
            proxy: self.exit_for(&domain_from_url(&p.url)),
            visible: false,
            identity_seed: identity_seed_for(None, &p.url, None),
        };
        self.charge_visit(&p.url, opts.tier, opts.proxy.as_deref())
            .await?;
        let (_pooled, page) = self.pool.page(&opts).await?;
        let found = async {
            self.pool.navigate(&page, &p.url).await?;
            let v = page
                .evaluate(FIND_SEARCH_FORM_JS)
                .await
                .ok()
                .and_then(|r| r.value().cloned())
                .unwrap_or(Value::Null);
            Ok::<_, anyhow::Error>(v)
        }
        .await;
        self.pool.close_page(page).await;
        let found = found?;
        if found.is_null() {
            return Ok(json!({
                "url": p.url,
                "found": false,
                "note": "no search box on this page. Try the home page, or web_map to see whether \
                         the site publishes a sitemap instead.",
            }));
        }
        let form = SearchForm {
            action: found["action"].as_str().unwrap_or_default().to_string(),
            method: found["method"].as_str().unwrap_or_default().to_string(),
            field: found["field"].as_str().unwrap_or_default().to_string(),
            hidden: found["hidden"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|r| {
                            Some((
                                r.get(0)?.as_str()?.to_string(),
                                r.get(1)?.as_str().unwrap_or_default().to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        let Some(pattern) = svipall_core::forms::url_for(&p.url, &form, &p.query) else {
            // A POST form, or one with no named field. Saying so is the useful answer: turning it
            // into a GET would return the home page with a 200, which reads as success.
            return Ok(json!({
                "url": p.url,
                "found": true,
                "usable_as_url": false,
                "method": form.method,
                "field": form.field,
                "note": "the search box submits by POST, so there is no URL for it. Use web_act to \
                         type into it and press Enter.",
            }));
        };
        let mut out = json!({
            "url": p.url,
            "found": true,
            "usable_as_url": true,
            "field": form.field,
            "results_url": pattern,
            "note": "Every later query is this URL with the field changed — an ordinary web_fetch, \
                     no browser needed.",
        });
        if p.fetch.unwrap_or(true) {
            let results = self
                .fetch_json(WebFetchParams {
                    url: pattern.clone(),
                    timeout: p.timeout,
                    ..Default::default()
                })
                .await;
            out.as_object_mut()
                .expect("object")
                .insert("results".into(), results.value);
        }
        Ok(out)
    }

    #[tool(
        description = "Watch a page and report when it changes. `add` starts watching, `list` shows what changed and when, `check` looks now. The comparison is a content hash, so a check that finds nothing costs one conditional request and no parsing. Watches survive restarts; they are only checked while the server is running, and a check that is late says so rather than pretending."
    )]
    async fn web_watch(
        &self,
        params: Parameters<WebWatchParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.watch_json(params.0).await.map_err(err)?)
    }

    pub async fn watch_json(&self, p: WebWatchParams) -> anyhow::Result<Value> {
        use svipall_core::watch::{Watch, PREFIX};
        let Some(store) = self.store.as_ref() else {
            anyhow::bail!("watches need the database, which could not be opened");
        };
        let load_all = || -> Vec<Watch> {
            store
                .kv_list(PREFIX)
                .into_iter()
                .filter_map(|(_, v)| serde_json::from_str::<Watch>(&v).ok())
                .collect()
        };
        match p.action.as_deref().unwrap_or("add") {
            "add" => {
                let Some(url) = p.url.as_deref() else {
                    anyhow::bail!("add needs a url");
                };
                if let Err(why) = self.origin_policy().check(url) {
                    anyhow::bail!("{why}");
                }
                let mut w = Watch::new(url, p.interval_secs.unwrap_or(3_600));
                w.label = p.label.clone().unwrap_or_default();
                w.css_selector = p.css_selector.clone().filter(|s| !s.trim().is_empty());
                // Adding a page already watched keeps what it has learned rather than forgetting
                // it: the point of a watch is the history.
                if let Some(existing) = store
                    .kv_get(&w.key())
                    .and_then(|v| serde_json::from_str::<Watch>(&v).ok())
                {
                    w.last_checked = existing.last_checked;
                    w.last_changed = existing.last_changed;
                    w.last_hash = existing.last_hash;
                    w.changes = existing.changes;
                }
                store.kv_set(&w.key(), &serde_json::to_string(&w)?)?;
                Ok(json!({"watching": w.url, "interval_secs": w.interval_secs, "added": true}))
            }
            "remove" => {
                let Some(url) = p.url.as_deref() else {
                    anyhow::bail!("remove needs a url");
                };
                let key = Watch::new(url, 60).key();
                Ok(json!({"url": url, "removed": store.kv_delete(&key)}))
            }
            "list" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let rows: Vec<Value> = load_all()
                    .into_iter()
                    .map(|w| {
                        json!({
                            "url": w.url,
                            "label": w.label,
                            "interval_secs": w.interval_secs,
                            "last_checked": w.last_checked,
                            "last_changed": w.last_changed,
                            "changes": w.changes,
                            "due": w.due(now),
                            "overdue_by_secs": w.overdue_by(now),
                        })
                    })
                    .collect();
                Ok(json!({"count": rows.len(), "watches": rows}))
            }
            "check" => {
                let watches = match p.url.as_deref() {
                    Some(url) => {
                        let key = Watch::new(url, 60).key();
                        store
                            .kv_get(&key)
                            .and_then(|v| serde_json::from_str::<Watch>(&v).ok())
                            .map(|w| vec![w])
                            .unwrap_or_default()
                    }
                    None => load_all(),
                };
                let changed = self.check_watches(watches).await;
                Ok(json!({"checked": changed.len(), "results": changed}))
            }
            other => anyhow::bail!("unknown action '{other}'; use add, list, remove or check"),
        }
    }

    /// Fetch each watch and record what it found.
    ///
    /// Sequential and through the ordinary ladder, so a watched page behind a wall is handled the
    /// same way any other page is, and a run of watches paces itself like a person rather than
    /// arriving as a burst.
    async fn check_watches(&self, watches: Vec<svipall_core::watch::Watch>) -> Vec<Value> {
        let Some(store) = self.store.as_ref() else {
            return Vec::new();
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut out = Vec::new();
        for mut w in watches {
            // A region watch is a one-field schema: the region's markdown is the item, and the
            // selector gets the same fingerprint memory and relocation a schema field gets.
            let schema = w.css_selector.as_ref().map(|sel| {
                json!({
                    "name": format!("watch/{}", svipall_core::domain::stable_hash(&w.url)),
                    "base_selector": sel,
                    "fields": [{"name": "content", "type": "markdown"}],
                })
            });
            let res = self
                .fetch_json(WebFetchParams {
                    url: w.url.clone(),
                    cache: Some("refresh".into()),
                    schema,
                    ..Default::default()
                })
                .await;
            // A blocked page is not an unchanged page, and recording it as one would make a wall
            // look like stability.
            if res.value["blocked_reason"].is_string() {
                out.push(json!({
                    "url": w.url,
                    "changed": false,
                    "blocked": res.value["blocked_reason"],
                }));
                continue;
            }
            let watched: String = match w.css_selector {
                Some(_) => res.value["extracted"]["items"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|i| i["content"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
                None => res.value["content"].as_str().unwrap_or("").to_string(),
            };
            let hash = svipall_core::domain::stable_hash(&watched);
            let changed = w.observe(hash, now);
            let _ = serde_json::to_string(&w).map(|v| store.kv_set(&w.key(), &v));
            let mut row = json!({
                "url": w.url,
                "label": w.label,
                "changed": changed,
                "changes": w.changes,
            });
            if let Some(sel) = &w.css_selector {
                row["css_selector"] = json!(sel);
                if let Some(h) = res.value.get("healed") {
                    row["healed"] = h.clone();
                }
                if watched.is_empty() {
                    row["note"] = json!("the selector matched nothing on this check");
                }
            }
            out.push(row);
        }
        out
    }

    #[tool(
        description = "Move a logged-in profile between machines. web_login is a person passing a challenge by hand — the most expensive thing this tool asks for — and until now the result lived on one machine only. The archive is encrypted with a password you supply; there is no unencrypted form, because a profile is the session. Caches are left behind."
    )]
    async fn web_profile(
        &self,
        params: Parameters<WebProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.profile_json(params.0).map_err(err)?)
    }

    pub fn profile_json(&self, p: WebProfileParams) -> anyhow::Result<Value> {
        match p.action.as_deref().unwrap_or("list") {
            "list" => {
                let dir = crate::browser::profiles_dir();
                let names: Vec<String> = std::fs::read_dir(dir)
                    .map(|entries| {
                        entries
                            .flatten()
                            .filter(|e| e.path().is_dir())
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json!({"count": names.len(), "profiles": names}))
            }
            action @ ("export" | "import") => {
                let Some(name) = p.name.as_deref() else {
                    anyhow::bail!("{action} needs a profile name");
                };
                let Some(file) = p.file.as_deref() else {
                    anyhow::bail!("{action} needs a file");
                };
                let password = p.password.as_deref().unwrap_or_default();
                let profile = crate::browser::named_profile(name);
                let archive = out_path(file)?;
                if action == "export" {
                    let (files, bytes) = crate::profiles::export(&profile, &archive, password)?;
                    Ok(json!({
                        "exported": name,
                        "file": archive.to_string_lossy(),
                        "files": files,
                        "bytes": bytes,
                    }))
                } else {
                    let files = crate::profiles::import(&archive, &profile, password)?;
                    Ok(json!({
                        "imported": name,
                        "from": archive.to_string_lossy(),
                        "files": files,
                    }))
                }
            }
            other => anyhow::bail!("unknown action '{other}'; use list, export or import"),
        }
    }

    /// Queue the sitemap's URLs, minus the ones the site says have not moved.
    ///
    /// Returns (queued, skipped). Every failure ends at "queue nothing and let the ordinary crawl
    /// happen": a missing sitemap, an unreachable one, or one that parses to nothing must never
    /// turn an incremental crawl into no crawl.
    async fn seed_from_sitemap(
        &self,
        start: &str,
        frontier: &mut svipall_core::frontier::Frontier,
    ) -> (usize, usize) {
        use svipall_core::incremental::{decide, Entry};
        let Some(store) = self.store.as_ref() else {
            return (0, 0);
        };
        let Ok(parsed) = url::Url::parse(start) else {
            return (0, 0);
        };
        // `host_str` drops the port, and a site served on one would have its sitemap looked for on
        // the default port instead — a request to a different server, or to nothing at all.
        let host = parsed.host_str().unwrap_or_default();
        let origin = match parsed.port() {
            Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
            None => format!("{}://{host}", parsed.scheme()),
        };
        let (mut queued, mut skipped) = (0usize, 0usize);
        for guess in svipall_core::sitemap::SITEMAP_GUESSES {
            let out = self
                .fetch_json(WebFetchParams {
                    url: format!("{origin}{guess}"),
                    mode: Some("http".into()),
                    robots: Some("ignore".into()),
                    extraction: Some("html".into()),
                    ..Default::default()
                })
                .await;
            let Some(body) = out.value["content"].as_str() else {
                continue;
            };
            let Ok(svipall_core::sitemap::Sitemap::Urls(entries)) =
                svipall_core::sitemap::parse(body.as_bytes(), 5_000)
            else {
                continue;
            };
            for e in entries {
                let entry = Entry {
                    url: e.url.clone(),
                    lastmod: e.lastmod.clone(),
                };
                if !decide(&entry, store.last_fetched(&e.url)).fetch() {
                    skipped += 1;
                    continue;
                }
                if frontier.push(svipall_core::frontier::Candidate {
                    url: e.url,
                    depth: 1,
                    source: svipall_core::frontier::Source::Sitemap,
                    anchor: String::new(),
                    parent_score: 0.5,
                    // The frontier scores freshness in seconds; the sitemap publishes a date.
                    lastmod: e
                        .lastmod
                        .as_deref()
                        .and_then(svipall_core::incremental::parse_time),
                }) {
                    queued += 1;
                }
            }
            if queued > 0 || skipped > 0 {
                break;
            }
        }
        (queued, skipped)
    }

    /// The notes, without the MCP wrapper, so the behaviour can be tested without one.
    pub fn notes_json(&self, p: WebNotesParams) -> anyhow::Result<Value> {
        let Some(store) = self.store.as_ref() else {
            anyhow::bail!(
                "notes need the database, which could not be opened; everything else still works"
            );
        };
        match p.action.as_deref().unwrap_or("get") {
            "set" => {
                let (Some(key), Some(value)) = (p.key.as_deref(), p.value.as_deref()) else {
                    anyhow::bail!("set needs both key and value");
                };
                store.kv_set(key, value)?;
                Ok(json!({"key": key, "stored": true, "bytes": value.len()}))
            }
            "list" => {
                let prefix = p.prefix.as_deref().or(p.key.as_deref()).unwrap_or("");
                let notes: Vec<Value> = store
                    .kv_list(prefix)
                    .into_iter()
                    .map(|(k, v)| json!({"key": k, "value": v}))
                    .collect();
                Ok(json!({"prefix": prefix, "count": notes.len(), "notes": notes}))
            }
            "delete" => {
                let Some(key) = p.key.as_deref() else {
                    anyhow::bail!("delete needs a key");
                };
                Ok(json!({"key": key, "deleted": store.kv_delete(key)}))
            }
            "get" => {
                let Some(key) = p.key.as_deref() else {
                    anyhow::bail!("get needs a key");
                };
                // Absent and empty are different answers: "" is something somebody stored, and
                // `found: false` is a question nobody has answered yet.
                match store.kv_get(key) {
                    Some(v) => Ok(json!({"key": key, "found": true, "value": v})),
                    None => Ok(json!({"key": key, "found": false})),
                }
            }
            other => anyhow::bail!("unknown action '{other}'; use get, set, list or delete"),
        }
    }

    #[tool(
        description = "Search the web without an API by scraping DuckDuckGo, Bing and Brave (first engine with results wins). Returns title, url, snippet."
    )]
    async fn web_search(
        &self,
        params: Parameters<WebSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.search_json(params.0).await)
    }

    /// `web_search` without the protocol wrapper, for the REST API and tests. Infallible: an engine
    /// that answered nothing is a search with no results, which `attempts` explains.
    pub async fn search_json(&self, p: WebSearchParams) -> Value {
        let limit = p.limit.unwrap_or(10).clamp(1, 50);
        let fetcher = self.fetcher.clone();
        // "all" asks every engine and merges. Each of these is being read off its own HTML and
        // each covers a different slice of the web badly, so taking the first that answers makes
        // the result depend on which engine happened to be up.
        let out = match p.engine.as_deref() {
            Some("all" | "merge") => search::search_all(fetcher.as_ref(), &p.query, limit).await,
            engine => search::search(fetcher.as_ref(), &p.query, limit, engine).await,
        };
        json!({"query": p.query, "engine": out.engine, "count": out.results.len(), "results": out.results, "attempts": out.attempts})
    }

    #[tool(
        description = "Save a PNG screenshot of a page rendered in a real browser (anti-bot handled like web_fetch). Returns the file path and, by default, the image inline."
    )]
    async fn web_screenshot(
        &self,
        params: Parameters<WebScreenshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let shot = self.screenshot_json(params.0).await.map_err(err)?;
        let mut contents = vec![Content::text(shot.value.to_string())];
        if shot.inline {
            contents.push(Content::image(
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &shot.png),
                "image/png",
            ));
        }
        Ok(CallToolResult::success(contents))
    }

    /// `web_screenshot` without the protocol wrapper, for the REST API and tests.
    ///
    /// Both protocols want the same picture in different shapes — MCP as a second `Content`, REST
    /// as base64 in the body — so the rule about *when* the picture is worth sending is decided
    /// here, once, rather than restated at each of them.
    pub async fn screenshot_json(&self, p: WebScreenshotParams) -> anyhow::Result<ShotOutcome> {
        let tier =
            BrowserTier::parse(p.tier.as_deref().unwrap_or("real")).unwrap_or(BrowserTier::Real);
        let domain = domain_from_url(&p.url);
        let proxy = p.proxy.clone().or_else(|| self.exit_for(&domain));
        let profile_dir = self.profile_dir_for(tier, &p.url, p.profile.as_deref());
        let opts = PageOpts {
            mobile: false,
            tier,
            identity_seed: identity_seed_for(profile_dir.as_deref(), &p.url, p.profile.as_deref()),
            profile_dir,
            proxy,
            visible: false,
        };
        let timeout = Duration::from_millis(p.timeout.unwrap_or(60_000));
        let fut = async {
            self.charge_visit(&p.url, opts.tier, opts.proxy.as_deref())
                .await?;
            let (_pooled, page) = self.pool.page(&opts).await?;
            let r = async {
                let status = self.pool.navigate(&page, &p.url).await?;
                self.pool.settle(&page, Duration::from_millis(800)).await;
                let png = self
                    .pool
                    .screenshot(&page, p.full_page.unwrap_or(false))
                    .await?;
                let final_url = page.url().await.ok().flatten().unwrap_or_default();
                Ok::<_, anyhow::Error>((status, png, final_url))
            }
            .await;
            self.pool.close_page(page).await;
            r
        };
        match tokio::time::timeout(timeout, fut).await {
            Ok(Ok((status, png, final_url))) => {
                let path = save_png(&p.url, &png)?;
                // Three megabytes is where a picture stops being worth carrying inline and starts
                // being worth fetching from `path`. Decided here so neither protocol restates it.
                let inline = p.inline.unwrap_or(true) && png.len() < 3_000_000;
                let value = json!({"url": p.url, "final_url": final_url, "status": status, "path": path, "bytes": png.len(), "tier_used": format!("{:?}", tier).to_lowercase()});
                Ok(ShotOutcome { value, png, inline })
            }
            // A page that would not render is a successful report of a page that would not render,
            // the same way `web_act` treats a failed interaction. There is no picture, so nothing
            // is inline.
            Ok(Err(e)) => Ok(ShotOutcome {
                value: json!({"url": p.url, "status": 0, "error": e.to_string()}),
                png: Vec::new(),
                inline: false,
            }),
            Err(_) => Ok(ShotOutcome {
                value: json!({"url": p.url, "status": 0, "error": "screenshot timed out"}),
                png: Vec::new(),
                inline: false,
            }),
        }
    }

    #[tool(
        description = "Open a persistent stealth browser session and return its session_id. Cookies and page state persist across browser_do calls until browser_close."
    )]
    async fn browser_open(
        &self,
        params: Parameters<BrowserOpenParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match self
            .pool
            .open_session(p.profile.as_deref(), p.proxy, p.visible.unwrap_or(false))
            .await
        {
            Ok(s) => ok(
                json!({"session_id": s.id, "tier": "real", "profile_dir": s.profile_dir.to_string_lossy(), "note": "Use browser_do with this session_id; browser_close when done."}),
            ),
            Err(e) => ok(json!({"error": e.to_string()})),
        }
    }

    #[tool(
        description = "Navigate and act inside a session opened with browser_open. Omit url to keep acting on the current page. Same actions as web_act."
    )]
    async fn browser_do(
        &self,
        params: Parameters<BrowserDoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let Some(s) = self.pool.session(&p.session_id).await else {
            return ok(json!({"error": format!("unknown session_id {}", p.session_id)}));
        };
        let timeout = Duration::from_millis(p.timeout.unwrap_or(90_000));
        let fut = async {
            let status = match &p.url {
                Some(u) => {
                    self.charge_visit(u, BrowserTier::Real, s.proxy.as_deref())
                        .await?;
                    Some(self.pool.navigate(&s.page, u).await?)
                }
                None => None,
            };
            let results = self
                .pool
                .run_actions(&s.page, p.actions.as_deref().unwrap_or(&[]))
                .await;
            let (html, final_url) = self.pool.content(&s.page).await?;
            Ok::<_, anyhow::Error>((status, results, html, final_url))
        };
        match tokio::time::timeout(timeout, fut).await {
            Ok(Ok((status, results, html, final_url))) => {
                let fp = WebFetchParams {
                    url: final_url.clone(),
                    query: p.query,
                    ..Default::default()
                };
                let kind = p.extraction.as_deref().unwrap_or("markdown");
                let wants = Self::wants_for(
                    &fp,
                    kind,
                    "text/html",
                    &final_url,
                    false,
                    Default::default(),
                );
                let parts = extraction::parse_page(&html, &wants);
                let content =
                    Self::render_content(&parts, &html, kind, "text/html", fp.query.as_ref());
                ok(
                    json!({"session_id": p.session_id, "url": final_url, "status": status, "title": parts.title, "actions": results, "chars": content.chars().count(), "content": content}),
                )
            }
            Ok(Err(e)) => ok(json!({"session_id": p.session_id, "error": e.to_string()})),
            Err(_) => ok(json!({"session_id": p.session_id, "error": "browser_do timed out"})),
        }
    }

    #[tool(
        description = "Manage the browser svipall uses for the browser/stealth/real/warm tiers. action=status (default) reports which binary would run and why; action=install downloads Chrome for Testing (~190 MB) when the machine has no suitable browser; action=update replaces it with the current Stable; action=remove deletes it. Nothing is downloaded unless you ask."
    )]
    async fn browser_setup(
        &self,
        params: Parameters<BrowserSetupParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.browser_setup_json(params.0).await.map_err(err)?)
    }

    /// The browser manager without the MCP wrapper, so the CLI can install one too.
    pub async fn browser_setup_json(&self, p: BrowserSetupParams) -> anyhow::Result<Value> {
        let prov = crate::provision::Provisioner::new(self.fetcher.clone(), p.artifact.as_deref());
        let action = p.action.as_deref().unwrap_or("status");
        let detected = self.pool.executable();
        match action {
            "status" => {
                let latest = prov.latest_stable().await.ok();
                // Learned here, spent later: every other caller reads this from the store rather
                // than reaching the network to phrase a warning.
                if let Some(r) = latest.as_ref() {
                    self.remember_latest_stable(&r.version);
                }
                Ok(json!({
                    "in_use": detected,
                    "chrome_major": self.pool.browser_major(),
                    "source": if detected.as_deref().map(|d| d.contains(".svipall")).unwrap_or(false) { "downloaded" } else if detected.is_some() { "detected" } else { "none" },
                    "managed_install": prov.installed(),
                    "latest_stable": latest.as_ref().map(|r| r.version.clone()),
                    "advice": self.pool.advice(self.latest_stable_major()),
                    "platform": crate::provision::platform(),
                    "note": if detected.is_some() { "a browser is available; nothing to do" } else { "call browser_setup(action=\"install\") to download one" },
                }))
            }
            "install" | "update" => {
                let release = prov.latest_stable().await?;
                self.remember_latest_stable(&release.version);
                // Progress is collected rather than streamed: it lands in the tool result so the
                // caller can see what a multi-minute download actually did.
                let mut log: Vec<String> = Vec::new();
                let installed = {
                    let mut sink = |line: String| {
                        tracing::info!("browser_setup: {line}");
                        log.push(line);
                    };
                    prov.install(&release, &mut sink).await?
                };
                svipall_core::config::update_in(
                    &svipall_core::config::home_dir(),
                    json!({"browser_path":installed.exe}),
                )?;
                Ok(json!({
                    "installed": installed,
                    "progress": log,
                    "note": "the next request uses the installed browser; existing sessions keep their browser",
                }))
            }
            "remove" => {
                let freed = prov.remove_all()?;
                let mut patch = json!({"browser_auto_install":false});
                if std::path::Path::new(&self.cfg.browser_path)
                    .starts_with(crate::browser::managed_browser_dir())
                {
                    patch["browser_path"] = json!("");
                }
                svipall_core::config::update_in(&svipall_core::config::home_dir(), patch)?;
                Ok(json!({"removed": true, "freed_bytes": freed}))
            }
            other => anyhow::bail!("unknown action '{other}' (status|install|update|remove)"),
        }
    }

    #[tool(
        description = "Solve a captcha on the blocked page itself and return the page behind it. Use this instead of solve_turnstile / solve_recaptcha_v2 / solve_hcaptcha whenever what you want is the content: those return a bare token, and a token is bound to the session and IP that produced it, so it rarely works anywhere else. Non-interactive widgets clear on their own; anything needing a person opens a visible window."
    )]
    async fn solve_and_continue(
        &self,
        params: Parameters<SolveAndContinueParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.solve_and_continue_json(params.0).await.map_err(err)?)
    }

    /// `solve_and_continue` without the protocol wrapper, for the REST API and tests.
    pub async fn solve_and_continue_json(
        &self,
        p: SolveAndContinueParams,
    ) -> anyhow::Result<Value> {
        self.charge_visit(&p.url, BrowserTier::Real, None).await?;
        let engine = match &self.solver_state {
            Some(st) => SolveEngine::with_state(self.pool.clone(), &self.cfg, st.clone()),
            None => SolveEngine::new(self.pool.clone(), &self.cfg),
        };
        let wait = Duration::from_secs(p.timeout_s.unwrap_or(120));
        let (html, final_url, solved) = engine
            .solve_in_place(&p.url, p.profile.as_deref(), wait)
            .await?;

        let kind = p.extraction.as_deref().unwrap_or("markdown");
        let fp = WebFetchParams {
            url: final_url.clone(),
            extraction: Some(kind.to_string()),
            max_tokens: p.max_tokens,
            ..Default::default()
        };
        let wants = Self::wants_for(
            &fp,
            kind,
            "text/html",
            &final_url,
            false,
            Default::default(),
        );
        let parts = extraction::parse_page(&html, &wants);
        let view = PageView::new(&html, &parts.text);
        let (reason, wall) = classify_view(200, &html, &view);

        let mut value = json!({
            "url": p.url,
            "final_url": final_url,
            "solved": solved,
            "still_blocked": reason.is_some(),
            "wall_kind": format!("{:?}", wall).to_lowercase(),
            "title": parts.title,
        });
        if let Some(r) = reason {
            let obj = value.as_object_mut().expect("object");
            obj.insert("blocked_reason".into(), json!(r));
            obj.insert(
                "note".into(),
                json!(format!(
                    "the challenge did not clear. {}",
                    guidance(
                        &wall,
                        &domain_from_url(&final_url),
                        "warm",
                        self.solver_state.is_some(),
                        self.dashboard()
                    )
                )),
            );
        }
        let content = Self::render_content(&parts, &html, kind, "text/html", None);
        self.budget_into(&mut value, content, &fp);
        Ok(value)
    }

    #[tool(
        description = "What changed on a page since svipall last saw it. Compares against the cached copy: `changed` says whether the content differs, `similarity` by how much, and `added`/`removed` list the markdown blocks that appeared or went away. Cheap — the comparison is a stored fingerprint, not a second copy of the page."
    )]
    async fn web_diff(
        &self,
        params: Parameters<WebDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.diff_json(params.0).await.map_err(err)?)
    }

    /// `web_diff` without the protocol wrapper, for the REST API and tests.
    pub async fn diff_json(&self, p: WebDiffParams) -> anyhow::Result<Value> {
        let Some(store) = self.store.clone() else {
            anyhow::bail!("the page cache is unavailable, so there is nothing to compare against");
        };
        let before = store.get(&p.url);

        if p.refetch.unwrap_or(true) {
            // `refresh` forces a fetch and stores the result, which is what produces the comparison.
            let fetch = WebFetchParams {
                url: p.url.clone(),
                cache: Some("refresh".into()),
                ..Default::default()
            };
            let _ = self.fetch_json(fetch).await;
        }
        let after = store.get(&p.url);

        let (Some(before), Some(after)) = (before, after) else {
            return Ok(json!({
                "url": p.url,
                "changed": true,
                "first_seen": true,
                "note": "no earlier copy to compare against; this fetch is now the baseline",
            }));
        };

        let changed = before.content_hash != after.content_hash;
        // A set difference over blocks, not a longest-common-subsequence: reordering a page is
        // noise to a reader, and LCS is quadratic for no gain here.
        let old_blocks: HashSet<&str> = budget::blocks(&before.markdown).into_iter().collect();
        let new_blocks: HashSet<&str> = budget::blocks(&after.markdown).into_iter().collect();
        let added: Vec<&&str> = new_blocks.difference(&old_blocks).take(40).collect();
        let removed: Vec<&&str> = old_blocks.difference(&new_blocks).take(40).collect();

        Ok(json!({
            "url": p.url,
            "changed": changed,
            "similarity": svipall_core::dedup::similarity(before.simhash, after.simhash),
            "previous_fetched_at": before.fetched_at,
            "fetched_at": after.fetched_at,
            "title_changed": before.title != after.title,
            "added": added,
            "removed": removed,
            "versions_known": store.versions(&p.url, 20).len(),
        }))
    }

    #[tool(
        description = "Map a site's URLs without crawling it: robots.txt, sitemaps (including nested indexes and .gz), RSS/Atom feeds, and the homepage links. Returns a few hundred tokens of structure instead of the many thousands a crawl would cost. Use it to decide what is worth fetching."
    )]
    async fn web_map(&self, params: Parameters<WebMapParams>) -> Result<CallToolResult, McpError> {
        ok(self.map_json(params.0).await.map_err(err)?)
    }

    /// The map itself, so the CLI and the tests can call it without the MCP wrapper.
    pub async fn map_json(&self, p: WebMapParams) -> anyhow::Result<Value> {
        let limit = p.limit.unwrap_or(1000).min(50_000);
        let sources: Vec<String> = p.sources.unwrap_or_else(|| {
            ["robots", "sitemap", "feeds", "links"]
                .map(String::from)
                .to_vec()
        });
        let want = |s: &str| sources.iter().any(|x| x.eq_ignore_ascii_case(s));

        let base = url::Url::parse(&p.url).map_err(|e| err(format!("bad url: {e}")))?;
        let origin = format!(
            "{}://{}",
            base.scheme(),
            base.host_str().unwrap_or_default()
        );
        let mut used: Vec<&str> = Vec::new();
        let mut urls: Vec<Value> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut sitemaps: Vec<String> = Vec::new();
        let mut feeds: Vec<String> = Vec::new();
        let mut robots_info = json!({"present": false});

        let get = |u: String| async move {
            let r = self
                .fetch_json(WebFetchParams {
                    url: u,
                    extraction: Some("html".into()),
                    max_tier: Some("http".into()),
                    ..Default::default()
                })
                .await;
            r.value
                .get("content")
                .and_then(|c| c.as_str())
                .map(str::to_string)
        };

        let mut robots: Option<svipall_core::Robots> = None;
        if want("robots") || want("sitemap") {
            if let Some(body) = get(format!("{origin}/robots.txt")).await {
                let parsed = svipall_core::Robots::parse(&body);
                robots_info = json!({
                    "present": true,
                    "allowed": parsed.allows("svipall", base.path()),
                    "crawl_delay_ms": parsed.crawl_delay("svipall").map(|d| d.as_millis() as u64),
                    "sitemaps": parsed.sitemaps.clone(),
                });
                sitemaps.extend(parsed.sitemaps.clone());
                robots = Some(parsed);
                used.push("robots");
            }
        }

        if want("sitemap") {
            // Whatever robots.txt named, then the conventional locations.
            let mut queue: Vec<String> = sitemaps.clone();
            if queue.is_empty() {
                queue.extend(
                    svipall_core::sitemap::SITEMAP_GUESSES
                        .iter()
                        .map(|g| format!("{origin}{g}")),
                );
            }
            let mut documents = 0usize;
            // Bounded: a nested index could otherwise walk a site forever.
            while let Some(next) = queue.pop() {
                if documents >= 50 || urls.len() >= limit {
                    break;
                }
                let Some(body) = get(next.clone()).await else {
                    continue;
                };
                documents += 1;
                match svipall_core::sitemap::parse(body.as_bytes(), limit) {
                    Ok(svipall_core::sitemap::Sitemap::Index(children)) => {
                        if !sitemaps.contains(&next) {
                            sitemaps.push(next);
                        }
                        queue.extend(children.into_iter().take(50));
                    }
                    Ok(svipall_core::sitemap::Sitemap::Urls(entries)) => {
                        if !sitemaps.contains(&next) {
                            sitemaps.push(next);
                        }
                        for e in entries {
                            if urls.len() >= limit || !seen.insert(e.url.clone()) {
                                continue;
                            }
                            urls.push(
                                json!({"url": e.url, "lastmod": e.lastmod, "source": "sitemap"}),
                            );
                        }
                    }
                    Err(_) => {}
                }
            }
            if !urls.is_empty() {
                used.push("sitemap");
            }
        }

        if want("feeds") {
            for guess in svipall_core::sitemap::FEED_GUESSES {
                if urls.len() >= limit {
                    break;
                }
                let Some(body) = get(format!("{origin}{guess}")).await else {
                    continue;
                };
                if let Ok(items) = svipall_core::sitemap::parse_feed(body.as_bytes(), 200) {
                    if items.is_empty() {
                        continue;
                    }
                    feeds.push(format!("{origin}{guess}"));
                    for it in items {
                        if urls.len() >= limit || !seen.insert(it.url.clone()) {
                            continue;
                        }
                        urls.push(json!({
                            "url": it.url, "title": it.title,
                            "lastmod": it.published, "source": "feed"
                        }));
                    }
                }
            }
            if !feeds.is_empty() {
                used.push("feeds");
            }
        }

        if want("links") && urls.len() < limit {
            let out = self
                .fetch_json_opts(
                    WebFetchParams {
                        url: p.url.clone(),
                        ..Default::default()
                    },
                    true,
                )
                .await;
            for link in out.links {
                if urls.len() >= limit || !seen.insert(link.clone()) {
                    continue;
                }
                urls.push(json!({"url": link, "source": "links"}));
            }
            used.push("links");
        }

        if let Some(inc) = &p.include {
            urls.retain(|u| u["url"].as_str().unwrap_or("").contains(inc.as_str()));
        }
        // Anything robots.txt disallows is flagged rather than removed: the caller may have a
        // reason to look at it, and hiding it would be a silent edit of their results.
        if let Some(r) = &robots {
            for u in &mut urls {
                if let Some(s) = u["url"].as_str() {
                    if let Ok(parsed) = url::Url::parse(s) {
                        let path = match parsed.query() {
                            Some(q) => format!("{}?{}", parsed.path(), q),
                            None => parsed.path().to_string(),
                        };
                        if !r.allows("svipall", &path) {
                            u["robots_disallowed"] = json!(true);
                        }
                    }
                }
            }
        }

        Ok(json!({
            "domain": domain_from_url(&p.url),
            "sources_used": used,
            "count": urls.len(),
            "urls": urls,
            "sitemaps": sitemaps,
            "feeds": feeds,
            "robots": robots_info,
        }))
    }

    #[tool(description = "Close a browser session opened with browser_open.")]
    async fn browser_close(
        &self,
        params: Parameters<BrowserSessionParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.pool.close_session(&params.0.session_id).await {
            Ok(()) => ok(json!({"closed": params.0.session_id})),
            Err(e) => ok(json!({"error": e.to_string()})),
        }
    }

    #[tool(
        description = "Open a visible browser window for a manual login or challenge, then save the cookies in a profile. Close the window when done. Without profile, the domain's auto profile is used and real/warm tiers pick it up automatically."
    )]
    async fn web_login(
        &self,
        params: Parameters<WebLoginParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        if !self.pool.available() {
            return ok(json!({"error": no_browser_hint(), "wall_kind": "no_browser"}));
        }
        let domain = domain_from_url(&p.url);
        let (name, dir) = match &p.profile {
            Some(n) => (n.clone(), named_profile(n)),
            None => (
                format!("auto:{}", domain),
                PathBuf::from(svipall_core::auto_profile_path(&p.url, true)),
            ),
        };
        let timeout = Duration::from_secs(p.timeout_s.unwrap_or(300).clamp(10, 3600));
        match self.pool.login(&p.url, dir.clone(), timeout).await {
            Ok(closed) => ok(
                json!({"profile": name, "path": dir.to_string_lossy(), "closed_by_user": closed,
                "note": if p.profile.is_some() { "Pass profile=NAME to web_fetch / web_act / browser_open to reuse these cookies." } else { "Saved in the domain auto profile: real and warm tiers use it automatically." }}),
            ),
            Err(e) => ok(json!({"error": e.to_string()})),
        }
    }

    #[tool(
        description = "Route one domain through a proxy from now on (subdomains inherit), remove a route, list routes, or check=true to test the exits (liveness, latency, DNS-leak) without any third-party service."
    )]
    async fn web_route(
        &self,
        params: Parameters<WebRouteParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.route_json(params.0).await.map_err(err)?)
    }

    /// `web_route` without the protocol wrapper, for the CLI and tests.
    pub async fn route_json(&self, p: WebRouteParams) -> anyhow::Result<Value> {
        if p.check.unwrap_or(false) {
            return self.check_exits(&p).await;
        }
        let home = svipall_core::config::home_dir();
        let _ = std::fs::create_dir_all(&home);
        let file = home.join("proxies.json");
        let mut table = self.routes();
        if let Some(domain) = p.domain {
            let domain = domain.trim().trim_start_matches("www.").to_lowercase();
            if p.remove.unwrap_or(false) {
                table.remove(&domain);
                svipall_core::exits::set_pool(&domain, &[]);
                svipall_core::exits::forget(&domain);
            } else if let Some(pool) = p.proxies.filter(|p| !p.is_empty()) {
                // A pool: every exit's country is declared alongside it, by position.
                let countries = p.countries.unwrap_or_default();
                for (i, proxy) in pool.iter().enumerate() {
                    let cc = countries.get(i).or(p.country.as_ref());
                    if let Some(cc) = cc {
                        if !svipall_core::store::set_proxy_region(proxy, cc) {
                            anyhow::bail!(
                                "unknown country {cc:?}. Known: {}",
                                svipall_core::geo::known_countries()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                    }
                }
                svipall_core::exits::set_pool(&domain, &pool);
                svipall_core::exits::forget(&domain);
                table.insert(domain, pool[0].clone());
            } else if let Some(proxy) = p.proxy {
                svipall_core::exits::set_pool(&domain, &[]);
                if let Some(cc) = p.country.as_deref() {
                    if !svipall_core::store::set_proxy_region(&proxy, cc) {
                        anyhow::bail!(
                            "unknown country {cc:?}. Known: {}",
                            svipall_core::geo::known_countries()
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
                table.insert(domain, proxy);
            }
            let _ = std::fs::write(
                &file,
                serde_json::to_string_pretty(&table).unwrap_or_default(),
            );
        }
        Ok(json!({
            "routes": table,
            "pools": svipall_core::exits::pools(),
            "exit_strategy": self.cfg.exit_strategy,
            "file": file.to_string_lossy(),
            "proxy_regions": svipall_core::store::PROXY_REGIONS.as_map(),
        }))
    }

    /// Check the exits configured for a domain, without any third-party service.
    ///
    /// Coherence, not location: does the exit answer, how fast, does its scheme leak DNS. The
    /// country is what the operator declared; nothing here calls a geolocation API. A `socks5://`
    /// exit is flagged because it resolves names on this machine — the fix is `socks5h://`, which
    /// resolves at the exit, and svipall says so rather than leaking quietly.
    async fn check_exits(&self, p: &WebRouteParams) -> anyhow::Result<Value> {
        let domain = p
            .domain
            .as_deref()
            .map(|d| d.trim().trim_start_matches("www.").to_lowercase());
        let exits: Vec<String> = match &domain {
            Some(d) => svipall_core::exits::exits_for(d),
            // No domain: check every configured route.
            None => self.routes().into_values().collect(),
        };
        if exits.is_empty() {
            anyhow::bail!(
                "no exits configured{}",
                domain.map(|d| format!(" for {d}")).unwrap_or_default()
            );
        }
        // A neutral, cheap endpoint the operator can override. Not wired to any provider: it is
        // just a page fetched through the proxy to prove the proxy carries traffic.
        let url = p
            .check_url
            .clone()
            .unwrap_or_else(|| "https://example.com/".to_string());
        let mut results = Vec::new();
        for proxy in exits {
            let scheme_leak = proxy.starts_with("socks5://");
            let identity =
                svipall_core::exits::identity_for_exit(&self.http_identity, Some(&proxy));
            let fetcher = build_fetcher(
                &identity,
                Engine::resolve(&self.cfg.http_engine),
                Some(&proxy),
            );
            let started = std::time::Instant::now();
            let req = HttpRequest {
                url: url.clone(),
                method: "GET".into(),
                headers: identity.nav_headers(),
                body: None,
            };
            let outcome = tokio::time::timeout(Duration::from_secs(20), fetcher.send(req)).await;
            let (ok, status, ms, error) = match outcome {
                Ok(Ok(resp)) => (
                    (200..500).contains(&resp.status),
                    resp.status,
                    started.elapsed().as_millis() as u64,
                    None,
                ),
                Ok(Err(e)) => (
                    false,
                    0,
                    started.elapsed().as_millis() as u64,
                    Some(e.to_string()),
                ),
                Err(_) => (false, 0, 20_000, Some("timed out".to_string())),
            };
            let region = svipall_core::store::region_for_proxy(&proxy).map(|r| r.country);
            results.push(json!({
                "exit": proxy,
                "ok": ok,
                "status": status,
                "ms": ms,
                "declared_country": region,
                "dns_leak": scheme_leak,
                "error": error,
                "note": scheme_leak.then_some("socks5:// resolves DNS on this machine; use socks5h:// so the exit resolves it"),
            }));
        }
        Ok(json!({
            "checked": url,
            "domain": domain,
            "exits": results,
        }))
    }

    #[tool(
        description = "Show learned tiers, cooldowns, proxy routes, profiles, open browsers/sessions and solver stats. clear_cooldown=DOMAIN or forget_tier=DOMAIN to reset."
    )]
    async fn web_status(
        &self,
        params: Parameters<WebStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        ok(self.status_json(params.0).await.map_err(err)?)
    }

    /// What this installation has learned, without the MCP wrapper.
    pub async fn status_json(&self, params: WebStatusParams) -> anyhow::Result<Value> {
        if let Some(patch) = &params.configure {
            let object = patch
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("configure must be an object"))?;
            for key in object.keys() {
                anyhow::ensure!(
                    [
                        "browser_identity",
                        "auto_native_fallback",
                        "auto_max_attempts",
                        "request_limit",
                        "request_window_seconds",
                        "request_cooldown_seconds",
                        "request_min_interval_ms",
                        "browser_auto_install",
                        "browser_path",
                        "max_tier",
                        "browser_timeout_ms",
                        "warm_wait_ms",
                        "warm_max_wait_ms",
                        "warm_adaptive",
                        "warm_keep_max",
                        "warm_keep_secs",
                        "browser_idle_secs",
                        "parallelism",
                        "locale",
                        "timezone",
                        "block_ads",
                        "http_engine",
                        "http_firefox",
                        "http3"
                    ]
                    .contains(&key.as_str()),
                    "{key} is not a live browser policy setting; configure it through the CLI"
                );
            }
            let cfg =
                svipall_core::config::update_in(&svipall_core::config::home_dir(), patch.clone())?;
            let mut value = serde_json::to_value(cfg)?;
            value["api_key"] = json!("[redacted]");
            return Ok(
                json!({"saved":true,"config":value,"applies":"next request; existing sessions retain their policy"}),
            );
        }
        if let Some(d) = &params.clear_cooldown {
            svipall_core::clear_cooldown(d);
            // An operator resetting a domain means a clean start, and a page held from before the
            // block is not one.
            self.pool
                .release_kept(|k| k.contains(&format!("|{d}|")))
                .await;
        }
        if let Some(d) = &params.forget_tier {
            svipall_core::forget_tier(d);
            svipall_core::automatic::forget(d);
        }
        if let Some(d) = &params.clear_budget {
            svipall_core::reputation::clear(d);
        }
        let mut cleared = None;
        if let (Some(what), Some(store)) = (&params.clear_cache, &self.store) {
            cleared = Some(match what {
                Value::String(domain) => store.clear(Some(domain)),
                _ => store.clear(None),
            });
        }
        let home = svipall_core::config::home_dir();
        let profiles: Vec<String> = std::fs::read_dir(svipall_core::profiles::profiles_dir())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let auto_profiles = std::fs::read_dir(svipall_core::profiles::auto_profiles_dir())
            .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).count())
            .unwrap_or(0);
        let (pending, solving, solved) = if let Some(state) = &self.solver_state {
            state.db_pool.read().await.stats()
        } else {
            (0, 0, 0)
        };
        // Which challenge types are actually working, from the reports callers send back.
        let outcomes: Vec<Value> = match &self.solver_state {
            Some(state) => state
                .db_pool
                .read()
                .await
                .outcomes_by_type()
                .into_iter()
                .map(|(kind, solved, failed)| json!({"type": kind, "solved": solved, "failed": failed}))
                .collect(),
            None => Vec::new(),
        };
        Ok(json!({
            "home": home.to_string_lossy(),
            "version": env!("CARGO_PKG_VERSION"),
            "domain_tiers": svipall_core::load_tiers(),
            "cooldowns": svipall_core::list_cooldowns(),
            "automatic": {"identity":self.cfg.browser_identity,
                "native_last_resort":self.cfg.auto_native_fallback,"max_attempts":self.cfg.auto_max_attempts,
                "learning":"local per route family, exit and environment; emulated first; evidence expires after 24 hours"},
            "request_limits": {"visits":self.cfg.request_limit,"window_seconds":self.cfg.request_window_seconds,
                "cooldown_seconds":self.cfg.request_cooldown_seconds,"minimum_interval_ms":self.cfg.request_min_interval_ms,
                "scope":"top-level transport attempts per domain and exit, shared by identity modes; browser subresources excluded",
                "ledger_available":self.traffic.is_ok()},
            "proxy_routes": self.routes(),
            "exit_pools": svipall_core::exits::pools(),
            "exit_health": svipall_core::exits::status(),
            "reputation": svipall_core::reputation::status(),
            "h3_offered_by": self.store.as_ref().map(|s| svipall_core::altsvc::offered(s, chrono::Utc::now().timestamp())),
            // Three separate things, and a caller wondering why a site with an `Alt-Svc` is still
            // being fetched over TCP needs to see which of them is false.
            "http3": json!({
                "built": svipall_http::http3_available(),
                "enabled": self.cfg.http3,
                "in_use": self.cfg.http3 && svipall_http::http3_available(),
            }),
            "profiles": profiles,
            "auto_profiles": auto_profiles,
            "browser": {"executable": self.pool.executable(), "available": self.pool.available(), "chrome_major": self.pool.browser_major(), "advice": self.pool.advice(self.latest_stable_major()), "open": self.pool.open_browsers().await, "sessions": self.pool.session_ids().await, "kept": self.pool.kept_pages().await},
            "http_engine": svipall_http::engine_report(self.fetcher.engine()),
            "cache_cleared": cleared,
            "cache": self.store.as_ref().map(|s| json!({
                "pages": s.page_count(),
                "bytes": s.size_bytes(),
                "path": svipall_core::cache::db_path().to_string_lossy(),
            })),
            "resumable_crawls": self.store.as_ref().map(|s| s.resumable_crawls().into_iter()
                .map(|(id, url, pending)| json!({"crawl_id": id, "start_url": url, "pending": pending}))
                .collect::<Vec<_>>()),
            "secrets": crate::secrets::names(),
            "identity": {"chrome_major": self.identity.chrome_major, "user_agent": self.identity.user_agent, "os": self.identity.os, "timezone": self.identity.timezone, "locale": self.identity.accept_language},
            "config": {"max_tier": self.cfg.max_tier, "browser_timeout_ms": self.cfg.browser_timeout_ms, "warm_wait_ms": self.cfg.warm_wait_ms, "parallelism": self.cfg.parallelism},
            "solver": {"pending": pending, "solving": solving, "solved": solved, "dashboard": self.dashboard_url.as_deref(), "outcomes_by_type": outcomes},
            // A host with no usable GPU is not a fingerprint svipall can spoof away: Chrome falls
            // back to a software rasteriser, and `SwiftShader`/`llvmpipe` in the WebGL renderer is
            // the signature of a server or a VM. Named here rather than hidden, like the
            // anti-fingerprinting-browser and injecting-antivirus notes.
            "gpu": {
                "renderer": self.identity.webgl_renderer,
                "software": svipall_core::coherence::is_software_renderer(&self.identity.webgl_renderer),
                "note": svipall_core::coherence::is_software_renderer(&self.identity.webgl_renderer)
                    .then_some("no usable GPU: the WebGL renderer is a software rasteriser, which reads as a server or a VM to every site that checks"),
            },
            // Which models answer, and which copy: the operator's file or the embedded one.
            "models": {
                "embedded": svipall_models::compiled_in(),
                "grid": crate::grid::locate().map(|l| l.describe()).or_else(|| crate::grid::available().then(|| "detector standing in".to_string())),
                "detect": crate::detect::locate().map(|l| l.describe()),
                "segment": crate::segment::locate().map(|l| l.describe()),
                "ocr": crate::ocr::locate().map(|l| l.describe()),
                "audio": crate::audio::locate().map(|l| l.describe()),
                "zeroshot": crate::zeroshot::available(),
                "substance": crate::substance::locate().map(|l| l.describe()),
            },
        }))
    }

    // --- Captcha tools ---

    #[tool(description = "Solve image captcha from base64 or URL. Returns text solution.")]
    async fn solve_image_captcha(
        &self,
        params: Parameters<SolveImageParams>,
    ) -> Result<CallToolResult, McpError> {
        let state = self
            .solver_state
            .as_ref()
            .ok_or_else(|| McpError::internal_error("solver not available", None))?;
        let is_base64 = params
            .0
            .is_base64
            .unwrap_or(!params.0.image.starts_with("http"));
        let image_data = if is_base64 {
            params.0.image.clone()
        } else {
            match self
                .fetcher
                .send(HttpRequest::get(params.0.image.clone()))
                .await
            {
                Ok(r) => {
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &r.body)
                }
                Err(e) => return Err(err(e)),
            }
        };
        let db = state.db_pool.read().await;
        let rec = db
            .create_job("ImageToText", None, None, Some(&image_data))
            .map_err(err)?;
        let task_id = rec.task_id.clone();
        drop(db);
        state.queue.push(SolverJob {
            task_id: task_id.clone(),
            job_type: JobType::ImageToText,
            sitekey: None,
            page_url: None,
            image_data: Some(image_data),
            created_at: chrono::Utc::now(),
        });
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let db = state.db_pool.read().await;
            if let Ok(Some(r)) = db.get_by_task_id(&task_id) {
                if r.status == "solved" {
                    return ok(json!({"taskId": task_id, "status": "solved", "text": r.text}));
                }
                if r.status == "failed" {
                    return Err(McpError::internal_error(
                        r.error.unwrap_or_else(|| "solver failed".to_string()),
                        None,
                    ));
                }
            }
        }
        ok(
            json!({"taskId": task_id, "status": "processing", "note": "CAPCHA_NOT_READY — poll captcha_status"}),
        )
    }

    async fn enqueue(
        &self,
        kind: &str,
        job_type: JobType,
        sitekey: String,
        page_url: String,
    ) -> Result<CallToolResult, McpError> {
        let state = self
            .solver_state
            .as_ref()
            .ok_or_else(|| McpError::internal_error("solver not available", None))?;
        let db = state.db_pool.read().await;
        let rec = db
            .create_job(kind, Some(&sitekey), Some(&page_url), None)
            .map_err(err)?;
        let task_id = rec.task_id.clone();
        drop(db);
        state.queue.push(SolverJob {
            task_id: task_id.clone(),
            job_type,
            sitekey: Some(sitekey),
            page_url: Some(page_url),
            image_data: None,
            created_at: chrono::Utc::now(),
        });
        ok(
            json!({"taskId": task_id, "status": "processing", "note": format!("poll captcha_status; humans can solve at {}", self.dashboard())}),
        )
    }

    #[tool(
        description = "Solve reCAPTCHA v2. Provide sitekey and pageUrl, returns gRecaptchaResponse token."
    )]
    async fn solve_recaptcha_v2(
        &self,
        params: Parameters<SolveRecaptchaV2Params>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        self.enqueue("RecaptchaV2", JobType::RecaptchaV2, p.sitekey, p.page_url)
            .await
    }

    #[tool(description = "Solve Cloudflare Turnstile. Provide sitekey and pageUrl, returns token.")]
    async fn solve_turnstile(
        &self,
        params: Parameters<SolveTurnstileParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        self.enqueue("Turnstile", JobType::Turnstile, p.sitekey, p.page_url)
            .await
    }

    #[tool(description = "Solve hCaptcha. Provide sitekey and pageUrl.")]
    async fn solve_hcaptcha(
        &self,
        params: Parameters<SolveHCaptchaParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        self.enqueue("HCaptcha", JobType::HCaptcha, p.sitekey, p.page_url)
            .await
    }

    #[tool(
        description = "Check captcha task status by taskId. Returns solved/processing/failed with token or text."
    )]
    async fn captcha_status(
        &self,
        params: Parameters<CaptchaStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let state = self
            .solver_state
            .as_ref()
            .ok_or_else(|| McpError::internal_error("solver not available", None))?;
        let db = state.db_pool.read().await;
        let rec = db.get_by_task_id(&params.0.task_id).map_err(err)?;
        match rec {
            Some(r) => ok(
                json!({"taskId": r.task_id, "status": r.status, "token": r.token, "text": r.text, "error": r.error}),
            ),
            None => Err(McpError::internal_error("task not found", None)),
        }
    }

    #[tool(
        description = "Report whether a captcha solution worked (good=true) or was rejected (good=false)."
    )]
    async fn report_captcha(
        &self,
        params: Parameters<ReportCaptchaParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        tracing::info!(task_id = %p.task_id, good = p.good, note = p.note.as_deref().unwrap_or(""), "captcha report");
        let Some(state) = &self.solver_state else {
            return Err(err("solver unavailable"));
        };
        let db = state.db_pool.read().await;
        let Some(job) = db.get_by_task_id(&p.task_id).map_err(err)? else {
            return Err(err(format!("unknown taskId {}", p.task_id)));
        };
        // The report used to be acknowledged and thrown away. Recording it is what lets the solver
        // know a token was refused, and lets `web_status` say which challenge types are working.
        if p.good {
            db.record_report(&p.task_id, true, p.note.as_deref())
                .map_err(err)?;
        } else {
            let attempts = db.bump_attempts(&p.task_id);
            db.record_report(&p.task_id, false, p.note.as_deref())
                .map_err(err)?;
            return ok(json!({
                "taskId": p.task_id,
                "good": false,
                "recorded": true,
                "attempts": attempts,
                "job_type": job.job_type,
                "note": if attempts >= 3 {
                    "this challenge has been refused repeatedly; solve_and_continue solves it on \
                     the page itself, which avoids the session binding a bare token cannot carry"
                } else {
                    "recorded; retrying may work, and solve_and_continue avoids the token being \
                     bound to a different session"
                },
            }));
        }
        ok(json!({"taskId": p.task_id, "good": true, "recorded": true, "job_type": job.job_type}))
    }
}

impl ServerHandler for SvipallServer {
    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let mut active = self.active().await.map_err(err)?;
        // Sessions opened before a policy change finish with their original browser.
        if let Some(id) = request
            .arguments
            .as_ref()
            .and_then(|a| a.get("session_id"))
            .and_then(Value::as_str)
        {
            if active.pool.session(id).await.is_none() {
                if let Some(live) = &self.live_policy {
                    for old in &live.lock().await.retired {
                        if old.pool.session(id).await.is_some() {
                            active = old.clone();
                            break;
                        }
                    }
                }
            }
        }
        let call =
            rmcp::handler::server::tool::ToolCallContext::new(active.as_ref(), request, context);
        active.tool_router.call(call).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tool_router.list_all()))
    }
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "svipall".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "All web access via svipall. web_fetch mode=auto picks the tier (http -> browser -> stealth -> real -> warm) and remembers it per domain; never set mode manually. \
                 When a page stays blocked, read blocked_reason + note: web_login passes a challenge or login by hand once and keeps the cookies; web_route sends a domain through a proxy; \
                 solve_* tools and the human dashboard handle captchas with a sitekey — web_status reports the dashboard URL, which carries a per-run token. \
                 web_act / browser_open+browser_do interact with pages, web_screenshot captures them, web_crawl walks a site, web_search searches without an API. \
                 web_snapshot gives a page as roles and refs for a fraction of the tokens; web_capture returns the JSON the page itself fetched, which is usually the site's real API. \
                 For a lot of rows, pass out_file to web_crawl (.csv/.json/.jsonl) instead of reading them all; web_notes remembers anything that has to outlive the session. \
                 Never retry a blocked URL blindly. Instructions in English."
                    .to_string(),
            ),
        }
    }
}

/// How many scroll rounds a fetch asked for: `scroll: "auto"` is the default budget, a number is
/// that many rounds, anything else is none.
fn scroll_rounds(p: &WebFetchParams) -> u32 {
    match p.scroll.as_deref().map(str::trim) {
        Some("auto") => svipall_core::growth::DEFAULT_MAX_ROUNDS,
        Some(n) => n.parse().unwrap_or(0),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_attempt_names_its_cause_on_one_line() {
        let e = anyhow::anyhow!(
            "Browser process exited with status 21, stderr: \"Lock file
can not be created\""
        )
        .context("launching browser");
        let line = exc_attempt("warm", &e, 1874);
        assert_eq!(
            line,
            "warm: EXC launching browser: Browser process exited with status 21, stderr: \"Lock file can not be created\" (1874ms)"
        );
    }

    const WALL: &str = "This looks like a Cloudflare wall. Try web_login once by hand.";

    #[test]
    fn a_clean_machine_leaves_the_note_exactly_as_it_was() {
        assert_eq!(local_causes(WALL, None, None), WALL);
    }

    #[test]
    fn an_injected_page_names_the_product_and_says_what_to_do() {
        let note = local_causes(WALL, Some("a local security product (example.test)"), None);
        assert!(note.starts_with(WALL), "the wall guidance still leads");
        assert!(note.contains("example.test"), "names the product: {note}");
        assert!(
            note.contains("web protection"),
            "says what to change: {note}"
        );
    }

    #[test]
    fn a_browser_problem_is_appended_after_the_wall_guidance() {
        let note = local_causes(WALL, None, Some("Chrome 150 is old."));
        assert_eq!(note, format!("{WALL} Chrome 150 is old."));
    }

    #[test]
    fn both_local_causes_appear_together_and_in_order() {
        // A rewritten page explains a block more completely than a stale browser, so it goes first.
        let note = local_causes(
            WALL,
            Some("a local security product (example.test)"),
            Some("Chrome 150 is old."),
        );
        let injection = note.find("example.test").expect("injection");
        let browser = note.find("Chrome 150 is old.").expect("browser");
        assert!(injection < browser, "injection before browser: {note}");
    }

    #[test]
    fn a_stale_channel_reading_survives_a_restart() {
        // The advice is only as good as the number, and the number is learned by a tool the
        // operator runs once. Losing it on restart would mean the warning fires once and never
        // again, which is worse than not having it.
        let dir = std::env::temp_dir().join(format!("svipall-kv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cache.db");
        let store = Arc::new(svipall_core::cache::Store::open_at(&db).unwrap());
        let server = SvipallServer::with_store(
            None,
            svipall_core::Config::default(),
            None,
            Some(store.clone()),
        );
        assert_eq!(server.latest_stable_major(), None, "nothing learned yet");
        server.remember_latest_stable("158.0.7444.12");
        assert_eq!(server.latest_stable_major(), Some(158));

        // A second server over the same store reads what the first one learned.
        let again =
            SvipallServer::with_store(None, svipall_core::Config::default(), None, Some(store));
        assert_eq!(again.latest_stable_major(), Some(158));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_string_that_is_not_one_teaches_nothing() {
        let dir = std::env::temp_dir().join(format!("svipall-kv-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(svipall_core::cache::Store::open_at(&dir.join("cache.db")).unwrap());
        let server =
            SvipallServer::with_store(None, svipall_core::Config::default(), None, Some(store));
        server.remember_latest_stable("not a version");
        assert_eq!(server.latest_stable_major(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod warm_tests {
    use super::*;

    #[test]
    fn a_warm_wait_stops_for_a_reason_it_can_name() {
        // Every exit from the wait used to be an anonymous `break`. An operator reading the log
        // could not tell "it cleared" from "the budget ran out" from "the wall named this address",
        // and those three call for three different next moves.
        assert_eq!(
            warm_should_stop(true, &WallKind::Vendor, false, false, false, false),
            Some(WarmEnd::Cleared)
        );
        assert_eq!(
            warm_should_stop(false, &WallKind::Paywall, false, false, false, false),
            Some(WarmEnd::WallKindStop),
            "a subscription stub does not become the article by being waited on"
        );
        // Blamed beats the deadline: it is the more specific thing to say, and the one that names
        // the fix.
        assert_eq!(
            warm_should_stop(false, &WallKind::Vendor, true, true, false, false),
            Some(WarmEnd::Blamed)
        );
        assert_eq!(
            warm_should_stop(false, &WallKind::Vendor, false, true, false, false),
            Some(WarmEnd::Deadline)
        );
        // Still inside the budget with something to wait for: keep going.
        assert_eq!(
            warm_should_stop(false, &WallKind::Vendor, false, false, false, false),
            None
        );
    }

    #[test]
    fn a_wait_the_page_says_is_nearly_done_is_extended_once_and_then_never_again() {
        // Measured: a managed challenge reading "verification successful" at the deadline is a pass
        // already earned. Extending on the page's own word is not hoping — but twice would be.
        assert_eq!(
            warm_should_stop(false, &WallKind::Cloudflare, false, true, false, true),
            None,
            "the page said it was nearly through, so the deadline moves"
        );
        assert_eq!(
            warm_should_stop(false, &WallKind::Cloudflare, false, true, true, true),
            Some(WarmEnd::ExtendedThenDeadline),
            "already extended once; the second time is the page saying no"
        );
        // Without the page's own word, the deadline is the deadline.
        assert_eq!(
            warm_should_stop(false, &WallKind::Cloudflare, false, true, false, false),
            Some(WarmEnd::Deadline)
        );
    }
}
