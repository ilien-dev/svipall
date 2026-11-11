//! The visit limiter, server backoff, cooldowns and the memory of a hopeless origin, end to end.
//!
//! Every host here is a public-looking name routed through a loopback fixture, so admission runs
//! exactly as it does for a real site, and every test uses a host of its own: the ledger and the
//! pacer are shared by the whole test binary, the way they are shared by every session on a
//! machine.
mod support;
use std::time::Instant;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

fn cfg() -> svipall_core::Config {
    support::isolate();
    svipall_core::Config {
        request_min_interval_ms: 1,
        http_engine: "reqwest".into(),
        max_tier: "http".into(),
        auto_native_fallback: false,
        ..Default::default()
    }
}

/// A browser that cannot start: every browser rung fails at once, offline, so one fetch walks the
/// whole ladder in milliseconds.
fn no_browser() -> String {
    let path = support::isolate().join("not-a-browser.txt");
    std::fs::write(&path, "this is not a browser").unwrap();
    path.to_string_lossy().to_string()
}

fn params(site: &Site, url: &str) -> WebFetchParams {
    WebFetchParams {
        url: url.into(),
        proxy: Some(site.url("")),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

fn text(v: &serde_json::Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

/// Item 1: a full window is a wait for the next slot, not a quarter of an hour.
#[tokio::test]
async fn a_full_window_waits_for_the_next_slot_instead_of_holding_the_site() {
    let url = "http://slot-wait.test/article";
    let site = Site::start(vec![(url, Reply::page("Slot", &[]))]).await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 1,
            request_window_seconds: 2,
            ..cfg()
        },
        None,
    );
    let first = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(first["status"], 200, "{first}");
    let t0 = Instant::now();
    let second = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(
        second["status"], 200,
        "the second visit waited for a slot: {second}"
    );
    assert_eq!(site.hits(url), 2);
    assert!(
        t0.elapsed().as_secs() <= 4,
        "waited far longer than the window"
    );
    let waited = &second["waited"];
    assert_eq!(waited["for"], "visit_limit", "{second}");
    assert!(
        text(waited, "note").contains("shared by every session on this machine"),
        "{second}"
    );
}

/// Items 1 and 3: a slot that does not fit the deadline is refused with the exact seconds, and the
/// refusal says whose budget it is.
#[tokio::test]
async fn a_slot_beyond_the_deadline_is_refused_with_the_exact_seconds_and_no_hold() {
    let url = "http://slot-refuse.test/article";
    let site = Site::start(vec![(url, Reply::page("Refuse", &[]))]).await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 1,
            request_window_seconds: 60,
            ..cfg()
        },
        None,
    );
    assert_eq!(
        server.fetch_json(params(&site, url)).await.value["status"],
        200
    );
    let out = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["wall_kind"], "visit_limit", "{out}");
    let left = out["cooldown_seconds_left"].as_u64().unwrap();
    assert!(left > 0 && left <= 60, "the next slot, not a hold: {out}");
    let note = text(&out, "note");
    assert!(
        note.contains("shared by every session on this machine"),
        "{note}"
    );
    assert!(
        note.contains("wait="),
        "names the way to wait it out: {note}"
    );
    assert_eq!(out["visit_budget"]["left"], 0, "{out}");
    assert_eq!(out["visit_budget"]["limit"], 1, "{out}");
    assert_eq!(site.hits(url), 1);
}

/// Item 2: one fetch climbing the ladder is one visit, however many rungs it tries.
#[tokio::test]
async fn one_fetch_up_the_whole_ladder_is_charged_one_visit() {
    let url = "http://one-visit.test/article";
    let site = Site::start(vec![(url, Reply::cloudflare())]).await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 2,
            max_tier: "warm".into(),
            browser_path: no_browser(),
            ..cfg()
        },
        None,
    );
    let out = server.fetch_json(params(&site, url)).await.value;
    let attempts = out["attempts"].as_array().cloned().unwrap_or_default();
    assert!(attempts.len() >= 4, "the ladder was walked: {out}");
    assert!(
        !out.to_string().contains("visit limit") && out["stopped_reason"].is_null(),
        "the walk was cut short by its own visits: {out}"
    );
    // One of the two visits is left, so a second, forced request is still admitted.
    let mut forced = params(&site, url);
    forced.mode = Some("http".into());
    let again = server.fetch_json(forced).await.value;
    assert_eq!(again["status"], 403, "{again}");
    assert_eq!(site.hits(url), 2);
}

