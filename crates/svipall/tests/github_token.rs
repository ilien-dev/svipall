//! The optional API token: named in `~/.svipall/secrets.env`, sent to the API and nowhere else,
//! and never part of a result. A binary of its own, because the secrets file is per home and the
//! other GitHub tests must run without one.
mod github_replay;
mod support;

use github_replay::replay;
use serde_json::json;
use svipall::server::SvipallServer;

const TOKEN: &str = "test-token-5f3a9c0e1d";

#[tokio::test]
async fn the_token_goes_to_the_api_only_and_never_into_a_result() {
    let home = support::isolate();
    std::fs::write(
        home.join("secrets.env"),
        format!("# api access\nGITHUB_TOKEN={TOKEN}\n"),
    )
    .unwrap();
    let r = replay();
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);

    let mut results = Vec::new();
    for path in [
        "/dtolnay/anyhow",
        "/dtolnay/anyhow/blob/master/src/lib.rs#L1-L3",
        "/limited/repo/issues/7",
    ] {
        let v = server
            .fetch_json(
                serde_json::from_value(json!({"url": r.url(path), "max_tier": "http"})).unwrap(),
            )
            .await
            .value;
        results.push(v);
    }
    assert_eq!(results[0]["via"], "github-api", "{}", results[0]);
    assert_eq!(
        results[0]["github"]["authenticated"], true,
        "{}",
        results[0]
    );

    let seen = r.seen();
    let api: Vec<_> = seen
        .iter()
        .filter(|s| s.target.starts_with("/repos/"))
        .collect();
    assert!(!api.is_empty(), "{seen:?}");
    for s in &api {
        assert_eq!(
            s.authorization.as_deref(),
            Some(format!("Bearer {TOKEN}").as_str()),
            "{s:?}"
        );
    }
    for s in seen.iter().filter(|s| !s.target.starts_with("/repos/")) {
        assert!(
            s.authorization.is_none(),
            "the raw host and the page never see the token: {s:?}"
        );
    }
    for v in &results {
        assert!(!v.to_string().contains(TOKEN), "the token leaked into {v}");
    }
}
