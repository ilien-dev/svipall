use svipall_core::Config;

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
