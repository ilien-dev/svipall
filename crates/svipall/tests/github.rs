//! A code-hosting URL read through its API instead of its page.
//!
//! The page of a repository, an issue or a pull request is a script-built application: read as
//! HTML it came back as empty tables, and agents fell back to a command-line client about fifty
//! times a month to read what the API serves in one call. These drive the real `web_fetch` and
//! `web_fetch_many` handlers against answers captured from the real API (see `github_replay`).
mod github_replay;
mod support;

use github_replay::{fixture_text, replay, ISSUE_PAGE};
use serde_json::{json, Value};
use svipall::server::SvipallServer;

fn server() -> SvipallServer {
    support::isolate();
    replay();
    SvipallServer::new(None, svipall_core::Config::default(), None)
}

async fn fetch(params: Value) -> Value {
    server()
        .fetch_json(serde_json::from_value(params).expect("params"))
        .await
        .value
}

fn content(v: &Value) -> &str {
    v["content"]
        .as_str()
        .unwrap_or_else(|| panic!("no content: {v}"))
}

#[tokio::test]
async fn a_repository_root_reads_as_facts_tree_and_readme() {
    let r = replay();
    let v = fetch(json!({"url": r.url("/dtolnay/anyhow")})).await;
    assert_eq!(v["via"], "github-api", "{v}");
    let c = content(&v);
    assert!(
        c.contains("Flexible concrete Error type built on std::error::Error"),
        "description: {c}"
    );
    for fact in ["6710", "Apache-2.0", "master", "2026-11-03"] {
        assert!(c.contains(fact), "missing fact {fact}: {c}");
    }
    for entry in ["Cargo.toml", "src/", "tests/", "build.rs"] {
        assert!(c.contains(entry), "tree entry {entry} missing: {c}");
    }
    assert!(c.contains("## README"), "{c}");
    assert!(
        c.contains("This library provides [`anyhow::Error`]"),
        "the README is there as markdown: {c}"
    );
    // Rate limit, as the API reported it on its last answer.
    assert_eq!(v["rate_limit"]["limit"], 60, "{v}");
    assert!(v["rate_limit"]["remaining"].is_u64(), "{v}");
    assert!(v["rate_limit"]["reset_at"].is_string(), "{v}");
    assert_eq!(v["status"], 200);
    assert!(v.get("blocked_reason").is_none(), "{v}");
}

#[tokio::test]
async fn a_tree_url_is_a_directory_listing() {
    let r = replay();
    let v = fetch(json!({"url": r.url("/dtolnay/anyhow/tree/master/src")})).await;
    assert_eq!(v["via"], "github-api", "{v}");
    let c = content(&v);
    for f in ["backtrace.rs", "error.rs", "lib.rs", "wrapper.rs"] {
        assert!(c.contains(f), "{f} missing from listing: {c}");
    }
}

#[tokio::test]
async fn a_blob_url_is_the_raw_file_and_honours_a_line_anchor() {
    let r = replay();
    let raw = fixture_text("raw_lib");

    let whole = fetch(json!({"url": r.url("/dtolnay/anyhow/blob/master/src/lib.rs")})).await;
    assert_eq!(whole["via"], "github-raw", "{whole}");
    let got = content(&whole).trim_end();
    let differs = got
        .chars()
        .zip(raw.chars())
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(raw.len()));
    assert!(
        got == raw.trim_end(),
        "the whole file, as it is: {} chars of {}, first difference at {differs}: {:?}",
        got.len(),
        raw.trim_end().len(),
        got.get(differs.saturating_sub(40)..(differs + 80).min(got.len()))
    );

    let part = fetch(json!({"url": r.url("/dtolnay/anyhow/blob/master/src/lib.rs#L10-L40")})).await;
    assert_eq!(part["via"], "github-raw", "{part}");
    let want: Vec<&str> = raw.lines().skip(9).take(31).collect();
    assert_eq!(content(&part).trim_end(), want.join("\n").trim_end());
    assert_eq!(part["lines"]["from"], 10);
    assert_eq!(part["lines"]["to"], 40);
    assert_eq!(part["lines"]["of"], raw.lines().count());
}

