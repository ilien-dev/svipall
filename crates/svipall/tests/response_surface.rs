//! What the model reads *after* every call.
//!
//! `tool_surface.rs` holds the shape of what is read before a tool is chosen. This is the other
//! half, and it is the half that repeats: a tool definition is read once per session, a response
//! envelope is read once per page — fifty times in one `web_fetch_many`, hundreds in one crawl.
//! Two rules keep it honest. A field whose value is the default says nothing, so it is absent; a
//! `null` says even less. And the `note` on a blocked page is the most-read sentence svipall
//! writes, so it has to name the tool that ends the problem, not the one next to it.
mod support;

use serde_json::Value;
use support::{Reply, Site};
use svipall::server::SvipallServer;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(None, svipall_core::Config::default(), None)
}

/// A server that has a solver behind it, which is what turns on the captcha half of the note.
fn server_with_solver() -> SvipallServer {
    support::isolate();
    let state = svipall_solver::AppState::new(
        svipall_solver::db::Db::open_memory().expect("in-memory solver db"),
        svipall_solver::queue::JobQueue::new(),
    );
    SvipallServer::new(
        Some(std::sync::Arc::new(state)),
        svipall_core::Config::default(),
        Some("http://127.0.0.1:8787".into()),
    )
}

const ARTICLE: &str = "<!doctype html><html><head><title>Harbour</title></head><body><main><p>\
    The council voted on Tuesday to approve the harbour measure after a debate that ran past \
    midnight. Supporters argued the change was overdue and that the money was already set aside \
    for it three budgets ago, and that the quay would reopen in November.</p></main></body></html>";

const TURNSTILE_WALL: &str = "<!doctype html><html><head><title>Just a moment...</title></head>\
    <body><div id=\"cf-challenge-running\"></div><h1>Checking your browser before accessing</h1>\
    <p>Please enable JavaScript and cookies to continue.</p>\
    <div class=\"cf-turnstile\" data-sitekey=\"0x4AAA\"></div></body></html>";

/// Every `(path, key, value)` in a JSON tree.
fn walk<'a>(v: &'a Value, path: String, out: &mut Vec<(String, &'a Value)>) {
    match v {
        Value::Object(m) => {
            for (k, child) in m {
                let p = format!("{path}/{k}");
                out.push((p.clone(), child));
                walk(child, p, out);
            }
        }
        Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                walk(child, format!("{path}/{i}"), out);
            }
        }
        _ => {}
    }
}

fn nulls(v: &Value) -> Vec<String> {
    let mut pairs = Vec::new();
    walk(v, String::new(), &mut pairs);
    pairs
        .into_iter()
        .filter(|(_, val)| val.is_null())
        .map(|(p, _)| p)
        .collect()
}

async fn fetch(server: &SvipallServer, url: String) -> Value {
    server
        .fetch_json(
            serde_json::from_value(serde_json::json!({"url": url, "max_tier": "http"}))
                .expect("params"),
        )
        .await
        .value
}

#[tokio::test]
async fn a_response_never_spends_tokens_on_a_null() {
    // A `null` is the Rust `Option` leaking onto the wire. It answers no question the absence of
    // the key would not answer, and it is read on every page: `"exit":null` alone is twelve
    // characters times every row of every crawl.
    let site = Site::start(vec![
        ("/article", Reply::html(ARTICLE)),
        ("/wall", Reply::html(TURNSTILE_WALL)),
    ])
    .await;

    let page = fetch(&server(), site.url("/article")).await;
    assert!(
        nulls(&page).is_empty(),
        "a delivered page carries nulls at {:?}: {page}",
        nulls(&page)
    );

    let wall = fetch(&server(), site.url("/wall")).await;
    assert!(
        nulls(&wall).is_empty(),
        "a blocked page carries nulls at {:?}: {wall}",
        nulls(&wall)
    );
}

#[tokio::test]
async fn an_ordinary_page_does_not_repeat_what_ordinary_means() {
    // `native_fallback: false` and a `final_url` equal to the `url` are the two fields that are
    // the same on almost every page svipall returns. Absent, each one becomes informative: the
    // field appears exactly when something happened.
    let site = Site::start(vec![("/article", Reply::html(ARTICLE))]).await;
    let url = site.url("/article");
    let page = fetch(&server(), url.clone()).await;

    assert!(
        page.get("native_fallback").is_none(),
        "no native attempt was made, so the flag says nothing: {page}"
    );
    assert!(
        page.get("final_url").is_none(),
        "nothing redirected, so final_url repeats url: {page}"
    );
    assert_eq!(page["url"], url, "{page}");
    // The privacy-relevant one stays affirmative: silence must never be how a caller learns that
    // their real device characteristics were not exposed.
    assert_eq!(page["identity_used"], "emulated", "{page}");
}

