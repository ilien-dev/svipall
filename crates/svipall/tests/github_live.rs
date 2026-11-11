//! The same path against the real hosts. Network, and about sixteen of the sixty requests an hour
//! the API allows an address without a token, so run by hand:
//!
//! ```text
//! cargo test -p svipall --test github_live -- --ignored --nocapture
//! ```
mod support;

use serde_json::json;
use svipall::server::SvipallServer;

#[tokio::test]
#[ignore = "network: reads five real public URLs through the API"]
async fn five_real_urls_read_through_the_api() {
    support::isolate();
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);
    let urls = [
        ("https://github.com/BurntSushi/ripgrep", "github-api"),
        (
            "https://github.com/tokio-rs/tokio/blob/master/tokio/src/lib.rs#L1-L12",
            "github-raw",
        ),
        // 454 comments: five pages of the comments list.
        (
            "https://github.com/rust-lang/rust/issues/31436",
            "github-api",
        ),
        (
            "https://github.com/BurntSushi/ripgrep/pull/3599",
            "github-api",
        ),
        (
            "https://github.com/BurntSushi/ripgrep/releases/tag/15.2.0",
            "github-api",
        ),
    ];
    for (url, via) in urls {
        let v = server
            .fetch_json(
                serde_json::from_value(json!({"url": url, "max_tokens": 4000, "cache": "bypass"}))
                    .unwrap(),
            )
            .await
            .value;
        let content = v["content"].as_str().unwrap_or_default();
        println!(
            "=== {url}\nvia={} status={} title={} rate_limit={} github={} truncated={}\n{}\n",
            v["via"],
            v["status"],
            v["title"],
            v["rate_limit"],
            v["github"],
            v["truncated"],
            content.chars().take(700).collect::<String>()
        );
        assert_eq!(v["via"], via, "{v}");
        assert!(!content.is_empty(), "{v}");
    }
}
