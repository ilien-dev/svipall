//! What the model reads before it chooses a tool, and what a call to it becomes.
//!
//! A client either loads every definition into the system prompt on every turn, or shows the tool
//! *names* and the server `instructions` and loads a definition when its tool is picked. Either
//! way the text is read by a model, never by a person, and it is paid for in tokens: so the list
//! is twelve tools (`surface`), each a rewrite onto the operations the router already had, and
//! these tests hold both halves — the size of what is read, and that nothing the 31 operations
//! did was lost on the way. The numbers are budgets, measured on the built list.
mod support;

use rmcp::{model::Tool, ServerHandler};
use serde_json::{json, Map, Value};
use svipall::server::SvipallServer;
use svipall::surface;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(None, svipall_core::Config::default(), None)
}

fn tools() -> Vec<Tool> {
    server().tools()
}

fn instructions() -> String {
    server().get_info().instructions.expect("instructions")
}

fn description(t: &Tool) -> &str {
    t.description.as_deref().unwrap_or("")
}

fn schema_len(t: &Tool) -> usize {
    serde_json::to_string(&*t.input_schema).expect("json").len()
}

fn tool(name: &str) -> Tool {
    tools()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{name} is not a listed tool"))
}

fn props(schema: &Map<String, Value>) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// Every `(key, value)` pair in a JSON tree, with the path that reaches it.
fn walk<'a>(v: &'a Value, path: String, out: &mut Vec<(String, &'a str, &'a Value)>) {
    if let Value::Object(m) = v {
        for (k, child) in m {
            out.push((path.clone(), k.as_str(), child));
            walk(child, format!("{path}/{k}"), out);
        }
    }
}

const LISTED: &[&str] = &[
    "web_fetch",
    "web_search",
    "web_crawl",
    "web_view",
    "browser",
    "web_watch",
    "web_status",
    "web_setup",
    "captcha",
    "web_capture",
    "web_video",
    "web_notes",
];

#[test]
fn twelve_tools_are_listed_and_no_other() {
    // 31 until 2.0. A month of calls: three tools took 96% of them and thirteen took none, while
    // every name was shown to the model at the start of every session.
    let names: Vec<String> = tools().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names, LISTED,
        "the listed tools, in the order they are read"
    );
}

#[test]
fn the_whole_tool_list_fits_a_budget() {
    // With deferred loading off (a gateway, `ENABLE_TOOL_SEARCH=false`, `alwaysLoad`, most clients
    // that are not Claude Code) every definition sits in the system prompt on every turn.
    //
    // 1.4.0 measured 38 547 characters over 31 tools by this same sum. 2.0 measures 19 441 over
    // twelve: the per-operation tools folded into the tool they were a mode of (the action
    // schema, read twice, is read once; five captcha tools repeated one warning five times), and
    // every sentence was cut to what changes the tool picked or the argument sent. The cap is the
    // measured result plus room for one small parameter, so growth is a decision, not a drift.
    //
    // 2.1 measures 19 927: `web_video` learned to show a video, which is five arguments
    // (`start`, `end`, `at`, `crop`, `query`) and one sentence saying they exist, 486 characters.
    let total: usize = tools()
        .iter()
        .map(|t| t.name.len() + description(t).len() + schema_len(t))
        .sum();
    assert!(
        total <= 20_000,
        "the tool list is {total} chars; the model reads all of it on every request"
    );
}

#[test]
fn the_three_tools_that_take_nearly_every_call_stay_light() {
    // With deferred loading a definition is paid for when its tool is first used, so what matters
    // is the size of the tools actually used. In 1.4.0 a session that fetched one page, fetched
    // several and searched loaded web_fetch (5 558), web_fetch_many (2 092) and web_search
    // (1 138): 8 788 characters. The same session now loads two definitions: 4 966.
    let cost = |name: &str| {
        let t = tool(name);
        t.name.len() + description(&t).len() + schema_len(&t)
    };
    let (fetch, search) = (cost("web_fetch"), cost("web_search"));
    assert!(fetch <= 4_300, "web_fetch is {fetch} chars");
    assert!(search <= 1_100, "web_search is {search} chars");
}