#[tokio::test]
async fn the_note_on_a_captcha_names_the_tool_that_returns_the_page() {
    // The note arrives at the moment of the decision, with the arguments filled in, so it beats
    // every general instruction the model was given earlier. It used to say
    // `call solve_turnstile(sitekey=…, pageUrl=…)`, which returns a bare token bound to the
    // session that produced it — the wrong branch whenever the goal is the content, which is
    // almost always.
    let site = Site::start(vec![("/wall", Reply::html(TURNSTILE_WALL))]).await;
    let wall = fetch(&server_with_solver(), site.url("/wall")).await;

    let note = wall["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("solve_and_continue"),
        "the note never names the tool that ends the problem: {note:?}"
    );
    let token_tool = note.find("solve_turnstile");
    if let Some(at) = token_tool {
        assert!(
            note.find("solve_and_continue").expect("checked above") < at,
            "the token tool is offered before the one that returns the page: {note:?}"
        );
        assert!(
            note.contains("post") || note.contains("submit"),
            "the token tool is offered without the condition that makes it right: {note:?}"
        );
    }
    assert!(
        !note.contains(&site.url("/wall")),
        "the note repeats the address of the page it is about: {note:?}"
    );
    // The widget is still reported: the note is advice, `challenge` and `widgets` are the facts.
    assert_eq!(wall["challenge"]["kind"], "turnstile", "{wall}");
}

#[tokio::test]
async fn the_envelope_around_a_page_fits_a_budget() {
    // Measured, not asserted: the overhead is everything in a row that is not the content itself,
    // and it is paid once per page. 370 characters before the nulls, the false flag and the
    // repeated `final_url` came out; 295 after, on a page whose content is 250. What is left is
    // the page's own address, its title, the tier that answered, the attempt trail and the
    // quality verdict — each of which a caller acts on. The cap is the measured result plus room
    // for one more field.
    let site = Site::start(vec![
        ("/a", Reply::html(ARTICLE)),
        ("/b", Reply::html(ARTICLE)),
        ("/c", Reply::html(ARTICLE)),
    ])
    .await;
    let out = server()
        .fetch_many_json(
            serde_json::from_value(serde_json::json!({
                "urls": [site.url("/a"), site.url("/b"), site.url("/c")],
                "max_tier": "http",
            }))
            .expect("params"),
        )
        .await;

    let rows = out["results"].as_array().expect("results");
    assert_eq!(rows.len(), 3, "{out}");
    let overhead: usize = rows
        .iter()
        .map(|r| {
            let whole = serde_json::to_string(r).expect("json").len();
            let content = r["content"].as_str().unwrap_or_default().len();
            whole - content
        })
        .sum::<usize>()
        / rows.len();
    assert!(
        overhead <= 310,
        "the envelope around one page is {overhead} characters, and a crawl pays it per row"
    );
}

#[test]
fn every_tool_and_parameter_a_note_names_is_a_real_one() {
    // The note is the only guidance that arrives *after* the model has already made one choice,
    // which makes it the one most likely to be acted on literally. A tool that does not exist or
    // a parameter that was renamed therefore costs a whole round trip and, on a wall, a retry the
    // domain charges a cooldown for. Checked against the built tool list, so a rename cannot pass.
    support::isolate();
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);
    let tools = server.tools();
    let known: std::collections::HashMap<String, std::collections::HashSet<String>> = tools
        .iter()
        .map(|t| {
            let props = t
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            (t.name.to_string(), props)
        })
        .collect();

    let mut messages: Vec<String> = [
        svipall_core::WallKind::None,
        svipall_core::WallKind::Cloudflare,
        svipall_core::WallKind::Vendor,
        svipall_core::WallKind::Generic,
        svipall_core::WallKind::Empty,
        svipall_core::WallKind::Gate,
        svipall_core::WallKind::Hold,
        svipall_core::WallKind::Login,
        svipall_core::WallKind::NotFound,
        svipall_core::WallKind::SoftNotFound,
        svipall_core::WallKind::Paywall,
        svipall_core::WallKind::Status,
    ]
    .iter()
    .flat_map(|k| {
        [
            svipall::steer::wall_next_step(
                k,
                "shop.example",
                "http",
                false,
                "http://127.0.0.1:8787",
            ),
            svipall::steer::wall_next_step(
                k,
                "shop.example",
                "http",
                true,
                "http://127.0.0.1:8787",
            ),
        ]
    })
    .collect();
    messages.push(svipall::steer::captcha_next_step(
        "Turnstile",
        "solve_turnstile",
        "0x4AAA",
    ));
    messages.push(svipall::steer::unknown_task("t"));
    messages.push(svipall::steer::unknown_session("s"));
    messages.push(svipall::steer::not_a_url("x"));

    // Scanned by hand rather than with a regex: the shape is `name(` … `)` and `name=value`, and
    // pulling a crate in for that would be the larger change.
    for note in &messages {
        let bytes: Vec<char> = note.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if !(bytes[i].is_ascii_lowercase() || bytes[i] == '_') {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let word: String = bytes[start..i].iter().collect();
            let looks_like_a_tool = ["web_", "browser_", "solve_", "captcha_", "report_"]
                .iter()
                .any(|p| word.starts_with(p));
            if !looks_like_a_tool {
                continue;
            }
            let Some(props) = known.get(&word) else {
                panic!("a note names {word}, which is not a tool: {note}");
            };
            if i >= bytes.len() || bytes[i] != '(' {
                continue;
            }
            let args_from = i + 1;
            let Some(close) = bytes[args_from..].iter().position(|c| *c == ')') else {
                continue;
            };
            let args: String = bytes[args_from..args_from + close].iter().collect();
            for arg in args.split(',') {
                let Some((name, _)) = arg.split_once('=') else {
                    continue;
                };
                let name = name.trim();
                if name.is_empty() || name.contains(' ') {
                    continue;
                }
                assert!(
                    props.contains(name),
                    "a note passes {name} to {word}, which has no such parameter: {note}"
                );
            }
        }
    }
}