#[tokio::test]
async fn an_issue_is_its_thread_in_order_with_reactions() {
    let r = replay();
    let v = fetch(json!({"url": r.url("/dtolnay/anyhow/issues/323")})).await;
    assert_eq!(v["via"], "github-api", "{v}");
    assert!(
        v["title"]
            .as_str()
            .unwrap()
            .contains("`build.rs` should not detect nightly feature"),
        "{v}"
    );
    let c = content(&v);
    assert!(c.contains("closed"), "state: {c}");
    // Twelve comments, oldest first, each with its author and date.
    let order = [
        ("loynoir", "2023-10-25"),
        ("epage", "2023-10-25"),
        ("dzmitry-lahoda", "2024-01-09"),
        ("spectria-limina", "2024-02-09"),
        ("RalfJung", "2024-03-26"),
        ("hkBst", "2024-12-23"),
    ];
    let mut at = 0;
    for (who, when) in order {
        let i = c[at..]
            .find(&format!("@{who}"))
            .unwrap_or_else(|| panic!("{who} missing or out of order: {c}"));
        at += i;
        assert!(c[at..].contains(when), "{who} without date {when}");
    }
    assert!(c.contains("+1 5"), "RalfJung's five thumbs: {c}");
    assert_eq!(v["github"]["comments"]["fetched"], 12, "{v}");
    assert_eq!(v["github"]["comments"]["total"], 12, "{v}");
}

#[tokio::test]
async fn a_pull_request_adds_branches_reviews_and_changed_files() {
    let r = replay();
    let v = fetch(json!({"url": r.url("/dtolnay/anyhow/pull/251")})).await;
    assert_eq!(v["via"], "github-api", "{v}");
    let c = content(&v);
    assert!(c.contains("work around RAs RUSTC_WRAPPER hack"), "{c}");
    assert!(c.contains("RalfJung:rustc-wrapper"), "head branch: {c}");
    assert!(c.contains("build.rs"), "changed file: {c}");
    assert!(c.contains("+8"), "additions: {c}");
    assert!(c.contains("CHANGES_REQUESTED"), "the review: {c}");
    assert!(c.contains("@jonhoo"), "a comment: {c}");
}

#[tokio::test]
async fn releases_read_as_a_list_and_one_by_tag() {
    let r = replay();
    let one = fetch(json!({"url": r.url("/dtolnay/anyhow/releases/tag/1.0.104")})).await;
    assert_eq!(one["via"], "github-api", "{one}");
    let c = content(&one);
    assert!(c.contains("1.0.104"), "{c}");
    assert!(
        c.contains("Update `syn` dev-dependency to version 3"),
        "{c}"
    );

    let list = fetch(json!({"url": r.url("/dtolnay/anyhow/releases")})).await;
    assert_eq!(list["via"], "github-api", "{list}");
    let c = content(&list);
    for tag in ["1.0.104", "1.0.100", "1.0.95"] {
        assert!(c.contains(tag), "{tag} missing: {c}");
    }
}

#[tokio::test]
async fn a_commit_reads_as_message_stats_and_diff() {
    let r = replay();
    let v = fetch(json!({"url": r.url(
        "/dtolnay/anyhow/commit/87c8571326d60b90391d0555c9675b5ac6f044b1"
    )}))
    .await;
    assert_eq!(v["via"], "github-api", "{v}");
    let c = content(&v);
    assert!(
        c.contains("Update ui test suite to nightly-2026-11-03"),
        "{c}"
    );
    assert!(c.contains("tests/ui/must-use.stderr"), "{c}");
    assert!(c.contains("David Tolnay"), "{c}");
}

#[tokio::test]
async fn a_rate_limited_api_says_until_when_and_reads_the_page_instead() {
    let r = replay();
    let v = fetch(json!({"url": r.url("/limited/repo/issues/7"), "max_tier": "http"})).await;
    // The page answered, and it says so.
    assert_eq!(v["via"], "page", "{v}");
    assert!(
        content(&v).contains("Issue seven, read from its page"),
        "{v}"
    );
    // Not a wall: the page itself was fine.
    assert!(v.get("blocked_reason").is_none(), "{v}");
    let api = &v["github_api"];
    assert_eq!(api["rate_limited_until"], "2026-11-11T21:43:54Z", "{v}");
    assert_eq!(api["remaining"], 0, "{v}");
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("rate limited until 2026-11-11T21:43:54Z"),
        "{v}"
    );
    // And no cooldown was started on anything.
    assert!(svipall_core::check_cooldown("127.0.0.1").is_none());
    let _ = ISSUE_PAGE;
}