/// Item 4: a short Retry-After is honoured as given.
#[tokio::test]
async fn a_short_retry_after_is_honoured_as_given() {
    let url = "http://retry-short.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("slow down")
            .with_status(429)
            .header("retry-after", "2"),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let out = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["status"], 429, "{out}");
    assert_eq!(out["cooldown_seconds_left"], 2, "{out}");
    assert!(!text(&out, "note").contains("15 min"), "{out}");
}

/// Item 4: without Retry-After the first strike is short, and a 503 is a server error, not a ban.
#[tokio::test]
async fn backoff_without_retry_after_starts_short_and_a_503_is_not_a_ban() {
    let limited = "http://strike.test/article";
    let broken = "http://unavailable.test/article";
    let site = Site::start(vec![
        (limited, Reply::html("too many").with_status(429)),
        (broken, Reply::html("maintenance").with_status(503)),
    ])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let out = server.fetch_json(params(&site, limited)).await.value;
    let left = out["cooldown_seconds_left"].as_u64().unwrap_or(0);
    assert!(
        left > 0 && left <= 60,
        "a first strike is tens of seconds: {out}"
    );
    let out = server.fetch_json(params(&site, broken)).await.value;
    let left = out["cooldown_seconds_left"].as_u64().unwrap_or(0);
    assert!(left > 0 && left <= 60, "a 503 is a short pause: {out}");
    let note = text(&out, "note");
    assert!(
        !note.contains("15 min") && !note.contains("Hard HTTP block"),
        "{note}"
    );
}

/// Item 5: clear_cooldown clears the server's backoff in the shared ledger too, and says so briefly.
#[tokio::test]
async fn clear_cooldown_clears_the_ledger_hold_and_answers_briefly() {
    let url = "http://clear-hold.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("go away")
            .with_status(429)
            .header("retry-after", "3600"),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let out = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["cooldown_seconds_left"], 3600, "{out}");
    let refused = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(refused["wall_kind"], "cooldown", "{refused}");
    assert_eq!(site.hits(url), 1);

    let answer = server
        .status_json(svipall::tools::WebStatusParams {
            clear_cooldown: Some("clear-hold.test".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        answer.to_string().len() < 600,
        "an action answers with what it did, not the whole report: {answer}"
    );
    assert_eq!(
        answer["cleared_cooldown"]["domain"], "clear-hold.test",
        "{answer}"
    );
    let after = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(after["status"], 429, "the site was asked again: {after}");
    assert_eq!(site.hits(url), 2);
}

/// Item 14: a walk that failed all the way up is remembered for the origin, and the next call is
/// answered from that memory without a request.
#[tokio::test]
async fn a_hopeless_origin_is_answered_from_memory_until_a_fresh_attempt_is_asked_for() {
    let url = "http://hopeless.test/a";
    let other = "http://hopeless.test/b/c";
    let site = Site::start(vec![
        (url, Reply::cloudflare()),
        (other, Reply::cloudflare()),
    ])
    .await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            max_tier: "warm".into(),
            browser_path: no_browser(),
            ..cfg()
        },
        None,
    );
    let first = server.fetch_json(params(&site, url)).await.value;
    assert!(first["blocked_reason"].is_string(), "{first}");
    let t0 = Instant::now();
    let second = server.fetch_json(params(&site, other)).await.value;
    assert!(t0.elapsed().as_millis() < 1000, "answered from memory");
    assert_eq!(site.hits(other), 0, "nothing was requested: {second}");
    assert!(second["remembered"].is_object(), "{second}");
    assert_eq!(second["wall_kind"], first["wall_kind"], "{second}");
    assert!(text(&second, "note").contains("fresh=true"), "{second}");

    // Asked for, a fresh walk goes out again; and a higher ceiling than the one that failed is a
    // different question, never answered from the memory of a lower one.
    let mut fresh = params(&site, other);
    fresh.fresh = Some(true);
    let third = server.fetch_json(fresh).await.value;
    assert!(third["remembered"].is_null(), "{third}");
    assert_eq!(site.hits(other), 1, "{third}");

    let low = "http://hopeless-low.test/a";
    let site = Site::start(vec![(low, Reply::cloudflare())]).await;
    let mut capped = params(&site, low);
    capped.max_tier = Some("browser".into());
    server.fetch_json(capped).await;
    let mut higher = params(&site, low);
    higher.max_tier = Some("warm".into());
    let out = server.fetch_json(higher).await.value;
    assert!(out["remembered"].is_null(), "{out}");
    assert_eq!(site.hits(low), 2);

    // And clear_cooldown forgets it.
    server
        .status_json(svipall::tools::WebStatusParams {
            clear_cooldown: Some("hopeless-low.test".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let out = server.fetch_json(params(&site, low)).await.value;
    // The ladder is walked again (the route learning may now skip rungs it saw fail; that is
    // forget_tier's to reset, not this).
    assert!(out["remembered"].is_null(), "{out}");
    assert!(!out["attempts"].as_array().unwrap().is_empty(), "{out}");
}

/// Item 6: a hold longer than the default patience is waited out when the caller says so.
#[tokio::test]
async fn an_explicit_wait_outlasts_a_hold_the_default_would_refuse() {
    let url = "http://wait-param.test/article";
    let site = Site::start(vec![(url, Reply::page("Wait", &[]))]).await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 1,
            request_window_seconds: 3,
            ..cfg()
        },
        None,
    );
    let short = |wait: Option<u64>| WebFetchParams {
        timeout: Some(2_000),
        wait,
        ..params(&site, url)
    };
    assert_eq!(server.fetch_json(short(None)).await.value["status"], 200);
    // Half of a two-second timeout does not cover a three-second window: refused, not hung.
    let refused = server.fetch_json(short(None)).await.value;
    assert_eq!(refused["wall_kind"], "visit_limit", "{refused}");
    // wait=0 never waits.
    let refused = server.fetch_json(short(Some(0))).await.value;
    assert_eq!(refused["wall_kind"], "visit_limit", "{refused}");
    // wait=5 does, and the wait is on top of the timeout, not taken out of it.
    let out = server.fetch_json(short(Some(5))).await.value;
    assert_eq!(out["status"], 200, "{out}");
    assert!(out["waited"]["seconds"].as_u64().unwrap() >= 1, "{out}");
    assert_eq!(site.hits(url), 2);
}

