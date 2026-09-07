use svipall_core::Config;

#[test]
fn remembered_fingerprint_walls_do_not_restart_at_untried_weaker_routes() {
    use svipall_core::automatic::{plan, Feedback, Sample};
    let tiers = ["http", "browser", "stealth", "real", "warm"].map(String::from);
    let mut row: Sample = serde_json::from_value(serde_json::json!({
        "tier": "http", "successes": 0.0, "failures": 0.0,
        "latency_ms": 10.0, "updated": 100
    }))
    .unwrap();
    row.observe(Feedback::FingerprintWall, 10, 101);
    let rows: Vec<Sample> =
        serde_json::from_str(&serde_json::to_string(&vec![row.clone()]).unwrap()).unwrap();
    assert_eq!(
        plan(&tiers, &rows, 102, true),
        ["real", "warm", "native:warm"]
    );
    assert_eq!(plan(&tiers, &rows, 102, false), ["real", "warm"]);
    // Wall evidence expires independently of generic failure counts.
    assert_eq!(plan(&tiers, &rows, 1901, false), tiers);
    // A lower ceiling still gets its permitted emulated browser probe.
    assert_eq!(
        plan(&tiers[..2], &rows, 102, true),
        ["http", "browser", "native:browser"]
    );
    // A delivered response on the same route clears the classified wall evidence.
    row.observe(Feedback::Delivered, 10, 103);
    assert_eq!(plan(&tiers, &[row], 104, false), tiers);

    let rows: Vec<Sample> = ["http", "real", "warm"]
        .into_iter()
        .map(|tier| {
            serde_json::from_value(serde_json::json!({
                "tier": tier, "successes": 0.0, "failures": 3.0,
                "latency_ms": 100.0, "updated": 100, "fingerprint_wall": 100
            }))
            .unwrap()
        })
        .collect();
    assert_eq!(plan(&tiers, &rows, 101, true), ["warm", "native:warm"]);
    assert_eq!(plan(&tiers, &rows, 101, false), ["warm"]);
}

#[test]
fn generic_errors_and_native_walls_do_not_predict_emulated_fingerprint_failures() {
    use svipall_core::automatic::{plan, Feedback, Sample};
    let tiers = ["http", "browser", "stealth", "real", "warm"].map(String::from);
    for (tier, feedback) in [
        ("http", Feedback::Failed),
        ("native:warm", Feedback::FingerprintWall),
    ] {
        let mut row: Sample = serde_json::from_value(serde_json::json!({
            "tier": tier, "successes": 0.0, "failures": 0.0,
            "latency_ms": 10.0, "updated": 100
        }))
        .unwrap();
        row.observe(feedback, 10, 101);
        assert_eq!(plan(&tiers, &[row], 102, false), tiers);
    }
}

#[test]
fn a_fingerprint_wall_skips_weak_routes_without_moving_native_ahead_of_a_browser() {
    use svipall_core::automatic::after_wall;
    use svipall_core::WallKind;
    let routes = ["http", "browser", "stealth", "real", "warm", "native:warm"].map(String::from);
    assert_eq!(after_wall(&routes, 0, &WallKind::Vendor, false), 3);
    assert_eq!(after_wall(&routes, 3, &WallKind::Vendor, false), 4);
    assert_eq!(after_wall(&routes, 4, &WallKind::Vendor, false), 5);
    assert_eq!(after_wall(&routes, 0, &WallKind::Hold, false), 3);
    // Generic walls still explore the cheaper browser: the classification is not evidence
    // that the page needs a persistent browser session.
    assert_eq!(after_wall(&routes, 0, &WallKind::Generic, false), 1);
    // A caller's lower tier ceiling must not turn the heuristic into native-first routing.
    let limited = ["http", "browser", "native:browser"].map(String::from);
    assert_eq!(after_wall(&limited, 0, &WallKind::Vendor, false), 1);
    assert_eq!(after_wall(&limited, 1, &WallKind::Vendor, false), 2);
}

#[test]
fn a_failed_promoted_browser_does_not_backtrack_through_weaker_fingerprint_routes() {
    use svipall_core::automatic::after_wall;
    let routes = ["real", "http", "browser", "stealth", "warm", "native:warm"].map(String::from);
    assert_eq!(
        after_wall(&routes, 0, &svipall_core::WallKind::Vendor, false),
        4
    );
    let warm = ["warm", "http", "browser", "real", "native:warm"].map(String::from);
    assert_eq!(
        after_wall(&warm, 0, &svipall_core::WallKind::Vendor, false),
        4
    );
    assert_eq!(
        after_wall(&warm[..4], 0, &svipall_core::WallKind::Vendor, false),
        4
    );
}

#[test]
fn a_managed_challenge_uses_headful_evidence_without_generalizing_to_interstitials() {
    use svipall_core::automatic::after_wall;
    use svipall_core::WallKind;
    let routes = ["http", "browser", "stealth", "real", "warm", "native:warm"].map(String::from);
    assert_eq!(after_wall(&routes, 0, &WallKind::Cloudflare, true), 3);
    assert_eq!(after_wall(&routes, 3, &WallKind::Cloudflare, true), 4);
    assert_eq!(after_wall(&routes, 4, &WallKind::Cloudflare, true), 5);
    assert_eq!(after_wall(&routes, 0, &WallKind::Cloudflare, false), 1);
    assert_eq!(after_wall(&routes, 0, &WallKind::Generic, true), 1);
    let limited = ["http", "browser", "native:browser"].map(String::from);
    assert_eq!(after_wall(&limited, 0, &WallKind::Cloudflare, true), 1);
    let promoted = ["real", "http", "browser", "warm", "native:warm"].map(String::from);
    assert_eq!(after_wall(&promoted, 0, &WallKind::Cloudflare, true), 3);
}