#[test]
fn every_tool_has_a_description_that_routes() {
    for t in tools() {
        let d = description(&t);
        assert!(
            d.len() >= 60,
            "{}: description is too thin to route on: {d:?}",
            t.name
        );
        assert!(
            d.len() <= 800,
            "{}: description is {} chars; a client truncates at 2 KB and the model reads every \
             one of them",
            t.name,
            d.len()
        );
    }
}

#[test]
fn schemas_carry_nothing_a_model_reads_past() {
    // No `$schema`, `title`, `nullable`, `format`, `default`, `$defs`, root description or
    // `additionalProperties`: none of it changes a call, all of it is tokens.
    for t in tools() {
        let schema = Value::Object((*t.input_schema).clone());
        assert!(schema.get("description").is_none(), "{}", t.name);
        let mut pairs = Vec::new();
        walk(&schema, String::new(), &mut pairs);
        for (path, key, value) in pairs {
            // A parameter may be *named* `default` or `title`; only schema keywords are judged.
            if path.ends_with("/properties") {
                continue;
            }
            let bad = matches!(
                key,
                "$schema"
                    | "title"
                    | "nullable"
                    | "format"
                    | "default"
                    | "$defs"
                    | "definitions"
                    | "$ref"
                    | "additionalProperties"
                    | "minimum"
            );
            assert!(!bad, "{}: {path}/{key} = {value} is boilerplate", t.name);
        }
    }
}

#[test]
fn a_parameter_is_described_unless_its_name_says_it_all() {
    // A property with a type and no description is a guess for the model, except the handful
    // whose name is the whole of it. A long one is a measurement note that belongs in a comment.
    let obvious = ["url", "query"];
    for t in tools() {
        let Some(p) = t.input_schema.get("properties").and_then(Value::as_object) else {
            continue;
        };
        for (name, prop) in p {
            let d = prop
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            assert!(
                d.len() >= 4 || obvious.contains(&name.as_str()),
                "{}.{name}: no description",
                t.name
            );
            assert!(
                d.len() <= 260,
                "{}.{name}: description is {} chars",
                t.name,
                d.len()
            );
        }
    }
}

#[test]
fn each_description_names_the_field_that_matters_or_the_tool_to_prefer() {
    let must_mention: &[(&str, &[&str])] = &[
        (
            "web_fetch",
            &[
                "query",
                "schema",
                "out_file",
                "cursor",
                "blocked_reason",
                "urls",
                "probe",
                "404",
            ],
        ),
        ("web_search", &["site", "note", "web_fetch"]),
        ("web_crawl", &["map", "out_file", "crawl_id", "web_fetch"]),
        ("web_view", &["ref", "image", "browser"]),
        (
            "browser",
            &["actions", "open", "session_id", "close", "web_view"],
        ),
        ("web_watch", &["diff", "add", "check"]),
        ("web_status", &["log", "sections", "clear_"]),
        ("web_setup", &["login", "route", "profile", "browser"]),
        (
            "captcha",
            &["url", "blocked_reason", "sitekey", "image", "task_id"],
        ),
        ("web_capture", &["pattern", "bodies"]),
        ("web_video", &["web_fetch", "drm"]),
        ("web_notes", &["across sessions"]),
    ];
    for (name, needles) in must_mention {
        let t = tool(name);
        for needle in *needles {
            assert!(
                description(&t).contains(needle),
                "{name}: description does not mention {needle:?}"
            );
        }
    }
    let covered: Vec<&str> = must_mention.iter().map(|(n, _)| *n).collect();
    for name in LISTED {
        assert!(covered.contains(name), "{name} has no routing expectation");
    }
}

#[test]
fn descriptions_name_no_vendor() {
    // The project rule: name protocols by endpoint, never a brand. Widget names that are also
    // the protocol (turnstile, recaptcha, hcaptcha) are the protocol.
    for t in tools() {
        let d = description(&t).to_ascii_lowercase();
        for brand in ["duckduckgo", "bing", "brave", "cloudflare", "google"] {
            assert!(!d.contains(brand), "{}: names {brand}", t.name);
        }
    }
}

