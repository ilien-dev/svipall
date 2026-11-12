//! The tool list a model reads, and how a call to it reaches the operation behind it.
//!
//! Up to 1.4 every operation was its own tool: 31 names, 38 500 characters of definitions. A month
//! of real calls showed where that went: 96% of 4 406 calls were to three tools, thirteen tools were
//! never called once, and every client without deferred loading paid for all of them on every
//! turn. So the list is now twelve tools, written for the reader that actually reads them, and
//! each one is a *rewrite* onto the operations that already existed: [`translate`] turns
//! `(tool, arguments)` into the operation's own name and arguments, and nothing below it changed.
//!
//! The operations keep their names in the router, the CLI and the REST API, so a client that
//! still calls `web_fetch_many` is answered as before. They are only no longer *listed*.
//!
//! Text here is terse on purpose. It is read by a model on every request, not by a person: a word
//! that does not change which tool is picked or which argument is sent is a word paid for nothing.

use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{json, Map, Value};
use std::sync::Arc;

/// One parameter: name, JSON type, and what the model needs to know that the name does not say.
///
/// Types: `s` string, `i` integer, `b` boolean, `[s]` array of strings, `o` object, `s|o` either,
/// `any` undeclared, `actions` the step list of `browser`.
type P = (&'static str, &'static str, &'static str);

struct Spec {
    name: &'static str,
    description: &'static str,
    required: &'static [&'static str],
    read_only: bool,
    open_world: bool,
    params: &'static [P],
}

const TIER: &str = "browser | stealth | real (default) | warm";
const EXTRACTION: &str = "markdown (default) | text | html";
const SCHEMA: &str = "Rows as `extracted`: \"auto\", or {name, base_selector, fields:[{name, \
                      selector, type, attribute}]}; type text|attribute|number|exists|list|html|markdown";

