//! The live sites the offline walls in `challenge_walls.rs` were measured on. By hand, one at a
//! time, never in a loop: a site that refuses an address remembers it.
//!
//! ```text
//! cargo test -p svipall --test challenge_walls_live -- --ignored --test-threads=1 --nocapture
//! ```

mod support;

use svipall::server::SvipallServer;
use svipall::tools::WebFetchParams;

async fn through(url: &str, expect: &str) {
    support::isolate();
    let s = SvipallServer::new(None, svipall_core::Config::default(), None);
    let out = s
        .fetch_json(WebFetchParams {
            url: url.into(),
            max_tokens: Some(300),
            timeout: Some(120_000),
            cache: Some("bypass".into()),
            ..Default::default()
        })
        .await
        .value;
    eprintln!(
        "{url}\n  title={} tier={} blocked={} attempts={}",
        out["title"], out["tier_used"], out["blocked_reason"], out["attempts"]
    );
    assert!(out["blocked_reason"].is_null(), "{out}");
    assert!(
        out["title"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains(expect),
        "{out}"
    );
}

#[tokio::test]
#[ignore = "live network and a real browser"]
async fn an_interstitial_that_clears_itself() {
    through(
        "https://www.scrapingcourse.com/cloudflare-challenge",
        "scrapingcourse",
    )
    .await;
}

#[tokio::test]
#[ignore = "live network and a real browser"]
async fn an_interstitial_that_asks_for_its_box() {
    through("https://lib.rs/crates/serde", "serde").await;
}

#[tokio::test]
#[ignore = "live network and a real browser"]
async fn a_search_engine_behind_a_proof_of_work() {
    through("https://www.mojeek.com/search?q=serde+rust", "serde").await;
}
