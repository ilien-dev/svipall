//! Several processes open the solver's database at the same moment: every session of an assistant
//! starts its own server. One that lost the race used to run its whole life with no solver, so a
//! captcha it met was answered "solver not available".
use svipall_solver::db::Db;

#[test]
fn every_opener_of_a_new_solver_database_gets_one_and_the_balance_is_written_once() {
    let dir = std::env::temp_dir().join(format!(
        "svipall-solver-open-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    for round in 0..25 {
        let path = dir.join(format!("jobs-{round}.db"));
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let openers: Vec<_> = (0..8)
            .map(|_| {
                let (path, start) = (path.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    Db::open_at(&path).map_err(|e| format!("{e:#}"))
                })
            })
            .collect();
        let mut opened = Vec::new();
        for opener in openers {
            let db = opener.join().expect("opener thread");
            assert!(
                db.is_ok(),
                "round {round}: an opener was left without a solver: {:?}",
                db.err()
            );
            opened.push(db.expect("checked"));
        }
        // Eight openers working at once on the database they share: none is told it is locked.
        let workers: Vec<_> = opened
            .into_iter()
            .map(|db| {
                std::thread::spawn(move || {
                    for _ in 0..5 {
                        db.create_job("Turnstile", Some("k"), Some("https://a.test/"), None)
                            .map_err(|e| format!("{e:#}"))?;
                    }
                    Ok::<_, String>(())
                })
            })
            .collect();
        for worker in workers {
            let worked = worker.join().expect("worker thread");
            assert!(
                worked.is_ok(),
                "round {round}: a write was refused: {worked:?}"
            );
        }
        let conn = rusqlite::Connection::open(&path).expect("raw open");
        let balances: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM balances WHERE key='balance'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(
            balances, 1,
            "round {round}: the opening balance was written {balances} times"
        );
        let jobs: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .expect("count");
        assert_eq!(jobs, 40, "round {round}: every job was stored");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
