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
    for url in [
        r.url("/dtolnay/anyhow"),
        r.url("/dtolnay/anyhow/blob/master/src/lib.rs#L1-L3"),
        r.url("/limited/repo/issues/7"),
        // An API URL named directly gets the token too.
        r.api_url("/repos/dtolnay/anyhow/issues/323"),
    ] {
        let v = server
            .fetch_json(serde_json::from_value(json!({"url": url, "max_tier": "http"})).unwrap())
            .await
            .value;
        results.push(v);
    }
    for i in [0, 3] {
        assert_eq!(results[i]["via"], "github-api", "{}", results[i]);
        assert_eq!(
            results[i]["github"]["authenticated"], true,
            "{}",
            results[i]
        );
    }

    let seen = r.seen();
    let api: Vec<_> = seen.iter().filter(|s| s.api).collect();
    assert!(!api.is_empty(), "{seen:?}");
    for s in &api {
        assert_eq!(
            s.authorization.as_deref(),
            Some(format!("Bearer {TOKEN}").as_str()),
            "{s:?}"
        );
    }
    for s in seen.iter().filter(|s| !s.api) {
        assert!(
            s.authorization.is_none(),
            "the raw host and the page never see the token: {s:?}"
        );
    }
    for v in &results {
        assert!(!v.to_string().contains(TOKEN), "the token leaked into {v}");
    }
}