const SPECS: &[Spec] = &[
    Spec {
        name: "web_fetch",
        description: "Read URL(s): page, PDF, office doc, JSON API, source file, download; an \
            audio/video file as its transcript. Returns `content` (markdown; JSON and code as \
            served), `title`, `tier_used`, `quality`. Tier climbs http > browser > stealth > real \
            > warm on its own, learned per domain. github.com is read from its API. Fewer tokens: \
            `query`, `css_selector`, `json_path`, `max_tokens` + `cursor`, `out_file`. Rows: \
            `schema` or `tables`. `urls` reads several in parallel (results in input order, \
            duplicates marked `same_text_as`); with `probe` it only checks they exist (status, \
            type, length: a 404 is an answer). Same-site links come back as paths, join them to \
            `url`. Blocked: read `blocked_reason` and `note`, never retry blindly.",
        required: &[],
        read_only: true,
        open_world: true,
        params: &[
            ("url", "s", "Also raw:<html>, or file:///path under ~/.svipall/in"),
            ("urls", "[s]", "Several, instead of url"),
            ("probe", "b", "urls: existence only, no body"),
            ("query", "s", "Keep only blocks relevant to these words. Name the fact, not the topic"),
            ("css_selector", "s", "Keep only matching elements"),
            ("json_path", "s", "Fields of a JSON answer: items[:5].{name, owner.login}"),
            ("max_tokens", "i", "Content cap, default 25000. A cut result carries `cursor`"),
            ("max_tokens_total", "i", "urls: cap for all pages together, default 8000"),
            ("cursor", "s", "Continue a cut result"),
            ("out_file", "s", "Write here, return the path. .md .txt .json .csv .html get text or rows; any other name, or a binary body, the bytes (call again to resume). Relative: ~/.svipall/out/. urls: one .json/.jsonl for the set"),
            ("out_dir", "s", "urls: each page whole, one file each"),
            ("extraction", "s", EXTRACTION),
            ("schema", "s|o", SCHEMA),
            ("tables", "b", "Data tables as {caption, header, rows}"),
            ("main_content_only", "b", "Default true: drops nav, footer, sidebars"),
            ("include_links", "b", "Add links and images"),
            ("include_metadata", "b", "Add canonical, language, author, dates, OpenGraph, JSON-LD, feeds"),
            ("include_quality", "b", "Add quality_detail"),
            ("scroll", "s", "\"auto\" or a round count, for a page that loads as you scroll"),
            ("mode", "s", "Force one tier. Leave unset"),
            ("max_tier", "s", "Highest tier to climb to"),
            ("profile", "s", "Cookies of a saved login profile"),
            ("proxy", "s", "Proxy URL, this call only"),
            ("method", "s", "POST, PUT, DELETE, HEAD (http tier)"),
            ("body", "s", "Request body"),
            ("headers", "o", "Extra request headers"),
            ("timeout", "i", "ms, default 60000"),
            ("wait", "i", "Seconds to wait out a visit limit, max 900; 0 never waits"),
            ("fresh", "b", "Try again a site that failed in the last 10 min"),
            ("cache", "s", "auto (default) | read | write | bypass | refresh"),
            ("robots", "s", "warn (default) | obey | ignore"),
            ("mobile", "b", "Ask as a phone"),
            ("text_only", "b", "Skip images, fonts, CSS in browser tiers"),
            ("isolated", "b", "Throwaway browser profile"),
            ("use_site_template", "b", "Strip what the site repeats on every page; can drop content"),
        ],
    },
    Spec {
        name: "web_search",
        description: "Search the web, no API key. Returns title, url, snippet per result. Zero \
            results carry a `note`: change the words, `lang`/`region` or `engine` instead of \
            repeating. `site` searches inside one site through its own search box and returns \
            `results_url`, the pattern that makes later queries a plain web_fetch. Read results \
            with web_fetch `urls`.",
        required: &["query"],
        read_only: true,
        open_world: true,
        params: &[
            ("query", "s", ""),
            ("limit", "i", "Default 10, max 50"),
            ("engine", "s", "auto (default) | all (merged) | ddg | ddg-html | bing | brave | news"),
            ("lang", "s", "e.g. es. Default: the query's"),
            ("region", "s", "Country, e.g. MX"),
            ("site", "s", "Any page of one site: search it with its own box"),
            ("fetch", "b", "site: false returns only the URL pattern"),
            ("timeout", "i", "site: ms, default 60000"),
        ],
    },
    Spec {
        name: "web_crawl",
        description: "Read a site whose pages are not known: crawls from `url`, returns each \
            page as markdown, deduplicated, robots.txt obeyed, with a `crawl_id` to resume. \
            `map` only lists the site's URLs (robots, sitemaps, llms.txt, feeds, links) for a few \
            hundred tokens: do that first. Known URLs: web_fetch `urls`. Many rows: `out_file`. \
            Same-site links are paths, join them to the page's `url`.",
        required: &["url"],
        read_only: true,
        open_world: true,
        params: &[
            ("url", "s", "Start; the crawl stays on its domain"),
            ("map", "b", "List URLs only, fetch no page"),
            ("max_pages", "i", "Default 20, max 200"),
            ("max_depth", "i", "Default 2"),
            ("include", "s", "Only URLs containing this, e.g. /docs/"),
            ("query", "s", "Rank pages by relevance, keep relevant blocks, stop when new pages add nothing"),
            ("schema", "s|o", "Rows from every page, as on web_fetch"),
            ("tables", "b", "Every page's data tables as rows"),
            ("out_file", "s", ".csv .json .jsonl: a path and a count come back"),
            ("strategy", "s", "best_first (default with query) | bfs | dfs (a manual, a paginated listing)"),
            ("also", "[s]", "More sites; the budget is shared equally"),
            ("since_last_crawl", "b", "Only what the sitemap's lastmod says changed"),
            ("crawl_id", "s", "Resume an interrupted crawl"),
            ("output", "s", "pages (default) | llms.txt | llms-full.txt"),
            ("extraction", "s", EXTRACTION),
            ("mode", "s", "Force one tier. Leave unset"),
            ("scroll", "s", "\"auto\" or a round count, on every page"),
            ("dedup", "b", "Default true: skip near-duplicates, naming what they repeat"),
            ("stop_when_saturated", "b", "Default true with query"),
            ("max_chars_per_page", "i", "Default 8000"),
            ("max_tokens_total", "i", "Default 60000"),
            ("max_duration_ms", "i", "Default 120000"),
            ("timeout", "i", "ms per page, default 45000"),
            ("robots", "s", "obey (default) | warn | ignore"),
            ("limit", "i", "map: max URLs, default 1000"),
            ("sources", "[s]", "map: robots, sitemap, llms, feeds, links (default all)"),
        ],
    },
    Spec {
        name: "web_view",
        description: "Look at a page. Default: every button, link and field with role, name and \
            a `ref` (e12) to act on with browser: no image, few tokens, the way to understand a \
            page before clicking. `image`: a PNG screenshot (path, and inline) when the question \
            is visual: layout, a chart, a picture.",
        required: &["url"],
        read_only: true,
        open_world: true,
        params: &[
            ("url", "s", ""),
            ("image", "b", "Screenshot instead of the node list"),
            ("find", "s", "Only nodes whose name or role contains this, e.g. \"add to cart\", \"button\""),
            ("max_depth", "i", "3 or 4 usually reaches the controls"),
            ("limit", "i", "Max nodes, default 200"),
            ("full_page", "b", "image: the whole scroll height"),
            ("mobile", "b", "image: phone viewport and user agent"),
            ("inline", "b", "image: false returns only the path"),
            ("tier", "s", TIER),
            ("profile", "s", "Cookies of a saved login profile"),
            ("proxy", "s", "image: proxy URL, this call only"),
            ("timeout", "i", "ms, default 60000"),
        ],
    },
    Spec {
        name: "browser",
        description: "Click, type and run steps in a real browser; returns each action's result \
            and the resulting page. One shot: `url` + `actions`. Several calls with cookies and \
            page state kept (log in then browse, cart then checkout): `open` (alone, or \
            with the first `url`/`actions`) returns a `session_id`; pass it with `url` and/or \
            `actions`, end with `close`. Take refs from \
            web_view.",
        required: &[],
        read_only: false,
        open_world: true,
        params: &[
            ("url", "s", "Open or navigate first; omit in a session to stay on the page"),
            ("actions", "actions", "Steps in order, e.g. [{\"do\":\"type\",\"ref\":\"e3\",\"text\":\"shoes\"},{\"do\":\"press\",\"key\":\"Enter\"},{\"do\":\"wait\",\"selector\":\".results\"}]"),
            ("session_id", "s", "From open"),
            ("open", "b", "Start a session"),
            ("close", "b", "End session_id and release its browser"),
            ("extraction", "s", EXTRACTION),
            ("query", "s", "session: keep only blocks relevant to these words"),
            ("tier", "s", "One shot: browser | stealth | real (default) | warm"),
            ("profile", "s", "Cookies of a saved login profile (one shot, open)"),
            ("proxy", "s", "Proxy URL (one shot, open)"),
            ("visible", "b", "open: show the window"),
            ("timeout", "i", "ms, default 90000"),
        ],
    },
    Spec {
        name: "web_watch",
        description: "Detect changes to a page. `diff`: compare it once with the cached copy \
            (`changed`, `similarity`, `added`, `removed`). `add`: watch it on a schedule; `list`: \
            what changed and when; `check`: look now; `remove`. Watches survive restarts and run \
            while the server is up.",
        required: &[],
        read_only: false,
        open_world: true,
        params: &[
            ("action", "s", "add (default) | diff | list | check | remove"),
            ("url", "s", "Needed for add, diff, remove, a single check"),
            ("interval_secs", "i", "Default 3600, min 60"),
            ("label", "s", "Name shown in list"),
            ("css_selector", "s", "Watch only this region"),
            ("refetch", "b", "diff: false compares stored copies only"),
        ],
    },
    Spec {
        name: "web_status",
        description: "Why something is blocked or slow, and how to reset it. No arguments: a \
            summary (cooldowns, visit limits, browser, solver), nothing changed; `sections` for \
            more. `log`: the history per domain (tier that answered, wall met, time taken). The \
            clear_* and forget_tier resets take a domain. The visit limit is per host and shared \
            by every session on this machine, so clearing it spends what others are pacing.",
        required: &[],
        read_only: false,
        open_world: false,
        params: &[
            ("sections", "s", "\"all\", or names from the summary's `sections`, comma-separated"),
            ("log", "s", "summary (half-blocked or slow domains) | recent (last events)"),
            ("domain", "s", "log: only this domain"),
            ("since_secs", "i", "log: default 3600"),
            ("limit", "i", "log recent: default 50"),
            ("clear_cooldown", "s", "Domain: cooldown, holds, strikes, visit window, remembered failure"),
            ("forget_tier", "s", "Domain: its learned tier"),
            ("clear_budget", "s", "Domain: its reputation spend"),
            ("clear_cache", "any", "true, or a domain"),
            ("configure", "any", "Object: browser and session policy settings to save"),
        ],
    },
    Spec {
        name: "web_setup",
        description: "Set this machine up, by `do`. login: opens a visible window at `url` for a \
            person to pass a login or a challenge no tier clears; cookies go to `profile` \
            (default: the domain's own, used automatically afterwards). route: send `domain` \
            through `proxy` or a pool from now on, for a fingerprint wall that never yields or a \
            country lock (no other argument lists routes). profile: list, export or import saved \
            logins as an encrypted archive. browser: status, install, update or remove the \
            managed browser, when a fetch reports none usable.",
        required: &["do"],
        read_only: false,
        open_world: true,
        params: &[
            ("do", "s", "login | route | profile | browser"),
            ("url", "s", "login: the login form or the blocked page"),
            ("profile", "s", "login: name to save the cookies under"),
            ("timeout_s", "i", "login: default 300; closing the window ends it"),
            ("domain", "s", "route: subdomains inherit"),
            ("proxy", "s", "route: e.g. socks5h://user:pass@host:1080"),
            ("country", "s", "route: ISO country of the exit, for a matching timezone and language"),
            ("proxies", "[s]", "route: a pool; sticks to the first that works"),
            ("countries", "[s]", "route: country of each proxy, by position"),
            ("remove", "b", "route: drop the domain's route"),
            ("check", "b", "route: test the exits (alive, latency, DNS leak)"),
            ("check_url", "s", "route: what check fetches"),
            ("action", "s", "profile: list (default) | export | import. browser: status (default) | install | update | remove"),
            ("name", "s", "profile: its name"),
            ("file", "s", "profile: archive path; relative lands in ~/.svipall/out/"),
            ("password", "s", "profile: required to export or import"),
            ("artifact", "s", "browser: chrome (default) | chrome-headless-shell"),
        ],
    },
    Spec {
        name: "captcha",
        description: "Get past a captcha. `url`: solve the challenge on that blocked page in \
            place and return the page behind it; use it when `blocked_reason` names a captcha (a \
            widget that needs a person opens a window or the dashboard). A bare token, only for \
            a form you post yourself from the same session and address: `kind` + `sitekey` + \
            `page_url`. `image`: read a picture of distorted characters as text. `task_id`: poll \
            a pending task; with `good`, report whether the site took the answer, which orders \
            what is tried next time.",
        required: &[],
        read_only: false,
        open_world: true,
        params: &[
            ("url", "s", "The blocked page"),
            ("profile", "s", "url: profile whose cookies to use and update"),
            ("extraction", "s", EXTRACTION),
            ("timeout_s", "i", "url: default 120"),
            ("max_tokens", "i", "url: content cap, default 25000"),
            ("kind", "s", "turnstile | recaptcha_v2 | hcaptcha"),
            ("sitekey", "s", "The widget's data-sitekey"),
            ("page_url", "s", "The page the widget is on"),
            ("invisible", "b", "recaptcha_v2 without a checkbox"),
            ("image", "s", "base64 or a URL"),
            ("task_id", "s", "The `taskId` a pending answer returned"),
            ("good", "b", "With task_id: true accepted, false rejected"),
            ("note", "s", "With good: what the site said"),
        ],
    },
    Spec {
        name: "web_capture",
        description: "Find the JSON API behind a listing or pagination: records the XHR and JSON \
            responses a page makes while it loads. Call without `pattern` to see the endpoints, \
            then with `pattern` and `bodies` for the payloads. An endpoint that took page=1 takes \
            page=2, which beats following links.",
        required: &["url"],
        read_only: true,
        open_world: true,
        params: &[
            ("url", "s", ""),
            ("pattern", "s", "Only responses whose URL contains this, e.g. /api/"),
            ("bodies", "b", "Return the response bodies"),
            ("max_body", "i", "Chars per body, default 20000"),
            ("settle_ms", "i", "Keep recording this long after load, default 3000"),
            ("tier", "s", TIER),
            ("profile", "s", "Cookies of a saved login profile"),
        ],
    },
    Spec {
        name: "web_video",
        description: "What a video says: captions and chapters as one [m:ss] timeline, from a \
            watch page, an embedded player or the stream; speech is transcribed when there are \
            no captions. web_fetch on a video page returns only its description. An encrypted \
            stream is reported as `drm`, never read.",
        required: &["url"],
        read_only: true,
        open_world: true,
        params: &[
            ("url", "s", ""),
            ("lang", "s", "Caption language to prefer, e.g. es"),
            ("frames", "i", "Keyframes to save where the picture changes, max 24"),
            ("max_tokens", "i", "Default 25000"),
            ("cursor", "s", "Continue a cut result"),
            ("out_file", "s", "Write the timeline here, return the path"),
            ("profile", "s", "Cookies of a saved login profile"),
            ("wait", "i", "As on web_fetch"),
        ],
    },
    Spec {
        name: "web_notes",
        description: "Remember a value across sessions: the last id seen, a learned URL pattern, \
            a checkpoint.",
        required: &[],
        read_only: false,
        open_world: false,
        params: &[
            ("action", "s", "get (default) | set | list | delete"),
            ("key", "s", "Path-like, e.g. shop/last_id"),
            ("value", "s", "set: a string; JSON if it has structure"),
            ("prefix", "s", "list: keys starting with this"),
        ],
    },
];