#[test]
fn the_instructions_route_every_task_to_a_name() {
    // The one piece of prose the model sees before choosing, for the whole session. 1 889
    // characters in 1.4.0; the budget is what the rewrite measured plus a margin.
    let text = instructions();
    assert!(text.len() <= 1_600, "instructions are {} chars", text.len());
    for name in LISTED {
        assert!(text.contains(name), "instructions never mention {name}");
    }
    assert!(
        text.contains("blocked_reason") && text.contains("Never retry"),
        "the two rules that cost cooldowns are missing"
    );
    // Found in a month of real calls: agents never learned that the visit budget is shared by
    // every session on the machine, that `wait` exists, or that a binary needs `out_file`; and
    // they ran curl and gh for what a probe and the GitHub path answer.
    for needle in [
        "urls=",
        "probe=true",
        "image=true",
        "map=true",
        "site=",
        "shared by every session",
        "wait=",
        "json_path",
        "out_file",
        "github.com",
        "do=login",
        "do=route",
        "log=summary",
        "clear_cooldown",
    ] {
        assert!(text.contains(needle), "instructions never mention {needle}");
    }
    // The owner's call: code search across GitHub belongs to the dedicated tool when the session
    // has it, and svipall has to say so and keep working fully without it.
    for needle in ["searchGitHub", "grep.app", "when that tool is absent"] {
        assert!(
            text.contains(needle),
            "instructions lack the code-search preference: {needle}"
        );
    }
    // A sentence about language in a system prompt is read as a directive about the *answer*.
    assert!(
        !text.to_ascii_lowercase().contains("in english"),
        "the instructions tell the model what language to work in: {text}"
    );
}

#[test]
fn nothing_the_model_reads_names_an_operation_it_cannot_call() {
    // The operations are still in the router, so a 1.x client keeps working; but a model shown
    // twelve tools cannot call a thirteenth, and a sentence that names one sends it nowhere.
    let s = server();
    let gone: Vec<String> = s
        .tool_names()
        .into_iter()
        .filter(|n| !LISTED.contains(&n.as_str()))
        .collect();
    assert!(gone.len() >= 19, "the router lost operations: {gone:?}");
    let mut texts = vec![("instructions".to_string(), instructions())];
    for t in tools() {
        texts.push((
            t.name.to_string(),
            format!(
                "{} {}",
                description(&t),
                serde_json::to_string(&*t.input_schema).expect("json")
            ),
        ));
    }
    for (where_, text) in texts {
        for name in &gone {
            let named = text.match_indices(name.as_str()).any(|(at, _)| {
                // `web_fetch_many` contains no listed name, but `browser_open` must not be
                // confused with a word that merely starts the same way.
                let after = text[at + name.len()..].chars().next();
                !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            });
            assert!(!named, "{where_} names `{name}`, which is not listed");
        }
    }
}

