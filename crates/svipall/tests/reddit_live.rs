//! Reddit, against the real site. Network, so run by hand:
//!
//! ```text
//! cargo test -p svipall --test reddit_live -- --ignored --nocapture
//! ```
//!
//! Every read here must be answered by the first tier: that is the whole claim. A browser tier
//! answering would pass a weaker test and cost the address twelve times as much.
mod support;

use serde_json::{json, Value};
use svipall::server::SvipallServer;

async fn read(server: &SvipallServer, url: &str) -> Value {
    server
        .fetch_json(
            // `wait`: fourteen reads against a limit of twelve visits a minute.
            serde_json::from_value(
                json!({"url": url, "max_tokens": 6000, "cache": "bypass", "wait": 90}),
            )
            .unwrap(),
        )
        .await
        .value
}

fn delivered(v: &Value, url: &str) -> String {
    assert!(v.get("blocked_reason").is_none(), "{url}: {v}");
    assert_eq!(v["status"], 200, "{url}: {v}");
    assert_eq!(v["tier_used"], "http", "{url}: {v}");
    v["content"].as_str().unwrap_or_default().to_string()
}

/// The first post a listing links, as an address.
fn first_thread(listing: &str) -> String {
    let at = listing
        .find("](https://www.reddit.com/r/")
        .expect("a listing links its posts");
    let rest = &listing[at + 2..];
    rest[..rest.find(')').unwrap()].to_string()
}

#[tokio::test]
#[ignore = "network: reads real public Reddit pages"]
async fn listings_threads_and_closed_addresses_are_read_at_the_first_tier() {
    support::isolate();
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);

    // Listings of different communities and sorts, the closed kinds among them.
    let listings = [
        ("https://www.reddit.com/r/rust/top/?t=week", false),
        ("https://www.reddit.com/r/AskHistorians/", false),
        ("https://www.reddit.com/r/programming/new/", false),
        (
            "https://www.reddit.com/r/rust/top.json?t=month&limit=50",
            true,
        ),
        ("https://old.reddit.com/r/Python/top/?t=week", true),
        ("https://www.reddit.com/r/linux/.rss", true),
    ];
    let mut threads = Vec::new();
    for (url, closed) in listings {
        let v = read(&server, url).await;
        let content = delivered(&v, url);
        let posts = content.matches(" comment").count();
        println!("{url}: {posts} posts, {} chars", content.len());
        assert!(posts >= 10, "{url}: {posts} posts\n{content}");
        assert!(content.contains(" points"), "{url}\n{content}");
        assert!(content.contains(" · u/"), "{url}\n{content}");
        assert!(
            content.contains("Next page: https://www.reddit.com/svc/"),
            "{url}\n{content}"
        );
        assert_eq!(v.get("rewritten_from").is_some(), closed, "{url}: {v}");
        threads.push(first_thread(&content));
    }

    // One thread from each listing, so the set is whatever is on the site today.
    for (i, url) in threads.iter().enumerate() {
        // Every other one through its closed twin.
        let asked = if i % 2 == 0 {
            url.clone()
        } else {
            format!("{}.json", url.trim_end_matches('/'))
        };
        let v = read(&server, &asked).await;
        let content = delivered(&v, &asked);
        println!("{asked}: {} chars", content.len());
        assert!(content.starts_with("# "), "{asked}\n{content}");
        assert!(content.contains("## Comments ("), "{asked}\n{content}");
        assert!(content.contains(" · u/"), "{asked}\n{content}");
        assert_eq!(v["url"], *url, "{asked}: {v}");
    }

    // A community's rules, asked for the way they used to be served.
    for url in [
        "https://www.reddit.com/r/rust/about/rules.json",
        "https://www.reddit.com/r/AskHistorians/about/rules.json",
    ] {
        let v = read(&server, url).await;
        let content = delivered(&v, url);
        println!("{url}: {} chars", content.len());
        assert!(content.contains("## Rules"), "{url}\n{content}");
        assert!(content.contains("**1. "), "{url}\n{content}");
        assert!(content.contains("**3. "), "{url}\n{content}");
        assert!(
            v["url"].as_str().unwrap_or_default().ends_with("/#rules"),
            "{v}"
        );
    }

    // A search inside a community and one across the site.
    for url in [
        "https://www.reddit.com/r/rust/search.json?q=allocator&restrict_sr=1&sort=top&t=year",
        "https://www.reddit.com/search/?q=borrow+checker",
    ] {
        let v = read(&server, url).await;
        let content = delivered(&v, url);
        let results = content.matches(" comment").count();
        println!("{url}: {results} results, {} chars", content.len());
        assert!(results >= 5, "{url}: {results} results\n{content}");
        assert!(
            content.contains("](https://www.reddit.com/r/"),
            "{url}\n{content}"
        );
    }

    // The next page of a feed is a plain read of the address the feed named.
    let v = read(&server, "https://www.reddit.com/r/rust/top/?t=month").await;
    let first = delivered(&v, "page one");
    let next = first
        .lines()
        .find_map(|l| l.strip_prefix("Next page: "))
        .expect("a feed names its next page")
        .to_string();
    let v = read(&server, &next).await;
    let second = delivered(&v, &next);
    assert!(second.matches(" comment").count() >= 10, "{second}");
    assert_ne!(first_thread(&first), first_thread(&second));
}