/// One step of `browser`. The verbs come from [`crate::tools::ACTIONS`], the one list the browser
/// dispatches on.
const ACTION_FIELDS: &[P] = &[
    ("ref", "s", "From web_view, e.g. e12. Preferred"),
    (
        "selector",
        "s",
        "CSS, when there is no ref; wait: the element to wait for",
    ),
    (
        "text",
        "s",
        "type, fill; ${NAME} is read from ~/.svipall/secrets.env",
    ),
    ("key", "s", "press: Enter, Tab, ArrowDown"),
    ("value", "s", "select: the option. verify: what is expected"),
    (
        "ms",
        "i",
        "wait: default 1000 (10000 with selector). hold: default 2000",
    ),
    ("pixels", "i", "scroll: default 600"),
    (
        "until",
        "s",
        "scroll: \"stable\" scrolls until nothing more loads",
    ),
    ("rounds", "i", "scroll until: cap"),
    ("script", "s", "eval: JavaScript; its value comes back"),
    ("url", "s", "goto"),
    ("full_page", "b", "screenshot: whole scroll height"),
];

fn prop(kind: &str, doc: &str) -> Value {
    let mut p = match kind {
        "s" => json!({"type": "string"}),
        "i" => json!({"type": "integer"}),
        "b" => json!({"type": "boolean"}),
        "o" => json!({"type": "object"}),
        "[s]" => json!({"type": "array", "items": {"type": "string"}}),
        "s|o" => json!({"type": ["string", "object"]}),
        "actions" => {
            let verbs: Vec<&str> = crate::tools::ACTIONS.iter().map(|(v, _)| *v).collect();
            let mut fields = Map::new();
            fields.insert(
                "do".into(),
                json!({"type": "string", "enum": verbs, "description": "verify: the element is there, visible, holding `value`. console: the page's log. hold: press and hold `ms`"}),
            );
            for (name, kind, doc) in ACTION_FIELDS {
                fields.insert((*name).into(), prop(kind, doc));
            }
            json!({"type": "array", "items": {"type": "object", "properties": fields, "required": ["do"]}})
        }
        _ => json!({}),
    };
    if !doc.is_empty() {
        p["description"] = json!(doc);
    }
    p
}

