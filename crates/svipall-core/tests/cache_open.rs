//! Several processes open the page cache at the same moment: every session of an assistant starts
//! its own server, and a CLI command starts beside them.
//!
//! One that lost the race used to run its whole life without a cache, saying so once in a log
//! nobody reads: every read was a download, a cursor continuation fetched the page again, and
//! `web_watch action=diff` had nothing to compare with.
use svipall_core::cache::Store;

#[test]
fn every_opener_of_a_new_cache_gets_one() {
    let dir = std::env::temp_dir().join(format!(
        "svipall-cache-open-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    for round in 0..25 {
        let path = dir.join(format!("cache-{round}.sqlite3"));
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let openers: Vec<_> = (0..8)
            .map(|_| {
                let (path, start) = (path.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    Store::open_at(&path)
                        .map(|_| ())
                        .map_err(|e| format!("{e:#}"))
                })
            })
            .collect();
        for opener in openers {
            let opened = opener.join().expect("opener thread");
            assert!(
                opened.is_ok(),
                "round {round}: an opener was left without a cache: {opened:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