/// Item 3: two servers, as two sessions, spend one budget, and are told so.
#[tokio::test]
async fn two_sessions_share_one_budget_and_the_refusal_says_so() {
    let url = "http://shared-budget.test/article";
    let site = Site::start(vec![(url, Reply::page("Shared", &[]))]).await;
    let config = svipall_core::Config {
        request_limit: 2,
        ..cfg()
    };
    let a = SvipallServer::new(None, config.clone(), None);
    let b = SvipallServer::new(None, config, None);
    let first = a.fetch_json(params(&site, url)).await.value;
    assert_eq!(first["status"], 200);
    let second = b.fetch_json(params(&site, url)).await.value;
    assert_eq!(
        second["visit_budget"]["left"], 0,
        "the last visit says so: {second}"
    );
    assert!(
        text(&second["visit_budget"], "scope").contains("every session"),
        "{second}"
    );
    let third = a.fetch_json(params(&site, url)).await.value;
    assert_eq!(third["wall_kind"], "visit_limit", "{third}");
    assert!(text(&third, "note").contains("shared by every session on this machine"));
}

/// Item 24: web_video meets the same limiter, and waits its short holds out instead of losing
/// the captions to a quarter-hour cooldown.
#[tokio::test]
async fn web_video_waits_for_its_slots_instead_of_losing_the_captions() {
    let host = "video-slots.test";
    let page = "http://video-slots.test/talk";
    let site = Site::start(vec![
        (
            page,
            Reply::html(
                "<html><head><title>Talk</title></head><body><h1>Talk</h1>\
                 <video controls><source src=\"/talk.mp4\" type=\"video/mp4\">\
                 <track kind=\"subtitles\" srclang=\"en\" label=\"English\" src=\"/talk-en.vtt\">\
                 </video></body></html>",
            ),
        ),
        (
            "http://video-slots.test/talk-en.vtt",
            Reply {
                content_type: "text/vtt".into(),
                ..Reply::plain("WEBVTT\n\n00:00:05.000 --> 00:00:08.000\nHello and welcome.\n")
            },
        ),
    ])
    .await;
    svipall_core::store::ROUTES.insert(host, &site.url(""));
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            request_limit: 1,
            request_window_seconds: 2,
            ..cfg()
        },
        None,
    );
    let v = server
        .video_json(svipall::tools::WebVideoParams {
            url: page.into(),
            ..Default::default()
        })
        .await
        .unwrap();
    svipall_core::store::ROUTES.remove(host);
    assert_eq!(v["captions"]["cues"], 1, "{v}");
    assert!(!v.to_string().contains("did not come"), "{v}");
}