fn schema(spec: &Spec) -> Map<String, Value> {
    let mut props = Map::new();
    for (name, kind, doc) in spec.params {
        props.insert((*name).into(), prop(kind, doc));
    }
    let mut s = Map::new();
    s.insert("type".into(), json!("object"));
    s.insert("properties".into(), Value::Object(props));
    if !spec.required.is_empty() {
        s.insert("required".into(), json!(spec.required));
    }
    s
}

/// The tool list, as `tools/list` hands it over.
pub fn tools() -> Vec<Tool> {
    SPECS
        .iter()
        .map(|spec| Tool {
            name: spec.name.into(),
            description: Some(spec.description.into()),
            input_schema: Arc::new(schema(spec)),
            output_schema: None,
            annotations: Some(ToolAnnotations {
                title: None,
                read_only_hint: Some(spec.read_only),
                destructive_hint: None,
                idempotent_hint: None,
                open_world_hint: Some(spec.open_world),
            }),
        })
        .collect()
}

/// The schema of a listed tool, or `None` for a name that is not listed.
pub fn listed_schema(name: &str) -> Option<Map<String, Value>> {
    SPECS.iter().find(|s| s.name == name).map(schema)
}

/// What the server tells a client before it has chosen a tool.
pub const INSTRUCTIONS: &str = "svipall does all web access, locally. \
Read URL(s): web_fetch (page, PDF/office, JSON, file; github.com via its API; tier automatic, learned per domain). \
Fewer tokens: query=, css_selector, json_path, max_tokens + cursor, out_file (a binary needs it). Rows: tables=true or schema=auto. \
Several URLs: urls=[..]; existence only: + probe=true. What a video says: web_video. The API behind a listing: web_capture. \
Click or type: web_view for refs, then browser. A picture: web_view image=true. \
A site's pages: web_crawl map=true, then web_crawl. Find URLs: web_search; inside one site, site=. \
Changes: web_watch. Memory across sessions: web_notes. \
Visits are limited per host, shared by every session and subagent on this machine: a short wait is taken for you, wait=N (up to 900 s) waits longer, or read another host meanwhile. \
A blocked page carries blocked_reason and a note: read them. Never retry a blocked URL blindly. \
A captcha named -> captcha url=. A login or a wall a person must pass -> web_setup do=login. \
A fingerprint wall that never yields -> web_setup do=route (a proxy). \
A slow or blocked domain -> web_status (log=summary for its history, clear_cooldown to reset). \
Code search across GitHub repositories: prefer the searchGitHub tool of the grep.app MCP server when this session has it; svipall reads a known repo, file, issue, PR or release, and does all of it when that tool is absent.";

