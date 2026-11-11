//! What a caller is told about a response, judged by what kind of response it is.
//!
//! A JSON answer, a source file and an article are three different things, and the classifier and
//! the quality rules used to read all three as an article: a code host's search API answer that quoted an
//! issue titled "access denied" was reported as a bot wall and climbed every tier, and a complete
//! JSON object read as "thin" because it is not prose. Every test here goes through the real fetch
//! path against a local site and asserts on the answer the caller receives.
//!
//! The tests that must look like a remote site to the ladder (so that it climbs, learns and
//! caches as it does for one) use a hostname of their own and the local site as their proxy. They
//! share one isolated home, so no two of them use the same hostname.

mod support;

use serde_json::Value;
use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

/// A server that treats the fixture site as the internet, through it as a proxy.
fn remote_server() -> SvipallServer {
    support::isolate();
    SvipallServer::with_store(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            http_engine: "reqwest".into(),
            ..Default::default()
        },
        None,
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    )
}

fn local_server() -> SvipallServer {
    support::isolate();
    SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    )
}

fn remote(url: &str, site: &Site) -> WebFetchParams {
    WebFetchParams {
        url: url.into(),
        proxy: Some(site.url("")),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

fn local(url: String) -> WebFetchParams {
    WebFetchParams {
        url,
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

fn content(v: &Value) -> &str {
    v["content"].as_str().unwrap_or_default()
}

fn attempts(v: &Value) -> Vec<String> {
    v["attempts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn json(body: &str) -> Reply {
    Reply::bytes(body.as_bytes(), "application/json; charset=utf-8")
}

// ---- 12: a non-HTML answer is not phrase-matched as a wall --------------------------------------

/// The measured case, reduced: a search API answers 200 with JSON, and one of the results is an
/// issue whose title says "access denied". The ladder read the phrase, called it a bot wall, and
/// spent 33 seconds climbing every browser tier to fetch the same JSON again.
#[tokio::test]
async fn a_json_answer_that_quotes_access_denied_is_delivered_by_the_first_tier() {
    let url = "http://api-twelve.test/search/issues?q=login";
    let body = r#"{"total_count":2,"incomplete_results":false,"items":[
        {"number":101,"title":"Access denied when the token has expired","state":"open"},
        {"number":102,"title":"Verify you are a human prompt loops forever","state":"closed"}]}"#;
    let site = Site::start(vec![(url, json(body))]).await;
    let out = remote_server().fetch_json(remote(url, &site)).await.value;

    assert!(out["blocked_reason"].is_null(), "{out}");
    assert_eq!(out["status"], 200, "{out}");
    assert_eq!(
        attempts(&out).len(),
        1,
        "a JSON answer never climbs to a browser: {out}"
    );
    assert!(
        content(&out).contains("Access denied when the token has expired"),
        "{out}"
    );
}

/// 202 and 405 are on the list of statuses a bot wall uses, and on an HTML page they still are. On
/// an API they are ordinary answers: "accepted, still working" and "use another method". The body
/// says so, and the caller needs the body, not a wall.
#[tokio::test]
async fn an_api_status_that_is_an_answer_is_returned_as_one() {
    let accepted = "http://api-twelve-b.test/jobs/7";
    let wrong_method = "http://api-twelve-b.test/jobs";
    let site = Site::start(vec![
        (
            accepted,
            json(r#"{"id":7,"state":"queued","poll":"/jobs/7"}"#).with_status(202),
        ),
        (
            wrong_method,
            json(r#"{"message":"Method Not Allowed","allowed":["POST"]}"#).with_status(405),
        ),
    ])
    .await;
    let server = remote_server();
    for (url, status) in [(accepted, 202), (wrong_method, 405)] {
        let out = server.fetch_json(remote(url, &site)).await.value;
        assert!(out["blocked_reason"].is_null(), "{status}: {out}");
        assert_eq!(out["status"], status, "{out}");
        assert_eq!(attempts(&out).len(), 1, "{out}");
    }
}

/// The rule is about JSON, not about the header that claims to be JSON. A wall served with the
/// wrong content type is still a wall.
#[tokio::test]
async fn an_html_wall_labelled_as_json_is_still_a_wall() {
    let site = Site::start(vec![(
        "/api/data",
        json(
            "<!DOCTYPE html><html><head><title>Just a moment...</title></head><body>\
             Checking your browser before accessing the site.</body></html>",
        ),
    )])
    .await;
    let out = local_server()
        .fetch_json(local(site.url("/api/data")))
        .await
        .value;
    assert!(out["blocked_reason"].is_string(), "{out}");
}

// ---- 13a: a refusal rendered by a browser is still a refusal ----------------------------------

/// The measured case: a forum's `.json` endpoint refused the http tier with 403, the browser tier
/// rendered the same 130-byte refusal, and the fetch reported "browser: 200 OK".
#[tokio::test]
#[ignore = "opens a local browser against a loopback proxy"]
async fn a_refusal_the_browser_renders_again_is_not_delivered() {
    let url = "http://forum-thirteen.test/r/rust/top.json";
    let site = Site::start(vec![(
        url,
        json(r#"{"message": "Forbidden", "error": 403}"#).with_status(403),
    )])
    .await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            max_tier: "browser".into(),
            request_min_interval_ms: 1,
            http_engine: "reqwest".into(),
            ..Default::default()
        },
        None,
    );
    assert!(server.pool().available(), "manual test requires a browser");
    let out = server.fetch_json(remote(url, &site)).await.value;
    server.shutdown_configuration().await;
    println!("{out}");
    assert!(
        attempts(&out).iter().any(|a| a.starts_with("browser")),
        "the browser tier was reached: {out}"
    );
    assert!(out["blocked_reason"].is_string(), "{out}");
}

// ---- 13b: wall phrases in other languages -----------------------------------------------------

/// The marketplace interstitial in Spanish. In English it was already a wall; in Spanish it came
/// back as the product page.
#[tokio::test]
async fn the_marketplace_interstitial_in_spanish_is_a_wall() {
    let page = "<!doctype html><html><head><title>Tienda</title></head><body><div>\
        <h4>Haz clic en el botón de abajo para continuar comprando</h4>\
        <button>Seguir comprando</button></div></body></html>";
    let site = Site::start(vec![("/dp/B0TEST", Reply::html(page))]).await;
    let out = local_server()
        .fetch_json(local(site.url("/dp/B0TEST")))
        .await
        .value;
    assert!(out["blocked_reason"].is_string(), "{out}");
    assert_eq!(out["wall_kind"], "generic", "{out}");
}

#[tokio::test]
async fn human_checks_in_portuguese_french_and_german_are_walls() {
    let pages = [
        ("/pt", "Confirme que você é humano para continuar."),
        (
            "/fr",
            "Nous avons détecté un trafic inhabituel depuis votre réseau.",
        ),
        ("/de", "Bitte bestätigen Sie, dass Sie ein Mensch sind."),
    ];
    let site = Site::start(
        pages
            .iter()
            .map(|(p, text)| {
                (
                    *p,
                    Reply::html(&format!(
                        "<!doctype html><html><head><title>Check</title></head>\
                         <body><p>{text}</p></body></html>"
                    )),
                )
            })
            .collect(),
    )
    .await;
    let server = local_server();
    for (path, _) in pages {
        let out = server.fetch_json(local(site.url(path))).await.value;
        assert!(out["blocked_reason"].is_string(), "{path}: {out}");
    }
}

// ---- 16: quality by content kind --------------------------------------------------------------

/// A complete JSON object is a complete answer. Judged as prose it is "thin": too few words, the
/// wrong shapes, too many symbols. And because only a `full` page is a useful one to the route
/// memory, the tier that served it was never learned.
#[tokio::test]
async fn a_complete_json_answer_is_full_and_teaches_the_route() {
    let url = "http://api-sixteen.test/v1/users/42";
    let site = Site::start(vec![(
        url,
        json(r#"{"id":42,"login":"ada","admin":false,"repos":[1,2,3]}"#),
    )])
    .await;
    let out = remote_server().fetch_json(remote(url, &site)).await.value;
    assert_eq!(out["quality"], "full", "{out}");

    let stored: std::collections::HashMap<String, String> = serde_json::from_str(
        &std::fs::read_to_string(svipall_core::config::home_dir().join("automatic_routes.json"))
            .unwrap_or_else(|_| "{}".into()),
    )
    .unwrap();
    let domain_key = {
        use sha2::Digest;
        format!("{:x}:", sha2::Sha256::digest(b"api-sixteen.test"))
    };
    assert!(
        stored.iter().any(|(k, rows)| k.starts_with(&domain_key)
            && serde_json::from_str::<Vec<svipall_core::automatic::Sample>>(rows)
                .unwrap()
                .iter()
                .any(|r| r.tier == "http" && r.successes > 0.0)),
        "a good JSON answer is useful evidence for its tier: {stored:?}"
    );
}

#[tokio::test]
async fn a_cut_off_json_answer_is_partial_and_an_empty_one_is_thin() {
    let site = Site::start(vec![
        ("/cut", json(r#"{"items":[{"id":1},{"id":2},{"id":"#)),
        ("/empty", json("[]")),
        (
            "/script.js",
            Reply::bytes(
                b"export const x = 1;\nexport function f(a) { return a * x; }\n",
                "text/javascript",
            ),
        ),
    ])
    .await;
    let server = local_server();
    let cut = server.fetch_json(local(site.url("/cut"))).await.value;
    assert_eq!(cut["quality"], "partial", "{cut}");
    assert!(cut["blocked_reason"].is_null(), "{cut}");
    let empty = server.fetch_json(local(site.url("/empty"))).await.value;
    assert_eq!(empty["quality"], "thin", "{empty}");
    let code = server.fetch_json(local(site.url("/script.js"))).await.value;
    assert_eq!(
        code["quality"], "full",
        "source code is not judged as prose: {code}"
    );
}

// ---- 17a: tables ------------------------------------------------------------------------------

const REPO_PAGE: &str = include_str!("../../svipall-extract/tests/fixtures/repo-file-listing.html");

/// The file listing of a real repository page. The site renders each name twice (a cell for
/// narrow screens and one for wide ones) under a header that spans both, and leaves the commit
/// columns as empty placeholders until its scripts run. It came out as "Name | Name | Name" over
/// rows of repeated names and empty cells.
#[tokio::test]
async fn a_repository_file_listing_comes_out_as_one_clean_column_of_names() {
    let site = Site::start(vec![("/serde-rs/serde", Reply::html(REPO_PAGE))]).await;
    let out = local_server()
        .fetch_json(local(site.url("/serde-rs/serde")))
        .await
        .value;
    let md = content(&out);
    assert!(!md.contains("Name | Name"), "{md}");
    assert!(
        md.lines()
            .filter(|l| l.starts_with('|'))
            .all(|l| !l.trim_matches(|c| c == '|' || c == ' ').is_empty()),
        "a row of empty cells is not a row: {md}"
    );
    let row = md
        .lines()
        .find(|l| l.contains("[.github]"))
        .unwrap_or_else(|| panic!("the first entry is missing: {md}"));
    assert_eq!(
        row.matches(".github]").count(),
        1,
        "each name once, not once per screen size: {row}"
    );
    for name in ["serde_derive", "Cargo.toml", "README.md"] {
        assert!(md.contains(name), "{name} is missing: {md}");
    }

    let rows = local_server()
        .fetch_json(WebFetchParams {
            tables: Some(true),
            ..local(site.url("/serde-rs/serde"))
        })
        .await
        .value;
    let header = &rows["tables"][0]["header"];
    assert_eq!(header, &serde_json::json!(["Name"]), "{rows}");
}

// ---- 18: local causes only where they can matter ----------------------------------------------

/// A page that does not exist is not something a security product on this machine could have
/// caused, and neither is a refusal at the http tier, where no browser ran. Saying so anyway sends
/// the caller to reconfigure their antivirus over a typo.
#[tokio::test]
async fn a_missing_page_is_not_blamed_on_this_machine() {
    let injected = "<!doctype html><html><head>\
        <script src=\"https://gc.kaspersky-labs.com/x/main.js\"></script></head>\
        <body><h1>Not found</h1></body></html>";
    let site = Site::start(vec![
        ("/gone", Reply::html(injected).with_status(404)),
        ("/refused", Reply::html(injected).with_status(403)),
    ])
    .await;
    let server = local_server();
    for path in ["/gone", "/refused"] {
        let out = server.fetch_json(local(site.url(path))).await.value;
        let note = out["note"].as_str().unwrap_or_default();
        assert!(out["blocked_reason"].is_string(), "{out}");
        assert!(
            !note.contains("security product"),
            "{path}: the note blames this machine for an outcome it cannot have caused: {note}"
        );
    }
}

// ---- 21: query with punctuation ---------------------------------------------------------------

fn cli_reference() -> String {
    let mut rows = String::new();
    for (flag, what) in [
        ("--model", "Choose the model for this session."),
        ("--print", "Print the response and exit."),
        ("--output-format", "Choose text, json or stream-json."),
        ("--verbose", "Show every step."),
        ("--max-turns", "Stop after this many turns."),
        ("--effort", "How hard to think: low, medium, high or max."),
        ("--resume", "Resume a previous session by id."),
    ] {
        rows.push_str(&format!(
            "<tr><td><code>{flag}</code></td><td>{what}</td></tr>"
        ));
    }
    format!(
        "<!doctype html><html><head><title>CLI reference</title></head><body><main>\
         <h1>CLI reference</h1>\
         <p>This page lists every flag the command accepts. It is long, so the start of it is a \
         general description of how flags are parsed, which is not what anyone looking for one \
         flag wants to read. Flags may be combined; later ones win over earlier ones; an unknown \
         flag is an error. Spending effort on reading this introduction is spending it on the \
         wrong thing.</p>\
         <p>Configuration files are read before flags, so a flag always overrides the file. \
         Environment variables sit between the two.</p>\
         <h2>Flags</h2><table><thead><tr><th>Flag</th><th>Description</th></tr></thead>\
         <tbody>{rows}</tbody></table>\
         <h2>Types</h2><p>The <code>max_turns</code> setting is an <code>Option&lt;usize&gt;</code>: \
         absent means no limit.</p>\
         <p>Paths are written as <code>src/cli/args.rs</code> in the examples.</p>\
         </main></body></html>"
    )
}

#[tokio::test]
async fn a_query_that_is_a_flag_finds_the_flag_and_not_the_page_header() {
    let site = Site::start(vec![("/cli", Reply::html(&cli_reference()))]).await;
    let server = local_server();
    let out = server
        .fetch_json(WebFetchParams {
            query: Some("--effort".into()),
            max_tokens: Some(60),
            ..local(site.url("/cli"))
        })
        .await
        .value;
    assert!(
        content(&out).contains("How hard to think"),
        "the flag's row is what was asked for: {out}"
    );
    for (q, want) in [
        ("Option<usize>", "absent means no limit"),
        ("src/cli/args.rs", "in the examples"),
        ("max_turns", "absent means no limit"),
    ] {
        let out = server
            .fetch_json(WebFetchParams {
                query: Some(q.into()),
                max_tokens: Some(60),
                ..local(site.url("/cli"))
            })
            .await
            .value;
        assert!(content(&out).contains(want), "{q}: {out}");
    }
}

// ---- 22: cursors ------------------------------------------------------------------------------

fn long_article() -> String {
    // Half the paragraphs are about one place and half about the other, so two queries are two
    // different views of the one page.
    let paras: String = (0..60)
        .map(|i| {
            let place = if i % 2 == 0 {
                "alpha quay"
            } else {
                "beta pier"
            };
            format!(
                "<p>Paragraph {i} about the harbour and the {place}, written out at length so that \
                 the page needs several calls to read.</p>"
            )
        })
        .collect();
    format!(
        "<!doctype html><html><head><title>Harbour</title></head><body><main><h1>Harbour</h1>\
         {paras}</main></body></html>"
    )
}

/// The measured failure: a cursor five seconds old, a cache hit, a different `query` on the
/// continuation, and the answer was "the page changed since that cursor was issued". The page had
/// not changed. A cursor continues the view it was issued for.
#[tokio::test]
async fn a_cursor_continues_on_a_cached_page_even_when_the_query_changes() {
    let site = Site::start(vec![("/harbour", Reply::html(&long_article()))]).await;
    let server = local_server();
    let base = WebFetchParams {
        cache: Some("auto".into()),
        max_tokens: Some(120),
        ..local(site.url("/harbour"))
    };
    let first = server
        .fetch_json(WebFetchParams {
            query: Some("alpha quay".into()),
            ..base.clone()
        })
        .await
        .value;
    assert_eq!(first["truncated"], true, "{first}");
    let cursor = first["cursor"].as_str().expect("a cursor").to_string();
    let hint = first["continue"].as_str().unwrap_or_default();
    assert!(
        hint.contains(&cursor),
        "the hint names the exact next call: {hint}"
    );

    for query in [Some("beta pier"), None, Some("alpha quay")] {
        let next = server
            .fetch_json(WebFetchParams {
                query: query.map(str::to_string),
                cursor: Some(cursor.clone()),
                ..base.clone()
            })
            .await
            .value;
        assert_eq!(next["from_cache"], true, "{next}");
        assert!(next["stale_cursor"].is_null(), "{query:?}: {next}");
        let text = content(&next);
        assert!(
            !text.contains("Paragraph 0 "),
            "{query:?}: the continuation restarted from the top: {next}"
        );
        assert!(
            text.contains("alpha quay") && !text.contains("beta pier"),
            "{query:?}: the continuation is the view the cursor was issued for: {next}"
        );
    }
    assert_eq!(
        site.hits("/harbour"),
        1,
        "every continuation came from the cache"
    );
}

// ---- 23: PDF spacing --------------------------------------------------------------------------

const RANGE_WIDTHS_PDF: &[u8] =
    include_bytes!("../../svipall-core/fixtures/pdf/cid-range-widths.pdf");

#[tokio::test]
async fn a_pdf_whose_glyphs_are_placed_one_by_one_reads_as_words() {
    let site = Site::start(vec![(
        "/sample.pdf",
        Reply::bytes(RANGE_WIDTHS_PDF, "application/pdf"),
    )])
    .await;
    let out = local_server()
        .fetch_json(local(site.url("/sample.pdf")))
        .await
        .value;
    assert!(
        content(&out).contains("The sample contains parts of Chapter 3"),
        "{out}"
    );
}

// ---- 37: this machine's address --------------------------------------------------------------

/// A wall that prints the visitor's address puts this machine's public IP into the transcript. It
/// is replaced by a label, the page is otherwise returned whole, and once learned the address is
/// masked on any later page that echoes it.
#[tokio::test]
async fn this_machines_address_is_masked_where_a_page_echoes_it() {
    let wall = "<!doctype html><html><head><title>Access denied</title></head><body>\
        <h1>Access denied</h1><p>Your IP address: 198.51.100.23</p>\
        <p>Ray ID: 8c1f2a</p></body></html>";
    let site = Site::start(vec![
        ("/blocked", Reply::html(wall).with_status(403)),
        (
            "/echo",
            json(r#"{"origin":"198.51.100.23","other":"192.0.2.9"}"#),
        ),
    ])
    .await;
    let server = local_server();
    let blocked = server.fetch_json(local(site.url("/blocked"))).await.value;
    assert!(!blocked.to_string().contains("198.51.100.23"), "{blocked}");
    assert!(content(&blocked).contains("Ray ID"), "{blocked}");
    assert_eq!(blocked["own_ip_masked"], 1, "{blocked}");

    let echo = server.fetch_json(local(site.url("/echo"))).await.value;
    assert!(!echo.to_string().contains("198.51.100.23"), "{echo}");
    assert!(
        content(&echo).contains("192.0.2.9"),
        "another address is not this machine's: {echo}"
    );
}