#[test]
fn automatic_identity_is_the_valid_default() {
    let cfg = Config::default();
    assert_eq!(cfg.browser_identity, "auto");
    cfg.validate().unwrap();
}

#[test]
fn short_delivered_pages_clear_failures_without_being_promoted_as_full() {
    use svipall_core::automatic::{Feedback, Sample};
    let mut row = Sample {
        tier: "http".into(),
        successes: 0.0,
        failures: 3.0,
        latency_ms: 1.0,
        updated: 100,
        fingerprint_wall: None,
    };
    row.observe(Feedback::Delivered, 10, 101);
    assert_eq!(row.failures, 0.0);
    assert_eq!(row.successes, 0.0);
}

#[test]
fn learned_routes_never_promote_native_or_cross_contexts() {
    use svipall_core::automatic::{plan, Sample};
    let tiers = vec!["http".into(), "browser".into(), "warm".into()];
    let records = vec![Sample {
        tier: "warm".into(),
        successes: 4.0,
        failures: 0.0,
        latency_ms: 500.0,
        updated: 100,
        fingerprint_wall: None,
    }];
    assert_eq!(
        plan(&tiers, &records, 101, true),
        vec!["warm", "http", "browser", "native:warm"]
    );
    assert_eq!(
        plan(&tiers, &records, 100 + 86401, true),
        vec!["http", "browser", "warm", "native:warm"]
    );
    assert_eq!(
        plan(&tiers, &records, 101, false),
        vec!["warm", "http", "browser"]
    );
    let http = vec!["http".into()];
    assert_eq!(plan(&http, &[], 101, true), http);
}

#[test]
fn route_keys_omit_queries_and_separate_exit_and_environment() {
    use svipall_core::automatic::context;
    let a = context("https://example.test/articles/a?secret=one", None, "env1");
    assert_eq!(
        a,
        context("https://example.test/articles/b?secret=two", None, "env1")
    );
    assert_ne!(a, context("https://example.test/products/b", None, "env1"));
    assert_ne!(
        a,
        context(
            "https://example.test/articles/a",
            Some("http://user:secret@proxy.test"),
            "env1"
        )
    );
    assert_ne!(a, context("https://example.test/articles/a", None, "env2"));
    assert!(!a.contains("secret"));
}

#[test]
fn repeated_failures_skip_wasted_routes_but_keep_an_emulated_browser_probe() {
    use svipall_core::automatic::{plan, Sample};
    let tiers = vec!["http".into(), "browser".into(), "warm".into()];
    let rows: Vec<_> = ["http", "browser", "warm", "native:warm"]
        .into_iter()
        .map(|tier| Sample {
            tier: tier.into(),
            successes: 0.0,
            failures: 3.0,
            latency_ms: 100.0,
            updated: 100,
            fingerprint_wall: None,
        })
        .collect();
    assert_eq!(plan(&tiers, &rows, 101, true), vec!["warm"]);
    assert_eq!(
        plan(&tiers, &rows, 2000, true),
        vec!["http", "browser", "warm", "native:warm"]
    );
}

#[test]
fn traffic_limits_survive_reopen_and_do_not_extend_when_refused() {
    use svipall_core::traffic::Ledger;
    let path =
        std::env::temp_dir().join(format!("svipall-traffic-{}.sqlite3", uuid::Uuid::new_v4()));
    let cfg = Config {
        request_limit: 2,
        request_window_seconds: 60,
        request_cooldown_seconds: 900,
        ..Default::default()
    };
    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.pace("a.test", None, 100000, 1000).unwrap(), 0);
    assert_eq!(ledger.pace("a.test", None, 100100, 1000).unwrap(), 900);
    assert_eq!(ledger.reserve("a.test", None, &cfg, 100).unwrap(), None);
    assert_eq!(ledger.reserve("a.test", None, &cfg, 101).unwrap(), None);
    assert_eq!(
        ledger.reserve("a.test", None, &cfg, 102).unwrap(),
        Some(900)
    );
    assert_eq!(
        ledger.reserve("a.test", Some("other"), &cfg, 103).unwrap(),
        None
    );
    assert_eq!(ledger.reserve("b.test", None, &cfg, 103).unwrap(), None);
    drop(ledger);
    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.pace("a.test", None, 100200, 1000).unwrap(), 1800);
    assert_eq!(
        ledger.reserve("a.test", None, &cfg, 202).unwrap(),
        Some(800)
    );
    assert_eq!(ledger.reserve("a.test", None, &cfg, 1002).unwrap(), None);
    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn concurrent_visits_cannot_overdraw_the_window() {
    use svipall_core::traffic::Ledger;
    let path = std::env::temp_dir().join(format!("svipall-race-{}.sqlite3", uuid::Uuid::new_v4()));
    let _ = Ledger::open(&path).unwrap();
    let cfg = Config {
        request_limit: 2,
        ..Default::default()
    };
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            let cfg = cfg.clone();
            std::thread::spawn(move || {
                Ledger::open(&path)
                    .unwrap()
                    .reserve("race.test", None, &cfg, 100)
                    .unwrap()
                    .is_none()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        2
    );
    std::fs::remove_file(path).unwrap();
}