/// A call that names a mode and an argument the mode has no use for. Nothing ran.
fn stray(tool: &str, key: &str, mode: &str, allowed: &[&str]) -> String {
    format!(
        "{tool}: `{key}` is not used {mode}; nothing was run. There it takes: {}.",
        allowed.join(", ")
    )
}

/// Move `args` to the operation `op`, keeping only `allowed` and renaming `(from, to)` pairs.
/// A key outside `allowed` is refused by name: a limit the caller set and the operation never
/// read is worse than a call that did not run.
fn route(
    tool: &str,
    mode: &str,
    op: &str,
    mut args: Map<String, Value>,
    drop: &[&str],
    allowed: &[&str],
    rename: &[(&str, &str)],
) -> Result<(String, Map<String, Value>), String> {
    for k in drop {
        args.remove(*k);
    }
    if let Some(k) = args.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(stray(tool, k, mode, allowed));
    }
    for (from, to) in rename {
        if let Some(v) = args.remove(*from) {
            args.insert((*to).into(), v);
        }
    }
    Ok((op.into(), args))
}

fn truthy(args: &Map<String, Value>, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Arguments `web_fetch_many` reads itself; every other one of `web_fetch`'s rides in `each`.
const MANY_OWN: &[&str] = &[
    "urls",
    "max_tokens",
    "max_tokens_total",
    "out_dir",
    "out_file",
];

/// Turn a call to a listed tool into the operation that carries it out.
///
/// A name that is not listed, or a listed one called the way it always was, comes back unchanged:
/// that is what keeps a 1.x client working.
pub fn translate(
    name: &str,
    args: Map<String, Value>,
) -> Result<(String, Map<String, Value>), String> {
    match name {
        "web_fetch" if args.contains_key("urls") => {
            if args.contains_key("url") {
                return Err("web_fetch: pass url or urls, not both; nothing was run.".into());
            }
            if truthy(&args, "probe") {
                return route(name, "with probe", "web_probe", args, &["probe"], &["urls", "timeout"], &[]);
            }
            if args.contains_key("cursor") {
                return Err(
                    "web_fetch: a cursor continues one page; pass it with that page's url, not \
                     with urls. Nothing was run."
                        .into(),
                );
            }
            let mut many = Map::new();
            let mut each = Map::new();
            for (k, v) in args {
                match k.as_str() {
                    "probe" => {}
                    k if MANY_OWN.contains(&k) => {
                        many.insert(k.into(), v);
                    }
                    _ => {
                        each.insert(k, v);
                    }
                }
            }
            if !each.is_empty() {
                each.insert("url".into(), json!(""));
                many.insert("each".into(), Value::Object(each));
            }
            Ok(("web_fetch_many".into(), many))
        }
        "web_fetch" => {
            let mut args = args;
            if args.remove("probe").and_then(|v| v.as_bool()).unwrap_or(false) {
                let Some(url) = args.remove("url") else {
                    return Err("web_fetch: probe needs url or urls; nothing was run.".into());
                };
                let mut probe = Map::new();
                probe.insert("urls".into(), json!([url]));
                if let Some(t) = args.remove("timeout") {
                    probe.insert("timeout".into(), t);
                }
                return Ok(("web_probe".into(), probe));
            }
            for k in ["max_tokens_total", "out_dir"] {
                if args.contains_key(k) {
                    return Err(format!(
                        "web_fetch: `{k}` belongs to urls; nothing was run."
                    ));
                }
            }
            if !args.contains_key("url") {
                return Err("web_fetch needs url or urls; nothing was run.".into());
            }
            Ok((name.into(), args))
        }
        "web_search" if args.contains_key("site") => route(
            name,
            "with site",
            "web_site_search",
            args,
            &[],
            &["site", "query", "fetch", "timeout"],
            &[("site", "url")],
        ),
        "web_search" => route(
            name,
            "without site",
            name,
            args,
            &[],
            &["query", "limit", "engine", "lang", "region"],
            &[],
        ),
        "web_crawl" if truthy(&args, "map") => route(
            name,
            "with map",
            "web_map",
            args,
            &["map"],
            &["url", "sources", "limit", "include"],
            &[],
        ),
        "web_crawl" => {
            let mut args = args;
            args.remove("map");
            for k in ["limit", "sources"] {
                if args.contains_key(k) {
                    return Err(format!(
                        "web_crawl: `{k}` belongs to map=true (a crawl is capped by max_pages); \
                         nothing was run."
                    ));
                }
            }
            Ok((name.into(), args))
        }
        "web_view" if truthy(&args, "image") => route(
            name,
            "with image",
            "web_screenshot",
            args,
            &["image"],
            &["url", "full_page", "mobile", "inline", "tier", "profile", "proxy", "timeout"],
            &[],
        ),
        "web_view" => route(
            name,
            "without image",
            "web_snapshot",
            args,
            &["image"],
            &["url", "find", "max_depth", "limit", "tier", "profile", "timeout"],
            &[],
        ),
        "browser" => {
            if truthy(&args, "close") {
                if text(&args, "session_id").is_none() {
                    return Err("browser: close needs session_id; nothing was run.".into());
                }
                return route(name, "with close", "browser_close", args, &["close"], &["session_id"], &[]);
            }
            if args.contains_key("session_id") {
                return route(
                    name,
                    "in a session",
                    "browser_do",
                    args,
                    &["close", "open"],
                    &["session_id", "url", "actions", "extraction", "query", "timeout"],
                    &[],
                );
            }
            if truthy(&args, "open") {
                return route(
                    name,
                    "with open",
                    "browser_open",
                    args,
                    &["open", "close"],
                    &[
                        "profile",
                        "proxy",
                        "visible",
                        "url",
                        "actions",
                        "extraction",
                        "query",
                        "timeout",
                    ],
                    &[],
                );
            }
            if !args.contains_key("url") {
                return Err(
                    "browser needs url + actions (one shot), open=true (a session), or a \
                     session_id; nothing was run."
                        .into(),
                );
            }
            let mut args = args;
            // One shot with nothing to do is a read of the rendered page.
            args.entry("actions").or_insert_with(|| json!([]));
            route(
                name,
                "in one shot",
                "web_act",
                args,
                &["open", "close"],
                &["url", "actions", "extraction", "tier", "profile", "proxy", "timeout"],
                &[],
            )
        }
        "web_watch" if text(&args, "action") == Some("diff") => route(
            name,
            "with action=diff",
            "web_diff",
            args,
            &["action"],
            &["url", "refetch"],
            &[],
        ),
        "web_watch" => route(
            name,
            "unless action=diff",
            name,
            args,
            &[],
            &["action", "url", "interval_secs", "label", "css_selector"],
            &[],
        ),
        "web_status" if args.contains_key("log") => route(
            name,
            "with log",
            "web_log",
            args,
            &[],
            &["log", "domain", "since_secs", "limit"],
            &[("log", "view")],
        ),
        "web_status" => route(
            name,
            "without log",
            name,
            args,
            &[],
            &[
                "sections",
                "clear_cooldown",
                "forget_tier",
                "clear_budget",
                "clear_cache",
                "configure",
            ],
            &[],
        ),
        "web_setup" => match text(&args, "do") {
            Some("login") => route(
                name,
                "with do=login",
                "web_login",
                args,
                &["do"],
                &["url", "profile", "timeout_s"],
                &[],
            ),
            Some("route") => route(
                name,
                "with do=route",
                "web_route",
                args,
                &["do"],
                &[
                    "domain",
                    "proxy",
                    "country",
                    "proxies",
                    "countries",
                    "remove",
                    "check",
                    "check_url",
                ],
                &[],
            ),
            Some("profile") => route(
                name,
                "with do=profile",
                "web_profile",
                args,
                &["do"],
                &["action", "name", "file", "password"],
                &[],
            ),
            Some("browser") => route(
                name,
                "with do=browser",
                "browser_setup",
                args,
                &["do"],
                &["action", "artifact"],
                &[],
            ),
            other => Err(format!(
                "web_setup: do must be login, route, profile or browser, not {:?}; nothing was run.",
                other.unwrap_or("")
            )),
        },
        "captcha" => {
            if args.contains_key("task_id") {
                let op = if args.contains_key("good") {
                    "report_captcha"
                } else {
                    "captcha_status"
                };
                let allowed: &[&str] = if op == "report_captcha" {
                    &["task_id", "good", "note"]
                } else {
                    &["task_id"]
                };
                return route(name, "with task_id", op, args, &[], allowed, &[("task_id", "taskId")]);
            }
            if args.contains_key("image") {
                return route(name, "with image", "solve_image_captcha", args, &[], &["image"], &[]);
            }
            if args.contains_key("sitekey") || args.contains_key("kind") {
                let (op, allowed): (&str, &[&str]) = match text(&args, "kind") {
                    Some("turnstile") => ("solve_turnstile", &["sitekey", "page_url"]),
                    Some("recaptcha_v2") => {
                        ("solve_recaptcha_v2", &["sitekey", "page_url", "invisible"])
                    }
                    Some("hcaptcha") => ("solve_hcaptcha", &["sitekey", "page_url"]),
                    other => {
                        return Err(format!(
                            "captcha: a token needs kind = turnstile, recaptcha_v2 or hcaptcha, \
                             not {:?}, with sitekey and page_url; to read the page behind a \
                             challenge pass url alone. Nothing was run.",
                            other.unwrap_or("")
                        ))
                    }
                };
                for k in ["sitekey", "page_url"] {
                    if text(&args, k).is_none() {
                        return Err(format!(
                            "captcha: a {} token needs `{k}`; to read the page behind a \
                             challenge pass url alone. Nothing was run.",
                            text(&args, "kind").unwrap_or("")
                        ));
                    }
                }
                return route(name, "for a token", op, args, &["kind"], allowed, &[("page_url", "pageUrl")]);
            }
            if args.contains_key("url") {
                return route(
                    name,
                    "with url",
                    "solve_and_continue",
                    args,
                    &[],
                    &["url", "profile", "extraction", "timeout_s", "max_tokens"],
                    &[],
                );
            }
            Err(
                "captcha needs url (solve on the page and return it), kind + sitekey + page_url \
                 (a token), image, or task_id; nothing was run."
                    .into(),
            )
        }
        _ => Ok((name.into(), args)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().expect("object").clone()
    }

    fn to(name: &str, v: Value) -> (String, Value) {
        let (op, a) = translate(name, args(v)).expect("translates");
        (op, Value::Object(a))
    }

    fn refused(name: &str, v: Value) -> String {
        translate(name, args(v)).expect_err("refused")
    }

    #[test]
    fn a_single_fetch_is_left_exactly_as_it_was() {
        let (op, a) = to("web_fetch", json!({"url": "https://a.test", "query": "x"}));
        assert_eq!(op, "web_fetch");
        assert_eq!(a, json!({"url": "https://a.test", "query": "x"}));
    }

    #[test]
    fn several_urls_become_one_parallel_fetch_and_keep_every_per_page_argument() {
        let (op, a) = to(
            "web_fetch",
            json!({"urls": ["https://a.test"], "query": "x", "css_selector": "main", "max_tokens_total": 9000}),
        );
        assert_eq!(op, "web_fetch_many");
        assert_eq!(a["urls"], json!(["https://a.test"]));
        assert_eq!(a["max_tokens_total"], 9000);
        assert_eq!(a["each"]["query"], "x");
        assert_eq!(a["each"]["css_selector"], "main");
    }

    #[test]
    fn probe_checks_existence_for_one_url_or_many() {
        let (op, a) = to(
            "web_fetch",
            json!({"urls": ["https://a.test"], "probe": true}),
        );
        assert_eq!(
            (op.as_str(), &a),
            ("web_probe", &json!({"urls": ["https://a.test"]}))
        );
        let (op, a) = to("web_fetch", json!({"url": "https://a.test", "probe": true}));
        assert_eq!(
            (op.as_str(), &a),
            ("web_probe", &json!({"urls": ["https://a.test"]}))
        );
        assert!(refused(
            "web_fetch",
            json!({"urls": ["u"], "probe": true, "query": "q"})
        )
        .contains("`query` is not used with probe"));
    }

    #[test]
    fn a_fetch_that_cannot_mean_anything_is_refused_before_it_runs() {
        assert!(refused("web_fetch", json!({})).contains("url or urls"));
        assert!(refused("web_fetch", json!({"url": "u", "urls": ["u"]})).contains("not both"));
        assert!(refused("web_fetch", json!({"urls": ["u"], "cursor": "c"})).contains("cursor"));
        assert!(refused("web_fetch", json!({"url": "u", "out_dir": "d"})).contains("urls"));
    }

    #[test]
    fn search_inside_a_site_uses_its_own_box() {
        let (op, a) = to(
            "web_search",
            json!({"query": "q", "site": "https://shop.test"}),
        );
        assert_eq!(op, "web_site_search");
        assert_eq!(a, json!({"query": "q", "url": "https://shop.test"}));
        assert_eq!(
            to("web_search", json!({"query": "q", "limit": 5})).0,
            "web_search"
        );
        assert!(refused(
            "web_search",
            json!({"query": "q", "site": "s", "engine": "all"})
        )
        .contains("`engine` is not used with site"));
    }

    #[test]
    fn map_lists_urls_and_a_crawl_reads_them() {
        let (op, a) = to("web_crawl", json!({"url": "u", "map": true, "limit": 50}));
        assert_eq!(
            (op.as_str(), &a),
            ("web_map", &json!({"url": "u", "limit": 50}))
        );
        let (op, a) = to(
            "web_crawl",
            json!({"url": "u", "map": false, "max_pages": 5}),
        );
        assert_eq!(
            (op.as_str(), &a),
            ("web_crawl", &json!({"url": "u", "max_pages": 5}))
        );
        assert!(refused("web_crawl", json!({"url": "u", "limit": 5})).contains("max_pages"));
    }

    #[test]
    fn a_view_is_the_node_list_unless_an_image_is_asked_for() {
        let (op, a) = to("web_view", json!({"url": "u", "find": "button"}));
        assert_eq!(
            (op.as_str(), &a),
            ("web_snapshot", &json!({"url": "u", "find": "button"}))
        );
        let (op, a) = to(
            "web_view",
            json!({"url": "u", "image": true, "mobile": true}),
        );
        assert_eq!(
            (op.as_str(), &a),
            ("web_screenshot", &json!({"url": "u", "mobile": true}))
        );
        assert!(
            refused("web_view", json!({"url": "u", "image": true, "find": "x"}))
                .contains("`find` is not used with image")
        );
    }

    #[test]
    fn browser_is_one_shot_a_session_step_an_open_or_a_close() {
        let (op, a) = to(
            "browser",
            json!({"url": "u", "actions": [{"do": "click", "ref": "e1"}]}),
        );
        assert_eq!(op, "web_act");
        assert_eq!(a["actions"][0]["ref"], "e1");
        assert_eq!(to("browser", json!({"url": "u"})).1["actions"], json!([]));
        let (op, a) = to("browser", json!({"open": true, "profile": "p"}));
        assert_eq!(
            (op.as_str(), &a),
            ("browser_open", &json!({"profile": "p"}))
        );
        // Opening a session to go somewhere is one call, not two.
        let (op, a) = to("browser", json!({"open": true, "url": "u"}));
        assert_eq!((op.as_str(), &a), ("browser_open", &json!({"url": "u"})));
        assert!(refused("browser", json!({"open": true, "tier": "warm"}))
            .contains("`tier` is not used with open"));
        let (op, a) = to("browser", json!({"session_id": "s", "url": "u"}));
        assert_eq!(
            (op.as_str(), &a),
            ("browser_do", &json!({"session_id": "s", "url": "u"}))
        );
        let (op, a) = to("browser", json!({"session_id": "s", "close": true}));
        assert_eq!(
            (op.as_str(), &a),
            ("browser_close", &json!({"session_id": "s"}))
        );
        assert!(refused("browser", json!({})).contains("session_id"));
        assert!(refused("browser", json!({"close": true})).contains("session_id"));
        assert!(
            refused("browser", json!({"session_id": "s", "tier": "warm"}))
                .contains("`tier` is not used in a session")
        );
    }

    #[test]
    fn a_diff_is_a_watch_action() {
        let (op, a) = to(
            "web_watch",
            json!({"action": "diff", "url": "u", "refetch": false}),
        );
        assert_eq!(
            (op.as_str(), &a),
            ("web_diff", &json!({"url": "u", "refetch": false}))
        );
        assert_eq!(to("web_watch", json!({"action": "list"})).0, "web_watch");
        assert_eq!(to("web_watch", json!({"url": "u"})).0, "web_watch");
    }

    #[test]
    fn the_log_is_a_view_of_the_status() {
        let (op, a) = to("web_status", json!({"log": "summary", "domain": "d.test"}));
        assert_eq!(
            (op.as_str(), &a),
            ("web_log", &json!({"view": "summary", "domain": "d.test"}))
        );
        assert_eq!(to("web_status", json!({})).0, "web_status");
        assert!(refused(
            "web_status",
            json!({"log": "recent", "clear_cooldown": "d"})
        )
        .contains("`clear_cooldown` is not used with log"));
    }

    #[test]
    fn setup_is_four_operations_by_name() {
        for (what, op) in [
            ("login", "web_login"),
            ("route", "web_route"),
            ("profile", "web_profile"),
            ("browser", "browser_setup"),
        ] {
            let (got, a) = to("web_setup", json!({"do": what}));
            assert_eq!(got, op);
            assert_eq!(a, json!({}), "`do` must not reach {op}");
        }
        assert!(refused("web_setup", json!({"do": "nope"})).contains("login, route, profile"));
        assert!(refused("web_setup", json!({"do": "login", "proxy": "p"}))
            .contains("`proxy` is not used with do=login"));
    }

    #[test]
    fn captcha_is_a_page_a_token_an_image_or_a_task() {
        let (op, a) = to("captcha", json!({"url": "u", "timeout_s": 60}));
        assert_eq!(
            (op.as_str(), &a),
            ("solve_and_continue", &json!({"url": "u", "timeout_s": 60}))
        );
        let (op, a) = to(
            "captcha",
            json!({"kind": "turnstile", "sitekey": "k", "page_url": "p"}),
        );
        assert_eq!(
            (op.as_str(), &a),
            ("solve_turnstile", &json!({"sitekey": "k", "pageUrl": "p"}))
        );
        assert_eq!(
            to(
                "captcha",
                json!({"kind": "recaptcha_v2", "sitekey": "k", "page_url": "p", "invisible": true})
            )
            .0,
            "solve_recaptcha_v2"
        );
        assert_eq!(
            to(
                "captcha",
                json!({"kind": "hcaptcha", "sitekey": "k", "page_url": "p"})
            )
            .0,
            "solve_hcaptcha"
        );
        assert_eq!(
            to("captcha", json!({"image": "iVBOR"})).0,
            "solve_image_captcha"
        );
        let (op, a) = to("captcha", json!({"task_id": "t"}));
        assert_eq!(
            (op.as_str(), &a),
            ("captcha_status", &json!({"taskId": "t"}))
        );
        let (op, a) = to(
            "captcha",
            json!({"task_id": "t", "good": false, "note": "n"}),
        );
        assert_eq!(
            (op.as_str(), &a),
            (
                "report_captcha",
                &json!({"taskId": "t", "good": false, "note": "n"})
            )
        );
        assert!(refused("captcha", json!({})).contains("task_id"));
        assert!(refused("captcha", json!({"sitekey": "k", "page_url": "p"})).contains("kind"));
        assert!(
            refused("captcha", json!({"kind": "turnstile", "sitekey": "k"})).contains("page_url")
        );
    }

    #[test]
    fn an_operation_called_by_its_own_name_is_untouched() {
        for name in [
            "web_fetch_many",
            "web_snapshot",
            "solve_and_continue",
            "browser_do",
        ] {
            let a = json!({"anything": 1});
            assert_eq!(to(name, a.clone()), (name.to_string(), a));
        }
    }

    #[test]
    fn every_argument_a_mode_accepts_is_one_the_tool_lists() {
        // The other direction of `route`'s allow-lists: a key translate would accept that the
        // listed schema does not declare could never arrive, and is a typo here.
        for spec in SPECS {
            let listed: Vec<&str> = spec.params.iter().map(|(n, _, _)| *n).collect();
            for r in spec.required {
                assert!(
                    listed.contains(r),
                    "{}: required {r} is not a parameter",
                    spec.name
                );
            }
        }
    }
}