#[test]
fn actions_are_typed_once() {
    // `items: true` left every action's shape to prose; the enum is what stops an invented verb.
    // It was read twice, in web_act and browser_do.
    let kinds = [
        "click",
        "type",
        "fill",
        "press",
        "hover",
        "select",
        "scroll",
        "wait",
        "eval",
        "goto",
        "verify",
        "console",
        "screenshot",
        "hold",
    ];
    let t = tool("browser");
    let items = &t.input_schema["properties"]["actions"]["items"];
    assert_eq!(items["type"], "object", "{items}");
    assert_eq!(items["required"], json!(["do"]), "{items}");
    let found = items["properties"]["do"]["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("`do` is not an enum: {items}"));
    for kind in kinds {
        assert!(found.iter().any(|v| v == kind), "`do` enum lacks {kind}");
    }
    for field in [
        "ref",
        "selector",
        "text",
        "key",
        "value",
        "ms",
        "pixels",
        "until",
        "rounds",
        "script",
        "url",
        "full_page",
    ] {
        assert!(
            items["properties"][field].is_object(),
            "action field {field} missing"
        );
    }
    let with_actions = tools()
        .iter()
        .filter(|t| t.input_schema["properties"]["actions"].is_object())
        .count();
    assert_eq!(with_actions, 1, "the action schema is listed once");
}

#[test]
fn a_row_schema_declares_both_shapes() {
    // "auto" or an object. Without a declared type the model had to infer that from prose.
    for name in ["web_fetch", "web_crawl"] {
        let prop = &tool(name).input_schema["properties"]["schema"];
        assert_eq!(prop["type"], json!(["string", "object"]), "{name}: {prop}");
    }
}

// ---- nothing was lost ---------------------------------------------------------------------------

/// One call per operation, as a model would make it against the listed tools: `(listed tool,
/// arguments, the operation it must reach)`.
fn calls() -> Vec<(&'static str, Value, &'static str)> {
    vec![
        ("web_fetch", json!({"url": "https://a.test/"}), "web_fetch"),
        (
            "web_fetch",
            json!({"urls": ["https://a.test/"], "query": "q", "max_tokens_total": 4000, "out_dir": "d"}),
            "web_fetch_many",
        ),
        (
            "web_fetch",
            json!({"urls": ["https://a.test/"], "probe": true, "timeout": 5000}),
            "web_probe",
        ),
        (
            "web_search",
            json!({"query": "q", "engine": "all", "limit": 5, "lang": "es", "region": "MX"}),
            "web_search",
        ),
        (
            "web_search",
            json!({"query": "q", "site": "https://a.test/", "fetch": false, "timeout": 1000}),
            "web_site_search",
        ),
        (
            "web_crawl",
            json!({"url": "https://a.test/", "max_pages": 3}),
            "web_crawl",
        ),
        (
            "web_crawl",
            json!({"url": "https://a.test/", "map": true, "limit": 10, "include": "/docs/", "sources": ["sitemap"]}),
            "web_map",
        ),
        (
            "web_view",
            json!({"url": "https://a.test/", "find": "button", "max_depth": 3, "limit": 50, "tier": "real", "profile": "p", "timeout": 1000}),
            "web_snapshot",
        ),
        (
            "web_view",
            json!({"url": "https://a.test/", "image": true, "full_page": true, "mobile": true, "inline": false, "tier": "real", "profile": "p", "proxy": "http://p.test:1", "timeout": 1000}),
            "web_screenshot",
        ),
        (
            "browser",
            json!({"url": "https://a.test/", "actions": [{"do": "click", "ref": "e1"}], "extraction": "text", "tier": "real", "profile": "p", "proxy": "http://p.test:1", "timeout": 1000}),
            "web_act",
        ),
        (
            "browser",
            json!({"open": true, "profile": "p", "proxy": "http://p.test:1", "visible": true, "url": "https://a.test/", "actions": [], "extraction": "text", "query": "q", "timeout": 1000}),
            "browser_open",
        ),
        (
            "browser",
            json!({"session_id": "s", "url": "https://a.test/", "actions": [], "extraction": "text", "query": "q", "timeout": 1000}),
            "browser_do",
        ),
        (
            "browser",
            json!({"session_id": "s", "close": true}),
            "browser_close",
        ),
        (
            "web_watch",
            json!({"action": "add", "url": "https://a.test/", "interval_secs": 60, "label": "l", "css_selector": "main"}),
            "web_watch",
        ),
        (
            "web_watch",
            json!({"action": "diff", "url": "https://a.test/", "refetch": false}),
            "web_diff",
        ),
        (
            "web_status",
            json!({"sections": "all", "clear_cooldown": "a.test", "forget_tier": "a.test", "clear_budget": "a.test", "clear_cache": true, "configure": {}}),
            "web_status",
        ),
        (
            "web_status",
            json!({"log": "recent", "domain": "a.test", "since_secs": 60, "limit": 5}),
            "web_log",
        ),
        (
            "web_setup",
            json!({"do": "login", "url": "https://a.test/", "profile": "p", "timeout_s": 30}),
            "web_login",
        ),
        (
            "web_setup",
            json!({"do": "route", "domain": "a.test", "proxy": "http://p.test:1", "country": "DE", "proxies": [], "countries": [], "remove": false, "check": false, "check_url": "https://a.test/"}),
            "web_route",
        ),
        (
            "web_setup",
            json!({"do": "profile", "action": "export", "name": "p", "file": "f", "password": "x"}),
            "web_profile",
        ),
        (
            "web_setup",
            json!({"do": "browser", "action": "status", "artifact": "chrome"}),
            "browser_setup",
        ),
        (
            "captcha",
            json!({"url": "https://a.test/", "profile": "p", "extraction": "text", "timeout_s": 30, "max_tokens": 1000}),
            "solve_and_continue",
        ),
        (
            "captcha",
            json!({"kind": "turnstile", "sitekey": "k", "page_url": "https://a.test/"}),
            "solve_turnstile",
        ),
        (
            "captcha",
            json!({"kind": "recaptcha_v2", "sitekey": "k", "page_url": "https://a.test/", "invisible": true}),
            "solve_recaptcha_v2",
        ),
        (
            "captcha",
            json!({"kind": "hcaptcha", "sitekey": "k", "page_url": "https://a.test/"}),
            "solve_hcaptcha",
        ),
        (
            "captcha",
            json!({"image": "iVBORw0KGgo="}),
            "solve_image_captcha",
        ),
        ("captcha", json!({"task_id": "t"}), "captcha_status"),
        (
            "captcha",
            json!({"task_id": "t", "good": true, "note": "n"}),
            "report_captcha",
        ),
        (
            "web_capture",
            json!({"url": "https://a.test/", "pattern": "/api/", "bodies": true, "max_body": 100, "settle_ms": 100, "tier": "real", "profile": "p"}),
            "web_capture",
        ),
        (
            "web_video",
            json!({"url": "https://a.test/", "lang": "es", "frames": 2, "max_tokens": 100, "cursor": "c", "out_file": "f", "profile": "p", "wait": 1}),
            "web_video",
        ),
        (
            "web_notes",
            json!({"action": "set", "key": "k", "value": "v", "prefix": "p"}),
            "web_notes",
        ),
    ]
}

#[test]
fn every_operation_is_reached_from_a_listed_tool() {
    // The whole point of "nothing was lost": each of the operations the router has is the target
    // of some call to the twelve.
    let s = server();
    let reached: Vec<&str> = calls().iter().map(|(_, _, op)| *op).collect();
    for op in s.tool_names() {
        assert!(
            reached.contains(&op.as_str()),
            "no listed tool reaches the operation `{op}`"
        );
    }
}

#[test]
fn a_call_to_a_listed_tool_arrives_as_arguments_its_operation_accepts() {
    // Both checks `call_tool` makes, on every sample: the listed schema knows every key the model
    // sent, and the operation's own schema knows every key the rewrite hands it. A rename missed
    // in `surface::translate` (`page_url` for `pageUrl`) is a field the operation silently never
    // reads, which is the failure `params` exists to end.
    let s = server();
    for (listed, args, op) in calls() {
        let mut args = args.as_object().expect("object").clone();
        let schema = surface::listed_schema(listed).expect("listed");
        let unknown = svipall::params::normalize(&schema, &mut args);
        assert!(unknown.is_empty(), "{listed}: the list lacks {unknown:?}");
        let (got, mut out) = surface::translate(listed, args)
            .unwrap_or_else(|e| panic!("{listed} -> {op}: refused: {e}"));
        assert_eq!(got, op, "{listed}");
        // What a multi-URL fetch asks of every page is a single fetch's arguments.
        if let Some(Value::Object(mut each)) = out.remove("each") {
            let single = s.operation_schema("web_fetch").expect("web_fetch");
            let unknown = svipall::params::normalize(&single, &mut each);
            assert!(unknown.is_empty(), "each: web_fetch lacks {unknown:?}");
        }
        let own = s
            .operation_schema(op)
            .unwrap_or_else(|| panic!("{op} is not an operation"));
        let unknown = svipall::params::normalize(&own, &mut out);
        assert!(
            unknown.is_empty(),
            "{listed} -> {op}: the operation has no {unknown:?}"
        );
        for required in own
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            assert!(
                out.contains_key(required),
                "{listed} -> {op}: `{required}` never arrives"
            );
        }
    }
}

