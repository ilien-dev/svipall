//! A body too large to hold is stopped and said so, never decoded and never sent up the ladder.
//!
//! Its own test binary because the cap is read from `SVIPALL_MAX_BODY_BYTES`, which is
//! process-wide: lowering it next to the other file tests would change what they measure.
mod support;

use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

#[tokio::test]
async fn a_body_over_the_cap_is_an_answer_that_names_the_way_out() {
    support::isolate();
    std::env::set_var("SVIPALL_MAX_BODY_BYTES", "4096");
    let site = Site::start(vec![(
        "/huge.bin",
        Reply::bytes(&vec![7u8; 64 * 1024], "application/octet-stream"),
    )])
    .await;
    let out = SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            max_tier: "stealth".into(),
            ..Default::default()
        },
        None,
    )
    .fetch_json(WebFetchParams {
        url: site.url("/huge.bin"),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    })
    .await
    .value;
    let error = out["file"]["error"].as_str().unwrap_or_default();
    assert!(error.contains("out_file"), "{out}");
    assert_eq!(out["file"]["bytes"], 64 * 1024, "{out}");
    assert_eq!(
        out["attempts"].as_array().map(|a| a.len()),
        Some(1),
        "a browser must not be asked to load it again: {out}"
    );
}