/// Item 29: sections are there on request, and a name that is not one says so.
#[tokio::test]
async fn status_sections_come_on_request() {
    let server = SvipallServer::new(None, cfg(), None);
    let some = server
        .status_json(svipall::tools::WebStatusParams {
            sections: Some("h3, exits".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(some["http3"].is_object(), "{some}");
    assert!(some.get("proxy_routes").is_some(), "{some}");
    assert!(some.get("models").is_none(), "{some}");
    let all = server
        .status_json(svipall::tools::WebStatusParams {
            sections: Some("all".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    for key in ["models", "reputation", "solver", "identity", "cache"] {
        assert!(all.get(key).is_some(), "{key} missing: {all}");
    }
    let odd = server
        .status_json(svipall::tools::WebStatusParams {
            sections: Some("nonsense".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(odd["unknown_sections"][0], "nonsense", "{odd}");
}

/// Item 34: `domain_tiers.json` is shown only where it is read.
#[tokio::test]
async fn learned_domain_tiers_are_shown_only_where_the_ladder_reads_them() {
    svipall_core::remember_tier("tiers-shown.test", "browser");
    let tiers = |identity: &str| {
        let server = SvipallServer::new(
            None,
            svipall_core::Config {
                browser_identity: identity.into(),
                ..cfg()
            },
            None,
        );
        async move {
            server
                .status_json(svipall::tools::WebStatusParams {
                    sections: Some("all".into()),
                    ..Default::default()
                })
                .await
                .unwrap()
        }
    };
    let auto = tiers("auto").await;
    assert!(
        auto.get("domain_tiers").is_none(),
        "nothing reads it under auto: {auto}"
    );
    let emulated = tiers("emulated").await;
    assert_eq!(
        emulated["domain_tiers"]["tiers-shown.test"], "browser",
        "{emulated}"
    );
    svipall_core::forget_tier("tiers-shown.test");
}

/// Item 18: a hard block that set no cooldown does not claim one.
#[tokio::test]
async fn a_hard_block_note_claims_no_cooldown_that_was_not_set() {
    let url = "http://hard-block.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("<html><body>Forbidden</body></html>").with_status(403),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let out = server.fetch_json(params(&site, url)).await.value;
    assert!(out["blocked_reason"].is_string(), "{out}");
    let note = text(&out, "note");
    assert!(!note.contains("cooldown"), "no cooldown was set: {note}");
    assert!(
        svipall_core::check_cooldown("hard-block.test").is_none(),
        "and indeed none exists"
    );
}

/// Item 18: a page svipall does not recognise is not sold as a check a person can pass.
#[tokio::test]
async fn a_generic_wall_gets_no_login_advice() {
    let url = "http://generic-wall.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html(
            "<html><body><h1>Oops!! Something went wrong. Please refresh page</h1>\
             <button>Refresh</button><p>How doers get more done. Need Help? Visit our \
             Customer Service Center</p></body></html>",
        ),
    )])
    .await;
    let server = SvipallServer::new(None, cfg(), None);
    let out = server.fetch_json(params(&site, url)).await.value;
    assert_eq!(out["wall_kind"], "generic", "{out}");
    let note = text(&out, "note");
    assert!(!note.contains("web_login"), "{note}");
    assert!(!note.contains("cooldown"), "{note}");
}

/// Item 18, live: a hard block that survives the whole ladder puts the domain on the real
/// cooldown, and the note says that one, with its length and the way to clear it.
#[tokio::test]
#[ignore = "launches a real local browser against a loopback proxy"]
async fn a_hard_block_at_the_top_names_the_cooldown_it_set() {
    let url = "http://hard-block-top.test/article";
    let site = Site::start(vec![(
        url,
        Reply::html("<html><body>Forbidden</body></html>").with_status(403),
    )])
    .await;
    let server = SvipallServer::new(
        None,
        svipall_core::Config {
            max_tier: "warm".into(),
            ..cfg()
        },
        None,
    );
    assert!(
        server.pool().available(),
        "this manual test requires a browser"
    );
    let mut p = params(&site, url);
    p.timeout = Some(120_000);
    let out = server.fetch_json(p).await.value;
    server.shutdown_configuration().await;
    eprintln!("{out}");
    let note = text(&out, "note");
    if svipall_core::check_cooldown("hard-block-top.test").is_some() {
        assert!(note.contains("15 min cooldown"), "{note}");
        assert!(note.contains("clear_cooldown"), "{note}");
    } else {
        assert!(!note.contains("cooldown"), "{note}");
    }
}

/// Item 29: the default report is compact.
#[tokio::test]
async fn the_default_status_is_compact() {
    let server = SvipallServer::new(None, cfg(), None);
    let status = server.status_json(Default::default()).await.unwrap();
    assert!(status["version"].is_string(), "{status}");
    assert!(
        status.to_string().len() < 2500,
        "{} characters: {status}",
        status.to_string().len()
    );
    assert!(status.get("h3_offered_by").is_none(), "{status}");
}