#[test]
fn every_parameter_an_operation_takes_is_still_offered() {
    // Folding a tool into another must not drop a knob on the way. Every property of every
    // operation is a property of the listed tool that reaches it, under its own name or the one
    // it was given there. The two exceptions say why.
    let renamed = [
        ("web_site_search", "url", "site"),
        ("web_log", "view", "log"),
        ("solve_turnstile", "pageUrl", "page_url"),
        ("solve_recaptcha_v2", "pageUrl", "page_url"),
        ("solve_hcaptcha", "pageUrl", "page_url"),
        ("captcha_status", "taskId", "task_id"),
        ("report_captcha", "taskId", "task_id"),
    ];
    let dropped = [
        // Only ever needed when base64 could be mistaken for a URL, and no image encodes to a
        // string that starts with "http". The operation still reads it.
        ("solve_image_captcha", "is_base64"),
    ];
    let s = server();
    for (listed, _, op) in calls() {
        let offered = props(&surface::listed_schema(listed).expect("listed"));
        for p in props(&s.operation_schema(op).expect("operation")) {
            if dropped.contains(&(op, p.as_str())) {
                continue;
            }
            let name = renamed
                .iter()
                .find(|(o, from, _)| *o == op && *from == p)
                .map_or(p.as_str(), |(_, _, to)| *to);
            assert!(
                offered.iter().any(|o| o == name),
                "{op}.{p} is not offered by {listed}"
            );
        }
    }
}

