//! The data lane in the visit ledger: a window of its own, a larger one, and the same holds.

use svipall_core::lane::Lane;
use svipall_core::traffic::{Admission, Backoff, Ledger};
use svipall_core::Config;

fn ledger() -> (Ledger, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("svipall-lane-{}.sqlite3", uuid::Uuid::new_v4()));
    (Ledger::open(&path).unwrap(), path)
}

fn cfg() -> Config {
    Config {
        request_limit: 2,
        request_window_seconds: 60,
        request_cooldown_seconds: 900,
        ..Default::default()
    }
}

#[test]
fn data_has_a_larger_window_and_does_not_spend_the_pages_one() {
    let (ledger, path) = ledger();
    let cfg = cfg();
    let limit = Lane::Data.limit(&cfg);
    assert!(limit > cfg.request_limit);
    for i in 0..limit {
        assert_eq!(
            ledger
                .reserve_in("api.test", None, Lane::Data, &cfg, 100)
                .unwrap(),
            Admission::Granted {
                visits_left: limit - i - 1
            },
            "data visit {i}"
        );
    }
    assert_eq!(
        ledger
            .reserve_in("api.test", None, Lane::Data, &cfg, 101)
            .unwrap(),
        Admission::Full { wait: 59 }
    );
    // The pages of the same host still have every visit of theirs.
    assert_eq!(
        ledger.reserve("api.test", None, &cfg, 101).unwrap(),
        Admission::Granted { visits_left: 1 }
    );
    assert_eq!(
        ledger
            .reserve_in("api.test", None, Lane::Page, &cfg, 101)
            .unwrap(),
        Admission::Granted { visits_left: 0 },
        "`reserve` is the page lane"
    );
    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_servers_backoff_holds_both_lanes_and_a_clear_empties_both() {
    let (ledger, path) = ledger();
    let cfg = cfg();
    assert!(ledger
        .reserve_in("held.test", None, Lane::Data, &cfg, 100)
        .unwrap()
        .is_granted());
    assert_eq!(
        ledger
            .backoff("held.test", None, Backoff::RetryAfter(40), &cfg, 100)
            .unwrap(),
        40
    );
    for lane in [Lane::Data, Lane::Page] {
        assert_eq!(
            ledger
                .reserve_in("held.test", None, lane, &cfg, 110)
                .unwrap(),
            Admission::Held { wait: 30 },
            "{lane:?}"
        );
    }
    let cleared = ledger.clear("held.test").unwrap();
    assert_eq!((cleared.holds, cleared.visits), (1, 1));
    assert!(ledger
        .reserve_in("held.test", None, Lane::Data, &cfg, 111)
        .unwrap()
        .is_granted());
    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_data_routes_strikes_start_short_and_still_grow() {
    let (ledger, path) = ledger();
    let cfg = cfg();
    let waits: Vec<u64> = (0..4)
        .map(|i| {
            ledger
                .backoff("struck.test", None, Backoff::DataRateLimited, &cfg, 100 + i)
                .unwrap()
        })
        .collect();
    assert_eq!(waits, [5, 20, 80, 320]);
    // A page's first strike is the longer one it always was.
    assert_eq!(
        ledger
            .backoff("page-struck.test", None, Backoff::RateLimited, &cfg, 100)
            .unwrap(),
        30
    );
    drop(ledger);
    std::fs::remove_file(path).unwrap();
}