#[tokio::test]
async fn an_explicit_mode_reads_the_plain_page() {
    let r = replay();
    let v = fetch(json!({"url": r.url("/plain/repo/issues/8"), "mode": "http"})).await;
    assert_eq!(v["via"], "page", "{v}");
    assert!(
        content(&v).contains("Issue seven, read from its page"),
        "{v}"
    );
    assert_eq!(r.hits("/repos/plain/"), 0, "the API was not asked");
}

#[tokio::test]
async fn a_long_thread_continues_by_cursor_without_asking_the_api_again() {
    let r = replay();
    let url = r.url("/dtolnay/anyhow/issues/323");
    let server = server();
    let first = server
        .fetch_json(
            serde_json::from_value(json!({"url": url, "max_tokens": 600, "cache": "refresh"}))
                .unwrap(),
        )
        .await
        .value;
    assert_eq!(first["via"], "github-api", "{first}");
    assert_eq!(first["truncated"], true, "{first}");
    let cursor = first["cursor"].as_str().expect("a cursor").to_string();

    let second = server
        .fetch_json(
            serde_json::from_value(
                json!({"url": url, "max_tokens": 600, "cursor": cursor, "cache": "auto"}),
            )
            .unwrap(),
        )
        .await
        .value;
    assert_eq!(second["via"], "github-api", "{second}");
    // Other tests read the same issue concurrently, so the proof is the cache's own word rather
    // than a request count.
    assert_eq!(
        second["from_cache"], true,
        "the continuation spent no API request: {second}"
    );
    assert!(second.get("stale_cursor").is_none(), "{second}");
    assert_ne!(content(&second), content(&first), "the next part");
}

#[tokio::test]
async fn web_fetch_many_takes_the_same_path() {
    let r = replay();
    let out = server()
        .fetch_many_json(
            serde_json::from_value(json!({"urls": [
                r.url("/dtolnay/anyhow/tree/master/src"),
                r.url("/dtolnay/anyhow/releases/tag/1.0.104"),
            ]}))
            .unwrap(),
        )
        .await;
    let results = out["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    for v in results {
        assert_eq!(v["via"], "github-api", "{v}");
    }
}

#[tokio::test]
async fn what_does_not_map_is_fetched_as_it_always_was() {
    let r = replay();
    // An owner's page has no API shape here.
    let owner = fetch(json!({"url": r.url("/dtolnay"), "max_tier": "http"})).await;
    assert!(owner.get("via").is_none(), "{owner}");
}

#[tokio::test]
async fn an_api_url_named_directly_comes_back_as_it_is() {
    let r = replay();
    let api = fetch(json!({"url": r.api_url("/repos/dtolnay/anyhow/issues/323")})).await;
    assert_eq!(api["via"], "github-api", "{api}");
    assert_eq!(api["status"], 200);
    assert_eq!(
        content(&api),
        fixture_text("issue_323"),
        "the JSON byte for byte"
    );
    assert_eq!(api["rate_limit"]["limit"], 60, "{api}");
}

#[tokio::test]
async fn a_direct_api_quota_refusal_is_not_a_wall() {
    let r = replay();
    let v = fetch(json!({"url": r.api_url("/repos/limited/repo/issues/7")})).await;
    assert_eq!(v["via"], "github-api", "{v}");
    assert_eq!(v["status"], 403, "{v}");
    assert_eq!(v["rate_limited_until"], "2026-11-11T21:43:54Z", "{v}");
    assert!(v.get("blocked_reason").is_none(), "{v}");
    assert!(v.get("wall_kind").is_none(), "{v}");
    let note = v["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("rate limited until 2026-11-11T21:43:54Z"),
        "{v}"
    );
    assert!(note.contains("10 requests of the search kind"), "{v}");
    assert!(
        content(&v).contains("API rate limit exceeded"),
        "the API's own words: {v}"
    );
    // One request, no ladder climbed.
    assert_eq!(v["attempts"].as_array().map(Vec::len), Some(1), "{v}");
    assert!(svipall_core::check_cooldown("127.0.0.1").is_none());
}