#[test]
fn a_tool_that_kept_its_name_still_takes_what_it_took() {
    // A 1.x client calls `web_crawl` with 1.x arguments. For the tools whose name survived, the
    // listed schema is a superset of the operation's, and the rewrite leaves such a call alone.
    let s = server();
    for name in LISTED {
        let Some(own) = s.operation_schema(name) else {
            continue;
        };
        let offered = props(&surface::listed_schema(name).expect("listed"));
        let mut old = Map::new();
        for p in props(&own) {
            assert!(offered.contains(&p), "{name}.{p} was dropped from the list");
            old.insert(p, Value::Null);
        }
        // Nulls: only the keys matter here, and no mode switch is set by a null.
        old.insert("url".into(), json!("https://a.test/"));
        if let Ok((op, _)) = surface::translate(name, old) {
            assert_eq!(&op, name, "a 1.x call to {name} was sent elsewhere");
        }
    }
}

#[test]
fn an_operation_no_longer_listed_still_answers_to_its_name() {
    for op in [
        "web_fetch_many",
        "web_snapshot",
        "browser_do",
        "solve_and_continue",
    ] {
        assert!(surface::listed_schema(op).is_none(), "{op} is listed");
        assert!(server().operation_schema(op).is_some(), "{op} is gone");
        let args = json!({"x": 1}).as_object().expect("object").clone();
        assert_eq!(
            surface::translate(op, args.clone()).expect("untouched"),
            (op.to_string(), args)
        );
    }
}

/// The measurement itself, for whoever changes the list: `cargo test -p svipall --test
/// tool_surface measure -- --ignored --nocapture` prints what a client receives, by tool.
#[test]
#[ignore = "prints the measurement; not an assertion"]
fn measure() {
    let mut total = 0;
    for t in tools() {
        let (d, s) = (description(&t).len(), schema_len(&t));
        let n = t.input_schema["properties"].as_object().map_or(0, Map::len);
        total += t.name.len() + d + s;
        println!(
            "{:<12} description {d:>4}  schema {s:>5}  parameters {n:>2}",
            t.name
        );
    }
    println!("total {total}  instructions {}", instructions().len());
    if std::env::var_os("SVIPALL_PRINT_TOOLS").is_some() {
        println!("{}", serde_json::to_string(&tools()).expect("json"));
    }
}

/// Where a list of calls ends up, for the blind routing runs: reads `[{"id", "name",
/// "arguments"}]` from `SVIPALL_CALLS` and prints, per call, the operation and arguments it
/// reaches or the refusal it gets. Nothing is fetched.
#[test]
#[ignore = "a tool for measuring routing; needs SVIPALL_CALLS"]
fn route_calls() {
    let Some(path) = std::env::var_os("SVIPALL_CALLS") else {
        return;
    };
    let calls: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(path).expect("calls file")).expect("json");
    let s = server();
    let mut out = Vec::new();
    for call in calls {
        let name = call["name"].as_str().unwrap_or("").to_string();
        let mut args = call["arguments"].as_object().cloned().unwrap_or_default();
        let verdict = match surface::listed_schema(&name).or_else(|| s.operation_schema(&name)) {
            None => json!({"refused": format!("no tool {name}")}),
            Some(schema) => {
                let unknown = svipall::params::normalize(&schema, &mut args);
                if !unknown.is_empty() {
                    json!({"refused": format!("unknown parameters {unknown:?}")})
                } else {
                    match surface::translate(&name, args) {
                        Ok((op, args)) => json!({"op": op, "arguments": args}),
                        Err(why) => json!({"refused": why}),
                    }
                }
            }
        };
        out.push(json!({"id": call["id"], "name": name, "verdict": verdict}));
    }
    println!("ROUTED {}", serde_json::to_string(&out).expect("json"));
}
